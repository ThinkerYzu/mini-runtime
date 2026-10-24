"""Firefox side of the static-page comparison.

For each page in ../pages: load it in a fresh stock Firefox (JS off, images blocked, web fonts off),
dump memory reports, then hide the viewer (drops the frame tree and style data, keeps the DOM) and
dump again. Prints the page's own memory by category, in MB.

Also records the page height (documentElement.scrollHeight and the root and body boxes) at a 1489 x 844 viewport.

usage (from the Firefox source tree): FIREFOX_BIN=... ./mach python /path/to/firefox_static.py
heights only, headless: HEIGHT_ONLY=1 HEADLESS=1 FIREFOX_BIN=... ./mach python /path/to/firefox_static.py
"""
import functools, gzip, http.server, json, os, re, threading, time
import firefox_exit
from marionette_driver.marionette import Marionette

HERE = os.path.dirname(os.path.abspath(__file__))
PAGES = os.path.join(HERE, "..", "pages")
BIN = os.environ["FIREFOX_BIN"]
NAMES = os.environ.get("ONLY", "synthetic,hackernews,commons,mdn-css,wikipedia,wiki-long").split(",")
# HEIGHT_ONLY=1: only the page heights, written to firefox_height.json (the memory results are kept).
# HEADLESS=1: a headless Firefox (no window on the desktop); MOZ_HEADLESS_WIDTH/HEIGHT give it room for the viewport.
HEIGHT_ONLY = os.environ.get("HEIGHT_ONLY") == "1"
HEADLESS = os.environ.get("HEADLESS") == "1"
if HEADLESS:
    os.environ.setdefault("MOZ_HEADLESS_WIDTH", "1600"); os.environ.setdefault("MOZ_HEADLESS_HEIGHT", "1200")

handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=PAGES)
handler.func.log_message = lambda *a, **k: None
srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
PORT = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()

PREFS = {
    "marionette.allowSystemAccess": True, "network.trr.mode": 5, "network.dns.disableIPv6": True,
    "app.normandy.enabled": False, "messaging-system.rsexperimentloader.enabled": False,
    "app.update.disabledForTesting": True, "app.update.auto": False, "app.update.enabled": False,
    "datareporting.policy.dataSubmissionEnabled": False, "toolkit.telemetry.enabled": False,
    "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False,
    "browser.newtabpage.enabled": False, "browser.search.update": False,
    "services.settings.server": "http://localhost:1/", "extensions.autoDisableScopes": 0,
    # the comparison is DOM + style + layout only
    "javascript.enabled": False, "permissions.default.image": 2, "gfx.downloadable_fonts.enabled": False,
}
if HEIGHT_ONLY:
    # overlay scrollbars take no width, so the layout viewport is the 1489 CSS px the mini runtime lays out at
    PREFS["ui.useOverlayScrollbars"] = 1
FRAME = 'addMessageListener("hib:hide", () => { const v = docShell.docViewer; v.sticky = false; v.hide(); });'
LOAD_FS = 'gBrowser.selectedBrowser.messageManager.loadFrameScript("data:application/javascript," + encodeURIComponent(arguments[0]), false);'
SEND = "gBrowser.selectedBrowser.messageManager.sendAsyncMessage(arguments[0]);"
# The last argument is minimizeMemoryUsage: true runs Firefox's "Minimize memory usage" in every process
# before the reports are collected. Until session 39 every dump passed true (GAPS A17); now each page is
# measured plain and, in a second fresh run, minimized (`minimized` in the results).
DUMP = """const [path, minimize] = arguments; const done = arguments[arguments.length - 1];
Components.classes["@mozilla.org/memory-info-dumper;1"].getService(Components.interfaces.nsIMemoryInfoDumper)
  .dumpMemoryReportsToNamedFile(path, {callback() { done(true); }}, null, false, minimize);"""

def chrome(m, script, args=()):
    m.set_context("chrome")
    try: return m.execute_script(script, script_args=args, sandbox="system")
    finally: m.set_context("content")

