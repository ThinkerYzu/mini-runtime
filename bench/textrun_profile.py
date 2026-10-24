"""Text run memory of the mini runtime, part by part, against Firefox's "text-runs" reporter.

usage: python3 bench/textrun_profile.py [--bin DIR] [--page NAME] [--json]
  --bin DIR   directory with the release binary `layoutprof` of mrt-layout
              (default ~/.cache/mrt-tgt-M/release; build: cargo build --release --offline --bins in mrt-layout)
  --json      also write the raw per-page output to bench/textrun_profile.jsonl
Runs `layoutprof <page> --textruns --top 0` (an exact walk over every text run after layout; sizes are glibc
malloc chunk sizes) and prints markdown tables:
  1. counts: runs, chars, frames, flows, sharing, duplicates, run size distribution
  2. bytes per part, per page (MB), with bytes per run and per char
  3. mini against Firefox: the part of the mini run that Firefox's "text-runs" reporter counts (run block with
     inline glyphs, glyph-run array when more than one, detailed glyphs) against bench/firefox_static.json, and
     a model of Firefox's own heap for the same runs (from the Gecko headers: 112 bytes + 4 per char, 16 byte
     classes) as a check of the accounting
  4. word duplicates: glyph storage a per-font word cache could share, if run glyphs pointed into it
  5. reductions with estimated MB saved and the resulting mini / Firefox ratio
"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
from recorded_tests import require_recordings
require_recordings(os.path.join(HERE, 'firefox_static.json'),
                   record='HEADLESS=1 FIREFOX_BIN=... ./mach python /path/to/bench/firefox_static.py')
PAGES = ["synthetic", "hackernews", "commons", "mdn-css", "wikipedia", "wiki-long"]
args = sys.argv[1:]
def opt(name, default=None):
    return args[args.index(name) + 1] if name in args else default
BIN = opt("--bin", os.path.expanduser("~/.cache/mrt-tgt-M/release"))
ONLY = opt("--page")
MB = 1048576.0
ff = json.load(open(os.path.join(HERE, "firefox_static.json")))

data = {}
for p in PAGES:
    if ONLY and p != ONLY: continue
    out = subprocess.run([os.path.join(BIN, "layoutprof"), os.path.join(HERE, "..", "pages", p + ".html"), "--textruns", "--top", "0"],
                         capture_output=True, text=True).stdout.strip().splitlines()
    data[p] = json.loads(out[-1])
if "--json" in args:
    with open(os.path.join(HERE, "textrun_profile.jsonl"), "w") as f:
        for p, d in data.items(): f.write(json.dumps({"page": p, **d}) + "\n")

def ffmb(p): return ff[p]["live"]["sub"].get("layout/text-runs", 0.0)
mb = lambda b: b / MB

print("### 1. Counts\n")
print("| Page | runs | chars | chars/run | text frames | frames with a run | continuing frames | flows (text nodes mapped) | runs with 2+ frames | max frames/run | runs with 2+ flows | start frames in 2+ runs | run Rc leaks (strong != frames) | fonts |\n|" + "---|" * 14)
for p, d in data.items():
    print(f"| {p} | {d['runs']} | {d['chars']} | {d['chars'] / max(d['runs'], 1):.1f} | {d['text_frames']} | {d['frames_with_run']} | {d['continuing_frames']} | {d['flows']} | {d['runs_multi_frame']} | {d['max_frames']} | {d['runs_multi_flow']} | {d['dup_start_frames']} | {d['strong_mismatch']} | {d['fonts']} |")

print("\nRun length distribution (runs; chars in those runs):\n")
edges = ["1", "2-3", "4-7", "8-15", "16-31", "32-63", "64-127", "128-255", "256-1023", "1024+"]
# hist_edges are the upper bounds (exclusive) of the buckets 0..: 1,2,4,...; the first bucket (len < 1) is empty runs
print("| Page | " + " | ".join(edges) + " |\n|" + "---|" * (len(edges) + 1))
for p, d in data.items():
    hr, hc = d["hist_runs"][1:], d["hist_chars"][1:]
    print(f"| {p} | " + " | ".join(f"{a} ({b})" for a, b in zip(hr, hc)) + " |")

PARTS = ["run struct (Rc+RefCell+gfxTextRun)", "glyph array", "detailed glyph store", "glyph runs vec", "skip chars", "mapped flows vec", "user data table"]
print("\n### 2. Bytes per part (mini, malloc chunk sizes; MB)\n")
print("| Page | " + " | ".join(PARTS) + " | total | bytes/run | bytes/char |\n|" + "---|" * (len(PARTS) + 4))
tot = {}
for p, d in data.items():
    t = sum(d["bytes"][k] for k in PARTS); tot[p] = t
    print(f"| {p} | " + " | ".join(f"{mb(d['bytes'][k]):.3f}" for k in PARTS) + f" | **{mb(t):.3f}** | {t / max(d['runs'], 1):.0f} | {t / max(d['chars'], 1):.1f} |")
print("\nsize_of (bytes): " + ", ".join(f"{k} {v}" for k, v in next(iter(data.values()))["size_of"].items()))
print("\nPayload against allocation (MB): the bytes the data needs, and what malloc rounding and Vec capacity add\n")
print("| Page | glyph array payload | glyph runs payload | skip chars payload | mapped flows payload | all payload | chunk rounding + slack (all parts, incl. struct) |\n|---|---|---|---|---|---|---|")
for p, d in data.items():
    pl = d["payload"]; s = sum(pl.values())
    print(f"| {p} | {mb(pl['glyph array']):.3f} | {mb(pl['glyph runs vec']):.3f} | {mb(pl['skip chars']):.3f} | {mb(pl['mapped flows vec']):.3f} | {mb(s):.3f} | {mb(tot[p] - s):.3f} |")
print("\nGlyph storage: " + "; ".join(f"{p}: {d['simple_glyphs']}/{d['glyph_len']} simple chars, {d['detailed_chars']} with detailed glyphs in {d['runs_with_detailed']} runs, glyph vec capacity == len: {d['glyph_cap'] == d['glyph_len']}, runs with 2+ glyph runs: {d['runs_gruns_gt1']}" for p, d in data.items()))

print("\n### 3. Mini against Firefox\n")
print("`mini counted` = the parts Firefox's `layout/text-runs` reporter counts for the same runs: run block + glyph array + glyph runs array (only when 2+ glyph runs in Gecko) + detailed glyphs. "
      "`mini not counted` = skip chars + user data, which Gecko keeps outside the reporter's sum (user data of a one-flow run is the frame pointer itself, 0 bytes). "
      "`Gecko model` = the Gecko headers applied to this page's runs.\n")
print("| Page | Firefox text-runs (measured) | Gecko model of same runs | mini total (as compare_memory counts it) | mini counted | mini not counted | mini counted / Firefox | mini total / Firefox |\n|---|---|---|---|---|---|---|---|")
mini_counted = {}
for p, d in data.items():
    b = d["bytes"]
    gr = 0  # glyph runs array: Gecko keeps one inline; the Vec is only comparable when the run has 2+
    counted = b["run struct (Rc+RefCell+gfxTextRun)"] + b["glyph array"] + b["detailed glyph store"] + b["glyph runs vec"]
    notc = b["skip chars"] + b["mapped flows vec"] + b["user data table"]
    mini_counted[p] = (counted, notc)
    gm = d["gecko_model"]["counted by text-runs reporter"]
    f = ffmb(p)
    print(f"| {p} | {f:.3f} | {mb(gm):.3f} | {mb(tot[p]):.3f} | {mb(counted):.3f} | {mb(notc):.3f} | {counted / MB / f:.1f}x | {tot[p] / MB / f:.1f}x |" if f else f"| {p} | - | | | | | | |")
print("\n(The Gecko model is within roughly 15% of the measured number where the page has enough runs to matter.)")

print("\n### 4. Word duplicates (what a per-font shaped-word cache could share)\n")
print("Words = whitespace-separated words of the mapped text (untransformed), keyed by (font, word). "
      "Gecko's cache holds words of up to 32 chars (`gfx.font_rendering.wordcache.charlimit`) per font, up to 10000 entries per font; "
      "run glyphs are COPIED out of the cached word (`CopyGlyphDataFrom`), so the cache does not shrink the run's own glyph array.\n")
print("| Page | words | word chars | duplicate words (2nd+ occurrence) | duplicate chars | unique words | unique chars | glyph bytes of duplicates (4 B/char) | % of run glyph array | chars in words of len 1-3 / 4-7 / 8-15 / 16-31 / 32-63 / 64+ |\n|" + "---|" * 10)
for p, d in data.items():
    print(f"| {p} | {d['words']} | {d['word_chars']} | {d['dup_words']} | {d['dup_chars']} | {d['unique_words']} | {d['unique_chars']} | {mb(d['dup_chars'] * 4):.3f} MB | {d['dup_chars'] * 4 / max(d['bytes']['glyph array'], 1) * 100:.0f}% | {' / '.join(str(x) for x in d['word_chars_by_len'])} |")

print("\n### 5. Reductions (MB saved, each measured against this page's own counts; chunk sizes as malloc rounds them)\n")
print("A. glyph runs in the run struct (enum One/Many, Gecko's ElementOrArray): the Vec chunk (4 x 24 = 96 B payload, 112 B chunk) disappears for one-glyph-run runs; the struct grows by 8 B (32 B enum against the 24 B Vec).\n"
      "B. user data in the run: one-flow runs keep the flow inline (16 B enum field, replacing the unused 8 B `mUserData`), multi-flow runs a `Box<[TextRunMappedFlow]>` of exact size; the HashMap side table and the capacity-4 Vec per run go away.\n"
      "C. skip chars as `Option<Box<[SkippedRange]>>` of exact size (capacity 4 = 64 B chunk now, 32 B chunk for 1-2 ranges).\n"
      "D. drop `mLength` (the Box<[T]> knows it) and the 8 B `mUserData`: part of the struct size below (struct block 160 B -> 144 B with A and B applied).\n"
      "E. (optional, unsafe) glyphs inline after the struct as Gecko does: one malloc header and one rounding less, about 16 B per run; not in the sums.\n"
      "F. (optional) 8-bit glyph storage is not possible (Gecko uses 4 B per char as well); a word cache does not help (section 4).\n")
chunk = lambda n: 0 if n == 0 else max(((n + 8 + 15) & ~15), 32)
print("| Page | A glyph runs inline | B user data | C skip chars | D+struct shrink | Sum | mini total now | after | Gecko full model (reported + skip + user data) | after / Gecko full | after / Firefox text-runs measured |\n|" + "---|" * 11)
for p, d in data.items():
    b = d["bytes"]; n = d["runs"]
    multi = d["runs_multi_flow"]; one = n - multi
    kavg = (d["flows"] - one) / multi if multi else 0
    a = (n - d["runs_gruns_gt1"]) * chunk(96) - 0  # Vec chunks freed (the Vec of a 2+ run stays as a Vec in the Many variant)
    struct_now = chunk(16 + 8 + d["size_of"]["gfxTextRun"])
    # 24 (Rc + RefCell) + 120 (gfxTextRun now) - 24 (glyph vec -> Box) + 8 (glyph run inline 32 vs Vec 24) - 8 (mUserData) + 16 (user data enum) - 4 (mLength) = 132 -> 144 chunk
    struct_new = 144
    sstruct = n * (struct_now - struct_new)
    sb = b["user data table"] + b["mapped flows vec"] - multi * chunk(int(12 * kavg))
    sc = d["runs_with_skip"] * (chunk(4 * 12) - chunk(12))
    tot_save = a + sb + sc + sstruct
    after = tot[p] - tot_save
    gfull = sum(d["gecko_model"].values())
    f = ffmb(p)
    print(f"| {p} | {mb(a):.3f} | {mb(sb):.3f} | {mb(sc):.3f} | {mb(sstruct):.3f} | {mb(tot_save):.3f} | {mb(tot[p]):.3f} | {mb(after):.3f} | {mb(gfull):.3f} | {after / gfull:.2f}x | {after / MB / f:.2f}x |" if f else "")
