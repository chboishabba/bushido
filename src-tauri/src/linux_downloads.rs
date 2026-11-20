//! Native WebKit transfers: never replay an authenticated browser download.
use crate::downloads::{DlItem, DlState, DownloadManager};
use crate::native_download_history as history;
use gtk::prelude::*;
use std::{cell::RefCell, collections::HashMap, path::{Path, PathBuf}, time::{Instant, SystemTime, UNIX_EPOCH}};
use tauri::{AppHandle, Emitter, Manager};
use webkit2gtk::{Download, DownloadExt, URIResponseExt, WebContextExt, WebViewExt};

struct Transfer {
    download: Download,
    cancelled: bool,
    // Only remove a path inside the private directory we created.
    owned_dir: Option<PathBuf>,
    chooser: Option<gtk::FileChooserNative>,
}
thread_local! {
    static ACTIVE: RefCell<HashMap<String, Transfer>> = RefCell::new(HashMap::new());
}

fn safe_filename(name: &str) -> String {
    let basename = name.rsplit(['/', '\\']).next().unwrap_or("");
    // Bound encoded filename size, not just character count; a Unicode
    // scalar can occupy up to four bytes in a Linux pathname.
    let mut clean = String::new();
    for c in basename.chars().filter(|c| !c.is_control()) {
        if clean.len() + c.len_utf8() > 200 { break; }
        clean.push(c);
    }
    if clean.is_empty() || clean == "." || clean == ".." { "download".into() } else { clean }
}

fn download_settings(app: &AppHandle) -> serde_json::Value {
    std::fs::read_to_string(super::settings_path(app)).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).unwrap_or_default()
}

fn configured_root(settings: &serde_json::Value, mime: &str) -> Result<PathBuf, String> {
    let mut root = settings.get("downloadLocation").and_then(|v| v.as_str())
        .filter(|s| !s.is_empty()).map(PathBuf::from)
        .or_else(dirs::download_dir).ok_or("No download directory configured")?;
    if let Some(routes) = settings.get("mimeRouting").and_then(|v| v.as_array()) {
        for route in routes {
            if let (Some(prefix), Some(folder)) = (route.get("mimePrefix").and_then(|v| v.as_str()),
                route.get("folder").and_then(|v| v.as_str())) {
                if !prefix.is_empty() && mime.starts_with(prefix) && !folder.is_empty() {
                    let folder = PathBuf::from(folder);
                    root = if folder.is_absolute() { folder } else { root.join(folder) };
                    break;
                }
            }
        }
    }
    Ok(root)
}

fn selected_target(path: &Path) -> Result<(PathBuf, String), String> {
    let root = path.parent().filter(|p| p.is_absolute())
        .ok_or("Choose an absolute local destination")?;
    let name = path.file_name().and_then(|n| n.to_str())
        .ok_or("Choose a valid UTF-8 filename")?;
    // Do not silently rewrite an explicit user choice.
    if safe_filename(name) != name {
        return Err("Choose a filename of at most 200 UTF-8 bytes without control characters or separators".into());
    }
    Ok((root.to_owned(), name.to_owned()))
}

fn destination(root: &Path, id: &str, name: &str) -> Result<(PathBuf, PathBuf), String> {
    use std::os::unix::fs::DirBuilderExt;
    if !root.is_absolute() { return Err("Download directory must be absolute".into()); }
    // The existing UI/backend path contract uses UTF-8 strings.
    if root.to_str().is_none() { return Err("Download directory is not valid UTF-8".into()); }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    // A unique directory avoids filename collisions and restricts cleanup
    // to our own transfer, including simultaneous identical filenames.
    let owned = root.join(format!(".bushido-download-{id}"));
    std::fs::DirBuilder::new().mode(0o700).create(&owned).map_err(|e| e.to_string())?;
    let file = owned.join(safe_filename(name));
    Ok((owned, file))
}

