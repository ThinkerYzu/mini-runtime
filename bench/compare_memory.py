"""Memory by layer, mini runtime against Firefox, through layout (the stage 5 numbers).

usage: python3 bench/compare_memory.py [--bin DIR] [--no-prof] [--page NAME]
  --bin DIR   directory with the release binaries `layoutcount` and `layoutprof` of mrt-layout
              (default ~/.cache/mrt-tgt-M/release; build: cargo build --release --offline --bins in mrt-layout)
  --no-prof   skip `layoutprof` (the split of the frame phases by allocation site); the layer table then
              puts the whole frames + reflow phases under "frames + layout"
Reads bench/firefox_static.json (from firefox_static.py; needs the `sub` and `arena` fields) and runs the
mini side itself. Prints markdown tables. Raw mini output is kept in bench/mrt_layout_memory.jsonl.

What is compared (all in MB, 1 MB = 2^20 bytes):
  Firefox   DOM    = dom (element nodes, text nodes, other)
            style  = layout/{computed-values, element-data-objects, style-sets, style-sheets, style-structs,
                     shadow-dom, svg-mapped-declarations}, the same set as the stage 2a table
            frames = layout/{pres-arena minus nsLineBox, pres-shell, pres-contexts, frame-properties}
            lines  = layout/pres-arena/nsLineBox
            text   = layout/text-runs
            left out: js, display-list and display-list-arena (painting; not built here), images, network,
            graphics, and everything outside the page's window (fonts, startup, shared caches).
  mini      DOM    = dom_mb of layoutcount (parse, scratch freed)
            style  = (style_mb - the stylist with only the user-agent sheets, measured on a trivial page, which
                     Firefox shares between pages) + resolve_mb + the style data made while building frames
                     (anonymous boxes, ::before/::after; found by allocation site)
            frames, lines, text = the rest of frames_mb + reflow_mb, split by allocation site (layoutprof)
            left out: caches of font files and metrics (shared in Firefox too)
Allocation-site splits are sampled (one sample per 2 KB allocated): good to a few percent for items over
0.2 MB, noisy below. Totals are exact (counting allocator).
"""
import json, os, re, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
from recorded_tests import require_recordings
require_recordings(os.path.join(HERE, 'firefox_static.json'),
                   record='HEADLESS=1 FIREFOX_BIN=... ./mach python /path/to/bench/firefox_static.py')
PAGES = ["synthetic", "hackernews", "commons", "mdn-css", "wikipedia", "wiki-long"]
args = sys.argv[1:]
def opt(name, default=None):
    return args[args.index(name) + 1] if name in args else default
BIN = opt("--bin", os.path.expanduser("~/.cache/mrt-tgt-M/release"))
PROF = "--no-prof" not in args
ONLY = opt("--page")
import firefox_state  # FIREFOX_STATE=minimized reads Firefox after "Minimize memory usage" (GAPS A17)
ff = firefox_state.test_pages(json.load(open(os.path.join(HERE, "firefox_static.json"))))

STYLE_KEYS = ["computed-values", "element-data-objects", "style-sets", "style-sheets", "style-structs", "shadow-dom", "svg-mapped-declarations"]
PAINT_KEYS = ["display-list", "display-list-arena"]

def ff_layers(p):
    """Firefox categories of one page: per-category dict plus the layer sums."""
    live = ff[p]["live"]; sub = live["sub"]; arena = live.get("arena", {})
    g = lambda k: sub.get("layout/" + k, 0.0)
    style = sum(g(k) for k in STYLE_KEYS)
    paint = sum(g(k) for k in PAINT_KEYS)
    lines = arena.get("nsLineBox", 0.0)
    frames = g("pres-arena") - lines + g("pres-shell") + g("pres-contexts") + g("frame-properties")
    text = g("text-runs")
    known = set("layout/" + k for k in STYLE_KEYS + PAINT_KEYS + ["pres-arena", "pres-shell", "pres-contexts", "frame-properties", "text-runs"])
    other = sum(v for k, v in sub.items() if k.startswith("layout/") and k not in known)
    return {"dom": live["dom"], "style": style, "frames": frames, "lines": lines, "text": text,
            "left_out_paint": paint, "unclassified": other,
            "hidden_dom": ff[p]["hidden"]["dom"], "hidden_layout": ff[p]["hidden"]["layout"]}

