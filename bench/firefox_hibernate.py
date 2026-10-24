"""Firefox side of the stage 4 comparison: hibernation in Gecko on the same pages.

For each page, in a fresh stock Firefox with JS on (images blocked, web fonts off, as in the other
harnesses): load, wait for the timers, GC and CC in the content process, dump memory ("live"); hide
the page with docViewer.hide(), which drops the frame tree and the style data and keeps the DOM and JS
(what the Gecko prototype does), GC and CC, dump again ("hidden"); then time docViewer.show() plus a
layout read (documentElement.offsetHeight) in the content process ("wake_ms", the rebuild), three
times with a hide in between. Memory as in firefox_js.py: the window's categories, js-zone and
js-realm.

usage (from a Firefox source tree): FIREFOX_BIN=... ./mach python /path/to/firefox_hibernate.py
  ONLY=a,b    pages (default: the six static pages and the five scripted ones); HEADLESS=1
Writes bench/firefox_hibernate.json.
"""
import functools, gzip, http.server, json, os, re, threading, time
import firefox_exit
from marionette_driver.marionette import Marionette

HERE = os.path.dirname(os.path.abspath(__file__))
PAGES = os.path.join(HERE, "..", "pages")
BIN = os.environ["FIREFOX_BIN"]
NAMES = os.environ.get("ONLY", "synthetic,hackernews,commons,mdn-css,wikipedia,wiki-long,synthetic-js,todo-js,table-sort-js,layout-js").split(",")
HEADLESS = os.environ.get("HEADLESS") == "1"
if HEADLESS:
    os.environ.setdefault("MOZ_HEADLESS_WIDTH", "1600"); os.environ.setdefault("MOZ_HEADLESS_HEIGHT", "1200")

handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=PAGES)
handler.func.log_message = lambda *a, **k: None
srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
PORT = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()

# firefox_static.py's prefs, with JS on
PREFS = {
    "marionette.allowSystemAccess": True, "network.trr.mode": 5, "network.dns.disableIPv6": True,
    "app.normandy.enabled": False, "messaging-system.rsexperimentloader.enabled": False,
    "app.update.disabledForTesting": True, "app.update.auto": False, "app.update.enabled": False,
    "datareporting.policy.dataSubmissionEnabled": False, "toolkit.telemetry.enabled": False,
    "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False,
    "browser.newtabpage.enabled": False, "browser.search.update": False,
    "services.settings.server": "http://localhost:1/", "extensions.autoDisableScopes": 0,
    "javascript.enabled": True, "permissions.default.image": 2, "gfx.downloadable_fonts.enabled": False,
}
# A frame script in the page's content process: a full GC plus a cycle collection, twice (the second
# GC frees what the first CC released), then a reply.
FRAME = """addMessageListener("mrt:gc", () => {
  for (let i = 0; i < 2; i++) content.windowUtils.garbageCollect();
  sendAsyncMessage("mrt:gc-done");
});
addMessageListener("hib:hide", () => {
  const v = docShell.docViewer; v.sticky = false; v.hide();
  sendAsyncMessage("hib:done", {});
});
addMessageListener("hib:show", () => {
  const t0 = content.performance.now();
  docShell.docViewer.show();
  const h = content.document.documentElement.offsetHeight;
  sendAsyncMessage("hib:done", {ms: content.performance.now() - t0, h});
});"""
# send a message to the frame script and wait for its "hib:done" reply
CALL = """const [msg] = arguments; const done = arguments[arguments.length - 1];
const mm = gBrowser.selectedBrowser.messageManager;
mm.addMessageListener("hib:done", function l(m) { mm.removeMessageListener("hib:done", l); done(m.data); });
mm.sendAsyncMessage(msg);"""
LOAD_FS = 'gBrowser.selectedBrowser.messageManager.loadFrameScript("data:application/javascript," + encodeURIComponent(arguments[0]), false);'
GC = """const done = arguments[arguments.length - 1]; const mm = gBrowser.selectedBrowser.messageManager;
mm.addMessageListener("mrt:gc-done", function l() { mm.removeMessageListener("mrt:gc-done", l); done(true); });
mm.sendAsyncMessage("mrt:gc");"""
# The last argument is minimizeMemoryUsage: true runs Firefox's "Minimize memory usage" in every process
# before the reports are collected. Until session 39 every dump passed true (GAPS A17); now each page is
# measured plain and, in a second fresh run, minimized (`minimized` in the results).
DUMP = """const [path, minimize] = arguments; const done = arguments[arguments.length - 1];
Components.classes["@mozilla.org/memory-info-dumper;1"].getService(Components.interfaces.nsIMemoryInfoDumper)
  .dumpMemoryReportsToNamedFile(path, {callback() { done(true); }}, null, false, minimize);"""

def chrome(m, script, args=(), run_async=False):
    m.set_context("chrome")
    try:
        if run_async:
            m.timeout.script = 120
            return m.execute_async_script(script, script_args=args, sandbox="system")
        return m.execute_script(script, script_args=args, sandbox="system")
    finally: m.set_context("content")

