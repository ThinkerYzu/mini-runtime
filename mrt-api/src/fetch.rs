// fetch, XMLHttpRequest and script sources (Phase 6.3, DESIGN "Phase 6"): the requests the runtime's
// fetch(), XHR, module scripts and link sheets with crossorigin forward (bridge.js `forward`), and
// the classic scripts and sheets it fetches without CORS (`script: true`), made here instead of in
// the bridge tab. What Firefox enforced for the page, this does: the Fetch Standard's main fetch as
// Gecko runs it (dom/fetch FetchDriver, netwerk's nsCORSListenerProxy and nsPreflightCache,
// HttpBaseChannel's Origin header and redirect checks, ReferrerInfo, SecFetch, the CORP check),
// with the cookies of the request's partition (cookie.rs) sent and set as Firefox's parent does.
//
// A request: the runtime's {url, method, headers, body | bodyBase64, credentials, mode, redirect,
// referrerPolicy, noBody, bytes, script} with bridge.js's `origin` (the global's) and `referrer`
// (its document's URL), and the driver's site and top. The reply is the bridge tab's: {status,
// statusText, url, redirected, type, headers, body, bodyBase64, took}, {pending: true} for a request
// not answered in PENDING (a long poll, as the tab's), or an error that the runtime throws as
// Firefox's TypeError.
//
// The network is reached through API_PROXY (the replay's mitmdump, its CA in API_CA trusted), or
// directly with Mozilla's roots. `took` is the network time (SPEC decision 20): the replay's
// recorded duration (its x-mrt-recorded-ms header) where it gave one, else the time measured here.
// The requesting document's policies (policy.rs, `doc` and `metaReferrer`) apply as in Firefox: its
// referrer policy, CSP's connect-src and upgrade-insecure-requests, mixed content, COEP. Not done:
// Firefox's HTTP cache (the cache modes only send their headers; only-if-cached always fails),
// HTTP/2 (the client speaks HTTP/1.1), CSP's other directives (GAPS B162, B21).

use std::io::Read;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use url::Url;

use crate::cookie;
use crate::policy::{self, Doc, trustworthy};
use crate::store::{Store, Stores, site_of};
use std::sync::{Arc, Mutex};

// Shared only around cookie access, never around a network operation. The main
// API thread and live fetch workers must see the same seeded cookie jar.
pub trait CookieStores {
    fn with_store<T>(&mut self, site: &str, top: &str, ancestor: bool, f: impl FnOnce(&Store) -> T) -> Option<T>;
}
impl CookieStores for Stores {
    fn with_store<T>(&mut self, site: &str, top: &str, ancestor: bool, f: impl FnOnce(&Store) -> T) -> Option<T> {
        self.get(site, top, ancestor).ok().map(f)
    }
}
impl CookieStores for Arc<Mutex<Stores>> {
    fn with_store<T>(&mut self, site: &str, top: &str, ancestor: bool, f: impl FnOnce(&Store) -> T) -> Option<T> {
        self.lock().ok()?.with_store(site, top, ancestor, f)
    }
}

/// mini_realsites.py PENDING_MS: a request not answered by then stays in flight in the runtime
const PENDING: Duration = Duration::from_millis(5000);
/// An HTTP viewer navigation's document request (mini_realsites.py `navigate`)
const NAVIGATION_LIMIT: Duration = Duration::from_secs(30);
/// Fetch's redirect limit (Gecko's network.http.redirection-limit)
const MAX_REDIRECTS: usize = 20;
const NETWORK_ERROR: &str = "TypeError: NetworkError when attempting to fetch resource.";
/// the replay proxy's recorded duration (replay_addon.py)
const RECORDED_MS: &str = "x-mrt-recorded-ms";
/// the largest media answer read (B172): a range answer is far smaller; a server that ignores Range
/// sends the whole resource, which the viewer service keeps to serve its ranges
const MEDIA_LIMIT: u64 = 32 * 1024 * 1024;
/// Gecko's network.http.accept header for a <video>'s and an <audio>'s loads (nsHttpHandler's media
/// Accept, HTMLMediaElement::ChannelLoader)
const MEDIA_ACCEPT_VIDEO: &[u8] = b"video/webm,video/ogg,video/*;q=0.9,application/ogg;q=0.7,audio/*;q=0.6,*/*;q=0.5";
const MEDIA_ACCEPT_AUDIO: &[u8] = b"audio/webm,audio/ogg,audio/wav,audio/*;q=0.9,application/ogg;q=0.7,video/*;q=0.6,*/*;q=0.5";
/// nsPreflightCache: an answer without Access-Control-Max-Age is kept 5 s
const PREFLIGHT_DEFAULT_S: u64 = 5;

pub struct Net {
    agent: ureq::Agent,
    /// keeps no idle connection: a request retried on a new one (`send`)
    fresh: ureq::Agent,
    /// what Firefox sends: navigator.userAgent, and intl.accept_languages as a header
    user_agent: String,
    accept_language: String,
    preflights: Vec<Preflight>,
}

/// One cached preflight answer (nsPreflightCache's entry for an origin, URL and credentials mode)
struct Preflight {
    origin: String,
    url: String,
    credentials: bool,
    methods: Vec<String>,
    headers: Vec<String>,
    expires: Instant,
}

impl Net {
    pub fn from_env() -> Net {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let mut tls = ureq::tls::TlsConfig::builder();
        if let Some(ca) = var("API_CA") {
            let pem = std::fs::read(&ca).unwrap_or_default();
            let certs: Vec<_> = ureq::tls::parse_pem(&pem)
                .filter_map(|item| match item {
                    Ok(ureq::tls::PemItem::Certificate(c)) => Some(c),
                    _ => None,
                })
                .collect();
            tls = tls.root_certs(ureq::tls::RootCerts::Specific(std::sync::Arc::new(certs)));
        }
        let proxy = var("API_PROXY").and_then(|p| ureq::Proxy::new(&p).ok());
        let tls = tls.build();
        let config = |fresh: bool| {
            let b = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .max_redirects(0)
                .max_redirects_will_error(false)
                .save_redirect_history(false)
                .allow_non_standard_methods(true)
                .user_agent("")
                .accept("")
                .accept_encoding("")
                .proxy(proxy.clone())
                .tls_config(tls.clone());
            if fresh { b.max_idle_connections(0).max_idle_connections_per_host(0) } else { b }.build()
        };
        Net {
            agent: config(false).new_agent(),
            fresh: config(true).new_agent(),
            user_agent: var("API_UA").unwrap_or_else(|| crate::env::DEFAULT_UA.into()),
            accept_language: var("API_ACCEPT_LANGUAGE").unwrap_or_else(|| "en-US,en;q=0.9".into()),
            preflights: Vec::new(),
        }
    }

    /// Another handle on the same connection pools, for a live job's worker thread (B222): its
    /// requests reuse the connections the other workers left idle, as Gecko's loads share one
    /// connection manager, instead of each job opening a new TCP and TLS connection. The preflight
    /// cache starts empty.
    pub fn share(&self) -> Net {
        Net {
            agent: self.agent.clone(),
            fresh: self.fresh.clone(),
            user_agent: self.user_agent.clone(),
            accept_language: self.accept_language.clone(),
            preflights: Vec::new(),
        }
    }

    /// Drops the agent's idle connections (their sockets and TLS buffers), as memory pressure would:
    /// a new agent from the same environment; the preflight cache is kept.
    pub fn drop_idle(&mut self) {
        let preflights = std::mem::take(&mut self.preflights);
        *self = Net::from_env();
        self.preflights = preflights;
    }
}

// ---- headers ----

