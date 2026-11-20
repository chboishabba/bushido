//! Explicit operator-selected synthetic destination scope. A preview is not
//! an admission token. CONNECT authority alone does not identify a tab,
//! document, frame or encrypted request. Never let DNS mint its own scope.
use super::{blocker, live_native_tab_owner, valid_workspace_identity, BlockerState, NativeTabOwner};
use std::net::{Ipv4Addr, SocketAddr};
use tauri::Manager;

const URL: &str = "https://bushido-tunnel.test/";
const DNS_NAME: &str = "bushido-tunnel.test.";

pub(crate) struct OperatorScope {
    workspace: String,
    endpoint: SocketAddr,
}

impl OperatorScope {
    pub(crate) fn from_environment(fixture_enabled: bool) -> Result<Option<Self>, String> {
        let workspace = std::env::var_os("BUSHIDO_LINUX_SCOPE_WORKSPACE");
        let port = std::env::var_os("BUSHIDO_LINUX_SCOPE_ORIGIN_PORT");
        match (workspace,port) {
            (None,None) => Ok(None),
            (Some(w),Some(p)) if fixture_enabled && cfg!(debug_assertions) =>
                Self::checked(w.to_str().ok_or("Invalid scope workspace")?,
                              p.to_str().ok_or("Invalid scope port")?).map(Some),
            _ => Err("Scope needs debug egress fixture and both explicit operator settings".into()),
        }
    }

    fn checked(workspace: &str, raw_port: &str) -> Result<Self,String> {
        if !valid_workspace_identity(workspace) || raw_port.is_empty()
            || !raw_port.bytes().all(|b| b.is_ascii_digit()) {
            return Err("Invalid operator scope".into());
        }
        let port: u16 = raw_port.parse().map_err(|_| "Invalid scope port")?;
        if port < 1024 { return Err("Scope port must be unprivileged".into()); }
        Ok(Self { workspace:workspace.to_owned(), endpoint:SocketAddr::from((Ipv4Addr::LOCALHOST,port)) })
    }

    fn propose(&self, owner: &NativeTabOwner, source: &url::Url,
               engine: &adblock::engine::Engine) -> Result<ScopeProposal,String> {
        if owner.workspace != self.workspace || !valid_workspace_identity(&owner.workspace) {
            return Err("Scope workspace mismatch".into());
        }
        // Deliberately only the fixed synthetic top-level document. No
        // whitelist bypass, source spoofing, paths, headers or query rewrite.
        if source.as_str() != URL {
            return Err("Scope requires the fixed synthetic committed document".into());
        }
        let decision = blocker::evaluate_request(engine, URL, source.as_str(), "document")
            .ok_or("Original request policy unavailable")?;
        if decision.blocked || decision.rewrite_url.is_some() || decision.redirect_url.is_some() {
            return Err("Original request policy rejects unchanged synthetic destination".into());
        }
        Ok(ScopeProposal { owner:owner.clone(), source:source.clone(), endpoint:self.endpoint })
    }
}

// Internal diagnostic observation. No serialization, clone or redeem method.
struct ScopeProposal {
    owner: NativeTabOwner,
    source: url::Url,
    endpoint: SocketAddr,
}

impl ScopeProposal {
    fn matches(&self, owner: &NativeTabOwner, source: &url::Url) -> bool {
        &self.owner == owner && &self.source == source
    }
}

