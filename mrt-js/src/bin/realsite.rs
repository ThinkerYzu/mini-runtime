//! realsite: stage 6, phase 1. A live site in the mini runtime, its network, cookies and storage
//! forwarded to Firefox through the bridge (DESIGN "The bridge, as built in phase 1").
//! usage: realsite <document.html> <url> [--console] [--timer-limit MS] [--max-flushes N] [--render]
//! Run by bench/mini_realsites.py: stdout and stdin are the bridge (one JSON line each way), so
//! nothing else may be printed on stdout. The measurements go back as the last request ("report").

use std::collections::HashMap;
use std::time::{Duration, Instant};

use mrt_js::mrt_style::mrt_dom::{DocumentParser, Dom, Kind};
use mrt_js::{GlobalOptions, Page};
use mrt_js::procmem;

// the Rust heap counted apart from SpiderMonkey's and the C++ layer's malloc (GAPS A16)
#[global_allocator]
static ALLOC: procmem::CountingAlloc = procmem::CountingAlloc;

/// glibc's malloc statistics; everything in this process allocates through glibc malloc.
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
    // bionic's mallinfo is mallinfo2 (the same struct of size_t, malloc.h)
    #[cfg_attr(target_os = "android", link_name = "mallinfo")]
    fn mallinfo2() -> MallInfo2;
    #[cfg(not(target_os = "android"))]
    fn malloc_trim(pad: usize) -> i32;
    fn malloc_info(options: i32, stream: *mut std::ffi::c_void) -> i32;
    fn fopen(path: *const std::ffi::c_char, mode: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    fn fclose(stream: *mut std::ffi::c_void) -> i32;
    fn mallopt(param: i32, value: i32) -> i32;
}

/// Android's allocator (bionic's scudo) has no malloc_trim; M_PURGE_ALL gives its free pages back to
/// the kernel, which is what malloc_trim(0) is used for here.
#[cfg(target_os = "android")]
unsafe fn malloc_trim(_pad: usize) -> i32 {
    const M_PURGE_ALL: i32 = -104;
    unsafe { mallopt(M_PURGE_ALL, 0) }
}

/// MRT_MALLOC_INFO=FILE: where the malloc heap's free memory is, written to FILE.PID right after the
/// trimmed measurement: glibc's per-arena statistics (malloc_info's XML), then the thread count and a
/// walk of every arena (procmem::arena_walk), for telling which arena and which kind of free chunk
/// holds a run-to-run difference (GAPS A19), and what pins the pages that keep free bytes (GAPS B95).
fn dump_malloc_info() {
    let Some(path) = std::env::var_os("MRT_MALLOC_INFO") else { return };
    // one file a process: a page's cross-origin frames run in realsites of their own
    let path = format!("{}.{}", path.to_string_lossy(), std::process::id());
    let Ok(c) = std::ffi::CString::new(path.as_bytes()) else { return };
    unsafe {
        let f = fopen(c.as_ptr(), c"w".as_ptr());
        if f.is_null() {
            return;
        }
        malloc_info(0, f);
        fclose(f);
    }
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let threads = status.lines().find(|l| l.starts_with("Threads:")).unwrap_or("").to_string();
    let w = procmem::arena_walk();
    let fill = w.free_by_fill.map(|b| b.to_string()).join(" ");
    let sparse: Vec<String> = (0..w.sparse_live.len())
        .filter(|&i| w.sparse_live[i].1 > 0)
        .map(|i| format!("{}:{}/{}", 16usize << i, w.sparse_live[i].0, w.sparse_live[i].1))
        .collect();
    let line = format!(
        "\n<!-- {threads} -->\n<!-- arenas: free {} free_resident {} free_in_used_pages {} resident {} unparsed_heaps {} top_resident {} unbinned_resident {} main_resident {} -->\n<!-- free_in_used_pages by page fill (free under 1/4, to 1/2, to 3/4, more): {fill} -->\n<!-- live bytes/pages on pages 3/4 free or more, by block size from: {} -->\n",
        w.free, w.free_resident, w.free_in_used_pages, w.resident, w.unparsed_heaps, w.top_resident, w.unbinned_resident, w.main_resident, sparse.join(" ")
    );
    let _ = std::fs::OpenOptions::new().append(true).open(&path).map(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
}

/// Sets glibc's M_ARENA_MAX before any thread starts: 1 unless MRT_ARENA_MAX=N says otherwise
/// (MRT_ARENA_MAX=0 keeps glibc's default, an arena per contending thread).
///
/// With 1 every thread allocates in the main arena, whose top chunk malloc_trim gives back; glibc's
/// default gives the page's thread an arena of its own, whose top malloc_trim leaves resident, 1 to
/// 30 MB that differ from run to run (GAPS A19; the default since the owner's SPEC decision 24).
fn set_arena_max() {
    const M_ARENA_MAX: i32 = -8;
    let n = std::env::var("MRT_ARENA_MAX").ok().and_then(|v| v.parse::<i32>().ok()).unwrap_or(1);
    if n > 0 {
        unsafe { mallopt(M_ARENA_MAX, n) };
    }
}

fn mb(b: usize) -> f64 {
    b as f64 / 1048576.0
}

/// MRT_PAGEOUT=S, after the report: the phone's own compressed tier (task 6da8). Every private
/// anonymous mapping of this process is paged out with MADV_PAGEOUT, which on Android moves it into
/// zram, as Android's CachedAppOptimizer does to a cached app (`am compact`); `adb shell` cannot do
/// it to another process (process_madvise needs CAP_SYS_NICE), so the runtime does it to itself.
/// Then one line on stderr, `MRT_PAGEOUT {json}`: this process's smaps_rollup before and after (kB),
/// and the process stays S seconds, so that the harness can read zram's use (dumpsys meminfo; zram's
/// sysfs is closed to adb) while the pages are still in it.
fn pageout(hold_s: u64) {
    const MADV_PAGEOUT: i32 = 21;
    unsafe extern "C" {
        fn madvise(addr: *mut std::ffi::c_void, len: usize, advice: i32) -> i32;
    }
    let rollup = || -> String {
        let text = std::fs::read_to_string("/proc/self/smaps_rollup").unwrap_or_default();
        let fields: Vec<String> = text
            .lines()
            .filter_map(|l| {
                let (k, v) = l.split_once(':')?;
                let kb = v.trim().strip_suffix(" kB")?.trim().parse::<u64>().ok()?;
                Some(format!("\"{}\":{kb}", k.trim()))
            })
            .collect();
        format!("{{{}}}", fields.join(","))
    };
    let before = rollup();
    let (mut ranges, mut bytes, mut failed) = (0, 0usize, 0);
    for line in std::fs::read_to_string("/proc/self/maps").unwrap_or_default().lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 || parts[4] != "0" || !parts[1].starts_with('r') || !parts[1].ends_with('p') {
            continue;
        }
        let path = parts.get(5).copied().unwrap_or("");
        if !(path.is_empty() || path.starts_with("[anon") || path.starts_with("[heap]") || path.starts_with("[stack")) {
            continue;
        }
        let Some((lo, hi)) = parts[0].split_once('-') else { continue };
        let (Ok(lo), Ok(hi)) = (usize::from_str_radix(lo, 16), usize::from_str_radix(hi, 16)) else { continue };
        ranges += 1;
        bytes += hi - lo;
        if unsafe { madvise(lo as *mut std::ffi::c_void, hi - lo, MADV_PAGEOUT) } != 0 {
            failed += 1;
        }
    }
    eprintln!(
        "MRT_PAGEOUT {{\"ranges\":{ranges},\"mapped_bytes\":{bytes},\"failed\":{failed},\"before\":{before},\"after\":{}}}",
        rollup()
    );
    std::thread::sleep(std::time::Duration::from_secs(hold_s));
}

