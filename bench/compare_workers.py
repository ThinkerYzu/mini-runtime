#!/usr/bin/env python3
"""Each site's workers on both sides: the runtime's `memory.workers` against Firefox's `workers/` in the
same replay (DESIGN, "Workers, W5 as built").

Firefox's side: `explicit/workers/workers(<site>)/worker(<url>, 0x...)` in the report of a replay run
without the scroll (`KEEP_REPORTS=1 NO_USE=1`, `<site>-live.json.gz` and `-live-minimized.json.gz` in
REPORTS_DIR), one row per worker, its whole explicit tree (runtime, zones, gc-heap). The chrome
workers of the parent process are left out. The mini side: each worker in `memory.workers` of
bench/mini_realsites_replay.json (the page's runtime, and a frame runtime's when its report has one):
malloc without `shared_immutable_strings` (the process-wide source cache every runtime reports in
full), the committed GC chunks, the GC buffers in use (session 84; not in the chunks) and the mapped
nursery and JIT code.

Workers are paired by owner site and URL. Firefox's report gives no worker's name or kind, and a site
may start several workers from one URL (Facebook: Messenger's and RST's from one `init_script`), so a
pair is a group of one URL on each side, with its total. Facebook's SharedWorker URL is the same with
`hash`, `use_broadcast` and `with_worker_id_lock` added; those are dropped for the key and the row
says "shared"; the runtime's shared workers (session 80) say so from their config.

With SPLIT=1 each group's two sides are also split into the same buckets (SPLIT_BUCKETS): Firefox's by
report path under the worker, the runtime's from the worker's `page` (its zone and realm, as
`mrt_js_memory_detail` names them) and `malloc` fields; what the runtime's buckets leave of its total
is "GC free, admin, other zones", where Firefox's `gc-heap/`, `unused-gc-things` and arena admin go;
Firefox's sundries and anything new are "other".

usage: [SPLIT=1] REPORTS_DIR=realsites-reports-workers python3 bench/compare_workers.py [SITE ...]
"""
import gzip
import json
import os
import re
import sys
from urllib.parse import parse_qsl, urlencode, urlsplit

HERE = os.path.dirname(os.path.abspath(__file__))
REPORTS = os.path.join(HERE, os.environ.get("REPORTS_DIR", "realsites-reports-nouse"))
# the query parameters Facebook's createSharedWorkerV2BundleUrl adds for a SharedWorker
SHARED_PARAMS = {"hash", "use_broadcast", "with_worker_id_lock"}
WORKER_PATH = re.compile(r"^explicit/workers/workers\(([^)]*)\)/worker\((.*), 0x[0-9a-f]+\)/")


# The buckets of SPLIT=1: a name, the regexp of Firefox's report path after `worker(...)/` (zone and
# realm addresses taken out), and the runtime's fields (`page/...` or `malloc/...`, `mapped/...`)
SPLIT_BUCKETS = [
    ("objects", r"^zone/realm/classes/", ["page/objects"]),
    ("scripts", r"^zone/realm/(scripts|jit-scripts|baseline-data|ion-data|alloc-sites)", ["page/scripts_gc_heap", "page/jit_scripts", "page/baseline_data", "page/ion_data", "page/alloc_sites"]),
    ("script data, sources", r"^(runtime/script-data|runtime/script-sources)", ["malloc/script_data", "malloc/script_sources"]),
    ("strings", r"^zone/strings/", ["page/strings"]),
    ("shapes, property maps", r"^zone/(shapes|property-maps|shape-tables|propmap-tables|getter-setters)", ["page/shapes", "page/prop_maps"]),
    ("scopes", r"^zone/scopes/", ["page/scopes"]),
    ("JIT", r"^(zone/code/|zone/jit-zone|zone/cacheir-stubs|zone/jit-codes|runtime/jit-lazylink)", ["page/jit_zone", "mapped/jit_code", "malloc/jit_lazy_link"]),
    ("regexp", r"^zone/regexp", ["page/regexp"]),
    # with the atoms zone's strings: on Firefox's side a zone of the worker with no realm
    # (firefox_workers), on the runtime's `other_zones/strings` (session 84, GAPS B89)
    ("atoms", r"^(runtime/atoms-|atoms-zone/)", ["malloc/atoms_table", "malloc/atoms_mark_bitmaps", "other_zones/strings"]),
    ("nursery", r"^runtime/gc/nursery-", ["mapped/nursery_committed", "malloc/gc_nursery_malloced_buffers"]),
    ("runtime, other", r"^runtime/", ["malloc/self_host_stencil", "malloc/runtime_object", "malloc/contexts", "malloc/temporary", "malloc/interpreter_stack", "malloc/gc_marker", "malloc/gc_store_buffer", "malloc/uncompressed_source_cache", "malloc/shared_intl_data", "malloc/wasm_runtime"]),
]


