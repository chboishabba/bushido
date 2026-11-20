//! Fixed-category native WebResource observations, not request authority.
//! No URL/header/body/frame identity or network dispatch evidence retained.
use parking_lot::Mutex;
use std::{collections::BTreeMap, sync::Arc};
use gtk::glib::{self, prelude::ObjectExt};
use webkit2gtk::{URIRequest, URIRequestExt, URIResponse, URIResponseExt, WebResource, WebResourceExt, WebView, WebViewExt};
use super::linux_document_epoch::LoadState;

const MAX_PER_EPOCH: u64 = 64;
const MAX_PER_VIEW: u64 = 512;
const MAX_SENT_PER_RESOURCE: u8 = 16;

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
    sent_initial: BTreeMap<&'static str,u64>,
    sent_redirect: BTreeMap<&'static str,u64>,
    fixture_redirect_edges: u64,
    unlinked_redirects: u64,
    sent_sequence_errors: u64,
    sent_omitted: u64,
    failed: u64,
    finished_without_observed_failure: u64,
    finished_after_observed_failure: u64,
}

#[derive(Clone,Copy)]
struct Ticket { epoch: uuid::Uuid }

#[derive(Default)]
struct Lifecycle {
    failed: bool,
    finished: bool,
    sent_count: u8,
    last_sent: Option<&'static str>,
    chain_invalid: bool,
}

#[derive(Clone,Copy)]
enum Sent {
    Initial(&'static str),
    Redirect { from: &'static str, to: &'static str, status: u32 },
}

// The C signal response is nullable. Decode the GValue as Option, never
// create a Rust reference from a null C pointer. None here means malformed
// arguments; Some(Initial) means a correctly typed NULL response.
fn decode_sent(values: &[glib::Value]) -> Option<Sent> {
    if values.len()!=3 { return None; }
    let _resource=values[0].get::<WebResource>().ok()?;
    let request=values[1].get::<URIRequest>().ok()?;
    let response=values[2].get::<Option<URIResponse>>().ok()?;
    let raw=request.uri();
    let to=category(raw.as_ref().map(|s|s.as_str()));
    Some(match response {
        None => Sent::Initial(to),
        Some(response) => {
            let old=response.uri();
            Sent::Redirect { from:category(old.as_ref().map(|s|s.as_str())),
                             to,status:response.status_code() }
        }
    })
}

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