/// Unique memory (USS: Private_Clean + Private_Dirty) and PSS of this process, from smaps_rollup, as
/// bench/firefox_realsites.py reads Firefox's processes.
fn uss_pss() -> (usize, usize) {
    let mut uss = 0;
    let mut pss = 0;
    for line in std::fs::read_to_string("/proc/self/smaps_rollup").unwrap_or_default().lines() {
        let mut it = line.split_whitespace();
        let (Some(k), Some(v)) = (it.next(), it.next()) else { continue };
        let kb: usize = v.parse().unwrap_or(0);
        match k {
            "Private_Clean:" | "Private_Dirty:" => uss += kb * 1024,
            "Pss:" => pss = kb * 1024,
            _ => {}
        }
    }
    (uss, pss)
}

/// The Rust heap and the whole malloc heap in use, in MB, at one step of the load.
fn step(name: &str) -> String {
    let m = unsafe { mallinfo2() };
    format!("{{\"step\":\"{name}\",\"rust_heap_mb\":{:.3},\"heap_mb\":{:.3}}}", mb(procmem::rust_heap()), mb(m.uordblks + m.hblkhd))
}

/// One turn of a merged event loop (DESIGN, "Cross-origin frames in runtimes of their own"): the
/// runtime tells the driver when its next task is due (none: it waits for messages) and the limit, and
/// the driver answers when it is this runtime's turn: go (after queueing the messages it hands over,
/// tasks at their senders' times) or finish. bridge.js reads the reply (__mrt.takeStep).
fn take_turn(page: &mut Page, next: f64, limit: u32) -> bool {
    let next = if next < 0.0 { "null".to_string() } else { format!("{next}") };
    let reply = page.bridge_raw(&format!("{{\"op\":\"step\",\"next\":{next},\"limit\":{limit}}}"));
    page.eval_number(&format!("__mrt.takeStep({reply})"), "driver") == 1.0
}

/// The page's tasks (timers, and messages from frame runtimes) until `limit` ms on the virtual clock,
/// one per evaluation so that the promise jobs each queues run before the next. While the page has a
/// cross-origin frame runtime, or is one (`frame`), each task waits for its turn in the event loop
/// the driver merges; otherwise the runtime runs alone, as before. Returns how many ran.
fn run_tasks(page: &mut Page, limit: u32, frame: bool) -> usize {
    // MRT_HIBERNATE_AT=MS: the runtime hibernates (Page::hibernate) once, before its first task due
    // at MS or later on the virtual clock, and the page goes on; set against a run without it, the
    // page's final state shows whether it works after waking
    let mut hibernate_at: Option<f64> = std::env::var("MRT_HIBERNATE_AT").ok().and_then(|v| v.parse().ok());
    // MRT_WAKE_AT=MS: the hibernated page is shown again (Page::wake) before its first task due at
    // MS or later
    let mut wake_at: Option<f64> = std::env::var("MRT_WAKE_AT").ok().and_then(|v| v.parse().ok());
    // MRT_BRIDGE_DROP=MS: MS after hibernating, the page's bridge tab is dropped, as under memory
    // pressure (SPEC decision 14; DESIGN "Bridge recovery, as tested"): before the first task from then
    // on with no IndexedDB transaction of the page live nor an answer of Firefox awaited
    // (__mrt.indexedDBLive), or BRIDGE_DROP_WAIT_MS later, whatever is live then being lost with the
    // tab. The page's runtime asks; a frame runtime's bridge is the page's tab, dropped with it
    let drop: Option<f64> = if frame { None } else { std::env::var("MRT_BRIDGE_DROP").ok().and_then(|v| v.parse().ok()) };
    const BRIDGE_DROP_WAIT_MS: f64 = 5000.0;
    let mut drop_from: Option<f64> = None;
    let mut ran = 0;
    while ran < 50_000 {
        let next = page.eval_number("__mrt.nextTimerTime()", "driver");
        if let Some(at) = hibernate_at {
            if next >= at {
                page.hibernate();
                eprintln!("hibernated before the task at {next} ms");
                hibernate_at = None;
                if let Some(after) = drop {
                    drop_from = Some(next + after);
                }
            }
        }
        if let Some(from) = drop_from.filter(|from| next < 0.0 || next >= *from) {
            let live = page.eval_number("__mrt.indexedDBLive()", "driver");
            if live == 0.0 || next < 0.0 || next >= from + BRIDGE_DROP_WAIT_MS {
                page.eval("__mrt.dropBridge()", "driver");
                eprintln!("bridge dropped before the task at {next} ms ({live} IndexedDB transactions or answers live)");
                drop_from = None;
            }
        }
        if let Some(at) = wake_at {
            if hibernate_at.is_none() && next >= at {
                page.wake();
                eprintln!("woken before the task at {next} ms");
                wake_at = None;
            }
        }
        let stepped = frame || page.eval_number("__mrt.shared.crossFrames ? 1 : 0", "driver") == 1.0;
        if !stepped {
            if page.run_timers_stepwise(limit, 1) == 0 {
                break;
            }
        } else {
            let next = page.eval_number("__mrt.nextTimerTime()", "driver");
            if !take_turn(page, next, limit) {
                break;
            }
            page.run_timers_stepwise(u32::MAX, 1);
        }
        ran += 1;
    }
    ran
}

