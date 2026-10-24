// The requesting document's policies over a request (GAPS B162): what Firefox's content policies and
// channel do with the document's CSP, referrer policy and embedder policy before and while the
// request goes out. The driver reads them from the document's response (mini_realsites.py
// doc_policies) and sends them with each request as `doc`; bridge.js adds the <meta name=referrer>
// policy as `metaReferrer`.
//
// `doc`: {csp: [{reportOnly, connect, default, upgrade}], referrerPolicy, coep}, where connect and
// default are the connect-src and default-src tokens as nsCSPParser kept them (quoted keywords
// lowercased), or null when the policy has no such directive.
//
// Ported from Gecko: nsCSPParser's source expressions and nsCSPUtils.cpp's permits (permitsScheme,
// permitsPort, nsCSPHostSrc::permits), nsCSPPolicy::permits's fallback to default-src,
// nsMixedContentBlocker::ShouldLoad for a fetch, NS_ShouldSecureUpgrade and NS_GetSecureUpgradedURI.

use serde_json::Value;
use url::Url;

/// One source expression of a source list
#[derive(Debug, PartialEq)]
enum Source {
    /// `https:`
    Scheme(String),
    /// `[scheme://]host[:port][/path]`, the scheme the document's when none is written; `self_`:
    /// made from 'self', which also allows ws and wss
    Host { scheme: String, host: String, port: String, path: String, self_: bool },
    /// 'none', a nonce, a hash or another keyword: allows no load from the network
    Nothing,
}

/// One of the document's policies, as far as a fetch is concerned
pub struct Policy {
    report_only: bool,
    connect: Option<Vec<Source>>,
    default: Option<Vec<Source>>,
    upgrade: bool,
}

/// What the request's document set
#[derive(Default)]
pub struct Doc {
    pub policies: Vec<Policy>,
    /// the Referrer-Policy header's (ReferrerPolicyFromHeaderString: the last valid token), or ""
    pub referrer_policy: String,
    /// "require-corp", "credentialless" or ""
    pub coep: String,
}

fn is_scheme(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic()) && c.all(|ch| ch.is_ascii_alphanumeric() || "+-.".contains(ch))
}

/// CSP_PercentDecodeStr
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
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
    String::from_utf8_lossy(&out).into_owned()
}

/// nsCSPParser::sourceExpression for one token; `self_url` the document's origin. None for a token
/// the parser drops.
fn source(token: &str, self_url: Option<&Url>) -> Option<Source> {
    if token.starts_with('\'') {
        if token == "'self'" {
            let u = self_url?;
            // CSP_CreateHostSrcFromSelfURI: a document without a host (data:) is a unique origin
            let host = u.host_str().unwrap_or("").to_ascii_lowercase();
            if host.is_empty() {
                return Some(Source::Nothing);
            }
            let port = u.port().map(|p| p.to_string()).unwrap_or_default();
            return Some(Source::Host { scheme: u.scheme().into(), host, port, path: String::new(), self_: true });
        }
        return Some(Source::Nothing);
    }
    if token == "*" {
        return Some(Source::Host { scheme: String::new(), host: "*".into(), port: String::new(), path: String::new(), self_: false });
    }
    let (scheme, rest) = match token.split_once(':') {
        Some((s, r)) if is_scheme(s) && (r.is_empty() || r.starts_with("//")) => {
            if r.is_empty() {
                return Some(Source::Scheme(s.to_ascii_lowercase()));
            }
            (s.to_ascii_lowercase(), &r[2..])
        }
        _ => (self_url.map(|u| u.scheme().to_string()).unwrap_or_default(), token),
    };
    // host [":" port] [path]
    let host_end = rest.find([':', '/']).unwrap_or(rest.len());
    let host = rest[..host_end].to_ascii_lowercase();
    let valid_label = |l: &str| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let host_ok = host == "*" || {
        let h = host.strip_prefix("*.").unwrap_or(&host);
        h.split('.').all(valid_label)
    };
    if !host_ok {
        return None;
    }
    let mut after = &rest[host_end..];
    let mut port = String::new();
    if let Some(p) = after.strip_prefix(':') {
        let end = p.find('/').unwrap_or(p.len());
        port = p[..end].to_string();
        if !(port == "*" || (!port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))) {
            return None;
        }
        after = &p[end..];
    }
    // the path ends at a query or fragment
    let path = after.split(['?', '#']).next().unwrap_or("");
    Some(Source::Host { scheme, host, port, path: percent_decode(path), self_: false })
}

fn sources(v: &Value, self_url: Option<&Url>) -> Option<Vec<Source>> {
    let list = v.as_array()?;
    Some(list.iter().filter_map(Value::as_str).filter_map(|t| source(t, self_url)).collect())
}

