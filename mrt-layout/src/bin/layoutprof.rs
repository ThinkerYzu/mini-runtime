//! layoutprof: where the live heap of a laid-out page is, by allocation site.
//! usage: layoutprof <file.html> [--depth N] [--top N]
//!
//! Runs the same pipeline as `layoutcount` (parse, style, frames, reflow at 1489 x 844) with an
//! allocator that samples about one allocation per 2 KB allocated, records the call stack of each
//! sample and removes it when the block is freed. At the end it prints the estimated live bytes by
//! the innermost `depth` interesting stack frames (frames of the allocator, `alloc`, `core`, `std`
//! and `hashbrown` are skipped), per phase of allocation, and the `size_of` of the key types.
//! Estimates, not exact counts: for the exact totals use `layoutcount`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;

use mrt_layout::driver::{ConstructFramesTrackingMode, ReflowFrames};
use mrt_style::mrt_dom::{Dom, Kind};
use mrt_style::StyleDom;

const SAMPLE_BYTES: usize = 2048;

struct Sample {
    site: u32,
    weight: usize,
    phase: u8,
}

struct State {
    live: HashMap<usize, Sample>,
    sites: HashMap<String, u32>,
    names: Vec<String>,
    acc: usize,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static PHASE: AtomicUsize = AtomicUsize::new(0);
static ON: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static BUSY: Cell<bool> = const { Cell::new(false) };
}

struct Sampling;

fn clean_stack(bt: &str, depth: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in bt.lines() {
        let line = line.trim();
        let Some((_, rest)) = line.split_once(": ") else { continue };
        if line.starts_with("at ") {
            continue;
        }
        let rest = rest.rsplit_once("::h").map_or(rest, |x| x.0);
        let skip = ["layoutprof::", "<layoutprof::", "alloc::", "<alloc::", "core::", "<core::", "std::", "<std::", "hashbrown::", "<hashbrown::", "__rust", "__GI", "malloc", "realloc", "calloc", "_start", "main", "<T as", "<I as"];
        if skip.iter().any(|p| rest.starts_with(p)) {
            continue;
        }
        let rest = rest.replace("mrt_layout::layout::generic::", "").replace("mrt_layout::", "").replace("mrt_style::", "ms::");
        out.push(rest);
        if out.len() >= depth {
            break;
        }
    }
    out.join(" < ")
}

static DEPTH: AtomicUsize = AtomicUsize::new(3);

fn record(p: *mut u8, size: usize) {
    if ON.load(Relaxed) == 0 || p.is_null() {
        return;
    }
    if BUSY.with(|b| b.replace(true)) {
        return;
    }
    {
        let mut g = STATE.lock().unwrap();
        let st = g.as_mut().unwrap();
        st.acc += size;
        if st.acc >= SAMPLE_BYTES {
            st.acc %= SAMPLE_BYTES;
            drop(g);
            let bt = std::backtrace::Backtrace::force_capture().to_string();
            let key = clean_stack(&bt, DEPTH.load(Relaxed));
            let mut g = STATE.lock().unwrap();
            let st = g.as_mut().unwrap();
            let n = st.names.len() as u32;
            let site = *st.sites.entry(key.clone()).or_insert(n);
            if site == n {
                st.names.push(key);
            }
            st.live.insert(p as usize, Sample { site, weight: size.max(SAMPLE_BYTES), phase: PHASE.load(Relaxed) as u8 });
        }
    }
    BUSY.with(|b| b.set(false));
}

fn forget(p: *mut u8) {
    if ON.load(Relaxed) == 0 {
        return;
    }
    if BUSY.with(|b| b.replace(true)) {
        return;
    }
    if let Some(st) = STATE.lock().unwrap().as_mut() {
        st.live.remove(&(p as usize));
    }
    BUSY.with(|b| b.set(false));
}

unsafe impl GlobalAlloc for Sampling {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        record(p, l.size());
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        forget(p);
        unsafe { System.dealloc(p, l) };
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        forget(p);
        let q = unsafe { System.realloc(p, l, new) };
        record(q, new);
        q
    }
}

#[global_allocator]
static A: Sampling = Sampling;

