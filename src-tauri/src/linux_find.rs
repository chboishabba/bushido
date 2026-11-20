//! Native FindController transport. WebKit signals do not carry a client ID.
//! Never overlap native operations on the same controller.
use gtk::{glib, prelude::*};
use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Duration};
use tokio::sync::oneshot;
use webkit2gtk::{ffi, FindController, FindControllerExt, LoadEvent, WebView, WebViewExt};

const MAX_MATCHES: u32 = 100_000;
type Reply = oneshot::Sender<Result<u32, String>>;

struct Pending {
    id: u32,
    reply: Option<Reply>,
}

#[derive(Default)]
struct SearchState {
    latest: u32,
    current_query: Option<String>,
    pending: Option<Pending>,
    // If WebKit never acknowledges an operation, its untagged late signal
    // cannot safely be assigned to a new request. Quarantine this view.
    quarantined: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum SearchAction { Fresh, Next, Previous }

fn action_for(current: Option<&str>, query: &str, forward: bool) -> SearchAction {
    if current != Some(query) { SearchAction::Fresh }
    else if forward { SearchAction::Next }
    else { SearchAction::Previous }
}

impl SearchState {
    fn accept_id(&mut self, id: u32) -> Result<(), String> {
        if id == 0 || id <= self.latest { return Err("Stale find request".into()); }
        self.latest = id;
        Ok(())
    }

    // Keep the in-flight slot occupied until its signal has been drained.
    fn invalidate(&mut self, reason: &str) -> bool {
        self.current_query = None;
        if let Some(pending) = self.pending.as_mut() {
            if let Some(reply) = pending.reply.take() { let _ = reply.send(Err(reason.into())); }
            false
        } else { true }
    }
}

thread_local! {
    static FIND_STATES: RefCell<HashMap<usize, Rc<RefCell<SearchState>>>> = RefCell::new(HashMap::new());
}

fn complete(state: &Rc<RefCell<SearchState>>, controller: &FindController, count: u32) {
    let pending = state.borrow_mut().pending.take();
    let Some(pending) = pending else { return; };
    if let Some(reply) = pending.reply {
        // G_MAXUINT denotes count above the engine limit, not an exact count.
        let _ = reply.send(Ok(if count > MAX_MATCHES { u32::MAX } else { count }));
    } else {
        // Cancelled operation has now drained; clear its highlights before
        // permitting another command. No new operation was issued meanwhile.
        controller.search_finish();
    }
}

fn state_for(view: &WebView, controller: &FindController) -> Rc<RefCell<SearchState>> {
    let key = view.as_ptr() as usize;
    if let Some(state) = FIND_STATES.with(|states| states.borrow().get(&key).cloned()) {
        return state;
    }
    let state = Rc::new(RefCell::new(SearchState::default()));
    FIND_STATES.with(|states| states.borrow_mut().insert(key, state.clone()));
    let found = state.clone();
    controller.connect_found_text(move |controller, count| complete(&found, controller, count));
    let failed = state.clone();
    controller.connect_failed_to_find_text(move |controller| complete(&failed, controller, 0));

    let load_state = state.clone();
    view.connect_load_changed(move |view, event| {
        if event == LoadEvent::Started {
            let finish = load_state.borrow_mut().invalidate("Page navigated during find");
            if finish {
                if let Some(controller) = view.find_controller() { controller.search_finish(); }
            }
        }
    });
    let destroy_state = state.clone();
    view.connect_destroy(move |_| {
        destroy_state.borrow_mut().invalidate("Find view was destroyed");
        FIND_STATES.with(|states| { states.borrow_mut().remove(&key); });
    });
    state
}

fn dispatch(view: &WebView, query: String, forward: bool, id: u32, reply: Reply) {
    let Some(controller) = view.find_controller() else {
        let _ = reply.send(Err("Native find controller unavailable".into()));
        return;
    };
    let state = state_for(view, &controller);
    let mut data = state.borrow_mut();
    if let Err(error) = data.accept_id(id) {
        let _ = reply.send(Err(error));
        return;
    }
    if query.is_empty() {
        let finish = data.invalidate("Find cleared");
        drop(data);
        if finish { controller.search_finish(); }
        let _ = reply.send(Ok(0));
        return;
    }
    if data.quarantined || data.pending.is_some() {
        let error = if data.quarantined { "Native find timed out; recreate the tab to search again" }
            else { "Previous native find is still completing" };
        let _ = reply.send(Err(error.into()));
        return;
    }
    let action = action_for(data.current_query.as_deref(), &query, forward);
    data.current_query = Some(query.clone());
    data.pending = Some(Pending { id, reply: Some(reply) });
    drop(data);

    // Timeout owns the same native request slot. Do not free it and start
    // another request after a timeout: the late signal would be ambiguous.
    let timer_state = state.clone();
    let weak_controller = controller.downgrade();
    glib::timeout_add_local_once(Duration::from_secs(4), move || {
        let mut data = timer_state.borrow_mut();
        if !data.pending.as_ref().is_some_and(|pending| pending.id == id) { return; }
        data.quarantined = true;
        data.invalidate("Native find signal timed out");
        drop(data);
        if let Some(controller) = weak_controller.upgrade() { controller.search_finish(); }
    });

    match action {
        SearchAction::Fresh => {
            let mut options = ffi::WEBKIT_FIND_OPTIONS_CASE_INSENSITIVE | ffi::WEBKIT_FIND_OPTIONS_WRAP_AROUND;
            if !forward { options |= ffi::WEBKIT_FIND_OPTIONS_BACKWARDS; }
            controller.search(&query, options, MAX_MATCHES);
        }
        SearchAction::Next => controller.search_next(),
        SearchAction::Previous => controller.search_previous(),
    }
}

pub async fn find(webview: &tauri::Webview, query: String, forward: bool, id: u32) -> Result<u32, String> {
    if query.contains('\0') { return Err("Find query contains NUL".into()); }
    let (tx, rx) = oneshot::channel();
    webview.with_webview(move |platform| dispatch(&platform.inner(), query, forward, id, tx))
        .map_err(|e| e.to_string())?;
    tokio::time::timeout(Duration::from_secs(6), rx).await
        .map_err(|_| "Native find dispatch timed out".to_owned())?
        .map_err(|_| "Native find callback lost".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_and_repeated_queries_choose_native_operation() {
        assert_eq!(action_for(None, "one", true), SearchAction::Fresh);
        assert_eq!(action_for(Some("one"), "two", false), SearchAction::Fresh);
        assert_eq!(action_for(Some("one"), "one", true), SearchAction::Next);
        assert_eq!(action_for(Some("one"), "one", false), SearchAction::Previous);
    }

    #[test]
    fn stale_ids_and_cancelled_slot_are_not_reused() {
        let mut state = SearchState::default();
        assert!(state.accept_id(10).is_ok());
        assert!(state.accept_id(9).is_err());
        assert!(state.accept_id(10).is_err());
        assert!(state.accept_id(0).is_err());
        let (tx, mut rx) = oneshot::channel();
        state.current_query = Some("old".into());
        state.pending = Some(Pending { id: 10, reply: Some(tx) });
        assert!(state.accept_id(11).is_ok());
        assert!(!state.invalidate("clear"));
        assert!(state.current_query.is_none());
        assert_eq!(rx.try_recv().unwrap(), Err("clear".into()));
        assert!(state.pending.as_ref().unwrap().reply.is_none());
        assert_eq!(state.pending.as_ref().unwrap().id, 10);
        assert!(state.pending.take().is_some()); // signal drain
        assert!(state.invalidate("clear"));
        assert!(state.accept_id(12).is_ok());
    }
}