/// Runs the loaded page on the real clock until the driver says stop (realsite --interactive, SPEC
/// decision 25), for a page someone uses from the bridge tab. Returns how many tasks ran.
///
/// Each turn runs the timers due by now, one per evaluation as run_tasks does. When none is due the
/// runtime sends `wait` with its next timer's time (null for none) and the time now, and blocks on
/// the answer: the driver answers once that time comes, or sooner (it wakes the runtime at least
/// every 250 ms), with `stop` when the page is to be measured and the run ended. A page whose
/// cross-origin frames run in runtimes of their own is not driven here: their merged loop is on the
/// virtual clock (run_tasks), and they get no turn. The bridge tab's copy of the document is brought
/// up to date after a task at most every 100 ms, and before every wait. After 5 s without the owner's input the page gets a
/// shrinking GC and its free memory is given back (see `Upkeep`).
///
/// The answer to a wait carries the owner's input caught in the tab since (`events`), which become
/// tasks (bridge.js inputEvents), and what the tab's media elements did (`media`, bridge.js
/// mediaEvents). A page that keeps busy still asks for it at least every 50 ms: a
/// wait with `until` now, which the driver answers at once.
///
/// The answer also says whether the tab is hidden (task e83d): the page hibernates while it is and
/// wakes when it is shown (see `Hibernation`). Returns how many tasks ran, and the hibernations and
/// wakes as JSON objects.
fn run_interactive(page: &mut Page) -> (usize, Vec<String>, String) {
    let mut hib = Hibernation::default();
    let mut upkeep = Upkeep::from_env();
    let mut profile = LoopProfile::from_env(page);
    let (ran, stopped) = run_interactive_loop(page, &mut hib, &mut upkeep, &mut profile);
    eprintln!("interactive: stopped after {ran} tasks, at {stopped:.0} ms");
    (ran, hib.log, format!("{{\"runs\":[{}],\"profile\":{}}}", upkeep.log.join(","), profile.to_json()))
}

fn run_interactive_loop(page: &mut Page, hib: &mut Hibernation, upkeep: &mut Upkeep, profile: &mut LoopProfile) -> (usize, f64) {
    let mut ran = 0;
    let mut warned_frames = false;
    let mut asked = Instant::now();
    loop {
        let now = page.eval_number("__mrt.clockNow()", "driver");
        if asked.elapsed().as_millis() >= 50 {
            profile.flush(page, false);
            let reply = profile.wait(|| page.bridge_raw(&format!("{{\"op\":\"wait\",\"until\":{now},\"now\":{now},\"busy\":true}}")));
            asked = Instant::now();
            profile.trace_input(page, &reply, now);
            if profile.input(|| take_input(page, &reply, hib, upkeep, now)) {
                page.eval("__mrt.mirrorCheck(); __mrt.mirrorStop()", "driver");
                return (ran, now);
            }
        }
        if profile.task(page, now as u32) {
            ran += 1;
            profile.flush(page, false);
            continue;
        }
        if !warned_frames && page.eval_number("__mrt.shared.crossFrames ? 1 : 0", "driver") == 1.0 {
            eprintln!("interactive: the page has cross-origin frame runtimes, which get no turn on the real clock");
            warned_frames = true;
        }
        profile.maybe_log(now);
        if upkeep.due() {
            let t = Instant::now();
            upkeep.run(page, now);
            profile.upkeep_ms += t.elapsed().as_secs_f64() * 1000.0;
        }
        profile.flush(page, true);
        let next = page.eval_number("__mrt.nextTimerTime()", "driver");
        let until = if next < 0.0 { "null".to_string() } else { format!("{next}") };
        let reply = profile.wait(|| page.bridge_raw(&format!("{{\"op\":\"wait\",\"until\":{until},\"now\":{now}}}")));
        asked = Instant::now();
        profile.trace_input(page, &reply, now);
        if profile.input(|| take_input(page, &reply, hib, upkeep, now)) {
            // the tab's copy checked, then its bookkeeping dropped before the page is measured
            page.eval("__mrt.mirrorCheck(); __mrt.mirrorStop()", "driver");
            return (ran, now);
        }
    }
}

/// The interactive page's upkeep while no one uses it (GAPS A22), as Firefox's content process
/// collects on its own once the user is inactive. Firefox's user goes inactive after 5 s without
/// input (dom.events.user_interaction_interval, the user-interaction-inactive notification), when
/// CCGCScheduler starts a shrinking GC (after compact_on_user_inactive_delay: 15 s on Nightly,
/// 300 s on release); its jemalloc gives freed pages back on its own all the time. glibc's malloc
/// does not, so here the page gets a shrinking GC and malloc_trim(0) (decision 14's pressure step,
/// without dropping anything) once it has had no input for `after`, then again every `every` while
/// it still has none, hibernated or not.
///
/// MRT_IDLE_GC_S (default 5; 0 turns the upkeep off) and MRT_IDLE_REPEAT_S (default 30) change the
/// two times. `log`: each run as a JSON object for the report.
struct Upkeep {
    after: Option<Duration>,
    every: Duration,
    last_input: Instant,
    last_run: Option<Instant>,
    log: Vec<String>,
}

impl Upkeep {
    fn from_env() -> Self {
        let secs = |name: &str, default: f64| std::env::var(name).ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(default);
        let after = secs("MRT_IDLE_GC_S", 5.0);
        Upkeep {
            after: (after > 0.0).then(|| Duration::from_secs_f64(after)),
            every: Duration::from_secs_f64(secs("MRT_IDLE_REPEAT_S", 30.0).max(1.0)),
            last_input: Instant::now(),
            last_run: None,
            log: Vec::new(),
        }
    }

    /// The owner's input came: the next run waits for `after` without input again.
    fn input(&mut self) {
        self.last_input = Instant::now();
        self.last_run = None;
    }

    fn due(&self) -> bool {
        match (self.after, self.last_run) {
            (None, _) => false,
            (Some(after), None) => self.last_input.elapsed() >= after,
            (Some(_), Some(last)) => last.elapsed() >= self.every,
        }
    }

    fn run(&mut self, page: &mut Page, now: f64) {
        let t = Instant::now();
        let uss_before = uss_pss().0;
        page.shrink_gc();
        let gc_ms = t.elapsed().as_secs_f64() * 1000.0;
        unsafe { malloc_trim(0) };
        let (ms, uss_after) = (t.elapsed().as_secs_f64() * 1000.0, uss_pss().0);
        eprintln!("interactive: idle upkeep at {now:.0} ms in {ms:.0} ms (GC {gc_ms:.0}): USS {:.1} -> {:.1} MB", mb(uss_before), mb(uss_after));
        self.log.push(format!("{{\"at\":{now:.0},\"ms\":{ms:.1},\"gc_ms\":{gc_ms:.1},\"uss_before_mb\":{:.3},\"uss_after_mb\":{:.3}}}", mb(uss_before), mb(uss_after)));
        self.last_run = Some(Instant::now());
    }
}

