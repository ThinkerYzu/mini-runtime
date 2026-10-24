"""Per-element layout comparison: Firefox's boxes (bench/firefox_boxes/<page>.json) against the mini runtime's
(`layoutcount <page>.html --boxes`), so a page-height delta can be traced to a box.

usage: python3 bench/compare_boxes.py <page> [--top N] [--bin path/to/layoutcount] [--tol PX]
       python3 bench/compare_boxes.py all        (the six pages)

Elements are matched by their index in document order. Firefox's side comes from
`FIREFOX_BIN=... ./mach python bench/firefox_boxes.py` (see that file).

Report:
  counts       elements, boxes on each side, matched, missing (Firefox has a box, the mini runtime has no
               frame), extra (the reverse), and the elements whose tag or parent differs (index misalignment)
  first divergence
               in document order, the first element whose x, y or width differs by more than the tolerance
               (1 px), or whose height differs and none of its children's heights does, while every
               ancestor's x, y and width match (an ancestor's height is ignored: it differs whenever a
               descendant does, so the first element by plain document order is always <html>)
  root causes  the elements whose height differs, none of whose matched children's heights differ (the deepest
               ones), whose ancestors all have the right x and width (y is ignored: it is a knock-on of earlier
               errors). Ranked by absolute height error. Each line shows the element's children
               (tag:Firefox height/mini height) to tell a taller box from a box lacking a child.
  width causes the same for widths: the top elements whose width differs while the ancestors' widths match
  histograms   the root causes' signed and absolute height error by tag and by display type; the sum of the
               root causes' errors against the page's height delta
"""
import argparse, collections, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
PAGES = ["mdn-css", "commons", "wiki-long", "hackernews", "wikipedia", "synthetic"]
DEFAULT_BIN = os.environ.get("LAYOUTCOUNT", os.path.expanduser("~/.cache/mrt-tgt-P/release/layoutcount"))


def load_mini(binary, page):
    r = subprocess.run([binary, os.path.join(HERE, "..", "pages", page + ".html"), "--boxes"], capture_output=True, text=True)
    lines = r.stdout.splitlines()
    head = json.loads(lines[0])
    if head.get("outcome") != "ok":
        sys.exit(f"{page}: layoutcount failed: {head}")
    tags, parent, boxes = [], [], {}
    for ln in lines[1:]:
        if not ln.startswith("E "):
            continue
        f = ln.split()
        i = int(f[1])
        assert i == len(tags)
        parent.append(int(f[2])); tags.append(f[3])
        if len(f) > 4:
            boxes[i] = tuple(float(v) for v in f[4:8])
    return {"tags": tags, "parent": parent, "boxes": boxes, "height": head["document_height_px"]}


def label(ff, i, meta):
    tag = ff["tags"][i]
    idc = meta.get(i, ("", "", ""))
    s = tag
    if idc[1]: s += "#" + idc[1]
    if idc[2]: s += "." + idc[2]
    return s


def rect(r):
    return "-" if r is None else "[%.1f %.1f %.1f %.1f]" % r


