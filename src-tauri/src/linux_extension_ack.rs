//! Ephemeral challenge/reply observation; not runtime attestation.
use gtk::{gio,glib::{self,prelude::ObjectExt,variant::ToVariant}};
use gio::prelude::CancellableExt;
use webkit2gtk::{UserMessage,WebView,WebViewExt};

const QUERY:&str="bushido.fixture.policy.challenge.v1";
const REPLY:&str="bushido.fixture.policy.reply.v1";

fn status(expected:&str,nonce:&str,name:&str,payload:Option<&str>,fd:bool)->&'static str {
    if name!=REPLY || fd {return "malformed";}
    let Some(payload)=payload else {return "malformed";};
    let Some((echo,received))=payload.split_once(':') else {return "malformed";};
    if echo!=nonce {return "challenge_mismatch";}
    if received=="invalid" {return "invalid_initialization";}
    if received!=expected {return "generation_or_table_mismatch";}
    "matching_reply_observed"
}

pub(crate) fn probe(view:&WebView,expected:String,done:impl FnOnce(&'static str)+'static) {
    let nonce=uuid::Uuid::new_v4().simple().to_string();
    let message=UserMessage::new(QUERY,Some(&nonce.to_variant()));
    let cancellable=gio::Cancellable::new();
    let cancel=cancellable.clone();
    // Independent native timeout bounds a missing reply. The outer command
    // has its own 5s deadline; callback never retries or persists authority.
    glib::timeout_add_local_once(std::time::Duration::from_secs(3),move ||cancel.cancel());
    view.send_message_to_page(&message,Some(&cancellable),move |result| {
        let outcome=match result {
            Err(_)=>"no_reply",
            Ok(reply)=>{
                let name=reply.property::<Option<String>>("name");
                let fd=reply.property::<Option<gio::UnixFDList>>("fd-list").is_some();
                let payload=reply.property::<Option<glib::Variant>>("parameters").and_then(|p|p.get::<String>());
                status(&expected,&nonce,name.as_deref().unwrap_or(""),payload.as_deref(),fd)
            }
        };
        done(outcome);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    const EXPECTED:&str="bushido.fixture.policy.v2:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:ADAD";
    const NONCE:&str="bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    #[test]
    fn matching_native_reply_shape() {
        let p=format!("{NONCE}:{EXPECTED}");
        assert_eq!(status(EXPECTED,NONCE,REPLY,Some(&p),false),"matching_reply_observed");
        assert_eq!(status(EXPECTED,NONCE,REPLY,Some(&p),true),"malformed");
        assert_eq!(status(EXPECTED,NONCE,"wrong",Some(&p),false),"malformed");
    }
    #[test]
    fn stale_generation_and_same_generation_wrong_table() {
        for stale in [EXPECTED.replace("aaaa","cccc"),EXPECTED.replace("ADAD","AAAA")] {
            let p=format!("{NONCE}:{stale}");
            assert_eq!(status(EXPECTED,NONCE,REPLY,Some(&p),false),"generation_or_table_mismatch");
        }
        let p=format!("old:{EXPECTED}");
        assert_eq!(status(EXPECTED,NONCE,REPLY,Some(&p),false),"challenge_mismatch");
    }
    #[test]
    fn absent_or_malformed_initialization_is_not_acknowledgement() {
        assert_eq!(status(EXPECTED,NONCE,REPLY,None,false),"malformed");
        assert_eq!(status(EXPECTED,NONCE,REPLY,Some("bad"),false),"malformed");
        assert_eq!(status(EXPECTED,NONCE,REPLY,Some(&format!("{NONCE}:invalid")),false),"invalid_initialization");
    }
}