// Actual native owner and committed WebView URL, never caller-provided
// workspace/source/port/hostname. Returns aggregate booleans, not capability.
#[cfg(debug_assertions)]
pub(crate) async fn preview(app: &tauri::AppHandle, tab_id: &str)
    -> Result<serde_json::Value,String> {
    use crate::{linux_document_epoch::DocumentEpochs, linux_egress_fixture::EgressFixture};
    let owner=live_native_tab_owner(app,tab_id).ok_or("No live ordinary tab owner")?;
    let view=app.get_webview(tab_id).ok_or("No live view")?;
    let app=app.clone();
    let id=tab_id.to_owned();
    let (tx,rx)=tokio::sync::oneshot::channel();
    view.with_webview(move |platform| {
        if tx.is_closed() { return; }
        let result=(|| -> Result<serde_json::Value,String> {
            if !crate::native_tab_matches_owner(&app,&id,&owner) {
                return Err("Tab owner changed".into());
            }
            let native=platform.inner();
            let fixture=app.state::<EgressFixture>();
            let scope=fixture.scope().ok_or("No explicit operator fixture scope")?;
            let epochs=app.state::<DocumentEpochs>();
            let doc=epochs.snapshot(&id,&owner,&native)
                .ok_or("No complete observed native load; reload the synthetic document")?;
            let blocker=app.state::<BlockerState>();
            let engine=blocker.engine.try_read().ok_or("Original request policy busy")?;
            let proposal=scope.propose(&owner,&doc.url,&engine)?;
            let current_owner=live_native_tab_owner(&app,&id).ok_or("Tab owner lost")?;
            let current_doc=epochs.snapshot(&id,&current_owner,&native)
                .ok_or("Native document observation lost")?;
            if !proposal.matches(&current_owner,&current_doc.url) || doc!=current_doc {
                return Err("Native scope observation became stale".into());
            }
            let resources=epochs.resource_snapshot(&id,&current_owner,&native,&current_doc)
                .ok_or("Native resource observation lost")?;
            Ok(serde_json::json!({
                "version":"bushido.egress.scope_preview.v4",
                "resource_observation":resources,
                "state":"proposal_only",
                "non_authoritative":true,
                "native_owner_matches":true,
                "operator_scope_matches":true,
                "original_document_policy_allows":true,
                "fixed_loopback_endpoint":proposal.endpoint.ip().is_loopback(),
                "fixed_dns_name":DNS_NAME,
                "native_load_epoch_observed":true,
                "document_epoch_verified":false,
                "request_attribution_verified":false,
                "execution_authorized":false,
                "native_acceptance":"OPEN"
            }))
        })();
        let _=tx.send(result);
    }).map_err(|e| e.to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(5),rx).await
        .map_err(|_| "Scope preview dispatch timed out".to_string())?
        .map_err(|_| "Scope preview callback lost".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use adblock::{engine::Engine, lists::{FilterSet, ParseOptions}};
    use crate::NativeTabOwners;

    fn source() -> url::Url { URL.parse().unwrap() }

    #[test]
    fn explicit_scope_only_fixed_local_destination() {
        let s=OperatorScope::checked("workspace-a","8765").unwrap();
        assert_eq!(s.endpoint,"127.0.0.1:8765".parse().unwrap());
        for (w,p) in [("","8765"),("../x","8765"),("a","80"),("a","65536"),("a","https://other")] {
            assert!(OperatorScope::checked(w,p).is_err());
        }
    }

    #[test]
    fn actual_owner_replacement_and_workspace_reject() {
        let owners=NativeTabOwners::default();
        let s=OperatorScope::checked("workspace-a","8765").unwrap();
        owners.register("tab","workspace-a");
        let a=owners.get("tab").unwrap();
        let proposal=s.propose(&a,&source(),&Engine::default()).unwrap();
        assert!(proposal.matches(&a,&source()));
        owners.remove("tab");
        owners.register("tab","workspace-a");
        let b=owners.get("tab").unwrap();
        assert!(!proposal.matches(&b,&source()));
        owners.register("tab","workspace-b");
        assert!(s.propose(&owners.get("tab").unwrap(),&source(),&Engine::default()).is_err());
    }

    #[test]
    fn original_blocker_and_exact_document_gate() {
        let owners=NativeTabOwners::default();
        owners.register("tab","workspace-a");
        let owner=owners.get("tab").unwrap();
        let scope=OperatorScope::checked("workspace-a","8765").unwrap();
        let mut filters=FilterSet::new(false);
        filters.add_filters(&["||bushido-tunnel.test^$document"],ParseOptions::default());
        let engine=Engine::from_filter_set(filters,true);
        // Exact original decision, not an assumed blocked result.
        assert!(blocker::evaluate_request(&engine,URL,URL,"document").unwrap().blocked);
        assert!(scope.propose(&owner,&source(),&engine).is_err());
        for raw in ["http://bushido-tunnel.test/","https://bushido-tunnel.test/path",
                    "https://bushido-tunnel.test/?utm_source=x","https://other.test/"] {
            assert!(scope.propose(&owner,&raw.parse().unwrap(),&Engine::default()).is_err());
        }
    }
}
