"""Compare the HTTP viewer's transports on the same workloads (DESIGN "WebSocket transport").

VIEWER_BROWSER is implied; run under Xvfb with Marionette importable:
FIREFOX_BIN=... xvfb-run -a .../python bench/viewer_transport_compare.py [OUT.json]

Each trial starts a Viewer and Service with no runtime, opens Firefox on the viewer page with
?transport=ws or poll, and stands in for the runtime: it drains input and echoes each keydown into
the mirrored page. Workloads (TRIAL_S seconds each, TRIALS times):
- idle: attached, no input and no DOM changes
- active: a 200-byte text change at 20 Hz, and typing ten keys a second
Measured over the workload only: API calls and bytes from the Service's counters, the CPU time and
context switches of this process (the service, the stand-in runtime and the Marionette client) and of
every Firefox process, and keydown-to-mirror latency (the browser's capture time to the echo's DOM
mutation in the viewer's frame, both Date.now(), so 1 ms resolution).
"""
import json
import os
from pathlib import Path
import resource
import statistics
import sys
import threading
import time

from http_viewer import Service, Viewer

TRIAL_S = float(os.environ.get("TRIAL_S", "20"))
TRIALS = int(os.environ.get("TRIALS", "3"))
TICK = os.sysconf("SC_CLK_TCK")
COUNTERS = ("http_api_calls", "http_api_bytes_in", "http_api_bytes_out", "ws_calls", "ws_bytes_in", "ws_bytes_out")
PAGE = {"i": 1, "n": "html", "a": [], "c": [
    {"i": 2, "n": "head", "a": [], "c": []},
    {"i": 3, "n": "body", "a": [], "c": [
        {"i": 4, "n": "input", "a": [["id", "field"]], "c": []},
        {"i": 5, "n": "p", "a": [["id", "echo"]], "c": [{"i": 6, "t": "0:0"}]},
        {"i": 7, "n": "p", "a": [["id", "ticker"]], "c": [{"i": 8, "t": ""}]}]}]}


def process_tree(root):
    """`root` and its descendants' PIDs"""
    children = {}
    for entry in os.listdir("/proc"):
        if entry.isdigit():
            try:
                parent = int(Path(f"/proc/{entry}/stat").read_text().rsplit(")", 1)[1].split()[1])
            except (OSError, IndexError):
                continue
            children.setdefault(parent, []).append(int(entry))
    out, todo = [], [root]
    while todo:
        pid = todo.pop()
        out.append(pid)
        todo.extend(children.get(pid, []))
    return out


def browser_usage(pids):
    """(CPU seconds, context switches) of these processes, all their threads"""
    cpu = switches = 0
    for pid in pids:
        try:
            fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
            cpu += (int(fields[11]) + int(fields[12])) / TICK
            for tid in os.listdir(f"/proc/{pid}/task"):
                for line in Path(f"/proc/{pid}/task/{tid}/status").read_text().splitlines():
                    if "ctxt_switches:" in line:
                        switches += int(line.split()[1])
        except (OSError, IndexError):
            continue
    return cpu, switches


def own_usage():
    r = resource.getrusage(resource.RUSAGE_SELF)
    return r.ru_utime + r.ru_stime, r.ru_nvcsw + r.ru_nivcsw


