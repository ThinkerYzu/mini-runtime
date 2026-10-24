"""Stage 6: the droppable share of each site's content process, plain and minimized (task ee98).

usage: python3 bench/droppable_realsites.py RESULTS.json bench/REPORTS_DIR   (from mini-runtime; a run of
firefox_realsites.py with KEEP_REPORTS=1, without EXTRA=1)
Classes from split_realsites.py; the unknown split by ff_unknown.py; heap-unclassified divided by
task 178d's DMD owners (state = DOM + JS, fixed = per process, the rest rebuildable).
"""
import os, re, statistics, subprocess, sys

res, rep = sys.argv[1], sys.argv[2]
ORDER = ["facebook", "instagram", "gdocs", "gmail", "x", "youtube", "gmaps", "reddit", "amazon", "cnn"]
# 178d DMD: total, DOM, JS, per process (MB)
DMD = {"facebook": (17.4, 2.6, 2.1, 1.6), "instagram": (10.7, 0.8, 0.3, 1.5), "gdocs": (35.4, 2.4, 0.9, 1.5),
       "gmail": (32.8, 10.5, 1.9, 1.5), "x": (9.4, 1.6, 0.7, 1.5), "youtube": (17.4, 1.8, 3.9, 1.7),
       "reddit": (18.6, 3.9, 2.3, 1.7), "amazon": (23.3, 6.4, 1.5, 1.8), "cnn": (24.5, 1.9, 0.4, 1.7)}
FS = {s: (d + j) / t for s, (t, d, j, p) in DMD.items()}
FF = {s: p / t for s, (t, d, j, p) in DMD.items()}
FS["gmaps"] = statistics.median(FS.values()); FF["gmaps"] = statistics.median(FF.values())


def table(out):
    rows = {}
    head = None
    for line in out.splitlines():
        if not line.startswith("|") or line.startswith("|---"):
            continue
        cells = [c.strip().strip("*") for c in line.strip("|").split("|")]
        if head is None:
            head = cells
            continue
        rows[cells[0]] = cells[1:]
    return head, rows


def num(c):
    return float(c)


for state in ("plain", "minimized"):
    env = dict(os.environ, REPORTS_DIR=os.path.basename(rep.rstrip("/")), FIREFOX_STATE=state)
    head, split = table(subprocess.run(["python3", "bench/split_realsites.py", res], env=env,
                                       capture_output=True, text=True).stdout)
    args = ["python3", "bench/ff_unknown.py", res, rep] + (["--minimized"] if state == "minimized" else [])
    uhead, unk = table(subprocess.run(args, capture_output=True, text=True).stdout)
    col = {s: i for i, s in enumerate(uhead[1:])}
    print(f"\n### {state}\n")
    print("| Site | Process | State | Rebuildable | Free | Fixed | Left | Droppable, process kept | Process goes |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    lo, hi = [], []
    for s in ORDER:
        if s not in split or s not in col:
            continue
        r = split[s]
        st, rb, fr, fx, un = (num(r[i]) for i in (1, 2, 3, 4, 5))
        u = lambda key: num(next(v for k, v in unk.items() if k.startswith(key))[col[s]])
        hu = u("heap-unclassified")
        p_state = st + hu * FS[s]
        p_rb = rb + hu * (1 - FS[s] - FF[s]) + u("decoded images")
        p_fr = fr + u("GC buffer chunks") + u("GC tenured")
        p_fx = fx + hu * FF[s] + u("file-backed") + u("IPC shared") + u("thread stacks")
        proc = st + rb + fr + fx + un
        left = proc - p_state - p_rb - p_fr - p_fx
        # a negative left is the allocator's dirty pages counted as free but already given back
        p_fr += min(left, 0)
        d1 = (p_rb + p_fr) / proc
        d2 = (proc - p_state) / proc
        lo.append(d1); hi.append(d2)
        print(f"| {s} | {proc:.0f} | {p_state:.0f} | {p_rb:.0f} | {p_fr:.0f} | {p_fx:.0f} | {left:.0f} | "
              f"{d1:.0%} ({p_rb + p_fr:.0f} MB) | {d2:.0%} ({proc - p_state:.0f} MB) |")
    print(f"\nrange: process kept {min(lo):.0%} to {max(lo):.0%}; process goes {min(hi):.0%} to {max(hi):.0%}")