// A completed file is published with an atomic no-replace directory entry.
// Unlike exists()+rename(), hard_link never clobbers another transfer or a
// dangling symlink. Staging and final destination are on the same filesystem.
// Do not copy or overwrite if hard links are unsupported: keep the original.
fn publish_completed_file(staged: &Path, owned: &Path) -> std::io::Result<PathBuf> {
    use std::io::{Error, ErrorKind};
    let root = owned.parent().ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Missing download root"))?;
    if staged.parent() != Some(owned) || !std::fs::symlink_metadata(staged)?.file_type().is_file() {
        return Err(Error::new(ErrorKind::InvalidInput, "Expected regular staged download"));
    }
    let name = staged.file_name().and_then(|s| s.to_str())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Invalid staged filename"))?;
    let stem = staged.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = staged.extension().and_then(|s| s.to_str());
    for n in 0..10_000 {
        let candidate = if n == 0 { name.to_owned() } else {
            match ext {
                Some(ext) => format!("{stem} ({n}).{ext}"),
                None => format!("{stem} ({n})"),
            }
        };
        let final_path = root.join(candidate);
        match std::fs::hard_link(staged, &final_path) {
            Ok(()) => return Ok(final_path),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(Error::new(ErrorKind::AlreadyExists, "Download filename collision limit reached"))
}

fn publish(app: &AppHandle, id: &str, event: &str, durable: bool, update: impl FnOnce(&mut DlItem)) -> bool {
    let dm = app.state::<DownloadManager>();
    // Serialize receipt and UI update against clear_completed_downloads.
    let mut items = dm.downloads.lock();
    let Some(item) = items.get_mut(id) else { return false; };
    update(item);
    let saved = !durable || history::save(app, item).is_ok();
    if !saved {
        crate::crash_log::log_error("native-download-history", "Cannot persist download receipt");
        let warning = "Download history could not be saved";
        item.error = Some(match item.error.take() {
            Some(e) => format!("{e}; {warning}"),
            None => warning.into(),
        });
    }
    let _ = app.emit_to("main", event, item.clone());
    saved
}

fn may_stage(id: &str) -> bool {
    ACTIVE.with(|active| active.borrow().get(id)
        .is_some_and(|entry| !entry.cancelled && entry.owned_dir.is_none() && entry.chooser.is_none()))
}

fn fail_destination(app: &AppHandle, id: &str, download: &Download, error: String) {
    publish(app, id, "download-failed", true, |item| {
        item.state = DlState::Failed; item.error = Some(error); item.speed = 0;
    });
    download.cancel();
}

fn commit_destination(app: &AppHandle, id: &str, download: &Download, root: &Path, name: &str) {
    if !may_stage(id) { return; }
    let result = destination(root, id, name).and_then(|(dir, file)| {
        let uri = match url::Url::from_file_path(&file) {
            Ok(uri) => uri,
            Err(_) => {
                let _ = std::fs::remove_dir(&dir);
                return Err("Invalid destination URI".to_owned());
            }
        };
        ACTIVE.with(|active| {
            if let Some(entry) = active.borrow_mut().get_mut(id) { entry.owned_dir = Some(dir); }
        });
        if !publish(app, id, "download-progress", true, |item| {
            item.file_path = file.to_string_lossy().into_owned();
            item.file_name = name.to_owned();
        }) {
            return Err("Cannot record native download destination".into());
        }
        download.set_allow_overwrite(false);
        download.set_destination(uri.as_str());
        Ok(())
    });
    if let Err(error) = result { fail_destination(app, id, download, error); }
}

fn supports_deferred_destination(major: u32, minor: u32) -> bool {
    major > 2 || (major == 2 && minor >= 40)
}

fn ask_destination(app: &AppHandle, id: &str, download: &Download, root: Option<&Path>, name: &str) {
    // The decide-destination signal only supports asynchronous handling
    // since WebKitGTK 2.40. These version functions are available on older builds.
    let supported = unsafe { supports_deferred_destination(
        webkit2gtk::ffi::webkit_get_major_version(), webkit2gtk::ffi::webkit_get_minor_version()) };
    if !supported {
        fail_destination(app, id, download, "Ask download location requires WebKitGTK 2.40 or newer".into());
        return;
    }
    let parent = download.web_view().and_then(|view| view.toplevel())
        .and_then(|widget| widget.downcast::<gtk::Window>().ok());
    let chooser = gtk::FileChooserNative::new(
        Some("Save download (existing names are kept)"), parent.as_ref(),
        gtk::FileChooserAction::Save, Some("Save"), Some("Cancel"));
    chooser.set_modal(true);
    chooser.set_local_only(true);
    chooser.set_do_overwrite_confirmation(false);
    chooser.set_current_name(name);
    if let Some(root) = root.filter(|p| p.is_absolute() && p.is_dir()) {
        let _ = chooser.set_current_folder(root);
    }
    let handle = app.clone();
    let transfer_id = id.to_owned();
    chooser.connect_response(move |dialog, response| {
        // Consume the matching live dialog exactly once; no references to
        // the WebKit transfer or dialog are held by this callback itself.
        let download = ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            let entry = active.get_mut(&transfer_id)?;
            if entry.cancelled || entry.chooser.as_ref() != Some(dialog) { return None; }
            entry.chooser.take();
            if response != gtk::ResponseType::Accept { entry.cancelled = true; }
            Some(entry.download.clone())
        });
        let Some(download) = download else { return; };
        if response != gtk::ResponseType::Accept {
            download.cancel();
            return;
        }
        let result = dialog.filename().ok_or("No local destination selected".to_owned())
            .and_then(|path| selected_target(&path));
        match result {
            Ok((root, name)) => commit_destination(&handle, &transfer_id, &download, &root, &name),
            Err(error) => fail_destination(&handle, &transfer_id, &download, error),
        }
    });
    // Hold the dialog alive until response, cancellation or terminal finish.
    ACTIVE.with(|active| {
        if let Some(entry) = active.borrow_mut().get_mut(id) { entry.chooser = Some(chooser.clone()); }
    });
    chooser.show();
}

