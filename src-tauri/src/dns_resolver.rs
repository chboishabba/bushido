use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use crate::dns_positive_cache::{self, PositiveCache};

use axum::{Router, body::Bytes, extract::State, http::{StatusCode, header}, routing::{get, post}};
use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::RData;
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
use parking_lot::{Mutex, RwLock};

const MAX_DNS_MESSAGE: usize = 65_535;
const UPSTREAM_TIMEOUT: u64 = 3;
const PRIMARY: &str = "https://cloudflare-dns.com/dns-query";
const FALLBACK: &str = "https://dns.google/dns-query";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum PrivacyLevel {
    Standard,  // doh + basic tracker blocklist
    Strict,    // + cname uncloaking + expanded lists
    Maximum,   // + deny alternate DoH upstream (not a browser-wide egress guarantee)
}

impl Default for PrivacyLevel { fn default() -> Self { Self::Strict } }

const CNAME_TRACKERS: &[&str] = &[
    "data.adobedc.net", "sc.omtrdc.net", "demdex.net", "2o7.net",
    "omniture.com", "everesttech.net", "tt.omtrdc.net", "eulerian.net",
    "dnsdelegation.io", "at-o.net", "keyade.com", "storetail.io",
    "affex.org", "wt-eu02.net", "webtrekk.net", "tracedock.com",
    "oghub.io", "wizaly.com", "npttech.com", "tagcommander.com",
];

// A policy snapshot expires on every actual policy transition, including A -> B -> A.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Policy { level: PrivacyLevel, generation: u64 }

impl Policy {
    fn change(&mut self, level: PrivacyLevel) -> Result<(), String> {
        if self.level != level {
            self.generation = self.generation.checked_add(1).ok_or("DNS policy generation exhausted")?;
            self.level = level;
        }
        Ok(())
    }
}

pub fn parse_level(value: &str) -> Result<PrivacyLevel, String> {
    match value {
        "standard" => Ok(PrivacyLevel::Standard),
        "strict" => Ok(PrivacyLevel::Strict),
        "maximum" => Ok(PrivacyLevel::Maximum),
        _ => Err("Unknown DNS privacy level".into()),
    }
}

#[derive(Clone)]
pub struct ResolverControl(Arc<Resolver>);

impl ResolverControl {
    pub fn set_level(&self, value: &str) -> Result<(), String> {
        let mut policy = self.0.policy.write();
        let previous = *policy;
        policy.change(parse_level(value)?)?;
        if *policy != previous { self.0.cache.lock().clear(); }
        Ok(())
    }
}

pub struct Stats {
    pub queries: AtomicU64,
    pub hits: AtomicU64,
    pub blocked: AtomicU64,
    pub cname_blocked: AtomicU64,
    pub upstream: AtomicU64,
    pub errors: AtomicU64,
}

struct DohReply { bytes: Vec<u8>, started: Instant, cache_max_age: Option<u32> }

struct Resolver {
    cache: Mutex<PositiveCache>,
    cache_enabled: bool,
    blocklist: RwLock<HashSet<String>>,
    client: reqwest::Client,
    stats: Arc<Stats>,
    policy: RwLock<Policy>,
}

