"""Pages that need the bridge (stage 6's realsite, bridge.js), against release Firefox: each case in
bench/bridge-cases/ runs in Firefox (firefox_probe.py) and in the mini runtime (mini_realsites.py),
from a local server, and the two logs it writes to `window.__log` are diffed. The binding cases
(tests/bindings.rs) cover what runs without the bridge.

  idb           IndexedDB forwarded to Firefox (SPEC decision 12): an upgrade, values of each
                structured-clone type, cursors, aborts, an explicit commit, key ranges, a versionchange
  scroll-size   clientWidth/Height and scrollWidth/Height of elements other than the scrolling element
  abspos        absolutely and fixed positioned boxes: offsets, static positions, sizes, containing blocks
  storage       sessionStorage (the runtime's, SPEC decision 14) and localStorage (forwarded): methods,
                named properties, the quota
  storage-edges localStorage and document.cookie edges (GAPS B161): lone surrogates, and storage events
                between the page and a same-origin frame (none to another origin of the site)
  ancestor-frame  a frame of the page's site inside another site's frame: a third party with the
                partition key's foreign-ancestor bit (API_PROCESS=all; the bridge tab has no such frame)
  cookie        document.cookie (the tab's, or the API process's with API_PROCESS=cookie): attributes,
                paths, expiry, domains, Secure and the prefixes, HttpOnly, SameSite, Partitioned, limits,
                a response's cookies (ws_echo.py), eviction, a cross-site frame's partitioned cookies
  fetch         fetch and XHR (the tab's, or the API process's with API_PROCESS=all), against ws_echo.py's /h:
                modes, credentials, CORS and preflights, exposed headers, redirects, no-cors, CORP, referrer
                policies, the request headers the server saw, cookies from responses, bodies, a cross-site script
  document-input  Document event-handler exposure, setter behavior and delegated textarea editing
  fetch-bytes   original response bytes under text MIME types, byte readers/clones/streams, UTF-8 and BOM
  websocket     WebSocket forwarded to Firefox (GAPS B70), or the API process's with API_PROCESS=all, against
                ws_echo.py: states, events, checks, a refused handshake
  websocket-policy  WebSocket beyond the protocol (GAPS B163): the document's connect-src, permessage-deflate
                both ways and its parameters, one handshake at a time per host, delayed reconnects after a refusal
  crypto        Web Crypto forwarded to Firefox, keys kept in the runtime (SPEC decision 21): the bindings,
                the sites' algorithms, keys' attributes, export, wrap, and keys used after the bridge tab
                forgot them
  blob          Blob and File from Gecko's dom/file (GAPS D84): conversions, slices, reads, and the Blobs
                Response, Request, XHR, FormData, structuredClone, postMessage and IndexedDB carry
  frame-layout  the frames' layout (DESIGN "The frames' layout (S5)"): each frame's document at its frame
                element's content box, innerWidth, client sizes, boxes, matchMedia inside it, resize
                events, a cross-origin frame's size
  message-ports MessagePorts from Gecko's dom/messagechannel (GAPS D97): holding, the clone, transfers and
                their errors, close(), and ports to, from and between workers and a shared worker
  frame-transfers  cross-window MessagePort and ArrayBuffer transfer, queue/reply delivery and invalid transfers
  blob-workers  blob: URLs (Gecko's BlobURLProtocolHandler, GAPS D98): fetch and XHR of them, and classic,
                module and shared workers from blob: and data: URLs
  plugins       navigator.plugins and navigator.mimeTypes (Gecko's nsPluginArray, GAPS D99)
  worker-apis   the bridge's APIs inside a worker (W2): fetch, XHR, Blob, FileReader, FileReaderSync,
                crypto, IndexedDB, WebSocket, nested workers
  web-locks     Web Locks from Gecko's dom/locks: grants, shared and exclusive, ifAvailable, steal, signal,
                the errors, query(), a worker's lock dropped when it is terminated, a data: worker
  compression   CompressionStream and DecompressionStream (Gecko's dom/compression): each format's bytes
                and chunks, the errors, a Response over a stream (the sites' log pings), a worker
  window-event  window.event (Gecko's nsPIDOMWindowInner::SetEvent around each listener, GAPS D108): the
                listener's window, nested dispatch, shadow trees, frames, [Replaceable]
  offsets       offsetParent and offsetLeft/Top/Width/Height (Gecko's Element::GetOffsetRect, GAPS D109):
                positioned ancestors, tables, transforms, wrapped inlines, shadow trees
  trusted-types Trusted Types without a CSP (Gecko's dom/security/trusted-types, GAPS D110): the API,
                policies, the type tables, the sinks taking Trusted objects, eval, a worker
  window-external window.external (Gecko's dom/base/External.cpp): the interface-less object, its two
                methods, brand checks, a frame's object, [Replaceable]
  window-screen-xy window.screenX, screenY, screenLeft, screenTop (Gecko's GetScreenXY): values,
                aliases, getters and setters, a frame's (its top's), [Replaceable]
  trusted-types-enforced  with require-trusted-types-for and trusted-types in a <meta> CSP: every
                sink, eval and Function, the default policy, violations, a frame made by script
  style-elements  <style> elements filled by script (innerHTML, textContent, text nodes), long sheets
  fragment-context  innerHTML, outerHTML and insertAdjacentHTML parsed in their context element
                (GAPS D112): table modes, RCDATA, raw text, SVG, templates
  permissions-profile  navigator.permissions from a profile's permission manager (GAPS B166), from
                b.a.localhost with PERMISSIONS_PROFILE: the parent domain's entries, ports and schemes,
                expiry, "Always Ask" camera and microphone, permissions.default.* preferences, a worker
  idb-profile   IndexedDB databases already in the profile (GAPS B168): idb-profile-seed.html writes them in
                Firefox first; then their schemas, every stored clone type and key type, a value over the data
                threshold, Blobs and Files, index order, the key generators continued, an upgrade
                (API_PROCESS=all: with a bridge tab the page runs in that tab first and changes the databases)
  permissions-storage  navigator.permissions and navigator.storage answered by Firefox (GAPS D114): every
                PermissionName, the descriptor's conversion, brand checks, when answers resolve,
                estimate, persisted, persist, a worker
  document-all  document.all (Gecko's HTMLAllCollection, GAPS B92): emulating undefined, also in hot
                loops, the sites' tests, indexed and named lookup, item(), the legacy caller, own keys
  notification  Notification (Gecko's dom/notification): the interface, the permission and
                requestPermission answered by Firefox, the constructor's options, the `error` event, a worker
  resize-observer  ResizeObserver (Gecko's dom/base/ResizeObserver.cpp, GAPS B15): the first observation,
                the three boxes and contentRect, changes, observing again, the depth loop and its error
  hit-test      elementFromPoint and elementsFromPoint (Gecko's QueryNodesFromPoint over a display list
                built for event delivery, GAPS B15): painting order, clips, visibility, pointer-events,
                tables, generated content, frames, shadow trees
  inner-text    innerText and outerText (Gecko's GetInnerText over the frames, GAPS B15): whitespace,
                visibility, blocks and <p>, <br>, tables, form controls, not rendered, the setters
  rule-style    live CSSStyleRule declarations, nested and adopted sheets, identity and invalid setters
  secondary-sheets  CSS parsing in a createHTMLDocument document, sheet lifetime and retained rules
  insert-rule   style sheets' CSSOM (Stylo's rules, GAPS B15): a sheet's own rules, insertRule and its
                errors, deleteRule, addRule, disabled, a new sheet when the owner changes, a link's
                and a constructed sheet, the computed style after each change
  parser-position  scripts run where the parser is (GAPS B8): what each sees of the document, in the
                head, the body, a table and SVG; microtasks between scripts; a linked sheet; the parser's
                mutation records; a custom element; deferred and external scripts
  bridge-recovery  the bridge tab dropped and made again (SPEC decision 14): fetch and XHR with the
                profile's cookies, both storages, an IndexedDB database reopened, a Web Crypto key
                re-imported, an open WebSocket closed as by a network loss, against ws_echo.py
  webgl-native  WebGL run in the API process (API_PROCESS=all, GAPS B169): limits, extensions, drawing,
                framebuffers, textures from arrays, ImageData, a WebGL canvas and images (a cross-origin
                one tainted), WebGL 2's vertex arrays, instancing, uniform blocks, transform feedback,
                integer targets, samplers, queries and syncs
  dom-parser-xml  DOMParser's XML types (GAPS B27): a DASH manifest as Facebook's video player reads
                it, entities, CDATA, SVG's namespace, the error document of XML that is not well-formed
  is-equal-node  Node.isEqualNode (GAPS B112): attributes in another order, values, children,
                text, comments, fragments, doctypes, documents, the errors, react-helmet-async's test
  touch-events  touch support as the bridge's Firefox has it: TouchEvent and the ontouch* handlers
                (none without a touchscreen), X's isTouchSupported, the pointer and hover queries
  input-events  the owner's input (Phase 5.3): clicks, typing, a checkbox, a wheel turn and a link
                done with WebDriver actions (input-events.actions.json), in Firefox's tab and in the
                bridge tab, from which the runtime gets them (realsite --interactive): each event's
                order, interface, phase, target, fields, isTrusted, the focused element, values
  create-event  document.createEvent over Gecko's table, initEvent, initCustomEvent, initUIEvent,
                initMouseEvent, the events' hierarchy, a UIEvent's view, defaultView, and the image's
                reflected attributes (longDesc and the others; GAPS B38, task 135a)
  botguard-reads  what YouTube's BotGuard reads, in its order, with the shape behind each read: a
                checklist that differs from Firefox until GAPS B141 to B148 are closed (task 135a)
  media-types   MIME/codec capability answers from the real Firefox bridge backend; standalone
                returns an empty support set because its HTTP viewer has no media transport
  media         media in the bridge tab (Phase 5.7, task 753cc): a plain src video and a MediaSource with
                two SourceBuffers, their metadata, appends, ranges, durations; after a click, play,
                pause, a seek, muting, a remove, endOfStream, playing to the end, replay, loop, a move
                into a new container while playing, and taking it out of the document
  scroll        scrolling (Phase 5.4, task 846d): the owner's wheel turns in the tab (scroll.actions.json)
                and the page's scrollTo, scrollBy, scrollTop, scrollIntoView: the scroll and scrollend
                events, the offsets, client rectangles of a static and a fixed box, elementFromPoint,
                IntersectionObserver entries

usage (from a Firefox source tree):
  FIREFOX_BIN=.../firefox xvfb-run -a ./mach python /path/to/bench/compare_bridge.py [CASE ...]
Prints each case's diff (a line count when the logs agree) and exits 1 if any differs; with API_PROCESS, a case the
runtime ran without a bridge tab says so, and one that made a tab names the request that needed it.
"""
import difflib, json, os, shutil, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
PORT = 8766
# another origin of the same site (sframes; 8767 and 8768 are taken on this machine)
SECOND_PORT = 8771
# bench/ws_echo.py, for the websocket case
WS_PORT = 8772
CASES = os.path.join(HERE, "bridge-cases")


