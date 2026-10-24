//! layoutcount: parse a page, style it (Firefox's user-agent sheets), build the frame tree, reflow
//! it at 1489 x 844 CSS px (inline-size 1489, unconstrained block-size) and report.
//! usage: layoutcount <file.html> [--tree[=DEPTH]] [--ids] [--boxes]
//!   --tree: also print the frame tree (type, content, box in CSS px) down to DEPTH levels
//!   --boxes: also print `E <index> <parent> <tag> [<x> <y> <width> <height>]` for every element in
//!     document order (parent -1 for the root; the rect, in document CSS px, only when the element has a
//!     frame), for bench/compare_boxes.py
//!   --ids: also print `ID <id> <x> <y> <width> <height>` for each element with an id attribute
//!
//! Prints one JSON line: the frame counts by type, the document height in CSS px and the live
//! heap bytes after each phase. A page that reaches code that is not translated (an
//! `unimplemented!("NOT PORTED: ...")` or any panic) is reported with the message, where it
//! happened and the first frames of the stack; the exit status is 0 either way (see "outcome").

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use mrt_layout::driver::{BoxInDocument, ConstructFramesTrackingMode, ElementBoxes, DocumentElement, DumpFrameTree, FrameCountsByType, IdOf, ReflowFrames};
use mrt_style::mrt_dom::{Dom, Kind};
use mrt_style::StyleDom;

/// Counts live heap bytes, as `stylecount` does.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
/// Highest live byte count since the last `take_peak` (the transient part of a phase shows as peak minus end).
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = LIVE.fetch_add(l.size(), Relaxed) + l.size();
            PEAK.fetch_max(now, Relaxed);
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
                let now = LIVE.fetch_add(new - l.size(), Relaxed) + (new - l.size());
                PEAK.fetch_max(now, Relaxed);
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

/// The peak since the last call, then restarts the peak from the current live bytes.
fn take_peak() -> i64 {
    PEAK.swap(LIVE.load(Relaxed), Relaxed) as i64
}

fn mb(b: i64) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

const WIDTH: i32 = 1489;
const HEIGHT: i32 = 844;

/// The phase the page reached, so that a panic can say where it was.
static PHASE: AtomicUsize = AtomicUsize::new(0);
const PHASES: [&str; 6] = ["parse", "style setup", "style resolve", "frame construction", "reflow", "done"];

struct Report {
    json: String,
    tree: Option<String>,
}