# Allocation-site classes of the mini runtime, first match wins. `site` is "innermost < caller < caller"
# of the frames that are not in std, alloc or core (layoutprof).
CLASSES = [
    ("dom", r"mrt_dom::"),
    ("shared-fonts", r"gfxFontCache|nsFontCache|nsFontMetrics|mrt_text|GetMetricsFor|gfxFontGroup::new|EnsureFontList"),
    ("text", r"gfxTextRun|BuildTextRun|TextRunUserData|gfxSkipChars|gfxFontGroup_rs::.*MakeTextRun|MakeTextRun"),
    ("lines", r"nsLineBox"),
    ("style", r"style::|selectors::|cssparser|servo_arc|ms::|string_cache|StrongRuleNode|rule_tree"),
    ("content adapter", r"nsIContent|CharacterDataBuffer"),
    ("overflow + properties", r"FinishAndStoreOverflow|with_stubs|SetProperty|Property"),
    ("text frame data", r"nsTextFrame_rs::"),
    ("frame structs", r"FrameTree::AddFrame"),
    ("child lists", r"Members|nsFrameList|AddFrames|SetInitialChildList"),
]
FRAME_PARTS = ["frame structs", "content adapter", "text frame data", "overflow + properties", "child lists", "other"]

def classify(site):
    for name, rx in CLASSES:
        if re.search(rx, site):
            return name
    return "other"

def run_mini(p):
    out = subprocess.run([os.path.join(BIN, "layoutcount"), os.path.join(HERE, "..", "pages", p + ".html")], capture_output=True, text=True).stdout.splitlines()
    d = json.loads(out[0])
    prof = {}
    if PROF and d.get("outcome") == "ok":
        txt = subprocess.run([os.path.join(BIN, "layoutprof"), os.path.join(HERE, "..", "pages", p + ".html"), "--top", "100000", "--depth", "3"], capture_output=True, text=True).stdout
        for line in txt.splitlines():
            m = re.match(r"\s*([\d.]+)\s+\[([\d. ]+)\]\s+(.*)", line)
            if not m: continue
            ph = [float(x) for x in m.group(2).split()]
            prof.setdefault(classify(m.group(3)), [0.0] * 5)
            for i in range(5): prof[classify(m.group(3))][i] += ph[i]
        sz = {}
        for line in txt.splitlines():
            m = re.match(r"size_of (\S+) (\d+)", line)
            if m: sz[m.group(1)] = int(m.group(2))
            m = re.match(r"frames (\d+) \(capacity (\d+)\)", line)
            if m: sz["frames"] = int(m.group(1)); sz["frames_capacity"] = int(m.group(2))
        d["_sizes"] = sz
    d["_prof"] = prof
    return d

base = run_mini_path = None
trivial = subprocess.run([os.path.join(BIN, "layoutcount"), os.path.join(HERE, "trivial.html")], capture_output=True, text=True).stdout if os.path.exists(os.path.join(HERE, "trivial.html")) else None
UA_STYLIST_MB = json.loads(trivial.splitlines()[0])["style_mb"] if trivial else 0.386

def mini_layers(d):
    """Layer sums of the mini runtime from layoutcount (exact phases) and layoutprof (site split of the frame phases)."""
    dom = d["dom_mb"]
    style = d["style_mb"] - UA_STYLIST_MB + d["resolve_mb"]
    fr_total = d["frames_mb"] + d["reflow_mb"]
    prof = d["_prof"]
    if not prof:
        return {"dom": dom, "style": style, "frames": fr_total, "lines": None, "text": None, "shared": 0.0, "parts": {}}
    # the sampled sites of phases 3 (frames) and 4 (reflow) only; scale to the exact total of those phases
    late = {k: v[3] + v[4] for k, v in prof.items() if k not in ("dom",)}
    late_total = sum(late.values()) or 1.0
    scale = fr_total / late_total
    late = {k: v * scale for k, v in late.items()}
    style += late.pop("style", 0.0)
    # a sampled site of the style setup phases that is not style (none expected) is ignored
    parts = {k: late.get(k, 0.0) for k in FRAME_PARTS}
    shared = late.get("shared-fonts", 0.0)
    return {"dom": dom, "style": style, "frames": sum(parts.values()), "lines": late.get("lines", 0.0),
            "text": late.get("text", 0.0), "shared": shared, "parts": parts}

rows = []
os.makedirs(HERE, exist_ok=True)
with open(os.path.join(HERE, "mrt_layout_memory.jsonl"), "w") as out:
    for p in PAGES:
        if ONLY and p != ONLY: continue
        d = run_mini(p)
        out.write(json.dumps({k: v for k, v in d.items() if not k.startswith("_")}) + "\n")
        rows.append((p, d))