/// permitsScheme
fn permits_scheme(enforced: &str, u: &Url, report_only: bool, upgrade: bool, from_self: bool) -> bool {
    let scheme = u.scheme();
    if enforced.is_empty() || enforced == scheme {
        return true;
    }
    if enforced == "http" && (scheme == "https" || (matches!(scheme, "ws" | "wss") && from_self)) {
        return true;
    }
    if enforced == "https" && scheme == "wss" && from_self {
        return true;
    }
    if enforced == "ws" && scheme == "wss" {
        return true;
    }
    upgrade && !report_only && ((scheme == "http" && enforced == "https") || (scheme == "ws" && enforced == "wss"))
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        "ftp" => Some(21),
        _ => None,
    }
}

/// permitsPort
fn permits_port(scheme: &str, port: &str, u: &Url) -> bool {
    if port == "*" {
        return true;
    }
    if u.port().is_none() && port.is_empty() {
        return true;
    }
    let Some(resource) = u.port_or_known_default() else { return port.is_empty() && default_port(scheme).is_none() };
    let resource = resource.to_string();
    if port == resource {
        return true;
    }
    let enforced = if port.is_empty() { default_port(scheme).map(|p| p.to_string()).unwrap_or_default() } else { port.to_string() };
    enforced == resource || (enforced == "80" && resource == "443")
}

impl Source {
    /// nsCSPSchemeSrc::permits, nsCSPHostSrc::permits
    fn permits(&self, u: &Url, redirected: bool, report_only: bool, upgrade: bool) -> bool {
        match self {
            Source::Nothing => false,
            Source::Scheme(s) => permits_scheme(s, u, report_only, upgrade, false),
            Source::Host { scheme, host, port, path, self_ } => {
                if !permits_scheme(scheme, u, report_only, upgrade, *self_) {
                    return false;
                }
                let uri_host = percent_decode(u.host_str().unwrap_or(""));
                if host == "*" {
                    if matches!(u.scheme(), "blob" | "data" | "filesystem") {
                        return false;
                    }
                    if scheme.is_empty() {
                        return true;
                    }
                } else if let Some(suffix) = host.strip_prefix('*') {
                    if !uri_host.ends_with(suffix) {
                        return false;
                    }
                } else if *host != uri_host {
                    return false;
                }
                if !permits_port(scheme, port, u) {
                    return false;
                }
                if !redirected && !path.is_empty() {
                    let file = percent_decode(u.path());
                    if path.ends_with('/') {
                        if !file.starts_with(path.as_str()) {
                            return false;
                        }
                    } else if file != *path {
                        return false;
                    }
                }
                true
            }
        }
    }
}

impl Doc {
    /// The request's `doc` field; `origin` the requesting global's (the 'self' of a policy)
    pub fn from_request(req: &Value, origin: &str) -> Doc {
        let Some(d) = req.get("doc").filter(|d| d.is_object()) else { return Doc::default() };
        let self_url = Url::parse(origin).ok();
        let policies = d
            .get("csp")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|p| Policy {
                report_only: p.get("reportOnly").and_then(Value::as_bool).unwrap_or(false),
                connect: p.get("connect").and_then(|v| sources(v, self_url.as_ref())),
                default: p.get("default").and_then(|v| sources(v, self_url.as_ref())),
                upgrade: p.get("upgrade").and_then(Value::as_bool).unwrap_or(false),
            })
            .collect();
        let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Doc { policies, referrer_policy: s("referrerPolicy"), coep: s("coep") }
    }

    /// The loadInfo's UpgradeInsecureRequests: an enforced policy has upgrade-insecure-requests
    pub fn upgrades(&self) -> bool {
        self.policies.iter().any(|p| p.upgrade && !p.report_only)
    }

    /// nsCSPContext::ShouldLoad for a fetch, XHR or beacon (connect-src, else default-src): whether
    /// every enforced policy allows `u`; `redirected`: the URL a redirect led to (paths not checked)
    pub fn connect_allowed(&self, u: &Url, redirected: bool) -> bool {
        self.policies.iter().filter(|p| !p.report_only).all(|p| {
            match p.connect.as_ref().or(p.default.as_ref()) {
                None => true,
                Some(list) => list.iter().any(|s| s.permits(u, redirected, false, p.upgrade)),
            }
        })
    }
}

/// nsMixedContentBlocker::IsPotentiallyTrustworthyLoopbackHost and the origin's other secure schemes
pub fn trustworthy(u: &Url) -> bool {
    let loopback = match u.host() {
        Some(url::Host::Domain(d)) => d == "localhost" || d.ends_with(".localhost"),
        Some(url::Host::Ipv4(a)) => a.is_loopback(),
        Some(url::Host::Ipv6(a)) => a.is_loopback(),
        None => false,
    };
    matches!(u.scheme(), "https" | "wss" | "file" | "data") || (u.scheme() == "http" && loopback)
}

