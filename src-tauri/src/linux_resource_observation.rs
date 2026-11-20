//! Fixed-category native WebResource observations, not request authority.
//! No URL/header/body/frame identity or network dispatch evidence retained.
use parking_lot::Mutex;
use std::{collections::BTreeMap, sync::Arc};
use webkit2gtk::{URIRequestExt, WebResourceExt, WebView, WebViewExt};
use super::linux_document_epoch::LoadState;

const MAX_PER_EPOCH: u64 = 64;
const MAX_PER_VIEW: u64 = 512;

fn category(raw: Option<&str>) -> &'static str {
    match raw {
        Some("https://bushido-tunnel.test/") => "document_route",
        Some("https://bushido-tunnel.test/script.js") => "script_route",
        Some("https://bushido-tunnel.test/pixel") => "image_route",
        Some("https://bushido-tunnel.test/fetch") => "fetch_route",
        Some("https://bushido-tunnel.test/frame") => "frame_route",
        Some("https://bushido-tunnel.test/worker.js") => "worker_route",
        Some("https://bushido-tunnel.test/worker-import.js") => "worker_import_route",
        Some("https://bushido-tunnel.test/redirect") => "redirect_route",
        Some("https://bushido-tunnel.test/redirect-target") => "redirect_target_route",
        _ => "other",
    }
}

#[derive(Default)]
pub(crate) struct Ledger {
    epoch: Option<uuid::Uuid>,
    lifetime_hooks: u64,
    tracked: u64,
    omitted: u64,
    started: BTreeMap<&'static str,u64>,
    uri_notifications: BTreeMap<&'static str,u64>,
    failed: u64,
    finished_without_observed_failure: u64,
    finished_after_observed_failure: u64,
}

#[derive(Clone,Copy)]
struct Ticket { epoch: uuid::Uuid }

#[derive(Default)]
struct Lifecycle { failed: bool, finished: bool }

fn increment(counts: &mut BTreeMap<&'static str,u64>, key: &'static str) {
    let count=counts.entry(key).or_default();
    *count=count.saturating_add(1);
}

impl Ledger {
    fn synchronize(&mut self, epoch: Option<uuid::Uuid>) {
        if self.epoch != epoch {
            let lifetime_hooks=self.lifetime_hooks;
            *self=Self {epoch,lifetime_hooks,..Default::default()};
        }
    }

    fn begin(&mut self, epoch: Option<uuid::Uuid>, route: &'static str) -> Option<Ticket> {
        self.synchronize(epoch);
        let epoch=epoch?;
        increment(&mut self.started,route);
        if self.tracked>=MAX_PER_EPOCH || self.lifetime_hooks>=MAX_PER_VIEW {
            self.omitted=self.omitted.saturating_add(1);
            return None;
        }
        self.tracked+=1;
        self.lifetime_hooks+=1;
        Some(Ticket {epoch})
    }

    fn current(&self, t: Ticket, epoch: Option<uuid::Uuid>) -> bool {
        self.epoch==Some(t.epoch) && epoch==Some(t.epoch)
    }

    fn notify_uri(&mut self,t:Ticket,epoch:Option<uuid::Uuid>,life:&Lifecycle,route:&'static str) {
        if self.current(t,epoch) && !life.finished { increment(&mut self.uri_notifications,route); }
    }

    fn fail(&mut self,t:Ticket,epoch:Option<uuid::Uuid>,life:&mut Lifecycle) {
        if life.finished || life.failed { return; }
        life.failed=true;
        if self.current(t,epoch) { self.failed+=1; }
    }

    fn finish(&mut self,t:Ticket,epoch:Option<uuid::Uuid>,life:&mut Lifecycle) {
        if life.finished { return; }
        life.finished=true;
        if self.current(t,epoch) {
            if life.failed { self.finished_after_observed_failure+=1; }
            else { self.finished_without_observed_failure+=1; }
        }
    }