fn run(html: String) {
    *STATE.lock().unwrap() = Some(State { live: HashMap::new(), sites: HashMap::new(), names: Vec::new(), acc: 0 });
    ON.store(1, Relaxed);
    PHASE.store(0, Relaxed);
    let dom = Dom::parse(&html);
    let _ = dom.count(Kind::Element);
    PHASE.store(1, Relaxed);
    let mut sd = StyleDom::new(dom, (1489.0, 844.0));
    sd.flush_stylist();
    PHASE.store(2, Relaxed);
    sd.resolve();
    PHASE.store(3, Relaxed);
    let mut layout = ConstructFramesTrackingMode(&sd, 1489, 844, false, false);
    PHASE.store(4, Relaxed);
    ReflowFrames(&mut layout);
    PHASE.store(5, Relaxed);
    ON.store(0, Relaxed);

    // Summarize without allocating through the sampler.
    let st = STATE.lock().unwrap().take().unwrap();
    let mut by_site: HashMap<u32, [usize; 5]> = HashMap::new();
    for s in st.live.values() {
        by_site.entry(s.site).or_insert([0; 5])[(s.phase as usize).min(4)] += s.weight;
    }
    let mut rows: Vec<(usize, u32)> = by_site.iter().map(|(k, v)| (v.iter().sum(), *k)).collect();
    rows.sort_by(|a, b| b.cmp(a));
    let total: usize = rows.iter().map(|r| r.0).sum();
    let mb = |b: usize| b as f64 / 1048576.0;
    println!("estimated live MB by allocation site (total {:.2} MB, phases: parse/setup/resolve/frames/reflow)", mb(total));
    let top = std::env::args().skip_while(|a| a != "--top").nth(1).and_then(|v| v.parse().ok()).unwrap_or(40);
    for (sum, site) in rows.iter().take(top) {
        let ph = by_site[site];
        println!("{:8.3}  [{:.2} {:.2} {:.2} {:.2} {:.2}]  {}", mb(*sum), mb(ph[0]), mb(ph[1]), mb(ph[2]), mb(ph[3]), mb(ph[4]), st.names[*site as usize]);
    }
    println!("--- sizes");
    println!("size_of FrameData {}", std::mem::size_of::<mrt_layout::FrameData>());
    println!("size_of nsIContent {}", std::mem::size_of::<mrt_layout::nsIContent>());
    println!("size_of nsLineBox {}", std::mem::size_of::<mrt_layout::nsLineBox>());
    println!("size_of nsTextFrame_Data {}", std::mem::size_of::<mrt_layout::nsTextFrame_Data>());
    println!("size_of nsFrameList {}", std::mem::size_of::<mrt_layout::nsFrameList>());
    println!("size_of PropertiesMap {}", std::mem::size_of::<std::collections::HashMap<u32, Box<dyn std::any::Any>>>());
    println!("size_of FrameProperties {}", std::mem::size_of::<mrt_layout::FrameProperties>());
    println!("size_of ComputedValues {}", std::mem::size_of::<mrt_style::style::properties::ComputedValues>());
    let (frames, text) = mrt_layout::WithFrameTree(|t| (t.mFrames.len(), t.mFrames.iter().filter(|f| mrt_layout::nsIContent::FromId(f.mContent).is_some_and(|c| c.IsText())).count()));
    let with_content = mrt_layout::WithFrameTree(|t| t.mFrames.iter().filter(|f| f.mContent != 0).count());
    let frames_cap = mrt_layout::WithFrameTree(|t| t.mFrames.capacity());
    println!("frames {frames} (capacity {frames_cap}), frames with content {with_content}, of them text {text}");
    if std::env::args().any(|a| a == "--textruns") {
        text_run_report();
    }
    std::mem::forget(layout);
    std::mem::forget(sd);
}

/// glibc malloc chunk size of a request of `n` bytes (8 byte header, 16 byte granularity, 32 byte minimum).
fn chunk(n: usize) -> usize {
    if n == 0 { 0 } else { ((n + 8 + 15) & !15).max(32) }
}

