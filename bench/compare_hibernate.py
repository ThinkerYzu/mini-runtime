"""Stage 4: hibernation in the mini runtime against hibernation in Gecko, same pages, same Firefox build.

usage: python3 bench/compare_hibernate.py [--bin DIR] [--cycles N]
  --bin DIR    directory with mrt-js's release `jscount` (default: mrt-js/target/release)
  --cycles N   hibernate and wake cycles per page on the mini side (default 3; the median wake is shown)
Reads bench/firefox_hibernate.json (from firefox_hibernate.py) and runs `jscount --cycles N` itself. Raw
mini output is kept in bench/mrt_hibernate.jsonl. MB = 2^20 bytes, times in ms on this desktop.

What is compared:
  Hibernated  mini: the DOM (a clone's bytes), the wrapper cache and the JS heap (realm and zone), what a
              hibernated page keeps; process-wide caches (Stylo's user-agent styles, fonts) are left out,
              as Firefox reports its own outside the window.
              Firefox: the window's dom, layout and js-realm plus js-zone after docViewer.hide() and a GC
  Wake        mini: style, frame construction and reflow of the whole page (warm caches), median of the
              cycles. Firefox: docViewer.show() plus a layout read, median of the 2nd and 3rd show (the
              first one also warms caches)
  Saved       mini only: the DOM written to bytes (Dom::to_bytes) and read back (Dom::from_bytes)
  Process     the whole process holding the page (one page per process on both sides), live and hibernated:
              heap = malloc'ed bytes in use (Firefox `heap-allocated`; mini glibc mallinfo2), which leaves out
              the JS GC heap on both sides (GC chunks are mmapped); resident = Firefox `resident`, mini VmRSS; unique = Firefox `resident-unique`, mini RssAnon
              (anonymous pages; mapped font files and code are file-backed and left out).
              The mini calls malloc_trim(0) at hibernation (Gecko's jemalloc purges by itself).
"""
import json, os, statistics, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
from recorded_tests import require_recordings
require_recordings(os.path.join(HERE, 'firefox_hibernate.json'),
                   record='FIREFOX_BIN=... ./mach python /path/to/bench/firefox_hibernate.py')
args = sys.argv[1:]
def opt(name, default=None):
    return args[args.index(name) + 1] if name in args else default
BIN = opt("--bin", os.path.join(HERE, "..", "mrt-js", "target", "release"))
CYCLES = opt("--cycles", "3")
import firefox_state  # FIREFOX_STATE=minimized reads Firefox after "Minimize memory usage" (GAPS A17)
ff = firefox_state.test_pages(json.load(open(os.path.join(HERE, "firefox_hibernate.json"))))

rows, raw, proc_rows = [], [], []
for p in ff:
    path = os.path.join(HERE, "..", "pages", p + ".html")
    if not os.path.exists(path):
        continue
    out = subprocess.run([os.path.join(BIN, "jscount"), path, "--cycles", CYCLES], capture_output=True, text=True).stdout
    m = json.loads(out.strip().splitlines()[-1]); raw.append(m)
    h = m["hibernate"]
    mini_hib = m["dom_mb"] + h["wrapper_cache_mb"] + h["js_mb"]
    c = ff[p]["hidden"]["cats"]
    ff_hib = c.get("dom", 0) + c.get("layout", 0) + c.get("js-realm", 0) + c.get("js-zone", 0)
    proc_rows.append((p, ff[p]["live"].get("process", {}), ff[p]["hidden"].get("process", {}), h.get("process", {})))
    rows.append((p, mini_hib, ff_hib, statistics.median(h["wake_ms"]), statistics.median(ff[p]["wake_ms"][1:]), h["saved_mb"], h["save_ms"], h["restore_ms"], h["same_as_first"] and h["restored_equal"]))

with open(os.path.join(HERE, "mrt_hibernate.jsonl"), "w") as f:
    for m in raw:
        f.write(json.dumps(m) + "\n")

print("| Page | Hibernated, mini | Hibernated, Firefox | Ratio | Wake, mini (ms) | Wake, Firefox (ms) | Ratio | Saved DOM (MB) | Save / restore (ms) | Checks |")
print("|---|---:|---:|---:|---:|---:|---:|---:|---:|---|")
for p, mh, fh, mw, fw, saved, sms, rms, ok in rows:
    print(f"| {p} | {mh:.2f} | {fh:.2f} | {mh / fh:.2f}x | {mw:.0f} | {fw:.0f} | {mw / max(fw, 0.5):.1f}x | {saved:.2f} | {sms:.1f} / {rms:.1f} | {'ok' if ok else 'FAILED'} |")

print()
print("| Page | Heap: Firefox live | Gecko hibernated | mini live | mini hibernated | Unique: Firefox live | Gecko hibernated | mini live | mini hibernated | Resident: Firefox live | Gecko hibernated | mini live | mini hibernated |")
print("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
for p, fl, fh, m in proc_rows:
    if not fl or not m:
        continue
    print(f"| {p} | {fl['heap-allocated']:.1f} | {fh['heap-allocated']:.1f} | {m['heap_live_mb']:.1f} | {m['heap_hibernated_mb']:.1f} | {fl['resident-unique']:.0f} | {fh['resident-unique']:.0f} | {m['anon_live_mb']:.0f} | {m['anon_hibernated_mb']:.0f} | {fl['resident']:.0f} | {fh['resident']:.0f} | {m['rss_live_mb']:.0f} | {m['rss_hibernated_mb']:.0f} |")
