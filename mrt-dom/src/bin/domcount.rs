//! domcount: parse an HTML file into the arena DOM and report counts, time and bytes.
//! usage: domcount [--tree] [--tags] [--json] <file.html>   (no file: a small built-in document)

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use mrt_dom::{Dom, Kind};

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

const BUILTIN: &str = "<!DOCTYPE html><html><head><title>T</title></head><body class=a><p>Hello <b>wor<i>ld</b></i> &amp; &lt;done&gt;<table><tr><td>x</table><!-- c --><div id=x>y</div></body></html>";

fn main() {
    let mut tree = false;
    let mut tags = false;
    let mut json = false;
    let mut file = None;
    for a in std::env::args().skip(1) {
        if a == "--tree" { tree = true } else if a == "--tags" { tags = true } else if a == "--json" { json = true } else { file = Some(a) }
    }
    let html = match &file {
        Some(f) => String::from_utf8_lossy(&std::fs::read(f).unwrap()).into_owned(),
        None => BUILTIN.to_string(),
    };
    Dom::parse(BUILTIN); // one-time parser setup is not charged to the page
    let before = LIVE.load(Relaxed);
    let t0 = Instant::now();
    let dom = Dom::parse(&html);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let live = LIVE.load(Relaxed) - before;
    if tree || file.is_none() {
        print!("{}", dom.dump());
    }
    if tags {
        // Per-tag counts of reachable elements, to compare with Firefox's querySelectorAll('*').
        let mut hist = std::collections::BTreeMap::new();
        let mut stack = vec![mrt_dom::DOCUMENT];
        while let Some(id) = stack.pop() {
            if dom.node(id).kind == Kind::Element {
                *hist.entry(format!("{}:{}", dom.node(id).ns, dom.tag(id))).or_insert(0) += 1;
            }
            stack.extend(dom.children(id));
        }
        for (k, v) in hist { println!("{k} {v}"); }
    }
    let elements = dom.count(Kind::Element);
    let texts = dom.count(Kind::Text);
    let h = dom.heap_bytes();
    if json {
        // one line for bench/compare_realsites.py; "_all" counts include template contents (shadow trees)
        println!(
            "{{\"bytes\": {}, \"elements\": {}, \"elements_all\": {}, \"text_nodes_all\": {}, \"arena_nodes\": {}, \"live\": {}, \"ms\": {:.1}}}",
            html.len(), elements, dom.count_with_templates(Kind::Element), dom.count_with_templates(Kind::Text), dom.nodes.len() - 2, live, ms
        );
        return;
    }
    println!(
        "{}: {} bytes, {} elements, {} text nodes, {} nodes in arena, {:.0} ms",
        file.as_deref().unwrap_or("builtin"), html.len(), elements, texts, dom.nodes.len() - 2, ms
    );
    println!("live after parse: {} B ({:.1} B/element); arrays: {:?}", live, live as f64 / elements.max(1) as f64, h);
}