def lines(text, tag):
    for line in reversed(text.splitlines()):
        if tag in line:
            return json.loads(line.split(tag, 1)[1].strip())
    return None


# what some cases are loaded with after their path: a fragment, which the document keeps (location,
# GAPS D71); the port of the second server (sframes)
URL_SUFFIXES = {"location": "#inbox", "sframes": f"?other={SECOND_PORT}", "xsite": f"?other={SECOND_PORT}", "websocket": f"?ws={WS_PORT}", "worker-apis": f"?ws={WS_PORT}", "bridge-recovery": f"?ws={WS_PORT}", "cookie": f"?ws={WS_PORT}", "fetch": f"?ws={WS_PORT}", "fetch-bytes": f"?ws={WS_PORT}", "images-api": f"?ws={WS_PORT}", "images-formats": f"?ws={WS_PORT}", "storage-edges": f"?other={SECOND_PORT}", "fetch-xframe": f"?ws={WS_PORT}", "webgl-native": f"?other={SECOND_PORT}"}


# a case whose document needs response headers: served by ws_echo.py's /doc/ with <case>.html.headers
DOC_CASES = ("fetch-policy", "fetch-coep", "fetch-coep-cl", "websocket-policy")


# a case loaded from another host than localhost (permissions-profile: a subdomain of a subdomain,
# so that its parent a.localhost is not the base domain's public suffix)
CASE_HOSTS = {"permissions-profile": "b.a.localhost"}