/// A ByteString of the page's: each char a byte (Headers takes Latin-1)
fn bytes_of(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// Fetch's forbidden request-header names (nsContentUtils::IsForbiddenRequestHeader): the page
/// cannot set them, Firefox drops them from its Headers
fn forbidden_request_header(name: &str, value: &str) -> bool {
    const NAMES: &[&str] = &[
        "accept-charset", "accept-encoding", "access-control-request-headers", "access-control-request-method",
        "connection", "content-length", "cookie", "cookie2", "date", "dnt", "expect", "host", "keep-alive",
        "origin", "referer", "set-cookie", "te", "trailer", "transfer-encoding", "upgrade", "via",
    ];
    if NAMES.contains(&name) || name.starts_with("proxy-") || name.starts_with("sec-") {
        return true;
    }
    // a method override naming a forbidden method
    if matches!(name, "x-http-method" | "x-http-method-override" | "x-method-override") {
        return value.split(',').any(|m| matches!(m.trim().to_ascii_uppercase().as_str(), "CONNECT" | "TRACE" | "TRACK"));
    }
    false
}

/// A CORS-unsafe request-header byte
fn cors_unsafe_byte(b: u8) -> bool {
    (b < 0x20 && b != 0x09) || b"\"():<>?@[\\]{}".contains(&b) || b == 0x7f
}

/// The essence of a MIME type: "type/subtype", lowercased
fn mime_essence(v: &str) -> String {
    v.split(';').next().unwrap_or("").trim().to_ascii_lowercase()
}

/// nsContentUtils::IsCORSSafelistedRequestHeader
fn cors_safelisted(name: &str, value: &[u8]) -> bool {
    if value.len() > 128 {
        return false;
    }
    match name {
        "accept" => !value.iter().any(|&b| cors_unsafe_byte(b)),
        "accept-language" | "content-language" => {
            value.iter().all(|&b| b.is_ascii_alphanumeric() || b" *,-.;=".contains(&b))
        }
        "content-type" => {
            !value.iter().any(|&b| cors_unsafe_byte(b))
                && matches!(
                    mime_essence(&latin1(value)).as_str(),
                    "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain"
                )
        }
        "range" => {
            // a simple range header value: bytes=N- or bytes=N-M
            let v = latin1(value);
            let Some(r) = v.strip_prefix("bytes=") else { return false };
            let (a, b) = r.split_once('-').unwrap_or(("", "x"));
            !a.is_empty()
                && a.bytes().all(|c| c.is_ascii_digit())
                && b.bytes().all(|c| c.is_ascii_digit())
                && (b.is_empty() || a.parse::<u64>().ok() <= b.parse::<u64>().ok())
        }
        _ => false,
    }
}

/// Fetch's CORS-safelisted response-header names
const SAFELISTED_RESPONSE: &[&str] =
    &["cache-control", "content-language", "content-length", "content-type", "expires", "last-modified", "pragma"];

/// The response's headers as Firefox's Headers iterates them: names lowercased and sorted, the values
/// of one name combined with ", ", Set-Cookie left out (the response guard), and for a CORS response
/// only the safelisted names and those Access-Control-Expose-Headers lists ("*" for all but a
/// credentialed request).
fn listed_headers(raw: &[(String, String)], cors: bool, credentialed: bool) -> Vec<Value> {
    let exposed: Vec<String> = raw
        .iter()
        .filter(|(k, _)| k == "access-control-expose-headers")
        .flat_map(|(_, v)| v.split(',').map(|s| s.trim().to_ascii_lowercase()).collect::<Vec<_>>())
        .filter(|s| !s.is_empty())
        .collect();
    let all = exposed.iter().any(|s| s == "*") && !credentialed;
    let mut names: Vec<&str> = raw.iter().map(|(k, _)| k.as_str()).collect();
    names.sort();
    names.dedup();
    let mut out = Vec::new();
    for name in names {
        if name == "set-cookie" || name == "set-cookie2" || name == RECORDED_MS {
            continue;
        }
        if cors && !all && !SAFELISTED_RESPONSE.contains(&name) && !exposed.iter().any(|e| e == name) {
            continue;
        }
        let values: Vec<&str> = raw.iter().filter(|(k, _)| k == name).map(|(_, v)| v.as_str()).collect();
        out.push(json!([name, values.join(", ")]));
    }
    out
}

/// A single byte range as a media request's Range header has it: bytes=N- or bytes=N-M
fn byte_range(r: &str) -> bool {
    let Some(spec) = r.strip_prefix("bytes=") else { return false };
    let Some((a, b)) = spec.split_once('-') else { return false };
    let digits = |s: &str| !s.is_empty() && s.len() <= 19 && s.bytes().all(|c| c.is_ascii_digit());
    digits(a) && (b.is_empty() || (digits(b) && b.parse::<u64>().ok() >= a.parse::<u64>().ok()))
}

fn header<'a>(raw: &'a [(String, String)], name: &str) -> Option<&'a str> {
    raw.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// nsURLHelper's net_GetDefaultStatusTextForCode: the reason phrase Firefox gives a response without
/// one (all of HTTP/2's). The client here does not hand on the server's, so every response has it.
fn status_text(code: u16) -> &'static str {
    match code {
        100 => "Continue", 101 => "Switching Protocols",
        200 => "OK", 201 => "Created", 202 => "Accepted", 203 => "Non Authoritative", 204 => "No Content",
        205 => "Reset Content", 206 => "Partial Content", 207 => "Multi-Status", 208 => "Already Reported",
        300 => "Multiple Choices", 301 => "Moved Permanently", 302 => "Found", 303 => "See Other",
        304 => "Not Modified", 305 => "Use Proxy", 307 => "Temporary Redirect", 308 => "Permanent Redirect",
        400 => "Bad Request", 401 => "Unauthorized", 402 => "Payment Required", 403 => "Forbidden",
        404 => "Not Found", 405 => "Method Not Allowed", 406 => "Not Acceptable",
        407 => "Proxy Authentication Required", 408 => "Request Timeout", 409 => "Conflict", 410 => "Gone",
        411 => "Length Required", 412 => "Precondition Failed", 413 => "Request Entity Too Large",
        414 => "Request URI Too Long", 415 => "Unsupported Media Type", 416 => "Requested Range Not Satisfiable",
        417 => "Expectation Failed", 418 => "I'm a teapot", 421 => "Misdirected Request",
        422 => "Unprocessable Entity", 423 => "Locked", 424 => "Failed Dependency", 425 => "Too Early",
        426 => "Upgrade Required", 428 => "Precondition Required", 429 => "Too Many Requests",
        431 => "Request Header Fields Too Large", 451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error", 501 => "Not Implemented", 502 => "Bad Gateway",
        503 => "Service Unavailable", 504 => "Gateway Timeout", 505 => "HTTP Version Unsupported",
        506 => "Variant Also Negotiates", 507 => "Insufficient Storage ", 508 => "Loop Detected",
        510 => "Not Extended", 511 => "Network Authentication Required",
        _ => "No Reason Phrase",
    }
}

// ---- origins, sites, referrers ----

fn origin_of(u: &Url) -> String {
    u.origin().ascii_serialization()
}

/// The referrer a request sends (ReferrerInfo::ComputeReferrer): the document's URL under the
/// request's policy (strict-origin-when-cross-origin when it has none, network.http.referer.
/// defaultPolicy); None for no Referer header
fn referrer_for(policy: &str, referrer: Option<&Url>, target: &Url) -> Option<String> {
    let r = referrer?;
    if !matches!(r.scheme(), "http" | "https") {
        return None;
    }
    let mut full = r.clone();
    full.set_fragment(None);
    let _ = full.set_username("");
    let _ = full.set_password(None);
    let origin_only = format!("{}/", origin_of(r));
    // network.http.referer.referrerLengthLimit: a longer URL is sent as its origin
    let full = if full.as_str().len() > 4096 { origin_only.clone() } else { full.to_string() };
    let same = origin_of(r) == origin_of(target);
    let downgrade = trustworthy(r) && !trustworthy(target);
    match policy {
        "no-referrer" => None,
        "no-referrer-when-downgrade" => (!downgrade).then_some(full),
        "same-origin" => same.then_some(full),
        "origin" => Some(origin_only),
        "strict-origin" => (!downgrade).then_some(origin_only),
        "origin-when-cross-origin" => Some(if same { full } else { origin_only }),
        "unsafe-url" => Some(full),
        _ => {
            if same {
                Some(full)
            } else if downgrade {
                None
            } else {
                Some(origin_only)
            }
        }
    }
}