    pub(crate) fn snapshot(&self, epoch: uuid::Uuid) -> serde_json::Value {
        if self.epoch!=Some(epoch) {
            return serde_json::json!({"observed":false,"tracked":0,"omitted":0,
                "coverage_complete":false});
        }
        serde_json::json!({
            "observed":self.tracked>0,
            "tracked":self.tracked,
            "omitted":self.omitted,
            "started_routes":self.started,
            "uri_notification_routes":self.uri_notifications,
            "failed":self.failed,
            "finished_without_observed_failure":self.finished_without_observed_failure,
            "finished_after_observed_failure":self.finished_after_observed_failure,
            "lifetime_hook_limit_reached":self.lifetime_hooks>=MAX_PER_VIEW,
            "coverage_complete":false,
            "frame_identity_verified":false,
            "network_dispatch_verified":false,
            "redirect_linkage_verified":false
        })
    }
}

pub(crate) fn attach(view:&WebView, state:Arc<Mutex<LoadState>>, ledger:Arc<Mutex<Ledger>>) {
    view.connect_resource_load_started(move |_, resource, request| {
        let epoch=state.lock().observed_epoch();
        let raw=request.uri();
        let Some(ticket)=ledger.lock().begin(epoch,category(raw.as_ref().map(|s|s.as_str()))) else { return; };
        let life=Arc::new(Mutex::new(Lifecycle::default()));
        let (s,l,k)=(state.clone(),ledger.clone(),life.clone());
        resource.connect_uri_notify(move |resource| {
            let epoch=s.lock().observed_epoch();
            let raw=resource.uri();
            l.lock().notify_uri(ticket,epoch,&k.lock(),category(raw.as_ref().map(|s|s.as_str())));
        });
        let (s,l,k)=(state.clone(),ledger.clone(),life.clone());
        resource.connect_failed(move |_,_| {
            let epoch=s.lock().observed_epoch();
            l.lock().fail(ticket,epoch,&mut k.lock());
        });
        let (s,l,k)=(state.clone(),ledger.clone(),life.clone());
        resource.connect_failed_with_tls_errors(move |_,_,_| {
            let epoch=s.lock().observed_epoch();
            l.lock().fail(ticket,epoch,&mut k.lock());
        });
        let (s,l,k)=(state.clone(),ledger.clone(),life);
        resource.connect_finished(move |_| {
            let epoch=s.lock().observed_epoch();
            l.lock().finish(ticket,epoch,&mut k.lock());
        });
        // Deliberately no sent-request signal: nullable redirected_response
        // must be checked against the pinned Rust binding before use.
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_resource_does_not_credit_replacement_epoch() {
        let mut l=Ledger::default();
        let a=uuid::Uuid::new_v4();
        let b=uuid::Uuid::new_v4();
        let old=l.begin(Some(a),"document_route").unwrap();
        l.begin(Some(b),"document_route").unwrap();
        let mut life=Lifecycle::default();
        l.notify_uri(old,Some(b),&life,"redirect_target_route");
        l.finish(old,Some(b),&mut life);
        assert_eq!(l.finished_without_observed_failure,0);
        assert!(l.uri_notifications.is_empty());
        assert_eq!(l.tracked,1);
    }
    #[test]
    fn failure_and_finish_do_not_double_count() {
        let mut l=Ledger::default();
        let epoch=uuid::Uuid::new_v4();
        let t=l.begin(Some(epoch),"image_route").unwrap();
        let mut life=Lifecycle::default();
        l.fail(t,Some(epoch),&mut life);
        l.fail(t,Some(epoch),&mut life);
        l.finish(t,Some(epoch),&mut life);
        l.finish(t,Some(epoch),&mut life);
        assert_eq!(l.failed,1);
        assert_eq!(l.finished_after_observed_failure,1);
        assert_eq!(l.finished_without_observed_failure,0);
    }
    #[test]
    fn fixed_categories_and_lifetime_bounds() {
        assert_eq!(category(Some("https://bushido-tunnel.test/?secret=x")),"other");
        assert_eq!(category(Some("https://bushido-tunnel.test/script.js")),"script_route");
        let mut l=Ledger::default();
        assert!(l.begin(None,"other").is_none());
        for _ in 0..8 {
            let epoch=uuid::Uuid::new_v4();
            for _ in 0..MAX_PER_EPOCH { assert!(l.begin(Some(epoch),"other").is_some()); }
            assert!(l.begin(Some(epoch),"other").is_none());
            assert_eq!(l.omitted,1);
        }
        assert!(l.begin(Some(uuid::Uuid::new_v4()),"other").is_none());
        assert_eq!(l.lifetime_hooks,MAX_PER_VIEW);
    }
}
