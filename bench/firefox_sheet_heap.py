"""What one style sheet costs Firefox's content process, against what its reporters show (GAPS B94).

Serves three local pages: `blank` (nothing), `text` (the sheet's text in a hidden <div>, so the
document holds the same characters) and `style` (the text in a <style>). Each is loaded in its own
release Firefox, and the page's process is measured with a memory report. `style` minus `text` is the
sheet: its heap-allocated delta, and the reporter rows that explain it (the sheet cache, the stylist,
atoms) with the rest as heap-unclassified. The page reads nothing of the sheet: reading `cssRules`
runs StyleSheet::EnsureUniqueInner, which clones the parsed sheet.

The runtime's side, on the same `style` page: `mrt-style/target/release/stylecount DIR/style.html`
(its `sheets_mb` and `flush_mb`).

usage (from a Firefox source tree, under xvfb-run like the harnesses):
  FIREFOX_BIN=... ./mach python /path/to/bench/firefox_sheet_heap.py SHEET.css OUT_DIR
"""
import collections, gzip, html, http.server, json, os, re, sys, threading
import firefox_exit
from marionette_driver.marionette import Marionette

DUMP = """const [path] = arguments; const done = arguments[arguments.length - 1];
Components.classes["@mozilla.org/memory-info-dumper;1"].getService(Components.interfaces.nsIMemoryInfoDumper)
  .dumpMemoryReportsToNamedFile(path, {callback() { done(true); }}, null, false, false);"""
PREFS = {"marionette.allowSystemAccess": True, "browser.shell.checkDefaultBrowser": False,
         "browser.aboutwelcome.enabled": False}


def pages(css, out):
    """Writes the three pages into OUT/www."""
    www = os.path.join(out, "www")
    os.makedirs(www, exist_ok=True)
    head, tail = "<!doctype html><html><head>", "</head><body><p>x</p>"
    bodies = {
        "blank": head + tail,
        "text": head + "<style>div{display:none}</style>" + tail + "<div>" + html.escape(css, quote=False) + "</div>",
        "style": head + "<style>" + css + "</style>" + tail,
    }
    for name, body in bodies.items():
        with open(os.path.join(www, name + ".html"), "w") as f:
            f.write(body)
    return www


def measure(url, path):
    """Loads `url` in a new Firefox and dumps its memory reports to `path`."""
    m = Marionette(bin=os.environ["FIREFOX_BIN"], prefs=PREFS)
    m.start_session()
    try:
        m.navigate(url)
        m.set_context("chrome")
        m.timeout.script = 180
        m.execute_async_script(DUMP, script_args=(path,), sandbox="system")
    finally:
        firefox_exit.close(m)


def rows(path):
    """The page's process: heap-allocated, and its explicit heap rows by path (ids taken out)."""
    c, heap = collections.Counter(), 0
    for r in json.load(gzip.open(path))["reports"]:
        if not r["process"].startswith("webIsolated=http://127.0.0.1"):
            continue
        if r["path"] == "heap-allocated":
            heap = r["amount"]
        if r["path"].startswith("explicit/") and r["kind"] == 1 and r["units"] == 0:
            c[re.sub(r"\(.*?\)", "()", r["path"])] += r["amount"]
    c["heap-unclassified"] = heap - sum(c.values())
    return c, heap


def main():
    css, out = open(sys.argv[1]).read(), os.path.abspath(sys.argv[2])
    www = pages(css, out)
    handler = lambda *a: http.server.SimpleHTTPRequestHandler(*a, directory=www)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        for name in ("blank", "text", "style"):
            measure(f"http://127.0.0.1:{server.server_port}/{name}.html", os.path.join(out, name + ".json.gz"))
    finally:
        server.shutdown()
    result = {}
    for a, b in (("blank", "text"), ("text", "style")):
        (ra, ha), (rb, hb) = rows(os.path.join(out, a + ".json.gz")), rows(os.path.join(out, b + ".json.gz"))
        delta = {k: round((rb[k] - ra[k]) / 1e6, 2) for k in set(ra) | set(rb) if abs(rb[k] - ra[k]) >= 10_000}
        result[f"{b} - {a}"] = {"heap_allocated_mb": round((hb - ha) / 1e6, 2),
                                "rows_mb": dict(sorted(delta.items(), key=lambda kv: -kv[1]))}
    print(json.dumps(result, indent=1))
    with open(os.path.join(out, "sheet_heap.json"), "w") as f:
        json.dump(result, f, indent=1)


if __name__ == "__main__":
    main()