def r(a, b):
    return f"{b / a:.1f}x" if a else "-"

print(f"Mini = layoutcount + layoutprof in {BIN}; Firefox = bench/firefox_static.json (live, headless, 1489x844). MB.\n")
print("### Per layer: mini against Firefox (mini / Firefox; below 1.0x means the mini is smaller)\n")
print("| Page | Layer | Mini | Firefox | Mini / Firefox |\n|---|---|---|---|---|")
totals = []
for p, d in rows:
    if d.get("outcome") != "ok":
        print(f"| {p} | - | {d.get('outcome')}: {d.get('message', '')[:60]} | | |"); continue
    m = mini_layers(d); f = ff_layers(p)
    if m["lines"] is None:
        layers = [("dom", m["dom"], f["dom"]), ("style", m["style"], f["style"]), ("frames + layout (frames, lines, text runs)", m["frames"], f["frames"] + f["lines"] + f["text"])]
    else:
        layers = [("dom", m["dom"], f["dom"]), ("style", m["style"], f["style"]), ("frames", m["frames"], f["frames"]), ("line boxes", m["lines"], f["lines"]), ("text runs", m["text"], f["text"])]
    for name, a, b in layers:
        print(f"| {p} | {name} | {a:.2f} | {b:.2f} | {r(b, a)} |")
    ma = sum(a for _, a, _ in layers); fb = sum(b for _, _, b in layers)
    print(f"| **{p}** | **total** | **{ma:.2f}** | **{fb:.2f}** | **{r(fb, ma)}** |")
    totals.append((p, d, m, f, ma, fb))

print("\n### Totals, peak and what remains\n")
print("Mini live = dom + style + frames + lines + text runs as defined above (the user-agent stylist and font caches left out). "
      "Mini peak = highest live bytes during the run (parse scratch included, nothing subtracted). "
      "Firefox hibernated = dom + style-sets after dropping the frame tree (Gecko hibernation prototype).\n")
print("| Page | Mini live | Firefox live (layers above) | Ratio | Mini peak | Mini dom + style only | Firefox hibernated (dom + layout left) | Mini frames+lines+text share of live |\n|---|---|---|---|---|---|---|---|")
for p, d, m, f, ma, fb in totals:
    peak = max(d["peak_dom_mb"], d["peak_style_mb"], d["peak_resolve_mb"], d["peak_frames_mb"], d["peak_reflow_mb"])
    keep = m["dom"] + m["style"]
    hib = f["hidden_dom"] + f["hidden_layout"]
    late = ma - keep
    print(f"| {p} | {ma:.2f} | {fb:.2f} | {r(fb, ma)} | {peak:.2f} | {keep:.2f} | {hib:.2f} | {late / ma * 100:.0f}% |")