# permissions-profile's profile, for both sides (REALSITES_PROFILE): moz_perms rows (origin, type,
# action, expireType, expireTime) and user.js preferences. Firefox's schema 13.
PERMISSIONS_PROFILE = {
    "perms": [
        (f"http://b.a.localhost:{PORT}", "microphone", 3, 0, 0),            # "Always Ask": granted
        (f"http://b.a.localhost:{PORT}", "camera", 2, 1, 1000),             # expired: the parent's
        (f"http://a.localhost:{PORT}", "camera", 3, 0, 0),
        (f"http://a.localhost:{PORT}", "desktop-notification", 1, 0, 0),
        (f"http://a.localhost:{PORT}", "persistent-storage", 1, 0, 0),
        (f"http://a.localhost:{PORT}", "midi", 1, 0, 0),
        (f"http://b.a.localhost:{PORT}", "midi-sysex", 2, 0, 0),
        (f"http://a.localhost:{PORT + 1}", "loopback-network", 2, 0, 0),   # another port
        (f"https://a.localhost:{PORT}", "local-network", 2, 0, 0),         # another scheme
    ],
    "prefs": {"permissions.default.geo": 2, "permissions.default.screen-wake-lock": 2,
              "permissions.default.microphone": 2, "permissions.default.local-network": 2},
}