def trial(transport, workload):
    from marionette_driver.marionette import Marionette
    import firefox_exit
    v = Viewer("demo", "https://example.test/", [800, 600])
    service = Service("127.0.0.1:0", v).start()
    browser = Marionette(bin=os.environ["FIREFOX_BIN"], prefs={"marionette.allowSystemAccess": True,
        "browser.aboutwelcome.enabled": False, "browser.shell.checkDefaultBrowser": False})
    stop = threading.Event()

    def runtime():
        """The stand-in runtime: drain input, echo keydowns"""
        while not stop.is_set():
            for e in v.wait({"now": 0, "busy": False})["ok"]["events"]:
                if e.get("type") == "keydown":
                    v.publish([["d", 6, f"{e['task']}:{e['at']}"]])
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
        browser.execute_script("""
            const doc = document.getElementById("page").contentDocument, out = document.getElementById("status");
            out.dataset.lat = "";
            new doc.defaultView.MutationObserver(records => {
                for (const r of records) {
                    const text = r.target.data;
                    if (r.target.parentNode && r.target.parentNode.id === "echo" && text !== "0:0")
                        out.dataset.lat += (Date.now() - Number(text.split(":")[1])) + ",";
                }
            }).observe(doc, {subtree: true, characterData: true});
        """)
        if workload == "active":
            browser.switch_to_frame(browser.find_element("id", "page"))
            field = browser.find_element("id", "field")
            field.click()
        time.sleep(1)  # settle
        pids = process_tree(browser.instance.runner.process_handler.pid)
        before_stats = {k: v.stats[k] for k in COUNTERS}
        before_own, before_browser = own_usage(), browser_usage(pids)
        started = time.monotonic()
        tick = keys = 0
        while time.monotonic() - started < TRIAL_S:
            if workload == "active":
                tick += 1
                v.publish([["d", 8, f"{tick:06d}" + "x" * 194]])
                if tick % 2 == 0:
                    field.send_keys("a")
                    keys += 1
                time.sleep(max(0, started + tick * 0.05 - time.monotonic()))
            else:
                time.sleep(0.1)
        elapsed = time.monotonic() - started
        after_own, after_browser = own_usage(), browser_usage(pids)
        result = {k: v.stats[k] - before_stats[k] for k in COUNTERS}
        if workload == "active":
            browser.switch_to_frame()
            time.sleep(0.5)
        lat = [int(x) for x in browser.execute_script('return document.getElementById("status").dataset.lat').split(",") if x]
        result.update(seconds=round(elapsed, 2), keys=keys, updates=tick,
                      service_cpu_s=round(after_own[0] - before_own[0], 3), service_switches=after_own[1] - before_own[1],
                      browser_cpu_s=round(after_browser[0] - before_browser[0], 3), browser_switches=after_browser[1] - before_browser[1],
                      latency_ms=lat)
        return result
    finally:
        stop.set()
        firefox_exit.close(browser)
        service.close()


def summary(trials):
    out = {}
    for key in trials[0]:
        if key == "latency_ms":
            lat = sorted(x for t in trials for x in t[key])
            if lat:
                out["latency_median_ms"] = statistics.median(lat)
                out["latency_p95_ms"] = lat[min(len(lat) - 1, int(len(lat) * 0.95))]
                out["latency_samples"] = len(lat)
        else:
            out[key] = statistics.median(t[key] for t in trials)
    calls = out["http_api_calls"] + out["ws_calls"]
    out["calls_per_s"] = round(calls / out["seconds"], 2)
    out["bytes_per_s"] = round(sum(out[k] for k in COUNTERS if "bytes" in k) / out["seconds"])
    return out


def main():
    results = {}
    for workload in ("idle", "active"):
        for transport in ("poll", "ws"):
            runs = []
            for i in range(TRIALS):
                runs.append(trial(transport, workload))
                print(workload, transport, i, {k: x for k, x in runs[-1].items() if k != "latency_ms"}, file=sys.stderr, flush=True)
            results[f"{workload}/{transport}"] = {"trials": runs, "median": summary(runs)}
    out = sys.argv[1] if len(sys.argv) > 1 else None
    if out:
        Path(out).write_text(json.dumps(results, indent=1))
    keys = ("calls_per_s", "bytes_per_s", "service_cpu_s", "service_switches", "browser_cpu_s", "browser_switches",
            "latency_median_ms", "latency_p95_ms")
    print("| workload/transport | " + " | ".join(keys) + " |")
    print("|---" * (len(keys) + 1) + "|")
    for name, r in results.items():
        print(f"| {name} | " + " | ".join(str(r["median"].get(k, "")) for k in keys) + " |")


if __name__ == "__main__":
    main()