/// Where the interactive loop's time goes (GAPS A22): wall milliseconds in the page's tasks (its
/// timers and the runtime's own), in the bridge tab's copy (mirrorFlush), in taking the tab's input
/// (with the hibernations), in the waits for the driver (blocked until the next timer or input), in
/// the idle upkeep; and the process's CPU seconds over the loop. With MRT_TIMER_PROFILE=1, also by
/// timer (prelude.js lastTimerKey: its kind, delay and the start of its callback), the busiest
/// first.
struct LoopProfile {
    start: Instant,
    logged: Instant,
    cpu_start: f64,
    tasks: usize,
    tasks_ms: f64,
    mirror_flushes: usize,
    mirror_ms: f64,
    /// of those, the flushes that sent the tab something (a round trip to the driver), and their time
    mirror_sent: usize,
    mirror_sent_ms: f64,
    input_ms: f64,
    wait_ms: f64,
    upkeep_ms: f64,
    by_timer: Option<HashMap<String, (usize, f64)>>,
    /// MRT_LOOP_TRACE=1: on stderr, each task or flush over 30 ms, each wait answer with the owner's
    /// input, and each input task with how long after its input's arrival it ran (task 0f15b)
    trace: Option<Instant>,
}

impl LoopProfile {
    fn from_env(page: &mut Page) -> Self {
        let trace = std::env::var_os("MRT_LOOP_TRACE").is_some();
        if trace {
            page.eval("__mrt.profileTimers()", "driver");
        }
        let by_timer = std::env::var_os("MRT_TIMER_PROFILE").is_some().then(|| {
            page.eval("__mrt.profileTimers()", "driver");
            HashMap::new()
        });
        LoopProfile {
            start: Instant::now(),
            logged: Instant::now(),
            cpu_start: cpu_s(),
            tasks: 0,
            tasks_ms: 0.0,
            mirror_flushes: 0,
            mirror_ms: 0.0,
            mirror_sent: 0,
            mirror_sent_ms: 0.0,
            input_ms: 0.0,
            wait_ms: 0.0,
            upkeep_ms: 0.0,
            by_timer,
            trace: trace.then(Instant::now),
        }
    }

    /// One due task run (Page::run_timers_stepwise); false when none was due by `now`.
    fn task(&mut self, page: &mut Page, now: u32) -> bool {
        if self.trace.is_some() {
            mrt_js::take_task_times();
        }
        let t = Instant::now();
        if page.run_timers_stepwise(now, 1) != 1 {
            return false;
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        self.tasks += 1;
        self.tasks_ms += ms;
        if let Some(by) = self.by_timer.as_mut() {
            let e = by.entry(page.eval_string("__mrt.lastTimerKey()", "driver")).or_default();
            e.0 += 1;
            e.1 += ms;
        }
        if let Some(since) = self.trace {
            let key = page.eval_string("__mrt.lastTimerKey()", "driver");
            let late = page.eval_number("__mrt.lastTimerLate()", "driver");
            if ms > 30.0 || key.contains("inputGroups") {
                // what of the task was layout and bridge; the rest is the page's scripts and the DOM
                let tt = mrt_js::take_task_times();
                let split = format!(
                    "flushes {} {:.0} ms (attach {:.0} style {:.0} frames {:.0} reflow {:.0} boxes {:.0} state {:.0}), reads {} {:.0} ms, bridge {} {:.0} ms",
                    tt.flushes, tt.flush_ms, tt.attach_ms, tt.style_ms, tt.construct_ms, tt.reflow_ms, tt.boxes_ms, tt.state_ms, tt.reads, tt.read_ms, tt.bridge, tt.bridge_ms
                );
                eprintln!("trace {:.0}: task {ms:.0} ms, {late:.0} ms late, {split}: {key}", since.elapsed().as_secs_f64() * 1000.0);
            }
        }
        true
    }

    /// The bridge tab's copy brought up to date (bridge.js mirrorFlush: at most every 100 ms unless
    /// `force`).
    fn flush(&mut self, page: &mut Page, force: bool) {
        let t = Instant::now();
        let ops = page.eval_number(if force { "__mrt.mirrorFlush(true)" } else { "__mrt.mirrorFlush(false)" }, "driver");
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        if let Some(since) = self.trace.filter(|_| ms > 30.0) {
            eprintln!("trace {:.0}: flush {ms:.0} ms, {ops} ops", since.elapsed().as_secs_f64() * 1000.0);
        }
        self.mirror_flushes += 1;
        self.mirror_ms += ms;
        if ops > 0.0 {
            self.mirror_sent += 1;
            self.mirror_sent_ms += ms;
        }
    }

    fn wait<T>(&mut self, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let r = f();
        self.wait_ms += t.elapsed().as_secs_f64() * 1000.0;
        r
    }

    /// MRT_LOOP_TRACE: a wait's answer that carries events, with their types
    fn trace_input(&self, page: &mut Page, reply: &str, now: f64) {
        if let Some(since) = self.trace {
            let types = page.eval_string(&format!("((({reply}).ok || {{}}).events || []).map(e => e.type).join(' ')"), "driver");
            if !types.is_empty() {
                eprintln!("trace {:.0}: input at clock {now:.0}: {types}", since.elapsed().as_secs_f64() * 1000.0);
            }
        }
    }

    fn input<T>(&mut self, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let r = f();
        self.input_ms += t.elapsed().as_secs_f64() * 1000.0;
        r
    }

    /// Every 10 s, the totals so far on stderr (`interactive: profile at T ms {...}`, without the
    /// timers), so that a stretch of the run (the idle part) can be told apart.
    fn maybe_log(&mut self, now: f64) {
        if self.logged.elapsed().as_secs() >= 10 {
            self.logged = Instant::now();
            eprintln!("interactive: profile at {now:.0} ms {}", self.to_json_with(false));
        }
    }

    fn to_json(&self) -> String {
        self.to_json_with(true)
    }

    fn to_json_with(&self, with_timers: bool) -> String {
        let timers = self.by_timer.as_ref().filter(|_| with_timers).map_or(String::new(), |by| {
            let mut list: Vec<_> = by.iter().collect();
            list.sort_by(|a, b| b.1 .1.total_cmp(&a.1 .1));
            let list: Vec<String> = list.iter().take(30).map(|(k, (n, ms))| format!("{{\"timer\":{},\"runs\":{n},\"ms\":{ms:.1}}}", mrt_js::js_string(k))).collect();
            format!(",\"by_timer\":[{}]", list.join(","))
        });
        format!(
            "{{\"wall_s\":{:.1},\"cpu_s\":{:.2},\"tasks\":{},\"tasks_ms\":{:.0},\"mirror_flushes\":{},\"mirror_ms\":{:.0},\"mirror_sent\":{},\"mirror_sent_ms\":{:.0},\"input_ms\":{:.0},\"wait_ms\":{:.0},\"upkeep_ms\":{:.0}{timers}}}",
            self.start.elapsed().as_secs_f64(),
            cpu_s() - self.cpu_start,
            self.tasks,
            self.tasks_ms,
            self.mirror_flushes,
            self.mirror_ms,
            self.mirror_sent,
            self.mirror_sent_ms,
            self.input_ms,
            self.wait_ms,
            self.upkeep_ms
        )
    }
}

/// The process's user and system CPU time so far, seconds (/proc/self/stat).
fn cpu_s() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.rsplit_once(')').map_or("", |r| r.1).split_whitespace().collect();
    let ticks = |i: usize| fields.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    (ticks(11) + ticks(12)) / 100.0
}

