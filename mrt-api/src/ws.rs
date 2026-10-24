// WebSocket (Phase 6.4, DESIGN "Phase 6"): the runtime's connections, made here instead of in the
// bridge tab. The calls are the bridge tab's (mini_realsites.py WS_SCRIPT), so bridge.js's WebSocket
// is the same either way: `open` (the connection made, or its constructor's exception), `poll` (the
// events of some connections since the last poll), `send` and `close`. Each event carries its time
// since the connection was made (`ms`), which the runtime delivers on its clock.
//
// What Firefox does after the bindings, this does: WebSocketImpl's checks (the protocol list, then
// ParseURL, then the document's CSP connect-src and upgrade-insecure-requests, then the
// mixed-content refusal of InitializeConnection), nsWSAdmissionManager (one handshake at a time to
// an address, and a growing delay before reconnecting to one that failed), and WebSocketChannel's
// handshake (Origin, the cookies of the URL's site in the page's partition, permessage-deflate
// offered and its answer checked) and framing (the page's messages deflated when it was taken). A
// failed handshake (a replay refuses every WebSocket with a 404) is an error and a close 1006; a
// connection the network ends without a close frame is a close 1006 alone.
//
// One thread per connection reads its frames and keeps its events; the page's sends and closes go
// to it through a channel. The 101's Set-Cookie headers come back to the main thread, which stores
// them (take_cookies) before it serves the next request. Firefox sends no pings of its own
// (network.websocket.timeout.ping.request is 0); it answers the server's, as this does.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use url::Url;

use crate::cookie;
use crate::policy::Doc;
use crate::store::{Stores, site_of};

/// mini_realsites.py PENDING_MS: `open` waits this long for the connection's first event
const PENDING: Duration = Duration::from_millis(5000);
/// the longest a poll waits for the network to catch up with the page's clock
const POLL_WAIT: Duration = Duration::from_millis(250);
/// how long a reader thread waits for bytes before it looks for the page's sends again
const TICK: Duration = Duration::from_millis(10);
/// network.websocket.timeout.open: from the admission to the 101
const OPEN_TIMEOUT: Duration = Duration::from_secs(20);
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const SYNTAX_ERR: &str = "An invalid or illegal string was specified";
const SECURITY_ERR: &str = "The operation is insecure.";
/// NS_ERROR_CONTENT_BLOCKED and its default text, which the bindings throw as an Exception
const CONTENT_BLOCKED: u32 = 0x805E0006;
const CONTENT_BLOCKED_TEXT: &str = "The load for this content was blocked.";

/// What a connection's thread shares with the main thread
#[derive(Default)]
struct Shared {
    events: Vec<Value>,
    /// the handshake answer's Set-Cookie lines, for the main thread to store
    cookies: Option<(String, Vec<String>)>,
    /// waiting for its admission: behind another handshake to its address, or its failure delay
    queued: bool,
}

enum Cmd {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<u16>, Option<String>),
}

struct Socket {
    shared: Arc<Mutex<Shared>>,
    /// when it was made (its events' `ms` count from here)
    t0: Instant,
    tx: Sender<Cmd>,
    /// the partition its cookies go to (site, top-level site), whether it is a foreign request, and
    /// whether the partition has the foreign-ancestor bit
    store: (String, String, bool, bool),
}

/// The connections of one API process, by the id the runtime knows them by
#[derive(Default)]
pub struct Sockets {
    next: u64,
    open: std::collections::HashMap<u64, Socket>,
}

/// Where a connection goes: through the replay's proxy (an HTTP CONNECT tunnel, as Firefox takes a
/// WebSocket through an HTTP proxy), and which roots its TLS trusts
pub struct Network {
    proxy: Option<String>,
    tls: Arc<rustls::ClientConfig>,
    user_agent: String,
    accept_language: String,
}

