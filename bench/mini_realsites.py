"""Stage 6, phase 1: live sites in the mini runtime, their network, cookies and storage forwarded to
Firefox through the bridge (DESIGN "The bridge, as built in phase 1").

For each site, in a fresh Firefox on a copy of the test profile (as bench/firefox_realsites.py):
  1. the bridge tab: about:blank, its browsingContext.allowJavascript set to false, then the site's
     URL (the site's origin, cookies and logins; none of its code runs)
  2. the document, fetched through the bridge tab, written to bench/realsites-mini/<site>.html
  3. mrt-js's `realsite` on it: every request it writes on its stdout runs in the bridge tab
     (Marionette's content sandbox, the page's principal), the reply goes back on its stdin
  4. its last request is the report (memory, element counts, scripts, the missing-API log)
Writes bench/mini_realsites.json; the documents and the runtime's stderr (bench/realsites-mini/,
not in git: third-party content).

usage (from a Firefox source tree, under Xvfb):
  FIREFOX_BIN=.../firefox xvfb-run -a ./mach python /path/to/mini_realsites.py
  ONLY=cnn,amazon     sites (default: cnn,amazon, phase 1's two)
  EXTRA_SITES='{"name": "url"}'   more sites by name, such as a local test page (compare_idb.py)
  REALSITE_ARGS="--console"   extra arguments for realsite (--timer-limit MS, --max-flushes N: no budget by default)
  INTERACTIVE=1       run the page on the real clock until its bridge tab or window is closed (or
                      INTERACTIVE_S=N seconds after load), then measure it; run without xvfb-run to see the tab
  RENDER=1            render the mini runtime's final DOM in a Firefox tab (JS off, the site's origin)
                      and take screenshots of it and of the real page: realsites-mini/<site>-*.png
  REPLAY=fill|replay  through mitmproxy, from the site's recording (bench/replay.py; DESIGN "Replay");
                      results go to mini_realsites_replay.json unless MINI_OUT is set
REALSITE_WRAP="heaptrack -o FILE" MRT_EXIT_AFTER_REPORT=1   realsite under a heap profiler (GAPS A16)
BRIDGE_LEAN=1       the bridge tab never loads the page: it opens a small document of the page's origin (a 404
                    at /__mrt_lean_bridge__), fetches the document and serves the runtime from there, as
                    a bridge that renders nothing would (SPEC decision 14); the referrer of its requests
                    is that URL, and the page's final URL after redirects is the replay's recorded one
BRIDGE_MEMORY=1     at the report, the memory of Firefox's processes (after a GC and CC everywhere; plain, then
                    after "Minimize memory usage"): each process's unique and anonymous memory and the windows
                    it holds, and for the bridge tab's process its window by category and its images
                    (with MRT_ADB=1 PACKAGE=org.mozilla.geckoview_example, also each content process's smaps)
                    With API_PROCESS, also each mrt-api's unique and anonymous memory, plain and after its `trim`
                    op (`api_memory`: what memory pressure would give back). Every run lists the bridge tab's
                    ops by kind with their Marionette round trips (`tab_ops`)
BRIDGE_SERIES=1     with BRIDGE_MEMORY, two more plain points 30 and 60 s after the first, before the minimize; on
                    the phone first `quiet`: the bridge's process every 15 s for 60 s and after a minimize
                    alone, with no memory report in between (mrt_phone.quiet_series)
BRIDGE_IDB_STOP=1   with BRIDGE_MEMORY, first end the IndexedDB transactions the bridge keeps open (idb_bridge.js
                    keepOpen spins a request after another while the runtime waits: task 4898)
BRIDGE_PRESS=1      with BRIDGE_MEMORY on the phone, last: memory pressure up to lmkd's first kill below
                    the cached apps, and the bridge's process read again (mrt_phone.pressure_point)
MRT_BRIDGE_DROP=MS  with MRT_HIBERNATE_AT, MS after hibernating the page asks for its bridge tab to be dropped
                    (SPEC decision 14): the tab is closed, and a new one, with the frames' <iframe>s, is made
                    when a request needs it (DESIGN "Bridge recovery, as tested"); `bridge_recovery` lists the
                    drop and the rebuild, with the virtual time, the request that needed it and the pids.
                    MRT_WAKE_AT=MS shows the page again (realsite)
BRIDGE_SANDBOX=reuse  the runtime's plain requests run in one Marionette content sandbox kept for the
                    whole run, not in a new one each (task 4898: the harness's share of the bridge)
                    (SPEC decision 14: the bridge's cost belongs in the comparison); `bridge_memory`.
                    With MRT_ADB, Fenix's processes, by dumpsys meminfo (task bbc6, mrt_phone.memory_reports)
MRT_ADB=1           the runtime on a phone (task 6da8, bench/mrt_phone.py): realsite under /data/local/tmp/mrt
                    through adb, release Fenix as the bridge (replay only; FIREFOX_BIN is not needed); with
                    HEAP_SCAN the pages are read by the phone's memdump; MRT_PAGEOUT=S: after the report the
                    runtime pages its anonymous memory out into zram and stays S seconds, while zram's use is read
VIEWER_HTTP=127.0.0.1:8765  keep one runtime alive and serve an attachable HTTP viewer (ONLY=name).
                    Implies NO_FIREFOX=1 and API_PROCESS=all; open the printed VIEWER_URL in any browser.
                    Closing/disconnecting the viewer leaves the runtime running. Ctrl+C stops it.
VIEWER_MEDIA_REPLAY_UNLIMITED=1  disable HTTP MSE replay byte/command retention quotas.
                    Retained memory can grow without bound; live transport and decoder limits still apply.
                    Startup option; absent or any value other than 1 keeps the bounded default.
NO_FIREFOX=1       run with mrt-api only: no Firefox, Marionette, display or ./mach needed. Defaults to
                    API_PROCESS=all; incompatible with interactive/rendered/phone runs and API_ENV_CHECK.
                    API_ENV=/path/to/env.json selects an existing browser environment recording (otherwise the
                    latest matching file in realsites-env). Missing context recordings and Firefox-only requests
                    fail explicitly; unsupported_ops lists the latter. REALSITES_PROFILE seeds stores as usual.
API_PROCESS=crypto,storage,cookie,fetch  the op kinds sent to an API process of our own, bench/../mrt-api
                    (SPEC decision 27, DESIGN "Phase 6"), not to the bridge tab: a comma list, or `all` for every kind it
                    implements (API_KINDS); one process per site, `api_process` in the results counts the calls and
                    their round trips. Its cookie jar and localStorage are SQLite files in realsites-mini/<site>-api-store,
                    seeded from the test profile. With fetch (which needs cookie) the page's requests and its document's
                    go out from it, through the replay's proxy; with cookie and not fetch, the page's cookie writes are
                    also made in the tab and Firefox's jar is copied back after the tab's network use (`cookie_sync`).
                    idb serves origin-scoped native IndexedDB in the partition SQLite store (fresh, not seeded from Firefox IDB).
                    mediaType is unsupported in plain standalone mode; HTTP viewers and Firefox bridge mode query their real browser backend.
                    permissions answers original-origin queries from persistent profile decisions/default policy, with no viewer-origin UI.
                    image (needs fetch,cookie) decodes network/data/blob images there, returning dimensions without keeping pixels.
                    webgl (needs image) runs the page's WebGL on real OpenGL ES contexts there (EGL surfaceless, Mesa's
                    llvmpipe unless MRT_GL=default); an HTTP viewer then only draws (GAPS B169).
                    ws (needs cookie) makes the page's WebSockets there; env answers the browser's identity from a
                    recording of this Firefox (realsites-env/<build>[-replay].json, made from the tab the first time).
                    With fetch and env routed the run has no bridge tab until the page makes a request only the tab
                    serves, then a lean one (`bridge_tab` in the results: by which request); a bridge drop ends the
                    API processes too. API_ENV_CHECK=1 keeps the tab and compares env with it (`api_process.env_check`).
                    Unset, the bridge tab answers everything. API_BIN=PATH another mrt-api binary
HEAP_SCAN=1         at the report, the runtime's resident anonymous memory split into SpiderMonkey's GC chunks
                    and the rest, and its DOM as saved (Dom::to_bytes), each compressed (bench/heap_compress.py)
Every request the runtime makes through the bridge is listed in realsites-mini/<site>-requests.jsonl
(method, URL, status or error), to diff against the proxy's log of a Firefox run.
"""
import json, os, shlex, shutil, subprocess, sys, threading, time
import firefox_exit
VIEWER_HTTP = os.environ.get("VIEWER_HTTP")
NO_FIREFOX = os.environ.get("NO_FIREFOX") == "1" or bool(VIEWER_HTTP)
if NO_FIREFOX and any(os.environ.get(k) == "1" for k in ("INTERACTIVE", "RENDER", "MRT_ADB", "API_ENV_CHECK")):
    sys.exit("NO_FIREFOX=1 cannot use INTERACTIVE, RENDER, MRT_ADB or API_ENV_CHECK")
if not NO_FIREFOX:
    from marionette_driver.marionette import Marionette
    from marionette_driver import errors

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import replay
import heap_compress
import firefox_process_files
HEAP_SCAN = os.environ.get("HEAP_SCAN") == "1"
BRIDGE_MEMORY = os.environ.get("BRIDGE_MEMORY") == "1"
BRIDGE_LEAN = os.environ.get("BRIDGE_LEAN") == "1"
BRIDGE_SANDBOX = os.environ.get("BRIDGE_SANDBOX", "new")
BRIDGE_SERIES = os.environ.get("BRIDGE_SERIES") == "1"
BRIDGE_PRESS = os.environ.get("BRIDGE_PRESS") == "1"
BRIDGE_IDB_STOP = os.environ.get("BRIDGE_IDB_STOP") == "1"
PHONE = os.environ.get("MRT_ADB") == "1"
if PHONE:
    import mrt_phone
PAGEOUT = os.environ.get("MRT_PAGEOUT")
BIN = os.environ.get("FIREFOX_BIN") if PHONE or NO_FIREFOX else os.environ["FIREFOX_BIN"]
PROFILE = os.environ.get("REALSITES_PROFILE", os.path.expanduser("~/.cache/mrt-realsites-profile"))
REALSITE = os.environ.get("REALSITE_BIN", os.path.join(HERE, "..", "mrt-js", "target", "release", "realsite"))
# MINI_DIR: another directory for the documents and logs, so that two runs (a phone's, a desktop's) do not mix
OUT_DIR = os.path.join(HERE, os.environ.get("MINI_DIR", "realsites-mini"))
VIEW_W, VIEW_H = 1489, 844
WALL_LIMIT = int(os.environ.get("WALL_LIMIT", "1200"))  # seconds per site
FRAME_WAIT = 10  # seconds a cross-origin frame's <iframe> in the bridge tab has to start its navigation
RENDER = os.environ.get("RENDER") == "1"
# INTERACTIVE=1: the page runs on the real clock until its bridge tab is closed (Ctrl-C would end
# Firefox and the replay proxy too) or INTERACTIVE_S seconds after it loaded, then is measured (SPEC decision 25); no WALL_LIMIT
INTERACTIVE = os.environ.get("INTERACTIVE") == "1"
INTERACTIVE_S = float(os.environ["INTERACTIVE_S"]) if os.environ.get("INTERACTIVE_S") else None

# the same pages as bench/firefox_realsites.py
SITES = {
    "facebook": "https://www.facebook.com/",
    "instagram": "https://www.instagram.com/",
    "gdocs": os.environ.get("GDOC_URL", "https://docs.google.com/"),
    "gmail": "https://mail.google.com/mail/u/0/#inbox",
    "x": "https://x.com/home",
    "youtube": "https://www.youtube.com/watch?v=jNQXAC9IVRw",
    "gmaps": "https://www.google.com/maps/@48.8566,2.3522,15z",
    "reddit": "https://www.reddit.com/r/AskReddit/top/?t=week",
    "amazon": "https://www.amazon.com/s?k=laptop",
    "cnn": "https://www.cnn.com/",
}
# EXTRA_SITES='{"name": "url"}': more pages, such as a local test page
SITES.update(json.loads(os.environ.get("EXTRA_SITES", "{}")))
NAMES = os.environ.get("ONLY", "cnn,amazon").split(",")
if "gdocs" in NAMES and not os.environ.get("GDOC_URL") and "gdocs" not in json.loads(os.environ.get("EXTRA_SITES", "{}")):
    sys.exit("Set GDOC_URL to your own document, or override gdocs in EXTRA_SITES.")
if VIEWER_HTTP and len(NAMES) != 1:
    sys.exit("VIEWER_HTTP serves one runtime per process; select it with ONLY=name")

PREFS = {
    "marionette.allowSystemAccess": True,
    "app.normandy.enabled": False, "messaging-system.rsexperimentloader.enabled": False,
    "app.update.disabledForTesting": True, "app.update.auto": False, "app.update.enabled": False,
    "datareporting.policy.dataSubmissionEnabled": False, "toolkit.telemetry.enabled": False,
    "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False,
}

JS_OFF = "gBrowser.selectedBrowser.browsingContext.allowJavascript = false;"

# One request from the mini runtime, run in the bridge tab's content sandbox. `window` is the page's
# window seen through Xrays: its WebIDL methods and attributes, none of the page's own JS.
# a bridge fetch Firefox has not answered by then stays in flight in the runtime (a long poll)
PENDING_MS = 5000

OP = """const [req] = arguments; const done = arguments[arguments.length - 1];
const PENDING_MS = """ + str(PENDING_MS) + """;
(async () => {
  switch (req.op) {
    case "env": {
""" + open(os.path.join(HERE, "browser_env.js")).read() + """
    }
    case "fetch": {
      const init = {method: req.method || "GET", credentials: req.credentials || "same-origin", headers: req.headers || [], redirect: req.redirect || "follow"};
      // the page's mode (an opaque answer for no-cors), cache, referrer policy and keepalive
      for (const k of ["mode", "cache", "referrerPolicy", "keepalive"]) if (req[k] !== undefined && req[k] !== "") init[k] = req[k];
      // a binary request body (YouTube's protobuf POSTs) comes as base64
      if (init.method !== "GET" && init.method !== "HEAD") {
        if (req.bodyBase64 != null) {
          // the page's own Uint8Array: the sandbox's typed arrays cannot cross the Xray into fetch
          const bin = window.atob(req.bodyBase64), bytes = new window.wrappedJSObject.Uint8Array(bin.length);
          for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
          init.body = bytes;
        }
        else if (req.body != null) init.body = req.body;
      }
      // a fetch Firefox has not answered in PENDING_MS is a long poll the server (or the replay,
      // replay_addon.py LONG_POLL_S) holds open: the runtime keeps it in flight, as the page in
      // Firefox has it in flight while it is measured, instead of the runtime waiting on it
      const t0 = Date.now();
      const r = await Promise.race([window.fetch(req.url, init), new Promise(done => window.setTimeout(() => done(null), PENDING_MS))]);
      if (!r) return {pending: true};
      const headers = [];
      r.headers.forEach((v, k) => headers.push([k, v]));
      const type = r.headers.get("content-type") || "";
      // text as text; anything else as base64, so its bytes survive: media segments a page appends
      // to a SourceBuffer, binary APIs, and images and fonts too, as only a page's own fetches and
      // XHRs come here, never an <img>'s (task 85f3: Instagram's ring animation is served as
      // image/x.fb.keyframes and read as an ArrayBuffer; session 49: YouTube's SVG icons)
      // Text in a charset other than UTF-8 comes as both: r.text() is UTF-8, and an XHR decodes the
      // bytes with the charset (task 09c9); scripts, sheets and documents read the text. A request
      // with `bytes` (an XHR whose overriding type has a charset) gets only the bytes
      let body = null, bodyBase64 = null;
      const charset = (/;\\s*charset\\s*=\\s*"?([^";]*)/i.exec(type) || [])[1];
      const utf8 = !charset || /^\\s*(utf-?8|unicode-1-1-utf-8|unicode11utf8|unicode20utf8|x-unicode20utf8)\\s*$/i.test(charset);
      // base64 made by the page's FileReader (the sandbox cannot read the page's ArrayBuffer)
      const base64Of = blob => new Promise((resolve, reject) => {
        const fr = new window.FileReader();
        fr.onload = () => resolve(fr.result.slice(fr.result.indexOf(",") + 1));
        fr.onerror = () => reject(fr.error);
        fr.readAsDataURL(blob);
      });
      // the body by the same deadline: a stream the server keeps open (X's live_pipeline/events,
      // text/event-stream) has its headers at once and never ends, and waiting on it held the driver,
      // and so the runtime, until the script timed out (task 0e92)
      const LATE = {};
      const late = new Promise(done => window.setTimeout(() => done(LATE), Math.max(0, t0 + PENDING_MS - Date.now())));
      const unfinished = await Promise.race([late, (async () => {
        if (!req.noBody) {
          if (type === "" || /^text\\/|json|xml|javascript|ecmascript|html|x-www-form-urlencoded|mpegurl/i.test(type)) {
            if (req.bytes) bodyBase64 = await base64Of(await r.blob());
            else if (utf8) body = await r.text();
            else {
              const blob = await r.blob();
              body = await blob.text();
              bodyBase64 = await base64Of(blob);
            }
          } else bodyBase64 = await base64Of(await r.blob());
        }
      })()]);
      if (unfinished === LATE) return {pending: true};
      return {status: r.status, statusText: r.statusText, url: r.url, redirected: r.redirected, type: r.type, headers, body, bodyBase64, took: Date.now() - t0};
    }
    case "image": {
      // an <img> of the runtime's (bridge.js, GAPS B128): Firefox loads it as an image of this tab's
      // document, not in it; the answer is whether it loaded and its size, with the time it took
      // (no pixels). A blob: URL of the runtime's comes as its bytes, loaded as a data: URL
      const t0 = window.performance.now();
      const img = new window.Image();
      if (req.crossOrigin !== null && req.crossOrigin !== undefined) img.setAttribute("crossorigin", req.crossOrigin);
      if (req.referrerPolicy) img.setAttribute("referrerpolicy", req.referrerPolicy);
      const url = req.bodyBase64 != null ? "data:" + (req.type || "").replace(/[,;].*/, "") + ";base64," + req.bodyBase64 : req.url;
      // one Firefox has not finished in PENDING_MS stays loading in the runtime, as a long fetch does
      const ok = await new Promise(resolve => {
        img.addEventListener("load", () => resolve(true));
        img.addEventListener("error", () => resolve(false));
        img.src = url;
        window.setTimeout(() => resolve(null), PENDING_MS);
      });
      if (ok === null) return {pending: true};
      return {ok, width: img.naturalWidth, height: img.naturalHeight, ms: window.performance.now() - t0};
    }
    case "mediaType":
      // Firefox's own answers (its platform's codecs): MediaSource.isTypeSupported and canPlayType
      return [window.MediaSource ? window.MediaSource.isTypeSupported(req.type) : false, window.document.createElement("video").canPlayType(req.type)];
    case "permissions": {
      // navigator.permissions.query and navigator.storage's estimate, persisted and persist, answered
      // by Firefox for the page's origin (its permission manager and quota manager), with the time
      // Firefox took, which the runtime waits on its clock
      const t0 = window.performance.now();
      const answer = await (async () => {
        try {
          if (req.call === "query") {
            const desc = window.wrappedJSObject.JSON.parse(JSON.stringify(req.desc));
            const status = await window.navigator.permissions.query(desc);
            return {value: {name: status.name, state: status.state}};
          }
          // Notification.permission, and requestPermission (the bridge tab has no user activation,
          // so Firefox's front-end cancels the request; one still pending is never answered)
          if (req.call === "notification") return {value: window.Notification.permission};
          // a geolocation request made in this tab (task 09c9): the error's code, or the position, or
          // pending when Firefox has not answered (a prompt someone could still answer)
          if (req.call === "geolocation") {
            const value = await Promise.race([new Promise(done => window.navigator.geolocation.getCurrentPosition(
              p => done({position: {timestamp: p.timestamp, coords: JSON.parse(window.wrappedJSObject.JSON.stringify(p.coords.toJSON()))}}),
              e => done({code: e.code}), {timeout: PENDING_MS})),
              new Promise(done => window.setTimeout(() => done(null), PENDING_MS))]);
            return value === null ? {pending: true} : {value};
          }
          if (req.call === "requestNotification") {
            const value = await Promise.race([window.Notification.requestPermission(),
                                              new Promise(done => window.setTimeout(() => done(null), PENDING_MS))]);
            return value === null ? {pending: true} : {value};
          }
          // persist() waits on a doorhanger unless the origin's own permission is saved
          if (req.call === "persist") {
            const value = await Promise.race([window.navigator.storage.persist(),
                                              new Promise(done => window.setTimeout(() => done(null), PENDING_MS))]);
            return value === null ? {pending: true} : {value};
          }
          return {value: await window.navigator.storage[req.call]()};
        } catch (e) {
          return {error: {name: e.name, message: e.message}};
        }
      })();
      // the dictionary out of the page's compartment
      if (answer.value && typeof answer.value === "object") answer.value = JSON.parse(window.wrappedJSObject.JSON.stringify(answer.value));
      answer.ms = window.performance.now() - t0;
      return answer;
    }
    case "cookie":
      if ("value" in req) { window.document.cookie = req.value; return null; }
      return window.document.cookie;
    case "storage": {
      // localStorage only: sessionStorage is the tab's, kept in the runtime (SPEC decision 14)
      const s = window[req.area];
      if (req.call === "length") return s.length;
      if (req.call === "names") {
        const names = [];
        for (let i = 0; i < s.length; i++) names.push(s.key(i));
        return names;
      }
      if (req.call === "setItem") {
        // false: the quota is exceeded (the runtime throws QuotaExceededError)
        try { s.setItem(...req.args); } catch (e) { if (e.name === "QuotaExceededError") return false; throw e; }
        return true;
      }
      const v = s[req.call](...req.args);
      return v === undefined ? null : v;
    }
  }
  throw new Error("unknown request " + req.op);
})().then(ok => done({ok: ok === undefined ? null : ok}), e => done({err: String(e)}));"""


