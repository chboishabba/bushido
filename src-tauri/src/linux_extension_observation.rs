//! Native page-to-view diagnostic signals, not page-script messages.
//! Delivery is asynchronous: no epoch, process or executable attestation.
use gtk::glib::{self, prelude::ObjectExt, value::ToValue};
use parking_lot::Mutex;
use std::{collections::BTreeMap,sync::Arc};
use webkit2gtk::{UserMessage,WebView};

const LIMIT:u64=512;
fn category(name:&str)->Option<&'static str> {
    match name {
        "bushido.fixture.v1.page_created"=>Some("page_created"),
        "bushido.fixture.v1.deny_direct"=>Some("deny_direct"),
        "bushido.fixture.v1.deny_redirect"=>Some("deny_redirect"),
        "bushido.fixture.v1.deny_target_direct"=>Some("deny_target_direct"),
        "bushido.fixture.v1.allow_control"=>Some("allow_control"),
        "bushido.fixture.v1.allow_redirect"=>Some("allow_redirect"),
        _=>None,
    }
}

#[derive(Default)]
pub(crate) struct ExtensionObservation {
    counts:BTreeMap<&'static str,u64>,
    tracked:u64,
    omitted:u64,
    malformed:u64,
}

impl ExtensionObservation {
    fn record(&mut self,name:&str,payload:bool)->bool {
        let Some(key)=category(name) else { return false; };
        if payload {
            self.malformed=self.malformed.saturating_add(1);
        } else if self.tracked>=LIMIT {
            self.omitted=self.omitted.saturating_add(1);
        } else {
            self.tracked+=1;
            *self.counts.entry(key).or_default()+=1;
        }
        true
    }
    pub(crate) fn snapshot(&self)->serde_json::Value {
        serde_json::json!({
            "scope":"native_view_lifetime",
            "counts":self.counts,"tracked":self.tracked,
            "omitted":self.omitted,"malformed":self.malformed,
            "document_epoch_linkage_verified":false,
            "extension_binary_linkage_verified":false,
            "pre_dispatch_enforcement_verified":false,
            "native_acceptance":"OPEN"
        })
    }
}

pub(crate) fn attach(view:&WebView,ledger:Arc<Mutex<ExtensionObservation>>) {
    view.connect_local("user-message-received",false,move |values| {
        let handled=(|| {
            if values.len()!=2 { return false; }
            let Ok(message)=values[1].get::<UserMessage>() else { return false; };
            let Some(name)=message.property::<Option<String>>("name") else { return false; };
            let has_payload=message.property::<Option<glib::Variant>>("parameters").is_some()
                || message.property::<Option<gtk::gio::UnixFDList>>("fd-list").is_some();
            ledger.lock().record(&name,has_payload)
        })();
        // Fixed diagnostic event only. No reply, no call into application
        // commands, no authority or passage from JavaScript IPC.
        Some(handled.to_value())
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_fixed_names_only_and_payload_rejected() {
        let mut l=ExtensionObservation::default();
        assert!(!l.record("unknown",false));
        assert!(l.record("bushido.fixture.v1.deny_direct",true));
        assert_eq!(l.malformed,1);
        assert_eq!(l.tracked,0);
        assert!(l.record("bushido.fixture.v1.deny_direct",false));
        assert_eq!(l.counts["deny_direct"],1);
    }
    #[test]
    fn view_lifetime_cap() {
        let mut l=ExtensionObservation::default();
        for _ in 0..=LIMIT { assert!(l.record("bushido.fixture.v1.allow_control",false)); }
        assert_eq!(l.tracked,LIMIT);
        assert_eq!(l.omitted,1);
    }
}
