//! stylecount: parse a page, set up the stylist with its CSS, compute every element's style, and
//! report live heap bytes after each phase.
//! usage: stylecount <file.html> [--hist] [--errors]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use mrt_style::mrt_dom::{Dom, Kind};
use mrt_style::StyleDom;
use mrt_style::style;

/// Counts live heap bytes, as `mrt` does.
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

fn mb(b: i64) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

fn run(html: &str, w: f32, h: f32, report: bool, name: &str, hist: bool, errors: bool) -> (f64, f64) {
    let t0 = Instant::now();
    let base = live();
    let dom = Dom::parse(html);
    let after_parse = live();
    let elements = dom.count(Kind::Element);
    let mut sd = StyleDom::new(dom, (w, h));
    let after_setup = live();
    if errors {
        for (name, n) in &sd.parse_errors {
            eprintln!("parse errors: {name}: {n}");
        }
    }
    sd.flush_stylist();
    let after_flush = live();
    let t1 = Instant::now();
    sd.resolve();
    let after_style = live();
    if hist {
        // Count elements by computed display and font-size, to compare with Firefox's getComputedStyle.
        let mut counts = std::collections::BTreeMap::new();
        for id in 0..sd.tree.dom.nodes.len() as u32 {
            if let Some(cv) = sd.computed(id) {
                let display = style_traits::ToCss::to_css_string(&cv.clone_display());
                *counts.entry(format!("float={} position={}", style_traits::ToCss::to_css_string(&cv.clone_float()), style_traits::ToCss::to_css_string(&cv.clone_position()))).or_insert(0usize) += 1;
                let size = (cv.get_font().font_size.computed_size().px() * 100.0).round() / 100.0;
                *counts.entry(format!("display={display} font-size={size}")).or_insert(0usize) += 1;
            }
        }
        for (k, v) in counts {
            println!("{k} {v}");
        }
    }
    if report {
        let (with, before, after) = sd.eager_pseudo_stats();
        eprintln!("eager pseudos: {with} elements, {before} ::before, {after} ::after");
        eprintln!("slot: {} B each x {} = {:.3} MB (ElementData {} B)", size_of::<mrt_style::Slot>(), sd.tree.slots.len(), mb((size_of::<mrt_style::Slot>() * sd.tree.slots.len()) as i64), size_of::<style::data::ElementData>());
        println!(
            "{{\"file\":\"{name}\",\"elements\":{elements},\"dom_mb\":{:.3},\"sheets_mb\":{:.3},\"flush_mb\":{:.3},\"style_mb\":{:.3},\"total_mb\":{:.3},\"resolve_ms\":{:.0},\"total_ms\":{:.0}}}",
            mb(after_parse - base),
            mb(after_setup - after_parse),
            mb(after_flush - after_setup),
            mb(after_style - after_flush),
            mb(after_style - base),
            t1.elapsed().as_secs_f64() * 1000.0,
            t0.elapsed().as_secs_f64() * 1000.0,
        );
    }
    (mb(after_flush - after_parse), mb(after_style - after_flush))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let file = args.get(1).expect("usage: stylecount <file.html> [--hist] [--errors]");
    let (w, h) = (1489.0, 844.0);
    let html = String::from_utf8_lossy(&std::fs::read(file).unwrap()).into_owned();
    let hist = args.iter().any(|a| a == "--hist");
    let errors = args.iter().any(|a| a == "--errors");
    // One warm-up document so one-time setup (Stylo globals, atom tables) is not charged to the page.
    // Its setup cost is a stylist with only the user-agent sheet and one rule.
    let ua_base = run("<!DOCTYPE html><html><head><style>p{color:red}</style></head><body><p>x</p></body></html>", w, h, false, "", false, false);
    println!("stylist with the user-agent sheet only: {:.3} MB (sheet, flushed tables)", ua_base.0);
    run(&html, w, h, true, file, hist, errors);
}
