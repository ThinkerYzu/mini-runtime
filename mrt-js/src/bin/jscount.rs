//! jscount: parse a page, run its inline scripts in SpiderMonkey (then DOMContentLoaded, load and
//! the timers), collect the JS garbage, then style, build the frame tree and reflow at 1489 x 844, as
//! layoutcount does. Prints one JSON line: the counts, the document height, the live Rust heap bytes
//! after each phase and the page's JS memory (its realm and zone, from JS::CollectRuntimeStats).
//! usage: jscount <file.html> [--console] [--timer-limit MS] [--hibernate | --cycles N]
//!   --console: print console.log lines to stderr as they come
//!   --hibernate: then stage 4 (a "hibernate" object in the JSON): drop the frame tree and the style
//!     data, keeping the DOM and JS; wake (style, frames, reflow again) and check the frame count, height
//!     and live bytes against the first build; save the DOM to bytes and restore it, checking the dump
//!   --cycles N: N hibernate and wake cycles instead of one (memory after each, to show it does not grow)
//!
//! The Rust heap (DOM, style, frames) is counted by the allocator wrapper below; SpiderMonkey and the
//! C++ layer allocate with malloc directly, so the JS numbers come from SpiderMonkey's own report, and
//! the wrapper cache (C++) is reported on its own.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use mrt_js::mrt_layout::driver::{ConstructFramesTrackingMode, DestroyFrameTree, FrameCountsByType, ReflowFrames};
use mrt_js::mrt_style::mrt_dom::{Dom, Kind};
use mrt_js::mrt_style::StyleDom;
use mrt_js::{JsSizes, Page, HEIGHT, WIDTH};

/// Counts live heap bytes, as layoutcount does.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            LIVE.fetch_add(l.size(), Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            if new >= l.size() {
                LIVE.fetch_add(new - l.size(), Relaxed);
            } else {
                LIVE.fetch_sub(l.size() - new, Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

fn live() -> i64 {
    LIVE.load(Relaxed) as i64
}

/// glibc's malloc statistics (struct mallinfo2): SpiderMonkey (jemalloc is off in obj-js), the C++
/// layer and the Rust heap all allocate through glibc malloc here, so this is the process's heap.
#[repr(C)]
#[derive(Default)]
struct MallInfo2 {
    arena: usize,
    ordblks: usize,
    smblks: usize,
    hblks: usize,
    hblkhd: usize,
    usmblks: usize,
    fsmblks: usize,
    uordblks: usize,
    fordblks: usize,
    keepcost: usize,
}

unsafe extern "C" {
    fn mallinfo2() -> MallInfo2;
    fn malloc_trim(pad: usize) -> i32;
}

/// The process's heap in use (malloc'ed bytes, mmapped blocks included), as Firefox's `heap-allocated`.
fn process_heap() -> i64 {
    let m = unsafe { mallinfo2() };
    (m.uordblks + m.hblkhd) as i64
}

/// A field of /proc/self/status in bytes: VmRSS (Firefox's `resident`), RssAnon (the anonymous part, not
/// backed by a file: closest to Firefox's `resident-unique`; mapped font files and the code are in RssFile).
fn process_status(field: &str) -> i64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with(field))?.split_whitespace().nth(1)?.parse::<i64>().ok())
        .map_or(0, |kb| kb * 1024)
}

fn process_rss() -> i64 {
    process_status("VmRSS:")
}

fn mb(b: i64) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

fn sizes_json(s: &JsSizes) -> String {
    format!(
        "{{\"gc_used\":{:.3},\"gc_unused\":{:.3},\"gc_admin\":{:.3},\"malloc\":{:.3},\"non_heap\":{:.3},\"total\":{:.3}}}",
        mb(s.gc_used as i64),
        mb(s.gc_unused as i64),
        mb(s.gc_admin as i64),
        mb(s.malloc_heap as i64),
        mb(s.non_heap as i64),
        mb(s.total() as i64)
    )
}

