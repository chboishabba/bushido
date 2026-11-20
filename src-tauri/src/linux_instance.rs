//! Early Linux process ownership and bounded same-user activation IPC.
//! No browser request is replayed, nor is a workspace selected by IPC.
use std::{
    collections::VecDeque, fs::{self, File, OpenOptions}, io::{Read, Write},
    os::fd::AsRawFd,
    os::unix::{fs::{DirBuilderExt, MetadataExt, OpenOptionsExt}, net::{UnixListener, UnixStream}},
    path::{Path, PathBuf}, sync::Arc, thread, time::{Duration, Instant},
};
use parking_lot::Mutex;

const MAX_URL: usize = 8192;
const MAX_URLS: usize = 8;
const MAX_FRAME: usize = 65536;
const MAX_PENDING: usize = 32;
const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Default)]
pub(crate) struct ActivationQueue(Arc<Mutex<VecDeque<Option<String>>>>);

impl ActivationQueue {
    fn enqueue(&self, urls: Vec<String>) -> Result<(), &'static str> {
        let mut pending = self.0.lock();
        if pending.len() + urls.len().max(1) > MAX_PENDING {
            return Err("External activation queue full");
        }
        if urls.is_empty() { pending.push_back(None); }
        else { pending.extend(urls.into_iter().map(Some)); }
        Ok(())
    }
    pub(crate) fn take(&self) -> Option<Option<String>> { self.0.lock().pop_front() }
}

pub(crate) struct Instance {
    // Held until Tauri run returns. Never unlink the lock inode.
    _lock: File,
    socket: PathBuf,
    pub(crate) queue: ActivationQueue,
}

impl Drop for Instance {
    fn drop(&mut self) { let _ = fs::remove_file(&self.socket); }
}

fn validate(raw: &str) -> Result<String, &'static str> {
    if raw.is_empty() || raw.len() > MAX_URL || raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("Invalid external URL length or characters");
    }
    // Reject shorthand and browser backslash repair at the external
    // protocol boundary; require an explicit authority-bearing URL.
    let (scheme, authority_and_rest) = raw.split_once("://").ok_or("Explicit HTTP(S) URL required")?;
    let authority = authority_and_rest.split(['/', '?', '#']).next().unwrap_or("");
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || authority.is_empty() || authority.contains('@') || raw.contains('\\') {
        return Err("Invalid external URL authority");
    }
    let parsed = url::Url::parse(raw).map_err(|_| "Invalid external URL")?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none()
        || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Only HTTP(S) URLs without credentials are accepted");
    }
    let canonical = parsed.to_string();
    if canonical.len() > MAX_URL { return Err("External URL too long"); }
    Ok(canonical)
}

fn validate_batch(raw: Vec<String>) -> Result<Vec<String>, &'static str> {
    if raw.len() > MAX_URLS { return Err("Too many external URLs"); }
    let urls = raw.iter().map(|url| validate(url)).collect::<Result<Vec<_>, _>>()?;
    if serde_json::to_vec(&urls).map_err(|_| "Cannot encode activation")?.len() > MAX_FRAME {
        return Err("External activation frame too large");
    }
    Ok(urls)
}

fn arguments() -> Result<Vec<String>, &'static str> {
    let mut args = std::env::args_os().skip(1).peekable();
    if args.peek().is_some_and(|s| s == "--") { args.next(); }
    let mut raw = Vec::new();
    for arg in args {
        if raw.len() >= MAX_URLS { return Err("Too many external URLs"); }
        raw.push(arg.into_string().map_err(|_| "Invalid external argument")?);
    }
    validate_batch(raw)
}

fn private_directory(root: &Path) -> Result<PathBuf, &'static str> {
    let dir = root.join("instance");
    fs::create_dir_all(root).map_err(|_| "Cannot create application data root")?;
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("Cannot create private instance directory"),
    }
    let m = fs::symlink_metadata(&dir).map_err(|_| "Cannot inspect instance directory")?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
        return Err("Instance directory must be private and owned by current user");
    }
    fs::canonicalize(dir).map_err(|_| "Cannot resolve instance directory")
}

fn lock_file(dir: &Path) -> Result<File, &'static str> {
    let file = OpenOptions::new().read(true).write(true).create(true)
        .mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(dir.join("owner.lock")).map_err(|_| "Cannot open instance lock")?;
    let m = file.metadata().map_err(|_| "Cannot inspect instance lock")?;
    if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 || m.nlink() != 1 {
        return Err("Unsafe instance lock");
    }
    Ok(file)
}

fn same_user(stream: &UnixStream) -> bool {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(), &mut size)
    };
    rc == 0 && size as usize == std::mem::size_of::<libc::ucred>() && cred.uid == unsafe { libc::geteuid() }
}

fn configure(stream: &UnixStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))
}

fn receive(stream: &mut UnixStream, queue: &ActivationQueue) -> Result<(), &'static str> {
    if !same_user(stream) { return Err("External activation peer rejected"); }
    configure(stream).map_err(|_| "Cannot configure activation socket")?;
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).map_err(|_| "Cannot read activation length")?;
    let n = u32::from_be_bytes(length) as usize;
    if n == 0 || n > MAX_FRAME { return Err("Invalid activation length"); }
    let mut data = vec![0; n];
    stream.read_exact(&mut data).map_err(|_| "Cannot read activation payload")?;
    let raw: Vec<String> = serde_json::from_slice(&data).map_err(|_| "Invalid activation payload")?;
    queue.enqueue(validate_batch(raw)?)
}

