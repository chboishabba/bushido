//! Process-start-only debug proxy selection. No page-supplied endpoint.
use std::path::{Path, PathBuf};
use std::os::unix::fs::MetadataExt;

pub(crate) struct EgressFixture {
    proxy: Option<url::Url>,
    scope: Option<crate::linux_destination_scope::OperatorScope>,
    extension: Option<crate::linux_extension_fixture::ExtensionFixture>,
    extension_policy: Option<String>,
}

fn endpoint(raw: &str) -> Result<url::Url, String> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Expected explicit numeric loopback fixture port".into());
    }
    let port: u16 = raw.parse().map_err(|_| "Invalid fixture port")?;
    // HTTP URL normalization removes explicit default port 80.
    if port < 1024 { return Err("Fixture port must be unprivileged".into()); }
    url::Url::parse(&format!("http://127.0.0.1:{port}")).map_err(|e| e.to_string())
}

fn fresh_root(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() { return Err("Absolute private XDG roots required".into()); }
    let m = std::fs::symlink_metadata(path).map_err(|_| "Create fresh private XDG roots before launch")?;
    if !m.is_dir() || m.file_type().is_symlink() || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0 {
        return Err("Fixture root must be a private owned directory".into());
    }
    let canonical = path.canonicalize().map_err(|_| "Cannot resolve fixture root")?;
    if canonical != path || std::fs::read_dir(&canonical).map_err(|_| "Cannot inspect fixture root")?.next().is_some() {
        return Err("Fixture requires empty canonical XDG roots".into());
    }
    Ok(canonical)
}

impl EgressFixture {
    pub(crate) fn from_environment() -> Result<Self, String> {
        let Some(raw) = std::env::var_os("BUSHIDO_LINUX_EGRESS_FIXTURE_PORT") else {
            let scope = crate::linux_destination_scope::OperatorScope::from_environment(false)?;
            let extension=crate::linux_extension_fixture::ExtensionFixture::from_environment(false)?;
            return Ok(Self { proxy: None, scope, extension, extension_policy: None });
        };
        if !cfg!(debug_assertions) { return Err("Egress fixture unavailable in release".into()); }
        let proxy = endpoint(raw.to_str().ok_or("Invalid fixture port")?)?;
        // Protect ordinary profiles: require three distinct, empty, owned XDG roots.
        // This is not an OS sandbox; run under a disposable account or isolated desktop.
        let mut roots = Vec::new();
        for name in ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"] {
            let value = std::env::var_os(name).ok_or("Explicit fresh XDG roots required")?;
            roots.push(fresh_root(Path::new(&value))?);
        }
        for i in 0..roots.len() {
            for j in i+1..roots.len() {
                if roots[i].starts_with(&roots[j]) || roots[j].starts_with(&roots[i]) {
                    return Err("Fixture XDG roots must be separate".into());
                }
            }
        }
        let scope = crate::linux_destination_scope::OperatorScope::from_environment(true)?;
        let extension=crate::linux_extension_fixture::ExtensionFixture::from_environment(scope.is_some())?;
        Ok(Self { proxy: Some(proxy), scope, extension, extension_policy: None })
    }

    pub(crate) fn scope(&self) -> Option<&crate::linux_destination_scope::OperatorScope> { self.scope.as_ref() }

    pub(crate) fn bind_extension_policy(&mut self, engine:&adblock::engine::Engine) {
        if self.extension.is_some() { self.extension_policy=Some(crate::linux_extension_policy::snapshot(engine)); }
    }
    pub(crate) fn extension_policy(&self)->Option<&str> { self.extension_policy.as_deref() }

    pub(crate) fn extension_configured(&self) -> bool { self.extension.is_some() }

    pub(crate) fn extension_path(&self) -> Result<Option<PathBuf>,String> {
        self.extension.as_ref().map(|f|f.path()).transpose()
    }

    pub(crate) fn proxy(&self) -> Option<url::Url> { self.proxy.clone() }

    pub(crate) fn profile_directory(&self, ordinary: PathBuf) -> PathBuf {
        if self.proxy.is_some() { PathBuf::from("egress-fixture").join(ordinary) } else { ordinary }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_loopback_endpoint_only() {
        assert_eq!(endpoint("8765").unwrap().as_str(),"http://127.0.0.1:8765/");
        for raw in ["", "0", "80", "65536", " 8765", "127.0.0.1:8765", "8/9", "http://example.test"] {
            assert!(endpoint(raw).is_err());
        }
    }
    #[test]
    fn profile_namespace_preserves_workspace() {
        let f = EgressFixture { proxy: Some(endpoint("8765").unwrap()), scope: None, extension: None, extension_policy: None };
        let a = PathBuf::from("linux-workspaces/profile-a");
        let b = PathBuf::from("linux-workspaces/profile-b");
        assert_eq!(f.profile_directory(a.clone()),PathBuf::from("egress-fixture").join(&a));
        assert_ne!(f.profile_directory(a.clone()),f.profile_directory(b));
        assert_eq!(EgressFixture { proxy: None, scope: None, extension: None, extension_policy: None }.profile_directory(a.clone()),a);
    }
}
