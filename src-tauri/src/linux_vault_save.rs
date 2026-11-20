//! Submission-triggered consent. Only metadata crosses the world-specific
//! message handler. A selected password is read after native GTK approval.
use gtk::{glib, prelude::*};
use javascriptcore::ValueExt;
use serde::Deserialize;
use std::{cell::{Cell, RefCell}, rc::Rc, sync::{Arc, atomic::{AtomicU64, Ordering}}, time::{Duration, Instant}};
use tauri::{Emitter, Manager};
use webkit2gtk::{LoadEvent, UserContentManagerExt, WebView, WebViewExt};
use zeroize::Zeroizing;

const WORLD: &str = "bushido-native-vault-save";
const INSTALL: &str = include_str!("linux_vault_save.js");
const TTL: Duration = Duration::from_secs(20);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Notice { ticket: String, href: String, id: u32, username: String }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture { username: String, password: String }

struct Arm { ticket: String, href: String, seen: u32 }

fn enabled(app: &tauri::AppHandle) -> bool {
    match std::fs::read_to_string(crate::settings_path(app)) {
        Ok(raw) => serde_json::from_str::<serde_json::Value>(&raw).ok()
            .and_then(|v| v.get("disablePasswordSave").map(|v| v.as_bool().map(|b| !b)).unwrap_or(Some(true)))
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

fn status(app: &tauri::AppHandle, text: &str) {
    let _ = app.emit_to("main", "linux-vault-status", text);
}

fn arm(view: &WebView, channel: &str, state: &RefCell<Option<Arm>>) {
    state.borrow_mut().take();
    let Some(href) = view.uri().map(|s| s.to_string()) else { return; };
    if crate::linux_vault::https_origin(&href).is_none() || view.is_loading()
        || !view.tls_info().is_some_and(|(_, errors)| errors.is_empty()) { return; }
    let ticket = uuid::Uuid::new_v4().to_string();
    let args = serde_json::to_string(&[channel, &ticket, &href]).expect("string array serializes");
    *state.borrow_mut() = Some(Arm { ticket, href, seen: 0 });
    view.run_javascript_in_world(&format!("({INSTALL})(...{args})"), WORLD,
        None::<&gtk::gio::Cancellable>, |_| {});
}

fn discard(view: &WebView, notice: &Notice) {
    let ticket = serde_json::to_string(&notice.ticket).expect("string serializes");
    let source = format!("globalThis.__bushidoSaveDiscard?.({ticket},{})", notice.id);
    view.run_javascript_in_world(&source, WORLD, None::<&gtk::gio::Cancellable>, |_| {});
}

async fn approve(view: &WebView, origin: &str, username: &str, valid: &impl Fn() -> bool) -> bool {
    let Some(parent) = view.toplevel().and_then(|w| w.downcast::<gtk::Window>().ok()) else { return false; };
    let dialog = gtk::Dialog::with_buttons(
        Some("Bushido — Save submitted login?"), Some(&parent),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[("Don't save", gtk::ResponseType::Cancel), ("Save new login", gtk::ResponseType::Accept)]);
    let user: String = username.chars().filter(|c| !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).take(200).collect();
    let label = gtk::Label::new(Some(&format!("Save the submitted login for {origin}?\nAccount: {user}\nOnly in this tab's workspace. Existing logins are not overwritten.")));
    label.set_line_wrap(true);
    dialog.content_area().add(&label);
    dialog.set_default_response(gtk::ResponseType::Cancel);
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let tx = Rc::new(RefCell::new(Some(tx)));
    let response = tx.clone();
    dialog.connect_response(move |d, r| {
        if let Some(tx) = response.borrow_mut().take() { let _ = tx.send(r == gtk::ResponseType::Accept); }
        d.hide();
    });
    dialog.connect_destroy(move |_| {
        if let Some(tx) = tx.borrow_mut().take() { let _ = tx.send(false); }
    });
    dialog.show_all();
    let approved = loop {
        if !valid() { break false; }
        tokio::select! {
            reply = &mut rx => break reply.unwrap_or(false),
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    };
    dialog.close();
    approved && valid()
}

pub(crate) fn install(app: &tauri::AppHandle, webview: &tauri::Webview, id: &str, workspace: &str) -> Result<(), String> {
    let app = app.clone();
    let id = id.to_owned();
    let workspace = workspace.to_owned();
    webview.with_webview(move |platform| {
        let view = platform.inner();
        let Some(manager) = view.user_content_manager() else { return; };
        // Unique per actual view; never register in the page's default world.
        let channel = format!("bushidoSave{}", uuid::Uuid::new_v4().simple());
        let state = Rc::new(RefCell::new(None::<Arm>));
        let generation = Arc::new(AtomicU64::new(0));
        let busy = Rc::new(Cell::new(false));
        let recv_state = state.clone();
        let recv_generation = generation.clone();
        let weak_view = view.downgrade();
        let handler = manager.connect_script_message_received(Some(&channel), move |_, message| {
            let Some(view) = weak_view.upgrade() else { return; };
            let Some(value) = message.js_value() else { return; };
            let raw = value.to_string();
            if raw.len() > 32768 { return; }
            let Ok(notice) = serde_json::from_str::<Notice>(&raw) else { return; };
            if notice.id == 0 || notice.username.is_empty() || notice.username.len() > 4096 { return; }
            {
                let mut state = recv_state.borrow_mut();
                let Some(armed) = state.as_mut() else { return; };
                if notice.ticket != armed.ticket || notice.href != armed.href || notice.id <= armed.seen { return; }
                armed.seen = notice.id;
            }
            let Some(origin) = crate::linux_vault::https_origin(&notice.href) else { return; };
            if busy.get() || !view.has_focus() || !live(&app, &id, &view, &notice.href) {
                discard(&view, &notice); return;
            }
            let Ok(epoch) = app.state::<crate::vault::VaultState>().session() else {
                discard(&view, &notice); return;
            };
            let opened = recv_generation.load(Ordering::SeqCst);
            let deadline = Instant::now() + TTL;
            busy.set(true);
            let (app, id, workspace, generation, busy) =
                (app.clone(), id.clone(), workspace.clone(), recv_generation.clone(), busy.clone());
            glib::MainContext::default().spawn_local(async move {
                struct Reset(Rc<Cell<bool>>);
                impl Drop for Reset { fn drop(&mut self) { self.0.set(false); } }
                let _reset = Reset(busy);
                let valid = || Instant::now() < deadline && generation.load(Ordering::SeqCst) == opened
                    && live(&app, &id, &view, &notice.href)
                    && app.state::<crate::vault::VaultState>().with_session(epoch, || ()).is_ok();
                if !valid() || !approve(&view, &origin, &notice.username, &valid).await {
                    discard(&view, &notice); return;
                }
                let ticket = serde_json::to_string(&notice.ticket).expect("string serializes");
                let source = format!("globalThis.__bushidoSaveTake?.({ticket},{}) ?? null", notice.id);
                let result = tokio::time::timeout(Duration::from_secs(3), view.run_javascript_in_world_future(&source, WORLD)).await;
                let Ok(Ok(result)) = result else { status(&app, "Login save cancelled: form no longer available"); return; };
                if !valid() { return; }
                let Some(value) = result.js_value() else { return; };
                let raw = Zeroizing::new(value.to_string().to_string());
                if raw.len() > 65536 { return; }
                let Ok(capture) = serde_json::from_str::<Capture>(raw.as_str()) else {
                    status(&app, "Login save cancelled: form changed or expired"); return;
                };
                let pass = Zeroizing::new(capture.password);
                if capture.username != notice.username || pass.is_empty() || pass.len() > 32768 || !valid() { return; }
                drop(raw);
                let worker_app = app.clone();
                let result = tauri::async_runtime::spawn_blocking(move || {
                    let permitted = || Instant::now() < deadline && generation.load(Ordering::SeqCst) == opened && enabled(&worker_app);
                    crate::vault::native_save_new(&worker_app.state::<crate::vault::VaultState>(),
                        &workspace, &origin, &capture.username, pass.as_str(), epoch, permitted)
                }).await;
                match result {
                    Ok(Ok(_)) => status(&app, "Login saved in this workspace"),
                    Ok(Err(e)) => status(&app, &e),
                    Err(_) => status(&app, "Login save failed"),
                }
            });
        });
        if !manager.register_script_message_handler_in_world(&channel, WORLD) {
            manager.disconnect(handler);
            return;
        }
        let load_state = state.clone();
        let load_gen = generation.clone();
        let load_channel = channel.clone();
        view.connect_load_changed(move |view, event| {
            if event == LoadEvent::Started {
                load_gen.fetch_add(1, Ordering::SeqCst);
                load_state.borrow_mut().take();
            } else if event == LoadEvent::Finished {
                arm(view, &load_channel, &load_state);
            }
        });
        let destroy_state = state.clone();
        let destroy_gen = generation.clone();
        let destroy_channel = channel.clone();
        view.connect_destroy(move |_| {
            destroy_gen.fetch_add(1, Ordering::SeqCst);
            destroy_state.borrow_mut().take();
            manager.unregister_script_message_handler_in_world(&destroy_channel, WORLD);
            manager.disconnect(handler);
        });
        arm(&view, &channel, &state);
    }).map_err(|e| e.to_string())
}