impl Network {
    pub fn from_env() -> Network {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let mut roots = rustls::RootCertStore::empty();
        match var("API_CA") {
            Some(ca) => {
                let pem = std::fs::read(&ca).unwrap_or_default();
                use rustls_pki_types::pem::PemObject;
                for cert in rustls_pki_types::CertificateDer::pem_slice_iter(&pem).flatten() {
                    let _ = roots.add(cert);
                }
            }
            None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
        }
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("TLS versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        let proxy = var("API_PROXY").map(|p| p.trim_start_matches("http://").trim_end_matches('/').to_string());
        Network {
            proxy,
            tls: Arc::new(tls),
            user_agent: var("API_UA").unwrap_or_else(|| "Mozilla/5.0".into()),
            accept_language: var("API_ACCEPT_LANGUAGE").unwrap_or_else(|| "en-US,en;q=0.9".into()),
        }
    }
}

fn throws(name: &str, message: &str) -> Value {
    json!({ "throws": { "name": name, "message": message } })
}

/// WebSocket::IsValidProtocolString: not empty, U+0021 to U+007E, none of RFC 2616's separators
fn valid_protocol(p: &str) -> bool {
    !p.is_empty() && p.chars().all(|c| ('\u{21}'..='\u{7e}').contains(&c) && !"()<>@,;:\\\"/[]?={}".contains(c))
}

fn loopback_host(u: &Url) -> bool {
    match u.host() {
        Some(url::Host::Domain(d)) => d == "localhost" || d.ends_with(".localhost"),
        Some(url::Host::Ipv4(a)) => a.is_loopback(),
        Some(url::Host::Ipv6(a)) => a.is_loopback(),
        None => false,
    }
}

/// The `ws` op: {call: open | poll | send | close, ...}
pub fn handle(req: &Value, net: &Network, sockets: &mut Sockets, stores: &mut Stores) -> Result<Value, String> {
    let s = |k: &str| req.get(k).and_then(Value::as_str).unwrap_or("");
    match s("call") {
        "open" => sockets.open(req, net, stores),
        "poll" => {
            let ids: Vec<u64> = req.get("ids").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
            // The page's clock runs ahead of the network's when it has nothing else to do: its polls
            // come one after another. A poll that finds nothing waits until each connection has been
            // open, in real time, as long as the page's clock says (`ages`, ms), at most POLL_WAIT, so
            // that an answer the server sends at once is there for the poll that follows it.
            let ages: Vec<f64> = req.get("ages").and_then(Value::as_array).into_iter().flatten().map(|a| a.as_f64().unwrap_or(0.0)).collect();
            let waited = Instant::now();
            loop {
                let socks: Vec<(&Socket, f64)> =
                    ids.iter().zip(ages.iter().chain(std::iter::repeat(&0.0))).filter_map(|(id, age)| Some((sockets.open.get(id)?, *age))).collect();
                let any = socks.iter().any(|(s, _)| !s.shared.lock().unwrap().events.is_empty());
                let behind = socks.iter().any(|(s, age)| s.t0.elapsed().as_secs_f64() * 1000.0 < *age);
                if any || !behind || waited.elapsed() >= POLL_WAIT {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            let mut out = serde_json::Map::new();
            for id in ids {
                if let Some(sock) = sockets.open.get(&id) {
                    let events = std::mem::take(&mut sock.shared.lock().unwrap().events);
                    out.insert(id.to_string(), Value::Array(events));
                }
            }
            Ok(Value::Object(out))
        }
        "send" => {
            let sock = req.get("id").and_then(Value::as_u64).and_then(|id| sockets.open.get(&id)).ok_or("ws: no such connection")?;
            let cmd = match req.get("dataBase64").and_then(Value::as_str) {
                Some(b) => Cmd::Binary(base64::engine::general_purpose::STANDARD.decode(b).map_err(|e| e.to_string())?),
                None => Cmd::Text(s("data").to_string()),
            };
            let _ = sock.tx.send(cmd);
            Ok(Value::Null)
        }
        "close" => {
            let sock = req.get("id").and_then(Value::as_u64).and_then(|id| sockets.open.get(&id)).ok_or("ws: no such connection")?;
            // (the runtime checked the code and the reason, as WebSocket::Close)
            let code = req.get("code").and_then(Value::as_u64).map(|c| c as u16);
            let reason = req.get("reason").and_then(Value::as_str).map(str::to_string);
            let _ = sock.tx.send(Cmd::Close(code, reason));
            Ok(Value::Null)
        }
        c => Err(format!("unknown ws call {c}")),
    }
}

impl Sockets {
    fn open(&mut self, req: &Value, net: &Network, stores: &mut Stores) -> Result<Value, String> {
        let s = |k: &str| req.get(k).and_then(Value::as_str).unwrap_or("");
        // WebSocket::ConstructorCommon: the protocol list first
        let protocols: Vec<String> = req.get("protocols").and_then(Value::as_array).into_iter().flatten().map(|p| p.as_str().unwrap_or("").to_string()).collect();
        for (i, p) in protocols.iter().enumerate() {
            if protocols[..i].contains(p) || !valid_protocol(p) {
                return Ok(throws("SyntaxError", SYNTAX_ERR));
            }
        }
        // WebSocketImpl::ParseURL
        let Ok(mut url) = Url::parse(s("url")) else { return Ok(throws("SyntaxError", SYNTAX_ERR)) };
        match url.scheme() {
            "http" => url.set_scheme("ws").map_err(|_| "ws scheme")?,
            "https" => url.set_scheme("wss").map_err(|_| "wss scheme")?,
            _ => {}
        }
        if url.fragment().is_some() || url.host_str().is_none_or(str::is_empty) || !matches!(url.scheme(), "ws" | "wss") {
            return Ok(throws("SyntaxError", SYNTAX_ERR));
        }
        let mut resource = url.path().to_string();
        if let Some(q) = url.query().filter(|q| !q.is_empty()) {
            resource = format!("{resource}?{q}");
        }
        if resource.bytes().any(|b| !(0x21..=0x7e).contains(&b)) {
            return Ok(throws("SyntaxError", SYNTAX_ERR));
        }
        // WebSocketImpl::Init: the document's CSP (connect-src) over the URL as written, then its
        // upgrade-insecure-requests, unless the document is on a loopback host
        let origin = s("origin").to_string();
        let doc = Doc::from_request(req, &origin);
        if !doc.connect_allowed(&url, false) {
            return Ok(json!({ "throws": { "name": "NS_ERROR_CONTENT_BLOCKED", "result": CONTENT_BLOCKED, "message": CONTENT_BLOCKED_TEXT } }));
        }
        if url.scheme() == "ws" && doc.upgrades() && !Url::parse(&origin).is_ok_and(|o| loopback_host(&o)) {
            // (mURI.ReplaceSubstring("ws://", "wss://"): the port as it was)
            url = Url::parse(&url.as_str().replacen("ws://", "wss://", 1)).map_err(|e| e.to_string())?;
        }
        let secure = url.scheme() == "wss";
        // InitializeConnection: no ws: to another host than a loopback one from a secure context or
        // an https page (network.websocket.allowInsecureFromHTTPS is false)
        if !secure && !loopback_host(&url) && (req.get("secure").and_then(Value::as_bool).unwrap_or(false) || origin.starts_with("https:")) {
            return Ok(throws("SecurityError", SECURITY_ERR));
        }

        // the cookies of the URL's site in the page's partition, as a fetch with credentials
        let mut http = url.clone();
        let _ = http.set_scheme(if secure { "https" } else { "http" });
        let url_site = site_of(&http);
        let top_site = site_of_url_str(s("top"));
        // (a window with another site's frame above it: foreign, its own partition; fetch.rs main_fetch)
        let ancestor = req.get("ancestor").and_then(Value::as_bool).unwrap_or(false);
        let foreign = ancestor || url_site != top_site || site_of_url_str(s("site")) != top_site;
        let store = (url_site.clone(), s("top").to_string(), foreign, ancestor);
        let cookies = if url_site.is_empty() { String::new() } else { stores.get(&store.0, &store.1, ancestor).map(|st| cookie::for_request(st, http.as_str(), foreign)).unwrap_or_default() };
        let origin_url = Url::parse(&origin).ok();
        let fetch_site = match &origin_url {
            Some(o) if o.origin() == http.origin() => "same-origin",
            Some(o) if site_of(o) == url_site && (o.scheme() == "https" || loopback_host(o)) => "same-site",
            _ => "cross-site",
        };

        let mut key = [0u8; 16];
        rand::Rng::fill(&mut rand::thread_rng(), &mut key);
        let key = base64::engine::general_purpose::STANDARD.encode(key);
        let host = match url.port() {
            Some(p) => format!("{}:{p}", url.host_str().unwrap()),
            None => url.host_str().unwrap().to_string(),
        };
        // WebSocketChannel::AsyncOpen's request, in Firefox's order
        let mut head = format!("GET {resource} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {}\r\nAccept: */*\r\nAccept-Language: {}\r\n", net.user_agent, net.accept_language);
        head += if secure { "Accept-Encoding: gzip, deflate, br, zstd\r\n" } else { "Accept-Encoding: gzip, deflate\r\n" };
        head += &format!("Sec-WebSocket-Version: 13\r\nOrigin: {origin}\r\n");
        if !protocols.is_empty() {
            head += &format!("Sec-WebSocket-Protocol: {}\r\n", protocols.join(", "));
        }
        head += &format!("Sec-WebSocket-Extensions: permessage-deflate\r\nSec-WebSocket-Key: {key}\r\nConnection: keep-alive, Upgrade\r\n");
        if !cookies.is_empty() {
            head += &format!("Cookie: {cookies}\r\n");
        }
        if secure || loopback_host(&url) {
            head += &format!("Sec-Fetch-Dest: empty\r\nSec-Fetch-Mode: websocket\r\nSec-Fetch-Site: {fetch_site}\r\n");
        }
        head += "Pragma: no-cache\r\nCache-Control: no-cache\r\nUpgrade: websocket\r\n\r\n";

        self.next += 1;
        let id = self.next;
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, rx) = mpsc::channel();
        let t0 = Instant::now();
        // the admission's partition: the origin attributes' partition key, which only a foreign
        // context has
        let partition = if store.2 { format!("{top_site}^{ancestor}") } else { String::new() };
        let conn = Conn {
            host: url.host_str().unwrap().trim_start_matches('[').trim_end_matches(']').to_string(),
            path: url.path().to_string(),
            partition,
            port: url.port_or_known_default().unwrap_or(if secure { 443 } else { 80 }),
            secure,
            head,
            key,
            protocols,
            proxy: net.proxy.clone(),
            tls: net.tls.clone(),
            http_url: http.to_string(),
            shared: shared.clone(),
            t0,
        };
        std::thread::spawn(move || conn.run(rx));
        self.open.insert(id, Socket { shared: shared.clone(), t0, tx, store });
        // as WS_SCRIPT: the first event, if it comes within PENDING; not for one the admission
        // holds back, whose events the polls get
        while t0.elapsed() < PENDING {
            let sh = shared.lock().unwrap();
            if !sh.events.is_empty() || sh.queued {
                break;
            }
            drop(sh);
            std::thread::sleep(Duration::from_millis(2));
        }
        self.take_cookies(stores);
        let events = std::mem::take(&mut shared.lock().unwrap().events);
        Ok(json!({ "id": id, "url": url.as_str(), "events": events }))
    }

    /// Stores the Set-Cookie lines of the handshakes answered since the last call (the cookies a
    /// WebSocket's 101 sets, as Firefox's parent does for any response)
    pub fn take_cookies(&mut self, stores: &mut Stores) {
        for sock in self.open.values() {
            let Some((url, lines)) = sock.shared.lock().unwrap().cookies.take() else { continue };
            let (site, top, foreign, ancestor) = &sock.store;
            if let Ok(st) = stores.get(site, top, *ancestor) {
                cookie::from_response(st, &url, &lines, *foreign);
            }
        }
    }
}

fn site_of_url_str(s: &str) -> String {
    Url::parse(s).map(|u| site_of(&u)).unwrap_or_else(|_| s.to_string())
}

// ---- a connection's thread ----

/// A connection's bytes: TCP, or TLS over it (its TCP reads time out after TICK, set when it was made)
trait Stream: Read + Write + Send {}
impl Stream for TcpStream {}
impl Stream for rustls::StreamOwned<rustls::ClientConnection, TcpStream> {}

struct Conn {
    host: String,
    /// the URL's path, without its query (the admission's mPath)
    path: String,
    /// the admission's partition (the origin attributes' suffix as far as it matters here)
    partition: String,
    port: u16,
    secure: bool,
    head: String,
    key: String,
    protocols: Vec<String>,
    proxy: Option<String>,
    tls: Arc<rustls::ClientConfig>,
    http_url: String,
    shared: Arc<Mutex<Shared>>,
    t0: Instant,
}

/// How a connection ended, for its close event
enum End {
    /// a close frame from the server: its code (1005 for none) and reason
    Clean(u16, String),
    /// the network ended it without one
    Abnormal,
}

/// Why a handshake did not give an open connection
enum Failed {
    /// the network or the server: an error and a close 1006
    Network,
    /// the TLS handshake (an NSS error: NS_ERROR_NET_INADEQUATE_SECURITY): an error and a close 1015
    Tls,
    /// the page closed it first (WebSocketChannel::Close without a transport: NS_ERROR_NOT_CONNECTED);
    /// the runtime has dispatched its events already
    Closed,
}

impl Conn {
    fn event(&self, mut e: Value) {
        e["ms"] = json!(self.t0.elapsed().as_millis() as u64);
        self.shared.lock().unwrap().events.push(e);
    }

    /// A connection that failed: error, then close 1006, or 1015 when its TLS did (WebSocketImpl::OnStop)
    fn fail(&self, code: u16) {
        self.event(json!({ "type": "error" }));
        self.event(json!({ "type": "close", "wasClean": false, "code": code, "reason": "" }));
    }

    /// The admission's address: the host name through a proxy, else its first resolved IP address
    /// (WebSocketChannel::OnLookupComplete), the host name when it does not resolve
    fn address(&self) -> String {
        if self.proxy.is_some() {
            return self.host.clone();
        }
        use std::net::ToSocketAddrs;
        (self.host.as_str(), self.port).to_socket_addrs().ok().and_then(|mut a| a.next()).map(|a| a.ip().to_string()).unwrap_or_else(|| self.host.clone())
    }

    /// TCP to the server, or to the proxy and through its tunnel, then TLS for wss; `stop` is true
    /// once the page closed the connection
    fn connect(&self, deadline: Instant, stop: &dyn Fn() -> bool) -> Result<Box<dyn Stream>, Failed> {
        use std::net::ToSocketAddrs;
        let left = || deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
        let dial = |host: &str, port: u16| -> Result<TcpStream, Failed> {
            let addr = (host, port).to_socket_addrs().ok().and_then(|mut a| a.next()).ok_or(Failed::Network)?;
            let t = TcpStream::connect_timeout(&addr, left()).map_err(|_| Failed::Network)?;
            let _ = t.set_read_timeout(Some(TICK));
            let _ = t.set_nodelay(true);
            Ok(t)
        };
        let tcp = match &self.proxy {
            Some(p) => {
                let (ph, pp) = p.rsplit_once(':').ok_or(Failed::Network)?;
                let mut t = dial(ph, pp.parse().map_err(|_| Failed::Network)?)?;
                let authority = if self.host.contains(':') { format!("[{}]:{}", self.host, self.port) } else { format!("{}:{}", self.host, self.port) };
                write!(t, "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: keep-alive\r\n\r\n").map_err(|_| Failed::Network)?;
                let head = read_head(&mut t, &mut Vec::new(), deadline, stop)?;
                if !head.lines().next().is_some_and(|l| l.split_whitespace().nth(1) == Some("200")) {
                    return Err(Failed::Network);
                }
                t
            }
            None => dial(&self.host, self.port)?,
        };
        if !self.secure {
            return Ok(Box::new(tcp));
        }
        let name = rustls_pki_types::ServerName::try_from(self.host.clone()).map_err(|_| Failed::Network)?;
        let mut conn = rustls::ClientConnection::new(self.tls.clone(), name).map_err(|_| Failed::Network)?;
        let mut tcp = tcp;
        // the TLS handshake here, so that a server that never answers it times out and the page's
        // close ends it
        while conn.is_handshaking() {
            if stop() {
                return Err(Failed::Closed);
            }
            if Instant::now() > deadline {
                return Err(Failed::Network);
            }
            match conn.complete_io(&mut tcp) {
                Ok((0, 0)) if conn.wants_read() => return Err(Failed::Network),
                Ok(_) => {}
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => {}
                // (rustls's own errors, a protocol's or a certificate's, come as InvalidData)
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => return Err(Failed::Tls),
                Err(_) => return Err(Failed::Network),
            }
        }
        Ok(Box::new(rustls::StreamOwned::new(conn, tcp)))
    }

    /// Connects and does the handshake: the stream, the bytes read after the 101, and the
    /// permessage-deflate parameters if the server took it
    fn handshake(&self, stop: &dyn Fn() -> bool) -> Result<(Box<dyn Stream>, Vec<u8>, String, Option<Pmce>), Failed> {
        let deadline = Instant::now() + OPEN_TIMEOUT;
        let mut stream = self.connect(deadline, stop)?;
        let mut buf = Vec::new();
        stream.write_all(self.head.as_bytes()).map_err(|_| Failed::Network)?;
        let head = read_head(&mut *stream, &mut buf, deadline, stop)?;
        let mut lines = head.split("\r\n");
        let status = lines.next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("");
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();
        let get = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        let set_cookie: Vec<String> = headers.iter().filter(|(n, _)| n == "set-cookie").map(|(_, v)| v.clone()).collect();
        if !set_cookie.is_empty() {
            self.shared.lock().unwrap().cookies = Some((self.http_url.clone(), set_cookie));
        }
        // WebSocketChannel::OnStartRequest's checks of the 101, then HandleExtensions
        let accept = base64::engine::general_purpose::STANDARD.encode(Sha1::digest(format!("{}{GUID}", self.key).as_bytes()));
        let protocol = get("sec-websocket-protocol").unwrap_or("").to_string();
        // (CompressWhitespace)
        let extensions = get("sec-websocket-extensions").unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
        if status != "101"
            || !get("upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
            || !get("connection").is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case("upgrade")))
            || get("sec-websocket-accept") != Some(accept.as_str())
            || (!protocol.is_empty() && !self.protocols.contains(&protocol))
        {
            return Err(Failed::Network);
        }
        let pmce = if extensions.is_empty() { None } else { Some(Pmce::parse(&extensions).ok_or(Failed::Network)?) };
        Ok((stream, buf, protocol, pmce))
    }

    fn run(self, rx: Receiver<Cmd>) {
        // while connecting, the page can only close it (send throws while CONNECTING)
        let stop = || matches!(rx.try_recv(), Ok(_) | Err(TryRecvError::Disconnected));
        let target = Target { address: self.address(), path: self.path.clone(), port: self.port, partition: self.partition.clone() };
        let Some(ticket) = ADMISSION.enter(&target, &stop, &|q| self.shared.lock().unwrap().queued = q) else {
            return ADMISSION.leave(None, &target, Outcome::Closed);
        };
        let (mut stream, buf, protocol, pmce) = match self.handshake(&stop) {
            Ok(h) => h,
            Err(Failed::Closed) => return ADMISSION.leave(Some(ticket), &target, Outcome::Closed),
            Err(f) => {
                ADMISSION.leave(Some(ticket), &target, Outcome::Failed);
                return self.fail(if matches!(f, Failed::Tls) { 1015 } else { 1006 });
            }
        };
        ADMISSION.leave(Some(ticket), &target, Outcome::Connected);
        self.event(json!({ "type": "open", "protocol": protocol, "extensions": if pmce.is_some() { "permessage-deflate" } else { "" } }));
        let end = self.frames(&mut *stream, buf, &rx, pmce);
        match end {
            End::Clean(code, reason) => self.event(json!({ "type": "close", "wasClean": true, "code": code, "reason": reason })),
            End::Abnormal => {
                // (OnStopSession with a failure: a delay before the next connection to it)
                ADMISSION.leave(None, &target, Outcome::Failed);
                self.event(json!({ "type": "close", "wasClean": false, "code": 1006, "reason": "" }))
            }
        }
    }

    /// The open connection: the server's frames as events, the page's sends and close as frames
    fn frames(&self, stream: &mut dyn Stream, mut buf: Vec<u8>, rx: &Receiver<Cmd>, pmce: Option<Pmce>) -> End {
        let deflate = pmce.is_some();
        let pmce = pmce.unwrap_or_default();
        // (zlib turns a raw window of 8 bits into 9, and an inflater's larger window takes a smaller one's)
        let mut inflate = flate2::Decompress::new_with_window_bits(false, pmce.server_bits.max(9));
        let mut compressor = deflate.then(|| Deflater::new(&pmce));
        // a message in fragments: its opcode, whether it is compressed, its bytes so far
        let mut partial: Option<(u8, bool, Vec<u8>)> = None;
        let mut closing = false;
        let mut chunk = [0u8; 16384];
        loop {
            // the page's sends and close, in order
            loop {
                match rx.try_recv() {
                    Ok(cmd) if !closing => {
                        let mut data = |opcode: u8, payload: &[u8]| match compressor.as_mut().and_then(|c| c.deflate(payload)) {
                            Some(d) => frame(opcode, &d, true),
                            None => frame(opcode, payload, false),
                        };
                        let frame = match cmd {
                            Cmd::Text(t) => data(0x1, t.as_bytes()),
                            Cmd::Binary(b) => data(0x2, &b),
                            Cmd::Close(code, reason) => {
                                closing = true;
                                let mut payload = Vec::new();
                                if let Some(c) = code {
                                    payload.extend_from_slice(&c.to_be_bytes());
                                    payload.extend_from_slice(reason.unwrap_or_default().as_bytes());
                                }
                                frame(0x8, &payload, false)
                            }
                        };
                        if stream.write_all(&frame).is_err() {
                            return End::Abnormal;
                        }
                    }
                    Ok(_) => {}
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return End::Abnormal,
                }
            }
            // whole frames in what was read
            while let Some((fin, rsv1, opcode, payload, used)) = parse_frame(&buf) {
                buf.drain(..used);
                match opcode {
                    0x0 | 0x1 | 0x2 => {
                        let (op, compressed, mut data) = match (opcode, partial.take()) {
                            (0x0, Some(p)) => p,
                            (0x0, None) => return End::Abnormal,
                            (op, _) => (op, rsv1 && deflate, Vec::new()),
                        };
                        data.extend_from_slice(&payload);
                        if !fin {
                            partial = Some((op, compressed, data));
                            continue;
                        }
                        if compressed {
                            data.extend_from_slice(&[0, 0, 0xff, 0xff]);
                            match inflate_all(&mut inflate, &data) {
                                Some(d) => data = d,
                                None => return End::Abnormal,
                            }
                        }
                        if closing {
                            continue;
                        }
                        if op == 0x1 {
                            match String::from_utf8(data) {
                                Ok(t) => self.event(json!({ "type": "message", "data": t })),
                                Err(_) => return End::Abnormal,
                            }
                        } else {
                            let b = base64::engine::general_purpose::STANDARD.encode(&data);
                            self.event(json!({ "type": "message", "binary": true, "data": b }));
                        }
                    }
                    0x8 => {
                        let code = if payload.len() >= 2 { u16::from_be_bytes([payload[0], payload[1]]) } else { 1005 };
                        let reason = String::from_utf8_lossy(payload.get(2..).unwrap_or(&[])).into_owned();
                        if !closing {
                            // the server began the close: Firefox answers with its code
                            let echo = if payload.len() >= 2 { payload[..2].to_vec() } else { Vec::new() };
                            let _ = stream.write_all(&frame(0x8, &echo, false));
                        }
                        return End::Clean(code, reason);
                    }
                    0x9 => {
                        if stream.write_all(&frame(0xA, &payload, false)).is_err() {
                            return End::Abnormal;
                        }
                    }
                    _ => {}
                }
            }
            match stream.read(&mut chunk) {
                Ok(0) => return End::Abnormal,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => return End::Abnormal,
            }
        }
    }
}

/// The HTTP head of an answer (up to the blank line), the bytes after it left in `rest`; the stream
/// reads with a timeout (TICK), so that the deadline and the page's close are seen
fn read_head(s: &mut dyn Read, rest: &mut Vec<u8>, deadline: Instant, stop: &dyn Fn() -> bool) -> Result<String, Failed> {
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(i) = rest.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&rest[..i]).into_owned();
            rest.drain(..i + 4);
            return Ok(head);
        }
        if stop() {
            return Err(Failed::Closed);
        }
        if Instant::now() > deadline {
            return Err(Failed::Network);
        }
        match s.read(&mut chunk) {
            Ok(0) => return Err(Failed::Network),
            Ok(n) => rest.extend_from_slice(&chunk[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => {}
            Err(_) => return Err(Failed::Network),
        }
    }
}

