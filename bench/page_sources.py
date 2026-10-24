"""The sources of a replayed page (HTML, CSS, JS, data, fonts, images, ...) and how small they compress,
to set against a compressed page (SPEC decision 14; DESIGN "Compressing a hibernated page").

The bodies come from the site's recording (realsites-replay/<site>/record.flows and fill.flows), so
both sides' requests are read against the same bytes. Two request lists per site:
  firefox  every response of Firefox's replay run (firefox-replay.jsonl), without Firefox's own
           traffic (Mozilla's services, DNS over HTTPS)
  mini     the mini runtime's document and its own requests (realsites-mini/<site>-requests.jsonl):
           what it needs to load the page again; no images, fonts or media (it does not load them)
Each body is counted once per URL. Sizes: as served (the transfer encoding the server used: gzip,
brotli, ...), decoded, and the decoded bodies of a type concatenated and compressed with `zstd -1`
and `zstd -19`, as a store of sources would hold them.

usage (mitmproxy's Python, which reads the recordings):
  bench/.venv-mitm/bin/python bench/page_sources.py [SITE ...]   (default: cnn amazon reddit youtube)
Writes bench/page_sources.json.
"""
import json, os, re, subprocess, sys
from urllib.parse import unquote
from collections import defaultdict
from mitmproxy import io as mio

HERE = os.path.dirname(os.path.abspath(__file__))
REPLAY = os.path.join(HERE, "realsites-replay")
MINI = os.path.join(HERE, "realsites-mini")
FIREFOX_OWN = re.compile(r"mozilla|firefox|cloudflare-dns|mozaws|mozgcp|mozit")
# the pages, as in mini_realsites.py and firefox_realsites.py
URLS = {
    "youtube": "https://www.youtube.com/watch?v=jNQXAC9IVRw",
    "reddit": "https://www.reddit.com/r/AskReddit/top/?t=week",
    "amazon": "https://www.amazon.com/s?k=laptop",
    "cnn": "https://www.cnn.com/",
    "gmail": "https://mail.google.com/mail/u/0/#inbox",
}
TYPES = ["html", "js", "css", "json", "font", "image", "media", "other"]


def norm(url):
    """A URL as both logs can be matched on: without its fragment, percent-decoded."""
    return unquote(url.split("#")[0])


def kind(content_type, url):
    """The source type of a response, by its media type, then by its URL."""
    t = (content_type or "").split(";")[0].strip().lower()
    path = url.split("?")[0].lower()
    if "html" in t:
        return "html"
    if "javascript" in t or "ecmascript" in t or path.endswith((".js", ".mjs")):
        return "js"
    if t == "text/css" or path.endswith(".css"):
        return "css"
    if "json" in t or "protobuf" in t or "x-www-form" in t:
        return "json"
    if t.startswith("font/") or "font" in t or path.endswith((".woff", ".woff2", ".ttf", ".otf")):
        return "font"
    if t.startswith("image/"):
        return "image"
    if t.startswith(("video/", "audio/")) or "mp2t" in t:
        return "media"
    return "other"


def bodies(site):
    """{(method, url): (content type, bytes as served, decoded bytes)} from the site's recordings."""
    out = {}
    for name in ("record.flows", "fill.flows"):
        path = os.path.join(REPLAY, site, name)
        if not os.path.exists(path):
            continue
        with open(path, "rb") as f:
            for flow in mio.FlowReader(f).stream():
                r = getattr(flow, "response", None)
                if r is None or r.raw_content is None:
                    continue
                try:
                    decoded = r.get_content(strict=False) or b""
                except Exception:
                    decoded = r.raw_content
                key = (flow.request.method, norm(flow.request.pretty_url))
                out.setdefault(key, (r.headers.get("content-type", ""), len(r.raw_content), decoded))
    return out


def requests(site, side):
    """[(method, url)] the side asked for, in order, each once."""
    seen, out = set(), []
    if side == "firefox":
        lines = open(os.path.join(REPLAY, site, "firefox-replay.jsonl"))
    else:
        lines = open(os.path.join(MINI, f"{site}-requests.jsonl"))
    for line in lines:
        e = json.loads(line)
        url = e.get("url", "")
        if side == "firefox" and FIREFOX_OWN.search(url.split("/")[2] if "//" in url else url):
            continue
        if url.startswith("data:"):
            continue  # no network: the data is in the URL, which is page state
        key = (e.get("method", "GET"), norm(url))
        if key not in seen:
            seen.add(key)
            out.append(key)
    return out


