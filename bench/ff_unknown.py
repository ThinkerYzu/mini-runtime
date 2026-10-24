"""The unknown memory of a Firefox content process, split by mapping (task 178d; DESIGN "The unknown memory
of a content process, attributed").

split_realsites.py calls "unknown" what the memory report does not explain: unique memory (USS) minus
explicit/ minus the allocator's own overhead. This splits the process's unique pages by where they are:
  file           private pages of a file mapping, by name in `files` (libxul's relocations and data, omni.ja)
  ipc-shm        memfd:mozilla-ipc mappings only this process maps (IPC shared memory)
  stacks         thread stacks: the mappings that hold a thread's stack pointer (/proc/PID/task/*/syscall)
  jit            anonymous executable mappings
  gc-tenured, gc-buffers, gc-nursery   pages of SpiderMonkey's GC chunks, by the chunk header at each 1 MiB
                 boundary (as bench/heap_compress.py)
  anon-other     every other anonymous page: mozjemalloc's heap, and anything else mmap'd
and sets each class against the reports of the memory in it, from the memory report taken just before
(report_side(), attribute()): GC chunks against the GC heap and buffer reports, stacks and JIT code against
theirs, anon-other against the heap the allocator has committed (heap-allocated plus its overhead) and the
other non-heap reports.

A page is unique when the pagemap marks it present and exclusively mapped (the same test smaps uses for
Private_*); content processes are forked from the fork server, so many anonymous pages are still shared
with it and are not counted. Reading another process's pagemap and mem needs ptrace access: call split()
from the harness that started Firefox (bench/firefox_realsites.py does at the live point).

usage: python3 bench/ff_unknown.py RESULTS.json REPORTS_DIR [--minimized]
  prints the table from a firefox_realsites.py run (KEEP_REPORTS=1): the live point, or with --minimized
  the same page after "Minimize memory usage"
"""
import collections, gzip, json, os, struct, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from heap_compress import CHUNK, PAGE, chunk_kind

PRESENT, EXCLUSIVE = 1 << 63, 1 << 56
SPECIAL = ("[vvar]", "[vdso]", "[vsyscall]", "[vvar_vclock]")


def stack_pointers(pid):
    """Every thread's stack pointer: the second-to-last field of /proc/PID/task/TID/syscall (a running
    thread shows "running" and is skipped)."""
    sps = []
    for tid in os.listdir(f"/proc/{pid}/task"):
        try:
            f = open(f"/proc/{pid}/task/{tid}/syscall").read().split()
        except OSError:
            continue
        if len(f) >= 3 and f[0] != "running":
            sps.append(int(f[-2], 16))
    return sps


def mappings(pid):
    """(start, end, perms, path, private bytes from smaps) for every mapping."""
    out, cur = [], None
    for line in open(f"/proc/{pid}/smaps"):
        parts = line.split()
        if "-" in parts[0] and ":" not in parts[0] and len(parts) >= 5:
            start, end = (int(x, 16) for x in parts[0].split("-"))
            cur = [start, end, parts[1], " ".join(parts[5:]) if len(parts) > 5 else "", 0]
            out.append(cur)
        elif parts[0] in ("Private_Clean:", "Private_Dirty:"):
            cur[4] += int(parts[1]) * 1024
    return out


