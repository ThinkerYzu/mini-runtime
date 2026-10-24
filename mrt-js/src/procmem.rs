//! Where this process's memory is (GAPS A16): a counting allocator for the Rust heap, and the
//! process's unique memory split by what each mapping holds.
//!
//! Everything in the process allocates through glibc malloc (SpiderMonkey is built with
//! `--disable-jemalloc`), so the heap in use is Rust's, SpiderMonkey's and the C++ layer's together.
//! The Rust part is counted by `CountingAlloc` when a binary installs it as its global allocator;
//! SpiderMonkey's comes from its own memory report (`Page::memory_detail`).

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

unsafe extern "C" {
    fn malloc_usable_size(p: *mut u8) -> usize;
    fn mincore(addr: *mut u8, len: usize, vec: *mut u8) -> i32;
    fn pthread_self() -> usize;
    fn pthread_getattr_np(thread: usize, attr: *mut u64) -> i32;
    fn pthread_attr_getstack(attr: *const u64, addr: *mut usize, size: *mut usize) -> i32;
    fn pthread_attr_destroy(attr: *mut u64) -> i32;
    fn sbrk(increment: isize) -> *mut u8;
}

static RUST_LIVE: AtomicUsize = AtomicUsize::new(0);

/// glibc malloc through `System`, counting the usable size of every live block, so that the count
/// is in the same units as `mallinfo2().uordblks` and SpiderMonkey's `malloc_usable_size` figures.
pub struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            RUST_LIVE.fetch_add(unsafe { malloc_usable_size(p) }, Ordering::Relaxed);
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            RUST_LIVE.fetch_add(unsafe { malloc_usable_size(p) }, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        RUST_LIVE.fetch_sub(unsafe { malloc_usable_size(p) }, Ordering::Relaxed);
        unsafe { System.dealloc(p, layout) }
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = unsafe { malloc_usable_size(p) };
        let q = unsafe { System.realloc(p, layout, new_size) };
        if !q.is_null() {
            RUST_LIVE.fetch_sub(old, Ordering::Relaxed);
            RUST_LIVE.fetch_add(unsafe { malloc_usable_size(q) }, Ordering::Relaxed);
        }
        q
    }
}

/// Bytes of the Rust heap in use (0 unless the binary installed `CountingAlloc`).
pub fn rust_heap() -> usize {
    RUST_LIVE.load(Ordering::Relaxed)
}

const PAGE: usize = 4096;
const MIB: usize = 1 << 20;
/// glibc's HEAP_MAX_SIZE on 64-bit: each heap of a thread arena is a mapping aligned to it.
const ARENA_HEAP: usize = 64 << 20;

/// The range [lo, hi) of the calling thread's stack.
pub fn thread_stack() -> (usize, usize) {
    let mut attr = [0u64; 8];
    let (mut addr, mut size) = (0usize, 0usize);
    unsafe {
        if pthread_getattr_np(pthread_self(), attr.as_mut_ptr()) != 0 {
            return (0, 0);
        }
        pthread_attr_getstack(attr.as_ptr(), &mut addr, &mut size);
        pthread_attr_destroy(attr.as_mut_ptr());
    }
    (addr, addr + size)
}

/// Resident pages of [lo, hi), at most 1 MiB, from mincore, into `out` (one entry per page). For
/// the private anonymous mappings of a process that has not forked since, resident is unique.
/// Nothing is allocated: the split must not make heap pages resident while it counts them.
fn resident(lo: usize, hi: usize, out: &mut [u8; MIB / PAGE]) -> usize {
    let n = (hi - lo) / PAGE;
    if n == 0 || unsafe { mincore(lo as *mut u8, hi - lo, out.as_mut_ptr()) } != 0 {
        out[..n].fill(0);
    }
    n
}

struct Mapping {
    lo: usize,
    hi: usize,
    perms: String,
    path: String,
    private: usize,
}

