"""The string census of a replayed site's workers in release Firefox (GAPS B89): the counterpart of the
runtime's MRT_WORKER_CENSUS. Loads the site (no scripted use), waits, then runs bench/worker_census.js
in each worker's debugger global (nsIWorkerDebugger.initialize, with the worker's global as `target`)
from a process script in every content process, and prints one JSON line a worker.

A debugger script must be a chrome: or resource: URL, so the script is written to a directory mapped
as resource://mrtcensus/; the content processes read it from there, which the content sandbox does
not allow: this run starts Firefox with MOZ_DISABLE_CONTENT_SANDBOX=1. Only for this measurement.

usage (from a Firefox source tree, under xvfb-run like the harnesses):
  REPLAY=replay FIREFOX_BIN=... ./mach python /path/to/bench/firefox_worker_census.py SITE URL [MATCH]
  MATCH: a regexp on the worker's URL or name (default: every worker); WAIT=20 (seconds after load);
  LIST_ONLY=1: list the workers' debuggers, no census; TRIVIAL=1: a debugger script that only answers;
  STAGE=N: worker_census.js's censusStage. (A heap snapshot of a worker, ChromeUtils.saveHeapSnapshot in
  its debugger global, crashes release Firefox's content process: MOZ_RELEASE_ASSERT "not on worker
  thread!", session 84.)
"""
import json, os, shutil, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import replay
from firefox_probe import PREFS, VIEW_W, VIEW_H
import firefox_exit
from marionette_driver.marionette import Marionette

# In the parent: the resource substitution, a listener for the answers, and the process script that
# finds each worker's debugger in its process and starts the census there.
CHROME = r"""
const [dirURL, match, listOnly] = arguments;
const res = Services.io.getProtocolHandler("resource").QueryInterface(Ci.nsIResProtocolHandler);
res.setSubstitution("mrtcensus", Services.io.newURI(dirURL));
// kept on Services, which outlives a chrome window: a long wait in an async script lost its window
// ("Document was unloaded"); Python polls POLL until `done`
const results = Services.__mrtCensus = [];
const listener = m => results.push(m.data);
Services.ppmm.addMessageListener("mrt-census", listener);
const script = `(${function (match, listOnly) {
  const wdm = Cc["@mozilla.org/dom/workers/workerdebuggermanager;1"].getService(Ci.nsIWorkerDebuggerManager);
  const re = new RegExp(match);
  for (const d of wdm.getWorkerDebuggerEnumerator()) {
    if (d.isClosed || d.isChrome || !(re.test(d.url) || re.test(d.name))) continue;
    const who = { url: d.url, name: d.name, type: d.type, remote: d.isRemote, initialized: d.isInitialized };
    if (listOnly) { sendAsyncMessage("mrt-census", Object.assign({ listed: true }, who)); continue; }
    d.addListener({
      onMessage(data) { sendAsyncMessage("mrt-census", Object.assign({ census: data }, who)); },
      onError(file, line, message) { sendAsyncMessage("mrt-census", Object.assign({ error: message + " " + file + ":" + line }, who)); },
      onClose() {},
    });
    try { d.initialize("resource://mrtcensus/debugger.js"); sendAsyncMessage("mrt-census", Object.assign({ started: true }, who)); }
    catch (e) { sendAsyncMessage("mrt-census", Object.assign({ error: String(e) }, who)); }
  }
}})(${JSON.stringify(match)}, ${listOnly})`;
Services.ppmm.loadProcessScript("data:," + encodeURIComponent(script), false);
// the census of a large worker takes seconds on its thread
return true;
"""

POLL = r"""
const results = Services.__mrtCensus || [];
const started = results.filter(r => r.started).length;
const done = results.filter(r => r.census || r.error).length;
return { done: started > 0 && done >= started, results };
"""


def main():
    site, url = sys.argv[1], sys.argv[2]
    match = sys.argv[3] if len(sys.argv) > 3 else "."
    census = open(os.path.join(HERE, "worker_census.js")).read()
    tmp = tempfile.mkdtemp(prefix="mrt-census-")
    with open(os.path.join(tmp, "debugger.js"), "w") as f:
        # the debugger global's `global` is the worker's
        # TRIVIAL=1: a script that only answers, to check the debugger path
        f.write("postMessage('hello ' + typeof Debugger + ' ' + typeof global);\n" if os.environ.get("TRIVIAL") == "1"
                else "const target = global;\n" + (f"const censusStage = {int(os.environ['STAGE'])};\n" if os.environ.get("STAGE") else "")
                + "postMessage(" + census + ");\n")
    os.environ["MOZ_DISABLE_CONTENT_SANDBOX"] = "1"
    proxy = replay.start(site, "probe")
    m = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={**PREFS, **(proxy.prefs if proxy else {})},
                   profile=os.environ.get("REALSITES_PROFILE", os.path.expanduser("~/.cache/mrt-realsites-profile")))
    m.start_session(proxy.capabilities if proxy else None)
    try:
        m.set_context("content")
        m.set_window_rect(width=VIEW_W + 40, height=VIEW_H + 120)
        m.timeout.page_load = 120
        t0 = time.time()
        m.navigate(url)
        print(f"loaded after {time.time() - t0:.1f} s", flush=True)
        time.sleep(float(os.environ.get("WAIT", "20")))
        with m.using_context("chrome"):
            m.execute_script(CHROME, script_args=["file://" + tmp + "/", match, os.environ.get("LIST_ONLY") == "1"])
        # the census of a large worker takes seconds to minutes on its thread
        deadline = time.time() + float(os.environ.get("CENSUS_LIMIT", "400"))
        while True:
            time.sleep(3)
            with m.using_context("chrome"):
                state = m.execute_script(POLL)
            if state["done"] or time.time() > deadline or (os.environ.get("LIST_ONLY") == "1"):
                results = state["results"]
                break
        print(f"census at {time.time() - t0:.1f} s", flush=True)
        for r in results:
            if "started" in r:
                continue
            print(json.dumps(r), flush=True)
    finally:
        firefox_exit.close(m)
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
