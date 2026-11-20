//! Native Linux desktop commands. Browser chrome selects a live ordinary tab.
//! This module does not grant arbitrary page IPC or silent printing.
use gtk::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
use webkit2gtk::{PrintOperation, PrintOperationExt, WebViewExt};

static PRINT_DIALOG: AtomicBool = AtomicBool::new(false);

pub(crate) fn inspector_allowed(debug_build: bool, disabled: bool, panel: bool) -> bool {
    debug_build && !disabled && !panel
}

struct DialogGuard<'a>(&'a AtomicBool);
impl<'a> DialogGuard<'a> {
    fn acquire(slot: &'a AtomicBool) -> Result<Self, String> {
        slot.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self(slot)).map_err(|_| "A browser print dialog is already open".into())
    }
}
impl Drop for DialogGuard<'_> {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}

pub(crate) async fn print(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let owner = crate::live_native_tab_owner(app, id).ok_or("Not a live ordinary tab")?;
    let wv = app.get_webview(id).ok_or("Webview not found")?;
    let pos = wv.position().map_err(|e| e.to_string())?;
    if pos.x < 0 || pos.y < 0 { return Err("Tab is not on screen".into()); }
    let app = app.clone();
    let id = id.to_owned();
    let (tx, rx) = tokio::sync::oneshot::channel();
    wv.with_webview(move |platform| {
        if tx.is_closed() { return; }
        let result = (|| -> Result<(), String> {
            let view = platform.inner();
            if !crate::native_tab_matches_owner(&app, &id, &owner)
                || !view.is_mapped() || view.is_loading() {
                return Err("Tab unavailable or loading".into());
            }
            let parent = view.toplevel().and_then(|widget| widget.downcast::<gtk::Window>().ok())
                .ok_or("No native print dialog parent")?;
            let _guard = DialogGuard::acquire(&PRINT_DIALOG)?;
            let operation = PrintOperation::new(&view);
            // WebKit owns printer selection and the native job. Returning from
            // run_dialog is not evidence that a physical print job completed.
            // The modal can run a nested GLib event loop: retain the guard
            // until it returns, and never hold a tab registry lock here.
            let _response = operation.run_dialog(Some(&parent));
            Ok(())
        })();
        let _ = tx.send(result);
    }).map_err(|e| e.to_string())?;
    // A native dialog is user-paced; never time it out and then retry.
    rx.await.map_err(|_| "Native print dialog callback lost".to_string())?
}

pub(crate) async fn inspect(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = (app, id);
        return Err("Developer tools are disabled in this release build".into());
    }
    #[cfg(debug_assertions)]
    {
        use webkit2gtk::WebInspectorExt;
        let owner = crate::live_native_tab_owner(app, id).ok_or("Not a live ordinary tab")?;
        let wv = app.get_webview(id).ok_or("Webview not found")?;
        let pos = wv.position().map_err(|e| e.to_string())?;
        if pos.x < 0 || pos.y < 0 { return Err("Tab is not on screen".into()); }
        let app = app.clone();
        let id = id.to_owned();
        let (tx, rx) = tokio::sync::oneshot::channel();
        wv.with_webview(move |platform| {
            if tx.is_closed() { return; }
            let result = (|| -> Result<(), String> {
                let view = platform.inner();
                if !crate::native_tab_matches_owner(&app, &id, &owner) || !view.is_mapped() {
                    return Err("Tab unavailable".into());
                }
                let settings = WebViewExt::settings(&view).ok_or("Missing WebKit settings")?;
                if !settings.property::<bool>("enable-developer-extras") {
                    return Err("Developer tools disabled for this view; reload after changing Security settings".into());
                }
                let inspector = view.inspector().ok_or("Native inspector unavailable")?;
                // Same show/open semantics as original Windows command.
                // A successful submission is not proof of window visibility.
                inspector.show();
                Ok(())
            })();
            let _ = tx.send(result);
        }).map_err(|e| e.to_string())?;
        tokio::time::timeout(std::time::Duration::from_secs(5), rx).await
            .map_err(|_| "Inspector dispatch timed out".to_string())?
            .map_err(|_| "Inspector callback lost".to_string())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspector_policy() {
        assert!(inspector_allowed(true, false, false));
        assert!(!inspector_allowed(false, false, false));
        assert!(!inspector_allowed(true, true, false));
        assert!(!inspector_allowed(true, false, true));
    }
    #[test]
    fn modal_slot_is_exclusive_and_released() {
        let slot = AtomicBool::new(false);
        let guard = DialogGuard::acquire(&slot).unwrap();
        assert!(DialogGuard::acquire(&slot).is_err());
        drop(guard);
        assert!(DialogGuard::acquire(&slot).is_ok());
    }
}