/// The interactive page's hibernation as its bridge tab is hidden and shown (task e83d, the tab's
/// `visibilitychange` as the driver reports it in each wait's answer): `hidden` is the page's state,
/// `log` each hibernation and wake as a JSON object for the report.
#[derive(Default)]
struct Hibernation {
    hidden: bool,
    log: Vec<String>,
}

impl Hibernation {
    /// The page hibernated (Page::hibernate: style data, frames and layout threads dropped, the page
    /// a background tab) as Firefox would when its tab goes to the background; malloc's free memory
    /// then given back, as under memory pressure, so that the USS shows what went.
    fn hibernate(&mut self, page: &mut Page, now: f64) {
        let t = Instant::now();
        let (layouts, uss_before) = (page.layouts_live(), uss_pss().0);
        page.hibernate();
        unsafe { malloc_trim(0) };
        let (ms, uss_after) = (t.elapsed().as_secs_f64() * 1000.0, uss_pss().0);
        eprintln!("interactive: hibernated at {now:.0} ms in {ms:.0} ms: layout threads {layouts} -> {}, USS {:.1} -> {:.1} MB",
            page.layouts_live(), mb(uss_before), mb(uss_after));
        self.log.push(format!("{{\"hibernated_at\":{now:.0},\"ms\":{ms:.1},\"layouts_before\":{layouts},\"layouts_after\":{},\"uss_before_mb\":{:.3},\"uss_after_mb\":{:.3}}}",
            page.layouts_live(), mb(uss_before), mb(uss_after)));
        self.hidden = true;
    }

    /// The page shown again (Page::wake): visible, its throttled timeouts and animation frames due;
    /// its layout comes back from nothing at the first read, the first input's handlers or frame.
    fn wake(&mut self, page: &mut Page, now: f64, why: &str) {
        page.wake();
        eprintln!("interactive: woken at {now:.0} ms ({why})");
        self.log.push(format!("{{\"woken_at\":{now:.0},\"by\":\"{why}\"}}"));
        self.hidden = false;
    }
}

/// A wait's answer: the tab hidden or shown (the page hibernates or wakes), and its events given to
/// the page as tasks; true when the driver said stop. Input while hibernated (none should come from
/// a hidden tab) wakes the page first, and the owner's input puts off the idle upkeep.
fn take_input(page: &mut Page, reply: &str, hib: &mut Hibernation, upkeep: &mut Upkeep, now: f64) -> bool {
    let hidden = page.eval_number(&format!("(({reply}).ok || {{}}).hidden ? 1 : 0"), "driver") == 1.0;
    let events = page.eval_number(&format!("((({reply}).ok || {{}}).events || []).length"), "driver");
    // the owner's own input, as Gecko's EventStateManager counts user activity: real mouse moves and
    // buttons, wheels, keys and touches, not the enter, leave, over and out events the page's own
    // changes cause under a still pointer
    let active = "/^(mouse(move|down|up)|pointer(move|down|up)|wheel|key|touch|drop$)/";
    if events > 0.0 && page.eval_number(&format!("((({reply}).ok || {{}}).events || []).filter(e => {active}.test(e.type)).length"), "driver") > 0.0 {
        upkeep.input();
    }
    if hidden && !hib.hidden && events == 0.0 {
        hib.hibernate(page, now);
    } else if hib.hidden && (!hidden || events > 0.0) {
        hib.wake(page, now, if hidden { "input" } else { "shown" });
    }
    // HTTP attachment: wake first, synchronize geometry, then publish a full
    // snapshot. The driver gates input until the browser acknowledges it.
    let width = page.eval_number(&format!("(({reply}).ok || {{}}).viewport?.[0] || 0"), "driver") as i32;
    let height = page.eval_number(&format!("(({reply}).ok || {{}}).viewport?.[1] || 0"), "driver") as i32;
    if width > 0 && height > 0 {
        page.resize_viewport(width, height);
    }
    // Queue accepted input before snapshot/visibility bookkeeping so the old
    // node map survives until those tasks complete; snapshots defer to them.
    page.eval(&format!("__mrt.inputEvents((({reply}).ok || {{}}).events || [])"), "driver");
    page.eval(&format!("{{ const v = ({reply}).ok || {{}}; if (v.mediaBackend) __mrt.viewerMedia(v.mediaBackend); if (v.webgl) __mrt.webglViewer?.(v.webgl); if ('mirror' in v) __mrt.viewerMirror(v.mirror); if (v.snapshot) {{ void document.documentElement?.offsetWidth; __mrt.mirrorSnapshot(v.snapshot); }} }}"), "driver");
    // the tab's media events and answers (task 753cc): not input, so they wake nothing
    page.eval(&format!("__mrt.mediaEvents((({reply}).ok || {{}}).media || [])"), "driver");
    page.eval_number(&format!("(({reply}).ok || {{}}).stop ? 1 : 0"), "driver") == 1.0
}