fn start(app: &AppHandle, download: &Download) {
    let id = uuid::Uuid::new_v4().to_string();
    // Native transfers are not replayable; never retain the request URL.
    let url = String::new();
    let item = DlItem {
        id: id.clone(), url, file_path: String::new(), file_name: "Preparing download".into(),
        mime_type: String::new(), total_bytes: None, received_bytes: 0, state: DlState::Downloading,
        speed: 0, error: None, created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        supports_range: false, native: true, segments: 0, priority: 0,
    };
    app.state::<DownloadManager>().downloads.lock().insert(id.clone(), item.clone());
    ACTIVE.with(|active| active.borrow_mut().insert(id.clone(), Transfer { download: download.clone(), cancelled: false, owned_dir: None, chooser: None }));
    let persisted = history::save(app, &item).is_ok();
    let _ = app.emit_to("main", "download-started", item);

    let handle = app.clone();
    let transfer_id = id.clone();
    download.connect_decide_destination(move |download, suggested| {
        let mime = download.response().and_then(|r| r.mime_type()).map(|s| s.to_string()).unwrap_or_default();
        let total = download.response().and_then(|r| u64::try_from(r.content_length()).ok()).filter(|n| *n > 0);
        // Ignore duplicate destination signals, including pending dialogs.
        if !may_stage(&transfer_id) { return true; }
        let settings = download_settings(&handle);
        let name = safe_filename(suggested);
        let root = configured_root(&settings, &mime);
        publish(&handle, &transfer_id, "download-progress", true, |item| {
            item.file_name = name.clone();
            item.mime_type = mime;
            item.total_bytes = total;
        });
        if settings.get("askDownloadLocation").and_then(|v| v.as_bool()) == Some(true) {
            ask_destination(&handle, &transfer_id, download, root.as_deref().ok(), &name);
        } else {
            match root {
                Ok(root) => commit_destination(&handle, &transfer_id, download, &root, &name),
                Err(error) => fail_destination(&handle, &transfer_id, download, error),
            }
        }
        true
    });
    let handle = app.clone();
    let transfer_id = id.clone();
    let last = std::cell::Cell::new(Instant::now());
    let last_checkpoint = std::cell::Cell::new(Instant::now());
    download.connect_received_data(move |download, _| {
        let now = Instant::now();
        if now.duration_since(last.get()).as_millis() < 500 { return; }
        last.set(now);
        let bytes = download.received_data_length();
        let elapsed = download.elapsed_time();
        let durable = now.duration_since(last_checkpoint.get()).as_secs() >= 5;
        if durable { last_checkpoint.set(now); }
        publish(&handle, &transfer_id, "download-progress", durable, |item| {
            if item.state == DlState::Downloading {
                item.received_bytes = bytes;
                item.speed = if elapsed > 0.0 { (bytes as f64 / elapsed) as u64 } else { 0 };
            }
        });
    });
    let handle = app.clone();
    let transfer_id = id.clone();
    download.connect_failed(move |_, error| {
        publish(&handle, &transfer_id, "download-failed", true, |item| {
            item.state = DlState::Failed; item.speed = 0;
            if item.error.is_none() { item.error = Some(error.to_string()); }
        });
    });
    let handle = app.clone();
    let download_id_for_failure = id.clone();
    download.connect_finished(move |_download| {
        let entry = ACTIVE.with(|active| active.borrow_mut().remove(&id));
        let Some(entry) = entry else { return; };
        if let Some(chooser) = entry.chooser.as_ref() { chooser.hide(); }
        let dm = handle.state::<DownloadManager>();
        // A cancel request can race with a successful finish. Success must
        // take precedence over the intent flag, or we could delete a file
        // that WebKit finished before cancellation took effect.
        let staged = dm.downloads.lock().get(&id).and_then(|item| {
            let path = PathBuf::from(&item.file_path);
            (item.state == DlState::Downloading && !item.file_path.is_empty()
                && std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()))
                .then_some(path)
        });
        if let Some(staged) = staged {
            // Sync the finished data before recording completion. A staging
            // file observed after a crash is not by itself a finish receipt.
            let synced = std::fs::File::open(&staged).and_then(|f| {
                f.sync_all()?;
                if let Some(dir) = staged.parent() {
                    std::fs::File::open(dir)?.sync_all()?;
                    if let Some(root) = dir.parent() { std::fs::File::open(root)?.sync_all()?; }
                }
                f.metadata()
            });
            let Ok(attrs) = synced else {
                publish(&handle, &id, "download-failed", true, |item| {
                    item.state = DlState::Failed; item.speed = 0;
                    item.error = Some("Finished data could not be synced; staged file retained".into());
                });
                return;
            };
            let bytes = attrs.len();
            // The completed staging receipt is private to the journal.
            // The public item remains downloading until publication is over.
            let staged_recorded = {
                let items = dm.downloads.lock();
                items.get(&id).is_some_and(|item| {
                    let mut receipt = item.clone();
                    receipt.state = DlState::Completed;
                    receipt.received_bytes = bytes; receipt.total_bytes = Some(bytes); receipt.speed = 0;
                    receipt.error = Some("Saved in private staging; publication pending".into());
                    history::save(&handle, &receipt).is_ok()
                })
            };
            if !staged_recorded {
                publish(&handle, &id, "download-complete", true, |item| {
                    item.state = DlState::Completed; item.received_bytes = bytes;
                    item.total_bytes = Some(bytes); item.speed = 0;
                    item.error = Some("Saved in private staging; initial completion receipt failed".into());
                });
                return;
            }
            let mut final_path = staged.clone();
            let mut warning = None;
            if let Some(owned) = entry.owned_dir.as_deref() {
                match publish_completed_file(&staged, owned) {
                    Ok(path) => {
                        // Persist a final-path receipt only after the new
                        // directory entry itself has been synced.
                        if path.parent().is_some_and(|root| std::fs::File::open(root).and_then(|f| f.sync_all()).is_ok()) {
                            final_path = path;
                        } else {
                            warning = Some("Saved in staging; final directory sync failed, both links retained".into());
                        }
                    }
                    Err(e) => warning = Some(format!("Saved in staging; folder publication failed: {e}")),
                }
            } else {
                warning = Some("Saved without flat-directory publication".into());
            }
            // Persist actual final path before unlinking staging. If the
            // process dies after hard_link but before this write, the prior
            // completed-staging receipt remains valid and both links survive.
            // Hold the same item lock as the clear command through receipt,
            // cleanup and terminal event. A clear cannot resurrect a row.
            let mut items = dm.downloads.lock();
            if let Some(item) = items.get_mut(&id) {
                item.state = DlState::Completed; item.received_bytes = bytes;
                item.total_bytes = Some(bytes); item.speed = 0;
                item.file_path = final_path.to_string_lossy().into_owned();
                item.file_name = final_path.file_name().and_then(|s| s.to_str())
                    .unwrap_or(&item.file_name).to_owned();
                item.error = warning;
                if history::save(&handle, item).is_ok() {
                    if final_path != staged {
                        if let Some(owned) = entry.owned_dir.as_deref() {
                            if std::fs::remove_file(&staged).and_then(|_| std::fs::remove_dir(owned)).is_err() {
                                item.error = Some("Saved; staging cleanup incomplete".into());
                                let _ = history::save(&handle, item);
                            }
                        }
                    }
                } else {
                    // The durable staged receipt is still authoritative.
                    // Preserve both paths, and show the known staged path.
                    item.file_path = staged.to_string_lossy().into_owned();
                    item.file_name = staged.file_name().and_then(|s| s.to_str()).unwrap_or(&item.file_name).to_owned();
                    item.error = Some("Saved in staging; final history update failed".into());
                }
                let _ = handle.emit_to("main", "download-complete", item.clone());
            }
        } else if entry.cancelled {
            if history::forget(&handle, &id).is_ok() {
                dm.downloads.lock().remove(&id);
                let _ = handle.emit_to("main", "download-cancelled", serde_json::json!({"id": id}));
            } else {
                publish(&handle, &id, "download-failed", true, |item| {
                    item.state = DlState::Failed; item.speed = 0;
                    item.error = Some("Cancelled; could not remove history".into());
                });
            }
            // Do not recursively delete files in an externally mutable root.
            // A partial retained by WebKit is never opened as completed.
            if let Some(dir) = entry.owned_dir { let _ = std::fs::remove_dir(dir); }
        } else {
            publish(&handle, &id, "download-failed", true, |item| {
                item.state = DlState::Failed; item.speed = 0;
                if item.error.is_none() { item.error = Some("No completed destination".into()); }
            });
            if let Some(dir) = entry.owned_dir { let _ = std::fs::remove_dir(dir); }
        }

    });
    if !persisted {
        // Refuse an unjournaled transfer before choosing its destination.
        fail_destination(app, &download_id_for_failure, download, "Cannot initialize native download history".into());
    }
}

