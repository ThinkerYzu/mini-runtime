"""Stage 3: the JS heap and the DOM of pages that run script, mini runtime against Firefox.

usage: python3 bench/compare_js.py [--bin DIR] [--page NAME]
  --bin DIR   directory with mrt-js's release `jscount` (default: mrt-js/target/release)
Reads bench/firefox_js.json (from firefox_js.py, same pages, JS on) and runs jscount itself. Raw mini output
is kept in bench/mrt_js.jsonl. MB = 2^20 bytes.

What is compared:
  JS realm  Firefox js-realm(...) under the window  / mini: the realm of the page's global (JS::CollectRuntimeStats,
            notable classes included)
  JS zone   Firefox js-zone(...) under the top window / mini: the page's zone, the same fields as Firefox's
            ReportZoneStats (the GC buffer allocator's totals left out, as Firefox does)
  DOM       Firefox dom / mini: the arena DOM after the scripts (dom_mb) plus the wrapper cache of the C++ layer
            (Gecko keeps that pointer inside each node, so it is in Firefox's dom)
Style, frames and text are the same as for the static page with the same DOM (compare_memory.py).
"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
from recorded_tests import require_recordings
require_recordings(os.path.join(HERE, 'firefox_js.json'),
                   record='FIREFOX_BIN=... ./mach python /path/to/bench/firefox_js.py')
args = sys.argv[1:]
def opt(name, default=None):
    return args[args.index(name) + 1] if name in args else default
BIN = opt("--bin", os.path.join(HERE, "..", "mrt-js", "target", "release"))
import firefox_state  # FIREFOX_STATE=minimized reads Firefox after "Minimize memory usage" (GAPS A17)
ff = firefox_state.test_pages(json.load(open(os.path.join(HERE, "firefox_js.json"))))
pages = [opt("--page")] if opt("--page") else [p for p in ff if os.path.exists(os.path.join(HERE, "..", "pages", p + ".html"))]

rows, raw = [], []
for p in pages:
    out = subprocess.run([os.path.join(BIN, "jscount"), os.path.join(HERE, "..", "pages", p + ".html")], capture_output=True, text=True).stdout
    m = json.loads(out.strip().splitlines()[-1]); raw.append(m)
    c = ff[p]["live"]["cats"]
    mini = {"JS realm": m["js_realm"]["total"], "JS zone": m["js_zone"]["total"], "DOM": m["dom_mb"] + m["wrapper_cache_mb"]}
    fire = {"JS realm": c.get("js-realm", 0), "JS zone": c.get("js-zone", 0), "DOM": c.get("dom", 0)}
    mini["JS + DOM"] = sum(mini.values()); fire["JS + DOM"] = sum(fire.values())
    rows.append((p, m, mini, fire))

with open(os.path.join(HERE, "mrt_js.jsonl"), "w") as f:
    for m in raw: f.write(json.dumps(m) + "\n")

print("| Page | Layer | Mini (MB) | Firefox (MB) | Mini / Firefox |")
print("|---|---|---:|---:|---:|")
for p, m, mini, fire in rows:
    for k in mini:
        ratio = f"{mini[k] / fire[k]:.2f}x" if fire[k] else "-"
        print(f"| {p} | {k} | {mini[k]:.2f} | {fire[k]:.2f} | {ratio} |")
print()
for p, m, mini, fire in rows:
    print(f"{p}: elements {m['elements']} (Firefox {ff[p]['elements']}), height {m['document_height_px']} (Firefox {ff[p]['height']}), "
          f"reflectors alive {m['reflectors']} ({m['preserved']} preserved), zone buffer slack not counted {m['js_zone_buffer_slack_mb']:.2f} MB, "
          f"script {m['script_ms']} ms")