def analyze(page, binary, top, tol):
    ff = json.load(open(os.path.join(HERE, "firefox_boxes", page + ".json")))
    mini = load_mini(binary, page)
    n = ff["n"]
    print("=" * 100)
    print(f"{page}: page height Firefox {ff['scrollHeight']} px, mini {mini['height']:.0f} px "
          f"({(mini['height'] - ff['scrollHeight']) / ff['scrollHeight'] * 100:+.1f}%)")
    meta = {b[0]: (b[5], b[6], b[7]) for b in ff["boxes"]}  # display, id, class
    fbox = {b[0]: tuple(b[1:5]) for b in ff["boxes"]}
    mbox = mini["boxes"]
    nm = len(mini["tags"])
    bad_tag = [i for i in range(min(n, nm)) if ff["tags"][i] != mini["tags"][i]]
    bad_parent = [i for i in range(min(n, nm)) if ff["parent"][i] != mini["parent"][i]]
    matched = [i for i in fbox if i in mbox]
    hidden = {b[0] for b in ff["boxes"] if len(b) > 8 and b[8]}
    missing = [i for i in fbox if i not in mbox and i not in hidden]
    extra = [i for i in mbox if i not in fbox]
    print(f"elements: Firefox {n}, mini {nm}; tag mismatches {len(bad_tag)}, parent mismatches {len(bad_parent)}"
          + (f" (first at index {bad_tag[0]})" if bad_tag else ""))
    print(f"boxes: Firefox {len(fbox)}, mini {len(mbox)}; matched {len(matched)}, missing frames {len(missing)}, extra {len(extra)}"
          + (f"; {len(hidden)} Firefox boxes in closed <details> left out" if hidden else ""))
    if n != nm:
        print("  WARNING: element counts differ, indexes past the first difference do not line up")
        return
    if missing:
        c = collections.Counter(ff["tags"][i] + "/" + meta[i][0] for i in missing)
        print("  missing frames by tag/display: " + ", ".join(f"{k} {v}" for k, v in c.most_common(10)))
        # the missing boxes with the most height (only those whose parent has a frame in the mini runtime)
        big = sorted((i for i in missing if ff["parent"][i] in mbox or ff["parent"][i] < 0), key=lambda i: -fbox[i][3])[:5]
        print("  tallest missing: " + "; ".join(f"{label(ff, i, meta)} {meta[i][0]} h={fbox[i][3]:.0f}" for i in big))
    if extra:
        c = collections.Counter(mini["tags"][i] for i in extra)
        print("  extra frames by tag: " + ", ".join(f"{k} {v}" for k, v in c.most_common(10)))

    def diff(i):
        f, m = fbox[i], mbox[i]
        return (m[0] - f[0], m[1] - f[1], m[2] - f[2], m[3] - f[3])

    ancestors = {}
    def anc_list(i):
        a = ancestors.get(i)
        if a is None:
            p = ff["parent"][i]
            a = [] if p < 0 else anc_list(p) + [p]
            ancestors[i] = a
        return a

    sys.setrecursionlimit(100000)
    mset = set(matched)
    ok_xyw = {}   # all matched ancestors have right x, y, width
    ok_xw = {}    # ... right x and width
    for i in range(n):
        p = ff["parent"][i]
        pa = ok_xyw[p] if p >= 0 else True
        pb = ok_xw[p] if p >= 0 else True
        if i in mset:
            d = diff(i)
            pa = pa and abs(d[0]) <= tol and abs(d[1]) <= tol and abs(d[2]) <= tol
            pb = pb and abs(d[0]) <= tol and abs(d[2]) <= tol
        ok_xyw[i] = pa; ok_xw[i] = pb
    # the flag of an element covers itself; its own check needs the parents' flags
    def parents_ok(table, i):
        p = ff["parent"][i]
        return True if p < 0 else table[p]

    # children with boxes in both (nearest matched ancestor)
    kids = collections.defaultdict(list)
    for i in matched:
        p = ff["parent"][i]
        while p >= 0 and p not in mset:
            p = ff["parent"][p]
        if p >= 0:
            kids[p].append(i)
    hd = {i: diff(i)[3] for i in matched}

    def leaf_height_error(i):
        return abs(hd[i]) > tol and not any(abs(hd[c]) > tol for c in kids[i])

    # first divergence: x, y or width differs, or the height differs and no child explains it
    first = None
    for i in sorted(matched):
        d = diff(i)
        if (max(abs(d[0]), abs(d[1]), abs(d[2])) > tol or leaf_height_error(i)) and parents_ok(ok_xyw, i):
            first = i
            break
    print("first divergence (document order; x, y or width differs, or height differs with no child to blame; ancestors' x, y, width match):")
    if first is None:
        print("  none")
    else:
        show_el(ff, mini, meta, fbox, mbox, first, diff(first), kids=kids)

    causes = []
    for i in matched:
        if leaf_height_error(i) and parents_ok(ok_xw, i):
            causes.append(i)
    causes.sort(key=lambda i: -abs(hd[i]))
    print(f"root causes of height error (deepest elements whose height differs; {len(causes)} found); top {top}:")
    for i in causes[:top]:
        show_el(ff, mini, meta, fbox, mbox, i, diff(i), kids=kids)
    total_delta = mini["height"] - ff["scrollHeight"]
    signed = sum(hd[i] for i in causes)
    print(f"  sum of root-cause height errors: {signed:+.0f} px (page height delta {total_delta:+.0f} px); "
          f"sum of absolute errors {sum(abs(hd[i]) for i in causes):.0f} px")

    # width causes
    wd = {i: diff(i)[2] for i in matched}
    wc = [i for i in matched if abs(wd[i]) > tol and parents_ok(ok_xw, i)]
    wc.sort(key=lambda i: -abs(wd[i]))
    print(f"width causes (width differs, ancestors' x and width match; {len(wc)} found); top {min(top, 8)}:")
    for i in wc[:min(top, 8)]:
        show_el(ff, mini, meta, fbox, mbox, i, diff(i))

    # histograms
    for title, keyf in (("tag", lambda i: ff["tags"][i]), ("display", lambda i: meta[i][0]),
                        ("tag/display", lambda i: ff["tags"][i] + "/" + meta[i][0])):
        agg = collections.defaultdict(lambda: [0, 0.0, 0.0])
        for i in causes:
            a = agg[keyf(i)]
            a[0] += 1; a[1] += hd[i]; a[2] += abs(hd[i])
        rows = sorted(agg.items(), key=lambda kv: -kv[1][2])[:8 if title != "tag/display" else 10]
        print(f"height error of root causes by {title}: " + "; ".join(f"{k} n={v[0]} signed={v[1]:+.0f} abs={v[2]:.0f}" for k, v in rows))