fn serve(listener: UnixListener, queue: ActivationQueue) {
    for incoming in listener.incoming() {
        let Ok(mut stream) = incoming else { break; };
        // One bounded message per connection. No URL logging. Same-user
        // callers are trusted to invoke desktop links, not choose workspaces.
        let accepted = receive(&mut stream, &queue).is_ok();
        let _ = stream.write_all(&[u8::from(accepted)]);
    }
}

fn forward(socket: &Path, urls: &[String]) -> Result<(), &'static str> {
    let data = serde_json::to_vec(urls).map_err(|_| "Cannot encode activation")?;
    if data.len() > MAX_FRAME { return Err("External activation frame too large"); }
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut stream = loop {
        match UnixStream::connect(socket) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Err(_) => return Err("Existing instance did not expose its activation socket"),
        }
    };
    if !same_user(&stream) { return Err("Existing instance peer rejected"); }
    configure(&stream).map_err(|_| "Cannot configure activation connection")?;
    stream.write_all(&(data.len() as u32).to_be_bytes()).and_then(|_| stream.write_all(&data))
        .map_err(|_| "Cannot send activation")?;
    // Never retry after a send: an ambiguous acknowledgement could otherwise
    // create duplicate tabs. Acceptance means queued, not navigated.
    let mut ack = [0u8; 1];
    stream.read_exact(&mut ack).map_err(|_| "Activation acknowledgement unavailable")?;
    if ack == [1] { Ok(()) } else { Err("Existing instance rejected activation") }
}

// None means the secondary process forwarded successfully and must exit.
// Called before application services, profile writes or WebView construction.
pub(crate) fn enter(root: &Path) -> Result<Option<Instance>, &'static str> {
    let urls = arguments()?;
    let dir = private_directory(root)?;
    let socket = dir.join("activation.sock");
    let lock = lock_file(&dir)?;
    let rc = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock {
            return Err("Cannot acquire instance lock");
        }
        forward(&socket, &urls)?;
        return Ok(None);
    }
    // Only the lock owner removes a stale socket. Never remove other types.
    match fs::symlink_metadata(&socket) {
        Ok(m) => {
            use std::os::unix::fs::FileTypeExt;
            if !m.file_type().is_socket() || m.uid() != unsafe { libc::geteuid() } {
                return Err("Unsafe stale activation socket");
            }
            fs::remove_file(&socket).map_err(|_| "Cannot remove stale activation socket")?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Cannot inspect activation socket"),
    }
    let listener = UnixListener::bind(&socket).map_err(|_| "Cannot bind private activation socket (path length or permissions)")?;
    let instance = Instance { _lock: lock, socket, queue: ActivationQueue::default() };
    // Private parent and peer UID already bound access; tighten the socket
    // inode too, without changing the process-global umask.
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&instance.socket, fs::Permissions::from_mode(0o600))
        .map_err(|_| "Cannot restrict activation socket permissions")?;
    if !urls.is_empty() { instance.queue.enqueue(urls)?; }
    let queue = instance.queue.clone();
    thread::Builder::new().name("bushido-activation".into()).spawn(move || serve(listener, queue))
        .map_err(|_| "Cannot start activation receiver")?;
    Ok(Some(instance))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_urls_are_bounded_and_never_privileged() {
        assert_eq!(validate("https://example.test/a?q=1").unwrap(), "https://example.test/a?q=1");
        for raw in ["--debug", "bushido://settings", "file:///tmp/a", "javascript:alert(1)",
            "data:text/plain,x", "https://user:pass@example.test/", "https://@example.test/",
            "https:example.test", "https:///example.test", "https://example.test\\a",
            "https://example.test/\n", "https://example.test/a b"] {
            assert!(validate(raw).is_err(), "{raw}");
        }
        assert!(validate_batch(vec!["https://example.test/".into(); 9]).is_err());
        assert!(validate(&format!("https://example.test/{}", "x".repeat(MAX_URL))).is_err());
    }
    #[test]
    fn queue_is_atomic_bounded_and_consumed_once() {
        let q = ActivationQueue::default();
        q.enqueue(vec![]).unwrap();
        assert_eq!(q.take(), Some(None));
        for _ in 0..4 { q.enqueue(vec!["https://example.test/".into(); 8]).unwrap(); }
        assert!(q.enqueue(vec![]).is_err());
        for _ in 0..32 { assert_eq!(q.take(), Some(Some("https://example.test/".into()))); }
        assert_eq!(q.take(), None);
    }
    #[test]
    fn socket_protocol_rejects_invalid_and_oversized_frames() {
        let q = ActivationQueue::default();
        for (payload, valid) in [(b"[\"https://example.test/\"]".as_slice(), true), (b"[\"file:///tmp/a\"]".as_slice(), false)] {
            let (mut sender, mut receiver) = UnixStream::pair().unwrap();
            sender.write_all(&(payload.len() as u32).to_be_bytes()).unwrap();
            sender.write_all(payload).unwrap();
            assert_eq!(receive(&mut receiver, &q).is_ok(), valid);
        }
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        sender.write_all(&((MAX_FRAME + 1) as u32).to_be_bytes()).unwrap();
        assert!(receive(&mut receiver, &q).is_err());
        assert_eq!(q.take(), Some(Some("https://example.test/".into())));
        assert_eq!(q.take(), None);
    }
}