impl Resolver {
    fn new(level: PrivacyLevel) -> Result<Arc<Self>, String> {
        let mut bl = HashSet::new();
        for t in CNAME_TRACKERS { bl.insert(t.to_string()); }

        Ok(Arc::new(Self {
            cache: Mutex::new(PositiveCache::default()),
            cache_enabled: cfg!(debug_assertions) && std::env::var("BUSHIDO_DOH_POSITIVE_CACHE").as_deref() == Ok("1"),
            blocklist: RwLock::new(bl),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(UPSTREAM_TIMEOUT + 2))
                .https_only(true)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                // Bootstrap without system DNS. TLS still validates the original hostname.
                .resolve("cloudflare-dns.com", SocketAddr::from(([1, 1, 1, 1], 443)))
                .resolve("dns.google", SocketAddr::from(([8, 8, 8, 8], 443)))
                .build().map_err(|e| e.to_string())?,
            stats: Arc::new(Stats {
                queries: AtomicU64::new(0),
                hits: AtomicU64::new(0),
                blocked: AtomicU64::new(0),
                cname_blocked: AtomicU64::new(0),
                upstream: AtomicU64::new(0),
                errors: AtomicU64::new(0),
            }),
            policy: RwLock::new(Policy { level, generation: 0 }),
        }))
    }

    fn is_blocked(&self, domain: &str) -> bool {
        let d = domain.trim_end_matches('.').to_ascii_lowercase();
        self.blocklist.read().iter().any(|b| d == b.as_str() || d.ends_with(&format!(".{}", b)))
    }

    fn check_policy(&self, snapshot: Policy) -> Result<(), String> {
        if *self.policy.read() != snapshot { return Err("DNS policy changed during query".into()); }
        Ok(())
    }

    async fn query(&self, bytes: &[u8], snapshot: Policy) -> Result<DohReply, String> {
        self.check_policy(snapshot)?;
        if let Ok(r) = self.doh(PRIMARY, bytes).await {
            self.check_policy(snapshot)?;
            return Ok(r);
        }
        self.check_policy(snapshot)?;
        if snapshot.level == PrivacyLevel::Maximum {
            return Err("primary failed, maximum mode blocks fallback".into());
        }
        let mut result = self.doh(FALLBACK, bytes).await?;
        self.check_policy(snapshot)?;
        result.cache_max_age = None; // never replay alternate-provider results
        Ok(result)
    }

    async fn doh(&self, url: &str, bytes: &[u8]) -> Result<DohReply, String> {
        let started = Instant::now();
        let mut r = self.client.post(url)
            .header("content-type", "application/dns-message")
            .header("accept", "application/dns-message")
            .header("cache-control", "no-cache")
            .body(bytes.to_vec())
            .timeout(Duration::from_secs(UPSTREAM_TIMEOUT))
            .send().await.map_err(|e| e.to_string())?;
        if !r.status().is_success() { return Err(format!("{}", r.status())); }
        if !r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(';').next().is_some_and(|v| v.trim().eq_ignore_ascii_case("application/dns-message"))) {
            return Err("Unexpected DNS response media type".into());
        }
        // RFC 8484 requires accounting for HTTP Age. Until general
        // message-aware aging exists, refuse nonzero/malformed/duplicate Age
        // rather than forward overstated TTLs. Absence is treated as zero.
        let mut ages = r.headers().get_all(reqwest::header::AGE).iter();
        if let Some(age) = ages.next() {
            if age.to_str().ok().and_then(|v| v.parse::<u64>().ok()) != Some(0) || ages.next().is_some() {
                return Err("Aged or ambiguous upstream DNS response".into());
            }
        }
        let mut controls = r.headers().get_all(reqwest::header::CACHE_CONTROL).iter();
        let cache_max_age = controls.next().and_then(|v| v.to_str().ok())
            .and_then(dns_positive_cache::explicit_max_age);
        let cache_max_age = if controls.next().is_none()
            && !r.headers().contains_key(reqwest::header::VARY)
            && !r.headers().contains_key(reqwest::header::SET_COOKIE)
            { cache_max_age } else { None };
        let mut out = Vec::new();
        while let Some(chunk) = r.chunk().await.map_err(|e| e.to_string())? {
            if chunk.len() > MAX_DNS_MESSAGE.saturating_sub(out.len()) {
                return Err("Oversized DNS response".into());
            }
            out.extend_from_slice(&chunk);
        }
        Ok(DohReply { bytes: out, started, cache_max_age })
    }

    fn nxdomain(req: &Message) -> Vec<u8> {
        let mut r = Message::new();
        r.set_id(req.id());
        r.set_message_type(MessageType::Response);
        r.set_op_code(OpCode::Query);
        r.set_response_code(ResponseCode::NXDomain);
        r.set_recursion_desired(req.recursion_desired());
        r.set_recursion_available(true);
        for q in req.queries() { r.add_query(q.clone()); }
        r.to_bytes().unwrap_or_default()
    }

    fn cnames(msg: &Message) -> Vec<String> {
        msg.answers().iter().filter_map(|r| match r.data() {
            RData::CNAME(c) => Some(c.0.to_ascii()),
            _ => None,
        }).collect()
    }

    fn checked_request(raw: &[u8]) -> Result<Message, String> {
        if raw.len() < 12 || raw.len() > MAX_DNS_MESSAGE { return Err("Invalid DNS message size".into()); }
        let req = Message::from_bytes(raw).map_err(|e| e.to_string())?;
        if req.message_type() != MessageType::Query || req.op_code() != OpCode::Query
            || req.queries().len() != 1 || req.truncated() || !req.answers().is_empty()
            || !req.name_servers().is_empty() {
            return Err("Unsupported DNS request shape".into());
        }
        Ok(req)
    }

    fn checked_response(req: &Message, raw: &[u8]) -> Result<Message, String> {
        let msg = Message::from_bytes(raw).map_err(|e| e.to_string())?;
        if msg.message_type() != MessageType::Response || msg.op_code() != req.op_code()
            || msg.id() != req.id() || msg.queries() != req.queries() || msg.truncated() {
            return Err("DNS response does not match request".into());
        }
        Ok(msg)
    }

    async fn resolve(&self, raw: &[u8]) -> Result<Vec<u8>, String> {
        self.stats.queries.fetch_add(1, Ordering::Relaxed);
        let req = Self::checked_request(raw)?;
        let q = &req.queries()[0];
        let snapshot = *self.policy.read();
        if self.is_blocked(&q.name().to_ascii()) {
            self.stats.blocked.fetch_add(1, Ordering::Relaxed);
            self.check_policy(snapshot)?;
            return Ok(Self::nxdomain(&req));
        }
        let key = if self.cache_enabled { dns_positive_cache::key(raw, snapshot.generation) } else { None };
        if let Some(key) = &key {
            // Shared lock order: policy then cache. A policy write clears
            // prior entries; generation binds lookup to exact policy epoch.
            let policy = self.policy.read();
            if *policy != snapshot { return Err("DNS policy changed during query".into()); }
            if let Some(hit) = self.cache.lock().lookup(key, raw, Instant::now()) {
                Self::checked_response(&req, &hit)?;
                self.stats.hits.fetch_add(1, Ordering::Relaxed);
                return Ok(hit);
            }
        }
        self.stats.upstream.fetch_add(1, Ordering::Relaxed);
        let resp = self.query(raw, snapshot).await?;
        let msg = Self::checked_response(&req, &resp.bytes)?;
        if snapshot.level != PrivacyLevel::Standard {
            for target in Self::cnames(&msg) {
                if self.is_blocked(&target) {
                    self.stats.cname_blocked.fetch_add(1, Ordering::Relaxed);
                    self.check_policy(snapshot)?;
                    return Ok(Self::nxdomain(&req));
                }
            }
        }
        let policy = self.policy.read();
        if *policy != snapshot { return Err("DNS policy changed during query".into()); }
        if let (Some(key), Some(max_age)) = (key, resp.cache_max_age) {
            self.cache.lock().insert(key, raw, &resp.bytes, resp.started, Instant::now(), max_age);
        }
        Ok(resp.bytes)
    }
}

