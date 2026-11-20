use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Weak};
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
    /// Original resolver candidate producer. Only an internal, trusted caller
    /// may supply the DNS query. No IPC/HTTP route exposes this method. The
    /// caller still needs explicit destination admission and egress confinement.
    #[allow(dead_code)]
    pub(crate) async fn direct_address_candidate(&self, raw: &[u8]) -> Result<AddressCandidate, String> {
        let r = &self.0;
        let _permit = r.admission.try_acquire().map_err(|_| "DNS resolver busy")?;
        let req = Resolver::checked_request(raw)?;
        let snapshot = *r.policy.read();
        if dns_positive_cache::key(raw, snapshot.generation).is_none() {
            return Err("Unsupported direct-address query".into());
        }
        let hostname = req.queries()[0].name().to_ascii();
        if r.is_blocked(&hostname) { return Err("Blocked destination".into()); }
        // No cache or fallback in this narrower candidate lane.
        let reply = r.doh(PRIMARY, raw).await?;
        Resolver::checked_response(&req, &reply.bytes)?;
        let now = Instant::now();
        let (addresses, expires) = dns_positive_cache::address_candidate(raw,&reply.bytes,reply.started,now)
            .ok_or("Unsupported direct-address response")?;
        r.check_policy(snapshot)?;
        Ok(AddressCandidate { owner: Arc::downgrade(r), hostname, policy: snapshot, addresses, expires })
    }

    pub fn set_level(&self, value: &str) -> Result<(), String> {
        let mut policy = self.0.policy.write();
        let previous = *policy;
        policy.change(parse_level(value)?)?;
        if *policy != previous {
            self.0.cache.lock().clear();
            self.0.policy_events.send_replace(policy.generation);
        }
        Ok(())
    }
}

/// Private candidate provenance. Not a socket capability and not serialized.
#[allow(dead_code)]
pub(crate) struct AddressCandidate {
    owner: Weak<Resolver>,
    hostname: String,
    policy: Policy,
    addresses: Vec<IpAddr>,
    expires: Instant,
}

impl AddressCandidate {
    /// Revalidate same configured host, exact independently approved IP and
    /// policy generation. This only constructs a socket address, never dials.
    #[allow(dead_code)]
    pub(crate) fn scoped_destination(&self, control: &ResolverControl,
        host: &str, approved: IpAddr, now: Instant) -> Result<SocketAddr, String> {
        if !self.owner.upgrade().is_some_and(|owner| Arc::ptr_eq(&owner, &control.0)) {
            return Err("Address candidate belongs to another resolver instance".into());
        }
        if host != self.hostname || !self.addresses.contains(&approved) || now >= self.expires {
            return Err("Address candidate outside scope or expired".into());
        }
        control.0.check_policy(self.policy)?;
        Ok(SocketAddr::new(approved, 443))
    }
}

// Child module has access to the original resolver/candidate fields. No IPC
// or HTTP handler can invoke its transport; standalone acceptance only.
#[cfg(any(test, all(target_os = "linux", debug_assertions)))]
#[path = "dns_scoped_transport.rs"]
mod scoped_transport;

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
    admission: tokio::sync::Semaphore,
    blocklist: RwLock<HashSet<String>>,
    client: reqwest::Client,
    stats: Arc<Stats>,
    policy: RwLock<Policy>,
    policy_events: tokio::sync::watch::Sender<u64>,
    transport_slots: tokio::sync::Semaphore,
}