def show_el(ff, mini, meta, fbox, mbox, i, d, kids=None):
    chain = []
    p = ff["parent"][i]
    while p >= 0 and len(chain) < 4:
        chain.append(ff["tags"][p]); p = ff["parent"][p]
    print(f"  #{i} <{label(ff, i, meta)}> display:{meta[i][0]}  in <{'< '.join(chain)}>")
    print(f"      Firefox {rect(fbox[i])}  mini {rect(mbox[i])}  d(x,y,w,h)=({d[0]:+.1f},{d[1]:+.1f},{d[2]:+.1f},{d[3]:+.1f})")
    if kids is not None:
        ks = kids.get(i, [])
        s = ", ".join(f"{ff['tags'][c]}:{fbox[c][3]:.0f}/{mbox[c][3]:.0f}" for c in ks[:5])
        print(f"      children ({len(ks)}): {s}" + (" ..." if len(ks) > 5 else ""))
        if ks:
            # how far down the children reach, from the element's top: tells padding and margins (the
            # box is taller than its content) from children that sit too far down
            fb = max(fbox[c][1] + fbox[c][3] for c in ks) - fbox[i][1]
            mb = max(mbox[c][1] + mbox[c][3] for c in ks) - mbox[i][1]
            print(f"      content bottom from top: Firefox {fb:.1f} of {fbox[i][3]:.1f}, mini {mb:.1f} of {mbox[i][3]:.1f}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("page")
    ap.add_argument("--top", type=int, default=15)
    ap.add_argument("--bin", default=DEFAULT_BIN)
    ap.add_argument("--tol", type=float, default=1.0)
    from recorded_tests import require_recordings
    require_recordings(record="FIREFOX_BIN=... ./mach python /path/to/bench/firefox_boxes.py")
    a = ap.parse_args()
    require_recordings(*(os.path.join(HERE, "firefox_boxes", p + ".json")
                         for p in (PAGES if a.page == "all" else [a.page])),
                       record="FIREFOX_BIN=... ./mach python /path/to/bench/firefox_boxes.py")
    for p in (PAGES if a.page == "all" else [a.page]):
        analyze(p, a.bin, a.top, a.tol)


main()
