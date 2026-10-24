"""Who owns a Firefox process's heap-unclassified memory, from a DMD dump (task 178d; DESIGN "The unknown
memory of a content process, attributed").

A DMD build run with DMD=1 (dark-matter mode) writes every live heap block with its allocation stack and,
for blocks a memory reporter measured, where it was reported (bench/firefox_realsites.py DMD_DUMP=1 keeps
the site's process's dump as bench/realsites-dmd/<site>.json.gz). The blocks no reporter measured are
heap-unclassified. Their frames are named with addr2line on the build's own binaries (the obj-hib build
has a symbol table but no debug info, which fix-stacks, dmd.py's symbolizer, does not fall back to), and
each block is put under:
  site    the first frame that is not an allocator, DMD, or a generic container or string growing
  owner   the first OWNERS rule that matches the site, or else the first that matches a frame of the
          stack in stack order (innermost first), or "other"
Blocks DMD gave no stack (small ones, in its default partial-stacks mode) are counted apart.

Run DMD with full stacks (DMD="--mode=dark-matter --stacks=full"): with partial stacks a small block
gets one only by chance, so the blocks without one are not spread like the rest.

usage: python3 bench/dmd_owners.py DMD.json.gz [--sites N]      one dump: owners and the top N sites
       python3 bench/dmd_owners.py A.json.gz B.json.gz ...      several: owners against sites (a table)
"""
import collections, gzip, json, os, re, subprocess, sys

# frames that say nothing about who wanted the memory
GENERIC = re.compile(
    r"^(mozilla::dmd::|replace_|moz_x|malloc|calloc|realloc|free|posix_memalign|memalign|aligned_alloc|"
    r"operator new|js_malloc|js_calloc|js_realloc|js_pod_|js::MallocProvider|js::SystemAllocPolicy|"
    r"js::TempAllocPolicy|js::MallocArena|ArenaAllocator|nsTArray|nsTArray_base|nsTArray_Impl|"
    r"AutoTArray|mozilla::Vector|mozilla::detail::VectorImpl|mozilla::detail::HashTable|mozilla::HashMap|"
    r"mozilla::HashSet|PLDHashTable|nsTHashtable|nsBaseHashtable|nsTSubstring|nsTString|nsTAutoString|"
    r"nsTStringRepr|mozilla::detail::nsTStringRepr|nsStringBuffer|mozilla::StringBuffer|"
    r"std::|alloc::|__rust|<alloc::|core::|hashbrown::|smallvec::|thin_vec::|nsCString|nsString|"
    r"AppendUTF|CopyUTF|AppendASCII|Copy(ASCII|UTF)|ToNewCString|ToNewUnicode|NS_strdup|moz_strdup|strdup|"
    r"Gecko_StartBulkWriteString|Gecko_.*String|__tls_get_addr|nsstring::|nsstring_|\?\?\?)")

# owner rules, first match in stack order wins: (owner, pattern on a frame's function name)
OWNERS = [
    ("DMD's own tables", r"^mozilla::dmd::(?!StackTrace::Get|AllocCallback)"),
    ("fonts: FreeType, fontconfig, HarfBuzz, gfxFont", r"FT_|^Fc|fontconfig|hb_|gfxFont|gfxFT2|gfxFcPlatform|gfxPlatformFontList|gfxShapedText|gfxTextRun|SharedFontList|fontlist::"),
    ("ICU", r"^icu_\d+::|^u[a-z]+_.*_\d+$|^icu4x|^icu_"),
    ("JS: parser, bytecode, JIT, regexp (unreported)", r"^js::(frontend|jit|irregexp|wasm|ScriptSource|ImmutableScriptData|SharedImmutableScriptData|StencilXDR)|^v8::internal|^JS::|^js::"),
    ("network: Necko, HTTP cache, cookies", r"^mozilla::net::|^nsHttp|^nsSocket|^nsStandardURL|^nsIOService|^nsCookie|nsDNS|^CacheFile"),
    ("IPC and actors", r"^IPC::|^mozilla::ipc::|^mojo::|^base::|Parent::Recv|Child::Recv|ParamTraits"),
    ("graphics: WebRender, gfx, layers, canvas", r"^mozilla::(gfx|layers|wr)::|^wr_|^webrender|^gfx|^Skia|^Sk[A-Z]|^cairo_|^pixman"),
    ("style: Servo (Rust)", r"^style::|^servo_|^Servo_|^selectors::|^cssparser::"),
    ("layout and text", r"^ns[A-Z]\w*Frame|^nsLineLayout|^nsTextFrame|^mozilla::(PresShell|RestyleManager|TextControl)|^nsLayoutUtils|^nsBidi|^nsLineBreaker|^mozilla::intl::"),
    ("DOM and its bindings", r"^mozilla::dom::|^nsINode|^nsContent|^nsGenericHTML|^nsDocument|^nsHTML|^nsAttr|^nsNodeInfo|^nsGlobalWindow|^nsJSUtils|^XPC|^xpc::|^nsXPC"),
    ("startup: preferences, components, observers, XPCOM", r"^mozilla::Preferences|^Preferences|^nsComponentManager|^nsObserverService|^nsCategoryManager|^mozilla::scache|^nsZipArchive|^mozilla::Omnijar|^nsChromeRegistry|^NS_InitXPCOM|^XRE_|^nsAppRunner|^mozilla::BootstrapImpl"),
    ("image decoding", r"^mozilla::image::|^png_|^jpeg_|^WebP|^dav1d|^nsJPEG|^nsPNG"),
    ("media: the media cache, demuxers, decoders", r"^mozilla::(Media|AudioStream|Decoder|MemoryBlockCache|FileBlockCache|MP4|Moof|MoofParser|Box|WebM|MediaCache)|^cubeb|^ffvpx|^av_|^mp4parse"),
    ("compression (zlib, brotli, zstd)", r"^zlib_rs|^deflate|^inflate|^BrotliDecoder|^brotli|^ZSTD"),
    ("NSS and NSPR", r"^PR_|^PL_|^NSS|^SEC|^PK11|^CERT|^nss|^pr_"),
    ("threads and event queues", r"^nsThread|^mozilla::TaskController|^mozilla::EventQueue|^mozilla::TaskQueue|^mozilla::detail::RunnableFunction|^mozilla::Runnable"),
]
OWNERS = [(o, re.compile(p)) for o, p in OWNERS]
# "#00: ??? (module + 0xoff)", or "#00: ???[module +0xoff]" as some dumps write it
FRAME = re.compile(r"^#\d+: (.*?) ?[(\[](\S+) \+ ?(0x[0-9a-f]+)[)\]]$")