fn run(html: String, url: String, global: GlobalOptions, echo: bool, timer_limit: u32, max_flushes: usize, render: bool, frame: Option<f64>, interactive: bool) {
    let t0 = Instant::now();
    let mut steps = vec![step("start")];
    // the document starts empty: it is parsed at its scripts' pace below (Firefox's parser stops at
    // each script, which runs where the parser is)
    let mut parser = DocumentParser::new(&html);
    drop(html);
    steps.push(step("parser_started"));
    let mut page = Page::with_global(Dom::new(), &global);
    page.echo_console(echo);
    page.enable_bridge();
    page.set_max_flushes(max_flushes);
    assert!(page.load_bridge(&url), "bridge.js threw");
    page.eval("__mrt.installMissingLog()", "driver");
    // interactive: the page loads and runs on the real clock, the wall clock's time since now
    if interactive {
        page.eval("__mrt.useRealClock()", "driver");
        // the bridge tab shows the document as it is built and changed (bridge.js mirrorFlush)
        if std::env::var_os("VIEWER_HTTP").is_none() {
            page.eval("__mrt.mirrorStart()", "driver");
        }
    }
    // a frame runtime's load is a task of the merged loop, at its parent's time when the frame opened
    if let Some(start) = frame {
        page.eval("__mrt.becomeFrameRuntime()", "driver");
        page.eval(&format!("__mrt.setClock({start})"), "driver");
        if !take_turn(&mut page, start, timer_limit) {
            return;
        }
    }

    // the document parsed with its scripts, then DOMContentLoaded and load, then the timers
    steps.push(step("bridge_loaded"));
    // MRT_EVAL_BEFORE=FILE: a script run before the page's own, for watching what they do
    if let Some(path) = std::env::var_os("MRT_EVAL_BEFORE") {
        let src = std::fs::read_to_string(&path).expect("MRT_EVAL_BEFORE: cannot read the file");
        page.eval(&src, "eval-before");
    }
    while page.parse_step(&mut parser) {
        if interactive {
            page.eval("__mrt.mirrorFlush(false)", "driver");
        }
    }
    drop(parser);
    page.eval("__mrt.parsingDone()", "driver");
    steps.push(step("scripts_run"));
    // (parsingDone set readyState "interactive" before the deferred scripts, as Firefox does)
    // DOMContentLoaded, with the stylesheet links' events around it (bridge.js)
    page.eval("__mrt.domContentLoaded()", "driver");
    // readyState complete and load, now or once what the document started loading is done (bridge.js
    // documentLoaded: Gecko's onload blockers); a frame runtime's load then reaches its frame element
    // in the parent (bridge.js tells the driver, which passes it on)
    page.eval("__mrt.documentLoaded()", "driver");
    let (timers, interactive_hibernations) = if interactive {
        let (ran, log, idle) = run_interactive(&mut page);
        (ran, format!(",\"interactive_hibernations\":[{}],\"interactive_idle\":{idle}", log.join(",")))
    } else {
        (run_tasks(&mut page, timer_limit, frame.is_some()), String::new())
    };
    steps.push(step("timers_run"));
    // MRT_EVAL_AFTER=FILE: a script run once the page has loaded, before it is measured; for looking
    // into a site (its console.log lines go to the console log)
    if let Some(path) = std::env::var_os("MRT_EVAL_AFTER") {
        let src = std::fs::read_to_string(&path).expect("MRT_EVAL_AFTER: cannot read the file");
        page.eval(&src, "eval-after");
    }
    page.trim();
    let load_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // Firefox's page has gone idle by the time it is measured (its shrinking GC once the user is
    // inactive: the nursery shrunk, script sources compressed; GAPS A16); then the harness's two GCs
    steps.push(step("trimmed"));
    let idle_ms = if std::env::var_os("MRT_NO_IDLE_GC").is_some() { 0 } else { page.idle_gc() };
    steps.push(step("idle"));
    page.gc();
    let js = page.memory();
    let dom_bytes = {
        let m = unsafe { mallinfo2() };
        let before = m.uordblks + m.hblkhd;
        let copy = page.dom().clone();
        let m = unsafe { mallinfo2() };
        let bytes = (m.uordblks + m.hblkhd).saturating_sub(before);
        drop(copy);
        bytes
    };
    let heap = {
        let m = unsafe { mallinfo2() };
        m.uordblks + m.hblkhd
    };
    steps.push(step("measured"));
    let (uss, pss) = uss_pss();
    // the same after giving free heap pages back (Gecko's jemalloc purges by itself): what the page
    // holds, without what layout flushes and loading freed
    unsafe { malloc_trim(0) };
    let (uss_trimmed, pss_trimmed) = uss_pss();
    dump_malloc_info();
    // where the heap and the unique memory are (GAPS A16): the Rust heap, glibc's totals,
    // SpiderMonkey's whole runtime, and the unique memory split by mapping
    let memory = {
        let m = unsafe { mallinfo2() };
        let rust = procmem::rust_heap();
        let console: usize = page.console().iter().map(|l| l.capacity()).sum();
        let unique = procmem::split_unique(procmem::thread_stack());
        let unique: Vec<String> = unique.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
        // the arena's arrays: removed nodes and replaced text stay in them (GAPS B10)
        let arena = {
            let d = page.dom();
            let h = d.heap_bytes();
            format!(
                "{{\"nodes\":{},\"nodes_bytes\":{},\"attrs\":{},\"attrs_bytes\":{},\"text_bytes\":{},\"names_bytes\":{}}}",
                d.nodes.len(),
                h.nodes,
                d.attrs.len(),
                h.attrs,
                h.text,
                h.names
            )
        };
        // each dedicated worker's runtime (W5), as Firefox reports each under workers/
        let workers = page.workers_memory().join(",");
        format!(
            "{{\"rust_heap\":{rust},\"dom\":{dom_bytes},\"workers\":[{workers}],\"dom_arena\":{arena},\"console\":{console},\"session_storage\":{},\"wrapper_cache\":{},\"mallinfo\":{{\"arena\":{},\"in_use\":{},\"free\":{},\"mmapped\":{},\"mmapped_blocks\":{},\"top_pad\":{}}},\"unique\":{{{}}},\"js\":{},\"steps\":[{}]}}",
            page.session_storage_bytes(),
            js.wrapper_cache,
            m.arena,
            m.uordblks,
            m.fordblks,
            m.hblkhd,
            m.hblks,
            m.keepcost,
            unique.join(","),
            page.memory_detail(),
            steps.join(",")
        )
    };
    // the style data kept between flushes (the parsed sheets, GAPS B41, the rule tables, the
    // elements' styles): computed results, as Firefox's style data, so they are in the totals above
    // and counted apart here, by dropping them; kept under MRT_EXIT_AFTER_REPORT, so that a heap
    // profiler sees them live at exit (kept_style_mb is then 0)
    let keep_live = std::env::var_os("MRT_EXIT_AFTER_REPORT").is_some();
    // MRT_HIBERNATE_CHECK=1: every document's boxes before hibernation (as hashes, so that they do not
    // weigh on the hibernated memory below), to check the wake against
    let check = std::env::var_os("MRT_HIBERNATE_CHECK").is_some() && !keep_live;
    let verbose = std::env::var("MRT_HIBERNATE_CHECK").as_deref() == Ok("verbose");
    let (before_hibernation, before_boxes, resizes_before) = if check {
        let all = page.flush_all();
        let hashes = box_hashes(&all);
        // MRT_HIBERNATE_CHECK=verbose keeps the boxes themselves, to name the elements that differ
        // (the hibernated memory then counts them)
        (hashes, if verbose { Some(all) } else { None }, page.resizes_pending())
    } else {
        (Vec::new(), None, false)
    };
    // then the page hibernates (DESIGN, "Hibernation of a frame (S6)"): the layout of every document
    // in the runtime, the page's and its frames', dropped; the boxes and the layout threads counted
    // apart from the style data
    let (kept_style_bytes, layout_cache_bytes) = if keep_live {
        (0, 0)
    } else {
        let before = procmem::rust_heap();
        page.drop_kept_style();
        let styled = procmem::rust_heap();
        page.hibernate();
        (before.saturating_sub(styled), styled.saturating_sub(procmem::rust_heap()))
    };
    // the DOM as saved (Dom::to_bytes), for measuring how well it compresses (DESIGN, "Compressing a
    // hibernated page"); the process's heap is read from outside by bench/heap_compress.py
    if let Ok(path) = std::env::var("MRT_DUMP_DOM") {
        std::fs::write(&path, page.dom().to_bytes()).unwrap();
    }
    let elements_all = page.dom().count_with_templates(Kind::Element);
    // what Firefox's DOM dump counts apart (`shadowRoots`, `shadowElements`)
    let (shadow_elements, shadow_roots) = page.dom().count_in_shadow_trees(Kind::Element);
    let elements = page.eval_number("document.querySelectorAll('*').length", "driver");
    let text_length = page.eval_number("document.documentElement ? document.documentElement.textContent.length : 0", "driver");
    let (bridge_calls, stale_reads, layout_panics) = page.bridge_stats();
    let layout_threads = page.layout_threads();
    // the linked-sheet cache, outside the page's state (SPEC decision 13): its text, counted apart
    let (linked_bytes, linked_sheets) = mrt_js::linked_sheets_bytes();
    let linked_sheets_mb = mb(linked_bytes);
    // the page under memory pressure (SPEC decision 14, GAPS A18): the style data and frames are
    // dropped (above), the garbage of the counting above collected and the free heap pages given
    // back; measured, not computed, as the allocator may keep pages it cannot give back. HEAP_SCAN's
    // compression scan (bench/heap_compress.py) reads the process in this state
    page.gc();
    unsafe { malloc_trim(0) };
    let (uss_pressure, _) = uss_pss();
    let pressure_unique = {
        let unique = procmem::split_unique(procmem::thread_stack());
        let unique: Vec<String> = unique.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
        unique.join(",")
    };
    // the wake: every document laid out from nothing, which must give the boxes it had and queue no
    // `resize` (its size is the one it had)
    let hibernate_check = if check {
        let t = Instant::now();
        let asleep = procmem::rust_heap();
        let all = page.flush_all();
        let wake_ms = t.elapsed().as_secs_f64() * 1000.0;
        // what the wake's layout from nothing holds (style data, frames, layout caches, and the boxes
        // returned here), against kept_style_mb plus layout_cache_mb, which the flushes had built
        // (GAPS B43: whether the kept data grows with the number of flushes)
        let woken_bytes = procmem::rust_heap().saturating_sub(asleep);
        let after = box_hashes(&all);
        if let Some(before) = &before_boxes {
            for (doc, boxes) in before {
                let woken = all.get(doc);
                let mut ids: Vec<&u32> = boxes.keys().collect();
                ids.sort();
                for id in ids {
                    let b = woken.and_then(|w| w.get(id));
                    if b != Some(&boxes[id]) {
                        let tag = describe(page.dom(), *id);
                        eprintln!("hibernate check: document {doc} node {id} {tag}: {:?} -> {:?}", boxes[id], b);
                    }
                }
                if let Some(w) = woken {
                    for id in w.keys().filter(|id| !boxes.contains_key(id)) {
                        eprintln!("hibernate check: document {doc} node {id} {}: new after the wake {:?}", describe(page.dom(), *id), w[id]);
                    }
                }
            }
        }
        let differ: Vec<String> = before_hibernation
            .iter()
            .filter(|b| !after.contains(b))
            .map(|(doc, n, _)| format!("{{\"document\":{doc},\"boxes\":{n}}}"))
            .collect();
        format!(
            ",\"hibernate_check\":{{\"documents\":{},\"boxes\":{},\"documents_after\":{},\"differ\":[{}],\"hashes\":[{}],\"resizes_before\":{resizes_before},\"resizes\":{},\"wake_ms\":{wake_ms:.1},\"woken_mb\":{:.3}}}",
            before_hibernation.len(),
            before_hibernation.iter().map(|b| b.1).sum::<usize>(),
            after.len(),
            differ.join(","),
            before_hibernation.iter().map(|(d, n, h)| format!("[{d},{n},\"{h:016x}\"]")).collect::<Vec<_>>().join(","),
            page.resizes_pending(),
            mb(woken_bytes)
        )
    } else {
        String::new()
    };
    let measured = format!(
        "{{\"url\":{},\"origin\":{},\"secure_context\":{},\"cross_origin_isolated\":{},\"elements\":{elements},\"elements_all\":{elements_all},\"shadow_roots\":{shadow_roots},\"shadow_elements\":{shadow_elements},\"text_length\":{text_length},\"timers_run\":{timers},\"js_errors\":{},\"console_lines\":{},\"layout_flushes\":{},\"stale_layout_reads\":{stale_reads},\"layout_panics\":{layout_panics},\"layout_threads\":{layout_threads},\"bridge_requests\":{bridge_calls},\"load_ms\":{load_ms:.0},\"idle_gc_wait_ms\":{idle_ms},\"dom_mb\":{:.3},\"js_realm_mb\":{:.3},\"js_zone_mb\":{:.3},\"js_mb\":{:.3},\"wrapper_cache_mb\":{:.3},\"heap_mb\":{:.3},\"uss_mb\":{:.3},\"pss_mb\":{:.3},\"uss_trimmed_mb\":{:.3},\"pss_trimmed_mb\":{:.3},\"linked_sheets\":{linked_sheets},\"linked_sheets_mb\":{linked_sheets_mb:.3},\"kept_style_mb\":{:.3},\"layout_cache_mb\":{:.3},\"uss_pressure_mb\":{:.3},\"memory\":{memory},\"memory_pressure\":{{\"unique\":{{{pressure_unique}}}}}{hibernate_check}{interactive_hibernations}}}",
        mrt_js::js_string(&url),
        mrt_js::js_string(&global.origin),
        global.secure_context,
        global.cross_origin_isolated,
        page.errors(),
        page.console().len(),
        page.flushes(),
        mb(dom_bytes),
        mb(js.realm.total()),
        mb(js.zone.total()),
        mb(js.realm.total() + js.zone.total()),
        mb(js.wrapper_cache),
        mb(heap),
        mb(uss),
        mb(pss),
        mb(uss_trimmed),
        mb(pss_trimmed),
        mb(kept_style_bytes),
        mb(layout_cache_bytes),
        mb(uss_pressure),
    );
    // the console as a JSON array of lines, to compare with Firefox's (GAPS B20, bench/compare_console.py)
    if let Ok(path) = std::env::var("MRT_DUMP_CONSOLE") {
        let lines: Vec<String> = page.console().iter().map(|l| mrt_js::js_string(l)).collect();
        std::fs::write(&path, format!("[{}]\n", lines.join(",\n"))).unwrap();
    }
    if render {
        page.eval("__mrt.render()", "driver");
    }
    page.eval(&format!("__mrt.report({measured})"), "driver");
    if let Some(hold_s) = std::env::var("MRT_PAGEOUT").ok().and_then(|v| v.parse::<u64>().ok()) {
        pageout(hold_s);
    }
    // for a heap profiler (heaptrack): what is still allocated at exit is what the page held when it
    // was measured
    if std::env::var_os("MRT_EXIT_AFTER_REPORT").is_some() {
        std::process::exit(0);
    }
}

