#!/usr/bin/env python3
"""Compare mrt-text with Firefox on a few fonts and sizes: string widths (must agree within half an
app unit, 1/120 px), line height (MaxAscent + MaxDescent + ExternalLeading), baseline (MaxAscent) and
the ex unit (XHeight). Needs the example built: CARGO_TARGET_DIR=... cargo build --example textdump.
usage: crosscheck.py            exit status 1 if anything differs
"""
import json, os, subprocess, sys
HERE = os.path.dirname(os.path.abspath(__file__))
TGT = os.environ.get("CARGO_TARGET_DIR", os.path.join(HERE, "..", "target"))
EXE = os.path.join(TGT, "debug/examples/textdump")
CASES = [("Noto Sans", 16, 400), ("Noto Sans", 11.7, 400), ("DejaVu Sans", 13, 400), ("DejaVu Sans", 13.4, 400),
         ("DejaVu Sans Mono", 14, 700), ("Noto Serif", 20, 400), ("DejaVu Serif", 19.5, 700)]
bad = 0
for fam, size, weight in CASES:
    ours = json.loads(subprocess.check_output([EXE, fam, str(size), str(weight), "--json"]))
    ff = json.loads(subprocess.check_output([sys.executable, os.path.join(HERE, "firefox_text.py"), fam, str(size), str(weight)]))
    diffs = []
    for s, w in ff["widths"].items():
        if abs(ours["widths"][s] - w) > 0.5 / 60 + 1e-9: diffs.append((s, ours["widths"][s], w))
    for k in ("lineHeight", "baselineFromTop", "ex"):
        tol = 0.5 / 60 + 1e-9 if k == "ex" else 0
        if abs(ours[k] - ff[k]) > tol: diffs.append((k, ours[k], ff[k]))
    print(("OK   " if not diffs else "DIFF ") + f"{fam} {size}px w{weight}", *diffs)
    bad += bool(diffs)
sys.exit(1 if bad else 0)
