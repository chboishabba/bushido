//! Native WebKit transfers: never replay an authenticated browser download.
use crate::downloads::{DlItem, DlState, DownloadManager};
use gtk::prelude::*;
use std::{cell::RefCell, collections::HashMap, path::{Path, PathBuf}, time::{Instant, SystemTime, UNIX_EPOCH}};
use tauri::{AppHandle, Emitter, Manager};
use webkit2gtk::{Download, DownloadExt, URIRequestExt, URIResponseExt, WebContextExt, WebViewExt};

struct Transfer {
    download: Download,
    cancelled: bool,
    // Only remove a path inside the private directory we created.
    owned_dir: Option<PathBuf>,
}
thread_local! {
    static ACTIVE: RefCell<HashMap<String, Transfer>> = RefCell::new(HashMap::new());
}

fn safe_filename(name: &str) -> String {
    let basename = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = basename.chars().filter(|c| !c.is_control()).take(100).collect();
    if clean.is_empty() || clean == "." || clean == ".." { "download".into() } else { clean }
}

fn destination(app: &AppHandle, id: &str, name: &str, mime: &str) -> Result<(PathBuf, PathBuf), String> {
    use std::os::unix::fs::DirBuilderExt;
    let settings = std::fs::read_to_string(super::settings_path(app)).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).unwrap_or_default();
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
    if !root.is_absolute() { return Err("Download directory must be absolute".into()); }
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    // A unique directory avoids filename collisions and restricts cleanup
    // to our own transfer, including simultaneous identical filenames.
    let owned = root.join(format!("bushido-{id}"));
    std::fs::DirBuilder::new().mode(0o700).create(&owned).map_err(|e| e.to_string())?;
    let file = owned.join(safe_filename(name));
    Ok((owned, file))
}

fn publish(app: &AppHandle, id: &str, event: &str, update: impl FnOnce(&mut DlItem)) {
    let dm = app.state::<DownloadManager>();
    let snapshot = {
        let mut items = dm.downloads.lock();
        items.get_mut(id).map(|item| { update(item); item.clone() })
    };
    if let Some(item) = snapshot { let _ = app.emit_to("main", event, item); }
}

fn start(app: &AppHandle, download: &Download) {
    let id = uuid::Uuid::new_v4().to_string();
    let url = download.request().and_then(|r| r.uri())
        .and_then(|s| url::Url::parse(s.as_str()).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.username().is_empty() && u.password().is_none())
        .map(|u| u.to_string()).unwrap_or_default();
    let item = DlItem {
        id: id.clone(), url, file_path: String::new(), file_name: "Preparing download".into(),
        mime_type: String::new(), total_bytes: None, received_bytes: 0, state: DlState::Downloading,
        speed: 0, error: None, created_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        supports_range: false, native: true, segments: 0, priority: 0,
    };
    app.state::<DownloadManager>().downloads.lock().insert(id.clone(), item.clone());
    ACTIVE.with(|active| active.borrow_mut().insert(id.clone(), Transfer { download: download.clone(), cancelled: false, owned_dir: None }));
    let _ = app.emit_to("main", "download-started", item);

    let handle = app.clone();
    let transfer_id = id.clone();
    download.connect_decide_destination(move |download, suggested| {
        let mime = download.response().and_then(|r| r.mime_type()).map(|s| s.to_string()).unwrap_or_default();
        let total = download.response().map(|r| r.content_length()).filter(|n| *n > 0);
        let result = destination(&handle, &transfer_id, suggested, &mime).and_then(|(dir, file)| {
            let uri = url::Url::from_file_path(&file).map_err(|_| "Invalid destination URI".to_owned())?;
            ACTIVE.with(|active| {
                if let Some(entry) = active.borrow_mut().get_mut(&transfer_id) { entry.owned_dir = Some(dir); }
            });
            publish(&handle, &transfer_id, "download-progress", |item| {
                item.file_path = file.to_string_lossy().into_owned();
                item.file_name = safe_filename(suggested);
                item.mime_type = mime;
                item.total_bytes = total;
            });
            download.set_allow_overwrite(false);
            download.set_destination(uri.as_str());
            Ok(())
        });
        if let Err(error) = result {
            publish(&handle, &transfer_id, "download-failed", |item| {
                item.state = DlState::Failed; item.error = Some(error);
            });
            download.cancel();
        }
        true
    });
    let handle = app.clone();
    let transfer_id = id.clone();
    let last = std::cell::Cell::new(Instant::now());
    download.connect_received_data(move |download, _| {
        let now = Instant::now();
        if now.duration_since(last.get()).as_millis() < 500 { return; }
        last.set(now);
        let bytes = download.received_data_length();
        let elapsed = download.elapsed_time();
        publish(&handle, &transfer_id, "download-progress", |item| {
            if item.state == DlState::Downloading {
                item.received_bytes = bytes;
                item.speed = if elapsed > 0.0 { (bytes as f64 / elapsed) as u64 } else { 0 };
            }
        });
    });
    let handle = app.clone();
    let transfer_id = id.clone();
    download.connect_failed(move |_, error| {
        publish(&handle, &transfer_id, "download-failed", |item| {
            item.state = DlState::Failed; item.speed = 0;
            if item.error.is_none() { item.error = Some(error.to_string()); }
        });
    });
    let handle = app.clone();
    download.connect_finished(move |download| {
        let entry = ACTIVE.with(|active| active.borrow_mut().remove(&id));
        let Some(entry) = entry else { return; };
        let dm = handle.state::<DownloadManager>();
        if entry.cancelled {
            dm.downloads.lock().remove(&id);
            if let Some(dir) = entry.owned_dir { let _ = std::fs::remove_dir_all(dir); }
            let _ = handle.emit_to("main", "download-cancelled", serde_json::json!({"id": id}));
            return;
        }
        let completed = dm.downloads.lock().get(&id).is_some_and(|item|
            item.state == DlState::Downloading && !item.file_path.is_empty() && Path::new(&item.file_path).is_file());
        if completed {
            publish(&handle, &id, "download-complete", |item| {
                item.state = DlState::Completed; item.received_bytes = download.received_data_length();
                item.total_bytes = Some(item.received_bytes); item.speed = 0;
            });
        } else {
            publish(&handle, &id, "download-failed", |item| {
                item.state = DlState::Failed; item.speed = 0;
                if item.error.is_none() { item.error = Some("No completed destination".into()); }
            });
            if let Some(dir) = entry.owned_dir { let _ = std::fs::remove_dir_all(dir); }
        }
    });
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
            active.get_mut(&id).map(|entry| { entry.cancelled = true; entry.download.clone() })
        });
        if let Some(download) = download {
            // Never hold a RefCell borrow across a native signal emission.
            download.cancel();
        } else {
            let dm = handle.state::<DownloadManager>();
            dm.downloads.lock().remove(&id);
            // Removing a finished record must not delete the completed file.
            let _ = handle.emit_to("main", "download-cancelled", serde_json::json!({"id": id}));
        }
        let _ = tx.send(());
    }).map_err(|e| e.to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(5), rx).await
        .map_err(|_| "Native cancellation dispatch timed out".to_owned())?
        .map_err(|_| "Native cancellation callback lost".to_owned())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safe_names() {
        assert_eq!(safe_filename("../../secret"), "secret");
        assert_eq!(safe_filename("C:\\fakepath\\a.pdf"), "a.pdf");
        assert_eq!(safe_filename(".."), "download");
        assert_eq!(safe_filename("\n"), "download");
    }
}
