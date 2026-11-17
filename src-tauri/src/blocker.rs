use adblock::engine::Engine;
use adblock::lists::{FilterSet, ParseOptions};
use adblock::regex_manager::RegexManagerDiscardPolicy;
use adblock::resources::{PermissionMask, Resource};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

// ── bundled filter lists (compiled into binary, fallback if no network) ──────
const EASYLIST: &str = include_str!("../adblock/easylist.txt");
const EASYPRIVACY: &str = include_str!("../adblock/easyprivacy.txt");
const UBLOCK_FILTERS: &str = include_str!("../adblock/ublock-filters.txt");
const UBLOCK_PRIVACY: &str = include_str!("../adblock/ublock-privacy.txt");
const UBLOCK_UNBREAK: &str = include_str!("../adblock/ublock-unbreak.txt");
const UBLOCK_BADWARE: &str = include_str!("../adblock/ublock-badware.txt");
const UBLOCK_QUICK_FIXES: &str = include_str!("../adblock/ublock-quick-fixes.txt");
const PETER_LOWE: &str = include_str!("../adblock/peter-lowe.txt");
const FANBOY_ANNOYANCE: &str = include_str!("../adblock/fanboy-annoyance.txt");
const URLHAUS_FILTER: &str = include_str!("../adblock/urlhaus-filter-online.txt");

// scriptlet + redirect resources (JSON array of Resource objects)
const RESOURCES_JSON: &str = include_str!("../adblock/scriptlet-resources.json");

const ENGINE_FILE: &str = "adblock/engine.dat";
const LISTS_DIR: &str = "adblock/lists";
const METADATA_FILE: &str = "adblock/metadata.json";

// ── filter list definitions ─────────────────────────────────────────────────
struct FilterListDef {
    name: &'static str,
    url: &'static str,
    bundled: &'static str,
    trusted: bool,
}

const FILTER_LISTS: &[FilterListDef] = &[
    FilterListDef {
        name: "easylist",
        url: "https://easylist.to/easylist/easylist.txt",
        bundled: EASYLIST,
        trusted: false,
    },
    FilterListDef {
        name: "easyprivacy",
        url: "https://easylist.to/easylist/easyprivacy.txt",
        bundled: EASYPRIVACY,
        trusted: false,
    },
    FilterListDef {
        name: "ublock-filters",
        url: "https://ublockorigin.github.io/uAssets/filters/filters.txt",
        bundled: UBLOCK_FILTERS,
        trusted: true,
    },
    FilterListDef {
        name: "ublock-privacy",
        url: "https://ublockorigin.github.io/uAssets/filters/privacy.txt",
        bundled: UBLOCK_PRIVACY,
        trusted: true,
    },
    FilterListDef {
        name: "ublock-unbreak",
        url: "https://ublockorigin.github.io/uAssets/filters/unbreak.txt",
        bundled: UBLOCK_UNBREAK,
        trusted: true,
    },
    FilterListDef {
        name: "ublock-badware",
        url: "https://ublockorigin.github.io/uAssets/filters/badware.txt",
        bundled: UBLOCK_BADWARE,
        trusted: true,
    },
    FilterListDef {
        name: "ublock-quick-fixes",
        url: "https://ublockorigin.github.io/uAssets/filters/quick-fixes.txt",
        bundled: UBLOCK_QUICK_FIXES,
        trusted: true,
    },
    FilterListDef {
        name: "peter-lowe",
        url: "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=adblockplus&showintro=1&mimetype=plaintext",
        bundled: PETER_LOWE,
        trusted: false,
    },
    FilterListDef {
        name: "fanboy-annoyance",
        url: "https://easylist.to/easylist/fanboy-annoyance.txt",
        bundled: FANBOY_ANNOYANCE,
        trusted: false,
    },
    FilterListDef {
        name: "urlhaus-filter",
        url: "https://malware-filter.gitlab.io/malware-filter/urlhaus-filter-online.txt",
        bundled: URLHAUS_FILTER,
        trusted: false,
    },
];