async fn handle_post(State(r): State<Arc<Resolver>>, headers: axum::http::HeaderMap, body: Bytes) -> axum::response::Response {
    if headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) != Some("application/dns-message") {
        return axum::response::Response::builder().status(415).body(axum::body::Body::empty()).unwrap();
    }
    match r.resolve(&body).await {
        Ok(b) => axum::response::Response::builder()
            .status(200).header(header::CONTENT_TYPE, "application/dns-message")
            .body(axum::body::Body::from(b)).unwrap(),
        Err(_) => { r.stats.errors.fetch_add(1, Ordering::Relaxed);
            axum::response::Response::builder().status(500).body(axum::body::Body::empty()).unwrap() }
    }
}

async fn handle_get(State(r): State<Arc<Resolver>>, req: axum::extract::Request) -> axum::response::Response {
    let param = req.uri().query().unwrap_or("").split('&')
        .find_map(|p| p.strip_prefix("dns=")).unwrap_or("");
    if param.is_empty() || param.len() > (MAX_DNS_MESSAGE + 2) / 3 * 4 {
        return axum::response::Response::builder().status(400).body(axum::body::Body::empty()).unwrap();
    }
    use base64::Engine;
    let raw = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(param) {
        Ok(b) => b, Err(_) => return axum::response::Response::builder()
            .status(400).body(axum::body::Body::empty()).unwrap(),
    };
    match r.resolve(&raw).await {
        Ok(b) => axum::response::Response::builder()
            .status(200).header(header::CONTENT_TYPE, "application/dns-message")
            .body(axum::body::Body::from(b)).unwrap(),
        Err(_) => { r.stats.errors.fetch_add(1, Ordering::Relaxed);
            axum::response::Response::builder().status(500).body(axum::body::Body::empty()).unwrap() }
    }
}