def dump(m, tag, host, minimize=False):
    path = os.path.join(HERE, tag + ".json.gz")
    m.set_context("chrome"); m.timeout.script = 120
    m.execute_async_script(DUMP, script_args=(path, minimize), sandbox="system")
    m.set_context("content")
    reps = json.load(gzip.open(path))["reports"]; os.remove(path)
    cats = {}; heap = {}; subs = {}; arena = {}
    best = None
    for r in reps:
        if r["path"].startswith("explicit/window-objects/") and host in r["path"]:
            parts = r["path"].split("/"); cat = "other"
            for i, s in enumerate(parts):
                if s.startswith("window(") and i + 1 < len(parts): cat = re.sub(r"\(.*", "", parts[i + 1])
            cats.setdefault(r["process"], {}); cats[r["process"]][cat] = cats[r["process"]].get(cat, 0) + r["amount"]
            # second level under layout/ and dom/ (style-sets, frames, ...), so style can be told from frames
            for i, s in enumerate(parts):
                if s.startswith("window(") and i + 2 < len(parts) and re.sub(r"\(.*", "", parts[i + 1]) in ("layout", "dom"):
                    key = re.sub(r"\(.*", "", parts[i + 1]) + "/" + re.sub(r"\(.*", "", parts[i + 2])
                    subs.setdefault(r["process"], {}); subs[r["process"]][key] = subs[r["process"]].get(key, 0) + r["amount"]
                # the objects in the presshell arena (frames by class, nsLineBox, nsFrameList, ...): third level
                if s.startswith("window(") and i + 3 < len(parts) and re.sub(r"\(.*", "", parts[i + 1]) == "layout" and parts[i + 2] == "pres-arena":
                    arena.setdefault(r["process"], {}); arena[r["process"]][parts[i + 3]] = arena[r["process"]].get(parts[i + 3], 0) + r["amount"]
    proc = max(cats, key=lambda p: sum(cats[p].values())) if cats else None
    for r in reps:
        if r["process"] == proc and r["path"] == "heap-allocated": heap = r["amount"]
    mb = lambda x: round(x / 1048576, 2)
    c = cats.get(proc, {})
    return {"dom": mb(c.get("dom", 0)), "layout": mb(c.get("layout", 0)), "js": mb(c.get("js-realm", 0)),
            "windows_total": mb(sum(c.values())), "process_heap": mb(heap or 0),
            "sub": {k: mb(v) for k, v in sorted(subs.get(proc, {}).items())},
            "arena": {k: mb(v) for k, v in sorted(arena.get(proc, {}).items())}}

# SPEC criterion 3: the page height at the viewport the mini runtime lays out at (1489 x 844 CSS px).
VIEW_W, VIEW_H = 1489, 844
HEIGHT_JS = """const de = document.documentElement, b = document.body;
const h = (e) => e ? e.getBoundingClientRect().height : null;
return {innerWidth: window.innerWidth, innerHeight: window.innerHeight, clientWidth: de.clientWidth,
        scrollHeight: de.scrollHeight, rootHeight: h(de), bodyHeight: h(b), bodyScrollHeight: b ? b.scrollHeight : null,
        compat: document.compatMode};"""

def set_viewport(m):
    """Resize the window until the content area is VIEW_W x VIEW_H CSS px."""
    m.set_window_rect(width=VIEW_W + 40, height=VIEW_H + 120)
    for _ in range(4):
        iw, ih = m.execute_script("return [window.innerWidth, window.innerHeight]")
        if (iw, ih) == (VIEW_W, VIEW_H): return
        r = m.window_rect
        m.set_window_rect(width=r["width"] + VIEW_W - iw, height=r["height"] + VIEW_H - ih)

def page_height(m):
    return m.execute_script(HEIGHT_JS)

def run(name, minimize=False):
    m = Marionette(bin=BIN, headless=HEADLESS, prefs=PREFS)
    m.start_session()
    try:
        m.set_context("content")
        set_viewport(m)
        m.navigate(f"http://localhost:{PORT}/{name}.html"); time.sleep(5)
        height = page_height(m)
        if HEIGHT_ONLY:
            return {"height": height}
        chrome(m, LOAD_FS, (FRAME,))
        elements = m.execute_script("return document.querySelectorAll('*').length")
        live = dump(m, name + "-live", "localhost", minimize)
        chrome(m, SEND, ("hib:hide",)); time.sleep(2)
        hidden = dump(m, name + "-hidden", "localhost", minimize)
        return {"elements": elements, "height": height, "live": live, "hidden": hidden}
    finally:
        firefox_exit.close(m)

results = {}
for n in NAMES:
    for attempt in range(3):
        try: results[n] = run(n); print("done", n, flush=True); break
        except Exception as e: print("retry", n, repr(e)[:120], flush=True)
    # GAPS A17: the same page in a fresh Firefox, every dump after "Minimize memory usage"
    if n in results and not HEIGHT_ONLY:
        for attempt in range(3):
            try: results[n]["minimized"] = run(n, True); print("done minimized", n, flush=True); break
            except Exception as e: print("retry minimized", n, repr(e)[:200], flush=True)
json.dump(results, open(os.path.join(HERE, "firefox_height.json" if HEIGHT_ONLY else "firefox_static.json"), "w"), indent=1)
print(json.dumps(results, indent=1))
os._exit(0)