// ── metadata ────────────────────────────────────────────────────────────────
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct FilterListMetadata {
    pub last_updated: Option<u64>,
    pub list_versions: HashMap<String, String>,
}

// ── cosmetic result ─────────────────────────────────────────────────────────
pub struct CosmeticResult {
    pub css: String,
    pub script: String,
    pub exceptions: HashSet<String>,
    pub generichide: bool,
}

// ── init engine ─────────────────────────────────────────────────────────────
pub fn init_engine(data_dir: &PathBuf) -> Arc<RwLock<Engine>> {
    let engine_path = data_dir.join(ENGINE_FILE);

    // try cached binary first (~5ms)
    if engine_path.exists() {
        if let Ok(data) = std::fs::read(&engine_path) {
            let mut engine = Engine::default();
            if engine.deserialize(&data).is_ok() {
                // resources are NOT included in serialized data — must reload
                load_resources(&mut engine, Some(data_dir));
                return Arc::new(RwLock::new(engine));
            }
        }
    }

    // cold compile from bundled/downloaded lists (~400-600ms, happens once)
    let engine = compile_engine(data_dir);
    cache_engine(&engine, data_dir);
    Arc::new(RwLock::new(engine))
}

// ── compile engine from best available lists ────────────────────────────────
fn compile_engine(data_dir: &PathBuf) -> Engine {
    let lists_dir = data_dir.join(LISTS_DIR);
    let mut filter_set = FilterSet::new(false);

    for list_def in FILTER_LISTS {
        // prefer downloaded list, fall back to bundled
        let content = read_downloaded_list(&lists_dir, list_def.name)
            .unwrap_or_else(|| list_def.bundled.to_string());
        if !content.is_empty() {
            let opts = if list_def.trusted {
                ParseOptions {
                    permissions: PermissionMask::from_bits(1),
                    ..ParseOptions::default()
                }
            } else {
                ParseOptions::default()
            };
            filter_set.add_filters(
                &content.lines().collect::<Vec<_>>(),
                opts,
            );
        }
    }

    let mut engine = Engine::from_filter_set(filter_set, true);

    // tune regex memory: discard unused regexes after 5 min, check every 2 min
    engine.set_regex_discard_policy(RegexManagerDiscardPolicy {
        cleanup_interval: std::time::Duration::from_secs(120),
        discard_unused_time: std::time::Duration::from_secs(300),
    });

    load_resources(&mut engine, Some(data_dir));
    engine
}

// ── load scriptlet + redirect resources ─────────────────────────────────────
fn load_resources(engine: &mut Engine, data_dir: Option<&PathBuf>) {
    // prefer downloaded resources if available, fall back to bundled
    let resources: Vec<Resource> = data_dir
        .and_then(|d| {
            let path = d.join("adblock/resources.json");
            std::fs::read_to_string(path).ok()
        })
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::from_str(RESOURCES_JSON).unwrap_or_default());
    if !resources.is_empty() {
        engine.use_resources(resources);
    }
}

// ── read a downloaded filter list file ──────────────────────────────────────
fn read_downloaded_list(lists_dir: &PathBuf, name: &str) -> Option<String> {
    let path = lists_dir.join(format!("{}.txt", name));
    std::fs::read_to_string(path).ok()
}

// ── cache the compiled engine to disk ───────────────────────────────────────
fn cache_engine(engine: &Engine, data_dir: &PathBuf) {
    let engine_path = data_dir.join(ENGINE_FILE);
    if let Some(parent) = engine_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let serialized = engine.serialize();
    let _ = std::fs::write(&engine_path, &serialized);
}