// ---- bodies ----

/// The body with its Content-Encoding taken off, as Firefox's channel decodes it (the last coding
/// applied is the first removed); an encoding it does not know is left as it is
fn decode_body(raw: Vec<u8>, encoding: Option<&str>, limit: u64) -> Result<Vec<u8>, String> {
    if raw.len() as u64 > limit { return Err(format!("{NETWORK_ERROR} (response body limit exceeded)")); }
    let Some(enc) = encoding else { return Ok(raw) };
    let mut body = raw;
    for coding in enc.split(',').map(|s| s.trim().to_ascii_lowercase()).rev() {
        if body.is_empty() {
            break;
        }
        let mut out = Vec::new();
        let ok = match coding.as_str() {
            "gzip" | "x-gzip" => flate2::read::MultiGzDecoder::new(&body[..]).take(limit.saturating_add(1)).read_to_end(&mut out).is_ok(),
            "deflate" => {
                // zlib-wrapped as the standard says, or raw deflate as some servers send
                flate2::read::ZlibDecoder::new(&body[..]).take(limit.saturating_add(1)).read_to_end(&mut out).is_ok() || {
                    out.clear();
                    flate2::read::DeflateDecoder::new(&body[..]).take(limit.saturating_add(1)).read_to_end(&mut out).is_ok()
                }
            }
            "br" => brotli::Decompressor::new(&body[..], 4096).take(limit.saturating_add(1)).read_to_end(&mut out).is_ok(),
            "zstd" => match zstd::stream::read::Decoder::new(&body[..]) {
                Ok(reader) => reader.take(limit.saturating_add(1)).read_to_end(&mut out).is_ok(),
                Err(_) => false,
            },
            _ => continue,
        };
        if !ok {
            return Err(format!("{NETWORK_ERROR} (content-encoding {coding})"));
        }
        if out.len() as u64 > limit { return Err(format!("{NETWORK_ERROR} (response body limit exceeded)")); }
        body = out;
    }
    Ok(body)
}

fn base64(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// Response.text(): UTF-8, its BOM dropped
fn utf8_text(b: &[u8]) -> String {
    encoding_rs::UTF_8.decode_with_bom_removal(b).0.into_owned()
}

/// The body as the bridge tab hands it on (mini_realsites.py OP "fetch"): text types as text (also
/// as bytes when their charset is not UTF-8, for an XHR to decode), any other type as base64; only
/// the bytes for a request with `bytes`
fn body_fields(bytes: &[u8], content_type: &str, want_bytes: bool) -> (Value, Value) {
    let lower = content_type.to_ascii_lowercase();
    let textual = content_type.is_empty()
        || lower.starts_with("text/")
        || ["json", "xml", "javascript", "ecmascript", "html", "x-www-form-urlencoded", "mpegurl"].iter().any(|t| lower.contains(t));
    if !textual {
        return (Value::Null, json!(base64(bytes)));
    }
    if want_bytes {
        return (Value::Null, json!(base64(bytes)));
    }
    let charset = lower
        .split(';')
        .skip(1)
        .filter_map(|p| p.trim().strip_prefix("charset").map(str::trim))
        .filter_map(|p| p.strip_prefix('='))
        .map(|p| p.trim().trim_matches('"').trim().to_string())
        .next();
    let utf8 = charset.as_deref().is_none_or(|c| {
        matches!(c, "utf-8" | "utf8" | "unicode-1-1-utf-8" | "unicode11utf8" | "unicode20utf8" | "x-unicode20utf8")
    });
    if utf8 {
        (json!(utf8_text(bytes)), Value::Null)
    } else {
        (json!(utf8_text(bytes)), json!(base64(bytes)))
    }
}

/// A data: URL (the Fetch Standard's data: URL processor, as Gecko's nsDataChannel answers it):
/// its type and bytes
fn data_url(u: &Url) -> Option<(String, Vec<u8>)> {
    let s = u.as_str();
    let rest = &s[5..];
    let rest = rest.split('#').next().unwrap_or("");
    let (head, body) = rest.split_once(',')?;
    let head = head.trim_matches(|c: char| c.is_ascii_whitespace());
    let decoded = percent_decode_bytes(body.as_bytes());
    let (mime, is_base64) = match head.rsplit_once(';') {
        Some((m, b)) if b.trim().eq_ignore_ascii_case("base64") => (m.trim().to_string(), true),
        _ => (head.to_string(), false),
    };
    let bytes = if is_base64 {
        use base64::Engine;
        let clean: Vec<u8> = decoded.into_iter().filter(|b| !b.is_ascii_whitespace()).collect();
        let trimmed: &[u8] = if clean.len() % 4 == 0 {
            let mut t = &clean[..];
            while t.last() == Some(&b'=') {
                t = &t[..t.len() - 1];
            }
            t
        } else {
            &clean[..]
        };
        base64::engine::general_purpose::STANDARD_NO_PAD.decode(trimmed).ok()?
    } else {
        decoded
    };
    let mime = if mime.is_empty() || mime.starts_with(';') {
        format!("text/plain{}", if mime.is_empty() { ";charset=US-ASCII".to_string() } else { mime })
    } else {
        mime
    };
    Some((mime, bytes))
}

fn percent_decode_bytes(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

// ---- the request ----

/// What a request is made with, from the runtime's fields
struct Request {
    url: Url,
    method: String,
    headers: Vec<(String, Vec<u8>)>,
    body: Option<Vec<u8>>,
    mode: String,
    credentials: String,
    redirect: String,
    referrer_policy: String,
    /// the global's origin ("null" when opaque) and its document's URL
    origin: String,
    referrer: Option<Url>,
    /// a classic script's, a sheet's or a worker script's source, as Firefox's script loader, sheet
    /// loader or worker fetches it: the runtime gets its body whatever the response's type
    script: bool,
    no_body: bool,
    want_bytes: bool,
    /// The request's destination (Sec-Fetch-Dest): "empty" for fetch and XHR, "image" (only the
    /// native image decoder sees an opaque body), "video" or "audio" (a media element's resource,
    /// for the HTTP viewer: `media`), or a script source's "script", "style", "worker"
    destination: String,
    /// The most response body bytes read (a media resource's); more is a network error
    body_limit: u64,
    timeout: Duration,
    /// The request's cache mode (Request.cache): the headers it sends (SetFetchCacheMode)
    cache: String,
    /// What the requesting document set (policy.rs)
    doc: Doc,
    /// The policy of the document's last <meta name=referrer> (bridge.js), or ""
    meta_referrer: String,
    /// A top-level navigation's (destination "document"): the initiator's site, and whether the
    /// owner's input started it (Sec-Fetch-User)
    navigation: Option<Navigation>,
}

struct Navigation {
    initiator_site: String,
    user: bool,
}

/// One answer off the network
struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    ms: f64,
}

enum Failure {
    Network(String),
    Pending,
}

/// The connection ended, or was refused writing, before any byte of an answer: ureq's
/// "Peer disconnected", a reset, an abort or a broken pipe
fn unanswered(e: &ureq::Error) -> bool {
    matches!(e, ureq::Error::Io(io) if matches!(io.kind(), std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::BrokenPipe))
}

impl Net {
    /// Sends one HTTP request (no redirect followed); its answer, or a failure
    fn send(&self, method: &str, url: &Url, headers: &[(String, Vec<u8>)], body: Option<&[u8]>, deadline: Instant, limit: u64) -> Result<Answer, Failure> {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Failure::Pending);
        }
        let t0 = Instant::now();
        let run = |agent: &ureq::Agent, left: Duration| -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
            let mut b = ureq::http::Request::builder().method(method).uri(url.as_str());
            for (k, v) in headers {
                if let Ok(v) = ureq::http::HeaderValue::from_bytes(v) {
                    b = b.header(k.as_str(), v);
                }
            }
            match body {
                Some(body) if !body.is_empty() || !matches!(method, "GET" | "HEAD") => {
                    agent.configure_request(b.body(body.to_vec())?).timeout_global(Some(left)).build().pipe(|r| agent.run(r))
                }
                _ => agent.configure_request(b.body(())?).timeout_global(Some(left)).build().pipe(|r| agent.run(r)),
            }
        };
        let fail = |e: ureq::Error| match e {
            ureq::Error::Timeout(_) => Failure::Pending,
            ureq::Error::Http(e) => Failure::Network(format!("{NETWORK_ERROR} ({e})")),
            e => Failure::Network(format!("{NETWORK_ERROR} ({e})")),
        };
        // A pooled connection that the server closed after its last answer gives no answer at all;
        // Firefox restarts such a request on a new connection (nsHttpTransaction::Restart: a reused
        // connection reset before any response byte). An HTTP/1.0 server that closes after every
        // answer without `Connection: close` (Python's http.server) is one, as ureq pools it.
        let resp = match run(&self.agent, left) {
            Err(e) if unanswered(&e) => {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(Failure::Pending);
                }
                run(&self.fresh, left)
            }
            other => other,
        }
        .map_err(fail)?;
        let status = resp.status().as_u16();
        let raw: Vec<(String, String)> =
            resp.headers().iter().map(|(k, v)| (k.as_str().to_ascii_lowercase(), latin1(v.as_bytes()))).collect();
        let head_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let mut body = Vec::new();
        if method != "HEAD" && !matches!(status, 204 | 304) {
            body = resp.into_body().with_config().limit(limit).read_to_vec().map_err(fail)?;
        }
        let measured = t0.elapsed().as_secs_f64() * 1000.0;
        if std::env::var_os("MRT_NET_TRACE").is_some() {
            eprintln!("net-trace {method} {} status {status} head {head_ms:.0} ms total {measured:.0} ms bytes {}", &url.as_str()[..url.as_str().len().min(90)], body.len());
        }
        let ms = header(&raw, RECORDED_MS).and_then(|v| v.trim().parse::<f64>().ok()).map(|v| v.max(0.0)).unwrap_or(measured);
        Ok(Answer { status, headers: raw, body, ms })
    }

    fn cached_preflight(&mut self, origin: &str, url: &Url, credentials: bool, method: &str, names: &[String]) -> bool {
        let now = Instant::now();
        self.preflights.retain(|p| p.expires > now);
        self.preflights.iter().any(|p| {
            p.origin == origin
                && p.url == url.as_str()
                && p.credentials == credentials
                && (matches!(method, "GET" | "HEAD" | "POST") || p.methods.iter().any(|m| m == method || (m == "*" && !credentials)))
                && names.iter().all(|n| p.headers.iter().any(|h| h == n || (h == "*" && !credentials && n != "authorization")))
        })
    }
}