# The mini runtime's DOM in place of the page's, in a tab on the site's URL with JS off: same origin
# (fonts, CORS, cookies for images), Firefox loads the stylesheets and images; no script runs.
SWAP_DOM = """const [html] = arguments;
const doc = new window.DOMParser().parseFromString(html, "text/html");
const root = window.document.importNode(doc.documentElement, true);
window.document.replaceChild(root, window.document.documentElement);
return window.document.querySelectorAll("*").length;"""


# The runtime's document copied into the bridge tab (bridge.js mirrorFlush, which lists the ops): run in
# one sandbox kept across calls, whose `mirrorNodes` maps the runtime's ids to the tab's nodes (0 is
# the document), and `mirrorIds` the other way, for the owner's input (INPUT_CAPTURE). A node the ops
# name that the tab lacks is skipped; an attribute or rule Firefox refuses is dropped. A "scroll" op
# (the page's scroll: its number, x, y) moves the tab there at once.
MIRROR_APPLY = open(os.path.join(HERE, "http-viewer", "adopted.js")).read() + """const [ops] = arguments;
const doc = window.document;
const nodes = globalThis.mirrorNodes || (globalThis.mirrorNodes = new Map([[0, doc]]));
const ids = globalThis.mirrorIds || (globalThis.mirrorIds = new WeakMap([[doc, 0]]));
const XHTML = "http://www.w3.org/1999/xhtml";
// (a media element's blob: src names the runtime's MediaSource, nothing in the tab: the tab's
// element gets its own, MEDIA_SETUP's attach)
const isMedia = el => el.namespaceURI === XHTML && (el.localName === "video" || el.localName === "audio");
const setAttrs = (el, attrs) => {
  const want = new Set(attrs.map(a => a[0]));
  for (const name of el.getAttributeNames()) if (!want.has(name)) el.removeAttribute(name);
  for (const [name, value] of attrs) {
    if (name === "src" && value.startsWith("blob:") && isMedia(el)) continue;
    try { if (el.getAttribute(name) !== value) el.setAttribute(name, value); } catch (e) {}
  }
};
const kidsOf = node => (node.localName === "template" && node.namespaceURI === XHTML ? node.content : node);
// the rules last given to each sheet, by id: Firefox parses a <style> again from its text when it is
// moved, so they are given again to one that moves
const rules = globalThis.mirrorRules || (globalThis.mirrorRules = new Map());
const setRules = (node, list) => {
  const sheet = node.sheet;
  if (!sheet) return;
  try { while (sheet.cssRules.length) sheet.deleteRule(sheet.cssRules.length - 1); } catch (e) { return; }
  for (const rule of list) {
    try { sheet.insertRule(rule, sheet.cssRules.length); }
    catch (e) { refused.count++; if (refused.examples.length < 5) refused.examples.push(e.name + ": " + rule.slice(0, 200)); }
  }
};
const refused = { count: 0, examples: [] };
const mediaRefused = { count: 0, examples: [] };
// a parent's children made `kids`, moving only those out of place (a move of a <style> drops its
// rules in Firefox); the moved ones are returned
function syncChildren(parent, kids) {
  const want = new Set(kids);
  for (const k of Array.from(parent.childNodes)) if (!want.has(k)) k.remove();
  const moved = [];
  let cur = parent.firstChild;
  for (const k of kids) {
    if (cur === k) { cur = cur.nextSibling; continue; }
    if (k.parentNode) moved.push(k);
    parent.insertBefore(k, cur);
  }
  while (cur) { const next = cur.nextSibling; cur.remove(); cur = next; }
  return moved;
}
function build(spec) {
  if (typeof spec === "number") return nodes.get(spec) || null;
  let node;
  if (spec.t !== undefined) node = doc.createTextNode(spec.t);
  else if (spec.m !== undefined) node = doc.createComment(spec.m);
  else {
    try { node = doc.createElementNS(spec.s || XHTML, spec.n); } catch (e) { node = doc.createElement("span"); }
    setAttrs(node, spec.a);
    const kids = spec.c.map(build).filter(Boolean);
    kidsOf(node).append(...kids);
    if (spec.r) shadow(node, spec.r);
  }
  nodes.set(spec.i, node);
  ids.set(node, spec.i);
  return node;
}
function shadow(host, spec) {
  let root = host.shadowRoot;
  try { root = root || host.attachShadow({ mode: "open" }); } catch (e) { return; }
  nodes.set(spec.i, root);
  ids.set(root, spec.i);
  if (globalThis.inputWatch) globalThis.inputWatch(root);
  if (globalThis.mediaWatch) globalThis.mediaWatch(root);
  root.replaceChildren(...spec.c.map(build).filter(Boolean));
  mirrorAdoptedSheets(root, spec.adopted);
}
let applied = 0;
for (const op of ops) {
  const [kind, id, arg] = op;
  if (kind === "u") { try { window.history.replaceState(null, "", id); } catch (e) {} continue; }
  if (kind === "scroll") {
    // the page scrolled (its `seq`, then x, y): the tab goes there at once; the scroll event this
    // gives the tab is the echo (INPUT_CAPTURE), the runtime having fired its own
    const before = [window.scrollX, window.scrollY];
    window.scrollTo({ left: arg, top: op[3], behavior: "instant" });
    globalThis.mirrorScrollSeq = id;
    const after = [window.scrollX, window.scrollY];
    globalThis.mirrorScrollEcho = before[0] === after[0] && before[1] === after[1] ? null : after;
    continue;
  }
  if (kind === "m") {
    // what the page asked of its media (MEDIA_SETUP), after the nodes it names
    try { globalThis.mediaApply(id); } catch (e) { mediaRefused.count++; if (mediaRefused.examples.length < 5) mediaRefused.examples.push(id.o + " " + e.name + ": " + e.message); }
    continue;
  }
  if (kind === "adopted") {
    let el = doc.getElementById("__mrt_adopted");
    if (!el && doc.head) { el = doc.createElement("style"); el.id = "__mrt_adopted"; doc.head.append(el); }
    if (el) el.textContent = id;
    continue;
  }
  const node = nodes.get(id);
  if (!node) continue;
  applied++;
  if (kind === "c") {
    const kids = arg.map(build).filter(Boolean);
    if (rules.has(id)) rules.delete(id);
    if (node === doc) {
      for (const k of Array.from(doc.childNodes)) if (k.nodeType === 1 || k.nodeType === 8) k.remove();
      doc.append(...kids);
    } else {
      for (const k of syncChildren(kidsOf(node), kids)) {
        if (k.nodeType !== 1) continue;
        for (const el of [k, ...k.querySelectorAll("style, link")]) {
          for (const [rid, list] of rules) if (nodes.get(rid) === el) setRules(el, list);
        }
      }
    }
  } else if (kind === "a") setAttrs(node, arg);
  else if (kind === "d") {
    node.data = arg;
    // a <style> whose text changed is parsed again on both sides: its stored rules are stale
    for (const [rid] of rules) if (nodes.get(rid) === node.parentNode) rules.delete(rid);
  }
  else if (kind === "r") shadow(node, arg);
  else if (kind === "adopted-root") mirrorAdoptedSheets(node, arg);
  else if (kind === "v") {
    // a form control's value and checkedness (set only when they differ: setting moves the caret)
    if (node.value !== arg) node.value = arg;
    if (node.localName === "input" && node.checked !== op[3]) node.checked = op[3];
  }
  else if (kind === "s") {
    rules.set(id, arg);
    setRules(node, arg);
  }
}
return { applied, refused, mediaRefused };"""

# The copy's media elements playing in the tab (task 753cc, SPEC decision 26; bridge.js "Media in the
# bridge tab" lists the ops): installed once in the mirror sandbox before the first MIRROR_APPLY.
# Each runtime MediaSource is a real one of the tab's window, its URL the copy element's src; the ops
# of a source and its buffers run one at a time in their order, an append or remove waiting for its
# buffer's updateend (abort drops that buffer's waiting ones at once). The bytes come as base64 and
# are decoded in the page's realm (Uint8Array.fromBase64 of the page's window: a sandbox cannot
# write into the page's typed arrays across the Xray, and a data: URL's fetch is the page's CSP's
# to refuse). Back to the runtime, in `mediaQueue` (INPUT_TAKE): every media event of
# a copy element with its state and the seq of the element's last op applied; each append's and
# remove's end with the buffer's ranges, the source's duration and the error if one; each play()'s
# answer. NaN and Infinity travel as strings.
MEDIA_SETUP = open(os.path.join(HERE, "http-viewer", "media.js")).read() + """
if (globalThis.mediaApply) return false;
const queue = globalThis.mediaQueue = [];
const media = createMrtMedia(window, () => globalThis.mirrorNodes || new Map(), () => globalThis.mirrorIds, m => queue.push(m));
globalThis.mediaApply = op => media.apply(op);
globalThis.mediaWatch = root => media.watchRoot(root);
return true;"""

# The owner's input in the bridge tab, caught for the runtime (realsite --interactive, bridge.js
# inputEvents): in the mirror sandbox, listeners at the window (capture) queue each trusted event the
# copy's nodes get, as its type, interface, target (the first node of its composed path, so inside
# shadow trees too), related nodes and the focused element by the copy's ids, and the fields of its
# interface as Firefox set them; a control's value and checkedness with input and change. Every event
# carries the tab's scroll offset and the number of the page's last scroll the tab applied (task
# 846d); the document's scroll and scrollend are sent too, those the page's own scroll gave the tab
# marked `echo` (the runtime fired its own); an element's scrolling is not (GAPS B155). `task`
# numbers the widget events (Gecko dispatches each one's events in one task: pointerdown, mousedown
# and the focus they move): one starts at a pointer event after other events, and at keydown, keyup
# and wheel; the compatibility mouse events, focus, input and change join the one before. The tab
# itself must not move: it cancels a primary click on a link or a submit button, and a submission.
# Its other default actions stay (focus, typing, scrolling, hover), as the tab is what the owner sees.
INPUT_CAPTURE = """if (globalThis.inputQueue) return false;
const queue = globalThis.inputQueue = [];
const doc = window.document;
const ids = globalThis.mirrorIds || (globalThis.mirrorIds = new WeakMap([[doc, 0]]));
const idOf = n => (n && ids.has(n) ? ids.get(n) : null);
const TYPES = ["pointerover", "pointerenter", "pointerdown", "pointermove", "pointerup", "pointercancel", "pointerout", "pointerleave",
  "gotpointercapture", "lostpointercapture", "mouseover", "mouseenter", "mousemove", "mousedown", "mouseup", "mouseout", "mouseleave",
  "click", "auxclick", "dblclick", "contextmenu", "wheel", "keydown", "keypress", "keyup", "focus", "blur", "focusin", "focusout",
  "beforeinput", "input", "change", "submit", "reset", "select", "scroll", "scrollend"];
// each interface's own fields, and its parent's (Gecko's WebIDL)
const OWN = {
  UIEvent: [null, ["detail", "which"]],
  MouseEvent: ["UIEvent", ["screenX", "screenY", "clientX", "clientY", "pageX", "pageY", "x", "y", "offsetX", "offsetY", "movementX", "movementY",
    "layerX", "layerY", "button", "buttons", "ctrlKey", "shiftKey", "altKey", "metaKey"]],
  PointerEvent: ["MouseEvent", ["pointerId", "width", "height", "pressure", "tangentialPressure", "tiltX", "tiltY", "twist", "altitudeAngle",
    "azimuthAngle", "pointerType", "isPrimary"]],
  WheelEvent: ["MouseEvent", ["deltaX", "deltaY", "deltaZ", "deltaMode"]],
  KeyboardEvent: ["UIEvent", ["key", "code", "keyCode", "charCode", "location", "repeat", "isComposing", "ctrlKey", "shiftKey", "altKey", "metaKey"]],
  FocusEvent: ["UIEvent", []],
  InputEvent: ["UIEvent", ["data", "inputType", "isComposing"]],
  SubmitEvent: [null, []],
};
let task = 0, lastType = "";
const startsTask = type => (type.startsWith("pointer") && !lastType.startsWith("pointer")) || type === "keydown" || type === "keyup" || type === "wheel"
  || type === "scroll" || type === "scrollend";
const fieldsOf = iface => { const out = []; for (let i = iface; OWN[i]; i = OWN[i][0]) out.push(...OWN[i][1]); return out; };
const isSubmit = n => (n.localName === "button" && (n.getAttribute("type") || "submit").toLowerCase() === "submit" && n.form)
  || (n.localName === "input" && ["submit", "image"].includes((n.getAttribute("type") || "").toLowerCase()) && n.form);
const listener = e => {
  if (!e.isTrusted) return;
  const path = e.composedPath();
  const target = idOf(path[0]);
  if (target === null) return;
  // the document's scrolling only: an element's scroll container does not scroll in the runtime
  if ((e.type === "scroll" || e.type === "scrollend") && target !== 0) return;
  const iface = Object.prototype.toString.call(e).slice(8, -1);
  const fields = {};
  for (const f of fieldsOf(iface)) fields[f] = e[f];
  if (startsTask(e.type)) task++;
  lastType = e.type;
  // the tab's scroll offset with every event, and the page's last scroll the tab has applied
  const ev = { type: e.type, iface, target, active: idOf(doc.activeElement), task, bubbles: e.bubbles,
    cancelable: e.cancelable, composed: e.composed, fields, scroll: [window.scrollX, window.scrollY],
    scrollSeq: globalThis.mirrorScrollSeq || 0, at: Date.now() };
  // the tab's events of the page's own scroll (MIRROR_APPLY), which the runtime fired itself: the
  // first scroll at the offset it went to, and the scrollend after it
  const echo = globalThis.mirrorScrollEcho;
  if (e.type === "scroll" && echo) {
    if (echo[0] === window.scrollX && echo[1] === window.scrollY) ev.echo = globalThis.mirrorScrollEndEcho = true;
    globalThis.mirrorScrollEcho = null;
  } else if (e.type === "scrollend" && globalThis.mirrorScrollEndEcho) {
    ev.echo = true;
    globalThis.mirrorScrollEndEcho = false;
  }
  if ("relatedTarget" in e) ev.related = idOf(e.relatedTarget);
  if (iface === "SubmitEvent") ev.submitter = idOf(e.submitter);
  // the tab's selection as the event found it (X's click on a post reads its text: none, it opens)
  const sel = window.getSelection();
  if (sel) ev.selection = [sel.type, sel.toString(), idOf(sel.anchorNode), sel.anchorOffset, idOf(sel.focusNode), sel.focusOffset];
  const t = path[0];
  if ((e.type === "input" || e.type === "change") && (t.localName === "input" || t.localName === "textarea")) ev.value = [t.value, !!t.checked];
  queue.push(ev);
  if ((e.type === "click" && e.button === 0 && path.some(n => n.nodeType === 1 && (((n.localName === "a" || n.localName === "area") && n.hasAttribute("href")) || isSubmit(n))))
      || e.type === "submit") e.preventDefault();
};
for (const type of TYPES) window.addEventListener(type, listener, true);
// the tab hidden or shown (task e83d): not an event for the page, but the driver's `hidden`, which
// hibernates or wakes the runtime
doc.addEventListener("visibilitychange", e => { if (e.isTrusted) queue.push({ type: "visibilitychange", hidden: doc.hidden, at: Date.now() }); }, true);
// an event that is not composed (mouseenter, pointerleave, change) stays in its shadow tree: caught
// at each open shadow root of the copy, now and as MIRROR_APPLY makes them
globalThis.inputWatch = root => { for (const type of TYPES) root.addEventListener(type, e => { if (!e.composed) listener(e); }, true); };
for (const node of (globalThis.mirrorNodes || new Map()).values()) if (node.nodeType === 11) globalThis.inputWatch(node);
return true;"""