/// A client's frame: FIN, RSV1 for a deflated message, the opcode, masked with a random key
fn frame(opcode: u8, payload: &[u8], rsv1: bool) -> Vec<u8> {
    let mut out = vec![0x80 | if rsv1 { 0x40 } else { 0 } | opcode];
    let n = payload.len();
    if n < 126 {
        out.push(0x80 | n as u8);
    } else if n < 65536 {
        out.push(0x80 | 126);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(n as u64).to_be_bytes());
    }
    let mut mask = [0u8; 4];
    rand::Rng::fill(&mut rand::thread_rng(), &mut mask);
    out.extend_from_slice(&mask);
    out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    out
}

/// One whole frame at the start of `b`: (FIN, RSV1, opcode, payload, bytes used)
fn parse_frame(b: &[u8]) -> Option<(bool, bool, u8, Vec<u8>, usize)> {
    if b.len() < 2 {
        return None;
    }
    let (fin, rsv1, opcode) = (b[0] & 0x80 != 0, b[0] & 0x40 != 0, b[0] & 0x0f);
    let masked = b[1] & 0x80 != 0;
    let (len, mut at) = match b[1] & 0x7f {
        126 => (u16::from_be_bytes(b.get(2..4)?.try_into().ok()?) as usize, 4),
        127 => (u64::from_be_bytes(b.get(2..10)?.try_into().ok()?) as usize, 10),
        n => (n as usize, 2),
    };
    let mask = if masked {
        let m: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
        at += 4;
        Some(m)
    } else {
        None
    };
    let data = b.get(at..at + len)?;
    let payload = match mask {
        Some(m) => data.iter().enumerate().map(|(i, x)| x ^ m[i % 4]).collect(),
        None => data.to_vec(),
    };
    Some((fin, rsv1, opcode, payload, at + len))
}

