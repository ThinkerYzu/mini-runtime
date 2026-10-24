"""The site's content process in a Firefox memory report, in the categories realsite's `memory`
report uses (GAPS A16): JS runtime, GC heap, nursery, malloc heap, unique memory.

usage: python3 bench/ff_process_split.py REPORT.json.gz SITE_HOST
"""
import collections, gzip, json, sys


def split(path, host):
    reps = json.load(gzip.open(path))["reports"]
    procs = collections.defaultdict(list)
    for r in reps:
        procs[r["process"]].append(r)
    # the process whose name carries the site (webIsolated=https://cnn.com)
    name = next(p for p in procs if p.startswith("webIsolated=https://" + host + " "))
    a = collections.Counter()
    for r in procs[name]:
        if r["units"] != 0:
            continue
        p, n = r["path"], r["amount"]
        if r["kind"] == 2 or not p.startswith("explicit/"):
            a[p] += n  # single values: resident-unique, heap-allocated, js-main-runtime-*
        if p.startswith("explicit/"):
            a["explicit"] += n
            parts = p.split("/")
            a["/".join(parts[:2])] += n
            if p.startswith("explicit/js-non-window/runtime/"):
                a["/".join(parts[:4])] += n
            if p.startswith("explicit/js-non-window/runtime/gc/"):
                a[p] += n
            if p.startswith("explicit/js-non-window/gc-heap/"):
                a["/".join(parts[:4])] += n
            if "/js-zone(" in p or "/js-realm(" in p:
                a["window js-zone+js-realm"] += n
            if "/script-sources" in p or "shared-immutable-strings" in p:
                a["sources (any path)"] += n
    return name, a


if __name__ == "__main__":
    name, a = split(sys.argv[1], sys.argv[2])
    print("==", name)
    for k, v in sorted(a.items(), key=lambda x: -x[1]):
        if v >= 100_000:
            print(f"{v / 1048576:9.2f}  {k}")