fn mappings() -> Vec<Mapping> {
    let mut out: Vec<Mapping> = Vec::new();
    for line in std::fs::read_to_string("/proc/self/smaps").unwrap_or_default().lines() {
        let mut it = line.split_whitespace();
        let Some(first) = it.next() else { continue };
        if let Some((a, b)) = first.split_once('-')
            && let (Ok(lo), Ok(hi)) = (usize::from_str_radix(a, 16), usize::from_str_radix(b, 16))
        {
            let perms = it.next().unwrap_or("").to_string();
            let path = it.nth(3).map(|_| line.splitn(6, char::is_whitespace).nth(5).unwrap_or("").trim().to_string());
            out.push(Mapping { lo, hi, perms, path: path.unwrap_or_default(), private: 0 });
        } else if first == "Private_Clean:" || first == "Private_Dirty:" {
            let kb: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            if let Some(m) = out.last_mut() {
                m.private += kb * 1024;
            }
        }
    }
    out
}

/// What a 1 MiB-aligned block of an anonymous mapping is, read from its first bytes: a SpiderMonkey
/// chunk (its ChunkBase header, js/public/HeapAPI.h, as bench/heap_compress.py reads it) or the start
/// of a heap of a glibc thread arena (its heap_info, malloc/arena.c).
fn block_kind(base: usize, first_page_resident: bool, hi: usize) -> Option<(&'static str, usize)> {
    if !first_page_resident {
        return None;
    }
    // Safe to read: the page is resident and the mapping is readable (checked by the caller).
    let w = |o: usize| unsafe { std::ptr::read_volatile((base + o) as *const usize) };
    let (store, runtime) = (w(0), w(8));
    let b = unsafe { std::ptr::read_volatile((base + 16) as *const [u8; 2]) };
    let (kind, index) = (b[0], b[1]);
    let tenured = (kind == 1 || kind == 2) && store == 0 && index == 255;
    let nursery = (kind == 3 || kind == 4) && store != 0 && index != 255;
    if (tenured || nursery) && runtime != 0 && runtime % 8 == 0 && runtime < 1 << 47 {
        let name = match kind {
            1 => "js_gc_chunks_tenured",
            2 => "js_gc_chunks_buffers",
            _ => "js_nursery",
        };
        return Some((name, (base + MIB).min(hi)));
    }
    if base % ARENA_HEAP == 0 {
        // heap_info: ar_ptr, prev, size. The arena's first heap holds its malloc_state right after
        // heap_info; a later heap points back to the previous one.
        let (ar_ptr, prev, size) = (w(0), w(8), w(16));
        let first = prev == 0 && ar_ptr > base && ar_ptr < base + 256;
        let later = prev != 0 && prev % ARENA_HEAP == 0 && ar_ptr % 16 == 0 && ar_ptr != 0;
        if (first || later) && size % PAGE == 0 && size > 0 && size <= ARENA_HEAP {
            return Some(("malloc_thread_arenas", (base + size).min(hi)));
        }
    }
    None
}