/// permessage-deflate: a message's bytes inflated with the connection's window (context takeover)
fn inflate_all(z: &mut flate2::Decompress, data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() * 3);
    let mut input = data;
    loop {
        if out.capacity() - out.len() < 4096 {
            out.reserve(out.capacity().max(4096));
        }
        let before_in = z.total_in();
        z.decompress_vec(input, &mut out, flate2::FlushDecompress::Sync).ok()?;
        input = &input[(z.total_in() - before_in) as usize..];
        if input.is_empty() && out.capacity() > out.len() {
            return Some(out);
        }
    }
}

// ---- permessage-deflate ----

/// What a server's Sec-WebSocket-Extensions agreed to (ParseWebSocketExtension, client side)
#[derive(Debug, PartialEq)]
struct Pmce {
    client_no_context_takeover: bool,
    server_no_context_takeover: bool,
    client_bits: u8,
    server_bits: u8,
}

impl Default for Pmce {
    fn default() -> Pmce {
        Pmce { client_no_context_takeover: false, server_no_context_takeover: false, client_bits: 15, server_bits: 15 }
    }
}

impl Pmce {
    /// The header's value, whitespace compressed: one extension, permessage-deflate, and only its
    /// four parameters, each at most once, the flags with no value and the window bits 8 to 15. None
    /// for anything else, which fails the connection (HandleExtensions' AbortSession).
    fn parse(ext: &str) -> Option<Pmce> {
        let mut tokens = ext.split(';').map(str::trim);
        if tokens.next() != Some("permessage-deflate") {
            return None;
        }
        let mut p = Pmce::default();
        let (mut cn, mut sn, mut cb, mut sb) = (false, false, false, false);
        let bits = |v: &str| v.parse::<i64>().ok().filter(|b| (8..=15).contains(b)).map(|b| b as u8);
        for token in tokens {
            let (name, value) = token.split_once('=').unwrap_or((token, ""));
            let had = match name {
                "client_no_context_takeover" if value.is_empty() => std::mem::replace(&mut cn, true),
                "server_no_context_takeover" if value.is_empty() => std::mem::replace(&mut sn, true),
                "client_max_window_bits" => {
                    p.client_bits = bits(value)?;
                    std::mem::replace(&mut cb, true)
                }
                "server_max_window_bits" => {
                    p.server_bits = bits(value)?;
                    std::mem::replace(&mut sb, true)
                }
                _ => return None,
            };
            if had {
                return None;
            }
        }
        p.client_no_context_takeover = cn;
        p.server_no_context_takeover = sn;
        Some(p)
    }
}