fn run(html: &str, name: &str, echo: bool, timer_limit: u32, cycles: usize) -> String {
    let t0 = Instant::now();
    let base = live();
    let dom = Dom::parse(html);
    let after_parse = live();

    let t1 = Instant::now();
    let mut page = Page::new(dom);
    page.echo_console(echo);
    let scripts = page.run_scripts(name);
    page.finish_loading(timer_limit);
    let script_ms = t1.elapsed().as_secs_f64() * 1000.0;
    page.gc();
    let js = page.memory();
    let after_script = live();
    // The DOM's own bytes: a clone of it (its arrays were trimmed at the end of loading, so a clone
    // has the same capacities). `after_script - base` also holds what layout flushes leave in global
    // caches (Stylo's, fonts), which the static comparison leaves out as shared.
    let dom_bytes = {
        let before = live();
        let copy = page.dom().clone();
        let bytes = live() - before;
        drop(copy);
        bytes
    };
    let elements = page.dom().count(Kind::Element);
    let texts = page.dom().count(Kind::Text);
    let nodes_in_arena = page.dom().nodes.len();

    let dom = page.take_dom();
    let mut sd = StyleDom::new(dom, (WIDTH as f32, HEIGHT as f32));
    sd.flush_stylist();
    let after_setup = live();
    sd.resolve();
    let after_style = live();
    let mut layout = ConstructFramesTrackingMode(&sd, WIDTH, HEIGHT, false, false);
    let after_frames = live();
    ReflowFrames(&mut layout);
    let after_layout = live();
    let frames: usize = FrameCountsByType().values().sum();
    let height = layout.DocumentHeightInCSSPixels();

    let stage4 = if cycles > 0 {
        let (mut sd, mut layout) = (sd, layout);
        let (mut drop_ms, mut wake_ms, mut after_drop, mut after_wake) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut js_hibernated = page.memory();
        let mut same = true;
        let (heap_live, rss_live, anon_live) = (process_heap(), process_rss(), process_status("RssAnon:"));
        let (mut heap_hibernated, mut rss_hibernated, mut anon_hibernated) = (0, 0, 0);
        for _ in 0..cycles {
            // Hibernate: as the Gecko prototype's Hide(), the frame tree goes, then the style data but
            // for the parsed sheets (SPEC decision 22; MRT_KEEP_STYLE=none drops them too, as before it).
            let t = Instant::now();
            DestroyFrameTree(layout);
            let sheets = if std::env::var("MRT_KEEP_STYLE").as_deref() == Ok("none") { Default::default() } else { sd.take_sheet_cache() };
            let dom = sd.into_dom();
            drop_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            after_drop.push(mb(live() - base));
            page.put_dom(dom);
            page.gc();
            js_hibernated = page.memory();
            // a hibernating page gives its freed pages back (Gecko's jemalloc purges them by itself)
            unsafe { malloc_trim(0) };
            heap_hibernated = process_heap();
            rss_hibernated = process_rss();
            anon_hibernated = process_status("RssAnon:");

            // Wake: everything load did after the scripts, warm (fonts and the user-agent stylist are cached).
            let t = Instant::now();
            sd = StyleDom::with_sheet_cache(page.take_dom(), (WIDTH as f32, HEIGHT as f32), Default::default(), sheets);
            sd.resolve();
            layout = ConstructFramesTrackingMode(&sd, WIDTH, HEIGHT, false, false);
            ReflowFrames(&mut layout);
            wake_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            after_wake.push(mb(live() - base));
            let frames_again: usize = FrameCountsByType().values().sum();
            same &= frames_again == frames && layout.DocumentHeightInCSSPixels() == height;
        }
        let list = |v: &[f64], digits: usize| v.iter().map(|x| format!("{x:.digits$}")).collect::<Vec<_>>().join(",");
        // the shaped-word caches of the fonts (process-wide, as Gecko's font-shaped-words)
        let (cached_words, word_cache_bytes) = mrt_js::mrt_layout::WordCacheSize();

        // Save and restore the DOM.
        let dom = &sd.tree.dom;
        let t = Instant::now();
        let bytes = dom.to_bytes();
        let save_ms = t.elapsed().as_secs_f64() * 1000.0;
        // the saved DOM, for measuring how well it compresses (bench/heap_compress.py compress_file)
        if let Ok(path) = std::env::var("MRT_DUMP_DOM") {
            std::fs::write(&path, &bytes).unwrap();
        }
        let t = Instant::now();
        let back = mrt_js::mrt_style::mrt_dom::Dom::from_bytes(&bytes);
        let restore_ms = t.elapsed().as_secs_f64() * 1000.0;
        let restored = back.as_ref().is_some_and(|b| b.dump() == dom.dump());
        format!(
            ",\"hibernate\":{{\"cycles\":{cycles},\"drop_ms\":[{}],\"rust_heap_after_drop_mb\":[{}],\"js_mb\":{:.3},\"wrapper_cache_mb\":{:.3},\"wake_ms\":[{}],\"rust_heap_after_wake_mb\":[{}],\"live_before_drop_mb\":{:.3},\"same_as_first\":{same},\"saved_mb\":{:.3},\"save_ms\":{save_ms:.1},\"restore_ms\":{restore_ms:.1},\"restored_equal\":{restored},\"word_cache_words\":{cached_words},\"word_cache_mb\":{:.3},\"process\":{{\"heap_live_mb\":{:.3},\"heap_hibernated_mb\":{:.3},\"rss_live_mb\":{:.3},\"rss_hibernated_mb\":{:.3},\"anon_live_mb\":{:.3},\"anon_hibernated_mb\":{:.3}}}}}",
            list(&drop_ms, 1),
            list(&after_drop, 3),
            mb((js_hibernated.realm.total() + js_hibernated.zone.total()) as i64),
            mb(js_hibernated.wrapper_cache as i64),
            list(&wake_ms, 1),
            list(&after_wake, 3),
            mb(after_layout - base),
            mb(bytes.len() as i64),
            mb(word_cache_bytes as i64),
            mb(heap_live),
            mb(heap_hibernated),
            mb(rss_live),
            mb(rss_hibernated),
            mb(anon_live),
            mb(anon_hibernated),
        )
    } else {
        String::new()
    };

    format!(
        "{{\"file\":\"{name}\",\"outcome\":\"ok\",\"scripts\":{},\"scripts_skipped\":{},\"js_errors\":{},\"console_lines\":{},\"layout_flushes\":{},\"elements\":{elements},\"text_nodes\":{texts},\"nodes_in_arena\":{nodes_in_arena},\"frames\":{frames},\"document_height_px\":{},\"dom_parse_mb\":{:.3},\"dom_mb\":{:.3},\"rust_heap_after_scripts_mb\":{:.3},\"wrapper_cache_mb\":{:.3},\"reflectors\":{},\"preserved\":{},\"js_realm\":{},\"js_zone\":{},\"js_mb\":{:.3},\"js_zone_buffer_slack_mb\":{:.3},\"style_mb\":{:.3},\"resolve_mb\":{:.3},\"frames_mb\":{:.3},\"reflow_mb\":{:.3},\"script_ms\":{:.0},\"total_ms\":{:.0}{stage4}}}",
        scripts.run,
        scripts.skipped,
        page.errors(),
        page.console().len(),
        page.flushes(),
        height,
        mb(after_parse - base),
        mb(dom_bytes),
        mb(after_script - base),
        mb(js.wrapper_cache as i64),
        js.reflectors,
        js.preserved,
        sizes_json(&js.realm),
        sizes_json(&js.zone),
        mb((js.realm.total() + js.zone.total()) as i64),
        mb(js.zone_buffer_slack as i64),
        mb(after_setup - after_script),
        mb(after_style - after_setup),
        mb(after_frames - after_style),
        mb(after_layout - after_frames),
        script_ms,
        t0.elapsed().as_secs_f64() * 1000.0,
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let file = args.get(1).expect("usage: jscount <file.html> [--console] [--timer-limit MS] [--hibernate | --cycles N]").clone();
    let echo = args.iter().any(|a| a == "--console");
    // --hibernate: one cycle; --cycles N: N cycles (to see that memory does not grow with them)
    let cycles = args.iter().position(|a| a == "--cycles").and_then(|i| args.get(i + 1)?.parse().ok()).unwrap_or(args.iter().any(|a| a == "--hibernate") as usize);
    let timer_limit = args.iter().position(|a| a == "--timer-limit").and_then(|i| args.get(i + 1)?.parse().ok()).unwrap_or(10_000);
    let html = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned();
    let name = std::path::Path::new(&file).file_stem().unwrap().to_string_lossy().into_owned();
    // a thread of its own with a big stack, as layoutcount: the page nests deeply, and the frame tree
    // and the JS context belong to the thread
    let json = std::thread::Builder::new().stack_size(2 << 30).spawn(move || run(&html, &name, echo, timer_limit, cycles)).unwrap().join().unwrap();
    mrt_js::shutdown();
    println!("{json}");
}