/// NS_ShouldSecureUpgrade with upgrade-insecure-requests: an http URL that is not potentially
/// trustworthy, made https (NS_GetSecureUpgradedURI: port 80 becomes the default)
pub fn upgraded(u: &Url) -> Option<Url> {
    if u.scheme() != "http" || trustworthy(u) {
        return None;
    }
    let mut v = u.clone();
    let port = v.port();
    v.set_scheme("https").ok()?;
    if port == Some(80) || port.is_none() {
        let _ = v.set_port(None);
    }
    Some(v)
}

/// nsMixedContentBlocker::ShouldLoad for a fetch (active content): an https document's request to
/// an http URL that is not potentially trustworthy is refused
pub fn mixed_content(origin: &str, u: &Url) -> bool {
    origin.starts_with("https:") && u.scheme() == "http" && !trustworthy(u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(csp: Value) -> Doc {
        Doc::from_request(&json!({ "doc": { "csp": csp } }), "https://a.example:8443")
    }
    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn connect_src_sources() {
        let d = doc(json!([{ "connect": ["'self'", "https://*.cdn.test", "api.test:8080/v1/", "data:", "http://old.test/x.json"] }]));
        assert!(d.connect_allowed(&url("https://a.example:8443/x"), false));
        assert!(d.connect_allowed(&url("wss://a.example:8443/s"), false));
        assert!(!d.connect_allowed(&url("https://a.example/x"), false));
        assert!(d.connect_allowed(&url("https://img.cdn.test/a.png"), false));
        assert!(!d.connect_allowed(&url("https://cdn.test/a.png"), false));
        // a scheme-less host takes the document's scheme
        assert!(d.connect_allowed(&url("https://api.test:8080/v1/items?q=1"), false));
        assert!(!d.connect_allowed(&url("https://api.test:8080/v2/items"), false));
        assert!(!d.connect_allowed(&url("http://api.test:8080/v1/items"), false));
        // a path is not checked after a redirect
        assert!(d.connect_allowed(&url("https://api.test:8080/v2/items"), true));
        assert!(d.connect_allowed(&url("data:text/plain,hi"), false));
        // http allows https; an exact file path
        assert!(d.connect_allowed(&url("https://old.test/x.json"), false));
        assert!(!d.connect_allowed(&url("http://old.test/y.json"), false));
    }

    #[test]
    fn fallback_star_none_and_report_only() {
        assert!(!doc(json!([{ "default": ["'none'"] }])).connect_allowed(&url("https://a.example:8443/"), false));
        assert!(doc(json!([{ "default": ["'none'"], "connect": ["*"] }])).connect_allowed(&url("http://x.test/"), false));
        assert!(!doc(json!([{ "connect": ["*"] }])).connect_allowed(&url("data:,x"), false));
        assert!(doc(json!([{ "reportOnly": true, "connect": ["'none'"] }])).connect_allowed(&url("https://x.test/"), false));
        // every enforced policy must allow
        assert!(!doc(json!([{ "connect": ["*"] }, { "connect": ["'self'"] }])).connect_allowed(&url("https://x.test/"), false));
        // no connect-src nor default-src: allowed
        assert!(doc(json!([{ "script": ["'none'"] }])).connect_allowed(&url("https://x.test/"), false));
    }

    #[test]
    fn upgrade_and_mixed_content() {
        let d = doc(json!([{ "connect": ["https://x.test"], "upgrade": true }]));
        assert!(d.upgrades());
        assert!(d.connect_allowed(&url("http://x.test/a"), false));
        assert_eq!(upgraded(&url("http://x.test:80/a")).unwrap().as_str(), "https://x.test/a");
        assert_eq!(upgraded(&url("http://x.test:81/a")).unwrap().as_str(), "https://x.test:81/a");
        assert!(upgraded(&url("http://localhost:81/a")).is_none());
        assert!(mixed_content("https://a.test", &url("http://x.test/")));
        assert!(!mixed_content("https://a.test", &url("http://127.0.0.1:9/")));
        assert!(!mixed_content("http://a.test", &url("http://x.test/")));
    }

    #[test]
    fn ports() {
        let d = Doc::from_request(&json!({ "doc": { "csp": [{ "connect": ["http://p.test:80", "q.test:*"] }] } }), "http://a.test");
        assert!(d.connect_allowed(&url("https://p.test/"), false));
        assert!(d.connect_allowed(&url("http://q.test:1234/"), false));
        assert!(!d.connect_allowed(&url("http://p.test:81/"), false));
    }
}