def firefox_bucket(rest):
    """The SPLIT_BUCKETS name of a report path under a worker, or "other"."""
    # zone(0x...) and realm(<url>): the names without what is in parentheses (a realm's URL writes
    # its slashes as backslashes, so splitting at "/" keeps it whole)
    rest = "/".join(part.split("(")[0] for part in rest.split("/"))
    for name, pattern, _ in SPLIT_BUCKETS:
        if re.match(pattern, rest):
            return name
    # the GC's free space and bookkeeping: what the runtime's buckets leave of its total
    if re.match(r"^(gc-heap/|zone/unused-gc-things|zone/gc-heap-arena-admin)", rest):
        return "GC free, admin, other zones"
    return "other"


def mini_split(js):
    """A worker runtime's bytes by SPLIT_BUCKETS, the rest under "GC free, admin, other zones"."""
    out = {}
    for name, _, fields in SPLIT_BUCKETS:
        out[name] = sum(js.get(f.split("/")[0], {}).get(f.split("/")[1], 0) for f in fields)
    out["GC free, admin, other zones"] = mini_size(js) - sum(out.values())
    return out


def key_of(url):
    """The pairing key of a worker's URL (path and query, without the SharedWorker parameters), and
    whether those parameters were there."""
    u = urlsplit(url)
    if u.scheme == "blob" or url.startswith("blob:"):
        # a blob URL's UUID differs every run: the key is its origin
        inner = urlsplit(url[len("blob:"):] if url.startswith("blob:") else u.path)
        return f"blob:{inner.scheme}://{inner.netloc}", False
    query = parse_qsl(u.query, keep_blank_values=True)
    kept = [(k, v) for k, v in query if k not in SHARED_PARAMS]
    return f"{u.path}?{urlencode(kept)}" if kept else u.path, len(kept) != len(query)


def registrable(host):
    """The last two labels of a host: Firefox's `workers(<site>)` name for these sites."""
    return ".".join((host or "").split(".")[-2:])


def firefox_workers(site, state):
    """{(owner site, key): [(MB, shared)]} from one Firefox report, or None if it is not there."""
    suffix = "-live.json.gz" if state == "live" else "-live-minimized.json.gz"
    path = os.path.join(REPORTS, site + suffix)
    if not os.path.exists(path):
        return None
    per_worker, splits = {}, {}
    reports = json.load(gzip.open(path))["reports"]
    # the zones of each worker that hold a realm; the one that does not is its atoms zone, whose
    # strings are the atoms (Firefox reports it as a zone, the runtime as other zones)
    realm_zones = set()
    for r in reports:
        m = WORKER_PATH.match(r["path"])
        z = m and re.match(r"(zone\(0x[0-9a-f]+\))/realm\(", r["path"][m.end():])
        if z:
            realm_zones.add((r["process"], r["path"][:m.end()], z.group(1)))
    for r in reports:
        m = WORKER_PATH.match(r["path"])
        if not m or m.group(1) == "chrome" or r["units"] != 0:
            continue
        ident = (r["process"], m.group(1), r["path"][:m.end()])
        per_worker[ident] = per_worker.get(ident, 0) + r["amount"]
        split = splits.setdefault(ident, {})
        rest = r["path"][m.end():]
        z = re.match(r"(zone\(0x[0-9a-f]+\))/", rest)
        if z and (r["process"], r["path"][:m.end()], z.group(1)) not in realm_zones:
            rest = "atoms-zone/" + rest[z.end():]
        bucket = firefox_bucket(rest)
        split[bucket] = split.get(bucket, 0) + r["amount"]
    out = {}
    for ident, amount in per_worker.items():
        _, owner, prefix = ident
        # the report writes the URL's path with backslashes and without its host
        url = WORKER_PATH.match(prefix).group(2).replace("\\", "/")
        key, shared = key_of(url)
        out.setdefault((owner, key), []).append((amount / 1e6, shared, splits[ident]))
    return out


