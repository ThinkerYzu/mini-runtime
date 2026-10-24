"""Firefox side of the text-run count comparison: how many gfxTextRuns, text frames and flows are live per page.

Needs the counting patch bench/firefox_textruns.patch applied to the Firefox tree (it makes PresShell::SizeOfTextRuns,
which the "layout/text-runs" memory reporter calls, append one JSON line of counts to the file named by
$HIB_TEXTRUN_STATS). The script loads each page the way firefox_static.py does (headless, JS off, images blocked,
1489 x 844), takes one memory report dump (the same moment as the memory numbers), and reads the counts line for the page.
Writes bench/firefox_textruns.json: per page, the counts, the reported text-runs bytes, and method.

usage (from the Firefox tree, MOZCONFIG=mozconfig-hib, patched build):
  FIREFOX_BIN=$PWD/obj-hib/dist/bin/firefox ./mach python /path/to/bench/firefox_textruns.py
"""
import functools, gzip, http.server, json, os, threading, time
import firefox_exit
from marionette_driver.marionette import Marionette

HERE = os.path.dirname(os.path.abspath(__file__))
PAGES = os.path.join(HERE, "..", "pages")
BIN = os.environ["FIREFOX_BIN"]
NAMES = os.environ.get("ONLY", "synthetic,hackernews,commons,mdn-css,wikipedia,wiki-long").split(",")
STATS = os.path.join(HERE, "firefox_textruns_raw.jsonl")
os.environ.setdefault("MOZ_HEADLESS_WIDTH", "1600"); os.environ.setdefault("MOZ_HEADLESS_HEIGHT", "1200")
os.environ["HIB_TEXTRUN_STATS"] = STATS
# the content sandbox forbids the counting patch writing its file
os.environ["MOZ_DISABLE_CONTENT_SANDBOX"] = "1"

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
    "javascript.enabled": False, "permissions.default.image": 2, "gfx.downloadable_fonts.enabled": False,
}
# The last argument is minimizeMemoryUsage: true runs Firefox's "Minimize memory usage" in every process
# before the reports are collected. Until session 39 every dump passed true (GAPS A17); the text-run
# count is taken with a plain dump.
DUMP = """const [path, minimize] = arguments; const done = arguments[arguments.length - 1];
Components.classes["@mozilla.org/memory-info-dumper;1"].getService(Components.interfaces.nsIMemoryInfoDumper)
  .dumpMemoryReportsToNamedFile(path, {callback() { done(true); }}, null, false, minimize);"""
VIEW_W, VIEW_H = 1489, 844

def set_viewport(m):
    m.set_window_rect(width=VIEW_W + 40, height=VIEW_H + 120)
    for _ in range(4):
        iw, ih = m.execute_script("return [window.innerWidth, window.innerHeight]")
        if (iw, ih) == (VIEW_W, VIEW_H): return
        r = m.window_rect
        m.set_window_rect(width=r["width"] + VIEW_W - iw, height=r["height"] + VIEW_H - ih)

def run(name):
    if os.path.exists(STATS): os.remove(STATS)
    m = Marionette(bin=BIN, headless=True, prefs=PREFS)
    m.start_session()
    try:
        m.set_context("content"); set_viewport(m)
        m.navigate(f"http://localhost:{PORT}/{name}.html"); time.sleep(5)
        path = os.path.join(HERE, name + "-tr.json.gz")
        m.set_context("chrome"); m.timeout.script = 120
        m.execute_async_script(DUMP, script_args=(path, False), sandbox="system")
        reps = json.load(gzip.open(path))["reports"]; os.remove(path)
        tr = sum(r["amount"] for r in reps if "window(" in r["path"] and "localhost" in r["path"] and r["path"].endswith("/layout/text-runs"))
        lines = [json.loads(l) for l in open(STATS)] if os.path.exists(STATS) else []
        mine = [l for l in lines if f"localhost:{PORT}/{name}.html" in l["url"]]
        assert len(mine) == 1, (name, lines)
        return {**mine[0], "textruns_reporter_bytes": tr}
    finally:
        firefox_exit.close(m)

results = {}
for n in NAMES:
    for attempt in range(3):
        try: results[n] = run(n); print("done", n, results[n], flush=True); break
        except Exception as e: print("retry", n, repr(e)[:200], flush=True)
json.dump(results, open(os.path.join(HERE, "firefox_textruns_partial.json" if "ONLY" in os.environ else "firefox_textruns.json"), "w"), indent=1)
os._exit(0)