/// `x.pipe(f)`: f(x), to keep the two sends above one expression each
trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}
impl<T> Pipe for T {}

/// The CORS check (nsCORSListenerProxy::CheckRequestApproved): the answer allows the origin, and
/// with credentials says so
fn cors_ok(headers: &[(String, String)], origin: &str, credentialed: bool) -> bool {
    let acao: Vec<&str> = headers.iter().filter(|(k, _)| k == "access-control-allow-origin").map(|(_, v)| v.trim()).collect();
    if acao.len() != 1 {
        return false;
    }
    if acao[0] == "*" && !credentialed {
        return true;
    }
    if acao[0] != origin {
        return false;
    }
    !credentialed || header(headers, "access-control-allow-credentials").map(str::trim) == Some("true")
}

fn list_header(headers: &[(String, String)], name: &str) -> Vec<String> {
    headers
        .iter()
        .filter(|(k, _)| k == name)
        .flat_map(|(_, v)| v.split(',').map(|s| s.trim().to_string()).collect::<Vec<_>>())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn handle(req: &Value, net: &mut Net, stores: &mut impl CookieStores) -> Result<Value, String> {
    // a script source names its destination (bridge.js `dest`); anything else is fetch or XHR's
    let dest = match req.get("dest").and_then(Value::as_str) {
        _ if req.get("navigate").and_then(Value::as_bool) == Some(true) => "document",
        Some("style") => "style",
        Some("worker") => "worker",
        _ if req.get("script").and_then(Value::as_bool) == Some(true) => "script",
        _ => "empty",
    };
    // a document load has no runtime waiting to poll it: it ends with an answer or an error
    let timeout = if dest == "document" { NAVIGATION_LIMIT } else { PENDING };
    handle_with_destination(req, net, stores, dest, timeout)
}

/// Live page requests finish with a response or an error, including deadline expiry.
pub fn live(req: &Value, net: &mut Net, stores: &mut impl CookieStores, timeout: Duration) -> Result<Value, String> {
    if req["script"] == true { return Err("mrt-api: script loads cannot use live fetch jobs".into()); }
    let mut request = req.clone();
    request["_liveJob"] = json!(true);
    handle_with_destination(&request, net, stores, "empty", timeout)
}

/// A media element's resource for the HTTP viewer (GAPS B172): the runtime's {url, crossOrigin,
/// kind: "video" | "audio", range} with the driver's origin, referrer and doc, fetched as Gecko's
/// media channel does, in the page's partition with its cookies. Without crossorigin it is no-cors
/// with credentials (the media pipeline sees an opaque body, as the image decoder does);
/// "anonymous" (or any other value) is CORS with same-origin credentials, "use-credentials" CORS
/// with credentials (HTMLMediaElement::ChannelLoader::LoadInternal). The reply is fetch's, with the
/// body as bytes; a body over MEDIA_LIMIT is a network error.
pub fn media(req: &Value, net: &mut Net, stores: &mut Stores) -> Result<Value, String> {
    let (mode, credentials) = match req.get("crossOrigin") {
        None | Some(Value::Null) => ("no-cors", "include"),
        Some(v) if v.as_str().is_some_and(|c| c.eq_ignore_ascii_case("use-credentials")) => ("cors", "include"),
        Some(_) => ("cors", "same-origin"),
    };
    let dest = if req.get("kind").and_then(Value::as_str) == Some("audio") { "audio" } else { "video" };
    let mut r = req.clone();
    r["method"] = json!("GET");
    r["mode"] = json!(mode);
    r["credentials"] = json!(credentials);
    r["headers"] = json!([]);
    r["bytes"] = json!(true);
    r["script"] = json!(false);
    r["body"] = Value::Null;
    r["bodyBase64"] = Value::Null;
    handle_with_destination(&r, net, stores, dest, PENDING)
}

/// Only the native image decoder sees the opaque response's body. No request
/// field can enable this path for fetch(), which always uses destination empty.
pub fn image(req: &Value, net: &mut Net, stores: &mut Stores) -> Result<Value, String> {
    handle_with_destination(req, net, stores, "image", PENDING)
}

/// An image request run by a live job (fetch_jobs, B218): as `image`, with the job's deadline
/// instead of PENDING, and a network error when it passes.
pub fn image_live(req: &Value, net: &mut Net, stores: &mut impl CookieStores, timeout: Duration) -> Result<Value, String> {
    let mut request = req.clone();
    request["_liveJob"] = json!(true);
    handle_with_destination(&request, net, stores, "image", timeout)
}

fn handle_with_destination(req: &Value, net: &mut Net, stores: &mut impl CookieStores, destination: &str, timeout: Duration) -> Result<Value, String> {
    let s = |k: &str| req.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let url = Url::parse(&s("url")).map_err(|_| NETWORK_ERROR.to_string())?;
    let mut url = url;
    url.set_fragment(None);
    let script = req.get("script").and_then(Value::as_bool).unwrap_or(false);
    let method = if script { "GET".to_string() } else { Some(s("method")).filter(|m| !m.is_empty()).unwrap_or_else(|| "GET".into()) };
    // a classic script or a sheet: no-cors (ScriptLoader, css::Loader); a worker's own script:
    // same-origin (WorkerScriptLoader)
    let navigate = destination == "document";
    let mode = if script {
        if destination == "worker" { "same-origin".to_string() } else { "no-cors".to_string() }
    } else if navigate {
        "navigate".to_string()
    } else {
        Some(s("mode")).filter(|m| !m.is_empty()).unwrap_or_else(|| "cors".into())
    };
    let body = if let Some(b) = req.get("bodyBase64").and_then(Value::as_str) {
        use base64::Engine;
        Some(base64::engine::general_purpose::STANDARD.decode(b).map_err(|e| format!("mrt-api: fetch body: {e}"))?)
    } else {
        req.get("body").and_then(Value::as_str).map(|b| b.as_bytes().to_vec())
    };
    let mut headers: Vec<(String, Vec<u8>)> = Vec::new();
    if script && destination == "style" {
        headers.push(("accept".into(), b"text/css,*/*;q=0.1".to_vec()));
    }
    if navigate {
        // a document load (nsDocShell, DocumentLoadListener): Firefox's Accept for documents and
        // Upgrade-Insecure-Requests; of the page's fields only a form's Content-Type
        headers.push(("accept".into(), b"text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".to_vec()));
        headers.push(("upgrade-insecure-requests".into(), b"1".to_vec()));
        for h in req.get("headers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            if let (Some(k), Some(v)) = (h.get(0).and_then(Value::as_str), h.get(1).and_then(Value::as_str)) {
                if k.eq_ignore_ascii_case("content-type") && !matches!(method.as_str(), "GET" | "HEAD") {
                    headers.push(("content-type".into(), bytes_of(v)));
                }
            }
        }
    } else if !script {
        for h in req.get("headers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            let (Some(k), Some(v)) = (h.get(0).and_then(Value::as_str), h.get(1).and_then(Value::as_str)) else { continue };
            let k = k.to_ascii_lowercase();
            if forbidden_request_header(&k, v) {
                continue;
            }
            // Headers' request-no-cors guard keeps only the no-CORS-safelisted ones
            if mode == "no-cors" && !(matches!(k.as_str(), "accept" | "accept-language" | "content-language" | "content-type") && cors_safelisted(&k, &bytes_of(v))) {
                continue;
            }
            headers.push((k, bytes_of(v)));
        }
    }
    // a media element's request (HTMLMediaElement's channel, ChannelMediaResource): its Accept, and
    // the byte range the media cache asks for, which no page header could add under no-cors
    if matches!(destination, "video" | "audio") {
        let accept: &[u8] = if destination == "video" { MEDIA_ACCEPT_VIDEO } else { MEDIA_ACCEPT_AUDIO };
        headers.push(("accept".into(), accept.to_vec()));
        if let Some(range) = req.get("range").and_then(Value::as_str).filter(|r| byte_range(r)) {
            headers.push(("range".into(), range.as_bytes().to_vec()));
        }
    }
    // a string body without a type is text/plain;charset=UTF-8 (BodyExtractor for a string)
    if req.get("body").and_then(Value::as_str).is_some() && !matches!(method.as_str(), "GET" | "HEAD") && !headers.iter().any(|(k, _)| k == "content-type") {
        headers.push(("content-type".into(), b"text/plain;charset=UTF-8".to_vec()));
    }
    // InternalRequest::MaybeSkipCacheIfPerformingRevalidation: a default request with a revalidation
    // header is no-store
    let mut cache = Some(s("cache")).filter(|c| !c.is_empty()).unwrap_or_else(|| "default".into());
    if cache == "default"
        && headers.iter().any(|(k, _)| matches!(k.as_str(), "if-modified-since" | "if-none-match" | "if-unmodified-since" | "if-match" | "if-range"))
    {
        cache = "no-store".into();
    }
    let origin = Some(s("origin")).filter(|o| !o.is_empty()).unwrap_or_else(|| "null".into());
    let r = Request {
        url,
        method,
        headers,
        body,
        mode,
        credentials: if script {
            if destination == "worker" { "same-origin".into() } else { "include".into() }
        } else if navigate {
            "include".into()
        } else {
            Some(s("credentials")).filter(|c| !c.is_empty()).unwrap_or_else(|| "same-origin".into())
        },
        redirect: if script || navigate { "follow".into() } else { Some(s("redirect")).filter(|c| !c.is_empty()).unwrap_or_else(|| "follow".into()) },
        referrer_policy: s("referrerPolicy"),
        doc: Doc::from_request(req, &origin),
        origin,
        referrer: Url::parse(&s("referrer")).ok(),
        script,
        no_body: req.get("noBody").and_then(Value::as_bool).unwrap_or(false),
        want_bytes: req.get("bytes").is_some_and(|b| !b.is_null() && b != &json!(false)),
        body_limit: if matches!(destination, "video" | "audio") { MEDIA_LIMIT } else if req["_liveJob"] == true { 16 * 1024 * 1024 } else { u64::MAX },
        timeout,
        destination: destination.to_string(),
        cache,
        meta_referrer: s("metaReferrer"),
        navigation: navigate.then(|| Navigation {
            initiator_site: s("initiatorSite"),
            user: req.get("user").and_then(Value::as_bool).unwrap_or(false),
        }),
    };
    // Request's constructor: no-cors takes only CORS-safelisted methods
    if r.mode == "no-cors" && !matches!(r.method.as_str(), "GET" | "HEAD" | "POST") {
        return Err(format!("TypeError: Request constructor: Invalid request method {}.", r.method));
    }
    let ancestor = req.get("ancestor").and_then(Value::as_bool).unwrap_or(false);
    match main_fetch(r, net, stores, &s("site"), &s("top"), ancestor) {
        Ok(v) => Ok(v),
        Err(Failure::Pending) if req["_liveJob"] == true => Err(format!("{NETWORK_ERROR} (live fetch timed out)")),
        Err(Failure::Pending) if destination == "document" => Err(format!("{NETWORK_ERROR} (no answer in {} s)", NAVIGATION_LIMIT.as_secs())),
        Err(Failure::Pending) => Ok(json!({ "pending": true })),
        Err(Failure::Network(e)) => Err(e),
    }
}