impl Resolver {
    fn new(level: PrivacyLevel) -> Result<Arc<Self>, String> {
        let mut bl = HashSet::new();
        for t in CNAME_TRACKERS { bl.insert(t.to_string()); }

        Ok(Arc::new(Self {
            cache: Mutex::new(PositiveCache::default()),
            admission: tokio::sync::Semaphore::new(32),
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
            policy_events: tokio::sync::watch::channel(0).0,
            transport_slots: tokio::sync::Semaphore::new(8),
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
        let age_values = r.headers().get_all(reqwest::header::AGE);
        let mut ages = age_values.iter();
        if let Some(age) = ages.next() {
            if age.to_str().ok().and_then(|v| v.parse::<u64>().ok()) != Some(0) || ages.next().is_some() {
                return Err("Aged or ambiguous upstream DNS response".into());
            }
        }
        let control_values = r.headers().get_all(reqwest::header::CACHE_CONTROL);
        let mut controls = control_values.iter();
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
        // Bound active resolver work; no unbounded pending queue.
        let _permit = self.admission.try_acquire().map_err(|_| "DNS resolver busy")?;
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

// Linux fixture is not a privileged browser endpoint. These checks reject
// ordinary browser-originated traffic and rebinding Host values, not arbitrary
// same-host native processes. Never interpret them as client authentication.
#[cfg(target_os = "linux")]
fn fixture_http_admitted(headers: &axum::http::HeaderMap, uri: &axum::http::Uri, port: u16) -> bool {
    let expected = format!("127.0.0.1:{port}");
    let local = format!("localhost:{port}");
    let allowed = |h: &str| h == expected || h == local;
    let hosts = headers.get_all(header::HOST);
    let mut values = hosts.iter();
    let Some(host) = values.next().and_then(|h| h.to_str().ok()) else { return false; };
    if !allowed(host) || values.next().is_some() { return false; }
    if uri.authority().is_some_and(|a| !allowed(a.as_str())) { return false; }
    !headers.contains_key(header::ORIGIN) && !headers.contains_key(header::REFERER)
        && !headers.keys().any(|name| name.as_str().starts_with("sec-fetch-"))
}

pub async fn start() -> Result<u16, String> {
    start_controlled(PrivacyLevel::Strict).await.map(|(port, _)| port)
}

pub async fn start_controlled(level: PrivacyLevel) -> Result<(u16, ResolverControl), String> {
    let resolver = Resolver::new(level)?;
    let control = ResolverControl(resolver.clone());
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await.map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let app = Router::new()
        .route("/dns-query", post(handle_post))
        .route("/dns-query", get(handle_get))
        .route("/stats", get(handle_stats))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_DNS_MESSAGE))
        .with_state(resolver);

    #[cfg(target_os = "linux")]
    let app = app.layer(axum::middleware::from_fn(
        move |req: axum::extract::Request, next: axum::middleware::Next| async move {
            use axum::response::IntoResponse;
            let mut response = if fixture_http_admitted(req.headers(), req.uri(), port) {
                next.run(req).await
            } else { StatusCode::FORBIDDEN.into_response() };
            response.headers_mut().insert(header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"));
            response
        },
    ));

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

    #[cfg(target_os = "linux")]
    #[test]
    fn fixture_http_rebinding_and_browser_metadata_rejected() {
        use axum::http::{HeaderMap, HeaderValue, Uri};
        let uri: Uri = "/dns-query".parse().unwrap();
        let mut h = HeaderMap::new();
        assert!(!fixture_http_admitted(&h,&uri,1234));
        h.insert(header::HOST,HeaderValue::from_static("127.0.0.1:1234"));
        assert!(fixture_http_admitted(&h,&uri,1234));
        assert!(!fixture_http_admitted(&h,&uri,1235));
        h.insert(header::ORIGIN,HeaderValue::from_static("null"));
        assert!(!fixture_http_admitted(&h,&uri,1234));
        h.remove(header::ORIGIN);
        h.insert("sec-fetch-site",HeaderValue::from_static("cross-site"));
        assert!(!fixture_http_admitted(&h,&uri,1234));
        h.remove("sec-fetch-site");
        h.append(header::HOST,HeaderValue::from_static("localhost:1234"));
        assert!(!fixture_http_admitted(&h,&uri,1234));
        h.insert(header::HOST,HeaderValue::from_static("attacker.test:1234"));
        assert!(!fixture_http_admitted(&h,&uri,1234));
    }

    #[test]
    fn scoped_address_candidate_rejects_other_epoch_and_destination() {
        let r = make_resolver(); let control = ResolverControl(r.clone());
        let ip: IpAddr = "192.0.2.1".parse().unwrap();
        let t = Instant::now();
        let candidate = AddressCandidate { owner: Arc::downgrade(&r), hostname: "a.test.".into(),
            policy: *r.policy.read(), addresses: vec![ip], expires: t + Duration::from_secs(3) };
        assert_eq!(candidate.scoped_destination(&control,"a.test.",ip,t).unwrap(), SocketAddr::new(ip,443));
        assert!(candidate.scoped_destination(&control,"b.test.",ip,t).is_err());
        let other = ResolverControl(make_resolver());
        assert!(candidate.scoped_destination(&other,"a.test.",ip,t).is_err());
        assert!(candidate.scoped_destination(&control,"a.test.","127.0.0.1".parse().unwrap(),t).is_err());
        assert!(candidate.scoped_destination(&control,"a.test.",ip,t+Duration::from_secs(3)).is_err());
        control.set_level("maximum").unwrap(); control.set_level("strict").unwrap();
        assert!(candidate.scoped_destination(&control,"a.test.",ip,t).is_err());
    }

    #[test]
    fn flags_contains_port() {
        let f = flags(8053);
        assert!(f.contains("8053"));
        assert!(f.contains("DnsOverHttps"));
    }
}