fn run(html: &str, name: &str, tree_depth: Option<usize>, ids: bool, boxes: bool) -> Report {
    let t0 = Instant::now();
    let base = live();
    take_peak();
    PHASE.store(0, Relaxed);
    let dom = Dom::parse(html);
    let after_parse = live();
    let peak_parse = take_peak();
    let elements = dom.count(Kind::Element);
    PHASE.store(1, Relaxed);
    let mut sd = StyleDom::new(dom, (WIDTH as f32, HEIGHT as f32));
    sd.flush_stylist();
    let after_setup = live();
    let peak_setup = take_peak();
    PHASE.store(2, Relaxed);
    sd.resolve();
    let after_style = live();
    let peak_style = take_peak();
    PHASE.store(3, Relaxed);
    let t1 = Instant::now();
    let mut layout = ConstructFramesTrackingMode(&sd, WIDTH, HEIGHT, ids, boxes);
    let after_frames = live();
    let peak_frames = take_peak();
    let construct_ms = t1.elapsed().as_secs_f64() * 1000.0;
    PHASE.store(4, Relaxed);
    let t2 = Instant::now();
    ReflowFrames(&mut layout);
    PHASE.store(5, Relaxed);
    let after_layout = live();
    let peak_layout = take_peak();
    let reflow_ms = t2.elapsed().as_secs_f64() * 1000.0;
    let counts = FrameCountsByType();
    let frames: usize = counts.values().sum();
    let counts_json: Vec<String> = counts.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
    let mut tree = tree_depth.map(|d| DumpFrameTree(layout.mViewport, d));
    if ids {
        // one line per element with an id: its box in the document, for comparing with Firefox
        let mut out = tree.take().unwrap_or_default();
        for &(node, frame) in &layout.mFramesOfElementsWithId {
            let (x, y, w, h) = BoxInDocument(frame);
            out += &format!("ID {} {} {} {} {}\n", IdOf(&sd, node), x, y, w, h);
        }
        tree = Some(out);
    }
    // the frames' bytes as stored, for measuring how well they compress (DESIGN, "Idea: keep the
    // layout result")
    if let Ok(path) = std::env::var("MRT_DUMP_FRAME_BYTES") {
        let bytes = mrt_layout::WithFrameTree(|t| t.mFrames.raw_bytes());
        std::fs::write(&path, &bytes).unwrap();
        eprintln!("frame bytes: {} frames of {} B written to {path}", bytes.len() / std::mem::size_of::<mrt_layout::FrameData>(), std::mem::size_of::<mrt_layout::FrameData>());
    }
    // the frames field by field (padding zeroed), then with ids for pointers, then with relative links
    if let Ok(prefix) = std::env::var("MRT_DUMP_FRAME_FIELDS") {
        let layout: Vec<String> = mrt_layout::FrameStore::field_layout().iter().map(|(n, o, s)| format!("{n} {o} {s}")).collect();
        std::fs::write(format!("{prefix}.layout"), layout.join("\n")).unwrap();
        for (suffix, ids, rel) in [("plain", false, false), ("ids", true, false), ("ids-rel", true, true)] {
            let bytes = mrt_layout::WithFrameTree(|t| t.mFrames.field_bytes(ids, rel));
            std::fs::write(format!("{prefix}.{suffix}"), &bytes).unwrap();
        }
    }
    // what the reflow decides besides the rectangles: the line boxes of every block (owner frame,
    // first child, child count, bounds, ascent, block line, wrapped) and each text frame's slice of
    // its text (offset, length, ascent, continuing); 32-bit numbers, a row each, in frame order
    if let Ok(prefix) = std::env::var("MRT_DUMP_LINE_INFO") {
        use mrt_layout::*;
        let count = mrt_layout::WithFrameTree(|t| t.mFrames.len());
        let (mut lines, mut texts) = (Vec::<u8>::new(), Vec::<u8>::new());
        let mut put = |out: &mut Vec<u8>, vals: &[i32]| for v in vals { out.extend_from_slice(&v.to_le_bytes()); };
        for i in 0..count {
            let f = nsIFrame(i as u32);
            if f.IsBlockFrameOrSubclass() {
                for line in nsBlockFrame::from_frame(f).Lines() {
                    let b = line.GetPhysicalBounds();
                    let first = line.mFirstChild.map_or(-1, |c| c.0 as i32);
                    put(&mut lines, &[i as i32, first, line.GetChildCount(), b.x, b.y, b.width, b.height,
                                      line.GetLogicalAscent(), line.IsBlock() as i32, line.IsLineWrapped() as i32]);
                }
            }
            if f.IsTextFrame() {
                let t = nsTextFrame::from_frame(f);
                put(&mut texts, &[i as i32, t.GetContentOffset(), t.GetContentLength(), t.mAscent(), t.GetPrevContinuation().is_some() as i32]);
            }
        }
        std::fs::write(format!("{prefix}.lines"), &lines).unwrap();
        std::fs::write(format!("{prefix}.text"), &texts).unwrap();
    }
    // every frame's rectangle (relative to its parent, as Gecko keeps it) and its parent's index, in
    // the frame store's order: x, y, width, height, parent as five little-endian 32-bit numbers
    if let Ok(path) = std::env::var("MRT_DUMP_FRAME_RECTS") {
        let bytes = mrt_layout::WithFrameTree(|t| {
            let mut out = Vec::new();
            for f in t.mFrames.iter() {
                let r = f.mRect;
                for v in [r.x, r.y, r.width, r.height, f.mParent.map_or(-1, |p| p.0 as i32)] {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            out
        });
        std::fs::write(&path, &bytes).unwrap();
    }
    if boxes {
        let mut out = tree.take().unwrap_or_default();
        for e in ElementBoxes(&sd, &layout) {
            let parent = e.mParentIndex.map_or(-1, |p| p as i64);
            match e.mBox {
                Some((x, y, w, h)) => out += &format!("E {} {} {} {} {} {} {}\n", e.mIndex, parent, e.mTag, x, y, w, h),
                None => out += &format!("E {} {} {}\n", e.mIndex, parent, e.mTag),
            }
        }
        tree = Some(out);
    }
    let json = format!(
        "{{\"file\":\"{name}\",\"outcome\":\"ok\",\"elements\":{elements},\"frames\":{frames},\"frame_counts\":{{{}}},\"document_height_px\":{},\"root_element_height_px\":{},\"dom_mb\":{:.3},\"style_mb\":{:.3},\"resolve_mb\":{:.3},\"frames_mb\":{:.3},\"reflow_mb\":{:.3},\"total_mb\":{:.3},\"peak_dom_mb\":{:.3},\"peak_style_mb\":{:.3},\"peak_resolve_mb\":{:.3},\"peak_frames_mb\":{:.3},\"peak_reflow_mb\":{:.3},\"construct_ms\":{:.0},\"reflow_ms\":{:.0},\"total_ms\":{:.0}}}",
        counts_json.join(","),
        layout.DocumentHeightInCSSPixels(),
        layout.RootElementHeightInCSSPixels().map_or("null".to_string(), |h| h.to_string()),
        mb(after_parse - base),
        mb(after_setup - after_parse),
        mb(after_style - after_setup),
        mb(after_frames - after_style),
        mb(after_layout - after_frames),
        mb(after_layout - base),
        mb(peak_parse - base),
        mb(peak_setup - base),
        mb(peak_style - base),
        mb(peak_frames - base),
        mb(peak_layout - base),
        construct_ms,
        reflow_ms,
        t0.elapsed().as_secs_f64() * 1000.0,
    );
    let _ = DocumentElement(&sd);
    Report { json, tree }
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let file = args.get(1).expect("usage: layoutcount <file.html> [--tree[=DEPTH]]");
    let tree_depth = args.iter().find_map(|a| {
        if a == "--tree" {
            Some(usize::MAX)
        } else {
            a.strip_prefix("--tree=").and_then(|d| d.parse().ok())
        }
    });
    let html = String::from_utf8_lossy(&std::fs::read(file).unwrap()).into_owned();
    let name = std::path::Path::new(file).file_stem().unwrap().to_string_lossy().into_owned();

    // The panic hook keeps what a report needs: the message, the place, and the first frames of
    // the stack that are in mrt-layout (the functions that were running).
    static PANIC_INFO: std::sync::Mutex<Option<(String, String, Vec<String>)>> = std::sync::Mutex::new(None);
    std::panic::set_hook(Box::new(|info| {
        let msg = match info.payload().downcast_ref::<&str>() {
            Some(s) => s.to_string(),
            None => info.payload().downcast_ref::<String>().cloned().unwrap_or_default(),
        };
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let mut frames: Vec<String> = Vec::new();
        for line in bt.lines() {
            let line = line.trim();
            if let Some(rest) = line.split_once(": ").map(|x| x.1) {
                if rest.starts_with("mrt_layout::") && !rest.contains("panic") && !rest.starts_with("mrt_layout::Assertions") {
                    // drop the hash suffix of a Rust symbol
                    let clean = rest.rsplit_once("::h").map_or(rest, |x| x.0);
                    if frames.len() < 6 {
                        frames.push(clean.to_string());
                    }
                }
            }
        }
        *PANIC_INFO.lock().unwrap() = Some((msg, loc, frames));
    }));

    // A page nests deeply (the frame constructor and the reflow recurse), and its frame tree is
    // in thread-local storage: a thread of its own, with a big stack, per page.
    let name2 = name.clone();
    let ids = args.iter().any(|a| a == "--ids");
    let boxes = args.iter().any(|a| a == "--boxes");
    let handle = std::thread::Builder::new().stack_size(2 << 30).spawn(move || run(&html, &name2, tree_depth, ids, boxes)).unwrap();
    match handle.join() {
        Ok(report) => {
            println!("{}", report.json);
            if let Some(tree) = report.tree {
                println!("{tree}");
            }
        }
        Err(_) => {
            let info = PANIC_INFO.lock().unwrap().take().unwrap_or_default();
            let phase = PHASES[PHASE.load(Relaxed).min(PHASES.len() - 1)];
            let blocked_by_not_ported = info.0.contains("NOT PORTED");
            println!(
                "{{\"file\":\"{name}\",\"outcome\":\"{}\",\"phase\":\"{phase}\",\"message\":\"{}\",\"at\":\"{}\",\"stack\":[{}]}}",
                if blocked_by_not_ported { "not_ported" } else { "panic" },
                json_escape(&info.0),
                json_escape(&info.1),
                info.2.iter().map(|f| format!("\"{}\"", json_escape(f))).collect::<Vec<_>>().join(",")
            );
        }
    }
}