/// The process's unique memory (Private_Clean + Private_Dirty) in bytes, by what holds it:
///   malloc_main_arena, malloc_thread_arenas   glibc's heaps (in use and free)
///   js_gc_chunks_tenured, js_gc_chunks_buffers, js_nursery   SpiderMonkey's chunks, by header
///   js_jit_code        executable anonymous mappings
///   stack_js_thread    the stack of the thread `stack` belongs to; stack_main the main thread's
///   anon_other         every other anonymous page: glibc's mmapped blocks, SpiderMonkey's large
///                      buffers, other threads' stacks, ...
///   lib_mozjs, lib_realsite, libs_other, files_other   file-backed private pages (relocations,
///                      data, bss)
pub fn split_unique(stack: (usize, usize)) -> BTreeMap<&'static str, usize> {
    let mut out: BTreeMap<&'static str, usize> = BTreeMap::new();
    for m in mappings() {
        if m.private == 0 {
            continue;
        }
        let file = m.path.starts_with('/');
        if file {
            let name = if m.path.contains("libmozjs") {
                "lib_mozjs"
            } else if m.path.ends_with("/realsite") || m.path.ends_with("/jscount") {
                "lib_realsite"
            } else if m.path.contains(".so") {
                "libs_other"
            } else {
                "files_other"
            };
            *out.entry(name).or_default() += m.private;
            continue;
        }
        match m.path.as_str() {
            "[heap]" => {
                *out.entry("malloc_main_arena").or_default() += m.private;
                continue;
            }
            "[stack]" => {
                *out.entry("stack_main").or_default() += m.private;
                continue;
            }
            "" => {}
            _ if m.path.starts_with("[anon") => {}
            _ => {
                *out.entry("special").or_default() += m.private;
                continue;
            }
        }
        if m.perms.contains('x') {
            *out.entry("js_jit_code").or_default() += m.private;
            continue;
        }
        if !m.perms.starts_with('r') {
            *out.entry("anon_other").or_default() += m.private;
            continue;
        }
        // walk the mapping 1 MiB at a time, classifying 1 MiB blocks and arena heaps by their
        // headers; a class found at a block's start holds until `class_end`
        let mut buf = [0u8; MIB / PAGE];
        let (mut class, mut class_end) = ("anon_other", m.lo);
        let (mut lo, mut counted) = (m.lo, 0);
        while lo < m.hi {
            let hi = (lo + 1).next_multiple_of(MIB).min(m.hi);
            let n = resident(lo, hi, &mut buf);
            if lo >= class_end {
                class = "anon_other";
                if lo % MIB == 0
                    && let Some((name, end)) = block_kind(lo, buf[0] & 1 != 0, m.hi)
                {
                    (class, class_end) = (name, end);
                }
            }
            for (i, r) in buf[..n].iter().enumerate() {
                if r & 1 == 0 {
                    continue;
                }
                let addr = lo + i * PAGE;
                let c = if addr >= stack.0 && addr < stack.1 {
                    "stack_js_thread"
                } else if addr < class_end {
                    class
                } else {
                    "anon_other"
                };
                *out.entry(c).or_default() += PAGE;
                counted += PAGE;
            }
            lo = hi;
        }
        // MRT_SPLIT_CHECK=1: a mapping whose resident pages (mincore) differ from what smaps counted
        // as private when it was read; the split's sum should equal smaps_rollup's unique memory
        if counted != m.private && std::env::var_os("MRT_SPLIT_CHECK").is_some() {
            eprintln!("SPLITCHECK {:x}-{:x} {} {:?} resident {counted} private {}", m.lo, m.hi, m.perms, m.path, m.private);
        }
    }
    out
}

/// What `arena_walk` found in glibc's arenas, in bytes.
#[derive(Default)]
pub struct ArenaWalk {
    /// the free chunks
    pub free: usize,
    /// resident pages lying wholly inside a free chunk, which malloc_trim should have given back
    pub free_resident: usize,
    /// of `free_resident`, in the arenas' top chunks
    pub top_resident: usize,
    /// of `free_resident`, in free chunks that are in none of their arena's bins, which malloc_trim
    /// cannot see (thread arenas only: the main arena's bins are not found)
    pub unbinned_resident: usize,
    /// free bytes on pages that also hold a live block: what no trim can give back
    pub free_in_used_pages: usize,
    /// `free_in_used_pages` by how much of its page is free: under 1/4, 1/4 to 1/2, 1/2 to 3/4,
    /// 3/4 or more
    pub free_by_fill: [usize; 4],
    /// on pages 3/4 or more free, the live bytes by the size of their block: bucket i holds blocks
    /// of 2^(i+4) to 2^(i+5) bytes; with the number of such pages the blocks of each bucket touch
    pub sparse_live: [(usize, usize); 24],
    /// all resident pages of the arenas' heaps
    pub resident: usize,
    /// of `resident`, the main arena's
    pub main_resident: usize,
    /// heaps whose walk did not end at the heap's end; their figures are left out
    pub unparsed_heaps: usize,
}

