"""How well a process's memory compresses, split into SpiderMonkey's GC heap and the rest (DESIGN,
"Compressing a hibernated page").

Reads the resident pages of every private anonymous mapping of a process (/proc/PID/maps, pagemap, mem)
and sorts each page into:
  gc-tenured   pages of SpiderMonkey's tenured GC chunks (ChunkKind TenuredArenas)
  gc-buffers   pages of its buffer chunks (ChunkKind Buffers: object slots, elements, ...)
  gc-nursery   pages of its nursery chunks
  other        every other resident anonymous page (malloc heaps, arenas, stacks)
A GC chunk is found by its header at the 1 MiB boundary (js/public/HeapAPI.h ChunkBase): storeBuffer,
runtime, kind, nurseryChunkIndex; a tenured chunk has no store buffer and index 255, a nursery chunk
both. Each class is compressed with lz4 -1 and zstd -1 (the command-line tools), as zram would see it.
Pages that are all zero are counted apart (zram stores them without data).

Reading another process's memory needs ptrace access: with kernel.yama.ptrace_scope = 1 only an
ancestor may, so the harnesses call scan() from their own process (they started Firefox or realsite).

usage: python3 bench/heap_compress.py PID     (for a process this shell started)
"""
import os, struct, subprocess, sys

PAGE = os.sysconf("SC_PAGE_SIZE")
CHUNK = 1 << 20
ZERO = bytes(PAGE)
KINDS = {1: "gc-tenured", 2: "gc-buffers", 3: "gc-nursery", 4: "gc-nursery"}
SPECIAL = ("[vvar]", "[vdso]", "[vsyscall]", "[vvar_vclock]")


def anonymous_mappings(pid):
    for line in open(f"/proc/{pid}/maps"):
        parts = line.split(None, 5)
        start, end = (int(x, 16) for x in parts[0].split("-"))
        perms, inode = parts[1], parts[4]
        path = parts[5].strip() if len(parts) > 5 else ""
        if inode != "0" or "r" not in perms or "p" not in perms or path in SPECIAL:
            continue
        if path and not (path.startswith("[heap]") or path.startswith("[anon") or path.startswith("[stack")):
            continue
        yield start, end


def resident_pages(pagemap, start, end):
    """The addresses of the pages in [start, end) that are in memory."""
    n = (end - start) // PAGE
    entries = os.pread(pagemap, n * 8, (start // PAGE) * 8)
    for i, (e,) in enumerate(struct.iter_unpack("<Q", entries)):
        if e >> 63 & 1:
            yield start + i * PAGE


def chunk_kind(mem, base, cache):
    """The kind of SpiderMonkey chunk at the 1 MiB boundary `base`, or None."""
    if base not in cache:
        kind = None
        try:
            store, runtime, k, index = struct.unpack("<QQBB", os.pread(mem, 18, base))
            tenured = k in (1, 2) and store == 0 and index == 255
            nursery = k in (3, 4) and store != 0 and index != 255
            if (tenured or nursery) and runtime and runtime % 8 == 0 and runtime < 1 << 47:
                kind = KINDS[k]
        except OSError:
            pass
        cache[base] = kind
    return cache[base]


def compressed(data, cmd):
    return len(subprocess.run(cmd, input=bytes(data), capture_output=True, check=True).stdout)


def scan(pid):
    """Resident anonymous memory of `pid` by class: pages, MB, zero pages, and MB after lz4 -1 and zstd -1."""
    mem = os.open(f"/proc/{pid}/mem", os.O_RDONLY)
    pagemap = os.open(f"/proc/{pid}/pagemap", os.O_RDONLY)
    data = {k: bytearray() for k in ("gc-tenured", "gc-buffers", "gc-nursery", "other")}
    zero = dict.fromkeys(data, 0)
    kinds = {}
    try:
        for start, end in anonymous_mappings(pid):
            for addr in resident_pages(pagemap, start, end):
                try:
                    page = os.pread(mem, PAGE, addr)
                except OSError:
                    continue
                cls = chunk_kind(mem, addr & ~(CHUNK - 1), kinds) or "other"
                if page == ZERO:
                    zero[cls] += 1
                else:
                    data[cls] += page
    finally:
        os.close(mem)
        os.close(pagemap)
    mb = lambda n: round(n / 1e6, 3)
    out = {}
    for cls, d in data.items():
        out[cls] = {"pages": len(d) // PAGE + zero[cls], "zero_pages": zero[cls], "mb": mb(len(d)),
                    "lz4_mb": mb(compressed(d, ["lz4", "-1", "-c"])) if d else 0.0,
                    "zstd1_mb": mb(compressed(d, ["zstd", "-1", "-c", "-q"])) if d else 0.0}
    return out


def compress_file(path):
    """A file's MB, and MB after lz4 -1 and zstd -1."""
    d = open(path, "rb").read()
    return {"mb": round(len(d) / 1e6, 3), "lz4_mb": round(compressed(d, ["lz4", "-1", "-c"]) / 1e6, 3),
            "zstd1_mb": round(compressed(d, ["zstd", "-1", "-c", "-q"]) / 1e6, 3)}


if __name__ == "__main__":
    for cls, r in scan(int(sys.argv[1])).items():
        print(cls, r)