// ── download all filter lists, recompile engine, swap atomically ────────────
pub async fn update_filter_lists(
    data_dir: PathBuf,
    engine: Arc<RwLock<Engine>>,
) -> Result<(), String> {
    let lists_dir = data_dir.join(LISTS_DIR);
    let _ = std::fs::create_dir_all(&lists_dir);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    // download all lists in parallel
    let mut handles = Vec::new();
    for list_def in FILTER_LISTS {
        let client = client.clone();
        let lists_dir = lists_dir.clone();
        let name = list_def.name.to_string();
        let url = list_def.url.to_string();

        handles.push(tokio::spawn(async move {
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(text) = resp.text().await {
                        let path = lists_dir.join(format!("{}.txt", name));
                        let _ = tokio::fs::write(path, &text).await;
                    }
                }
                _ => {}
            }
        }));
    }

    // wait for all downloads
    for handle in handles {
        let _ = handle.await;
    }

    // recompile engine on blocking thread (CPU-intensive)
    let data_dir_clone = data_dir.clone();
    let new_engine = tokio::task::spawn_blocking(move || compile_engine(&data_dir_clone))
        .await
        .map_err(|e| e.to_string())?;

    // cache to disk
    cache_engine(&new_engine, &data_dir);

    // atomic swap
    {
        let mut guard = engine.write();
        *guard = new_engine;
    }

    // update metadata
    let metadata = FilterListMetadata {
        last_updated: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        ),
        list_versions: HashMap::new(),
    };
    let meta_path = data_dir.join(METADATA_FILE);
    let _ = std::fs::write(
        &meta_path,
        serde_json::to_string(&metadata).unwrap_or_default(),
    );

    Ok(())
}

// ── load metadata to check last update time ─────────────────────────────────
pub fn load_metadata(data_dir: &PathBuf) -> FilterListMetadata {
    let meta_path = data_dir.join(METADATA_FILE);
    std::fs::read_to_string(&meta_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

// Native WebKitGTK bootstrap from Bushido's existing bundled/downloaded
// Peter Lowe list. This is deliberately an OPT-IN, debug-only subset:
// the full Rust ABP engine also has cross-list exceptions, resource-scoped
// conditions and rewrites not represented by these unconditional rules.
// The purpose is exercising real source -> WebKit JSON -> pre-load filter.
#[cfg(all(target_os = "linux", debug_assertions))]
#[derive(Debug, Default, serde::Serialize)]
pub struct NativeSubsetReceipt {
    pub source: String,
    pub total_lines: usize,
    pub emitted: usize,
    pub unsupported: usize,
    pub duplicates: usize,
}

#[cfg(all(target_os = "linux", debug_assertions))]
fn native_host_rule(raw: &str) -> Option<String> {
    let text = raw.trim();
    let host = text.strip_prefix("||")?.strip_suffix('^')?;
    if host.len() > 253 || !host.is_ascii() || !host.contains('.') {
        return None;
    }
    let host = host.to_ascii_lowercase();
    if host.ends_with(".local") || host.ends_with(".localhost")
        || host.ends_with(".test") || host.ends_with(".invalid")
        || host.ends_with(".example") {
        return None;
    }
    if !host.split('.').all(|label| {
        !label.is_empty() && label.len() <= 63
            && !label.starts_with('-') && !label.ends_with('-')
            && label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }) {
        return None;
    }
    Some(host)
}

#[cfg(all(target_os = "linux", debug_assertions))]
pub fn native_subset_from_peter_lowe(text: &str) -> (Vec<serde_json::Value>, NativeSubsetReceipt) {
    let mut names = std::collections::BTreeSet::new();
    let mut receipt = NativeSubsetReceipt {
        source: "peter-lowe".to_owned(),
        ..Default::default()
    };
    for line in text.lines() {
        receipt.total_lines += 1;
        if let Some(host) = native_host_rule(line) {
            if !names.insert(host) { receipt.duplicates += 1; }
        } else {
            receipt.unsupported += 1;
        }
    }
    // WebKit regexes only need to escape the fixed dot separators because
    // the parser excludes every other regex metacharacter.
    let json = names.into_iter().map(|host| {
        let escaped = host.replace('.', "[.]");
        serde_json::json!({
            "trigger": {"url-filter":
                format!("^https?://([a-z0-9-]+[.])*{}([:/?#]|$)", escaped)},
            "action": {"type": "block"}
        })
    }).collect::<Vec<_>>();
    receipt.emitted = json.len();
    (json, receipt)
}

#[cfg(all(target_os = "linux", debug_assertions))]
pub fn write_native_webkit_subset(data_dir: &PathBuf) -> Result<PathBuf, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let text = read_downloaded_list(&data_dir.join(LISTS_DIR), "peter-lowe")
        .unwrap_or_else(|| PETER_LOWE.to_owned());
    let (rules, receipt) = native_subset_from_peter_lowe(&text);
    if rules.is_empty() { return Err("No eligible native WebKit host rules".into()); }
    let output_dir = data_dir.join("native-webkit");
    std::fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Cannot create native WebKit filter directory: {e}"))?;
    let serialized = serde_json::to_vec(&rules).map_err(|e| e.to_string())?;
    let mut entropy = [0u8; 8];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut entropy);
    let temp = output_dir.join(format!("rules-{:016x}.tmp", u64::from_ne_bytes(entropy)));
    let write_result = (|| -> Result<(), String> {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true)
            .mode(0o600).open(&temp).map_err(|e| e.to_string())?;
        file.write_all(&serialized).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temp, output_dir.join("rules.json"))
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if write_result.is_err() { let _ = std::fs::remove_file(&temp); }
    write_result?;
    let manifest = serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?;
    std::fs::write(output_dir.join("receipt.json"), manifest).map_err(|e| e.to_string())?;
    Ok(output_dir.join("rules.json"))
}