/// Per-page scratch for `walk_heap`, allocated once before any walk: an allocation during the walk
/// would change the heap being walked.
struct PageMaps {
    res: Vec<bool>,
    free_on: Vec<usize>,
    kind_on: Vec<u8>,
}

fn word(a: usize) -> usize {
    unsafe { std::ptr::read_volatile(a as *const usize) }
}

/// Walk one heap's chunks, from `first` to `end`, adding to `out`. `page0` is the heap's first page;
/// `top` its arena's top chunk; `bins` the arena's binned free chunks, if known. False when the walk
/// does not end at `end`, and then nothing is added.
fn walk_heap(out: &mut ArenaWalk, maps: &mut PageMaps, page0: usize, first: usize, end: usize, top: usize, bins: Option<&std::collections::HashSet<usize>>) -> bool {
    let pages = (end - page0).div_ceil(PAGE);
    if pages > maps.res.len() {
        return false;
    }
    let mut buf = [0u8; MIB / PAGE];
    let mut lo = page0;
    let page_end = page0 + pages * PAGE;
    while lo < page_end {
        let hi = (lo + 1).next_multiple_of(MIB).min(page_end);
        let n = resident(lo, hi, &mut buf);
        for i in 0..n {
            maps.res[(lo - page0) / PAGE + i] = buf[i] & 1 != 0;
        }
        lo = hi;
    }
    maps.free_on[..pages].fill(0);
    maps.kind_on[..pages].fill(0);
    // a chunk is free when the next one lacks PREV_INUSE (tcache chunks count as live, as mallinfo
    // counts them); the top chunk, or a thread heap's fencepost, ends the heap
    let (mut free, mut ok) = (0usize, false);
    let mut p = first;
    while p + 16 <= end {
        let sz = word(p + 8) & !7;
        if sz < 16 || p + sz > end {
            break;
        }
        let next = p + sz;
        let is_free = next + 16 <= end && word(next + 8) & 1 == 0 || next == end;
        if is_free {
            free += sz;
            // the top chunk is the one that reaches the heap's end (the main arena's has no malloc_state
            // here to name it)
            let kind = if p == top || next == end {
                1
            } else if bins.is_none_or(|b| b.contains(&p)) {
                2
            } else {
                3
            };
            let mut q = p + 16;
            while q < next {
                let pg = (q - page0) / PAGE;
                let pe = (page0 + (pg + 1) * PAGE).min(next);
                maps.free_on[pg] += pe - q;
                maps.kind_on[pg] = kind;
                q = pe;
            }
        }
        if next == end {
            ok = true;
            break;
        }
        if next + 16 > end || (word(next + 8) & !7) == 0 {
            // a fencepost (size 0 or tiny) at the end of a full thread heap
            ok = end - next < 64;
            break;
        }
        p = next;
    }
    if !ok {
        return false;
    }
    out.free += free;
    let sparse = |maps: &PageMaps, i: usize| maps.res[i] && maps.free_on[i] < PAGE && maps.free_on[i] >= PAGE * 3 / 4;
    for i in 0..pages {
        if !maps.res[i] {
            continue;
        }
        out.resident += PAGE;
        let f = maps.free_on[i];
        if f >= PAGE {
            out.free_resident += PAGE;
            match maps.kind_on[i] {
                1 => out.top_resident += PAGE,
                3 => out.unbinned_resident += PAGE,
                _ => {}
            }
        } else {
            out.free_in_used_pages += f;
            out.free_by_fill[(f * 4 / PAGE).min(3)] += f;
        }
    }
    // second pass: the live blocks on sparse pages, by size
    let mut p = first;
    while p + 16 <= end {
        let sz = word(p + 8) & !7;
        let next = p + sz;
        if next >= end {
            break;
        }
        if word(next + 8) & 1 != 0 {
            let bucket = (usize::BITS - 1 - sz.leading_zeros()).saturating_sub(4).min(23) as usize;
            let mut q = p;
            while q < next {
                let pg = (q - page0) / PAGE;
                let pe = (page0 + (pg + 1) * PAGE).min(next);
                if sparse(maps, pg) {
                    out.sparse_live[bucket].0 += pe - q;
                    out.sparse_live[bucket].1 += 1;
                }
                q = pe;
            }
        }
        p = next;
    }
    true
}