/// PMCECompression's deflater: the page's messages, compressed with the client's window
struct Deflater {
    z: flate2::Compress,
    takeover: bool,
    reset: bool,
}

impl Deflater {
    fn new(p: &Pmce) -> Deflater {
        let z = flate2::Compress::new_with_window_bits(flate2::Compression::default(), false, p.client_bits.max(9));
        Deflater { z, takeover: !p.client_no_context_takeover, reset: false }
    }

    /// OutboundMessage::DeflatePayload: the message deflated (sync flush, its last four bytes
    /// dropped), or None to send it as it is: an empty one, a failure, or, when the window is not
    /// kept between messages, one that deflating makes larger
    fn deflate(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        if data.is_empty() {
            return None;
        }
        if self.reset || !self.takeover {
            self.z.reset();
            self.reset = false;
        }
        let mut out = Vec::with_capacity(data.len() / 2 + 64);
        let mut input = data;
        loop {
            if out.capacity() - out.len() < 4096 {
                out.reserve(4096);
            }
            let before = self.z.total_in();
            if self.z.compress_vec(input, &mut out, flate2::FlushCompress::Sync).is_err() {
                self.reset = true;
                return None;
            }
            input = &input[(self.z.total_in() - before) as usize..];
            if input.is_empty() && out.capacity() > out.len() {
                break;
            }
        }
        if !out.ends_with(&[0, 0, 0xff, 0xff]) {
            self.reset = true;
            return None;
        }
        out.truncate(out.len() - 4);
        if !self.takeover && out.len() > data.len() {
            return None;
        }
        Some(out)
    }
}