/// The Fetch Standard's main fetch with HTTP fetch and its redirects, as Gecko runs them
/// `ancestor`: the initiator's window has another site's frame above it (main.rs): every request is
/// foreign, its cookies in the foreign-ancestor partition (CookieJarSettings::
/// UpdatePartitionKeyForDocumentLoadedByChannel gives the document's loads that partition key).
fn main_fetch(mut r: Request, net: &mut Net, stores: &mut impl CookieStores, site: &str, top: &str, ancestor: bool) -> Result<Value, Failure> {
    let deadline = Instant::now() + r.timeout;
    let network = |e: &str| Failure::Network(format!("{NETWORK_ERROR} ({e})"));
    let request_origin = r.origin.clone();
    let request_origin_url = Url::parse(&request_origin).ok();
    // basic, cors or opaque; once cors or opaque it stays so across redirects
    let mut tainting = "basic";
    let mut tainted_origin = false;
    let mut url_list: Vec<Url> = Vec::new();
    let mut total_ms = 0.0;
    // the page's top-level site, and whether the initiator is itself cross-site (a frame's runtime)
    let top_url = Url::parse(top).ok();
    let top_site = top_url.as_ref().map(site_of).unwrap_or_default();
    // the referrer policy: the request's, else the document's (its last <meta name=referrer>, else
    // its Referrer-Policy header; Document::UpdateReferrerInfoFromMeta replaces the header's)
    // (an attribute's value that is not a policy is none)
    if !is_policy(&r.referrer_policy) {
        r.referrer_policy = if !r.meta_referrer.is_empty() { r.meta_referrer.clone() } else { r.doc.referrer_policy.clone() };
    }
    // a navigation: whether the initiator and every hop so far are same-site (SameSite=Strict)
    let mut nav_same_site = true;
    // (and the Referer its last hop sent: the new document's document.referrer)
    let mut sent_referrer: Option<String>;
    loop {
        // the document's policies over this URL, before anything goes out (each redirect's too):
        // CSP's connect-src for fetch and XHR (nsCSPService, its upgrade allowance), then
        // upgrade-insecure-requests (nsHttpChannel), then mixed content (nsMixedContentBlocker)
        if r.destination == "empty" && !r.doc.connect_allowed(&r.url, !url_list.is_empty()) {
            return Err(network("refused by the document's Content Security Policy (connect-src)"));
        }
        let mut upgraded = false;
        // (a navigation is upgraded only to its initiator's host: CSP's upgrade-insecure-requests
        // upgrades same-origin navigations, nsHttpChannel's insecure navigations set)
        let same_host = || Url::parse(&r.origin).ok().is_some_and(|o| o.host_str() == r.url.host_str());
        if r.doc.upgrades() && (r.navigation.is_none() || same_host()) {
            if let Some(u) = policy::upgraded(&r.url) {
                r.url = u;
                upgraded = true;
            }
        }
        // (a top-level document is never mixed content)
        if !upgraded && r.navigation.is_none() && policy::mixed_content(&r.origin, &r.url) {
            // display content is upgraded (security.mixed_content.upgrade_display_content), the rest refused
            match policy::upgraded(&r.url).filter(|_| matches!(r.destination.as_str(), "image" | "video" | "audio")) {
                Some(u) => r.url = u,
                None => return Err(network("blocked mixed content")),
            }
        }
        let url = r.url.clone();
        url_list.push(url.clone());
        let same_origin = r.origin != "null" && origin_of(&url) == r.origin;
        // data: answers at once, as a basic response in any mode
        if url.scheme() == "data" {
            if r.method != "GET" && !r.script {
                return Err(network("data: with a method other than GET"));
            }
            let (mime, bytes) = data_url(&url).ok_or_else(|| network("bad data: URL"))?;
            if bytes.len() as u64 > r.body_limit { return Err(network("response body limit exceeded")); }
            let headers = vec![("content-type".to_string(), mime.clone()), ("content-length".to_string(), bytes.len().to_string())];
            let type_ = if tainting == "basic" { "basic" } else { tainting };
            if tainting == "opaque" {
                return Ok(opaque(r.no_body, total_ms));
            }
            let (body, body64) = if r.no_body { (Value::Null, Value::Null) } else { body_fields(&bytes, &mime, r.want_bytes) };
            let body = if r.script && body.is_null() { json!(utf8_text(&bytes)) } else { body };
            return Ok(json!({ "status": 200, "statusText": "OK", "url": url.as_str(), "redirected": url_list.len() > 1,
                               "type": type_, "headers": listed_headers(&headers, false, false), "body": body,
                               "bodyBase64": if r.script { Value::Null } else { body64 }, "took": total_ms }));
        }
        if !matches!(url.scheme(), "http" | "https") {
            return Err(network("scheme"));
        }
        if (same_origin && tainting == "basic") || r.navigation.is_some() {
            // basic (a navigation's response is the new document's: always readable)
        } else if r.mode == "same-origin" {
            return Err(network("same-origin mode, cross-origin URL"));
        } else if r.mode == "no-cors" {
            tainting = "opaque";
        } else {
            tainting = "cors";
        }
        let url_site = site_of(&url);
        // COEP credentialless (CrossOriginEmbedderPolicyAllowsCredentials): a no-cors request to
        // another origin goes without credentials
        let coep_anonymous = r.doc.coep == "credentialless" && r.mode == "no-cors" && !(same_origin && !tainted_origin);
        let credentialed =
            !coep_anonymous && (r.credentials == "include" || (r.credentials == "same-origin" && tainting == "basic"));
        let serialized_origin = if tainted_origin { "null".to_string() } else { r.origin.clone() };

        // the headers Firefox adds
        let mut h: Vec<(String, Vec<u8>)> = Vec::new();
        h.push(("user-agent".into(), net.user_agent.as_bytes().to_vec()));
        if !r.headers.iter().any(|(k, _)| k == "accept") {
            h.push(("accept".into(), b"*/*".to_vec()));
        }
        if !r.headers.iter().any(|(k, _)| k == "accept-language") {
            h.push(("accept-language".into(), net.accept_language.as_bytes().to_vec()));
        }
        let encodings: &[u8] = if url.scheme() == "https" { b"gzip, deflate, br, zstd" } else { b"gzip, deflate" };
        h.push(("accept-encoding".into(), encodings.to_vec()));
        h.extend(r.headers.iter().cloned());
        let cache_headers = cache_mode_headers(&r.cache, &r.headers);
        h.extend(cache_headers.iter().cloned());
        if r.cache == "only-if-cached" {
            // LOAD_ONLY_FROM_CACHE, and there is no cache here
            return Err(network("only-if-cached: not in the cache"));
        }
        // ReferrerInfo::ShouldIgnoreLessRestrictedPolicies (network.http.referer.
        // disallowCrossSiteRelaxingDefault): a cross-site request does not take a policy that relaxes
        // the default; it gets the default
        let cross_site = request_origin_url.as_ref().is_none_or(|o| base_domain(o) != base_domain(&url));
        let policy = if cross_site && matches!(r.referrer_policy.as_str(), "unsafe-url" | "no-referrer-when-downgrade" | "origin-when-cross-origin") {
            ""
        } else {
            r.referrer_policy.as_str()
        };
        let referrer = referrer_for(policy, r.referrer.as_ref(), &url);
        sent_referrer = referrer.clone();
        {
            // HttpBaseChannel::SetOriginHeader and the CORS listener: a CORS request, and any request
            // whose method is not GET or HEAD, says where it is from
            if tainting == "cors" {
                h.push(("origin".into(), serialized_origin.as_bytes().to_vec()));
            } else if !matches!(r.method.as_str(), "GET" | "HEAD") {
                let null = match r.referrer_policy.as_str() {
                    "no-referrer" => true,
                    "same-origin" => !same_origin,
                    "no-referrer-when-downgrade" | "strict-origin" | "strict-origin-when-cross-origin" | "" => {
                        request_origin_url.as_ref().is_some_and(|o| o.scheme() == "https") && url.scheme() != "https" && !trustworthy(&url)
                    }
                    _ => false,
                };
                let v = if null || tainted_origin { "null".to_string() } else { r.origin.clone() };
                h.push(("origin".into(), v.into_bytes()));
            }
            if let Some(rf) = &referrer {
                h.push(("referer".into(), rf.as_bytes().to_vec()));
            }
        }
        // the cookies of the request's partition: the URL's site under the page's top-level site
        // (a navigation's hop is a top-level document of its own site: that site's first-party store)
        let (store_site, store_top, foreign, store_ancestor) = if r.navigation.is_some() {
            (url_site.clone(), url_site.clone(), false, false)
        } else {
            (url_site.clone(), top.to_string(), ancestor || url_site != top_site || site_of_str(site) != top_site, ancestor)
        };
        if let Some(n) = &r.navigation {
            nav_same_site &= n.initiator_site == url_site;
        }
        if credentialed && !store_site.is_empty() {
            let safe = matches!(r.method.as_str(), "GET" | "HEAD");
            let c = stores.with_store(&store_site, &store_top, store_ancestor, |store| match &r.navigation {
                Some(_) => cookie::for_navigation(store, url.as_str(), nav_same_site, safe),
                None => cookie::for_request(store, url.as_str(), foreign),
            }).unwrap_or_default();
            if !c.is_empty() { h.push(("cookie".into(), c.into_bytes())); }
        }
        // SecFetch::AddSecFetchHeader: for potentially trustworthy URLs
        if trustworthy(&url) {
            h.push(("sec-fetch-dest".into(), r.destination.as_bytes().to_vec()));
            h.push(("sec-fetch-mode".into(), r.mode.as_bytes().to_vec()));
            let site_value: &[u8] = if sec_fetch_same_origin(&r.origin, &url_list) {
                b"same-origin"
            } else if sec_fetch_same_site(request_origin_url.as_ref(), &url_list) {
                b"same-site"
            } else {
                b"cross-site"
            };
            h.push(("sec-fetch-site".into(), site_value.to_vec()));
            if r.navigation.as_ref().is_some_and(|n| n.user) {
                h.push(("sec-fetch-user".into(), b"?1".to_vec()));
            }
        }

        // a CORS preflight, when the method or a header is not safelisted
        if tainting == "cors" {
            let mut unsafe_names: Vec<String> = r
                .headers
                .iter()
                .filter(|(k, v)| !cors_safelisted(k, v))
                .map(|(k, _)| k.clone())
                .collect();
            unsafe_names.sort();
            unsafe_names.dedup();
            let simple_method = matches!(r.method.as_str(), "GET" | "HEAD" | "POST");
            // LOAD_BYPASS_CACHE (no-store, reload) also bypasses the preflight cache
            let bypass = matches!(r.cache.as_str(), "no-store" | "reload");
            if (!simple_method || !unsafe_names.is_empty())
                && (bypass || !net.cached_preflight(&serialized_origin, &url, credentialed, &r.method, &unsafe_names))
            {
                let mut ph: Vec<(String, Vec<u8>)> = vec![
                    ("user-agent".into(), net.user_agent.as_bytes().to_vec()),
                    ("accept".into(), b"*/*".to_vec()),
                    ("accept-language".into(), net.accept_language.as_bytes().to_vec()),
                    ("accept-encoding".into(), encodings.to_vec()),
                    ("access-control-request-method".into(), r.method.as_bytes().to_vec()),
                ];
                if !unsafe_names.is_empty() {
                    ph.push(("access-control-request-headers".into(), unsafe_names.join(",").into_bytes()));
                }
                ph.extend(cache_headers.iter().cloned());
                ph.push(("origin".into(), serialized_origin.as_bytes().to_vec()));
                if let Some(rf) = &referrer {
                    ph.push(("referer".into(), rf.as_bytes().to_vec()));
                }
                if trustworthy(&url) {
                    ph.push(("sec-fetch-dest".into(), r.destination.as_bytes().to_vec()));
                    ph.push(("sec-fetch-mode".into(), b"cors".to_vec()));
                    let v: &[u8] = if sec_fetch_same_origin(&r.origin, &url_list) {
                        b"same-origin"
                    } else if sec_fetch_same_site(request_origin_url.as_ref(), &url_list) {
                        b"same-site"
                    } else {
                        b"cross-site"
                    };
                    ph.push(("sec-fetch-site".into(), v.to_vec()));
                }
                let pre = net.send("OPTIONS", &url, &ph, None, deadline, u64::MAX)?;
                total_ms += pre.ms;
                if !(200..300).contains(&pre.status) || !cors_ok(&pre.headers, &serialized_origin, credentialed) {
                    return Err(network("CORS preflight did not succeed"));
                }
                let methods = list_header(&pre.headers, "access-control-allow-methods");
                let allowed_headers: Vec<String> =
                    list_header(&pre.headers, "access-control-allow-headers").into_iter().map(|s| s.to_ascii_lowercase()).collect();
                let method_ok = simple_method || methods.iter().any(|m| m == &r.method || (m == "*" && !credentialed));
                let headers_ok = unsafe_names
                    .iter()
                    .all(|n| allowed_headers.iter().any(|a| a == n || (a == "*" && !credentialed && n != "authorization")));
                if !method_ok {
                    return Err(network("CORS method not allowed"));
                }
                if !headers_ok {
                    return Err(network("CORS header not allowed"));
                }
                let max_age = header(&pre.headers, "access-control-max-age").and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(PREFLIGHT_DEFAULT_S);
                if max_age > 0 {
                    net.preflights.push(Preflight {
                        origin: serialized_origin.clone(),
                        url: url.to_string(),
                        credentials: credentialed,
                        methods,
                        headers: allowed_headers,
                        expires: Instant::now() + Duration::from_secs(max_age),
                    });
                }
            }
        }

        let send_body = if matches!(r.method.as_str(), "GET" | "HEAD") { None } else { r.body.as_deref().or(Some(&[][..])) };
        let answer = net.send(&r.method, &url, &h, send_body, deadline, r.body_limit)?;
        total_ms += answer.ms;
        // the response's cookies, when its request could carry them (not LOAD_ANONYMOUS)
        if credentialed && !store_site.is_empty() {
            let lines: Vec<String> = answer.headers.iter().filter(|(k, _)| k == "set-cookie").map(|(_, v)| v.clone()).collect();
            if !lines.is_empty() {
                stores.with_store(&store_site, &store_top, store_ancestor,
                    |store| cookie::from_response(store, url.as_str(), &lines, foreign));
            }
        }
        if tainting == "cors" && !cors_ok(&answer.headers, &serialized_origin, credentialed) {
            return Err(network("CORS header 'Access-Control-Allow-Origin' missing or not matching"));
        }
        let location = header(&answer.headers, "location").map(str::to_string);
        if matches!(answer.status, 301 | 302 | 303 | 307 | 308) && location.is_some() {
            match r.redirect.as_str() {
                "error" => return Err(network("redirect mode error")),
                "manual" => {
                    if tainting == "opaque" {
                        return Err(network("no-cors with redirect mode manual"));
                    }
                    return Ok(json!({ "status": 0, "statusText": "", "url": url.as_str(), "redirected": url_list.len() > 1,
                                       "type": "opaqueredirect", "headers": [], "body": if r.no_body { Value::Null } else { json!("") },
                                       "bodyBase64": Value::Null, "took": total_ms }));
                }
                _ => {}
            }
            if tainting == "opaque" && r.redirect != "follow" {
                return Err(network("no-cors redirect"));
            }
            if url_list.len() > MAX_REDIRECTS {
                return Err(network("too many redirects"));
            }
            let next = url.join(location.as_deref().unwrap()).map_err(|_| network("bad Location"))?;
            if !matches!(next.scheme(), "http" | "https") {
                return Err(network("redirect to a scheme other than http(s)"));
            }
            if tainting == "cors" && (!next.username().is_empty() || next.password().is_some()) {
                return Err(network("CORS redirect with credentials in the URL"));
            }
            // 301 and 302 turn a POST into a GET, 303 anything but HEAD
            if ((answer.status == 301 || answer.status == 302) && r.method == "POST") || (answer.status == 303 && r.method != "GET" && r.method != "HEAD") {
                r.method = "GET".into();
                r.body = None;
                r.headers.retain(|(k, _)| !matches!(k.as_str(), "content-encoding" | "content-language" | "content-location" | "content-type"));
            }
            // a cross-origin hop from another origin than the request's taints the origin
            if origin_of(&next) != origin_of(&url) && request_origin != origin_of(&url) {
                tainted_origin = true;
            }
            if origin_of(&next) != origin_of(&url) {
                r.headers.retain(|(k, _)| k != "authorization");
            }
            // the redirect's Referrer-Policy becomes the request's
            if let Some(p) = header(&answer.headers, "referrer-policy") {
                if let Some(last) = p.split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| is_policy(s)).last() {
                    r.referrer_policy = last;
                }
            }
            let mut next = next;
            next.set_fragment(None);
            r.url = next;
            continue;
        }
        // CORP (Cross-Origin-Resource-Policy, HttpBaseChannel::ProcessCrossOriginResourcePolicyHeader)
        // for a no-cors answer from another origin; under COEP require-corp an answer without one
        // is same-origin (credentialless: only when credentials were included, which a no-cors
        // channel never says: GetCorsIncludeCredentials is the CORS listener's)
        if tainting == "opaque" {
            let corp = header(&answer.headers, "cross-origin-resource-policy").map(str::trim).unwrap_or("");
            let corp = if corp.is_empty() && r.doc.coep == "require-corp" { "same-origin" } else { corp };
            let blocked = match corp {
                "same-origin" => origin_of(&url) != request_origin,
                "same-site" => {
                    request_origin_url.as_ref().is_none_or(|o| base_domain(o) != base_domain(&url) || (o.scheme() != "https" && url.scheme() == "https"))
                }
                _ => false,
            };
            if blocked {
                return Err(network("blocked by Cross-Origin-Resource-Policy"));
            }
            if !matches!(r.destination.as_str(), "image" | "video" | "audio") && !r.script {
                return Ok(opaque(r.no_body, total_ms));
            }
        }
        let body = decode_body(answer.body, header(&answer.headers, "content-encoding"), r.body_limit).map_err(Failure::Network)?;
        let content_type = header(&answer.headers, "content-type").unwrap_or("").to_string();
        // (the media pipeline reads the internal response's headers, Content-Range among them)
        let media = matches!(r.destination.as_str(), "video" | "audio");
        let listed = listed_headers(&answer.headers, tainting == "cors" && !r.script && !media, credentialed);
        let (text, text64) = if r.no_body {
            (Value::Null, Value::Null)
        } else if r.script {
            (json!(utf8_text(&body)), Value::Null)
        } else {
            body_fields(&body, &content_type, r.want_bytes)
        };
        let mut reply = json!({ "status": answer.status, "statusText": status_text(answer.status), "url": url.as_str(),
                           "redirected": url_list.len() > 1, "type": if r.script { "basic" } else { tainting },
                           "headers": listed, "body": text, "bodyBase64": text64, "took": total_ms });
        if r.navigation.is_some() {
            reply["referrer"] = json!(sent_referrer.unwrap_or_default());
        }
        return Ok(reply);
    }
}