print("\n### Phase detail from layoutcount (exact live MB after each phase; peak = highest live bytes seen up to the end of the phase)\n")
print("| Page | dom | style setup | resolve | frames | reflow | total | peak dom | peak setup | peak resolve | peak frames | peak reflow | frames | elements |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
for p, d in rows:
    if d.get("outcome") != "ok": continue
    print(f"| {p} | {d['dom_mb']:.2f} | {d['style_mb']:.2f} | {d['resolve_mb']:.2f} | {d['frames_mb']:.2f} | {d['reflow_mb']:.2f} | {d['total_mb']:.2f} | {d['peak_dom_mb']:.2f} | {d['peak_style_mb']:.2f} | {d['peak_resolve_mb']:.2f} | {d['peak_frames_mb']:.2f} | {d['peak_reflow_mb']:.2f} | {d['frames']} | {d['elements']} |")
print(f"\nUser-agent stylist baseline subtracted from `style setup`: {UA_STYLIST_MB:.3f} MB (trivial page).")

if PROF:
    print("\n### Mini frame + reflow memory by allocation class (sampled, scaled to the exact frames + reflow total; MB)\n")
    cols = FRAME_PARTS + ["lines", "text", "style", "shared-fonts"]
    print("| Page | " + " | ".join(cols) + " | total |\n|" + "---|" * (len(cols) + 2))
    for p, d, m, f, ma, fb in totals:
        late = {k: v[3] + v[4] for k, v in d["_prof"].items() if k != "dom"}
        sc = (d["frames_mb"] + d["reflow_mb"]) / (sum(late.values()) or 1)
        print(f"| {p} | " + " | ".join(f"{late.get(c, 0) * sc:.2f}" for c in cols) + f" | {d['frames_mb'] + d['reflow_mb']:.2f} |")
    print("\n### Firefox per category (MB, live)\n")
    allk = ["dom/element-nodes", "dom/text-nodes", "dom/other", "layout/computed-values", "layout/element-data-objects", "layout/style-sets", "layout/style-sheets", "layout/style-structs",
            "layout/pres-arena", "layout/pres-shell", "layout/pres-contexts", "layout/frame-properties", "layout/text-runs", "layout/display-list", "layout/display-list-arena"]
    print("| Category | " + " | ".join(p for p, *_ in totals) + " |\n|" + "---|" * (len(totals) + 1))
    for k in allk:
        print(f"| {k} | " + " | ".join(f"{ff[p]['live']['sub'].get(k, 0):.2f}" for p, *_ in totals) + " |")
    ar = sorted({k for p, *_ in totals for k in ff[p]["live"].get("arena", {})})
    for k in ar:
        print(f"|   pres-arena/{k} | " + " | ".join(f"{ff[p]['live'].get('arena', {}).get(k, 0):.2f}" for p, *_ in totals) + " |")
    print("| unclassified layout/* | " + " | ".join(f"{ff_layers(p)['unclassified']:.2f}" for p, *_ in totals) + " |")
    print("\n### Sizes\n")
    for p, d, *_ in totals[:1]:
        print("size_of (bytes):", ", ".join(f"{k} {v}" for k, v in d["_sizes"].items() if k.startswith(("Frame", "nsI", "nsL", "nsT", "Comp", "Prop"))))
    print("\n| Page | frames | Vec capacity | FrameData bytes in Vec | Firefox frames+lines (pres-arena) | bytes per frame, Firefox | bytes per frame, mini frames+lines |\n|---|---|---|---|---|---|---|")
    for p, d, m, f, ma, fb in totals:
        sz = d["_sizes"]
        fa = ff[p]["live"]["sub"].get("layout/pres-arena", 0)
        print(f"| {p} | {sz['frames']} | {sz['frames_capacity']} | {sz['frames_capacity'] * sz['FrameData'] / 1048576:.2f} | {fa:.2f} | {fa * 1048576 / sz['frames']:.0f} | {(m['frames'] + m['lines']) * 1048576 / sz['frames']:.0f} |")

    print("\n### Estimated saving per removable item (MB; upper bounds from the allocation classes above; assumptions in the notes)\n")
    print("Notes: (1) Vec slack = (capacity - frames) x FrameData: the Vec doubles as it grows; shrink_to_fit after reflow. "
          "(2) nsIContent inline = 56 B x frames, plus its heap parts (Rc text copies, tag Strings): replace by the arena NodeId (4 B). "
          "(3) properties map = 48 B x frames of the empty HashMap inside every FrameData: replace by a side table made on first use. "
          "(4) text frame data: a HashMap<frame id, 168 B struct>; assumed 56 B per text frame as a plain Vec entry would take. "
          "(5) overflow areas: a HashMap entry per frame; Gecko keeps them only when they differ from the frame rect (assumed 90% not needed). "
          "(6) child lists and inline baselines: boxed frame lists and a HashMap of baselines; assumed 60% removable.\n")
    print("| Page | Vec slack | nsIContent inline + heap | properties map | text frame data | overflow areas | child lists + other | Sum | Mini total | Mini total after | After / Firefox |\n|---|---|---|---|---|---|---|---|---|---|---|")
    MB = 1048576.0
    for p, d, m, f, ma, fb in totals:
        sz = d["_sizes"]; n = sz["frames"]; parts = m["parts"]
        nt = d["frame_counts"].get("Text", 0) + d["frame_counts"].get("TextContinuation", 0)
        slack = (sz["frames_capacity"] - n) * sz["FrameData"] / MB
        content = n * sz["nsIContent"] / MB + parts["content adapter"]
        props = n * sz["PropertiesMap"] / MB
        tfd = max(parts["text frame data"] - nt * 56 / MB, 0)
        ovf = parts["overflow + properties"] * 0.9
        cl = (parts["child lists"] + parts["other"]) * 0.6
        tot = slack + content + props + tfd + ovf + cl
        print(f"| {p} | {slack:.2f} | {content:.2f} | {props:.2f} | {tfd:.2f} | {ovf:.2f} | {cl:.2f} | {tot:.2f} | {ma:.2f} | {ma - tot:.2f} | {r(fb, ma - tot)} |")