// ---- nsWSAdmissionManager ----

/// What a connection is admitted by: the address (IP, or host name through a proxy) and partition
/// for the one-at-a-time rule, with the path and port for the failure delays
struct Target {
    address: String,
    path: String,
    port: u16,
    partition: String,
}

/// FailDelay: a target that failed, and how long a new connection to it waits
struct FailDelay {
    address: String,
    path: String,
    port: u16,
    partition: String,
    last: Instant,
    next_delay: Duration,
}

impl FailDelay {
    fn is(&self, t: &Target) -> bool {
        self.address == t.address && self.path == t.path && self.port == t.port && self.partition == t.partition
    }
    /// kWSReconnectBaseLifeTime (60 s) past its delay, it is forgotten
    fn expired(&self, now: Instant) -> bool {
        self.last + Duration::from_secs(60) + self.next_delay <= now
    }
    fn remaining(&self, now: Instant) -> Duration {
        (self.last + self.next_delay).saturating_duration_since(now)
    }
}

struct Queued {
    ticket: u64,
    address: String,
    partition: String,
    failed: bool,
}

#[derive(Default)]
struct AdmissionState {
    next: u64,
    /// the connections connecting or waiting to, in the order they go
    queue: Vec<Queued>,
    failures: Vec<FailDelay>,
}