    fn sent(&mut self,t:Ticket,epoch:Option<uuid::Uuid>,life:&mut Lifecycle,event:Option<Sent>) {
        if !self.current(t,epoch) || life.finished || life.failed { return; }
        if life.sent_count>=MAX_SENT_PER_RESOURCE {
            self.sent_omitted=self.sent_omitted.saturating_add(1);
            life.chain_invalid=true;
            return;
        }
        life.sent_count+=1;
        match event {
            Some(Sent::Initial(route)) => {
                increment(&mut self.sent_initial,route);
                if life.sent_count!=1 || life.chain_invalid {
                    self.sent_sequence_errors=self.sent_sequence_errors.saturating_add(1);
                    life.chain_invalid=true;
                }
                life.last_sent=Some(route);
            }
            Some(Sent::Redirect {from,to,status}) => {
                increment(&mut self.sent_redirect,to);
                // Only the exact fixed origin fixture edge can count as
                // linked; "other" never acts as identity by category.
                let continuous=!life.chain_invalid && from!="other"
                    && life.last_sent==Some(from) && matches!(status,301|302|303|307|308);
                if continuous && from=="redirect_route" && to=="redirect_target_route" && status==302 {
                    self.fixture_redirect_edges=self.fixture_redirect_edges.saturating_add(1);
                } else {
                    self.unlinked_redirects=self.unlinked_redirects.saturating_add(1);
                }
                if !continuous || to=="other" { life.chain_invalid=true; }
                life.last_sent=Some(to);
            }
            None => {
                self.sent_sequence_errors=self.sent_sequence_errors.saturating_add(1);
                life.chain_invalid=true;
            }
        }
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
                "coverage_complete":false,"redirect_linkage_verified":false,
                "frame_identity_verified":false,"network_dispatch_verified":false});
        }
        serde_json::json!({
            "observed":self.tracked>0,
            "tracked":self.tracked,
            "omitted":self.omitted,
            "started_routes":self.started,
            "uri_notification_routes":self.uri_notifications,
            "sent_initial_routes":self.sent_initial,
            "sent_redirect_routes":self.sent_redirect,
            "fixture_redirect_edges_observed":self.fixture_redirect_edges,
            "unlinked_redirects":self.unlinked_redirects,
            "sent_sequence_errors":self.sent_sequence_errors,
            "sent_omitted":self.sent_omitted,
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
        let (s,l,k)=(state.clone(),ledger.clone(),life.clone());
        // GLib 0.18 ObjectExt::connect_local uses checked GValues and runs
        // on the GTK thread. This avoids the generated nonnullable trampoline.
        resource.connect_local("sent-request",false,move |values| {
            let epoch=s.lock().observed_epoch();
            let event=decode_sent(values);
            l.lock().sent(ticket,epoch,&mut k.lock(),event);
            None
        });
        let (s,l,k)=(state.clone(),ledger.clone(),life);
        resource.connect_finished(move |_| {
            let epoch=s.lock().observed_epoch();
            l.lock().finish(ticket,epoch,&mut k.lock());
        });
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
    fn null_initial_and_exact_fixture_redirect_sequence() {
        let mut l=Ledger::default();
        let epoch=uuid::Uuid::new_v4();
        let t=l.begin(Some(epoch),"redirect_route").unwrap();
        let mut life=Lifecycle::default();
        l.sent(t,Some(epoch),&mut life,Some(Sent::Initial("redirect_route")));
        l.sent(t,Some(epoch),&mut life,Some(Sent::Redirect {
            from:"redirect_route",to:"redirect_target_route",status:302 }));
        assert_eq!(l.fixture_redirect_edges,1);
        assert_eq!(l.unlinked_redirects,0);
        assert_eq!(l.sent_initial["redirect_route"],1);
        assert_eq!(l.sent_redirect["redirect_target_route"],1);
    }

    #[test]
    fn malformed_missing_predecessor_and_other_cannot_join() {
        for initial in [None,Some(Sent::Initial("other")),Some(Sent::Initial("image_route"))] {
            let mut l=Ledger::default();
            let epoch=uuid::Uuid::new_v4();
            let t=l.begin(Some(epoch),"redirect_route").unwrap();
            let mut life=Lifecycle::default();
            l.sent(t,Some(epoch),&mut life,initial);
            l.sent(t,Some(epoch),&mut life,Some(Sent::Redirect {
                from:"redirect_route",to:"redirect_target_route",status:302 }));
            assert_eq!(l.fixture_redirect_edges,0);
            assert_eq!(l.unlinked_redirects,1);
        }
        assert!(decode_sent(&[]).is_none());
    }

    #[test]
    fn stale_terminal_duplicate_and_excess_sent_events() {
        let mut l=Ledger::default();
        let epoch=uuid::Uuid::new_v4();
        let t=l.begin(Some(epoch),"redirect_route").unwrap();
        let mut life=Lifecycle::default();
        let initial=Some(Sent::Initial("redirect_route"));
        l.sent(t,Some(uuid::Uuid::new_v4()),&mut life,initial);
        assert_eq!(life.sent_count,0);
        for _ in 0..=MAX_SENT_PER_RESOURCE { l.sent(t,Some(epoch),&mut life,initial); }
        assert_eq!(l.sent_omitted,1);
        assert_eq!(l.sent_sequence_errors,u64::from(MAX_SENT_PER_RESOURCE)-1);
        l.finish(t,Some(epoch),&mut life);
        l.sent(t,Some(epoch),&mut life,initial);
        assert_eq!(l.sent_omitted,1);
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