// ── get cosmetic resources for a URL ────────────────────────────────────────
pub fn get_cosmetic_resources(engine: &Engine, url: &str) -> CosmeticResult {
    let resources = engine.url_cosmetic_resources(url);

    let css = if !resources.hide_selectors.is_empty() && !resources.generichide {
        let selectors: Vec<&str> = resources.hide_selectors.iter().map(|s| s.as_str()).collect();
        format!("{}{{display:none!important}}", selectors.join(","))
    } else {
        String::new()
    };

    CosmeticResult {
        css,
        script: resources.injected_script,
        exceptions: resources.exceptions,
        generichide: resources.generichide,
    }
}

// ── dynamic cosmetic filtering: probe DOM classes/ids ───────────────────────
pub fn get_hidden_selectors(
    engine: &Engine,
    classes: &[String],
    ids: &[String],
    exceptions: &HashSet<String>,
) -> Vec<String> {
    engine.hidden_class_id_selectors(classes, ids, exceptions)
}

// Pure network decision shared by the document-navigation hook and the
// Windows WebView2 request handler. This does NOT intercept Linux subresources;
// a WebKitGTK network adapter is still required to enforce this decision.
pub struct RequestPolicyDecision {
    pub blocked: bool,
    pub filter_rule: Option<String>,
    pub rewrite_url: Option<String>,
    pub redirect_url: Option<String>,
}

pub fn evaluate_request(
    engine: &Engine,
    url: &str,
    source_url: &str,
    resource_type: &str,
) -> Option<RequestPolicyDecision> {
    let req = adblock::request::Request::new(url, source_url, resource_type).ok()?;
    let response = engine.check_network_request(&req);
    // WebView2 originally stripped tracking params and then applied
    // adblock's $removeparam rewrite. Preserve that precedence for allowed
    // requests, but do not rewrite a blocked request to a clean network URL.
    let rewrite_url = if response.matched {
        None
    } else {
        response.rewritten_url.or_else(|| strip_tracking_params(url))
    };
    Some(RequestPolicyDecision {
        blocked: response.matched,
        filter_rule: response.filter,
        rewrite_url,
        redirect_url: if response.matched {
            Some(response.redirect.unwrap_or_else(|| "about:blank".to_owned()))
        } else {
            None
        },
    })
}

// ── map webview2 resource context enum to adblock resource type string ──────
pub fn resource_type_str(ctx: u32) -> &'static str {
    match ctx {
        2 => "document",
        3 => "stylesheet",
        4 => "image",
        5 => "media",
        6 => "font",
        7 => "script",
        8 => "xmlhttprequest",
        9 => "fetch",
        10 => "ping",
        13 => "websocket",
        _ => "other",
    }
}