/// An element for the hibernate check's messages: its tag, id and class.
fn describe(dom: &Dom, id: u32) -> String {
    format!("<{} id={:?} class={:?}>", dom.tag(id), dom.get_attribute(id, "id").unwrap_or(""), dom.get_attribute(id, "class").unwrap_or(""))
}

/// Each document's boxes as (document, element count, hash of every element's boxes in id order):
/// small enough to keep across a hibernation without counting in its memory.
fn box_hashes(all: &std::collections::BTreeMap<u32, std::collections::HashMap<u32, [f64; 8]>>) -> Vec<(u32, usize, u64)> {
    use std::hash::{Hash, Hasher};
    all.iter()
        .map(|(&doc, boxes)| {
            let mut ids: Vec<&u32> = boxes.keys().collect();
            ids.sort();
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for id in ids {
                id.hash(&mut h);
                boxes[id].iter().for_each(|v| v.to_bits().hash(&mut h));
            }
            (doc, boxes.len(), h.finish())
        })
        .collect()
}

/// scheme://host[:port] of an http(s) URL, "null" (an opaque origin) for anything else.
fn origin_of(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else { return "null".into() };
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https" | "ws" | "wss") {
        return "null".into();
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit('@').next().unwrap_or("").to_ascii_lowercase();
    let default_port = if matches!(scheme.as_str(), "https" | "wss") { ":443" } else { ":80" };
    format!("{scheme}://{}", host_port.strip_suffix(default_port).unwrap_or(&host_port))
}