enum Outcome {
    Connected,
    Failed,
    /// the page closed it before it connected
    Closed,
}

/// The process's admission manager, which every connection's thread goes through
struct Admission {
    state: Mutex<AdmissionState>,
    changed: Condvar,
}

static ADMISSION: LazyLock<Admission> = LazyLock::new(|| Admission { state: Mutex::new(AdmissionState::default()), changed: Condvar::new() });

impl AdmissionState {
    /// FailDelayManager::Lookup, which drops the expired entries it passes
    fn lookup(&mut self, t: &Target) -> Option<usize> {
        let now = Instant::now();
        let mut i = self.failures.len();
        while i > 0 {
            i -= 1;
            if self.failures[i].is(t) {
                return Some(i);
            }
            if self.failures[i].expired(now) {
                self.failures.remove(i);
            }
        }
        None
    }
}

impl Admission {
    /// ConditionallyConnect, then ConnectNext and DelayOrBegin: waits until no other connection to
    /// the address (and partition) is ahead of this one and its failure delay has passed. Queued
    /// after the others, but ahead of those that failed before when it has not. None when `stop`
    /// (the page closed it) came first.
    /// `queued` is told when it starts waiting and when it stops.
    fn enter(&self, t: &Target, stop: &dyn Fn() -> bool, queued: &dyn Fn(bool)) -> Option<u64> {
        let mut st = self.state.lock().unwrap();
        st.next += 1;
        let ticket = st.next;
        let failed = st.lookup(t).is_some();
        let q = Queued { ticket, address: t.address.clone(), partition: t.partition.clone(), failed };
        if failed {
            st.queue.push(q);
        } else {
            let at = st.queue.iter().position(|q| q.failed).unwrap_or(st.queue.len());
            st.queue.insert(at, q);
        }
        loop {
            if stop() {
                st.queue.retain(|q| q.ticket != ticket);
                self.changed.notify_all();
                queued(false);
                return None;
            }
            let first = st.queue.iter().find(|q| q.address == t.address && q.partition == t.partition).map(|q| q.ticket);
            let mut wait = TICK;
            if first == Some(ticket) {
                let now = Instant::now();
                match st.lookup(t) {
                    Some(i) if !st.failures[i].remaining(now).is_zero() => wait = wait.min(st.failures[i].remaining(now)),
                    Some(i) => {
                        if st.failures[i].expired(now) {
                            st.failures.remove(i);
                        }
                        queued(false);
                        return Some(ticket);
                    }
                    None => {
                        queued(false);
                        return Some(ticket);
                    }
                }
            }
            queued(true);
            st = self.changed.wait_timeout(st, wait).unwrap().0;
        }
    }