# Google's module bundles, matched as replay_addon.py serves them: the same modules asked for (m=), by a
# page that had no more modules (exm=) than this one; the recorded bundle with the most of them
EXCLUDED_PATH = re.compile(r"/exm=([^/?]*)")


def module_key(key):
    method, url = key
    path = url.split("?", 1)[0]
    if "/m=" not in path or not EXCLUDED_PATH.search(path):
        return None
    return method, EXCLUDED_PATH.sub("/exm=", path)


def excluded_modules(url):
    m = EXCLUDED_PATH.search(url.split("?", 1)[0])
    return set(m.group(1).split(",")) if m else set()


def bundle_served(key, by_module):
    """The recorded key replay_addon.py answers a module bundle `key` with, or None."""
    have = excluded_modules(key[1])
    fits = [k for k in by_module.get(module_key(key), []) if excluded_modules(k[1]) <= have]
    return max(fits, key=lambda k: len(excluded_modules(k[1]))) if fits else None


def zstd(data, level):
    if not data:
        return 0
    return len(subprocess.run(["zstd", f"-{level}", "-c", "-q"], input=data, capture_output=True, check=True).stdout)


def table(site, side, recorded, document):
    by_type = defaultdict(lambda: {"count": 0, "served": 0, "decoded": 0, "data": []})
    missing = 0
    keys = requests(site, side)
    if side == "mini":
        # the document, as the bridge tab fetched it (the runtime's requests do not include it)
        b = by_type["html"]
        b["count"] += 1
        b["decoded"] += len(document)
        b["data"].append(document)
        doc = recorded.get(("GET", norm(URLS[site])))
        b["served"] += doc[1] if doc else len(document)
        b["unrecorded_document"] = doc is None
    by_module = defaultdict(list)
    for k in recorded:
        if module_key(k):
            by_module[module_key(k)].append(k)
    served_keys = set()
    for key in keys:
        if key not in recorded:
            key = bundle_served(key, by_module)
            if key is None:
                missing += 1
                continue
        if key in served_keys:
            continue  # one recorded bundle served for two paths: its bytes are loaded once
        served_keys.add(key)
        ct, served, decoded = recorded[key]
        b = by_type[kind(ct, key[1])]
        b["count"] += 1
        b["served"] += served
        b["decoded"] += len(decoded)
        b["data"].append(decoded)
    out = {}
    for t in TYPES:
        if t not in by_type:
            continue
        b = by_type[t]
        blob = b"".join(b["data"])
        out[t] = {"count": b["count"], "served": b["served"], "decoded": b["decoded"], "zstd1": zstd(blob, 1), "zstd19": zstd(blob, 19)}
    total = {k: sum(v[k] for v in out.values()) for k in ("count", "served", "decoded", "zstd1", "zstd19")}
    return {"types": out, "total": total, "not_recorded": missing}


def main():
    sites = sys.argv[1:] or ["cnn", "amazon", "reddit", "youtube"]
    results = {}
    for site in sites:
        recorded = bodies(site)
        document = open(os.path.join(MINI, f"{site}.html"), "rb").read()
        results[site] = {side: table(site, side, recorded, document) for side in ("firefox", "mini")}
        for side in ("firefox", "mini"):
            r = results[site][side]
            mb = lambda n: f"{n / 1048576:7.2f}"
            print(f"{site} {side}: {r['total']['count']} responses ({r['not_recorded']} not in the recording)")
            print(f"  {'type':6} {'count':>5} {'served':>8} {'decoded':>8} {'zstd-1':>8} {'zstd-19':>8}  MB")
            for t, v in list(r["types"].items()) + [("total", r["total"])]:
                print(f"  {t:6} {v['count']:5} {mb(v['served'])} {mb(v['decoded'])} {mb(v['zstd1'])} {mb(v['zstd19'])}")
    # the sites run now replace theirs; the others' are kept
    out = os.path.join(HERE, "page_sources.json")
    merged = json.load(open(out)) if os.path.exists(out) else {}
    merged.update(results)
    json.dump(merged, open(out, "w"), indent=1)


if __name__ == "__main__":
    main()