def make_profile(spec, path):
    import sqlite3
    os.makedirs(path)
    db = sqlite3.connect(os.path.join(path, "permissions.sqlite"))
    db.executescript("""CREATE TABLE moz_perms ( id INTEGER PRIMARY KEY,origin TEXT,type TEXT,permission INTEGER,expireType INTEGER,expireTime INTEGER,modificationTime INTEGER);
        CREATE TABLE moz_hosts ( id INTEGER PRIMARY KEY,host TEXT,type TEXT,permission INTEGER,expireType INTEGER,expireTime INTEGER,modificationTime INTEGER,isInBrowserElement INTEGER);
        CREATE TABLE moz_origin_interactions ( origin TEXT PRIMARY KEY,lastInteractionTime INTEGER);
        PRAGMA user_version = 13;""")
    db.executemany("INSERT INTO moz_perms (origin, type, permission, expireType, expireTime, modificationTime) VALUES (?, ?, ?, ?, ?, 0)", spec["perms"])
    db.commit()
    db.close()
    with open(os.path.join(path, "user.js"), "w") as f:
        for name, value in spec["prefs"].items():
            f.write(f"user_pref({json.dumps(name)}, {json.dumps(value)});\n")


CASE_PROFILES = {"permissions-profile": PERMISSIONS_PROFILE}

# a case whose profile Firefox makes by running a page (<case>-seed.html) on a fresh one: what that page
# stores (idb-profile: IndexedDB databases) is then in each side's copy
CASE_SEEDS = ("idb-profile",)


def seed_profile(case, path):
    os.makedirs(path + ".empty")
    probe = path + ".probe.js"
    open(probe, "w").write("return window.wrappedJSObject.__log ? Array.from(window.wrappedJSObject.__log) : null;")
    out = subprocess.run([sys.executable, os.path.join(HERE, "firefox_probe.py"), case + "-seed", f"http://localhost:{PORT}/{case}-seed.html", probe],
                         env=dict(os.environ, STEPS="2", EVERY="4", SAVE_PROFILE=path, REALSITES_PROFILE=path + ".empty"), capture_output=True, text=True)
    if not os.path.isdir(path) or "seeded" not in out.stdout:
        sys.exit(f"{case}: the seed page did not finish in Firefox:\n{out.stdout}{out.stderr[-2000:]}")

# how long a case with input runs in the runtime (INTERACTIVE_S), when 8 s is not enough
INTERACTIVE_SECONDS = {"scroll": "16", "media": "36"}
# when Firefox's tab is read (firefox_probe.py's EVERY, twice), for a case that runs longer than 12 s
PROBE_EVERY = {"media": "16", "websocket-policy": "14"}