/// `layoutprof --textruns`: one JSON line with the exact size of every part of every text run (see
/// bench/textrun_profile.py). Sizes are malloc chunk sizes (what the heap really holds), not payloads.
fn text_run_report() {
    use std::collections::{HashMap, HashSet};
    let r = mrt_layout::TextRunReportOfAllFrames();
    let n = r.runs.len();
    if let Ok(path) = std::env::var("MRT_DUMP_RUNS") {
        // one line per run: its flows as "len:first 24 chars", sorted so two dumps can be diffed
        let mut lines: Vec<String> = r.runs.iter().map(|run| run.flow_texts.iter().map(|t| format!("{}:{}", t.chars().count(), t.chars().take(24).collect::<String>().replace('\n', " "))).collect::<Vec<_>>().join(" | ")).collect();
        lines.sort();
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }
    let hist_edges = [1usize, 2, 4, 8, 16, 32, 64, 128, 256, 1024, usize::MAX];
    let mut hist_runs = vec![0usize; hist_edges.len()];
    let mut hist_chars = vec![0usize; hist_edges.len()];
    let (mut chars, mut glyph_len, mut glyph_cap, mut simple, mut det_chars) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let (mut b_struct, mut b_glyphs, mut b_det, mut b_gruns, mut b_skip, mut b_flows) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let (mut p_glyphs, mut p_det, mut p_gruns, mut p_skip, mut p_flows) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let (mut runs_with_det, mut runs_gruns_gt1, mut skip_ranges, mut skip_runs) = (0usize, 0usize, 0usize, 0usize);
    let (mut frames_sum, mut runs_multi_frame, mut max_frames, mut strong_mismatch, mut flows_sum, mut multi_flow) = (0u64, 0usize, 0u32, 0usize, 0usize, 0usize);
    let mut sizes = [0usize; 11];
    let (mut g_counted, mut g_skip, mut g_userdata) = (0usize, 0usize, 0usize);
    let (mut sz_struct, mut sz_gr, mut sz_skip) = (0, 0, 0);
    let mut starts: HashMap<u32, usize> = HashMap::new();
    let mut fonts: HashSet<usize> = HashSet::new();
    for run in &r.runs {
        let (m, grl, grc, szgr, skip, szrun, font, len) = &run.mem;
        sizes = *m;
        (sz_struct, sz_gr, sz_skip) = (*szrun, *szgr, skip.2);
        chars += *len as usize;
        let b = hist_edges.iter().position(|&e| (*len as usize) < e).unwrap();
        hist_runs[b] += 1;
        hist_chars[b] += *len as usize;
        glyph_len += m[0];
        glyph_cap += m[1];
        simple += m[7];
        det_chars += m[5];
        fonts.insert(*font);
        b_struct += chunk(16 + 8 + szrun);
        b_glyphs += chunk(m[1] * m[8]);
        p_glyphs += m[0] * m[8];
        if m[2] == 1 {
            runs_with_det += 1;
            b_det += chunk(56) + chunk(m[4] * m[9]) + chunk(m[6] * m[10]);
            p_det += m[3] * m[9] + m[5] * m[10];
        }
        b_gruns += chunk(grc * szgr);
        p_gruns += grl * szgr;
        if *grl > 1 { runs_gruns_gt1 += 1; }
        b_skip += chunk(skip.1 * skip.2);
        p_skip += skip.0 * skip.2;
        skip_ranges += skip.0;
        if skip.0 > 0 { skip_runs += 1; }
        b_flows += chunk(run.flows_cap * r.size_of_mapped_flow);
        p_flows += run.flows * r.size_of_mapped_flow;
        frames_sum += run.frames as u64;
        if run.frames > 1 { runs_multi_frame += 1; }
        max_frames = max_frames.max(run.frames);
        if run.strong != (run.frames + run.frames_uninflated) as usize { strong_mismatch += 1; }
        flows_sum += run.flows;
        if run.flows > 1 { multi_flow += 1; }
        // Gecko model (sizes read from the headers in firefox-hibernate; mozjemalloc rounds to 16): a text run is
        // one block of sizeof(gfxTextRun) = 112 + 4 bytes per char (glyphs inline, the single glyph run inline).
        // Counted by the "text-runs" reporter: the block, plus the detailed glyph store when there is one.
        // Not counted: skip chars (nsTArray: 8 + 12 per range), the user data (none for a simple flow, the frame
        // pointer is stored in the run; otherwise 8 + 16 per mapped flow).
        let r16 = |x: usize| (x + 15) & !15;
        g_counted += r16(112 + 4 * *len as usize);
        if m[2] == 1 {
            g_counted += 32 + r16(8 + m[3] * 16) + r16(8 + m[5] * 8);
        }
        if skip.0 > 0 { g_skip += r16(8 + 12 * skip.0); }
        if run.flows > 1 { g_userdata += r16(8 + 16 * run.flows); }
        for s in &run.flow_starts { *starts.entry(*s).or_insert(0) += 1; }
    }
    let dup_start_frames = starts.values().filter(|&&c| c > 1).count();
    // The user data side table: one hashbrown allocation (buckets x entry + control bytes).
    let buckets = if r.userdata_capacity == 0 { 0 } else if r.userdata_capacity < 4 { 4 } else if r.userdata_capacity < 8 { 8 } else { (r.userdata_capacity * 8 / 7).next_power_of_two() };
    let b_userdata_table = if buckets == 0 { 0 } else { buckets * r.size_of_userdata_entry + buckets + 16 }; // (no table now: the user data is in the runs)
    let _ = sizes;
    // Words: how much glyph storage a per-font word cache could share.
    let mut seen: HashMap<(usize, &str), usize> = HashMap::new();
    let (mut words, mut word_chars, mut dup_words, mut dup_chars) = (0usize, 0usize, 0usize, 0usize);
    let mut wlen_hist = [0usize; 6]; // chars in words of len 1-3, 4-7, 8-15, 16-31, 32-63, 64+
    for run in &r.runs {
        for (f, w) in &run.words {
            let l = w.chars().count();
            words += 1;
            word_chars += l;
            wlen_hist[match l { 0..=3 => 0, 4..=7 => 1, 8..=15 => 2, 16..=31 => 3, 32..=63 => 4, _ => 5 }] += l;
            let c = seen.entry((*f, w.as_str())).or_insert(0);
            if *c > 0 { dup_words += 1; dup_chars += l; }
            *c += 1;
        }
    }
    let unique_chars: usize = seen.keys().map(|(_, w)| w.chars().count()).sum();
    let j = |v: &[usize]| format!("[{}]", v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","));
    println!(
        "{{\"runs\":{n},\"chars\":{chars},\"hist_edges\":{},\"hist_runs\":{},\"hist_chars\":{},\"text_frames\":{},\"continuing_frames\":{},\"frames_with_run\":{},\"frames_sum\":{frames_sum},\"runs_multi_frame\":{runs_multi_frame},\"max_frames\":{max_frames},\"strong_mismatch\":{strong_mismatch},\"flows\":{flows_sum},\"runs_multi_flow\":{multi_flow},\"dup_start_frames\":{dup_start_frames},\"fonts\":{},\"glyph_len\":{glyph_len},\"glyph_cap\":{glyph_cap},\"simple_glyphs\":{simple},\"detailed_chars\":{det_chars},\"runs_with_detailed\":{runs_with_det},\"runs_gruns_gt1\":{runs_gruns_gt1},\"skip_ranges\":{skip_ranges},\"runs_with_skip\":{skip_runs},\"bytes\":{{\"run struct (Rc+RefCell+gfxTextRun)\":{b_struct},\"glyph array\":{b_glyphs},\"detailed glyph store\":{b_det},\"glyph runs vec\":{b_gruns},\"skip chars\":{b_skip},\"mapped flows vec\":{b_flows},\"user data table\":{b_userdata_table}}},\"payload\":{{\"glyph array\":{p_glyphs},\"detailed glyph store\":{p_det},\"glyph runs vec\":{p_gruns},\"skip chars\":{p_skip},\"mapped flows vec\":{p_flows}}},\"size_of\":{{\"gfxTextRun\":{sz_struct},\"GlyphRun\":{sz_gr},\"SkippedRange\":{sz_skip},\"CompressedGlyph\":{},\"DetailedGlyph\":{},\"DGRec\":{},\"TextRunMappedFlow\":{},\"user data entry\":{},\"nsTextFrame_Data\":{}}},\"gecko_model\":{{\"counted by text-runs reporter\":{g_counted},\"skip chars (not counted)\":{g_skip},\"user data (not counted)\":{g_userdata}}},\"words\":{words},\"word_chars\":{word_chars},\"dup_words\":{dup_words},\"dup_chars\":{dup_chars},\"unique_words\":{},\"unique_chars\":{unique_chars},\"word_chars_by_len\":{}}}",
        j(&hist_edges.iter().map(|&e| e.min(1 << 30)).collect::<Vec<_>>()), j(&hist_runs), j(&hist_chars),
        r.text_frames, r.continuing_frames, r.frames_with_run, fonts.len(),
        sizes[8], sizes[9], sizes[10], r.size_of_mapped_flow, r.size_of_userdata_entry, r.size_of_text_frame_data,
        seen.len(), j(&wlen_hist),
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let file = args.get(1).expect("usage: layoutprof <file.html> [--depth N] [--top N]");
    if let Some(d) = args.iter().skip_while(|a| *a != "--depth").nth(1).and_then(|v| v.parse().ok()) {
        DEPTH.store(d, Relaxed);
    }
    let html = String::from_utf8_lossy(&std::fs::read(file).unwrap()).into_owned();
    std::thread::Builder::new().stack_size(2 << 30).spawn(move || run(html)).unwrap().join().unwrap();
}