/// Gecko's nsMixedContentBlocker::IsPotentiallyTrustworthyOrigin for the URLs a site can have:
/// https, wss and file are trustworthy a priori; http and ws only on a loopback host
/// (IsPotentiallyTrustworthyLoopbackURL: localhost, *.localhost, 127.0.0.0/8, [::1]). The pref
/// allowlist and .onion hosts (both off by default) are not looked at.
fn is_potentially_trustworthy(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else { return false };
    match scheme.to_ascii_lowercase().as_str() {
        "https" | "wss" | "file" => true,
        "http" | "ws" => {
            let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
            let host_port = authority.rsplit('@').next().unwrap_or("").to_ascii_lowercase();
            let host = if host_port.starts_with('[') {
                host_port.split(']').next().unwrap_or("").trim_start_matches('[').to_string()
            } else {
                host_port.split(':').next().unwrap_or("").to_string()
            };
            host == "localhost" || host.ends_with(".localhost") || host == "::1" || host.starts_with("127.")
        }
        _ => false,
    }
}

fn main() {
    set_arena_max();
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: realsite <document.html> <url> [--console] [--timer-limit MS] [--max-flushes N] [--render] [--origin O] [--secure-context 0|1] [--cross-origin-isolated 0|1] [--viewport W H] [--frame START_MS] [--interactive]";
    let file = args.get(1).expect(usage).clone();
    let url = args.get(2).expect(usage).clone();
    let echo = args.iter().any(|a| a == "--console");
    // send the final DOM to the driver to be rendered in a Firefox tab (after the measurements)
    let render = args.iter().any(|a| a == "--render");
    let opt = |name: &str, default: usize| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)?.parse().ok()).unwrap_or(default);
    // 15 s on the virtual clock, as long as Firefox's settle before it is measured
    let timer_limit = opt("--timer-limit", 15_000) as u32;
    // no flush budget by default: every layout read gets fresh boxes, as in Firefox. The budget was
    // 50 while each flush laid out the whole page (GAPS B16); it stays for debugging a page that
    // flushes too often
    let max_flushes = opt("--max-flushes", usize::MAX);
    // The page's global as Firefox makes its window's: the origin, secure context and cross-origin
    // isolation Firefox reports for the page (the driver reads them in the bridge tab); without
    // them, the origin and secure context are worked out from the URL, and the page is taken as not
    // cross-origin isolated (that needs COOP and COEP headers the runtime does not see)
    let text_opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let origin = text_opt("--origin").unwrap_or_else(|| origin_of(&url));
    let global = GlobalOptions {
        secure_context: text_opt("--secure-context").map_or_else(|| is_potentially_trustworthy(&url), |v| v == "1"),
        cross_origin_isolated: text_opt("--cross-origin-isolated").is_some_and(|v| v == "1"),
        origin,
    };
    // the viewport to lay out at: the bridge tab's (innerWidth, innerHeight), so the page sees the
    // window Firefox gives it (GAPS B28); 1489 by 844 without it
    if let Some(i) = args.iter().position(|a| a == "--viewport") {
        let size = |j: usize| args.get(i + j).and_then(|v| v.parse::<i32>().ok()).expect(usage);
        mrt_js::set_viewport(size(1), size(2));
    }
    // a cross-origin frame's runtime (SPEC decision 15): its tasks, its load among them, wait for their
    // turn in the event loop the driver merges, from its parent's time when the frame opened
    let frame = text_opt("--frame").map(|v| v.parse::<f64>().expect(usage));
    // run on the real clock until the driver says stop, for a page used from the bridge tab (SPEC
    // decision 25); a frame runtime is never interactive
    let interactive = frame.is_none() && args.iter().any(|a| a == "--interactive");
    let html = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned();
    // a thread with a big stack, as jscount: the page nests deeply, and the JS context belongs to it
    std::thread::Builder::new().stack_size(2 << 30).spawn(move || run(html, url, global, echo, timer_limit, max_flushes, render, frame, interactive)).unwrap().join().unwrap();
    mrt_js::shutdown();
}
