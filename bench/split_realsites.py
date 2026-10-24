"""Stage 6: what of a real page's cost could be dropped while keeping the page's state.

Reads the kept memory reports of bench/firefox_realsites.py (KEEP_REPORTS=1, bench/realsites-reports/)
and its results (EXTRA=1), and splits the site's content process by memory-report path into:
  state       the page's JS heap and DOM (the floor), the JS memory the process keeps for the page's code
              outside its windows (bytecode, script sources, atoms), and workers' heaps
  rebuildable layout and style, decoded images, media, graphics: derived from the page and its resources
  free        GC heap bookkeeping and unused GC chunks or arenas, allocator slack (mozjemalloc's bin-unused,
              bookkeeping and dirty pages: heap/committed/... outside explicit/ in release 157)
  fixed       per-process runtime (XPConnect, ICU, Gecko atoms, preferences, ...): paid once per process
  unknown     heap the report does not attribute (heap-allocated minus the heap reports), and unique
              memory outside every report (code relocations, stacks, unreported mappings); split further
              by bench/ff_unknown.py (task 178d)
The other processes come from the result file's cost by kind: the main process and media decoding are
rebuildable (caches, compositor, decoders), service workers are dropped by Firefox when idle, cross-site
frames are listed apart (other pages' state).

usage: python3 bench/split_realsites.py [results.json]   (default bench/firefox_realsites_extra.json)
  REPORTS_DIR=dir         the run's kept reports under bench/ (default realsites-reports)
  FIREFOX_STATE=minimized the page after "Minimize memory usage" (<site>-live-minimized and
                          browser_minimized; a run without EXTRA=1, session 39 or later); default plain
"""
import gzip, json, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import firefox_state
REPORTS = os.path.join(HERE, os.environ.get("REPORTS_DIR", "realsites-reports"))
ORDER = ["facebook", "instagram", "gdocs", "gmail", "x", "youtube", "gmaps", "reddit", "amazon", "cnn"]
HEAP = 1  # the memory report's KIND_HEAP

# explicit/ subtrees of the site's process, first match wins; the path is matched with each "(...)"
# argument removed (window(...), js-zone(...), worker(...), ...)
RULES = [
    (r"^window-objects/.*/js-zone/(.*/)?(unused-gc-things|gc-heap-arena-admin)", "free"),
    (r"^window-objects/.*/(js-realm|js-zone|dom)(/|$)", "state"),
    (r"^window-objects/.*/(layout|style)(/|$)", "rebuildable"),
    (r"^window-objects/", "state"),
    (r"^js-non-window/gc-heap/", "free"),
    (r"^js-non-window/zones/.*/(unused-gc-things|gc-heap-arena-admin)", "free"),
    (r"^js-non-window/runtime/(script-data|shared-immutable-strings-cache|atoms-table|script-sources)", "state"),
    (r"^js-non-window/zones/", "state"),
    (r"^js-non-window/", "fixed"),
    (r"^workers/.*/(unused-gc-things|gc-heap-arena-admin|gc-heap)", "free"),
    (r"^workers/", "state"),
    (r"^(images|media|gfx|layout)/", "rebuildable"),
    (r"^heap-overhead/", "free"),
]


def classify(path):
    p = re.sub(r"\([^/]*\)", "", path[len("explicit/"):])
    for rx, cls in RULES:
        if re.search(rx, p):
            return cls
    return "fixed"


def split_process(reps, proc):
    out = {"state": 0, "rebuildable": 0, "free": 0, "fixed": 0, "unknown": 0}
    heap_reported, explicit = 0, 0
    whole = {}
    for r in reps:
        if r["process"] != proc:
            continue
        if r["path"] in ("heap-allocated", "resident-unique"):
            whole[r["path"]] = r["amount"]
        # the allocator's committed memory beyond the live blocks; before task 178d it fell into unknown
        # (the ^heap-overhead/ rule matched nothing: 157 reports it outside explicit/)
        if r["units"] == 0 and r["path"].startswith("heap/committed/") and not r["path"].startswith("heap/committed/allocated"):
            out["free"] += r["amount"]
            whole["overhead"] = whole.get("overhead", 0) + r["amount"]
        if r["units"] != 0 or not r["path"].startswith("explicit/"):
            continue
        out[classify(r["path"])] += r["amount"]
        explicit += r["amount"]
        heap_reported += r["amount"] if r["kind"] == HEAP else 0
    heap_unclassified = whole.get("heap-allocated", 0) - heap_reported
    out["unknown"] += heap_unclassified + max(0, whole.get("resident-unique", 0) - explicit - heap_unclassified - whole.get("overhead", 0))
    return {k: v / 1048576 for k, v in out.items()}, whole.get("resident-unique", 0) / 1048576


res_file = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "firefox_realsites_extra.json")
results = json.load(open(res_file))
print("| Site | Cost | State | Rebuildable | Free | Fixed | Unknown | Main + media + SW | Frames | Droppable, low to high |")
print("|---|---:|---:|---:|---:|---:|---:|---:|---:|---|")
for site in ORDER:
    v = results.get(site)
    path = os.path.join(REPORTS, site + firefox_state.report_suffix() + ".json.gz")
    if not v or "browser" not in v or not os.path.exists(path):
        continue
    v = firefox_state.real_site(v)
    reps = json.load(gzip.open(path))["reports"]
    top = v["live"]["top_process"]
    split, uss = split_process(reps, top)
    k = v["browser"]["page_cost_by_kind"]
    g = lambda n: k.get(n, {}).get("uss", 0)
    others = g("Main Process") + g("RDD") + g("service worker")
    frames = g("content") - (uss - 9)  # content kind minus the site's process (less the spare one it used up)
    cost = v["browser"]["page_cost"]["uss"]
    low = split["rebuildable"] + split["free"] + others
    high = low + split["fixed"] + split["unknown"]
    print(f"| {site} | {cost:.0f} | {split['state']:.0f} | {split['rebuildable']:.0f} | {split['free']:.0f} | {split['fixed']:.0f} | {split['unknown']:.0f} | {others:.0f} | {frames:.0f} | {low / cost:.0%} to {high / cost:.0%} |")
print("\nMB, unique memory. State, rebuildable, free, fixed, unknown: the site's content process. "
      "Droppable low: rebuildable + free + main/media/SW; high: also fixed and unknown. "
      "State is at Firefox's size (the DOM in it, 1 to 13 MB, would be 0.25x to 0.6x in the mini DOM).")