// ── tracking parameter stripping ────────────────────────────────────────────
// FIX #4: Expanded tracking param stripping (research/18 B.2)
// Brave strips 60+ params. Our old list had 31 — now 62.
const TRACKING_PARAMS: &[&str] = &[
    // Google Analytics / Ads
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "utm_name",
    "utm_cid",
    "utm_reader",
    "utm_viz_id",
    "utm_pubreferrer",
    "gclid",
    "gclsrc",
    "dclid",
    "gbraid",
    "wbraid",
    "_ga",
    "_gl",
    "_gac",
    // Facebook / Meta
    "fbclid",
    "fb_action_ids",
    "fb_action_types",
    "fb_source",
    "fb_ref",
    // Microsoft
    "msclkid",
    // Twitter / X
    "twclid",
    // TikTok
    "ttclid",
    // Mailchimp
    "mc_cid",
    "mc_eid",
    // Yandex
    "yclid",
    "ymclid",
    "_ym_uid",
    "_ym_d",
    // HubSpot
    "_hsenc",
    "_hsmi",
    "__hsfp",
    "__hssc",
    "__hstc",
    "hsa_cam",
    "hsa_grp",
    "hsa_mt",
    "hsa_src",
    "hsa_ad",
    "hsa_acc",
    "hsa_net",
    "hsa_ver",
    // Marketo
    "mkt_tok",
    // Adobe
    "s_cid",
    "s_kwcid",
    // Vero
    "vero_id",
    // Instagram
    "igshid",
    // Outbrain / Taboola
    "obOrigUrl",
    "ob_click_id",
    // Omnisend
    "oly_enc_id",
    "oly_anon_id",
    // Drip
    "__s",
    // Wicked Reports
    "wickedid",
    // SourcePoint
    "spm",
    // Reddit
    "rb_clickid",
    // Snapchat
    "ScCid",
    // Pinterest
    "epik",
    // LinkedIn
    "li_fat_id",
    // OpenStat
    "_openstat",
    // Pardot
    "pi_campaign_id",
    "piCId",
];

