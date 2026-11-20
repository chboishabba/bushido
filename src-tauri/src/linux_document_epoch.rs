//! Native top-level load observation for the debug egress scope preview.
//! No page-supplied epoch or arbitrary request attribution. A late install
//! starts unknown and must observe a complete later load to become eligible.
use crate::{live_native_tab_owner, NativeTabOwner};
use gtk::prelude::*;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};
use tauri::Manager;
use webkit2gtk::{LoadEvent, WebView, WebViewExt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocumentSnapshot {
    epoch: uuid::Uuid,
    pub(crate) url: url::Url,
}

#[derive(Default)]
pub(crate) enum LoadState {
    #[default]
    Unknown,
    Started(uuid::Uuid),
    Committed(DocumentSnapshot),
    Finished(DocumentSnapshot),
    Invalid,
    Destroyed,
}

impl LoadState {
    pub(crate) fn observed_epoch(&self) -> Option<uuid::Uuid> {
        match self {
            Self::Started(epoch) => Some(*epoch),
            Self::Committed(doc) | Self::Finished(doc) => Some(doc.epoch),
            _ => None,
        }
    }

    fn started(&mut self) {
        if !matches!(self, Self::Destroyed) {
            *self = Self::Started(uuid::Uuid::new_v4());
        }
    }

    fn committed(&mut self, uri: Option<url::Url>) {
        if matches!(self, Self::Destroyed) { return; }
        *self = match (&*self, uri) {
            (Self::Started(epoch), Some(url)) => Self::Committed(DocumentSnapshot { epoch:*epoch, url }),
            _ => Self::Invalid,
        };
    }

    fn finished(&mut self, uri: Option<url::Url>) {
        if matches!(self, Self::Destroyed) { return; }
        *self = match (&*self, uri) {
            (Self::Committed(doc), Some(url)) if doc.url == url => Self::Finished(doc.clone()),
            _ => Self::Invalid,
        };
    }

    fn invalidate(&mut self) {
        if !matches!(self, Self::Destroyed) { *self=Self::Invalid; }
    }

    fn snapshot(&self, uri: Option<url::Url>, loading: bool) -> Option<DocumentSnapshot> {
        if loading { return None; }
        match (self,uri) {
            (Self::Finished(doc),Some(url)) if doc.url==url => Some(doc.clone()),
            _ => None,
        }
    }
}

struct Entry {
    owner: NativeTabOwner,
    native_identity: usize,
    state: Arc<Mutex<LoadState>>,
    resources: Arc<Mutex<crate::linux_resource_observation::Ledger>>,
    extension: Option<Arc<Mutex<crate::linux_extension_observation::ExtensionObservation>>>,
}

#[derive(Default)]
pub(crate) struct DocumentEpochs(Mutex<HashMap<String,Entry>>);

fn uri(view: &WebView) -> Option<url::Url> {
    view.uri().and_then(|s| url::Url::parse(s.as_str()).ok())
}

impl DocumentEpochs {
    pub(crate) fn remove(&self, id: &str) {
        self.0.lock().remove(id);
    }

    pub(crate) fn extension_snapshot(&self,id:&str,owner:&NativeTabOwner,view:&WebView) -> Option<serde_json::Value> {
        let entries=self.0.lock();
        let entry=entries.get(id)?;
        if &entry.owner!=owner || entry.native_identity!=view.as_ptr() as usize { return None; }
        let observation=entry.extension.as_ref().map(|l|l.lock().snapshot());
        Some(serde_json::json!({"configured":observation.is_some(),"observation":observation}))
    }

    pub(crate) fn resource_snapshot(&self, id:&str, owner:&NativeTabOwner, view:&WebView,
        doc:&DocumentSnapshot) -> Option<serde_json::Value> {
        let entries=self.0.lock();
        let entry=entries.get(id)?;
        if &entry.owner!=owner || entry.native_identity!=view.as_ptr() as usize { return None; }
        let observation=entry.resources.lock().snapshot(doc.epoch);
        Some(observation)
    }

    pub(crate) fn snapshot(&self, id: &str, owner: &NativeTabOwner, view: &WebView)
        -> Option<DocumentSnapshot> {
        let state = {
            let entries=self.0.lock();
            let entry=entries.get(id)?;
            if &entry.owner != owner || entry.native_identity != view.as_ptr() as usize { return None; }
            entry.state.clone()
        };
        let snapshot=state.lock().snapshot(uri(view),view.is_loading());
        snapshot
    }
}

pub(crate) fn install(app: &tauri::AppHandle, webview: &tauri::Webview, id: &str)
    -> Result<(),String> {
    if app.state::<crate::linux_egress_fixture::EgressFixture>().scope().is_none() {
        return Ok(());
    }
    let owner=live_native_tab_owner(app,id).ok_or("No live native owner for scope observation")?;
    let app=app.clone();
    let id=id.to_owned();
    webview.with_webview(move |platform| {
        if !crate::native_tab_matches_owner(&app,&id,&owner) { return; }
        let view=platform.inner();
        let state=Arc::new(Mutex::new(LoadState::Unknown));
        let identity=view.as_ptr() as usize;
        let resources=Arc::new(Mutex::new(crate::linux_resource_observation::Ledger::default()));
        let extension=app.state::<crate::linux_egress_fixture::EgressFixture>().extension_configured()
            .then(||Arc::new(Mutex::new(crate::linux_extension_observation::ExtensionObservation::default())));
        // Never adopt a currently displayed page; attach fresh observation.
        let registry=app.state::<DocumentEpochs>();
        {
            let mut entries=registry.0.lock();
            if entries.get(&id).is_some_and(|e| e.owner==owner && e.native_identity==identity) {
                return;
            }
            entries.insert(id.clone(),Entry {owner,native_identity:identity,state:state.clone(),resources:resources.clone(),extension:extension.clone()});
        }
        crate::linux_resource_observation::attach(&view,state.clone(),resources);
        if let Some(ledger)=extension { crate::linux_extension_observation::attach(&view,ledger); }
        let load=state.clone();
        view.connect_load_changed(move |view,event| {
            let mut state=load.lock();
            match event {
                LoadEvent::Started => state.started(),
                LoadEvent::Redirected => {
                    // A redirect is allowed only within an observed provisional load.
                    if !matches!(*state,LoadState::Started(_)) { state.invalidate(); }
                }
                LoadEvent::Committed => state.committed(uri(view)),
                LoadEvent::Finished => state.finished(uri(view)),
                _ => state.invalidate(),
            }
        });
        let failed=state.clone();
        view.connect_load_failed(move |_,_,_,_| { failed.lock().invalidate(); false });
        let tls=state.clone();
        view.connect_load_failed_with_tls_errors(move |_,_,_,_| { tls.lock().invalidate(); false });
        let process=state.clone();
        view.connect_web_process_terminated(move |_,_| { process.lock().invalidate(); });
        let changed=state.clone();
        view.connect_uri_notify(move |view| {
            // A finished snapshot is invalidated even on URI ABA; a same-URL
            // reload is independently invalidated by LoadEvent::Started.
            let mut state=changed.lock();
            if let LoadState::Finished(doc)=&*state {
                if uri(view).as_ref()!=Some(&doc.url) { state.invalidate(); }
            }
        });
        view.connect_destroy(move |_| { *state.lock()=LoadState::Destroyed; });
    }).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn url() -> url::Url { "https://bushido-tunnel.test/".parse().unwrap() }
    fn complete(state:&mut LoadState) -> DocumentSnapshot {
        state.started(); state.committed(Some(url())); state.finished(Some(url()));
        state.snapshot(Some(url()),false).unwrap()
    }
    #[test]
    fn same_url_reload_gets_distinct_identity() {
        let mut state=LoadState::default();
        let a=complete(&mut state);
        state.started();
        assert!(state.snapshot(Some(url()),false).is_none());
        state.committed(Some(url())); state.finished(Some(url()));
        let b=state.snapshot(Some(url()),false).unwrap();
        assert_ne!(a,b);
        assert_eq!(a.url,b.url);
    }
    #[test]
    fn late_install_and_failure_cannot_be_promoted_by_finished() {
        let mut s=LoadState::default();
        s.committed(Some(url())); s.finished(Some(url()));
        assert!(s.snapshot(Some(url()),false).is_none());
        s.started(); s.committed(Some(url())); s.invalidate(); s.finished(Some(url()));
        assert!(s.snapshot(Some(url()),false).is_none());
        complete(&mut s);
        s.invalidate();
        assert!(s.snapshot(Some(url()),false).is_none());
    }
    #[test]
    fn loading_uri_mismatch_and_destroy_reject() {
        let mut s=LoadState::default(); complete(&mut s);
        assert!(s.snapshot(Some(url()),true).is_none());
        assert!(s.snapshot(Some("https://other.test/".parse().unwrap()),false).is_none());
        s=LoadState::Destroyed;
        s.started();s.committed(Some(url()));s.finished(Some(url()));
        assert!(s.snapshot(Some(url()),false).is_none());
    }
}