# the tab's media elements at an INPUT_ACTIONS mark (task 753cc)
MEDIA_MARK = """const ids = globalThis.mirrorIds;
const els = [];
const walk = root => { for (const el of root.querySelectorAll("video, audio, *")) { if (el.localName === "video" || el.localName === "audio") els.push(el); if (el.shadowRoot) walk(el.shadowRoot); } };
walk(window.document);
// and the copies no longer in the document (a copy the mirror replaced keeps its own media)
for (const node of (globalThis.mirrorNodes || new Map()).values()) if (node.nodeType === 1 && (node.localName === "video" || node.localName === "audio") && !els.includes(node)) els.push(node);
return els.map(el => ({ id: ids && ids.has(el) ? ids.get(el) : null, connected: el.isConnected, paused: el.paused, ended: el.ended,
  t: Math.round(el.currentTime * 10) / 10, rs: el.readyState, ns: el.networkState, muted: el.muted, loop: el.loop, src: (el.getAttribute("src") || "").slice(0, 40), srcp: (el.src || "").slice(0, 40),
  error: el.error && el.error.message, buffered: Array.from({ length: el.buffered.length }, (_, i) => [el.buffered.start(i), el.buffered.end(i)].map(v => Math.round(v * 10) / 10)) }));"""

# the events caught since the last call
INPUT_TAKE = """const q = globalThis.inputQueue || [], mq = globalThis.mediaQueue || [];
return q.splice(0, q.length).concat(mq.splice(0, mq.length));"""

# INPUT_ACTIONS=FILE: WebDriver actions done in the bridge tab from the page's first wait on, as the
# owner would, a few at each wait (input_actions.Steps; the bridge cases' input, firefox_probe.py
# does the same in Firefox's own tab)
INPUT_ACTIONS = os.environ.get("INPUT_ACTIONS")

# The bridge tab's document as bridge.js's mirrorDigest writes the runtime's, to compare the two
# (nonce values left out: Firefox hides them once a CSP applies, the runtime does not, GAPS B150).
MIRROR_DIGEST = """const XHTML = "http://www.w3.org/1999/xhtml";
const out = [];
const visit = (node, depth) => {
  const pad = " ".repeat(depth);
  if (node.nodeType === 3) out.push(pad + "#text " + JSON.stringify(node.data));
  else if (node.nodeType === 8) out.push(pad + "#comment " + JSON.stringify(node.data));
  else if (node.nodeType === 1) {
    const media = node.namespaceURI === XHTML && (node.localName === "video" || node.localName === "audio");
    const attrs = node.getAttributeNames().filter(a => !(a === "src" && media && (node.getAttribute(a) || "").startsWith("blob:")))
      .map(a => [a, a === "nonce" ? "" : node.getAttribute(a) ?? ""]).sort();
    out.push(pad + (node.namespaceURI === XHTML ? "" : node.namespaceURI + " ") + node.localName + " " + JSON.stringify(attrs));
    const root = node.shadowRoot;
    if (root) { out.push(pad + " #shadow-root " + root.mode); for (const k of root.childNodes) visit(k, depth + 2); }
    for (const k of (node.localName === "template" && node.namespaceURI === XHTML ? node.content : node).childNodes) visit(k, depth + 1);
  }
};
for (const k of window.document.childNodes) if (k.nodeType === 1 || k.nodeType === 8) visit(k, 0);
return out.join("\\n");"""


# Each runtime sheet's rule count against the tab's copy's, and the tab's fonts.
MIRROR_SHEETS = """const [sheets] = arguments;
const nodes = globalThis.mirrorNodes || new Map();
const out = sheets.map(([id, tag, n]) => {
  const el = nodes.get(id);
  let tab = null;
  try { tab = el && el.sheet ? el.sheet.cssRules.length : null; } catch (e) { tab = "error " + e.name; }
  return [id, tag, n, tab];
}).filter(([, , n, tab]) => n !== tab);
const fonts = [];
window.document.fonts.forEach(f => fonts.push(f.family + " " + f.weight + " " + f.status));
return { sheets_differing: out, sheets: sheets.length, fonts };"""


def mirror_check(m, name, digest, sheets=()):
    """The tab's copy against the runtime's document (bridge.js mirrorDigest): equal, or the first lines
    that differ; and a screenshot of the tab, realsites-mini/<site>-mirror.png."""
    tab = m.execute_script(MIRROR_DIGEST, sandbox="mirror", new_sandbox=False)
    shoot(m, os.path.join(OUT_DIR, name + "-mirror.png"))
    mine, theirs = digest.split("\n"), tab.split("\n")
    out = {"equal": mine == theirs, "runtime_lines": len(mine), "tab_lines": len(theirs)}
    out.update(m.execute_script(MIRROR_SHEETS, script_args=(list(sheets),), sandbox="mirror", new_sandbox=False))
    if mine != theirs:
        i = next((i for i, (a, b) in enumerate(zip(mine, theirs)) if a != b), min(len(mine), len(theirs)))
        out["first_difference"] = {"line": i, "runtime": mine[i:i + 3], "tab": theirs[i:i + 3]}
        open(os.path.join(OUT_DIR, name + "-mirror-runtime.txt"), "w").write(digest)
        open(os.path.join(OUT_DIR, name + "-mirror-tab.txt"), "w").write(tab)
    return out


def click_times(acts, timeline):
    """Each click or Enter of the owner's, with how long the tab took to show its effect, ms from the
    tab's event: `taken` (the driver took it from the tab), `first` (the first change applied after
    it), `url` (the first URL change after it, within 15 s), `settled` (the last change of the burst
    it began: changes less than 1 s apart, up to 15 s) and `flushes` in that burst."""
    out = []
    for a in acts:
        at = a["at"]
        if at is None:
            continue
        after = [f for f in timeline if f[0] >= at and f[0] - at <= 15000]
        r = {"type": a["type"], "taken": a["taken"] - at, "first": after[0][0] - at if after else None,
             "url": next((f[0] - at for f in after if f[2]), None)}
        last = after[0][0] if after else None
        n = 0
        for f in after:
            if f[0] - last > 1000:
                break
            last, n = f[0], n + 1
        r["settled"], r["flushes"] = (last - at if last else None), n
        out.append(r)
    return out


def shoot(m, path, full=False):
    """A screenshot of the current tab's viewport (or the whole page) to `path`."""
    import base64
    open(path, "wb").write(base64.b64decode(m.screenshot(full=full)))


def render(m, name, url, html):
    """Renders the mini runtime's DOM in a new tab and takes its screenshots; back to the bridge tab."""
    open(os.path.join(OUT_DIR, name + "-mini-dom.html"), "w").write(html)
    bridge = m.current_window_handle
    h = m.open(type="tab", focus=True)["handle"]
    m.switch_to_window(h)
    m.navigate("about:blank")
    chrome(m, JS_OFF)
    m.navigate(url)
    time.sleep(3)
    m.execute_script("window.scrollTo(0, 0)", sandbox="render")
    elements = m.execute_script(SWAP_DOM, script_args=(html,), sandbox="render")
    time.sleep(10)  # stylesheets, fonts and images
    shoot(m, os.path.join(OUT_DIR, name + "-mini.png"))
    shoot(m, os.path.join(OUT_DIR, name + "-mini-full.png"), full=True)
    height = m.execute_script("return document.documentElement.scrollHeight", sandbox="render")
    m.close()
    m.switch_to_window(bridge)
    return {"elements": elements, "height": height}


def chrome(m, script):
    m.set_context("chrome")
    try:
        return m.execute_script(script, sandbox="system")
    finally:
        m.set_context("content")


def chrome_async(m, script, args=()):
    m.set_context("chrome")
    try:
        m.timeout.script = 180
        return m.execute_async_script(script, script_args=args, sandbox="system")
    finally:
        m.set_context("content")


# A GC and a CC in every process, and a memory report of every process (plain, or after "Minimize
# memory usage"), as bench/firefox_realsites.py takes them (GC_ALL, DUMP)
GC_ALL = """const done = arguments[arguments.length - 1];
Services.obs.notifyObservers(null, "child-gc-request");
Cu.forceGC(); Cu.forceCC();
Services.obs.notifyObservers(null, "child-cc-request");
setTimeout(() => { Services.obs.notifyObservers(null, "child-gc-request"); Cu.forceGC(); done(true); }, 2000);"""
DUMP = """const [path, minimize] = arguments; const done = arguments[arguments.length - 1];
Components.classes["@mozilla.org/memory-info-dumper;1"].getService(Components.interfaces.nsIMemoryInfoDumper)
  .dumpMemoryReportsToNamedFile(path, {callback() { done(true); }}, null, false, minimize);"""


def bridge_memory(m, name, bridge_url):
    """Firefox's processes while the runtime waits on its report: see BRIDGE_MEMORY. MB."""
    import gzip, re, tempfile
    mb = lambda x: round(x / 1048576, 2)
    key = "top(" + bridge_url.replace("/", "\\")
    out = {}
    if BRIDGE_IDB_STOP:
        # the transactions idb_bridge.js keeps open spin a request after another while the runtime
        # waits (task 4898): stopped, Firefox commits them, and the bridge is measured without that
        out["idb_stopped"] = m.execute_script(
            "const S = globalThis.__mrtIdb; if (!S) return null; S.stopSpin = true; "
            "return [...S.objs.values()].filter(o => o && 'spins' in o).map(o => o.spins);", sandbox="idb", new_sandbox=False)
        time.sleep(2)
    if PHONE and BRIDGE_SERIES:
        # the process with no report in between first (a report's own work is garbage in it)
        out["quiet"] = mrt_phone.quiet_series(m)
    states = [("plain", False), ("minimized", True)]
    if BRIDGE_SERIES:
        states[1:1] = [("plain-30s", False), ("plain-60s", False)]
    t_first = time.time()
    for state, minimize in states:
        if state.startswith("plain-"):
            time.sleep(max(0, t_first + int(state[6:-1]) - time.time()))
        ds = None
        if PHONE:
            # Fenix: the report through the app (adb cannot read its files), dumpsys meminfo right after
            reps, ds = mrt_phone.memory_reports(m, f"{name}-{state}", minimize)
        else:
            chrome_async(m, GC_ALL)
            time.sleep(1)
            tmp = tempfile.mkdtemp()
            path = os.path.join(tmp, f"{name}-{state}.json.gz")
            chrome_async(m, DUMP, (path, minimize))
            reps = json.load(gzip.open(path))["reports"]
            os.remove(path)
            os.rmdir(tmp)
        procs = {}
        for r in reps:
            d = procs.setdefault(r["process"], {"windows": set(), "window": {}, "images": 0, "single": {}, "explicit": {}})
            p = r["path"]
            if p.startswith("explicit/") and r["units"] == 0:
                # explicit by its first two levels (window-objects and js-non-window three)
                parts = p.split("/")
                n = 4 if parts[1] in ("js-non-window", "window-objects") else 3
                k = "/".join(re.sub(r"\(.*", "(...)", x) for x in parts[1:n])
                d["explicit"][k] = d["explicit"].get(k, 0) + r["amount"]
            if p in ("resident-unique", "heap-allocated", "explicit/heap-unclassified"):
                d["single"][p] = r["amount"]
            if p.startswith("explicit/images/") and r["units"] == 0:
                d["images"] += r["amount"]
            if p.startswith("explicit/window-objects/top("):
                top = p.split("/")[2]
                d["windows"].add(top[:120])
                if top.startswith(key):
                    parts = p.split("/")
                    wi = next((i for i, x in enumerate(parts) if x.startswith("window(")), None)
                    zi = next((i for i, x in enumerate(parts) if x.startswith("js-zone(")), None)
                    cat = "js-zone" if zi is not None else (re.sub(r"\(.*", "", parts[wi + 1]) if wi is not None and wi + 1 < len(parts) else "other")
                    d["window"][cat] = d["window"].get(cat, 0) + r["amount"]
        rows = []
        for pname, d in procs.items():
            pid = re.search(r"pid (\d+)", pname)
            split = None
            if pid and PHONE:
                split = mrt_phone.dumpsys_anon(ds.get(pname))
            elif pid:
                try:
                    split = firefox_process_files.split(int(pid.group(1)))
                except OSError:
                    pass
            rows.append({
                "process": re.sub(r"\s*\(pid \d+\)", "", pname)[:120], "pid": int(pid.group(1)) if pid else None,
                "unique": mb(split["unique"]) if split else None, "anonymous": mb(split["anonymous"]) if split else None,
                "heap_allocated": mb(d["single"].get("heap-allocated", 0)), "images": mb(d["images"]),
                "windows": sorted(d["windows"]), "bridge": bool(d["window"]),
                "bridge_window": {k: mb(v) for k, v in sorted(d["window"].items())},
                "bridge_window_total": mb(sum(d["window"].values())),
                "explicit": {k: mb(v) for k, v in sorted(d["explicit"].items(), key=lambda kv: -kv[1]) if v >= 262144} if d["window"] else None,
            })
            if split and "dumpsys" in split:
                rows[-1]["dumpsys"] = split["dumpsys"]
            if split and "smaps" in split:
                rows[-1]["smaps"] = split["smaps"]
        out[state] = rows
    if PHONE and BRIDGE_PRESS:
        out["pressure"] = mrt_phone.pressure_point(out["quiet"]["pid"] if "quiet" in out else m.execute_script(
            "return Services.appinfo.processID;", sandbox="system"))
    return out


# A script's source: browsers load <script src> without CORS, so it is fetched from the chrome side
# (system principal), with the cookies of its own site.
SCRIPT_FETCH = """const [req] = arguments; const done = arguments[arguments.length - 1];
const t0 = Date.now();
fetch(req.url, {credentials: "include"}).then(async r => {
  const headers = [];
  r.headers.forEach((v, k) => headers.push([k, v]));
  const body = await r.text();
  return {status: r.status, statusText: r.statusText, url: r.url, redirected: r.redirected, headers, body, took: Date.now() - t0};
}).then(ok => done({ok}), e => done({err: String(e)}));"""
# On Fenix (MRT_ADB) the chrome window's own fetch fails for every URL (NetworkError, even data:), while
# a system-principal sandbox's works: the same fetch from such a sandbox, made once
SCRIPT_FETCH_PHONE = SCRIPT_FETCH.replace("const t0 = Date.now();", """const t0 = Date.now();
const sb = globalThis.__mrtFetchSandbox || (globalThis.__mrtFetchSandbox =
  Cu.Sandbox(Services.scriptSecurityManager.getSystemPrincipal(), {wantGlobalProperties: ["fetch"]}));
const fetch = Cu.evalInSandbox("fetch", sb);""", 1)


# IndexedDB in the bridge tab (SPEC decision 12): Firefox's side of the runtime's IndexedDB, with the
# value codec both sides share
IDB_SCRIPT = ("const CODEC = " + open(os.path.join(HERE, "..", "mrt-js", "glue", "clone_codec.js")).read() + ";\n"
              + open(os.path.join(HERE, "idb_bridge.js")).read())


# Web Crypto in the bridge tab (SPEC decision 21): Firefox's SubtleCrypto, with its keys cached by the
# runtime's ids; the runtime keeps every key's material
CRYPTO_SCRIPT = open(os.path.join(HERE, "crypto_bridge.js")).read()
# WebGL batches (task 23bb): the contexts and their objects live in their sandbox between calls
WEBGL_SCRIPT = open(os.path.join(HERE, "http-viewer", "webgl.js")).read() + open(os.path.join(HERE, "webgl_bridge.js")).read()


# WebSocket in the bridge tab (GAPS B70): the runtime's connections are real WebSockets of this window,
# kept in Marionette's content sandbox "ws" between calls. Each connection keeps the events Firefox
# fired at it, with their time since it was made, until the runtime asks for them. "open" waits for
# the first event (the connection made or failed) up to PENDING_MS, as a fetch waits for its answer.
WS_SCRIPT = """const [req] = arguments; const done = arguments[arguments.length - 1];
const PENDING_MS = """ + str(PENDING_MS) + """;
const S = globalThis.__mrtWs || (globalThis.__mrtWs = {next: 1, socks: new Map()});
const take = s => s.events.splice(0);
// a binary message's bytes as base64, read through a view in the page's realm (the sandbox cannot
// read the page's ArrayBuffer)
const base64 = buf => {
  const b = new window.wrappedJSObject.Uint8Array(buf);
  let bin = "";
  for (let i = 0; i < b.length; i++) bin += String.fromCharCode(b[i]);
  return window.btoa(bin);
};
(async () => {
  switch (req.call) {
    case "open": {
      let ws;
      try { ws = req.protocols === undefined ? new window.WebSocket(req.url) : new window.WebSocket(req.url, req.protocols); }
      catch (e) {
        // (Gecko's Exception for an nsresult, CSP's NS_ERROR_CONTENT_BLOCKED: its result, and the
        // nsresult's text its stringifier shows)
        if (e.result !== undefined && !(e instanceof window.DOMException))
          return {throws: {name: String(e.name), result: e.result, message: (/^\[Exception\.\.\. "(.*?)"/.exec(String(e)) || [])[1] || ""}};
        return {throws: {name: String(e.name), message: String(e.message)}};
      }
      ws.binaryType = "arraybuffer";
      const s = {ws, t0: Date.now(), events: []};
      const id = S.next++;
      S.socks.set(id, s);
      const at = () => Date.now() - s.t0;
      ws.onopen = () => s.events.push({type: "open", ms: at(), protocol: ws.protocol, extensions: ws.extensions});
      ws.onmessage = e => s.events.push(typeof e.data === "string" ? {type: "message", ms: at(), data: e.data}
                                                                 : {type: "message", ms: at(), binary: true, data: base64(e.data)});
      ws.onerror = () => s.events.push({type: "error", ms: at()});
      ws.onclose = e => s.events.push({type: "close", ms: at(), wasClean: e.wasClean, code: e.code, reason: e.reason});
      while (!s.events.length && Date.now() - s.t0 < PENDING_MS) await new Promise(r => window.setTimeout(r, 5));
      return {id, url: ws.url, events: take(s)};
    }
    case "poll": {
      const out = {};
      for (const id of req.ids) { const s = S.socks.get(id); if (s) out[id] = take(s); }
      return out;
    }
    case "send": {
      const s = S.socks.get(req.id);
      if (req.dataBase64 != null) {
        const bin = window.atob(req.dataBase64), bytes = new window.wrappedJSObject.Uint8Array(bin.length);
        for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
        s.ws.send(bytes);
      } else s.ws.send(req.data);
      return null;
    }
    case "close": {
      const s = S.socks.get(req.id);
      try {
        if (req.code === undefined) s.ws.close();
        else if (req.reason === undefined) s.ws.close(req.code);
        else s.ws.close(req.code, req.reason);
      } catch (e) { return {throws: {name: String(e.name), message: String(e.message)}}; }
      return null;
    }
  }
  throw new Error("unknown ws call " + req.call);
})().then(ok => done({ok: ok === undefined ? null : ok}), e => done({err: String(e)}));"""

