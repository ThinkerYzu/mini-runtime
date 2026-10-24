"""Firefox side of the per-element layout comparison: the border box of every element that has a box.

For each page in ../pages, load it in a headless Firefox (JS off, images blocked, web fonts off, the same
prefs as firefox_static.py) at a 1489 x 844 viewport and write bench/firefox_boxes/<page>.json:

  {"page", "viewport": [w, h], "scrollHeight", "n": element count (querySelectorAll('*') order),
   "tags": [tag per element index], "parent": [parent element index or -1],
   "boxes": [[index, x, y, width, height, display, id, class, in_closed_details], ...]}

The index is the element's position in document order among all elements; `layoutcount --boxes` numbers
elements the same way. Rects are getBoundingClientRect() plus the scroll offsets (CSS px, document
coordinates). in_closed_details is 1 for an element in the body of a closed <details>: Firefox keeps boxes for
it (content-visibility: hidden) that take no space on the page. An element without a box (display: none or contents, or inside such a subtree:
getClientRects() is empty) is left out of `boxes`.

usage (from the Firefox tree, MOZCONFIG=mozconfig-hib):
  FIREFOX_BIN=$PWD/obj-hib/dist/bin/firefox ./mach python /path/to/bench/firefox_boxes.py
  ONLY=hackernews,commons restricts the pages.
"""
import functools, http.server, json, os, sys, threading, time
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))  # bench/, for firefox_exit
import firefox_exit
from marionette_driver.marionette import Marionette

HERE = os.path.dirname(os.path.abspath(__file__))
PAGES = os.environ["OPAGES"]
OUT = os.environ["OOUT"]
BIN = os.environ["FIREFOX_BIN"]
NAMES = os.environ.get("ONLY", "synthetic,hackernews,commons,mdn-css,wikipedia,wiki-long").split(",")
VIEW_W, VIEW_H = 1489, 844
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
    "javascript.enabled": False, "permissions.default.image": 2, "gfx.downloadable_fonts.enabled": False,
    # overlay scrollbars take no width, so the layout viewport is the 1489 CSS px the mini runtime lays out at
    "ui.useOverlayScrollbars": 1,
}

BOXES_JS = """
const els = document.querySelectorAll('*');
const index = new Map();
for (let i = 0; i < els.length; i++) index.set(els[i], i);
const sx = window.scrollX, sy = window.scrollY, r2 = (v) => Math.round(v * 100) / 100;
const tags = [], parent = [], boxes = [];
for (let i = 0; i < els.length; i++) {
  const e = els[i];
  tags.push(e.localName);
  const p = e.parentElement;
  parent.push(p ? index.get(p) : -1);
  if (e.getClientRects().length == 0) continue;
  const r = e.getBoundingClientRect();
  const cls = (e.getAttribute('class') || '').trim().replace(/\\s+/g, '.').slice(0, 60);
  let hidden = 0;
  const d = p ? p.closest('details:not([open])') : null;
  if (d) { const s = Array.from(d.children).find((c) => c.localName == 'summary'); hidden = (s && s.contains(e)) ? 0 : 1; }
  boxes.push([i, r2(r.left + sx), r2(r.top + sy), r2(r.width), r2(r.height),
              getComputedStyle(e).display, (e.getAttribute('id') || '').slice(0, 60), cls, hidden]);
}
return {innerWidth: window.innerWidth, innerHeight: window.innerHeight,
        scrollHeight: document.documentElement.scrollHeight, n: els.length, tags, parent, boxes};
"""

def set_viewport(m):
    """Resize the window until the content area is VIEW_W x VIEW_H CSS px."""
    m.set_window_rect(width=VIEW_W + 40, height=VIEW_H + 120)
    for _ in range(4):
        iw, ih = m.execute_script("return [window.innerWidth, window.innerHeight]")
        if (iw, ih) == (VIEW_W, VIEW_H): return
        r = m.window_rect
        m.set_window_rect(width=r["width"] + VIEW_W - iw, height=r["height"] + VIEW_H - ih)

def run(name):
    m = Marionette(bin=BIN, headless=True, prefs=PREFS)
    m.start_session()
    try:
        m.set_context("content")
        m.timeout.script = 120
        set_viewport(m)
        m.navigate(f"http://localhost:{PORT}/{name}.html"); time.sleep(5)
        d = m.execute_script(BOXES_JS)
        assert (d["innerWidth"], d["innerHeight"]) == (VIEW_W, VIEW_H), (d["innerWidth"], d["innerHeight"])
        return {"page": name, "viewport": [d["innerWidth"], d["innerHeight"]], "scrollHeight": d["scrollHeight"],
                "n": d["n"], "tags": d["tags"], "parent": d["parent"], "boxes": d["boxes"]}
    finally:
        firefox_exit.close(m)

os.makedirs(OUT, exist_ok=True)
for n in NAMES:
    for attempt in range(3):
        try:
            r = run(n)
            with open(os.path.join(OUT, n + ".json"), "w") as f: json.dump(r, f, separators=(",", ":"))
            print("done", n, r["n"], "elements,", len(r["boxes"]), "boxes, scrollHeight", r["scrollHeight"], flush=True)
            break
        except Exception as e: print("retry", n, repr(e)[:200], flush=True)
os._exit(0)
