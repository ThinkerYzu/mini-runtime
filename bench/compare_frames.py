#!/usr/bin/env python3
"""The memory of a page's frames on both sides, process against process (DESIGN, "Frames grouped by
site", G4; SPEC decision 19).

Both sides run a site's frames together. Firefox (Fission) puts all of a tab's frames of one site
(scheme and registrable domain) in one `webIsolated=<site>` process, and the frames of the page's own
site in the page's process; the runtime does the same since session 68: the page's runtime runs the
frames of its site, and each other site's frames share one runtime. Firefox's side: the content
processes of the replay run without the scroll (bench/firefox_realsites_replay_nouse.json,
browser.live). The mini side: each runtime's unique memory after trimming
(bench/mini_realsites_replay.json, the page's and `frames`, one entry per frame runtime with the
frames it runs).

usage: python3 bench/compare_frames.py [SITE ...]
"""
import json
import os
import sys
from urllib.parse import urlsplit

HERE = os.path.dirname(os.path.abspath(__file__))


def site_of(url):
    """scheme://registrable-domain, the key of Firefox's webIsolated processes (the last two labels;
    enough for the hosts these sites use)."""
    u = urlsplit(url)
    host = u.hostname or ""
    return f"{u.scheme}://" + ".".join(host.split(".")[-2:])


def main():
    from recorded_tests import require_recordings
    require_recordings(os.path.join(HERE, "firefox_realsites_replay_nouse.json"),
                       os.path.join(HERE, "mini_realsites_replay.json"),
                       record="matching NO_USE=1 REPLAY=replay Firefox and runtime runs; see the script header")
    firefox = json.load(open(os.path.join(HERE, "firefox_realsites_replay_nouse.json")))
    mini = json.load(open(os.path.join(HERE, "mini_realsites_replay.json")))
    names = sys.argv[1:] or [n for n in mini if n in firefox]
    for name in names:
        f, m = firefox.get(name, {}), mini.get(name, {})
        if "browser" not in f or "elements_all" not in m:
            continue
        page_site = site_of(m.get("document_url") or m.get("url"))
        ff = {}
        for key, p in f["browser"]["live"]["processes"].items():
            kind = key.split(" (pid")[0]
            if kind.startswith("webIsolated=") and kind[len("webIsolated="):] != page_site:
                ff[kind[len("webIsolated="):]] = p["uss"]
        mm = {}
        for fr in m.get("frames", []):
            if not fr.get("report"):
                continue
            # the runtime's site as Firefox gave it (results before session 68: from the URL)
            s = fr.get("site") or site_of(fr["url"])
            mm.setdefault(s, []).append((fr["report"]["uss_trimmed_mb"], len(fr.get("frames") or [fr["url"]])))
        sites = sorted(set(ff) | set(mm))
        print(f"{name}: page site {page_site}")
        if not sites:
            print("  no cross-site frame on either side")
        for s in sites:
            mini_list = mm.get(s, [])
            # one runtime per site since session 68; more is an older result
            mini_total = (f"{sum(mb for mb, _ in mini_list):.1f} ({len(mini_list)} runtime{'s' if len(mini_list) > 1 else ''}, "
                          f"{sum(n for _, n in mini_list)} frames)") if mini_list else "-"
            ff_total = f"{ff[s]:.1f}" if s in ff else "-"
            print(f"  {s:40} Firefox {ff_total:>8} MB   mini {mini_total}")
        # the page's process against the page's runtime, which runs its site's frames (a result before
        # session 68 may still list runtimes of the page's site: added to the page's, as decision 17 did)
        own = [fr for fr in m.get("frames", []) if (fr.get("site") or site_of(fr["url"])) == page_site and fr.get("report")]
        page_ff = next((p["uss"] for key, p in f["browser"]["live"]["processes"].items() if key.split(" (pid")[0] == "webIsolated=" + page_site), None)
        page_mini = m.get("uss_trimmed_mb")
        own_mb = sum(fr["report"]["uss_trimmed_mb"] for fr in own)
        if page_ff is not None and page_mini is not None:
            extra = f" + {own_mb:.1f} for {len(own)} frame runtimes of its own site = {page_mini + own_mb:.1f}" if own else ""
            print(f"  {'the page (' + page_site + ')':40} Firefox {page_ff:>8.1f} MB   mini {page_mini:.1f}{extra}")


if __name__ == "__main__":
    main()