/// The headers a cache mode adds (HttpBaseChannel::SetFetchCacheMode's load flags, sent by
/// nsHttpChannel::SetupTransaction), each only when the page set none: LOAD_BYPASS_CACHE (no-store,
/// reload) sends Pragma and Cache-Control no-cache, VALIDATE_ALWAYS (no-cache) Cache-Control max-age=0
fn cache_mode_headers(cache: &str, set: &[(String, Vec<u8>)]) -> Vec<(String, Vec<u8>)> {
    let has = |n: &str| set.iter().any(|(k, _)| k == n);
    let mut out = Vec::new();
    match cache {
        "no-store" | "reload" => {
            if !has("pragma") {
                out.push(("pragma".into(), b"no-cache".to_vec()));
            }
            if !has("cache-control") {
                out.push(("cache-control".into(), b"no-cache".to_vec()));
            }
        }
        "no-cache" if !has("cache-control") => out.push(("cache-control".into(), b"max-age=0".to_vec())),
        _ => {}
    }
    out
}

/// The host part of a URL's site: Gecko's base domain (ThirdPartyUtil::GetBaseDomain)
fn base_domain(u: &Url) -> String {
    let site = site_of(u);
    site.split_once("://").map(|(_, h)| h.to_string()).unwrap_or(site)
}

