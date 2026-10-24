"""The lean bridge's Fenix process with no runtime behind it (task bbc6's control).

For each site, release Fenix with a wiped profile, set up as mini_realsites.py's MRT_ADB bridge (the replay
proxy, JS off in the tab), opens the lean bridge document (the 404 at /__mrt_lean_bridge__ of the page's
origin) and nothing else: no document fetch, no runtime requests. After 20 s, a memory report of every
process with dumpsys meminfo, plain and minimized (mrt_phone.memory_reports), as BRIDGE_MEMORY takes them
while the runtime waits. The difference from a BRIDGE_MEMORY run is what serving the runtime left in
the bridge process.

usage (from a Firefox source tree): REPLAY=replay ONLY=cnn,amazon ./mach python lean_bridge_phone.py
  PACKAGE=org.mozilla.geckoview_example   the bridge in GeckoView's example app, with smaps (task 4898)
Writes bench/lean_bridge_phone.json: per site and state, the bridge process's and the prealloc
process's anonymous and unique MiB, heap-allocated, and the windows it holds.
"""
import json, os, re, sys, time
from urllib.parse import urlsplit

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import replay  # noqa: E402
import mrt_phone  # noqa: E402

SITES = {
    "cnn": "https://www.cnn.com/", "amazon": "https://www.amazon.com/", "reddit": "https://www.reddit.com/",
    "youtube": "https://www.youtube.com/", "facebook": "https://www.facebook.com/", "instagram": "https://www.instagram.com/",
    "gdocs": "https://docs.google.com/", "gmail": "https://mail.google.com/",
}
OUT = os.path.join(HERE, os.environ.get("OUT", "lean_bridge_phone.json"))


def rows(reps, ds, lean_url):
    """The process holding `lean_url`'s window and the prealloc process: MiB by dumpsys and the report."""
    key = "top(" + lean_url.replace("/", "\\")
    out = {}
    for proc in {r["process"] for r in reps}:
        tops = {r["path"].split("/")[2][:120] for r in reps if r["process"] == proc and r["path"].startswith("explicit/window-objects/top(")}
        role = "bridge" if any(t.startswith(key) for t in tops) else "prealloc" if proc.startswith("prealloc") else None
        if not role:
            continue
        split = mrt_phone.dumpsys_anon(ds.get(proc))
        heap = next((r["amount"] for r in reps if r["process"] == proc and r["path"] == "heap-allocated"), 0)
        out[role] = {"process": re.sub(r"\s*\(pid \d+\)", "", proc), "windows": sorted(tops),
                     "anonymous": split and round(split["anonymous"] / 1048576, 2),
                     "unique": split and round(split["unique"] / 1048576, 2),
                     "heap_allocated": round(heap / 1048576, 2), "dumpsys": split and split["dumpsys"]}
        if split and "smaps" in split:
            out[role]["smaps"] = split["smaps"]
    return out


def run(name, url):
    proxy = replay.start(name, "mini")
    m = mrt_phone.bridge(proxy)
    try:
        m.timeout.page_load = 120
        m.navigate("about:blank")
        m.set_context("chrome")
        m.execute_script(mrt_phone.JS_OFF, sandbox="system")
        m.set_context("content")
        u = urlsplit(url)
        lean = f"{u.scheme}://{u.netloc}/__mrt_lean_bridge__"
        m.navigate(lean)
        time.sleep(20)
        out = {"lean_url": lean}
        for state, minimize in (("plain", False), ("minimized", True)):
            reps, ds = mrt_phone.memory_reports(m, f"{name}-control-{state}", minimize)
            out[state] = rows(reps, ds, lean)
        return out
    finally:
        mrt_phone.close_bridge(m)
        proxy.close()


results = json.load(open(OUT)) if os.path.exists(OUT) else {}
for name in os.environ.get("ONLY", ",".join(SITES)).split(","):
    try:
        results[name] = run(name, SITES[name])
    except Exception as e:
        results[name] = {"error": repr(e)[:300]}
    r = results[name]
    print(name, r.get("error") or {st: {k: (v["anonymous"], v["heap_allocated"]) for k, v in r[st].items()} for st in ("plain", "minimized")}, flush=True)
    json.dump(results, open(OUT, "w"), indent=1)
os._exit(0)
