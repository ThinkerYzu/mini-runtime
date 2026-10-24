// document.cookie, as Gecko answers it (dom/base/Document.cpp GetCookie and SetCookie over
// netwerk/cookie: CookieCommons, CookieParser, CookieValidation, CookiePrefixes and
// CookieStorage::AddCookie), on the partition's jar in its SQLite file. The prefs are release
// Firefox's (157): Total Cookie Protection on (cookieBehavior 5), SameSite not lax by default,
// SameSite=None requires Secure, CHIPS on without opt-in partitioning, max-age capped at 400 days,
// valueless cookies (a string without '=' is a name), 180 cookies per base domain evicted to 150.
//
// A request: {url, value?} from bridge.js (the document's principal URL; the string assigned) with
// the driver's site and top; or {call: "sync", cookies} while fetch is still the tab's (cookie routed
// without fetch): Firefox's jar for the partition, which received the HTTP responses' Set-Cookie,
// replaces this one's. With fetch routed, fetch.rs sends and sets a request's cookies here
// (for_request, from_response).
//
// A partition's buckets are Gecko's (base domain, OriginAttributes) entries: 0 the jar the site's
// documents use (unpartitioned for a first party, the top-level site's partition for a third
// party), 1 a first party's own CHIPS partition (cookies set with the Partitioned attribute).

use std::sync::atomic::{AtomicI64, Ordering};

use rusqlite::params;
use serde_json::{Value, json};

use crate::store::Store;

const SAMESITE_NONE: i64 = 0;
const SAMESITE_LAX: i64 = 1;
const SAMESITE_STRICT: i64 = 2;
const SAMESITE_UNSET: i64 = 256;
const SCHEME_HTTP: i64 = 1;
const SCHEME_HTTPS: i64 = 2;
const SCHEME_FILE: i64 = 4;

const MAX_BYTES_PER_COOKIE: usize = 4096;
const MAX_BYTES_PER_ATTRIBUTE: usize = 1024;
const MAXAGE_CAP_S: i64 = 34_560_000;
const MAX_PER_HOST: usize = 180;
const QUOTA_PER_HOST: usize = 150;
const STALE_THRESHOLD_US: i64 = 60 * 1_000_000;

pub fn now_us() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as i64).unwrap_or(0)
}