    /// OnConnected and OnStopSession: out of the queue (when `ticket`), and the target's failures
    /// cleared by a connection, or recorded (the delay grows for one seen before; the page's close
    /// before a connection does not make it grow)
    fn leave(&self, ticket: Option<u64>, t: &Target, outcome: Outcome) {
        let mut st = self.state.lock().unwrap();
        if let Some(ticket) = ticket {
            st.queue.retain(|q| q.ticket != ticket);
        }
        let now = Instant::now();
        match outcome {
            Outcome::Connected => st.failures.retain(|f| !f.is(t) && !f.expired(now)),
            Outcome::Failed | Outcome::Closed => match st.lookup(t) {
                Some(i) => {
                    if matches!(outcome, Outcome::Failed) {
                        // FailedAgain: x1.5, at most kWSReconnectMaxDelay
                        let f = &mut st.failures[i];
                        f.last = now;
                        f.next_delay = Duration::from_millis(((f.next_delay.as_millis() as f64 * 1.5) as u64).min(60_000));
                    }
                }
                None => {
                    // kWSReconnectInitialBaseDelay + rand() % kWSReconnectInitialRandomDelay
                    let next_delay = Duration::from_millis(200 + rand::Rng::gen_range(&mut rand::thread_rng(), 0..200));
                    st.failures.push(FailDelay { address: t.address.clone(), path: t.path.clone(), port: t.port, partition: t.partition.clone(), last: now, next_delay });
                }
            },
        }
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_parse() {
        assert_eq!(Pmce::parse("permessage-deflate"), Some(Pmce::default()));
        let p = Pmce::parse("permessage-deflate; client_no_context_takeover; server_max_window_bits=10").unwrap();
        assert!(p.client_no_context_takeover && !p.server_no_context_takeover && p.server_bits == 10 && p.client_bits == 15);
        for bad in ["x-webkit-deflate-frame", "permessage-deflate; bogus", "permessage-deflate; server_max_window_bits=7",
                    "permessage-deflate; server_max_window_bits=16", "permessage-deflate; client_max_window_bits",
                    "permessage-deflate; client_no_context_takeover; client_no_context_takeover",
                    "permessage-deflate; server_no_context_takeover=1", "permessage-deflate, permessage-deflate"] {
            assert_eq!(Pmce::parse(bad), None, "{bad}");
        }
    }

    fn inflate(z: &mut flate2::Decompress, d: &[u8]) -> Vec<u8> {
        let mut v = d.to_vec();
        v.extend_from_slice(&[0, 0, 0xff, 0xff]);
        inflate_all(z, &v).unwrap()
    }

    #[test]
    fn deflate_messages() {
        let text = b"hello hello hello hello hello hello hello hello";
        // context takeover: every message deflated, even one that grows; the window kept
        let mut d = Deflater::new(&Pmce::default());
        let mut z = flate2::Decompress::new(false);
        let a = d.deflate(text).unwrap();
        assert!(a.len() < text.len());
        assert_eq!(inflate(&mut z, &a), text);
        let b = d.deflate(text).unwrap();
        assert!(b.len() < a.len());
        assert_eq!(inflate(&mut z, &b), text);
        assert!(d.deflate(b"ab").is_some());
        assert!(d.deflate(b"").is_none());
        // no context takeover: one that grows goes as it is
        let mut d = Deflater::new(&Pmce { client_no_context_takeover: true, ..Pmce::default() });
        assert!(d.deflate(b"ab").is_none());
        let a = d.deflate(text).unwrap();
        assert_eq!(a, d.deflate(text).unwrap());
        // a small window
        let mut d = Deflater::new(&Pmce { client_bits: 8, ..Pmce::default() });
        let big = "x".repeat(70000);
        let mut z = flate2::Decompress::new_with_window_bits(false, 9);
        assert_eq!(inflate(&mut z, &d.deflate(big.as_bytes()).unwrap()), big.as_bytes());
    }

    #[test]
    fn admission_delays() {
        let t = Target { address: "192.0.2.1".into(), path: "/t".into(), port: 1, partition: String::new() };
        let a = Admission { state: Mutex::new(AdmissionState::default()), changed: Condvar::new() };
        let stop = || false;
        let k = a.enter(&t, &stop, &|_| {}).unwrap();
        a.leave(Some(k), &t, Outcome::Failed);
        let first = a.state.lock().unwrap().failures[0].next_delay;
        assert!((200..400).contains(&(first.as_millis() as u64)));
        let at = Instant::now();
        let k = a.enter(&t, &stop, &|_| {}).unwrap();
        assert!(at.elapsed() + Duration::from_millis(20) >= first);
        a.leave(Some(k), &t, Outcome::Failed);
        assert_eq!(a.state.lock().unwrap().failures[0].next_delay.as_millis(), (first.as_millis() as f64 * 1.5) as u128);
        // the page's close does not make it grow; a connection clears it
        a.leave(None, &t, Outcome::Closed);
        assert_eq!(a.state.lock().unwrap().failures[0].next_delay.as_millis(), (first.as_millis() as f64 * 1.5) as u128);
        a.leave(None, &t, Outcome::Connected);
        assert!(a.state.lock().unwrap().failures.is_empty());
        // one at a time to an address: the second waits for the first to leave
        let k1 = a.enter(&t, &stop, &|_| {}).unwrap();
        let other = Target { path: "/u".into(), ..Target { address: t.address.clone(), path: String::new(), port: 1, partition: String::new() } };
        let gone = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(50));
                gone.store(true, std::sync::atomic::Ordering::SeqCst);
                a.leave(Some(k1), &t, Outcome::Connected);
            });
            let k2 = a.enter(&other, &stop, &|_| {}).unwrap();
            assert!(gone.load(std::sync::atomic::Ordering::SeqCst));
            a.leave(Some(k2), &other, Outcome::Connected);
        });
    }
}
