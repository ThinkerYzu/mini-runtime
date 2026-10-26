"""Where a keydown's echo spends its time on each viewer transport (task 61692).

FIREFOX_BIN=... xvfb-run -a .../python bench/viewer_latency_stages.py [OUT.json]

The active workload of viewer_transport_compare.py (a 20 Hz text change and ten keys a second, the
stand-in runtime echoing each keydown), with every stage of a keydown's trip stamped on the wall
clock (the browser's Date.now() and time.time(), one clock, 1 ms resolution in the browser):
  capture   the viewer's capture of the keydown (its `at`)
  sent      the browser's send of the input call carrying it (WebSocket send or fetch)
  received  Service.call("input") starting on it
  taken     the stand-in runtime's wait returning it
  published the runtime's publish of its echo
  answered  Service.call("poll") returning the echo's batch
  delivered the browser's WebSocket message, or the fetch's response headers, carrying it
  applied   the echo's DOM mutation in the viewer's frame
The browser's stamps come from a hook served in front of viewer.js; viewer.js itself is unchanged.
Each input call is also classed by what sent it: "poll" when a poll answer was delivered in the
5 ms before it, else "timer" (the 30 ms input timer, or the 1 s lease timer).
"""
import json
import os
from pathlib import Path
import shutil
import statistics
import sys
import tempfile
import threading
import time

import http_viewer
from http_viewer import Service, Viewer
from viewer_transport_compare import PAGE

TRIAL_S = float(os.environ.get("TRIAL_S", "20"))
TRIALS = int(os.environ.get("TRIALS", "3"))
TRANSPORTS = os.environ.get("TRANSPORTS", "poll,ws").split(",")
STAGES = ("capture", "sent", "received", "taken", "published", "answered", "delivered", "applied")

# (prepended to viewer.js: stamps the outer page's API traffic and the echo's mutations)
HOOK = r"""
(() => {
  const log = {sent: [], delivered: [], polls: [], applied: []};
  // (called by the driver once the mirror is built; Marionette's scripts see the page through Xrays)
  window.__observe = () => {
    const doc = document.getElementById("page").contentDocument;
    new doc.defaultView.MutationObserver(records => {
      const t = Date.now();
      for (const r of records)
        if (r.target.parentNode && r.target.parentNode.id === "echo" && r.target.data !== "0:0")
          log.applied.push([t, ...r.target.data.split(":").map(Number)]);
    }).observe(doc, {subtree: true, characterData: true});
  };
  window.__dump = () => JSON.stringify(log);
  const keydowns = body => (body.events || []).filter(e => e.type === "keydown").map(e => e.task);
  const echoes = answer => {
    const out = [];
    for (const b of answer.batches || []) for (const op of b.ops) if (op[0] === "d" && op[1] === 6) out.push(Number(op[2].split(":")[0]));
    return out;
  };
  const sentAt = (op, body) => { if (op === "input") log.sent.push([Date.now(), keydowns(body)]); };
  const deliveredAt = (t, op, answer) => {
    if (op !== "poll") return;
    log.polls.push(t);
    for (const task of echoes(answer)) log.delivered.push([t, task]);
  };
  const Socket = window.WebSocket, pending = new Map();
  window.WebSocket = function (...args) {
    const ws = new Socket(...args);
    ws.addEventListener("message", e => {
      const t = Date.now(), answer = JSON.parse(e.data), op = pending.get(answer.id);
      pending.delete(answer.id);
      if (answer.ok) deliveredAt(t, op, answer.ok);
    });
    const send = ws.send.bind(ws);
    ws.send = data => { const call = JSON.parse(data); pending.set(call.id, call.op); sentAt(call.op, call.body); send(data); };
    return ws;
  };
  const fetch0 = window.fetch;
  window.fetch = async (url, init) => {
    const op = String(url).replace(/^\/api\//, "").split("?")[0];
    if (init && init.body) sentAt(op, JSON.parse(init.body));
    const response = await fetch0(url, init), t = Date.now();
    if (op === "poll") response.clone().json().then(answer => deliveredAt(t, op, answer));
    return response;
  };
})();
"""