/// Walk every heap of glibc's arenas chunk by chunk: the main arena (the `[heap]` mapping, from its
/// start to `sbrk(0)`) and each heap of the thread arenas, and count where the free memory is (see
/// `ArenaWalk`). Layout of glibc 2.43 on x86-64 (no fastbins): heap_info is 48 bytes, malloc_state
/// 2112. For GAPS A19 and B95.
pub fn arena_walk() -> ArenaWalk {
    const HEAP_INFO: usize = 48;
    const STATE: usize = 2112;
    let mut out = ArenaWalk::default();
    let maps_all = mappings();
    let main = maps_all.iter().find(|m| m.path == "[heap]").map(|m| (m.lo, m.hi));
    // room for the main arena as it is now plus 64 MiB, or for one thread heap
    let cap = (main.map_or(0, |(lo, hi)| hi - lo) + ARENA_HEAP) / PAGE;
    let mut maps = PageMaps { res: vec![false; cap], free_on: vec![0; cap], kind_on: vec![0; cap] };
    // each thread arena's binned free chunks: malloc_state's bins start at offset 24 (after the lock,
    // flags, top and last_remainder), two pointers a bin; bin_at(i) is &bins[(i - 1) * 2] - 16 and a
    // chunk's bk is at offset 24
    let mut binned: std::collections::HashMap<usize, std::collections::HashSet<usize>> = Default::default();
    let mut heaps = Vec::new();
    let mut buf = [0u8; MIB / PAGE];
    for m in &maps_all {
        if m.path != "" || !m.perms.starts_with("rw") {
            continue;
        }
        let mut base = m.lo.next_multiple_of(ARENA_HEAP);
        while base + PAGE <= m.hi {
            let here = base;
            base += ARENA_HEAP;
            if resident(here, here + PAGE, &mut buf) != 1 || buf[0] & 1 == 0 {
                continue;
            }
            let (ar_ptr, prev, size) = (word(here), word(here + 8), word(here + 16));
            let first = prev == 0 && ar_ptr == here + HEAP_INFO;
            let later = prev != 0 && prev % ARENA_HEAP == 0 && ar_ptr % 16 == 0 && ar_ptr != 0;
            if !(first || later) || size % PAGE != 0 || size == 0 || size > ARENA_HEAP || here + size > m.hi {
                continue;
            }
            binned.entry(ar_ptr).or_insert_with(|| {
                let mut set = std::collections::HashSet::new();
                for i in 1..128 {
                    let b = ar_ptr + 24 + (i - 1) * 16 - 16;
                    let mut p = word(b + 24);
                    let mut n = 0;
                    while p != b && p != 0 && n < 1_000_000 {
                        set.insert(p);
                        p = word(p + 24);
                        n += 1;
                    }
                }
                set
            });
            let chunk0 = if first { (ar_ptr + STATE).next_multiple_of(16) } else { here + HEAP_INFO };
            heaps.push((here, chunk0, here + size, ar_ptr));
        }
    }
    for (here, chunk0, end, ar_ptr) in heaps {
        if !walk_heap(&mut out, &mut maps, here, chunk0, end, word(ar_ptr + 8), binned.get(&ar_ptr)) {
            out.unparsed_heaps += 1;
        }
    }
    // the main arena last, after every allocation of this walk: its top chunk ends at the break
    if let Some((lo, _)) = main {
        let end = unsafe { sbrk(0) } as usize;
        let before = out.resident;
        if walk_heap(&mut out, &mut maps, lo, lo.next_multiple_of(16), end, usize::MAX, None) {
            out.main_resident = out.resident - before;
        } else {
            out.unparsed_heaps += 1;
        }
    }
    out
}