def mini_size(js):
    """A worker runtime's bytes as Firefox counts its own (see the module comment)."""
    malloc = sum(v for k, v in js["malloc"].items() if k != "shared_immutable_strings")
    chunks = js["chunks"]["total"] - js["chunks"]["decommitted"]
    # the GC buffers in use (objects' slots and elements, scripts' and scopes' data): buffer chunks
    # are not in JSGC_TOTAL_CHUNKS (countTotalChunks counts the zones' arena chunks), and Firefox's
    # report counts the bytes in use under each category's gc-buffers, not the free ones (session 84)
    return malloc + chunks + js["chunks"].get("buffers_used", 0) + sum(js["mapped"].values())


def mini_workers(result):
    """{(owner site, key): [(MB, name)]} from a site's runtime result."""
    reports = [result] + [fr["report"] for fr in result.get("frames", []) if fr.get("report")]
    out = {}
    for rep in reports:
        for w in rep.get("memory", {}).get("workers", []):
            cfg = w["config"]
            key, _ = key_of(cfg["url"])
            owner = registrable(urlsplit(cfg.get("owner") or cfg["url"]).hostname)
            label = (cfg.get("name") or "") + (" shared" if cfg.get("kind") == "shared" else "")
            out.setdefault((owner, key), []).append((mini_size(w["js"]) / 1e6, label.strip(), mini_split(w["js"])))
    return out


def describe(rows, label):
    """'31.7 shared + 3.5' or '25.3 MAWMain... + 4.3 RST...' for one side of a group."""
    return " + ".join(f"{mb:.1f}" + (f" {label(x)}" if label(x) else "") for mb, x, _ in sorted(rows, key=lambda r: -r[0]))


def print_split(ff, mi):
    """One group's buckets, Firefox's live run against the runtime, in MB."""
    def total(rows):
        t = {}
        for _, _, split in rows:
            for k, v in split.items():
                t[k] = t.get(k, 0) + v
        return t
    a, b = total(ff), total(mi)
    for name in [n for n, _, _ in SPLIT_BUCKETS] + ["GC free, admin, other zones", "other"]:
        if name in a or name in b:
            print(f"      {name:30} {a.get(name, 0) / 1e6:6.2f} {b.get(name, 0) / 1e6:6.2f}")


def main():
    from recorded_tests import require_recordings
    require_recordings(os.path.join(HERE, "mini_realsites_replay.json"),
                       os.path.join(REPORTS, "*-live.json.gz"),
                       record="matching replay runs; Firefox KEEP_REPORTS=1 NO_USE=1 with REPORTS_DIR set")
    mini = json.load(open(os.path.join(HERE, "mini_realsites_replay.json")))
    names = sys.argv[1:] or list(mini)
    for name in names:
        live, minimized = firefox_workers(name, "live"), firefox_workers(name, "minimized")
        mm = mini_workers(mini.get(name, {}))
        if live is None:
            print(f"{name}: no Firefox report in {os.path.relpath(REPORTS, HERE)}; mini {len(sum(mm.values(), []))} workers")
            continue
        keys = sorted(set(live) | set(minimized or {}) | set(mm))
        print(f"{name}: {len(sum(live.values(), []))} workers in Firefox, {len(sum(mm.values(), []))} in the runtime")
        for owner, key in keys:
            ff, ffm, mi = live.get((owner, key), []), (minimized or {}).get((owner, key), []), mm.get((owner, key), [])
            ff_total, ffm_total, mi_total = (sum(row[0] for row in rows) for rows in (ff, ffm, mi))
            print(f"  {owner} {key[:90]}")
            print(f"    Firefox   {ff_total:6.1f} MB ({describe(ff, lambda s: 'shared' if s else '') or '-'}); "
                  f"minimized {ffm_total:.1f}")
            print(f"    runtime   {mi_total:6.1f} MB ({describe(mi, lambda n: n) or '-'})")
            if os.environ.get("SPLIT") == "1":
                print(f"      {'(MB)':30} {'Firefox':>6} {'runtime':>6}")
                print_split(ff, mi)


if __name__ == "__main__":
    main()