def trial(transport, assets):
    from marionette_driver.marionette import Marionette
    import firefox_exit
    v = Viewer("demo", "https://example.test/", [800, 600])
    service = Service("127.0.0.1:0", v).start()
    stamps = {}  # task -> {stage: ms}
    sends = []   # service-side input arrivals (ms)
    call0 = service.call

    def stamp(task, stage, t=None):
        stamps.setdefault(task, {}).setdefault(stage, (time.time() if t is None else t) * 1000)

    def call(op, req):
        t = time.time()
        result = call0(op, req)
        if op == "input":
            for e in req.get("events", []):
                if e.get("type") == "keydown":
                    stamp(e["task"], "received", t)
        elif op == "poll":
            done = time.time()
            for b in result.get("batches", []):
                for o in b["ops"]:
                    if o[0] == "d" and o[1] == 6:
                        stamp(int(o[2].split(":")[0]), "answered", done)
        return result
    service.call = call
    browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
        "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
    stop = threading.Event()

    def runtime():
        while not stop.is_set():
            for e in v.wait({"now": 0, "busy": False})["ok"]["events"]:
                if e.get("type") == "keydown":
                    stamp(e["task"], "taken")
                    v.publish([["d", 6, f"{e['task']}:{e['at']}"]])
                    stamp(e["task"], "published")
    try:
        browser.start_session()
        browser.set_context("content")
        browser.navigate(service.url + "&transport=" + transport)
        end = time.monotonic() + 10
        while v.session is None and time.monotonic() < end:
            time.sleep(0.03)
        session = v.session
        v.wait({"now": 0, "busy": True})
        v.publish([["c", 0, [PAGE]]], session)
        thread = threading.Thread(target=runtime, daemon=True)
        thread.start()
        while not browser.execute_script('return document.getElementById("status").textContent.startsWith("Connected")'):
            time.sleep(0.05)
        browser.execute_script("window.wrappedJSObject.__observe()")
        browser.switch_to_frame(browser.find_element("id", "page"))
        field = browser.find_element("id", "field")
        field.click()
        time.sleep(1)
        started = time.monotonic()
        tick = 0
        while time.monotonic() - started < TRIAL_S:
            tick += 1
            v.publish([["d", 8, f"{tick:06d}" + "x" * 194]])
            if tick % 2 == 0:
                field.send_keys("a")
            time.sleep(max(0, started + tick * 0.05 - time.monotonic()))
        browser.switch_to_frame()
        time.sleep(0.5)
        log = json.loads(browser.execute_script("return window.wrappedJSObject.__dump()"))
    finally:
        stop.set()
        firefox_exit.close(browser)
        service.close()
    for t, task, at in log["applied"]:
        stamp(task, "capture", at / 1000)
        stamp(task, "applied", t / 1000)
    for t, task in log["delivered"]:
        stamp(task, "delivered", t / 1000)
    polls = sorted(log["polls"])
    triggers = {}
    for t, tasks in log["sent"]:
        cause = "poll" if any(t - 5 <= p <= t for p in polls) else "timer"
        for task in tasks:
            stamp(task, "sent", t / 1000)
            triggers[task] = cause
    rows = []
    for task, s in stamps.items():
        if all(k in s for k in STAGES):
            rows.append({"task": task, "trigger": triggers.get(task), **{k: s[k] for k in STAGES}})
    return rows


def describe(rows):
    def stats(xs):
        xs = sorted(xs)
        return {"median": round(statistics.median(xs), 1), "p95": round(xs[min(len(xs) - 1, int(len(xs) * 0.95))], 1)} if xs else {}
    out = {"samples": len(rows), "total": stats([r["applied"] - r["capture"] for r in rows])}
    for a, b in zip(STAGES, STAGES[1:]):
        out[f"{a}->{b}"] = stats([r[b] - r[a] for r in rows])
    for cause in ("poll", "timer"):
        sel = [r for r in rows if r["trigger"] == cause]
        out[f"trigger={cause}"] = {"share": round(len(sel) / max(1, len(rows)), 2),
                                   "total": stats([r["applied"] - r["capture"] for r in sel]),
                                   "capture->sent": stats([r["sent"] - r["capture"] for r in sel])}
    return out


def main():
    results = {}
    with tempfile.TemporaryDirectory() as tmp:
        assets = Path(tmp)
        for f in http_viewer.ASSETS.iterdir():
            shutil.copy(f, assets / f.name)
        (assets / "viewer.js").write_text(HOOK + (http_viewer.ASSETS / "viewer.js").read_text())
        http_viewer.ASSETS = assets
        for transport in TRANSPORTS:
            rows = []
            for i in range(TRIALS):
                rows += trial(transport, assets)
                print(transport, i, len(rows), file=sys.stderr, flush=True)
            results[transport] = {"rows": rows, "summary": describe(rows)}
    if len(sys.argv) > 1:
        Path(sys.argv[1]).write_text(json.dumps(results, indent=1))
    for transport, r in results.items():
        print(transport, json.dumps(r["summary"], indent=1))


if __name__ == "__main__":
    main()