pub fn install(app: &AppHandle, webview: &tauri::Webview) -> Result<(), String> {
    let handle = app.clone();
    webview.with_webview(move |platform| {
        let view = platform.inner();
        let Some(context) = view.context() else { return; };
        let weak_view = view.downgrade();
        // Shared WebContext can signal every tab's handler; match the
        // initiating native view, not the URL. One download -> one record.
        let signal = context.connect_download_started(move |_, download| {
            let Some(view) = weak_view.upgrade() else { return; };
            if download.web_view().as_ref() == Some(&view) { start(&handle, download); }
        });
        let weak_context = context.downgrade();
        let signal = RefCell::new(Some(signal));
        view.connect_destroy(move |_| {
            if let (Some(context), Some(signal)) = (weak_context.upgrade(), signal.borrow_mut().take()) {
                context.disconnect(signal);
            }
        });
    }).map_err(|e| e.to_string())
}

pub fn is_native(app: &AppHandle, id: &str) -> bool {
    app.state::<DownloadManager>().downloads.lock().get(id).is_some_and(|item| item.native)
}

pub async fn cancel(app: &AppHandle, id: String) -> Result<(), String> {
    let handle = app.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let download = ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            active.get_mut(&id).map(|entry| {
                entry.cancelled = true;
                (entry.download.clone(), entry.chooser.take())
            })
        });
        if let Some((download, chooser)) = download {
            // Never hold a RefCell borrow across a native signal emission.
            if let Some(chooser) = chooser { chooser.hide(); }
            download.cancel();
        } else {
            let dm = handle.state::<DownloadManager>();
            let result = history::forget(&handle, &id);
            if result.is_ok() {
                dm.downloads.lock().remove(&id);
                // Removing a history record must never delete any file.
                let _ = handle.emit_to("main", "download-cancelled", serde_json::json!({"id": id}));
            }
            let _ = tx.send(result);
            return;
        }
        let _ = tx.send(Ok(()));
    }).map_err(|e| e.to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(5), rx).await
        .map_err(|_| "Native cancellation dispatch timed out".to_owned())?
        .map_err(|_| "Native cancellation callback lost".to_owned())??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestRoot(PathBuf);
    impl TestRoot {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("bushido-publish-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn stage(&self, id: &str, name: &str, data: &[u8]) -> (PathBuf, PathBuf) {
            let owned = self.0.join(id);
            std::fs::create_dir(&owned).unwrap();
            let staged = owned.join(name);
            std::fs::write(&staged, data).unwrap();
            (owned, staged)
        }
    }
    impl Drop for TestRoot {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn publication_no_overwrite_and_no_symlink_follow() {
        use std::os::unix::fs::symlink;
        let root = TestRoot::new();
        std::fs::write(root.0.join("report.pdf"), b"original").unwrap();
        symlink(root.0.join("missing"), root.0.join("report (1).pdf")).unwrap();
        let (owned, staged) = root.stage("private", "report.pdf", b"new");
        let published = publish_completed_file(&staged, &owned).unwrap();
        assert_eq!(published, root.0.join("report (2).pdf"));
        assert_eq!(std::fs::read(root.0.join("report.pdf")).unwrap(), b"original");
        assert!(std::fs::symlink_metadata(root.0.join("report (1).pdf")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&published).unwrap(), b"new");
        std::fs::remove_file(staged).unwrap();
        std::fs::remove_dir(owned).unwrap();
        assert_eq!(std::fs::read(published).unwrap(), b"new");
    }

    #[test]
    fn simultaneous_publications_do_not_clobber() {
        let root = TestRoot::new();
        let jobs: Vec<_> = (0..12).map(|n| {
            let (owned, staged) = root.stage(&format!("private-{n}"), "same", &[n]);
            std::thread::spawn(move || {
                let published = publish_completed_file(&staged, &owned).unwrap();
                std::fs::remove_file(staged).unwrap();
                std::fs::remove_dir(owned).unwrap();
                assert_eq!(std::fs::read(&published).unwrap(), vec![n]);
                published
            })
        }).collect();
        let paths: std::collections::HashSet<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
        assert_eq!(paths.len(), 12);
    }

    #[test]
    fn failed_publication_preserves_original() {
        let root = TestRoot::new();
        let (owned, staged) = root.stage("private", "report", b"retain");
        assert!(publish_completed_file(&staged, &root.0.join("wrong")).is_err());
        assert_eq!(std::fs::read(&staged).unwrap(), b"retain");
    }

    #[test]
    fn deferred_destination_version_boundary() {
        assert!(!supports_deferred_destination(2, 38));
        assert!(!supports_deferred_destination(2, 39));
        assert!(supports_deferred_destination(2, 40));
        assert!(supports_deferred_destination(2, 48));
        assert!(supports_deferred_destination(3, 0));
    }

    #[test]
    fn explicit_selected_target_is_not_silently_rewritten() {
        assert_eq!(selected_target(Path::new("/tmp/my selected.txt")).unwrap(),
            (PathBuf::from("/tmp"), "my selected.txt".into()));
        assert!(selected_target(Path::new("relative.txt")).is_err());
        assert!(selected_target(Path::new("/")).is_err());
        assert!(selected_target(Path::new("/tmp/a\\b")).is_err());
        assert!(selected_target(Path::new("/tmp/a\nb")).is_err());
        assert!(selected_target(&PathBuf::from(format!("/tmp/{}", "界".repeat(100)))).is_err());
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe_filename("../../secret"), "secret");
        assert_eq!(safe_filename("C:\\fakepath\\a.pdf"), "a.pdf");
        assert_eq!(safe_filename(".."), "download");
        assert_eq!(safe_filename("\n"), "download");
        let unicode = "界".repeat(100);
        assert!(safe_filename(&unicode).len() <= 200);
    }
}