/// Cookie::GenerateUniqueCreationTimeInUSec: creation times never repeat (they order cookies).
fn unique_creation(t: i64) -> i64 {
    static LAST: AtomicI64 = AtomicI64::new(0);
    let mut old = LAST.load(Ordering::Relaxed);
    loop {
        let want = if t > old { t } else { old + 1 };
        match LAST.compare_exchange(old, want, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return want,
            Err(o) => old = o,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Cookie {
    name: String,
    value: String,
    host: String,
    path: String,
    expiry_ms: i64,
    last_accessed: i64,
    creation: i64,
    secure: bool,
    http_only: bool,
    same_site: i64,
    scheme_map: i64,
    partitioned_attr: bool,
    session: bool,
}

impl Cookie {
    fn is_domain(&self) -> bool {
        self.host.starts_with('.')
    }
    fn raw_host(&self) -> &str {
        self.host.strip_prefix('.').unwrap_or(&self.host)
    }
    fn expired(&self, now_ms: i64) -> bool {
        self.expiry_ms <= now_ms
    }
}

/// The document's address as the cookie code reads it from its principal.
struct HostUri {
    scheme: String,
    host: String,
    /// GetFilePath: the path without query or fragment
    file_path: String,
    trustworthy: bool,
}

fn host_uri(url: &str) -> Option<HostUri> {
    let u = url::Url::parse(url).ok()?;
    let scheme = u.scheme().to_string();
    let host = match u.host() {
        Some(url::Host::Ipv6(a)) => format!("[{a}]"),
        Some(h) => h.to_string(),
        None => String::new(),
    };
    // nsMixedContentBlocker::IsPotentiallyTrustworthyOrigin: https, file, and loopback over http
    let loopback = host == "localhost"
        || host.ends_with(".localhost")
        || host == "[::1]"
        || host.parse::<std::net::Ipv4Addr>().is_ok_and(|a| a.is_loopback());
    let trustworthy = scheme == "https" || scheme == "wss" || scheme == "file" || (scheme == "http" && loopback);
    Some(HostUri { scheme, host, file_path: u.path().to_string(), trustworthy })
}

fn scheme_type(scheme: &str) -> i64 {
    match scheme {
        "https" => SCHEME_HTTPS,
        "http" => SCHEME_HTTP,
        "file" => SCHEME_FILE,
        _ => 0,
    }
}

/// CookieCommons::GetBaseDomain for the document's host: its registrable domain by the public
/// suffix list, and whether the host must match exactly: an IP address, or a host with no
/// registrable domain (localhost, or a public suffix itself such as github.io), keyed by the host.
fn base_domain(h: &HostUri) -> (String, bool) {
    let ip = h.host.starts_with('[') || h.host.parse::<std::net::Ipv4Addr>().is_ok();
    match psl::domain_str(&h.host) {
        Some(base) if !ip => (base.to_string(), false),
        _ => (h.host.clone(), true),
    }
}

fn is_subdomain_of(a: &str, b: &str) -> bool {
    a == b || (a.len() > b.len() && a.ends_with(b) && a.as_bytes()[a.len() - b.len() - 1] == b'.')
}

fn domain_matches(c: &Cookie, host: &str) -> bool {
    c.raw_host() == host || (c.is_domain() && host.ends_with(&c.host))
}

fn path_matches(cookie_path: &str, path: &str) -> bool {
    if cookie_path.is_empty() {
        return false;
    }
    if cookie_path == path {
        return true;
    }
    let prefix = path.starts_with(cookie_path);
    if prefix && cookie_path.ends_with('/') {
        return true;
    }
    prefix && path.as_bytes().get(cookie_path.len()) == Some(&b'/')
}

// ---- the jar ----

fn row(r: &rusqlite::Row) -> rusqlite::Result<Cookie> {
    Ok(Cookie {
        name: r.get(0)?,
        value: r.get(1)?,
        host: r.get(2)?,
        path: r.get(3)?,
        expiry_ms: r.get(4)?,
        last_accessed: r.get(5)?,
        creation: r.get(6)?,
        secure: r.get::<_, i64>(7)? != 0,
        http_only: r.get::<_, i64>(8)? != 0,
        same_site: r.get(9)?,
        scheme_map: r.get(10)?,
        partitioned_attr: r.get::<_, i64>(11)? != 0,
        session: r.get::<_, i64>(12)? != 0,
    })
}

const COLUMNS: &str = "name, value, host, path, expiry, last_accessed, creation, secure, http_only, same_site, scheme_map, partitioned_attr, session";

fn bucket_cookies(store: &Store, bucket: i64) -> Vec<Cookie> {
    let Ok(mut st) = store.conn.prepare_cached(&format!("SELECT {COLUMNS} FROM cookies WHERE bucket = ?1 ORDER BY creation")) else {
        return Vec::new();
    };
    st.query_map([bucket], row).map(|it| it.flatten().collect()).unwrap_or_default()
}

fn insert(store: &Store, bucket: i64, c: &Cookie) {
    let _ = store.conn.execute(
        &format!("INSERT OR REPLACE INTO cookies (bucket, {COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"),
        params![bucket, c.name, c.value, c.host, c.path, c.expiry_ms, c.last_accessed, c.creation, c.secure as i64,
                c.http_only as i64, c.same_site, c.scheme_map, c.partitioned_attr as i64, c.session as i64],
    );
}

fn remove(store: &Store, bucket: i64, c: &Cookie) {
    let _ = store.conn.execute(
        "DELETE FROM cookies WHERE bucket = ?1 AND host = ?2 AND name = ?3 AND path = ?4",
        params![bucket, c.host, c.name, c.path],
    );
}

// ---- reading ----

/// Document::GetCookie (`http` false), or the Cookie header of a request (CookieService::
/// GetCookieStringFromHttp, `http` true: HttpOnly cookies too). `foreign`: the request is cross-site
/// (a document's: it is a third party), which takes SameSite=None (or unset) cookies only.
fn get(store: &Store, h: &HostUri, http: bool, foreign: bool) -> String {
    let same_site = |s: i64| !foreign || s == SAMESITE_NONE || s == SAMESITE_UNSET;
    get_in(store, h, http, top_partition(store, foreign), same_site)
}

/// `get` with the partition and the SameSite test given apart: a top-level navigation's request is
/// first-party in its own store, but its initiator may be another site (for_navigation)
fn get_in(store: &Store, h: &HostUri, http: bool, partition: bool, same_site: impl Fn(i64) -> bool) -> String {
    let third_party = !store.first_party();
    let now = now_us();
    let now_ms = now / 1000;
    // the document's principal, and for a first party its CHIPS partition too
    let buckets: &[i64] = if partition { &[1] } else if third_party { &[0] } else { &[0, 1] };
    let mut list: Vec<(i64, Cookie)> = Vec::new();
    for &bucket in buckets {
        for c in bucket_cookies(store, bucket) {
            if c.expired(now_ms) || !domain_matches(&c, &h.host) || (c.http_only && !http) {
                continue;
            }
            // ShouldIncludeCrossSiteCookie: a third party gets SameSite=None (or unset) cookies only
            if !same_site(c.same_site) {
                continue;
            }
            if c.secure && !h.trustworthy {
                continue;
            }
            if !path_matches(&c.path, &h.file_path) {
                continue;
            }
            // network.cookie.CHIPS.affectsTCP false: the partition gives only cookies set Partitioned
            if bucket == 1 && !partition && !c.partitioned_attr {
                continue;
            }
            list.push((bucket, c));
        }
    }
    for (bucket, c) in &list {
        if now - c.last_accessed > STALE_THRESHOLD_US {
            let _ = store.conn.execute(
                "UPDATE cookies SET last_accessed = ?1 WHERE bucket = ?2 AND host = ?3 AND name = ?4 AND path = ?5",
                params![now, bucket, c.host, c.name, c.path],
            );
        }
    }
    // CompareCookiesForSending: the longest path first, then the oldest
    list.sort_by(|(_, a), (_, b)| b.path.len().cmp(&a.path.len()).then(a.creation.cmp(&b.creation)));
    let mut out = String::new();
    for (_, c) in &list {
        if c.name.is_empty() && c.value.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out += "; ";
        }
        if c.name.is_empty() {
            out += &c.value;
        } else {
            out += &c.name;
            out.push('=');
            out += &c.value;
        }
    }
    out
}

// ---- writing ----

/// CookieParser::GetTokenValue over the bytes from `i`: the token, the value (None: no '='), and
/// where the next pair starts.
fn token_value(s: &[u8], mut i: usize) -> (&[u8], &[u8], bool, usize) {
    let ws = |c: u8| c == b' ' || c == b'\t';
    while i < s.len() && ws(s[i]) {
        i += 1;
    }
    let start = i;
    while i < s.len() && s[i] != b';' && s[i] != b'=' {
        i += 1;
    }
    let mut last = i;
    while last > start && ws(s[last - 1]) {
        last -= 1;
    }
    let token = &s[start..last];
    let equals = i < s.len() && s[i] == b'=';
    let mut value: &[u8] = &s[i..i];
    if equals {
        i += 1;
        while i < s.len() && ws(s[i]) {
            i += 1;
        }
        let vstart = i;
        while i < s.len() && s[i] != b';' {
            i += 1;
        }
        let mut vend = i;
        while vend > vstart && ws(s[vend - 1]) {
            vend -= 1;
        }
        value = &s[vstart..vend];
    }
    if i < s.len() {
        i += 1;
    }
    (token, value, equals, i)
}

fn control_chars(b: &[u8]) -> bool {
    b.iter().any(|&c| (c <= 0x1f && c != 0x09) || c == 0x7f)
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// What ParseAttributes leaves for GetExpiry
struct Parsed {
    cookie: Cookie,
    expires: String,
    maxage: String,
}

/// CookieParser::ParseAttributes; None: rejected (a control character in an attribute).
fn parse_attributes(s: &[u8]) -> Option<Parsed> {
    let mut c = Cookie {
        name: String::new(),
        value: String::new(),
        host: String::new(),
        path: String::new(),
        expiry_ms: i64::MAX,
        last_accessed: 0,
        creation: 0,
        secure: false,
        http_only: false,
        same_site: SAMESITE_UNSET,
        scheme_map: 0,
        partitioned_attr: false,
        session: true,
    };
    let (token, value, equals, mut i) = token_value(s, 0);
    if equals {
        c.name = lossy(token);
        c.value = lossy(value);
    } else {
        // network.cookie.valueless_cookie: a token without '=' is a name with an empty value
        c.name = lossy(token);
    }
    let mut expires = String::new();
    let mut maxage = String::new();
    // CheckAttributeSize: an attribute over 1024 bytes is ignored
    let fits = |v: &[u8]| v.len() <= MAX_BYTES_PER_ATTRIBUTE;
    while i < s.len() {
        let (token, value, _, next) = token_value(s, i);
        i = next;
        if control_chars(token) || control_chars(value) {
            return None;
        }
        let t = token.to_ascii_lowercase();
        match t.as_slice() {
            b"path" => {
                if fits(value) {
                    c.path = lossy(value)
                }
            }
            b"domain" => {
                if fits(value) {
                    c.host = lossy(value)
                }
            }
            b"expires" => {
                if fits(value) {
                    expires = lossy(value)
                }
            }
            b"max-age" => {
                if fits(value) {
                    maxage = lossy(value)
                }
            }
            b"secure" => c.secure = true,
            b"partitioned" => c.partitioned_attr = true,
            b"httponly" => c.http_only = true,
            b"samesite" => {
                c.same_site = match value.to_ascii_lowercase().as_slice() {
                    b"lax" => SAMESITE_LAX,
                    b"strict" => SAMESITE_STRICT,
                    b"none" => SAMESITE_NONE,
                    _ => SAMESITE_UNSET,
                }
            }
            _ => {}
        }
    }
    Some(Parsed { cookie: c, expires, maxage })
}

/// CookieParser::GetExpiry: whether the cookie is a session cookie; sets its expiry.
fn get_expiry(c: &mut Cookie, expires: &str, maxage: &str) -> bool {
    let creation_ms = c.creation / 1000;
    if !maxage.is_empty() {
        let b = maxage.as_bytes();
        let parsed = if b[0] == b'-' {
            Some(i64::MIN)
        } else if b.iter().all(u8::is_ascii_digit) {
            Some(maxage.parse::<i64>().unwrap_or(i64::MAX))
        } else {
            None
        };
        if let Some(v) = parsed {
            c.expiry_ms = if v == i64::MIN {
                v
            } else {
                // MaybeCapMaxAge
                creation_ms.checked_add(v.min(MAXAGE_CAP_S) * 1000).unwrap_or(i64::MAX)
            };
            return false;
        }
    }
    if !expires.is_empty() {
        let Some(us) = crate::prtime::parse_time_string(expires.as_bytes()) else { return true };
        // MaybeCapExpiry
        c.expiry_ms = (us / 1000).min(creation_ms + MAXAGE_CAP_S * 1000);
        return false;
    }
    true
}

/// CookieParser::FixDomain
fn fix_domain(c: &mut Cookie, h: &HostUri, base: &str, require_host_match: bool) {
    if c.host.is_empty() {
        c.host = h.host.clone();
        return;
    }
    let mut cookie_host = c.host.clone();
    if cookie_host.len() > 1 && cookie_host.starts_with('.') {
        cookie_host.remove(0);
    }
    cookie_host.make_ascii_lowercase();
    if require_host_match {
        if h.host == cookie_host {
            c.host = cookie_host;
        }
        return;
    }
    if is_subdomain_of(&cookie_host, base) && is_subdomain_of(&h.host, &cookie_host) {
        c.host = format!(".{cookie_host}");
    }
}

/// CookieParser's GetPathFromURI: the directory of the document's path, without its last '/'
/// unless it is the only one.
fn default_path(h: &HostUri) -> String {
    let mut path = match h.file_path.rfind('/') {
        Some(i) => h.file_path[..=i].to_string(),
        None => String::new(),
    };
    let last = path.rfind('/');
    let first = path.find('/');
    if last != first && last.is_some() && last == Some(path.len() - 1) {
        path.pop();
    }
    path
}

fn has_prefix(name: &str, prefix: &str) -> bool {
    name.len() >= prefix.len() && name.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// CookiePrefixes::Check
fn prefixes_ok(c: &Cookie, secure_request: bool) -> bool {
    let host_ok = !c.host.starts_with('.') && c.path == "/";
    if has_prefix(&c.name, "__Host-Http-") {
        return secure_request && c.secure && c.http_only && host_ok;
    }
    if has_prefix(&c.name, "__Host-") {
        return secure_request && c.secure && host_ok;
    }
    if has_prefix(&c.name, "__Http-") {
        return secure_request && c.secure && c.http_only;
    }
    if has_prefix(&c.name, "__Secure-") {
        return secure_request && c.secure;
    }
    true
}

/// CookieValidation::ValidateInContext (from script, or from a response's Set-Cookie): whether the
/// cookie may be stored.
fn validate(c: &Cookie, h: &HostUri, base: &str, require_host_match: bool, foreign: bool, from_http: bool) -> bool {
    // ValidateInternal
    if c.name.is_empty() && c.value.is_empty() {
        return false;
    }
    if c.name.len() + c.value.len() > MAX_BYTES_PER_COOKIE {
        return false;
    }
    let name = c.name.as_bytes();
    if name.first() == Some(&0x20) || name.last() == Some(&0x20) || name.is_empty() {
        return false;
    }
    if name.iter().any(|&b| (b < 0x20 && b != 0x09) || b == 0x3b || b == 0x3d || b == 0x7f) {
        return false;
    }
    let value = c.value.as_bytes();
    if value.first() == Some(&0x20) || value.last() == Some(&0x20) {
        return false;
    }
    if value.iter().any(|&b| (b < 0x20 && b != 0x09) || b == 0x3b || b == 0x7f) {
        return false;
    }
    if c.path.len() > MAX_BYTES_PER_ATTRIBUTE || c.path.contains('\t') || c.host.len() > MAX_BYTES_PER_ATTRIBUTE {
        return false;
    }
    if !c.secure && c.same_site == SAMESITE_NONE {
        return false;
    }
    if !c.session && c.expiry_ms > c.creation / 1000 + MAXAGE_CAP_S * 1000 {
        return false;
    }
    // ValidateForHost: CheckDomain
    if !base.is_empty() {
        if c.host.is_empty() {
            return false;
        }
        let ok = if require_host_match {
            h.host == c.host
        } else {
            let ch = if c.host.len() > 1 && c.host.starts_with('.') { &c.host[1..] } else { &c.host[..] };
            is_subdomain_of(ch, base) && is_subdomain_of(&h.host, ch)
        };
        if !ok {
            return false;
        }
    }
    if c.scheme_map & SCHEME_FILE != 0 && !c.host.is_empty() {
        return false;
    }
    // from script
    if c.http_only && !from_http {
        return false;
    }
    if !prefixes_ok(c, h.trustworthy) {
        return false;
    }
    if c.secure && !h.trustworthy {
        return false;
    }
    // ValidateInContext: a foreign context takes SameSite=None (unset is None: not lax by default)
    let same_site = if c.same_site == SAMESITE_UNSET { SAMESITE_NONE } else { c.same_site };
    if same_site != SAMESITE_NONE && foreign {
        return false;
    }
    if c.partitioned_attr && !c.secure {
        return false;
    }
    true
}

/// Document::SetCookie: CookieCommons::CreateCookieFromDocument, then CookieStorage::AddCookie; or
/// one line of a response's Set-Cookie (`from_http`, CookieService::SetCookieStringFromHttp).
fn set(store: &Store, h: &HostUri, s: &str, from_http: bool, foreign: bool) {
    if !matches!(h.scheme.as_str(), "http" | "https" | "file") {
        return;
    }
    let bytes = s.as_bytes();
    // CheckCookieStringFromDocument: a control character other than HTAB drops the string
    if !from_http && bytes.iter().any(|&c| (c < 0x20 && c != 0x09) || c == 0x7f) {
        return;
    }
    let (base, require_host_match) = base_domain(h);
    let now = now_us();
    let Some(Parsed { mut cookie, expires, maxage }) = parse_attributes(bytes) else { return };
    cookie.creation = unique_creation(now);
    cookie.scheme_map = scheme_type(&h.scheme);
    cookie.session = get_expiry(&mut cookie, &expires, &maxage);
    fix_domain(&mut cookie, h, &base, require_host_match);
    if !validate(&cookie, h, &base, require_host_match, foreign, from_http) {
        return;
    }
    // FixPath, after the validation (__Host- is about the Path attribute as written)
    if cookie.path.is_empty() || !cookie.path.starts_with('/') {
        let p = default_path(h);
        if p.len() <= MAX_BYTES_PER_ATTRIBUTE {
            cookie.path = p;
        }
    }
    cookie.last_accessed = now;
    // CreateCookieFromDocument takes a later creation time than the parser's
    cookie.creation = unique_creation(now);
    // a first party's Partitioned cookie goes to its CHIPS partition
    let bucket = if top_partition(store, foreign) || (store.first_party() && cookie.partitioned_attr) { 1 } else { 0 };
    add_cookie(store, bucket, cookie, h.trustworthy, now, from_http);
}

/// Whether a foreign request to the top-level site's own jar uses that site's partition instead: a
/// request of a cross-site frame back to the top-level site (B in A fetching A). Its channel is a
/// third party with the frame document's partition key, (scheme, A), which is bucket 1 of A's
/// first-party store; every cookie there is sent and every cookie set goes there (checked against
/// Firefox by the fetch-xframe case, GAPS B162).
fn top_partition(store: &Store, foreign: bool) -> bool {
    foreign && store.first_party()
}

/// A cookie from script, as the document sees it: CookieServiceChild::AddCookieFromDocument and
/// RecordDocumentCookie (a changed cookie is replaced by the new one, which sorts as the newest),
/// with the parent's CookieStorage::AddCookie checks that reject it (a non-secure cookie over a
/// secure one) and its eviction, which a page sees a moment later in Firefox and here at once.
/// A response's cookie (`from_http`) is the parent's: it may replace an HttpOnly one, and a
/// replacement keeps the old cookie's creation time, which the document's copy then gets too.
fn add_cookie(store: &Store, bucket: i64, mut cookie: Cookie, trustworthy: bool, now: i64, from_http: bool) {
    let now_ms = now / 1000;
    let cookies = bucket_cookies(store, bucket);
    let exact = cookies.iter().find(|c| c.host == cookie.host && c.name == cookie.name && c.path == cookie.path).cloned();
    let found_secure_exact = exact.as_ref().is_some_and(|c| c.secure);
    // FindSecureCookie: a non-secure cookie from an insecure page may not shadow a secure one
    let secure_overlay = cookies.iter().any(|c| {
        c.secure
            && c.name == cookie.name
            && (domain_matches(c, &cookie.host) || domain_matches(&cookie, &c.host))
            && path_matches(&c.path, &cookie.path)
    });
    if !cookie.secure && (found_secure_exact || secure_overlay) && !trustworthy {
        return;
    }
    let count = cookies.len();
    if let Some(old) = exact {
        if old.expired(now_ms) {
            if cookie.expired(now_ms) {
                return;
            }
            // a stale cookie is removed and the new one added as if there had been none
            remove(store, bucket, &old);
            insert(store, bucket, &cookie);
            return;
        } else {
            if old.http_only && !from_http {
                return;
            }
            if old.value == cookie.value
                && old.expiry_ms == cookie.expiry_ms
                && old.secure == cookie.secure
                && old.session == cookie.session
                && old.http_only == cookie.http_only
                && old.same_site == cookie.same_site
            {
                let _ = store.conn.execute(
                    "UPDATE cookies SET last_accessed = ?1 WHERE bucket = ?2 AND host = ?3 AND name = ?4 AND path = ?5",
                    params![cookie.last_accessed, bucket, old.host, old.name, old.path],
                );
                return;
            }
            remove(store, bucket, &old);
            if cookie.expired(now_ms) {
                return;
            }
            if from_http {
                cookie.creation = old.creation;
            }
            insert(store, bucket, &cookie);
            return;
        }
    }
    if cookie.expired(now_ms) {
        return;
    }
    // too many cookies for the base domain: the expired and the least recently used go, insecure
    // ones first (FindStaleCookies)
    if count >= MAX_PER_HOST {
        let excess = count - MAX_PER_HOST + 1;
        let limit = MAX_PER_HOST - QUOTA_PER_HOST + excess;
        let pick = |secure_too: bool| -> Vec<Cookie> {
            let mut c: Vec<Cookie> = bucket_cookies(store, bucket)
                .into_iter()
                .filter(|c| c.expired(now_ms) || secure_too || !c.secure)
                .collect();
            c.sort_by(|a, b| {
                b.expired(now_ms).cmp(&a.expired(now_ms)).then(a.last_accessed.cmp(&b.last_accessed)).then(a.creation.cmp(&b.creation))
            });
            c.truncate(limit);
            c
        };
        let mut evict = pick(false);
        if evict.is_empty() {
            if !cookie.secure {
                return;
            }
            evict = pick(true);
        }
        for c in evict {
            remove(store, bucket, &c);
        }
    }
    insert(store, bucket, &cookie);
}

// ---- the tab's jar (until fetch moves, Phase 6.3) ----

/// Firefox's cookies for the partition replace this jar's, as its notifications update a content
/// process's copy: a cookie the same in both stays as it is here (the document's creation time,
/// where the parent kept the old one on an overwrite), one Firefox changed or added is taken, one
/// it no longer has (evicted, expired, removed by a response) goes. The number of rows that differed.
fn sync(store: &Store, cookies: &[Value]) -> usize {
    let mut theirs: Vec<(i64, Cookie)> = Vec::new();
    for v in cookies {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        // (a session cookie's expiry is 2^63 - 1, a JS number of 2^63 on the way here)
        let n = |k: &str| v.get(k).and_then(Value::as_f64).map(|f| if f >= i64::MAX as f64 { i64::MAX } else { f as i64 }).unwrap_or(0);
        let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
        let Some(bucket) = store.bucket_of(&s("partitionKey")) else { continue };
        theirs.push((
            bucket,
            Cookie {
                name: s("name"),
                value: s("value"),
                host: s("host"),
                path: s("path"),
                expiry_ms: n("expiry"),
                last_accessed: n("lastAccessed"),
                creation: n("creationTime"),
                secure: b("isSecure"),
                http_only: b("isHttpOnly"),
                same_site: n("sameSite"),
                scheme_map: n("schemeMap"),
                // the cookie manager does not say; Firefox puts only Partitioned cookies there
                partitioned_attr: bucket == 1,
                session: b("isSession"),
            },
        ));
    }
    let key = |b: i64, c: &Cookie| (b, c.host.clone(), c.name.clone(), c.path.clone());
    let mut ours: std::collections::HashMap<_, Cookie> = std::collections::HashMap::new();
    for bucket in [0, 1] {
        for c in bucket_cookies(store, bucket) {
            ours.insert(key(bucket, &c), c);
        }
    }
    let mut changed = 0;
    let now_ms = now_us() / 1000;
    let mut seen = std::collections::HashSet::new();
    for (b, c) in &theirs {
        let k = key(*b, c);
        seen.insert(k.clone());
        match ours.get(&k) {
            Some(o) if o.value == c.value && o.secure == c.secure && o.http_only == c.http_only && o.same_site == c.same_site
                && o.session == c.session && o.expiry_ms == c.expiry_ms => {}
            _ => {
                changed += 1;
                insert(store, *b, c);
            }
        }
    }
    for (k, o) in &ours {
        if !seen.contains(k) {
            if !o.expired(now_ms) {
                changed += 1;
            }
            remove(store, k.0, o);
        }
    }
    changed
}

pub fn handle(req: &Value, store: &Store) -> Result<Value, String> {
    if req.get("call").and_then(Value::as_str) == Some("sync") {
        let list = req.get("cookies").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        return Ok(json!({ "changed": sync(store, list) }));
    }
    let url = req.get("url").and_then(Value::as_str).unwrap_or("");
    // a cookie-averse document (no http, https or file principal): "" and no cookie set
    let h = host_uri(url).filter(|h| matches!(h.scheme.as_str(), "http" | "https" | "file"));
    match req.get("value") {
        Some(v) => {
            if let Some(h) = &h {
                set(store, h, v.as_str().unwrap_or(""), false, !store.first_party());
            }
            Ok(Value::Null)
        }
        None => Ok(Value::String(h.map(|h| get(store, &h, false, !store.first_party())).unwrap_or_default())),
    }
}

// ---- a request's cookies (Phase 6.3: fetch in the API process) ----

/// The Cookie header of a request to `url` in this partition ("" for none); `foreign`: the request
/// is cross-site for its top-level site (SameSite=None and unset cookies only).
pub fn for_request(store: &Store, url: &str, foreign: bool) -> String {
    match host_uri(url).filter(|h| matches!(h.scheme.as_str(), "http" | "https")) {
        Some(h) => get(store, &h, true, foreign),
        None => String::new(),
    }
}

/// The Cookie header of a top-level navigation's request to `url` (the hop's own store, first-party;
/// CookieCommons::IsSameSiteForeign for a document load): SameSite=Strict cookies only when the
/// initiator and every hop so far are same-site (`same_site`), Lax ones also for a cross-site GET or
/// HEAD (`safe_method`), None and unset ones always.
pub fn for_navigation(store: &Store, url: &str, same_site: bool, safe_method: bool) -> String {
    let allowed = |s: i64| same_site || s == SAMESITE_NONE || s == SAMESITE_UNSET || (safe_method && s == SAMESITE_LAX);
    match host_uri(url).filter(|h| matches!(h.scheme.as_str(), "http" | "https")) {
        Some(h) => get_in(store, &h, true, false, allowed),
        None => String::new(),
    }
}

/// A response's Set-Cookie headers for `url`, each line a cookie, stored as Firefox's parent does.
pub fn from_response(store: &Store, url: &str, lines: &[String], foreign: bool) {
    let Some(h) = host_uri(url).filter(|h| matches!(h.scheme.as_str(), "http" | "https")) else { return };
    for header in lines {
        for line in header.split('\n') {
            if !line.trim().is_empty() {
                set(store, &h, line, true, foreign);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(url: &str) -> (String, bool) {
        base_domain(&host_uri(url).unwrap())
    }

    #[test]
    fn require_host_match_by_public_suffix() {
        assert_eq!(domain("https://www.example.co.uk/"), ("example.co.uk".into(), false));
        assert_eq!(domain("https://a.github.io/"), ("a.github.io".into(), false));
        // a public suffix itself, a single label, an IP address: the host, matched exactly
        assert_eq!(domain("https://github.io/"), ("github.io".into(), true));
        assert_eq!(domain("http://localhost:8000/"), ("localhost".into(), true));
        assert_eq!(domain("http://127.0.0.1/"), ("127.0.0.1".into(), true));
        assert_eq!(domain("http://[::1]/"), ("[::1]".into(), true));
    }
}
