#!/usr/bin/env python3
"""Phase 6.5 (task 49749, SPEC decision 27): the API process measured against the lean bridge.

usage: api_summary.py TAB.json API.json FIREFOX.json
  TAB.json      mini_realsites.py's results with REPLAY=replay BRIDGE_LEAN=1 BRIDGE_MEMORY=1: every call
                served by a lean bridge tab
  API.json      the same with API_PROCESS=all, the same day: the kinds mrt-api implements served there,
                a lean tab made at the first call only the tab serves
  FIREFOX.json  firefox_realsites.py's results on the same recordings (firefox_realsites_21c4_desktop.json)

MiB. The runtime leaves out its binaries' pages (as android/mini_phone_summary.py); the page's runtime
only, its cross-origin frames' runtimes apart. Firefox is its site process's anonymous memory (unique
less file-backed and shared memory), plain and minimized. The bridge tab is its process's anonymous
memory (`bridge_memory`), plain and minimized; mrt-api is the anonymous memory of the page's site's
process (`api_memory`), plain and after its `trim` op, with all of the run's mrt-api processes after it.
  live       runtime uss_mb + tab plain (+ mrt-api plain), against Firefox plain
  pressure   runtime uss_pressure_mb + tab minimized (+ mrt-api trimmed), against Firefox minimized
Then the time per call by kind: the tab's Marionette round trips (`tab_ops` of TAB.json) against the
API process's pipe round trips (`api_process` of API.json), every site together.
"""
import json, sys

MB = 1048576
SITES = ("facebook", "instagram", "gdocs", "gmail", "x", "youtube", "reddit", "amazon", "cnn")


def runtime(v):
    """The page's runtime without its binaries: live and under pressure."""
    bins = lambda u: sum(u.get(k, 0) for k in ("lib_mozjs", "lib_realsite", "libs_other", "files_other")) / MB
    return v["uss_mb"] - bins(v["memory"]["unique"]), v["uss_pressure_mb"] - bins(v["memory_pressure"]["unique"])


def tab(v):
    """The bridge tab's process, anonymous, plain and minimized; None when the run made no tab."""
    b = v.get("bridge_memory") or {}
    rows = [next((r for r in b.get(st) or [] if r["bridge"]), None) for st in ("plain", "minimized")]
    return tuple(r["anonymous"] for r in rows) if all(rows) else None


def api(v):
    """mrt-api, anonymous: the page's site's process plain and trimmed, then every process's."""
    mem = v.get("api_memory") or {}
    site = next(iter(mem), None)
    if site is None:
        return None
    total = lambda st: sum(p[st]["anonymous"] for p in mem.values() if st in p)
    return mem[site]["plain"]["anonymous"], mem[site]["trimmed"]["anonymous"], total("plain"), total("trimmed"), len(mem)


def firefox(v):
    anon = lambda u: (u["unique"] - u.get("file", 0) - u.get("ipc-shm", 0)) / MB if u else None
    return anon(v.get("unknown_split")), anon(v.get("unknown_split_minimized"))


def main():
    tabrun, apirun, ff = (json.load(open(p)) for p in sys.argv[1:4])
    print("site | Firefox plain / min | tab run: runtime live / pressure, tab plain / min | API run: runtime live / pressure, "
          "tab plain / min, mrt-api plain / trimmed (all n: plain / trimmed) | live: tab run, API run, against Firefox plain "
          "| pressure: tab run, API run, against Firefox minimized | elements tab / API")
    for s in SITES:
        t, a = tabrun.get(s), apirun.get(s)
        if not t or not a or "error" in t or "error" in a:
            print(s, "no run", (t or {}).get("error"), (a or {}).get("error"))
            continue
        fp, fm = firefox(ff[s])
        tl, tp = runtime(t)
        al, ap = runtime(a)
        tt, at, ai = tab(t), tab(a), api(a)
        live_t, press_t = tl + tt[0], tp + tt[1]
        live_a = al + (at[0] if at else 0) + ai[2]
        press_a = ap + (at[1] if at else 0) + ai[3]
        print(f"{s} | {fp:.1f} / {fm:.1f} | {tl:.1f} / {tp:.1f}, {tt[0]:.1f} / {tt[1]:.1f} | {al:.1f} / {ap:.1f}, "
              + (f"{at[0]:.1f} / {at[1]:.1f}" if at else "no tab")
              + f", {ai[0]:.1f} / {ai[1]:.1f} (all {ai[4]}: {ai[2]:.1f} / {ai[3]:.1f}) | "
              f"{live_t:.1f} {live_t / fp:.2f}x, {live_a:.1f} {live_a / fp:.2f}x | {press_t:.1f} {press_t / fm:.2f}x, {press_a:.1f} {press_a / fm:.2f}x | "
              f"{t.get('elements_all')} / {a.get('elements_all')}")

    print()
    print("kind | tab: calls, ms, ms a call | API process: calls, ms, ms a call | the API run's tab: calls, ms a call")
    kinds = {}
    for s in SITES:
        for k, x in (tabrun.get(s, {}).get("tab_ops") or {}).items():
            kinds.setdefault(k, [0, 0.0, 0, 0.0, 0, 0.0])
            kinds[k][0] += x["calls"]
            kinds[k][1] += x["ms"]
        ap = apirun.get(s, {}).get("api_process") or {}
        for k, n in ap.get("calls", {}).items():
            kinds.setdefault(k, [0, 0.0, 0, 0.0, 0, 0.0])
            kinds[k][2] += n
            kinds[k][3] += ap["ms"][k]
        for k, x in (apirun.get(s, {}).get("tab_ops") or {}).items():
            kinds.setdefault(k, [0, 0.0, 0, 0.0, 0, 0.0])
            kinds[k][4] += x["calls"]
            kinds[k][5] += x["ms"]
    per = lambda n, ms: f"{ms / n:.2f}" if n else "-"
    for k, (tn, tms, an, ams, rn, rms) in sorted(kinds.items(), key=lambda kv: -kv[1][0]):
        print(f"{k} | {tn}, {tms:.0f}, {per(tn, tms)} | {an}, {ams:.0f}, {per(an, ams)} | {rn}, {per(rn, rms)}")


if __name__ == "__main__":
    main()