def run(case, tmp):
    """The case's log in Firefox and in the mini runtime (None when one wrote none). A case with
    <case>.actions.json gets that input (input_actions.py): in Firefox's tab after load, and in the
    bridge tab of an interactive run (INTERACTIVE_S seconds) for the runtime."""
    host = CASE_HOSTS.get(case, "localhost")
    url = (f"http://localhost:{WS_PORT}/doc/{case}.html" if case in DOC_CASES else f"http://{host}:{PORT}/{case}.html") + URL_SUFFIXES.get(case, "")
    # one copy for each side: Firefox writes to its profile
    profile, mini_profile = {}, {}
    if case in CASE_PROFILES:
        profile = dict(REALSITES_PROFILE=os.path.join(tmp, "profile-firefox"))
        mini_profile = dict(REALSITES_PROFILE=os.path.join(tmp, "profile-mini"))
        for side in (profile, mini_profile):
            make_profile(CASE_PROFILES[case], side["REALSITES_PROFILE"])
    if case in CASE_SEEDS:
        seeded = os.path.join(tmp, "profile-seeded")
        seed_profile(case, seeded)
        profile = dict(REALSITES_PROFILE=os.path.join(tmp, "profile-firefox"))
        mini_profile = dict(REALSITES_PROFILE=os.path.join(tmp, "profile-mini"))
        for side in (profile, mini_profile):
            shutil.copytree(seeded, side["REALSITES_PROFILE"])
    actions = os.path.join(CASES, case + ".actions.json")
    with_input = dict(INPUT_ACTIONS=actions) if os.path.exists(actions) else {}
    probe = os.path.join(tmp, "probe.js")
    open(probe, "w").write("const l = window.wrappedJSObject.__log; return l ? 'CASELOG ' + JSON.stringify(Array.from(l)) : null;")
    out = subprocess.run([sys.executable, os.path.join(HERE, "firefox_probe.py"), case, url, probe],
                         env=dict(os.environ, STEPS="2", EVERY=PROBE_EVERY.get(case, "6"), **with_input, **profile), capture_output=True, text=True).stdout
    # firefox_probe.py prints "<seconds> <JSON of what the probe returned>"; the last log wins
    firefox = None
    for line in out.splitlines():
        try:
            value = json.loads(line.split(" ", 1)[1])
        except (IndexError, ValueError):
            continue
        if isinstance(value, str) and value.startswith("CASELOG "):
            firefox = json.loads(value[len("CASELOG "):])
    after = os.path.join(tmp, "after.js")
    open(after, "w").write('(function(){ console.log("CASELOG " + JSON.stringify(window.__log)); })();')
    subprocess.run([sys.executable, os.path.join(HERE, "mini_realsites.py")], capture_output=True,
                   env=dict(os.environ, **mini_profile, EXTRA_SITES=json.dumps({case: url}), ONLY=case, MINI_OUT=os.path.join(tmp, "mini.json"),
                            MRT_EVAL_AFTER=after, REALSITE_ARGS="--console", WALL_LIMIT="300",
                            **(dict(with_input, INTERACTIVE="1", INTERACTIVE_S=INTERACTIVE_SECONDS.get(case, "8")) if with_input else {})))
    mini = lines(open(os.path.join(HERE, "realsites-mini", case + ".log"), errors="replace").read(), "CASELOG ")
    # whether the runtime's side had a bridge tab (with API_PROCESS, a run may need none)
    try:
        tab = json.load(open(os.path.join(tmp, "mini.json")))[case].get("bridge_tab")
    except (OSError, ValueError, KeyError):
        tab = None
    return firefox, mini, tab


def main():
    # a page named *-child.html is loaded by a case (a frame's src), and *-seed.html makes a case's
    # profile (CASE_SEEDS): not cases
    cases = sys.argv[1:] or sorted(f[:-5] for f in os.listdir(CASES) if f.endswith(".html") and not f.endswith(("-child.html", "-seed.html")))
    server = subprocess.Popen([sys.executable, "-m", "http.server", str(PORT), "-d", CASES],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    # the same cases on another port: another origin of the same site (sframes, SPEC decision 19)
    second = subprocess.Popen([sys.executable, "-m", "http.server", str(SECOND_PORT), "-d", CASES],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ws = subprocess.Popen([sys.executable, os.path.join(HERE, "ws_echo.py"), str(WS_PORT)],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    failed = 0
    try:
        for case in cases:
            tmp = tempfile.mkdtemp(prefix="compare_bridge")
            try:
                firefox, mini, tab = run(case, tmp)
            finally:
                shutil.rmtree(tmp, ignore_errors=True)
            if firefox is None or mini is None:
                print(f"{case}: no log from", "firefox" if firefox is None else "the mini runtime")
                failed += 1
                continue
            diff = list(difflib.unified_diff(firefox, mini, "firefox", "mini", lineterm=""))
            made = (tab or {}).get("made")
            note = "" if made in (None, "at start") else " (no bridge tab)" if made is False else f" (a bridge tab made for {tab['by'].get('op')})"
            print(f"{case}{note}:", "\n".join(diff) if diff else f"{len(firefox)} lines, the same on both sides")
            failed += bool(diff)
    finally:
        server.kill()
        second.kill()
        ws.kill()
    return 1 if failed else 0


sys.exit(main())
