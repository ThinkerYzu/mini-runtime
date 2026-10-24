"""Stage 6: Firefox's cost for a real page against the floor of the page's essential state.

For each site, from the phase 0a runs of bench/firefox_realsites.py and the DOM files it wrote:
  cost       the whole browser's unique memory with the page minus with a blank tab (median of the runs)
  floor      the page's JS heap in Firefox (js-realm + js-zone of the top-level process's windows; the
             mini runtime runs the same SpiderMonkey) plus the mini DOM's live bytes after parsing
             Firefox's finished DOM (bench/realsites-dom/<site>.html, shadow trees included)
  frames     the unique memory of the content processes other than the site's own (cross-site frames);
             in stage 6's design they stay in Firefox behind the bridge, so they are listed apart
  overhead   cost - floor - frames
The faithfulness check compares the mini DOM's element count (template contents included) with
Firefox's: the document's elements, the shadow trees' elements and one template per shadow root.

usage: python3 bench/compare_realsites.py [results.json ...]   (default: firefox_realsites_0a_xvfb_run*.json)
"""
import glob, json, os, statistics, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
from recorded_tests import require_recordings
require_recordings(*(sys.argv[1:] or [os.path.join(HERE, "firefox_realsites_0a_xvfb_run*.json")]),
                   record="FIREFOX_BIN=... ./mach python /path/to/bench/firefox_realsites.py; pass the resulting report path")
DOMCOUNT = os.path.join(HERE, "..", "mrt-dom", "target", "release", "domcount")
ORDER = ["facebook", "instagram", "gdocs", "gmail", "x", "youtube", "gmaps", "reddit", "amazon", "cnn"]

files = sys.argv[1:] or sorted(glob.glob(os.path.join(HERE, "firefox_realsites_0a_xvfb_run*.json")))
import firefox_state  # FIREFOX_STATE=minimized reads Firefox after "Minimize memory usage" (GAPS A17)
runs = [{s: firefox_state.real_site(v) for s, v in json.load(open(f)).items()} for f in files]
mb = lambda b: b / 1048576


def top_window(v):
    """The top-level process's window categories, from the page's live memory report."""
    return v["live"]["processes"][v["live"]["top_process"]]["window"]


def frames_uss(v):
    """USS of the content processes with the page other than the site's own and the spare one."""
    top = v["live"]["top_process"]
    return sum(p["uss"] for k, p in v["browser"]["live"]["processes"].items()
               if k != top and p["kind"].startswith("webIsolated="))


print(f"{len(runs)} runs: " + ", ".join(os.path.basename(f) for f in files))
print("| Site | Runs | Cost | Page's windows | JS | Firefox DOM | Mini DOM | Floor | Frames | Overhead | Floor / cost | Elements, mini / Firefox |")
print("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|")
for site in ORDER:
    vs = [r[site] for r in runs if site in r and "browser" in r[site]]
    if not vs:
        print(f"| {site} | 0 | | | | | | | | | | |")
        continue
    med = lambda f: statistics.median(f(v) for v in vs)
    cost = med(lambda v: v["browser"]["page_cost"]["uss"])
    windows = med(lambda v: v["live"]["processes"][v["live"]["top_process"]]["window_total"])
    js = med(lambda v: top_window(v).get("js-realm", 0) + top_window(v).get("js-zone", 0))
    ff_dom = med(lambda v: top_window(v).get("dom", 0))
    frames = med(frames_uss)
    # the DOM file is the last run's (each run overwrites it); its Firefox counts come from that run
    last = [v for v in vs if "dom_dump" in v][-1:]
    mini, check = None, ""
    if last:
        dd = last[0]["dom_dump"]
        out = subprocess.run([DOMCOUNT, "--json", os.path.join(HERE, dd["file"])], capture_output=True, text=True, check=True)
        mini = json.loads(out.stdout.strip().splitlines()[-1])
        ff_elements = dd["elements"] + dd["shadowElements"] + dd["shadowRoots"]
        ratio = mini["elements_all"] / ff_elements
        check = f"{mini['elements_all']:,} / {ff_elements:,}" + ("" if abs(ratio - 1) <= 0.05 else " **outside 5%**")
    mini_dom = mb(mini["live"]) if mini else float("nan")
    floor = js + mini_dom
    overhead = cost - floor - frames
    print(f"| {site} | {len(vs)} | {cost:.0f} | {windows:.0f} | {js:.0f} | {ff_dom:.1f} | {mini_dom:.1f} | {floor:.0f} | {frames:.0f} | {overhead:.0f} | {floor / cost:.0%} | {check} |")
print("\nMB, unique memory (USS). Cost, windows, JS, Firefox DOM, frames: median of the runs.")
