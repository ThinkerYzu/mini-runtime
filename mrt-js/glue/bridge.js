// Stage 6, phase 1: the page's view of the services Firefox provides, forwarded through the bridge
// (DESIGN "The bridge, as built in phase 1"). Run after prelude.js, only when the page has a bridge.
// `__mrtBridge(json)` sends one request to the driver, which runs it in the bridge tab (the site's
// origin, its JS off) and returns the reply. Also here: the page's URL objects, script loading, and
// the missing-API log. Pure-JS pieces the sites need before anything else (URL, atob, TextEncoder)
// are written here too; Firefox has them in C++ or Rust.
"use strict";
(function (global) {
  const define = (obj, name, value) =>
    Object.defineProperty(obj, name, { value, writable: true, configurable: true, enumerable: false });
  const getter = (obj, name, get, set) =>
    Object.defineProperty(obj, name, { get, set, configurable: true, enumerable: true });

  // What all the runtime's windows share (prelude.js): the counts of the report, and the state
  // Gecko keeps on a node (DESIGN, "Same-origin frames: the build plan", S3).
  const shared = global.__mrt.shared;
  const share = (name, make) => (Object.prototype.hasOwnProperty.call(shared, name) ? shared[name] : (shared[name] = make()));
  // a frame's window (CreateFrameWindow gave it the page's shared state)
  const isFrameWindow = !!global.__mrtShared;
  // a dedicated worker's global (prelude.js): [Exposed=Window]-only statements are guarded
  const isWorker = typeof __mrtWorkerScope === "object";

  // ---- the bridge ----

  const calls = share("calls", () => ({}));
  // The window's own context in the bridge tab, which the driver serves its requests in: a frame
  // window of another origin than the runtime's first window has one of its own (its cookies and
  // storage partition; SPEC decision 19), a frame window of its parent's origin uses its parent's.
  let bridgeContext = null;
  // Whether another site's frame is above this window although its own site is the top-level
  // site's (an A frame in a B frame in an A page): a root window of the page's runtime and the
  // windows under it. Its requests say so (`ancestor`), and the API process gives it the partition
  // key's foreign-ancestor bit, a third party's cookies and storage; the bridge tab ignores it.
  let foreignAncestor = false;
  if (isFrameWindow) {
    try {
      const parent = __mrtWindows(2);
      if (parent !== global && parent.__mrt && typeof parent.__mrt.bridgeContext === "function") {
        bridgeContext = parent.__mrt.bridgeContext();
        foreignAncestor = parent.__mrt.foreignAncestor();
      }
    } catch (e) {
      // a parent of another origin: its view throws (a window of another origin gets its context
      // from setBridgeContext)
    }
  }
  // The bridge tab this global last talked to: the driver's generation of it (SPEC decision 14: a
  // hibernated page's bridge is dropped, and a new one made when the page needs it). A request names
  // it; when the driver has made a new tab since, it runs nothing and answers `bridgeNew`, and this
  // global first gives up what lived in the old tab (bridgeLost), then sends the request again.
  // Each request carries the virtual time it is made at (`t`), for the driver's record of when the
  // page needed a bridge again.
  let bridgeGen = null;
  // set once the page's address is known (below)
  let referrerURL = () => "";
  // the policy of the document's last <meta name=referrer> (set below)
  let docMetaReferrer = () => "";
  const bridgeLostHooks = [];
  const bridgeClock = global.performance.now;
  function bridge(op, args) {
    calls[op] = (calls[op] || 0) + 1;
    const req = Object.assign({ op, t: bridgeClock() }, bridgeContext !== null ? { ctx: bridgeContext } : {}, bridgeGen !== null ? { gen: bridgeGen } : {},
      foreignAncestor ? { ancestor: true } : {}, args);
    // a request made in the API process (mini_realsites.py API_PROCESS) needs whose it is: the
    // global's origin and its document's URL, the referrer (the tab's fetch takes both from its own
    // document and ignores them)
    // and the referrer policy its <meta name=referrer> set; a worker from the network has none of its
    // document's policies (`docless`: its own response would set them), a blob: or data: one inherits
    // (webgl: the images its textures load, GAPS B169; mediaResource: a media element's, loaded by
    // the HTTP viewer through the API process, GAPS B172)
    if (op === "fetch" || op === "image" || op === "webgl" || op === "mediaResource") {
      Object.assign(req, { origin: global.origin, referrer: referrerURL() });
      const meta = docMetaReferrer();
      if (meta) req.metaReferrer = meta;
      if (isWorker && !/^(blob|data):/i.test(referrerURL())) req.docless = true;
    }
    // as do a WebSocket (its origin, whether it is a secure context: the mixed-content check, and
    // its document's policies: connect-src) and the browser's identity (the interface names differ
    // by the kind of global)
    else if (op === "ws" && args.call === "open") {
      Object.assign(req, { origin: global.origin, secure: global.isSecureContext });
      if (isWorker && !/^(blob|data):/i.test(referrerURL())) req.docless = true;
    }
    else if (op === "idb") Object.assign(req, { origin: global.origin, client: idbClient });
    else if (op === "permissions") Object.assign(req, { origin: global.origin, secure: global.isSecureContext, worker: isWorker });
    else if (op === "env") Object.assign(req, { secure: global.isSecureContext, isolated: global.crossOriginIsolated });
    const reply = JSON.parse(__mrtBridge(JSON.stringify(req)));
    if (reply && reply.bridgeNew !== undefined) {
      bridgeGen = reply.bridgeNew;
      calls["bridge-new"] = (calls["bridge-new"] || 0) + 1;
      for (const hook of bridgeLostHooks) hook();
      calls[op]--;
      // a WebSocket poll asks the old tab for news: its sockets are closed now, and asking the new
      // tab would make it for nothing
      if (op === "ws" && args.call === "poll") return {};
      return bridge(op, args);
    }
    if (reply && reply.gen !== undefined) bridgeGen = reply.gen;
    if (reply && "err" in reply) throw new TypeError(`${op}: ${reply.err}`);
    return reply.ok;
  }
  // the page's bridge tab dropped, as at hibernation under memory pressure (realsite's MRT_BRIDGE_DROP;
  // a bridge case calls it): the driver closes the tab and makes a new one when a request needs it
  if (!isFrameWindow && !isWorker) global.__mrt.dropBridge = () => bridge("bridgeDrop", {});
  // the browser's identity and the API names, asked once for all windows
  const env = share("env", () => bridge("env", {}));
  // Touch events as the bridge tab's Firefox has them: Gecko exposes TouchEvent and HTMLElement's
  // ontouch* only with a touchscreen or the pref forced on (TouchEvent::PrefEnabled,
  // nsGenericHTMLElement::LegacyTouchAPIEnabled), so desktop Firefox under Xvfb has neither and
  // Fenix has both. X draws its touch layout (a pull-to-refresh wrapper) when `"ontouchstart" in
  // div` (task 3843). Kept when the bridge did not list its names.
  const touchEnabled = !env.names || !env.names.HTMLElement || env.names.HTMLElement.includes("ontouchstart");

  // ---- URL and URLSearchParams (enough for resolving and reading URLs) ----

  const { URL_RE, removeDots, resolve } = global.__mrt.url;
  class URLSearchParams {
    constructor(init = "") {
      this._list = [];
      if (typeof init === "string") {
        for (const part of init.replace(/^\?/, "").split("&")) {
          if (!part) continue;
          const i = part.indexOf("=");
          const dec = s => { try { return decodeURIComponent(s.replace(/\+/g, " ")); } catch (e) { return s; } };
          this._list.push(i < 0 ? [dec(part), ""] : [dec(part.slice(0, i)), dec(part.slice(i + 1))]);
        }
      } else if (init && typeof init === "object") {
        const pairs = Symbol.iterator in init ? init : Object.entries(init);
        for (const [k, v] of pairs) this._list.push([String(k), String(v)]);
      }
    }
    get(k) { const e = this._list.find(p => p[0] === k); return e ? e[1] : null; }
    getAll(k) { return this._list.filter(p => p[0] === k).map(p => p[1]); }
    has(k) { return this._list.some(p => p[0] === k); }
    set(k, v) { this.delete(k); this._list.push([String(k), String(v)]); this._update(); }
    append(k, v) { this._list.push([String(k), String(v)]); this._update(); }
    delete(k) { this._list = this._list.filter(p => p[0] !== k); this._update(); }
    forEach(f, self) { for (const [k, v] of this._list) f.call(self, v, k, this); }
    keys() { return this._list.map(p => p[0])[Symbol.iterator](); }
    values() { return this._list.map(p => p[1])[Symbol.iterator](); }
    entries() { return this._list.map(p => [p[0], p[1]])[Symbol.iterator](); }
    [Symbol.iterator]() { return this.entries(); }
    get size() { return this._list.length; }
    // the application/x-www-form-urlencoded serializer: a space as "+", and !'()~ escaped too
    toString() { return this._list.map(([k, v]) => formEncode(k) + "=" + formEncode(v)).join("&"); }
    _update() { if (this._url) this._url._search = this._list.length ? "?" + this.toString() : ""; }
  }
  const formEncode = v => encodeURIComponent(String(v).isWellFormed() ? v : String(v).toWellFormed())
    .replace(/[!'()~]/g, c => "%" + c.charCodeAt(0).toString(16).toUpperCase()).replace(/%20/g, "+");
  // The URL Standard's userinfo percent-encode set: C0 controls, space, '"#<>?`{}' (the path set)
  // and '/:;=@[\\]^|', DEL and every non-ASCII code point (as UTF-8); the rest is kept
  const USERINFO_SET = /[\u0000-\u0020"#<>?`{}\/:;=@[\\\]^|\u007f-\uffff]/;
  const encodeUserinfo = v => Array.from(v, c => (USERINFO_SET.test(c) ? encodeURIComponent(c.isWellFormed() ? c : "\ufffd") : c)).join("");
  const SPECIAL_DEFAULT_PORTS = { "http:": 80, "https:": 443, "ws:": 80, "wss:": 443, "ftp:": 21 };
  class URL {
    constructor(input, base) {
      const href = base === undefined ? String(input) : resolve(input, base instanceof URL ? base.href : resolve(String(base), "about:blank"));
      const m = URL_RE.exec(href.trim());
      if (!m) throw new TypeError("Invalid URL: " + href);
      [, this.protocol, this._user, this.hostname, this.port, this._path, this._search, this._hash] = m;
      this.protocol = this.protocol.toLowerCase();
      this.hostname = (this.hostname || "").toLowerCase();
      // the URL Standard's host parser: a bracketed host is an IPv6 address; any other host has no
      // forbidden host code point ("http://[::1" fails, as in Firefox)
      if (this.hostname.startsWith("[") ? !/^\[[0-9a-f:.]+\]$/.test(this.hostname) : /[\u0000\t\n\r #/:<>?@[\\\]^|]/.test(this.hostname)) {
        throw new TypeError("Invalid URL: " + href);
      }
      // the URL Standard's port state: a number up to 65535, and none when it is the special scheme's
      // default ("https://a:443/" is "https://a/", so its origin is "https://a")
      if (this.port) {
        const port = Number(this.port);
        if (port > 65535) throw new TypeError("Invalid URL: " + href);
        this.port = port === SPECIAL_DEFAULT_PORTS[this.protocol] ? "" : String(port);
      }
      this.port = this.port || "";
      this._user = this._user || "";
      this._path = this.hostname ? removeDots(this._path || "/") : this._path;
      this._search = this._search && this._search !== "?" ? this._search : "";
      this.hash = this._hash || "";
    }
    get host() { return this.hostname + (this.port ? ":" + this.port : ""); }
    // the URL Standard's origin: a blob: URL's is its path's, when that is an http(s) URL; a special
    // scheme's (not file:) is its scheme, host and port; any other's is opaque ("foo://a.com" is "null")
    get origin() {
      if (this.protocol === "blob:") {
        try { const inner = new URL(this._path); return /^https?:$/.test(inner.protocol) ? inner.origin : "null"; } catch (e) { return "null"; }
      }
      return this.hostname && this.protocol in SPECIAL_DEFAULT_PORTS ? this.protocol + "//" + this.host : "null";
    }
    get pathname() { return this._path; }
    set pathname(p) { this._path = p.startsWith("/") ? p : "/" + p; }
    // An empty fragment serializes as a trailing #, although URL.hash reads as "".
    get hash() { return this._hash === "#" ? "" : this._hash; }
    set hash(h) {
      h = String(h);
      if (!h) { this._hash = ""; return; }
      const fragment = (h.startsWith("#") ? h.slice(1) : h).toWellFormed();
      this._hash = "#" + Array.from(fragment, c => /[\u0000-\u0020"<>`\u007f-\uffff]/.test(c) || c.codePointAt(0) > 0xffff ? encodeURIComponent(c) : c).join("");
    }
    get search() { return this._search; }
    set search(s) { s = String(s); this._search = s && s !== "?" ? (s.startsWith("?") ? s : "?" + s) : ""; delete this._params; }
    get searchParams() {
      if (!this._params) { this._params = new URLSearchParams(this._search); this._params._url = this; }
      return this._params;
    }
    get username() { return this._user.split(":")[0]; }
    get password() { return this._user.split(":")[1] || ""; }
    // the URL Standard's username and password setters: nothing for a URL that cannot have them (no
    // host, or file:), else the value percent-encoded with the userinfo set
    set username(v) { if (this.hostname && this.protocol !== "file:") this._setUserinfo(encodeUserinfo(String(v)), this.password); }
    set password(v) { if (this.hostname && this.protocol !== "file:") this._setUserinfo(this.username, encodeUserinfo(String(v))); }
    _setUserinfo(user, password) { this._user = user + (password ? ":" + password : ""); }
    get href() {
      const auth = this._user ? this._user + "@" : "";
      return this.protocol + (this.hostname ? "//" + auth + this.host : "") + this._path + this._search + this._hash;
    }
    set href(v) { Object.assign(this, new URL(v)); }
    toString() { return this.href; }
    toJSON() { return this.href; }
    static canParse(u, b) { try { new URL(u, b); return true; } catch (e) { return false; } }
  }
  define(global, "URL", URL);
  define(global, "URLSearchParams", URLSearchParams);

  // ---- the page's address, the browser's identity ----

  let pageURL = new URL("about:blank");
  // the document's principal URL, which document.cookie reads (its host and path): the URL it was
  // loaded at, not where pushState moved it; an about:blank or about:srcdoc frame's is its parent's
  let principalURL = null;
  const inheritedPrincipal = () => {
    try { return global.parent.__mrt.principalURL(); } catch (e) { return global.parent.location.href; }
  };
  // Document::GetDocumentURIAsReferrer: the document's URL, as pushState moved it; an about:srcdoc
  // frame's is its parent's (a worker's: its script's URL)
  referrerURL = () => {
    if (isWorker || pageURL.href !== "about:srcdoc") return pageURL.href;
    try { return global.parent.location.href; } catch (e) { return ""; }
  };
  // The document's referrer policy from <meta name=referrer> (HTMLMetaElement::MetaAddedOrChanged,
  // Document::UpdateReferrerInfoFromMeta): each such element put in the document, or whose content
  // changed, since the last request sets it, in document order, when its content is a policy (the
  // legacy tokens too); removing one does not undo it. An about: frame without one has its parent's.
  {
    let policy = "";
    const seen = new WeakMap();
    const LEGACY = new Map([["never", "no-referrer"], ["default", "no-referrer-when-downgrade"], ["always", "unsafe-url"],
      ["origin-when-crossorigin", "origin-when-cross-origin"]]);
    const POLICIES = new Set(["no-referrer", "no-referrer-when-downgrade", "origin", "origin-when-cross-origin", "same-origin",
      "strict-origin", "strict-origin-when-cross-origin", "unsafe-url"]);
    docMetaReferrer = () => {
      if (isWorker || !global.document) return "";
      const metas = global.document.getElementsByTagName("meta");
      for (let i = 0; i < metas.length; i++) {
        const m = metas[i], name = m.getAttribute("name");
        if (name === null || name.toLowerCase() !== "referrer") continue;
        const content = m.getAttribute("content");
        if (seen.has(m) && seen.get(m) === content) continue;
        seen.set(m, content);
        const v = (content || "").replace(/^[\t\n\f\r ]+|[\t\n\f\r ]+$/g, "").toLowerCase();
        const p = LEGACY.get(v) || (POLICIES.has(v) ? v : "");
        if (p) policy = p;
      }
      if (!policy && isFrameWindow && pageURL.protocol === "about:") {
        try { return global.parent.__mrt.metaReferrer(); } catch (e) {}
      }
      return policy;
    };
    global.__mrt.metaReferrer = () => docMetaReferrer();
  }
  // Location (Location.webidl): a setter or method that changes the URL navigates (navigateTo,
  // below): to another document through the driver, or in this one when only the fragment differs.
  // A part's setter navigates to the URL with that part changed (Location::SetProtocol and the rest).
  const withPart = (part, v) => { const u = new URL(pageURL.href); u[part] = String(v); return u.href; };
  const location = {
    get href() { return pageURL.href; },
    set href(v) { navigateTo(String(v), {}); },
    get protocol() { return pageURL.protocol; },
    set protocol(v) { navigateTo(withPart("protocol", v), {}); },
    get host() { return pageURL.host; },
    set host(v) { navigateTo(withPart("host", v), {}); },
    get hostname() { return pageURL.hostname; },
    set hostname(v) { navigateTo(withPart("hostname", v), {}); },
    get port() { return pageURL.port; },
    set port(v) { navigateTo(withPart("port", v), {}); },
    get pathname() { return pageURL.pathname; },
    set pathname(v) { navigateTo(withPart("pathname", v), {}); },
    get search() { return pageURL.search; },
    set search(v) { navigateTo(withPart("search", v), {}); },
    get hash() { return pageURL.hash; },
    // (Location::SetHash: a fragment navigation, also to the same fragment)
    set hash(h) { h = String(h); navigateTo(pageURL.href.replace(/#.*$/s, "") + "#" + (h.startsWith("#") ? h.slice(1) : h), {}); },
    get origin() { return pageURL.origin; },
    assign(u) { navigateTo(String(u), {}); },
    replace(u) { navigateTo(String(u), { replace: true }); },
    reload() { navigateTo(pageURL.href, { replace: true, reload: true }); },
    toString() { return pageURL.href; },
  };
  getter(global, "location", () => location, v => location.assign(v));
  if (!isWorker) getter(Document.prototype, "location", () => location, v => location.assign(v));
  if (!isWorker) getter(Document.prototype, "URL", () => pageURL.href);
  if (!isWorker) getter(Document.prototype, "documentURI", () => pageURL.href);
  if (!isWorker) getter(Document.prototype, "baseURI", () => pageURL.href);
  if (!isWorker) getter(Document.prototype, "domain", () => pageURL.hostname);
  // (a navigated document's: the referrer its request sent, mini_realsites.py's env `navigation`)
  if (!isWorker) getter(Document.prototype, "referrer", () => (!isFrameWindow && env.navigation && env.navigation.referrer) || "");
  let readyState = "loading";
  if (!isWorker) getter(Document.prototype, "readyState", () => readyState);

  // ---- the load event: what blocks it (Gecko's Document::BlockOnload and the document's load group) ----
  //
  // The window's load waits for what the document started loading before it: a script inserted by
  // script, until it has run and fired its load or error (its channel is in the document's load
  // group); an async XMLHttpRequest, until loadend; a stylesheet link inserted by script, until its
  // load or error; a frame, until its own load (nsDocShell blocks the parent document while a frame
  // loads; a cross-origin frame until its load reaches the embedder, BrowserBridgeChild's
  // UnblockOwnerDocsLoadEvent). fetch() blocks nothing here: the bridge answers it at once. Once the
  // document is parsed (documentLoaded), the load fires when nothing blocks it, in a task after the
  // last unblocking (Document::UnblockOnload posts DoUnblockOnload). A new document in the window
  // (a frame navigated) starts again; the old one's unblockings then do nothing.
  let onloadBlockCount = 0, loadArmed = false, loadFired = false, onLoaded = null, loadGeneration = 0;
  // what blocks it now, by what it is (__mrt.onloadBlockers, for looking into a site)
  const onloadBlockers = new Set();
  // Blocks the load event; the function returned unblocks it (once). Nothing after the load.
  function blockOnload(what = "") {
    if (loadFired) return () => {};
    const generation = loadGeneration;
    onloadBlockCount++;
    const blocker = { what };
    onloadBlockers.add(blocker);
    let done = false;
    return () => {
      if (done || generation !== loadGeneration) return;
      done = true;
      onloadBlockers.delete(blocker);
      if (--onloadBlockCount === 0 && loadArmed) global.__mrt.queueTask(fireLoad);
    };
  }
  function fireLoad() {
    if (loadFired || !loadArmed || onloadBlockCount !== 0) return;
    scrollInitialFragment();
    loadFired = true;
    global.__mrt.setReadyState("complete");
    global.__mrt.fireWindowLoad();
    const then = onLoaded;
    onLoaded = null;
    if (then) then();
  }
  if (!isWorker) define(Document.prototype, "hasFocus", () => true);
  let currentScript = null;
  if (!isWorker) getter(Document.prototype, "currentScript", () => currentScript);
  // (the url is for an API process, SPEC decision 27; the bridge tab reads its own document's)
  const cookiePrincipal = () => principalURL === null ? (isFrameWindow ? inheritedPrincipal() : pageURL.href) : principalURL;
  if (!isWorker) getter(Document.prototype, "cookie", () => bridge("cookie", { url: cookiePrincipal() }), v => { bridge("cookie", { url: cookiePrincipal(), value: String(v) }); });

  const navigator = Object.assign({}, env.navigator);
  navigator.webdriver = false;
  navigator.javaEnabled = () => false;
  navigator.sendBeacon = (url, data) => {
    try { bridge("fetch", { url: resolve(url, pageURL.href), method: "POST", body: data == null ? null : String(data), noBody: true }); return true; } catch (e) { return false; }
  };
  // Media Session: nothing plays, so what a site sets is kept and never used
  navigator.mediaSession = { metadata: null, playbackState: "none", setActionHandler() {}, setPositionState() {}, setMicrophoneActive() {}, setCameraActive() {} };
  define(global, "MediaMetadata", class MediaMetadata { constructor(init = {}) { Object.assign(this, { title: "", artist: "", album: "", artwork: [] }, init); } });
  // navigator.plugins and navigator.mimeTypes (Gecko's nsPluginArray, nsPluginElement, nsMimeTypeArray,
  // nsMimeType): the HTML spec's hard-coded PDF viewer plugins, five plugins sharing two MIME types,
  // none when the PDF viewer is off (ForceNoPlugins, Firefox's navigator.pdfViewerEnabled). Indexed
  // items are own read-only enumerable properties, named items own unenumerable ones
  // ([LegacyUnenumerableNamedProperties]); expandos stay. X's Castle reads them as its module loads.
  if (!isWorker) {
    const states = new WeakMap();
    // the interface, its members from each object in turn, in WebIDL's order (getters copied as getters)
    const iface = (name, ...members) => {
      const C = { [name]: function () { throw new TypeError("Illegal constructor."); } }[name];
      for (const m of members) {
        for (const [key, value] of Object.entries(Object.getOwnPropertyDescriptors(m))) {
          Object.defineProperty(C.prototype, key, { ...value, enumerable: true });
        }
      }
      Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true });
      define(global, name, C);
      return C;
    };
    const state = (self, member, name) => {
      const st = self !== null && typeof self === "object" ? states.get(self) : undefined;
      if (!st || st.iface !== name) throw new TypeError(`'${member}' called on an object that does not implement interface ${name}.`);
      return st;
    };
    const args = (n, name, member) => {
      if (n < 1) throw new TypeError(`${name}.${member}: At least 1 argument required, but only 0 passed`);
    };
    // an indexed and named collection's members (IndexedGetter, NamedGetter, Length)
    const collection = name => ({
      item(index) { args(arguments.length, name, "item"); return state(this, "item", name).items[Number(index) >>> 0] || null; },
      namedItem(n) { args(arguments.length, name, "namedItem"); const st = state(this, "namedItem", name); return st.items.find(x => states.get(x).name === String(n)) || null; },
    });
    const length = name => ({ get length() { return state(this, "get length", name).items.length; } });
    const PluginArray = iface("PluginArray", { refresh() { state(this, "refresh", "PluginArray"); } }, collection("PluginArray"), length("PluginArray"));
    const Plugin = iface("Plugin", collection("Plugin"), {
      get description() { state(this, "get description", "Plugin"); return "Portable Document Format"; },
      get filename() { state(this, "get filename", "Plugin"); return "internal-pdf-viewer"; },
      get name() { return state(this, "get name", "Plugin").name; },
    }, length("Plugin"));
    const MimeTypeArray = iface("MimeTypeArray", collection("MimeTypeArray"), length("MimeTypeArray"));
    const MimeType = iface("MimeType", {
      get type() { return state(this, "get type", "MimeType").name; },
      get description() { state(this, "get description", "MimeType"); return "Portable Document Format"; },
      get suffixes() { state(this, "get suffixes", "MimeType"); return "pdf"; },
      get enabledPlugin() { return state(this, "get enabledPlugin", "MimeType").plugin; },
    });
    for (const C of [PluginArray, Plugin, MimeTypeArray]) {
      Object.defineProperty(C.prototype, Symbol.iterator, { value: Array.prototype.values, writable: true, configurable: true });
    }
    // an object of the interface, its items as own properties: indexes, then names
    const make = (C, st, items) => {
      const o = Object.create(C.prototype);
      states.set(o, Object.assign(st, { iface: C.name, items }));
      items.forEach((x, i) => Object.defineProperty(o, i, { value: x, writable: false, enumerable: true, configurable: true }));
      for (const x of items) Object.defineProperty(o, states.get(x).name, { value: x, writable: false, enumerable: false, configurable: true });
      return o;
    };
    const on = env.navigator.pdfViewerEnabled !== false;
    const names = ["PDF Viewer", "Chrome PDF Viewer", "Chromium PDF Viewer", "Microsoft Edge PDF Viewer", "WebKit built-in PDF"];
    const mimeTypes = [], plugins = [];
    const main = Object.create(Plugin.prototype);
    for (const type of ["application/pdf", "text/pdf"]) {
      const m = Object.create(MimeType.prototype);
      states.set(m, { iface: "MimeType", name: type, plugin: main });
      mimeTypes.push(m);
    }
    const mimeTypeItems = on ? mimeTypes : [];
    // each plugin's items are the MIME type array's (nsPluginElement::IndexedGetter)
    const pluginOf = (o, name) => {
      states.set(o, { iface: "Plugin", name, items: mimeTypeItems });
      mimeTypeItems.forEach((x, i) => Object.defineProperty(o, i, { value: x, writable: false, enumerable: true, configurable: true }));
      for (const x of mimeTypeItems) Object.defineProperty(o, states.get(x).name, { value: x, writable: false, enumerable: false, configurable: true });
      return o;
    };
    plugins.push(pluginOf(main, names[0]));
    for (const name of names.slice(1)) plugins.push(pluginOf(Object.create(Plugin.prototype), name));
    navigator.plugins = make(PluginArray, {}, on ? plugins : []);
    navigator.mimeTypes = make(MimeTypeArray, {}, mimeTypeItems);
  }
  define(global, "navigator", navigator);
  define(global, "screen", Object.assign({}, env.screen));
  define(global, "devicePixelRatio", 1);
  // The window's viewport in CSS px (DESIGN, "The frames' layout (S5)"): the page's is the bridge
  // tab's; a frame window's is its frame element's content box in its parent's layout (flushed), and
  // 0 by 0 while the element has no frame (nsGlobalWindowOuter::GetInnerSize with no pres shell). A
  // runtime of its own for a cross-origin frame has the frame's size as its page's (--viewport).
  const viewportSize = isFrameWindow
    ? () => { const v = __mrtViewport(global.document); return v ? { width: v[0], height: v[1] } : { width: 0, height: 0 }; }
    : () => ({ width: env.innerWidth, height: env.innerHeight });
  if (isFrameWindow) {
    // [Replaceable] attributes: a page's assignment makes an own value
    const replaceable = (name, get) => getter(global, name, get, v => define(global, name, v));
    replaceable("innerWidth", () => viewportSize().width);
    replaceable("innerHeight", () => viewportSize().height);
  } else {
    define(global, "innerWidth", env.innerWidth);
    define(global, "innerHeight", env.innerHeight);
  }
  define(global, "outerWidth", env.outerWidth ?? env.innerWidth);
  define(global, "outerHeight", env.outerHeight ?? env.innerHeight);
  // The page document's root scroll position in CSS px (task 846d): the bridge tab's in interactive
  // mode (each event the tab sends carries its offset), or where the page's own scrolling put it; a
  // frame window's document does not scroll here (0, 0). Layout reads follow it (__mrtRootScroll:
  // the canvas frame placed at minus it, so boxes, elementFromPoint and IntersectionObserver are in
  // client coordinates while fixed boxes stay). A change by the page posts one `scroll` at the
  // document for the next refresh tick (ScrollContainerFrame::PostScrollEvent), `scrollend` right
  // after it (an instant scroll ends there; Firefox delivers it before that tick's
  // IntersectionObserver entries), and numbers itself
  // (`seq`) for the tab (mirrorFlush's "scroll" op): an offset from the tab counts only once the tab
  // has applied the page's last change. Instant only: `behavior: "smooth"` jumps (GAPS B155).
  const rootScroll = { x: 0, y: 0, seq: 0, sent: 0, posted: false };
  const setRootScroll = (x, y) => {
    rootScroll.x = x;
    rootScroll.y = y;
    __mrtRootScroll(x, y);
    // the next refresh tick updates the intersection observations (the scroll moved the targets)
    if (shared.intersectionObservers) shared.intersectionObservers.scheduled = true;
  };
  // the scroll range: the scrolling element's scroll size less the scroll port (the viewport)
  const scrollRange = () => {
    const el = document.scrollingElement;
    if (!el) return [0, 0];
    const v = viewportSize();
    return [Math.max(0, el.scrollWidth - v.width), Math.max(0, el.scrollHeight - v.height)];
  };
  // ScrollContainerFrame::ScrollToCSSPixels, instant: clamped to the range, whole pixels (`round`:
  // half up, or half down for scrollIntoView, whose range lets Gecko take the lower pixel)
  function pageScrollTo(x, y, round = Math.round) {
    if (isFrameWindow || isWorker) return;
    const [mx, my] = scrollRange();
    x = round(Math.min(Math.max(0, x), mx));
    y = round(Math.min(Math.max(0, y), my));
    if (x === rootScroll.x && y === rootScroll.y) return;
    setRootScroll(x, y);
    rootScroll.seq++;
    if (rootScroll.posted) return;
    rootScroll.posted = true;
    global.__mrt.requestScroll(() => {
      rootScroll.posted = false;
      // a WidgetGUIEvent: a UIEvent, cancelable; scrollend is not cancelable
      for (const [type, cancelable] of [["scroll", true], ["scrollend", false]]) {
        const e = new UIEvent(type, { bubbles: true, cancelable, view: global });
        Object.assign(e, { isTrusted: true });
        document.dispatchEvent(e);
      }
    });
  }
  // the arguments of scroll(), scrollTo() and scrollBy() (window's and Element's): (x, y) or
  // ScrollToOptions, as [left, top] with undefined for a member not given; non-finite numbers are 0
  // (mozilla::ToZeroIfNonfinite)
  function scrollArgs(args, what) {
    const num = v => { const n = Number(v); return Number.isFinite(n) ? n : 0; };
    if (args.length >= 2) return [num(args[0]), num(args[1])];
    const o = args[0];
    if (o === undefined || o === null) return [undefined, undefined];
    if (typeof o !== "object" && typeof o !== "function") throw new TypeError(`${what}: Argument 1 can't be converted to a dictionary.`);
    return [o.left === undefined ? undefined : num(o.left), o.top === undefined ? undefined : num(o.top)];
  }
  const scrollTo = (args, what) => { const [l, t] = scrollArgs(args, what); pageScrollTo(l ?? rootScroll.x, t ?? rootScroll.y); };
  const scrollBy = (args, what) => { const [l, t] = scrollArgs(args, what); pageScrollTo(rootScroll.x + (l ?? 0), rootScroll.y + (t ?? 0)); };
  {
    // [Replaceable] attributes: a page's assignment makes an own value
    const replaceable = (name, get) => getter(global, name, get, v => define(global, name, v));
    replaceable("scrollX", () => rootScroll.x);
    replaceable("scrollY", () => rootScroll.y);
    replaceable("pageXOffset", () => rootScroll.x);
    replaceable("pageYOffset", () => rootScroll.y);
  }
  // a frame's about:blank or about:srcdoc has its parent's origin (the principal it inherits)
  getter(global, "origin", () => (isFrameWindow && pageURL.protocol === "about:" ? global.parent.origin : pageURL.origin));
  // isSecureContext and crossOriginIsolated are defined by the C++ layer from the realm's options
  // (Firefox's answers for the page, mrt_js_new)
  // top, parent, frames, opener and closed are prelude.js's (the windows' relations)
  // window.name, the window's browsing-context name, kept by window in the windows' shared state so a
  // parent finds its frames by name whatever their origin (the named properties, "frames by name"
  // below). A frame's starts as its element's name attribute; Gecko keeps it in the BrowsingContext,
  // so a frame's navigations keep it (openSameSiteFrame carries it to a new window). `version` counts
  // the changes, for the parents' caches.
  const contextNames = share("contextNames", () => ({ byWindow: new WeakMap(), version: 0 }));
  const setContextName = (win, name) => { contextNames.byWindow.set(win, String(name)); contextNames.version++; };
  if (!isWorker) {
    if (isFrameWindow) {
      // (the initial about:blank is of its parent's origin, so its frame element is visible here)
      const frame = __mrtWindows(3);
      if (frame) setContextName(global, Element.prototype.getAttribute.call(frame, "name") ?? "");
    }
    getter(global, "name", () => contextNames.byWindow.get(global) ?? "", v => setContextName(global, v));
  }

  // ---- base64 and text encoding ----

  const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  define(global, "btoa", s => {
    s = String(s);
    let out = "";
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
      if (a > 255 || b > 255 || c > 255) throw new Error("InvalidCharacterError");
      const n = (a << 16) | ((b || 0) << 8) | (c || 0);
      out += B64[n >> 18] + B64[(n >> 12) & 63] + (i + 1 < s.length ? B64[(n >> 6) & 63] : "=") + (i + 2 < s.length ? B64[n & 63] : "=");
    }
    return out;
  });
  define(global, "atob", s => {
    s = String(s).replace(/[\s=]/g, "");
    let out = "", bits = 0, n = 0;
    for (const ch of s) {
      const v = B64.indexOf(ch === "-" ? "+" : ch === "_" ? "/" : ch);
      if (v < 0) throw new Error("InvalidCharacterError");
      n = (n << 6) | v;
      bits += 6;
      if (bits >= 8) { bits -= 8; out += String.fromCharCode((n >> bits) & 255); }
    }
    return out;
  });
  // TextEncoder and TextDecoder are in the prelude

  // ---- fetch and XMLHttpRequest ----

  class Headers {
    constructor(init) {
      this._map = new Map();
      if (init instanceof Headers) init.forEach((v, k) => this.append(k, v));
      else if (Array.isArray(init)) for (const [k, v] of init) this.append(k, v);
      else if (init && typeof init === "object") for (const k of Object.keys(init)) this.append(k, init[k]);
    }
    append(k, v) { k = String(k).toLowerCase(); this._map.set(k, this._map.has(k) ? this._map.get(k) + ", " + v : String(v)); }
    set(k, v) { this._map.set(String(k).toLowerCase(), String(v)); }
    get(k) { const v = this._map.get(String(k).toLowerCase()); return v === undefined ? null : v; }
    has(k) { return this._map.has(String(k).toLowerCase()); }
    delete(k) { this._map.delete(String(k).toLowerCase()); }
    forEach(f, self) { for (const [k, v] of this._map) f.call(self, v, k, this); }
    keys() { return this._map.keys(); }
    values() { return this._map.values(); }
    entries() { return this._map.entries(); }
    [Symbol.iterator]() { return this._map.entries(); }
  }
  const bodyText = body => (body == null ? null : typeof body === "string" ? body : body instanceof URLSearchParams ? body.toString() : String(body));
  // Bodies keep their bytes (Gecko's BodyExtractor): an ArrayBuffer or a view is copied, text stays
  // a string until bytes are asked for; binary answers cross the bridge as base64 (YouTube's video
  // segments, which the page hands to a SourceBuffer)
  const bytesOf = v => (v instanceof ArrayBuffer ? new Uint8Array(v.slice(0))
    : ArrayBuffer.isView(v) ? new Uint8Array(v.buffer.slice(v.byteOffset, v.byteOffset + v.byteLength))
    : blobImplOf(v) ? readBlobImpl(blobImplOf(v)) : null);
  // BodyExtractor's content type for a Blob body: its type, when it has one
  const setBlobContentType = (headers, body) => {
    const impl = blobImplOf(body);
    if (impl && impl.contentType && !headers.has("content-type")) headers.set("content-type", impl.contentType);
  };
  // a body's bytes (a Request's or a Response's), and a Blob of them typed by the content type
  const bodyBlob = (bytes, headers) => createBlob(new MemoryBlobImpl(bytes, makeValidBlobType(headers.get("content-type") || "")));
  const fromBase64 = s => { const bin = atob(s), b = new Uint8Array(bin.length); for (let i = 0; i < bin.length; i++) b[i] = bin.charCodeAt(i); return b; };
  const toBase64 = b => { let bin = ""; for (let i = 0; i < b.length; i += 0x8000) bin += String.fromCharCode.apply(null, b.subarray(i, i + 0x8000)); return btoa(bin); };
  const bodyAsText = b => (typeof b === "string" ? b : new TextDecoder().decode(b));
  const bodyAsBytes = b => (typeof b === "string" ? new TextEncoder().encode(b) : b);
  // ---- the encodings XMLHttpRequest decodes its text with (Encoding::ForLabel, the Encoding
  // Standard's labels). Only these: a label of another legacy encoding (Shift_JIS, GBK, ...) is
  // decoded as UTF-8 (GAPS B127) ----
  const ENCODING_LABELS = {
    "utf-8": ["unicode-1-1-utf-8", "unicode11utf8", "unicode20utf8", "utf-8", "utf8", "x-unicode20utf8"],
    "windows-1252": ["ansi_x3.4-1968", "ascii", "cp1252", "cp819", "csisolatin1", "ibm819", "iso-8859-1", "iso-ir-100",
      "iso8859-1", "iso88591", "iso_8859-1", "iso_8859-1:1987", "l1", "latin1", "us-ascii", "windows-1252", "x-cp1252"],
    "utf-16le": ["csunicode", "iso-10646-ucs-2", "ucs-2", "unicode", "unicodefeff", "utf-16", "utf-16le"],
    "utf-16be": ["unicodefffe", "utf-16be"],
    "x-user-defined": ["x-user-defined"],
  };
  const encodingOfLabel = new Map();
  for (const [name, labels] of Object.entries(ENCODING_LABELS)) for (const l of labels) encodingOfLabel.set(l, name);
  // windows-1252's 0x80 to 0x9F (the rest of its bytes are their code points)
  const WINDOWS_1252_HIGH = [0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039,
    0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc, 0x2122, 0x161, 0x203a,
    0x153, 0x9d, 0x17e, 0x178];
  // String.fromCharCode over an array of UTF-16 code units, in slices that fit the argument limit
  const fromCodeUnits = units => {
    let s = "";
    for (let i = 0; i < units.length; i += 0x8000) s += String.fromCharCode.apply(null, units.subarray(i, i + 0x8000));
    return s;
  };
  // Decode a whole body as Encoding::NewDecoder does: a BOM picks UTF-8 or UTF-16 over `encoding`
  // and is dropped (with `sniffBOM` false, only a BOM of `encoding` itself is dropped:
  // NewDecoderWithBOMRemoval); malformed input becomes U+FFFD.
  const decodeBody = (bytes, encoding, sniffBOM) => {
    let start = 0;
    if (bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf && (sniffBOM || encoding === "utf-8")) { encoding = "utf-8"; start = 3; }
    else if (bytes[0] === 0xfe && bytes[1] === 0xff && (sniffBOM || encoding === "utf-16be")) { encoding = "utf-16be"; start = 2; }
    else if (bytes[0] === 0xff && bytes[1] === 0xfe && (sniffBOM || encoding === "utf-16le")) { encoding = "utf-16le"; start = 2; }
    const b = bytes.subarray(start);
    if (encoding === "utf-8") return new TextDecoder("utf-8", { ignoreBOM: true }).decode(b);
    if (encoding === "x-user-defined" || encoding === "windows-1252") {
      const units = new Uint16Array(b.length);
      for (let i = 0; i < b.length; i++) {
        const c = b[i];
        units[i] = c < 0x80 ? c : encoding === "x-user-defined" ? 0xf700 + c : c < 0xa0 ? WINDOWS_1252_HIGH[c - 0x80] : c;
      }
      return fromCodeUnits(units);
    }
    // UTF-16: code units as they come, a lone surrogate U+FFFD; an odd last byte is one U+FFFD, also
    // with a lead surrogate before it (the Encoding Standard's end of queue with both pending)
    let n = b.length >> 1;
    const le = encoding === "utf-16le", unit = i => (le ? b[2 * i] | (b[2 * i + 1] << 8) : (b[2 * i] << 8) | b[2 * i + 1]);
    const odd = b.length & 1;
    if (odd && n > 0 && (unit(n - 1) & 0xfc00) === 0xd800) n--;
    const units = new Uint16Array(n);
    for (let i = 0; i < n; i++) units[i] = unit(i);
    return fromCodeUnits(units).toWellFormed() + (odd ? "�" : "");
  };
  // A Content-Type's charset parameter (net_ParseContentType), or null
  const charsetOf = contentType => {
    for (const param of String(contentType || "").split(";").slice(1)) {
      const eq = param.indexOf("=");
      if (eq < 0 || param.slice(0, eq).trim().toLowerCase() !== "charset") continue;
      return param.slice(eq + 1).trim().replace(/^"([^]*)"$/, "$1");
    }
    return null;
  };
  const freshBuffer = b => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
  // a body read from a ReadableStream (Gecko's BodyConsumer over the stream's reader): Uint8Array
  // chunks only; an errored stream gives the consumer's abort
  const readBodyStream = stream => {
    const reader = stream.getReader(), chunks = [];
    const pump = () => reader.read().then(({ value, done }) => {
      if (done) {
        const out = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
        let at = 0;
        for (const c of chunks) { out.set(c, at); at += c.length; }
        return out;
      }
      if (!(value instanceof Uint8Array)) {
        const e = new TypeError("Can't convert value to Uint8Array while consuming Body");
        reader.cancel(e);
        throw e;
      }
      chunks.push(value.slice());
      return pump();
    }, () => { throw new DOMException("The operation was aborted. ", "AbortError"); });
    return pump();
  };
  class Response {
    constructor(body = null, init = {}) {
      // a ReadableStream body (BodyExtractor's stream case): read when the body is consumed
      if (body !== null && typeof body === "object" && global.__mrt.isReadableStream(body)) {
        if (body._disturbed || body._reader) throw new TypeError("Response constructor: Body has already been consumed.");
        this._stream = body;
        this._fromStream = true;
        body = null;
      }
      this._body = init.bodyBase64 != null ? fromBase64(init.bodyBase64) : body == null ? "" : bytesOf(body) || bodyText(body);
      this.status = init.status === undefined ? 200 : init.status;
      this.statusText = init.statusText || "";
      this.headers = new Headers(init.headers);
      setBlobContentType(this.headers, body);
      this.url = init.url || "";
      this.redirected = !!init.redirected;
      this.type = init.type || "basic";
      this.bodyUsed = false;
    }
    get ok() { return this.status >= 200 && this.status < 300; }
    // the body as a stream of one chunk, its bytes; null for a response without a body (a null
    // body status)
    get body() {
      if (this.type === "opaque" || [101, 103, 204, 205, 304].includes(this.status)) return null;
      if (!this._stream) {
        const bytes = bodyAsBytes(this._body);
        this._stream = new ReadableStream({ start(c) { if (bytes.length) c.enqueue(bytes); c.close(); } });
      }
      return this._stream;
    }
    _consume(method) {
      if (this._stream && (this._stream._disturbed || (this._fromStream && this._stream._reader))) {
        return Promise.reject(new TypeError(`Response.${method}: Body has already been consumed.`));
      }
      this.bodyUsed = true;
      return this._fromStream ? readBodyStream(this._stream) : Promise.resolve(this._body);
    }
    text() { return this._consume("text").then(bodyAsText); }
    json() { return this._consume("json").then(b => JSON.parse(bodyAsText(b))); }
    arrayBuffer() { return this._consume("arrayBuffer").then(b => freshBuffer(bodyAsBytes(b))); }
    bytes() { return this._consume("bytes").then(b => new Uint8Array(freshBuffer(bodyAsBytes(b)))); }
    blob() { return this._consume("blob").then(b => bodyBlob(bodyAsBytes(b), this.headers)); }
    clone() {
      let body = this._body;
      if (this._fromStream) [this._stream, body] = this._stream.tee();
      return new Response(body, { status: this.status, statusText: this.statusText, headers: this.headers, url: this.url });
    }
  }
  // The request lives in a private field, as Gecko's Request holds an InternalRequest; the
  // attributes are getters on the prototype over it. Request::Constructor and FetchRequest take
  // an input Request's internal request, never its JS properties: YouTube's ad-blocker check
  // fetches new Request("data:...") with url, method and mode redefined on the instance, and
  // Firefox fetches the data: URL
  let internalRequest;
  class Request {
    #request;
    static { internalRequest = r => r.#request; }
    constructor(input, init = {}) {
      const inputReq = input instanceof Request ? input.#request : null;
      const from = (k, d) => (init[k] !== undefined ? init[k] : inputReq ? inputReq[k] : d);
      this.#request = {
        url: inputReq ? inputReq.url : resolve(input, pageURL.href),
        method: (init.method || (inputReq ? inputReq.method : "GET")).toUpperCase(),
        headers: new Headers(init.headers || (inputReq ? inputReq.headers : undefined)),
        body: init.body !== undefined ? init.body : inputReq ? inputReq.body : null,
        credentials: from("credentials", "same-origin"),
        mode: from("mode", "cors"),
        cache: from("cache", "default"),
        redirect: from("redirect", "follow"),
        referrerPolicy: from("referrerPolicy", ""),
        keepalive: !!from("keepalive", false),
      };
      // Request's constructor (MSG_ONLY_IF_CACHED_WITHOUT_SAME_ORIGIN)
      if (this.#request.cache === "only-if-cached" && this.#request.mode !== "same-origin")
        throw new TypeError(`Request constructor: Request mode '${this.#request.mode}' was used, but request cache mode 'only-if-cached' can only be used with request mode 'same-origin'.`);
      setBlobContentType(this.#request.headers, this.#request.body);
      // a blob: URL's revoked flag, taken when it is parsed (BlobURLProtocolHandler::CreateNewURI)
      if (isBlobURL(this.#request.url)) this.#request.blobLive = inputReq ? inputReq.blobLive : __mrtBlobURL(3, this.#request.url);
    }
    get url() { return this.#request.url; }
    get method() { return this.#request.method; }
    get headers() { return this.#request.headers; }
    get credentials() { return this.#request.credentials; }
    get mode() { return this.#request.mode; }
    get cache() { return this.#request.cache; }
    get redirect() { return this.#request.redirect; }
    get referrerPolicy() { return this.#request.referrerPolicy; }
    get keepalive() { return this.#request.keepalive; }
    get bodyUsed() { return false; }
    clone() { return new Request(this); }
    // the body mixin, over the body the request was made with
    _bodyBytes() { const b = this.#request.body; return b == null ? new Uint8Array(0) : bytesOf(b) || bodyAsBytes(bodyText(b)); }
    text() { return Promise.resolve(bodyAsText(this._bodyBytes())); }
    json() { return this.text().then(JSON.parse); }
    arrayBuffer() { return Promise.resolve(freshBuffer(this._bodyBytes())); }
    bytes() { return Promise.resolve(new Uint8Array(freshBuffer(this._bodyBytes()))); }
    blob() { return Promise.resolve(bodyBlob(this._bodyBytes(), this.#request.headers)); }
  }
  // credentials as the page asked: fetch's default is same-origin, XHR's include only with
  // withCredentials (a credentialed request fails CORS where the server answers "*")
  // the request's mode, cache, redirect, referrer policy and keepalive go with it: a no-cors request
  // (analytics pings) gets an opaque response in Firefox where a cors one would fail
  // A network answer reaches the page `ms` after its request on the virtual clock (SPEC decision 20):
  // the recorded duration in a replay, else the time the bridge tab's fetch took (mini_realsites.py
  // op). As a task, as Gecko's network events are, not as a page timeout (no nesting clamp).
  const clockNow = global.performance.now;
  const afterNetwork = (r, fn) => global.__mrt.queueAt(fn, clockNow() + ((r && r.ms) || 0));
  // the tag of the worker whose blob: URLs this global makes ("" in a window): its end revokes them
  let blobURLOwner = "";
  // A blob: URL loaded from this global (BlobURLChannel): GET only, from the origin that made it; a
  // URL revoked since it was parsed loads for RELEASING_TIMER more (`parsedLive`, the URL's revoked
  // flag when its Request or XHR was made). A response {status, headers, bytes, ...} or null.
  const blobResponse = (url, method, parsedLive) => {
    if (method !== "GET") return null;
    const entry = __mrtBlobURL(2, url, global.origin, !!parsedLive, clockNow());
    if (!entry) return null;
    const [bytes, type] = entry, length = String(bytes.length);
    return { status: 200, statusText: "OK", url: url.replace(/#[^]*$/, ""), type: "basic", bytes,
      headers: [["content-type", type], ["content-length", length]], rawHeaders: [["Content-Type", type], ["Content-Length", length]] };
  };
  const isBlobURL = url => /^blob:/i.test(url);
  function forward(url, method, headers, body, credentials, options = {}) {
    const bytes = bytesOf(body);
    const sent = bytes ? { body: null, bodyBase64: toBase64(bytes) } : { body: bodyText(body) };
    // BodyExtractor's type for URLSearchParams, when the page set none
    const typed = body instanceof URLSearchParams && !headers.has("content-type")
      ? [["content-type", "application/x-www-form-urlencoded;charset=UTF-8"]] : [];
    const request = Object.assign({ url, method, headers: [...headers, ...typed], credentials }, sent, options);
    if (options.async && env.fetchAsync) request.call = "start";
    return bridge("fetch", request);
  }
  // Live native jobs retain their response past the old five-second cutoff.
  // Poll in timer tasks so input, other APIs and viewer replacement keep running.
  function receiveResponse(initial, aborted = () => false) {
    return new Promise((resolve, reject) => {
      const id = initial?.job;
      const poll = () => {
        try {
          if (aborted()) {
            if (id) bridge("fetch", {call: "cancel", id});
            resolve(null); return;
          }
          const response = id ? bridge("fetch", {call: "poll", id}) : initial;
          if (response?.job) { global.__mrt.queueAt(poll, clockNow() + 100); return; }
          // Legacy measuring/privileged bridge long polls still have no late delivery.
          if (response?.pending) return;
          afterNetwork(response, () => resolve(response));
        } catch (error) { reject(error); }
      };
      if (id) global.__mrt.queueAt(poll, clockNow()); else poll();
    });
  }
  function fetch(input, init = {}) {
    try {
      const req = internalRequest(new Request(input, init));
      if (isBlobURL(req.url)) {
        const r = blobResponse(req.url, req.method, req.blobLive);
        if (!r) throw new TypeError("NetworkError when attempting to fetch resource.");
        return new Promise(resolve => afterNetwork(r, () => resolve(new Response(r.bytes, r))));
      }
      const r = forward(req.url, req.method, req.headers, req.body, req.credentials,
        { bytes: true, async: true, mode: req.mode, cache: req.cache, redirect: req.redirect, referrerPolicy: req.referrerPolicy, keepalive: req.keepalive });
      if (r.job) return receiveResponse(r).then(response => new Response(response.body, response));
      if (r.pending) return new Promise(() => {});
      return new Promise(resolve => afterNetwork(r, () => resolve(new Response(r.body, r))));
    } catch (e) {
      return Promise.reject(e);
    }
  }
  define(global, "Headers", Headers);
  define(global, "Response", Response);
  define(global, "Request", Request);
  define(global, "fetch", fetch);

  // XMLHttpRequest's interfaces as Gecko's (XMLHttpRequestEventTarget.webidl): XMLHttpRequest and
  // XMLHttpRequestUpload inherit the progress event handlers from XMLHttpRequestEventTarget, an
  // EventTarget that cannot be constructed
  const xhrToken = {};
  class XMLHttpRequestEventTarget extends EventTarget {
    constructor(token) {
      if (token !== xhrToken) throw new TypeError("Illegal constructor.");
      super();
    }
  }
  global.__mrt.defineEventHandlers(XMLHttpRequestEventTarget.prototype,
    ["onloadstart", "onprogress", "onabort", "onerror", "onload", "ontimeout", "onloadend"]);
  class XMLHttpRequestUpload extends XMLHttpRequestEventTarget {}
  class XMLHttpRequest extends XMLHttpRequestEventTarget {
    constructor() {
      super(xhrToken);
      this.readyState = 0;
      this.status = 0;
      this.statusText = "";
      this.responseText = "";
      this.response = "";
      this.responseType = "";
      this.responseURL = "";
      this.withCredentials = false;
      this.timeout = 0;
      this._headers = new Headers();
      this._responseHeaders = new Headers();
      // nothing is sent as a body stream, so it gets no events
      this.upload = new XMLHttpRequestUpload(xhrToken);
    }
    open(method, url, async = true) {
      this._fetchGeneration = null;
      this._method = String(method).toUpperCase();
      this._url = resolve(url, pageURL.href);
      this._blobLive = isBlobURL(this._url) && __mrtBlobURL(3, this._url);
      this._async = async !== false;
      this.readyState = 1;
    }
    setRequestHeader(k, v) { this._headers.append(k, v); }
    getResponseHeader(k) { return this._responseHeaders.get(k); }
    getAllResponseHeaders() { return (this._rawHeaders || [...this._responseHeaders]).map(([k, v]) => `${k}: ${v}\r\n`).join(""); }
    // XMLHttpRequestMainThread::OverrideMimeType: kept for the text's charset (DetectCharset); the
    // type itself changes nothing here, as responseXML is not made
    overrideMimeType(mime) {
      if (arguments.length < 1) throw new TypeError("XMLHttpRequest.overrideMimeType: At least 1 argument required, but only 0 passed");
      if (this.readyState === 3 || this.readyState === 4)
        throw new DOMException("Cannot call 'overrideMimeType()' on XMLHttpRequest after 'send()' (when its state is LOADING or DONE).", "InvalidStateError");
      this._overrideMimeType = String(mime);
    }
    // DetectCharset: the charset of the final MIME type (the overriding type's when it has one, as
    // HttpBaseChannel::SetContentType keeps the response's otherwise), UTF-8 when there is none or
    // it is unknown; "json" is always UTF-8 and only its own BOM is dropped
    _decodeText(bytes) {
      if (this.responseType === "json") return decodeBody(bytes, "utf-8", false);
      const label = charsetOf(this._overrideMimeType) ?? charsetOf(this._responseHeaders.get("content-type"));
      return decodeBody(bytes, encodingOfLabel.get(String(label ?? "").trim().toLowerCase()) || "utf-8", true);
    }
    abort() { this._aborted = true; }
    send(body = null) {
      // the request goes out at send(), as in Firefox; an async one completes when its answer arrives
      // (afterNetwork), a sync one at once
      const request = () => {
        if (isBlobURL(this._url)) return blobResponse(this._url, this._method, this._blobLive);
        // Keep bytes until the final response type and charset are known.
        try { return forward(this._url, this._method, this._headers, body, this.withCredentials ? "include" : "same-origin", { bytes: true, async: this._async }); }
        catch (e) { console.error("XMLHttpRequest:", e); return null; }
      };
      const complete = r => {
        if (this._aborted) return;
        // a long poll Firefox has not answered: the request stays in flight, no events
        if (r && r.pending) return;
        // an async answer goes through HEADERS_RECEIVED and LOADING first, each with its
        // readystatechange (XMLHttpRequestMainThread::ChangeState)
        if (r && this._async) {
          this.readyState = 2;
          this.status = r.status;
          this.statusText = r.statusText;
          this.responseURL = r.url;
          this._responseHeaders = new Headers(r.headers);
          this._rawHeaders = r.rawHeaders || null;
          this.dispatchEvent(new Event("readystatechange"));
          if (this._aborted) return;
          this.readyState = 3;
          this.dispatchEvent(new Event("readystatechange"));
          if (this._aborted) return;
        }
        this.readyState = 4;
        if (r) {
          this.status = r.status;
          this.statusText = r.statusText;
          this.responseURL = r.url;
          this._responseHeaders = new Headers(r.headers);
          this._rawHeaders = r.rawHeaders || null;
          const bytes = r.bodyBase64 != null ? fromBase64(r.bodyBase64) : r.bytes || null;
          this.responseText = bytes ? this._decodeText(bytes) : r.body == null ? "" : r.body;
          if (this.responseType === "json") { try { this.response = JSON.parse(this.responseText); } catch (e) { this.response = null; } }
          else if (this.responseType === "arraybuffer") this.response = freshBuffer(bytes || bodyAsBytes(this.responseText));
          else if (this.responseType === "blob") this.response = bodyBlob(bytes || bodyAsBytes(this.responseText), this._responseHeaders);
          else this.response = this.responseText;
        }
        for (const type of ["readystatechange", r ? "load" : "error", "loadend"]) {
          this.dispatchEvent(new Event(type));
        }
      };
      if (this._async) {
        const unblock = blockOnload("xhr " + this._url);
        const r = request();
        if (r?.job) {
          const generation = this._fetchGeneration = {};
          this._aborted = false;
          receiveResponse(r, () => this._aborted || this._fetchGeneration !== generation).then(response => {
            if (this._fetchGeneration === generation) complete(response);
          }, error => {
            if (this._fetchGeneration !== generation || this._aborted) return;
            console.error("XMLHttpRequest:", error); complete(null);
          }).finally(unblock);
        } else {
          if (r && r.pending) { unblock(); return; }
          afterNetwork(r, () => { try { complete(r); } finally { unblock(); } });
        }
      } else complete(request());
    }
  }
  global.__mrt.defineEventHandlers(XMLHttpRequest.prototype, ["onreadystatechange"]);
  Object.assign(XMLHttpRequest, { UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 });
  for (const C of [XMLHttpRequestEventTarget, XMLHttpRequestUpload, XMLHttpRequest]) {
    Object.defineProperty(C.prototype, Symbol.toStringTag, { value: C.name, configurable: true });
    define(global, C.name, C);
  }

  // ---- WebSocket, forwarded to Firefox (GAPS B70; DESIGN "WebSocket, forwarded to Firefox") ----
  //
  // The connection is a real WebSocket of the bridge tab (mini_realsites.py WS_SCRIPT, sandbox "ws"),
  // made in this window's context, so Firefox checks the URL and protocols, connects, and fails as it
  // does for the page. Here, as Gecko's WebSocket: readyState, the events as tasks in Gecko's order,
  // Send's and Close's checks. Firefox's events come back with their time since the connection was
  // made; each is delivered at that time on the virtual clock, never earlier than now. While a
  // connection is not closed this window asks Firefox for new events every WS_POLL_MS.
  // What Gecko throws for an nsresult that is not a DOM error (a WebSocket that CSP refuses:
  // NS_ERROR_CONTENT_BLOCKED): an Exception (dom/bindings/Exceptions.cpp, Exceptions.webidl), an
  // interface with no interface object, so its prototype's constructor is Object's. Its `message` is
  // the one the code gave (none here); its stringifier falls back to the nsresult's default text.
  const geckoException = (() => {
    const members = new WeakMap();
    const get = (o, k) => { const m = members.get(o); if (!m) throw new TypeError("'" + k + "' getter called on an object that does not implement interface Exception."); return m[k]; };
    const proto = {};
    for (const k of ["message", "result", "name", "filename", "lineNumber", "columnNumber", "data", "stack"])
      Object.defineProperty(proto, k, { get: Object.defineProperty(function () { return get(this, k); }, "name", { value: "get " + k }), enumerable: true, configurable: true });
    Object.defineProperty(proto, "toString", { value: function toString() {
      const m = members.get(this);
      if (!m) throw new TypeError("'toString' called on an object that does not implement interface Exception.");
      const location = m.filename ? `JS frame :: ${m.filename} :: ${m.caller} :: line ${m.lineNumber}` : "<unknown>";
      return `[Exception... "${m.message || m.defaultMessage}"  nsresult: "0x${(m.result >>> 0).toString(16)} (${m.name})"  location: "${location}"  data: no]`;
    }, writable: true, enumerable: true, configurable: true });
    Object.defineProperty(proto, Symbol.toStringTag, { value: "Exception", configurable: true });
    return (name, result, defaultMessage) => {
      const e = Object.create(proto);
      const stack = String(new Error().stack).split("\n").filter(f => f && !/@(bridge|prelude)\.js:/.test(f));
      const at = stack.map(f => /^(.*)@(.*):(\d+):(\d+)$/.exec(f)).find(Boolean);
      members.set(e, { name, result, message: "", defaultMessage, data: null, stack: stack.map(f => f + "\n").join(""),
                       filename: at ? at[2] : "", lineNumber: at ? +at[3] : 0, columnNumber: at ? +at[4] : 0, caller: at ? at[1] || "<TOP_LEVEL>" : "" });
      return e;
    };
  })();
  const WS_POLL_MS = 250;
  const CONNECTING = 0, OPEN = 1, CLOSING = 2, CLOSED = 3;
  const wsStates = new WeakMap();
  const liveSockets = new Set();
  let wsPolling = false;
  const wsState = (ws, member) => {
    const st = wsStates.get(ws);
    if (!st) throw new TypeError(`'${member}' called on an object that does not implement interface WebSocket.`);
    return st;
  };
  const wsBridge = (call, args) => {
    const r = bridge("ws", Object.assign({ call }, args));
    if (r && r.throws && r.throws.result !== undefined) throw geckoException(r.throws.name, r.throws.result, r.throws.message);
    if (r && r.throws) throw new DOMException(r.throws.message, r.throws.name);
    return r;
  };
  // Firefox's events for a connection, each a task at its time (WebSocketImpl's runnables)
  function deliverWsEvents(ws, events) {
    const st = wsStates.get(ws);
    for (const e of events) {
      if (e.type === "error" || e.type === "close") st.closedInFirefox = true;
      const at = Math.max(clockNow(), st.madeAt + e.ms);
      st.lastAt = Math.max(st.lastAt || 0, at);
      global.__mrt.queueAt(() => { if (!st.failed) dispatchWsEvent(ws, e); }, at);
    }
    if (st.closedInFirefox) liveSockets.delete(ws);
  }
  function dispatchWsEvent(ws, e) {
    const st = wsStates.get(ws);
    if (st.closeDispatched) return;
    if (e.type === "open") {
      // WebSocketImpl::OnStart: the established protocol and extensions, then OPEN, then the event
      st.protocol = e.protocol || "";
      st.extensions = e.extensions || "";
      if (st.readyState === CONNECTING) st.readyState = OPEN;
      ws.dispatchEvent(new Event("open"));
    } else if (e.type === "message") {
      // DoOnMessageAvailable: dispatched only while OPEN; dropped while CLOSING
      if (st.readyState !== OPEN) return;
      let data = e.data;
      if (e.binary) {
        const bytes = fromBase64(e.data);
        data = st.binaryType === "arraybuffer" ? freshBuffer(bytes) : createBlob(new MemoryBlobImpl(bytes, ""));
      }
      ws.dispatchEvent(new MessageEvent("message", { data, origin: st.origin }));
    } else if (e.type === "error") {
      // DispatchConnectionCloseEvents: CLOSED, then error (a failed connection), then close
      st.readyState = CLOSED;
      ws.dispatchEvent(new Event("error"));
    } else if (e.type === "close") {
      st.readyState = CLOSED;
      st.closeDispatched = true;
      ws.dispatchEvent(new CloseEvent("close", { wasClean: !!e.wasClean, code: e.code | 0, reason: e.reason || "" }));
    }
  }
  // The bridge tab dropped (bridge()'s bridgeLost): its sockets went with it. Each live connection
  // ends as one whose TCP connection the network ended without a close frame does in Firefox
  // (WebSocketImpl::OnStop with NS_BASE_STREAM_CLOSED: CLOSED, no error event, close 1006 not clean),
  // after the events Firefox had already received for it
  bridgeLostHooks.push(() => {
    for (const ws of [...liveSockets]) {
      const st = wsStates.get(ws);
      liveSockets.delete(ws);
      st.closedInFirefox = true;
      calls["ws-lost"] = (calls["ws-lost"] || 0) + 1;
      global.__mrt.queueAt(() => { if (!st.failed) dispatchWsEvent(ws, { type: "close", code: 1006, reason: "", wasClean: false }); },
                           Math.max(clockNow(), st.lastAt || 0));
    }
  });
  function pollWebSockets() {
    wsPolling = false;
    if (!liveSockets.size) return;
    let r;
    // with each connection's age on this clock: the API process waits until the network has had as
    // long (mrt-api ws.rs; the bridge tab ignores it)
    const live = [...liveSockets].map(ws => wsStates.get(ws));
    try { r = wsBridge("poll", { ids: live.map(st => st.id), ages: live.map(st => clockNow() - st.madeAt) }); }
    catch (e) { console.error("WebSocket poll:", e); return; }
    for (const ws of [...liveSockets]) {
      const events = r[wsStates.get(ws).id];
      if (events && events.length) deliverWsEvents(ws, events);
    }
    scheduleWsPoll();
  }
  function scheduleWsPoll() {
    if (wsPolling || !liveSockets.size) return;
    wsPolling = true;
    global.__mrt.queueAt(pollWebSockets, clockNow() + WS_POLL_MS);
  }
  class WebSocketImpl extends EventTarget {
    constructor(url) {
      const protocols = arguments[1];
      if (arguments.length < 1) throw new TypeError("WebSocket constructor: At least 1 argument required, but only 0 passed");
      super();
      url = String(url);
      // a relative URL against the document's (WebSocketImpl::ParseURL); the rest is Firefox's to check
      if (!/^[a-z][a-z0-9+.-]*:/i.test(url)) { try { url = resolve(url, pageURL.href); } catch (e) {} }
      const list = protocols === undefined ? undefined : typeof protocols === "string" ? [protocols] : Array.from(protocols, String);
      const r = wsBridge("open", list === undefined ? { url } : { url, protocols: list });
      const st = { id: r.id, url: r.url, readyState: CONNECTING, protocol: "", extensions: "", binaryType: "blob",
                   bufferedAmount: 0, madeAt: clockNow(), closedInFirefox: false, closeDispatched: false, failed: false, origin: "" };
      // a message event's origin is the connection URL's (WebSocketImpl::mUTF16Origin)
      try { st.origin = new URL(r.url).origin; } catch (e) {}
      wsStates.set(this, st);
      liveSockets.add(this);
      deliverWsEvents(this, r.events || []);
      scheduleWsPoll();
    }
    get url() { return wsState(this, "get url").url; }
    get readyState() { return wsState(this, "get readyState").readyState; }
    get bufferedAmount() { return wsState(this, "get bufferedAmount").bufferedAmount; }
    get extensions() { return wsState(this, "get extensions").extensions; }
    get protocol() { return wsState(this, "get protocol").protocol; }
    get binaryType() { return wsState(this, "get binaryType").binaryType; }
    set binaryType(v) {
      const st = wsState(this, "set binaryType");
      // an enumeration attribute: a value outside it is ignored
      if (v === "blob" || v === "arraybuffer") st.binaryType = v;
    }
    // WebSocket::Send: InvalidStateError while CONNECTING; counted in bufferedAmount and dropped once
    // CLOSING or CLOSED; else sent by Firefox's socket (acknowledged at once here)
    send(data) {
      const st = wsState(this, "send");
      if (arguments.length < 1) throw new TypeError("WebSocket.send: At least 1 argument required, but only 0 passed");
      if (st.readyState === CONNECTING) throw new DOMException("An attempt was made to use an object that is not, or is no longer, usable", "InvalidStateError");
      let bytes = bytesOf(data), text = null;
      if (!bytes) text = String(data);
      const length = bytes ? bytes.length : bodyAsBytes(text).length;
      if (st.readyState === CLOSING || st.readyState === CLOSED) { st.bufferedAmount += length; return; }
      // closed in Firefox, its close not delivered yet: nothing to send it on
      if (st.closedInFirefox) return;
      wsBridge("send", bytes ? { id: st.id, dataBase64: toBase64(bytes) } : { id: st.id, data: text });
    }
    // WebSocket::Close: the code and reason checked, nothing once CLOSING or CLOSED; else CLOSING and
    // Firefox's socket closes (or fails, while connecting), its events coming back as usual
    close(code, reason) {
      const st = wsState(this, "close");
      if (code !== undefined) {
        code = Number(code) & 0xffff;
        if (code !== 1000 && (code < 3000 || code > 4999)) throw new DOMException("A parameter or an operation is not supported by the underlying object", "InvalidAccessError");
      }
      if (reason !== undefined && bodyAsBytes(String(reason)).length > 123) throw new DOMException("An invalid or illegal string was specified", "SyntaxError");
      if (st.readyState === CLOSING || st.readyState === CLOSED) return;
      const connecting = st.readyState === CONNECTING;
      st.readyState = CLOSING;
      if (connecting) {
        // FailConnection: the connection fails here, whatever Firefox's socket has reached meanwhile
        // (it may be open already: its open event is not delivered yet); error and close 1006 come as
        // a task, and Firefox's own events for it are dropped
        st.failed = true;
        liveSockets.delete(this);
        global.__mrt.queueTask(() => {
          dispatchWsEvent(this, { type: "error" });
          dispatchWsEvent(this, { type: "close", code: 1006, reason: "", wasClean: false });
        });
      }
      if (st.closedInFirefox) return;
      const args = { id: st.id };
      if (code !== undefined) args.code = code;
      if (reason !== undefined) args.reason = String(reason);
      wsBridge("close", args);
      scheduleWsPoll();
    }
  }
  // the interface object: a function, so that its length is WebIDL's (1) and a call without `new`
  // throws Gecko's TypeError; its prototype is the class's
  const WebSocket = function WebSocket(url) {
    if (!new.target) throw new TypeError("WebSocket constructor: 'new' is required");
    return Reflect.construct(WebSocketImpl, arguments, new.target);
  };
  Object.setPrototypeOf(WebSocket, EventTarget);
  Object.defineProperty(WebSocket, "prototype", { value: WebSocketImpl.prototype, writable: false });
  define(WebSocketImpl.prototype, "constructor", WebSocket);
  global.__mrt.defineEventHandlers(WebSocket.prototype, ["onopen", "onerror", "onclose", "onmessage"]);
  for (const [k, v] of Object.entries({ CONNECTING, OPEN, CLOSING, CLOSED })) {
    Object.defineProperty(WebSocket, k, { value: v, enumerable: true });
    Object.defineProperty(WebSocket.prototype, k, { value: v, enumerable: true });
  }
  for (const name of ["url", "readyState", "bufferedAmount", "extensions", "protocol", "binaryType", "send", "close"]) {
    Object.defineProperty(WebSocket.prototype, name, Object.assign(Object.getOwnPropertyDescriptor(WebSocket.prototype, name), { enumerable: true }));
  }
  Object.defineProperty(WebSocket.prototype, Symbol.toStringTag, { value: "WebSocket", configurable: true });
  define(global, "WebSocket", WebSocket);

  // ---- storage ----

  // Storage (dom/webidl/Storage.webidl). localStorage is the origin's and lives in Firefox's profile:
  // forwarded to the bridge tab's. sessionStorage is the tab's, page state: SessionStorage translated
  // in the glue (__mrtSessionStorage, by origin; SPEC decision 14). Each is a legacy platform object
  // with a named getter, setter and deleter (getItem, setItem, removeItem), made with a Proxy as the
  // WebIDL bindings' DOMProxyHandler behaves: a named property is visible unless the object or its
  // prototype chain has a property of that name (Storage is not [LegacyOverrideBuiltIns]); assigning
  // or defining any string property sets an item.
  const storageBackend = new WeakMap();
  const quotaExceeded = () => new DOMException("The quota has been exceeded.", "QuotaExceededError");
  // the document's URI when the window first used localStorage (the storage events' url)
  let storageURL = null;
  const ls = (call, args) => {
    if (storageURL === null && !isWorker) storageURL = global.document.URL;
    return bridge("storage", { origin: global.origin, area: "localStorage", call, args });
  };
  // The storage event (Storage::NotifyChange, StorageNotifierService::Broadcast, nsGlobalWindowInner::
  // ObserveStorageNotification): a change to localStorage reaches every other window of the runtime
  // with the same origin and partition, each as a task, and not the window that made it. Its url is
  // the changing window's document URI when it first used localStorage (LSObject::mDocumentURI). The
  // old value is asked for first only when there is such a window (Gecko's snapshot has it at hand).
  // Windows of other runtimes (another tab of the site) get none.
  const storageWindows = share("storageWindows", () => []);
  if (!isWorker) storageWindows.push(global);
  const storageReceivers = () => {
    for (let i = storageWindows.length - 1; i >= 0; i--) if (__mrtWindows(5, storageWindows[i])) storageWindows.splice(i, 1);
    return storageWindows.filter(w => {
      if (w === global) return false;
      // (a window of another origin may refuse to be looked at)
      try { return w.origin === global.origin && w.__mrt.foreignAncestor() === foreignAncestor; } catch (e) { return false; }
    });
  };
  const notifyStorage = (receivers, key, oldValue, newValue) => {
    for (const w of receivers) {
      w.__mrt.queueTask(() => {
        // (the runtime's events have no isTrusted, GAPS B72; this one is the browser's)
        const e = new w.StorageEvent("storage", { key, oldValue, newValue, url: storageURL, storageArea: w.localStorage });
        e.isTrusted = true;
        w.dispatchEvent(e);
      });
    }
  };
  const lsChange = (change, key, newValue) => {
    const receivers = storageReceivers();
    if (!receivers.length) return change();
    // clear() notifies when there was an item (LSSnapshot::Clear), the others when the value changed
    const before = key === null ? ls("length", []) : ls("getItem", [key]);
    change();
    if (key === null ? before > 0 : before !== newValue) notifyStorage(receivers, key, key === null ? null : before, newValue);
  };
  const backends = {
    localStorage: {
      length: () => ls("length", []),
      key: i => ls("key", [i]),
      getItem: k => ls("getItem", [k]),
      setItem: (k, v) => lsChange(() => { if (!ls("setItem", [k, v])) throw quotaExceeded(); }, k, v),
      removeItem: k => lsChange(() => { ls("removeItem", [k]); }, k, null),
      clear: () => lsChange(() => { ls("clear", []); }, null, null),
      names: () => ls("names", []),
    },
    sessionStorage: {
      length: () => __mrtSessionStorage(pageURL.origin, "length"),
      key: i => __mrtSessionStorage(pageURL.origin, "key", i),
      getItem: k => __mrtSessionStorage(pageURL.origin, "getItem", k),
      setItem: (k, v) => { if (!__mrtSessionStorage(pageURL.origin, "setItem", k, v)) throw quotaExceeded(); },
      removeItem: k => { __mrtSessionStorage(pageURL.origin, "removeItem", k); },
      clear: () => { __mrtSessionStorage(pageURL.origin, "clear"); },
      names: () => __mrtSessionStorage(pageURL.origin, "names"),
    },
  };
  const backendOf = (obj, member) => {
    const b = storageBackend.get(obj);
    if (!b) throw new TypeError("'" + member + "' called on an object that does not implement interface Storage.");
    return b;
  };
  class Storage {
    constructor() { throw new TypeError("Illegal constructor."); }
    get length() { return backendOf(this, "length").length(); }
    key(index) { return backendOf(this, "key").key(index >>> 0); }
    getItem(key) { return backendOf(this, "getItem").getItem(String(key)); }
    setItem(key, value) { backendOf(this, "setItem").setItem(String(key), String(value)); }
    removeItem(key) { backendOf(this, "removeItem").removeItem(String(key)); }
    clear() { backendOf(this, "clear").clear(); }
  }
  for (const m of ["length", "key", "getItem", "setItem", "removeItem", "clear"]) {
    Object.defineProperty(Storage.prototype, m, { enumerable: true });
  }
  Object.defineProperty(Storage.prototype, Symbol.toStringTag, { value: "Storage", configurable: true });
  const makeStorage = area => {
    const target = Object.create(Storage.prototype);
    const b = backends[area];
    // the named property visibility algorithm: a supported name, not an own property of the
    // target, not on the prototype chain
    const visible = (t, k) => typeof k === "string" && !Object.prototype.hasOwnProperty.call(t, k) && !(k in Storage.prototype) && b.getItem(k) !== null;
    const proxy = new Proxy(target, {
      get(t, k, receiver) { return visible(t, k) ? b.getItem(k) : Reflect.get(t, k, receiver); },
      set(t, k, v, receiver) {
        if (typeof k === "string" && receiver === proxy) { b.setItem(k, String(v)); return true; }
        return Reflect.set(t, k, v, receiver);
      },
      has(t, k) { return visible(t, k) || Reflect.has(t, k); },
      deleteProperty(t, k) {
        if (visible(t, k)) { b.removeItem(k); return true; }
        return Reflect.deleteProperty(t, k);
      },
      defineProperty(t, k, desc) {
        if (typeof k === "string") {
          // a named setter takes only data properties (DOMProxyHandler::defineProperty's error)
          if ("get" in desc || "set" in desc) throw new TypeError("can't define a getter/setter for element '" + JSON.stringify(k) + "' of Storage object");
          b.setItem(k, String(desc.value));
          return true;
        }
        return Reflect.defineProperty(t, k, desc);
      },
      getOwnPropertyDescriptor(t, k) {
        if (visible(t, k)) return { value: b.getItem(k), writable: true, enumerable: true, configurable: true };
        return Reflect.getOwnPropertyDescriptor(t, k);
      },
      // the supported property names, then the target's own keys
      ownKeys(t) { return [...b.names().filter(k => visible(t, k)), ...Reflect.ownKeys(t)]; },
    });
    storageBackend.set(proxy, b);
    return proxy;
  };
  define(global, "Storage", Storage);
  define(global, "localStorage", makeStorage("localStorage"));
  define(global, "sessionStorage", makeStorage("sessionStorage"));
  // a navigated document's origin gets the items its last document of that origin left in the tab's
  // sessionStorage (the driver keeps them: one runtime a document)
  if (!isWorker && !isFrameWindow && env.navigation && env.navigation.sessionStorage) {
    const { origin, items } = env.navigation.sessionStorage;
    for (const [k, v] of items) __mrtSessionStorage(origin, "setItem", k, v);
  }

  // ---- script loading ----

  // A module's source for the C++ layer's module loader: data: URLs in the prelude; the others
  // fetched by the bridge tab's page as Firefox fetches a module script (CORS mode, credentials
  // same-origin; the content type must be the module type's). {url, text} or null.
  const fetchDataModule = global.__mrt.fetchModule;
  global.__mrt.fetchModule = (url, moduleType) => {
    if (url.startsWith("data:")) return fetchDataModule(url, moduleType);
    if (isBlobURL(url)) {
      const entry = __mrtBlobURL(2, url, global.origin, blobModuleParsedLive(url), clockNow());
      if (!entry || !global.__mrt.mimeTypeMatches(entry[1], moduleType)) return null;
      return { url, text: new TextDecoder().decode(entry[0]) };
    }
    let r;
    try {
      r = bridge("fetch", { url, method: "GET", headers: [["accept", moduleType === "json" ? "application/json,*/*;q=0.5" : "*/*"]],
        body: null, credentials: "same-origin" });
    } catch (e) { console.error("module fetch:", url, e); return null; }
    if (!(r.status >= 200 && r.status < 300) || r.body == null) return null;
    const type = (r.headers.find(([k]) => k.toLowerCase() === "content-type") || [, ""])[1];
    if (!global.__mrt.mimeTypeMatches(type, moduleType)) return null;
    return { url: r.url || url, text: r.body };
  };
  global.__mrt.baseURL = () => pageURL.href;
  // whether a blob: module's URL was live when parsed: a module worker's own script's, from its
  // constructor; an import's is parsed as it is fetched
  let blobModuleParsedLive = () => false;

  const scripts = { inline: 0, external: 0, dynamic: 0, failed: 0, module: 0, module_failed: 0, module_fetch_failed: 0,
    nomodule: 0, not_js: 0, fetch_failed: 0 };
  const ran = share("ran", () => new WeakSet());
  const CLASSIC = new Set(["", "text/javascript", "application/javascript", "text/ecmascript", "application/x-javascript", "module"]);
  // A script element's source fetched ahead of running it (an inserted script: SPEC decision 20)
  const prefetched = new WeakMap();
  function fetchScriptSource(el) {
    const src = el.getAttribute("src");
    if (src === null) return null;
    const name = resolve(src, pageURL.href);
    let r = null;
    // with the element's referrerpolicy (ScriptLoader: an invalid one is the document's)
    const referrerPolicy = el.getAttribute("referrerpolicy");
    try { r = bridge("fetch", { url: name, method: "GET", headers: [], body: null, script: true, dest: "script", referrerPolicy: referrerPolicy || "" }); }
    catch (e) { console.error("script fetch:", name, e); }
    const f = { name, r };
    prefetched.set(el, f);
    return f;
  }
  function runScriptElement(el, dynamic) {
    if (ran.has(el)) return;
    ran.add(el);
    const type = (el.getAttribute("type") || "").trim().toLowerCase();
    if (!CLASSIC.has(type)) { scripts.not_js++; return; }
    if (type === "module") {
      const r = global.__mrt.runModuleScript(el);
      if (r === 1) scripts.module_fetch_failed++;
      else if (r === 2) scripts.module_failed++;
      else scripts.module++;
      return;
    }
    // Firefox runs module scripts, so it skips the fallbacks marked nomodule
    if (el.hasAttribute("nomodule")) { scripts.nomodule++; return; }
    const src = el.getAttribute("src");
    let text = null, name = null;
    if (src !== null) {
      const f = prefetched.get(el) || fetchScriptSource(el);
      prefetched.delete(el);
      name = f.name;
      const r = f.r;
      if (r && r.status >= 200 && r.status < 300 && r.body != null) text = r.body;
      if (text === null) {
        scripts.fetch_failed++;
        el.dispatchEvent(new Event("error"));
        return;
      }
      if (dynamic) scripts.dynamic++;
      else scripts.external++;
    } else {
      text = el.textContent;
      scripts.inline++;
    }
    // its place for its errors and stacks (ScriptLoader::FillCompileOptionsForRequest): an external
    // script's URL from line 1; an inline one's document from where the parser found it, or from line
    // 1 for one a script made, with that script's place when it is on the stack (an inline script
    // inserted runs at once)
    const position = __mrtScriptPosition(el);
    const prev = currentScript;
    currentScript = el;
    // a callback of the page's: its microtasks run before its load event when nothing else of the
    // page's is on the stack (ScriptLoader::EvaluateScript's nsAutoMicroTask, after the exception
    // is reported)
    global.__mrt.enterCallback();
    try {
      if (name) __mrtClassicScript(text, name, 1, 0);
      else if (position) __mrtClassicScript(text, pageURL.href, position[0], position[1]);
      else {
        const caller = String(new Error().stack).split("\n").map(f => /@(.*):(\d+):\d+$/.exec(f))
          .find(at => at && at[1] !== "bridge.js" && at[1] !== "prelude.js");
        if (caller) __mrtClassicScript(text, pageURL.href, 1, 0, caller[1], +caller[2]);
        else __mrtClassicScript(text, pageURL.href, 1, 0);
      }
    } catch (e) {
      scripts.failed++;
      console.error("uncaught exception in a script:", name || "inline", e, e && e.stack);
    }
    global.__mrt.leaveCallback();
    currentScript = prev;
    if (src !== null) el.dispatchEvent(new Event("load"));
  }
  // A script element inserted by script (the prelude's appendChild and insertBefore call this once it
  // is in the document): with a src it loads and runs on a 0 ms timer, as an async script does; an
  // inline classic one runs at once, as in Firefox, an inline module one in a task of its own
  // (ModuleLoader::AsyncExecuteInlineModule).
  const isModule = global.__mrt.isModuleScript;
  // script elements whose async was set by script: no longer force-async (HTMLScriptElement::SetAsync)
  const nonAsync = share("nonAsyncScripts", () => new WeakSet());
  global.__mrt.scriptAsyncSet = el => nonAsync.add(el);
  // the ordered inserted scripts in insertion order: each runs once it has arrived and those before
  // it have run
  const orderedScripts = [];
  const runOrderedScripts = () => {
    while (orderedScripts.length && orderedScripts[0].ready <= clockNow()) orderedScripts.shift().run();
    if (orderedScripts.length) global.__mrt.queueAt(runOrderedScripts, orderedScripts[0].ready);
  };
  global.__mrt.insertedScript = node => {
    if (ran.has(node)) return;
    if (isModule(node)) {
      const unblock = blockOnload("script module");
      global.__mrt.queueTask(() => { try { runScriptElement(node, true); } finally { unblock(); } });
    } else if (node.hasAttribute("src")) {
      // fetched now, run when its answer arrives (SPEC decision 20); a script whose async was set to
      // false by script keeps its place among the others like it (Gecko's ordered async list:
      // ScriptLoader::mNonAsyncExternalScriptInsertedRequests)
      const unblock = blockOnload("script " + node.getAttribute("src"));
      const f = fetchScriptSource(node);
      const run = () => { try { runScriptElement(node, true); } finally { unblock(); } };
      if (!node.hasAttribute("async") && nonAsync.has(node)) {
        const ready = clockNow() + ((f.r && f.r.ms) || 0);
        orderedScripts.push({ node, ready, run });
        if (orderedScripts.length === 1) global.__mrt.queueAt(runOrderedScripts, ready);
      } else afterNetwork(f.r, run);
    }
    else if (node.textContent) runScriptElement(node, true);
  };

  // ---- reflected attributes (Firefox's bindings reflect these; the mini ones only id and class) ----

  // Each on the prototypes of the interfaces that declare it in Gecko's WebIDL, as in Firefox
  // (html_interfaces.js, generated); Element's own (slot, role) on Element.prototype.
  // @HTML_INTERFACES@
  const ownersOf = name => {
    const owners = HTML_ATTRIBUTE_OWNERS[name];
    if (!owners) return [Element.prototype];
    return owners.map(n => (n === "HTMLElement" ? HTMLElement : global[n]).prototype);
  };
  const reflect = (name, get, set) => {
    for (const proto of ownersOf(name)) {
      if (Object.prototype.hasOwnProperty.call(proto, name)) continue;
      getter(proto, name, get, set);
    }
  };

  // Event handler IDL attributes (GlobalEventHandlers and the others): HTMLElement's on its
  // prototype, from Gecko's WebIDL (html_interfaces.js); the window's on the window itself (it is
  // the [Global]), in the order release Firefox 157 lists them. The handler is called with the event,
  // also window.onerror (Gecko's OnErrorEventHandler passes the error's parts; no error event reaches
  // the window yet, B31). A page that set one as an own property before is unchanged: dispatch reads
  // either (prelude.js).
  if (!isWorker) {
    const handlers = Object.keys(HTML_ATTRIBUTE_OWNERS).filter(n => n.startsWith("on") && HTML_ATTRIBUTE_OWNERS[n].includes("HTMLElement") && (touchEnabled || !n.startsWith("ontouch")));
    // Document includes GlobalEventHandlers too (Document.webidl). React tests
    // "oninput" in document before choosing its modern input-event path.
    for (const proto of [HTMLElement.prototype, Document.prototype]) global.__mrt.defineEventHandlers(proto, handlers);
  }
  const WINDOW_EVENT_HANDLERS = ["ondevicemotion", "ondeviceorientation", "ondeviceorientationabsolute", "onabort", "onblur", "onfocus", "oncancel", "onauxclick", "onbeforeinput", "onbeforematch", "onbeforetoggle", "oncanplay", "oncanplaythrough", "onchange", "onclick", "onclose", "oncontentvisibilityautostatechange", "oncontextlost", "oncontextmenu", "oncommand", "oncontextrestored", "oncopy", "oncuechange", "oncut", "ondblclick", "ondrag", "ondragend", "ondragenter", "ondragexit", "ondragleave", "ondragover", "ondragstart", "ondrop", "ondurationchange", "onemptied", "onended", "onformdata", "oninput", "oninvalid", "onkeydown", "onkeypress", "onkeyup", "onload", "onloadeddata", "onloadedmetadata", "onloadstart", "onmousedown", "onmouseenter", "onmouseleave", "onmousemove", "onmouseout", "onmouseover", "onmouseup", "onwheel", "onpaste", "onpause", "onplay", "onplaying", "onprogress", "onratechange", "onreset", "onresize", "onscroll", "onscrollend", "onsecuritypolicyviolation", "onseeked", "onseeking", "onselect", "onslotchange", "onstalled", "onsubmit", "onsuspend", "ontimeupdate", "onvolumechange", "onwaiting", "onselectstart", "onselectionchange", "ontoggle", "onpointercancel", "onpointerdown", "onpointerup", "onpointermove", "onpointerout", "onpointerover", "onpointerenter", "onpointerleave", "ongotpointercapture", "onlostpointercapture", "onmozfullscreenchange", "onmozfullscreenerror", "onanimationcancel", "onanimationend", "onanimationiteration", "onanimationstart", "ontransitioncancel", "ontransitionend", "ontransitionrun", "ontransitionstart", "onwebkitanimationend", "onwebkitanimationiteration", "onwebkitanimationstart", "onwebkittransitionend", "onerror", "onafterprint", "onbeforeprint", "onbeforeunload", "onhashchange", "onlanguagechange", "onmessage", "onmessageerror", "onoffline", "ononline", "onpagehide", "onpageshow", "onpopstate", "onrejectionhandled", "onstorage", "onunhandledrejection", "onunload", "ongamepadconnected", "ongamepaddisconnected"];
  global.__mrt.defineEventHandlers(global, WINDOW_EVENT_HANDLERS.filter(n => !Object.prototype.hasOwnProperty.call(global, n)));

  const URL_ATTRS = ["src", "href", "action", "poster", "data"];
  if (!isWorker) for (const name of URL_ATTRS) {
    reflect(name, function () {
      const v = this.getAttribute(name);
      if (v === null) return "";
      try { return resolve(v, pageURL.href); } catch (e) { return v; }
    }, function (v) { this.setAttribute(name, String(v)); });
  }
  const STRING_ATTRS = { type: "type", rel: "rel", charset: "charset", crossOrigin: "crossorigin", integrity: "integrity",
    nonce: "nonce", name: "name", content: "content", media: "media", alt: "alt", title: "title", lang: "lang",
    target: "target", value: "value", placeholder: "placeholder", width: "width", height: "height", as: "as", referrerPolicy: "referrerpolicy",
    loading: "loading", role: "role", dir: "dir", slot: "slot", srcdoc: "srcdoc" };
  if (!isWorker) for (const [prop, attr] of Object.entries(STRING_ATTRS)) {
    reflect(prop, function () { const v = this.getAttribute(attr); return v === null ? (prop === "crossOrigin" ? null : "") : v; },
      function (v) { if (v === null && prop === "crossOrigin") this.removeAttribute(attr); else this.setAttribute(attr, String(v)); });
  }
  // The image's legacy and newer attributes (HTMLImageElement.h): plain strings (GetHTMLAttr), URLs
  // (GetURIAttr: resolved, "" when absent), the enumerated ones (GetEnumAttr: the keyword in its
  // table's case, else the default) and hspace and vspace (GetDimensionAttrAsUnsignedInt: the
  // leading digits of "10px", "50%", "+100"; 0 when there are none). Each on the interfaces that
  // declare it, except sizes, which is a DOMTokenList on <link>. x and y need the layout (GAPS B139).
  if (!isWorker) {
    for (const [prop, attr] of Object.entries({ align: "align", border: "border", useMap: "usemap", srcset: "srcset" })) {
      reflect(prop, function () { return this.getAttribute(attr) ?? ""; }, function (v) { this.setAttribute(attr, String(v)); });
    }
    for (const C of [HTMLImageElement, HTMLSourceElement]) {
      if (!Object.prototype.hasOwnProperty.call(C.prototype, "sizes")) {
        getter(C.prototype, "sizes", function () { return this.getAttribute("sizes") ?? ""; }, function (v) { this.setAttribute("sizes", String(v)); });
      }
    }
    for (const [prop, attr] of Object.entries({ longDesc: "longdesc", lowsrc: "lowsrc" })) {
      reflect(prop, function () {
        const v = this.getAttribute(attr);
        if (v === null) return "";
        try { return resolve(v, pageURL.href); } catch (e) { return v; }
      }, function (v) { this.setAttribute(attr, String(v)); });
    }
    const enumerated = (prop, attr, keywords, missing) => reflect(prop, function () {
      const v = this.getAttribute(attr);
      const k = v === null ? null : keywords.find(k => k === v.toLowerCase());
      return k ?? missing;
    }, function (v) { this.setAttribute(attr, String(v)); });
    enumerated("decoding", "decoding", ["sync", "async", "auto"], "auto");
    enumerated("fetchPriority", "fetchpriority", ["high", "low", "auto"], "auto");
    for (const name of ["hspace", "vspace"]) {
      getter(HTMLImageElement.prototype, name, function () {
        const m = /^[\t\n\f\r ]*\+?([0-9]+)/.exec(this.getAttribute(name) ?? "");
        const n = m ? Number(m[1]) : 0;
        return n <= 2147483647 ? n : 0;
      }, function (v) {
        const n = Number(v) >>> 0;
        this.setAttribute(name, String(n > 2147483647 ? 0 : n));
      });
    }
  }
  if (!isWorker) for (const [prop, attr] of Object.entries({ async: "async", defer: "defer", noModule: "nomodule", hidden: "hidden", disabled: "disabled",
    checked: "checked", selected: "selected", required: "required", readOnly: "readonly", multiple: "multiple", autofocus: "autofocus",
    open: "open", inert: "inert", reversed: "reversed", noValidate: "novalidate", formNoValidate: "formnovalidate",
    allowFullscreen: "allowfullscreen", isMap: "ismap", default: "default",
    loop: "loop", autoplay: "autoplay", controls: "controls", defaultMuted: "muted" })) {
    reflect(prop, function () { return this.hasAttribute(attr); },
      function (v) { if (v) this.setAttribute(attr, ""); else this.removeAttribute(attr); });
  }
  // Setting a script's async clears its force-async (HTMLScriptElement::SetAsync): inserted without
  // the attribute, it then runs in insertion order with the others like it (insertedScript). The
  // getter still reads the attribute only, not force-async.
  if (!isWorker) {
    const d = Object.getOwnPropertyDescriptor(HTMLScriptElement.prototype, "async");
    Object.defineProperty(HTMLScriptElement.prototype, "async", Object.assign({}, d, {
      set(v) { global.__mrt.scriptAsyncSet(this); d.set.call(this, v); },
    }));
  }
  // dataset: Gecko's nsDOMStringMap (dom/html/nsDOMStringMap.cpp), an interface with a named getter,
  // setter and deleter over the element's data-* attributes, [LegacyOverrideBuiltIns], one object
  // per element ([SameObject]). A Proxy whose target has DOMStringMap.prototype as its prototype.
  if (!isWorker) if (!("dataset" in HTMLElement.prototype)) {
    const DOMStringMap = { DOMStringMap: function () { throw new TypeError("Illegal constructor."); } }.DOMStringMap;
    Object.defineProperty(DOMStringMap.prototype, Symbol.toStringTag, { value: "DOMStringMap", configurable: true });
    Object.defineProperty(global, "DOMStringMap", { value: DOMStringMap, writable: true, configurable: true, enumerable: false });
    // DataPropToAttr: aBigFish to data-a-big-fish; null for a "-" followed by a to z
    const dataPropToAttr = prop => /-[a-z]/.test(prop) ? null : "data-" + prop.replace(/[A-Z]/g, c => "-" + c.toLowerCase());
    // AttrToDataProp: data-a-big-fish to aBigFish; null without the data- prefix
    const attrToDataProp = attr => attr.startsWith("data-") ? attr.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase()) : null;
    // NamedGetter: the attribute's value, or undefined when the name is not supported
    const named = (el, prop) => {
      if (typeof prop !== "string") return undefined;
      const attr = dataPropToAttr(prop);
      return attr === null ? undefined : el.getAttribute(attr) ?? undefined;
    };
    const elements = new WeakMap();  // target -> element
    const handler = {
      get(target, prop, receiver) {
        const value = named(elements.get(target), prop);
        return value !== undefined ? value : Reflect.get(target, prop, receiver);
      },
      has(target, prop) { return named(elements.get(target), prop) !== undefined || Reflect.has(target, prop); },
      // NamedSetter
      set(target, prop, value) {
        if (typeof prop !== "string") return Reflect.set(target, prop, value);
        const attr = dataPropToAttr(prop);
        if (attr === null) throw new DOMException(`DOMStringMap: '${prop}' is not a valid property name`, "SyntaxError");
        elements.get(target).setAttribute(attr, String(value));
        return true;
      },
      defineProperty(target, prop, desc) {
        if (typeof prop !== "string") return Reflect.defineProperty(target, prop, desc);
        if (!("value" in desc)) return false;
        return handler.set(target, prop, desc.value);
      },
      // NamedDeleter
      deleteProperty(target, prop) {
        if (typeof prop !== "string") return Reflect.deleteProperty(target, prop);
        const attr = dataPropToAttr(prop), el = elements.get(target);
        if (attr !== null && el.hasAttribute(attr)) el.removeAttribute(attr);
        return true;
      },
      // GetSupportedNames: in attribute order
      ownKeys(target) {
        const names = elements.get(target).getAttributeNames().map(attrToDataProp).filter(p => p !== null);
        return names.concat(Reflect.ownKeys(target).filter(k => !names.includes(k)));
      },
      getOwnPropertyDescriptor(target, prop) {
        const value = named(elements.get(target), prop);
        return value !== undefined ? { value, writable: true, enumerable: true, configurable: true } : Reflect.getOwnPropertyDescriptor(target, prop);
      },
    };
    const datasets = share("datasets", () => new WeakMap());
    getter(HTMLElement.prototype, "dataset", function () {
      let map = datasets.get(this);
      if (!map) {
        const target = Object.create(DOMStringMap.prototype);
        elements.set(target, this);
        datasets.set(this, (map = new Proxy(target, handler)));
      }
      return map;
    });
  }
  // `text` is the content of a, option, script and title (on body it is a legacy color attribute)
  if (!isWorker) for (const name of ["HTMLAnchorElement", "HTMLOptionElement", "HTMLScriptElement", "HTMLTitleElement"]) {
    getter(global[name].prototype, "text", function () { return this.textContent; }, function (v) { this.textContent = String(v); });
  }

  // ---- DOM APIs written in JS over the native ones (DESIGN "The DOM APIs"; phase 2 moves the
  // ones that hold memory to C++) ----

  // APIs answered by an approximation; counted, so the report shows where the page saw less than
  // Firefox gives it
  const stubbed = share("stubbed", () => new Map());
  const stub = name => stubbed.set(name, (stubbed.get(name) || 0) + 1);
  global.__mrt.stub = stub;

  // A method written here that makes several DOM changes runs as one [CEReactions] method: the
  // custom element reactions of all its changes run once, at its end (glue/mrt_js.cpp)
  const ceReactions = fn => function (...a) { return __mrtCEReactions(fn, this, ...a); };

  const VOID = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"]);
  const RAW = new Set(["script", "style", "xmp", "iframe", "noembed", "noframes", "plaintext"]);
  // nsContentUtils' nonEscapingElements: a <noscript>'s text is written raw when its document runs
  // scripts (the page), escaped otherwise (a DOMParser or createHTMLDocument document, B23)
  const isRaw = el => RAW.has(el.localName) || (el.localName === "noscript" && el.ownerDocument === document);
  const escText = t => t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/ /g, "&nbsp;");
  const escAttr = t => t.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/ /g, "&nbsp;");
  let skipNoscript = false;
  // An element's tag is its local name, case kept (an SVG clipPath; the arena keeps no prefix); the
  // void, raw text and template rules are for HTML elements only (the HTML fragment serialization)
  const isHTML = node => node.namespaceURI === "http://www.w3.org/1999/xhtml";
  function serialize(node, out) {
    if (node.nodeType === 1) {
      const tag = node.localName, html = isHTML(node);
      if (skipNoscript && html && tag === "noscript") return;
      out.push("<" + tag);
      for (const name of node.getAttributeNames()) out.push(" " + name + '="' + escAttr(node.getAttribute(name) ?? "") + '"');
      out.push(">");
      if (html && VOID.has(tag)) return;
      if (html && isRaw(node)) out.push(node.textContent);
      else for (const c of (html && tag === "template" ? node.content : node).childNodes) serialize(c, out);
      out.push("</" + tag + ">");
    } else if (node.nodeType === 3) {
      const p = node.parentNode;
      out.push(p && p.nodeType === 1 && isHTML(p) && isRaw(p) ? node.data : escText(node.data));
    } else if (node.nodeType === 8) {
      out.push("<!--" + node.data + "-->");
    } else {
      for (const c of node.childNodes) serialize(c, out);
    }
  }
  // a <template>'s markup is its contents' (HTMLTemplateElement in Gecko)
  const contentOf = el => (el.nodeType === 1 && el.tagName === "TEMPLATE" ? el.content : el);
  // XMLSerializer (nsDOMSerializer with nsXMLContentSerializer), for nodes of an HTML document: an
  // element gets an xmlns attribute where its namespace differs from its parent's (the XHTML one at
  // the top); an HTML void element closes as "<br />", any other childless element as
  // "<div></div>" in HTML and "<path/>" elsewhere; a template's contents stand for its children;
  // text escapes & < >, attribute values also " and tab, newline and carriage return.
  const XHTML_NS = "http://www.w3.org/1999/xhtml";
  const xmlText = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const xmlAttr = s => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
    .replace(/\t/g, "&#9;").replace(/\n/g, "&#10;").replace(/\r/g, "&#13;");
  function serializeXML(node, out, parentNS) {
    if (node.nodeType === 1) {
      const ns = node.namespaceURI;
      const name = ns === XHTML_NS ? node.localName : node.tagName;
      out.push("<" + name);
      const attrs = node.getAttributeNames();
      if (ns !== parentNS && !attrs.includes("xmlns")) out.push(' xmlns="' + xmlAttr(ns || "") + '"');
      for (const a of attrs) out.push(" " + a + '="' + xmlAttr(node.getAttribute(a) ?? "") + '"');
      const children = ns === XHTML_NS && node.localName === "template" ? node.content.childNodes : node.childNodes;
      if (children.length === 0) {
        out.push(ns === XHTML_NS ? (VOID.has(node.localName) ? " />" : "></" + name + ">") : "/>");
        return;
      }
      out.push(">");
      for (const c of children) serializeXML(c, out, ns);
      out.push("</" + name + ">");
    } else if (node.nodeType === 3) {
      out.push(xmlText(node.data));
    } else if (node.nodeType === 8) {
      out.push("<!--" + node.data + "-->");
    } else if (node.nodeType === 10) {
      out.push("<!DOCTYPE " + node.name + ">");
    } else {
      for (const c of node.childNodes) serializeXML(c, out, parentNS);
    }
  }
  class XMLSerializer {
    serializeToString(node) {
      if (!(node instanceof Node)) throw new TypeError("XMLSerializer.serializeToString: Argument 1 does not implement interface Node.");
      const out = [];
      serializeXML(node, out, node.nodeType === 9 ? null : null);
      return out.join("");
    }
  }
  define(global, "XMLSerializer", XMLSerializer);
  // markup parsed for innerHTML, outerHTML and insertAdjacentHTML in the context of an element
  // (nsContentUtils::ParseFragmentHTML; a <body> when there is none): its scripts never run
  const HTML_NS = "http://www.w3.org/1999/xhtml";
  const parsedFragment = (html, context) => { const f = __mrtParseFragment(html, context || undefined); global.__mrt.preventExecution(f); return f; };
  // HTMLStyleElement and HTMLScriptElement override innerHTML: their text as it is, never parsed
  // (GetInnerHTML is the text of their Text children; SetInnerHTMLTrusted sets the text content)
  const isRawTextHTML = el => el.nodeType === 1 && el.namespaceURI === HTML_NS && (el.localName === "style" || el.localName === "script");
  const innerOf = el => {
    if (isRawTextHTML(el)) return [...el.childNodes].filter(c => c.nodeType === 3).map(c => c.data).join("");
    const out = [];
    for (const c of contentOf(el).childNodes) serialize(c, out);
    return out.join("");
  };
  if (!isWorker) for (const proto of [Element.prototype, DocumentFragment.prototype]) {
    // the children replaced in one mutation batch (glue/mrt_js.cpp, FragmentOrElement::SetInnerHTMLInternal);
    // a shadow root's markup is parsed in its host's context, a <template>'s in its own
    getter(proto, "innerHTML", function () { return innerOf(this); }, ceReactions(function (html) {
      html = html === null ? "" : String(html);
      if (isRawTextHTML(this)) { this.textContent = html; return; }
      const context = this.nodeType === 11 ? __mrtLink(this, 5) : this;
      __mrtSetInnerHTML(contentOf(this), html !== "" ? parsedFragment(html, context) : null);
    }));
  }
  // Element::SetOuterHTML: parsed in its parent's context (a <body> for a fragment's child); a child of
  // the document cannot be replaced
  if (!isWorker) getter(Element.prototype, "outerHTML", function () { const out = []; serialize(this, out); return out.join(""); }, ceReactions(function (html) {
    const parent = this.parentNode;
    if (!parent) return;
    if (parent.nodeType === 9) throw new DOMException("Modifications are not allowed for this document", "NoModificationAllowedError");
    parent.insertBefore(parsedFragment(String(html), parent.nodeType === 1 ? parent : null), this);
    this.remove();
  }));
  function insertAdjacent(el, where, node) {
    switch (String(where).toLowerCase()) {
      case "beforebegin": if (el.parentNode) el.parentNode.insertBefore(node, el); break;
      case "afterbegin": el.insertBefore(node, el.firstChild); break;
      case "beforeend": el.appendChild(node); break;
      case "afterend": if (el.parentNode) el.parentNode.insertBefore(node, el.nextSibling); break;
    }
    return node;
  }
  // Range: createContextualFragment parses as innerHTML does (the context element is not used), but
  // its scripts are not marked: they run once inserted, as in Gecko; the boundary methods keep their
  // arguments and do nothing else
  class Range {
    constructor() { this.startContainer = this.endContainer = this.commonAncestorContainer = document; this.startOffset = this.endOffset = 0; this.collapsed = true; }
    createContextualFragment(html) { return __mrtCEReactions(__mrtParseFragment, null, String(html)); }
    setStart(n, o) { stub("Range.boundaries"); this.startContainer = n; this.startOffset = o; }
    setEnd(n, o) { stub("Range.boundaries"); this.endContainer = n; this.endOffset = o; }
    setStartBefore(n) { this.setStart(n.parentNode, 0); }
    setStartAfter(n) { this.setStart(n.parentNode, 0); }
    setEndBefore(n) { this.setEnd(n.parentNode, 0); }
    setEndAfter(n) { this.setEnd(n.parentNode, 0); }
    selectNode(n) { this.setStart(n.parentNode, 0); this.setEnd(n.parentNode, 0); }
    selectNodeContents(n) { this.setStart(n, 0); this.setEnd(n, 0); }
    collapse() {}
    detach() {}
    cloneRange() { return Object.assign(new Range(), this); }
    toString() { return ""; }
    getClientRects() { stub("Range.getClientRects"); return []; }
    getBoundingClientRect() { stub("Range.getClientRects"); return { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0 }; }
  }
  define(global, "Range", Range);
  if (!isWorker) define(Document.prototype, "createRange", () => new Range());
  // Element::InsertAdjacentHTML: the context is the parent for beforebegin and afterend, else the
  // element; an <html> element of an HTML document parses as a <body>
  if (!isWorker) define(Element.prototype, "insertAdjacentHTML", ceReactions(function (where, html) {
    const position = String(where).toLowerCase();
    let context = position === "beforebegin" || position === "afterend" ? this.parentNode : this;
    if (context && context.nodeType === 9) throw new DOMException("Modifications are not allowed for this document", "NoModificationAllowedError");
    if (!context || context.nodeType !== 1 || (context.localName === "html" && context.namespaceURI === HTML_NS)) context = null;
    insertAdjacent(this, where, parsedFragment(String(html), context));
  }));
  if (!isWorker) define(Element.prototype, "insertAdjacentElement", ceReactions(function (where, el) { return insertAdjacent(this, where, el); }));
  if (!isWorker) define(Element.prototype, "insertAdjacentText", ceReactions(function (where, t) { insertAdjacent(this, where, document.createTextNode(String(t))); }));
  // Attr (Attr.webidl): the node an element's nsDOMAttributeMap keeps for each attribute name, the
  // same object every time it is asked for (`attributes[i]`, getAttributeNode). Its prototype is
  // Node's, but it is not a node of the arena: its state is here, and it answers the Node members an
  // attribute answers (nodeType, nodeName, nodeValue, textContent) on Attr.prototype (GAPS B65).
  // While attached, its value reads and writes the element's attribute (Attr::GetValue,
  // Attr::SetValue: YouTube renames its icons' ids with attr.value += "_yt1"); detached
  // (removeAttributeNode, or replaced by setAttributeNode), it keeps its last value.
  const Attr = { Attr: function () { throw new TypeError("Illegal constructor."); } }.Attr;
  if (!isWorker) Object.setPrototypeOf(Attr, Node);
  if (!isWorker) Object.setPrototypeOf(Attr.prototype, Node.prototype);
  const attrStates = new WeakMap();  // Attr -> { element (null when detached), name, value }
  const attrNodes = share("attrNodes", () => new WeakMap());  // element -> Map(name -> Attr)
  const attrState = (attr, what) => {
    const st = attrStates.get(attr);
    if (!st) throw new TypeError(`'${what}' called on an object that does not implement interface Attr.`);
    return st;
  };
  const attrValue = st => st.element ? st.element.getAttribute(st.name) ?? st.value : st.value;
  // the Attr for an attribute the element has (`name` as the element stores it)
  function attrFor(el, name) {
    let map = attrNodes.get(el);
    if (!map) attrNodes.set(el, (map = new Map()));
    let attr = map.get(name);
    if (!attr) {
      attr = Object.create(Attr.prototype);
      attrStates.set(attr, { element: el, name, value: "" });
      map.set(name, attr);
    }
    return attr;
  }
  const detachAttr = (el, attr) => {
    const st = attrStates.get(attr);
    st.value = attrValue(st);
    st.element = null;
    const map = attrNodes.get(el);
    if (map && map.get(st.name) === attr) map.delete(st.name);
  };
  for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors({
    get localName() { const n = attrState(this, "localName").name; return n.slice(n.indexOf(":") + 1); },
    get name() { return attrState(this, "name").name; },
    get value() { return attrValue(attrState(this, "value")); },
    set value(v) {
      const st = attrState(this, "value");
      if (st.element) st.element.setAttribute(st.name, String(v));
      else st.value = String(v);
    },
    // namespaced attributes are kept under their qualified name, the namespace dropped (B18)
    get namespaceURI() { attrState(this, "namespaceURI"); return null; },
    get prefix() { attrState(this, "prefix"); return null; },
    get ownerElement() { return attrState(this, "ownerElement").element; },
    get specified() { attrState(this, "specified"); return true; },
  }))) Object.defineProperty(Attr.prototype, key, { ...desc, enumerable: true });
  for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors({
    get nodeType() { attrState(this, "nodeType"); return 2; },
    get nodeName() { return attrState(this, "nodeName").name; },
    get nodeValue() { return attrValue(attrState(this, "nodeValue")); },
    set nodeValue(v) { this.value = v === null ? "" : v; },
    get textContent() { return attrValue(attrState(this, "textContent")); },
    set textContent(v) { this.value = v === null ? "" : v; },
    get parentNode() { attrState(this, "parentNode"); return null; },
    get ownerDocument() { const st = attrState(this, "ownerDocument"); return st.element ? st.element.ownerDocument : document; },
  }))) Object.defineProperty(Attr.prototype, key, { ...desc, enumerable: false });
  Object.defineProperty(Attr.prototype, Symbol.toStringTag, { value: "Attr", configurable: true });
  define(global, "Attr", Attr);
  // the stored name of an attribute: lowercased for an HTML element only, as getAttribute (an SVG
  // element's viewBox)
  const attrName = (el, name) => el.namespaceURI === "http://www.w3.org/1999/xhtml" ? String(name).toLowerCase() : String(name);
  if (!isWorker) define(Element.prototype, "getAttributeNode", function getAttributeNode(name) {
    name = attrName(this, name);
    return this.hasAttribute(name) ? attrFor(this, name) : null;
  });
  if (!isWorker) define(Element.prototype, "getAttributeNodeNS", function getAttributeNodeNS(ns, localName) {
    localName = String(localName);
    const name = this.getAttributeNames().find(n => n.slice(n.indexOf(":") + 1) === localName && (ns == null || ns === "" ? !n.includes(":") : n.includes(":")));
    return name === undefined ? null : attrFor(this, name);
  });
  // Element::SetAttributeNode: the attribute's old node, detached, is returned
  function setAttributeNode(attr) {
    const st = attrState(attr, "setAttributeNode");
    if (st.element && st.element !== this) throw new DOMException("Element.setAttributeNode: Attribute already in use", "InUseAttributeError");
    if (st.element === this) return attr;
    const old = this.hasAttribute(st.name) ? attrFor(this, st.name) : null;
    if (old) detachAttr(this, old);
    this.setAttribute(st.name, st.value);
    st.element = this;
    let map = attrNodes.get(this);
    if (!map) attrNodes.set(this, (map = new Map()));
    map.set(st.name, attr);
    return old;
  }
  if (!isWorker) define(Element.prototype, "setAttributeNode", ceReactions(setAttributeNode));
  if (!isWorker) define(Element.prototype, "setAttributeNodeNS", ceReactions(setAttributeNode));
  if (!isWorker) define(Element.prototype, "removeAttributeNode", ceReactions(function removeAttributeNode(attr) {
    const st = attrState(attr, "removeAttributeNode");
    if (st.element !== this) throw new DOMException("Element.removeAttributeNode: Attribute not found", "NotFoundError");
    detachAttr(this, attr);
    this.removeAttribute(st.name);
    return attr;
  }));
  // getElementsByTagNameNS: a filtered getElementsByTagName("*"), an array as that one is (B11)
  const byTagNameNS = function getElementsByTagNameNS(ns, localName) {
    ns = ns === null || ns === undefined ? "" : String(ns);
    localName = String(localName);
    return Array.from(this.getElementsByTagName("*")).filter(el =>
      (ns === "*" || (el.namespaceURI || "") === ns) && (localName === "*" || el.localName === localName));
  };
  if (!isWorker) for (const proto of [Element.prototype, Document.prototype])
    if (!Object.prototype.hasOwnProperty.call(proto, "getElementsByTagNameNS")) define(proto, "getElementsByTagNameNS", byTagNameNS);
  // attributes: a snapshot of the element's Attr nodes, indexed by position and by name (NamedNodeMap)
  if (!isWorker) getter(Element.prototype, "attributes", function () {
    const el = this;
    const list = this.getAttributeNames().map(name => attrFor(el, name));
    for (const a of list) if (!(a.name in list)) list[a.name] = a;
    list.getNamedItem = n => el.getAttributeNode(n);
    list.item = i => list[i] || null;
    return list;
  });
  // innerText and outerText (HTMLElement.webidl, nsGenericHTMLElement): the getters are
  // __mrtInnerText (Host::inner_text: GetInnerText's rendered text over the layout's frames, or
  // textContent when the element is not rendered); the setters make TextToNode's nodes.
  if (!isWorker) {
    const { createTextNode, createElementNS, createDocumentFragment } = Document.prototype;
    const appendChild = Node.prototype.appendChild;
    const checkThis = (self, what) => {
      if (!(self instanceof HTMLElement)) throw new TypeError(`'${what}' called on an object that does not implement interface HTMLElement.`);
    };
    // [LegacyNullToEmptyString] DOMString
    const domString = v => (v === null ? "" : `${v}`);
    // TextToNode: text runs as Text nodes, each \r, \n or \r\n a <br>; one node alone, more in a
    // fragment, null for ""
    const textToNode = (doc, value) => {
      let str = "", fragment = null;
      const add = node => {
        if (!fragment) fragment = createDocumentFragment.call(doc);
        appendChild.call(fragment, node);
      };
      for (let i = 0; ; i++) {
        // a \r\n pair should only generate one <br>, so just skip the \r
        if (i < value.length && value[i] === "\r" && i + 1 < value.length && value[i + 1] === "\n") i++;
        if (i === value.length || value[i] === "\r" || value[i] === "\n") {
          if (str) {
            const text = createTextNode.call(doc, str);
            if (!fragment && i === value.length) return text;
            add(text);
          }
          if (i === value.length) break;
          str = "";
          const br = createElementNS.call(doc, "http://www.w3.org/1999/xhtml", "br");
          if (!fragment && i + 1 === value.length) return br;
          add(br);
        } else {
          str += value[i];
        }
      }
      return fragment;
    };
    // MergeWithNextTextNode (AppendDataInternal as one replacement of the data: the runtime's
    // CharacterData has no appendData)
    const mergeWithNextTextNode = text => {
      const next = text.nextSibling;
      if (!(next instanceof Text)) return;
      text.data += next.data;
      next.remove();
    };
    getter(HTMLElement.prototype, "innerText", function () {
      checkThis(this, "get innerText");
      return __mrtInnerText(this);
    }, ceReactions(function (v) {
      checkThis(this, "set innerText");
      __mrtReplaceChildren(this, textToNode(this.ownerDocument, domString(v)));
    }));
    getter(HTMLElement.prototype, "outerText", function () {
      checkThis(this, "get outerText");
      return __mrtInnerText(this);
    }, ceReactions(function (v) {
      // SetOuterText
      checkThis(this, "set outerText");
      const value = domString(v);
      const parent = this.parentNode;
      if (!parent) throw new DOMException("HTMLElement.outerText setter: Element has no parent", "NoModificationAllowedError");
      const next = this.nextSibling, previous = this.previousSibling;
      // an empty Text node for "" (https://github.com/whatwg/html/issues/7508)
      const node = textToNode(this.ownerDocument, value) || createTextNode.call(this.ownerDocument, "");
      // (Node.prototype.replaceChild is defined further down)
      Node.prototype.replaceChild.call(parent, node, this);
      if (next && next.previousSibling instanceof Text) mergeWithNextTextNode(next.previousSibling);
      if (previous instanceof Text) mergeWithNextTextNode(previous);
    }));
  }
  if (!isWorker) define(Element.prototype, "toggleAttribute", function (name, force) {
    const has = this.hasAttribute(name);
    const want = force === undefined ? !has : !!force;
    if (want && !has) this.setAttribute(name, "");
    if (!want && has) this.removeAttribute(name);
    return want;
  });
  // the URL parts of a link (HTMLHyperlinkElementUtils), from its href
  if (!isWorker) for (const part of ["protocol", "host", "hostname", "port", "pathname", "search", "hash", "origin", "username", "password"]) {
    reflect(part, function () {
      const href = this.getAttribute("href");
      if (href === null) return "";
      try { return new URL(href, pageURL.href)[part]; } catch (e) { return ""; }
    }, function (v) {
      try { const u = new URL(this.getAttribute("href") || "", pageURL.href); u[part] = v; this.setAttribute("href", u.href); } catch (e) {}
    });
  }
  if (!isWorker) getter(Node.prototype, "nodeValue", function () { return this.nodeType === 3 || this.nodeType === 8 ? this.data : null; },
    function (v) { if (this.nodeType === 3 || this.nodeType === 8) this.data = String(v); });
  // The CSSOM of style sheets (Gecko's StyleSheet, ServoCSSRuleList and css::Rule): a sheet's rules
  // are Stylo's, parsed, changed and serialized in the runtime (__mrtCssom, mrt_style::cssom), so the
  // rules a sheet starts with are parsed into cssRules, and a rule a script inserts is parsed at the
  // call (a SyntaxError as Gecko's) and reaches the style. What a script does to the sheet of a
  // <style> or <link> is page state in the arena (SPEC decision 13): its rules once changed, and
  // `disabled`. CSSStyleRule.style edits reach the same parsed rules and arena operation log.
  // Other rule setters and CSSGroupingRule mutation remain GAPS B117.
  const CSSOM = { generation: 0, rules: 1, insert: 2, remove: 3, setDisabled: 4, disabled: 5, replace: 6, create: 7, declaration: 8 };
  // how many times script changed the rules of each <style> or <link>'s sheet, for the bridge tab's
  // copy of the document (mirrorFlush), which takes the rules again when the count moves
  const cssomEdits = new WeakMap();
  const constructedEdits = new WeakMap();
  const noteSheetEdit = (owner, sheet) => {
    if (owner) cssomEdits.set(owner, (cssomEdits.get(owner) || 0) + 1);
    if (sheet && sheet._constructed) constructedEdits.set(sheet, (constructedEdits.get(sheet) || 0) + 1);
  };
  const internal = Symbol("internal");
  const illegal = () => { throw new TypeError("Illegal constructor."); };
  // a refused insertRule or deleteRule: "!" and the DOMException's name (Gecko's messages)
  const ruleMessages = {
    SyntaxError: "An invalid or illegal string was specified",
    IndexSizeError: "Index or size is negative or greater than the allowed amount",
    HierarchyRequestError: "Node cannot be inserted at the specified point in the hierarchy",
    InvalidStateError: "An attempt was made to use an object that is not, or is no longer, usable",
  };
  // (Gecko's messages come from the nsresult, with no method name)
  const cssomCall = (op, target, text, n) => {
    const r = __mrtCssom(op, target, text, n);
    if (r[0] === "!") {
      const name = r.slice(1);
      throw new DOMException(ruleMessages[name] || name, name);
    }
    return r;
  };
  const requireArgs = (n, have, what) => {
    if (have < n) throw new TypeError(`${what}: At least ${n} argument required, but only ${have} passed`);
  };
  // WebIDL's unsigned long
  const unsignedLong = v => Number(v) >>> 0;
  // CSSRuleList: a live list over its owner's rule objects (`rules()`), indexed as Gecko's
  class CSSRuleList {
    constructor(token, rules) {
      if (token !== internal) illegal();
      this._rules = rules;
      return new Proxy(this, {
        get(t, k, r) {
          if (typeof k === "string" && /^(0|[1-9]\d*)$/.test(k)) return t._rules()[Number(k)];
          return Reflect.get(t, k, r);
        },
        has(t, k) {
          if (typeof k === "string" && /^(0|[1-9]\d*)$/.test(k)) return Number(k) < t._rules().length;
          return Reflect.has(t, k);
        },
        ownKeys(t) { return [...t._rules().keys()].map(String).concat(Reflect.ownKeys(t)); },
        getOwnPropertyDescriptor(t, k) {
          if (typeof k === "string" && /^(0|[1-9]\d*)$/.test(k)) {
            const rule = t._rules()[Number(k)];
            return rule ? { value: rule, writable: false, enumerable: true, configurable: true } : undefined;
          }
          return Reflect.getOwnPropertyDescriptor(t, k);
        },
      });
    }
    get length() { return this._rules().length; }
    item(index) { return this._rules()[unsignedLong(index)] || null; }
    [Symbol.iterator]() { return this._rules()[Symbol.iterator](); }
  }
  // a MediaList for reading: its text (Stylo's serialization, or the attribute's for an owner)
  const mediaList = text => {
    const list = text ? text.split(/\s*,\s*/) : [];
    return { mediaText: text, length: list.length, item: i => list[i] || null, toString: () => text };
  };
  // css::Rule's subclasses, made from the runtime's JSON for a rule (mrt_style::cssom's rule_json)
  const ruleClasses = {};
  const makeRule = (data, sheet, parent) => new (ruleClasses[data.i] || CSSRule)(internal, data, sheet, parent);
  class CSSRule {
    constructor(token, data, sheet, parent) {
      if (token !== internal) illegal();
      this._d = data;
      this._sheet = sheet;
      this._parent = parent || null;
    }
    get cssText() { return this._d.c; }
    // (Gecko's setter does nothing)
    set cssText(v) {}
    get type() { return this._d.t; }
    get parentStyleSheet() { return this._sheet; }
    get parentRule() { return this._parent; }
  }
  const RULE_TYPES = { STYLE_RULE: 1, CHARSET_RULE: 2, IMPORT_RULE: 3, MEDIA_RULE: 4, FONT_FACE_RULE: 5, PAGE_RULE: 6,
    NAMESPACE_RULE: 10, KEYFRAMES_RULE: 7, KEYFRAME_RULE: 8, COUNTER_STYLE_RULE: 11, SUPPORTS_RULE: 12, FONT_FEATURE_VALUES_RULE: 14 };
  for (const [k, v] of Object.entries(RULE_TYPES)) {
    for (const o of [CSSRule, CSSRule.prototype]) Object.defineProperty(o, k, { value: v, enumerable: true });
  }
  // the child rules of a grouping rule or of @keyframes, made once
  const childRules = rule => rule._kids || (rule._kids = (rule._d.r || []).map(d => makeRule(d, rule._sheet, rule)));
  class CSSGroupingRule extends CSSRule {
    get cssRules() { return this._list || (this._list = new CSSRuleList(internal, () => childRules(this))); }
    insertRule(rule, index = 0) { stub("CSSGroupingRule.insertRule"); return unsignedLong(index); }
    deleteRule(index) { stub("CSSGroupingRule.deleteRule"); }
  }
  class CSSConditionRule extends CSSGroupingRule {
    get conditionText() { return this._d.x !== undefined ? this._d.x : this._d.m || ""; }
  }
  const updateRuleData = (rule, data) => {
    rule._d = data;
    if (rule._kids) rule._kids.forEach((child, i) => updateRuleData(child, data.r[i]));
  };
  function ruleDeclaration(rule) {
    const call = (kind, name = "", value = "", important = false) => {
      if (name.includes("\0")) return "";
      const sheet = rule._sheet, target = sheet._live(), path = [];
      for (let r = rule; r; r = r._parent) {
        const siblings = r._parent ? childRules(r._parent) : sheet._ruleObjects();
        path.unshift(siblings.indexOf(r));
      }
      const result = cssomCall(CSSOM.declaration, target, "path=" + path.join(",") + "&name=" + encodeURIComponent(name.toWellFormed()) + "&value=" + encodeURIComponent(value.toWellFormed()), kind + (important ? 16 : 0));
      if ([2, 3, 7].includes(kind)) {
        const data = JSON.parse(__mrtCssom(CSSOM.rules, target));
        sheet._ruleObjects().forEach((r, i) => updateRuleData(r, data[i]));
        noteSheetEdit(sheet._owner, sheet);
      }
      return result;
    };
    return global.__mrt.makeDeclaration({
      parentRule: rule,
      get: name => call(0, name), priority: name => call(1, name),
      set(name, value, priority) {
        if (value === "") { call(3, name); return; }
        if (priority !== "" && priority.toLowerCase() !== "important") return;
        call(2, name, value, priority !== "");
      },
      remove: name => call(3, name), text: () => call(4),
      length: () => Number(call(5)), item: index => call(6, "", String(index)),
      setText: text => call(7, "", text),
    });
  }
  class CSSStyleRule extends CSSGroupingRule {
    get style() { return this._style || (this._style = ruleDeclaration(this)); }
    set style(text) { this.style.cssText = text; }
    get selectorText() { return this._d.s; }
    set selectorText(v) { stub("CSSStyleRule.selectorText"); }
  }
  class CSSMediaRule extends CSSConditionRule { get media() { return mediaList(this._d.m); } }
  class CSSSupportsRule extends CSSConditionRule {}
  class CSSContainerRule extends CSSConditionRule {}
  class CSSMozDocumentRule extends CSSConditionRule {}
  class CSSLayerBlockRule extends CSSGroupingRule { get name() { return this._d.n; } }
  class CSSScopeRule extends CSSGroupingRule {}
  class CSSStartingStyleRule extends CSSGroupingRule {}
  class CSSPageRule extends CSSGroupingRule {}
  class CSSImportRule extends CSSRule {
    get href() { return this._d.h; }
    get media() { return mediaList(""); }
    // (never loaded: @import is not built)
    get styleSheet() { return null; }
  }
  class CSSNamespaceRule extends CSSRule {
    get namespaceURI() { return this._d.u; }
    get prefix() { return this._d.p; }
  }
  class CSSKeyframesRule extends CSSRule {
    get name() { return this._d.n; }
    get cssRules() { return this._list || (this._list = new CSSRuleList(internal, () => childRules(this))); }
    get length() { return childRules(this).length; }
  }
  class CSSKeyframeRule extends CSSRule { get keyText() { return this._d.k; } }
  class CSSFontFaceRule extends CSSRule {}
  class CSSCounterStyleRule extends CSSRule {}
  class CSSFontFeatureValuesRule extends CSSRule {}
  class CSSFontPaletteValuesRule extends CSSRule {}
  class CSSPropertyRule extends CSSRule {}
  class CSSLayerStatementRule extends CSSRule {}
  class CSSPositionTryRule extends CSSRule {}
  class CSSNestedDeclarations extends CSSRule {}
  class CSSMarginRule extends CSSRule {}
  for (const C of [CSSRule, CSSGroupingRule, CSSConditionRule, CSSStyleRule, CSSMediaRule, CSSSupportsRule, CSSContainerRule,
    CSSMozDocumentRule, CSSLayerBlockRule, CSSScopeRule, CSSStartingStyleRule, CSSPageRule, CSSImportRule, CSSNamespaceRule,
    CSSKeyframesRule, CSSKeyframeRule, CSSFontFaceRule, CSSCounterStyleRule, CSSFontFeatureValuesRule, CSSFontPaletteValuesRule,
    CSSPropertyRule, CSSLayerStatementRule, CSSPositionTryRule, CSSNestedDeclarations, CSSMarginRule]) {
    ruleClasses[C.name] = C;
    // (Window-only interfaces, as CSSRule's subclasses are in Gecko)
    if (C !== CSSRule && !isWorker) define(global, C.name, C);
  }
  if (!isWorker) define(global, "CSSRuleList", CSSRuleList);
  // A sheet's `_target` is what __mrtCssom takes: its owner element, or a constructed sheet's id.
  // A sheet whose owner got a new sheet since (its text changed, it left the document) keeps its
  // rules in a constructed sheet of its own, as Gecko's old StyleSheet keeps its inner; it no
  // longer styles anything.
  const ownerToken = Symbol("owner");
  class StyleSheet {
    constructor(token) { if (token !== internal) illegal(); }
    get type() { return "text/css"; }
    get href() { const o = this.ownerNode; return o && o.localName === "link" ? o.href || null : null; }
    get ownerNode() { this._live(); return this._owner; }
    get parentStyleSheet() { return null; }
    get title() { const o = this.ownerNode; return (o && o.getAttribute("title")) || null; }
    get media() {
      const o = this.ownerNode;
      return mediaList(this._constructed ? this._mediaText : (o && o.getAttribute("media")) || "");
    }
    get disabled() { return __mrtCssom(CSSOM.disabled, this._live()) === "1"; }
    set disabled(v) { __mrtCssom(CSSOM.setDisabled, this._live(), undefined, v ? 1 : 0); }
  }
  class CSSStyleSheet extends StyleSheet {
    // new CSSStyleSheet(options): a constructed sheet, its rules in the DOM (Gecko's StyleSheet
    // with a constructor document), styling the documents and shadow roots that adopt it;
    // bridge.js makes the sheets of <style> and <link> elements with the owner token
    constructor(options = {}, owner = undefined, generation = 0) {
      super(internal);
      if (options === ownerToken) {
        this._owner = owner;
        this._generation = generation;
        this._constructed = false;
        this._target = owner;
      } else {
        options = options || {};
        this._owner = null;
        this._constructed = true;
        this._mediaText = options.media !== undefined ? String(options.media && options.media.mediaText !== undefined ? options.media.mediaText : options.media) : "";
        this._id = Number(__mrtCssom(CSSOM.create, 0, this._mediaText, options.disabled ? 1 : 0));
        this._target = this._id;
        this._rules = [];
      }
    }
    // the target, moved to a constructed sheet of its own once the owner's sheet was made again
    _live() {
      if (this._owner && Number(__mrtCssom(CSSOM.generation, this._owner)) !== this._generation) {
        const rules = this._ruleObjects();
        const id = Number(__mrtCssom(CSSOM.create, 0, "", 0));
        __mrtCssom(CSSOM.replace, id, rules.map(r => r.cssText).join("\n"), 0);
        this._owner = null;
        this._target = id;
      }
      return this._target;
    }
    _ruleObjects() {
      if (!this._rules) this._rules = JSON.parse(__mrtCssom(CSSOM.rules, this._target)).map(d => makeRule(d, this, null));
      return this._rules;
    }
    get cssRules() { this._live(); return this._list || (this._list = new CSSRuleList(internal, () => this._ruleObjects())); }
    get rules() { return this.cssRules; }
    get ownerRule() { return null; }
    insertRule(rule, index = 0) {
      requireArgs(1, arguments.length, "CSSStyleSheet.insertRule");
      const target = this._live();
      const rules = this._ruleObjects();
      index = unsignedLong(index);
      const data = JSON.parse(cssomCall(CSSOM.insert, target, String(rule), index));
      rules.splice(index, 0, makeRule(data, this, null));
      noteSheetEdit(this._owner, this);
      return index;
    }
    // StyleSheet::DeleteRuleInternal checks the index itself
    deleteRule(index) {
      requireArgs(1, arguments.length, "CSSStyleSheet.deleteRule");
      const target = this._live();
      const rules = this._ruleObjects();
      index = unsignedLong(index);
      if (index >= rules.length) {
        throw new DOMException(`CSSStyleSheet.deleteRule: Cannot delete rule at index ${index} because the number of rules is only ${rules.length}`, "IndexSizeError");
      }
      cssomCall(CSSOM.remove, target, undefined, index);
      rules.splice(index, 1);
      noteSheetEdit(this._owner, this);
    }
    // StyleSheet::AddRule: "selector { style }" at the end by default; returns -1
    addRule(selector = "undefined", style = "undefined", index) {
      const rules = this._ruleObjects();
      this.insertRule(`${selector} { ${style} }`, index === undefined ? rules.length : index);
      return -1;
    }
    removeRule(index = 0) { this.deleteRule(index); }
    // StyleSheet::ReplaceSync: only a constructed sheet; @import rules are dropped
    replaceSync(text) {
      if (!this._constructed) throw new DOMException("CSSStyleSheet.replaceSync: Can only be called on constructed style sheets", "NotAllowedError");
      __mrtCssom(CSSOM.replace, this._id, String(text), 0);
      // (the rule objects are made when cssRules is first read)
      this._rules = null;
      noteSheetEdit(null, this);
    }
    replace(text) {
      if (!this._constructed) return Promise.reject(new DOMException("CSSStyleSheet.replace: Can only be called on constructed style sheets", "NotAllowedError"));
      return Promise.resolve().then(() => { this.replaceSync(text); return this; });
    }
  }
  // adoptedStyleSheets (DocumentOrShadowRoot): an observable array, so that assigning it and
  // changing it in place (push, splice) both reach the DOM; only constructed sheets are allowed
  const adopted = share("adopted", () => new WeakMap());
  const adoptedOf = node => {
    let entry = adopted.get(node);
    if (entry) return entry.proxy;
    const backing = [];
    const sync = () => __mrtSheet(2, node, backing.map(sh => sh._id));
    const check = v => {
      if (!(v instanceof CSSStyleSheet) || !v._constructed) {
        throw new DOMException("Each adopted style sheet must be created through the Constructable StyleSheets API", "NotAllowedError");
      }
    };
    const proxy = new Proxy(backing, {
      set(t, k, v) {
        if (typeof k === "string" && /^\d+$/.test(k)) check(v);
        t[k] = v;
        sync();
        return true;
      },
      deleteProperty(t, k) { delete t[k]; sync(); return true; },
    });
    adopted.set(node, entry = { backing, proxy, sync });
    return proxy;
  };
  if (!isWorker) for (const proto of [Document.prototype, ShadowRoot.prototype]) {
    getter(proto, "adoptedStyleSheets", function () { return adoptedOf(this); }, function (list) {
      const sheets = [...list];
      for (const sh of sheets) {
        if (!(sh instanceof CSSStyleSheet) || !sh._constructed) {
          throw new DOMException("Each adopted style sheet must be created through the Constructable StyleSheets API", "NotAllowedError");
        }
      }
      adoptedOf(this);
      const entry = adopted.get(this);
      entry.backing.length = 0;
      entry.backing.push(...sheets);
      entry.sync();
    });
  }
  define(global, "CSSStyleSheet", CSSStyleSheet);
  if (!isWorker) define(global, "StyleSheet", StyleSheet);
  define(global, "CSSRule", CSSRule);
  const sheets = share("sheets", () => new WeakMap());
  // the links whose sheet loaded (loadLinkSheet): a link has a sheet only then, as in Gecko a failed
  // load or an alternate link without a title leaves `sheet` null
  const loadedLinks = share("loadedLinks", () => new WeakSet());
  const isSheetOwner = el => el.tagName === "STYLE" || (el.tagName === "LINK" && loadedLinks.has(el));
  // Inert documents also own sheets, e.g. CSS parsed in createHTMLDocument.
  const inDocument = n => { const doc = n.ownerDocument; for (; n; n = n.parentNode) if (n === doc) return true; return false; };
  // (a new object each time the element got a new sheet: CSSOM.generation)
  if (!isWorker) reflect("sheet", function () {
    if (!isSheetOwner(this)) return undefined;
    if (!inDocument(this)) return null;
    const generation = Number(__mrtCssom(CSSOM.generation, this));
    let sheet = sheets.get(this);
    if (!sheet || sheet._generation !== generation) sheets.set(this, sheet = new CSSStyleSheet(ownerToken, this, generation));
    return sheet;
  });
  // A stylesheet <link> loads its sheet and fires load or error at the link (Loader::LoadStyleLink,
  // SheetLoadData::PrepareLoadEventIfNeeded). The sheet is fetched through the bridge and its text
  // goes to the runtime's linked-sheet cache, outside the arena (__mrtLinkSheet; SPEC decision 13);
  // the style system takes it at the link's tree position. Which links load is
  // HTMLLinkElement::GetStyleSheetInfo; a failure is SheetLoadData::VerifySheetReadyToParse.
  // YouTube's ytd-player waits for its player-css link's load before it adopts the player.
  const linkLoadsSheet = el => {
    const rel = (el.getAttribute("rel") || "").toLowerCase().split(/[\t\n\f\r ]+/);
    if (!rel.includes("stylesheet")) return false;
    const type = (el.getAttribute("type") || "").split(";")[0].trim().toLowerCase();
    if (type !== "" && type !== "text/css") return false;
    if (el.hasAttribute("disabled")) return false;
    if (rel.includes("alternate") && !el.getAttribute("title")) return false;
    return !!el.getAttribute("href");
  };
  const sheetLoadFailed = (url, r) => {
    if (!r || r.status < 200 || r.status > 299) return true;
    const ct = new Headers(r.headers).get("content-type");
    const contentType = ct === null ? "" : ct.split(";")[0].trim().toLowerCase();
    if (contentType === "text/css" || contentType === "application/x-unknown-content-type" || contentType === "") return false;
    // quirks mode accepts any type from the document's own origin, with a warning
    let sameOrigin = false;
    try { sameOrigin = new URL(url).origin === pageURL.origin; } catch (e) {}
    return !(sameOrigin && document.compatMode === "BackCompat");
  };
  // The link's sheet: fetched, then kept or removed; the event type its load ends with.
  const fetchLinkSheet = el => {
    let url, r = null;
    try { url = new URL(el.getAttribute("href"), pageURL.href).href; } catch (e) { url = null; }
    if (url !== null) {
      // without CORS, as browsers load stylesheets (Loader::LoadSheet: no-cors unless the link has a
      // crossorigin attribute): from the chrome side, as script sources (the `script` fetch); with
      // crossorigin, through the page's fetch in CORS mode
      const cors = el.hasAttribute("crossorigin");
      const credentials = cors && (el.getAttribute("crossorigin") || "").toLowerCase() === "use-credentials" ? "include" : "same-origin";
      try {
        r = bridge("fetch", cors ? { url, method: "GET", headers: [["accept", "text/css,*/*;q=0.1"]], body: null, mode: "cors", credentials,
          referrerPolicy: el.getAttribute("referrerpolicy") || "" }
          : { url, script: true, dest: "style", referrerPolicy: el.getAttribute("referrerpolicy") || "" });
      }
      catch (e) { r = null; }
    }
    return { url, r };
  };
  const applyLinkSheet = (el, { url, r }) => {
    const failed = url === null || sheetLoadFailed(url, r);
    if (failed) { __mrtLinkSheet(el, null); loadedLinks.delete(el); }
    else { __mrtLinkSheet(el, r.url || url, r.body || ""); loadedLinks.add(el); }
    return failed ? "error" : "load";
  };
  const loadLinkSheet = el => applyLinkSheet(el, fetchLinkSheet(el));
  // inserted by script: the load and its event in a task of its own
  global.__mrt.insertedLink = el => {
    if (!linkLoadsSheet(el)) return;
    const unblock = blockOnload("link " + el.getAttribute("href"));
    // fetched now; the sheet applies and its event fires when the answer arrives (SPEC decision 20)
    const f = fetchLinkSheet(el);
    afterNetwork(f.r, () => { try { el.dispatchEvent(new Event(applyLinkSheet(el, f))); } finally { unblock(); } });
  };
  // The document's own stylesheet links load in tree order with its scripts: a stylesheet blocks
  // the scripts after it until it loads (ScriptLoader's ReadyToExecuteParserBlockingScripts, the
  // document's pending sheets), and its load or error event, a task queued when the sheet
  // completes, runs before such a script. Here no task runs while the document is parsed (the
  // parser does not yield between its stops at scripts), so a link's sheet is loaded when the parser
  // inserts it, and its event fired just before the first script that follows it;
  // those after the last script, before DOMContentLoaded (__mrt.domContentLoaded). The bridge's
  // fetches complete at once.
  // A sheet whose media does not match blocks no script (SheetLoadData::ShouldDefer): its event comes
  // after the document is parsed, here between DOMContentLoaded and load (__mrt.domContentLoaded).
  let parsedLinkEvents = [], deferredLinkEvents = [];
  // the deferred scripts the parser reached, in tree order (__mrt.parserScript)
  let parserDeferred = [];
  const deferredScript = el => (isModule(el) ? !el.hasAttribute("async")
    : el.hasAttribute("defer") && el.hasAttribute("src") && !el.hasAttribute("async"));
  const loadParsedLink = el => {
    if (!linkLoadsSheet(el)) return;
    const media = el.getAttribute("media");
    const defer = media !== null && media.trim() !== "" && !matchMedia(media).matches;
    (defer ? deferredLinkEvents : parsedLinkEvents).push([el, loadLinkSheet(el)]);
  };
  const fireParsedLinkEvents = () => {
    const events = parsedLinkEvents;
    parsedLinkEvents = [];
    for (const [el, type] of events) el.dispatchEvent(new Event(type));
  };
  if (!isWorker) getter(Document.prototype, "styleSheets", function () {
    const list = [];
    for (const el of document.querySelectorAll("style, link")) {
      const sheet = isSheetOwner(el) && el.sheet;
      if (sheet) list.push(sheet);
    }
    list.item = i => list[i] || null;
    return list;
  });
  // matches, webkitMatchesSelector, closest, getRootNode, attachShadow and shadowRoot are native
  // (glue/mrt_js.cpp)
  const nodesOf = args => args.map(n => (typeof n === "object" && n !== null ? n : document.createTextNode(String(n))));
  if (!isWorker) for (const proto of [Element.prototype, CharacterData.prototype]) {
    define(proto, "before", ceReactions(function (...n) { const p = this.parentNode; if (p) for (const x of nodesOf(n)) p.insertBefore(x, this); }));
    define(proto, "after", ceReactions(function (...n) { const p = this.parentNode, next = this.nextSibling; if (p) for (const x of nodesOf(n)) p.insertBefore(x, next); }));
    define(proto, "replaceWith", ceReactions(function (...n) { const p = this.parentNode; if (p) { for (const x of nodesOf(n)) p.insertBefore(x, this); p.removeChild(this); } }));
  }
  if (!isWorker) for (const proto of [Element.prototype, Document.prototype, DocumentFragment.prototype]) {
    // nsINode::ReplaceChildren: the nodes made into one (ConvertNodesOrStringsIntoNode), then one batch
    define(proto, "replaceChildren", ceReactions(function (...n) {
      const nodes = nodesOf(n);
      let node = nodes[0] || null;
      if (nodes.length > 1) { node = document.createDocumentFragment(); for (const x of nodes) node.appendChild(x); }
      __mrtReplaceChildren(this, node);
    }));
  }
  if (!isWorker) define(Node.prototype, "replaceChild", ceReactions(function (n, old) { this.insertBefore(n, old); this.removeChild(old); return old; }));
  if (!isWorker) define(Node.prototype, "normalize", function () {});
  if (!isWorker) define(Node.prototype, "isSameNode", function (o) { return this === o; });
  // Gecko: nsINode::IsEqualNode. Two nodes are equal when their types, names and namespaces, their
  // attributes as a set (the same count, each name with the same value), their character data or
  // doctype ids, and their children in order are equal. Attribute namespaces are not kept apart
  // (GAPS B18): the qualified name stands for them. X's react-helmet-async keeps its head tags by
  // this test (GAPS B112).
  if (!isWorker) define(Node.prototype, "isEqualNode", function (other) {
    if (other === null || other === undefined) return false;
    if (typeof other !== "object" || typeof other.nodeType !== "number") throw new TypeError("Node.isEqualNode: Argument 1 is not an object.");
    const equal = (a, b) => {
      if (a === b) return true;
      if (a.nodeType !== b.nodeType || a.nodeName !== b.nodeName) return false;
      switch (a.nodeType) {
        case 1: {
          if (a.namespaceURI !== b.namespaceURI || a.localName !== b.localName) return false;
          const names = a.getAttributeNames();
          if (names.length !== b.getAttributeNames().length) return false;
          for (const n of names) if (!b.hasAttribute(n) || a.getAttribute(n) !== b.getAttribute(n)) return false;
          break;
        }
        case 3: case 4: case 7: case 8:
          if (a.data !== b.data) return false;
          break;
        case 10:
          if (a.name !== b.name || a.publicId !== b.publicId || a.systemId !== b.systemId) return false;
          break;
      }
      let x = a.firstChild, y = b.firstChild;
      for (; x && y; x = x.nextSibling, y = y.nextSibling) if (!equal(x, y)) return false;
      return !x && !y;
    };
    return equal(this, other);
  });
  if (!isWorker) define(Node.prototype, "compareDocumentPosition", function (o) {
    if (o === this) return 0;
    if (this.contains(o)) return 20;
    if (o.contains(this)) return 10;
    const all = document.querySelectorAll("*");
    const a = all.indexOf(this), b = all.indexOf(o);
    return a < 0 || b < 0 ? 1 : a < b ? 4 : 2;
  });
  if (!isWorker) Object.assign(Node, { ELEMENT_NODE: 1, ATTRIBUTE_NODE: 2, TEXT_NODE: 3, CDATA_SECTION_NODE: 4, COMMENT_NODE: 8, DOCUMENT_NODE: 9,
    DOCUMENT_TYPE_NODE: 10, DOCUMENT_FRAGMENT_NODE: 11, DOCUMENT_POSITION_DISCONNECTED: 1, DOCUMENT_POSITION_PRECEDING: 2,
    DOCUMENT_POSITION_FOLLOWING: 4, DOCUMENT_POSITION_CONTAINS: 8, DOCUMENT_POSITION_CONTAINED_BY: 16 });
  if (!isWorker) for (const m of ["focus", "blur", "setPointerCapture", "releasePointerCapture"]) {
    define(Element.prototype, m, function () {});
  }
  // (its activation: a link followed, a submit button's form submitted; bridge.js activate, below)
  if (!isWorker) define(Element.prototype, "click", function () {
    const e = new MouseEvent("click", { bubbles: true, cancelable: true, composed: true });
    this.dispatchEvent(e);
    activate(this, e, false, true);
  });
  if (!isWorker) define(Element.prototype, "getClientRects", function () { return [this.getBoundingClientRect()]; });
  // The scrolling element's client area is the root scroll frame's scroll port and its scroll size
  // the scroll range plus the port (Gecko's Element::GetClientAreaRect and GetScrollSize). With no
  // classic scrollbars, as in the Firefox it is compared with, the port is the viewport. Amazon's
  // jQuery reads $(window).height() as documentElement.clientHeight || body.clientHeight: with the
  // root's border box (0 before its frame was tracked) it took the whole body and loaded the
  // footer's recommendations at once. Other elements: their frame's client area and scroll size
  // (__mrtClientScroll, the layout driver's ClientAreaAndScrollSize). YouTube's description expander
  // grows its snippet until scrollHeight exceeds offsetHeight under `overflow: hidden`.
  // Gecko: Document::IsScrollingElement
  const isScrollingElement = el => {
    const doc = el.ownerDocument;
    if (!doc || doc !== document) return false;
    if (doc.compatMode !== "BackCompat") return el === doc.documentElement;
    return el === doc.body;  // body's own overflow (IsPotentiallyScrollable) is not looked at
  };
  if (!isWorker) getter(Element.prototype, "clientWidth", function () { return isScrollingElement(this) ? viewportSize().width : __mrtClientScroll(this)[0]; });
  if (!isWorker) getter(Element.prototype, "clientHeight", function () { return isScrollingElement(this) ? viewportSize().height : __mrtClientScroll(this)[1]; });
  if (!isWorker) getter(Element.prototype, "scrollWidth", function () {
    return isScrollingElement(this) ? Math.max(viewportSize().width, document.documentElement.offsetWidth) : __mrtClientScroll(this)[2];
  });
  if (!isWorker) getter(Element.prototype, "scrollHeight", function () {
    return isScrollingElement(this) ? Math.max(viewportSize().height, document.documentElement.offsetHeight) : __mrtClientScroll(this)[3];
  });
  if (!isWorker) for (const p of ["clientTop", "clientLeft"]) getter(Element.prototype, p, () => 0, () => {});
  // The scrolling element scrolls the document (Element::GetScrollTop and SetScrollTop, Element::Scroll
  // reach the root scroll frame); other elements do not scroll here: 0, and nothing moves (GAPS B155).
  if (!isWorker) {
    getter(Element.prototype, "scrollTop", function () { return isScrollingElement(this) ? rootScroll.y : 0; },
      function (v) { if (isScrollingElement(this)) { const n = Number(v); pageScrollTo(rootScroll.x, Number.isFinite(n) ? n : 0); } });
    getter(Element.prototype, "scrollLeft", function () { return isScrollingElement(this) ? rootScroll.x : 0; },
      function (v) { if (isScrollingElement(this)) { const n = Number(v); pageScrollTo(Number.isFinite(n) ? n : 0, rootScroll.y); } });
    define(Element.prototype, "scroll", function scroll() { if (isScrollingElement(this)) scrollTo(arguments, "Element.scroll"); });
    define(Element.prototype, "scrollTo", function scrollTo_() { if (isScrollingElement(this)) scrollTo(arguments, "Element.scrollTo"); });
    define(Element.prototype, "scrollBy", function scrollBy_() { if (isScrollingElement(this)) scrollBy(arguments, "Element.scrollBy"); });
    // Element::ScrollIntoView: the root scroll frame moves so the border box is at the block's start
    // (the default, and `true`), end (`false`), center or nearest edge, and likewise inline (nearest by
    // default). Its box in client coordinates is read after a layout flush. Not done: scroll-margin,
    // scroll-padding, the element's scroll containers (GAPS B155); an element of a frame's document
    // does not scroll its parent's.
    const align = (pos, start, size, port, how) => {
      const end = start + size;
      if (how === "start") return pos + start;
      if (how === "end") return pos + end - port;
      if (how === "center") return pos + start + size / 2 - port / 2;
      // nearest: as little as makes the box visible, its start first when it does not fit
      if ((start >= 0 && end <= port) || (start < 0 && end > port)) return pos;
      if (start < 0 !== size > port) return pos + start;
      return pos + end - port;
    };
    define(Element.prototype, "scrollIntoView", function scrollIntoView() {
      const arg = arguments[0];
      let block = "start", inline = "nearest";
      if (arg === false) block = "end";
      else if (arg !== null && typeof arg === "object") {
        if (arg.block !== undefined) block = String(arg.block);
        if (arg.inline !== undefined) inline = String(arg.inline);
        for (const [k, v] of [["block", block], ["inline", inline]]) {
          if (!["start", "center", "end", "nearest"].includes(v)) {
            throw new TypeError(`Element.scrollIntoView: '${k}' member of ScrollIntoViewOptions '${v}' is not a valid value for enumeration ScrollLogicalPosition.`);
          }
        }
      }
      if (this.ownerDocument !== document || !this.isConnected || __mrtBorderBox(this) === null) return;
      const r = this.getBoundingClientRect(), v = viewportSize();
      pageScrollTo(align(rootScroll.x, r.left, r.width, v.width, inline), align(rootScroll.y, r.top, r.height, v.height, block), v => Math.ceil(v - 0.5));
    });
  }

  // document
  // Document::CreateEvent over EventDispatcher::CreateEvent's table: the interface each name (any
  // case) stands for, an uninitialized event of it (type "", bubbles and cancelable false), and
  // NotSupportedError for any other name. The runtime lacks DragEvent, CompositionEvent,
  // DeviceOrientationEvent, DeviceMotionEvent and BeforeUnloadEvent: their names give the nearest
  // interface it has (GAPS B140); so does TextEvent, which is a CompositionEvent while
  // dom.events.textevent.enabled is off, as in release.
  if (!isWorker) {
    const CREATE_EVENT = { mouseevent: "MouseEvent", mouseevents: "MouseEvent", dragevent: "MouseEvent", keyboardevent: "KeyboardEvent",
      compositionevent: "UIEvent", textevent: "UIEvent", deviceorientationevent: "Event", devicemotionevent: "Event", uievent: "UIEvent",
      uievents: "UIEvent", event: "Event", events: "Event", htmlevents: "Event", svgevents: "Event", messageevent: "MessageEvent",
      beforeunloadevent: "Event", hashchangeevent: "HashChangeEvent", customevent: "CustomEvent", storageevent: "StorageEvent",
      focusevent: "FocusEvent" };
    if (touchEnabled) CREATE_EVENT.touchevent = "TouchEvent";
    define(Document.prototype, "createEvent", function createEvent(name) {
      if (arguments.length < 1) throw new TypeError("Document.createEvent: At least 1 argument required, but only 0 passed");
      const C = CREATE_EVENT[String(name).toLowerCase()];
      if (!C) throw new DOMException("Operation is not supported", "NotSupportedError");
      const e = new global[C]("");
      // a UIEvent's view is the window of the document's pres context (UIEvent::UIEvent)
      if (e instanceof global.UIEvent) e.view = this.defaultView ?? null;
      return e;
    });
  }
  if (!isWorker) define(Document.prototype, "getElementsByName", function (n) { return this.querySelectorAll(`[name="${String(n).replace(/"/g, '\\"')}"]`); });
  if (!isWorker) for (const [p, sel] of [["forms", "form"], ["images", "img"], ["links", "a[href],area[href]"], ["scripts", "script"], ["embeds", "embed"], ["plugins", "embed"]]) {
    getter(Document.prototype, p, function () { return this.querySelectorAll(sel); });
  }

  // NodeList and HTMLCollection. The lists stay new arrays on each read (not live, GAPS B11), given
  // these prototypes so that `instanceof NodeList` and item() work; the prototypes inherit Array's,
  // which Firefox's do not (a site that tests for `map` on a NodeList sees one).
  const listClass = name => {
    const C = function () { throw new TypeError("Illegal constructor."); };
    Object.defineProperty(C, "name", { value: name });
    C.prototype = Object.create(Array.prototype, {
      constructor: { value: C, writable: true, configurable: true },
      item: { value(i) { return this[i] ?? null; }, writable: true, configurable: true },
      [Symbol.toStringTag]: { value: name, configurable: true },
    });
    define(global, name, C);
    return C;
  };
  const NodeList = listClass("NodeList"), HTMLCollection = listClass("HTMLCollection");
  HTMLCollection.prototype.namedItem = function (n) { return this.find(el => el.id === n || el.getAttribute("name") === n) || null; };
  const asList = (C, a) => (Array.isArray(a) ? Object.setPrototypeOf(a, C.prototype) : a);
  const ownerOf = (proto, name) => { for (let o = proto; o; o = Object.getPrototypeOf(o)) if (Object.getOwnPropertyDescriptor(o, name)) return o; return null; };
  if (!isWorker) for (const [proto, name, C] of [[Node.prototype, "childNodes", NodeList], [Element.prototype, "children", HTMLCollection],
                                  [Document.prototype, "children", HTMLCollection], [DocumentFragment.prototype, "children", HTMLCollection],
                                  [Document.prototype, "forms", HTMLCollection], [Document.prototype, "images", HTMLCollection],
                                  [Document.prototype, "links", HTMLCollection], [Document.prototype, "scripts", HTMLCollection],
                                  [Document.prototype, "embeds", HTMLCollection], [Document.prototype, "plugins", HTMLCollection]]) {
    const o = ownerOf(proto, name);
    const d = o && Object.getOwnPropertyDescriptor(o, name);
    if (d && d.get) Object.defineProperty(proto, name, { configurable: true, enumerable: true, get() { return asList(C, d.get.call(this)); } });
  }
  if (!isWorker) for (const proto of [Element.prototype, Document.prototype, DocumentFragment.prototype]) {
    for (const [name, C] of [["querySelectorAll", NodeList], ["getElementsByTagName", HTMLCollection],
                             ["getElementsByClassName", HTMLCollection], ["getElementsByName", NodeList]]) {
      const o = ownerOf(proto, name);
      const f = o && o[name];
      if (typeof f === "function") define(proto, name, function (...args) { return asList(C, f.apply(this, args)); });
    }
  }

  // ---- document.all: Gecko's HTMLAllCollection (dom/html/HTMLAllCollection.cpp), one per document ----
  // The object is the C++ layer's proxy that emulates undefined (__mrtEmulateUndefined) over a JS
  // Proxy that does the rest: the indexed getter over getElementsByTagName("*") (Collection()), the
  // named getter (NamedGetter: ids on any element, `name` on the IsAllNamedElement tags; two matches
  // give an HTMLCollection, an array as the others, GAPS B11), named properties unenumerable and
  // hidden by the prototype's, and the legacy caller (Item). Expandos live on the Proxy's target.
  if (!isWorker) {
    const allStates = new WeakMap();
    const HTMLAllCollection = function () { throw new TypeError("Illegal constructor."); };
    Object.defineProperty(HTMLAllCollection, "name", { value: "HTMLAllCollection" });
    Object.defineProperty(HTMLAllCollection, "length", { value: 0 });
    const allState = (self, member) => {
      const doc = allStates.get(self);
      if (!doc) throw new TypeError(`'${member}' called on an object that does not implement interface HTMLAllCollection.`);
      return doc;
    };
    const allElements = doc => Document.prototype.getElementsByTagName.call(doc, "*");
    const ALL_NAMED_TAGS = ["a", "button", "embed", "form", "iframe", "img", "input", "map", "meta", "object", "select", "textarea", "frame", "frameset"];
    const isAllNamed = el => el.namespaceURI === "http://www.w3.org/1999/xhtml" && ALL_NAMED_TAGS.includes(el.localName);
    // NamedGetter: undefined when nothing matches
    const allNamed = (doc, name) => {
      if (name === "") return undefined;
      const found = Array.from(allElements(doc)).filter(el =>
        el.getAttribute("id") === name || (isAllNamed(el) && el.getAttribute("name") === name));
      return found.length > 1 ? asList(HTMLCollection, found) : found[0];
    };
    // Item(nameOrIndex): null when not passed (undefined counts as not passed, an optional argument)
    const allItem = (doc, nameOrIndex) => {
      if (nameOrIndex === undefined) return null;
      const s = String(nameOrIndex);
      if (isArrayIndex(s)) return allElements(doc)[Number(s)] || null;
      return allNamed(doc, s) || null;
    };
    // js::StringIsArrayIndex: a canonical number below 2^32 - 1
    const isArrayIndex = s => /^(0|[1-9]\d*)$/.test(s) && Number(s) < 4294967295;
    define(HTMLAllCollection.prototype, "namedItem", function namedItem(name) {
      const doc = allState(this, "namedItem");
      if (arguments.length < 1) throw new TypeError("HTMLAllCollection.namedItem: At least 1 argument required, but only 0 passed");
      return allNamed(doc, String(name)) || null;
    });
    // its argument is optional: length 0
    define(HTMLAllCollection.prototype, "item", function item() { return allItem(allState(this, "item"), arguments[0]); });
    getter(HTMLAllCollection.prototype, "length", function () { return allElements(allState(this, "get length")).length; });
    for (const name of ["namedItem", "item"]) Object.defineProperty(HTMLAllCollection.prototype, name, { enumerable: true });
    define(HTMLAllCollection.prototype, Symbol.iterator, Array.prototype.values);
    Object.defineProperty(HTMLAllCollection.prototype, Symbol.toStringTag, { value: "HTMLAllCollection", configurable: true });
    define(global, "HTMLAllCollection", HTMLAllCollection);

    const makeAll = doc => {
      const target = () => {};
      delete target.length;
      delete target.name;
      Object.setPrototypeOf(target, HTMLAllCollection.prototype);
      const proto = HTMLAllCollection.prototype;
      // a visible named property: supported, not an expando, not found on the prototype chain
      const named = key => (typeof key === "string" && !isArrayIndex(key) && !Object.prototype.hasOwnProperty.call(target, key) && !(key in proto)
        ? allNamed(doc, key) : undefined);
      const indexed = key => (typeof key === "string" && isArrayIndex(key) ? allElements(doc)[Number(key)] : undefined);
      const own = key => {
        if (typeof key === "string" && isArrayIndex(key)) {
          const el = indexed(key);
          return el ? { value: el, writable: false, enumerable: true, configurable: true } : undefined;
        }
        const el = named(key);
        if (el) return { value: el, writable: false, enumerable: false, configurable: true };
        return Reflect.getOwnPropertyDescriptor(target, key);
      };
      const all = __mrtEmulateUndefined(new Proxy(target, {
        getOwnPropertyDescriptor: (t, key) => own(key),
        has: (t, key) => !!own(key) || key in proto,
        get(t, key, receiver) {
          const d = own(key);
          if (d) return d.get ? d.get.call(receiver) : d.value;
          return Reflect.get(proto, key, receiver);
        },
        defineProperty(t, key, desc) {
          if (typeof key === "string" && isArrayIndex(key)) return false;
          if (named(key)) return false;
          return Reflect.defineProperty(t, key, desc);
        },
        deleteProperty(t, key) {
          if (typeof key === "string" && isArrayIndex(key)) return !indexed(key);
          if (named(key)) return false;
          return Reflect.deleteProperty(t, key);
        },
        ownKeys(t) {
          const keys = [], names = [];
          const els = allElements(doc);
          for (let i = 0; i < els.length; i++) keys.push(String(i));
          // ContentList::GetSupportedNames with IsAllNamedElement, less the names the prototype hides
          for (const el of els) {
            const id = el.getAttribute("id"), name = isAllNamed(el) ? el.getAttribute("name") : null;
            for (const n of [id, name]) if (n && !names.includes(n)) names.push(n);
          }
          for (const n of names) if (!isArrayIndex(n) && !(n in proto) && !Object.prototype.hasOwnProperty.call(t, n)) keys.push(n);
          return keys.concat(Reflect.ownKeys(t));
        },
        apply: (t, self, args) => allItem(doc, args[0]),
      }));
      allStates.set(all, doc);
      return all;
    };
    const docAlls = new WeakMap();
    getter(Document.prototype, "all", Object.getOwnPropertyDescriptor({
      get all() {
        if (!(this instanceof Document)) throw new TypeError("'get all' called on an object that does not implement interface Document.");
        // not `if (!all)`: the object converts to false
        if (!docAlls.has(this)) docAlls.set(this, makeAll(this));
        return docAlls.get(this);
      },
    }, "all").get);
  }

  // ---- window.external: Gecko's External (dom/base/External.cpp), one per window ----
  // [LegacyNoInterfaceObject]: no global `External`, and the prototype's constructor is Object's.
  // Both methods do nothing (search engines come from OpenSearch links now). The sites only test
  // that it exists (X's isWebView and Castle, Amazon, Gmail, Google Docs; session 92).
  if (!isWorker) {
    // shared by the runtime's windows: a method takes any window's object, and the getter any window
    // (a frame's getter called on its parent gives the parent's object)
    const externals = share("externals", () => new WeakSet());
    const externalOf = share("externalOf", () => new WeakMap());
    const externalProto = {};
    for (const name of ["AddSearchProvider", "IsSearchProviderInstalled"]) {
      // a method's syntax: no `prototype`, not a constructor, as a binding's function
      const method = { [name]() {
        if (!externals.has(this)) throw new TypeError(`'${name}' called on an object that does not implement interface External.`);
      } }[name];
      Object.defineProperty(externalProto, name, { value: method, writable: true, enumerable: true, configurable: true });
    }
    Object.defineProperty(externalProto, Symbol.toStringTag, { value: "External", configurable: true });
    let external;
    externalOf.set(global, () => {
      if (!external) externals.add(external = Object.create(externalProto));
      return external;
    });
    // [Replaceable]: an assignment makes an own data property
    Object.defineProperty(global, "external", {
      get: Object.getOwnPropertyDescriptor({ get external() {
        // undefined or null is the global, as a [Global] interface's attribute takes it
        const of = externalOf.get(this ?? global);
        if (!of) throw new TypeError("'get external' called on an object that does not implement interface Window.");
        return of();
      } }, "external").get,
      set: Object.getOwnPropertyDescriptor({ set external(value) {
        Object.defineProperty(global, "external", { value, writable: true, enumerable: true, configurable: true });
      } }, "external").set,
      enumerable: true,
      configurable: true,
    });
  }

  // ---- screenX, screenY and their aliases screenLeft, screenTop (Window.webidl) ----
  // nsGlobalWindowOuter::GetScreenXY: the top-level browser window's position in CSS px, so a frame
  // reads its top's. The bridge tab's, read once at start (env); a window moved later is not followed.
  if (!isWorker) {
    const screenWindows = share("screenWindows", () => new WeakSet());
    screenWindows.add(global);
    // undefined or null is the global, as a [Global] interface's attribute takes it
    const windowOf = (self, what) => {
      const w = self ?? global;
      if (!screenWindows.has(w)) throw new TypeError(`'${what}' called on an object that does not implement interface Window.`);
      return w;
    };
    for (const [name, key] of [["screenX", "screenX"], ["screenY", "screenY"], ["screenLeft", "screenX"], ["screenTop", "screenY"]]) {
      // [Replaceable]: an assignment makes an own data property on the window it is made on
      Object.defineProperty(global, name, {
        get: Object.getOwnPropertyDescriptor({ get [name]() {
          windowOf(this, `get ${name}`);
          return env[key] ?? 0;
        } }, name).get,
        set: Object.getOwnPropertyDescriptor({ set [name](value) {
          Object.defineProperty(windowOf(this, `set ${name}`), name, { value, writable: true, enumerable: true, configurable: true });
        } }, name).set,
        enumerable: true,
        configurable: true,
      });
    }
  }
  // ---- reportError (WindowOrWorkerGlobalScope.webidl) ----
  // The parts of the error report Gecko builds for the value (JS::ErrorReportBuilder, NoSideEffects):
  // an Error (a DOMException is one here) gives "name: message" and the place it was made; any other
  // value "uncaught exception: " and its string, every object "Object", at the caller's place, the
  // frame after reportError's own in `stack`.
  const GLUE_FILES = new Set(["prelude.js", "bridge.js"]);
  const reportedParts = (e, stack) => {
    if (Error.isError(e)) {
      const name = typeof e.name === "string" ? e.name : "Error";
      const message = typeof e.message === "string" ? e.message : "";
      const parts = { message: `${name}: ${message}`, filename: String(e.fileName ?? ""), lineno: e.lineNumber | 0, colno: e.columnNumber | 0 };
      // one the runtime's own script made (a DOMException, an API's TypeError) is Firefox's binding's,
      // which takes the page's place that called it: the first frame of its stack outside the glue
      if (GLUE_FILES.has(parts.filename)) {
        for (const line of String(e.stack).split("\n")) {
          const at = /@(.*):(\d+):(\d+)$/.exec(line);
          if (at && !GLUE_FILES.has(at[1])) return { ...parts, filename: at[1], lineno: +at[2], colno: +at[3] };
        }
      }
      return parts;
    }
    const text = typeof e === "symbol" ? e.toString() : e !== null && (typeof e === "object" || typeof e === "function") ? "Object" : String(e);
    const at = /@(.*):(\d+):(\d+)$/.exec(String(stack).split("\n")[1] || "");
    return { message: "uncaught exception: " + text, filename: at ? at[1] : "", lineno: at ? +at[2] : 0, colno: at ? +at[3] : 0 };
  };
  // nsGlobalWindowInner::ReportError: DispatchScriptErrorEvent fires a trusted ErrorEvent at the window
  // at once (a script runner, run while script is running), its onerror called with the parts; none
  // while one is handled (sHandlingScriptError); not cancelled, the report goes to the console. A
  // worker's goes through the worker's own report (its section below).
  if (!isWorker) {
    const reportWindows = share("reportWindows", () => new WeakSet());
    reportWindows.add(global);
    const handling = share("reportHandling", () => ({ now: false }));
    const reportOn = (win, e, parts) => {
      let cancelled = false;
      if (!handling.now) {
        handling.now = true;
        try {
          const ev = new win.ErrorEvent("error", { bubbles: true, cancelable: true, ...parts, error: e });
          // (the runtime's events have no isTrusted, GAPS B72; this one is the browser's)
          ev.isTrusted = true;
          ev._onErrorArgs = [parts.message, parts.filename, parts.lineno, parts.colno, e];
          cancelled = !win.dispatchEvent(ev);
        } finally {
          handling.now = false;
        }
      }
      if (!cancelled) console.error(parts.message + (Error.isError(e) ? "\n" + e.stack : ""));
    };
    // nsIScriptGlobalObject::HandleScriptError: an error the browser reports at this window, with no
    // error value (ResizeObserver's loop limit); `parts` may set the event's bubbles and cancelable
    define(global.__mrt, "reportScriptError", parts => reportOn(global, undefined, parts));
    Object.defineProperty(global, "reportError", { value: { reportError(e) {
      const stack = new Error().stack;
      // undefined or null is the global, as a [Global] interface's operation takes it
      const win = this ?? global;
      if (!reportWindows.has(win)) throw new TypeError("'reportError' called on an object that does not implement interface Window.");
      if (arguments.length < 1) throw new TypeError("Window.reportError: At least 1 argument required, but only 0 passed");
      reportOn(win, e, reportedParts(e, stack));
    } }.reportError, writable: true, enumerable: true, configurable: true });
  }
  // ---- tables: HTMLTableElement, HTMLTableSectionElement, HTMLTableRowElement and HTMLTableCellElement
  // (dom/html/HTMLTable*Element.cpp), over the DOM; the collections are arrays (GAPS B11) ----
  const isHTMLEl = (n, ...tags) => n && n.nodeType === 1 && n.namespaceURI === "http://www.w3.org/1999/xhtml" && tags.includes(n.localName);
  const childrenTagged = (el, ...tags) => Array.from(el.childNodes).filter(c => isHTMLEl(c, ...tags));
  const firstChildTagged = (el, tag) => childrenTagged(el, tag)[0] || null;
  const indexSizeError = () => new DOMException("Index or size is negative or greater than the allowed amount", "IndexSizeError");
  if (!isWorker) {
    const T = global.HTMLTableElement.prototype, S = global.HTMLTableSectionElement.prototype;
    const R = global.HTMLTableRowElement.prototype, C = global.HTMLTableCellElement.prototype;
    // TableRowsCollection: the rows of the thead children, then the table's own and its tbody
    // children's in tree order, then those of the tfoot children
    const tableRows = table => {
      const out = [], heads = [], feet = [];
      for (const c of table.childNodes) {
        if (isHTMLEl(c, "thead")) heads.push(...childrenTagged(c, "tr"));
        else if (isHTMLEl(c, "tfoot")) feet.push(...childrenTagged(c, "tr"));
        else if (isHTMLEl(c, "tbody")) out.push(...childrenTagged(c, "tr"));
        else if (isHTMLEl(c, "tr")) out.push(c);
      }
      return heads.concat(out, feet);
    };
    // the WebIDL type first (a TypeError), then the tag (HTMLTableElement::SetTHead and the others)
    const setSection = (table, tag, value, before) => {
      const type = tag === "caption" ? global.HTMLTableCaptionElement : global.HTMLTableSectionElement;
      if (value !== null && !(value instanceof type)) throw new TypeError(`Value being assigned is not an object of type ${type.name}.`);
      if (value !== null && !isHTMLEl(value, tag)) throw new DOMException("Hierarchy request error", "HierarchyRequestError");
      const old = firstChildTagged(table, tag);
      if (old) old.remove();
      if (value) table.insertBefore(value, before(table));
    };
    const firstNotCaptionOrColgroup = t => Array.from(t.childNodes).find(c => c.nodeType === 1 && !isHTMLEl(c, "caption", "colgroup")) || null;
    getter(T, "caption", function () { return firstChildTagged(this, "caption"); }, function (v) { setSection(this, "caption", v, t => t.firstChild); });
    getter(T, "tHead", function () { return firstChildTagged(this, "thead"); }, function (v) { setSection(this, "thead", v, firstNotCaptionOrColgroup); });
    getter(T, "tFoot", function () { return firstChildTagged(this, "tfoot"); }, function (v) { setSection(this, "tfoot", v, () => null); });
    getter(T, "tBodies", function () { return asList(HTMLCollection, childrenTagged(this, "tbody")); });
    getter(T, "rows", function () { return asList(HTMLCollection, tableRows(this)); });
    const create = (tag, where) => function () {
      let el = firstChildTagged(this, tag);
      if (!el) { el = this.ownerDocument.createElement(tag); this.insertBefore(el, where(this)); }
      return el;
    };
    define(T, "createCaption", create("caption", t => t.firstChild));
    define(T, "createTHead", create("thead", firstNotCaptionOrColgroup));
    define(T, "createTFoot", create("tfoot", () => null));
    for (const [name, tag] of [["deleteCaption", "caption"], ["deleteTHead", "thead"], ["deleteTFoot", "tfoot"]]) {
      define(T, name, function () { const el = firstChildTagged(this, tag); if (el) el.remove(); });
    }
    // CreateTBody: after the last tbody child, or at the end
    define(T, "createTBody", function () {
      const el = this.ownerDocument.createElement("tbody");
      const bodies = childrenTagged(this, "tbody");
      this.insertBefore(el, bodies.length ? bodies[bodies.length - 1].nextSibling : null);
      return el;
    });
    define(T, "insertRow", function (index = -1) {
      index = Number(index) | 0;
      const rows = tableRows(this);
      if (index < -1 || index > rows.length) throw indexSizeError();
      const row = this.ownerDocument.createElement("tr");
      if (rows.length) {
        const ref = rows[index === -1 || index === rows.length ? rows.length - 1 : index];
        if (index === -1 || index === rows.length) ref.parentNode.appendChild(row);
        else ref.parentNode.insertBefore(row, ref);
      } else {
        let group = null;
        for (let c = this.lastChild; c; c = c.previousSibling) if (isHTMLEl(c, "tbody")) { group = c; break; }
        if (!group) { group = this.ownerDocument.createElement("tbody"); this.appendChild(group); }
        group.appendChild(row);
      }
      return row;
    });
    define(T, "deleteRow", function (index) {
      index = Number(index) | 0;
      const rows = tableRows(this);
      if (index === -1) { if (rows.length) rows[rows.length - 1].remove(); return; }
      if (index < -1 || index >= rows.length) throw indexSizeError();
      rows[index].remove();
    });
    getter(S, "rows", function () { return asList(HTMLCollection, childrenTagged(this, "tr")); });
    define(S, "insertRow", function (index = -1) {
      index = Number(index) | 0;
      const rows = childrenTagged(this, "tr");
      if (index < -1 || index > rows.length) throw indexSizeError();
      const row = this.ownerDocument.createElement("tr");
      this.insertBefore(row, index === -1 || index === rows.length ? null : rows[index]);
      return row;
    });
    define(S, "deleteRow", function (index) {
      index = Number(index) | 0;
      const rows = childrenTagged(this, "tr");
      if (index === -1) { if (rows.length) rows[rows.length - 1].remove(); return; }
      if (index < -1 || index >= rows.length) throw indexSizeError();
      rows[index].remove();
    });
    getter(R, "cells", function () { return asList(HTMLCollection, childrenTagged(this, "td", "th")); });
    // rowIndex: the row's place in its table's rows; -1 outside a table (HTMLTableRowElement::RowIndex)
    getter(R, "rowIndex", function () {
      let p = this.parentNode;
      if (isHTMLEl(p, "thead", "tbody", "tfoot")) p = p.parentNode;
      return isHTMLEl(p, "table") ? tableRows(p).indexOf(this) : -1;
    });
    getter(R, "sectionRowIndex", function () {
      const p = this.parentNode;
      return isHTMLEl(p, "thead", "tbody", "tfoot", "table") ? childrenTagged(p, "tr").indexOf(this) : -1;
    });
    define(R, "insertCell", function (index = -1) {
      index = Number(index) | 0;
      const cells = childrenTagged(this, "td", "th");
      if (index < -1 || index > cells.length) throw indexSizeError();
      const cell = this.ownerDocument.createElement("td");
      this.insertBefore(cell, index === -1 || index === cells.length ? null : cells[index]);
      return cell;
    });
    define(R, "deleteCell", function (index) {
      index = Number(index) | 0;
      const cells = childrenTagged(this, "td", "th");
      if (index === -1) { if (cells.length) cells[cells.length - 1].remove(); return; }
      if (index < -1 || index >= cells.length) throw indexSizeError();
      cells[index].remove();
    });
    getter(C, "cellIndex", function () {
      return isHTMLEl(this.parentNode, "tr") ? childrenTagged(this.parentNode, "td", "th").indexOf(this) : -1;
    });
  }
  // ---- editing commands (Document::QueryCommandSupported and the others, dom/base/Document.cpp) ----
  //
  // The command table of EnsureInitializeInternalCommandDataHashtable, looked up without case;
  // contentReadOnly and insertBrOnReturn are off by their prefs, and justifynone, saveas and print are
  // Command::Undefined (not supported). Nothing is editable here (no editor, no selection):
  // queryCommandEnabled answers as Firefox does with nothing selected (selectAll only), execCommand does
  // nothing and answers false (GAPS B55).
  const EDIT_COMMANDS = new Set(["bold", "italic", "underline", "strikethrough", "subscript", "superscript", "cut", "copy", "paste",
    "delete", "forwarddelete", "selectall", "undo", "redo", "indent", "outdent", "backcolor", "hilitecolor", "forecolor", "fontname",
    "fontsize", "inserthorizontalrule", "createlink", "insertimage", "inserthtml", "inserttext", "justifyleft", "justifyright",
    "justifycenter", "justifyfull", "removeformat", "unlink", "insertorderedlist", "insertunorderedlist", "insertparagraph",
    "insertlinebreak", "formatblock", "stylewithcss", "usecss", "defaultparagraphseparator", "enableobjectresizing",
    "enableinlinetableediting", "enableabsolutepositionediting", "enablecompatiblejoinsplitdirection"]);
  const commandSupported = name => EDIT_COMMANDS.has(String(name).toLowerCase());
  if (!isWorker) define(Document.prototype, "queryCommandSupported", function (name) { return commandSupported(name); });
  if (!isWorker) define(Document.prototype, "queryCommandEnabled", function (name) {
    stub("Document.queryCommandEnabled");
    return String(name).toLowerCase() === "selectall";
  });
  if (!isWorker) define(Document.prototype, "queryCommandState", function () { stub("Document.queryCommandState"); return false; });
  if (!isWorker) define(Document.prototype, "queryCommandIndeterm", function () { return false; });
  if (!isWorker) define(Document.prototype, "queryCommandValue", function () { stub("Document.queryCommandValue"); return ""; });
  if (!isWorker) define(Document.prototype, "execCommand", function () { stub("Document.execCommand"); return false; });
  if (!isWorker) {
    const designModes = new WeakMap();
    getter(Document.prototype, "designMode", function () { return designModes.get(this) || "off"; },
      function (v) { const m = String(v).toLowerCase(); if (m === "on" || m === "off") designModes.set(this, m); });
  }
  // nsGenericHTMLElement::GetContentEditable, SetContentEditable and IsContentEditable (the
  // attribute's state, inherited from the nearest ancestor that sets it)
  const editableState = el => {
    if (!el.hasAttribute("contenteditable")) return "inherit";
    const v = el.getAttribute("contenteditable").toLowerCase();
    return v === "" || v === "true" ? "true" : v === "false" ? "false" : v === "plaintext-only" ? "plaintext-only" : "inherit";
  };
  if (!isWorker) getter(HTMLElement.prototype, "contentEditable", function () { return editableState(this); }, function (v) {
    const m = String(v).toLowerCase();
    if (m === "inherit") this.removeAttribute("contenteditable");
    else if (m === "true" || m === "false" || m === "plaintext-only") this.setAttribute("contenteditable", m);
    else throw new DOMException("An invalid or illegal string was specified", "SyntaxError");
  });
  if (!isWorker) getter(HTMLElement.prototype, "isContentEditable", function () {
    if (this.ownerDocument && this.ownerDocument.designMode === "on") return true;
    for (let n = this; n && n.nodeType === 1; n = n.parentNode) {
      const st = n instanceof HTMLElement ? editableState(n) : "inherit";
      if (st !== "inherit") return st !== "false";
    }
    return false;
  });

  if (!isWorker) getter(Document.prototype, "title", function () { const t = this.querySelector("title"); return t ? t.textContent.trim() : ""; },
    function (v) { let t = this.querySelector("title"); if (!t && this.head) { t = this.createElement("title"); this.head.appendChild(t); } if (t) t.textContent = String(v); });
  // the focused element: the bridge tab's, as the owner's input moves it (realsite --interactive,
  // inputEvents); else, and once it has left the document, the body (element.focus() does nothing yet)
  const focus = { element: null };
  if (!isWorker) getter(Document.prototype, "activeElement", function () {
    const el = focus.element;
    return el && this === global.document && el.isConnected && el.ownerDocument === this ? el : this.body;
  });
  // Document::GetDefaultView: the window whose document this is; null for one that has no window
  // (DOMParser's, createHTMLDocument's, XHR's)
  if (!isWorker) getter(Document.prototype, "defaultView", function () { return this === global.document ? global : null; });
  if (!isWorker) getter(Document.prototype, "characterSet", () => "UTF-8");
  if (!isWorker) getter(Document.prototype, "charset", () => "UTF-8");
  if (!isWorker) getter(Document.prototype, "contentType", () => "text/html");
  // compatMode is native (GAPS D26); Gecko: Document::GetScrollingElement
  if (!isWorker) getter(Document.prototype, "scrollingElement", function () {
    return this.compatMode === "BackCompat" ? this.body : this.documentElement;
  });
  if (!isWorker) getter(Document.prototype, "fonts", () => ({ ready: Promise.resolve(), check: () => true, load: () => Promise.resolve([]), add() {}, addEventListener() {}, status: "loaded" }));
  if (!isWorker) define(Document.prototype, "hasStorageAccess", () => Promise.resolve(true));
  if (!isWorker) define(Document.prototype, "execCommand", () => false);
  // The document's Selection (Gecko's Selection.webidl): the bridge tab's, as the owner's input
  // left it (realsite --interactive: each event brings the tab's type, text, anchor and focus); with
  // no input, nothing selected ("None"). Its range is the runtime's Range, which keeps boundaries
  // only, and toString() is the tab's text. What the page does to it stays here: the tab's selection
  // does not follow (GAPS B152).
  class Selection {
    constructor(token) {
      if (token !== selectionToken) throw new TypeError("Illegal constructor.");
      this._s = { type: "None", text: "", anchor: null, anchorOffset: 0, focus: null, focusOffset: 0 };
      this._range = null;
    }
    get type() { return this._s.type; }
    get anchorNode() { return this._s.anchor; }
    get anchorOffset() { return this._s.anchorOffset; }
    get focusNode() { return this._s.focus; }
    get focusOffset() { return this._s.focusOffset; }
    get isCollapsed() { return this._s.type !== "Range"; }
    get rangeCount() { return this._s.type === "None" ? 0 : 1; }
    get direction() { return this._s.type === "Range" ? "forward" : "none"; }
    getRangeAt(index) {
      if (Number(index) !== 0 || this._s.type === "None") throw new DOMException("The index is not in the allowed range.", "IndexSizeError");
      if (!this._range) {
        this._range = Object.assign(new Range(), { startContainer: this._s.anchor, startOffset: this._s.anchorOffset,
          endContainer: this._s.focus, endOffset: this._s.focusOffset, collapsed: this._s.type !== "Range" });
      }
      return this._range;
    }
    _set(type, anchor, anchorOffset, focus, focusOffset, text) {
      this._s = { type, text, anchor, anchorOffset, focus, focusOffset };
      this._range = null;
    }
    addRange(r) { if (this._s.type === "None") { this._set(r.collapsed ? "Caret" : "Range", r.startContainer, r.startOffset, r.endContainer, r.endOffset, ""); this._range = r; } }
    removeRange() { this._set("None", null, 0, null, 0, ""); }
    removeAllRanges() { this._set("None", null, 0, null, 0, ""); }
    empty() { this.removeAllRanges(); }
    collapse(node, offset = 0) { if (node === null) this.removeAllRanges(); else this._set("Caret", node, Number(offset) | 0, node, Number(offset) | 0, ""); }
    setPosition(node, offset = 0) { this.collapse(node, offset); }
    collapseToStart() { if (this._s.type !== "None") this.collapse(this._s.anchor, this._s.anchorOffset); }
    collapseToEnd() { if (this._s.type !== "None") this.collapse(this._s.focus, this._s.focusOffset); }
    extend(node, offset = 0) { if (this._s.type !== "None") this._set("Range", this._s.anchor, this._s.anchorOffset, node, Number(offset) | 0, ""); }
    setBaseAndExtent(a, ao, f, fo) { this._set(a === f && ao === fo ? "Caret" : "Range", a, Number(ao) | 0, f, Number(fo) | 0, ""); }
    selectAllChildren(node) { this._set("Range", node, 0, node, node.childNodes.length, node.textContent); }
    containsNode() { return false; }
    deleteFromDocument() {}
    modify() {}
    toString() { return this._s.text; }
  }
  const selectionToken = Symbol("selection");
  Object.defineProperty(Selection.prototype, Symbol.toStringTag, { value: "Selection", configurable: true });
  define(global, "Selection", Selection);
  const selection = isWorker ? null : new Selection(selectionToken);
  if (!isWorker) define(Document.prototype, "getSelection", function getSelection() { return this === global.document ? selection : null; });
  if (!isWorker) define(global, "getSelection", function getSelection() { return selection; });

  // EventTarget can be constructed and extended, as in Firefox
  {
    const proto = EventTarget.prototype;
    const ET = function EventTarget() {};
    ET.prototype = proto;
    define(proto, "constructor", ET);
    define(global, "EventTarget", ET);
  }
  // the dictionary defaults that are not 0 (MessageEventInit, CloseEventInit, ErrorEventInit: its
  // `any error` has no default, so it is undefined)
  const EVENT_INIT_DEFAULTS = { view: null, relatedTarget: null, data: null, origin: "", lastEventId: "", source: null, wasClean: false, reason: "", message: "", filename: "", error: undefined };
  // an interface's own defaults where a field name means something else elsewhere (KeyboardEvent's key)
  const EVENT_INIT_DEFAULTS_OF = { StorageEvent: { key: null, oldValue: null, newValue: null, url: "", storageArea: null } };
  // each interface's parent in Gecko's WebIDL, when it is not Event (MouseEvent : UIEvent)
  const EVENT_PARENTS = { MouseEvent: "UIEvent", PointerEvent: "MouseEvent", WheelEvent: "MouseEvent", KeyboardEvent: "UIEvent",
    FocusEvent: "UIEvent", InputEvent: "UIEvent", TouchEvent: "UIEvent" };
  for (const [name, fields] of [["UIEvent", ["view", "detail"]], ["MouseEvent", ["clientX", "clientY", "screenX", "screenY", "button", "buttons", "ctrlKey", "shiftKey", "altKey", "metaKey", "relatedTarget"]],
    ["PointerEvent", ["pointerId", "pointerType"]], ["KeyboardEvent", ["key", "code", "keyCode", "ctrlKey", "shiftKey", "altKey", "metaKey", "repeat"]], ["FocusEvent", ["relatedTarget"]],
    ["MessageEvent", ["data", "origin", "lastEventId", "source", "ports"]], ["CloseEvent", ["wasClean", "code", "reason"]], ["ErrorEvent", ["message", "filename", "lineno", "colno", "error"]], ["PopStateEvent", ["state"]],
    ["StorageEvent", ["key", "oldValue", "newValue", "url", "storageArea"]], ["ProgressEvent", ["loaded", "total", "lengthComputable"]], ["PageTransitionEvent", ["persisted"]],
    ["HashChangeEvent", ["oldURL", "newURL"]], ["AnimationEvent", ["animationName"]], ["TransitionEvent", ["propertyName"]], ["WheelEvent", ["deltaX", "deltaY"]],
    ["TouchEvent", ["touches"]], ["InputEvent", ["data", "inputType"]], ["SubmitEvent", ["submitter"]], ["PromiseRejectionEvent", ["promise", "reason"]]]) {
    if (name === "TouchEvent" && !touchEnabled) continue;
    const own = EVENT_INIT_DEFAULTS_OF[name] || {};
    const C = class extends (global[EVENT_PARENTS[name]] || Event) {
      constructor(type, init = {}) { super(type, init); for (const f of fields) this[f] = init[f] === undefined ? (f in own ? own[f] : f in EVENT_INIT_DEFAULTS ? EVENT_INIT_DEFAULTS[f] : f.endsWith("Key") || f === "repeat" ? false : f === "ports" ? [] : 0) : init[f]; }
    };
    Object.defineProperty(C, "name", { value: name });
    Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true });
    define(global, name, C);
  }
  // UIEvent::InitUIEvent and MouseEvent::InitMouseEvent (the legacy initializers of createEvent's
  // events): nothing while the event is being dispatched; the WebIDL long arguments ToInt32
  {
    const int32 = v => Number(v) | 0;
    define(global.UIEvent.prototype, "initUIEvent", function initUIEvent(type, bubbles = false, cancelable = false, view = null, detail = 0) {
      if (arguments.length < 1) throw new TypeError("UIEvent.initUIEvent: At least 1 argument required, but only 0 passed");
      if (this._path.length) return;
      this.initEvent(type, bubbles, cancelable);
      this.view = view;
      this.detail = int32(detail);
    });
    define(global.MouseEvent.prototype, "initMouseEvent", function initMouseEvent(type, bubbles = false, cancelable = false, view = null, detail = 0,
      screenX = 0, screenY = 0, clientX = 0, clientY = 0, ctrlKey = false, altKey = false, shiftKey = false, metaKey = false, button = 0, relatedTarget = null) {
      if (arguments.length < 1) throw new TypeError("MouseEvent.initMouseEvent: At least 1 argument required, but only 0 passed");
      if (this._path.length) return;
      global.UIEvent.prototype.initUIEvent.call(this, type, bubbles, cancelable, view, detail);
      Object.assign(this, { screenX: int32(screenX), screenY: int32(screenY), clientX: int32(clientX), clientY: int32(clientY),
        ctrlKey: !!ctrlKey, altKey: !!altKey, shiftKey: !!shiftKey, metaKey: !!metaKey, button: (Number(button) << 16) >> 16, relatedTarget });
    });
  }

  // window.postMessage and MessageChannel: delivered on a 0 ms timer (React's scheduler uses them)
  // window.postMessage is the C++ layer's __mrtPostMessage, which knows the calling window; here what
  // it then does (Window::PostMessageMoz): targetOrigin checked against this window when the message
  // is dispatched, the message cloned, a task of this window dispatching it with the caller's origin
  // and this window's view of the caller as its source
  if (!isWorker) define(global, "postMessage", global.__mrtPostMessage);
  global.__mrt.postMessageFrom = (caller, message, options = "/", transfer = []) => {
    let targetOrigin = typeof options === "string" ? options : (options && options.targetOrigin) || "/";
    const list = windowTransferList(options, transfer);
    const callerOrigin = caller.origin;
    if (targetOrigin === "/") targetOrigin = callerOrigin;
    else if (targetOrigin !== "*") {
      try { targetOrigin = new URL(targetOrigin).origin; } catch (e) { throw new DOMException("The string did not match the expected pattern.", "SyntaxError"); }
    }
    // the ports go with it to this window, on this thread
    const env = encodeMessage(message, list, "local");
    global.__mrt.queueTask(() => {
      if (targetOrigin === "*" || targetOrigin === global.origin) global.__mrt.receiveMessage(env, callerOrigin, caller);
    });
  };
  // window.postMessage's transfer list: (message, targetOrigin, transfer) or (message, {transfer})
  const windowTransferList = (options, transfer) =>
    typeof options === "string" ? transferList("Window.postMessage", transfer === undefined ? [] : transfer)
      : transferList("Window.postMessage", options && options.transfer !== undefined ? { transfer: options.transfer } : undefined);
  // ---- MessagePort and MessageChannel (GAPS B61, B82; Gecko's dom/messagechannel) ----
  //
  // A port has an id, unique in the process (the page's "p<N>", a worker's "<its tag>.<N>"), and the
  // id of its other end. Messages go by id: a port of this thread gets them in its queue; the other
  // end in another thread is reached through worker.rs, the page's thread being the hub: every port
  // that changes thread goes through it, so it knows where each one is (`portWhere`: a worker's id, or
  // "page"); a port it has not seen is still in the thread that made it, named by its id's prefix
  // (`workerOfTag`). As Gecko's port:
  // - it holds what comes to it until start() or setting onmessage (mMessageQueueEnabled), and then
  //   dispatches one message per task (PostMessageRunnable);
  // - a message is cloned when posted; the ports in its transfer list are sent with it
  //   (CloneAndDisentangle): the object is dead after that, and what it held goes with it, to a new
  //   port object made in the receiver's realm (TakeTransferredPortsAsSequence);
  // - close() ends both ends: nothing posted after it is delivered, what was queued still is.
  // A message for a port on its way to this thread (sent, not yet received) waits in `portPending`
  // until the port is made. Every window of the runtime is on the page's thread, so they share the
  // tables; each port's state keeps its own realm's functions. A port sent to a window in another
  // runtime is still dropped (B61).
  const portStates = share("portStates", () => new WeakMap());
  const portRegistry = share("ports", () => new Map());
  const portPending = share("portPending", () => new Map());
  const portWhere = share("portWhere", () => new Map());
  const workerOfTag = share("workerOfTag", () => new Map());
  // the page's: the worker port `id` is in, or undefined (it is here, or on its way here)
  const portWorker = id => {
    const where = portWhere.get(id);
    if (where !== undefined) return where === "page" ? undefined : where;
    const dot = id.indexOf(".");
    return dot < 0 ? undefined : workerOfTag.get(id.slice(0, dot));
  };
  // a worker's: the ids it sent out to the page; a message that comes for one goes back to the hub
  const portShippedOut = new Set();
  // a worker's way to the hub, set by the worker section (its `{port, m}` items)
  let portOut = null;
  // the page's way to a worker's thread (worker.rs's Post)
  const postToWorker = (workerId, item) => __mrtWorker(1, workerId, JSON.stringify(item), clockNow());
  let portPrefix = isWorker ? null : "p";
  const newPortId = () => {
    if (isWorker) return portPrefix + "." + (shared.nextPortId = (shared.nextPortId || 0) + 1);
    return "p" + (shared.nextPortId = (shared.nextPortId || 0) + 1);
  };
  const isPort = v => v !== null && typeof v === "object" && portStates.has(v);
  const livePort = id => { const st = portRegistry.get(id); return st && !st.shipped ? st : null; };
  // the ports an envelope ({d, t}) carries, also inside the messages those ports held
  const forEachCarriedPort = (env, fn) => {
    for (const rec of (env && env.t) || []) {
      fn(rec.id);
      for (const held of rec.held) forEachCarriedPort(held, fn);
    }
  };

  const MessagePort = function MessagePort() { throw new TypeError("Illegal constructor."); };
  Object.setPrototypeOf(MessagePort.prototype, EventTarget.prototype);
  // a port object of this realm for state `st` (its id and its other end's), registered
  const makePort = st => {
    const port = Object.create(MessagePort.prototype);
    Object.assign(st, { port, messages: st.messages || [], enabled: false, running: false, shipped: false, closed: false,
                        queueTask: fn => global.__mrt.queueTask(fn), queueAt: (fn, at) => global.__mrt.queueAt(fn, at),
                        deliver: msg => deliverPortMessage(port, msg) });
    portStates.set(port, st);
    portRegistry.set(st.id, st);
    // what came for it before it was here
    const waiting = portPending.get(st.id);
    if (waiting) { portPending.delete(st.id); for (const msg of waiting) enqueuePort(st, msg); }
    portShippedOut.delete(st.id);
    if (!isWorker) portWhere.set(st.id, "page");
    return port;
  };
  // the ports of a received envelope, made in this realm
  const receivePorts = env => ((env && env.t) || []).map(rec => makePort({ id: rec.id, other: rec.other, messages: rec.held.slice() }));
  // MessagePort::Dispatch: one message a task, once enabled
  const dispatchPort = st => {
    if (!st.enabled || st.running || st.shipped || st.messages.length === 0) return;
    st.running = true;
    st.queueTask(() => {
      st.running = false;
      if (st.shipped || st.messages.length === 0) return;
      st.deliver(st.messages.shift());
      dispatchPort(st);
    });
  };
  // a message or a close ({c}) for a port of this thread; one for a port sent on meanwhile follows it
  const enqueuePort = (st, msg) => {
    if (st.shipped) { routePort(st.id, msg, true, clockNow()); return; }
    if (msg.c) { st.closed = true; return; }
    if (st.closed) return;
    st.messages.push(msg);
    dispatchPort(st);
  };
  // the page sends an envelope to a worker: the ports it carries are that worker's now, and what
  // waited here for them follows
  const portsToWorker = (workerId, env) => forEachCarriedPort(env, id => {
    portWhere.set(id, workerId);
    const waiting = portPending.get(id);
    if (waiting) { portPending.delete(id); for (const msg of waiting) postToWorker(workerId, { port: id, m: msg }); }
  });
  // a worker sends an envelope out: the same, through the hub
  const portsOut = env => forEachCarriedPort(env, id => {
    portShippedOut.add(id);
    const waiting = portPending.get(id);
    if (waiting) { portPending.delete(id); for (const msg of waiting) portOut(id, msg); }
  });
  // the page receives an envelope (from a worker, or from a window or port of its own): its ports
  // are on their way here
  const portsArrived = env => forEachCarriedPort(env, id => portWhere.set(id, "page"));
  // A message for port `id` from this thread (`incoming`: one that came here from another thread,
  // at `at`): to the port if it is here, else to the worker that has it (the page) or to the hub (a
  // worker); one for a port on its way here waits for it.
  function routePort(id, msg, incoming, at) {
    const st = livePort(id);
    if (st) {
      if (incoming) st.queueAt(() => enqueuePort(st, msg), at);
      else enqueuePort(st, msg);
      return;
    }
    if (!isWorker) {
      const workerId = portWorker(id);
      if (workerId !== undefined) {
        portsToWorker(workerId, msg);
        postToWorker(workerId, { port: id, m: msg });
        return;
      }
    } else if (!incoming) {
      portsOut(msg);
      portOut(id, msg);
      return;
    } else if (portShippedOut.has(id)) {
      // back to the hub in a task of its own: what a worker hands over while it takes a message
      // (worker.rs's Post) waits for its next task
      global.__mrt.queueAt(() => { portsOut(msg); portOut(id, msg); }, at);
      return;
    }
    if (!isWorker) portsArrived(msg);
    let waiting = portPending.get(id);
    if (!waiting) portPending.set(id, (waiting = []));
    waiting.push(msg);
  }
  // where a message to port `id` goes from here: "local" (this thread), "out" (a worker's, to the
  // hub) or the id of the worker that has it (the page's)
  const portDestination = id => {
    if (livePort(id)) return "local";
    if (isWorker) return "out";
    const workerId = portWorker(id);
    return workerId === undefined ? "local" : workerId;
  };
  const portState = (port, member) => {
    const st = isPort(port) ? portStates.get(port) : undefined;
    if (!st) throw new TypeError(`'${member}' called on an object that does not implement interface MessagePort.`);
    return st;
  };

  // (message, transfer) or (message, {transfer}): the transfer list (GAPS B82). Its ArrayBuffers are
  // detached once the message is encoded (their bytes go in it), its ports sent with it; a duplicate
  // fails first, then anything else that cannot be transferred.
  // Intrinsic brand checks work across window realms and cannot be forged with toStringTag.
  const bufferByteLength = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, "byteLength").get;
  const transferBuffer = ArrayBuffer.prototype.transfer;
  const isTransferBuffer = value => {
    try { bufferByteLength.call(value); return true; } catch (_) { return false; }
  };
  const transferList = (what, arg) => {
    if (arg === undefined || arg === null) return [];
    if (typeof arg !== "object" && typeof arg !== "function") throw new TypeError(`${what}: Argument 2 is not valid for any of the 2-argument overloads.`);
    const list = typeof arg[Symbol.iterator] === "function" ? arg : arg.transfer;
    if (list === undefined) return [];
    if (typeof list !== "object" || typeof list[Symbol.iterator] !== "function") throw new TypeError(`${what}: Argument 2 can't be converted to a sequence.`);
    const items = Array.from(list);
    for (let i = 0; i < items.length; i++) {
      if (items.indexOf(items[i]) !== i) throw new DOMException("duplicate transferable for structured clone", "DataCloneError");
      if (!isTransferBuffer(items[i]) && !isPort(items[i])) throw new DOMException("The object could not be cloned.", "DataCloneError");
    }
    return items;
  };
  // The structured clone of a message with its transfer list (StructuredCloneHolder::Write): an
  // envelope {d, t}, `d` clone_codec.js's encoding, `t` a record per port sent ({id, other, held}).
  // `to` is where it goes (portDestination). A port sent or closed before cannot be (CanBeCloned);
  // one in the message and not in the list cannot be cloned. Gecko names the object it cannot clone
  // (JS_WriteStructuredClone's "X object could not be cloned.").
  const encodeMessage = (message, transfer, to) => {
    const ports = transfer.filter(isPort);
    for (const port of ports) {
      const st = portStates.get(port);
      if (st.shipped || st.closed) throw new DOMException("invalid transferable array for structured clone", "DataCloneError");
    }
    let failed;
    let data;
    const options = Object.assign({}, cloneOptions, { onFail: v => { failed = v; }, transferIndex: v => ports.indexOf(v) });
    try { data = CODEC.encode(message, options); } catch (e) {
      if (e !== CLONE_FAIL) throw e;
      // a symbol is not an object (JSMSG_SC_UNSUPPORTED_TYPE)
      if (typeof failed === "symbol") throw new DOMException("unsupported type for structured data", "DataCloneError");
      // its interface's name (a node's prototype has no toStringTag of its own here)
      const ctor = failed !== null && typeof failed === "object" && Object.getPrototypeOf(failed) && Object.getPrototypeOf(failed).constructor;
      const name = typeof failed === "function" ? "Function" : typeof ctor === "function" && ctor.name ? ctor.name : Object.prototype.toString.call(failed).slice(8, -1);
      throw new DOMException(`${name} object could not be cloned.`, "DataCloneError");
    }
    for (const buffer of transfer) if (isTransferBuffer(buffer)) transferBuffer.call(buffer);
    const env = { d: data, t: ports.map(port => {
      const st = portStates.get(port);
      st.shipped = true;
      return { id: st.id, other: st.other, held: st.messages.splice(0) };
    }) };
    if (to === "out") portsOut(env);
    else if (to !== "local") portsToWorker(to, env);
    return env;
  };
  // an envelope's value and ports, made in this realm; throws when it cannot be decoded
  const decodeEnvelope = env => {
    const ports = receivePorts(env);
    return { value: CODEC.decode(env.d, global, ports), ports: Object.freeze(ports) };
  };
  // a message from the other side, dispatched at `target` (MessageEventRunnable::DispatchDOMEvent,
  // PostMessageRunnable::DispatchMessage): a messageerror when it cannot be decoded
  const dispatchMessage = (target, env) => {
    let m;
    try { m = decodeEnvelope(env); } catch (e) {
      target.dispatchEvent(new MessageEvent("messageerror"));
      return;
    }
    target.dispatchEvent(new MessageEvent("message", { data: m.value, ports: m.ports }));
  };
  const deliverPortMessage = (port, msg) => dispatchMessage(port, msg);

  for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors({
    // MessagePort::PostMessage: the port itself may not be in the list; cloned now, then dropped if
    // this port is no longer entangled
    postMessage(message) {
      const st = portState(this, "postMessage");
      if (arguments.length < 1) throw new TypeError("MessagePort.postMessage: At least 1 argument required, but only 0 passed");
      const transfer = transferList("MessagePort.postMessage", arguments[1]);
      if (transfer.includes(this)) throw new DOMException("The object could not be cloned.", "DataCloneError");
      const env = encodeMessage(message, transfer, st.shipped || st.closed ? "local" : portDestination(st.other));
      if (st.shipped || st.closed) return;
      routePort(st.other, env, false);
    },
    start() {
      const st = portState(this, "start");
      if (st.enabled) return;
      st.enabled = true;
      dispatchPort(st);
    },
    close() {
      const st = portState(this, "close");
      if (st.shipped || st.closed) return;
      st.closed = true;
      routePort(st.other, { c: 1 }, false);
    },
  }))) Object.defineProperty(MessagePort.prototype, key, { ...desc, enumerable: true });
  global.__mrt.defineEventHandlers(MessagePort.prototype, ["onmessage", "onmessageerror"]);
  {
    // MessagePort::SetOnmessage: setting onmessage starts the port
    const desc = Object.getOwnPropertyDescriptor(MessagePort.prototype, "onmessage");
    Object.defineProperty(MessagePort.prototype, "onmessage", { ...desc, set(v) { desc.set.call(this, v); MessagePort.prototype.start.call(this); } });
  }
  // a pair of ports of this realm, entangled
  const portPair = () => {
    const a = newPortId(), b = newPortId();
    return [makePort({ id: a, other: b }), makePort({ id: b, other: a })];
  };
  const channelPorts = new WeakMap();
  const MessageChannel = function MessageChannel() {
    if (!new.target) throw new TypeError("MessageChannel constructor: 'new' is required");
    channelPorts.set(this, portPair());
  };
  for (const [i, name] of [[0, "port1"], [1, "port2"]]) {
    Object.defineProperty(MessageChannel.prototype, name, {
      get() {
        const ports = this !== null && typeof this === "object" ? channelPorts.get(this) : undefined;
        if (!ports) throw new TypeError(`'get ${name}' called on an object that does not implement interface MessageChannel.`);
        return ports[i];
      },
      configurable: true, enumerable: true,
    });
  }
  Object.defineProperty(MessagePort.prototype, Symbol.toStringTag, { value: "MessagePort", configurable: true });
  Object.defineProperty(MessageChannel.prototype, Symbol.toStringTag, { value: "MessageChannel", configurable: true });
  define(global, "MessagePort", MessagePort);
  define(global, "MessageChannel", MessageChannel);
  class BroadcastChannel { constructor(name) { this.name = name; } postMessage() {} close() {} }
  for (const m of ["addEventListener", "removeEventListener", "dispatchEvent"]) define(BroadcastChannel.prototype, m, EventTarget.prototype[m]);
  define(global, "BroadcastChannel", BroadcastChannel);

  // media queries against the window's viewport, a light theme, a mouse
  function evalMedia(q) {
    const { width, height } = viewportSize();
    q = String(q).toLowerCase().trim();
    if (!q || q === "all" || q === "screen") return true;
    return q.split(",").some(part => part.trim().split(/\s+and\s+/).every(t => {
      t = t.trim().replace(/^only\s+/, "");
      if (t === "screen" || t === "all") return true;
      if (t === "print") return false;
      let not = false;
      if (t.startsWith("not ")) { not = true; t = t.slice(4).trim(); }
      const m = /^\(\s*([a-z-]+)\s*(?::\s*([^)]+))?\)$/.exec(t);
      if (!m) return not;
      const [, f, v = ""] = m;
      const px = s => { const n = parseFloat(s); return /em$/.test(s) ? n * 16 : n; };
      let r;
      switch (f) {
        case "min-width": r = width >= px(v); break;
        case "max-width": r = width <= px(v); break;
        case "min-height": r = height >= px(v); break;
        case "max-height": r = height <= px(v); break;
        // portrait when the height is at least the width (Gecko's nsMediaFeatures GetOrientation)
        case "orientation": r = v.trim() === (height >= width ? "portrait" : "landscape"); break;
        case "prefers-color-scheme": r = v.trim() === "light"; break;
        case "prefers-reduced-motion": r = v.trim() === "no-preference"; break;
        case "hover": r = v.trim() === "hover"; break;
        case "any-hover": r = v.trim() === "hover"; break;
        case "pointer": case "any-pointer": r = v.trim() === "fine"; break;
        case "min-resolution": case "-webkit-min-device-pixel-ratio": case "min--moz-device-pixel-ratio": r = parseFloat(v) <= 1; break;
        case "display-mode": r = v.trim() === "browser"; break;
        // the page's scripts run (Gecko_MediaFeatures_Scripting), as in Stylo's (vendor/stylo)
        case "scripting": r = v === "" || v.trim() === "enabled"; break;
        default: r = false;
      }
      return not ? !r : r;
    }));
  }
  define(global, "matchMedia", q => {
    const mql = { media: String(q), matches: evalMedia(q), onchange: null, addListener() {}, removeListener() {} };
    for (const m of ["addEventListener", "removeEventListener", "dispatchEvent"]) mql[m] = () => {};
    return mql;
  });
  // ---- form constraint validation (dom/html/ConstraintValidation.cpp), reduced ----
  //
  // A custom error (setCustomValidity) and `required` with an empty value; the type, pattern,
  // length, range and step checks are not made (GAPS B27). Values are the value attribute (B20).
  class ValidityState {
    constructor(el) { define(this, "_el", el); }
    get customError() { return !!customValidity.get(this._el); }
    get valueMissing() { return valueMissing(this._el); }
    get typeMismatch() { return false; }
    get patternMismatch() { return false; }
    get tooLong() { return false; }
    get tooShort() { return false; }
    get rangeUnderflow() { return false; }
    get rangeOverflow() { return false; }
    get stepMismatch() { return false; }
    get badInput() { return false; }
    get valid() { return !this.customError && !this.valueMissing; }
  }
  define(global, "ValidityState", ValidityState);
  const customValidity = share("customValidity", () => new WeakMap());
  const BARRED_INPUT_TYPES = new Set(["hidden", "button", "reset", "image"]);
  const typeOf = el => (el.getAttribute("type") || "").toLowerCase();
  // IsBarredFromConstraintValidation and willValidate
  function willValidate(el) {
    const tag = el.localName;
    if (tag === "fieldset" || tag === "output" || tag === "object") return false;
    if (tag === "button" && ["reset", "button"].includes(typeOf(el))) return false;
    if (tag === "input" && BARRED_INPUT_TYPES.has(typeOf(el))) return false;
    if ((tag === "input" || tag === "textarea") && el.hasAttribute("readonly")) return false;
    for (let n = el; n && n.nodeType === 1; n = n.parentNode) {
      if (n.hasAttribute("disabled") && n.localName === "fieldset" || n === el && n.hasAttribute("disabled")) return false;
      if (n.localName === "datalist") return false;
    }
    return true;
  }
  function valueMissing(el) {
    if (!el.hasAttribute("required") || !willValidate(el)) return false;
    if (el.localName === "input" && ["checkbox", "radio"].includes(typeOf(el))) return !el.hasAttribute("checked");
    return (el.localName === "textarea" ? el.textContent : el.getAttribute("value") || "") === "";
  }
  // HTMLTextAreaElement's value (Gecko's TextControlState): its default value is its text; its value
  // is the default value until script sets one (the dirty flag), line breaks normalized to LF
  if (!isWorker) {
    const textareaValues = new WeakMap();
    const normalizeLines = v => v.replace(/\r\n?/g, "\n");
    getter(HTMLTextAreaElement.prototype, "defaultValue", function () { return this.textContent; }, ceReactions(function (v) { this.textContent = v === null ? "" : String(v); }));
    getter(HTMLTextAreaElement.prototype, "value", function () {
      return textareaValues.has(this) ? textareaValues.get(this) : normalizeLines(this.textContent);
    }, function (v) { textareaValues.set(this, normalizeLines(v === null ? "" : String(v))); });
    getter(HTMLTextAreaElement.prototype, "textLength", function () { return this.value.length; });
  }
  // HTMLInputElement's value and checkedness (Gecko's HTMLInputElement: GetValueMode, the dirty
  // value and mCheckedChanged): a text-like input keeps the value set by script or typed by the
  // owner (realsite --interactive), else its value attribute; checkbox and radio return the
  // attribute or "on", and set the attribute, as the default-value types do. Checked is the
  // attribute until set; checking a radio unchecks the others of its group (its name, in the same
  // form and tree). The sanitization is Gecko's for the text types: line breaks dropped, and for url
  // and email the ends trimmed too.
  if (!isWorker) {
    const inputValues = new WeakMap(), inputChecked = new WeakMap();
    const DEFAULT_MODE = new Set(["hidden", "submit", "image", "reset", "button"]);
    // the value-mode types whose sanitization is not the text types' (not done here; an unknown
    // type is text)
    const OTHER_SANITIZED = new Set(["number", "range", "color", "date", "time", "datetime-local", "month", "week"]);
    const valueMode = el => {
      const t = typeOf(el);
      return t === "checkbox" || t === "radio" ? "default/on" : DEFAULT_MODE.has(t) ? "default" : t === "file" ? "filename" : "value";
    };
    const sanitize = (el, v) => {
      const t = typeOf(el);
      if (!OTHER_SANITIZED.has(t)) v = v.replace(/[\r\n]/g, "");
      return t === "url" || t === "email" ? v.replace(/^[\t\n\f\r ]+|[\t\n\f\r ]+$/g, "") : v;
    };
    getter(HTMLInputElement.prototype, "defaultValue", function () { return this.getAttribute("value") ?? ""; }, function (v) { this.setAttribute("value", String(v)); });
    getter(HTMLInputElement.prototype, "value", function () {
      const mode = valueMode(this);
      if (mode === "value") return inputValues.has(this) ? inputValues.get(this) : sanitize(this, this.getAttribute("value") ?? "");
      if (mode === "default/on") return this.getAttribute("value") ?? "on";
      return mode === "filename" ? "" : this.getAttribute("value") ?? "";
    }, function (v) {
      v = v === null ? "" : String(v);
      const mode = valueMode(this);
      if (mode === "value") inputValues.set(this, sanitize(this, v));
      else if (mode === "filename") { if (v !== "") throw new DOMException("An attempt was made to use an object that is not, or is no longer, usable", "InvalidStateError"); }
      else this.setAttribute("value", v);
    });
    getter(HTMLInputElement.prototype, "defaultChecked", function () { return this.hasAttribute("checked"); },
      function (v) { if (v) this.setAttribute("checked", ""); else this.removeAttribute("checked"); });
    getter(HTMLInputElement.prototype, "checked", function () {
      return inputChecked.has(this) ? inputChecked.get(this) : this.hasAttribute("checked");
    }, function (v) {
      inputChecked.set(this, !!v);
      const name = this.getAttribute("name");
      if (!v || typeOf(this) !== "radio" || !name) return;
      const form = this.closest("form"), root = this.getRootNode();
      for (const other of root.querySelectorAll("input")) {
        if (other !== this && typeOf(other) === "radio" && other.getAttribute("name") === name && other.closest("form") === form) inputChecked.set(other, false);
      }
    });
  }
  if (!isWorker) for (const name of ["HTMLInputElement", "HTMLSelectElement", "HTMLTextAreaElement", "HTMLButtonElement", "HTMLFieldSetElement",
    "HTMLOutputElement", "HTMLObjectElement"]) {
    const proto = global[name].prototype;
    define(proto, "setCustomValidity", function (message) { customValidity.set(this, String(message)); });
    getter(proto, "validity", function () { return new ValidityState(this); });
    getter(proto, "willValidate", function () { return willValidate(this); });
    getter(proto, "validationMessage", function () {
      if (!willValidate(this)) return "";
      if (customValidity.get(this)) return customValidity.get(this);
      return valueMissing(this) ? "Please fill out this field." : "";
    });
    const check = function () {
      if (!willValidate(this) || new ValidityState(this).valid) return true;
      this.dispatchEvent(new Event("invalid", { cancelable: true }));
      return false;
    };
    define(proto, "checkValidity", check);
    define(proto, "reportValidity", check);
  }

  // ---- text tracks: the cue classes and tracks as data (dom/media/webvtt); nothing plays ----

  const cueToken = Symbol("cue");
  class TextTrackCue extends EventTarget {
    constructor(token, startTime, endTime) {
      super();
      if (token !== cueToken) throw new TypeError("Illegal constructor.");
      Object.assign(this, { id: "", track: null, startTime: Number(startTime), endTime: Number(endTime), pauseOnExit: false, onenter: null, onexit: null });
    }
  }
  class VTTCue extends TextTrackCue {
    constructor(startTime, endTime, text) {
      super(cueToken, startTime, endTime);
      Object.assign(this, { region: null, vertical: "", snapToLines: true, line: "auto", lineAlign: "start", position: "auto",
        positionAlign: "auto", size: 100, align: "center", text: String(text) });
    }
    getCueAsHTML() { const f = document.createDocumentFragment(); f.appendChild(document.createTextNode(this.text)); return f; }
  }
  // TextTrack, TextTrackList and TextTrackCueList (dom/media/webvtt TextTrack.cpp, TextTrackList.cpp,
  // TextTrackCueList.cpp): a media element's textTracks, the tracks addTextTrack makes and their
  // cues, kept here as data (task 753cc: X's player walks textTracks for its id3 metadata track). No
  // cue becomes active and the tab shows none; <track> elements make no track (GAPS B35).
  const trackToken = Symbol("track");
  class TextTrackCueList {
    constructor(token) { if (token !== trackToken) throw new TypeError("Illegal constructor."); this._cues = []; }
    get length() { return this._cues.length; }
    getCueById(id) { return id === "" ? null : this._cues.find(c => c.id === String(id)) || null; }
    _sync() { for (let i = 0; i <= this._cues.length; i++) { if (i < this._cues.length) this[i] = this._cues[i]; else delete this[i]; } }
    [Symbol.iterator]() { return this._cues[Symbol.iterator](); }
  }
  const TEXT_TRACK_KINDS = ["subtitles", "captions", "descriptions", "chapters", "metadata"];
  class TextTrack extends EventTarget {
    constructor(token, kind, label, language) {
      super();
      if (token !== trackToken) throw new TypeError("Illegal constructor.");
      this._kind = kind; this._label = label; this._language = language; this._mode = "disabled";
      this._cues = new TextTrackCueList(trackToken); this._active = new TextTrackCueList(trackToken);
      this.oncuechange = null;
    }
    get kind() { return this._kind; }
    get label() { return this._label; }
    get language() { return this._language; }
    get id() { return ""; }
    get inBandMetadataTrackDispatchType() { return ""; }
    get mode() { return this._mode; }
    set mode(v) { v = String(v); if (["disabled", "hidden", "showing"].includes(v)) this._mode = v; }
    // (null while the track is disabled, as Gecko's GetCues)
    get cues() { return this._mode === "disabled" ? null : this._cues; }
    get activeCues() { return this._mode === "disabled" ? null : this._active; }
    addCue(cue) {
      if (!(cue instanceof TextTrackCue)) throw new TypeError("TextTrack.addCue: Argument 1 does not implement interface TextTrackCue.");
      if (cue.track && cue.track !== this) cue.track.removeCue(cue);
      if (!this._cues._cues.includes(cue)) {
        // in start time order, then end time order (descending), then the order added
        const list = this._cues._cues;
        let i = list.length;
        while (i > 0 && (list[i - 1].startTime > cue.startTime || (list[i - 1].startTime === cue.startTime && list[i - 1].endTime < cue.endTime))) i--;
        list.splice(i, 0, cue);
        this._cues._sync();
      }
      cue.track = this;
    }
    removeCue(cue) {
      const list = this._cues._cues, i = list.indexOf(cue);
      if (i < 0) throw new DOMException("The object can not be found here.", "NotFoundError");
      list.splice(i, 1);
      this._cues._sync();
      cue.track = null;
    }
  }
  class TextTrackList extends EventTarget {
    constructor(token) {
      super();
      if (token !== trackToken) throw new TypeError("Illegal constructor.");
      this._tracks = [];
      this.onchange = this.onaddtrack = this.onremovetrack = null;
    }
    get length() { return this._tracks.length; }
    getTrackById(id) { return id === "" ? null : this._tracks.find(t => t.id === String(id)) || null; }
    // AddTextTrack: the track, then addtrack (a TrackEvent) as a task
    _add(track) {
      this[this._tracks.length] = track;
      this._tracks.push(track);
      global.__mrt.queueTask(() => {
        const e = new Event("addtrack");
        Object.defineProperty(e, "track", { value: track, enumerable: true });
        this.dispatchEvent(e);
      });
    }
    [Symbol.iterator]() { return this._tracks[Symbol.iterator](); }
  }
  for (const c of [TextTrackCue, VTTCue, TextTrack, TextTrackList, TextTrackCueList]) define(global, c.name, c);
  if (typeof HTMLMediaElement !== "undefined") {
    Object.defineProperty(HTMLMediaElement.prototype, "textTracks", {
      configurable: true, enumerable: true,
      get() { return this._textTracks || (this._textTracks = new TextTrackList(trackToken)); },
    });
    define(HTMLMediaElement.prototype, "addTextTrack", function addTextTrack(kind, label = "", language = "") {
      if (arguments.length < 1) throw new TypeError("HTMLMediaElement.addTextTrack: At least 1 argument required, but only 0 passed");
      kind = String(kind);
      if (!TEXT_TRACK_KINDS.includes(kind)) throw new TypeError(`HTMLMediaElement.addTextTrack: '${kind}' (value of argument 1) is not a valid value for enumeration TextTrackKind.`);
      const track = new TextTrack(trackToken, kind, String(label), String(language));
      // (a track made by script starts hidden)
      track._mode = "hidden";
      this.textTracks._add(track);
      return track;
    });
  }

  // elementFromPoint and elementsFromPoint (DocumentOrShadowRoot.webidl): the bindings' checks, then
  // __mrtNodesFromPoint (Host::nodes_from_point: QueryNodesFromPoint over the layout's hit test,
  // PresShellFrames::NodesFromPoint). The coordinates are WebIDL floats.
  if (!isWorker) for (const [proto, iface] of [[Document.prototype, "Document"], [ShadowRoot.prototype, "ShadowRoot"]]) {
    const Iface = iface === "Document" ? Document : ShadowRoot;
    const args = (self, method, a) => {
      if (!(self instanceof Iface)) throw new TypeError(`'${method}' called on an object that does not implement interface ${iface}.`);
      if (a.length < 2) throw new TypeError(`${iface}.${method}: At least 2 arguments required, but only ${a.length} passed`);
      return [0, 1].map(i => {
        const n = +a[i];
        if (!Number.isFinite(n)) throw new TypeError(`${iface}.${method}: Argument ${i + 1} is not a finite floating-point value.`);
        return Math.fround(n);
      });
    };
    define(proto, "elementFromPoint", function elementFromPoint(x, y) {
      const [fx, fy] = args(this, "elementFromPoint", arguments);
      return __mrtNodesFromPoint(this, fx, fy, false)[0] || null;
    });
    define(proto, "elementsFromPoint", function elementsFromPoint(x, y) {
      const [fx, fy] = args(this, "elementsFromPoint", arguments);
      return __mrtNodesFromPoint(this, fx, fy, true);
    });
  }

  // MutationObserver is native since session 46 (glue/mrt_js.cpp, nsDOMMutationObserver)

  // NodeFilter, TreeWalker and NodeIterator: the DOM spec's traversal algorithms over the tree links.
  // A NodeIterator is not moved when the nodes around it are removed (its pre-removing steps).
  const NodeFilter = { FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3, SHOW_ALL: 0xFFFFFFFF, SHOW_ELEMENT: 0x1,
    SHOW_ATTRIBUTE: 0x2, SHOW_TEXT: 0x4, SHOW_CDATA_SECTION: 0x8, SHOW_ENTITY_REFERENCE: 0x10, SHOW_ENTITY: 0x20,
    SHOW_PROCESSING_INSTRUCTION: 0x40, SHOW_COMMENT: 0x80, SHOW_DOCUMENT: 0x100, SHOW_DOCUMENT_TYPE: 0x200,
    SHOW_DOCUMENT_FRAGMENT: 0x400, SHOW_NOTATION: 0x800 };
  define(global, "NodeFilter", Object.freeze(NodeFilter));
  // "filter": whatToShow first, then the filter (a function or an object with acceptNode)
  function filterNode(it, node) {
    if (it._active) throw new DOMException("The filter is already running", "InvalidStateError");
    if (!(it.whatToShow & (1 << (node.nodeType - 1)))) return 3;
    const f = it.filter;
    if (f === null || f === undefined) return 1;
    it._active = true;
    try { return Number(typeof f === "function" ? f.call(undefined, node) : f.acceptNode(node)); }
    finally { it._active = false; }
  }
  class TreeWalker {
    constructor(root, whatToShow, filter) { this.root = root; this.whatToShow = whatToShow >>> 0; this.filter = filter; this.currentNode = root; this._active = false; }
    parentNode() {
      let node = this.currentNode;
      while (node !== null && node !== this.root) {
        node = node.parentNode;
        if (node !== null && filterNode(this, node) === 1) { this.currentNode = node; return node; }
      }
      return null;
    }
    _traverseChildren(first) {
      let node = first ? this.currentNode.firstChild : this.currentNode.lastChild;
      while (node !== null) {
        const result = filterNode(this, node);
        if (result === 1) { this.currentNode = node; return node; }
        if (result === 3) {
          const child = first ? node.firstChild : node.lastChild;
          if (child !== null) { node = child; continue; }
        }
        for (;;) {
          const sibling = first ? node.nextSibling : node.previousSibling;
          if (sibling !== null) { node = sibling; break; }
          const parent = node.parentNode;
          if (parent === null || parent === this.root || parent === this.currentNode) return null;
          node = parent;
        }
      }
      return null;
    }
    firstChild() { return this._traverseChildren(true); }
    lastChild() { return this._traverseChildren(false); }
    _traverseSiblings(next) {
      let node = this.currentNode;
      if (node === this.root) return null;
      for (;;) {
        let sibling = next ? node.nextSibling : node.previousSibling;
        while (sibling !== null) {
          node = sibling;
          const result = filterNode(this, node);
          if (result === 1) { this.currentNode = node; return node; }
          sibling = next ? node.firstChild : node.lastChild;
          if (result === 2 || sibling === null) sibling = next ? node.nextSibling : node.previousSibling;
        }
        node = node.parentNode;
        if (node === null || node === this.root) return null;
        if (filterNode(this, node) === 1) return null;
      }
    }
    nextSibling() { return this._traverseSiblings(true); }
    previousSibling() { return this._traverseSiblings(false); }
    previousNode() {
      let node = this.currentNode;
      while (node !== this.root) {
        let sibling = node.previousSibling;
        while (sibling !== null) {
          node = sibling;
          let result = filterNode(this, node);
          while (result !== 2 && node.lastChild !== null) { node = node.lastChild; result = filterNode(this, node); }
          if (result === 1) { this.currentNode = node; return node; }
          sibling = node.previousSibling;
        }
        if (node === this.root || node.parentNode === null) return null;
        node = node.parentNode;
        if (filterNode(this, node) === 1) { this.currentNode = node; return node; }
      }
      return null;
    }
    nextNode() {
      let node = this.currentNode, result = 1;
      for (;;) {
        while (result !== 2 && node.firstChild !== null) {
          node = node.firstChild;
          result = filterNode(this, node);
          if (result === 1) { this.currentNode = node; return node; }
        }
        let sibling = null;
        for (let temporary = node; temporary !== null; temporary = temporary.parentNode) {
          if (temporary === this.root) return null;
          sibling = temporary.nextSibling;
          if (sibling !== null) { node = sibling; break; }
        }
        if (sibling === null) return null;
        result = filterNode(this, node);
        if (result === 1) { this.currentNode = node; return node; }
      }
    }
  }
  // the node after and before `node` in tree order, within `root`
  const following = (node, root) => {
    if (node.firstChild) return node.firstChild;
    for (; node; node = node.parentNode) {
      if (node === root) return null;
      if (node.nextSibling) return node.nextSibling;
    }
    return null;
  };
  const preceding = (node, root) => {
    if (node === root) return null;
    let p = node.previousSibling;
    if (!p) return node.parentNode;
    while (p.lastChild) p = p.lastChild;
    return p;
  };
  class NodeIterator {
    constructor(root, whatToShow, filter) {
      this.root = root; this.whatToShow = whatToShow >>> 0; this.filter = filter;
      this.referenceNode = root; this.pointerBeforeReferenceNode = true; this._active = false;
    }
    _traverse(next) {
      let node = this.referenceNode, before = this.pointerBeforeReferenceNode;
      for (;;) {
        if (next) {
          if (!before) { node = following(node, this.root); if (node === null) return null; } else before = false;
        } else {
          if (before) { node = preceding(node, this.root); if (node === null) return null; } else before = true;
        }
        if (filterNode(this, node) === 1) break;
      }
      this.referenceNode = node;
      this.pointerBeforeReferenceNode = before;
      return node;
    }
    nextNode() { return this._traverse(true); }
    previousNode() { return this._traverse(false); }
    detach() {}
  }
  const checkRoot = root => { if (!(root instanceof Node)) throw new TypeError("Argument 1 is not an object implementing Node."); };
  if (!isWorker) define(Document.prototype, "createTreeWalker", function (root, whatToShow = 0xFFFFFFFF, filter = null) { checkRoot(root); return new TreeWalker(root, whatToShow, filter); });
  if (!isWorker) define(Document.prototype, "createNodeIterator", function (root, whatToShow = 0xFFFFFFFF, filter = null) { checkRoot(root); return new NodeIterator(root, whatToShow, filter); });
  define(global, "TreeWalker", TreeWalker);
  define(global, "NodeIterator", NodeIterator);

  // TextMetrics (CanvasRenderingContext2D.webidl), its members in Gecko's order, all of them on in
  // release Firefox: measureText's answer is the stub's guess (a 6 px advance a character, 8 px up and
  // 2 down; 0 for the rest). Google Maps draws with WebGL only when TextMetrics has the
  // actualBoundingBox members (task 23bb)
  const TextMetrics = { TextMetrics: function () { throw new TypeError("Illegal constructor."); } }.TextMetrics;
  const textMetrics = new WeakMap();
  for (const m of ["width", "actualBoundingBoxLeft", "actualBoundingBoxRight", "fontBoundingBoxAscent", "fontBoundingBoxDescent",
                   "actualBoundingBoxAscent", "actualBoundingBoxDescent", "emHeightAscent", "emHeightDescent", "hangingBaseline",
                   "alphabeticBaseline", "ideographicBaseline"]) {
    getter(TextMetrics.prototype, m, function () {
      const d = textMetrics.get(this);
      if (!d) throw new TypeError(`'get ${m}' called on an object that does not implement interface TextMetrics.`);
      return d[m] || 0;
    });
  }
  Object.defineProperty(TextMetrics.prototype, Symbol.toStringTag, { value: "TextMetrics", configurable: true });
  define(global, "TextMetrics", TextMetrics);
  const makeTextMetrics = values => { const o = Object.create(TextMetrics.prototype); textMetrics.set(o, values); return o; };
  // <canvas>: a 2D context that draws nothing and reads back transparent pixels (counted; forwarding
  // it is phase 4's other half, not built), and WebGL forwarded to the bridge tab (webgl.js, task 23bb)
  class CanvasRenderingContext2D {
    constructor(canvas) {
      this.canvas = canvas;
      Object.assign(this, { fillStyle: "#000000", strokeStyle: "#000000", font: "10px sans-serif", globalAlpha: 1, lineWidth: 1,
                            textAlign: "start", textBaseline: "alphabetic", globalCompositeOperation: "source-over" });
    }
    getImageData(x, y, w, h) { return { width: w, height: h, data: new Uint8ClampedArray(Math.max(0, w * h * 4)) }; }
    createImageData(w, h) { return this.getImageData(0, 0, w, h); }
    measureText(t) { return makeTextMetrics({ width: String(t).length * 6, actualBoundingBoxAscent: 8, actualBoundingBoxDescent: 2 }); }
    getTransform() { return { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }; }
    createLinearGradient() { return { addColorStop() {} }; }
    createRadialGradient() { return { addColorStop() {} }; }
    createPattern() { return null; }
    isPointInPath() { return false; }
    getLineDash() { return []; }
  }
  for (const m of ["fillRect", "strokeRect", "clearRect", "fillText", "strokeText", "beginPath", "closePath", "moveTo", "lineTo",
                   "arc", "arcTo", "bezierCurveTo", "quadraticCurveTo", "rect", "fill", "stroke", "clip", "save", "restore",
                   "scale", "rotate", "translate", "transform", "setTransform", "resetTransform", "drawImage", "putImageData",
                   "setLineDash", "ellipse"]) {
    CanvasRenderingContext2D.prototype[m] = function () {};
  }
  define(global, "CanvasRenderingContext2D", CanvasRenderingContext2D);
  const contexts = share("contexts", () => new WeakMap());
  // @WEBGL@
  // A canvas has one context, of the type first asked for: the same one again for that type, null
  // for another (CanvasRenderingContextHelper::GetOrCreateContext); WebGL's are forwarded (webgl.js)
  if (!isWorker) define(HTMLCanvasElement.prototype, "getContext", function (type, attrs) {
    if (arguments.length < 1) throw new TypeError("HTMLCanvasElement.getContext: At least 1 argument required, but only 0 passed");
    type = String(type);
    const gl = type === "webgl" || type === "experimental-webgl" || type === "webgl2";
    if (type !== "2d" && !gl) { stub("HTMLCanvasElement.getContext(other)"); return null; }
    let ctx = contexts.get(this);
    if (ctx) {
      const kind = global.__mrt.webglKind(ctx);
      return (type === "2d" ? kind === 0 : kind === (type === "webgl2" ? 2 : 1)) ? ctx : null;
    }
    if (gl) {
      ctx = global.__mrt.webglContext(this, type, attrs);
      if (ctx) contexts.set(this, ctx);
      return ctx;
    }
    stub("HTMLCanvasElement.getContext(2d)");
    contexts.set(this, ctx = new CanvasRenderingContext2D(this));
    return ctx;
  });
  // width and height: unsigned long, 300 by 150 when absent or invalid (HTMLCanvasElement.webidl)
  if (!isWorker) for (const [name, fallback] of [["width", 300], ["height", 150]]) {
    getter(HTMLCanvasElement.prototype, name, function () {
      const m = /^[\t\n\f\r ]*\+?(\d+)/.exec(this.getAttribute(name) || "");
      const v = m ? Number(m[1]) : NaN;
      return Number.isFinite(v) && v <= 0x7fffffff ? v : fallback;
    }, function (v) {
      const n = Number(v) >>> 0;
      this.setAttribute(name, String(n > 0x7fffffff ? fallback : n));
    });
  }
  if (!isWorker) define(HTMLCanvasElement.prototype, "toDataURL", function (...args) {
    const ctx = contexts.get(this);
    return ctx && global.__mrt.webglKind(ctx) ? global.__mrt.webglDataURL(ctx, args) : "data:,";
  });
  if (!isWorker) define(HTMLCanvasElement.prototype, "toBlob", function (cb) { global.__mrt.queueTask(() => global.__mrt.callPage(cb, undefined, [null])); });

  // namespaced attributes: kept under their qualified name, the namespace dropped (an approximation,
  // counted: the arena DOM's attributes have a namespace, the bindings do not set it yet)
  const localOf = q => String(q).replace(/^.*:/, "");
  const attrNamed = (el, local) => el.getAttributeNames().find(a => localOf(a) === String(local));
  if (!isWorker) define(Element.prototype, "setAttributeNS", function (ns, qname, value) { stub("Element.setAttributeNS"); this.setAttribute(String(qname), value); });
  if (!isWorker) define(Element.prototype, "getAttributeNS", function (ns, local) { const n = attrNamed(this, local); return n === undefined ? null : this.getAttribute(n); });
  if (!isWorker) define(Element.prototype, "hasAttributeNS", function (ns, local) { return attrNamed(this, local) !== undefined; });
  if (!isWorker) define(Element.prototype, "removeAttributeNS", function (ns, local) { const n = attrNamed(this, local); if (n !== undefined) this.removeAttribute(n); });

  // ---- DOMRectReadOnly, DOMRect (Gecko's dom/base/DOMRect.cpp) ----
  //
  // What a ResizeObserverEntry's contentRect is. The edges are the min and max of the origin and the
  // far side (a negative size gives the edges the other way round); NaN in either gives NaN.
  // getBoundingClientRect still gives a plain object (Box in mrt_js.cpp).
  const rectStates = share("domRects", () => new WeakMap());
  const rectOf = (self, iface, member) => {
    const st = self !== null && typeof self === "object" ? rectStates.get(self) : undefined;
    if (!st || (iface === "DOMRect" && !st.mutable)) throw new TypeError(`'${member}' called on an object that does not implement interface ${iface}.`);
    return st;
  };
  const nanSafe = (pick, a, b) => (Number.isNaN(a) || Number.isNaN(b) ? NaN : pick(a, b));
  const makeRect = (proto, mutable, x, y, width, height) => {
    const r = Object.create(proto);
    rectStates.set(r, { mutable, x: +x, y: +y, width: +width, height: +height });
    return r;
  };
  // the constructor's and fromRect's unrestricted doubles (DOMRectInit's members read in order)
  const rectArgs = args => [0, 1, 2, 3].map(i => (args[i] === undefined ? 0 : Number(args[i])));
  const rectInit = (o, name) => {
    if (o !== undefined && o !== null && typeof o !== "object" && typeof o !== "function") throw new TypeError(`${name}.fromRect: Argument 1 can't be converted to a dictionary.`);
    const v = k => (o === undefined || o === null || o[k] === undefined ? 0 : Number(o[k]));
    const height = v("height"), width = v("width"), x = v("x"), y = v("y");
    return [x, y, width, height];
  };
  const DOMRectReadOnly = function DOMRectReadOnly(x, y, width, height) {
    if (new.target === undefined) throw new TypeError("DOMRectReadOnly constructor: 'new' is required");
    const proto = new.target.prototype;
    return makeRect(proto !== null && typeof proto === "object" ? proto : DOMRectReadOnly.prototype, new.target !== DOMRectReadOnly, ...rectArgs(arguments));
  };
  const DOMRect = function DOMRect(x, y, width, height) {
    if (new.target === undefined) throw new TypeError("DOMRect constructor: 'new' is required");
    const proto = new.target.prototype;
    return makeRect(proto !== null && typeof proto === "object" ? proto : DOMRect.prototype, true, ...rectArgs(arguments));
  };
  Object.setPrototypeOf(DOMRect, DOMRectReadOnly);
  DOMRectReadOnly.prototype = {};
  DOMRect.prototype = Object.create(DOMRectReadOnly.prototype);
  {
    const ro = DOMRectReadOnly.prototype, rw = DOMRect.prototype;
    Object.defineProperty(DOMRectReadOnly, "fromRect", { value: { fromRect(other) { return makeRect(DOMRectReadOnly.prototype, false, ...rectInit(other, "DOMRectReadOnly")); } }.fromRect, writable: true, enumerable: true, configurable: true });
    Object.defineProperty(DOMRect, "fromRect", { value: { fromRect(other) { return makeRect(DOMRect.prototype, true, ...rectInit(other, "DOMRect")); } }.fromRect, writable: true, enumerable: true, configurable: true });
    // the operations, then the attributes, as Gecko's bindings define them
    Object.defineProperty(ro, "toJSON", { value: { toJSON() {
      rectOf(this, "DOMRectReadOnly", "toJSON");
      const out = {};
      for (const k of ["x", "y", "width", "height", "top", "right", "bottom", "left"]) out[k] = this[k];
      return out;
    } }.toJSON, writable: true, enumerable: true, configurable: true });
    const edges = {
      x: st => st.x, y: st => st.y, width: st => st.width, height: st => st.height,
      top: st => nanSafe(Math.min, st.y, st.y + st.height), right: st => nanSafe(Math.max, st.x, st.x + st.width),
      bottom: st => nanSafe(Math.max, st.y, st.y + st.height), left: st => nanSafe(Math.min, st.x, st.x + st.width),
    };
    for (const [k, read] of Object.entries(edges)) {
      const get = Object.getOwnPropertyDescriptor({ get [k]() { return read(rectOf(this, "DOMRectReadOnly", `get ${k}`)); } }, k).get;
      Object.defineProperty(ro, k, { get, enumerable: true, configurable: true });
    }
    for (const k of ["x", "y", "width", "height"]) {
      const d = Object.getOwnPropertyDescriptor({
        get [k]() { return rectOf(this, "DOMRect", `get ${k}`)[k]; },
        set [k](v) { rectOf(this, "DOMRect", `set ${k}`)[k] = Number(v); },
      }, k);
      Object.defineProperty(rw, k, { get: d.get, set: d.set, enumerable: true, configurable: true });
    }
    for (const [C, name] of [[DOMRectReadOnly, "DOMRectReadOnly"], [DOMRect, "DOMRect"]]) {
      Object.defineProperty(C.prototype, "constructor", { value: C, writable: true, enumerable: false, configurable: true });
      Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true });
      Object.defineProperty(C, "prototype", { writable: false, enumerable: false, configurable: false });
    }
  }
  define(global, "DOMRectReadOnly", DOMRectReadOnly);
  define(global, "DOMRect", DOMRect);

  // ---- ResizeObserver (Gecko's dom/base/ResizeObserver.cpp) ----
  //
  // An observer keeps its observations in order (mObservationList), each with its target, its box and
  // the size last reported (-1 x -1 at first, so the first look always reports). The document keeps
  // its observers from their first observe() until none is left (Document::AddResizeObserver); the
  // refresh tick (prelude.js) runs notify(), Gecko's
  // Document::DetermineProximityToViewportAndNotifyResizeObservers: gather the observations whose size
  // changed and whose target is deeper than the depth reached, call each observer with its entries,
  // and go round again from the shallowest target reported, until nothing changed; an observation
  // left out by the depth makes the loop-limit error at the window and another tick. observe()
  // schedules a tick (ScheduleResizeObserversNotification); so does any change of the DOM while
  // observers are registered (Gecko's refresh driver runs its layout phase after one).
  // Sizes come from __mrtResizeBoxes (PresShellFrames::ResizeObserverBoxes): per element, [has a
  // frame, border-box, content-box and device-pixel content-box sizes (inline, block), contentRect].
  // Not done: content-visibility (layout ignores it, GAPS B77); SVG elements' bounding boxes (no SVG
  // layout, C10); an exception in the callback goes to the console (GAPS B31).
  if (!isWorker) {
    const BOXES = ["content-box", "border-box", "device-pixel-content-box"];
    const BOX_OFFSET = { "border-box": 1, "content-box": 3, "device-pixel-content-box": 5 };
    const BOX_LENGTH = 11;
    const resizeObservers = share("resizeObservers", () => ({ docs: [], scheduled: false, generation: -1 }));
    const observerStates = share("resizeObserverStates", () => new WeakMap());
    // this window's document's observers (Document::mResizeObservers), registered with the tick once
    const docObservers = [];
    const docEntry = { win: global, observers: docObservers, notify: () => notifyResizeObservers() };
    const addResizeObserver = observer => {
      if (!resizeObservers.docs.includes(docEntry)) resizeObservers.docs.push(docEntry);
      docObservers.push(observer);
    };
    const removeResizeObserver = observer => {
      const i = docObservers.indexOf(observer);
      if (i >= 0) docObservers.splice(i, 1);
    };
    const scheduleNotification = () => { resizeObservers.scheduled = true; };
    // GetNodeDepth: the flattened tree's depth (a shadow root is skipped: a slotted node's parent is its
    // slot, a shadow tree's top nodes' parent is the host; a host's child in no slot has none)
    const LINK_HOST = 5, LINK_SHADOW_ROOT = 6, LINK_ASSIGNED_SLOT = 8;
    const flattenedTreeParent = n => {
      const p = n.parentNode;
      if (p === null) return null;
      if (p.nodeType === 11) return __mrtLink(p, LINK_HOST) || p;
      if (p.nodeType === 1 && __mrtLink(p, LINK_SHADOW_ROOT)) return __mrtLink(n, LINK_ASSIGNED_SLOT);
      return p;
    };
    const nodeDepth = n => {
      let depth = 1;
      while ((n = flattenedTreeParent(n))) depth++;
      return depth;
    };
    const sizeOf = (boxes, i, box) => {
      const at = i * BOX_LENGTH + BOX_OFFSET[box];
      return [boxes[at], boxes[at + 1]];
    };
    const observerOf = (self, member) => {
      const st = self !== null && typeof self === "object" ? observerStates.get(self) : undefined;
      if (!st) throw new TypeError(`'${member}' called on an object that does not implement interface ResizeObserver.`);
      return st;
    };
    const isElement = v => v instanceof Element;

    // ResizeObserverSize and ResizeObserverEntry: no constructor; their members on the prototypes
    const sizeStates = new WeakMap(), entryStates = new WeakMap();
    const makeInterface = (name, members, states) => {
      const C = { [name]: function () { throw new TypeError("Illegal constructor."); } }[name];
      // the attributes, then the constructor, as Gecko's bindings define them
      delete C.prototype.constructor;
      for (const k of members) {
        const get = Object.getOwnPropertyDescriptor({ get [k]() {
          const st = this !== null && typeof this === "object" ? states.get(this) : undefined;
          if (!st) throw new TypeError(`'get ${k}' called on an object that does not implement interface ${name}.`);
          return st[k];
        } }, k).get;
        Object.defineProperty(C.prototype, k, { get, enumerable: true, configurable: true });
      }
      Object.defineProperty(C.prototype, "constructor", { value: C, writable: true, enumerable: false, configurable: true });
      Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true });
      Object.defineProperty(C, "prototype", { writable: false, enumerable: false, configurable: false });
      define(global, name, C);
      return C;
    };
    const ResizeObserverEntry = makeInterface("ResizeObserverEntry", ["target", "contentRect", "borderBoxSize", "contentBoxSize", "devicePixelContentBoxSize"], entryStates);
    const ResizeObserverSize = makeInterface("ResizeObserverSize", ["inlineSize", "blockSize"], sizeStates);
    // [Frozen, Cached] sequences: one frozen array per entry and box
    const sizeList = ([inlineSize, blockSize]) => {
      const s = Object.create(ResizeObserverSize.prototype);
      sizeStates.set(s, { inlineSize, blockSize });
      return Object.freeze([s]);
    };
    // ResizeObserverEntry's constructor: the three sizes and the contentRect at the padding's origin
    const makeEntry = (target, boxes, i) => {
      const e = Object.create(ResizeObserverEntry.prototype);
      const at = i * BOX_LENGTH;
      entryStates.set(e, {
        target, contentRect: makeRect(DOMRectReadOnly.prototype, false, boxes[at + 7], boxes[at + 8], boxes[at + 9], boxes[at + 10]),
        borderBoxSize: sizeList(sizeOf(boxes, i, "border-box")), contentBoxSize: sizeList(sizeOf(boxes, i, "content-box")),
        devicePixelContentBoxSize: sizeList(sizeOf(boxes, i, "device-pixel-content-box")),
      });
      return e;
    };

    const ResizeObserver = function ResizeObserver(callback) {
      if (new.target === undefined) throw new TypeError("ResizeObserver constructor: 'new' is required");
      if (arguments.length < 1) throw new TypeError("ResizeObserver constructor: At least 1 argument required, but only 0 passed");
      if (callback === null || (typeof callback !== "object" && typeof callback !== "function")) throw new TypeError("ResizeObserver constructor: Argument 1 is not an object.");
      if (typeof callback !== "function") throw new TypeError("ResizeObserver constructor: Argument 1 is not callable.");
      const proto = new.target.prototype;
      const self = Object.create(proto !== null && typeof proto === "object" ? proto : ResizeObserver.prototype);
      // observations: { target, box, last: [inline, block] }; active: those gathered; skipped: whether
      // the depth left one out
      observerStates.set(self, { callback, observations: [], active: [], skipped: false });
      return self;
    };
    ResizeObserver.prototype = {};
    {
      const proto = ResizeObserver.prototype;
      const methods = {
        observe(target) {
          const st = observerOf(this, "observe");
          if (arguments.length < 1) throw new TypeError("ResizeObserver.observe: At least 1 argument required, but only 0 passed");
          if (!isElement(target)) throw new TypeError("ResizeObserver.observe: Argument 1 does not implement interface Element.");
          const options = arguments[1];
          if (options !== undefined && options !== null && typeof options !== "object" && typeof options !== "function") {
            throw new TypeError("ResizeObserver.observe: Argument 2 can't be converted to a dictionary.");
          }
          let box = "content-box";
          if (options !== undefined && options !== null && options.box !== undefined) {
            box = String(options.box);
            if (!BOXES.includes(box)) throw new TypeError(`ResizeObserver.observe: '${box}' (value of 'box' member of ResizeObserverOptions) is not a valid value for enumeration ResizeObserverBoxOptions.`);
          }
          if (st.observations.length === 0) addResizeObserver(this);
          const i = st.observations.findIndex(o => o.target === target);
          if (i >= 0) {
            // the same box: kept as it is, with its last reported size
            if (st.observations[i].box === box) return;
            st.observations.splice(i, 1);
          }
          st.observations.push({ target, box, last: [-1, -1] });
          scheduleNotification();
        },
        unobserve(target) {
          const st = observerOf(this, "unobserve");
          if (arguments.length < 1) throw new TypeError("ResizeObserver.unobserve: At least 1 argument required, but only 0 passed");
          if (!isElement(target)) throw new TypeError("ResizeObserver.unobserve: Argument 1 does not implement interface Element.");
          const i = st.observations.findIndex(o => o.target === target);
          if (i < 0) return;
          st.observations.splice(i, 1);
          if (st.observations.length === 0) removeResizeObserver(this);
        },
        disconnect() {
          const st = observerOf(this, "disconnect");
          const registered = st.observations.length > 0;
          st.observations = [];
          st.active = [];
          if (registered) removeResizeObserver(this);
        },
      };
      for (const name of ["observe", "unobserve", "disconnect"]) Object.defineProperty(proto, name, { value: methods[name], writable: true, enumerable: true, configurable: true });
      Object.defineProperty(proto, "constructor", { value: ResizeObserver, writable: true, enumerable: false, configurable: true });
      Object.defineProperty(proto, Symbol.toStringTag, { value: "ResizeObserver", configurable: true });
      Object.defineProperty(ResizeObserver, "prototype", { writable: false, enumerable: false, configurable: false });
    }
    define(global, "ResizeObserver", ResizeObserver);

    // Document::GatherAllActiveResizeObservations: each observer's observations whose size is not the
    // last reported, deeper than `depth` (the others skipped); one layout read for all of them
    const gather = depth => {
      const all = [];
      for (const observer of docObservers) {
        const st = observerStates.get(observer);
        st.active = [];
        st.skipped = false;
        for (const o of st.observations) all.push([st, o]);
      }
      const boxes = __mrtResizeBoxes(all.map(([, o]) => o.target), true);
      let any = false;
      all.forEach(([st, o], i) => {
        const [inline, block] = sizeOf(boxes, i, o.box);
        if (inline === o.last[0] && block === o.last[1]) return;
        if (nodeDepth(o.target) > depth) {
          st.active.push(o);
          any = true;
        } else {
          st.skipped = true;
        }
      });
      return any;
    };
    // Document::BroadcastAllActiveResizeObservations: each observer with active observations gets
    // its entries, their sizes read from the frames as they are, without a flush (what an earlier
    // callback changed shows only if something flushed since); the last reported size is the observed
    // box's. Returns the shallowest depth reported.
    const broadcast = () => {
      let shallowest = Infinity;
      for (const observer of docObservers.slice()) {
        const st = observerStates.get(observer);
        if (st.active.length === 0) continue;
        const active = st.active;
        const boxes = __mrtResizeBoxes(active.map(o => o.target), false);
        const entries = active.map((o, i) => {
          o.last = sizeOf(boxes, i, o.box);
          shallowest = Math.min(shallowest, nodeDepth(o.target));
          return makeEntry(o.target, boxes, i);
        });
        st.active = [];
        st.skipped = false;
        global.__mrt.enterCallback();
        try { st.callback.call(observer, entries, observer); }
        catch (e) { console.error("uncaught exception in a ResizeObserver:", e); }
        global.__mrt.leaveCallback();
      }
      return shallowest;
    };
    function notifyResizeObservers() {
      let depth = 0;
      while (gather(depth)) depth = broadcast();
      if (docObservers.some(o => observerStates.get(o).skipped)) {
        global.__mrt.reportScriptError({ message: "ResizeObserver loop completed with undelivered notifications.", filename: "", lineno: 0, colno: 0, bubbles: false, cancelable: false });
        scheduleNotification();
      }
    }
  }

  // ---- IntersectionObserver (Gecko's dom/base/DOMIntersectionObserver.cpp) ----
  //
  // An observer keeps its targets in the order observe() was called, each with the threshold index
  // last seen (-2 at first, so the first update always queues an entry). The document keeps its
  // observers in order from their first observe() until their last target goes (Connect,
  // Disconnect). The refresh tick (prelude.js) runs update(), Gecko's Document::UpdateIntersections:
  // each observer's Update queues an entry for each target whose threshold index changed, then a
  // task calls each observer with its queued entries (NotifyIntersectionObservers). observe() asks
  // for a tick (EnsureIntersectionObservationsUpdateHappens); so does a change of the DOM while
  // observers are registered (the refresh driver's layout phase implies the update).
  // The geometry comes from __mrtIntersections (Host::intersections): per target, in app units,
  // [computed, rootBounds, the target's rectangle, intersects, the intersection rectangle].
  // A scroll of the document (rootScroll) moves the targets: the canvas frame is placed at minus it,
  // and the next tick updates. Not done: element scroll containers (GAPS B155);
  // a frame of another origin runs in its own runtime, so its implicit root is its own viewport;
  // content-visibility (GAPS B77); an exception in the callback goes to the console (GAPS B31).
  if (!isWorker) {
    const intersectionObservers = share("intersectionObservers", () => ({ docs: [], scheduled: false, generation: -1 }));
    const observerStates = share("intersectionObserverStates", () => new WeakMap());
    const UNINITIALIZED = -2;
    // this window's document's observers (Document::mIntersectionObservers), registered with the tick
    const docObservers = [];
    const docEntry = { win: global, observers: docObservers, update: () => updateIntersections() };
    const scheduleUpdate = () => { intersectionObservers.scheduled = true; };
    // Element and Document of any window of the runtime (a frame's elements too): the native
    // nodeType getter's brand check
    const nodeTypeGetter = Object.getOwnPropertyDescriptor(Node.prototype, "nodeType").get;
    const nodeTypeOf = v => {
      if (v === null || typeof v !== "object") return 0;
      try { return nodeTypeGetter.call(v); } catch (e) { return 0; }
    };
    const isElement = v => nodeTypeOf(v) === 1;
    const isDocument = v => nodeTypeOf(v) === 9;

    // Servo_IntersectionObserverMargin_Parse: one to four px lengths or percentages (a bare number,
    // even 0, is refused); kept as Servo keeps them (f32 lengths, percentages as fractions)
    const MARGIN_TOKEN = /^([+-]?(?:\d+(?:\.\d+)?|\.\d+)(?:[eE][+-]?\d+)?)(px|%)$/i;
    const parseMargin = text => {
      const parts = text.replace(/\/\*[\s\S]*?\*\//g, " ").split(/[ \t\n\r\f]+/).filter(Boolean);
      if (parts.length === 0) return [[0, false], [0, false], [0, false], [0, false]];
      if (parts.length > 4) return null;
      const values = [];
      for (const p of parts) {
        const m = MARGIN_TOKEN.exec(p);
        if (!m) return null;
        const percent = m[2] === "%";
        values.push([percent ? Math.fround(Math.fround(Number(m[1]) / 100) * 100) : Math.fround(Number(m[1])), percent]);
      }
      const [t, r = t, b = t, l = r] = values;
      return [t, r, b, l];
    };
    // Servo_IntersectionObserverMargin_ToString: all four sides, numbers as cssparser writes an f32
    // (an integer as one, otherwise six significant digits)
    const serializeNumber = v => (Number.isInteger(v) ? String(v) : String(Number(v.toPrecision(6))));
    const serializeMargin = m => m.map(([v, percent]) => serializeNumber(v) + (percent ? "%" : "px")).join(" ");
    // the restricted doubles of the threshold member, and their range
    const finite = (v, what) => {
      const n = Number(v);
      if (!Number.isFinite(n)) throw new TypeError(`IntersectionObserver constructor: ${what} is not a finite floating-point value.`);
      return n;
    };

    const observerOf = (self, member) => {
      const st = self !== null && typeof self === "object" ? observerStates.get(self) : undefined;
      if (!st) throw new TypeError(`'${member}' called on an object that does not implement interface IntersectionObserver.`);
      return st;
    };

    // IntersectionObserverEntry: no constructor; [Constant] members on the prototype
    const entryStates = new WeakMap();
    const ENTRY_MEMBERS = ["time", "rootBounds", "boundingClientRect", "intersectionRect", "isIntersecting", "intersectionRatio", "target"];
    const IntersectionObserverEntry = function IntersectionObserverEntry() { throw new TypeError("Illegal constructor."); };
    {
      const proto = IntersectionObserverEntry.prototype;
      delete proto.constructor;
      for (const k of ENTRY_MEMBERS) {
        const get = Object.getOwnPropertyDescriptor({ get [k]() {
          const st = this !== null && typeof this === "object" ? entryStates.get(this) : undefined;
          if (!st) throw new TypeError(`'get ${k}' called on an object that does not implement interface IntersectionObserverEntry.`);
          return st[k];
        } }, k).get;
        Object.defineProperty(proto, k, { get, enumerable: true, configurable: true });
      }
      Object.defineProperty(proto, "constructor", { value: IntersectionObserverEntry, writable: true, enumerable: false, configurable: true });
      Object.defineProperty(proto, Symbol.toStringTag, { value: "IntersectionObserverEntry", configurable: true });
      Object.defineProperty(IntersectionObserverEntry, "prototype", { writable: false, enumerable: false, configurable: false });
    }
    define(global, "IntersectionObserverEntry", IntersectionObserverEntry);
    // DOMRect::SetLayoutRect: app units to CSS px in 1/65536 px steps (the far edges rounded too)
    const layoutRect = (a, at) => {
      const px = v => Math.round(v * (65536 / 60)) / 65536;
      const x = px(a[at]), y = px(a[at + 1]);
      return makeRect(DOMRect.prototype, true, x, y, px(a[at] + a[at + 2]) - x, px(a[at + 1] + a[at + 3]) - y);
    };

    const IntersectionObserver = function IntersectionObserver(callback) {
      if (new.target === undefined) throw new TypeError("IntersectionObserver constructor: 'new' is required");
      if (arguments.length < 1) throw new TypeError("IntersectionObserver constructor: At least 1 argument required, but only 0 passed");
      if (callback === null || (typeof callback !== "object" && typeof callback !== "function")) throw new TypeError("IntersectionObserver constructor: Argument 1 is not an object.");
      if (typeof callback !== "function") throw new TypeError("IntersectionObserver constructor: Argument 1 is not callable.");
      // IntersectionObserverInit, its members read in order
      const options = arguments[1];
      if (options !== undefined && options !== null && typeof options !== "object" && typeof options !== "function") {
        throw new TypeError("IntersectionObserver constructor: Argument 2 can't be converted to a dictionary.");
      }
      const member = k => (options === undefined || options === null ? undefined : options[k]);
      let root = member("root");
      if (root === undefined || root === null) root = null;
      else if (!isElement(root) && !isDocument(root)) {
        throw new TypeError("IntersectionObserver constructor: 'root' member of IntersectionObserverInit could not be converted to any of: Element, Document.");
      }
      const rootMarginText = member("rootMargin") === undefined ? "0px" : String(member("rootMargin"));
      const scrollMarginText = member("scrollMargin") === undefined ? "0px" : String(member("scrollMargin"));
      const threshold = member("threshold");
      let thresholds;
      const iterator = threshold !== null && (typeof threshold === "object" || typeof threshold === "function") ? threshold[Symbol.iterator] : undefined;
      if (iterator !== undefined && iterator !== null) {
        thresholds = [];
        for (const v of threshold) thresholds.push(finite(v, "Element of sequence<double> branch of (double or sequence<double>)"));
      } else {
        thresholds = [threshold === undefined ? 0 : finite(threshold, "Double branch of (double or sequence<double>)")];
      }
      const rootMargin = parseMargin(rootMarginText);
      if (!rootMargin) throw new DOMException("IntersectionObserver constructor: rootMargin must be specified in pixels or percent.", "SyntaxError");
      const scrollMargin = parseMargin(scrollMarginText);
      if (!scrollMargin) throw new DOMException("IntersectionObserver constructor: scrollMargin must be specified in pixels or percent.", "SyntaxError");
      if (thresholds.some(t => t < 0 || t > 1)) throw new RangeError("IntersectionObserver constructor: Threshold values must all be in the range [0, 1].");
      thresholds.sort((a, b) => a - b);
      if (thresholds.length === 0) thresholds.push(0);
      const proto = new.target.prototype;
      const self = Object.create(proto !== null && typeof proto === "object" ? proto : IntersectionObserver.prototype);
      // targets: { target, index } in observe() order; queued: the entries not yet delivered
      observerStates.set(self, { callback, root, rootMargin, scrollMargin, thresholds, thresholdsArray: null, targets: [], queued: [], connected: false });
      return self;
    };
    IntersectionObserver.prototype = {};
    {
      const proto = IntersectionObserver.prototype;
      // Connect and Disconnect: the document's list
      const connect = (self, st) => {
        if (st.connected) return;
        st.connected = true;
        if (!intersectionObservers.docs.includes(docEntry)) intersectionObservers.docs.push(docEntry);
        docObservers.push(self);
      };
      const disconnect = (self, st) => {
        if (!st.connected) return;
        st.connected = false;
        st.targets = [];
        const i = docObservers.indexOf(self);
        if (i >= 0) docObservers.splice(i, 1);
      };
      const methods = {
        observe(target) {
          const st = observerOf(this, "observe");
          if (arguments.length < 1) throw new TypeError("IntersectionObserver.observe: At least 1 argument required, but only 0 passed");
          if (target === null || typeof target !== "object") throw new TypeError("IntersectionObserver.observe: Argument 1 is not an object.");
          if (!isElement(target)) throw new TypeError("IntersectionObserver.observe: Argument 1 does not implement interface Element.");
          if (st.targets.some(t => t.target === target)) return;
          st.targets.push({ target, index: UNINITIALIZED });
          connect(this, st);
          scheduleUpdate();
        },
        unobserve(target) {
          const st = observerOf(this, "unobserve");
          if (arguments.length < 1) throw new TypeError("IntersectionObserver.unobserve: At least 1 argument required, but only 0 passed");
          if (target === null || typeof target !== "object") throw new TypeError("IntersectionObserver.unobserve: Argument 1 is not an object.");
          if (!isElement(target)) throw new TypeError("IntersectionObserver.unobserve: Argument 1 does not implement interface Element.");
          const i = st.targets.findIndex(t => t.target === target);
          if (i < 0) return;
          st.targets.splice(i, 1);
          if (st.targets.length === 0) disconnect(this, st);
        },
        disconnect() {
          disconnect(this, observerOf(this, "disconnect"));
        },
        takeRecords() {
          const st = observerOf(this, "takeRecords");
          const records = st.queued;
          st.queued = [];
          return records;
        },
      };
      for (const name of ["observe", "unobserve", "disconnect", "takeRecords"]) Object.defineProperty(proto, name, { value: methods[name], writable: true, enumerable: true, configurable: true });
      // the operations, then the attributes, as Gecko's bindings define them here
      const attributes = {
        root: st => st.root,
        rootMargin: st => serializeMargin(st.rootMargin),
        scrollMargin: st => serializeMargin(st.scrollMargin),
        // [Cached]: the same array each time
        thresholds: st => st.thresholdsArray || (st.thresholdsArray = st.thresholds.slice()),
      };
      for (const [k, read] of Object.entries(attributes)) {
        const get = Object.getOwnPropertyDescriptor({ get [k]() { return read(observerOf(this, `get ${k}`)); } }, k).get;
        Object.defineProperty(proto, k, { get, enumerable: true, configurable: true });
      }
      Object.defineProperty(proto, "constructor", { value: IntersectionObserver, writable: true, enumerable: false, configurable: true });
      Object.defineProperty(proto, Symbol.toStringTag, { value: "IntersectionObserver", configurable: true });
      Object.defineProperty(IntersectionObserver, "prototype", { writable: false, enumerable: false, configurable: false });
    }
    define(global, "IntersectionObserver", IntersectionObserver);

    // DOMIntersectionObserver::Update: each target's threshold index from its intersection ratio
    // (areas in app units); an entry queued when the index changed, its isIntersecting the index > 0
    const updateObserver = (st, time) => {
      if (st.targets.length === 0) return;
      const margins = [...st.rootMargin.flatMap(([v, p]) => [v, p ? 1 : 0]), ...st.scrollMargin.flatMap(([v, p]) => [v, p ? 1 : 0])];
      const g = __mrtIntersections(document, st.root, margins, st.targets.map(t => t.target));
      st.targets.forEach((t, i) => {
        const at = i * 14;
        const intersects = g[at + 9] === 1;
        const targetArea = g[at + 7] * g[at + 8];
        const intersectionArea = intersects ? g[at + 12] * g[at + 13] : 0;
        const ratio = targetArea > 0 ? Math.min(intersectionArea / targetArea, 1) : intersects ? 1 : 0;
        let index = -1;
        if (intersects) {
          index = st.thresholds.findIndex(v => v > ratio);
          if (index < 0) index = st.thresholds.length;
          // below every threshold but intersecting counts as not intersecting (w3c/IntersectionObserver#432)
          if (index === 0) index = -1;
        }
        if (index === t.index) return;
        t.index = index;
        const entry = Object.create(IntersectionObserverEntry.prototype);
        entryStates.set(entry, {
          time, rootBounds: layoutRect(g, at + 1), boundingClientRect: layoutRect(g, at + 5),
          intersectionRect: intersects ? layoutRect(g, at + 10) : makeRect(DOMRect.prototype, true, 0, 0, 0, 0),
          isIntersecting: index > 0, intersectionRatio: ratio, target: t.target,
        });
        st.queued.push(entry);
      });
    };
    // Document::UpdateIntersections, then NotifyIntersectionObservers as a task of its own: each
    // observer of the list as it is then, called with the entries queued (none: not called)
    function updateIntersections() {
      const time = performance.now();
      for (const observer of docObservers) updateObserver(observerStates.get(observer), time);
      global.__mrt.queueTask(() => {
        for (const observer of docObservers.slice()) {
          const st = observerStates.get(observer);
          if (st.queued.length === 0) continue;
          const entries = st.queued;
          st.queued = [];
          global.__mrt.enterCallback();
          try { st.callback.call(observer, entries, observer); }
          catch (e) { console.error("uncaught exception in an IntersectionObserver:", e); }
          global.__mrt.leaveCallback();
        }
      });
    }
  }

  // The rest of Firefox's console (the C++ layer has log, info, warn and error): the printing ones
  // print as log, the others do nothing
  for (const name of ["debug", "trace", "dir", "dirxml", "table"]) {
    if (!console[name]) console[name] = console.log;
  }
  for (const name of ["assert", "clear", "count", "countReset", "group", "groupCollapsed", "groupEnd", "time", "timeEnd",
                      "timeLog", "timeStamp", "profile", "profileEnd"]) {
    if (!console[name]) console[name] = () => {};
  }

  // HTMLMediaElement (<video> and <audio>; a read on another object, such as a subclass's
  // prototype, gets undefined and a write makes an own property): nothing is decoded. play() is refused as Firefox refuses it with autoplay blocked
  // (media.autoplay.default = 5 in the harnesses); the state reads say "nothing loaded, paused".
  const isMedia = el => el.tagName === "VIDEO" || el.tagName === "AUDIO";
  // Backend answers, asked once per media type: [MediaSource.isTypeSupported, canPlayType].
  // Firefox uses its platform codecs; standalone has no playback transport and supports no types.
  const mediaTypes = new Map();
  const mediaTypeAnswer = type => {
    type = String(type);
    let a = mediaTypes.get(type);
    if (!a) {
      try { a = bridge("mediaType", { type }); } catch (e) { a = [false, ""]; }
      // HTTP answers include a backend epoch and availability. Do not cache
      // detached negatives; viewer lifecycle also invalidates attached entries.
      if (a[3] !== false) mediaTypes.set(type, a);
    }
    return a;
  };
  // With the bridge tab as the page's screen (realsite --interactive), a media element plays in the
  // tab instead (SPEC decision 26): see "Media in the bridge tab" by the mirror, below.
  const mediaMethods = {
    play() {
      if (isMedia(this) && mediaLink.on()) return mediaLink.play(this);
      stub("HTMLMediaElement.play");
      const e = new Error("The play method is not allowed by the user agent.");
      e.name = "NotAllowedError";
      return Promise.reject(e);
    },
    pause() { if (isMedia(this) && mediaLink.on()) mediaLink.pause(this); },
    load() { if (isMedia(this) && mediaLink.on()) mediaLink.send(this, { o: "load" }); },
    canPlayType(type) { return mediaTypeAnswer(type)[1]; },
  };
  if (!isWorker) for (const [name, fn] of Object.entries(mediaMethods)) {
    define(HTMLMediaElement.prototype, name, fn);
  }
  // MediaError (dom/html/MediaError.cpp): what a media element's `error` is; nothing loads here, so
  // none is ever made, but its constants are read by players
  class MediaError {
    constructor() { throw new TypeError("Illegal constructor."); }
    get code() { return this._code; }
    get message() { return this._message || ""; }
  }
  Object.entries({ MEDIA_ERR_ABORTED: 1, MEDIA_ERR_NETWORK: 2, MEDIA_ERR_DECODE: 3, MEDIA_ERR_SRC_NOT_SUPPORTED: 4 }).forEach(([k, v]) => {
    Object.defineProperty(MediaError, k, { value: v, enumerable: true });
    Object.defineProperty(MediaError.prototype, k, { value: v, enumerable: true });
  });
  define(global, "MediaError", MediaError);

  // ---- Media Source Extensions (dom/media/mediasource), SPEC decisions 11 and 26 ----
  //
  // MediaSource, SourceBuffer and SourceBufferList as Gecko's, with its checks and events, but no
  // demuxer: appended bytes are counted and dropped, as a segment can be fetched again; `buffered`
  // stays empty. What a page can observe without parsing is kept: readyState, duration, updating.
  // The media element reaches HAVE_METADATA once every SourceBuffer has had its first append, which
  // MSE requires to be an initialization segment. Interactive mode forwards real decoding;
  // HTTP mode additionally retains a bounded reconstruction cache in mediaLink below.
  // Events are tasks (QueueAsyncSimpleEvent).
  const domError = name => new DOMException(name === "InvalidStateError" ? "An attempt was made to use an object that is not, or is no longer, usable"
    : name === "NotFoundError" ? "The object can not be found here." : "Operation is not supported", name);
  const queueEvent = (target, type) => global.__mrt.queueTask(() => target.dispatchEvent(new Event(type)));
  class TimeRanges {
    constructor(ranges = []) { this._ranges = ranges; }
    get length() { return this._ranges.length; }
    start(i) { if (i >= this._ranges.length) throw new DOMException(`TimeRanges.start: Index is out of range`, "IndexSizeError"); return this._ranges[i][0]; }
    end(i) { if (i >= this._ranges.length) throw new DOMException(`TimeRanges.end: Index is out of range`, "IndexSizeError"); return this._ranges[i][1]; }
  }
  define(global, "TimeRanges", TimeRanges);
  class SourceBufferList extends EventTarget {
    constructor() { super(); this._list = []; }
    get length() { return this._list.length; }
    _append(sb) { this[this._list.length] = sb; this._list.push(sb); queueEvent(this, "addsourcebuffer"); }
    _remove(sb) {
      const i = this._list.indexOf(sb);
      if (i < 0) return;
      this._list.splice(i, 1);
      for (let j = 0; j <= this._list.length; j++) { if (j < this._list.length) this[j] = this._list[j]; else delete this[j]; }
      queueEvent(this, "removesourcebuffer");
    }
    [Symbol.iterator]() { return this._list[Symbol.iterator](); }
  }
  class SourceBuffer extends EventTarget {
    constructor(mediaSource, type) {
      super();
      this._ms = mediaSource; this._type = type; this._updating = false; this._appends = 0;
      this._mode = "segments"; this._timestampOffset = 0; this._appendWindowStart = 0; this._appendWindowEnd = Infinity;
    }
    get updating() { return this._updating; }
    get buffered() { if (!this._ms) throw domError("InvalidStateError"); return new TimeRanges(this._ranges || []); }
    get mode() { return this._mode; }
    set mode(v) {
      if (!this._ms || this._updating) throw domError("InvalidStateError");
      if (this._ms._readyState === "ended") this._ms._setReadyState("open");
      this._mode = v;
      mediaLink.msOp(this._ms, { o: "sbset", sb: mediaLink.id(this), name: "mode", value: mediaNum(this._mode) });
    }
    get timestampOffset() { return this._timestampOffset; }
    set timestampOffset(v) {
      if (!this._ms || this._updating) throw domError("InvalidStateError");
      if (this._ms._readyState === "ended") this._ms._setReadyState("open");
      this._timestampOffset = +v;
      mediaLink.msOp(this._ms, { o: "sbset", sb: mediaLink.id(this), name: "timestampOffset", value: mediaNum(this._timestampOffset) });
    }
    get appendWindowStart() { return this._appendWindowStart; }
    set appendWindowStart(v) {
      if (!this._ms || this._updating) throw domError("InvalidStateError");
      if (!(v >= 0) || v >= this._appendWindowEnd) throw new TypeError("Invalid appendWindowStart value");
      this._appendWindowStart = +v;
      mediaLink.msOp(this._ms, { o: "sbset", sb: mediaLink.id(this), name: "appendWindowStart", value: mediaNum(this._appendWindowStart) });
    }
    get appendWindowEnd() { return this._appendWindowEnd; }
    set appendWindowEnd(v) {
      if (!this._ms || this._updating) throw domError("InvalidStateError");
      if (Number.isNaN(+v) || v <= this._appendWindowStart) throw new TypeError("Invalid appendWindowEnd value");
      this._appendWindowEnd = +v;
      mediaLink.msOp(this._ms, { o: "sbset", sb: mediaLink.id(this), name: "appendWindowEnd", value: mediaNum(this._appendWindowEnd) });
    }
    // PrepareAppend, then the append completes in a later task (StartUpdating, StopUpdating); the
    // data is not demuxed here. With the bridge tab as the screen, bytes go to its SourceBuffer
    // (HTTP mode also retains bounded encoded commands for reconnect)
    // and the append completes when the tab's does (_tabDone)
    appendBuffer(data) {
      if (!this._ms || this._updating) throw domError("InvalidStateError");
      if (this._ms._element && this._ms._element._error) throw domError("InvalidStateError");
      if (env.httpViewer && ((data?.byteLength || 0) > 4 * 1024 * 1024 ||
          mediaLink.pendingBytes + (data?.byteLength || 0) > 8 * 1024 * 1024 ||
          !mediaLink.canRetain(data?.byteLength || 0))) throw domError("QuotaExceededError");
      if (this._ms._readyState === "ended") this._ms._setReadyState("open");
      this._updating = true;
      queueEvent(this, "updatestart");
      if (mediaLink.on() && this._ms._element) {
        const bytes = ArrayBuffer.isView(data) ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength) : new Uint8Array(data);
        this._n = (this._n || 0) + 1;
        this._op = "append";
        mediaLink.stats.bytes += bytes.length;
        mediaLink.msOp(this._ms, { o: "append", sb: mediaLink.id(this), n: this._n, b64: toBase64(bytes) });
        return;
      }
      const appending = this._appending = {};
      global.__mrt.queueTask(() => {
        if (this._appending !== appending) return;
        this._appending = null;
        this._appends++;
        this._updating = false;
        queueEvent(this, "update");
        queueEvent(this, "updateend");
        this._ms._initializationSegmentReceived();
      });
    }
    abort() {
      if (!this._ms || this._ms._readyState !== "open") throw domError("InvalidStateError");
      this._abortBufferAppend();
      this._appendWindowStart = 0;
      this._appendWindowEnd = Infinity;
    }
    _abortBufferAppend() {
      if (!this._updating) return;
      if (this._n) mediaLink.msOp(this._ms, { o: "abort", sb: mediaLink.id(this), n:this._n });
      this._appending = null;
      this._updating = false;
      queueEvent(this, "abort");
      queueEvent(this, "updateend");
    }
    remove(start, end) {
      if (!this._ms || this._updating) throw domError("InvalidStateError");
      if (Number.isNaN(this._ms.duration)) throw new TypeError("Duration is NaN");
      if (start < 0 || start > this._ms.duration) throw new TypeError("Invalid start value");
      if (end <= start || Number.isNaN(+end)) throw new TypeError("Invalid end value");
      if (this._ms._readyState === "ended") this._ms._setReadyState("open");
      this._updating = true;
      queueEvent(this, "updatestart");
      if (mediaLink.on() && this._ms._element) {
        this._n = (this._n || 0) + 1;
        this._op = "remove";
        mediaLink.msOp(this._ms, { o: "remove", sb: mediaLink.id(this), n: this._n, start: mediaNum(+start), end: mediaNum(+end) });
        return;
      }
      global.__mrt.queueTask(() => { this._updating = false; queueEvent(this, "update"); queueEvent(this, "updateend"); });
    }
    // the tab's append or remove `n` ended (mediaEvents): the tab's ranges and the source's duration,
    // then update and updateend, or error and updateend; nothing for one aborted since
    _tabDone(m) {
      if (m.n !== this._n || !this._updating) return;
      this._updating = false;
      this._ranges = m.buffered.map(r => r.map(mediaValue));
      if (this._ms && m.dur !== undefined) this._ms._duration = mediaValue(m.dur);
      if (m.error) { queueEvent(this, "error"); queueEvent(this, "updateend"); return; }
      if (this._op === "append") this._appends++;
      queueEvent(this, "update");
      queueEvent(this, "updateend");
    }
    changeType(type) {
      if (String(type) === "") throw new TypeError("Type must not be empty");
      if (!this._ms || this._updating) throw domError("InvalidStateError");
      if (!mediaTypeAnswer(type)[0]) throw new DOMException("Type not supported in MediaSource", "NotSupportedError");
      if (this._ms._readyState === "ended") this._ms._setReadyState("open");
      this._type = String(type);
      mediaLink.msOp(this._ms, { o: "changetype", sb: mediaLink.id(this), type: this._type });
    }
  }
  class MediaSource extends EventTarget {
    constructor() {
      super();
      this._readyState = "closed"; this._duration = NaN; this._element = null;
      this._sourceBuffers = new SourceBufferList(); this._activeSourceBuffers = new SourceBufferList();
    }
    static isTypeSupported(type) { return String(type) !== "" && mediaTypeAnswer(type)[0]; }
    get readyState() { return this._readyState; }
    get sourceBuffers() { return this._sourceBuffers; }
    get activeSourceBuffers() { return this._activeSourceBuffers; }
    get duration() { return this._readyState === "closed" ? NaN : this._duration; }
    set duration(v) {
      v = +v;
      if (v < 0 || Number.isNaN(v)) throw new TypeError(`Invalid duration value ${v}`);
      if (this._readyState !== "open" || this._sourceBuffers._list.some(b => b._updating)) throw domError("InvalidStateError");
      // (in the tab, its element gets the durationchange, which comes back with it)
      if (mediaLink.on() && this._element) { this._duration = v; mediaLink.msOp(this, { o: "duration", value: mediaNum(v) }); return; }
      this._durationChange(v);
    }
    // DurationChange: the media element's duration change algorithm (no buffered frames here)
    _durationChange(v) {
      if (this._duration === v) return;
      this._duration = v;
      if (this._element) { this._element._duration = v; queueEvent(this._element, "durationchange"); }
    }
    addSourceBuffer(type) {
      type = String(type);
      if (type === "") throw new TypeError("Empty type");
      if (!mediaTypeAnswer(type)[0]) throw new DOMException("Type not supported in MediaSource", "NotSupportedError");
      if (this._readyState !== "open") throw domError("InvalidStateError");
      const sb = new SourceBuffer(this, type);
      this._sourceBuffers._append(sb);
      this._activeSourceBuffers._append(sb);
      mediaLink.msOp(this, { o: "addsb", sb: mediaLink.id(sb), type });
      return sb;
    }
    removeSourceBuffer(sb) {
      if (!this._sourceBuffers._list.includes(sb)) throw domError("NotFoundError");
      sb._abortBufferAppend();
      mediaLink.msOp(this, { o: "rmsb", sb: mediaLink.id(sb) });
      this._activeSourceBuffers._remove(sb);
      this._sourceBuffers._remove(sb);
      sb._ms = null;
    }
    endOfStream(error) {
      if (this._readyState !== "open" || this._sourceBuffers._list.some(b => b._updating)) throw domError("InvalidStateError");
      if (error !== undefined && error !== "network" && error !== "decode") {
        this._setReadyState("ended");
        throw new TypeError(`MediaSource.endOfStream: '${error}' (value of argument 1) is not a valid value for enumeration MediaSourceEndOfStreamError.`);
      }
      this._setReadyState("ended");
      mediaLink.msOp(this, { o: "eos", error });
    }
    setLiveSeekableRange(start, end) { if (this._readyState !== "open") throw domError("InvalidStateError"); if (start < 0 || start > end) throw new TypeError("Invalid start value"); }
    clearLiveSeekableRange() { if (this._readyState !== "open") throw domError("InvalidStateError"); }
    // SetReadyState: sourceopen, sourceended, sourceclose
    _setReadyState(state) {
      const old = this._readyState;
      this._readyState = state;
      if (state === "open" && (old === "closed" || old === "ended")) queueEvent(this, "sourceopen");
      else if (state === "ended" && old === "open") queueEvent(this, "sourceended");
      else if (state === "closed" && old !== "closed") queueEvent(this, "sourceclose");
    }
    // Attach (HTMLMediaElement's resource selection with a MediaSource URL)
    _attach(element) {
      if (this._readyState !== "closed") return false;
      this._element = element;
      // (with the bridge tab, the source opens when the tab's does, after its element's loadstart:
      // mediaEvents)
      if (mediaLink.on()) mediaLink.send(element, { o: "attach", ms: mediaLink.id(this) });
      else this._setReadyState("open");
      return true;
    }
    _detach() {
      if (!this._element) return;
      if (mediaLink.on()) mediaLink.send(this._element, { o: "detach", ms:mediaLink.id(this) });
      this._element = null;
      this._duration = NaN;
      for (const sb of [...this._sourceBuffers._list]) { this._activeSourceBuffers._remove(sb); this._sourceBuffers._remove(sb); sb._ms = null; }
      this._setReadyState("closed");
    }
    // the element's HAVE_METADATA once every source buffer has its initialization segment
    _initializationSegmentReceived() {
      const el = this._element;
      if (mediaLink.on() || !el || this._metadata || !this._sourceBuffers._list.length || this._sourceBuffers._list.some(b => !b._appends)) return;
      this._metadata = true;
      el._readyState = 1;
      queueEvent(el, "loadedmetadata");
    }
  }
  define(global, "MediaSource", MediaSource);
  define(global, "SourceBuffer", SourceBuffer);
  define(global, "SourceBufferList", SourceBufferList);
  // URL.createObjectURL (URLMainThread and URLWorker; BlobURLProtocolHandler's blob:<origin>/<uuid>).
  // A MediaSource's URL stays in this window: a media element whose src is set to it attaches the
  // source. A Blob's goes in the process's table (__mrtBlobURL, a copy of its bytes), where fetch, XHR
  // and the workers' script loaders of every thread find it; a worker's is revoked when it ends.
  const objectURLs = new Map();
  define(URL, "createObjectURL", function createObjectURL(obj) {
    if (arguments.length < 1) throw new TypeError("URL.createObjectURL: At least 1 argument required, but only 0 passed");
    const impl = blobImplOf(obj);
    if (!impl && !(obj instanceof MediaSource)) throw new TypeError("URL.createObjectURL: Argument 1 could not be converted to any of: Blob, MediaSource.");
    const url = `blob:${global.origin}/${crypto.randomUUID()}`;
    if (impl) __mrtBlobURL(0, url, readBlobImpl(impl), impl.contentType, global.origin, blobURLOwner, clockNow());
    else objectURLs.set(url, obj);
    return url;
  });
  // URL::RevokeObjectURL: only the origin that made it can (RemoveDataEntry's Subsumes)
  define(URL, "revokeObjectURL", function revokeObjectURL(url) {
    if (arguments.length < 1) throw new TypeError("URL.revokeObjectURL: At least 1 argument required, but only 0 passed");
    url = String(url);
    if (!objectURLs.delete(url)) __mrtBlobURL(1, url, global.origin, clockNow());
  });
  if (!isWorker) {
    let proto = HTMLMediaElement.prototype, desc;
    while (proto && !(desc = Object.getOwnPropertyDescriptor(proto, "src"))) proto = Object.getPrototypeOf(proto);
    if (desc) {
      Object.defineProperty(HTMLMediaElement.prototype, "src", {
        configurable: true, enumerable: true,
        get() { return desc.get.call(this); },
        set(v) {
          desc.set.call(this, v);
          this._error = null;
          if (this._mediaSource) { this._mediaSource._detach(); this._mediaSource = null; this._readyState = 0; this._duration = NaN; }
          const ms = objectURLs.get(String(v));
          // a runtime Blob's bytes, taken as the load starts (Gecko's media channel holds the
          // BlobImpl from then: a revoke after does not stop it), for the HTTP viewer (GAPS B172)
          this._mediaBlob = null;
          if (env.httpViewer && !ms && isBlobURL(String(v))) {
            const entry = __mrtBlobURL(2, String(v), global.origin, true, clockNow());
            if (entry) this._mediaBlob = { url: String(v), entry };
          }
          if (ms) {
            // (in the tab, the element's own loadstart comes back)
            if (!mediaLink.on()) queueEvent(this, "loadstart");
            if (ms._attach(this)) this._mediaSource = ms;
          }
        },
      });
    }
  }
  // (with the bridge tab as the screen, the state is the tab's element's as of its last media event,
  // and what the page set since: mediaEvents; currentTime moves on from it while playing)
  const mediaState = { paused: true, ended: false, seeking: false, readyState: 0, networkState: 0, duration: NaN,
                       currentTime: 0, volume: 1, muted: false, playbackRate: 1, defaultPlaybackRate: 1, buffered: null,
                       seekable: null, played: null };
  const MEDIA_FORWARDED = new Set(["currentTime", "volume", "muted", "playbackRate", "defaultPlaybackRate"]);
  if (!isWorker) for (const [name, value] of Object.entries(mediaState)) {
    Object.defineProperty(HTMLMediaElement.prototype, name, {
      configurable: true,
      get() {
        if (!isMedia(this)) return undefined;
        if (name === "muted") return mediaLink.on() && "_muted" in this ? this._muted : this.hasAttribute("muted") || !!this._muted;
        if (name === "buffered" || name === "seekable" || name === "played") return new TimeRanges(this["_" + name] || []);
        if (name === "currentTime" && mediaLink.on()) return mediaLink.currentTime(this);
        return "_" + name in this ? this["_" + name] : value;
      },
      set(v) {
        if (isMedia(this) && mediaLink.on() && MEDIA_FORWARDED.has(name)) mediaLink.set(this, name, v);
        else if (isMedia(this)) this["_" + name] = v;
        else Object.defineProperty(this, name, { value: v, writable: true, configurable: true, enumerable: true });
      },
    });
  }
  // the element's MediaError (one comes only from the tab) and a video's size (0 until the tab's
  // element has its metadata)
  if (!isWorker) {
    Object.defineProperty(HTMLMediaElement.prototype, "error", {
      configurable: true, enumerable: true, get() { return isMedia(this) ? this._error || null : undefined; },
    });
    if (global.HTMLVideoElement) for (const name of ["videoWidth", "videoHeight"]) {
      Object.defineProperty(global.HTMLVideoElement.prototype, name, {
        configurable: true, enumerable: true, get() { return this.tagName === "VIDEO" ? this["_" + name] || 0 : undefined; },
      });
    }
  }
  class PerformanceObserver { constructor() {} observe() {} disconnect() {} takeRecords() { return []; } }
  PerformanceObserver.supportedEntryTypes = [];
  define(global, "PerformanceObserver", PerformanceObserver);
  Object.assign(performance, {
    timeOrigin: Date.now(),
    timing: { navigationStart: Date.now(), responseStart: Date.now(), domLoading: Date.now(), domContentLoadedEventEnd: 0, loadEventEnd: 0 },
    // (a navigated document's type: 1 reload, 2 back or forward)
    navigation: { type: (!isFrameWindow && env.navigation && env.navigation.type) || 0, redirectCount: 0 },
    getEntries: () => [], getEntriesByType: () => [], getEntriesByName: () => [],
    mark() {}, measure() {}, clearMarks() {}, clearMeasures() {}, clearResourceTimings() {}, setResourceTimingBufferSize() {}, toJSON() { return {}; },
  });
  define(global, "requestIdleCallback", cb => setTimeout(() => cb({ didTimeout: false, timeRemaining: () => 50 }), 1));
  define(global, "cancelIdleCallback", id => clearTimeout(id));

  // custom elements: window.customElements is native (glue/mrt_js.cpp, Gecko's CustomElementRegistry)

  // history: the History interface (History.webidl) over this document's session entries, as Gecko
  // keeps same-document ones: pushState adds an entry after the current one (dropping those that
  // followed it), replaceState changes the current one, and back/forward/go move to another on a
  // queued task that sets the URL and fires popstate with its state. A state is kept as a structured
  // clone. go(0) does nothing. A navigated document (DESIGN "Top-level navigation in the HTTP
  // viewer") starts with the tab's entries from the driver, those of other documents marked
  // `other`: going to one of them is a navigation that loads its URL again (navigateTo `traverse`).
  const historyState = { entries: null, index: 0, scrollRestoration: "auto" };
  const saveHistoryScroll = () => {
    historyEntries()[historyState.index].scroll = [rootScroll.x, rootScroll.y];
  };
  const historyEntries = () => {
    if (historyState.entries) return historyState.entries;
    const given = !isFrameWindow && env.navigation && env.navigation.history;
    if (!given) return (historyState.entries = [{ url: pageURL.href, state: null }]);
    historyState.index = given.index;
    return (historyState.entries = given.entries.map((url, i) => (i === given.index ? { url: pageURL.href, state: null } : { url, state: null, other: true })));
  };
  const cloneState = state => (state === undefined || state === null ? null : global.structuredClone(state));
  const setEntry = (entry, state, u) => {
    const clone = cloneState(state);
    const url = u != null ? new URL(resolve(u, pageURL.href)) : pageURL;
    // Gecko's nsDocShell::UpdateURLAndHistory refuses another origin's URL
    if (url.origin !== pageURL.origin) throw new DOMException(`Cannot use '${url.href}' as a URL from '${pageURL.href}'.`, "SecurityError");
    entry.state = clone;
    pageURL = url;
    entry.url = url.href;
  };
  const History = { History: function () { throw new TypeError("Illegal constructor."); } }.History;
  for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors({
    get length() { return historyEntries().length; },
    get scrollRestoration() { return historyState.scrollRestoration; },
    set scrollRestoration(v) { v = String(v); if (v === "auto" || v === "manual") historyState.scrollRestoration = v; },
    get state() { return historyEntries()[historyState.index].state; },
    go(delta = 0) {
      delta = Math.trunc(Number(delta)) || 0;
      const to = historyState.index + delta;
      if (delta === 0 || to < 0 || to >= historyEntries().length) return;
      if (historyEntries()[to].other) { navigateTo(historyEntries()[to].url, { traverse: to }); return; }
      global.__mrt.queueTask(() => {
        const entries = historyEntries();
        if (to >= entries.length || entries[to].other) return;
        const from = entries[historyState.index];
        saveHistoryScroll();
        historyState.index = to;
        pageURL = new URL(entries[to].url);
        // (the runtime's events have no isTrusted, GAPS B72; these are the browser's)
        global.dispatchEvent(Object.assign(new PopStateEvent("popstate", { state: entries[to].state }), { isTrusted: true }));
        if (historyState.scrollRestoration === "auto") {
          if (entries[to].scroll) pageScrollTo(...entries[to].scroll);
          else scrollToFragment(pageURL);
        }
        const a = new URL(from.url), b = new URL(entries[to].url);
        a.hash = b.hash = "";
        if (a.href === b.href && from.url !== entries[to].url) {
          global.dispatchEvent(Object.assign(new HashChangeEvent("hashchange", { oldURL: from.url, newURL: entries[to].url }), { isTrusted: true }));
        }
      });
    },
    back() { this.go(-1); },
    forward() { this.go(1); },
    pushState(state, t, u) {
      const entries = historyEntries();
      const entry = { url: pageURL.href, state: null };
      setEntry(entry, state, u);
      saveHistoryScroll();
      entries.splice(historyState.index + 1, entries.length, entry);
      historyState.index = entries.length - 1;
    },
    replaceState(state, t, u) { setEntry(historyEntries()[historyState.index], state, u); },
  }))) Object.defineProperty(History.prototype, key, { ...desc, enumerable: true });
  Object.defineProperty(History.prototype, Symbol.toStringTag, { value: "History", configurable: true });
  define(global, "History", History);
  const pageHistory = Object.create(History.prototype);
  const traverseHistory = History.prototype.go;
  define(global, "history", pageHistory);

  // ---- navigation of the top-level document (DESIGN "Top-level navigation in the HTTP viewer", GAPS B151) ----
  //
  // A runtime's document cannot change, so a navigation to another document is the driver's: it
  // loads the destination into a new runtime and, once that one is ready, the viewer shows it and
  // this runtime unloads its document and ends (mini_realsites.py `navigate`). A URL that differs
  // from the document's only in its fragment stays in this document. As in Gecko a navigation starts
  // in a task after the script that asked for it, and a later one before that task replaces it. Only
  // the page's own window navigates, and to another document only in the HTTP viewer: a frame window
  // warns as before (GAPS B52).
  // (`input`: the owner's input is being dispatched, Gecko's transient user activation: a navigation
  // the page starts then is the user's, Sec-Fetch-User)
  const navigation = { pending: null, queued: false, input: 0 };
  const navigable = !isWorker && !isFrameWindow;
  // The document's base URL: its first <base href>, else its URL (Document::GetDocBaseURI)
  function documentBase() {
    const base = global.document && global.document.querySelector("base[href]");
    if (base) { try { return new URL(resolve(base.getAttribute("href"), pageURL.href)).href; } catch (e) {} }
    return pageURL.href;
  }
  // A navigation that is not made: the console says why, the input record keeps it, and the driver
  // shows it in the viewer (an op that only records)
  function refuseNavigation(url, why) {
    console.warn("navigation not made:", url, "(" + why + ")");
    input.unhandled.push(why + " " + url);
    try { bridge("navigate", { url, refused: why }); } catch (e) {}
  }
  // navigate(): `opts` {replace, reload, traverse, method, body, contentType, target, noReferrer,
  // referrerPolicy, user, link} (link: from a hyperlink, whose bad URL is ignored, not thrown)
  function navigateTo(u, opts) {
    if (!navigable) { console.warn("navigation is not supported:", u); return; }
    let url;
    try { url = new URL(resolve(u, opts.base || documentBase())); }
    catch (e) {
      if (opts.link) return;
      throw new DOMException(`Location.href: '${u}' is not a valid URL.`, "SyntaxError");
    }
    // a javascript: URL's script is not run (its result could replace the document); the usual
    // placeholders, whose result is undefined, do nothing in Firefox either
    if (url.protocol === "javascript:") {
      let code = url.href.slice("javascript:".length);
      try { code = decodeURIComponent(code); } catch (e) {}
      if (/^\s*(void\s*\(?\s*0\s*\)?|undefined|)\s*;?\s*$/.test(code)) return;
      return refuseNavigation(url.href, "javascript: URL not run");
    }
    const target = (opts.target || "").toLowerCase();
    // a new window: refused, as Firefox refuses a blocked popup (window.open gives null; GAPS B188)
    if (target && target !== "_self" && target !== "_top" && target !== "_parent") return refuseNavigation(url.href, "new window " + target);
    const noFragment = h => h.replace(/#.*$/s, "");
    if (!opts.reload && opts.traverse === undefined && (opts.method || "GET") === "GET" && url.href.includes("#") &&
        noFragment(url.href) === noFragment(pageURL.href)) {
      navigation.pending = null;
      return navigateFragment(url, !!opts.replace || url.href === pageURL.href);
    }
    // another document: only the HTTP viewer's driver loads one; elsewhere (the bridge tab, a
    // measurement) the document stays, with no beforeunload, as before (GAPS B151)
    if (!env.httpViewer) {
      console.warn("navigation is not supported:", url.href);
      input.unhandled.push("navigation " + url.href);
      return;
    }
    navigation.pending = { url: url.href, replace: !!opts.replace, reload: !!opts.reload, traverse: opts.traverse,
      method: opts.method || "GET", body: opts.body, contentType: opts.contentType, noReferrer: !!opts.noReferrer,
      referrerPolicy: opts.referrerPolicy || "", user: !!opts.user || navigation.input > 0 };
    if (!navigation.queued) { navigation.queued = true; global.__mrt.queueTask(startNavigation); }
  }
  // nsDocShell::ScrollToAnchor and nsContentUtils::GetTargetElement: try the literal fragment,
  // then UTF-8 percent-decoding, with an ID taking precedence over a legacy HTML named anchor.
  // Use the root-scroll transport shared with scrollIntoView, including reconnect snapshots.
  function scrollToFragment(url) {
    if (isWorker || isFrameWindow || !url.href.includes("#")) return false;
    const raw = url.href.slice(url.href.indexOf("#") + 1);
    if (!raw) { pageScrollTo(0, 0); return true; }
    const decoded = raw.replace(/(?:%[0-9a-f]{2})+/gi, part =>
      new TextDecoder().decode(new Uint8Array(part.slice(1).split("%").map(v => parseInt(v, 16)))));
    for (const name of raw === decoded ? [raw] : [raw, decoded]) {
      const target = document.getElementById(name) || [...document.getElementsByTagName("a")].find(el =>
        el.namespaceURI === "http://www.w3.org/1999/xhtml" && el.getAttribute("name") === name);
      if (target) {
        // Invoke our implementation directly, rather than a page's override of scrollIntoView.
        if (__mrtBorderBox(target) !== null) {
          const r = target.getBoundingClientRect(), v = viewportSize();
          let x = rootScroll.x;
          if (r.left < 0) x += r.left;
          else if (r.right > v.width) x += r.right - v.width;
          pageScrollTo(x, rootScroll.y + r.top, n => Math.ceil(n - 0.5));
        }
        return true;
      }
      if (/^top$/i.test(name)) { pageScrollTo(0, 0); return true; }
    }
    return false;
  }
  let initialFragmentScrolled = false;
  function scrollInitialFragment() {
    if (!initialFragmentScrolled && rootScroll.seq === 0) initialFragmentScrolled = scrollToFragment(pageURL);
  }
  // The fragment navigation: history and URL update synchronously, followed by the target scroll
  // and, when the fragment changed, hashchange in a task. Repeated links still scroll.
  function navigateFragment(url, replace) {
    const entries = historyEntries(), old = pageURL.href;
    saveHistoryScroll();
    if (replace) entries[historyState.index] = { url: url.href, state: null };
    else {
      entries.splice(historyState.index + 1, entries.length, { url: url.href, state: null });
      historyState.index = entries.length - 1;
    }
    pageURL = url;
    scrollToFragment(url);
    if (old !== url.href) {
      global.__mrt.queueTask(() => {
        global.dispatchEvent(Object.assign(new HashChangeEvent("hashchange", { oldURL: old, newURL: url.href }), { isTrusted: true }));
      });
    }
  }
  // The navigation's start: beforeunload, then the driver's load of the destination; on commit the
  // document unloads (pagehide, unload) and the driver ends this runtime
  function startNavigation() {
    navigation.queued = false;
    const nav = navigation.pending;
    navigation.pending = null;
    if (!nav) return;
    // (no "Leave page?" prompt: a canceled beforeunload stops the navigation, and the viewer says so)
    const before = Object.assign(new Event("beforeunload", { cancelable: true }), { isTrusted: true, returnValue: "" });
    global.dispatchEvent(before);
    if (before.defaultPrevented || (typeof before.returnValue === "string" && before.returnValue !== "")) {
      return refuseNavigation(nav.url, "beforeunload canceled");
    }
    const entries = historyEntries();
    const items = __mrtSessionStorage(pageURL.origin, "names").map(k => [k, __mrtSessionStorage(pageURL.origin, "getItem", k)]);
    let reply;
    try {
      reply = bridge("navigate", Object.assign({}, nav, { origin: global.origin, referrer: nav.noReferrer ? "" : referrerURL(),
        metaReferrer: docMetaReferrer(), history: { entries: entries.map(e => e.url), index: historyState.index },
        sessionStorage: { origin: pageURL.origin, items } }));
    } catch (e) {
      console.warn("navigation failed:", nav.url, String(e.message).replace(/^navigate: /, ""));
      return;
    }
    if (!reply || !reply.committed) return;
    // the old document's unloading (nsDocumentViewer::PageHide, then Unload); a listener's
    // exception is reported by dispatchEvent
    global.dispatchEvent(Object.assign(new PageTransitionEvent("pagehide", { persisted: false }), { isTrusted: true }));
    global.dispatchEvent(Object.assign(new Event("unload"), { isTrusted: true }));
    bridge("navigateDone", {});
  }
  // window.stop() cancels a navigation that has not started
  if (navigable) {
    const stop = global.stop;
    define(global, "stop", function () { navigation.pending = null; if (typeof stop === "function") return stop.call(this); });
  }
  // ---- hyperlinks and form submission (HTMLAnchorElement's and HTMLFormElement's activation) ----
  const isHTMLElement = n => n && n.nodeType === 1 && n.namespaceURI === "http://www.w3.org/1999/xhtml";
  const isLink = n => isHTMLElement(n) && (n.localName === "a" || n.localName === "area") && n.hasAttribute("href");
  const isSubmitButton = n => isHTMLElement(n) && ((n.localName === "button" && !["reset", "button"].includes(typeOf(n))) ||
    (n.localName === "input" && ["submit", "image"].includes(typeOf(n))));
  // A control's form owner: its form attribute's element, else its nearest form ancestor
  function formOwner(el) {
    if (el.hasAttribute("form")) {
      const f = global.document.getElementById(el.getAttribute("form"));
      return f && f.localName === "form" ? f : null;
    }
    return el.closest("form");
  }
  // The anchor's activation (HTMLAnchorElement::PostHandleEvent, nsContentUtils::TriggerLink)
  function followHyperlink(link, user, newWindow) {
    const href = link.getAttribute("href");
    if (link.hasAttribute("download")) return refuseNavigation(href, "download not supported");
    let target = link.getAttribute("target");
    if (target === null) {
      const base = global.document.querySelector("base[target]");
      target = base ? base.getAttribute("target") : "";
    }
    const rel = (link.getAttribute("rel") || "").toLowerCase().split(/[\t\n\f\r ]+/);
    navigateTo(href, { target: newWindow ? "_blank" : target, noReferrer: rel.includes("noreferrer"),
      referrerPolicy: link.getAttribute("referrerpolicy") || "", user, link: true });
  }
  // The click's activation target: the first link or submit button from its target up, across
  // shadow roots (EventDispatcher's activation, as for the elements whose default action the runtime
  // makes; the event's path is gone once it is dispatched). `buttons`: submit buttons too (the HTTP
  // viewer's frame has no forms; the bridge tab fires submit itself).
  function activate(target, e, user, buttons) {
    if (!navigable || e.defaultPrevented || e.button !== 0) return;
    let el = target;
    while (el && !(isLink(el) || (buttons && isSubmitButton(el)))) el = el.parentNode || el.host || null;
    if (!el) return;
    if (isLink(el)) return followHyperlink(el, user, e.ctrlKey || e.shiftKey || e.metaKey);
    if (el.hasAttribute("disabled")) return;
    const form = formOwner(el);
    if (form) submitForm(form, el, { request: true, user });
  }
  // Implicit submission (HTMLInputElement's Enter keypress, nsIFormControl::MaybeSubmitForm): the
  // form's default button is clicked, or with none and at most one field that blocks it, submitted
  const IMPLICIT_TYPES = new Set(["", "text", "search", "url", "tel", "email", "password", "date", "month", "week", "time", "datetime-local", "number"]);
  function implicitSubmit(el, e) {
    if (!navigable || e.defaultPrevented || e.key !== "Enter" || !isHTMLElement(el) || el.localName !== "input" || !IMPLICIT_TYPES.has(typeOf(el))) return;
    const form = formOwner(el);
    if (!form) return;
    const controls = Array.from(global.document.querySelectorAll("input, button")).filter(c => formOwner(c) === form);
    const button = controls.find(isSubmitButton);
    if (button) {
      if (button.hasAttribute("disabled")) return;
      const click = Object.assign(new MouseEvent("click", { bubbles: true, cancelable: true, composed: true, view: global }), { isTrusted: true });
      button.dispatchEvent(click);
      activate(button, click, true, true);
    } else if (controls.filter(c => c.localName === "input" && IMPLICIT_TYPES.has(typeOf(c))).length <= 1) {
      submitForm(form, null, { request: true, user: true });
    }
  }
  // The form's entry list (constructing the entry list; Gecko's HTMLFormElement::ConstructEntryList):
  // in tree order, each enabled control with a name, a checkbox or radio only when checked, the
  // submitter alone of the buttons, an image button as name.x and name.y (0, 0), a file input as an
  // empty file (no files are picked here)
  function entryList(form, submitter) {
    const out = [];
    for (const c of global.document.querySelectorAll("input, select, textarea, button")) {
      if (formOwner(c) !== form || c.closest("datalist")) continue;
      if (c.hasAttribute("disabled") || (c.closest("fieldset[disabled]") && !c.closest("legend"))) continue;
      const t = typeOf(c), name = c.getAttribute("name");
      const button = c.localName === "button" || (c.localName === "input" && ["submit", "image", "reset", "button"].includes(t));
      if (button && c !== submitter) continue;
      if (c.localName === "input" && t === "image") {
        const prefix = name ? name + "." : "";
        out.push([prefix + "x", "0"], [prefix + "y", "0"]);
        continue;
      }
      if (!name) continue;
      if (c.localName === "input" && (t === "checkbox" || t === "radio") && !c.checked) continue;
      if (c.localName === "select") {
        for (const o of c.querySelectorAll("option")) if (o.selected && !o.hasAttribute("disabled")) out.push([name, o.value]);
      } else if (c.localName === "input" && t === "file") out.push([name, { file: "" }]);
      else out.push([name, String(c.value).replace(/\r\n?|\n/g, "\r\n")]);
    }
    return out;
  }
  // A required control that is empty, with the value the owner typed (GAPS B27: the other checks are
  // not made); `invalid` fires at each, and the first stops the submission
  function invalidControls(form) {
    const bad = [];
    for (const c of global.document.querySelectorAll("input, select, textarea")) {
      if (formOwner(c) !== form || !c.hasAttribute("required") || !willValidate(c)) continue;
      const t = typeOf(c);
      const missing = c.localName === "input" && (t === "checkbox" || t === "radio") ? !c.checked
        : c.localName === "select" ? !Array.from(c.querySelectorAll("option")).some(o => o.selected && o.value !== "") : c.value === "";
      if (missing) bad.push(c);
    }
    for (const c of bad) c.dispatchEvent(Object.assign(new Event("invalid", { cancelable: true }), { isTrusted: true }));
    return bad;
  }
  // Form submission (HTMLFormElement::SubmitSubmission): `request` is requestSubmit's path (validity,
  // then the submit event), else form.submit()'s or a submit event that was not canceled
  function submitForm(form, submitter, { request, user }) {
    const attr = (name, formName) => (submitter && submitter.hasAttribute(formName) ? submitter.getAttribute(formName) : form.getAttribute(name));
    if (request) {
      const novalidate = form.hasAttribute("novalidate") || (submitter && submitter.hasAttribute("formnovalidate"));
      if (!novalidate && invalidControls(form).length) return;
      const submit = Object.assign(new SubmitEvent("submit", { bubbles: true, cancelable: true, submitter }), { isTrusted: !!user });
      if (!form.dispatchEvent(submit)) return;
    }
    if (!navigable) return;
    const action = attr("action", "formaction") || pageURL.href;
    const method = (attr("method", "formmethod") || "get").toLowerCase();
    let enctype = (attr("enctype", "formenctype") || "").toLowerCase();
    if (!["multipart/form-data", "text/plain"].includes(enctype)) enctype = "application/x-www-form-urlencoded";
    let target = attr("target", "formtarget");
    if (target === null) {
      const base = global.document.querySelector("base[target]");
      target = base ? base.getAttribute("target") : "";
    }
    let url;
    try { url = new URL(resolve(action, documentBase())); } catch (e) { return; }
    if (method === "dialog") return refuseNavigation(url.href, "form method dialog not supported");
    if (url.protocol !== "http:" && url.protocol !== "https:") return refuseNavigation(url.href, "form action scheme " + url.protocol);
    const entries = entryList(form, submitter);
    const rel = (form.getAttribute("rel") || "").toLowerCase().split(/[\t\n\f\r ]+/);
    const common = { target, user, noReferrer: rel.includes("noreferrer") };
    if (method !== "post") {
      const query = entries.map(([k, v]) => formEncode(k) + "=" + formEncode(typeof v === "string" ? v : "")).join("&");
      return navigateTo(url.href.replace(/[?#].*$/s, "") + "?" + query + url.hash, common);
    }
    let body, contentType = enctype;
    if (enctype === "multipart/form-data") {
      const boundary = "----geckoformboundary" + Array.from({ length: 16 }, () => Math.floor(Math.random() * 16).toString(16)).join("");
      const quote = v => v.replace(/"/g, "%22").replace(/\r/g, "%0D").replace(/\n/g, "%0A");
      body = entries.map(([k, v]) => `--${boundary}\r\nContent-Disposition: form-data; name="${quote(k)}"` +
        (typeof v === "string" ? `\r\n\r\n${v}\r\n` : `; filename="${quote(v.file)}"\r\nContent-Type: application/octet-stream\r\n\r\n\r\n`)).join("") + `--${boundary}--\r\n`;
      contentType += "; boundary=" + boundary;
    } else if (enctype === "text/plain") body = entries.map(([k, v]) => `${k}=${typeof v === "string" ? v : ""}\r\n`).join("");
    else body = entries.map(([k, v]) => formEncode(k) + "=" + formEncode(typeof v === "string" ? v : "")).join("&");
    navigateTo(url.href, Object.assign(common, { method: "POST", body, contentType }));
  }
  if (!isWorker) {
    define(HTMLFormElement.prototype, "submit", function submit() { submitForm(this, null, { request: false, user: false }); });
    define(HTMLFormElement.prototype, "requestSubmit", function requestSubmit(submitter) {
      if (submitter !== undefined && submitter !== null) {
        if (!isSubmitButton(submitter)) throw new TypeError("HTMLFormElement.requestSubmit: The submitter is not a submit button.");
        if (formOwner(submitter) !== this) throw new DOMException("The submitter is not owned by this form.", "NotFoundError");
      }
      submitForm(this, submitter || null, { request: true, user: false });
    });
  }

  // ---- Web Crypto, forwarded to Firefox (SPEC decision 21; DESIGN "Web Crypto: keys kept in the runtime") ----
  //
  // Every SubtleCrypto operation is done by the bridge tab's real SubtleCrypto (bench/crypto_bridge.js,
  // sandbox "crypto"), so Firefox normalizes the algorithms, checks the keys and raises its own errors.
  // Kept here, as Gecko's bindings and CryptoKey: the arguments' WebIDL conversions, each key's type,
  // extractable, algorithm and usages, and its material, so that dropping the bridge loses no key:
  // the data the page imported, or, for a key Firefox made (generateKey, deriveKey, unwrapKey), the
  // export the bridge tab made of it, made extractable there for this. Exporting or wrapping a key the
  // page made non-extractable is refused here, with Gecko's checks in Gecko's order. A request names
  // the keys it uses by id; one the bridge tab does not have is imported again from its material.
  // Not built: a CryptoKey through the structured clone (postMessage, IndexedDB: DataCloneError, GAPS
  // B68), and the private copies of non-extractable keys read from IndexedDB, written at hibernate.
  const keyStates = new WeakMap();
  let cryptoToken = null;
  const newKeyId = () => {
    // ids unique to this runtime: the bridge tab's sandbox may serve several
    if (cryptoToken === null) cryptoToken = share("cryptoToken", () => bridge("crypto", { call: "token" }));
    const n = shared.cryptoNextKey = (shared.cryptoNextKey || 0) + 1;
    return `${cryptoToken}-${n}`;
  };
  const Crypto = { Crypto: function () { throw new TypeError("Illegal constructor."); } }.Crypto;
  const SubtleCrypto = { SubtleCrypto: function () { throw new TypeError("Illegal constructor."); } }.SubtleCrypto;
  const CryptoKey = { CryptoKey: function () { throw new TypeError("Illegal constructor."); } }.CryptoKey;
  const keyState = (k, member) => {
    const st = keyStates.get(k);
    if (!st) throw new TypeError(`'${member}' called on an object that does not implement interface CryptoKey.`);
    return st;
  };
  for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors({
    get type() { return keyState(this, "get type").type; },
    get extractable() { return keyState(this, "get extractable").extractable; },
    // [Cached]: the same object every time; usages also [Frozen]
    get algorithm() { return keyState(this, "get algorithm").algorithm; },
    get usages() { return keyState(this, "get usages").usages; },
  }))) Object.defineProperty(CryptoKey.prototype, key, { ...desc, enumerable: true });
  Object.defineProperty(CryptoKey.prototype, Symbol.toStringTag, { value: "CryptoKey", configurable: true });

  // A value for the bridge (crypto_bridge.js has the other half). An object crosses as the members a
  // dictionary of SubtleCrypto.webidl may read, in order; `ids` collects the keys it refers to.
  const DICT_MEMBERS = ["additionalData", "alg", "counter", "crv", "d", "dp", "dq", "e", "ext", "generator", "hash", "info",
    "iterations", "iv", "k", "key_ops", "kty", "label", "length", "modulusLength", "n", "name", "namedCurve", "oth", "p",
    "prime", "public", "publicExponent", "q", "qi", "r", "salt", "saltLength", "t", "tagLength", "use", "x", "y"];
  const isBufferSource = v => v instanceof ArrayBuffer || ArrayBuffer.isView(v);
  const encodeBytes = v => {
    const u8 = v instanceof ArrayBuffer ? new Uint8Array(v) : new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
    return { $b: toBase64(u8), $t: v instanceof ArrayBuffer ? "ArrayBuffer" : Object.prototype.toString.call(v).slice(8, -1) };
  };
  function encodeCryptoValue(v, ids) {
    if (v === undefined) return { $u: 1 };
    if (typeof v === "symbol") throw new TypeError("can't convert symbol to string");
    if (typeof v === "bigint") return String(v);
    if (v === null || (typeof v !== "object" && typeof v !== "function")) return v;
    if (keyStates.has(v)) { const st = keyStates.get(v); ids.push(st); return { $k: st.id }; }
    if (isBufferSource(v)) return encodeBytes(v);
    if (Array.isArray(v)) return Array.from(v, x => encodeCryptoValue(x, ids));
    const members = [];
    for (const name of DICT_MEMBERS) {
      const x = v[name];
      if (x !== undefined) members.push([name, encodeCryptoValue(x, ids)]);
    }
    return { $o: members };
  }
  function decodeCryptoValue(v, extractable) {
    if (v === null || typeof v !== "object") return v;
    if (Array.isArray(v)) return v.map(x => decodeCryptoValue(x, extractable));
    if ("$u" in v) return undefined;
    if ("$b" in v) {
      const u8 = fromBase64(v.$b);
      if (v.$t === "ArrayBuffer") return u8.buffer;
      if (v.$t === "DataView") return new DataView(u8.buffer);
      return new global[v.$t](u8.buffer, 0, u8.byteLength / global[v.$t].BYTES_PER_ELEMENT);
    }
    if ("$key" in v) return makeCryptoKey(v.$key, extractable);
    const o = {};
    for (const [k, x] of v.$o) o[k] = decodeCryptoValue(x, extractable);
    return o;
  }
  // a CryptoKey for a key Firefox holds; `extractable(type)` is what the page asked for (undefined:
  // Firefox's own answer, for an imported key)
  function makeCryptoKey(d, extractable) {
    const k = Object.create(CryptoKey.prototype);
    const own = extractable && extractable(d.type);
    keyStates.set(k, { id: d.id, type: d.type, extractable: own === undefined ? d.extractable : own,
                       algorithm: decodeCryptoValue(d.algorithm), usages: Object.freeze(d.usages.slice()), material: d.material });
    return k;
  }

  // One operation by the bridge tab: its reply, sending the material of the keys it lacks (`keys`:
  // the states of the keys the request uses)
  function cryptoBridge(call, args, keys, newIds) {
    const req = { call, args, keys: keys.map(st => st.id), newIds };
    let r = bridge("crypto", req);
    if (r && r.missing) {
      // counted in the report: keys imported again from their material
      calls["crypto-reimport"] = (calls["crypto-reimport"] || 0) + r.missing.length;
      req.materials = {};
      for (const st of keys) if (r.missing.includes(st.id)) req.materials[st.id] = st.material;
      r = bridge("crypto", req);
    }
    return r;
  }

  // WebIDL's conversions, as Gecko's generated binding for a Promise-returning method does them: a
  // failure rejects the promise
  const SUBTLE_BRAND = new WeakSet();
  const argError = (method, msg) => new TypeError(`SubtleCrypto.${method}: ${msg}`);
  const toKey = (method, i, v) => {
    if (!keyStates.has(v)) throw argError(method, `Argument ${i} does not implement interface CryptoKey.`);
    return v;
  };
  const toBufferSource = (method, i, v) => {
    if (!isBufferSource(v)) throw argError(method, `Argument ${i} could not be converted to any of: ArrayBufferView, ArrayBuffer.`);
    return v;
  };
  const toUsages = (method, i, v) => {
    if (v === null || (typeof v !== "object" && typeof v !== "function") || typeof v[Symbol.iterator] !== "function")
      throw argError(method, `Argument ${i} can't be converted to a sequence.`);
    return Array.from(v, String);
  };
  const toAlgorithm = v => (v !== null && (typeof v === "object" || typeof v === "function") ? v : String(v));
  const toUnsignedLong = v => { const n = Number(v); return Number.isFinite(n) ? ((Math.trunc(n) % 2 ** 32) + 2 ** 32) % 2 ** 32 : 0; };
  // Gecko's FailureTask: rejected at once, with the error's default message
  const MESSAGES = {
    SyntaxError: "An invalid or illegal string was specified",
    InvalidAccessError: "A parameter or an operation is not supported by the underlying object",
  };
  const failure = name => ({ early: true, reject: { name, message: MESSAGES[name] } });
  const KEY_FORMATS = ["raw", "spki", "pkcs8", "jwk"];

  // `convert(args, keys)` gives the request's arguments (`args`, or `encoded` already encoded), or a
  // local failure (`fail`); how many keys it may make (`newKeys`), the page's extractable for each by
  // its type (`extractable`) and an imported key's material (`material`). The operation runs at the call,
  // as Gecko starts its task there, and settles as Firefox's promise did: at once if Firefox settled it
  // at the call (a FailureTask, a binding error, a symmetric key's import), else as a task (the result
  // of the crypto thread)
  function subtleMethod(name, required, convert) {
    const f = {
      [name](...args) {
        return new Promise((resolve, reject) => {
          const settle = r => {
            const run = () => {
              if (r.reject) reject(r.reject.name === "TypeError" ? new TypeError(r.reject.message) : new DOMException(r.reject.message, r.reject.name));
              else resolve(r.value);
            };
            if (r.early) run(); else global.__mrt.queueAt(run, clockNow());
          };
          try {
            if (!SUBTLE_BRAND.has(this)) throw new TypeError(`'${name}' called on an object that does not implement interface SubtleCrypto.`);
            if (args.length < required)
              throw argError(name, `At least ${required} argument${required > 1 ? "s" : ""} required, but only ${args.length} passed`);
            const keys = [];
            const c = convert(args, keys);
            if (c.fail) return settle(c.fail);
            const encoded = c.encoded || c.args.map(a => encodeCryptoValue(a, keys));
            const newIds = Array.from({ length: c.newKeys || 0 }, newKeyId);
            const r = cryptoBridge(name, encoded, keys, newIds);
            if (r.reject) return settle(r);
            const value = decodeCryptoValue(r.result, c.extractable);
            // an imported key's material is what the page imported (Firefox's export for a key it made)
            if (c.material && keyStates.has(value)) keyStates.get(value).material = c.material;
            settle({ early: !!r.early, value });
          } catch (e) {
            settle({ early: true, reject: { name: e.name === "TypeError" ? "TypeError" : e.name, message: e.message } });
          }
        });
      },
    }[name];
    Object.defineProperty(f, "length", { value: required });
    return f;
  }
  const subtleProto = {
    encrypt: subtleMethod("encrypt", 3, a => ({ args: [toAlgorithm(a[0]), toKey("encrypt", 2, a[1]), toBufferSource("encrypt", 3, a[2])] })),
    decrypt: subtleMethod("decrypt", 3, a => ({ args: [toAlgorithm(a[0]), toKey("decrypt", 2, a[1]), toBufferSource("decrypt", 3, a[2])] })),
    sign: subtleMethod("sign", 3, a => ({ args: [toAlgorithm(a[0]), toKey("sign", 2, a[1]), toBufferSource("sign", 3, a[2])] })),
    verify: subtleMethod("verify", 4, a => ({ args: [toAlgorithm(a[0]), toKey("verify", 2, a[1]), toBufferSource("verify", 3, a[2]), toBufferSource("verify", 4, a[3])] })),
    digest: subtleMethod("digest", 2, a => ({ args: [toAlgorithm(a[0]), toBufferSource("digest", 2, a[1])] })),
    generateKey: subtleMethod("generateKey", 3, a => {
      const extractable = !!a[1];
      // a key pair's public key is always extractable (GenerateAsymmetricKeyTask)
      return { args: [toAlgorithm(a[0]), extractable, toUsages("generateKey", 3, a[2])], newKeys: 2,
               extractable: type => (type === "public" ? true : extractable) };
    }),
    deriveKey: subtleMethod("deriveKey", 5, a => {
      const extractable = !!a[3];
      return { args: [toAlgorithm(a[0]), toKey("deriveKey", 2, a[1]), toAlgorithm(a[2]), extractable, toUsages("deriveKey", 5, a[4])],
               newKeys: 1, extractable: () => extractable };
    }),
    deriveBits: subtleMethod("deriveBits", 2, a => ({
      args: [toAlgorithm(a[0]), toKey("deriveBits", 2, a[1]), a[2] === undefined || a[2] === null ? null : toUnsignedLong(a[2])],
    })),
    importKey: subtleMethod("importKey", 5, (a, keys) => {
      const format = String(a[0]);
      if (a[1] === null || (typeof a[1] !== "object" && typeof a[1] !== "function")) throw argError("importKey", "Argument 2 is not an object.");
      const keyData = a[1], algorithm = toAlgorithm(a[2]), extractable = !!a[3], usages = toUsages("importKey", 5, a[4]);
      // the runtime keeps what the page imported (DESIGN: keys the runtime can hold the material of)
      const data = encodeCryptoValue(keyData, keys), alg = encodeCryptoValue(algorithm, keys);
      return { encoded: [format, data, alg, extractable, usages], newKeys: 1, material: { format, data, algorithm: alg, extractable, usages } };
    }),
    exportKey: subtleMethod("exportKey", 2, a => {
      const format = String(a[0]), key = toKey("exportKey", 2, a[1]);
      // CreateExportKeyTask: the format, then the key's extractable, before anything Firefox checks
      if (!KEY_FORMATS.includes(format)) return { fail: failure("SyntaxError") };
      if (!keyStates.get(key).extractable) return { fail: failure("InvalidAccessError") };
      return { args: [format, key] };
    }),
    wrapKey: subtleMethod("wrapKey", 4, a => {
      const format = String(a[0]), key = toKey("wrapKey", 2, a[1]), wrappingKey = toKey("wrapKey", 3, a[2]), alg = toAlgorithm(a[3]);
      // CreateWrapKeyTask: the format, the wrapping key's usage, then the key's extractable
      if (!KEY_FORMATS.includes(format)) return { fail: failure("SyntaxError") };
      if (!keyStates.get(wrappingKey).usages.includes("wrapKey")) return { fail: failure("InvalidAccessError") };
      if (!keyStates.get(key).extractable) return { fail: failure("InvalidAccessError") };
      return { args: [format, key, wrappingKey, alg] };
    }),
    unwrapKey: subtleMethod("unwrapKey", 7, a => {
      const extractable = !!a[5];
      return { args: [String(a[0]), toBufferSource("unwrapKey", 2, a[1]), toKey("unwrapKey", 3, a[2]), toAlgorithm(a[3]), toAlgorithm(a[4]),
                      extractable, toUsages("unwrapKey", 7, a[6])], newKeys: 1, extractable: () => extractable };
    }),
  };
  for (const [key, value] of Object.entries(subtleProto))
    Object.defineProperty(SubtleCrypto.prototype, key, { value, writable: true, enumerable: true, configurable: true });
  Object.defineProperty(SubtleCrypto.prototype, Symbol.toStringTag, { value: "SubtleCrypto", configurable: true });
  const subtle = Object.create(SubtleCrypto.prototype);
  SUBTLE_BRAND.add(subtle);

  for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors({
    getRandomValues(a) { for (let i = 0; i < a.length; i++) a[i] = Math.floor(Math.random() * 2 ** (a.BYTES_PER_ELEMENT * 8)); return a; },
    // a version 4 UUID with the RFC 4122 variant (nsID::GenerateUUID): "4" at 14, one of 8, 9, a, b at 19
    randomUUID() { const h = "0123456789abcdef"; let s = ""; for (let i = 0; i < 36; i++) s += [8, 13, 18, 23].includes(i) ? "-" : i === 14 ? "4" : i === 19 ? h[8 + (Math.random() * 4 | 0)] : h[Math.random() * 16 | 0]; return s; },
    // [SecureContext, SameObject]
    get subtle() { return subtle; },
  }))) {
    if (key === "subtle" && !global.isSecureContext) continue;
    Object.defineProperty(Crypto.prototype, key, { ...desc, enumerable: true });
  }
  Object.defineProperty(Crypto.prototype, Symbol.toStringTag, { value: "Crypto", configurable: true });
  define(global, "Crypto", Crypto);
  // SubtleCrypto and CryptoKey are [SecureContext]
  if (global.isSecureContext) {
    define(global, "SubtleCrypto", SubtleCrypto);
    define(global, "CryptoKey", CryptoKey);
  }
  define(global, "crypto", Object.create(Crypto.prototype));
  // for the crypto bridge case: the bridge tab forgets every key, as when the bridge is dropped
  global.__mrt.forgetBridgeKeys = () => bridge("crypto", { call: "forget" });
  // the structured clone, through the codec postMessage and IndexedDB use
  // with StructuredSerializeOptions' transfer: its ports become new ports of this realm
  define(global, "structuredClone", function structuredClone(v, options) {
    const list = options === undefined || options === null ? [] : transferList("structuredClone", { transfer: options.transfer });
    return decodeEnvelope(encodeMessage(v, list, "local")).value;
  });
  // ---- AbortSignal, AbortController (Gecko's dom/abort) ----
  //
  // A signal's state (AbortSignalImpl and AbortSignal) lives in a map shared by the windows, so a
  // signal from another window passes the brand checks: whether it is aborted; its reason, undefined
  // when aborted without one until the `reason` getter makes the AbortError (MaybeAssignAbortError,
  // once, so every read sees the same exception); its followers, the abort algorithms run before its
  // `abort` event (the scheduler's tasks, a lock request); and for AbortSignal.any its source and
  // dependent signals. The realm's DOMException and Event are kept for what the signal makes later.
  const abortSignals = share("abortSignals", () => new WeakMap());
  const controllerSignals = share("abortControllers", () => new WeakMap());
  const isAbortSignal = s => s !== null && typeof s === "object" && abortSignals.has(s);
  const abortSignalOf = (self, member) => {
    const st = self !== null && typeof self === "object" ? abortSignals.get(self) : undefined;
    if (!st) throw new TypeError(`'${member}' called on an object that does not implement interface AbortSignal.`);
    return st;
  };
  // AbortSignal::Create: a signal with `proto` (AbortSignal's or TaskSignal's)
  const createSignal = (proto, aborted = false, reason = undefined) => {
    const s = Object.create(proto);
    abortSignals.set(s, { aborted, reason, followers: [], dependent: false, sources: [], dependents: [], DOMException, Event });
    return s;
  };
  // AbortSignalImpl::GetReason
  const reasonOf = st => {
    if (!st.aborted) return undefined;
    if (st.reason === undefined) st.reason = new st.DOMException("The operation was aborted. ", "AbortError");
    return st.reason;
  };
  // AbortFollower::Follow: `fn` runs when the signal aborts, unless the returned function (Unfollow)
  // ran first; nothing follows an aborted signal
  const followAbort = (signal, fn) => {
    const st = abortSignals.get(signal);
    if (st.aborted) return () => {};
    const follower = { fn, active: true };
    st.followers.push(follower);
    return () => {
      follower.active = false;
      const i = st.followers.indexOf(follower);
      if (i >= 0) st.followers.splice(i, 1);
    };
  };
  const dispatchSignalEvent = EventTarget.prototype.dispatchEvent;
  // AbortSignal::RunAbortSteps: the abort algorithms (a follower removed by an earlier one does not
  // run), then a trusted `abort`
  const runAbortSteps = s => {
    const st = abortSignals.get(s);
    const followers = st.followers;
    st.followers = [];
    for (const f of followers) if (f.active) { f.active = false; f.fn(); }
    const e = new st.Event("abort");
    // (the runtime's events have no isTrusted, GAPS B72; this one is the browser's)
    e.isTrusted = true;
    dispatchSignalEvent.call(s, e);
  };
  // AbortSignalImpl::SignalAbort with AbortSignal::SignalAbortWithDependents: the dependents not yet
  // aborted take the reason first, then the signal's abort steps run, then each dependent's
  const signalAbort = (s, reason) => {
    const st = abortSignals.get(s);
    if (st.aborted) return;
    st.aborted = true;
    st.reason = reason;
    const dependents = st.dependents;
    st.dependents = [];
    const toAbort = [];
    if (dependents.length > 0) {
      const r = reasonOf(st);
      for (const d of dependents) {
        const ds = abortSignals.get(d);
        if (!ds.aborted) { ds.aborted = true; ds.reason = r; toAbort.push(d); }
      }
    }
    runAbortSteps(s);
    for (const d of toAbort) runAbortSteps(d);
  };
  // AbortSignal::Any: a dependent signal (made by `create`), already aborted with the first aborted
  // one's reason, or following the signals that are not dependent and the sources of those that are
  const anySignal = (signals, create) => {
    const result = create();
    const rs = abortSignals.get(result);
    for (const s of signals) {
      const st = abortSignals.get(s);
      if (st.aborted) { rs.aborted = true; rs.reason = reasonOf(st); return result; }
    }
    rs.dependent = true;
    const dependOn = source => {
      if (!rs.sources.includes(source)) rs.sources.push(source);
      const ss = abortSignals.get(source);
      if (!ss.dependents.includes(result)) ss.dependents.push(result);
    };
    for (const s of signals) {
      const st = abortSignals.get(s);
      if (!st.dependent) dependOn(s);
      else for (const source of st.sources) dependOn(source);
    }
    return result;
  };
  // sequence<AbortSignal>'s conversion, Gecko's binding messages
  const signalSequence = (v, method) => {
    if (v === null || typeof v !== "object" || typeof v[Symbol.iterator] !== "function") throw new TypeError(`${method}: Argument 1 can't be converted to a sequence.`);
    const list = [];
    for (const s of v) {
      if (s === null || typeof s !== "object") throw new TypeError(`${method}: Element of argument 1 is not an object.`);
      if (!isAbortSignal(s)) throw new TypeError(`${method}: Element of argument 1 does not implement interface AbortSignal.`);
      list.push(s);
    }
    return list;
  };

  const AbortSignal = function AbortSignal() { throw new TypeError("Illegal constructor."); };
  Object.setPrototypeOf(AbortSignal, global.EventTarget);
  AbortSignal.prototype = Object.create(EventTarget.prototype);
  {
    const statics = {
      // (optional any: length 0)
      abort() { return createSignal(AbortSignal.prototype, true, arguments[0]); },
      // [EnforceRange] unsigned long long; the delay is capped at int32's maximum, as Gecko's timers
      timeout(milliseconds) {
        if (arguments.length < 1) throw new TypeError("AbortSignal.timeout: At least 1 argument required, but only 0 passed");
        const n = Number(milliseconds);
        if (!Number.isFinite(n)) throw new TypeError("AbortSignal.timeout: Argument 1 is not a finite value, so is out of range for unsigned long long.");
        const ms = Math.trunc(n);
        if (ms < 0 || ms > Number.MAX_SAFE_INTEGER) throw new TypeError("AbortSignal.timeout: Argument 1 is out of range for unsigned long long.");
        const signal = createSignal(AbortSignal.prototype);
        const st = abortSignals.get(signal);
        // AbortSignalTimeoutHandler::Call
        global.__mrt.queueTimeout(() => signalAbort(signal, new st.DOMException("The operation timed out.", "TimeoutError")), Math.min(ms, 2147483647));
        return signal;
      },
      any(signals) {
        if (arguments.length < 1) throw new TypeError("AbortSignal.any: At least 1 argument required, but only 0 passed");
        return anySignal(signalSequence(signals, "AbortSignal.any"), () => createSignal(AbortSignal.prototype));
      },
    };
    for (const name of ["abort", "timeout", "any"]) Object.defineProperty(AbortSignal, name, { value: statics[name], writable: true, enumerable: true, configurable: true });
    const proto = AbortSignal.prototype;
    // the operations, then the attributes, as Gecko's bindings define them
    Object.defineProperty(proto, "throwIfAborted", { value: { throwIfAborted() {
      const st = abortSignalOf(this, "throwIfAborted");
      if (st.aborted) throw reasonOf(st);
    } }.throwIfAborted, writable: true, enumerable: true, configurable: true });
    const accessors = {
      get aborted() { return abortSignalOf(this, "get aborted").aborted; },
      get reason() { return reasonOf(abortSignalOf(this, "get reason")); },
    };
    for (const name of ["aborted", "reason"]) Object.defineProperty(proto, name, { get: Object.getOwnPropertyDescriptor(accessors, name).get, enumerable: true, configurable: true });
    const handlers = {};
    global.__mrt.defineEventHandlers(handlers, ["onabort"]);
    const inner = Object.getOwnPropertyDescriptor(handlers, "onabort");
    const onabort = Object.getOwnPropertyDescriptor({
      get onabort() { abortSignalOf(this, "get onabort"); return inner.get.call(this); },
      set onabort(value) { abortSignalOf(this, "set onabort"); inner.set.call(this, value); },
    }, "onabort");
    Object.defineProperty(proto, "onabort", { get: onabort.get, set: onabort.set, enumerable: true, configurable: true });
    Object.defineProperty(proto, "constructor", { value: AbortSignal, writable: true, enumerable: false, configurable: true });
    Object.defineProperty(proto, Symbol.toStringTag, { value: "AbortSignal", configurable: true });
    Object.defineProperty(AbortSignal, "prototype", { writable: false, enumerable: false, configurable: false });
  }

  // AbortController: its signal made with it ([SameObject]); a subclass's prototype from new.target
  const AbortController = function AbortController() {
    if (new.target === undefined) throw new TypeError("AbortController constructor: 'new' is required");
    const proto = new.target.prototype;
    const self = Object.create(proto !== null && typeof proto === "object" ? proto : AbortController.prototype);
    controllerSignals.set(self, createSignal(AbortSignal.prototype));
    return self;
  };
  AbortController.prototype = {};
  {
    const controllerOf = (self, member) => {
      const s = self !== null && typeof self === "object" ? controllerSignals.get(self) : undefined;
      if (!s) throw new TypeError(`'${member}' called on an object that does not implement interface AbortController.`);
      return s;
    };
    const proto = AbortController.prototype;
    Object.defineProperty(proto, "abort", { value: { abort() { signalAbort(controllerOf(this, "abort"), arguments[0]); } }.abort, writable: true, enumerable: true, configurable: true });
    Object.defineProperty(proto, "signal", { get: Object.getOwnPropertyDescriptor({ get signal() { return controllerOf(this, "get signal"); } }, "signal").get, enumerable: true, configurable: true });
    Object.defineProperty(proto, "constructor", { value: AbortController, writable: true, enumerable: false, configurable: true });
    Object.defineProperty(proto, Symbol.toStringTag, { value: "AbortController", configurable: true });
    Object.defineProperty(AbortController, "prototype", { writable: false, enumerable: false, configurable: false });
  }
  define(global, "AbortSignal", AbortSignal);
  define(global, "AbortController", AbortController);

  // ---- Prioritized task scheduling: scheduler, TaskController, TaskSignal (Gecko's dom/webscheduling) ----
  //
  // As WebTaskScheduler: postTask and yield put a task in a queue, one per priority and one per
  // TaskSignal whose priority it follows, and dispatch a runnable to the event loop at the priority's
  // EventQueuePriority (prelude.js's queuePriorityTask: user-blocking and a user-visible yield before
  // other tasks, background after them; in a worker all alike). A runnable does not run its own
  // task: it runs the next one of all the event loop's schedulers (GetNextTask), the highest
  // effective priority (a yield's continuation one above its priority's tasks), then the oldest.
  // A delay is a timer that dispatches the task when it fires. While a user-visible or user-blocking
  // task is queued, timeouts wait (prelude.js's webTasks). A task's callback runs with its scheduling
  // state set, which a yield inside it inherits (its signal and priority).
  {
    const PRIORITIES = ["user-blocking", "user-visible", "background"];
    // TaskPriority's conversion, Gecko's binding message; `what` names the value
    const taskPriority = (v, what, method) => {
      const s = String(v);
      if (!PRIORITIES.includes(s)) throw new TypeError(`${method}: '${s}' (value of ${what}) is not a valid value for enumeration TaskPriority.`);
      return s;
    };
    // the event loop's tasks, scheduled or delayed (all the windows' schedulers share one, a worker
    // has its own), and the next task's enqueue order (gWebTaskEnqueueOrder)
    const loop = share("webTaskLoop", () => ({ tasks: new Set(), order: 0 }));
    // HasScheduledNormalOrHighPriorityWebTasks, for prelude.js's timeouts
    const webTasks = global.__mrt.webTasks;
    if (!Object.getOwnPropertyDescriptor(webTasks, "scheduled").get) {
      Object.defineProperty(webTasks, "scheduled", { get() {
        for (const t of loop.tasks) if (t.scheduled && priorityOf(t) !== "background") return 1;
        return 0;
      } });
    }
    const signalData = share("taskSignalData", () => new WeakMap());
    const controllers = share("taskControllers", () => new WeakSet());
    const schedulerData = share("schedulerData", () => new WeakMap());
    // a task's priority: its queue's, a TaskSignal's priority when the queue follows one
    const priorityOf = t => typeof t.source === "string" ? t.source : signalData.get(t.source).priority;
    // WebTaskQueueHashKey::EffectivePriority
    const effective = t => ({ "background": 0, "user-visible": 2, "user-blocking": 4 })[priorityOf(t)] + (t.continuation ? 1 : 0);
    // WebTaskScheduler::GetEventQueuePriority, on prelude.js's scale (2 MediumHigh, 1 Normal, 0 Low)
    const eventQueuePriority = (priority, continuation) => isWorker ? 1
      : priority === "user-blocking" ? 2 : priority === "background" ? 0 : continuation ? 2 : 1;
    // GetNextTask: the scheduled task of highest effective priority, then the oldest
    const runNextTask = () => {
      let next = null;
      for (const t of loop.tasks) {
        if (!t.scheduled) continue;
        if (!next || effective(t) > effective(next) || (effective(t) === effective(next) && t.order < next.order)) next = t;
      }
      if (next) next.run();
    };
    const dispatch = t => {
      t.scheduled = true;
      global.__mrt.queuePriorityTask(runNextTask, eventQueuePriority(priorityOf(t), t.continuation));
    };

    // ---- TaskSignal : AbortSignal ----
    const TaskSignal = function TaskSignal() { throw new TypeError("Illegal constructor."); };
    Object.setPrototypeOf(TaskSignal, AbortSignal);
    Object.setPrototypeOf(TaskSignal.prototype, AbortSignal.prototype);
    Object.defineProperty(TaskSignal, "prototype", { writable: false, enumerable: false, configurable: false });
    const makeTaskSignal = (priority, dependent = false) => {
      const s = createSignal(TaskSignal.prototype);
      signalData.set(s, { priority, priorityChanging: false, dependent, source: null, dependents: [] });
      return s;
    };
    const taskSignalOf = (self, member) => {
      const d = self !== null && typeof self === "object" ? signalData.get(self) : undefined;
      if (!d) throw new TypeError(`'${member}' called on an object that does not implement interface TaskSignal.`);
      return d;
    };
    // TaskSignal::Any: a dependent signal, aborted with the first of its sources; its priority fixed,
    // or following a source TaskSignal's (that one's own source when it is dependent itself)
    Object.defineProperty(TaskSignal, "any", { value: { any(signals) {
      if (arguments.length < 1) throw new TypeError("TaskSignal.any: At least 1 argument required, but only 0 passed");
      const list = signalSequence(signals, "TaskSignal.any");
      const init = arguments[1];
      if (init !== undefined && init !== null && typeof init !== "object" && typeof init !== "function") throw new TypeError("TaskSignal.any: Argument 2 can't be converted to a dictionary.");
      let p = init === undefined || init === null ? undefined : init.priority;
      if (p === undefined) p = "user-visible";
      const sourceSignal = p !== null && typeof p === "object" && signalData.has(p) ? p : null;
      const priority = sourceSignal ? null : taskPriority(p, "TaskPriority branch of (TaskPriority or TaskSignal)", "TaskSignal.any");
      // TaskSignal::Any: AbortSignal::Any making a TaskSignal
      const result = anySignal(list, () => makeTaskSignal(priority, true));
      const d = signalData.get(result);
      if (sourceSignal) {
        let source = sourceSignal, sd = signalData.get(source);
        d.priority = sd.priority;
        // HasFixedPriority: dependent with no source
        if (!(sd.dependent && !sd.source)) {
          if (sd.dependent) { source = sd.source; sd = signalData.get(source); }
          d.source = source;
          sd.dependents.push(result);
        }
      }
      return result;
    } }.any, writable: true, enumerable: true, configurable: true });
    Object.defineProperty(TaskSignal.prototype, "priority", {
      get: Object.getOwnPropertyDescriptor({ get priority() { return taskSignalOf(this, "get priority").priority; } }, "priority").get,
      enumerable: true, configurable: true,
    });
    {
      const handlers = {};
      global.__mrt.defineEventHandlers(handlers, ["onprioritychange"]);
      const inner = Object.getOwnPropertyDescriptor(handlers, "onprioritychange");
      const accessors = {
        get onprioritychange() { taskSignalOf(this, "get onprioritychange"); return inner.get.call(this); },
        set onprioritychange(value) { taskSignalOf(this, "set onprioritychange"); inner.set.call(this, value); },
      };
      const d = Object.getOwnPropertyDescriptor(accessors, "onprioritychange");
      Object.defineProperty(TaskSignal.prototype, "onprioritychange", { get: d.get, set: d.set, enumerable: true, configurable: true });
    }
    Object.defineProperty(TaskSignal.prototype, Symbol.toStringTag, { value: "TaskSignal", configurable: true });

    // ---- TaskPriorityChangeEvent : Event ----
    const changeEvents = share("taskPriorityChangeEvents", () => new WeakMap());
    const TaskPriorityChangeEvent = function TaskPriorityChangeEvent(type, init) {
      if (new.target === undefined) throw new TypeError("TaskPriorityChangeEvent constructor: 'new' is required");
      if (arguments.length < 2) throw new TypeError(`TaskPriorityChangeEvent constructor: At least 2 arguments required, but only ${arguments.length} passed`);
      if (init === null || (typeof init !== "object" && typeof init !== "function")) {
        if (init !== undefined && init !== null) throw new TypeError("TaskPriorityChangeEvent constructor: Argument 2 can't be converted to a dictionary.");
      }
      const e = Reflect.construct(Event, [type, init ?? undefined], new.target);
      const p = init === undefined || init === null ? undefined : init.previousPriority;
      if (p === undefined) throw new TypeError("TaskPriorityChangeEvent constructor: Missing required 'previousPriority' member of TaskPriorityChangeEventInit.");
      changeEvents.set(e, taskPriority(p, "'previousPriority' member of TaskPriorityChangeEventInit", "TaskPriorityChangeEvent constructor"));
      return e;
    };
    Object.setPrototypeOf(TaskPriorityChangeEvent, Event);
    Object.setPrototypeOf(TaskPriorityChangeEvent.prototype, Event.prototype);
    Object.defineProperty(TaskPriorityChangeEvent, "prototype", { writable: false, enumerable: false, configurable: false });
    Object.defineProperty(TaskPriorityChangeEvent.prototype, "previousPriority", {
      get: Object.getOwnPropertyDescriptor({ get previousPriority() {
        if (this === null || typeof this !== "object" || !changeEvents.has(this)) throw new TypeError("'get previousPriority' called on an object that does not implement interface TaskPriorityChangeEvent.");
        return changeEvents.get(this);
      } }, "previousPriority").get,
      enumerable: true, configurable: true,
    });
    Object.defineProperty(TaskPriorityChangeEvent.prototype, Symbol.toStringTag, { value: "TaskPriorityChangeEvent", configurable: true });

    // ---- TaskController : AbortController ----
    // WebTaskController::SignalPriorityChange: NotAllowedError while the signal's priority is changing;
    // the queues following it move (their tasks are picked by the new priority), a trusted
    // `prioritychange`, then the dependent signals
    const signalPriorityChange = (signal, priority) => {
      const d = signalData.get(signal);
      if (d.priorityChanging) throw new DOMException("TaskController.setPriority: Signal's priority changing is true", "NotAllowedError");
      if (d.priority === priority) return;
      d.priorityChanging = true;
      try {
        const previousPriority = d.priority;
        d.priority = priority;
        const e = new TaskPriorityChangeEvent("prioritychange", { previousPriority });
        // (the runtime's events have no isTrusted, GAPS B72; this one is the browser's)
        e.isTrusted = true;
        signal.dispatchEvent(e);
        for (const dependent of d.dependents.slice()) signalPriorityChange(dependent, priority);
      } finally {
        d.priorityChanging = false;
      }
    };
    const TaskController = function TaskController() {
      if (new.target === undefined) throw new TypeError("TaskController constructor: 'new' is required");
      const init = arguments[0];
      if (init !== undefined && init !== null && typeof init !== "object" && typeof init !== "function") throw new TypeError("TaskController constructor: Argument 1 can't be converted to a dictionary.");
      const p = init === undefined || init === null ? undefined : init.priority;
      const priority = p === undefined ? "user-visible" : taskPriority(p, "'priority' member of TaskControllerInit", "TaskController constructor");
      const proto = new.target.prototype;
      const self = Object.create(proto !== null && typeof proto === "object" ? proto : TaskController.prototype);
      controllerSignals.set(self, makeTaskSignal(priority));
      controllers.add(self);
      return self;
    };
    Object.setPrototypeOf(TaskController, AbortController);
    Object.setPrototypeOf(TaskController.prototype, AbortController.prototype);
    Object.defineProperty(TaskController, "prototype", { writable: false, enumerable: false, configurable: false });
    Object.defineProperty(TaskController.prototype, "setPriority", { value: { setPriority(priority) {
      if (this === null || typeof this !== "object" || !controllers.has(this)) throw new TypeError("'setPriority' called on an object that does not implement interface TaskController.");
      if (arguments.length < 1) throw new TypeError("TaskController.setPriority: At least 1 argument required, but only 0 passed");
      signalPriorityChange(controllerSignals.get(this), taskPriority(priority, "argument 1", "TaskController.setPriority"));
    } }.setPriority, writable: true, enumerable: true, configurable: true });
    Object.defineProperty(TaskController.prototype, Symbol.toStringTag, { value: "TaskController", configurable: true });

    // ---- Scheduler ----
    const Scheduler = function Scheduler() { throw new TypeError("Illegal constructor."); };
    Object.defineProperty(Scheduler, "prototype", { writable: false, enumerable: false, configurable: false });
    // the scheduler's state: the scheduling state of the task running (yield's inheritance)
    const schedulerOf = (self, member) => {
      const d = self !== null && typeof self === "object" ? schedulerData.get(self) : undefined;
      if (!d) throw new TypeError(`'${member}' called on an object that does not implement interface Scheduler.`);
      return d;
    };
    const rejectWithReason = (signal, reject) => { reject(reasonOf(abortSignals.get(signal))); };
    // CreateTask: queued in the loop, following its abort source; `callback` absent for a yield
    const createTask = (sched, abortSource, source, continuation, callback, state, resolve, reject) => {
      const t = { order: ++loop.order, source, continuation, scheduled: false, settled: false };
      t.run = () => {
        loop.tasks.delete(t);
        if (t.settled) return;
        if (!callback) { t.settled = true; resolve(undefined); return; }
        const outer = sched.state;
        sched.state = state;
        let value, threw = false;
        global.__mrt.enterCallback();
        try {
          value = callback.call(undefined);
        } catch (e) {
          threw = true;
          value = e;
        }
        // The callback's microtask checkpoint, then the promise settled (WebTask::Run). The state is
        // put back first: Gecko runs each promise job with the scheduling state of when it was
        // queued (HostMakeJobCallback), which the runtime does not keep (GAPS B118), so a job the
        // page queued before this task must not inherit the task's.
        sched.state = outer;
        global.__mrt.leaveCallback();
        if (!t.settled) {
          t.settled = true;
          (threw ? reject : resolve)(value);
        }
      };
      loop.tasks.add(t);
      if (abortSource) {
        // WebTask::RunAbortAlgorithm: a no-op once the promise is settled
        followAbort(abortSource, () => {
          if (t.settled) return;
          t.settled = true;
          loop.tasks.delete(t);
          rejectWithReason(abortSource, reject);
        });
      }
      return t;
    };
    const ulonglong = (v, what) => {
      const n = Number(v);
      if (!Number.isFinite(n)) throw new TypeError(`Scheduler.postTask: ${what} is not a finite value, so is out of range for unsigned long long.`);
      const i = Math.trunc(n);
      if (i < 0 || i > Number.MAX_SAFE_INTEGER) throw new TypeError(`Scheduler.postTask: ${what} is out of range for unsigned long long.`);
      return i;
    };
    Object.defineProperty(Scheduler.prototype, "postTask", { value: { postTask(callback) {
      // a Promise-returning operation: every failure, the brand check's too, is a rejection
      let sched, delay = 0, priority, signal;
      try {
        sched = schedulerOf(this, "postTask");
        if (arguments.length < 1) throw new TypeError("Scheduler.postTask: At least 1 argument required, but only 0 passed");
        if (typeof callback !== "function") throw new TypeError(`Scheduler.postTask: Argument 1 is ${callback !== null && typeof callback === "object" ? "not callable" : "not an object"}.`);
        const options = arguments[1];
        if (options !== undefined && options !== null) {
          if (typeof options !== "object" && typeof options !== "function") throw new TypeError("Scheduler.postTask: Argument 2 can't be converted to a dictionary.");
          // SchedulerPostTaskOptions' members in name order
          const d = options.delay;
          if (d !== undefined) delay = ulonglong(d, "'delay' member of SchedulerPostTaskOptions");
          const p = options.priority;
          if (p !== undefined) priority = taskPriority(p, "'priority' member of SchedulerPostTaskOptions", "Scheduler.postTask");
          const s = options.signal;
          if (s !== undefined) {
            if (s === null || (typeof s !== "object" && typeof s !== "function")) throw new TypeError("Scheduler.postTask: 'signal' member of SchedulerPostTaskOptions is not an object.");
            if (!isAbortSignal(s)) throw new TypeError("Scheduler.postTask: 'signal' member of SchedulerPostTaskOptions does not implement interface AbortSignal.");
            signal = s;
          }
        }
      } catch (e) {
        return Promise.reject(e);
      }
      return new Promise((resolve, reject) => {
        // an aborted signal: rejected with its reason
        if (signal && abortSignals.get(signal).aborted) { rejectWithReason(signal, reject); return; }
        // the priority source: the option, else a TaskSignal's, else user-visible
        const source = priority !== undefined ? priority : signal && signalData.has(signal) ? signal : "user-visible";
        const state = { abortSource: signal || null, prioritySource: source };
        const t = createTask(sched, signal || null, source, false, callback, state, resolve, reject);
        if (delay > 0) global.__mrt.queuePriorityTask(() => { if (!t.settled) dispatch(t); }, 1, delay);
        else dispatch(t);
      });
    } }.postTask, writable: true, enumerable: true, configurable: true });
    Object.defineProperty(Scheduler.prototype, "yield", { value: { yield() {
      let sched;
      try { sched = schedulerOf(this, "yield"); } catch (e) { return Promise.reject(e); }
      return new Promise((resolve, reject) => {
        // the running task's scheduling state: its abort source and priority source
        const inherited = sched.state;
        const abortSource = inherited ? inherited.abortSource : null;
        if (abortSource && abortSource.aborted) { rejectWithReason(abortSource, reject); return; }
        const source = inherited ? inherited.prioritySource : "user-visible";
        dispatch(createTask(sched, abortSource, source, true, null, null, resolve, reject));
      });
    } }.yield, writable: true, enumerable: true, configurable: true });
    Object.defineProperty(Scheduler.prototype, Symbol.toStringTag, { value: "Scheduler", configurable: true });

    define(global, "Scheduler", Scheduler);
    define(global, "TaskController", TaskController);
    define(global, "TaskSignal", TaskSignal);
    define(global, "TaskPriorityChangeEvent", TaskPriorityChangeEvent);

    // the global's scheduler: [Replaceable, SameObject], an own accessor of a window, on
    // WorkerGlobalScope.prototype in a worker
    const scheduler = Object.create(Scheduler.prototype);
    schedulerData.set(scheduler, { state: null });
    const schedulerGlobals = share("schedulerGlobals", () => new WeakMap());
    schedulerGlobals.set(global, scheduler);
    const scope = isWorker ? "WorkerGlobalScope" : "Window";
    // undefined or null is the global, as a [Global] interface's attribute takes it
    const globalOf = (self, what) => {
      const g = self ?? global;
      if (!schedulerGlobals.has(g)) throw new TypeError(`'${what}' called on an object that does not implement interface ${scope}.`);
      return g;
    };
    Object.defineProperty(isWorker ? global.WorkerGlobalScope.prototype : global, "scheduler", {
      get: Object.getOwnPropertyDescriptor({ get scheduler() { return schedulerGlobals.get(globalOf(this, "get scheduler")); } }, "scheduler").get,
      set: Object.getOwnPropertyDescriptor({ set scheduler(value) {
        Object.defineProperty(globalOf(this, "set scheduler"), "scheduler", { value, writable: true, enumerable: true, configurable: true });
      } }, "scheduler").set,
      enumerable: true,
      configurable: true,
    });
  }

  // ---- Web Locks (Gecko's dom/locks) ----
  //
  // Gecko keeps an origin's locks in the parent process (LockManagerParent: the held locks and a queue
  // per name, shared by every window and worker of the principal), and each global reaches them
  // through an actor: a request, a release and an abort go there, a grant, a steal's abort and a
  // query's snapshot come back, each a message, so a callback runs in a task after the request. Here
  // the table is the runtime's (one origin and partition a runtime, SPEC decision 19), kept by the
  // page's thread as for ports (the hub): a window's messages reach it in a microtask, a worker's go
  // out with its task's items ({locks}) and its answers come back as worker messages. A worker that
  // ends drops its locks and requests (LockManagerParent::ActorDestroy).
  const lockHub = share("lockHub", () => {
    const origins = new Map();  // origin -> {held: [request], queues: Map name -> [request]}
    const managed = origin => {
      if (!origins.has(origin)) origins.set(origin, { held: [], queues: new Map() });
      return origins.get(origin);
    };
    // request: {id, name, mode, client: {id, deliver(msg, at)}}
    const grantable = (m, r) => !m.held.some(h => h.name === r.name && (r.mode === "exclusive" || h.mode === "exclusive"));
    const processQueue = (m, queue, at) => {
      while (queue.length && grantable(m, queue[0])) {
        const first = queue.shift();
        m.held.push(first);
        first.client.deliver({ grant: first.id, mode: first.mode, available: true }, at);
      }
    };
    const removeIfEmpty = (m, name) => { const q = m.queues.get(name); if (q && !q.length) m.queues.delete(name); };
    const info = r => ({ clientId: r.client.id, mode: r.mode, name: r.name });
    return {
      // LockManagerParent::RecvPLockRequestConstructor, RecvQuery, LockRequestParent::Recv__delete__
      handle(msg, client, at) {
        const m = managed(msg.origin);
        if (msg.op === "request") {
          const r = { id: msg.id, name: msg.name, mode: msg.mode, client };
          if (!m.queues.has(r.name)) m.queues.set(r.name, []);
          const queue = m.queues.get(r.name);
          if (msg.steal) {
            m.held = m.held.filter(h => {
              if (h.name !== r.name) return true;
              h.client.deliver({ aborted: h.id }, at);
              return false;
            });
            queue.unshift(r);
          } else if (msg.ifAvailable && (queue.length || !grantable(m, r))) {
            r.client.deliver({ grant: r.id, mode: r.mode, available: false }, at);
            return;
          } else {
            queue.push(r);
          }
          processQueue(m, queue, at);
        } else if (msg.op === "release" || msg.op === "abort") {
          const mine = r => r.id === msg.id && r.client.id === client.id;
          const held = m.held.find(mine);
          m.held = m.held.filter(r => !mine(r));
          const name = held ? held.name : msg.name;
          const queue = m.queues.get(name);
          if (queue) {
            if (msg.op === "abort") { const i = queue.findIndex(mine); if (i >= 0) queue.splice(i, 1); }
            processQueue(m, queue, at);
            removeIfEmpty(m, name);
          }
        } else if (msg.op === "query") {
          const pending = [];
          for (const queue of m.queues.values()) for (const r of queue) pending.push(info(r));
          client.deliver({ snapshot: msg.id, held: m.held.map(info), pending }, at);
        }
      },
      // LockManagerParent::ActorDestroy: a client gone, its locks and requests with it
      dropClient(clientId, at) {
        for (const m of origins.values()) {
          const names = new Set();
          m.held = m.held.filter(r => (r.client.id === clientId ? (names.add(r.name), false) : true));
          for (const [name, queue] of m.queues) {
            const kept = queue.filter(r => r.client.id !== clientId);
            if (kept.length !== queue.length) { m.queues.set(name, kept); names.add(name); }
          }
          for (const name of names) {
            const queue = m.queues.get(name);
            if (queue) { processQueue(m, queue, at); removeIfEmpty(m, name); }
          }
        }
      },
    };
  });
  // this global's side: its client id (Gecko's ClientInfo id), its requests by id, how its messages
  // reach the hub (a worker's set in its section below)
  let lockClientId = null;
  const lockClient = () => lockClientId || (lockClientId = crypto.randomUUID());
  const lockRequests = new Map();
  const lockQueries = new Map();
  let lockNextId = 1;
  let locksToHub = msg => {
    const client = { id: lockClient(), deliver: (m, at) => global.__mrt.queueAt(() => locksFromHub(m), at) };
    Promise.resolve().then(() => lockHub.handle(msg, client, clockNow()));
  };
  // LockRequestChild::RecvResolve, Recv__delete__, and a query's answer
  const locksFromHub = msg => {
    if ("snapshot" in msg) {
      const resolve = lockQueries.get(msg.snapshot);
      lockQueries.delete(msg.snapshot);
      if (resolve) resolve({ held: msg.held, pending: msg.pending });
      return;
    }
    if ("aborted" in msg) {
      const req = lockRequests.get(msg.aborted);
      if (!req) return;
      lockRequests.delete(msg.aborted);
      req.dead = true;
      req.reject(new DOMException("The lock request is aborted", "AbortError"));
      return;
    }
    const req = lockRequests.get(msg.grant);
    if (!req) return;
    req.unfollow();
    let settle;
    if (msg.available) {
      // the lock's waiting promise: when it settles the lock is released and the request's promise
      // takes its value (Lock::ResolvedCallback, RejectedCallback)
      const lock = Object.create(Lock.prototype);
      lockData.set(lock, { name: req.name, mode: msg.mode });
      let resolveW, rejectW;
      const waiting = new Promise((res, rej) => { resolveW = res; rejectW = rej; });
      const release = () => {
        lockRequests.delete(req.id);
        if (!req.dead) locksToHub({ op: "release", id: req.id, name: req.name, origin: global.origin });
      };
      waiting.then(v => { release(); req.resolve(v); }, e => { release(); req.reject(e); });
      settle = [resolveW, rejectW];
      runLockCallback(req.callback, lock, settle);
    } else {
      // ifAvailable and not available: no lock, the request's promise settled by the callback
      lockRequests.delete(req.id);
      runLockCallback(req.callback, null, [req.resolve, req.reject]);
    }
  };
  // RunCallbackAndSettlePromise
  const runLockCallback = (callback, lock, [resolve, reject]) => {
    let result;
    try { result = global.__mrt.callPage(callback, undefined, [lock]); } catch (e) { reject(e); return; }
    resolve(result);
  };
  const lockData = new WeakMap();
  const illegalLockClass = name => ({ [name]: function () { throw new TypeError("Illegal constructor."); } })[name];
  const Lock = illegalLockClass("Lock");
  for (const member of ["name", "mode"]) {
    Object.defineProperty(Lock.prototype, member, {
      get() {
        const d = this !== null && typeof this === "object" ? lockData.get(this) : undefined;
        if (!d) throw new TypeError(`'get ${member}' called on an object that does not implement interface Lock.`);
        return d[member];
      },
      configurable: true, enumerable: true,
    });
  }
  Object.defineProperty(Lock.prototype, Symbol.toStringTag, { value: "Lock", configurable: true });
  const LockManager = illegalLockClass("LockManager");
  // a principal Gecko makes no lock manager actor for (an opaque origin): its methods throw
  const lockManagerWorks = () => global.origin !== "null";
  const lockError = (method, message, name) => new DOMException(`LockManager.${method}: ${message}`, name);
  // LockManager::Request, with the WebIDL overloads' argument conversion
  const lockRequest = function (args) {
    const fail = e => Promise.reject(e);
    const n = args.length;
    if (n < 2) return fail(new TypeError(`LockManager.request: ${n} is not a valid argument count for any overload.`));
    let name, options = {}, callback;
    try {
      name = String(args[0]);
      if (n >= 3) {
        const o = args[1];
        if (o !== undefined && o !== null && typeof o !== "object" && typeof o !== "function") throw new TypeError("LockManager.request: Argument 2 can't be converted to a dictionary.");
        if (o !== undefined && o !== null) {
          // LockOptions' members, in WebIDL's (alphabetical) order
          const ifAvailable = o.ifAvailable, mode = o.mode, signal = o.signal, steal = o.steal;
          options.ifAvailable = !!ifAvailable;
          if (mode !== undefined) {
            const m = String(mode);
            if (m !== "shared" && m !== "exclusive") throw new TypeError(`LockManager.request: '${m}' (value of 'mode' member of LockOptions) is not a valid value for enumeration LockMode.`);
            options.mode = m;
          }
          if (signal !== undefined) {
            if (!isAbortSignal(signal)) throw new TypeError("LockManager.request: 'signal' member of LockOptions does not implement interface AbortSignal.");
            options.signal = signal;
          }
          options.steal = !!steal;
        }
      }
      callback = args[n >= 3 ? 2 : 1];
      const which = n >= 3 ? 3 : 2;
      if (callback === null || (typeof callback !== "object" && typeof callback !== "function")) throw new TypeError(`LockManager.request: Argument ${which} is not an object.`);
      if (typeof callback !== "function") throw new TypeError(`LockManager.request: Argument ${which} is not callable.`);
    } catch (e) {
      return fail(e);
    }
    const mode = options.mode || "exclusive";
    if (!lockManagerWorks()) return fail(lockError("request", "Web Locks API is not enabled for this kind of document", "NotSupportedError"));
    // ValidateRequestArguments
    if (name.length > 0 && name[0] === "-") return fail(lockError("request", "Names starting with `-` are reserved", "NotSupportedError"));
    if (options.steal) {
      if (options.ifAvailable) return fail(lockError("request", "`steal` and `ifAvailable` cannot be used together", "NotSupportedError"));
      if (mode !== "exclusive") return fail(lockError("request", "`steal` is only supported for exclusive lock requests", "NotSupportedError"));
    }
    if (options.signal) {
      if (options.steal) return fail(lockError("request", "`steal` and `signal` cannot be used together", "NotSupportedError"));
      if (options.ifAvailable) return fail(lockError("request", "`ifAvailable` and `signal` cannot be used together", "NotSupportedError"));
      const st = abortSignals.get(options.signal);
      if (st.aborted) return fail(reasonOf(st));
    }
    const id = lockNextId++;
    return new Promise((resolve, reject) => {
      const req = { id, name, callback, resolve, reject, dead: false, unfollow: () => {} };
      // LockRequestChild::RunAbortAlgorithm: a pending request aborted by its signal
      if (options.signal) {
        const signal = options.signal;
        req.unfollow = followAbort(signal, () => {
          lockRequests.delete(id);
          req.dead = true;
          reject(reasonOf(abortSignals.get(signal)));
          locksToHub({ op: "abort", id, name, origin: global.origin });
        });
      }
      lockRequests.set(id, req);
      locksToHub({ op: "request", id, name, mode, ifAvailable: !!options.ifAvailable, steal: !!options.steal, origin: global.origin });
    });
  };
  for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors({
    request(name, callback) { return lockRequest(arguments); },
    query() {
      if (!lockManagerWorks()) return Promise.reject(lockError("query", "Web Locks API is not enabled for this kind of document", "NotSupportedError"));
      const id = lockNextId++;
      return new Promise(resolve => {
        lockQueries.set(id, resolve);
        locksToHub({ op: "query", id, origin: global.origin });
      });
    },
  }))) Object.defineProperty(LockManager.prototype, key, { ...desc, enumerable: true });
  Object.defineProperty(LockManager.prototype, Symbol.toStringTag, { value: "LockManager", configurable: true });
  // [SecureContext]: neither interface nor navigator.locks elsewhere
  const lockManager = Object.create(LockManager.prototype);
  if (global.isSecureContext) {
    define(global, "Lock", Lock);
    define(global, "LockManager", LockManager);
    if (!isWorker) getter(navigator, "locks", () => lockManager);
  }
  // ---- navigator.permissions (Gecko's dom/permission) and navigator.storage (dom/quota's StorageManager) ----
  //
  // What a permission's state is and what the origin's storage holds are Firefox's (its permission
  // manager, its quota manager, which holds the IndexedDB the runtime forwards), so the bridge asks
  // Firefox for them (the `permissions` request). The runtime does what Gecko does in the content
  // process before that: the WebIDL conversion of the descriptor (PermissionDescriptor's and, for
  // midi, MidiPermissionDescriptor's Init, so `name` is read twice there, then `sysex`), the brand
  // checks and argument errors, and the PermissionStatus object. An answer resolves after the time
  // Firefox took for it, as Gecko resolves after its round trip to the parent process.
  // Not built: `change` events (PermissionObserver; a permission does not change while a page is
  // measured), getDirectory (the origin private file system, GAPS B96).
  const illegalPermClass = name => ({ [name]: function () { throw new TypeError("Illegal constructor."); } })[name];
  const brandError = (member, iface) => new TypeError(`'${member}' called on an object that does not implement interface ${iface}.`);
  // a bridge answer, settled after Firefox's time for it. The bridge tab's clock counts whole
  // milliseconds, and most answers take under one; Gecko's answer comes back through several
  // MozPromise hops, after a 0 ms timeout the page set after the call, so at least 1 ms here.
  const permissionsBridge = (args, settle) => {
    let answer;
    try { answer = bridge("permissions", args); } catch (e) { answer = { error: { name: "UnknownError", message: String(e.message) } }; }
    global.__mrt.queueAt(() => settle(answer), clockNow() + Math.max(answer.ms || 0, 1));
  };
  const permissionError = e => (e.name === "TypeError" ? new TypeError(e.message) : new DOMException(e.message, e.name));

  const permissionStatusData = new WeakMap();
  const PermissionStatus = illegalPermClass("PermissionStatus");
  Object.setPrototypeOf(PermissionStatus, EventTarget);
  Object.setPrototypeOf(PermissionStatus.prototype, EventTarget.prototype);
  for (const member of ["name", "state"]) {
    Object.defineProperty(PermissionStatus.prototype, member, {
      get() {
        const d = this !== null && typeof this === "object" ? permissionStatusData.get(this) : undefined;
        if (!d) throw new TypeError(`'get ${member}' called on an object that does not implement interface PermissionStatus.`);
        return d[member];
      },
      configurable: true, enumerable: true,
    });
  }
  global.__mrt.defineEventHandlers(PermissionStatus.prototype, ["onchange"]);
  Object.defineProperty(PermissionStatus.prototype, Symbol.toStringTag, { value: "PermissionStatus", configurable: true });

  const Permissions = illegalPermClass("Permissions");
  const isPermissions = new WeakSet();
  // Permissions::Query: the conversion's errors reject at once; the status resolves after Firefox's answer
  const permissionsQuery = function (self, args) {
    if (self === null || typeof self !== "object" || !isPermissions.has(self)) return Promise.reject(brandError("query", "Permissions"));
    if (args.length < 1) return Promise.reject(new TypeError("Permissions.query: At least 1 argument required, but only 0 passed"));
    const desc = args[0];
    if (desc === null || (typeof desc !== "object" && typeof desc !== "function")) return Promise.reject(new TypeError("Permissions.query: Argument 1 is not an object."));
    let name, sysex;
    try {
      // PermissionDescriptor::Init: its one member, required
      const readName = () => {
        const v = desc.name;
        if (v === undefined) throw new TypeError("Missing required 'name' member of PermissionDescriptor.");
        return String(v);
      };
      name = readName();
      // MidiPermissionDescriptor::Init reads the descriptor again: the parent's member, then its own
      if (name === "midi") {
        name = readName();
        sysex = !!desc.sysex;
      }
    } catch (e) {
      return Promise.reject(e);
    }
    return new Promise((resolve, reject) => {
      permissionsBridge({ call: "query", desc: name === "midi" ? { name, sysex } : { name } }, answer => {
        if (answer.error) { reject(permissionError(answer.error)); return; }
        const status = Object.create(PermissionStatus.prototype);
        permissionStatusData.set(status, { name: answer.value.name, state: answer.value.state });
        resolve(status);
      });
    });
  };
  Object.defineProperty(Permissions.prototype, "query", {
    value: { query(permission) { return permissionsQuery(this, arguments); } }.query, writable: true, configurable: true, enumerable: true,
  });
  Object.defineProperty(Permissions.prototype, Symbol.toStringTag, { value: "Permissions", configurable: true });
  const permissions = Object.create(Permissions.prototype);
  isPermissions.add(permissions);
  define(global, "Permissions", Permissions);
  define(global, "PermissionStatus", PermissionStatus);
  if (!isWorker) getter(navigator, "permissions", () => permissions);

  // ---- navigator.geolocation (Gecko's dom/geolocation; task 09c9) ----
  //
  // Google Maps shows its "Show Your Location" button when navigator.geolocation exists. A request
  // (Geolocation::GetCurrentPosition, WatchPosition) is denied in an insecure context, as a task
  // (RequestDelayedTask Deny); else the bridge tab makes the same request (RequestIfPermitted: the
  // permission, a prompt, the location provider) and its answer comes back after Firefox's time: an
  // error's code, a position, or nothing while Firefox waits (a prompt nobody answers). The bridge's
  // Firefox under Marionette denies at once. A watch gets that first answer only; options' timeout
  // and maximumAge are Firefox's request's.
  if (!isWorker) {
    const Geolocation = illegalPermClass("Geolocation");
    const GeolocationPositionError = illegalPermClass("GeolocationPositionError");
    const GeolocationPosition = illegalPermClass("GeolocationPosition");
    const GeolocationCoordinates = illegalPermClass("GeolocationCoordinates");
    const GEO_CODES = { PERMISSION_DENIED: 1, POSITION_UNAVAILABLE: 2, TIMEOUT: 3 };
    const GEO_MESSAGES = { 1: "User denied geolocation prompt", 2: "Unknown error acquiring position", 3: "Position acquisition timed out" };
    for (const [k, v] of Object.entries(GEO_CODES)) {
      Object.defineProperty(GeolocationPositionError, k, { value: v, enumerable: true });
      Object.defineProperty(GeolocationPositionError.prototype, k, { value: v, enumerable: true });
    }
    // each object's data, read by its interface's getters (brand-checked)
    const geoData = new WeakMap();
    const geoGetters = (C, members, read) => {
      for (const member of members) {
        Object.defineProperty(C.prototype, member, {
          get() {
            const d = geoData.get(this);
            if (!d || d.iface !== C) throw new TypeError(`'get ${member}' called on an object that does not implement interface ${C.name}.`);
            return read(d, member);
          },
          configurable: true, enumerable: true,
        });
      }
    };
    geoGetters(GeolocationPositionError, ["code", "message"], (d, m) => (m === "code" ? d.code : GEO_MESSAGES[d.code]));
    geoGetters(GeolocationPosition, ["coords", "timestamp"], (d, m) => d[m]);
    geoGetters(GeolocationCoordinates, ["latitude", "longitude", "altitude", "accuracy", "altitudeAccuracy", "heading", "speed"], (d, m) => d.values[m] ?? null);
    for (const C of [Geolocation, GeolocationPositionError, GeolocationPosition, GeolocationCoordinates]) {
      Object.defineProperty(C.prototype, Symbol.toStringTag, { value: C.name, configurable: true });
      define(global, C.name, C);
    }
    const make = (C, data) => {
      const o = Object.create(C.prototype);
      geoData.set(o, Object.assign({ iface: C }, data));
      return o;
    };
    const geolocation = make(Geolocation, {});
    let lastWatchId = 1;
    const clearedWatches = new Set();
    // the PositionCallback and PositionErrorCallback conversions, then the request
    const geoRequest = (member, args, watchId) => {
      if (args.length < 1) throw new TypeError(`Geolocation.${member}: At least 1 argument required, but only 0 passed`);
      const onSuccess = args[0], onError = args[1];
      if (typeof onSuccess !== "function") throw new TypeError(`Geolocation.${member}: Argument 1 is not callable.`);
      if (onError !== undefined && onError !== null && typeof onError !== "function")
        throw new TypeError(`Geolocation.${member}: Argument 2 is not ${typeof onError === "object" ? "callable" : "an object"}.`);
      const answer = value => {
        if (watchId && clearedWatches.has(watchId)) return;
        if (value.position) {
          const p = value.position;
          global.__mrt.callPage(onSuccess, undefined, [make(GeolocationPosition, { timestamp: p.timestamp, coords: make(GeolocationCoordinates, { values: p.coords }) })]);
        } else if (typeof onError === "function") global.__mrt.callPage(onError, undefined, [make(GeolocationPositionError, { code: value.code })]);
      };
      if (!global.isSecureContext) { global.__mrt.queueTask(() => answer({ code: GEO_CODES.PERMISSION_DENIED })); return; }
      permissionsBridge({ call: "geolocation" }, r => {
        if (r.error) answer({ code: GEO_CODES.POSITION_UNAVAILABLE });
        else if (!r.pending) answer(r.value);
      });
    };
    const geoMembers = {
      getCurrentPosition(successCallback, errorCallback = null, options = {}) {
        if (geoData.get(this) !== geoData.get(geolocation)) throw brandError("getCurrentPosition", "Geolocation");
        geoRequest("getCurrentPosition", arguments, 0);
      },
      watchPosition(successCallback, errorCallback = null, options = {}) {
        if (geoData.get(this) !== geoData.get(geolocation)) throw brandError("watchPosition", "Geolocation");
        const watchId = lastWatchId;
        geoRequest("watchPosition", arguments, watchId);
        lastWatchId++;
        return watchId;
      },
      clearWatch(watchId) {
        if (geoData.get(this) !== geoData.get(geolocation)) throw brandError("clearWatch", "Geolocation");
        if (arguments.length < 1) throw new TypeError("Geolocation.clearWatch: At least 1 argument required, but only 0 passed");
        const id = Number(watchId) | 0;
        if (id >= 1) clearedWatches.add(id);
      },
    };
    for (const [name, fn] of Object.entries(geoMembers)) {
      Object.defineProperty(Geolocation.prototype, name, { value: fn, writable: true, configurable: true, enumerable: true });
    }
    getter(navigator, "geolocation", () => geolocation);
  }

  // StorageManager: [SecureContext]; persist() is [Exposed=Window]
  const StorageManager = illegalPermClass("StorageManager");
  const isStorageManager = new WeakSet();
  const storageCall = (self, call) => new Promise((resolve, reject) => {
    if (self === null || typeof self !== "object" || !isStorageManager.has(self)) { reject(brandError(call, "StorageManager")); return; }
    permissionsBridge({ call }, answer => {
      if (answer.error) reject(permissionError(answer.error));
      // StorageEstimate's members in WebIDL's (alphabetical) order
      else resolve(call === "estimate" ? { quota: answer.value.quota, usage: answer.value.usage } : answer.value);
    });
  });
  const storageMembers = {
    persisted() { return storageCall(this, "persisted"); },
    // PersistentStoragePermissionRequest: with the persistent-storage permission allowed for the
    // origin itself (the front end's exact check, not query()'s parent domains) Gecko persists the
    // origin, denied it answers false, otherwise it asks the user (a doorhanger) and the promise
    // waits for the answer (`pending`), which in a page nobody looks at never comes
    persist() {
      if (this === null || typeof this !== "object" || !isStorageManager.has(this)) return Promise.reject(brandError("persist", "StorageManager"));
      return new Promise((resolve, reject) => {
        permissionsBridge({ call: "persist" }, answer => {
          if (answer.error) reject(permissionError(answer.error));
          else if (!answer.pending) resolve(answer.value);
        });
      });
    },
    estimate() { return storageCall(this, "estimate"); },
  };
  for (const key of isWorker ? ["persisted", "estimate"] : ["persisted", "persist", "estimate"]) {
    Object.defineProperty(StorageManager.prototype, key, { value: storageMembers[key], writable: true, configurable: true, enumerable: true });
  }
  Object.defineProperty(StorageManager.prototype, Symbol.toStringTag, { value: "StorageManager", configurable: true });
  const storageManager = Object.create(StorageManager.prototype);
  isStorageManager.add(storageManager);
  if (global.isSecureContext) {
    define(global, "StorageManager", StorageManager);
    if (!isWorker) getter(navigator, "storage", () => storageManager);
  }
  // ---- Notification (Gecko's dom/notification), windows and workers ----
  //
  // The permission is Firefox's (its permission manager, for the page's principal), asked through
  // the bridge's `permissions` request on the first read and kept: it does not change while a page
  // is measured, except by requestPermission, whose answer replaces it. requestPermission is
  // Firefox's own call in the bridge tab, which has no user activation, so Firefox's front-end
  // cancels it and it resolves with the permission as it was (normally "default"); it resolves
  // after Firefox's time, at least 1 ms, as navigator.permissions does. The constructor converts
  // NotificationOptions as Gecko's binding does (members in name order) and ValidateAndCreate's
  // steps; Gecko then asks the parent process to show it, which refuses when the permission is
  // not granted, and the notification gets `error` after that round trip.
  // Not built: showing one with the permission granted (GAPS B101); `show`, `click` and `close`
  // never fire.
  {
    const notificationData = new WeakMap();
    const actionsMax = 2; // NotificationUtils.h kMaxActions
    let permission = null;
    const notificationPermission = () => {
      if (permission === null) {
        let answer;
        try { answer = bridge("permissions", { call: "notification" }); } catch (e) { answer = { value: "denied" }; }
        permission = answer.error ? "denied" : answer.value;
      }
      return permission;
    };
    const notificationBase = () => (isWorker ? String(global.location.href) : document.baseURI);
    // Notification::ResolveURL: an empty or unparsable URL is no URL
    const resolveNotificationURL = s => {
      if (s === "") return "";
      try { return new URL(s, notificationBase()).href; } catch (e) { return ""; }
    };
    const ctorError = msg => new TypeError(`Notification constructor: ${msg}`);
    // DOMString: ToString, which refuses a symbol
    const domString = v => `${v}`;
    // UTF8String: a DOMString whose lone surrogates become U+FFFD
    const utf8String = v => domString(v).replace(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g, "�");
    const sequenceOf = (v, what, each) => {
      if (v === null || (typeof v !== "object" && typeof v !== "function")) throw ctorError(`${what} can't be converted to a sequence.`);
      const iter = v[Symbol.iterator];
      if (typeof iter !== "function") throw ctorError(`${what} can't be converted to a sequence.`);
      const out = [];
      for (const x of { [Symbol.iterator]: () => iter.call(v) }) out.push(each(x));
      return out;
    };
    const toUint32 = v => Number(v) >>> 0;
    // NotificationAction::Init: action, title (release Firefox's navigate pref is off, so no navigate)
    const notificationAction = v => {
      if (v !== undefined && v !== null && typeof v !== "object" && typeof v !== "function") throw ctorError("Element of 'actions' member of NotificationOptions can't be converted to a dictionary.");
      const o = v ?? {};
      const action = o.action;
      if (action === undefined) throw ctorError("Missing required 'action' member of NotificationAction.");
      const out = { action: domString(action) };
      const title = o.title;
      if (title === undefined) throw ctorError("Missing required 'title' member of NotificationAction.");
      out.title = domString(title);
      return out;
    };
    // NotificationOptions::Init: the members in name order, each read once (navigate's pref is off)
    const notificationOptions = v => {
      if (v !== undefined && v !== null && typeof v !== "object" && typeof v !== "function") throw ctorError("Argument 2 can't be converted to a dictionary.");
      const o = v ?? {};
      const read = name => (v === undefined || v === null ? undefined : o[name]);
      const r = {};
      let x;
      x = read("actions"); r.actions = x === undefined ? [] : sequenceOf(x, "'actions' member of NotificationOptions", notificationAction);
      x = read("badge"); if (x !== undefined) r.badge = utf8String(x);
      x = read("body"); r.body = x === undefined ? "" : domString(x);
      x = read("data"); r.data = x === undefined ? null : x;
      x = read("dir"); r.dir = x === undefined ? "auto" : domString(x);
      if (!["auto", "ltr", "rtl"].includes(r.dir)) throw ctorError(`'${r.dir}' (value of 'dir' member of NotificationOptions) is not a valid value for enumeration NotificationDirection.`);
      x = read("icon"); r.icon = x === undefined ? "" : utf8String(x);
      x = read("image"); if (x !== undefined) r.image = utf8String(x);
      x = read("lang"); r.lang = x === undefined ? "" : domString(x);
      x = read("renotify"); r.renotify = !!x;
      x = read("requireInteraction"); r.requireInteraction = !!x;
      x = read("silent"); r.silent = !!x;
      x = read("tag"); r.tag = x === undefined ? "" : domString(x);
      x = read("timestamp"); if (x !== undefined) r.timestamp = Number(x);
      x = read("vibrate");
      // (unsigned long or sequence<unsigned long>): an object is the sequence only if it is iterable
      if (x !== undefined) r.vibrate = x !== null && (typeof x === "object" || typeof x === "function") && x[Symbol.iterator] !== undefined
        ? sequenceOf(x, "'vibrate' member of NotificationOptions", toUint32) : toUint32(x);
      return r;
    };
    // (length 1: the options are optional)
    const Notification = function Notification(title) {
      if (new.target === undefined) throw new TypeError("Notification constructor: 'new' is required");
      if (arguments.length < 1) throw ctorError("At least 1 argument required, but only 0 passed");
      const t = domString(title);
      const o = notificationOptions(arguments[1]);
      // ValidateAndCreate: the data serialized first (a DataCloneError leaves no object), the URLs
      // resolved against the global's base, at most kMaxActions actions (vibrate's pref is off)
      let data = null;
      if (o.data !== null) {
        // nsStructuredCloneContainer's failure, not the clone's own message
        try { data = structuredClone(o.data); } catch (e) {
          throw e && e.name === "DataCloneError" ? new DOMException("The object could not be cloned.", "DataCloneError") : e;
        }
      }
      const actions = o.actions.slice(0, actionsMax);
      const self = Reflect.construct(EventTarget, [], new.target);
      notificationData.set(self, {
        title: t, dir: o.dir, lang: o.lang, body: o.body, tag: o.tag, icon: resolveNotificationURL(o.icon),
        requireInteraction: o.requireInteraction, silent: o.silent, data, actions, actionsArray: null,
      });
      // LoadImageAndShow, then the parent's RecvShow: refused unless the permission is granted
      let answer;
      try { answer = bridge("permissions", { call: "notification" }); } catch (e) { answer = { value: "denied", ms: 0 }; }
      if (!answer.error) permission = answer.value;
      // (the runtime's events have no isTrusted, GAPS B72; this one is the browser's)
      if (permission !== "granted") global.__mrt.queueAt(() => self.dispatchEvent(Object.assign(new Event("error"), { isTrusted: true })), clockNow() + Math.max(answer.ms || 0, 1));
      return self;
    };
    Object.setPrototypeOf(Notification, EventTarget);
    Object.setPrototypeOf(Notification.prototype, EventTarget.prototype);
    Object.defineProperty(Notification, "prototype", { writable: false, enumerable: false, configurable: false });
    const notificationOf = (self, member) => {
      const d = self !== null && typeof self === "object" ? notificationData.get(self) : undefined;
      if (!d) throw new TypeError(`'${member}' called on an object that does not implement interface Notification.`);
      return d;
    };
    // the statics: requestPermission only on the main thread (RequestPermissionEnabledForScope)
    if (!isWorker) {
      Object.defineProperty(Notification, "requestPermission", {
        value: { requestPermission() {
          // the optional callback's conversion, its error a rejection (the method returns a Promise)
          const callback = arguments[0];
          if (callback !== undefined && typeof callback !== "function") {
            const what = callback !== null && typeof callback === "object" ? "is not callable" : "is not an object";
            return Promise.reject(new TypeError(`Notification.requestPermission: Argument 1 ${what}.`));
          }
          return new Promise(resolve => {
            permissionsBridge({ call: "requestNotification" }, answer => {
              if (answer.pending) return;
              const value = answer.error ? notificationPermission() : (permission = answer.value);
              if (callback !== undefined) {
                // the callback's exception is reported (CallbackObject's eReportExceptions), the promise resolves
                try { global.__mrt.callPage(callback, undefined, [value]); } catch (e) { global.__mrt.queueTask(() => { throw e; }); }
              }
              resolve(value);
            });
          });
        } }.requestPermission,
        writable: true, enumerable: true, configurable: true,
      });
    }
    Object.defineProperty(Notification, "permission", {
      get: Object.getOwnPropertyDescriptor({ get permission() { return notificationPermission(); } }, "permission").get,
      enumerable: true, configurable: true,
    });
    Object.defineProperty(Notification, "maxActions", {
      get: Object.getOwnPropertyDescriptor({ get maxActions() { return actionsMax; } }, "maxActions").get,
      enumerable: true, configurable: true,
    });
    // the prototype: close(), then the attributes in WebIDL order
    Object.defineProperty(Notification.prototype, "close", {
      value: { close() { notificationOf(this, "close"); } }.close, writable: true, enumerable: true, configurable: true,
    });
    // the event handlers: prelude.js's, behind Gecko's names and brand checks
    const handlers = {};
    global.__mrt.defineEventHandlers(handlers, ["onclick", "onshow", "onerror", "onclose"]);
    for (const name of ["onclick", "onshow", "onerror", "onclose"]) {
      const inner = Object.getOwnPropertyDescriptor(handlers, name);
      const accessors = {
        get [name]() { notificationOf(this, `get ${name}`); return inner.get.call(this); },
        set [name](value) { notificationOf(this, `set ${name}`); inner.set.call(this, value); },
      };
      const d = Object.getOwnPropertyDescriptor(accessors, name);
      Object.defineProperty(Notification.prototype, name, { get: d.get, set: d.set, enumerable: true, configurable: true });
    }
    for (const member of ["title", "dir", "lang", "body", "tag", "icon", "requireInteraction", "silent", "data", "actions"]) {
      const get = {
        get [member]() {
          const d = notificationOf(this, `get ${member}`);
          if (member !== "actions") return d[member];
          // [Frozen, Cached]: one frozen array, of plain dictionaries
          if (d.actionsArray === null) d.actionsArray = Object.freeze(d.actions.map(a => ({ ...a })));
          return d.actionsArray;
        },
      };
      Object.defineProperty(Notification.prototype, member, { get: Object.getOwnPropertyDescriptor(get, member).get, enumerable: true, configurable: true });
    }
    Object.defineProperty(Notification.prototype, Symbol.toStringTag, { value: "Notification", configurable: true });
    define(global, "Notification", Notification);
  }
  // ---- Blob and File (dom/file), their bytes kept in the runtime (GAPS B69) ----
  //
  // As Gecko's: a Blob is a handle on a BlobImpl, which holds the bytes. The constructor builds a
  // MultipartBlobImpl through BlobSet: a buffer's bytes or a string's (as UTF-8) become a
  // MemoryBlobImpl, and a Blob part adds its impl's own sub-impls, so nesting stays flat. A slice
  // shares the bytes it covers. Reads resolve two tasks after the call, as BodyConsumer's do: a 0 ms
  // timer the page sets after the call runs first.
  class MemoryBlobImpl {
    constructor(bytes, contentType) { this.bytes = bytes; this.contentType = contentType; }
    get size() { return this.bytes.length; }
    get isFile() { return false; }
    getSubBlobImpls() { return null; }
    createSlice(start, length, contentType) { return new MemoryBlobImpl(this.bytes.subarray(start, start + length), contentType); }
    copyTo(out, at) { out.set(this.bytes, at); }
  }
  // `name` is a File's; a Blob's is undefined. Dates are in microseconds, as BaseBlobImpl's.
  class MultipartBlobImpl {
    constructor(blobImpls, contentType, name) {
      this.blobImpls = blobImpls;
      this.contentType = contentType;
      this.isFile = name !== undefined;
      this.name = name;
      this.setLengthAndModifiedDate();
    }
    // a File's date is the latest of its parts that are files, else now (JS_Now, at Firefox's 1 ms
    // timer precision); the unit mix-up when a part sets it is Gecko's
    setLengthAndModifiedDate() {
      let totalLength = 0, lastModified = 0, lastModifiedSet = false;
      for (const blob of this.blobImpls) {
        totalLength += blob.size;
        if (blob.isFile) {
          const partLastModified = blob.getLastModified();
          if (lastModified < partLastModified) { lastModified = partLastModified * 1000; lastModifiedSet = true; }
        }
      }
      this.size = totalLength;
      if (this.isFile) this.lastModificationDate = lastModifiedSet ? lastModified : Date.now() * 1000;
    }
    getLastModified() { return Math.trunc(this.lastModificationDate / 1000); }
    setLastModified(ms) { this.lastModificationDate = ms * 1000; }
    getSubBlobImpls() { return this.blobImpls; }
    createSlice(aStart, aLength, contentType) {
      const blobImpls = [];
      let length = aLength, skipStart = aStart, i;
      // prune the list of blobs if we can
      for (i = 0; length && skipStart && i < this.blobImpls.length; i++) {
        const blobImpl = this.blobImpls[i], l = blobImpl.size;
        if (skipStart < l) {
          const upperBound = Math.min(l - skipStart, length);
          const firstBlobImpl = blobImpl.createSlice(skipStart, upperBound, contentType);
          // avoid wrapping a single blob inside a MultipartBlobImpl
          if (length === upperBound) return firstBlobImpl;
          blobImpls.push(firstBlobImpl);
          length -= upperBound;
          i++;
          break;
        }
        skipStart -= l;
      }
      // now append enough blobs until we're done
      for (; length && i < this.blobImpls.length; i++) {
        const blobImpl = this.blobImpls[i], l = blobImpl.size;
        blobImpls.push(length < l ? blobImpl.createSlice(0, length, contentType) : blobImpl);
        length -= Math.min(l, length);
      }
      return new MultipartBlobImpl(blobImpls, contentType);
    }
    copyTo(out, at) { for (const b of this.blobImpls) { b.copyTo(out, at); at += b.size; } }
  }
  // BlobSet::AppendBlobImpl: a multipart impl adds its parts
  const appendBlobImpl = (list, impl) => {
    const subBlobs = impl.getSubBlobImpls();
    if (subBlobs) for (const sub of subBlobs) appendBlobImpl(list, sub);
    else list.push(impl);
  };
  // the impl's bytes, a fresh copy
  const readBlobImpl = impl => {
    const out = new Uint8Array(impl.size);
    impl.copyTo(out, 0);
    return out;
  };
  // Blob::MakeValidBlobType: lower case, or empty when a character is outside printable ASCII
  const makeValidBlobType = type => (/^[\x20-\x7e]*$/.test(type) ? type.replace(/[A-Z]+/g, c => c.toLowerCase()) : "");
  // WebIDL's conversions, with Gecko's messages
  const toSequence = (v, what) => {
    if ((typeof v !== "object" && typeof v !== "function") || v === null || typeof v[Symbol.iterator] !== "function")
      throw new TypeError(`${what} can't be converted to a sequence.`);
    return Array.from(v);
  };
  const toDictionary = (v, what) => {
    if (v === undefined || v === null) return {};
    if (typeof v !== "object" && typeof v !== "function") throw new TypeError(`${what} can't be converted to a dictionary.`);
    return v;
  };
  const toUSVString = v => new TextDecoder().decode(new TextEncoder().encode(`${v}`));
  // long long, and [Clamp] long long (rounded half to even)
  const toLongLong = v => { const n = Number(v); return Number.isFinite(n) ? Number(BigInt.asIntN(64, BigInt(Math.trunc(n)))) : 0; };
  const toClampedLongLong = v => {
    const n = Number(v);
    if (Number.isNaN(n)) return 0;
    const c = Math.min(Math.max(n, -(2 ** 63)), 2 ** 63 - 1), f = Math.floor(c), d = c - f;
    return d > 0.5 || (d === 0.5 && f % 2 !== 0) ? f + 1 : f;
  };
  // BlobPropertyBag's members, in Gecko's order (endings, type)
  const blobPropertyBag = (bag, iface) => {
    let native = false, type = "";
    const endingsValue = bag.endings;
    if (endingsValue !== undefined) {
      const endings = `${endingsValue}`;
      if (endings !== "transparent" && endings !== "native")
        throw new TypeError(`${iface} constructor: '${endings}' (value of 'endings' member of BlobPropertyBag) is not a valid value for enumeration EndingType.`);
      native = endings === "native";
    }
    const typeValue = bag.type;
    if (typeValue !== undefined) type = `${typeValue}`;
    return { native, type: makeValidBlobType(type) };
  };
  let blobImplOf, createBlob;
  // MultipartBlobImpl::InitializeBlob over BlobSet
  const blobImplsOf = (parts, nativeEOL) => {
    const list = [];
    for (const data of parts) {
      const impl = blobImplOf(data);
      if (impl) { appendBlobImpl(list, impl); continue; }
      const bytes = bytesOf(data);
      if (bytes) { if (bytes.length) list.push(new MemoryBlobImpl(bytes, "")); continue; }
      // BlobSet::AppendUTF8String: native line endings are "\n" here, as on Linux
      let str = toUSVString(data);
      if (nativeEOL && str.includes("\r")) str = str.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
      list.push(new MemoryBlobImpl(new TextEncoder().encode(str), ""));
    }
    return list;
  };
  // a promise-returning operation: a wrong `this` rejects, as WebIDL's
  const blobRead = (self, member, read) => {
    let impl;
    try { impl = blobImplOf(self, member); } catch (e) { return Promise.reject(e); }
    // BodyConsumer: the input stream is read on another task, the promise settled on the next
    return new Promise(resolve => global.__mrt.queueTask(() => global.__mrt.queueTask(() => resolve(read(readBlobImpl(impl))))));
  };
  class BlobInterface {
    #impl;
    static {
      // the impl of a Blob, or null; with `member`, a brand check that throws as Gecko's
      blobImplOf = (v, member, iface = "Blob") => {
        if ((typeof v === "object" || typeof v === "function") && v !== null && #impl in v) return v.#impl;
        if (member) throw new TypeError(`'${member}' called on an object that does not implement interface ${iface}.`);
        return null;
      };
      // Blob::Create: a File when the impl is a file's
      createBlob = impl => {
        const proto = impl.isFile ? File.prototype : Blob.prototype;
        return Reflect.construct(BlobInterface, [blobToken, impl], proto.constructor);
      };
    }
    constructor(parts, options) {
      if (parts === blobToken) { this.#impl = options; return; }
      let blobParts = [];
      if (parts !== undefined) blobParts = toSequence(parts, "Blob constructor: Argument 1");
      const { native, type } = blobPropertyBag(toDictionary(options, "Blob constructor: Argument 2"), "Blob");
      this.#impl = new MultipartBlobImpl(blobImplsOf(blobParts, native), type);
    }
    get size() { return blobImplOf(this, "get size").size; }
    get type() { return blobImplOf(this, "get type").contentType; }
    // BlobImpl::Slice: ParseSize clamps start and end into the size; the type is made valid
    slice(start, end, contentType) {
      const impl = blobImplOf(this, "slice"), size = impl.size;
      const parse = (v, d) => {
        if (v === undefined) return d;
        const n = toClampedLongLong(v);
        return n < -size ? 0 : n < 0 ? n + size : n > size ? size : n;
      };
      let s = parse(start, 0), e = parse(end, size);
      if (s >= e) s = e = 0;
      const type = makeValidBlobType(contentType === undefined ? "" : `${contentType}`);
      return createBlob(impl.createSlice(s, e - s, type));
    }
    stream() {
      const bytes = readBlobImpl(blobImplOf(this, "stream"));
      return new ReadableStream({ start(c) { if (bytes.length) c.enqueue(bytes); c.close(); } });
    }
    text() { return blobRead(this, "text", b => new TextDecoder().decode(b)); }
    arrayBuffer() { return blobRead(this, "arrayBuffer", b => b.buffer); }
    bytes() { return blobRead(this, "bytes", b => b); }
  }
  // the token that makes a Blob over an impl (Blob::Create) instead of from parts
  const blobToken = {};
  class FileInterface extends BlobInterface {
    constructor(fileBits, fileName, options) {
      if (fileBits === blobToken) { super(blobToken, fileName); return; }
      const parts = toSequence(fileBits, "File constructor: Argument 1");
      const name = toUSVString(fileName);
      const bag = toDictionary(options, "File constructor: Argument 3");
      const { native, type } = blobPropertyBag(bag, "File");
      const lastModifiedValue = bag.lastModified;
      const lastModified = lastModifiedValue === undefined ? undefined : toLongLong(lastModifiedValue);
      // File::Constructor
      const impl = new MultipartBlobImpl(blobImplsOf(parts, native), type, name);
      if (lastModified !== undefined) impl.setLastModified(lastModified);
      super(blobToken, impl);
    }
    get name() { return fileImpl(this, "get name").name; }
    get lastModified() { return fileImpl(this, "get lastModified").getLastModified(); }
    get webkitRelativePath() { fileImpl(this, "get webkitRelativePath"); return ""; }
  }
  const fileImpl = (v, member) => {
    const impl = blobImplOf(v, member, "File");
    if (!impl.isFile) throw new TypeError(`'${member}' called on an object that does not implement interface File.`);
    return impl;
  };
  // Blob::ToFile(name): a File over the Blob's impl
  const blobToFile = (blob, name) => {
    const impl = blobImplOf(blob);
    return createBlob(new MultipartBlobImpl([impl], impl.contentType, name));
  };
  // the interface objects: functions, so that their length is WebIDL's and a call without `new`
  // throws Gecko's TypeError; their prototypes are the classes'
  const blobInterface = (name, Impl, length, parent) => {
    const F = { [name]: function () {
      if (!new.target) throw new TypeError(`${name} constructor: 'new' is required`);
      if (name === "File" && arguments.length < 2) throw new TypeError(`File constructor: At least 2 arguments required, but only ${arguments.length} passed`);
      return Reflect.construct(Impl, arguments, new.target);
    } }[name];
    Object.defineProperty(F, "length", { value: length });
    if (parent) Object.setPrototypeOf(F, parent);
    Object.defineProperty(F, "prototype", { value: Impl.prototype, writable: false });
    Object.defineProperty(Impl.prototype, "constructor", { value: F, writable: true, configurable: true });
    Object.defineProperty(Impl.prototype, Symbol.toStringTag, { value: name, configurable: true });
    // WebIDL members are enumerable
    for (const k of Object.getOwnPropertyNames(Impl.prototype)) {
      if (k !== "constructor") Object.defineProperty(Impl.prototype, k, { enumerable: true });
    }
    return F;
  };
  const Blob = blobInterface("Blob", BlobInterface, 0);
  const File = blobInterface("File", FileInterface, 2, Blob);
  // FormData's entry values: a Blob becomes a File, "blob" when it has no name (FormData::
  // GetOrCreateFileCalledBlob); anything else a USVString
  const formDataValue = (value, filename) => {
    const impl = blobImplOf(value);
    if (!impl) return toUSVString(value);
    if (filename !== undefined) return blobToFile(value, toUSVString(filename));
    return impl.isFile ? value : blobToFile(value, "blob");
  };
  class FormData {
    constructor() { this._list = []; }
    append(k, v, filename) { this._list.push([toUSVString(k), formDataValue(v, filename)]); }
    set(k, v, filename) { this.delete(k); this.append(k, v, filename); }
    get(k) { const e = this._list.find(p => p[0] === k); return e ? e[1] : null; } getAll(k) { return this._list.filter(p => p[0] === k).map(p => p[1]); }
    has(k) { return this._list.some(p => p[0] === k); } delete(k) { this._list = this._list.filter(p => p[0] !== k); }
    entries() { return this._list[Symbol.iterator](); } [Symbol.iterator]() { return this.entries(); }
    toString() { return new URLSearchParams(this._list.map(([k, v]) => [k, String(v)])).toString(); }
  }
  define(global, "Blob", Blob);
  define(global, "File", File);
  define(global, "FormData", FormData);

  // ---- FileReader and FileReaderSync (Gecko's dom/file/FileReader.cpp, FileReaderSync.cpp) ----
  //
  // A Blob's bytes are in this thread's heap, so a read takes Gecko's steps as tasks without a stream:
  // the read call makes the reader LOADING; a task dispatches loadstart (InitialAsyncWait); a task
  // reads every byte and dispatches progress (OnInputStreamReady); a task ends it (DONE, the result,
  // load and loadend; OnLoadEnd). abort() during them is DONE at once with abort and loadend, and the
  // tasks left do nothing.
  const fileReaderArgs = (what, args) => {
    if (args.length < 1) throw new TypeError(`${what}: At least 1 argument required, but only 0 passed`);
    const blob = args[0];
    if ((typeof blob !== "object" && typeof blob !== "function") || blob === null) throw new TypeError(`${what}: Argument 1 is not an object.`);
    const impl = blobImplOf(blob);
    if (!impl) throw new TypeError(`${what}: Argument 1 does not implement interface Blob.`);
    return impl;
  };
  // FileReader::GetAsText: the label given, else the Blob type's charset, else UTF-8; a BOM wins
  // (Encoding::Decode sniffs it)
  const decodeBlobText = (bytes, label, contentType) => {
    const encodingFor = l => { if (l == null) return null; try { return new TextDecoder(String(l)).encoding; } catch (e) { return null; } };
    let encoding = encodingFor(label);
    if (!encoding) {
      const m = /;\s*charset\s*=\s*"?([^";\s]+)/i.exec(contentType || "");
      encoding = encodingFor(m && m[1]) || "utf-8";
    }
    if (bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) encoding = "utf-8";
    else if (bytes[0] === 0xfe && bytes[1] === 0xff) encoding = "utf-16be";
    else if (bytes[0] === 0xff && bytes[1] === 0xfe) encoding = "utf-16le";
    return new TextDecoder(encoding).decode(bytes);
  };
  // FileReader::GetAsDataURL: the Blob's type, or application/octet-stream
  const blobDataURL = (bytes, contentType) => `data:${contentType || "application/octet-stream"};base64,${toBase64(bytes)}`;
  const binaryString = bytes => { let s = ""; for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000)); return s; };
  const blobResult = (format, bytes, impl, label) =>
    format === "arraybuffer" ? freshBuffer(bytes) : format === "dataurl" ? blobDataURL(bytes, impl.contentType)
      : format === "binary" ? binaryString(bytes) : decodeBlobText(bytes, label, impl.contentType);
  const EMPTY = 0, LOADING = 1, DONE = 2;
  const fileReaderStates = new WeakMap();
  const readerState = (r, member) => {
    const st = r !== null && typeof r === "object" ? fileReaderStates.get(r) : undefined;
    if (!st) throw new TypeError(`'${member}' called on an object that does not implement interface FileReader.`);
    return st;
  };
  // FileReader::DispatchProgressEvent: not cancelable, the bytes read so far of the total
  const progress = (reader, st, type) => reader.dispatchEvent(new global.ProgressEvent(type, { lengthComputable: true, loaded: st.transferred, total: st.total }));
  class FileReader extends global.EventTarget {
    constructor() {
      super();
      fileReaderStates.set(this, { readyState: EMPTY, result: null, error: null, read: 0, transferred: 0, total: 0 });
    }
    get readyState() { return readerState(this, "get readyState").readyState; }
    get result() { const st = readerState(this, "get result"); return st.readyState === DONE ? st.result : null; }
    get error() { return readerState(this, "get error").error; }
    // FileReader::Abort
    abort() {
      const st = readerState(this, "abort");
      if (st.readyState !== LOADING) return;
      st.readyState = DONE;
      st.read++;
      st.error = new DOMException("The operation was aborted. ", "AbortError");
      st.result = null;
      progress(this, st, "abort");
      progress(this, st, "loadend");
    }
  }
  // FileReader::ReadFileContent and the tasks after it
  const readAs = (format, member) => function (blob, label) {
    const st = readerState(this, member);
    const impl = fileReaderArgs(`FileReader.${member}`, arguments);
    if (st.readyState === LOADING) throw new DOMException("An attempt was made to use an object that is not, or is no longer, usable", "InvalidStateError");
    const read = ++st.read;
    Object.assign(st, { error: null, result: null, transferred: 0, total: impl.size, readyState: LOADING });
    const bytes = readBlobImpl(impl);
    const textLabel = format === "text" && label !== undefined ? String(label) : null;
    const step = fn => global.__mrt.queueTask(() => { if (st.read === read && st.readyState === LOADING) fn(); });
    step(() => {
      progress(this, st, "loadstart");
      const end = () => {
        st.readyState = DONE;
        st.result = blobResult(format, bytes, impl, textLabel);
        progress(this, st, "load");
        progress(this, st, "loadend");
      };
      if (!bytes.length) { step(end); return; }
      step(() => {
        st.transferred = bytes.length;
        progress(this, st, "progress");
        step(end);
      });
    });
  };
  for (const [name, format] of [["readAsArrayBuffer", "arraybuffer"], ["readAsBinaryString", "binary"], ["readAsText", "text"], ["readAsDataURL", "dataurl"]]) {
    Object.defineProperty(FileReader.prototype, name, { value: { [name]: readAs(format, name) }[name], writable: true, enumerable: true, configurable: true });
  }
  for (const [k, v] of [["EMPTY", EMPTY], ["LOADING", LOADING], ["DONE", DONE]]) {
    Object.defineProperty(FileReader, k, { value: v, enumerable: true });
    Object.defineProperty(FileReader.prototype, k, { value: v, enumerable: true });
  }
  global.__mrt.defineEventHandlers(FileReader.prototype, ["onloadstart", "onprogress", "onload", "onabort", "onerror", "onloadend"]);
  Object.defineProperty(FileReader.prototype, Symbol.toStringTag, { value: "FileReader", configurable: true });
  define(global, "FileReader", FileReader);
  // FileReaderSync (workers only): the same results, at once
  if (isWorker) {
    class FileReaderSync {}
    for (const [name, format] of [["readAsArrayBuffer", "arraybuffer"], ["readAsBinaryString", "binary"], ["readAsText", "text"], ["readAsDataURL", "dataurl"]]) {
      const method = { [name](blob, label) {
        const impl = fileReaderArgs(`FileReaderSync.${name}`, arguments);
        return blobResult(format, readBlobImpl(impl), impl, format === "text" && label !== undefined ? String(label) : null);
      } }[name];
      Object.defineProperty(FileReaderSync.prototype, name, { value: method, writable: true, enumerable: true, configurable: true });
    }
    Object.defineProperty(FileReaderSync.prototype, Symbol.toStringTag, { value: "FileReaderSync", configurable: true });
    define(global, "FileReaderSync", FileReaderSync);
  }
  // The legacy factory functions (WebIDL LegacyFactoryFunction): `new` is required, and `prototype`
  // is the interface's prototype object. Gecko: HTMLImageElement::Image, HTMLAudioElement::Audio
  const legacyFactory = (name, iface, make) => {
    const F = function (...args) {
      if (!new.target) throw new TypeError(`${name} constructor: 'new' is required`);
      return make(...args);
    };
    Object.defineProperty(F, "name", { value: name });
    Object.defineProperty(F, "prototype", { value: global[iface].prototype });
    define(global, name, F);
  };
  if (!isWorker) legacyFactory("Image", "HTMLImageElement", (w, h) => {
    const img = document.createElement("img");
    if (w !== undefined) img.setAttribute("width", w);
    if (h !== undefined) img.setAttribute("height", h);
    return img;
  });
  // ---- Images (Gecko's nsImageLoadingContent and HTMLImageElement; GAPS B128, task 09c9) ----
  //
  // An <img> whose src is set (by script, the parser or innerHTML) loads in Firefox: the bridge tab
  // loads the URL as an image (mini_realsites.py's "image" request) and answers whether it loaded and
  // its size; no pixels come back (SPEC decision 11). The image task runs in a microtask after the
  // change (QueueImageTask's ImageLoadTask), or at once for an image Firefox's cache has and for a
  // src removed or emptied while there is a current request (QueueImageTask's sync load); load or
  // error is a task at the image's network time (FireEvent's AsyncEventDispatcher). A request
  // starts as the current one unless the current one has loaded; then it is pending until it ends
  // and becomes current (PrepareNextRequest, MakePendingRequestCurrent). Loads started before the
  // window's load event block it (the document's load group). Not here: srcset and <picture>
  // (ResponsiveImageSelector) and loading="lazy" load nothing, as before; an image in a tree
  // innerHTML made outside the document loads when the tree is inserted, not at once; a clone does
  // not load; layout still sizes images from their attributes.
  const imageStates = share("imageStates", () => new WeakMap());
  // the images that loaded, by URL, with their sizes: Firefox's image cache, which keeps them for the
  // document's life (nsContentUtils::IsImageAvailable)
  const imageCache = share("imageCache", () => new Map());
  const imageState = el => {
    let st = imageStates.get(el);
    if (!st) imageStates.set(el, st = { current: null, pending: null, task: false, force: false, decodeGeneration: 0, decodes: [] });
    return st;
  };
  // the decode() promises waiting on the current request (RejectDecodePromises, MaybeResolveDecodePromises)
  const settleDecodes = (st, error) => {
    const decodes = st.decodes;
    st.decodes = [];
    for (const d of decodes) {
      if (error) d.reject(new DOMException(error, "EncodingError"));
      else d.resolve(undefined);
    }
  };
  // A new current request (PrepareCurrentRequest): replacing one for another URL rejects what waits
  // on decode() (MaybeAgeRequestGeneration)
  const makeCurrent = (st, request) => {
    const changed = !!st.current && (!request || st.current.url !== request.url);
    st.current = request;
    if (changed) {
      st.decodeGeneration++;
      settleDecodes(st, "Invalid image request.");
    }
  };
  const imageSource = el => {
    if (el.hasAttribute("srcset") || (el.parentNode && el.parentNode.localName === "picture")) return undefined;
    if ((el.getAttribute("loading") || "").toLowerCase() === "lazy") return undefined;
    const src = el.getAttribute("src");
    if (src === null || src === "") return src;
    try { return resolve(src, pageURL.href); } catch (e) { return ""; }
  };
  // The image task (ImageLoadTask::Run, LoadSelectedImage). `force`: a src set by script, which loads
  // again even the URL it has (AfterMaybeChangeAttr); without it a src already loading or loaded stays
  function runImageTask(el, force) {
    const st = imageState(el);
    st.task = false;
    st.force = false;
    const url = imageSource(el);
    if (url === undefined) return;
    if (url === null) {
      // src removed: the requests are cancelled, nothing fires (CancelImageRequests)
      st.pending = null;
      makeCurrent(st, null);
      return;
    }
    const last = st.pending || st.current;
    if (!force && last && last.url === url) return;
    const request = { url, status: "loading", width: 0, height: 0 };
    const cached = url ? imageCache.get(url) : undefined;
    if (cached) {
      // from Firefox's cache: current and complete now, its load event still a task
      Object.assign(request, { status: "loaded", width: cached.width, height: cached.height });
      st.pending = null;
      makeCurrent(st, request);
      global.__mrt.queueTask(() => { if (st.current === request) el.dispatchEvent(new Event("load")); });
      return;
    }
    // the new request is current unless the current one has its size (PrepareNextRequest)
    if (url && st.current && st.current.status === "loaded") st.pending = request;
    else {
      st.pending = null;
      makeCurrent(st, request);
    }
    // an empty or invalid src fails, with an error event (LoadImage's empty URI)
    if (!url) {
      request.status = "broken";
      global.__mrt.queueTask(() => { if (st.current === request) el.dispatchEvent(new Event("error")); });
      return;
    }
    const unblock = blockOnload("image " + url);
    let r;
    try {
      const ask = { url, crossOrigin: el.getAttribute("crossorigin"), referrerPolicy: el.getAttribute("referrerpolicy") };
      if (isBlobURL(url)) {
        // a blob: URL of this runtime: Firefox gets its bytes
        const entry = __mrtBlobURL(2, url, global.origin, true, clockNow());
        if (entry) Object.assign(ask, { bodyBase64: toBase64(entry[0]), type: entry[1] });
      }
      r = isBlobURL(url) && !ask.bodyBase64 ? { ok: false, ms: 0 } : bridge("image", ask);
    } catch (e) { r = { ok: false, ms: 0 }; }
    // still loading in Firefox (mini_realsites.py PENDING_MS): it stays loading, no event
    if (r && r.pending) { unblock(); return; }
    afterNetwork(r, () => { try { finishImage(el, st, request, r); } finally { unblock(); } });
  }
  // The request ended (OnLoadComplete): a pending one becomes current, then load or error at the element
  function finishImage(el, st, request, r) {
    if (st.current !== request && st.pending !== request) return;
    request.status = r && r.ok ? "loaded" : "broken";
    if (r && r.ok) {
      request.width = r.width | 0;
      request.height = r.height | 0;
      if (!isBlobURL(request.url)) imageCache.set(request.url, { width: request.width, height: request.height });
    }
    if (st.pending === request) {
      st.pending = null;
      makeCurrent(st, request);
    }
    el.dispatchEvent(new Event(request.status === "loaded" ? "load" : "error"));
    if (st.current === request) settleDecodes(st, request.status === "loaded" ? null : "Invalid encoded image data.");
  }
  // QueueImageTask: at once when Firefox would load synchronously, else one microtask per element
  // (a newer change replaces the queued one's src, and a forced one stays forced)
  const queueImageTask = (el, force) => {
    const st = imageState(el);
    const url = imageSource(el);
    if (url !== undefined && ((url && imageCache.has(url) && (force || !st.current || st.current.url !== url)) || (!url && st.current))) {
      runImageTask(el, force);
      return;
    }
    st.force = st.force || force;
    if (st.task) return;
    st.task = true;
    Promise.resolve().then(() => { if (st.task) runImageTask(el, st.force); });
  };
  if (!isWorker) {
    global.__mrt.imageSrcChanged = el => queueImageTask(el, true);
    // inserted with a src the parser or innerHTML set: loads unless it already has that URL
    global.__mrt.imageInserted = el => { if (el.hasAttribute("src")) queueImageTask(el, false); };
    const imageGetter = (name, get) => getter(HTMLImageElement.prototype, name, get);
    const current = el => { const st = imageStates.get(el); return st && st.current; };
    imageGetter("naturalWidth", function () { const c = current(this); return c && c.status === "loaded" ? c.width : 0; });
    imageGetter("naturalHeight", function () { const c = current(this); return c && c.status === "loaded" ? c.height : 0; });
    // HTMLImageElement::Complete: no src (nor srcset); else no request pending nor task queued, and the
    // current one ended; without a current one, true unless the image is lazy
    imageGetter("complete", function () {
      if (!this.hasAttribute("srcset") && !this.getAttribute("src")) return true;
      const st = imageStates.get(this);
      if (st && (st.pending || st.task)) return false;
      if (!st || !st.current) return (this.getAttribute("loading") || "").toLowerCase() !== "lazy";
      return st.current.status !== "loading";
    });
    imageGetter("currentSrc", function () { const c = current(this); return c && c.url || ""; });
    // HTMLImageElement::Width and Height when the image is not rendered (GetWidthHeightForImage):
    // the attribute, else the image's size; the setters set the attribute
    for (const [name, natural] of [["width", "naturalWidth"], ["height", "naturalHeight"]]) {
      getter(HTMLImageElement.prototype, name, function () {
        const v = /^\s*\+?(\d+)/.exec(this.getAttribute(name) || "");
        return v ? Math.min(+v[1], 0x7fffffff) : this[natural];
      }, function (v) { this.setAttribute(name, String(Number(v) >>> 0)); });
    }
    // nsImageLoadingContent::QueueDecodeAsync and DecodeAsync: in a microtask, the current request's
    // (a pending one does not count); rejected when the current request changes URL before it ends
    define(HTMLImageElement.prototype, "decode", function decode() {
      const el = this;
      return new Promise((resolve, reject) => {
        const generation = imageState(el).decodeGeneration;
        Promise.resolve().then(() => {
          const st = imageState(el);
          if (generation !== st.decodeGeneration || !st.current) reject(new DOMException("Invalid image request.", "EncodingError"));
          else if (st.current.status === "broken") reject(new DOMException("Invalid encoded image data.", "EncodingError"));
          else if (st.current.status === "loaded") resolve(undefined);
          else st.decodes.push({ resolve, reject });
        });
      });
    });
  }
  // YouTube's voice search makes one when the masthead's data arrives; without it the navigation
  // that applies the watch page's data threw before the page was built
  if (!isWorker) legacyFactory("Audio", "HTMLAudioElement", src => {
    const audio = document.createElement("audio");
    audio.setAttribute("preload", "auto");
    if (src !== undefined) audio.src = String(src);
    return audio;
  });
  // Gecko's CSS::Supports: two arguments, a property and a value (Servo_CSSSupports2); one, a
  // condition such as "(display: grid) and selector(:has(a))" or a bare declaration (Servo_CSSSupports).
  // Instagram reveals its server-rendered app by removing `content-visibility: hidden` only when
  // CSS.supports("content-visibility", "hidden"); it was false here, so the app stayed hidden.
  // (one named parameter: WebIDL gives an overloaded operation the length of its shortest overload)
  define(CSS, "supports", function supports(property) {
    if (arguments.length === 0) throw new TypeError("CSS.supports: At least 1 argument required, but only 0 passed");
    if (arguments.length >= 2) return __mrtStyle(8, "", String(property), String(arguments[1]), false) === "1";
    return __mrtStyle(9, String(property), "", "", false) === "1";
  });

  // HTMLDivElement and the others are native interfaces since session 38 (glue/html_interfaces.h)
  for (const m of ["focus", "blur", "print", "alert", "stop", "moveTo", "resizeTo"]) define(global, m, () => {});
  define(global, "scroll", function scroll() { scrollTo(arguments, "Window.scroll"); });
  define(global, "scrollTo", function scrollTo_() { scrollTo(arguments, "Window.scrollTo"); });
  define(global, "scrollBy", function scrollBy_() { scrollBy(arguments, "Window.scrollBy"); });
  define(global, "confirm", () => false);
  define(global, "prompt", () => null);
  define(global, "open", () => null);
  define(global, "close", () => {});

  // ---- IndexedDB (dom/indexedDB), forwarded to Firefox (SPEC decision 12) ----
  //
  // The records, key order, indexes and cursor positions are Firefox's: every request goes through
  // the bridge to the bridge tab's real IndexedDB (bench/idb_bridge.js), which keeps each transaction
  // open until this side commits or aborts it. Kept here is what a page observes: the objects, as
  // handles; Gecko's transaction states (IDBTransaction.cpp); results delivered as tasks in request
  // order, as BackgroundRequestChild delivers the parent's responses; the events. A request is sent
  // when it is made, so Firefox does every check and raises its own errors; when this side's
  // transaction is not active, Firefox's is not either, and it raises TransactionInactiveError.
  // Values and keys cross as JSON (clone_codec.js). The registry of open databases and their live
  // transactions (`idbDatabases`, `IDBDatabase._transactions`) is what hibernation must look at
  // (DESIGN "IndexedDB, forwarded to Firefox").
  const CODEC = (
// @CLONE_CODEC@
  );
  const INTERNAL = {};
  const CLONE_FAIL = {};
  const queueTask = fn => global.__mrt.queueTask(fn);
  const idbClient = String(Date.now()) + ":" + String(Math.random());
  let idbNative = false, idbPollQueued = false;
  const idbDatabases = new Map(); // Firefox's id -> IDBDatabase, while open
  const isPlatform = v => (!isWorker && v instanceof Node) || v instanceof EventTarget || isPort(v) || keyStates.has(v);
  // a Blob's bytes for the codec; an object that only claims to be a Blob is cloned as an object
  const readBlob = (v, set) => {
    const impl = blobImplOf(v);
    if (impl) set(readBlobImpl(impl));
    return !!impl;
  };
  const cloneOptions = { isPlatform, readBlob, fail: CLONE_FAIL };
  const encodeArg = a => {
    if (a instanceof IDBKeyRange) return { range: [encodeKey(a._lower), encodeKey(a._upper), a._lowerOpen, a._upperOpen] };
    try {
      return { v: CODEC.encode(a, cloneOptions) };
    } catch (e) {
      if (e === CLONE_FAIL) return { fail: true };
      throw e;
    }
  };
  const encodeKey = k => CODEC.encode(k, { fail: CLONE_FAIL });
  const decodeValue = j => CODEC.decode(j, global);
  const idbException = e => (e.name === "TypeError" ? new TypeError(e.message) : new DOMException(e.message, e.name));
  const IDB_MSG = {
    InvalidStateError: "An attempt was made to use an object that is not, or is no longer, usable",
    AbortError: "A request was aborted, for example through a call to IDBTransaction.abort.",
    NotAllowed: "A mutation operation was attempted on a database that did not allow mutations.",
    ReadOnlyError: "A mutation operation was attempted in a READ_ONLY transaction.",
  };
  // One call to the bridge tab's IndexedDB; what Firefox fired at the databases meanwhile is queued.
  // How long Firefox took over the last call: its answer is delivered that much later on the clock
  // (SPEC decision 20). An answer still pending in Firefox adds nothing: it waits on this runtime.
  let idbLastMs = 0, idbLastWait = 0;
  const idbPending = v => v !== null && typeof v === "object" && (v.pending !== undefined || (v.state && v.state.kind === "pending"));
  const afterIdb = fn => { const ms = idbLastMs; return ms > 0 ? global.__mrt.queueAt(fn, clockNow() + ms) : queueTask(fn); };
  // asking Firefox again for an answer it does not have yet: after the time Firefox waited for it in
  // the last call (at least a millisecond), so that the clock moves on as Firefox's did, and reaches
  // the deliveries Firefox's answer may wait for (an earlier transaction's, due later)
  const idbRetry = fn => global.__mrt.queueAt(fn, clockNow() + Math.max(1, idbLastWait));
  // The bridge tab dropped (bridge()'s bridgeLost): the transactions it held are gone, so they are
  // aborted here without asking it (`idbLostTx`), and the open databases are opened again at their
  // version in the new tab at the next IndexedDB call (`idbReopen`), under new ids. A database whose
  // version moved on or that was deleted meanwhile is closed, as Gecko closes a frozen page's database
  // on a version change (DESIGN "IndexedDB, forwarded to Firefox").
  // Every transaction until its complete or abort fired (`idbLiveTx`), its database open or not: a
  // transaction of a closed database still commits. The answers Firefox had not given yet
  // (`idbAwaited`: a request's or a commit's, an open's) are lost with the tab: each comes back as
  // an abort (`idbLostPending`).
  const idbLostTx = new Set();
  const idbLiveTx = new Set();
  const idbAwaited = new Set();
  const idbLostPending = new Set();
  let idbReopen = false;
  const LOST_ANSWER = { name: "AbortError", message: "A request was aborted, for example through a call to IDBTransaction.abort." };
  bridgeLostHooks.push(() => {
    const live = [...idbLiveTx];
    for (const id of idbAwaited) idbLostPending.add(id);
    idbAwaited.clear();
    for (const tx of live) idbLostTx.add(tx._id);
    if (live.length) calls["idb-lost-transactions"] = (calls["idb-lost-transactions"] || 0) + live.length;
    for (const tx of live) tx._abort(new DOMException(IDB_MSG.AbortError, "AbortError"));
    if (idbDatabases.size) idbReopen = true;
  });
  // the databases opened again; their old ids to the new ones
  function reopenDatabases() {
    idbReopen = false;
    const moved = new Map();
    for (const db of [...idbDatabases.values()]) {
      const r = bridge("idb", { call: "reopen", name: db._name, version: db._version }).value;
      idbDatabases.delete(db._id);
      calls["idb-reopen"] = (calls["idb-reopen"] || 0) + 1;
      if (r && r.id !== undefined) {
        moved.set(db._id, r.id);
        db._id = r.id;
        idbDatabases.set(db._id, db);
      } else {
        db._closed = true;
        calls["idb-reopen-closed"] = (calls["idb-reopen-closed"] || 0) + 1;
      }
    }
    return moved;
  }
  // Native connections in another window/worker need versionchange delivery
  // even while their owner makes no requests. Firefox's bridge has its own path.
  function pollNativeIdb() {
    idbPollQueued = false;
    if (!idbNative || !idbDatabases.size) return;
    try { idb("poll", {}); } catch (e) {}
    if (idbDatabases.size && !idbPollQueued) {
      idbPollQueued = true;
      global.__mrt.queueAt(pollNativeIdb, clockNow() + 100);
    }
  }
  function idb(call, args) {
    if (call === "result" && idbLostPending.has(args.id)) return { error: LOST_ANSWER, aborted: LOST_ANSWER };
    if (call === "openWait" && idbLostPending.has(args.req)) return { state: { kind: "error", error: LOST_ANSWER } };
    if (args.tx !== undefined && idbLostTx.has(args.tx)) {
      if (call === "abort" || call === "commit") return { aborted: null };
      throw new DOMException("A request was placed against a transaction which is currently not active, or which is finished.", "TransactionInactiveError");
    }
    if (idbReopen) {
      const moved = reopenDatabases();
      if (args.db !== undefined && moved.has(args.db)) args = Object.assign({}, args, { db: moved.get(args.db) });
    }
    const reply = bridge("idb", Object.assign({ call }, args));
    if (reply.native) {
      idbNative = true;
      if (call !== "poll" && !idbPollQueued) {
        idbPollQueued = true;
        global.__mrt.queueAt(pollNativeIdb, clockNow() + 100);
      }
    }
    const v = reply.value;
    if (call === "result" && !(v && v.pending !== undefined)) idbAwaited.delete(args.id);
    else if (v && v.pending !== undefined) idbAwaited.add(v.pending);
    if (call === "openWait" && !(v && v.state && v.state.kind === "pending")) idbAwaited.delete(args.req);
    else if ((call === "open" || call === "deleteDatabase") && v && v.state && v.state.kind === "pending") idbAwaited.add(v.req);
    idbLastMs = idbPending(reply.value) ? 0 : reply.ms || 0;
    idbLastWait = idbPending(reply.value) ? reply.ms || 0 : 0;
    // `__mrt.idbTrace = true` (from MRT_EVAL_BEFORE) logs every call and Firefox's reply
    if (global.__mrt.idbTrace) console.log("idb", JSON.stringify(Object.assign({ call }, args)).slice(0, 300), "->", JSON.stringify(reply).slice(0, 300));
    if (reply.events && reply.events.length) firefoxEvents(reply.events);
    const t = reply.throw || (reply.value && reply.value.throw);
    if (t) throw idbException(t);
    return reply.value;
  }
  // versionchange and forced close (BackgroundDatabaseChild::RecvVersionChange, RecvCloseAfterInvalidationComplete)
  const versionChangeSent = new Set();
  function firefoxEvents(events) {
    for (const ev of events) {
      const db = idbDatabases.get(ev.db);
      if (!db) continue;
      if (ev.type === "versionchange") versionChangeSent.add(db);
      queueTask(() => {
        if (ev.type === "versionchange") {
          if (!db._closed) db.dispatchEvent(new IDBVersionChangeEvent("versionchange", { oldVersion: ev.oldVersion, newVersion: ev.newVersion }));
        } else if (ev.type === "close") {
          db._closePending();
          for (const tx of db._transactions) tx._abort(new DOMException(IDB_MSG.AbortError, "AbortError"));
          db.dispatchEvent(new Event("close"));
        }
      });
    }
  }

  // CycleCollectedJSContext::CleanupIDBTransactions, called after each microtask checkpoint (the C++
  // layer's AfterProcessMicrotasks): what this task left to do for its transactions.
  let idbCleanup = [];
  define(global, "__mrtAfterMicrotasks", () => {
    while (idbCleanup.length) {
      const list = idbCleanup;
      idbCleanup = [];
      for (const f of list) f();
    }
  });

  class DOMStringList {
    constructor(token, list) {
      if (token !== INTERNAL) throw new TypeError("Illegal constructor.");
      this._list = list.slice();
      this._list.forEach((s, i) => { this[i] = s; });
    }
    get length() { return this._list.length; }
    item(i) { const s = this._list[i >>> 0]; return s === undefined ? null : s; }
    contains(s) { return this._list.includes(String(s)); }
    [Symbol.iterator]() { return this._list[Symbol.iterator](); }
  }
  const stringList = list => new DOMStringList(INTERNAL, list.slice().sort());

  class IDBVersionChangeEvent extends Event {
    constructor(type, init = {}) {
      super(type, init);
      this.oldVersion = init.oldVersion === undefined ? 0 : init.oldVersion;
      this.newVersion = init.newVersion === undefined ? null : init.newVersion;
    }
  }
  define(global, "IDBVersionChangeEvent", IDBVersionChangeEvent);

  class IDBRequest extends EventTarget {
    constructor(token, source, transaction) {
      if (token !== INTERNAL) throw new TypeError("Illegal constructor.");
      super();
      this._source = source;
      this._transaction = transaction;
      this._readyState = "pending";
      this._result = undefined;
      this._error = null;
    }
    // IDBRequest::GetResult and GetError: only once the request is done
    get result() { if (this._readyState !== "done") throw new DOMException(IDB_MSG.InvalidStateError, "InvalidStateError"); return this._result; }
    get error() { if (this._readyState !== "done") throw new DOMException(IDB_MSG.InvalidStateError, "InvalidStateError"); return this._error; }
    get source() { return this._source; }
    get transaction() { return this._transaction; }
    get readyState() { return this._readyState; }
    _eventTargetParent() { return this._transaction; }
    _setResult(result) { this._readyState = "done"; this._result = result; this._error = null; }
    _setError(error) { this._readyState = "done"; this._result = undefined; this._error = error; }
  }
  class IDBOpenDBRequest extends IDBRequest {}

  // The transaction's states as Gecko's IDBTransaction: active in the task that made it and while
  // one of its requests delivers; inactive otherwise; committing once the commit is sent; finished.
  class IDBTransaction extends EventTarget {
    constructor(token, db, id, mode, durability, stores) {
      if (token !== INTERNAL) throw new TypeError("Illegal constructor.");
      super();
      this._db = db;
      this._id = id;
      this._mode = mode;
      this._durability = durability;
      this._info = new Map(stores.map(s => [s.name, s]));
      this._scope = stores.map(s => s.name);
      this._stores = new Map();
      this._state = "active";
      this._pending = 0;
      this._started = false;
      this._sentCommitOrAbort = false;
      this._aborted = false;
      this._error = null;
      this._onFinish = null;
      this._responses = [];
      // IDBDatabase::RegisterTransaction, and the pending list cleaned up after this task
      // (nsContentUtils::AddPendingIDBTransaction; IDBTransaction::Run)
      db._transactions.add(this);
      idbLiveTx.add(this);
      idbCleanup.push(() => {
        if (this._state !== "active") return;
        this._state = "inactive";
        if (!this._started) this._sendCommit();
      });
    }
    get db() { return this._db; }
    get mode() { return this._mode; }
    get durability() { return this._durability; }
    get error() { return this._error; }
    get objectStoreNames() { return stringList(this._mode === "versionchange" ? this._db._stores : this._scope); }
    _eventTargetParent() { return this._db; }
    _finishedOrCommitting() { return this._state === "committing" || this._state === "finished"; }
    objectStore(name) {
      name = String(name);
      if (this._finishedOrCommitting()) throw new DOMException("IDBTransaction.objectStore: Transaction is already committing or done.", "InvalidStateError");
      const cached = this._stores.get(name);
      if (cached) return cached;
      // not seen yet: Firefox answers, or raises NotFoundError
      const info = this._info.get(name) || idb("store", { tx: this._id, store: name });
      const store = new IDBObjectStore(INTERNAL, this, info);
      this._stores.set(name, store);
      return store;
    }
    abort() {
      if (this._finishedOrCommitting()) throw new DOMException(IDB_MSG.NotAllowed, "InvalidStateError");
      this._abort(null);
    }
    commit() {
      if (this._state !== "active") throw new DOMException(IDB_MSG.InvalidStateError, "InvalidStateError");
      this._state = "committing";
      this._sendCommit();
    }
    // OnNewRequest, then the response delivered as a task (BackgroundRequestChild::HandleResponse):
    // `apply(reply)` sets the request's result from Firefox's reply
    _request(request, reply, apply) {
      if (!this._pending) this._started = true;
      this._pending++;
      this._responses.push({ request, reply, apply });
      this._lastDeliveryAt = Math.max(this._lastDeliveryAt || 0, clockNow() + idbLastMs);
      afterIdb(() => this._deliverNext());
    }
    // The oldest response not delivered yet: Firefox answers a transaction's requests in order. One it
    // has not answered (the transaction waits for an earlier one on the same stores, which may finish
    // only after another of this page's tasks) is asked for again behind the tasks queued meanwhile;
    // the transaction's other deliveries wait behind it.
    _deliverNext() {
      const next = this._responses[0];
      if (!next) return;
      if (next.waiting) return idbRetry(() => this._deliverNext());
      next.waiting = true;
      this._whenAnswered(next.reply, reply => {
        this._responses.shift();
        // a check Firefox could make only once the transaction started: reported as the request's error
        this._deliver(next.request, reply.throw ? { error: reply.throw } : reply, next.apply);
      });
    }
    _whenAnswered(reply, then, polls = 0) {
      if (reply.pending !== undefined) {
        const r = idb("result", { id: reply.pending });
        if (r.pending !== undefined) {
          if (polls < 300) return idbRetry(() => this._whenAnswered(reply, then, polls + 1));
          const none = { name: "UnknownError", message: "bridge: no answer from Firefox" };
          return then({ error: none, aborted: none });
        }
        if (idbLastMs > 0) return afterIdb(() => then(r));
        reply = r;
      }
      then(reply);
    }
    _deliver(request, reply, apply) {
      const committing = this._finishedOrCommitting();
      if (!committing) this._state = "active";
      let event;
      if (this._aborted || reply.error) {
        request._setError(this._aborted ? new DOMException(IDB_MSG.AbortError, "AbortError") : idbException(reply.error));
        event = new Event("error", { bubbles: true, cancelable: true });
        request.dispatchEvent(event);
        // DispatchErrorEvent: an error nobody prevented aborts the transaction with it
        if (!this._aborted && (event._exceptionWasRaised || !event.defaultPrevented)) {
          this._abort(event._exceptionWasRaised ? new DOMException(IDB_MSG.AbortError, "AbortError") : request._error);
        }
      } else {
        apply(reply);
        event = new Event("success");
        request.dispatchEvent(event);
        // DispatchSuccessEvent: an exception in a listener aborts the transaction
        if (event._exceptionWasRaised && !this._finishedOrCommitting()) this._abort(new DOMException(IDB_MSG.AbortError, "AbortError"));
      }
      // after this task's microtasks, which may have made more requests (the transaction is still
      // active for them, as in Firefox, where they run before the listener returns)
      idbCleanup.push(() => {
        if (this._state === "active") this._state = "inactive";
        this._onRequestFinished();
      });
    }
    _onRequestFinished() {
      this._pending--;
      if (this._pending || this._sentCommitOrAbort) return;
      if (this._state === "inactive") this._state = "committing";
      this._sendCommit();
    }
    _sendCommit() {
      if (this._sentCommitOrAbort) return;
      this._sentCommitOrAbort = true;
      if (this._state !== "committing") this._state = "committing";
      const result = idb("commit", { tx: this._id });
      afterIdb(() => this._whenAnswered(result, r => this._fireCompleteOrAbort(r)));
    }
    // AbortInternal: the transaction finishes; requests not yet delivered fail with AbortError;
    // `abort` fires after them
    _abort(error) {
      if (this._aborted || this._state === "finished") return;
      this._aborted = true;
      this._error = error;
      this._state = "finished";
      const sent = this._sentCommitOrAbort;
      this._sentCommitOrAbort = true;
      let result = { aborted: null };
      try { result = idb("abort", { tx: this._id }); } catch (e) {}
      // after the requests' error events already on their way (AbortInternal fires them first)
      const fire = () => this._whenAnswered(result, r => this._fireCompleteOrAbort(r));
      if (!sent) {
        if ((this._lastDeliveryAt || 0) > clockNow()) global.__mrt.queueAt(() => queueTask(fire), this._lastDeliveryAt);
        else queueTask(fire);
      }
    }
    // FireCompleteOrAbortEvents
    _fireCompleteOrAbort(result) {
      this._state = "finished";
      this._db._transactions.delete(this);
      idbLiveTx.delete(this);
      if (this._mode === "versionchange") this._db._versionChange = null;
      if (result && result.complete && !this._aborted) {
        this.dispatchEvent(new Event("complete"));
      } else {
        this._aborted = true;
        if (!this._error && result && result.aborted) this._error = idbException(result.aborted);
        if (this._mode === "versionchange") this._db._revert();
        this.dispatchEvent(new Event("abort", { bubbles: true }));
      }
      if (this._onFinish) this._onFinish();
    }
  }

  // A request on a store, an index or a cursor: made in Firefox at once (it checks the arguments),
  // its result delivered later
  function makeRequest(tx, source, target, method, args, apply) {
    const reply = idb("request", Object.assign({ tx: tx._id, method, args: Array.from(args, encodeArg), inactive: tx._state !== "active" }, target));
    const request = new IDBRequest(INTERNAL, source, tx);
    tx._request(request, reply, apply || (r => request._setResult(decodeValue(r.result))));
    return request;
  }
  function openCursor(tx, source, target, method, args) {
    const request = makeRequest(tx, source, target, method, args, r => {
      if (r.result === null) return request._setResult(null);
      const cursor = method === "openCursor" ? new IDBCursorWithValue(INTERNAL) : new IDBCursor(INTERNAL);
      cursor._init(request, source, tx, r.cursor, args.length > 1 && args[1] !== undefined ? String(args[1]) : "next");
      cursor._apply(r);
    });
    return request;
  }
  const keyPathOf = k => (Array.isArray(k) ? k.slice() : k);

  class IDBObjectStore {
    constructor(token, tx, info) {
      if (token !== INTERNAL) throw new TypeError("Illegal constructor.");
      this._tx = tx;
      this._name = info.name;
      this._keyPath = keyPathOf(info.keyPath);
      this._autoIncrement = info.autoIncrement;
      this._indexNames = info.indexNames.slice();
      this._indexes = new Map();
    }
    get name() { return this._name; }
    set name(v) {
      v = String(v);
      idb("schema", { tx: this._tx._id, method: "renameStore", store: this._name, name: v, inactive: this._tx._state !== "active" });
      const db = this._tx._db;
      db._stores = db._stores.map(n => (n === this._name ? v : n));
      this._tx._stores.delete(this._name);
      this._tx._stores.set(v, this);
      this._name = v;
    }
    get keyPath() { return this._keyPath; }
    get indexNames() { return stringList(this._indexNames); }
    get transaction() { return this._tx; }
    get autoIncrement() { return this._autoIncrement; }
    _target() { return { store: this._name }; }
    put(value, key) { return makeRequest(this._tx, this, this._target(), "put", arguments); }
    add(value, key) { return makeRequest(this._tx, this, this._target(), "add", arguments); }
    delete(query) { return makeRequest(this._tx, this, this._target(), "delete", arguments); }
    clear() { return makeRequest(this._tx, this, this._target(), "clear", arguments); }
    get(query) { return makeRequest(this._tx, this, this._target(), "get", arguments); }
    getKey(query) { return makeRequest(this._tx, this, this._target(), "getKey", arguments); }
    getAll(query, count) { return makeRequest(this._tx, this, this._target(), "getAll", arguments); }
    getAllKeys(query, count) { return makeRequest(this._tx, this, this._target(), "getAllKeys", arguments); }
    count(query) { return makeRequest(this._tx, this, this._target(), "count", arguments); }
    openCursor(query, direction) { return openCursor(this._tx, this, this._target(), "openCursor", arguments); }
    openKeyCursor(query, direction) { return openCursor(this._tx, this, this._target(), "openKeyCursor", arguments); }
    index(name) {
      name = String(name);
      // IDBObjectStore::Index
      if (this._tx._finishedOrCommitting()) throw new DOMException(IDB_MSG.InvalidStateError, "InvalidStateError");
      const cached = this._indexes.get(name);
      if (cached) return cached;
      const index = new IDBIndex(INTERNAL, this, idb("index", { tx: this._tx._id, store: this._name, name }));
      this._indexes.set(name, index);
      return index;
    }
    createIndex(name, keyPath, options) {
      if (arguments.length < 2) throw new TypeError(`IDBObjectStore.createIndex: At least 2 arguments required, but only ${arguments.length} passed`);
      const opts = options && typeof options === "object" ? { unique: !!options.unique, multiEntry: !!options.multiEntry } : {};
      const info = idb("schema", { tx: this._tx._id, method: "createIndex", store: this._name, name: String(name),
                                   keyPath: typeof keyPath === "string" ? keyPath : Array.from(keyPath, String), options: opts, inactive: this._tx._state !== "active" });
      this._indexNames.push(info.name);
      const index = new IDBIndex(INTERNAL, this, info);
      this._indexes.set(info.name, index);
      return index;
    }
    deleteIndex(name) {
      name = String(name);
      idb("schema", { tx: this._tx._id, method: "deleteIndex", store: this._name, name, inactive: this._tx._state !== "active" });
      this._indexNames = this._indexNames.filter(n => n !== name);
      this._indexes.delete(name);
    }
  }

  class IDBIndex {
    constructor(token, store, info) {
      if (token !== INTERNAL) throw new TypeError("Illegal constructor.");
      this._store = store;
      this._name = info.name;
      this._keyPath = keyPathOf(info.keyPath);
      this._unique = info.unique;
      this._multiEntry = info.multiEntry;
    }
    get name() { return this._name; }
    set name(v) {
      v = String(v);
      const tx = this._store._tx;
      idb("schema", { tx: tx._id, method: "renameIndex", store: this._store._name, index: this._name, name: v, inactive: tx._state !== "active" });
      this._store._indexNames = this._store._indexNames.map(n => (n === this._name ? v : n));
      this._store._indexes.delete(this._name);
      this._store._indexes.set(v, this);
      this._name = v;
    }
    get objectStore() { return this._store; }
    get keyPath() { return this._keyPath; }
    get unique() { return this._unique; }
    get multiEntry() { return this._multiEntry; }
    _target() { return { store: this._store._name, index: this._name }; }
    get(query) { return makeRequest(this._store._tx, this, this._target(), "get", arguments); }
    getKey(query) { return makeRequest(this._store._tx, this, this._target(), "getKey", arguments); }
    getAll(query, count) { return makeRequest(this._store._tx, this, this._target(), "getAll", arguments); }
    getAllKeys(query, count) { return makeRequest(this._store._tx, this, this._target(), "getAllKeys", arguments); }
    count(query) { return makeRequest(this._store._tx, this, this._target(), "count", arguments); }
    openCursor(query, direction) { return openCursor(this._store._tx, this, this._target(), "openCursor", arguments); }
    openKeyCursor(query, direction) { return openCursor(this._store._tx, this, this._target(), "openKeyCursor", arguments); }
  }

  // The cursor's position comes from Firefox at each step; its request fires again (IDBCursor.cpp)
  class IDBCursor {
    constructor(token) {
      if (token !== INTERNAL) throw new TypeError("Illegal constructor.");
    }
    _init(request, source, tx, id, direction) {
      this._request = request;
      this._source = source;
      this._tx = tx;
      this._id = id;
      this._direction = direction;
      this._haveValue = false;
      this._continueCalled = false;
    }
    _apply(r) {
      this._continueCalled = false;
      this._haveValue = r.result !== null;
      if (r.result === null) {
        this._request._setResult(null);
        return;
      }
      this._key = decodeValue(r.key);
      this._primaryKey = decodeValue(r.primaryKey);
      if ("value" in r) this._value = decodeValue(r.value);
      this._request._setResult(this);
    }
    get source() { return this._source; }
    get direction() { return this._direction; }
    get key() { return this._key; }
    get primaryKey() { return this._primaryKey; }
    get request() { return this._request; }
    // The checks Gecko makes on the cursor's own state (IDBTypedCursor::Continue, Advance,
    // ContinuePrimaryKey, Update, Delete), in its order. Those it makes first are left to Firefox's
    // cursor: an inactive transaction, a zero count, a primary key on the wrong cursor. Firefox's
    // cursor is already at its next position once the bridge answers, so it cannot tell whether this
    // one has been delivered.
    _checkState(method, args) {
      const tx = this._tx;
      if (method === "advance" && !(Number(args[0]) >= 1)) return;
      if (tx._state !== "active") return;
      const write = method === "update" || method === "delete";
      if (write && tx._mode === "readonly") throw new DOMException(IDB_MSG.ReadOnlyError, "ReadOnlyError");
      if (method === "continuePrimaryKey" && (this._source instanceof IDBObjectStore || (this._direction !== "next" && this._direction !== "prev"))) return;
      if (!this._haveValue || this._continueCalled || (write && !(this instanceof IDBCursorWithValue))) {
        throw new DOMException(IDB_MSG.NotAllowed, "InvalidStateError");
      }
    }
    _move(method, args) {
      this._checkState(method, args);
      const tx = this._tx;
      const reply = idb("cursor", { tx: tx._id, cursor: this._id, method, args: Array.from(args, encodeArg), inactive: tx._state !== "active" });
      // IDBRequest::Reset
      this._request._readyState = "pending";
      this._request._result = undefined;
      this._continueCalled = true;
      tx._request(this._request, reply, r => this._apply(r));
    }
    advance(count) { this._move("advance", arguments); }
    continue(key) { this._move("continue", arguments); }
    continuePrimaryKey(key, primaryKey) { this._move("continuePrimaryKey", arguments); }
    update(value) { this._checkState("update", arguments); return makeRequest(this._tx, this, { cursor: this._id }, "update", arguments); }
    delete() { this._checkState("delete", arguments); return makeRequest(this._tx, this, { cursor: this._id }, "delete", arguments); }
  }
  class IDBCursorWithValue extends IDBCursor {
    get value() { return this._value; }
  }

  class IDBDatabase extends EventTarget {
    constructor(token, info) {
      if (token !== INTERNAL) throw new TypeError("Illegal constructor.");
      super();
      this._id = info.id;
      this._name = info.name;
      this._version = info.version;
      this._stores = info.stores.slice();
      this._transactions = new Set();
      this._closed = false;
      this._versionChange = null;
      idbDatabases.set(this._id, this);
    }
    get name() { return this._name; }
    get version() { return this._version; }
    get objectStoreNames() { return stringList(this._stores); }
    transaction(storeNames, mode, options) {
      if (arguments.length < 1) throw new TypeError("IDBDatabase.transaction: At least 1 argument required, but only 0 passed");
      const stores = typeof storeNames === "string" ? [storeNames] : Array.from(storeNames, String);
      const args = { db: this._id, stores, mode: mode === undefined ? "readonly" : String(mode) };
      if (options && typeof options === "object" && options.durability !== undefined) args.durability = String(options.durability);
      const r = idb("transaction", args);
      return new IDBTransaction(INTERNAL, this, r.tx, r.mode, r.durability, r.stores);
    }
    close() {
      this._closePending();
      idb("close", { db: this._id });
    }
    _closePending() {
      this._closed = true;
      idbDatabases.delete(this._id);
    }
    // a versionchange transaction that aborted: the database is back as it was, and closed
    _revert() {
      this._version = this._oldVersion;
      this._stores = this._oldStores;
      this._closePending();
    }
    _schema(method, args) {
      const tx = this._versionChange;
      if (!tx || tx._state === "finished") throw new DOMException(IDB_MSG.NotAllowed, "InvalidStateError");
      return idb("schema", Object.assign({ tx: tx._id, method, inactive: tx._state !== "active" }, args));
    }
    createObjectStore(name, options) {
      if (arguments.length < 1) throw new TypeError("IDBDatabase.createObjectStore: At least 1 argument required, but only 0 passed");
      const opts = {};
      if (options && typeof options === "object") {
        if (options.keyPath !== undefined) opts.keyPath = options.keyPath === null || typeof options.keyPath === "string" ? options.keyPath : Array.from(options.keyPath, String);
        if (options.autoIncrement !== undefined) opts.autoIncrement = !!options.autoIncrement;
      }
      const info = this._schema("createObjectStore", { name: String(name), options: opts });
      this._stores.push(info.name);
      const tx = this._versionChange;
      tx._info.set(info.name, info);
      const store = new IDBObjectStore(INTERNAL, tx, info);
      tx._stores.set(info.name, store);
      return store;
    }
    deleteObjectStore(name) {
      name = String(name);
      this._schema("deleteObjectStore", { name });
      this._stores = this._stores.filter(n => n !== name);
      this._versionChange._info.delete(name);
      this._versionChange._stores.delete(name);
    }
  }

  class IDBKeyRange {
    constructor(token, lower, upper, lowerOpen, upperOpen) {
      if (token !== INTERNAL) throw new TypeError("Illegal constructor.");
      this._lower = lower;
      this._upper = upper;
      this._lowerOpen = lowerOpen;
      this._upperOpen = upperOpen;
    }
    get lower() { return this._lower; }
    get upper() { return this._upper; }
    get lowerOpen() { return this._lowerOpen; }
    get upperOpen() { return this._upperOpen; }
    includes(key) {
      if (arguments.length < 1) throw new TypeError("IDBKeyRange.includes: At least 1 argument required, but only 0 passed");
      return idb("includes", { range: encodeArg(this), key: encodeArg(key) });
    }
    // Firefox checks the keys and their order (DataError); the bounds are the keys as Firefox converts them
    static _make(method, args, lower, upper, lowerOpen, upperOpen) {
      idb("keyRange", { method, args: Array.from(args, encodeArg) });
      const copy = k => (k === undefined ? undefined : decodeValue(encodeKey(k)));
      return new IDBKeyRange(INTERNAL, copy(lower), copy(upper), lowerOpen, upperOpen);
    }
    static only(value) { return IDBKeyRange._make("only", arguments, value, value, false, false); }
    static lowerBound(lower, open = false) { return IDBKeyRange._make("lowerBound", arguments, lower, undefined, !!open, true); }
    static upperBound(upper, open = false) { return IDBKeyRange._make("upperBound", arguments, undefined, upper, true, !!open); }
    static bound(lower, upper, lowerOpen = false, upperOpen = false) {
      return IDBKeyRange._make("bound", arguments, lower, upper, !!lowerOpen, !!upperOpen);
    }
  }

  // An open or delete request's states, as Firefox fires them, each delivered as a task
  function openStep(request, fid, state, isDelete, waits = 0) {
    afterIdb(() => {
      switch (state.kind) {
        case "pending":
          // still running in Firefox (blocked by another connection): ask again, for a while
          if (waits < 30) idbRetry(() => openWait(request, fid, isDelete, waits + 1));
          return;
        case "blocked": {
          // Firefox's side cannot close a connection for the page, so it reports blocked; Gecko fires
          // it only when a connection is still open once the versionchange events have run
          const open = [...versionChangeSent].some(db => !db._closed);
          versionChangeSent.clear();
          if (open) request.dispatchEvent(new IDBVersionChangeEvent("blocked", { oldVersion: state.oldVersion, newVersion: state.newVersion }));
          openWait(request, fid, isDelete, waits);
          return;
        }
        case "error":
          request._setError(idbException(state.error));
          request.dispatchEvent(new Event("error", { bubbles: true, cancelable: true }));
          return;
        case "success":
          if (isDelete) {
            request._setResult(undefined);
            request.dispatchEvent(new IDBVersionChangeEvent("success", { oldVersion: state.oldVersion, newVersion: null }));
          } else {
            let db = request._result;
            if (db instanceof IDBDatabase) {
              db._version = state.db.version;
              db._stores = state.db.stores.slice();
            } else db = new IDBDatabase(INTERNAL, state.db);
            request._transaction = null;
            request._setResult(db);
            request.dispatchEvent(new Event("success"));
          }
          return;
        case "upgradeneeded": {
          // IDBOpenDBRequest's upgradeneeded with a versionchange transaction (CreateVersionChange)
          const db = new IDBDatabase(INTERNAL, state.db);
          db._oldVersion = state.oldVersion;
          db._oldStores = state.db.stores.slice();
          const tx = new IDBTransaction(INTERNAL, db, state.tx, "versionchange", "default", []);
          db._versionChange = tx;
          request._transaction = tx;
          request._setResult(db);
          tx._onFinish = () => {
            request._transaction = null;
            openWait(request, fid, false, 0);
          };
          const event = new IDBVersionChangeEvent("upgradeneeded", { oldVersion: state.oldVersion, newVersion: state.newVersion });
          request.dispatchEvent(event);
          if (event._exceptionWasRaised) tx._abort(new DOMException(IDB_MSG.AbortError, "AbortError"));
          return;
        }
      }
    });
  }
  function openWait(request, fid, isDelete, waits) {
    openStep(request, fid, idb("openWait", { req: fid }).state, isDelete, waits);
  }

  class IDBFactory {
    constructor(token) {
      if (token !== INTERNAL) throw new TypeError("Illegal constructor.");
    }
    open(name, version) {
      if (arguments.length < 1) throw new TypeError("IDBFactory.open: At least 1 argument required, but only 0 passed");
      const args = { name: String(name) };
      if (version !== undefined) args.version = version;
      const r = idb("open", args);
      const request = new IDBOpenDBRequest(INTERNAL, null, null);
      openStep(request, r.req, r.state, false);
      return request;
    }
    deleteDatabase(name) {
      if (arguments.length < 1) throw new TypeError("IDBFactory.deleteDatabase: At least 1 argument required, but only 0 passed");
      const r = idb("deleteDatabase", { name: String(name) });
      const request = new IDBOpenDBRequest(INTERNAL, null, null);
      openStep(request, r.req, r.state, true);
      return request;
    }
    databases() {
      return new Promise((resolve, reject) => {
        let list;
        try { list = idb("databases", {}); } catch (e) { queueTask(() => reject(e)); return; }
        queueTask(() => resolve(list));
      });
    }
    cmp(a, b) {
      if (arguments.length < 2) throw new TypeError(`IDBFactory.cmp: At least 2 arguments required, but only ${arguments.length} passed`);
      return idb("cmp", { a: encodeArg(a), b: encodeArg(b) });
    }
  }
  for (const C of [IDBFactory, IDBDatabase, IDBTransaction, IDBObjectStore, IDBIndex, IDBCursor, IDBCursorWithValue, IDBKeyRange,
                   IDBRequest, IDBOpenDBRequest, IDBVersionChangeEvent, DOMStringList]) {
    Object.defineProperty(C.prototype, Symbol.toStringTag, { value: C.name, configurable: true });
    if (!Object.prototype.hasOwnProperty.call(global, C.name)) define(global, C.name, C);
  }
  define(global, "indexedDB", new IDBFactory(INTERNAL));
  // the registry hibernation must look at: open databases and their live transactions
  global.__mrt.indexedDB = () => Array.from(idbDatabases.values(), db => ({ name: db.name, version: db.version, transactions: db._transactions.size }));
  // what a drop of the bridge tab would lose: the transactions not finished, of open or closed
  // databases, and the answers Firefox has not given yet (realsite's MRT_BRIDGE_DROP waits for none)
  global.__mrt.indexedDBLive = () => idbLiveTx.size + idbAwaited.size;

  // ---- Trusted Types at the sinks (GAPS B91; Gecko's TrustedTypeUtils.cpp) ----
  //
  // The document's CSP as Firefox parsed it: the page's from the bridge tab (env.csp), and a frame
  // whose document is about:blank or about:srcdoc inherits its parent's (its policy container).
  // Frames from the network and workers have none here (GAPS B91). In a window that requires Trusted
  // objects (require-trusted-types-for 'script'), each sink's value goes through the compliance check
  // (prelude.js's trustedTypes.compliant): a Trusted object of the sink's type gives its string, any
  // other value the default policy's, or a violation. Elsewhere the sinks take Trusted objects as the
  // strings they stringify to, as they did. eval and Function are checked by the C++ layer's hooks.
  if (!isWorker) {
    const tt = global.__mrt.trustedTypes;
    const inherited = isFrameWindow && pageURL.protocol === "about:" ? global.parent.__mrt.trustedTypes.policies : null;
    tt.setPolicies(inherited || (isFrameWindow ? [] : env.csp || []));
    if (tt.requireState() > 0) {
      // a sink's own work may use another sink (a script's src setter sets the attribute): only the
      // outer one checks, as each is one sink in Gecko
      let inSink = 0;
      const checked = (value, type, sink, context) => (inSink ? value : tt.compliant(value, type, sink, context));
      const within = f => { inSink++; try { return f(); } finally { inSink--; } };
      // a setter whose value is checked (the error's context names the interface that declares it)
      const setterSink = (proto, name, type, sink, iface = sink.split(" ")[0]) => {
        const d = Object.getOwnPropertyDescriptor(proto, name);
        if (!d || !d.set) return;
        const set = d.set;
        Object.defineProperty(proto, name, { ...d, set: { [name](v) { const s = checked(v, type, sink, `${iface}.${name} setter`); within(() => set.call(this, s)); } }[name] });
      };
      // a method whose argument at `index` is checked (all of them for document.write)
      const methodSink = (proto, name, index, type, sink) => {
        const f = proto[name];
        if (typeof f !== "function") return;
        const context = sink.split(" ")[0] + "." + name;
        const wrapped = { [name](...args) {
          // document.write(...text) (Document::WriteCommon): the arguments joined, trusted when each
          // is a TrustedHTML, else checked as one string
          if (index < 0) {
            const data = args.map(a => tt.dataOf(a, type));
            args = [data.every(d => d !== undefined) ? data.join("") : checked(args.length === 1 ? args[0] : args.join(""), type, sink, context)];
          }
          else if (args.length > index) args[index] = checked(args[index], type, sink, context);
          return within(() => f.apply(this, args));
        } }[name];
        Object.defineProperty(wrapped, "length", { value: f.length });
        define(proto, name, wrapped);
      };
      setterSink(Element.prototype, "innerHTML", "TrustedHTML", "Element innerHTML");
      setterSink(ShadowRoot.prototype, "innerHTML", "TrustedHTML", "ShadowRoot innerHTML");
      setterSink(Element.prototype, "outerHTML", "TrustedHTML", "Element outerHTML");
      methodSink(Element.prototype, "insertAdjacentHTML", 1, "TrustedHTML", "Element insertAdjacentHTML");
      methodSink(Range.prototype, "createContextualFragment", 0, "TrustedHTML", "Range createContextualFragment");
      methodSink(DOMParser.prototype, "parseFromString", 0, "TrustedHTML", "DOMParser parseFromString");
      methodSink(Document.prototype, "write", -1, "TrustedHTML", "Document write");
      methodSink(Document.prototype, "writeln", -1, "TrustedHTML", "Document writeln");
      setterSink(HTMLIFrameElement.prototype, "srcdoc", "TrustedHTML", "HTMLIFrameElement srcdoc");
      setterSink(HTMLScriptElement.prototype, "src", "TrustedScriptURL", "HTMLScriptElement src");
      setterSink(HTMLScriptElement.prototype, "text", "TrustedScript", "HTMLScriptElement text");
      // HTMLScriptElement's own textContent and innerText (HTMLScriptElement.webidl)
      for (const [name, from] of [["textContent", Node.prototype], ["innerText", HTMLElement.prototype]]) {
        const d = Object.getOwnPropertyDescriptor(from, name);
        if (d) Object.defineProperty(HTMLScriptElement.prototype, name, { ...d, set: { [name](v) {
          const s = checked(v, "TrustedScript", "HTMLScriptElement " + name, `HTMLScriptElement.${name} setter`);
          within(() => d.set.call(this, s));
        } }[name] });
      }
      // the attributes that are sinks (GetTrustedTypesCompliantAttributeValue): event handlers, an
      // iframe's srcdoc, a script's src
      const NS = { "http://www.w3.org/1999/xhtml": "html", "http://www.w3.org/2000/svg": "svg", "http://www.w3.org/1998/Math/MathML": "mathml" };
      const attributeValue = (el, name, attrNs, value, context) => {
        const ns = NS[el.namespaceURI];
        const data = ns && tt.dataForAttribute(el.localName, ns, name, attrNs);
        return data ? checked(value, data[0], data[1], context) : value;
      };
      const setAttribute = Element.prototype.setAttribute;
      define(Element.prototype, "setAttribute", { setAttribute(name, value) {
        if (arguments.length < 2) return setAttribute.apply(this, arguments);
        // an HTML element in an HTML document matches its attributes' names in lower case
        const n = this.namespaceURI === "http://www.w3.org/1999/xhtml" ? String(name).toLowerCase() : String(name);
        return setAttribute.call(this, name, attributeValue(this, n, "none", value, "Element.setAttribute"));
      } }.setAttribute);
      const setAttributeNS = Element.prototype.setAttributeNS;
      if (typeof setAttributeNS === "function") define(Element.prototype, "setAttributeNS", { setAttributeNS(ns, qname, value) {
        if (arguments.length < 3) return setAttributeNS.apply(this, arguments);
        const attrNs = ns === null || ns === "" ? "none" : String(ns) === "http://www.w3.org/1999/xlink" ? "xlink" : null;
        const local = String(qname).slice(String(qname).indexOf(":") + 1);
        return setAttributeNS.call(this, ns, qname, attrNs ? attributeValue(this, local, attrNs, value, "Element.setAttributeNS") : value);
      } }.setAttributeNS);
    }
  }

  // ---- the missing-API log ----

  // For every name Firefox has and the mini runtime does not: counted when a script reads it or
  // tests for it with `in`, and still missing to the script (undefined, and `in` is false). A proxy
  // at the end of the prototype chains sees exactly those lookups, since nothing before it has the
  // name: one after EventTarget.prototype (the window, documents and nodes), one after navigator.
  // (A getter per name, as before session 38, made `in` true and broke feature detection.)
  const missing = share("missing", () => new Map());
  function logMissingAt(end, lists) {
    const labels = new Map();  // name -> [[label, test of the receiver]]
    for (const [label, holder, names, isReceiver] of lists) {
      for (const name of names) {
        if (name in holder || name === "constructor") continue;
        if (!labels.has(name)) labels.set(name, []);
        labels.get(name).push([label, isReceiver]);
      }
    }
    const count = (name, receiver) => {
      const candidates = typeof name === "string" && labels.get(name);
      if (!candidates) return;
      // the label of the receiver's kind; `in` has no receiver, so the first one
      const hit = (receiver !== undefined && candidates.find(([, is]) => is(receiver))) || candidates[0];
      const key = hit[0] + "." + name;
      missing.set(key, (missing.get(key) || 0) + 1);
    };
    const target = Object.getPrototypeOf(end);
    Object.setPrototypeOf(end, new Proxy(target, {
      has(t, name) { count(name); return Reflect.has(t, name); },
      get(t, name, receiver) { count(name, receiver); return Reflect.get(t, name, receiver); },
    }));
  }

  // ---- what the driver calls ----

  // ---- The bridge tab as the page's screen (realsite --interactive, SPEC decision 25) ----
  //
  // The bridge tab shows a copy of this document, which Firefox draws (the tab has JS off). Each
  // node sent has an id; `sent` keeps per node what the tab has (its attributes, text, child ids and
  // sheet edit count), and a flush walks the document and sends what differs, in order:
  //   ["c", parent, [child, ...]]  the parent's children: an id for a node the tab has, a spec for a
  //                                new one ({i, n: name, s: namespace, a: attributes, c: children,
  //                                r: open shadow root} for an element, {i, t} text, {i, m} comment)
  //   ["a", id, [[name, value], ...]]  an element's attributes
  //   ["d", id, data]               a text or comment node's data
  //   ["s", id, [rule text, ...]]   the rules of a <style> or <link>'s sheet a script changed
  //   ["adopted-root", id, sheets]  shadow adopted sheets ({text, media, disabled}), also in root specs
  //   ["r", host, spec]             an open shadow root attached since
  //   ["u", href]                   the document's URL, after pushState or replaceState
  //   ["history", index, count, ack] HTTP toolbar history availability and completed command task
  //   ["adopted", text]             the document's adopted sheets, as one <style> in the tab
  //   ["v", id, value, checked]     a form control's value and checkedness, once they are not what
  //                                 its attributes and text give
  //   ["f", id, spec or null]       (HTTP viewer) the document of a frame element now, or none
  //   ["fu", id, url]               (HTTP viewer) a frame document's base URL, changed since
  //   ["fadopted", id, text]        (HTTP viewer) a frame document's adopted sheets, as one <style>
  // In the HTTP viewer a frame element of a window of this runtime carries its document as `f`:
  // {i, u (its base URL), c (its children), adopted}, walked by that window's own code (the copy has
  // each node once, so the windows share this state, and ids are unique across the documents).
  // A walk rather than a MutationObserver: it also sees CSSOM edits and nodes changed while out of
  // the document, and puts no observer beside the page's. <noscript> is a comment (the tab has JS
  // off, and would show its contents). The document is id 0.
  // (`byId`: each id's node at the last flush, for the owner's input, which names nodes by id;
  // `windows`: the window that mirrored each node, whose fireInput runs the input on it (a frame's).
  // Unlike `sent` it outlives a detach, so input queued before one still reaches its frame.)
  const mirror = share("viewerMirror", () => ({ on: false, next: 1, ids: new WeakMap(), windows: new WeakMap(), snapshot: null, pendingSnapshot: null, sent: new WeakMap(), byId: new Map(), last: -Infinity, href: "", adopted: "", history: "", historyAck: 0 }));
  const XHTML = "http://www.w3.org/1999/xhtml";
  // the DOM's own methods, as they were before the page's scripts, which may wrap them
  const domOwn = (proto, name) => {
    const d = Object.getOwnPropertyDescriptor(proto, name);
    return d ? (d.get || d.value) : null;
  };
  // (none in a worker, which has no DOM)
  const DOM_OWN = typeof Element === "undefined" ? {} : {
    attributeNames: domOwn(Element.prototype, "getAttributeNames"), attribute: domOwn(Element.prototype, "getAttribute"),
    childNodes: domOwn(Node.prototype, "childNodes"), shadowRoot: domOwn(Element.prototype, "shadowRoot"),
    inputValue: domOwn(HTMLInputElement.prototype, "value"), inputChecked: domOwn(HTMLInputElement.prototype, "checked"),
    textareaValue: domOwn(HTMLTextAreaElement.prototype, "value"),
    setInputValue: Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set,
    setInputChecked: Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "checked").set,
    setTextareaValue: Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set,
  };
  const domCall = (fn, self, ...args) => (fn ? Reflect.apply(fn, self, args) : undefined);
  const childNodes = node => domCall(DOM_OWN.childNodes, node) || node.childNodes;
  const shadowOf = el => (DOM_OWN.shadowRoot ? domCall(DOM_OWN.shadowRoot, el) : el.shadowRoot);
  const mirrorKids = node => childNodes(node.nodeType === 1 && node.localName === "template" && node.namespaceURI === XHTML ? node.content : node);
  const isNoscript = node => node.nodeType === 1 && node.localName === "noscript" && node.namespaceURI === XHTML;
  const attrList = el => domCall(DOM_OWN.attributeNames, el).map(a => [a, domCall(DOM_OWN.attribute, el, a) ?? ""]);
  // A media element's resource through the API process (HTTP viewer, GAPS B172): the src of a
  // <video>, an <audio> or a <source> in one goes to the viewer as a URL of the viewer service
  // (http_viewer.py /media/<handle>), which loads it with this page's origin and cookie jar; a
  // runtime Blob's bytes go along, as the src setter took them (`_mediaBlob`), else as the table
  // has them now (a URL revoked since still loads for Gecko's RELEASING_TIMER, `parsedLive`; one
  // set with setAttribute and revoked longer ago does not). One handle per element, its value and
  // crossorigin; it is
  // released when they change or the element is collected. A MediaSource's blob: URL, or one this
  // runtime does not hold, goes as it is (the viewer's MediaSource has its own).
  const mediaResources = {
    held: new WeakMap(), released: [],
    finalizer: typeof FinalizationRegistry === "undefined" ? null : new FinalizationRegistry(h => mediaResources.released.push(h)),
    url(el, value) {
      const media = isMedia(el) ? el : el.parentNode;
      if (!/\S/.test(value) || (el._mediaSource && /^\s*blob:/i.test(value))) return value;
      const crossOrigin = media.getAttribute("crossorigin"), kind = media.localName;
      const held = this.held.get(el);
      if (held && held.value === value && held.crossOrigin === crossOrigin && held.kind === kind) return held.url;
      let url;
      try { url = new URL(value, document.baseURI).href; } catch (e) { return value; }
      const req = { url, crossOrigin, kind, release: this.released.splice(0) };
      if (held && held.handle) req.release.push(held.handle);
      if (isBlobURL(url)) {
        const taken = el._mediaBlob && el._mediaBlob.url === url ? el._mediaBlob.entry : null;
        const entry = taken || __mrtBlobURL(2, url, global.origin, true, clockNow());
        if (!entry) return value;
        el._mediaBlob = null;
        Object.assign(req, { b64: toBase64(entry[0]), type: entry[1] });
      }
      let r = null;
      try { r = bridge("mediaResource", req); } catch (e) {}
      if (this.finalizer) this.finalizer.unregister(el);
      // a resource the viewer cannot have fails to load there, with the element's error event
      const next = { value, crossOrigin, kind, url: r ? r.url : "about:invalid", handle: r && r.handle };
      this.held.set(el, next);
      if (r && this.finalizer) this.finalizer.register(el, r.handle, el);
      return next.url;
    },
    // the handles of collected elements, given back at a flush
    flush() {
      if (this.released.length) { try { bridge("mediaResource", { release: this.released.splice(0) }); } catch (e) {} }
    },
  };
  const isMediaSource = el => el.namespaceURI === XHTML && el.localName === "source" && el.parentNode &&
    el.parentNode.nodeType === 1 && isMedia(el.parentNode);
  // an element's attributes as the copy has them
  const mirrorAttrs = el => {
    const a = attrList(el);
    if (!env.httpViewer || el.namespaceURI !== XHTML || !(isMedia(el) || isMediaSource(el))) return a;
    return a.map(([k, v]) => (k === "src" ? [k, mediaResources.url(el, v)] : [k, v]));
  };
  const sheetRules = el => { try { return Array.from(el.sheet ? el.sheet.cssRules : [], r => r.cssText); } catch (e) { return []; } };
  // a form control's value and checkedness as JSON (null for other elements), read with the DOM's
  // own getters (React puts a value property of its own on each input); and what the tab gives a new
  // one from its attributes and text
  const isControl = el => el.namespaceURI === XHTML && (el.localName === "input" || el.localName === "textarea");
  const controlState = el => (!isControl(el) ? null : el.localName === "input"
    ? JSON.stringify([domCall(DOM_OWN.inputValue, el), domCall(DOM_OWN.inputChecked, el)])
    : JSON.stringify([domCall(DOM_OWN.textareaValue, el), false]));
  const controlDefault = el => {
    if (el.localName === "textarea") return JSON.stringify([el.textContent.replace(/\r\n?/g, "\n"), false]);
    const t = (el.getAttribute("type") || "").toLowerCase();
    const v = el.getAttribute("value") ?? (t === "checkbox" || t === "radio" ? "on" : "");
    return JSON.stringify([v, el.hasAttribute("checked")]);
  };
  function mirrorControl(el, rec, ops) {
    const state = controlState(el);
    if (state !== null && state !== rec.v) { rec.v = state; ops.push(["v", rec.i, ...JSON.parse(state)]); }
  }
  // the spec of a node the tab does not have, its subtree's ids given now; a node the tab has (one
  // the page moved into a new parent) is its id, moved there and walked for what changed, so the
  // tab keeps its copy (a <video>'s copy holds what the tab plays, task 753cc)
  function mirrorSpec(node, ops) {
    const sent = mirror.sent.get(node);
    if (sent) { mirrorWalk(node, ops); return sent.i; }
    const i = mirror.ids.get(node) || mirror.next++;
    mirror.ids.set(node, i);
    if (node.nodeType === 3) { mirror.sent.set(node, { i, d: node.data }); return { i, t: node.data }; }
    if (node.nodeType !== 1 || isNoscript(node)) {
      const data = node.nodeType === 8 ? node.data : "mrt-noscript";
      mirror.sent.set(node, { i, d: data, leaf: true });
      return { i, m: data };
    }
    if (env.httpViewer && isMedia(node)) mediaLink.els.set(i, new WeakRef(node));
    const a = mirrorAttrs(node);
    const rec = { i, a: JSON.stringify(a), e: cssomEdits.get(node) || 0, k: "" };
    mirror.windows.set(node, global);
    mirror.sent.set(node, rec);
    mirror.byId.set(i, node);
    const spec = { i, n: node.localName, a };
    if (node.namespaceURI !== XHTML) spec.s = node.namespaceURI;
    const kids = Array.from(mirrorKids(node), k => mirrorSpec(k, ops));
    rec.k = kids.map(k => (typeof k === "number" ? k : k.i)).join(",");
    spec.c = kids;
    if (env.httpViewer && isFrameElement(node)) {
      const doc = mirrorFrame(node, ops);
      rec.f = doc ? (typeof doc === "number" ? doc : doc.i) : 0;
      if (doc) spec.f = doc;
    }
    if (rec.e) ops.push(["s", i, sheetRules(node)]);
    if (isControl(node)) { rec.v = controlDefault(node); mirrorControl(node, rec, ops); }
    const root = shadowOf(node);
    if (root) spec.r = mirrorRoot(root, ops);
    return spec;
  }
  // Each adopter has scoped state. Cache CSS serialization across roots sharing a sheet;
  // ordinary mirror ticks compare only identities, edit versions, media and disabled state.
  const mirrorAdoptedText = new WeakMap();
  function mirrorAdopted(root, rec) {
    const sheets = Array.from(root.adoptedStyleSheets);
    const state = sheets.map(s => [s._id, constructedEdits.get(s) || 0, s.media.mediaText, s.disabled]);
    const signature = JSON.stringify(state);
    if (signature === rec.adopted) return null;
    rec.adopted = signature;
    return sheets.map((sheet, i) => {
      const version = state[i][1];
      let cached = mirrorAdoptedText.get(sheet);
      if (!cached || cached.version !== version) {
        cached = {version, text: Array.from(sheet.cssRules, r => r.cssText).join("\n")};
        mirrorAdoptedText.set(sheet, cached);
      }
      return {text: cached.text, media: state[i][2], disabled: state[i][3]};
    });
  }
  function mirrorRoot(root, ops) {
    const i = mirror.ids.get(root) || mirror.next++;
    mirror.ids.set(root, i);
    const rec = { i, k: "" };
    mirror.sent.set(root, rec);
    mirror.byId.set(i, root);
    const kids = Array.from(childNodes(root), k => mirrorSpec(k, ops));
    rec.k = kids.map(k => (typeof k === "number" ? k : k.i)).join(",");
    return { i, o: root.mode, c: kids, adopted: mirrorAdopted(root, rec) };
  }
  // a node the tab has: what changed in it and below it
  function mirrorWalk(node, ops) {
    const rec = mirror.sent.get(node);
    mirror.byId.set(rec.i, node);
    if (node.nodeType === 3 || rec.leaf) {
      if (node.nodeType !== 1 && rec.d !== node.data) { rec.d = node.data; ops.push(["d", rec.i, node.data]); }
      return;
    }
    if (node.nodeType === 1) {
      const a = mirrorAttrs(node), text = JSON.stringify(a);
      if (text !== rec.a) { rec.a = text; ops.push(["a", rec.i, a]); }
      const e = cssomEdits.get(node) || 0;
      if (e !== rec.e) { rec.e = e; ops.push(["s", rec.i, sheetRules(node)]); }
      if (rec.v !== undefined) mirrorControl(node, rec, ops);
      if (rec.f !== undefined) {
        // its document walked; a new one (the frame navigated) replaces the copy's
        const at = ops.push(null) - 1;
        const doc = mirrorFrame(node, ops);
        const i = doc ? (typeof doc === "number" ? doc : doc.i) : 0;
        if (i !== rec.f || (doc && typeof doc === "object")) { rec.f = i; ops[at] = ["f", rec.i, doc]; }
        else ops.splice(at, 1);
      }
      const root = shadowOf(node);
      if (root && !mirror.sent.has(root)) {
        const at = ops.push(null) - 1;
        ops[at] = ["r", rec.i, mirrorRoot(root, ops)];
      }
      else if (root) {
        mirror.byId.set(mirror.sent.get(root).i, root);
        const rootRec = mirror.sent.get(root);
        const sheets = mirrorAdopted(root, rootRec);
        if (sheets !== null) ops.push(["adopted-root", rootRec.i, sheets]);
        mirrorChildren(root, rootRec, ops);
      }
    }
    mirrorChildren(node, rec, ops);
  }
  function mirrorChildren(parent, rec, ops) {
    const kids = Array.from(parent.nodeType === 9 ? Array.from(childNodes(parent)).filter(k => k.nodeType === 1 || k.nodeType === 8) : mirrorKids(parent));
    const ids = kids.map(k => { const r = mirror.sent.get(k); return r ? r.i : 0; }).join(",");
    if (ids !== rec.k) {
      // (its place taken first: the new nodes' own ops, as their sheets' rules, come after it)
      const at = ops.push(null) - 1;
      const specs = kids.map(k => (mirror.sent.has(k) ? mirror.sent.get(k).i : mirrorSpec(k, ops)));
      rec.k = specs.map(k => (typeof k === "number" ? k : k.i)).join(",");
      ops[at] = ["c", rec.i, specs];
    }
    for (const k of kids) if (mirror.sent.has(k)) mirrorWalk(k, ops);
  }
  // The HTTP viewer's frames: a frame element's window, if this runtime has it, mirrors its own
  // document (its __mrt.mirrorDocument): the document's spec when the copy does not have it yet, else
  // its id, walked for what changed; null without a window here (a frame of another runtime's: the
  // copy's frame stays empty).
  const isFrameElement = el => el.namespaceURI === XHTML && (el.localName === "iframe" || el.localName === "frame");
  function mirrorFrame(el, ops) {
    const win = frameWindowOf(el);
    if (!win || crossOriginWindowsHeld.has(win) || __mrtWindows(5, win)) return null;
    return win.__mrt.mirrorDocument(ops);
  }
  // a document's base URL as the copy's <base> gives it: an about:blank or about:srcdoc frame's is its
  // parent's (it has no URL of its own to resolve against)
  const mirrorBase = () => {
    if (!isFrameWindow || pageURL.protocol !== "about:") return pageURL.href;
    try { return global.parent.__mrt.mirrorBase(); } catch (e) { return pageURL.href; }
  };
  const adoptedText = () => (document.adoptedStyleSheets || []).map(sh => Array.from(sh.cssRules, r => r.cssText).join("\n")).join("\n");
  // what changed since the last flush, sent to the bridge tab; at most every 100 ms unless `force`
  function mirrorFlush(force) {
    if (!mirror.on) return 0;
    // the runtime's clock, not the page's Date, which a page may replace (the replay's nearly still one)
    const now = global.__mrt.clockNow();
    if (!force && now - mirror.last < 100) return 0;
    mirror.last = now;
    const ops = [];
    // Even an empty document must replace the viewer's old children in a fresh snapshot.
    if (!mirror.sent.has(document)) mirror.sent.set(document, { i: 0, k: null });
    mirror.byId = new Map([[0, document]]);
    mirrorChildren(document, mirror.sent.get(document), ops);
    if (pageURL.href !== mirror.href) { mirror.href = pageURL.href; ops.push(["u", pageURL.href]); }
    if (env.httpViewer) {
      const count = historyEntries().length;
      const state = JSON.stringify([historyState.index, count, mirror.historyAck]);
      if (mirror.snapshot || state !== mirror.history) {
        mirror.history = state;
        ops.push(["history", historyState.index, count, mirror.historyAck]);
      }
    }
    const adopted = adoptedText();
    if (adopted !== mirror.adopted) { mirror.adopted = adopted; ops.push(["adopted", adopted]); }
    // the page scrolled since: the tab moves there, after the nodes it scrolls to
    if (rootScroll.sent !== rootScroll.seq) { rootScroll.sent = rootScroll.seq; ops.push(["scroll", rootScroll.seq, rootScroll.x, rootScroll.y]); }
    // what the page asked of its media, once their elements are in the copy
    mediaLink.flush(ops);
    if (env.httpViewer) mediaResources.flush();
    if (ops.length || mirror.snapshot) bridge("mirror", { ops, snapshot: mirror.snapshot });
    mirror.snapshot = null;
    return ops.length;
  }

  // ---- Media in the bridge tab (realsite --interactive, SPEC decision 26, task 753cc) ----
  //
  // The copy's <video> and <audio> elements play in the bridge tab, in Firefox. What the page asks of
  // a media element, a MediaSource or a SourceBuffer goes to the tab as ["m", {o, el, seq, ...}]
  // mirror ops after a flush's DOM ops, once the element is in the copy (an element's ops wait until
  // then; `el` is its copy id, `seq` numbers the ops of all elements):
  //   attach {ms} / detach              a MediaSource's URL made the element's src, or replaced
  //   addsb {ms, sb, type}, rmsb, sbset {name, value}, changetype {type}, abort
  //   append {ms, sb, n, b64}, remove {ms, sb, n, start, end}
  //   duration {ms, value}, eos {ms, error}
  //   play {p}, pause, load, set {name, value}
  // The tab makes a real MediaSource for each (mini_realsites.py MEDIA_SETUP) and sends back, at the
  // waits (mediaEvents): each media event of a copy element, with the element's state then; the end
  // of each append and remove, with the buffer's ranges; each play()'s answer. The appended bytes go
  // to the tab's SourceBuffer, where Firefox keeps them as it would its own page's; the runtime keeps
  // none (SPEC decision 11). A number that JSON cannot carry (NaN, Infinity) is sent as its string.
  const mediaNum = v => (typeof v === "number" && !Number.isFinite(v) ? String(v) : v);
  const mediaValue = v => (typeof v === "string" ? Number(v) : v);
  const mediaLink = {
    on: () => mirror.on || !!env.httpViewer,
    available: false, epoch: 0, pendingBytes: 0,
    history: new Map(), historyBytes: 0, historyOps: 0,
    canRetain(bytes) {
      // JSON's base64 plus a conservative envelope, measured as UTF-16 storage.
      return env.mediaReplayUnlimited === true ||
        (this.historyBytes + Math.ceil(bytes / 3) * 8 + 1024 <= 32 * 1024 * 1024 && this.historyOps < 10000);
    },
    forget(ms) {
      const rec = this.history.get(ms);
      if (!rec) return;
      this.historyBytes -= rec.bytes; this.historyOps -= rec.ops.length;
      this.history.delete(ms);
    },
    lose(ms, message) {
      const obj = this.objs.get(ms), el = obj?._element;
      this.forget(ms);
      if (!el) return;
      for (const [id, [, reject, target]] of this.plays) if (target === el) {
        this.plays.delete(id);
        global.__mrt.queueTask(() => reject(new DOMException(message, "AbortError")));
      }
      for (const sb of obj._sourceBuffers._list) sb._abortBufferAppend();
      obj._detach(); el._mediaSource = null;
      el._buffered = []; el._seekable = []; el._readyState = 0; el._duration = NaN; el._mediaSuspended = false;
      const error = Object.create(MediaError.prototype);
      error._code = 1; error._message = message; el._error = error;
      queueEvent(el, "error"); queueEvent(el, "emptied");
    },
    next: 1, objs: new Map(), queues: new Map(), plays: new Map(), seq: 0,
    // each element sent ops by its copy id, held weakly: the tab's events still find one the page took
    // out of the document (Firefox pauses its copy then)
    els: new Map(),
    stats: { ops: 0, bytes: 0, events: 0, plays: 0 },
    // a MediaSource's or SourceBuffer's id, which the tab's object has too
    id(obj) {
      if (!obj._mediaId) { obj._mediaId = this.next++; this.objs.set(obj._mediaId, obj); }
      return obj._mediaId;
    },
    send(el, op) {
      if (env.httpViewer) {
        if (op.o === "detach") {
          for (const [ms, rec] of this.history) if (rec.el === el) this.forget(ms);
        } else if (op.o === "attach" || op.ms) {
          if (op.o === "attach") {
            this.forget(op.ms);
            this.history.set(op.ms, {el, ops: [], bytes: 0});
          }
          const rec = this.history.get(op.ms);
          if (rec) {
            if (op.o === "abort") {
              const at = rec.ops.findIndex(o => o.sb === op.sb && o.n === op.n && (o.o === "append" || o.o === "remove"));
              if (at >= 0) {
                const bytes = JSON.stringify(rec.ops[at]).length * 2;
                rec.ops.splice(at, 1); rec.bytes -= bytes; this.historyBytes -= bytes; this.historyOps--;
              }
            }
            const bytes = JSON.stringify(op).length * 2;
            if (env.mediaReplayUnlimited !== true &&
                (this.historyBytes + bytes > 32 * 1024 * 1024 || this.historyOps >= 10000)) {
              this.lose(op.ms, "Media replay quota exceeded; rebuild MediaSource"); return;
            }
            rec.ops.push(Object.assign({}, op)); rec.bytes += bytes;
            this.historyBytes += bytes; this.historyOps++;
          }
        }
        if (!this.available && op.o !== "attach") return;
      }
      if (op.b64) this.pendingBytes += op.b64.length * 3 / 4;
      op.seq = el._mediaSeq = ++this.seq;
      // a new load (AbortExistingLoads): the tab's events for the element from before it are dropped
      if (op.o === "load" || op.o === "attach" || op.o === "detach") el._mediaLoadSeq = op.seq;
      let q = this.queues.get(el);
      if (!q) this.queues.set(el, q = []);
      q.push(op);
    },
    // a source's or its buffers' op, sent with its element's (none once it is detached)
    msOp(ms, op) {
      if (!ms || !ms._element || !this.on()) return;
      op.ms = this.id(ms);
      this.send(ms._element, op);
    },
    // HTMLMediaElement::Play: paused at once, the promise settled by the tab's play()
    // (the current time held where it was: it moves on from now, not from the tab's last report;
    // an element that ended seeks to the start, as HTMLMediaElement::Play does)
    play(el) {
      if (env.httpViewer && !this.available) return Promise.reject(new DOMException("No media viewer attached", "NotSupportedError"));
      el._currentTime = this.currentTime(el);
      if (el._ended) { el._ended = false; el._currentTime = 0; el._seeking = true; }
      el._mediaAt = clockNow();
      el._paused = false;
      const p = this.next++;
      this.stats.plays++;
      return new Promise((resolve, reject) => { this.plays.set(p, [resolve, reject, el]); this.send(el, { o: "play", p }); });
    },
    pause(el) {
      el._currentTime = this.currentTime(el);
      el._mediaAt = clockNow();
      el._paused = true;
      this.send(el, { o: "pause" });
    },
    set(el, name, v) {
      v = name === "muted" ? !!v : +v;
      el["_" + name] = v;
      if (name === "currentTime") { el._seeking = true; el._mediaAt = clockNow(); }
      this.send(el, { o: "set", name, value: mediaNum(v) });
    },
    // the tab's current time as of its last event, moved on by the time since while it plays
    currentTime(el) {
      const t = el._currentTime || 0;
      if (el._mediaSuspended || el._paused !== false || el._seeking || !(el._readyState >= 3) || el._mediaAt === undefined) return t;
      const now = t + (clockNow() - el._mediaAt) / 1000 * (el._playbackRate ?? 1);
      return Number.isFinite(el._duration) ? Math.min(now, el._duration) : now;
    },
    // the queued ops of the elements now in the copy
    flush(ops) {
      if (env.httpViewer && this.available) {
        for (const [ms, entry] of this.history) {
          const rec = mirror.sent.get(entry.el);
          if (entry.epoch === this.epoch || !rec || mirror.byId.get(rec.i) !== entry.el) continue;
          // Accepted source work is already retained, including not-yet-flushed commands.
          const q = this.queues.get(entry.el) || [];
          const keep = q.filter(op => op.o !== "attach" && !op.ms);
          this.pendingBytes -= q.reduce((n, op) => n + (op.b64 ? op.b64.length * 3 / 4 : 0), 0);
          this.queues.set(entry.el, keep);
          entry.epoch = this.epoch;
          for (const op of entry.ops) ops.push(["m", Object.assign({}, op, {
            el:rec.i, seq:entry.el._mediaSeq || 0, replay:true
          })]);
          const el = entry.el;
          ops.push(["m", {o:"resume", replay:true, ms, el:rec.i, seq:el._mediaSeq || 0,
            currentTime:this.currentTime(el), paused:el.paused, volume:el.volume,
            muted:el.muted, playbackRate:el.playbackRate, defaultPlaybackRate:el.defaultPlaybackRate}]);
        }
      }
      if (env.httpViewer && mirror.snapshot) {
        for (const [id, ref] of this.els) {
          const el = ref.deref();
          if (!el) { this.els.delete(id); continue; }
          // (a runtime Blob's src loads in the viewer too, GAPS B172: only a MediaSource's is rebuilt)
          if (el._mediaSource) continue;
          ops.push(["m", {o:"restore", el:id, seq:el._mediaSeq || 0, currentTime:this.currentTime(el), volume:el.volume,
            muted:el.muted, playbackRate:el.playbackRate, defaultPlaybackRate:el.defaultPlaybackRate}]);
        }
      }
      for (const [el, q] of this.queues) {
        const rec = mirror.sent.get(el);
        if (!rec || mirror.byId.get(rec.i) !== el) continue;
        for (const op of q) { op.el = rec.i; ops.push(["m", op]); }
        this.els.set(rec.i, new WeakRef(el));
        this.stats.ops += q.length;
        this.pendingBytes -= q.reduce((n, op) => n + (op.b64 ? op.b64.length * 3 / 4 : 0), 0);
        this.queues.delete(el);
      }
    },
  };
  // HTTP decoder state is ephemeral; bounded source commands reconstruct it.
  global.__mrt.viewerMedia = state => {
    if (!env.httpViewer) return;
    if (state.epoch !== mediaLink.epoch) {
      mediaTypes.clear();
      if (mediaLink.available) {
        for (const [, reject] of mediaLink.plays.values())
          global.__mrt.queueTask(() => reject(new DOMException("Media viewer disconnected", "AbortError")));
        mediaLink.plays.clear(); mediaLink.queues.clear(); mediaLink.pendingBytes = 0;
        for (const ref of mediaLink.els.values()) {
          const el = ref.deref();
          if (!el) continue;
          el._currentTime = mediaLink.currentTime(el); el._mediaAt = clockNow();
          if (el._mediaSource) { el._mediaSuspended = true; queueEvent(el, "waiting"); }
          else if (el._paused === false) { el._paused = true; queueEvent(el, "pause"); }
        }
      }
    }
    mediaLink.epoch = state.epoch; mediaLink.available = state.attached;
  };
  // What the tab sent back since the last wait: {k: "el", el, ev, s, seq} a media event of a copy
  // element and its state, fired as a task with the state set just before (but what the page set
  // since the tab applied its op `seq`: paused, ended, currentTime, seeking, volume, muted, the rates);
  // {k: "sb", sb, n, buffered, dur, error} an append or remove ended; {k: "ms", ms, ev} the tab's
  // source opened; {k: "play", p, ok, name, message} a play()'s answer. Each is a task, in the
  // tab's order.
  const MEDIA_SET_HERE = new Set(["paused", "ended", "currentTime", "seeking", "volume", "muted", "playbackRate", "defaultPlaybackRate"]);
  const MEDIA_RANGES = new Set(["buffered", "seekable", "played"]);
  function mediaEvents(list) {
    const epoch = mediaLink.epoch;
    const queueMedia = fn => global.__mrt.queueTask(() => { if (!env.httpViewer || epoch === mediaLink.epoch) fn(); });
    for (const m of list) {
      if (m.k === "el") {
        mediaLink.stats.events++;
        queueMedia(() => {
          const el = mirror.byId.get(m.el) || mediaLink.els.get(m.el)?.deref();
          if (!el || !isMedia(el)) return;
          // fired for a load the page has since replaced: Gecko cancels such events (mCurrentLoadID),
          // and a page that already moved on would take an old resource's ended for the new one's
          if (m.seq < (el._mediaLoadSeq || 0)) return;
          if (m.resumed) el._mediaSuspended = false;
          const stale = m.seq < (el._mediaSeq || 0);
          for (const [k, v] of Object.entries(m.s)) {
            if (stale && MEDIA_SET_HERE.has(k)) continue;
            if (MEDIA_RANGES.has(k)) el["_" + k] = v.map(r => r.map(mediaValue));
            else if (k === "error") {
              if (!v) { el._error = null; continue; }
              const err = Object.create(MediaError.prototype);
              [err._code, err._message] = v;
              el._error = err;
            }
            else el["_" + k] = mediaValue(v);
          }
          el._mediaAt = clockNow();
          const e = new Event(m.ev);
          Object.assign(e, { isTrusted: true });
          el.dispatchEvent(e);
        });
      } else if (m.k === "loss") {
        queueMedia(() => mediaLink.lose(m.ms, m.message || "Media reconstruction failed; rebuild MediaSource"));
      } else if (m.k === "sb") {
        const sb = mediaLink.objs.get(m.sb);
        if (sb) queueMedia(() => sb._tabDone(m));
      } else if (m.k === "ms") {
        // the tab's source opened (attach): open here, its sourceopen fired now
        const ms = mediaLink.objs.get(m.ms);
        if (ms && ms._element && ms._readyState === "closed" && m.ev === "sourceopen") {
          queueMedia(() => { ms._readyState = "open"; ms.dispatchEvent(new Event("sourceopen")); });
        }
      } else if (m.k === "play") {
        const settle = mediaLink.plays.get(m.p);
        if (!settle) continue;
        queueMedia(() => {
          mediaLink.plays.delete(m.p);
          if (m.ok) settle[0]();
          else settle[1](new DOMException(m.message || "", m.name || "AbortError"));
        });
      }
    }
  }
  // The owner's input, caught in the bridge tab (mini_realsites.py's INPUT_CAPTURE): each event as
  // the tab's Firefox dispatched it, {type, iface, target, related, submitter, active, selection,
  // task, bubbles, cancelable, composed, fields, value?}, its nodes named by the copy's ids. Each
  // event is fired here on its node, trusted, with the tab's fields. The events of one widget event
  // (`task`: in Gecko, pointerdown, mousedown and the focus they move are one task) run in one task,
  // in the tab's order, and each group's task is queued once the one before has run, so a 0 ms timer
  // set in pointerdown runs before pointerup, as in Firefox, where the next widget event comes later.
  // Before each event, the focused element and the selection become the tab's, and so do a control's
  // value and checkedness (an input or change event): what Firefox's editor did there. The tab has
  // already done the default actions (focus, typing, scrolling); a click on a link the page did not
  // cancel, or a submit, would navigate, which the runtime does not do (GAPS B151): counted in
  // `input.unhandled`.
  const INPUT_INTERFACES = new Set(["Event", "UIEvent", "MouseEvent", "PointerEvent", "WheelEvent", "KeyboardEvent", "FocusEvent", "InputEvent", "SubmitEvent"]);
  const input = { events: 0, missed: 0, unhandled: [] };
  const inputGroups = [];
  let inputQueued = false;
  function inputEvents(list) {
    for (const ev of list) {
      const last = inputGroups[inputGroups.length - 1];
      if (last && last.task === ev.task) last.events.push(ev);
      else inputGroups.push({ task: ev.task, events: [ev] });
    }
    if (!inputQueued && inputGroups.length) queueInputGroup();
  }
  function queueInputGroup() {
    inputQueued = true;
    global.__mrt.queueTask(() => {
      const group = inputGroups.shift();
      try {
        for (const e of group.events) fireInput(e);
      } finally {
        inputQueued = false;
        if (inputGroups.length) queueInputGroup();
        else if (mirror.pendingSnapshot) {
          // Snapshot after this group's microtasks, preserving accepted input
          // effects before a replacement viewer sees the document.
          global.__mrt.queueTask(() => {
            const pending = mirror.pendingSnapshot;
            if (pending) global.__mrt.mirrorSnapshot(pending);
          });
        } else if (!mirror.on) mirror.byId.clear();
      }
    });
  }
  function fireInput(ev) {
    if (env.httpViewer && ev.type === "viewer-history") {
      if ((ev.delta !== -1 && ev.delta !== 1) || !Number.isSafeInteger(ev.task) || ev.task < 1) return;
      input.events++;
      navigation.input++;
      try { Reflect.apply(traverseHistory, pageHistory, [ev.delta]); }
      finally { navigation.input--; }
      // go() queues an in-document traversal; acknowledge after that task. For
      // document changes the driver freezes this runtime until commit or failure.
      global.__mrt.queueTask(() => { mirror.historyAck = ev.task; mirror.history = ""; });
      return;
    }
    const node = id => (id === null || id === undefined ? null : mirror.byId.get(id) || null);
    // The tab's scroll offset, once the tab has the page's last scroll (`scrollSeq`): the document
    // scrolled there before the event, as the owner saw it. A scroll or scrollend event of the tab's
    // is the document's, but for the tab's echo of the page's own scroll, whose events the runtime
    // fired itself (`echo`), and one from before the page's last scroll reached the tab.
    if (ev.scroll && ev.scrollSeq === rootScroll.seq) {
      const [x, y] = ev.scroll;
      if (x !== rootScroll.x || y !== rootScroll.y) setRootScroll(x, y);
    }
    if ((ev.type === "scroll" || ev.type === "scrollend") && (ev.echo || ev.scrollSeq !== rootScroll.seq)) return;
    const target = node(ev.target);
    if (!target) { input.missed++; return; }
    input.events++;
    // a node of a frame's document: its window dispatches the event, with its own focus, selection
    // and default actions (the frame element is focused here); not the page's scroll
    const owner = mirror.windows.get(target);
    if (owner && owner !== global) {
      if ("outerActive" in ev) focus.element = node(ev.outerActive);
      owner.__mrt.fireViewerInput(Object.assign({}, ev, { scroll: null }));
      return;
    }
    focus.element = node(ev.active);
    if (env.httpViewer && ev.type === "input" && Array.isArray(ev.editable) && ev.editSelection) {
      const host = node(ev.editSelection.host);
      if (host && host.isContentEditable && host.contains(target)) {
        // Native editing changed this inert browser subtree before its input event.
        // Keep compatible nodes (and React's fields on them); only browser-created
        // structure requires new runtime nodes. Reject excessive/active markup.
        let count = 0, size = 0;
        const tags = new Set(["div", "p", "span", "br", "b", "i", "u", "s", "strong", "em", "a", "code", "pre", "blockquote", "ul", "ol", "li"]);
        const valid = (spec, depth) => {
          if (++count > 2048 || depth > 64 || !spec || typeof spec !== "object") return false;
          if (typeof spec.t === "string") { size += spec.t.length; return size <= 65536; }
          return tags.has(spec.n) && Array.isArray(spec.c) && Array.isArray(spec.a) && spec.a.every(a => Array.isArray(a) && a.length === 2 && a.every(v => typeof v === "string") && !/^on/i.test(a[0])) && spec.c.every(c => valid(c, depth + 1));
        };
        if (ev.editable.every(s => valid(s, 0))) {
          const sync = (parent, specs) => {
            const old = Array.from(childNodes(parent));
            const wanted = specs.map((spec, index) => {
              let n = old[index];
              if (typeof spec.t === "string") {
                if (!n || n.nodeType !== 3) n = document.createTextNode(spec.t);
                else if (n.data !== spec.t) n.data = spec.t;
              } else {
                if (!n || n.nodeType !== 1 || n.localName !== spec.n) {
                  n = document.createElement(spec.n);
                  for (const [name, value] of spec.a) if (!/^(src|href|style)$/i.test(name)) n.setAttribute(name, value);
                }
                sync(n, spec.c);
              }
              return n;
            });
            let cursor = parent.firstChild;
            for (const n of wanted) {
              if (n === cursor) cursor = cursor.nextSibling;
              else parent.insertBefore(n, cursor);
            }
            const keep = new Set(wanted);
            for (const n of Array.from(childNodes(parent))) if (!keep.has(n)) n.remove();
          };
          sync(host, ev.editable);
        }
      }
    }
    if (ev.selection) {
      const [type, text, anchor, anchorOffset, focusNode, focusOffset] = ev.selection;
      selection._set(type, node(anchor), anchorOffset, node(focusNode), focusOffset, text);
    }
    // A controlled editor may have replaced the text nodes since the browser
    // captured the event. Resolve positions inside the stable editing host.
    if (ev.editSelection) {
      const {host: hostId, anchor, focus: end, a: anchorPoint, f: focusPoint} = ev.editSelection;
      const host = node(hostId);
      if (host && host.isContentEditable && host.contains(target) && Number.isSafeInteger(anchor) && Number.isSafeInteger(end) && anchor >= 0 && end >= 0) {
        const texts = [];
        const visit = n => { if (n.nodeType === 3) texts.push(n); else for (const c of childNodes(n)) visit(c); };
        visit(host);
        const point = (offset, endpoint) => {
          if (endpoint && Array.isArray(endpoint.path) && endpoint.path.length <= 64 && Number.isSafeInteger(endpoint.offset) && endpoint.offset >= 0) {
            let candidate = host;
            for (const i of endpoint.path) candidate = candidate && Number.isSafeInteger(i) && i >= 0 ? childNodes(candidate)[i] : null;
            if (candidate && endpoint.offset <= (candidate.nodeType === 3 ? candidate.data.length : childNodes(candidate).length)) {
              let prefix = 0, found = false;
              const before = n => {
                if (found) return;
                if (n === candidate) {
                  prefix += n.nodeType === 3 ? endpoint.offset : Array.from(childNodes(n)).slice(0, endpoint.offset).reduce((sum, c) => sum + c.textContent.length, 0);
                  found = true;
                } else if (n.nodeType === 3) prefix += n.data.length;
                else for (const c of childNodes(n)) before(c);
              };
              before(host);
              if (found && prefix === offset) return [candidate, endpoint.offset];
            }
          }
          for (const t of texts) { if (offset <= t.data.length) return [t, offset]; offset -= t.data.length; }
          const last = texts[texts.length - 1];
          return last ? [last, last.data.length] : [host, 0];
        };
        const a = point(anchor, anchorPoint), f = point(end, focusPoint);
        selection._set(anchor === end ? "Caret" : "Range", a[0], a[1], f[0], f[1], host.textContent.slice(Math.min(anchor, end), Math.max(anchor, end)));
      }
    }
    if (ev.value && isControl(target)) {
      const [value, checked] = ev.value;
      if (target.localName === "input") {
        // (only when it differs: a checkbox's value is its attribute, which setting would add)
        if (domCall(DOM_OWN.inputValue, target) !== value) domCall(DOM_OWN.setInputValue, target, value);
        if (domCall(DOM_OWN.inputChecked, target) !== checked) domCall(DOM_OWN.setInputChecked, target, checked);
      } else domCall(DOM_OWN.setTextareaValue, target, value);
      // the tab has it: no "v" op back
      const rec = mirror.sent.get(target);
      if (rec) rec.v = controlState(target);
    }
    const C = INPUT_INTERFACES.has(ev.iface) && global[ev.iface] ? global[ev.iface] : Event;
    const init = { bubbles: ev.bubbles, cancelable: ev.cancelable, composed: ev.composed };
    if (C !== Event && C !== global.SubmitEvent) init.view = global;
    const e = new C(ev.type, init);
    const extra = { isTrusted: true };
    if ("related" in ev) extra.relatedTarget = node(ev.related);
    if ("submitter" in ev) extra.submitter = node(ev.submitter);
    Object.assign(e, ev.fields, extra);
    navigation.input++;
    try { fireInputDefault(target, e, ev); } finally { navigation.input--; }
  }
  function fireInputDefault(target, e, ev) {
    target.dispatchEvent(e);
    if (e.defaultPrevented) return;
    // the default actions the tab cancelled or could not make (DESIGN "Top-level navigation in the
    // HTTP viewer"): a submit the page let through is the form's submission; a click follows a link,
    // and in the HTTP viewer (whose frame has no forms) submits; Enter there submits implicitly
    if (ev.type === "submit" && target.localName === "form") submitForm(target, e.submitter || null, { request: false, user: true });
    else if (ev.type === "click") activate(target, e, true, !!env.httpViewer);
    else if (ev.type === "keypress" && env.httpViewer) implicitSubmit(target, e);
  }

  // the document as the tab should have it, one line a node, for the driver to compare with the tab
  function mirrorDigest() {
    const out = [];
    const visit = (node, depth) => {
      const pad = " ".repeat(depth);
      if (node.nodeType === 3) out.push(pad + "#text " + JSON.stringify(node.data));
      else if (node.nodeType === 8) out.push(pad + "#comment " + JSON.stringify(node.data));
      else if (isNoscript(node)) out.push(pad + "#comment " + JSON.stringify("mrt-noscript"));
      else if (node.nodeType === 1) {
        // (nonce values left out: Firefox's tab hides them, GAPS B150; and a media element's blob:
        // src, which is the runtime's MediaSource here and the tab's own there)
        const attrs = attrList(node).filter(([a, v]) => !(a === "src" && isMedia(node) && v.startsWith("blob:")))
          .map(([a, v]) => [a, a === "nonce" ? "" : v]).sort();
        out.push(pad + (node.namespaceURI === XHTML ? "" : node.namespaceURI + " ") + node.localName + " " + JSON.stringify(attrs));
        const root = shadowOf(node);
        if (root) { out.push(pad + " #shadow-root " + root.mode); for (const k of childNodes(root)) visit(k, depth + 2); }
        for (const k of mirrorKids(node)) visit(k, depth + 1);
      }
    };
    for (const k of childNodes(document)) if (k.nodeType === 1 || k.nodeType === 8) visit(k, 0);
    return out.join("\n");
  }

  Object.assign(global.__mrt, {
    principalURL: () => cookiePrincipal(),
    setURL(u) {
      pageURL = new URL(u);
      principalURL = pageURL.href;
      // the runtime's first window: its site is the runtime's (SPEC decision 19)
      shared.rootURL = u;
    },
    // a frame's window after its document changed (prelude.js's loadFrame): the document's URL
    // (about:srcdoc, or the src resolved against the parent's), and readyState back to loading
    frameNavigated(url) {
      // (an absolute URL needs no base: a frame of another origin cannot read its parent's location)
      pageURL = new URL(url === "about:srcdoc" || /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(url) ? url : resolve(url, global.parent.location.href));
      principalURL = pageURL.protocol === "about:" ? inheritedPrincipal() : pageURL.href;
      readyState = "loading";
      loadGeneration++;
      onloadBlockCount = 0;
      onloadBlockers.clear();
      loadArmed = loadFired = false;
      onLoaded = null;
    },
    // a frame's document once it is in its window: as realsite loads the page's (loadScripts,
    // DOMContentLoaded, then load once nothing blocks it); `loaded` after the load event
    runDocument(loaded) {
      global.__mrt.loadScripts();
      global.__mrt.domContentLoaded();
      global.__mrt.documentLoaded(loaded);
    },
    blockOnload,
    onloadBlockers: () => Array.from(onloadBlockers, b => b.what),
    // The document is parsed and DOMContentLoaded fired: readyState complete and the window's load
    // event now if nothing blocks it, else in a task after the last unblocking; then `loaded`. A
    // cross-origin frame's runtime tells the driver, which passes the load on to its embedder.
    documentLoaded(loaded) {
      loadArmed = true;
      onLoaded = () => {
        if (isFrameRuntime) bridge("frameLoaded", { frame: rootFrameId });
        if (loaded) loaded();
      };
      fireLoad();
    },
    // as Document::SetReadyStateInternal: every change fires readystatechange on the document (not
    // bubbling), before the driver's DOMContentLoaded or load
    setReadyState(s) {
      readyState = s;
      document.dispatchEvent(new Event("readystatechange"));
    },
    // The page's document is parsed at its scripts' pace (Page::parse_step): the parser stops after
    // each <script> it pops and calls parserScript, after parsedLink for each <link> it inserted
    // since its last stop; parsingDone once the whole document is parsed. Firefox's order: a
    // parser-blocking or async script runs where the parser is, after the events of the sheets
    // before it; a deferred one (a classic script with defer and src, a module script without
    // async) waits for the end of the document (ScriptLoader's mDeferRequests); then readyState
    // "interactive" and the deferred scripts in tree order, before DOMContentLoaded. An async script
    // runs where the parser is, not when its fetch completes as in Firefox (the bridge's fetches
    // complete at once).
    parsedLink(el) { loadParsedLink(el); },
    parserScript(el) {
      // a script in a template's contents, or one a script took out of the document, does not run
      // (ScriptLoader::ProcessScriptElement needs it in the composed document)
      if (!el.isConnected || el.ownerDocument !== document) return;
      if (deferredScript(el)) { parserDeferred.push(el); return; }
      fireParsedLinkEvents();
      runScriptElement(el, false);
    },
    parsingDone() {
      this.setReadyState("interactive");
      const list = parserDeferred;
      parserDeferred = [];
      for (const el of list) runScriptElement(el, false);
    },
    // A frame's document's scripts, all after the whole document is parsed (GAPS B8: a frame's
    // document is still parsed in one go), in the order above: the parser-blocking and async ones
    // in tree order; then readyState "interactive"; then the deferred ones in tree order.
    loadScripts() {
      // the frames in the document as parsed, before its scripts: each gets its window, and its src
      // loads in a task, blocking the document's load
      __mrtWindows(9, document);
      const ordered = [...document.querySelectorAll("script, link")];
      const all = ordered.filter(el => el.tagName === "SCRIPT");
      for (const el of ordered) {
        if (el.tagName === "LINK") loadParsedLink(el);
        else if (!deferredScript(el)) { fireParsedLinkEvents(); runScriptElement(el, false); }
      }
      this.setReadyState("interactive");
      for (const el of all) if (deferredScript(el)) runScriptElement(el, false);
    },
    // realsite's DOMContentLoaded step: the events of the parsed links after the last script,
    // DOMContentLoaded, then the events of the links whose media does not match (before the window's
    // load, the driver's next step)
    domContentLoaded() {
      fireParsedLinkEvents();
      scrollInitialFragment();
      document.dispatchEvent(new Event("DOMContentLoaded", { bubbles: true }));
      const deferred = deferredLinkEvents;
      deferredLinkEvents = [];
      for (const [el, type] of deferred) el.dispatchEvent(new Event(type));
    },
    installMissingLog() {
      const n = env.names;
      logMissingAt(EventTarget.prototype, [
        ["window", global, n.window, r => r === global],
        ["Document", Document.prototype, n.Document, r => r instanceof Document],
        ["HTMLElement", HTMLElement.prototype, n.HTMLElement, r => r instanceof HTMLElement],
        ["Element", Element.prototype, n.Element, r => r instanceof Element],
        ["Node", Node.prototype, n.Node, r => r instanceof Node],
      ]);
      logMissingAt(navigator, [["Navigator", navigator, n.Navigator, () => true]]);
    },
    // The final DOM, for rendering in a Firefox tab (bench/mini_realsites.py RENDER=1): a <base> so
    // that relative stylesheets, images and links resolve, and no <noscript> (scripts ran here; the
    // render tab has JS off and would show it).
    // the bridge tab as the page's screen (realsite --interactive)
    // (HTTP viewer) this window's document for its frame element's spec (mirrorFrame): its spec the
    // first time, else its id once walked for what changed
    mirrorDocument(ops) {
      const doc = document;
      let rec = mirror.sent.get(doc);
      if (rec) {
        mirror.byId.set(rec.i, doc);
        mirrorChildren(doc, rec, ops);
        const base = mirrorBase(), adopted = adoptedText();
        if (base !== rec.u) { rec.u = base; ops.push(["fu", rec.i, base]); }
        if (adopted !== rec.adopted) { rec.adopted = adopted; ops.push(["fadopted", rec.i, adopted]); }
        return rec.i;
      }
      const i = mirror.ids.get(doc) || mirror.next++;
      mirror.ids.set(doc, i);
      rec = { i, k: "", u: mirrorBase(), adopted: adoptedText() };
      mirror.sent.set(doc, rec);
      mirror.windows.set(doc, global);
      mirror.byId.set(i, doc);
      const kids = Array.from(childNodes(doc)).filter(k => k.nodeType === 1 || k.nodeType === 8).map(k => mirrorSpec(k, ops));
      rec.k = kids.map(k => (typeof k === "number" ? k : k.i)).join(",");
      return { i, u: rec.u, c: kids, adopted: rec.adopted };
    },
    mirrorBase,
    // (HTTP viewer) an input event on a node of this frame's document, from the page's window's
    // queue (fireInput), run here as the page's window runs its own
    fireViewerInput(ev) { fireInput(ev); },
    mirrorNodeId(node) {
      let id = mirror.ids.get(node);
      if (!id) { id = mirror.next++; mirror.ids.set(node, id); }
      return id;
    },
    mirrorStart() { mirror.on = true; },
    // HTTP attachment retains node ids but resets incremental bookkeeping.
    mirrorSnapshot(session) {
      if (inputQueued) { mirror.pendingSnapshot = session; return 0; }
      mirror.pendingSnapshot = null;
      mirror.on = true;
      mirror.sent = new WeakMap();
      mirror.href = ""; mirror.adopted = "";
      mirror.snapshot = session;
      rootScroll.sent = -1;
      return mirrorFlush(true);
    },
    viewerMirror(enabled) {
      if (!enabled && mirror.on) {
        if (!inputQueued) mirror.byId.clear();
        mirror.pendingSnapshot = null;
        mirror.sent = new WeakMap();
      }
      mirror.on = enabled;
    },
    viewerViewport(width, height) {
      env.innerWidth = width; env.innerHeight = height;
      global.innerWidth = width; global.innerHeight = height;
    },
    mirrorFlush,
    inputEvents,
    mediaEvents,
    // at the stop: the check against the tab, then the copy's state dropped before the page is measured
    mirrorCheck() {
      if (!mirror.on) return;
      mirrorFlush(true);
      // each sheet's rule count, by the element's id in the tab
      const sheets = [];
      for (const el of document.querySelectorAll("style, link[rel~=stylesheet]")) {
        const rec = mirror.sent.get(el);
        if (rec) sheets.push([rec.i, el.localName, sheetRules(el).length]);
      }
      bridge("mirrorCheck", { digest: mirrorDigest(), sheets, input: Object.assign({}, input, { media: mediaLink.stats }) });
    },
    mirrorStop() { mirror.on = false; mirror.sent = new WeakMap(); mirror.byId = new Map(); },
    render() {
      const de = document.documentElement;
      if (!de) return;
      let base = null;
      if (document.head && !document.head.querySelector("base")) {
        base = document.createElement("base");
        base.setAttribute("href", pageURL.href);
        document.head.insertBefore(base, document.head.firstChild);
      }
      const out = ["<!DOCTYPE html>"];
      skipNoscript = true;
      try { serialize(de, out); } finally { skipNoscript = false; }
      if (base) base.remove();
      bridge("render", { html: out.join("") });
    },
    report(measured) {
      const log = [...missing].sort((a, b) => b[1] - a[1]);
      const stubs = [...stubbed].sort((a, b) => b[1] - a[1]);
      const [fetched, fetch_failed, in_map] = __mrtModuleStats();
      const modules = { fetched, fetch_failed, in_map };
      bridge("report", { data: Object.assign({}, measured, { scripts, modules, bridge_calls: calls, missing_apis: log, stubbed_apis: stubs, readyState, webgl: shared.webglStats || null }) });
    },
  });

  // A frame's window starts at its initial about:blank, already loaded (nsDocShell's initial
  // document); a frame's src is fetched through the bridge and loaded as a same-origin document. A
  // cross-origin src gets its own mini runtime (SPEC decision 15), not built yet: the frame stays at
  // about:blank (GAPS B52).
  if (isFrameWindow) {
    pageURL = new URL("about:blank");
    readyState = "complete";
  }
  const loadFrameDocument = global.__mrt.loadFrame;
  global.__mrt.loadFrame = (el, html, url, unblock = () => {}) => {
    if (html === null) {
      let abs;
      try { abs = resolve(url, pageURL.href); } catch (e) { unblock(); return; }
      if (new URL(abs).origin !== global.origin) {
        // of this runtime's site: a window in this runtime, as Firefox keeps a site's frames in one
        // process (SPEC decision 19); of another site: a runtime of its own
        const site = siteOf(abs);
        if (site !== null && site === siteOf(shared.rootURL)) openSameSiteFrame(el, abs, unblock);
        else {
          // its size comes from this window's next layout, in the refresh tick (nsSubDocumentFrame's
          // reflow sends a remote frame its dimensions, BrowserParent::UpdateDimensions); its load
          // starts now, as Firefox's does at insertion
          const time = performance.now();
          share("refresh", () => null).opens.push(() => openCrossOriginFrame(el, abs, unblock, time));
        }
        return;
      }
      let r;
      try {
        r = bridge("fetch", { url: abs, method: "GET", headers: [["accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"]] });
      } catch (e) { unblock(); return; }
      // the document keeps the fragment it was asked for, which Response.url never has (Gecko's
      // HttpBaseChannel::PropagateReferenceIfNeeded, across redirects too; GAPS D71)
      let docURL = r.url || abs;
      if (abs.includes("#") && !docURL.includes("#")) docURL += abs.slice(abs.indexOf("#"));
      // loaded when its answer arrives (SPEC decision 20)
      afterNetwork(r, () => loadFrameDocument(el, r.body || "", docURL, unblock));
      return;
    }
    loadFrameDocument(el, html, url, unblock);
  };

  // ---- cross-origin frames, each in a mini runtime of its own (SPEC decisions 15 and 16) ----
  //
  // A frame whose src is of another origin is loaded by the driver in a runtime of its own (its
  // requests served by an <iframe> of its URL in the bridge tab, so that Firefox partitions its
  // storage as a third-party frame's). Each side sees the other as a cross-origin WindowProxy: only
  // postMessage, closed, length, frames, top, parent, window, self, location (set only), close, focus
  // and blur; every other access throws SecurityError (Gecko's RemoteOuterWindowProxy and the
  // cross-origin properties of HTML's WindowProxy). Messages go through the driver, which hands them
  // over at the sender's virtual time, as tasks of the merged event loop (realsite's run_tasks). A
  // message is dispatched a task after it arrives, as Gecko's PostMessageEvent is dispatched as a task
  // of its own once the message reaches the target's process; a frame's load notification fires its
  // element's load event as it arrives (MaybeFireEmbedderLoadEvents), so it comes before a message the
  // frame posted during its load.
  const CROSS_ORIGIN_ALLOWED = new Set(["postMessage", "closed", "length", "frames", "top", "parent", "window", "self", "location", "opener",
    "close", "focus", "blur", "then", Symbol.toPrimitive, Symbol.toStringTag]);
  const crossOriginWindows = new Map();
  // (the proxies above: windows of other runtimes)
  const crossOriginWindowsHeld = new WeakSet();
  // A cross-origin window's location: Gecko's cross-origin Location, which allows setting href and
  // calling replace() only (CrossOriginProperties in the HTML spec); `navigate` gets the URL. The
  // frames do not navigate yet (GAPS B52), so both do nothing.
  function crossOriginLocation(navigate) {
    const deny = name => new DOMException(`Permission denied to access property "${String(name)}" on cross-origin object`, "SecurityError");
    return new Proxy(Object.create(null), {
      get(_, name) {
        if (name === "replace") return url => navigate(String(url));
        if (name === "then" || typeof name === "symbol") return undefined;
        throw deny(name);
      },
      set(_, name, value) {
        if (name === "href") { navigate(String(value)); return true; }
        throw deny(name);
      },
      has(_, name) { return name === "href" || name === "replace" || name === "then"; },
    });
  }
  // this runtime is a cross-origin frame's: its load is passed to its embedder by the driver
  let isFrameRuntime = false;
  // a root window: a frame whose embedder is in another runtime, opened in this one because it is
  // of this runtime's site (SPEC decision 19); the driver's id of the frame, which its requests
  // about its embedder carry (null: the runtime's first window, whose frame is the runtime's)
  let rootFrameId = null;
  function crossOriginWindow(target) {
    if (crossOriginWindows.has(target)) return crossOriginWindows.get(target);
    const methods = {
      postMessage(message, options = "/") {
        // targetOrigin parsed as a URL and compared by origin (Window::GetPrincipalForPostMessage), so
        // "https://www.reddit.com:443" is "https://www.reddit.com"
        let targetOrigin = typeof options === "string" ? options : (options && options.targetOrigin) || "/";
        if (targetOrigin === "/") targetOrigin = global.origin;
        else if (targetOrigin !== "*") {
          try { targetOrigin = new URL(targetOrigin).origin; } catch (e) { throw new DOMException("The string did not match the expected pattern.", "SyntaxError"); }
        }
        let data;
        try { data = CODEC.encode(message, cloneOptions); } catch (e) {
          if (e === CLONE_FAIL) throw new DOMException("The object could not be cloned.", "DataCloneError");
          throw e;
        }
        bridge("frameMessage", { to: target, frame: rootFrameId, origin: global.origin, data,
          targetOrigin, time: performance.now() });
      },
      close() {}, focus() {}, blur() {},
    };
    const location = crossOriginLocation(url => bridge("frameNavigate", { to: target, url }));
    const proxy = new Proxy(Object.create(null), {
      get(_, name) {
        if (name in methods) return methods[name];
        if (name === "closed") return false;
        if (name === "length") return 0;
        if (name === "window" || name === "self" || name === "frames") return proxy;
        if (name === "top") return target === "parent" ? proxy : global.top;
        if (name === "parent") return target === "parent" ? proxy : global;
        if (name === "opener") return null;
        if (name === "location") return location;
        if (name === "then" || typeof name === "symbol") return undefined;
        throw new DOMException(`Permission denied to access property "${String(name)}" on cross-origin object`, "SecurityError");
      },
      set(_, name, value) {
        if (name === "location") { bridge("frameNavigate", { to: target, url: String(value) }); return true; }
        throw new DOMException(`Permission denied to access property "${String(name)}" on cross-origin object`, "SecurityError");
      },
      has(_, name) { return CROSS_ORIGIN_ALLOWED.has(name); },
    });
    crossOriginWindows.set(target, proxy);
    crossOriginWindowsHeld.add(proxy);
    return proxy;
  }
  // A URL's site (scheme and registrable domain), as Firefox's eTLD service gives it, or null; asked
  // once per origin for all the runtime's windows
  function siteOf(url) {
    let origin;
    try { origin = new URL(url).origin; } catch (e) { return null; }
    if (origin === "null") return null;
    const sites = share("sites", () => new Map());
    if (!sites.has(origin)) {
      let site = null;
      try { site = bridge("site", { url: origin + "/" }); } catch (e) {}
      sites.set(origin, site);
    }
    return sites.get(origin);
  }
  // A frame of this runtime's site and another origin: the driver opens its context in the bridge
  // tab and fetches its document there; the frame's initial about:blank gives way to a window of the
  // frame's origin (its own compartment), which loads the document. Not loaded by Firefox (tracking
  // protection), or its document failed: the frame keeps its initial about:blank and gets no load.
  // The size a frame element gives its document: its content box in this window's layout (flushed),
  // as nsSubDocumentFrame gives a remote frame's process (BrowserParent::UpdateDimensions); 0 by 0
  // without a frame. Later changes of it do not reach a frame in another runtime (GAPS B75).
  function frameSize(el) {
    const v = __mrtViewport(el);
    return v ? { width: v[0], height: v[1] } : { width: 0, height: 0 };
  }
  function openSameSiteFrame(el, url, unblock) {
    // (the size of its context's <iframe> in the bridge tab; the window's own comes from the layout)
    const width = parseInt(el.getAttribute("width"), 10) || 300, height = parseInt(el.getAttribute("height"), 10) || 150;
    let r;
    try { r = bridge("frameContext", { url, width, height }); } catch (e) { unblock(); return; }
    if (!r || r.loaded === false) { unblock(); return; }
    const before = frameWindowOf(el);
    const win = __mrtWindows(10, el, r.body, r.origin, r.secure, r.isolated);
    if (!win) { unblock(); return; }
    // the browsing context keeps its name
    if (before) setContextName(win, contextNames.byWindow.get(before) ?? "");
    win.__mrt.setBridgeContext(r.ctx, foreignAncestor);
    win.__mrt.frameNavigated(r.url);
    win.__mrt.runDocument(() => {
      el.dispatchEvent(new Event("load"));
      unblock();
    });
  }
  // Another window of this runtime, of another origin, as this window sees it (ViewOfWindow in the
  // C++ layer asks for it): Gecko's cross-origin WindowProxy over a window in the same process. The
  // same properties as crossOriginWindow's; postMessage queues the message as a task of the target
  // window (PostMessageEvent), on the loop the runtime's windows share.
  const inProcessViews = new WeakMap();
  // each view's window, for this window's own code (frames by name, the viewer's mirror)
  const viewTargets = new WeakMap();
  function crossOriginView(target) {
    let view = inProcessViews.get(target);
    if (view) return view;
    const methods = {
      postMessage(message, options = "/", transfer = []) {
        let targetOrigin = typeof options === "string" ? options : (options && options.targetOrigin) || "/";
        const list = windowTransferList(options, transfer);
        if (targetOrigin === "/") targetOrigin = global.origin;
        else if (targetOrigin !== "*") {
          try { targetOrigin = new URL(targetOrigin).origin; } catch (e) { throw new DOMException("The string did not match the expected pattern.", "SyntaxError"); }
        }
        const env = encodeMessage(message, list, "local");
        // targetOrigin is checked when the message is dispatched (the target may have navigated)
        const from = global, origin = global.origin;
        target.__mrt.queueTask(() => {
          if (targetOrigin === "*" || targetOrigin === target.origin) target.__mrt.receiveMessage(env, origin, from);
        });
      },
      close() {}, focus() {}, blur() {},
    };
    const location = crossOriginLocation(() => {});
    view = new Proxy(Object.create(null), {
      get(_, name) {
        if (name in methods) return methods[name];
        if (name === "closed") return __mrtWindows(5, target);
        if (name === "length") return target.length;
        if (name === "window" || name === "self" || name === "frames") return view;
        if (name === "top") return __mrtWindows(11, target, true);
        if (name === "parent") return __mrtWindows(11, target, false);
        if (name === "opener") return null;
        if (name === "location") return location;
        // its frames by index, while the index is below its length (CrossOriginGetOwnPropertyHelper)
        if (typeof name === "string" && /^(0|[1-9][0-9]*)$/.test(name)) {
          const frame = __mrtWindows(13, target, Number(name));
          if (frame !== undefined) return frame;
        }
        if (name === "then" || typeof name === "symbol") return undefined;
        // its frames by name (CrossOriginGetOwnPropertyHelper's child navigable target names)
        const child = typeof name === "string" ? target.__mrt.namedFrame(name) : undefined;
        if (child !== undefined) return asSeenHere(child);
        throw new DOMException(`Permission denied to access property "${String(name)}" on cross-origin object`, "SecurityError");
      },
      set(_, name, value) {
        if (name === "location") return true;
        throw new DOMException(`Permission denied to access property "${String(name)}" on cross-origin object`, "SecurityError");
      },
      has(_, name) { return CROSS_ORIGIN_ALLOWED.has(name) || (typeof name === "string" && target.__mrt.namedFrame(name) !== undefined); },
    });
    inProcessViews.set(target, view);
    viewTargets.set(view, target);
    return view;
  }
  // ---- frames by name (window[name], HTML's document-tree child navigable target name property set) ----
  //
  // A frame element's window as this window's code has it: the window itself, also when it is of
  // another origin (not the view a page gets), or, for a frame in another runtime, its proxy; null
  // without one.
  const qsaOwn = isWorker ? null : Document.prototype.querySelectorAll, attrOwn = isWorker ? null : Element.prototype.getAttribute;
  function frameWindowOf(el) {
    const id = crossFrameOf.get(el);
    if (id !== undefined) return crossOriginWindow(id);
    const v = __mrtWindows(0, el);
    return v ? viewTargets.get(v) || v : null;
  }
  // a window of this runtime as this window sees it (a proxy of another runtime's is already a view)
  const asSeenHere = win => (crossOriginWindowsHeld.has(win) ? win : __mrtWindows(12, win));
  // This document's frames by browsing-context name, the first in tree order for each, leaving out a
  // frame of another origin whose name is not its element's name attribute (a name its own script
  // set). Made again when the DOM or a name changed.
  const namedFrames = { generation: -1, version: -1, map: new Map() };
  function framesByName() {
    const generation = __mrtRefresh(1);
    if (generation === namedFrames.generation && contextNames.version === namedFrames.version) return namedFrames.map;
    const map = new Map();
    for (const el of Reflect.apply(qsaOwn, document, ["iframe, frame"])) {
      const win = frameWindowOf(el);
      if (!win) continue;
      const remote = crossOriginWindowsHeld.has(win);
      const attr = Reflect.apply(attrOwn, el, ["name"]) ?? "";
      const name = remote ? attr : contextNames.byWindow.get(win) ?? "";
      if (name === "" || map.has(name)) continue;
      if (!remote && __mrtWindows(12, win) !== win && name !== attr) continue;
      map.set(name, win);
    }
    Object.assign(namedFrames, { generation, version: contextNames.version, map });
    return map;
  }
  // Gecko's WindowProperties object between Window.prototype and EventTarget.prototype
  // (WindowNamedPropertiesHandler): window[name] is the frame of that name, unless the window or
  // Window.prototype has the property. (Named elements are not here: GAPS B52.) Only after a frame
  // was inserted: a global name the page does not define looks here.
  let hasFrames = false;
  if (!isWorker) {
    const windowProto = Object.getPrototypeOf(global);
    const above = Object.getPrototypeOf(windowProto);
    const named = name => (hasFrames && typeof name === "string" ? framesByName().get(name) : undefined);
    const properties = Object.create(above);
    Object.defineProperty(properties, Symbol.toStringTag, { value: "WindowProperties", configurable: true });
    Object.setPrototypeOf(windowProto, new Proxy(properties, {
      get(target, name, receiver) {
        const win = named(name);
        return win !== undefined ? asSeenHere(win) : Reflect.get(target, name, receiver);
      },
      has(target, name) { return named(name) !== undefined || Reflect.has(target, name); },
      getOwnPropertyDescriptor(target, name) {
        const win = named(name);
        return win !== undefined ? { value: asSeenHere(win), writable: true, enumerable: false, configurable: true } : Reflect.getOwnPropertyDescriptor(target, name);
      },
    }));
    const frameInserted = global.__mrt.frameInserted;
    Object.assign(global.__mrt, {
      frameInserted(el) { hasFrames = true; return frameInserted(el); },
      // a frame of this runtime by name, for another window's cross-origin view of this one
      namedFrame(name) {
        const win = hasFrames ? framesByName().get(String(name)) : undefined;
        return win === undefined || crossOriginWindowsHeld.has(win) ? undefined : win;
      },
      // the C++ layer's: a frame element's name attribute was set or removed
      frameNameChanged(el) {
        const win = frameWindowOf(el);
        if (win && !crossOriginWindowsHeld.has(win)) setContextName(win, Reflect.apply(attrOwn, el, ["name"]) ?? "");
      },
    });
  }
  // The driver's "open": a frame of this runtime's site whose embedder is in another runtime (a page's
  // second frame of a site, or a frame of the page's site inside another site's frame) becomes a root
  // window here, as Firefox runs it in the site's process (SPEC decision 19); it loads its document
  // as a frame runtime's first window does, and its load goes to the driver.
  function openRootWindow(m) {
    const win = __mrtWindows(14, m.html, m.origin, m.secure, m.isolated, m.width, m.height);
    if (!win) return;
    share("rootWindows", () => new Map()).set(m.frame, win);
    win.__mrt.becomeRootFrame(m.frame, `mrt-frame-${m.frame}`);
    win.__mrt.frameNavigated(m.url);
    win.__mrt.runDocument(() => {});
  }
  // the frame elements of the cross-origin frames this runtime opened, by the driver's id
  const crossFrames = new Map();
  const crossFrameOf = new WeakMap();
  // the unblocking of this window's load for each cross-origin frame still loading, by the driver's id
  const crossFrameUnblocks = new Map();
  function openCrossOriginFrame(el, url, unblock, time) {
    const { width, height } = frameSize(el);
    // the driver could not fetch its document (Firefox would show an error page; GAPS B57), or
    // Firefox did not load it (tracking protection refused it): the frame keeps its initial
    // about:blank, of this window's origin, and its element gets no load event
    let r;
    try { r = bridge("frameOpen", { url, width, height, origin: global.origin, time }); } catch (e) { unblock(); return; }
    if (r.loaded === false) { unblock(); return; }
    crossFrames.set(r.id, el);
    crossFrameUnblocks.set(r.id, unblock);
    crossFrameOf.set(el, r.id);
    // its messages are this window's (a frame opened by a same-origin frame's window is that
    // window's child); takeStep, run in the page's window, hands them here
    share("crossFrameOwners", () => new Map()).set(r.id, deliverFrameMessage);
    // counted for the whole runtime (a frame of a same-origin frame's window counts too): realsite
    // steps the page's tasks while there is one
    shared.crossFrames = (shared.crossFrames || 0) + 1;
  }
  if (!isWorker) {
  const frameProto = global.HTMLIFrameElement.prototype;
  const ownContentWindow = Object.getOwnPropertyDescriptor(frameProto, "contentWindow");
  const ownContentDocument = Object.getOwnPropertyDescriptor(frameProto, "contentDocument");
  Object.defineProperty(frameProto, "contentWindow", { configurable: true, enumerable: true, get() {
    const id = crossFrameOf.get(this);
    return id !== undefined && this.isConnected ? crossOriginWindow(id) : ownContentWindow.get.call(this);
  } });
  Object.defineProperty(frameProto, "contentDocument", { configurable: true, enumerable: true, get() {
    return crossFrameOf.has(this) ? null : ownContentDocument.get.call(this);
  } });
  }
  // A message the driver hands over: dispatched at the window as a MessageEvent, its source the
  // sender's WindowProxy; a frame runtime's load fires its frame element's load event
  function deliverFrameMessage(m) {
    if (m.kind === "load" || m.kind === "gone") {
      const el = crossFrames.get(m.from);
      if (el && m.kind === "load") el.dispatchEvent(new Event("load"));
      // a frame runtime that ended before its load (the driver's "gone") unblocks it too
      const unblock = crossFrameUnblocks.get(m.from);
      crossFrameUnblocks.delete(m.from);
      if (unblock) unblock();
      return;
    }
    const source = m.from === "parent" ? crossOriginWindow("parent") : crossOriginWindow(m.from);
    const data = CODEC.decode(m.data, global);
    global.dispatchEvent(new global.MessageEvent("message", { data, origin: m.origin, source, ports: [] }));
  }
  Object.assign(global.__mrt, {
    // the driver's answer to a step (realsite's take_turn): its messages queued as tasks at their
    // senders' times; 1 to run the next task, 0 to finish
    takeStep(reply) {
      const r = (reply && reply.ok) || {};
      for (const m of r.messages || []) {
        if (m.kind === "open") {
          global.__mrt.queueAt(() => openRootWindow(m), m.time);
          continue;
        }
        const owners = shared.crossFrameOwners;
        // from its embedder to a root window of this runtime: that window's
        const root = m.from === "parent" && m.to !== undefined && share("rootWindows", () => new Map()).get(m.to);
        const deliver = root ? root.__mrt.deliverFromEmbedder : (m.from !== "parent" && owners && owners.get(m.from)) || deliverFrameMessage;
        if (m.kind === "load" || m.kind === "gone") global.__mrt.queueAt(() => deliver(m), m.time);
        else global.__mrt.queueAt(() => global.__mrt.queueTask(() => deliver(m)), m.time);
      }
      return r.finish ? 0 : 1;
    },
    crossOriginView,
    // a message from another window of this runtime, as a task of this window: its source this
    // window's view of the sender
    // (its ports made in this window's realm)
    receiveMessage(env, origin, from) {
      const source = __mrtWindows(12, from);
      let m;
      try { m = decodeEnvelope(env); } catch (e) {
        global.dispatchEvent(new global.MessageEvent("messageerror", { origin, source }));
        return;
      }
      global.dispatchEvent(new global.MessageEvent("message", { data: m.value, origin, source, ports: m.ports }));
    },
    setBridgeContext(ctx, ancestor) { bridgeContext = ctx; foreignAncestor = !!ancestor; },
    bridgeContext: () => bridgeContext,
    foreignAncestor: () => foreignAncestor,
    deliverFromEmbedder: m => deliverFrameMessage(m),
    // a root window (openRootWindow): its parent and top are its embedder in another runtime; its
    // requests are served in its frame's <iframe> in the bridge tab
    becomeRootFrame(frameId, context) {
      rootFrameId = frameId;
      bridgeContext = context;
      // in the page's runtime, the embedder is another site's frame (a frame runtime's windows are
      // third parties anyway, with no foreign-ancestor bit)
      foreignAncestor = !shared.frameProcess;
      global.__mrt.becomeFrameRuntime();
    },
    // a frame runtime: its parent and top are its embedder, of another origin
    becomeFrameRuntime() {
      if (rootFrameId === null) shared.frameProcess = true;
      isFrameRuntime = true;
      Object.defineProperty(global, "parent", { get: () => crossOriginWindow("parent"), configurable: true, enumerable: true });
      Object.defineProperty(global, "top", { get: () => crossOriginWindow("parent"), configurable: true, enumerable: true });
    },
  });

  // ---- dedicated workers (DESIGN, "Workers: the build plan"; Gecko's dom/workers) ----
  //
  // The page's side is the Worker interface (Worker.cpp); a worker's side, in its own global,
  // DedicatedWorkerGlobalScope and WorkerGlobalScope (WorkerScope.cpp). Between them, worker.rs: a
  // message is a task at the sender's virtual time in the other side's queue (MessageEventRunnable).
  // What a worker hands its parent comes back when its task ends, as JSON texts, each with `at`, the
  // worker's virtual time:
  //   {m}       a message, an envelope of encodeMessage's (its encoding and the ports it carries)
  //   {port, m} a message for a port in another thread (routePort); `m` {c: 1} is that port closed
  //   {error}   an error its global did not cancel: message, filename, lineno, colno (WorkerErrorReport)
  //   {load}    its script did not load or compile: a plain `error` event at the Worker

  // The Worker constructor's arguments (Worker::Constructor, WorkerPrivate::Constructor), in a window
  // and in a worker (a nested worker): WorkerOptions' members read in WebIDL's order (by name), the
  // URL against `base` (the document's base, or the worker's script URL)
  const ENUM = (what, value, allowed, member, enumName) => {
    const s = String(value);
    if (!allowed.includes(s)) throw new TypeError(`${what}: '${s}' (value of '${member}' member of WorkerOptions) is not a valid value for enumeration ${enumName}.`);
    return s;
  };
  const workerArgs = (args, base) => {
    const what = "Worker constructor";
    if (args.length < 1) throw new TypeError(`${what}: At least 1 argument required, but only 0 passed`);
    // (TrustedScriptURL or USVString): a Trusted Types sink
    const url = global.__mrt.trustedTypes.compliant(args[0], "TrustedScriptURL", what, what);
    const options = args[1];
    let credentials = "same-origin", name = "", type = "classic";
    if (options !== undefined && options !== null) {
      if (typeof options !== "object" && typeof options !== "function") throw new TypeError(`${what}: Value can't be converted to a dictionary.`);
      const c = options.credentials;
      if (c !== undefined) credentials = ENUM(what, c, ["omit", "same-origin", "include"], "credentials", "RequestCredentials");
      const n = options.name;
      if (n !== undefined) name = String(n);
      const t = options.type;
      if (t !== undefined) type = ENUM(what, t, ["classic", "module"], "type", "WorkerType");
    }
    let resolved;
    try { resolved = new URL(url, base).href; } catch (e) {
      throw new DOMException(`${what}: Failed to load worker script at "${url}"`, "SyntaxError");
    }
    return { resolved, name, type, credentials };
  };
  // the worker's script URL, with a blob: URL's revoked flag when parsed (WorkerPrivate::GetLoadInfo)
  const workerScript = url => ({ url, blobLive: isBlobURL(url) && __mrtBlobURL(3, url) });
  // the worker's origin: a data: URL's script gets an opaque one (a null principal), any other its
  // parent's
  const workerOrigin = url => (/^data:/i.test(url) ? "null" : global.origin);

  if (!isWorker) {
    // ---- the page's side: Worker ----
    // per Worker: its id in worker.rs, and whether terminate() was called
    const workerStates = new WeakMap();
    // the prefix of the port ids a worker makes (newPortId)
    const workerPortTag = () => "w" + (shared.nextWorkerTag = (shared.nextWorkerTag || 0) + 1);
    // An item a worker handed over that is for the hub, done at once: a message for a port, routed
    // on (routePort); a message for this side, whose ports are on their way here. True when done.
    // A worker's nested workers (GAPS B82) are workers of this list too, the hub between them and
    // their parent: a worker asks for one ({spawn}), posts to it ({toChild}) and ends it
    // ({terminateChild}); what the nested worker hands over goes to its parent ({child, item}), at
    // the nested worker's time; a worker that ends ends its nested workers.
    const fromWorker = (item, fromId) => {
      if ("port" in item) { routePort(item.port, item.m, true, item.at); return true; }
      if ("locks" in item) { lockHub.handle(item.locks, workerLockClient(fromId, item.locks.client), item.at); return true; }
      if ("spawn" in item) { startNested(fromId, item.spawn, item.at); return true; }
      if ("toChild" in item) {
        const c = nested.get(item.toChild);
        if (c && !c.ended) { portsToWorker(c.id, item.m); __mrtWorker(1, c.id, JSON.stringify(item.m), item.at); }
        return true;
      }
      if ("terminateChild" in item) { const c = nested.get(item.terminateChild); if (c) endNested(c); return true; }
      if ("closed" in item) { endNestedOf(fromId); dropWorkerLocks(fromId); }
      if ("m" in item) portsArrived(item.m);
      return false;
    };
    // Web Locks: a worker's client, its answers sent to it as worker messages; the client ids each
    // worker used, dropped from the hub when it ends
    const workerLockClients = new Map();
    const workerLockClient = (workerId, clientId) => {
      if (!workerLockClients.has(workerId)) workerLockClients.set(workerId, new Set());
      workerLockClients.get(workerId).add(clientId);
      return { id: clientId, deliver: (m, at) => __mrtWorker(1, workerId, JSON.stringify({ locks: m }), at) };
    };
    const dropWorkerLocks = workerId => {
      const ids = workerLockClients.get(workerId);
      if (!ids) return;
      workerLockClients.delete(workerId);
      for (const id of ids) lockHub.dropClient(id, clockNow());
    };
    // by tag: {id, parent, tag, ended}
    const nested = share("nestedWorkers", () => new Map());
    const startNested = (parentId, spawn, at) => {
      const id = __mrtWorker(0, JSON.stringify(spawn.config), spawn.origin, global.isSecureContext, global.crossOriginIsolated, at);
      workerOfTag.set(spawn.config.portTag, id);
      const c = { id, parent: parentId, tag: spawn.config.portTag, ended: false };
      nested.set(c.tag, c);
      shared.workers.set(id, items => {
        for (const item of items) {
          if (fromWorker(item, id) || c.ended) continue;
          if ("m" in item) portsToWorker(parentId, item.m);
          __mrtWorker(1, parentId, JSON.stringify({ child: c.tag, item }), item.at);
        }
      });
    };
    const endNested = c => {
      if (c.ended) return;
      c.ended = true;
      __mrtWorker(4, c.id);
      dropWorkerLocks(c.id);
      __mrtBlobURL(4, c.tag, clockNow());
      shared.workers.delete(c.id);
      endNestedOf(c.id);
    };
    const endNestedOf = parentId => { for (const c of nested.values()) if (c.parent === parentId) endNested(c); };
    const workerState = (w, member) => {
      const st = w !== null && typeof w === "object" ? workerStates.get(w) : undefined;
      if (!st) throw new TypeError(`'${member}' called on an object that does not implement interface Worker.`);
      return st;
    };
    // Worker::Constructor and WorkerPrivate::Constructor
    const Worker = function Worker(scriptURL) {
      if (!new.target) throw new TypeError("Worker constructor: 'new' is required");
      const { resolved, name, type, credentials } = workerArgs(arguments, document.baseURI);
      const portTag = workerPortTag();
      const config = JSON.stringify(Object.assign({ name, type, credentials, ctx: bridgeContext, owner: global.origin, portTag }, workerScript(resolved)));
      const id = __mrtWorker(0, config, workerOrigin(resolved), global.isSecureContext, global.crossOriginIsolated, clockNow());
      workerOfTag.set(portTag, id);
      const st = { id, terminated: false, portTag };
      workerStates.set(this, st);
      // what the worker hands back, each a task of this loop at the worker's time
      shared.workers.set(id, items => {
        for (const item of items) {
          if (fromWorker(item, id)) continue;
          global.__mrt.queueAt(() => {
            if (st.terminated) return;
            if ("m" in item) dispatchMessage(this, item.m);
            else if ("error" in item) {
              // WorkerErrorReport::ReportError at the parent: cancelable; not cancelled, the console
              const e = item.error;
              const ev = new ErrorEvent("error", { message: e.message, filename: e.filename, lineno: e.lineno, colno: e.colno, error: null, cancelable: true })
              // (the runtime's events have no isTrusted, GAPS B72; this one is the browser's)
              ev.isTrusted = true;
              if (this.dispatchEvent(ev)) console.error(e.message);
            } else if ("load" in item) {
              this.dispatchEvent(new Event("error"));
            }
          }, item.at);
        }
      });
    };
    Object.setPrototypeOf(Worker, EventTarget);
    Object.setPrototypeOf(Worker.prototype, EventTarget.prototype);
    for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors({
      // Worker::PostMessage: nothing once terminated; otherwise cloned now, a task in the worker
      postMessage(message) {
        const st = workerState(this, "postMessage");
        if (arguments.length < 1) throw new TypeError("Worker.postMessage: At least 1 argument required, but only 0 passed");
        const transfer = transferList("Worker.postMessage", arguments[1]);
        const env = encodeMessage(message, transfer, st.id);
        if (st.terminated) return;
        postToWorker(st.id, env);
      },
      // Worker::Terminate: the worker ends; what it posted and was not delivered is dropped
      terminate() {
        const st = workerState(this, "terminate");
        if (st.terminated) return;
        st.terminated = true;
        __mrtWorker(4, st.id);
        dropWorkerLocks(st.id);
        __mrtBlobURL(4, st.portTag, clockNow());
        endNestedOf(st.id);
        shared.workers.delete(st.id);
      },
    }))) Object.defineProperty(Worker.prototype, key, { ...desc, enumerable: true });
    global.__mrt.defineEventHandlers(Worker.prototype, ["onmessage", "onmessageerror", "onerror"]);
    Object.defineProperty(Worker.prototype, Symbol.toStringTag, { value: "Worker", configurable: true });
    define(global, "Worker", Worker);

    // ---- the page's side: SharedWorker (GAPS B85; Gecko's SharedWorker.cpp, SharedWorkerManager.cpp,
    // SharedWorkerChild.cpp) ----
    // The runtime's shared workers by script URL and name (SharedWorkerManager::MatchSharedWorker; a
    // runtime is one origin and one partition): its id in worker.rs, its type and credentials, the
    // SharedWorker objects connected to it by port id, and whether it has ended (close(), or a script
    // that did not load), after which a new SharedWorker starts another.
    const sharedWorkers = new Map();
    // per SharedWorker: its port
    const sharedWorkerStates = new WeakMap();
    const startSharedWorker = (key, config) => {
      const id = __mrtWorker(0, JSON.stringify(config), workerOrigin(config.url), global.isSecureContext, global.crossOriginIsolated, clockNow(), true);
      workerOfTag.set(config.portTag, id);
      const sw = { id, type: config.type, credentials: config.credentials, clients: new Map(), ended: false };
      sharedWorkers.set(key, sw);
      // what the worker hands back: its end at once (a SharedWorker made after it starts a new
      // worker), the rest each a task of this loop at the worker's time
      shared.workers.set(id, items => {
        for (const item of items) {
          if ("closed" in item) {
            sw.ended = true;
            endNestedOf(id);
            dropWorkerLocks(id);
            if (sharedWorkers.get(key) === sw) sharedWorkers.delete(key);
            continue;
          }
          if (fromWorker(item, id)) continue;
          global.__mrt.queueAt(() => {
            if ("error" in item) {
              // SharedWorkerChild::RecvError: an ErrorEvent at each SharedWorker of the worker,
              // cancelable, its `error` not set; one not cancelled, the console
              const e = item.error;
              for (const target of sw.clients.values()) {
                const ev = new ErrorEvent("error", { message: e.message, filename: e.filename, lineno: e.lineno, colno: e.colno, cancelable: true })
                // (the runtime's events have no isTrusted, GAPS B72; this one is the browser's)
                ev.isTrusted = true;
                if (target.dispatchEvent(ev)) console.error(e.message);
              }
            } else if ("load" in item) {
              // SharedWorker::ErrorPropagation: a plain error event
              for (const target of sw.clients.values()) target.dispatchEvent(new Event("error"));
            }
          }, item.at);
        }
      });
      return sw;
    };
    // SharedWorker::Constructor
    const SharedWorker = function SharedWorker(scriptURL) {
      const what = "SharedWorker constructor";
      if (!new.target) throw new TypeError(`${what}: 'new' is required`);
      if (arguments.length < 1) throw new TypeError(`${what}: At least 1 argument required, but only 0 passed`);
      const url = global.__mrt.trustedTypes.compliant(scriptURL, "TrustedScriptURL", what, what);
      // (DOMString or WorkerOptions): a string is the name; a dictionary's members in WebIDL's order
      const options = arguments[1];
      let credentials = "same-origin", name = "", type = "classic";
      if (options !== undefined && options !== null && typeof options !== "object" && typeof options !== "function") {
        name = String(options);
      } else if (options !== undefined && options !== null) {
        const c = options.credentials;
        if (c !== undefined) credentials = ENUM(what, c, ["omit", "same-origin", "include"], "credentials", "RequestCredentials");
        const n = options.name;
        if (n !== undefined) name = String(n);
        const t = options.type;
        if (t !== undefined) type = ENUM(what, t, ["classic", "module"], "type", "WorkerType");
      }
      // the URL against the document's base (WorkerPrivate::GetLoadInfo); a script of another origin
      // fails at load, not here
      let resolved;
      try { resolved = new URL(url, document.baseURI).href; } catch (e) {
        throw new DOMException("An invalid or illegal string was specified", "SyntaxError");
      }
      // SharedWorker.port and the worker's end of it, a MessageChannel's two ports (as Gecko's)
      const portId = newPortId(), workerPortId = newPortId();
      const st = { port: makePort({ id: portId, other: workerPortId }) };
      sharedWorkerStates.set(this, st);
      const key = resolved + "\n" + name;
      const found = sharedWorkers.get(key);
      if (found && (found.type !== type || found.credentials !== credentials)) {
        // the same URL and name with another type or credentials (ErrorMismatchOptions): the
        // console, then a plain error event; this one connects to nothing
        global.__mrt.queueTask(() => {
          console.error("Failed to connect an existing shared worker because the type or credentials given on the SharedWorker constructor do not match the existing shared worker's type or credentials");
          this.dispatchEvent(new Event("error"));
        });
        return;
      }
      const sw = found || startSharedWorker(key, Object.assign({ name, type, credentials, ctx: bridgeContext, owner: global.origin, kind: "shared", portTag: workerPortTag() }, workerScript(resolved)));
      sw.clients.set(portId, this);
      // the worker's connect event for its end of the port (WorkerPrivate::ConnectMessagePort)
      const env = { d: "", t: [{ id: workerPortId, other: portId, held: [] }] };
      portsToWorker(sw.id, env);
      postToWorker(sw.id, { connect: env });
    };
    Object.setPrototypeOf(SharedWorker, EventTarget);
    Object.setPrototypeOf(SharedWorker.prototype, EventTarget.prototype);
    Object.defineProperty(SharedWorker.prototype, "port", {
      get() {
        const st = this !== null && typeof this === "object" ? sharedWorkerStates.get(this) : undefined;
        if (!st) throw new TypeError("'get port' called on an object that does not implement interface SharedWorker.");
        return st.port;
      },
      configurable: true, enumerable: true,
    });
    global.__mrt.defineEventHandlers(SharedWorker.prototype, ["onerror"]);
    Object.defineProperty(SharedWorker.prototype, Symbol.toStringTag, { value: "SharedWorker", configurable: true });
    define(global, "SharedWorker", SharedWorker);
  } else {
    // ---- a worker's side: DedicatedWorkerGlobalScope or SharedWorkerGlobalScope, and WorkerGlobalScope ----
    // @WORKER_NAMES@
    // a shared worker's global (the C++ layer made it a SharedWorkerGlobalScope; GAPS B85)
    const isShared = typeof global.SharedWorkerGlobalScope === "function";
    let config = null, started = false, closing = false, reporting = false;
    // what came before the script ran, messages and a shared worker's connections, each a task to
    // queue then (Gecko queues them until the worker runs)
    const early = [];
    const runEarly = () => { for (const task of early.splice(0)) global.__mrt.queueTask(task); };
    const out = item => __mrtWorkerOut(JSON.stringify(Object.assign(item, { at: clockNow() })));
    // Web Locks: this worker's requests go to the hub with the task's items
    locksToHub = msg => out({ locks: Object.assign(msg, { client: lockClient() }) });
    const scope = global.__mrtWorkerScope;
    // the worker ends after this task (close(), or a script that did not load); a shared worker's page
    // forgets it, so that a SharedWorker made after this starts another
    // the worker ends: a shared worker tells its parent, and the blob: URLs it made are revoked
    // (WorkerGlobalScope::UnregisterHostObjectURI)
    const closeScope = () => {
      if (!closing) out({ closed: 1 });
      if (!closing) __mrtBlobURL(4, blobURLOwner, clockNow());
      closing = true;
    };

    // An uncaught exception (WorkerPrivate::ReportError, WorkerErrorReport): an ErrorEvent at the
    // global, its onerror called with the error's parts; not cancelled, the parent gets it. One thrown
    // while that runs goes to the parent at once.
    const errorParts = e => (e !== null && typeof e === "object" && "message" in e && "fileName" in e
      ? { message: `${e.name}: ${e.message}`, filename: String(e.fileName), lineno: e.lineNumber | 0, colno: e.columnNumber | 0 }
      : { message: "uncaught exception: " + String(e), filename: config ? config.url : "", lineno: 0, colno: 0 });
    const reportError = (e, parts = errorParts(e)) => {
      if (reporting) { out({ error: parts }); return; }
      reporting = true;
      let cancelled = false;
      try {
        const ev = new ErrorEvent("error", { message: parts.message, filename: parts.filename, lineno: parts.lineno, colno: parts.colno, error: e, cancelable: true });
        // (the runtime's events have no isTrusted, GAPS B72; this one is the browser's)
        ev.isTrusted = true;
        ev._onErrorArgs = [parts.message, parts.filename, parts.lineno, parts.colno, e];
        cancelled = !global.dispatchEvent(ev);
      } finally {
        reporting = false;
      }
      if (!cancelled) out({ error: parts });
    };
    global.__mrt.setWorkerErrorReport(reportError);

    // WorkerLocation and WorkerNavigator, interfaces as Gecko's
    const illegal = name => ({ [name]: function () { throw new TypeError("Illegal constructor."); } })[name];
    const WorkerLocation = illegal("WorkerLocation");
    let scriptURL = null;
    for (const part of ["href", "origin", "protocol", "host", "hostname", "port", "pathname", "search", "hash"]) {
      Object.defineProperty(WorkerLocation.prototype, part, { get() { return scriptURL[part]; }, configurable: true, enumerable: true });
    }
    Object.defineProperty(WorkerLocation.prototype, "toString", { value: function toString() { return scriptURL.href; }, writable: true, configurable: true, enumerable: true });
    Object.defineProperty(WorkerLocation.prototype, Symbol.toStringTag, { value: "WorkerLocation", configurable: true });
    const workerLocation = Object.create(WorkerLocation.prototype);
    const WorkerNavigator = illegal("WorkerNavigator");
    for (const member of isShared ? SHARED_WORKER_NAVIGATOR_MEMBERS : WORKER_NAVIGATOR_MEMBERS) {
      if (member in env.navigator) Object.defineProperty(WorkerNavigator.prototype, member, { get() { return env.navigator[member]; }, configurable: true, enumerable: true });
    }
    if (global.isSecureContext) Object.defineProperty(WorkerNavigator.prototype, "locks", { get() { return lockManager; }, configurable: true, enumerable: true });
    Object.defineProperty(WorkerNavigator.prototype, "permissions", { get() { return permissions; }, configurable: true, enumerable: true });
    if (global.isSecureContext) Object.defineProperty(WorkerNavigator.prototype, "storage", { get() { return storageManager; }, configurable: true, enumerable: true });
    Object.defineProperty(WorkerNavigator.prototype, Symbol.toStringTag, { value: "WorkerNavigator", configurable: true });
    const workerNavigator = Object.create(WorkerNavigator.prototype);
    define(global, "WorkerLocation", WorkerLocation);
    define(global, "WorkerNavigator", WorkerNavigator);
    getter(global, "location", () => workerLocation);
    getter(global, "navigator", () => workerNavigator);

    // a script fetched for the worker (WorkerScriptLoader), loaded for `origin`: a data: URL's text; a
    // blob: URL's bytes from the table when that origin made it (`parsedLive`: the URL was not revoked
    // when parsed); otherwise, for the main script, the owner's origin only, and for an imported one
    // any origin (GetClassicSecFlags: SEC_REQUIRE_SAME_ORIGIN_DATA_IS_BLOCKED for the main script,
    // SEC_ALLOW_CROSS_ORIGIN_INHERITS_SEC_CONTEXT for importScripts). Any content type runs. Null when
    // it does not load.
    const fetchWorkerScript = (url, parsedLive, origin, isMainScript) => {
      if (/^data:/i.test(url)) {
        const d = global.__mrt.readDataURL(url);
        return d ? { status: 200, body: d.text } : null;
      }
      if (isBlobURL(url)) {
        const entry = __mrtBlobURL(2, url, origin, !!parsedLive, clockNow());
        return entry ? { status: 200, body: new TextDecoder().decode(entry[0]) } : null;
      }
      if (isMainScript && new URL(url).origin !== config.owner) return null;
      let r = null;
      try { r = bridge("fetch", { url, method: "GET", headers: [], body: null, script: true, dest: isMainScript ? "worker" : "script" }); } catch (e) {}
      return r && r.status >= 200 && r.status < 300 && r.body != null ? r : null;
    };
    // WorkerGlobalScope::ImportScripts: every URL resolved first, then each fetched and run in order,
    // at once; one that does not load throws a NetworkError, one that does not compile its SyntaxError.
    // A script of another origin runs with muted errors (NetworkLoadHandler: the channel's principal
    // not subsumed), and what it throws, compiling or running, becomes a NetworkError
    // (WorkerScriptLoader::ShutdownScriptLoader).
    define(global, "importScripts", function importScripts(...urls) {
      if (config && config.type === "module") throw new TypeError("WorkerGlobalScope.importScripts: Using `ImportScripts` inside a Module Worker is disallowed.");
      // a blob: URL revoked before it is parsed does not parse (ChannelFromScriptURL)
      const resolved = urls.map(u => {
        let url;
        try { url = new URL(String(u), scriptURL.href).href; } catch (e) {}
        if (url === undefined || (isBlobURL(url) && !__mrtBlobURL(3, url))) throw new DOMException(`WorkerGlobalScope.importScripts: Failed to load worker script at "${u}"`, "SyntaxError");
        return url;
      });
      for (const url of resolved) {
        const r = fetchWorkerScript(url, true, global.origin, false);
        if (!r) throw new DOMException("A network error occurred.", "NetworkError");
        const muted = !isBlobURL(url) && new URL(url).origin !== global.origin;
        let result;
        try {
          result = scope.runScript(r.body, url, muted);
        } catch (e) {
          if (muted) throw new DOMException("A network error occurred.", "NetworkError");
          throw e;
        }
        if (result !== 0) throw muted ? new DOMException("A network error occurred.", "NetworkError") : result;
      }
    });
    // WorkerGlobalScopeBase::ReportError: the value's report (reportedParts) through WorkerPrivate's,
    // as an uncaught exception's (moved to WorkerGlobalScope's prototype below)
    define(global, "reportError", { reportError(e) {
      const stack = new Error().stack;
      if ((this ?? global) !== global) throw new TypeError("'reportError' called on an object that does not implement interface WorkerGlobalScope.");
      if (arguments.length < 1) throw new TypeError("WorkerGlobalScope.reportError: At least 1 argument required, but only 0 passed");
      reportError(e, reportedParts(e, stack));
    } }.reportError);
    // DedicatedWorkerGlobalScope's or SharedWorkerGlobalScope's own members, on the global as Firefox
    // has them; a shared worker has no postMessage, its ports do
    const ownMembers = {
      postMessage(message) {
        if (arguments.length < 1) throw new TypeError("DedicatedWorkerGlobalScope.postMessage: At least 1 argument required, but only 0 passed");
        out({ m: encodeMessage(message, transferList("DedicatedWorkerGlobalScope.postMessage", arguments[1]), "out") });
      },
      // WorkerGlobalScope::Close: what was posted in this task still goes; nothing runs after it
      close() { closeScope(); },
      get name() { return config ? config.name : ""; },
    };
    if (isShared) delete ownMembers.postMessage;
    for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors(ownMembers))) {
      Object.defineProperty(global, key, { ...desc, enumerable: true, configurable: true });
    }
    global.__mrt.defineEventHandlers(global, isShared ? ["onconnect"] : ["onmessage", "onmessageerror", "onrtctransform"]);
    global.__mrt.defineEventHandlers(global, ["onerror", "onlanguagechange", "onoffline", "ononline", "onrejectionhandled", "onunhandledrejection"]);

    // Each name where Firefox's worker has it (worker_names.js): WorkerGlobalScope's members on its
    // prototype; a name Firefox's worker lacks (a window's that bridge.js defined anyway) removed.
    // Only names: the standard classes that are not made yet stay unmade.
    {
      const wgs = Object.getPrototypeOf(Object.getPrototypeOf(global));
      for (const name of Object.getOwnPropertyNames(global)) {
        if (name.startsWith("__")) continue;
        if (WORKER_GLOBAL_SCOPE_MEMBERS.includes(name)) {
          const desc = Object.getOwnPropertyDescriptor(global, name);
          delete global[name];
          Object.defineProperty(wgs, name, { ...desc, enumerable: true, configurable: true });
        } else if (!(isShared ? SHARED_WORKER_GLOBAL_NAMES : WORKER_GLOBAL_NAMES).has(name)) {
          delete global[name];
        }
      }
    }

    // A module worker (WorkerModuleLoader): its graph fetched, linked and evaluated by the C++ layer's
    // module loader, which reports what it throws (workerError); a graph that does not load is a
    // plain error event at the Worker
    const runModuleWorker = () => {
      started = true;
      const url = config.url;
      blobModuleParsedLive = u => u === url && config.blobLive;
      const result = !/^data:/i.test(url) && new URL(url).origin !== config.owner ? 1 : __mrtModuleScript(url);
      if (result === 1 || result === 3) {
        out({ load: "failed" });
        closeScope();
        return;
      }
      runEarly();
    };
    // a message for a port in another thread, to the hub (routePort)
    portOut = (id, msg) => out({ port: id, m: msg });
    // WorkerPrivate::ConnectMessagePort: the worker's end of the SharedWorker's port, made here, in a
    // connect event at the global
    const connect = env => {
      const ports = receivePorts(env);
      global.dispatchEvent(new MessageEvent("connect", { data: "", source: ports[0], ports: Object.freeze(ports) }));
    };

    // ---- a worker's side: nested workers (Worker in a worker; GAPS B82) ----
    // The hub (the page's thread) starts each one and routes between it and this worker: this side
    // names it with a tag now, and its requests go out when the task ends.
    const nestedWorkers = new Map();
    const nestedStates = new WeakMap();
    let nestedCount = 0;
    const nestedState = (w, member) => {
      const st = w !== null && typeof w === "object" ? nestedStates.get(w) : undefined;
      if (!st) throw new TypeError(`'${member}' called on an object that does not implement interface Worker.`);
      return st;
    };
    const NestedWorker = function Worker(url) {
      if (!new.target) throw new TypeError("Worker constructor: 'new' is required");
      // the URL against this worker's script URL
      const { resolved, name, type, credentials } = workerArgs(arguments, scriptURL.href);
      const tag = config.portTag + "c" + ++nestedCount;
      nestedStates.set(this, { tag, terminated: false });
      nestedWorkers.set(tag, this);
      out({ spawn: { config: Object.assign({ name, type, credentials, ctx: bridgeContext, owner: global.origin, portTag: tag }, workerScript(resolved)),
                     origin: workerOrigin(resolved) } });
    };
    // what the nested worker handed over, at this worker's task: a message, an error (not cancelled
    // here, it goes on to this worker's parent, WorkerErrorReport), a script that did not load
    const nestedEvent = (w, item) => {
      if (nestedState(w, "event").terminated) return;
      if ("m" in item) dispatchMessage(w, item.m);
      else if ("error" in item) {
        const e = item.error;
        const ev = new ErrorEvent("error", { message: e.message, filename: e.filename, lineno: e.lineno, colno: e.colno, error: null, cancelable: true })
        // (the runtime's events have no isTrusted, GAPS B72; this one is the browser's)
        ev.isTrusted = true;
        if (w.dispatchEvent(ev)) out({ error: e });
      } else if ("load" in item) w.dispatchEvent(new Event("error"));
    };
    Object.setPrototypeOf(NestedWorker, EventTarget);
    Object.setPrototypeOf(NestedWorker.prototype, EventTarget.prototype);
    for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors({
      postMessage(message) {
        const st = nestedState(this, "postMessage");
        if (arguments.length < 1) throw new TypeError("Worker.postMessage: At least 1 argument required, but only 0 passed");
        const env = encodeMessage(message, transferList("Worker.postMessage", arguments[1]), "out");
        if (!st.terminated) out({ toChild: st.tag, m: env });
      },
      terminate() {
        const st = nestedState(this, "terminate");
        if (st.terminated) return;
        st.terminated = true;
        nestedWorkers.delete(st.tag);
        out({ terminateChild: st.tag });
      },
    }))) Object.defineProperty(NestedWorker.prototype, key, { ...desc, enumerable: true });
    global.__mrt.defineEventHandlers(NestedWorker.prototype, ["onmessage", "onmessageerror", "onerror"]);
    Object.defineProperty(NestedWorker.prototype, Symbol.toStringTag, { value: "Worker", configurable: true });
    define(global, "Worker", NestedWorker);

    Object.assign(global.__mrt, {
      // worker.rs, once the global is set up: the worker's config (the Worker constructor's JSON), its
      // script loaded in a task at `at` and run when its response comes (SPEC decision 20)
      workerStart(json, at) {
        // the missing-API log (as installMissingLog's for a window) against Firefox's worker names.
        // Not on the navigator: a proxy after WorkerNavigator.prototype would be seen by a script
        // that walks the chain to Object.prototype (the worker-names case does), where the global's
        // stands in Object.prototype's place with its names.
        logMissingAt(EventTarget.prototype, [["WorkerGlobalScope", global, [...(isShared ? SHARED_WORKER_GLOBAL_NAMES : WORKER_GLOBAL_NAMES), ...WORKER_GLOBAL_SCOPE_MEMBERS], r => r === global]]);
        config = JSON.parse(json);
        portPrefix = config.portTag;
        blobURLOwner = config.portTag;
        scriptURL = new URL(config.url);
        pageURL = scriptURL;
        bridgeContext = config.ctx;
        if (config.type === "module") {
          global.__mrt.queueAt(runModuleWorker, at);
          return;
        }
        global.__mrt.queueAt(() => {
          const r = fetchWorkerScript(config.url, config.blobLive, config.owner, true);
          afterNetwork(r, () => {
            const fail = () => { out({ load: "failed" }); closeScope(); };
            if (!r) return fail();
            started = true;
            try {
              const result = scope.runScript(r.body, config.url);
              if (result !== 0) return fail();
            } catch (e) {
              reportError(e);
            }
            runEarly();
          });
        }, at);
      },
      // a message from the parent (an envelope as JSON): a task at the parent's time, or waiting for
      // the script. A shared worker's is a connection ({connect}). A message for a port ({port, m})
      // goes to the port, or waits for it (routePort).
      workerReceive(json, at) {
        if (closing) return;
        const data = JSON.parse(json);
        if ("port" in data) { routePort(data.port, data.m, true, at); return; }
        if ("locks" in data) { global.__mrt.queueAt(() => locksFromHub(data.locks), at); return; }
        if ("child" in data) { const w = nestedWorkers.get(data.child); if (w) global.__mrt.queueAt(() => nestedEvent(w, data.item), at); return; }
        const task = "connect" in data ? () => connect(data.connect) : () => dispatchMessage(global, data);
        if (!started) early.push(task);
        else global.__mrt.queueAt(() => { if (!closing) task(); }, at);
      },
      workerClosed: () => closing,
      // worker.rs's Memory command: the missing-API log, most read first
      workerMissingAPIs: () => JSON.stringify([...missing].sort((a, b) => b[1] - a[1])),
      // an uncaught exception of a module (the C++ layer's ReportModuleException in a worker)
      workerError: e => reportError(e),
    });
  }
})(this);
