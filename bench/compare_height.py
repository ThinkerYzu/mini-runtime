"""Page height: the mini runtime (layoutcount) against Firefox (firefox_height.json), SPEC criterion 3.

usage: python3 bench/compare_height.py [path/to/layoutcount]
Runs layoutcount on the six pages (1489 x 844 CSS px), writes bench/mrt_layout_height.jsonl and prints the
table. Firefox's numbers come from `HEIGHT_ONLY=1 HEADLESS=1 ... ./mach python bench/firefox_static.py`.
"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
from recorded_tests import require_recordings
require_recordings(os.path.join(HERE, 'firefox_height.json'),
                   record='HEIGHT_ONLY=1 HEADLESS=1 FIREFOX_BIN=... ./mach python /path/to/bench/firefox_static.py')
BIN = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~/.cache/mrt-tgt-F/release/layoutcount")
PAGES = ["synthetic", "hackernews", "commons", "mdn-css", "wikipedia", "wiki-long"]
ff = json.load(open(os.path.join(HERE, "firefox_height.json")))

rows = []
with open(os.path.join(HERE, "mrt_layout_height.jsonl"), "w") as out:
    for p in PAGES:
        r = subprocess.run([BIN, os.path.join(HERE, "..", "pages", p + ".html")], capture_output=True, text=True)
        line = r.stdout.splitlines()[0] if r.stdout else '{"file":"%s","outcome":"no output"}' % p
        out.write(line + "\n")
        rows.append(json.loads(line))

print(f"{'page':11s} {'mini px':>9s} {'Firefox px':>10s} {'delta':>8s}  note")
for d in rows:
    name = d["file"]
    f = ff[name]["height"]["scrollHeight"]
    if d["outcome"] != "ok":
        print(f"{name:11s} {'-':>9s} {f:10d} {'-':>8s}  {d['outcome']}: {d.get('message', '')} at {d.get('at', '')}")
        continue
    m = d["document_height_px"]
    pct = (m - f) / f * 100
    print(f"{name:11s} {m:9.0f} {f:10d} {pct:+7.1f}%  {'within 10%' if abs(pct) <= 10 else 'outside 10%'}")