def trusted_types_policies(m):
    """The Trusted Types directives of the CSP of the document Marionette is in, as Firefox parsed it
    (the chrome-only Document.cspJSON, read from a system sandbox): for each policy that has one,
    {reportOnly, require (require-trusted-types-for 'script'), names (the trusted-types values, or
    None), text}. [] when there are none."""
    try:
        data = json.loads(m.execute_script("return document.cspJSON;", sandbox="system") or "{}")
    except Exception:
        return []
    out = []
    for p in data.get("csp-policies") or []:
        require, names = p.get("require-trusted-types-for"), p.get("trusted-types")
        if require is None and names is None:
            continue
        out.append({"reportOnly": bool(p.get("report-only")), "require": bool(require) and "'script'" in require,
                    "names": names, "text": "; ".join(k + " " + " ".join(v) for k, v in p.items() if isinstance(v, list))})
    return out


def document_url(response_url, requested):
    """The URL a document loaded from `requested` has: the response's URL with the fragment.

    A fetch's Response.url never has one, but a navigation keeps the requested URL's, across
    redirects too unless the redirect gives its own (Gecko's HttpBaseChannel::
    PropagateReferenceIfNeeded). Gmail's router reads location.hash: without "#inbox" it rewrites
    the URL to /chat/u/0/ and every relative request after that goes there (GAPS B58).
    """
    from urllib.parse import urldefrag
    url = response_url or requested
    fragment = urldefrag(requested).fragment
    if fragment and "#" not in url:
        url += "#" + fragment
    return url


def op(m, req):
    """Runs one request in the bridge tab; the reply as the mini runtime reads it.

    A fetch's reply carries `ms`, its network time (SPEC decision 20): the recorded duration the replay
    proxy put in its x-mrt-recorded-ms header (dropped here), else how long the bridge tab's fetch took
    (to the end of its body; the driver's round trip left out)."""
    if req["op"] != "fetch":
        reply = run_op(m, req)
        # IndexedDB's and images' time in Firefox (idb_bridge.js, the "image" request), none in the timing of before session 70
        if req["op"] in ("idb", "image") and os.environ.get("REPLAY_TIMING") == "instant" and isinstance(reply.get("ok"), dict):
            reply["ok"]["ms"] = 0
        return reply
    reply = run_op(m, req)
    ok = reply.get("ok")
    if isinstance(ok, dict) and not ok.get("pending"):
        recorded = None
        headers = []
        for k, v in ok.get("headers") or []:
            if k.lower() == RECORDED_MS:
                recorded = v
            else:
                headers.append([k, v])
        ok["headers"] = headers
        took = ok.pop("took", 0)
        if os.environ.get("REPLAY_TIMING") == "instant":
            # every answer at once, as before session 70 (replay.py)
            recorded, took = 0, 0
        try:
            ok["ms"] = max(0, int(recorded)) if recorded is not None else took
        except ValueError:
            ok["ms"] = took
    return reply


RECORDED_MS = "x-mrt-recorded-ms"


# The API process (SPEC decision 27): the op kinds mrt-api implements, and the ones API_PROCESS sends it
API_KINDS = ("crypto", "storage", "cookie", "fetch", "ws", "env", "image", "permissions", "mediaType", "idb", "webgl")
API_BIN = os.environ.get("API_BIN", os.path.join(HERE, "..", "mrt-api", "target", "release", "mrt-api"))


def api_routed():
    """The kinds API_PROCESS names (`all`: API_KINDS); the run stops on one mrt-api does not implement."""
    value = os.environ.get("API_PROCESS", "all" if NO_FIREFOX else "")
    kinds = API_KINDS if value == "all" else tuple(k for k in value.split(",") if k)
    unknown = [k for k in kinds if k not in API_KINDS]
    if unknown:
        sys.exit(f"API_PROCESS: mrt-api does not implement {unknown}; it has {list(API_KINDS)}")
    # the API process's requests send and set its cookies: the page's jar must be the same one
    for k in ("fetch", "ws", "image"):
        if k in kinds and "cookie" not in kinds:
            sys.exit(f"API_PROCESS: {k} needs cookie too (the page's requests and document.cookie share one jar)")
    if "image" in kinds and "fetch" not in kinds:
        sys.exit("API_PROCESS: image needs fetch too (images and page requests share their network and cookie jar)")
    if "webgl" in kinds and "image" not in kinds:
        sys.exit("API_PROCESS: webgl needs image too (a texture's <img> is the image the page loaded)")
    return frozenset(kinds)


API_ROUTED = api_routed()
# API_ENV_CHECK=1: each routed env answer compared with the bridge tab's (`api_process.env_check`); it
# keeps the tab of before, so that there is one to ask
API_ENV_CHECK = os.environ.get("API_ENV_CHECK") == "1"
# Without a tab (DESIGN "Phase 6", SPEC decision 27): with the document's fetch and env in the API process,
# the run opens no bridge tab until the page makes a request only the tab serves; then a lean one
# (BRIDGE_LEAN's), as a dropped tab is made again. An interactive or rendered run, or the phone's, needs
# the tab from the start
TABLESS = "fetch" in API_ROUTED and "env" in API_ROUTED and not (INTERACTIVE or RENDER or PHONE or API_ENV_CHECK)
# env's recordings of the test profile's Firefox, one file per build (env_file)
API_ENV_DIR = os.path.join(HERE, "realsites-env")

if NO_FIREFOX and (not TABLESS or set(API_KINDS) != API_ROUTED):
    sys.exit("NO_FIREFOX=1 requires API_PROCESS=all and cannot use INTERACTIVE, RENDER, MRT_ADB or API_ENV_CHECK")


def standalone_env_file():
    """Use an existing recording; this mode must never open Firefox to make one."""
    path = os.environ.get("API_ENV")
    if not path:
        suffix = "-replay.json" if replay.MODE else ".json"
        files = os.listdir(API_ENV_DIR) if os.path.isdir(API_ENV_DIR) else []
        candidates = sorted(os.path.join(API_ENV_DIR, f) for f in files
                            if f.endswith(suffix) and (replay.MODE or not f.endswith("-replay.json")))
        path = candidates[-1] if candidates else None
    if not path:
        raise ValueError("NO_FIREFOX=1 needs an existing browser environment recording: set API_ENV=/path/to/env.json")
    data = load_env(path)
    if not data.get("entries") or len(data.get("viewport", [])) != 2:
        raise ValueError(f"NO_FIREFOX=1: invalid environment recording {path}; expected entries and viewport")
    return os.path.abspath(path)


def env_file(m, replayed):
    """The file of env's recording for the Firefox `m` drives (mrt-api's API_ENV): {firefox, viewport:
    the bridge tab's innerWidth and innerHeight, entries: by mrt-api env.rs key, what the tab's env
    answered (OP's env), without the window's inner size}. One per build, and apart for a replay's
    Firefox, whose prefs (replay.py) turn service workers off and so take their names away."""
    build = chrome(m, "return Services.appinfo.appBuildID;")
    return os.path.join(API_ENV_DIR, f"{build}{'-replay' if replayed else ''}.json")


def env_key(secure, isolated):
    """mrt-api env.rs key()"""
    return f"secure={int(bool(secure))},isolated={int(bool(isolated))}"


def load_env(path):
    try:
        return json.load(open(path))
    except (OSError, ValueError):
        return {"entries": {}}


def record_env(m, path, top):
    """env asked of the bridge tab in the window Marionette is in, kept in `path` under its kind of
    global; `top`: that window is the tab's own, whose inner size is the page's viewport. The key."""
    reply = run_op(m, {"op": "env"})
    if "err" in reply:
        raise RuntimeError("env: " + reply["err"])
    entry = reply["ok"]
    secure, isolated = m.execute_script("return [window.isSecureContext, window.crossOriginIsolated];", sandbox="bridge")
    data = load_env(path)
    data["firefox"] = os.path.basename(path)[:-len(".json")].split("-")[0]
    w, h = entry.pop("innerWidth"), entry.pop("innerHeight")
    if top:
        data["viewport"] = [w, h]
    key = env_key(secure, isolated)
    data["entries"][key] = entry
    os.makedirs(API_ENV_DIR, exist_ok=True)
    json.dump(data, open(path, "w"), indent=1, sort_keys=True)
    return key


# Firefox's CSP directives (dom/webidl/CSPDictionaries.webidl's CSP), which Document.cspJSON lists, in
# the dictionary's order (WebIDL sorts its members); a <meta> policy drops the three it cannot carry
CSP_DIRECTIVES = sorted(["default-src", "script-src", "object-src", "style-src", "img-src", "media-src", "frame-src", "font-src",
                         "connect-src", "report-uri", "frame-ancestors", "base-uri", "form-action", "referrer", "manifest-src",
                         "upgrade-insecure-requests", "child-src", "block-all-mixed-content", "sandbox", "worker-src",
                         "script-src-elem", "script-src-attr", "require-trusted-types-for", "trusted-types", "report-to"])
CSP_META_IGNORED = ("frame-ancestors", "report-uri", "sandbox")


def csp_policies(headers, html, base):
    """A document's CSP as Firefox's cspJSON has it, from its response headers ([name, value] pairs)
    and the <meta http-equiv> policies of its head: [(report_only, {directive: [values]})]. Values are
    the tokens, with the quoted keywords lowercased and report-uri's URLs made absolute against the
    document's URL `base`, as nsCSPParser keeps them."""
    from urllib.parse import urljoin
    import re
    raw = []
    for k, v in headers or []:
        k = k.lower()
        if k in ("content-security-policy", "content-security-policy-report-only"):
            raw += [(k.endswith("report-only"), p, False) for p in v.split(",")]
    head = re.split(r"<body[\s>]|</head\s*>", html or "", maxsplit=1, flags=re.I)[0]
    for tag in re.findall(r"<meta\b[^>]*>", head, flags=re.I):
        attrs = {a.lower(): (b or c or d) for a, b, c, d in re.findall(r'([\w-]+)\s*=\s*(?:"([^"]*)"|'"'"r"([^']*)'|([^\s>]+))", tag)}
        if (attrs.get("http-equiv") or "").lower() == "content-security-policy" and attrs.get("content"):
            import html as html_mod
            raw.append((False, html_mod.unescape(attrs["content"]), True))
    out = []
    for report_only, text, meta in raw:
        directives = {}
        for part in text.split(";"):
            tokens = part.split()
            if not tokens:
                continue
            name = tokens[0].lower()
            if name not in CSP_DIRECTIVES or name in directives or (meta and name in CSP_META_IGNORED):
                continue
            values = [t.lower() if t.startswith("'") and not re.match(r"'(nonce|sha256|sha384|sha512)-", t, re.I) else t for t in tokens[1:]]
            directives[name] = [urljoin(base, v) for v in values] if name == "report-uri" else values
        if directives:
            out.append((report_only, directives))
    return out


def trusted_types_of(policies):
    """trusted_types_policies()'s answer for csp_policies()"""
    out = []
    for report_only, d in policies:
        require, names = d.get("require-trusted-types-for"), d.get("trusted-types")
        if require is None and names is None:
            continue
        out.append({"reportOnly": report_only, "require": bool(require) and "'script'" in require, "names": names,
                    "text": "; ".join(k + " " + " ".join(d[k]) for k in CSP_DIRECTIVES if k in d)})
    return out


REFERRER_POLICIES = ("no-referrer", "no-referrer-when-downgrade", "origin", "origin-when-cross-origin", "same-origin",
                     "strict-origin", "strict-origin-when-cross-origin", "unsafe-url")


def doc_policies(headers, html, base):
    """What a document set that the API process applies to its requests (mrt-api's policy.rs, GAPS
    B162): for each CSP policy of csp_policies() its connect-src and default-src tokens and whether
    it has upgrade-insecure-requests; its Referrer-Policy header (ReferrerPolicyFromHeaderString:
    the last valid token); its Cross-Origin-Embedder-Policy (NS_GetCrossOriginEmbedderPolicyFromHeader:
    a token, its parameters ignored)."""
    csp = [{"reportOnly": ro, "connect": d.get("connect-src"), "default": d.get("default-src"),
            "upgrade": "upgrade-insecure-requests" in d}
           for ro, d in csp_policies(headers, html, base)
           if "connect-src" in d or "default-src" in d or "upgrade-insecure-requests" in d]
    referrer = ""
    coep = ""
    for k, v in headers or []:
        k = k.lower()
        if k == "referrer-policy":
            for token in v.split(","):
                if token.strip().lower() in REFERRER_POLICIES:
                    referrer = token.strip().lower()
        elif k == "cross-origin-embedder-policy":
            token = v.split(";")[0].strip()
            coep = token if token in ("require-corp", "credentialless") else ""
    return {"csp": csp, "referrerPolicy": referrer, "coep": coep}


def document_global(url, headers):
    """A document's origin, secure context and cross-origin isolation, from its URL and response, as
    the tab's window has them: a potentially trustworthy URL (https, a loopback host) is a secure
    context; isolation is COOP same-origin with COEP require-corp or credentialless, in one."""
    from urllib.parse import urlsplit
    u = urlsplit(url)
    host = (u.hostname or "").lower()
    loopback = host == "localhost" or host.endswith(".localhost") or host.startswith("127.") or host == "::1"
    secure = u.scheme == "https" or (u.scheme == "http" and loopback)
    h = {k.lower(): v for k, v in headers or []}
    coop = (h.get("cross-origin-opener-policy") or "").split(";")[0].strip().lower()
    coep = (h.get("cross-origin-embedder-policy") or "").split(";")[0].strip().lower()
    isolated = secure and coop == "same-origin" and coep in ("require-corp", "credentialless")
    port = f":{u.port}" if u.port and u.port != {"http": 80, "https": 443}.get(u.scheme) else ""
    return f"{u.scheme}://{host}{port}", secure, isolated


PSL_SITES = {}


def psl_site(url):
    """A URL's site by the public suffix list, as mrt-api computes it (`mrt-api --site`, the same
    form as Services.eTLD.getSite), or None: for a run with no Firefox to ask (TABLESS)."""
    if url not in PSL_SITES:
        out = subprocess.run([API_BIN, "--site", url], capture_output=True, text=True).stdout.strip()
        PSL_SITES[url] = out or None
    return PSL_SITES[url]