def split(pid):
    """The unique memory of `pid` by class (see the module comment), in bytes, with `files` by name and
    `unique` the total (equal to smaps_rollup's Private_Clean + Private_Dirty)."""
    sps = stack_pointers(pid)
    mem = os.open(f"/proc/{pid}/mem", os.O_RDONLY)
    pagemap = os.open(f"/proc/{pid}/pagemap", os.O_RDONLY)
    out, files, kinds = collections.Counter(), collections.Counter(), {}
    try:
        for start, end, perms, path, private in mappings(pid):
            if path in SPECIAL or not private:
                continue
            if path.startswith("/memfd:mozilla-ipc"):
                out["ipc-shm"] += private
                continue
            if path.startswith("/"):
                name = os.path.basename(path.replace(" (deleted)", ""))
                files[name] += private
                out["file"] += private
                continue
            if any(start <= sp < end for sp in sps) or path.startswith("[stack"):
                out["stacks"] += private
                continue
            if "x" in perms:
                out["jit"] += private
                continue
            # anonymous: page by page, GC chunks by their header
            n = (end - start) // PAGE
            entries = os.pread(pagemap, n * 8, (start // PAGE) * 8)
            for i, (e,) in enumerate(struct.iter_unpack("<Q", entries)):
                if e & PRESENT and e & EXCLUSIVE:
                    addr = start + i * PAGE
                    out[chunk_kind(mem, addr & ~(CHUNK - 1), kinds) or "anon-other"] += PAGE
    finally:
        os.close(mem)
        os.close(pagemap)
    return {"unique": sum(out.values()), **out, "files": dict(files.most_common())}


def nonheap_class(path):
    """Which scanned class a non-heap explicit/ report's memory lives in (see split()), or "other"."""
    if "/gc-buffers" in path:
        return "gc-buffers"
    if "gc-heap" in path or "/gc/nursery" in path:
        return "gc-heap"
    if path.startswith("explicit/threads/stacks/"):
        return "stacks"
    if path.startswith("explicit/threads/overhead/kernel"):
        return "kernel"  # the kernel's own memory for the threads: in explicit/, never in USS
    if "/code/" in path:
        return "jit"
    if path.startswith("explicit/images/") and "nonheap" in path:
        return "images"  # decoded surfaces in shared memory the parent maps too: reported, not unique
    return "other"


def report_side(reps, proc):
    """The same process in its memory report, in bytes: resident-unique, heap-allocated, explicit/ and
    its heap reports, the non-heap reports by the class their memory is in (nonheap_<class>; gc-buffers
    reported as heap are `heap_gc_buffers`: they live in GC buffer chunks, not in the malloc heap), the
    allocator's overhead (heap/committed/ beyond allocated), heap-unclassified, and the unknown as
    split_realsites.py counts it (USS - explicit - overhead). `nonheap_other_paths`: the "other"
    non-heap reports by their first two path parts."""
    w, other = collections.Counter(), collections.Counter()
    for r in reps:
        if r["process"] != proc or r["units"] != 0:
            continue
        p, n = r["path"], r["amount"]
        if p.startswith("explicit/"):
            w["explicit"] += n
            if r["kind"] == 1:
                w["heap_reported"] += n
                if "/gc-buffers" in p:
                    w["heap_gc_buffers"] += n
            else:
                c = nonheap_class(p)
                w["nonheap_" + c] += n
                if c == "other":
                    other["/".join(p.split("/")[1:3])] += n
        elif p in ("heap-allocated", "resident-unique"):
            w[p] = n
        elif p.startswith("heap/committed/") and not p.startswith("heap/committed/allocated"):
            w["allocator_overhead"] += n
    w["heap_unclassified"] = w["heap-allocated"] - w["heap_reported"]
    w["unknown"] = w["resident-unique"] - w["explicit"] - w["allocator_overhead"]
    return {**w, "nonheap_other_paths": dict(other.most_common(8))}


def attribute(rep, maps):
    """The unknown (rep["unknown"]) split into named parts, MB: each scanned class less the reports of
    memory in it. Heap-unclassified is corrected for the gc-buffers reported as heap; the malloc heap's
    part is anon-other less heap-allocated, the allocator's overhead and the other non-heap reports;
    the kernel's thread memory and decoded images in shared memory are in explicit/ but not in USS, so
    they come out negative. The parts add
    up to the unknown exactly when every unique page is either scanned or reported once."""
    mb = lambda x: round(x / 1048576, 1)
    g = lambda k: rep.get(k, 0)
    parts = {
        "heap-unclassified": g("heap_unclassified") + g("heap_gc_buffers"),
        "GC buffer chunks beyond the reports (free, admin)": maps.get("gc-buffers", 0) - g("heap_gc_buffers") - g("nonheap_gc-buffers"),
        "GC tenured and nursery chunks beyond the reports": maps.get("gc-tenured", 0) + maps.get("gc-nursery", 0) - g("nonheap_gc-heap"),
        "file-backed private pages (libxul, omni.ja, ...)": maps.get("file", 0),
        "IPC shared memory only this process maps": maps.get("ipc-shm", 0),
        "decoded images in shared memory (reported, not unique)": -g("nonheap_images"),
        "thread stacks beyond the report": maps.get("stacks", 0) - g("nonheap_stacks"),
        "JIT code beyond the report": maps.get("jit", 0) - g("nonheap_jit"),
        "other anonymous, beyond the allocator and the reports": maps.get("anon-other", 0) - g("heap-allocated") - g("allocator_overhead") - g("nonheap_other"),
        "kernel thread memory (reported, not in USS)": -g("nonheap_kernel"),
    }
    # the scan runs after the report: the process's growth in between (the scan's unique minus the
    # report's resident-unique) comes out here
    parts["unaccounted (the process grew between report and scan)"] = g("unknown") - sum(parts.values())
    return {k: mb(v) for k, v in parts.items()}


if __name__ == "__main__":
    ORDER = ["facebook", "instagram", "gdocs", "gmail", "x", "youtube", "gmaps", "reddit", "amazon", "cnn"]
    results = json.load(open(sys.argv[1]))
    minimized = "--minimized" in sys.argv
    field, report = ("unknown_split_minimized", "-live-minimized") if minimized else ("unknown_split", "-live")
    rows = []
    for site in ORDER:
        v = results.get(site)
        path = os.path.join(sys.argv[2], site + report + ".json.gz")
        if not v or not v.get(field) or not os.path.exists(path):
            continue
        reps = json.load(gzip.open(path))["reports"]
        # the site's process by its pid (its name in the minimized report is the same)
        proc = next(r["process"] for r in reps if r["process"].endswith(f"(pid {v[field]['pid']})"))
        rep = report_side(reps, proc)
        rows.append((site, rep, attribute(rep, v[field]), v[field]))
    if not rows:
        raise SystemExit("no site with unknown_split and its live report")
    keys = list(rows[0][2])
    print("| Part | " + " | ".join(r[0] for r in rows) + " |")
    print("|---|" + "---:|" * len(rows))
    print("| **unknown** | " + " | ".join(f"**{r[1]['unknown'] / 1048576:.1f}**" for r in rows) + " |")
    for k in keys:
        print(f"| {k} | " + " | ".join(f"{r[2][k]:.1f}" for r in rows) + " |")
    print("| (allocator overhead, counted as free) | " + " | ".join(f"{r[1]['allocator_overhead'] / 1048576:.1f}" for r in rows) + " |")
    print("| (scan's unique against the report's) | " + " | ".join(f"{r[3]['unique'] / 1048576:.1f} / {r[1]['resident-unique'] / 1048576:.1f}" for r in rows) + " |")
    print("\nother non-heap reports:", {r[0]: {k: round(b / 1048576, 1) for k, b in r[1]["nonheap_other_paths"].items()} for r in rows})
    print("\nfiles:", {r[0]: {k: round(b / 1048576, 1) for k, b in list(r[3]["files"].items())[:4]} for r in rows})