def symbolize(frames):
    """{frame id: function name} for DMD's frame table (its "#NN: ??? (module + 0xoff)" entries, FRAME), with
    addr2line once per module. Frames DMD already named keep their name."""
    names, by_module = {}, collections.defaultdict(list)
    for fid, text in frames.items():
        m = FRAME.match(text)
        if not m:
            names[fid] = text
        elif m.group(1) != "???":
            names[fid] = m.group(1)
        else:
            by_module[m.group(2)].append((fid, m.group(3)))
    for module, items in by_module.items():
        out = subprocess.run(["addr2line", "-f", "-C", "-e", module] + [off for _, off in items],
                             capture_output=True, text=True).stdout.splitlines()
        for (fid, off), fn in zip(items, out[0::2]):
            names[fid] = fn if fn != "??" else f"??? ({module.rsplit('/', 1)[-1]} + {off})"
    return names


def owners(path):
    """Heap-unclassified of a DMD dump, bytes: (total, by owner, by site, without a stack)."""
    d = json.load(gzip.open(path))
    names = symbolize(d["frameTable"])
    traces = {t: [names[f] for f in fs] for t, fs in d["traceTable"].items()}
    by_owner, by_site, total, no_stack = collections.Counter(), collections.Counter(), 0, 0
    for b in d["blockList"]:
        if "reps" in b:
            continue
        size = (b["req"] + b.get("slop", 0)) * b.get("num", 1)
        total += size
        stack = traces.get(b.get("alloc"))
        if not stack:
            no_stack += size
            continue
        site = next((f for f in stack if not GENERIC.match(f)), "(only generic frames)")
        owner = next((o for o, rx in OWNERS if rx.search(site)), None) or \
            next((o for f in stack for o, rx in OWNERS if rx.search(f)), "other")
        by_owner[owner] += size
        by_site[(owner, site)] += size
    return total, by_owner, by_site, no_stack


def table(paths):
    """Owners against sites, MB, for several dumps (each file's name without .json.gz is the site)."""
    rows = {os.path.basename(p).replace(".json.gz", ""): owners(p) for p in paths}
    names = collections.Counter()
    for total, by_owner, _, _ in rows.values():
        names.update(by_owner)
    print("| Owner | " + " | ".join(rows) + " |")
    print("|---|" + "---:|" * len(rows))
    mb = lambda x: f"{x / 1048576:.1f}"
    print("| **heap-unclassified (DMD)** | " + " | ".join(f"**{mb(r[0])}**" for r in rows.values()) + " |")
    for o, _ in names.most_common():
        print(f"| {o} | " + " | ".join(mb(r[1].get(o, 0)) for r in rows.values()) + " |")
    print("| (no stack) | " + " | ".join(mb(r[3]) for r in rows.values()) + " |")
    for site, (_, _, by_site, _) in rows.items():
        print(f"\n{site}: " + "; ".join(f"{mb(v)} {s[:70]}" for (o, s), v in by_site.most_common(6)))


if __name__ == "__main__":
    files = [a for a in sys.argv[1:] if a.endswith(".json.gz")]
    if len(files) > 1:
        table(files)
        sys.exit(0)
    n = int(sys.argv[sys.argv.index("--sites") + 1]) if "--sites" in sys.argv else 25
    total, by_owner, by_site, no_stack = owners(sys.argv[1])
    mb = lambda x: f"{x / 1048576:.1f}"
    print(f"heap-unclassified in DMD's dump: {mb(total)} MB ({mb(no_stack)} without a stack)")
    for o, v in by_owner.most_common():
        print(f"  {mb(v):>6}  {o}")
    print(f"\ntop {n} allocation sites:")
    for (o, s), v in by_site.most_common(n):
        print(f"  {mb(v):>6}  [{o}] {s[:110]}")
