//! Finite synthetic policy projection, not an ABP compiler or universal
//! frame-independent theorem. Only explicit isolated debug fixture uses it.
use crate::blocker;
use adblock::engine::Engine;

const ROOT:&str="https://bushido-tunnel.test/";
const ROUTES:[&str;4]=["extension-allow","extension-deny","extension-redirect-deny","extension-redirect-deny-target"];
// Explicit finite ambiguity check. Not the set of all WebKit contexts.
const SOURCES:[&str;3]=[ROOT,"https://bushido-tunnel.test/frame","https://other.test/"];
const TYPES:[&str;9]=["document","subdocument","script","image","xmlhttprequest","fetch","stylesheet","media","other"];

fn decision(engine:&Engine,url:&str)->char {
    let mut seen=None;
    for source in SOURCES {
        for kind in TYPES {
            let Some(d)=blocker::evaluate_request(engine,url,source,kind) else {return 'X';};
            let next=if d.blocked && d.rewrite_url.is_none() && d.redirect_url.as_deref()==Some("about:blank") {
                'D'
            } else if !d.blocked && d.rewrite_url.is_none() && d.redirect_url.is_none() {
                'A'
            } else {return 'X';};
            if seen.is_some_and(|v|v!=next) {return 'X';}
            seen=Some(next);
        }
    }
    seen.unwrap_or('X')
}

pub(crate) fn launch_snapshot(engine:&Engine)->String {
    let generation=uuid::Uuid::new_v4().simple().to_string();
    let projected=snapshot(engine);
    let table=projected.strip_prefix("bushido.fixture.policy.v1:").expect("original projection format");
    format!("bushido.fixture.policy.v2:{generation}:{table}")
}

pub(crate) fn snapshot(engine:&Engine)->String {
    let table:String=ROUTES.iter().map(|route| {
        decision(engine,&format!("{ROOT}{route}"))
    }).collect();
    format!("bushido.fixture.policy.v1:{table}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use adblock::lists::{FilterSet,ParseOptions};
    fn engine(rules:&[&str])->Engine {
        let mut set=FilterSet::new(false);
        set.add_filters(rules,ParseOptions::default());
        Engine::from_filter_set(set,true)
    }
    #[test]
    fn exact_rules_and_allow_controls() {
        let e=engine(&["|https://bushido-tunnel.test/extension-deny|$all",
            "|https://bushido-tunnel.test/extension-redirect-deny-target|$all"]);
        assert_eq!(snapshot(&e),"bushido.fixture.policy.v1:ADAD");
        assert_eq!(decision(&e,"https://bushido-tunnel.test/extension-deny?extra=1"),'A');
    }
    #[test]
    fn context_sensitive_rule_abstains() {
        let e=engine(&["|https://bushido-tunnel.test/extension-deny|$script"]);
        assert_eq!(snapshot(&e),"bushido.fixture.policy.v1:AXAA");
    }
    #[test]
    fn same_table_new_launch_identity() {
        let e=Engine::default();
        let a=launch_snapshot(&e);
        let b=launch_snapshot(&e);
        assert_ne!(a,b);
        assert!(a.ends_with(":AAAA") && b.ends_with(":AAAA"));
        assert_eq!(a.len(),"bushido.fixture.policy.v2:".len()+32+1+4);
    }
    #[test]
    fn empty_engine_does_not_manufacture_denials() {
        assert_eq!(snapshot(&Engine::default()),"bushido.fixture.policy.v1:AAAA");
    }
}
