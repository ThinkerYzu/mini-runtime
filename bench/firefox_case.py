"""Run a binding case in Firefox: the console lines a case's script prints, to compare with what
tests/bindings.rs expects from the mini runtime.

The case is the body of a page, as in `run_case` (tests/bindings.rs): it is put in the same
skeleton and served from a local HTTP server. A script in the head collects `console.log` lines
(the arguments as strings, joined by spaces, as the mini runtime's console prints primitives) and
uncaught errors ("ERROR <message>"); they are read one second after the load.

usage (from the Firefox tree):
  FIREFOX_BIN=$HOME/.cache/mrt-firefox-release/firefox/firefox ./mach python /path/to/bench/firefox_case.py CASE.html [...]
"""
import functools, http.server, os, shutil, sys, tempfile, threading, time
import firefox_exit
from marionette_driver.marionette import Marionette

BIN = os.environ["FIREFOX_BIN"]
CAPTURE = """<script>
window.__mrtLog = [];
console.log = (...a) => window.__mrtLog.push(a.map(String).join(" "));
addEventListener("error", e => window.__mrtLog.push("ERROR " + e.message));
addEventListener("unhandledrejection", e => window.__mrtLog.push("ERROR " + e.reason));
</script>"""

def main():
    root = tempfile.mkdtemp(prefix="mrt-case-")
    names = []
    for i, path in enumerate(sys.argv[1:]):
        body = open(path).read()
        name = f"case{i}.html"
        with open(os.path.join(root, name), "w") as f:
            f.write(f"<!doctype html><html><head><title>t</title>{CAPTURE}</head><body>{body}</body></html>")
        names.append((path, name))
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=root)
    handler.func.log_message = lambda *a, **k: None
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    m = Marionette(bin=BIN, headless=True, prefs={"marionette.allowSystemAccess": True, "network.trr.mode": 5,
                   "app.update.disabledForTesting": True, "browser.shell.checkDefaultBrowser": False,
                   "datareporting.policy.dataSubmissionEnabled": False, "toolkit.telemetry.enabled": False})
    m.start_session()
    try:
        for path, name in names:
            m.navigate(f"http://127.0.0.1:{srv.server_address[1]}/{name}")
            time.sleep(1)
            print(f"== {path}")
            for line in m.execute_script("return Array.from(window.wrappedJSObject.__mrtLog)"):
                print(line)
    finally:
        firefox_exit.close(m)
        srv.shutdown()
        shutil.rmtree(root, ignore_errors=True)

main()