/// Strip tracking parameters from a URL, return new URL if modified
pub fn strip_tracking_params(url_str: &str) -> Option<String> {
    let mut parsed = url::Url::parse(url_str).ok()?;
    if parsed.query().is_none() {
        return None;
    }

    let original_count = parsed.query_pairs().count();
    let filtered: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| !TRACKING_PARAMS.contains(&k.as_ref()))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    if filtered.len() == original_count {
        return None; // nothing changed
    }

    if filtered.is_empty() {
        parsed.set_query(None);
    } else {
        let new_query: String = filtered
            .iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    urlencoding::encode(k).to_string()
                } else {
                    format!("{}={}", urlencoding::encode(k), urlencoding::encode(v))
                }
            })
            .collect::<Vec<_>>()
            .join("&");
        parsed.set_query(Some(&new_query));
    }

    Some(parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(target_os = "linux", debug_assertions))]
    #[test]
    fn native_source_producer_emits_only_literal_host_blocks() {
        let (rules, receipt) = native_subset_from_peter_lowe(
            "||ads.example.net^\n||tracker.other.org^\n||ADS.EXAMPLE.NET^\n@@||ads.example.net^\n||ads.example.net^$script\n||a..net^\n"
        );
        assert_eq!(receipt.emitted, 2);
        assert_eq!(receipt.duplicates, 1);
        assert_eq!(receipt.unsupported, 3);
        assert_eq!(rules[0]["action"]["type"], "block");
        let first = rules[0]["trigger"]["url-filter"].as_str().unwrap();
        assert!(first.starts_with("^https?://"));
        assert!(first.contains("ads[.]example[.]net"));
    }

    #[cfg(all(target_os = "linux", debug_assertions))]
    #[test]
    fn native_source_producer_rejects_regex_and_traversal_syntax() {
        for host in ["||a..net^", "||-foo.example.org^", "||foo-.example.org^",
                     "||foo[.]example.org^", "||../../tmp^", "||localhost^"] {
            assert!(native_host_rule(host).is_none(), "{host}");
        }
        assert_eq!(native_host_rule("||CDN.Example.ORG^").as_deref(), Some("cdn.example.org"));
    }

    #[test]
    fn shared_policy_blocks_matching_subresources() {
        let mut filters = FilterSet::new(false);
        filters.add_filters(&["||tracker.example^"], ParseOptions::default());
        let engine = Engine::from_filter_set(filters, true);

        let decision = evaluate_request(
            &engine, "https://tracker.example/pixel.png",
            "https://publisher.example/article", "image"
        ).expect("valid resource");
        assert!(decision.blocked);
        assert_eq!(decision.redirect_url.as_deref(), Some("about:blank"));
        assert!(decision.rewrite_url.is_none());

        let allowed = evaluate_request(
            &engine, "https://publisher.example/logo.png",
            "https://publisher.example/article", "image"
        ).expect("valid resource");
        assert!(!allowed.blocked);
        assert!(allowed.redirect_url.is_none());
    }

    #[test]
    fn shared_policy_strips_tracking_params_only_on_allowed_requests() {
        let engine = Engine::default();
        let clean = evaluate_request(
            &engine, "https://cdn.example/a.js?utm_source=x&keep=ok",
            "https://publisher.example", "script"
        ).expect("valid resource");
        assert!(!clean.blocked);
        assert_eq!(clean.rewrite_url.as_deref(), Some("https://cdn.example/a.js?keep=ok"));
        assert!(clean.redirect_url.is_none());
    }

    #[test]
    fn shared_policy_invalid_request_cannot_be_mistaken_for_blocked() {
        let engine = Engine::default();
        assert!(evaluate_request(&engine, "not a URL", "https://publisher.example", "script").is_none());
    }

    #[test]
    fn resource_type_known_codes() {
        assert_eq!(resource_type_str(2), "document");
        assert_eq!(resource_type_str(3), "stylesheet");
        assert_eq!(resource_type_str(4), "image");
        assert_eq!(resource_type_str(5), "media");
        assert_eq!(resource_type_str(6), "font");
        assert_eq!(resource_type_str(7), "script");
        assert_eq!(resource_type_str(8), "xmlhttprequest");
        assert_eq!(resource_type_str(9), "fetch");
        assert_eq!(resource_type_str(10), "ping");
        assert_eq!(resource_type_str(13), "websocket");
    }

    #[test]
    fn resource_type_unknown_returns_other() {
        assert_eq!(resource_type_str(0), "other");
        assert_eq!(resource_type_str(1), "other");
        assert_eq!(resource_type_str(99), "other");
    }

    #[test]
    fn strip_tracking_removes_utm() {
        let result = strip_tracking_params("https://example.com/page?utm_source=twitter&ref=home");
        assert_eq!(result, Some("https://example.com/page?ref=home".to_string()));
    }

    #[test]
    fn strip_tracking_removes_fbclid() {
        let result = strip_tracking_params("https://example.com/?fbclid=abc123");
        assert_eq!(result, Some("https://example.com/".to_string()));
    }

    #[test]
    fn strip_tracking_removes_multiple() {
        let result = strip_tracking_params("https://example.com/p?utm_source=x&gclid=y&utm_medium=z&keep=1");
        assert_eq!(result, Some("https://example.com/p?keep=1".to_string()));
    }

    #[test]
    fn strip_tracking_no_change_clean_url() {
        assert_eq!(strip_tracking_params("https://example.com/page?q=rust&lang=en"), None);
    }

    #[test]
    fn strip_tracking_no_query() {
        assert_eq!(strip_tracking_params("https://example.com/page"), None);
    }

    #[test]
    fn strip_tracking_all_removed() {
        let result = strip_tracking_params("https://example.com/?utm_source=x&utm_medium=y");
        assert_eq!(result, Some("https://example.com/".to_string()));
    }
}