async fn handle_stats(State(r): State<Arc<Resolver>>) -> axum::Json<serde_json::Value> {
    let level = match r.policy.read().level {
        PrivacyLevel::Standard => "standard",
        PrivacyLevel::Strict => "strict",
        PrivacyLevel::Maximum => "maximum",
    };
    axum::Json(serde_json::json!({
        "level": level,
        "queries": r.stats.queries.load(Ordering::Relaxed),
        "hits": r.stats.hits.load(Ordering::Relaxed),
        "blocked": r.stats.blocked.load(Ordering::Relaxed),
        "cname_blocked": r.stats.cname_blocked.load(Ordering::Relaxed),
        "upstream": r.stats.upstream.load(Ordering::Relaxed),
        "errors": r.stats.errors.load(Ordering::Relaxed),
        "cache": r.cache.lock().len(),
        "cache_enabled": r.cache_enabled,
        "cache_scope": "debug_positive_address_only",
        "state": "proposal_only",
    }))
}

pub async fn start() -> Result<u16, String> {
    start_controlled(PrivacyLevel::Strict).await.map(|(port, _)| port)
}

pub async fn start_controlled(level: PrivacyLevel) -> Result<(u16, ResolverControl), String> {
    let resolver = Resolver::new(level)?;
    let control = ResolverControl(resolver.clone());
    let app = Router::new()
        .route("/dns-query", post(handle_post))
        .route("/dns-query", get(handle_get))
        .route("/stats", get(handle_stats))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_DNS_MESSAGE))
        .with_state(resolver);

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await.map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    Ok((port, control))
}

pub fn flags(port: u16) -> String {
    format!("--enable-features=DnsOverHttps --dns-over-https-mode=secure --dns-over-https-templates=http://localhost:{}/dns-query", port)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_resolver() -> Arc<Resolver> {
        Resolver::new(PrivacyLevel::Strict).unwrap()
    }

    #[test]
    fn is_blocked_exact_match() {
        let r = make_resolver();
        assert!(r.is_blocked("data.adobedc.net"));
        assert!(r.is_blocked("demdex.net"));
    }

    #[test]
    fn is_blocked_subdomain() {
        let r = make_resolver();
        assert!(r.is_blocked("foo.data.adobedc.net"));
        assert!(r.is_blocked("sub.demdex.net"));
    }

    #[test]
    fn is_blocked_trailing_dot() {
        let r = make_resolver();
        assert!(r.is_blocked("data.adobedc.net."));
    }

    #[test]
    fn is_blocked_non_match() {
        let r = make_resolver();
        assert!(!r.is_blocked("example.com"));
        assert!(!r.is_blocked("google.com"));
        assert!(!r.is_blocked("notdemdex.net"));
    }

    #[test]
    fn policy_generation_rejects_aba() {
        let mut p = Policy { level: PrivacyLevel::Strict, generation: 0 };
        let first = p;
        p.change(PrivacyLevel::Maximum).unwrap();
        p.change(PrivacyLevel::Strict).unwrap();
        assert_ne!(p, first);
        assert_eq!(p.generation, 2);
        p.change(PrivacyLevel::Strict).unwrap();
        assert_eq!(p.generation, 2);
        assert!(parse_level("unknown").is_err());
    }

    #[test]
    fn transaction_identity_and_shape() {
        use hickory_proto::op::Query;
        use hickory_proto::rr::{Name, RecordType};
        let mut req = Message::new();
        req.set_id(123);
        req.add_query(Query::query(Name::from_ascii("example.test.").unwrap(), RecordType::A));
        let wire = req.to_bytes().unwrap();
        let parsed = Resolver::checked_request(&wire).unwrap();
        let mut response = Resolver::nxdomain(&parsed);
        assert!(Resolver::checked_response(&parsed, &response).is_ok());
        response[1] ^= 1;
        assert!(Resolver::checked_response(&parsed, &response).is_err());
        req.add_query(Query::query(Name::from_ascii("other.test.").unwrap(), RecordType::AAAA));
        assert!(Resolver::checked_request(&req.to_bytes().unwrap()).is_err());
    }

    #[test]
    fn flags_contains_port() {
        let f = flags(8053);
        assert!(f.contains("8053"));
        assert!(f.contains("DnsOverHttps"));
    }
}
