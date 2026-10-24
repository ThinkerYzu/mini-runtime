"""Where the mini process's memory is (GAPS A16), next to Firefox's content process for the same
recording.

The mini side is realsite's `memory` report (bench/mini_realsites.py's results); the Firefox side is
the site's content process in a memory report kept by `KEEP_REPORTS=1 REPORTS_DIR=...` from
bench/firefox_realsites.py (bench/ff_process_split.py reads it).

usage: python3 bench/split_mini_memory.py MINI_RESULTS.json [FIREFOX_REPORTS_DIR]
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import ff_process_split
import firefox_state  # FIREFOX_STATE=minimized reads Firefox after "Minimize memory usage" (GAPS A17)

# The site of each page's Firefox process (webIsolated=https://<site>); Gmail and Docs run in google.com's
HOSTS = {"cnn": "cnn.com", "amazon": "amazon.com", "reddit": "reddit.com", "youtube": "youtube.com",
         "gmail": "google.com", "gdocs": "google.com", "facebook": "facebook.com",
         "instagram": "instagram.com", "x": "x.com"}
MB = 1048576


def mini_rows(r):
    """(row, MB) of the mini process: the unique memory by what holds it, the heap by owner."""
    m = r["memory"]
    u, js, mi = m["unique"], m["js"], m["mallinfo"]
    jsm = js["malloc"]
    sources = jsm["shared_immutable_strings"] + jsm["script_sources"] + jsm["uncompressed_source_cache"]
    js_malloc = sum(jsm.values())
    heap = mi["in_use"] + mi["mmapped"]
    rust_other = m["rust_heap"] - m["dom"]
    unattributed = heap - m["rust_heap"] - js_malloc - m["wrapper_cache"]
    malloc_resident = u.get("malloc_thread_arenas", 0) + u.get("malloc_main_arena", 0) + mi["mmapped"]
    code = u.get("lib_mozjs", 0) + u.get("lib_realsite", 0) + u.get("libs_other", 0) + u.get("files_other", 0)
    stacks = u.get("stack_js_thread", 0) + u.get("stack_main", 0)
    gc = u.get("js_gc_chunks_tenured", 0) + u.get("js_gc_chunks_buffers", 0)
    other_anon = u.get("anon_other", 0) - mi["mmapped"] + u.get("js_jit_code", 0) + u.get("special", 0)
    total = sum(u.values())
    return [
        ("unique memory (sum of the split)", total),
        ("  uss_trimmed_mb (smaps_rollup)", r["uss_trimmed_mb"] * MB),
        ("  code and data of the binaries", code),
        ("  GC chunks (tenured + buffers)", gc),
        ("  nursery", u.get("js_nursery", 0)),
        ("  malloc, resident", malloc_resident),
        ("    in use", heap),
        ("      DOM (Rust)", m["dom"]),
        ("      Rust, other", rust_other),
        ("      JS: script sources", sources),
        ("      JS: script data (bytecode)", jsm["script_data"]),
        ("      JS: page zone and realm", jsm["page_zone"] + jsm["page_realm"]),
        ("      JS: atoms, other zones, runtime", js_malloc - sources - jsm["script_data"] - jsm["page_zone"] - jsm["page_realm"]),
        ("      C++ wrapper cache", m["wrapper_cache"]),
        ("      not attributed (C/C++ libraries)", unattributed),
        ("    free but resident", malloc_resident - heap),
        ("  stacks", stacks),
        ("  other anonymous", other_anon),
    ]


def firefox_rows(path, host):
    _, a = ff_process_split.split(path, host)
    g = lambda k: a.get(k, 0)
    return {
        "unique memory (sum of the split)": g("resident-unique"),
        "  GC chunks (tenured + buffers)": g("js-main-runtime-gc-heap-committed/used/gc-things/objects") and None,
        "  nursery": g("explicit/js-non-window/runtime/gc/nursery-committed"),
        "    in use": g("heap-allocated"),
        "      JS: script sources": g("explicit/js-non-window/runtime/shared-immutable-strings-cache") + g("explicit/js-non-window/runtime/script-sources"),
        "      JS: script data (bytecode)": g("explicit/js-non-window/runtime/script-data"),
        "      JS: atoms, other zones, runtime": g("explicit/js-non-window/runtime/atoms-table"),
    }


def main():
    results = json.load(open(sys.argv[1]))
    reports = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "realsites-reports-replay")
    for site, r in results.items():
        if "memory" not in r:
            continue
        ff_path = os.path.join(reports, site + firefox_state.report_suffix() + ".json.gz")
        ff = firefox_rows(ff_path, HOSTS[site]) if site in HOSTS and os.path.exists(ff_path) else {}
        print(f"\n{site}: mini JS heap {r['js_mb']:.1f} MB, DOM {r['dom_mb']:.1f} MB; idle GC waited {r.get('idle_gc_wait_ms', 0)} ms")
        print(f"{'':42}{'mini MB':>9}{'Firefox MB':>12}")
        for name, v in mini_rows(r):
            f = ff.get(name)
            print(f"{name:42}{v / MB:9.2f}{'' if f is None else f'{f / MB:12.2f}'}")


if __name__ == "__main__":
    main()