/// SecFetch's IsSameOrigin: the request's origin is the final URL's, and so is every URL that
/// redirected on the way
fn sec_fetch_same_origin(origin: &str, urls: &[Url]) -> bool {
    let last = urls.last().unwrap();
    origin != "null" && origin_of(last) == origin && urls[..urls.len() - 1].iter().all(|u| origin_of(u) == origin_of(last))
}

/// SecFetch's IsSameSite: the request's origin has the final URL's base domain and is https or a
/// loopback host, and every URL that redirected on the way has that base domain and is https
fn sec_fetch_same_site(origin: Option<&Url>, urls: &[Url]) -> bool {
    let Some(o) = origin else { return false };
    let last = urls.last().unwrap();
    let base = base_domain(last);
    let loopback = matches!(o.host(), Some(url::Host::Domain("localhost")))
        || matches!(o.host(), Some(url::Host::Ipv4(a)) if a.is_loopback())
        || matches!(o.host(), Some(url::Host::Ipv6(a)) if a.is_loopback());
    base_domain(o) == base
        && (o.scheme() == "https" || loopback)
        && urls[..urls.len() - 1].iter().all(|u| base_domain(u) == base && u.scheme() == "https")
}

fn is_policy(s: &str) -> bool {
    matches!(
        s,
        "no-referrer" | "no-referrer-when-downgrade" | "origin" | "origin-when-cross-origin" | "same-origin" | "strict-origin"
            | "strict-origin-when-cross-origin" | "unsafe-url"
    )
}