def dump(m, tag, page, minimize=False):
    """The page's reports: by category under its window, js-zone and js-realm split one level down."""
    path = os.path.join(HERE, tag + ".json.gz")
    chrome(m, DUMP, (path, minimize), run_async=True)
    reps = json.load(gzip.open(path))["reports"]; os.remove(path)
    by_proc = {}
    for r in reps:
        p = r["path"]
        if not (p.startswith("explicit/window-objects/") and page in p):
            continue
        parts = p.split("/")
        d = by_proc.setdefault(r["process"], {"cats": {}, "zone": {}, "realm": {}, "sub": {}})
        zi = next((i for i, s in enumerate(parts) if s.startswith("js-zone(")), None)
        wi = next((i for i, s in enumerate(parts) if s.startswith("window(")), None)
        if zi is not None:
            cat = "js-zone"
            key = re.sub(r"\(.*", "", parts[zi + 1]) if zi + 1 < len(parts) else "(self)"
            d["zone"][key] = d["zone"].get(key, 0) + r["amount"]
        elif wi is not None and wi + 1 < len(parts):
            cat = re.sub(r"\(.*", "", parts[wi + 1])
            if cat == "js-realm" and wi + 2 < len(parts):
                key = re.sub(r"\(.*", "", parts[wi + 2])
                d["realm"][key] = d["realm"].get(key, 0) + r["amount"]
            if cat in ("dom", "layout") and wi + 2 < len(parts):
                key = cat + "/" + re.sub(r"\(.*", "", parts[wi + 2])
                d["sub"][key] = d["sub"].get(key, 0) + r["amount"]
        else:
            cat = "other"
        d["cats"][cat] = d["cats"].get(cat, 0) + r["amount"]
    proc = max(by_proc, key=lambda p: sum(by_proc[p]["cats"].values()))
    d = by_proc[proc]
    mb = lambda x: round(x / 1048576, 3)
    # the content process as a whole: what lmkd sees (resident) and SPEC's heap target (heap-allocated)
    whole = {r["path"]: r["amount"] for r in reps if r["process"] == proc and r["path"] in ("heap-allocated", "resident", "resident-unique")}
    return {"process": {k: mb(v) for k, v in sorted(whole.items())},
            "cats": {k: mb(v) for k, v in sorted(d["cats"].items())},
            "js_zone": {k: mb(v) for k, v in sorted(d["zone"].items())},
            "js_realm": {k: mb(v) for k, v in sorted(d["realm"].items())},
            "sub": {k: mb(v) for k, v in sorted(d["sub"].items())}}

VIEW_W, VIEW_H = 1489, 844

def set_viewport(m):
    """Resize the window until the content area is VIEW_W x VIEW_H CSS px (as firefox_static.py)."""
    m.set_window_rect(width=VIEW_W + 40, height=VIEW_H + 120)
    for _ in range(4):
        iw, ih = m.execute_script("return [window.innerWidth, window.innerHeight]")
        if (iw, ih) == (VIEW_W, VIEW_H): return
        r = m.window_rect
        m.set_window_rect(width=r["width"] + VIEW_W - iw, height=r["height"] + VIEW_H - ih)

def run(name, minimize=False):
    m = Marionette(bin=BIN, headless=HEADLESS, prefs=PREFS)
    m.start_session()
    try:
        m.set_context("content")
        set_viewport(m)
        m.navigate(f"http://localhost:{PORT}/{name}.html"); time.sleep(12)
        info = m.execute_script("return {elements: document.querySelectorAll('*').length, height: document.documentElement.scrollHeight, innerWidth: innerWidth, innerHeight: innerHeight}")
        chrome(m, LOAD_FS, (FRAME,))
        chrome(m, GC, run_async=True); time.sleep(2)
        info["live"] = dump(m, name + "-live", name + ".html", minimize)
        chrome(m, CALL, ("hib:hide",), run_async=True); time.sleep(1)
        chrome(m, GC, run_async=True); time.sleep(2)
        info["hidden"] = dump(m, name + "-hidden", name + ".html", minimize)
        wakes = []
        for i in range(3):
            r = chrome(m, CALL, ("hib:show",), run_async=True)
            wakes.append(round(r["ms"], 1)); info["height_after_wake"] = r["h"]
            if i < 2:
                chrome(m, CALL, ("hib:hide",), run_async=True); time.sleep(0.5)
        info["wake_ms"] = wakes
        return info
    finally:
        firefox_exit.close(m)

results = {}
for n in NAMES:
    for attempt in range(3):
        try: results[n] = run(n); print("done", n, flush=True); break
        except Exception as e: print("retry", n, repr(e)[:200], flush=True)
    # GAPS A17: the same page in a fresh Firefox, every dump after "Minimize memory usage"
    if n in results:
        for attempt in range(3):
            try: results[n]["minimized"] = run(n, True); print("done minimized", n, flush=True); break
            except Exception as e: print("retry minimized", n, repr(e)[:200], flush=True)
json.dump(results, open(os.path.join(HERE, "firefox_hibernate.json"), "w"), indent=1)
print(json.dumps(results, indent=1))
os._exit(0)
