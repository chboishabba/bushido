//! Explicit native credential selection. No remote IPC or automatic secret broadcast.
use gtk::{glib, prelude::*};
use javascriptcore::ValueExt;
use std::{cell::Cell, rc::Rc};
use tauri::{Emitter, Manager};
use webkit2gtk::{ContextMenuExt, ContextMenuItem, LoadEvent, WebView, WebViewExt};
use zeroize::Zeroizing;

const WORLD: &str = "bushido-native-vault";
const FILL: &str = include_str!("linux_vault_fill.js");

pub(crate) fn https_origin(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    if url.scheme() != "https" || url.host_str().is_none()
        || !url.username().is_empty() || url.password().is_some() { return None; }
    Some(url.origin().ascii_serialization())
}

fn enabled(app: &tauri::AppHandle) -> bool {
    // Also check persisted policy at activation and before dispatch.
    match std::fs::read_to_string(crate::settings_path(app)) {
        Ok(raw) => serde_json::from_str::<serde_json::Value>(&raw).ok()
            .and_then(|v| v.get("disableAutofill").map(|v| v.as_bool().map(|b| !b)).unwrap_or(Some(true)))
            .unwrap_or(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

fn live(app: &tauri::AppHandle, id: &str, view: &WebView, href: &str) -> bool {
    enabled(app) && app.state::<crate::WebviewState>().tabs.lock().contains_key(id)
        && !app.state::<crate::PanelState>().ids.lock().contains(id)
        && !view.is_loading() && view.uri().as_deref() == Some(href)
        && view.tls_info().is_some_and(|(_, errors)| errors.is_empty())
}

fn script(mode: &str, token: &str, origin: &str, href: &str, user: &str, pass: &str) -> Zeroizing<String> {
    let args = Zeroizing::new(serde_json::to_string(&[mode, token, origin, href, user, pass])
        .expect("string array serializes"));
    Zeroizing::new(format!("({FILL})(...{})", args.as_str()))
}

async fn evaluate(view: &WebView, source: &str) -> bool {
    view.run_javascript_in_world_future(source, WORLD).await.ok()
        .and_then(|r| r.js_value()).is_some_and(|v| v.to_boolean())
}

fn message(app: &tauri::AppHandle, text: &str) {
    let _ = app.emit_to("main", "linux-vault-status", text);
}

async fn select(view: &WebView, origin: &str, choices: &[(String, String)]) -> Option<String> {
    let parent = view.toplevel()?.downcast::<gtk::Window>().ok()?;
    let dialog = gtk::Dialog::with_buttons(
        Some("Bushido — Fill saved login"), Some(&parent),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[("Cancel", gtk::ResponseType::Cancel), ("Fill", gtk::ResponseType::Accept)]);
    let area = dialog.content_area();
    let label = gtk::Label::new(Some(&format!("Fill one saved account into {}?\nThe site will be able to read the filled password.", origin)));
    label.set_line_wrap(true);
    area.add(&label);
    let accounts = gtk::ComboBoxText::new();
    for (id, name) in choices {
        let clean: String = name.chars().filter(|c| !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).take(200).collect();
        accounts.append(Some(id), &clean);
    }
    accounts.set_active(Some(0));
    area.add(&accounts);
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Rc::new(std::cell::RefCell::new(Some(tx)));
    let response_tx = tx.clone();
    dialog.connect_response(move |d, response| {
        if let Some(tx) = response_tx.borrow_mut().take() {
            let id = if response == gtk::ResponseType::Accept {
                accounts.active_id().map(|s| s.to_string())
            } else { None };
            let _ = tx.send(id);
        }
        d.hide();
    });
    dialog.connect_destroy(move |_| {
        if let Some(tx) = tx.borrow_mut().take() { let _ = tx.send(None); }
    });
    dialog.show_all();
    let selected = rx.await.ok().flatten();
    dialog.close();
    selected
}

pub(crate) fn install(app: &tauri::AppHandle, webview: &tauri::Webview, id: &str) -> Result<(), String> {
    let app = app.clone();
    let id = id.to_owned();
    webview.with_webview(move |platform| {
        let view = platform.inner();
        let generation = Rc::new(Cell::new(0u64));
        let on_load = generation.clone();
        view.connect_load_changed(move |_, event| {
            if event == LoadEvent::Started { on_load.set(on_load.get().wrapping_add(1)); }
        });
        let on_destroy = generation.clone();
        view.connect_destroy(move |_| on_destroy.set(on_destroy.get().wrapping_add(1)));
        let busy = Rc::new(Cell::new(false));
        view.connect_context_menu(move |view, menu, _, _| {
            let Some(href) = view.uri().map(|s| s.to_string()) else { return false; };
            let Some(origin) = https_origin(&href) else { return false; };
            if busy.get() || !live(&app, &id, view, &href) { return false; }
            let action = gtk::gio::SimpleAction::new("bushido-fill-saved-login", None);
            let weak = view.downgrade();
            let app = app.clone();
            let id = id.clone();
            let generation = generation.clone();
            let opened = generation.get();
            let busy = busy.clone();
            action.connect_activate(move |_, _| {
                let Some(view) = weak.upgrade() else { return; };
                if busy.get() || generation.get() != opened || !live(&app, &id, &view, &href) { return; }
                busy.set(true);
                let (app, id, origin, href, generation, busy) =
                    (app.clone(), id.clone(), origin.clone(), href.clone(), generation.clone(), busy.clone());
                glib::MainContext::default().spawn_local(async move {
                    // One operation per view; reset even on an early return.
                    struct Reset(Rc<Cell<bool>>);
                    impl Drop for Reset { fn drop(&mut self) { self.0.set(false); } }
                    let _reset = Reset(busy);
                    let valid = || generation.get() == opened && live(&app, &id, &view, &href);
                    if !valid() { return; }
                    let token = uuid::Uuid::new_v4().to_string();
                    if !evaluate(&view, script("probe", &token, &origin, &href, "", "").as_str()).await || !valid() {
                        message(&app, "No unambiguous empty same-origin HTTPS login form"); return;
                    }
                    let db_app = app.clone();
                    let db_origin = origin.clone();
                    let choices = tauri::async_runtime::spawn_blocking(move ||
                        crate::vault::native_choices(&db_app.state::<crate::vault::VaultState>(), &db_origin)).await;
                    let choices = match choices {
                        Ok(Ok(v)) if !v.is_empty() => v,
                        Ok(Err(e)) => { message(&app, &e); return; }
                        _ => { message(&app, "No exact-origin saved login; add the HTTPS origin in Settings → Passwords"); return; }
                    };
                    if !valid() { return; }
                    let Some(selected) = select(&view, &origin, &choices).await else { return; };
                    if !valid() { return; }
                    let db_app = app.clone();
                    let db_origin = origin.clone();
                    let result = tauri::async_runtime::spawn_blocking(move ||
                        crate::vault::native_selected(&db_app.state::<crate::vault::VaultState>(), &db_origin, &selected)).await;
                    let Ok(Ok((user, pass))) = result else { message(&app, "Cannot decrypt selected account"); return; };
                    if !valid() || app.state::<crate::vault::VaultState>().derived_key.lock().is_none() {
                        return;
                    }
                    let source = script("fill", &token, &origin, &href, &user, pass.as_str());
                    drop(pass);
                    if evaluate(&view, source.as_str()).await {
                        message(&app, "Saved login filled; review before submitting");
                    } else {
                        message(&app, "Login form changed; nothing filled");
                    }
                });
            });
            let item = ContextMenuItem::from_gaction(&action, "Fill saved login…", None);
            menu.append(&item);
            false
        });
    }).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origin_key_is_https_exact_and_port_sensitive() {
        assert_eq!(https_origin("https://EXAMPLE.org:443/login").as_deref(), Some("https://example.org"));
        assert_eq!(https_origin("https://example.org:8443/login").as_deref(), Some("https://example.org:8443"));
        assert_ne!(https_origin("https://sub.example.org/"), https_origin("https://example.org/"));
        for uri in ["http://example.org/", "https://user:pw@example.org/", "file:///tmp/a", "data:text/plain,x"] {
            assert!(https_origin(uri).is_none());
        }
    }
}
