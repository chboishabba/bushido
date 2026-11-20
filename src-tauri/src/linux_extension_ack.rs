//! Ephemeral challenge/reply observation; not runtime attestation.
use gtk::{gio,glib::{self,prelude::ObjectExt,variant::ToVariant}};
use gio::prelude::CancellableExt;
use webkit2gtk::{UserMessage,WebView,WebViewExt};

const QUERY:&str="bushido.fixture.policy.challenge.v2";
const REPLY:&str="bushido.fixture.policy.reply.v2";

fn hex32(s:&str)->bool {
    s.len()==32 && s.bytes().all(|b|b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Debug)]
pub(crate) struct Ack {
    pub status:&'static str,
    pub trace:Option<serde_json::Value>,
}
impl Ack {
    fn failure(status:&'static str)->Self { Self{status,trace:None} }
}

// One bounded native reply contains the full prefix from the same page
// state used by on_send. No joining by asynchronous aggregate counts.
fn decode(expected:&str,nonce:&str,name:&str,payload:Option<&str>,fd:bool)->Ack {
    if name!=REPLY || fd {return Ack::failure("malformed");}
    let Some(payload)=payload.filter(|p|p.len()<=600) else {return Ack::failure("malformed");};
    let fields:Vec<_>=payload.split('|').collect();
    if fields.len()!=5 {return Ack::failure("malformed");}
    if fields[0]!=nonce {return Ack::failure("challenge_mismatch");}
    if fields[1]=="invalid" {return Ack::failure("invalid_initialization");}
    if fields[1]!=expected {return Ack::failure("generation_or_table_mismatch");}
    let Some((_,table))=expected.rsplit_once(':') else {return Ack::failure("malformed");};
    if table.len()!=4 || !table.bytes().all(|b|matches!(b,b'A'|b'D'|b'X')) || !hex32(fields[2]) {
        return Ack::failure("malformed");
    }
    let Ok(omitted)=fields[3].parse::<u64>() else {return Ack::failure("malformed");};
    if omitted.to_string()!=fields[3] {return Ack::failure("malformed");}
    let bytes=fields[4].as_bytes();
    if bytes.len()>384 || bytes.len()%3!=0 || (omitted>0 && bytes.len()!=384) {
        return Ack::failure("malformed");
    }
    let mut events=Vec::new();
    let routes=["extension_allow_route","extension_deny_route","extension_redirect_route","extension_redirect_target_route"];
    for (i,e) in bytes.chunks_exact(3).enumerate() {
        if !(b'0'..=b'3').contains(&e[0]) || !matches!(e[2],b'I'|b'R') {
            return Ack::failure("malformed");
        }
        let route=usize::from(e[0]-b'0');
        if e[1]!=table.as_bytes()[route] {return Ack::failure("applied_decision_mismatch");}
        events.push(serde_json::json!({"ordinal":i+1,"route":routes[route],
            "decision":char::from(e[1]),"previous_response_present":e[2]==b'R'}));
    }
    Ack {status:"matching_reply_observed",trace:Some(serde_json::json!({
        "scope":"reported_native_page_lifetime","page_instance":fields[2],
        "policy_snapshot":expected,"events":events,"omitted":omitted,
        "reported_prefix_matches_table":true,"reported_prefix_nonempty":!bytes.is_empty(),
        "reported_prefix_untruncated":omitted==0,
        "document_epoch_linkage_verified":false,"physical_dispatch_verified":false
    }))}
}

pub(crate) fn probe(view:&WebView,expected:String,done:impl FnOnce(Ack)+'static) {
    let nonce=uuid::Uuid::new_v4().simple().to_string();
    let message=UserMessage::new(QUERY,Some(&nonce.to_variant()));
    let cancellable=gio::Cancellable::new();
    let cancel=cancellable.clone();
    // Independent native timeout bounds a missing reply. The outer command
    // has its own 5s deadline; callback never retries or persists authority.
    glib::timeout_add_local_once(std::time::Duration::from_secs(3),move ||cancel.cancel());
    view.send_message_to_page(&message,Some(&cancellable),move |result| {
        let outcome=match result {
            Err(_)=>Ack::failure("no_reply"),
            Ok(reply)=>{
                let name=reply.property::<Option<String>>("name");
                let fd=reply.property::<Option<gio::UnixFDList>>("fd-list").is_some();
                let payload=reply.property::<Option<glib::Variant>>("parameters").and_then(|p|p.get::<String>());
                decode(&expected,&nonce,name.as_deref().unwrap_or(""),payload.as_deref(),fd)
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
    const PAGE:&str="cccccccccccccccccccccccccccccccc";
    fn payload(policy:&str,events:&str,omitted:u64)->String {
        format!("{NONCE}|{policy}|{PAGE}|{omitted}|{events}")
    }
    #[test]
    fn matching_native_reply_shape() {
        let p=payload(EXPECTED,"0AI1DI2AI3DR",0);
        let a=decode(EXPECTED,NONCE,REPLY,Some(&p),false);
        assert_eq!(a.status,"matching_reply_observed");
        assert_eq!(a.trace.unwrap()["events"].as_array().unwrap().len(),4);
        assert_eq!(decode(EXPECTED,NONCE,REPLY,Some(&p),true).status,"malformed");
        assert_eq!(decode(EXPECTED,NONCE,"wrong",Some(&p),false).status,"malformed");
    }
    #[test]
    fn stale_generation_and_same_generation_wrong_table() {
        for stale in [EXPECTED.replace("aaaa","dddd"),EXPECTED.replace("ADAD","AAAA")] {
            assert_eq!(decode(EXPECTED,NONCE,REPLY,Some(&payload(&stale,"",0)),false).status,"generation_or_table_mismatch");
        }
        assert_eq!(decode(EXPECTED,"old",REPLY,Some(&payload(EXPECTED,"",0)),false).status,"challenge_mismatch");
    }
    #[test]
    fn absent_or_malformed_initialization_is_not_acknowledgement() {
        assert_eq!(decode(EXPECTED,NONCE,REPLY,None,false).status,"malformed");
        assert_eq!(decode(EXPECTED,NONCE,REPLY,Some("bad"),false).status,"malformed");
        assert_eq!(decode(EXPECTED,NONCE,REPLY,Some(&payload("invalid","",0)),false).status,"invalid_initialization");
    }
    #[test]
    fn wrong_applied_decision_and_malformed_events() {
        assert_eq!(decode(EXPECTED,NONCE,REPLY,Some(&payload(EXPECTED,"1AI",0)),false).status,"applied_decision_mismatch");
        for events in ["4AI","0AZ","0A"] {
            assert_eq!(decode(EXPECTED,NONCE,REPLY,Some(&payload(EXPECTED,events,0)),false).status,"malformed");
        }
    }
    #[test]
    fn bounded_prefix_and_explicit_truncation() {
        let full="0AI".repeat(128);
        let a=decode(EXPECTED,NONCE,REPLY,Some(&payload(EXPECTED,&full,1)),false);
        assert_eq!(a.status,"matching_reply_observed");
        assert_eq!(a.trace.unwrap()["reported_prefix_untruncated"],false);
        assert_eq!(decode(EXPECTED,NONCE,REPLY,Some(&payload(EXPECTED,"",1)),false).status,"malformed");
        assert_eq!(decode(EXPECTED,NONCE,REPLY,Some(&payload(EXPECTED,&"0AI".repeat(129),0)),false).status,"malformed");
        let a=decode(EXPECTED,NONCE,REPLY,Some(&payload(EXPECTED,"",0)),false);
        assert_eq!(a.trace.unwrap()["reported_prefix_nonempty"],false);
    }
}