class ApiProcesses:
    """The API processes of one site's run (SPEC decision 27): one mrt-api per site the runtimes
    belong to, started at that site's first call. A request goes as the runtime wrote it, with the
    site and the top-level site added (every store in mrt-api is keyed by both), one JSON line; the
    reply comes back as one, as the bridge tab's would."""

    def __init__(self, name, net_env=None, keep_store=False, logs=None):
        self.name = name
        self.logs = logs or name    # its processes' logs: <logs>-api-<n>.log
        # what its requests go through and say (fetch): the replay's proxy and CA, Firefox's
        # User-Agent and Accept-Language
        self.net_env = net_env or {}
        self.procs = {}             # site -> its mrt-api
        self.started = 0            # processes started, the dropped ones included
        self.sites = []             # the sites they were started for
        self.dropped = 0            # times the processes were dropped with the bridge tab (bridgeDrop)
        self.env_check = []         # API_ENV_CHECK: env's answers that differed from the tab's
        self.calls = {}             # op kind -> calls
        self.ms = {}                # op kind -> the round trips' total, ms
        # the stores on disk (a fresh directory per run), seeded from the profile Firefox started with
        self.store = os.path.join(OUT_DIR, f"{name}-api-store")
        # (a navigation's set shares the page's store: its cookies, localStorage and databases)
        if not keep_store:
            shutil.rmtree(self.store, ignore_errors=True)
        # while fetch is the tab's: Firefox's jar copied back (syncs, rows that differed) and the
        # page's writes made in the tab too
        self.cookie_sync = {"syncs": 0, "changed": 0, "ms": 0.0, "writes_to_tab": 0}
        # one request at a time per process: an HTTP viewer's media loads (GAPS B172) call from
        # the service's threads
        self.lock = threading.RLock()

    def call(self, site, top, req):
        with self.lock:
            return self._call(site, top, req)

    def _call(self, site, top, req):
        proc = self.procs.get(site)
        if proc is None:
            self.sites.append(site)
            log = open(os.path.join(OUT_DIR, f"{self.logs}-api-{self.started}.log"), "w")
            self.started += 1
            proc = self.procs[site] = subprocess.Popen([API_BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log,
                                                       text=True, bufsize=1, env=dict(os.environ, API_STORE=self.store, API_PROFILE=PROFILE, **self.net_env))
        t = time.perf_counter()
        try:
            proc.stdin.write(json.dumps(dict(req, site=site, top=top)) + "\n")
            line = proc.stdout.readline()
        except (BrokenPipeError, OSError):
            line = ""
        kind = req["op"]
        self.calls[kind] = self.calls.get(kind, 0) + 1
        self.ms[kind] = self.ms.get(kind, 0.0) + (time.perf_counter() - t) * 1000
        if not line:
            return {"err": f"mrt-api for {site} ended (exit {proc.poll()})"}
        return json.loads(line)

    def close(self):
        for proc in self.procs.values():
            try:
                proc.stdin.close()
                proc.wait(timeout=5)
            except (OSError, subprocess.TimeoutExpired):
                proc.kill()
                proc.wait()
        self.procs = {}

    def drop(self):
        """The processes ended with the bridge tab (SPEC decision 14: the bridge is dropped before the
        pages it serves). Their stores are on disk; their WebSockets and crypto keys go, as the tab's
        did, and the next call of a site starts its process again."""
        with self.lock:
            self._drop()

    def _drop(self):
        if self.procs:
            self.dropped += 1
        for proc in self.procs.values():
            proc.kill()
            proc.wait()
        self.procs = {}

    def memory(self):
        """BRIDGE_MEMORY: each running process's unique and anonymous memory, MB, plain and after its
        `trim` op (the stores' caches, the idle connections and glibc's free pages given back, as
        memory pressure would), by site."""
        mb = lambda x: round(x / 1048576, 2)
        read = lambda pid: (lambda sp: {"unique": mb(sp["unique"]), "anonymous": mb(sp["anonymous"])})(firefox_process_files.split(pid))
        out = {}
        with self.lock:
            procs = list(self.procs.items())
        for site, proc in procs:
            try:
                plain = read(proc.pid)
                with self.lock:
                    proc.stdin.write(json.dumps({"op": "trim"}) + "\n")
                    proc.stdout.readline()
                out[site] = {"pid": proc.pid, "plain": plain, "trimmed": read(proc.pid)}
            except OSError as e:
                out[site] = {"error": repr(e)[:200]}
        return out

    def stats(self):
        out = {"kinds": sorted(API_ROUTED), "sites": sorted(set(self.sites)), "started": self.started, "calls": self.calls,
               "ms": {k: round(v, 1) for k, v in self.ms.items()}}
        if self.dropped:
            out["dropped"] = self.dropped
        if API_ENV_CHECK:
            out["env_check"] = self.env_check
        if "cookie" in API_ROUTED and "fetch" not in API_ROUTED:
            out["cookie_sync"] = dict(self.cookie_sync, ms=round(self.cookie_sync["ms"], 1))
        return out


# Firefox's cookies of a base domain, every partition (the API process keeps the ones of its own)
TAB_COOKIES = """
return Services.cookies.getCookiesWithOriginAttributes("{}", arguments[0]).map(c => ({
  name: c.name, value: c.value, host: c.host, path: c.path, expiry: c.expiry, lastAccessed: c.lastAccessed,
  creationTime: c.creationTime, isSecure: c.isSecure, isHttpOnly: c.isHttpOnly, sameSite: c.sameSite,
  schemeMap: c.schemeMap, isSession: c.isSession, partitionKey: c.originAttributes.partitionKey || ""}));"""
# the tab's ops after which Firefox's jar may hold cookies the API process has not seen: responses'
# Set-Cookie (the page's fetches and XHRs, images, a WebSocket's handshake, what the mirror loads)
TAB_NETWORK_OPS = ("fetch", "image", "ws", "mirror")


def firefox_site(m, url):
    """A URL's site as Firefox has it (nsIEffectiveTLDService.getSite: scheme and registrable
    domain, Fission's process key), or None (about:blank, a data: URL)."""
    m.set_context("chrome")
    try:
        return m.execute_script("try { return Services.eTLD.getSite(Services.io.newURI(arguments[0])); } catch (e) { return null; }",
                                script_args=(url,))
    finally:
        m.set_context("content")


def fetch_reply(reply):
    """A routed fetch's reply (SPEC decision 27) as op() gives the tab's: `took` (mrt-api's network
    time: the replay's recorded durations where it gave them) becomes `ms`."""
    ok = reply.get("ok")
    if isinstance(ok, dict) and not ok.get("pending"):
        took = ok.pop("took", 0)
        ok["ms"] = 0 if os.environ.get("REPLAY_TIMING") == "instant" else max(0, int(round(took or 0)))
    return reply


def api_net_env(m, proxy, recorded=None):
    """The environment of an mrt-api that makes the page's requests: through the replay's proxy, its CA
    trusted (bench/replay.py; mitmproxy's own CA); Firefox's User-Agent, and its Accept-Language from
    navigator.languages as Gecko writes it (rust_prepare_accept_languages: q=0.9, 0.8, ..., at least 0.1),
    asked of the tab or taken from env's recording (`recorded`, an entry of env_file)."""
    if recorded:
        ua, langs = recorded["navigator"]["userAgent"], recorded["navigator"]["languages"]
    else:
        ua, langs = m.execute_script("return [navigator.userAgent, Array.from(navigator.languages)];", sandbox="bridge")
    accept = ",".join(l + (f";q=0.{max(10 - min(10, i), 1)}" if i else "") for i, l in enumerate(langs))
    env = {"API_UA": ua, "API_ACCEPT_LANGUAGE": accept}
    if proxy:
        env["API_PROXY"] = f"http://127.0.0.1:{proxy.port}"
        env["API_CA"] = os.path.expanduser("~/.mitmproxy/mitmproxy-ca-cert.pem")
    return env


def run_op(m, req):
    try:
        if req["op"] == "fetch" and req.get("script"):
            m.set_context("chrome")
            try:
                return m.execute_async_script(SCRIPT_FETCH_PHONE if PHONE else SCRIPT_FETCH, script_args=(req,), sandbox="system")
            finally:
                m.set_context("content")
        if req["op"] == "ws":
            # the connections live in their sandbox between calls, as IndexedDB's objects
            return m.execute_async_script(WS_SCRIPT, script_args=(req,), sandbox="ws", new_sandbox=False)
        if req["op"] == "crypto":
            # Firefox's keys are kept in their sandbox between calls (a cache: the runtime has the material)
            return m.execute_async_script(CRYPTO_SCRIPT, script_args=(req,), sandbox="crypto", new_sandbox=False)
        if req["op"] == "webgl":
            return m.execute_async_script(WEBGL_SCRIPT, script_args=(req,), sandbox="webgl", new_sandbox=False)
        if req["op"] == "idb":
            # IndexedDB keeps Firefox's objects between calls: its own sandbox, never renewed
            return m.execute_async_script(IDB_SCRIPT, script_args=(req,), sandbox="idb", new_sandbox=False)
        # BRIDGE_SANDBOX=reuse: one sandbox for the whole run, as a bridge that is not a test harness would keep
        return m.execute_async_script(OP, script_args=(req,), sandbox="bridge", new_sandbox=BRIDGE_SANDBOX != "reuse")
    except errors.MarionetteException as e:
        return {"err": str(e).splitlines()[0][:300]}


class Runtime:
    """A mini runtime the driver serves: the page's, or a cross-origin frame's (SPEC decision 15)."""

    def __init__(self, proc, frame_id, origin, fetches, parent):
        self.proc = proc
        self.id = frame_id          # None for the page
        self.origin = origin
        self.fetches = fetches
        self.parent = parent
        self.waiting = None         # its last step request, while it waits for its turn
        self.queue = []             # messages for it: tasks at their senders' times
        self.time = 0.0             # its virtual time at its last turn
        self.report = None
        self.url = None
        self.done = False
        self.error = None
        self.steps = 0              # step requests it sent
        self.since = 0              # when it began to wait (the driver's count of steps), for ties
        self.turns = 0              # turns it was given
        self.loaded = False         # a frame runtime: its load event has fired
        self.site = None            # its site (Firefox's): the frames of that site run in it
        self.frames = []            # the frames it runs: (frame id, URL), its first window's first
        self.viewport = None        # its first window's inner size, [width, height] (env in the API process)
        self.csp = []               # its document's Trusted Types directives (env in the API process)
        self.doc = None             # its document's policies over its requests (doc_policies, the API process's fetch)
        # an HTTP viewer navigation's runtime (Frames.navigate): its own API processes and top-level
        # site, what its env reply adds (`navigation`: referrer, history, sessionStorage, type), and
        # the first wait it sent, answered once it is the page
        self.api = None
        self.top = None
        self.navigation = None
        self.first_wait = None

    def next_time(self):
        times = [m["time"] for m in self.queue]
        if self.waiting is not None and self.waiting.get("next") is not None:
            times.append(self.waiting["next"])
        return min(times) if times else None

    def reply(self, value):
        self.proc.stdin.write(json.dumps(value) + "\n")
        self.proc.stdin.flush()


class Frames:
    """The runtimes of a page and of its cross-origin frames, and the event loop they share
    (DESIGN, "Cross-origin frames in runtimes of their own"; SPEC decision 16). A frame runtime's
    requests are served in an <iframe> of its URL in the bridge tab, so that Firefox partitions its
    cookies and storage as a third-party frame's; one runtime runs at a time, the next one being the
    one with the earliest task or message, by virtual time."""

    def __init__(self, m, name, page, global_args, page_url=None):
        self.m = m
        self.name = name
        self.page = page
        self.page_url = page_url
        self.runtimes = [page]
        self.next_frame = 1
        # by the driver's frame id: the runtime that runs the frame, the one that embeds it, and the
        # origins of the frame and of the window that opened it (SPEC decision 19: a runtime runs
        # every frame of its site, so a frame is not a runtime)
        self.frame_host = {}
        self.frame_parent = {}
        self.frame_origin = {}
        self.frame_opener_origin = {}
        self.frame_loaded = set()
        self.context = None
        self.contexts = []          # the frame windows of another origin in a runtime: their URLs
        self.refused = []           # frames Firefox did not load (tracking protection)
        self.serve = None
        # how a frame's document is fetched: in the bridge tab, or by the API process (API_PROCESS fetch)
        self.doc_fetch = lambda req: op(self.m, req)
        self.idle_since = None      # interactive: when the page first waited, and its tab
        self.bridge_handle = None
        self.stop_checked = 0.0     # interactive: when the last wait looked for a stop
        # interactive: the owner's input taken from the tab (INPUT_CAPTURE) and given to the page
        self.input = {"events": 0, "by_type": {}, "errors": 0}
        # interactive: what the tab's media elements did (MEDIA_SETUP), for the page's next wait
        self.media = []
        self.hidden = False         # interactive: the bridge tab is hidden (the runtime hibernates)
        self.actions = None
        self.finishing = False
        self.seq = 0
        # the bridge tab (SPEC decision 14; DESIGN "Bridge recovery, as tested"): its generation, which
        # every reply carries; dropped at the page's request (bridgeDrop) and made again, with its
        # frames, when a request needs it (setup_tab: the tab's first setup, done again)
        self.gen = 1
        self.dropped = False
        # interactive: the owner closed Firefox's window, and Firefox quit with it; the page is
        # measured without a tab (serve_without_firefox)
        self.firefox_quit = False
        self.setup_tab = None
        self.frame_url = {}         # frame id -> URL of its <iframe>, for a new tab
        self.context_loaded = set()  # the frame contexts Firefox loaded
        self.recovery = []          # what happened to the bridge tab: drops and rebuilds
        self.tabless = False         # the run began with no bridge tab (TABLESS)
        self.tab_made = None        # TABLESS: when the first tab was made, and for which request
        self.unsupported_ops = {}   # NO_FIREFOX: requests needing services outside mrt-api
        self.viewer = None         # optional HTTP transport; runtime outlives viewer connections
        # an HTTP viewer navigation (DESIGN "Top-level navigation in the HTTP viewer"): run()'s
        # handler of the op, the destination's runtime once it is ready (until the old page has
        # unloaded), and run()'s switch to it
        self.navigate = None
        self.pending_page = None
        self.unloaded = False
        self.on_switch = None
        self.site_of = None         # a URL's site (run(): Firefox's or the public suffix list's)
        # Firefox's cookie jar changed since the API process last copied it (Phase 6.2, until fetch
        # moves): the tab's first load set cookies too
        self.cookies_changed = True

    def use(self, rt, ctx=None):
        """Marionette in the context of a request of runtime `rt`: the bridge tab, or the <iframe> in
        it of the runtime's frame, or of the frame window `ctx` of another origin in the runtime
        (SPEC decision 19: a window keeps its own cookies and storage partition)."""
        want = ctx if ctx is not None else None if rt.id is None else f"mrt-frame-{rt.id}"
        if want == self.context:
            return
        self.m.switch_to_frame()
        if want:
            self.m.switch_to_frame(self.m.find_element("id", want))
        self.context = want

    def open(self, parent, req):
        """frameOpen: an <iframe> of the URL in the bridge tab, its document fetched there, and the frame
        run where Firefox runs it (SPEC decision 19): in the runtime of its site if there is one, as a
        root window opened at that runtime's next turn; else in a new realsite of its own, run to its
        first step (its load is a task of the loop)."""
        self.ensure_bridge(req)
        self.cookies_changed = True
        fid = self.next_frame
        self.next_frame += 1
        self.use(self.page)
        w, h = int(req.get("width", 300)), int(req.get("height", 150))
        self.m.execute_script("const f = document.createElement('iframe'); f.id = arguments[0]; f.src = arguments[1]; "
                              "f.width = arguments[2]; f.height = arguments[3]; document.body.appendChild(f);",
                              script_args=(f"mrt-frame-{fid}", req["url"], w, h), sandbox="bridge")
        self.use(parent, f"mrt-frame-{fid}")
        self.frame_parent[fid] = parent
        self.frame_opener_origin[fid] = req.get("origin") or parent.origin
        # Firefox refuses some frames before any request: tracking protection blocks a third-party
        # frame on its lists (CNN's m.stripe.network, a known fingerprinter). The frame stays at its
        # initial about:blank and its element gets no load event; the runtime's frame does the same
        if not self.frame_navigated(FRAME_WAIT):
            self.refused.append(req["url"])
            return {"ok": {"id": fid, "loaded": False}}
        self.frame_url[fid] = req["url"]
        # a frame of the page's site opened from another site's frame becomes a root window of the
        # page's runtime with that frame above it: its document loads in the partition with the
        # foreign-ancestor bit, as its requests will (bridge.js foreignAncestor)
        top_site = self.page.site or (self.site(self.page_url) if self.page_url else None)
        ancestor = (bool(req.get("ancestor")) or parent is not self.page) and top_site is not None and self.site(req["url"]) == top_site
        doc = self.doc_fetch({"op": "fetch", "url": req["url"], "method": "GET", "cache": "reload",
                          "headers": [["accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"]], "body": None,
                          **({"ancestor": True} if ancestor else {})})
        if "err" in doc:
            return {"err": "frame document: " + doc["err"]}
        origin, secure, isolated = self.m.execute_script("return [window.origin, window.isSecureContext, window.crossOriginIsolated];", sandbox="bridge")
        self.frame_origin[fid] = origin
        url = document_url(doc["ok"]["url"], req["url"])
        site = self.site(url)
        if self.page.site is None and self.page_url:
            self.page.site = self.site(self.page_url)
        host = next((r for r in self.runtimes if not r.done and r.site is not None and r.site == site), None)
        if host is not None:
            # a root window in the site's runtime, opened at its next turn from the opener's time
            host.frames.append((fid, url))
            self.frame_host[fid] = host
            host.queue.append({"kind": "open", "frame": fid, "url": url, "html": doc["ok"]["body"] or "", "origin": origin,
                               "secure": bool(secure), "isolated": bool(isolated), "width": w, "height": h,
                               "time": float(req.get("time") or parent.time)})
            return {"ok": {"id": fid}}
        fetches = open(os.path.join(OUT_DIR, f"{self.name}-frame{fid}-requests.jsonl"), "w")
        rt = Runtime(None, fid, origin, fetches, parent)
        rt.url = req["url"]
        rt.site = site
        rt.frames.append((fid, url))
        rt.viewport = [w, h]
        rt.csp = trusted_types_of(csp_policies(doc["ok"].get("headers"), doc["ok"]["body"], url))
        rt.doc = doc_policies(doc["ok"].get("headers"), doc["ok"]["body"], url)
        self.runtimes.append(rt)
        self.frame_host[fid] = rt
        html_file = os.path.join(OUT_DIR, f"{self.name}-frame{fid}.html")
        open(html_file, "w").write(doc["ok"]["body"] or "")
        args = [REALSITE, html_file, url, "--origin", origin, "--secure-context", str(int(bool(secure))),
                "--cross-origin-isolated", str(int(bool(isolated))), "--viewport", str(w), str(h), "--frame", str(req.get("time") or 0)]
        args += shlex.split(os.environ.get("REALSITE_ARGS", ""))
        log = open(os.path.join(OUT_DIR, f"{self.name}-frame{fid}.log"), "w")
        rt.proc = start_realsite(args, log)
        # its startup (bridge.js's first requests) until it waits for its first turn
        self.run_until_wait(rt)
        if rt.done:
            rt.error = rt.error or "ended before its first turn"
            return {"ok": {"id": fid, "loaded": False}}
        return {"ok": {"id": fid}}

    def open_context(self, rt, req):
        """frameContext: a frame of the runtime's site but another origin is a window in the runtime
        (SPEC decision 19); its requests are served in an <iframe> of its URL in the bridge tab, from
        which its document is fetched. Not loaded by Firefox (tracking protection): loaded false."""
        self.ensure_bridge(req)
        self.cookies_changed = True
        cid = len(self.contexts)
        self.contexts.append(req["url"])
        ctx = f"mrt-ctx-{cid}"
        self.use(self.page)
        w, h = int(req.get("width", 300)), int(req.get("height", 150))
        self.m.execute_script("const f = document.createElement('iframe'); f.id = arguments[0]; f.src = arguments[1]; "
                              "f.width = arguments[2]; f.height = arguments[3]; document.body.appendChild(f);",
                              script_args=(ctx, req["url"], w, h), sandbox="bridge")
        self.use(rt, ctx)
        if not self.frame_navigated(FRAME_WAIT):
            return {"ok": {"loaded": False}}
        self.context_loaded.add(cid)
        doc = self.doc_fetch({"op": "fetch", "url": req["url"], "method": "GET", "cache": "reload",
                          "headers": [["accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"]], "body": None})
        if "err" in doc:
            return {"ok": {"loaded": False, "error": doc["err"]}}
        origin, secure, isolated = self.m.execute_script("return [window.origin, window.isSecureContext, window.crossOriginIsolated];", sandbox="bridge")
        return {"ok": {"ctx": ctx, "url": document_url(doc["ok"]["url"], req["url"]), "body": doc["ok"]["body"] or "",
                       "origin": origin, "secure": bool(secure), "isolated": bool(isolated)}}

    def tab_pid(self):
        """The pid of the content process of the bridge tab (the selected one)."""
        return chrome(self.m, "return gBrowser.selectedBrowser.browsingContext.currentWindowGlobal?.osPid ?? null;")

    def drop(self, req):
        """bridgeDrop: the bridge tab closed, as under memory pressure; nothing is made until a request
        needs a tab (ensure_bridge). Marionette keeps a blank tab to be in meanwhile."""
        self.m.switch_to_frame()
        self.context = None
        old_pid = self.tab_pid()
        old = self.m.current_window_handle
        new = self.m.open(type="tab", focus=True)["handle"]
        self.m.switch_to_window(old)
        self.m.close()
        self.m.switch_to_window(new)
        self.gen += 1
        self.dropped = True
        # whether the tab's content process ended with it (Fission keeps none for a site no tab has)
        end = time.time() + 10
        while os.path.exists(f"/proc/{old_pid}") and time.time() < end:
            time.sleep(0.2)
        self.recovery.append({"drop": req.get("t"), "gen": self.gen, "old_pid": old_pid,
                              "old_process_ended": not os.path.exists(f"/proc/{old_pid}") if old_pid else None})

    def ensure_bridge(self, req):
        """A request needs the bridge tab: made again if it was dropped, its frames' <iframe>s
        included, under the same ids."""
        if NO_FIREFOX:
            raise RuntimeError(f"NO_FIREFOX=1: {req['op']} requires the Firefox bridge")
        if not self.dropped:
            return
        t0 = time.time()
        self.setup_tab()
        self.cookies_changed = True
        self.context = None
        made = [(f"mrt-frame-{fid}", url) for fid, url in self.frame_url.items()]
        made += [(f"mrt-ctx-{cid}", self.contexts[cid]) for cid in sorted(self.context_loaded)]
        loaded = 0
        for element_id, url in made:
            self.m.switch_to_frame()
            self.m.execute_script("const f = document.createElement('iframe'); f.id = arguments[0]; f.src = arguments[1]; document.body.appendChild(f);",
                                  script_args=(element_id, url), sandbox="bridge")
            self.m.switch_to_frame(self.m.find_element("id", element_id))
            loaded += self.frame_navigated(FRAME_WAIT)
        self.m.switch_to_frame()
        self.dropped = False
        made_by = {"by": {k: req.get(k) for k in ("op", "call", "url", "area") if req.get(k) is not None}, "pid": self.tab_pid(), "wall_s": round(time.time() - t0, 1)}
        if self.tabless and self.tab_made is None:
            self.tab_made = {"made": req.get("t"), "gen": self.gen, **made_by}
        else:
            self.recovery.append({"rebuild": req.get("t"), "gen": self.gen, **made_by, "frames": len(made), "frames_loaded": loaded})

    def site(self, url):
        return self.site_of(url)

    def frame_navigated(self, wait):
        """Whether the frame Marionette is in has left its initial about:blank within `wait` seconds."""
        end = time.time() + wait
        while True:
            try:
                if self.m.execute_script("return location.href;", sandbox="bridge") != "about:blank":
                    return True
            except errors.MarionetteException:
                pass  # the frame's document is being replaced
            if time.time() > end:
                return False
            time.sleep(0.25)

    def load_until_ready(self, rt, limit=60):
        """Serve a navigation's new runtime until its first wait (parsed, DOMContentLoaded fired, its
        event loop started), which stays unanswered until it is the page. The old page meanwhile waits
        for its `navigate` reply. Returns None, or why it did not get there: it exited, or `limit`
        seconds passed (then it is killed)."""
        late = []
        timer = threading.Timer(limit, lambda: (late.append(True), rt.proc.kill()))
        timer.start()
        try:
            while True:
                line = rt.proc.stdout.readline()
                if not line:
                    rt.done = True
                    return f"did not load in {limit} s" if late else f"its runtime exited (code {rt.proc.wait()}) while loading"
                req = json.loads(line)
                if req["op"] == "wait":
                    rt.first_wait = req
                    return None
                if req["op"] == "step":
                    # (one runtime: its turn at once)
                    rt.reply({"ok": {"go": True, "messages": []}})
                    continue
                rt.reply(self.handle(rt, req))
        finally:
            timer.cancel()

    def switch_page(self):
        """The navigation's commit: the old page has unloaded (or ended); run() kills it and makes the
        new runtime the page, whose first wait is answered now. Returns it."""
        new, self.pending_page, self.unloaded = self.pending_page, None, False
        old = self.page
        self.page = new
        self.runtimes = [new]
        self.on_switch(old, new)
        new.reply(self.wait(new.first_wait))
        new.first_wait = None
        return new

    def run_until_wait(self, rt):
        """Serve `rt` until it waits for a turn (a step) or exits."""
        while True:
            line = rt.proc.stdout.readline()
            if not line:
                rt.done = True
                return
            req = json.loads(line)
            if req["op"] == "step":
                rt.waiting = req
                self.seq += 1
                rt.since = self.seq
                return
            rt.reply(self.handle(rt, req))

    def handle(self, rt, req):
        """A request other than a step."""
        if NO_FIREFOX and req["op"] not in API_ROUTED and req["op"] not in (
                "site", "report", "bridgeDrop", "frameMessage", "frameLoaded", "frameNavigate", "navigate", "navigateDone") and not (self.viewer and req["op"] in ("mirror", "mirrorCheck", "webgl", "mediaResource")):
            kind = req["op"]
            self.unsupported_ops[kind] = self.unsupported_ops.get(kind, 0) + 1
            return {"err": f"NO_FIREFOX=1: {kind} requires the Firefox bridge"}
        if req["op"] == "frameOpen":
            return self.open(rt, req)
        if req["op"] == "frameContext":
            return self.open_context(rt, req)
        if req["op"] == "site":
            return {"ok": self.site(req["url"])}
        if req["op"] == "frameMessage":
            sender = req.get("frame") if req.get("frame") is not None else rt.id
            origin = req.get("origin") or rt.origin
            time_ = float(req.get("time") or rt.time)
            if req["to"] == "parent":
                # from a frame to its embedder: the window that opened it gets it (takeStep's owners)
                target, target_origin = self.frame_parent.get(sender), self.frame_opener_origin.get(sender)
                message = {"kind": "message", "from": sender, "origin": origin, "data": req["data"], "time": time_}
            else:
                # from an embedder to its frame, wherever the frame runs
                target, target_origin = self.frame_host.get(req["to"]), self.frame_origin.get(req["to"])
                message = {"kind": "message", "from": "parent", "to": req["to"], "origin": origin, "data": req["data"], "time": time_}
            # postMessage's targetOrigin: "*", or the target's origin; else dropped (Window::PostMessage)
            if target and not target.done and req.get("targetOrigin") in ("*", target_origin):
                target.queue.append(message)
            return {"ok": None}
        if req["op"] == "frameLoaded":
            fid = req.get("frame") if req.get("frame") is not None else rt.id
            self.frame_loaded.add(fid)
            if fid == rt.id:
                rt.loaded = True
            parent = self.frame_parent.get(fid)
            if parent and not parent.done:
                parent.queue.append({"kind": "load", "from": fid, "time": rt.time})
            return {"ok": None}
        if req["op"] == "frameNavigate":
            return {"ok": None}
        if req["op"] == "navigate":
            # a refusal is only recorded; without the HTTP viewer the bridge tab keeps the document
            if self.navigate is None:
                return {"ok": None} if req.get("refused") else {"err": "navigation needs the HTTP viewer (VIEWER_HTTP); the bridge tab keeps the document (GAPS B151)"}
            return self.navigate(rt, req)
        if req["op"] == "navigateDone":
            # the old page has unloaded: drive() switches to the new one
            self.unloaded = True
            return {"ok": None}
        if self.firefox_quit:
            return self.serve_without_firefox(rt, req)
        try:
            return self.serve(rt, req)
        except Exception:
            if not (INTERACTIVE and self.firefox_ended()):
                raise
            # the owner closed the window: the page stops at its next wait and is measured
            print(f"  interactive: Firefox quit (its window closed) at a {req['op']} request; measuring without it", flush=True)
            self.firefox_quit = self.dropped = True
            return self.serve_without_firefox(rt, req)

    def firefox_ended(self):
        """Whether the bridge Firefox's process has exited (the owner closed its last window). Called
        after a Marionette call failed: a Firefox that is shutting down gets up to 10 s to exit."""
        end = time.time() + 10
        while True:
            try:
                if not self.m.instance.runner.is_running():
                    return True
            except Exception:
                return False
            if time.time() >= end:
                return False
            time.sleep(0.2)

    def serve_without_firefox(self, rt, req):
        """A request after Firefox quit: the report, the mirror check and the API process's kinds are
        served as with a dropped tab; a mirror flush is dropped; what needs the tab gets an error."""
        if req["op"] == "mirror":
            return {"ok": None}
        if req["op"] in ("report", "mirrorCheck") or req["op"] in API_ROUTED:
            try:
                return self.serve(rt, req)
            except Exception as e:
                return {"err": repr(e)[:300]}
        return {"err": "Firefox quit: no bridge tab"}

    def turn(self):
        """Every runtime waits: the one with the earliest task or message runs one task; or, past the
        limit everywhere, all are told to finish."""
        live = [r for r in self.runtimes if not r.done and r.waiting is not None]
        if not live:
            return None
        limit = max(float(r.waiting.get("limit") or 15000) for r in live)
        best = None
        for r in live:
            t = r.next_time()
            # at equal times, the one that has waited longest (a loop of tasks at one time must not
            # keep the other runtimes from theirs)
            if t is not None and t <= limit and (best is None or (t, r.since) < (best[0], best[1].since)):
                best = (t, r)
        if best is None or self.finishing:
            self.finishing = True
            for r in live:
                r.waiting = None
                r.reply({"ok": {"finish": True}})
            return "finish"
        t, r = best
        messages, r.queue = r.queue, []
        r.waiting, r.time = None, t
        r.turns += 1
        r.reply({"ok": {"go": True, "messages": messages}})
        return r

    def drive(self, t0):
        """Serve the page's runtime, and its frames' once it has opened any, until the page exits."""
        active = self.page
        while True:
            if not INTERACTIVE and not self.viewer and time.time() - t0 > WALL_LIMIT:
                for r in self.runtimes:
                    if r.proc and not r.done:
                        r.proc.kill()
                return
            if active.done:
                return
            line = active.proc.stdout.readline()
            if not line:
                active.done = True
                if active is self.page and self.pending_page is not None:
                    # the old page ended while unloading: the navigation commits all the same
                    active = self.switch_page()
                    continue
                if active is self.page:
                    return
                # the frames it ran that had not loaded no longer block their embedders' loads (Gecko
                # unblocks the owner document when a frame's load ends in any way)
                for fid, _ in active.frames:
                    parent = self.frame_parent.get(fid)
                    if fid not in self.frame_loaded and parent and not parent.done:
                        parent.queue.append({"kind": "gone", "from": fid, "time": active.time})
                # a frame runtime ended: the turn goes on among those left (the page waits for one)
                nxt = self.turn() if not self.finishing else "finish"
                active = (self.next_after_finish() or self.page) if nxt in (None, "finish") else nxt
                continue
            req = json.loads(line)
            if req["op"] == "wait":
                active.reply(self.wait(req))
                continue
            if req["op"] == "step":
                active.steps += 1
                active.waiting = req
                self.seq += 1
                active.since = self.seq
                if self.finishing:
                    active.waiting = None
                    active.reply({"ok": {"finish": True}})
                    continue
                nxt = self.turn()
                if nxt == "finish":
                    # each one runs to its report: the frames first, the page last
                    active = self.next_after_finish() or self.page
                elif nxt is not None:
                    active = nxt
                continue
            active.reply(self.handle(active, req))
            if self.unloaded and self.pending_page is not None and active is self.page:
                active = self.switch_page()

    def take_input(self):
        """The owner's input caught in the bridge tab since the last call (INPUT_CAPTURE)."""
        try:
            events = self.m.execute_script(INPUT_TAKE, sandbox="mirror", new_sandbox=False)
        except Exception as e:
            self.input["errors"] += 1
            self.input["last_error"] = repr(e)[:300]
            return []
        # the tab hidden or shown (INPUT_CAPTURE's visibilitychange): the wait's `hidden`, not an event
        for ev in events:
            if ev["type"] == "visibilitychange":
                self.hidden = ev["hidden"]
                self.input.setdefault("visibility", []).append({"hidden": ev["hidden"], "at": ev.get("at")})
        events = [ev for ev in events if ev["type"] != "visibilitychange"]
        # the media elements' events and answers: the wait's `media`, not input
        media = [ev for ev in events if ev["type"] == "media"]
        if media:
            self.media += media
            by = self.input.setdefault("media", {})
            for ev in media:
                key = ev["k"] + (" " + ev["ev"] if ev.get("ev") else "")
                by[key] = by.get(key, 0) + 1
                # the first appends' and removes' ends, and every one that failed
                if ev["k"] == "sb" and (ev.get("error") or len(self.input.setdefault("media_sb", [])) < 12):
                    if len(self.input.setdefault("media_sb", [])) < 40:
                        self.input["media_sb"].append({k: ev.get(k) for k in ("sb", "n", "buffered", "dur", "error")})
            events = [ev for ev in events if ev["type"] != "media"]
        self.input["events"] += len(events)
        for ev in events:
            self.input["by_type"][ev["type"]] = self.input["by_type"].get(ev["type"], 0) + 1
            # the owner's clicks and Enter keys, with the tab's time: click_times() measures when the
            # tab showed what each did
            if ev["type"] == "click" or (ev["type"] == "keydown" and ev["fields"].get("key") == "Enter"):
                self.input.setdefault("acts", []).append({"type": ev["type"], "at": ev.get("at"), "taken": round(time.time() * 1000)})
        return events

    def wait(self, req):
        """An interactive page waits (realsite --interactive): answer with the owner's input as soon
        as the tab has some (looked for every 30 ms), else when the page's next timer is due, or
        within 250 ms to look for a stop: the bridge tab closed, or INTERACTIVE_S seconds since the
        page first went idle. A busy page's wait (`busy`) is answered at once with what input there
        is. The first wait starts catching input in the tab, and does INPUT_ACTIONS there."""
        if self.viewer:
            return self.viewer.wait(req)
        if self.idle_since is None:
            self.idle_since = time.time()
            self.bridge_handle = self.m.current_window_handle
            print(f"  interactive: {self.name} loaded; close its tab to measure it and stop", flush=True)
            try:
                self.m.execute_script(INPUT_CAPTURE, sandbox="mirror", new_sandbox=False)
            except Exception as e:
                self.input["errors"] += 1
                self.input["last_error"] = repr(e)[:300]
            if INPUT_ACTIONS:
                import input_actions
                self.actions = input_actions.Steps(json.load(open(INPUT_ACTIONS)), self.on_mark)
        if self.firefox_quit:
            return {"ok": {"stop": True}}
        if self.actions and not self.actions.finished():
            try:
                self.actions.advance(self.m)
            except Exception as e:
                self.actions.failed = repr(e)[:300]
            self.input["actions"] = {"done": self.actions.done, "of": len(self.actions.steps), "failed": self.actions.failed}
        if req.get("busy") and time.time() - self.stop_checked < 0.25:
            events = self.take_input()
            return {"ok": {"events": events, "hidden": self.hidden, "media": self.take_media()}}
        self.stop_checked = time.time()
        stop = (INTERACTIVE_S is not None and time.time() - self.idle_since > INTERACTIVE_S)
        events = []
        if not stop:
            stop = not self.bridge_open()
        if not stop:
            due = 0.25 if req.get("until") is None else (req["until"] - req["now"]) / 1000
            end = time.time() + min(max(0.0, due), 0.25)
            while True:
                events = self.take_input()
                if events or self.media or time.time() >= end:
                    break
                time.sleep(min(0.03, max(0.0, end - time.time())))
        if stop and not self.dropped and self.bridge_open():
            # the report's requests are served there
            try:
                self.m.switch_to_window(self.bridge_handle)
            except Exception:
                self.bridge_open()
        if os.environ.get("INTERACTIVE_DEBUG"):
            print(f"  wait {req} stop={stop} dropped={self.dropped}", flush=True)
        return {"ok": {"stop": True}} if stop else {"ok": {"events": events, "hidden": self.hidden, "media": self.take_media()}}

    def bridge_open(self):
        """Whether the bridge tab is still open; if not, it counts as dropped, and when Firefox has quit
        with the owner's window, as quit too (the report needs no tab)."""
        try:
            if self.bridge_handle in self.m.window_handles:
                return True
        except Exception:
            self.firefox_quit = self.firefox_ended()
        self.dropped = True
        return False

    def take_media(self):
        media, self.media = self.media, []
        return media

    def on_mark(self, label):
        """An INPUT_ACTIONS mark: the page runtime's unique memory and CPU time, the bridge Firefox's
        memory (proc_uss), and a screenshot of the tab as realsites-mini/<label>-mirror.png."""
        import proc_uss
        pid = self.page.proc.pid
        mark = {"t": round(time.time() - self.idle_since, 1), "runtime_uss": proc_uss.uss_mb(pid), "runtime_cpu_s": proc_uss.cpu_s(pid),
                "firefox": proc_uss.firefox_mb(self.m.process_id), "url": self.m.get_url()}
        self.input.setdefault("marks", {})[label] = mark
        # the tab's media elements then (task 753cc): each one's copy id, whether it is in the document,
        # and its state
        try:
            mark["media"] = self.m.execute_script(MEDIA_MARK, sandbox="mirror", new_sandbox=False)
        except Exception as e:
            mark["media"] = repr(e)[:200]
        try:
            shoot(self.m, os.path.join(OUT_DIR, f"{label}-mirror.png"))
        except Exception as e:
            mark["shot_error"] = repr(e)[:200]
        print(f"  mark {label}: {mark}", flush=True)

    def close(self):
        """End the frame runtimes still running when the page's is done: one left alive would share
        the binaries' pages with the next site's runtime and lower its unique memory."""
        for r in self.runtimes[1:]:
            if r.proc and r.proc.poll() is None:
                r.proc.kill()
                r.proc.wait()
            r.done = True
            # its request list: the script ends with os._exit, which would leave it unwritten
            r.fetches.close()

    def next_after_finish(self):
        for r in self.runtimes[1:]:
            if not r.done:
                return r
        return None

    def frame_reports(self):
        """One entry per frame runtime (a process: its site's frames), and the frames Firefox refused."""
        out = [{"id": r.id, "url": r.url, "origin": r.origin, "site": r.site, "frames": [u for _, u in r.frames],
                "steps": r.steps, "turns": r.turns, **({"report": r.report} if r.report else {"error": r.error or "no report"})}
               for r in self.runtimes[1:]]
        return out + [{"url": u, "error": "not loaded by Firefox: its frame in the bridge tab stayed at about:blank"} for u in self.refused]

    def loop_stats(self):
        """The merged loop: steps and turns of the page's runtime, and whether it ended by finishing."""
        return {"page_steps": self.page.steps, "page_turns": self.page.turns, "finished": self.finishing}


def start_realsite(args, log, env=None):
    """realsite with `args`, its stderr to `log`: here, or on the phone (MRT_ADB)."""
    if PHONE:
        extra = {k: v for k, v in (env or {}).items() if k.startswith("MRT_") and k not in os.environ}
        # MRT_PAGEOUT for the page's runtime only (env given): a frame's runtime ends before the page's report
        extra.update({k: v for k, v in os.environ.items() if k.startswith("MRT_") and k != "MRT_ADB" and (env or k != "MRT_PAGEOUT")})
        return mrt_phone.popen(args, log, extra)
    return subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True, bufsize=1, env=env)