fn site_of_str(s: &str) -> String {
    Url::parse(s).map(|u| site_of(&u)).unwrap_or_else(|_| s.to_string())
}

/// An opaque filtered response, as the bridge tab hands it on: status 0, no headers, an empty body
fn opaque(no_body: bool, took: f64) -> Value {
    json!({ "status": 0, "statusText": "", "url": "", "redirected": false, "type": "opaque", "headers": [],
            "body": if no_body { Value::Null } else { json!("") }, "bodyBase64": Value::Null, "took": took })
}

#[cfg(test)]
mod tests {
    use super::{byte_range, Net};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    /// Reads one request's head off `s`
    fn request(s: &mut std::net::TcpStream) {
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && s.read(&mut b).unwrap() == 1 {
            head.push(b[0]);
        }
    }

    #[test]
    fn request_on_a_closed_pooled_connection_is_sent_again() {
        // An HTTP/1.0 answer without `Connection: close` (Python's http.server): ureq pools the
        // connection. The server takes the next request on it and closes, as when its close
        // crosses that request; the request is answered on a new connection.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = url::Url::parse(&format!("http://127.0.0.1:{}/", listener.local_addr().unwrap().port())).unwrap();
        let server = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            request(&mut first);
            first.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\none").unwrap();
            request(&mut first);
            drop(first);
            let (mut second, _) = listener.accept().unwrap();
            request(&mut second);
            second.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\ntwo").unwrap();
        });
        let net = Net::from_env();
        let deadline = Instant::now() + Duration::from_secs(5);
        let body = |r: Result<super::Answer, super::Failure>| match r {
            Ok(a) => String::from_utf8(a.body).unwrap(),
            Err(super::Failure::Network(e)) => e,
            Err(super::Failure::Pending) => "pending".into(),
        };
        assert_eq!(body(net.send("GET", &url, &[], None, deadline, u64::MAX)), "one");
        assert_eq!(body(net.send("GET", &url, &[], None, deadline, u64::MAX)), "two");
        server.join().unwrap();
    }

    #[test]
    fn media_range_header() {
        assert!(byte_range("bytes=0-"));
        assert!(byte_range("bytes=100-4195"));
        assert!(!byte_range("bytes=-10"));
        assert!(!byte_range("bytes=10-5"));
        assert!(!byte_range("bytes=0-1,5-6"));
        assert!(!byte_range("items=0-"));
        assert!(!byte_range("bytes=0-\r\nx: y"));
    }
}