def run(name, url):
    os.makedirs(OUT_DIR, exist_ok=True)
    proxy = replay.start(name, "mini")
    m = api = viewer_service = None
    proc = None
    if PHONE:
        m = mrt_phone.bridge(proxy)
    elif not NO_FIREFOX:
        # MARIONETTE_PORT: another port than 2828, for a run beside another harness's Firefox
        m = Marionette(bin=BIN, prefs={**PREFS, **(proxy.prefs if proxy else {})}, profile=PROFILE, port=int(os.environ.get("MARIONETTE_PORT", "2828")))
        m.start_session(proxy.capabilities if proxy else None)
    t0 = time.time()
    try:
        if m is not None:
            m.set_context("content")
            if not PHONE:
                m.set_window_rect(width=VIEW_W + 40, height=VIEW_H + 120)
            m.timeout.page_load = 120
            m.timeout.script = 90
        # one visit with JS on first, as a user's: it passes bot checks that need script (Amazon's
        # interstitial reloads the page after a JS challenge) and sets their cookies. Not in replay:
        # the recording already passed them, and the visit would use up recorded answers
        # the bridge tab goes to where that visit ended: a bot check's page (Reddit's JS challenge,
        # served at the site's URL with CSP default-src 'none') refuses the bridge's fetches, and with
        # JS off it never moves on to the real page. In replay, where Firefox's recorded run ended
        page_url = url
        if not NO_FIREFOX and replay.MODE != "replay":
            m.navigate(url)
            time.sleep(10)
            page_url = m.get_url()
        elif replay.MODE == "replay":
            recorded = os.path.join(HERE, "firefox_realsites_replay.json")
            if os.path.exists(recorded):
                page_url = json.load(open(recorded)).get(name, {}).get("final_url") or url
        reference = None
        if RENDER:
            if replay.MODE == "replay":
                m.navigate(url)
                time.sleep(10)
            # the real page as Firefox renders it, at the top, for the comparison with the mini DOM
            m.execute_script("window.scrollTo(0, 0)")
            time.sleep(1)
            shoot(m, os.path.join(OUT_DIR, name + "-firefox.png"))
            shoot(m, os.path.join(OUT_DIR, name + "-firefox-full.png"), full=True)
            reference = {"height": m.execute_script("return document.documentElement.scrollHeight"),
                         "elements": m.execute_script("return document.querySelectorAll('*').length")}
        def setup_tab(url=page_url, lean=BRIDGE_LEAN):
            """The bridge tab: the selected tab, JS off, on the page's URL or the lean bridge's; its URL."""
            m.navigate("about:blank")
            chrome(m, mrt_phone.JS_OFF if PHONE else JS_OFF)
            if lean:
                # never the page itself: a small document of its origin, from which the document is fetched
                from urllib.parse import urlsplit
                u = urlsplit(url)
                m.navigate(f"{u.scheme}://{u.netloc}/__mrt_lean_bridge__")
                time.sleep(2)
                return url
            m.navigate(url)
            time.sleep(5)
            return m.get_url()
        if TABLESS:
            # no bridge tab: Marionette's blank one stays as it is until a request needs the tab
            if m is not None:
                m.navigate("about:blank")
            final, js_off = page_url, None
        else:
            final = setup_tab()
            # with JS off, <noscript> content is parsed as elements: proof that the site's code did not run
            js_off = m.execute_script("const n = document.querySelector('noscript'); return n ? n.children.length > 0 : null;", sandbox="bridge")
        # env from the API process: its recording of this Firefox, made from the bridge tab on the page
        # the first time (TABLESS: a tab opened for that, and closed)
        env_path = standalone_env_file() if NO_FIREFOX else env_file(m, proxy is not None) if "env" in API_ROUTED else None

        def record_page_env():
            if NO_FIREFOX:
                raise ValueError(f"NO_FIREFOX=1: {env_path} has no environment for this document's secure/isolated context")
            if TABLESS:
                setup_tab(final)
            try:
                return record_env(m, env_path, top=True)
            finally:
                if TABLESS:
                    m.navigate("about:blank")
        if env_path and not load_env(env_path)["entries"]:
            record_page_env()
        recorded = next(iter(load_env(env_path)["entries"].values())) if env_path else None
        api = ApiProcesses(name, api_net_env(m, proxy, recorded) if "fetch" in API_ROUTED else None)
        if env_path:
            api.net_env["API_ENV"] = env_path
        # a URL's site: Firefox's (Services.eTLD), or with env in the API process the public suffix
        # list's, as mrt-api has it (a run without a tab has no Firefox to ask)
        site_of = psl_site if "env" in API_ROUTED else (lambda u: firefox_site(m, u))
        # from the server (or the replay proxy), not the HTTP cache: the bridge tab's own load cached
        # it, and a bridge case changed since would be read as it was
        doc_req = {"op": "fetch", "url": final, "method": "GET", "cache": "reload", "headers": [["accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"]], "body": None}
        if "fetch" in API_ROUTED:
            # the API process fetches the document too, so its Set-Cookie lands in the page's jar: as the
            # tab's fetch did, from the tab's document (its origin and URL; with no tab, the page's own)
            if TABLESS:
                tab_origin, tab_url = document_global(final, [])[0], final
            else:
                tab_origin, tab_url = m.execute_script("return [window.origin, document.URL];", sandbox="bridge")
            doc_site = site_of(final) or tab_origin
            doc_fetch = lambda r: fetch_reply(api.call(doc_site, doc_site, dict(r, origin=tab_origin, referrer=tab_url)))
        else:
            doc_fetch = lambda r: op(m, r)
        doc = doc_fetch(doc_req)
        if "err" in doc:
            # Docs' recording first answers its document with a redirect to the login page and back, which
            # a fetch of the page's origin cannot follow across origins (task 6da8; a profile without the
            # login cookies, as Fenix's fresh one, gets it): fetched as a script source is, from the chrome side
            doc = doc_fetch({"op": "fetch", "url": final, "method": "GET", "headers": [], "body": None, "script": True})
        if "err" in doc:
            return {"url": url, "error": "document: " + doc["err"]}
        html_file = os.path.join(OUT_DIR, name + ".html")
        open(html_file, "w").write(doc["ok"]["body"] or "")
        log_path = os.path.join(OUT_DIR, name + ".log")
        log = open(log_path, "w")
        # REALSITE_WRAP: a command to run realsite under, e.g. "heaptrack -o FILE" (with
        # MRT_EXIT_AFTER_REPORT=1, its leak list is what the page held when measured)
        # the page's global is made from Firefox's own answers for the page (mrt_js_new); with no tab,
        # from its document's URL and response, as Firefox makes them
        if TABLESS:
            origin, secure, isolated = document_global(document_url(doc["ok"]["url"], final), doc["ok"].get("headers"))
        else:
            origin, secure, isolated = m.execute_script("return [window.origin, window.isSecureContext, window.crossOriginIsolated];", sandbox="bridge")
        if env_path and env_key(secure, isolated) not in load_env(env_path)["entries"]:
            record_page_env()
        global_args = ["--origin", origin, "--secure-context", str(int(bool(secure))), "--cross-origin-isolated", str(int(bool(isolated)))]
        # lay out at the bridge tab's viewport, which is Firefox's for the page (GAPS B28); with no tab,
        # the recorded tab's
        if TABLESS:
            view_w, view_h = load_env(env_path)["viewport"]
        else:
            view_w, view_h = m.execute_script("return [window.innerWidth, window.innerHeight];", sandbox="bridge")
        global_args += ["--viewport", str(view_w), str(view_h)]
        args = shlex.split(os.environ.get("REALSITE_WRAP", "")) + [REALSITE, html_file, document_url(doc["ok"]["url"], final)] + global_args + shlex.split(os.environ.get("REALSITE_ARGS", "")) + (["--render"] if RENDER else []) + (["--interactive"] if INTERACTIVE or VIEWER_HTTP else [])
        dom_file = os.path.join(OUT_DIR, name + ".dom")
        # the runtime's console, next to Firefox's from firefox_realsites.py (GAPS B20; not in git)
        os.makedirs(os.path.join(HERE, "realsites-console"), exist_ok=True)
        env = dict(os.environ, MRT_DUMP_CONSOLE=os.path.join(HERE, "realsites-console", name + "-mini" + ("-" + os.path.basename(os.path.normpath(os.environ["MINI_DIR"])) if os.environ.get("MINI_DIR") else "") + ".json"))
        if HEAP_SCAN:
            env["MRT_DUMP_DOM"] = dom_file
        proc = start_realsite(args, log, env)
        heap = bridge_mem = api_mem = None
        tab_ops = {}            # op kind -> [calls, ms]: the requests the bridge tab served (Marionette round trips)
        pageout = {}
        report, requests, rendered = None, {}, None
        mirrored = {"flushes": 0, "ops": 0, "errors": 0}
        fetches = open(os.path.join(OUT_DIR, name + "-requests.jsonl"), "w")
        page = Runtime(proc, None, origin, fetches, None)
        page.viewport = [view_w, view_h]
        # the Trusted Types directives of its CSP: the tab's (env) or its document's, read here
        page.csp = trusted_types_of(csp_policies(doc["ok"].get("headers"), doc["ok"]["body"], document_url(doc["ok"]["url"], final)))
        page.doc = doc_policies(doc["ok"].get("headers"), doc["ok"]["body"], document_url(doc["ok"]["url"], final))
        frames = Frames(m, name, page, global_args, page_url=document_url(doc["ok"]["url"], final))
        frames.site_of = site_of
        if VIEWER_HTTP:
            import http_viewer
            frames.viewer = http_viewer.Viewer(name, document_url(doc["ok"]["url"], final), [view_w, view_h])
            frames.viewer.runtime_pid = proc.pid
            viewer_service = http_viewer.Service(VIEWER_HTTP, frames.viewer).start()
            if "fetch" in API_ROUTED:
                # the page's media resources, as its origin and jar get them (GAPS B172)
                frames.viewer.media_fetch = lambda entry, rng: fetch_reply(api.call(entry["site"], entry["top"], {
                    "op": "media", "range": rng, **{k: v for k, v in entry.items() if k in ("url", "crossOrigin", "kind", "origin", "referrer", "metaReferrer", "doc") and v is not None}}))
            print(f"VIEWER_URL {viewer_service.url}", flush=True)
        if TABLESS:
            frames.dropped = frames.tabless = True
        sites = {}
        environment_errors = []
        if "fetch" in API_ROUTED:
            # a frame's document from the API process too: as the tab fetched it, from the frame's own
            # document there (its origin and URL), in the frame site's partition under the page's
            def frame_doc(req):
                origin, url = m.execute_script("return [window.origin, document.URL];", sandbox="bridge")
                return fetch_reply(api.call(frames.site(req["url"]) or origin, doc_site, dict(req, origin=origin, referrer=url)))
            frames.doc_fetch = frame_doc

        def serve_env(rt, req):
            """env from the API process (its recording, at the runtime's viewport), the Trusted Types
            directives of the runtime's document added; a kind of global not recorded yet is recorded
            from the bridge tab first. API_ENV_CHECK: compared with the tab's answer."""
            if env_key(req.get("secure"), req.get("isolated")) not in load_env(env_path)["entries"]:
                if NO_FIREFOX:
                    message = f"NO_FIREFOX=1: {env_path} has no {env_key(req.get('secure'), req.get('isolated'))}"
                    environment_errors.append(message)
                    return {"err": message}
                frames.ensure_bridge(req)
                frames.use(rt, req.get("ctx"))
                record_env(m, env_path, top=False)
            reply = api_of(rt).call(api_site(rt), top_of(rt), dict(req, viewport=rt.viewport))
            if "ok" not in reply:
                return reply
            reply["ok"]["csp"] = rt.csp
            if rt.navigation:
                reply["ok"]["navigation"] = rt.navigation
            if frames.viewer:
                reply["ok"]["httpViewer"] = True
                reply["ok"]["fetchAsync"] = "fetch" in API_ROUTED
                reply["ok"]["mediaReplayUnlimited"] = os.environ.get("VIEWER_MEDIA_REPLAY_UNLIMITED") == "1"
            if API_ENV_CHECK and not frames.dropped:
                frames.use(rt, req.get("ctx"))
                tab = run_op(m, {"op": "env"}).get("ok") or {}
                tab["csp"] = trusted_types_policies(m)
                differ = sorted(k for k in set(tab) | set(reply["ok"]) if tab.get(k) != reply["ok"].get(k))
                api.env_check.append({"runtime": rt.id, "differ": {k: {"tab": tab.get(k), "api": reply["ok"].get(k)} for k in differ}})
            return reply

        def api_site(rt):
            """A runtime's site as Firefox has it (Fission's key): a frame runtime's own, else its origin's."""
            if rt.site:
                return rt.site
            if rt.origin not in sites:
                sites[rt.origin] = frames.site(rt.origin) or rt.origin
            return sites[rt.origin]

        def api_of(rt):
            """The API processes a runtime's requests go to: a navigation's new runtime has its own
            until it is the page (Frames.navigate)"""
            return rt.api or api

        def top_of(rt):
            """A runtime's top-level site: a navigation's new runtime is its own top-level document"""
            return rt.top or api_site(page)

        def cookie_beside_tab(rt, req):
            """document.cookie in the API process while fetch is still the tab's (Phase 6.2): the two
            jars kept the same. Firefox's is copied in first if the tab used the network since the
            last copy (its responses' Set-Cookie), and a write is made in the tab too (Firefox's
            fetches send what the page set)."""
            site, top = api_site(rt), api_site(page)
            if frames.cookies_changed:
                t = time.perf_counter()
                from urllib.parse import urlsplit
                m.set_context("chrome")
                try:
                    cookies = m.execute_script(TAB_COOKIES, script_args=(urlsplit(site).hostname or "",), sandbox="system")
                finally:
                    m.set_context("content")
                synced = api.call(site, top, {"op": "cookie", "call": "sync", "cookies": cookies})
                api.cookie_sync["syncs"] += 1
                api.cookie_sync["changed"] += (synced.get("ok") or {}).get("changed", 0)
                api.cookie_sync["ms"] += (time.perf_counter() - t) * 1000
                frames.cookies_changed = False
            reply = api.call(site, top, req)
            if "value" in req and "err" not in reply:
                frames.ensure_bridge(req)
                frames.use(rt, req.get("ctx"))
                op(m, {"op": "cookie", "value": req["value"]})
                api.cookie_sync["writes_to_tab"] += 1
            return reply

        def log_fetch(rt, req, reply):
            ok = reply.get("ok") or {}
            if req.get("call") == "poll" and ok.get("pending"):
                return
            rt.fetches.write(json.dumps({"t": req.get("t"), "method": req.get("method") or "GET", "url": req.get("url") or ok.get("url"), "script": bool(req.get("script")),
                                         "call": req.get("call"), "job": req.get("id") or ok.get("job"), "pending": ok.get("pending"),
                                         "status": ok.get("status"), "ms": ok.get("ms"), "err": reply.get("err"),
                                         # the request's header names, not their values (X's carry the login's tokens)
                                         "headers": sorted(h[0].lower() for h in req.get("headers") or [])}) + "\n")

        def log_ws(rt, req, reply):
            ok = reply.get("ok") or {}
            rt.fetches.write(json.dumps({"method": "WEBSOCKET", "url": req["url"], "events": [e["type"] for e in ok.get("events") or []],
                                         "throws": (ok.get("throws") or {}).get("name"), "err": reply.get("err")}) + "\n")

        def serve(rt, req):
            """A request of runtime `rt` that is not about the merged event loop: its reply."""
            nonlocal report, heap, bridge_mem, api_mem, rendered, mirrored
            requests[req["op"]] = requests.get(req["op"], 0) + 1
            if frames.viewer and req["op"] == "mediaType":
                return frames.viewer.media_type(req)
            if frames.viewer and req["op"] == "webgl":
                # the API process's contexts answer (GAPS B169); the viewer draws
                native = (lambda r: api_of(rt).call(api_site(rt), top_of(rt), dict(r, doc=rt.doc) if rt.doc else r)) if "webgl" in API_ROUTED else None
                return frames.viewer.webgl(req, native=native)
            if frames.viewer and req["op"] == "mirror":
                frames.viewer.publish(req["ops"], req.get("snapshot"))
                return {"ok": None}
            if frames.viewer and req["op"] == "mirrorCheck":
                return {"ok": None}
            if frames.viewer and req["op"] == "mediaResource":
                # a media element's resource, loaded by the viewer through the API process (GAPS B172)
                return frames.viewer.media_resource(req, {"site": api_site(rt), "top": top_of(rt), "doc": rt.doc})
            if req["op"] == "cookie" and "cookie" in API_ROUTED and "fetch" not in API_ROUTED:
                return cookie_beside_tab(rt, req)
            # Browser-backed mediaType was routed above for HTTP viewers;
            # privileged Firefox mode keeps its reference backend too. The
            # native API alone reports an empty set because it has no player.
            if req["op"] in API_ROUTED and not (req["op"] == "mediaType" and not NO_FIREFOX):
                # a kind the API process serves never reaches the bridge tab, nor makes a dropped one again;
                # the processes go with the tab, so a global that talked to older ones first gives up
                # what lived there, as for the tab (bridge.js)
                if req.get("gen") is not None and req["gen"] != frames.gen:
                    return {"bridgeNew": frames.gen}
                # a request of the runtime's document (not a worker's from the network) under its policies
                if (req["op"] in ("fetch", "image", "webgl") or (req["op"] == "ws" and req.get("call") == "open")) and rt.doc and not req.get("docless"):
                    req = dict(req, doc=rt.doc)
                reply = serve_env(rt, req) if req["op"] == "env" else api_of(rt).call(api_site(rt), top_of(rt), req)
                reply["gen"] = frames.gen
                if req["op"] == "fetch":
                    fetch_reply(reply)
                    log_fetch(rt, req, reply)
                elif req["op"] == "ws" and req.get("call") == "open":
                    log_ws(rt, req, reply)
                elif req["op"] == "image":
                    ok = reply.get("ok") or {}
                    rt.fetches.write(json.dumps({"t": req.get("t"), "method": "IMAGE", "url": req["url"], "ok": ok.get("ok"),
                                                 "size": [ok.get("width"), ok.get("height")], "ms": ok.get("ms"), "err": reply.get("err")}) + "\n")
                return reply
            if req["op"] == "bridgeDrop":
                # the page's bridge tab dropped (SPEC decision 14), and the API processes before it; a
                # frame runtime shares them
                if rt is page:
                    if not frames.dropped:
                        frames.drop(req)
                    elif API_ROUTED and api.procs:
                        # no tab to drop (TABLESS, none made yet): the API processes alone
                        frames.gen += 1
                        frames.recovery.append({"drop": req.get("t"), "gen": frames.gen, "tab": False})
                    api.drop()
                return {"ok": None}
            if req["op"] == "mirrorCheck" and frames.firefox_quit:
                pass  # no tab to check against, nor to make again; the owner's input is kept below
            elif req["op"] not in ("report", "render"):
                # a global that last talked to an older tab first gives up what lived there (bridge.js)
                if req.get("gen") is not None and req["gen"] != frames.gen:
                    return {"bridgeNew": frames.gen}
                frames.ensure_bridge(req)
                frames.use(rt, req.get("ctx"))
            elif not frames.dropped:
                # (a report needs no bridge tab: a dropped one is not made again for it)
                frames.use(rt, req.get("ctx"))
            if req["op"] == "report":
                if rt is not page:
                    rt.report = req["data"]
                    return {"ok": None}
                report = req["data"]
                # the runtime waits for this reply, so its memory is still while it is read
                if HEAP_SCAN and PHONE:
                    pid = mrt_phone.pid_in(log_path)
                    mrt_phone.fetch(dom_file)
                    heap = {"pid": pid, **mrt_phone.scan(pid), "dom": heap_compress.compress_file(dom_file)}
                elif HEAP_SCAN:
                    heap = {"pid": proc.pid, **heap_compress.scan(proc.pid), "dom": heap_compress.compress_file(dom_file)}
                if PHONE and PAGEOUT:
                    # zram's use now, and once the runtime has paged itself out after this reply
                    pageout["zram_before"] = mrt_phone.zram()
                    pageout["thread"] = mrt_phone.watch_pageout(log_path, pageout)
                if BRIDGE_MEMORY and API_ROUTED:
                    api_mem = api.memory()
                if BRIDGE_MEMORY and not frames.dropped:
                    try:
                        bridge_mem = bridge_memory(m, name, m.get_url())
                    except Exception as e:
                        bridge_mem = {"error": repr(e)[:300]}
                return {"ok": None}
            if req["op"] == "mirror":
                # the runtime's document copied into the bridge tab (INTERACTIVE); what it loads
                # (images, sheets, media) may set cookies
                frames.cookies_changed = True
                mirrored["flushes"] += 1
                mirrored["ops"] += len(req["ops"])
                flushed = [len(req["ops"]), any(o[0] == "u" for o in req["ops"])]
                for o in req["ops"]:
                    mirrored.setdefault("by_kind", {})[o[0]] = mirrored.setdefault("by_kind", {}).get(o[0], 0) + 1
                    # the document's URLs, as pushState moved it (opening a post on X)
                    if o[0] == "u" and len(mirrored.setdefault("urls", [])) < 20:
                        mirrored["urls"].append([round(time.time() - t0, 1), o[1]])
                try:
                    if not mirrored.get("media_setup"):
                        mirrored["media_setup"] = m.execute_script(MEDIA_SETUP, sandbox="mirror", new_sandbox=False)
                    done = m.execute_script(MIRROR_APPLY, script_args=(req["ops"],), sandbox="mirror", new_sandbox=False)
                    # when the tab had applied it (click_times)
                    if len(mirrored.setdefault("timeline", [])) < 50000:
                        mirrored["timeline"].append([round(time.time() * 1000)] + flushed)
                    # rules Firefox refused to take from the runtime's sheets (insertRule)
                    mirrored["rules_refused"] = mirrored.get("rules_refused", 0) + done["refused"]["count"]
                    if done["refused"]["examples"] and len(mirrored.setdefault("refused_examples", [])) < 10:
                        mirrored["refused_examples"] += done["refused"]["examples"]
                    # media ops the tab's MediaSource or element threw on (task 753cc)
                    mirrored["media_refused"] = mirrored.get("media_refused", 0) + done["mediaRefused"]["count"]
                    if done["mediaRefused"]["examples"] and len(mirrored.setdefault("media_refused_examples", [])) < 10:
                        mirrored["media_refused_examples"] += done["mediaRefused"]["examples"]
                except Exception as e:
                    mirrored["errors"] += 1
                    mirrored["last_error"] = repr(e)[:300]
                return {"ok": None}
            if req["op"] == "mirrorCheck":
                # the owner's input: as the driver took it from the tab, and as the page got it
                mirrored["input"] = dict(frames.input, page=req.get("input"))
                if frames.firefox_quit:
                    mirrored["check"] = {"skipped": "Firefox quit"}
                    return {"ok": None}
                try:
                    mirrored["check"] = mirror_check(m, name, req["digest"], req.get("sheets") or [])
                except Exception as e:
                    mirrored["check"] = {"error": repr(e)[:300]}
                return {"ok": None}
            if req["op"] == "render":
                if rt is not page:
                    return {"ok": None}
                try:
                    rendered = render(m, name, final, req["html"])
                    return {"ok": None}
                except Exception as e:
                    rendered = {"error": repr(e)[:300]}
                    return {"err": repr(e)[:300]}
            t_op = time.perf_counter()
            reply = op(m, req)
            # the tab's ops by kind: calls and their Marionette round trips, beside api_process's
            tab_ops.setdefault(req["op"], [0, 0.0])
            tab_ops[req["op"]][0] += 1
            tab_ops[req["op"]][1] += (time.perf_counter() - t_op) * 1000
            reply["gen"] = frames.gen
            if req["op"] in TAB_NETWORK_OPS:
                frames.cookies_changed = True
            if req["op"] == "env" and "ok" in reply:
                # the document's Trusted Types directives, as Firefox parsed its CSP (GAPS B91)
                reply["ok"]["csp"] = trusted_types_policies(m)
            if req["op"] == "fetch":
                log_fetch(rt, req, reply)
            elif req["op"] == "image":
                ok = reply.get("ok") or {}
                rt.fetches.write(json.dumps({"t": req.get("t"), "method": "IMAGE", "url": req["url"], "ok": ok.get("ok"),
                                             "size": [ok.get("width"), ok.get("height")], "ms": ok.get("ms"), "err": reply.get("err")}) + "\n")
            elif req["op"] == "ws" and req.get("call") == "open":
                log_ws(rt, req, reply)
            return reply

        frames.serve = serve

        # ---- navigation in the HTTP viewer (DESIGN "Top-level navigation in the HTTP viewer") ----
        navigations = []        # each navigation's outcome, for the report
        tab_session = {}        # origin -> the sessionStorage items the tab's last document of it left
        # a document URL of the tab's history -> the referrer its load sent: a traversal back to it
        # sends that one again, as Gecko's session history entry keeps its referrer info
        entry_referrer = {}
        page.url = document_url(doc["ok"]["url"], final)

        def load_destination(rt, req, new_api, n):
            """The destination of runtime `rt`'s navigation `req`, fetched through `new_api` (a set of
            API processes on the page's store) and loaded into a new runtime, served until its first
            wait: (that Runtime, None), or (None, why not)."""
            import re
            url = req["url"]
            if not re.match(r"(?i)https?:", url):
                return None, f"{url.split(':', 1)[0]}: URLs are not loaded"
            hist = req.get("history") or {"entries": [rt.url], "index": 0}
            entries, index = list(hist["entries"]), int(hist["index"])
            if req.get("traverse") is not None:
                index, kind = int(req["traverse"]), 2
                if not 0 <= index < len(entries):
                    return None, "no such history entry"
            elif req.get("replace"):
                kind = 1 if req.get("reload") else 0
            else:
                entries, index, kind = entries[:index + 1] + [url], index + 1, 0
            dest_site = site_of(url) or document_global(url, [])[0]
            fetch_req = {"op": "fetch", "navigate": True, "url": url, "method": req.get("method") or "GET",
                         "headers": [["content-type", req["contentType"]]] if req.get("contentType") else [],
                         "body": req.get("body"), "origin": req.get("origin") or rt.origin, "referrer": req.get("referrer") or "",
                         "referrerPolicy": req.get("referrerPolicy") or "", "metaReferrer": req.get("metaReferrer") or "",
                         "initiatorSite": api_site(rt), "user": bool(req.get("user")), "cache": "reload" if req.get("reload") else "default"}
            if rt.doc:
                fetch_req["doc"] = rt.doc
            if req.get("traverse") is not None:
                fetch_req.update(referrer=entry_referrer.get(url, ""), referrerPolicy="", metaReferrer="")
            reply = fetch_reply(new_api.call(dest_site, dest_site, fetch_req))
            log_fetch(rt, fetch_req, reply)
            if "err" in reply:
                return None, reply["err"]
            got = reply["ok"]
            headers = {k.lower(): v for k, v in got.get("headers") or []}
            if got.get("status") in (204, 205):
                return None, f"HTTP {got['status']}: no content, the document stays"
            if "attachment" in headers.get("content-disposition", "").lower():
                return None, "a download (Content-Disposition: attachment)"
            kind_of = headers.get("content-type", "").split(";")[0].strip().lower()
            if kind_of and kind_of not in ("text/html", "application/xhtml+xml"):
                return None, f"a {kind_of} document, which the runtime does not show"
            dest = document_url(got["url"], url)
            entries[index] = dest
            origin, secure, isolated = document_global(dest, got.get("headers"))
            if env_key(secure, isolated) not in load_env(env_path)["entries"]:
                return None, f"{env_path} has no {env_key(secure, isolated)} environment"
            html = os.path.join(OUT_DIR, f"{name}-nav{n}.html")
            open(html, "w").write(got.get("body") or "")
            width, height = frames.viewer.viewport
            args = [REALSITE, html, dest, "--origin", origin, "--secure-context", str(int(bool(secure))),
                    "--cross-origin-isolated", str(int(bool(isolated))), "--viewport", str(width), str(height)] + shlex.split(os.environ.get("REALSITE_ARGS", "")) + ["--interactive"]
            env = dict(os.environ, MRT_DUMP_CONSOLE=os.path.join(HERE, "realsites-console", f"{name}-mini-nav{n}.json"))
            new = Runtime(start_realsite(args, open(os.path.join(OUT_DIR, f"{name}-nav{n}.log"), "w"), env), None, origin,
                          open(os.path.join(OUT_DIR, f"{name}-nav{n}-requests.jsonl"), "w"), None)
            new.viewport = [width, height]
            new.csp = trusted_types_of(csp_policies(got.get("headers"), got.get("body"), dest))
            new.doc = doc_policies(got.get("headers"), got.get("body"), dest)
            new.api, new.top, new.url = new_api, site_of(dest) or origin, dest
            entry_referrer[dest] = got.get("referrer") or ""
            new.navigation = {"referrer": got.get("referrer") or "", "history": {"entries": entries, "index": index}, "type": kind,
                              "sessionStorage": {"origin": origin, "items": tab_session.get(origin, [])}}
            why = frames.load_until_ready(new)
            if why:
                if new.proc.poll() is None:
                    new.proc.kill()
                new.proc.wait()
                new.fetches.close()
                return None, why
            return new, None

        def navigate(rt, req):
            """A runtime's `navigate`: a refusal is recorded and shown; else the destination is loaded
            into a new runtime, and once that one is ready the old page is told to unload
            (`committed`), after which Frames.switch_page makes the new one the page. A failure
            leaves the old document, and the viewer says why."""
            viewer = frames.viewer
            if req.get("refused"):
                if len(navigations) < 200:
                    navigations.append({"url": req.get("url"), "refused": req["refused"]})
                viewer.navigation_notice(f"Not navigated to {req.get('url')}: {req['refused']}")
                return {"ok": None}
            if rt is not page or frames.pending_page is not None:
                return {"err": "another navigation is committing"}
            kept = req.get("sessionStorage") or {}
            if kept.get("origin"):
                tab_session[kept["origin"]] = kept.get("items") or []
            hist = req.get("history") or {}
            current = hist["entries"][hist["index"]] if hist.get("entries") else rt.url
            record = {"from": current, "url": req["url"], "method": req.get("method") or "GET",
                      "how": "traverse" if req.get("traverse") is not None else "reload" if req.get("reload") else "replace" if req.get("replace") else "push"}
            t = time.time()
            viewer.navigation_started(req["url"])
            n = len([r for r in navigations if "refused" not in r]) + 1
            new_api = ApiProcesses(name, dict(api.net_env), keep_store=True, logs=f"{name}-nav{n}")
            try:
                new, why = load_destination(rt, req, new_api, n)
            except Exception as e:
                new, why = None, repr(e)[:300]
            record["ms"] = round((time.time() - t) * 1000)
            if new is None:
                new_api.close()
                record["error"] = why
                navigations.append(record)
                viewer.navigation_failed(req["url"], why)
                return {"err": why}
            record.update(url=new.url, origin=new.origin, pid=new.proc.pid)
            navigations.append(record)
            frames.pending_page = new
            return {"ok": {"committed": True}}

        def switch(old, new):
            """The commit's end: the old runtime and its API processes (its sockets, contexts and jobs)
            end, and the new runtime, with its processes, is the page."""
            nonlocal page, proc, api
            if old.proc.poll() is None:
                old.proc.kill()
            old.proc.wait()
            old.done = True
            old.fetches.close()
            old_api, api = api, new.api
            new.api = None
            old_api.close()
            page, proc = new, new.proc
            frames.viewer.navigation_committed(new.url, new.proc.pid)

        if frames.viewer:
            frames.navigate, frames.on_switch = navigate, switch
        # a new bridge tab, after a drop: as this one, on the URL it ended at; with no tab at the start,
        # a lean one (SPEC decision 27: one that renders nothing)
        frames.setup_tab = (lambda: setup_tab(final, lean=True)) if TABLESS else (lambda: setup_tab(final))
        try:
            frames.drive(t0)
        finally:
            frames.close()
            api.close()
        rc = proc.wait()
        fetches.close()
        page.fetches.close()
        if pageout:
            pageout.pop("thread").join()
        if PHONE:
            mrt_phone.fetch(env["MRT_DUMP_CONSOLE"])
        out = {"url": url, "document_url": doc["ok"]["url"], "document_bytes": len(doc["ok"]["body"] or ""), "bridge_tab_js_off": js_off,
               "exit": rc, "wall_s": round(time.time() - t0), "requests": requests}
        if RENDER:
            out["render"] = {"firefox": reference, "mini": rendered}
        if INTERACTIVE:
            if mirrored.get("input", {}).get("acts"):
                mirrored["input"]["acts"] = click_times(mirrored["input"]["acts"], mirrored.get("timeline", []))
            mirrored.pop("timeline", None)
            out["mirror"] = mirrored
            if frames.firefox_quit:
                out["firefox_quit"] = True
        if API_ROUTED:
            out["api_process"] = api.stats()
        if api_mem:
            out["api_memory"] = api_mem
        out["tab_ops"] = {k: {"calls": n, "ms": round(ms, 1)} for k, (n, ms) in sorted(tab_ops.items())}
        # whether the run had a bridge tab: from the start, or made at the first request that needed one
        out["bridge_tab"] = (frames.tab_made or {"made": False}) if TABLESS else {"made": "at start"}
        if frames.viewer:
            out["viewer"] = dict(frames.viewer.stats, runtime_pid=proc.pid)
            out["navigations"] = navigations
            out["final_url"] = page.url
        if NO_FIREFOX:
            out["firefox_started"] = False
            out["environment_file"] = env_path
            out["unsupported_ops"] = frames.unsupported_ops
        if heap:
            out["heap_scan"] = heap
        if pageout:
            out["pageout"] = pageout
        if PHONE:
            out["phone"] = {"bridge": "release Fenix (Marionette over adb)", "viewport": [view_w, view_h]}
        if bridge_mem:
            out["bridge_memory"] = bridge_mem
        if proxy:
            out["replay"] = {"mode": replay.MODE, "log": os.path.relpath(proxy.log, HERE)}
        if frames.recovery:
            out["bridge_recovery"] = frames.recovery
        if len(frames.runtimes) > 1 or frames.refused:
            out["frames"] = frames.frame_reports()
            out["frame_loop"] = frames.loop_stats()
        if report:
            out.update(report)
        else:
            out["error"] = f"no report (exit {rc}; see {os.path.relpath(OUT_DIR, HERE)}/{name}.log)"
        if NO_FIREFOX and environment_errors:
            out["environment_errors"] = environment_errors
            out["error"] = environment_errors[0]
        if NO_FIREFOX and frames.unsupported_ops:
            out["error"] = "Firefox-only operations requested: " + ", ".join(sorted(frames.unsupported_ops))
        return out
    finally:
        if viewer_service is not None:
            viewer_service.close()
        if proc is not None and proc.poll() is None:
            proc.kill()
            proc.wait()
        if api is not None:
            api.close()
        if m is not None:
            if PHONE:
                mrt_phone.close_bridge(m)
            else:
                firefox_exit.close(m)
        if proxy:
            proxy.close()


out_file = os.path.join(HERE, os.environ.get("MINI_OUT", "mini_realsites_replay.json" if replay.MODE else "mini_realsites.json"))
results = json.load(open(out_file)) if os.path.exists(out_file) else {}
for name in NAMES:
    try:
        results[name] = run(name, SITES[name])
    except Exception as e:
        import traceback
        traceback.print_exc()
        results[name] = {"url": SITES[name], "error": repr(e)[:300]}
    r = results[name]
    print(name, "error" if "error" in r else "ok", r.get("error", ""), "| uss", r.get("uss_mb"), "js", r.get("js_mb"), "dom", r.get("dom_mb"),
          "elements", r.get("elements_all"), "scripts", r.get("scripts"), "errors", r.get("js_errors"), "wall", r.get("wall_s"), flush=True)
    json.dump(results, open(out_file, "w"), indent=1)
os._exit(1 if NO_FIREFOX and any("error" in results[name] for name in NAMES) else 0)
