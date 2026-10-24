//! Dedicated and shared workers (DESIGN, "Workers: the build plan"; GAPS B85): each on a thread of
//! its own with a JS context of its own (Gecko's WorkerThread and WorkerJSContext), its global a
//! DedicatedWorkerGlobalScope or a SharedWorkerGlobalScope made by the C++ layer
//! (`mrt_js_new_worker`), with prelude.js and bridge.js run in it. A shared worker is one of the
//! runtime's like a dedicated one; bridge.js connects each of the page's SharedWorker objects to it
//! with a port.
//!
//! The page and its workers take turns: one task at a time, in the order of their virtual times
//! (SPEC decision 16's merged loop, inside one process). The page's task queue (prelude.js) counts a
//! worker's next task as one of its own; when it comes up, the page's thread hands it to the
//! worker's thread and waits for the task to end. So no two threads run script at once, and the
//! bridge's channel (stdout and stdin) is used by one thread at a time.

use std::ffi::{c_char, c_void};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

use crate::{GlobalOptions, Host, Page};

/// What the page's thread asks a worker's.
enum Command {
    /// A message for the worker (clone_codec.js's encoding), a task at this virtual time.
    Post(String, f64),
    /// Run the worker's task due first.
    Run,
    /// A GC of the worker's runtime with the page's (W5, Gecko's memory-pressure GC on workers):
    /// shrinking when the page goes idle (`true`, `Page::idle_gc`), else two full ones (`Page::gc`).
    Gc(bool),
    /// One shrinking GC, without the idle GC's wait (`Page::shrink_gc`).
    Shrink,
    /// The worker's JS runtime, as `Page::memory_detail` reports the page's.
    Memory,
    /// End the worker (`terminate()`, or the page going away).
    Terminate,
}

/// A worker's answer to each command.
struct Reply {
    /// When its next task is due on the virtual clock; None when it has none (it waits for messages).
    next: Option<f64>,
    /// What it handed its parent meanwhile (`__mrtWorkerOut`): JSON texts bridge.js reads.
    out: Vec<String>,
    /// Its console lines meanwhile, for the page's console (Firefox's console shows workers' too).
    console: Vec<String>,
    /// It called `close()`, or its script failed to load: the thread has ended.
    closed: bool,
    /// The answer to `Gc` (the milliseconds the idle GC waited) or to `Memory` (a JSON object).
    info: String,
}

/// One worker's thread and the two channels to it.
struct WorkerThread {
    commands: Sender<Command>,
    replies: Receiver<Reply>,
    thread: Option<JoinHandle<()>>,
    next: Option<f64>,
}

impl WorkerThread {
    /// Send a command and wait for the worker to finish it. None if its thread has ended.
    fn ask(&mut self, command: Command) -> Option<Reply> {
        self.commands.send(command).ok()?;
        let reply = self.replies.recv().ok()?;
        self.next = reply.next;
        Some(reply)
    }

    fn join(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The workers of one runtime, by id (the index; an ended worker leaves None).
#[derive(Default)]
pub struct Workers {
    list: Vec<Option<WorkerThread>>,
}

impl Workers {
    pub fn count(&self) -> usize {
        self.list.len()
    }

    fn get(&mut self, id: usize) -> Option<&mut WorkerThread> {
        self.list.get_mut(id).and_then(|w| w.as_mut())
    }

    fn end(&mut self, id: usize) {
        if let Some(mut w) = self.list.get_mut(id).and_then(|w| w.take()) {
            let _ = w.commands.send(Command::Terminate);
            w.join();
        }
    }
}

impl Workers {
    /// A GC of every live worker (`Command::Gc`); the longest idle GC's wait, in ms.
    pub fn gc(&mut self, idle: bool) -> usize {
        let mut waited = 0;
        for w in self.list.iter_mut().flatten() {
            if let Some(reply) = w.ask(Command::Gc(idle)) {
                waited = waited.max(reply.info.parse().unwrap_or(0));
            }
        }
        waited
    }

    /// A shrinking GC of every live worker (`Command::Shrink`).
    pub fn shrink_gc(&mut self) {
        for w in self.list.iter_mut().flatten() {
            w.ask(Command::Shrink);
        }
    }

    /// Each live worker's runtime (`Command::Memory`), JSON objects.
    pub fn memory(&mut self) -> Vec<String> {
        self.list.iter_mut().flatten().filter_map(|w| w.ask(Command::Memory)).map(|r| r.info).collect()
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        for id in 0..self.list.len() {
            self.end(id);
        }
    }
}

/// A worker's thread: its context and global, its JS half, then commands until it ends. The page's
/// thread waits for each reply, so this thread runs script only while the page's does not.
fn worker_main(config: String, global: GlobalOptions, shared: bool, at: f64, echo: bool, commands: Receiver<Command>, replies: Sender<Reply>) {
    let mut page = Page::new_worker(&global, shared);
    page.enable_bridge();
    page.echo_console(echo);
    page.load_worker_bridge();
    page.eval(&format!("__mrt.workerStart({}, {at})", crate::js_string(&config)), "worker");
    let mut info = String::new();
    let reply = |page: &mut Page, info: String| {
        let out = std::mem::take(&mut page.host_mut().worker_out);
        let closed = page.eval_number("__mrt.workerClosed() ? 1 : 0", "worker") == 1.0;
        let next = page.eval_number("__mrt.nextTimerTime()", "worker");
        let console = std::mem::take(&mut page.host_mut().console);
        let _ = replies.send(Reply { next: if closed || next < 0.0 { None } else { Some(next) }, out, console, closed, info });
        closed
    };
    if reply(&mut page, String::new()) {
        return;
    }
    while let Ok(command) = commands.recv() {
        match command {
            Command::Post(data, at) => {
                page.eval(&format!("__mrt.workerReceive({}, {at})", crate::js_string(&data)), "worker");
            }
            Command::Run => {
                page.run_timers_stepwise(u32::MAX, 1);
            }
            Command::Gc(idle) => {
                info = if idle { page.idle_gc().to_string() } else { page.gc(); String::new() };
            }
            Command::Shrink => page.shrink_gc(),
            Command::Memory => {
                // the worker as Firefox's report names it (workers/workers(<site>)/worker(<url>)):
                // its config (URL, name, type), what it read that is not built (handed over as an
                // out item and taken back here), then its runtime
                page.eval("__mrtWorkerOut(__mrt.workerMissingAPIs())", "worker");
                let missing = page.host_mut().worker_out.pop().unwrap_or_else(|| "[]".into());
                info = format!("{{\"config\":{config},\"missing_apis\":{missing},\"js\":{}}}", page.memory_detail());
                // MRT_WORKER_CENSUS=SCRIPT (with MRT_WORKER_CENSUS_OUT=DIR): a census of the worker's
                // strings after its memory is taken (bench/worker_census.js, GAPS B89), one JSON file
                // a worker, named by its name
                if let (Some(script), Some(dir)) = (std::env::var_os("MRT_WORKER_CENSUS"), std::env::var_os("MRT_WORKER_CENSUS_OUT")) {
                    let src = std::fs::read_to_string(&script).expect("MRT_WORKER_CENSUS: cannot read the script");
                    page.eval(&format!("__mrtWorkerOut(__mrtDebuggerEval({}))", crate::js_string(&src)), "worker");
                    let census = page.host_mut().worker_out.pop().unwrap_or_default();
                    let name: String = worker_file_name(&config);
                    let _ = std::fs::write(std::path::Path::new(&dir).join(format!("{name}.json")), census);
                }
            }
            Command::Terminate => return,
        }
        if reply(&mut page, std::mem::take(&mut info)) {
            return;
        }
    }
}

unsafe fn host<'a>(p: *mut c_void) -> &'a mut Host {
    unsafe { &mut *(p as *mut Host) }
}

unsafe fn text<'a>(p: *const c_char, len: usize) -> &'a str {
    if len == 0 {
        return "";
    }
    unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(p as *const u8, len)) }
}

/// `new Worker()`, or a `new SharedWorker()` that no running shared worker matches (`shared`): a
/// worker thread for `config` (bridge.js's JSON), its global of `origin`; its script loads in its
/// first task, at `at`. Returns its id.
#[unsafe(no_mangle)]
extern "C" fn mrt_worker_new(h: *mut c_void, config: *const c_char, config_len: usize, origin: *const c_char,
                             origin_len: usize, secure: i32, isolated: i32, at: f64, shared: i32) -> i32 {
    let h = unsafe { host(h) };
    let config = unsafe { text(config, config_len) }.to_string();
    let global = GlobalOptions {
        origin: unsafe { text(origin, origin_len) }.to_string(),
        secure_context: secure != 0,
        cross_origin_isolated: isolated != 0,
    };
    let echo = h.echo_console;
    let (commands, worker_commands) = channel();
    let (worker_replies, replies) = channel();
    // the main thread's size: SpiderMonkey has no stack quota set here (mrt_js_new does not set one)
    let thread = std::thread::Builder::new()
        .name("mrt-worker".into())
        .stack_size(8 << 20)
        .spawn(move || worker_main(config, global, shared != 0, at, echo, worker_commands, worker_replies))
        .expect("a worker's thread");
    let mut w = WorkerThread { commands, replies, thread: Some(thread), next: None };
    // its first reply: set up, its script's load queued
    let first = w.replies.recv().ok();
    let id = h.workers.list.len();
    if let Some(first) = first {
        w.next = first.next;
        h.console.extend(first.console);
        h.worker_pending.push((id, first.out));
        if first.closed {
            w.join();
        }
    }
    h.workers.list.push(Some(w));
    id as i32
}

/// `postMessage` to a worker: a task at `at` in its queue. Nothing for a worker that has ended.
#[unsafe(no_mangle)]
extern "C" fn mrt_worker_post(h: *mut c_void, id: f64, data: *const c_char, len: usize, at: f64) {
    let h = unsafe { host(h) };
    let data = unsafe { text(data, len) }.to_string();
    let id = id as usize;
    let Some(w) = h.workers.get(id) else { return };
    if let Some(reply) = w.ask(Command::Post(data, at)) {
        h.console.extend(reply.console);
        if !reply.out.is_empty() {
            h.worker_pending.push((id, reply.out));
        }
    }
}

/// The worker task due first, by virtual time then id; -1 if no worker has one.
#[unsafe(no_mangle)]
extern "C" fn mrt_worker_next(h: *mut c_void, at: *mut f64) -> i32 {
    let h = unsafe { host(h) };
    let mut best: Option<(usize, f64)> = None;
    for (id, w) in h.workers.list.iter().enumerate() {
        if let Some(next) = w.as_ref().and_then(|w| w.next) {
            if best.is_none_or(|(_, t)| next < t) {
                best = Some((id, next));
            }
        }
    }
    match best {
        Some((id, t)) => {
            unsafe { *at = t };
            id as i32
        }
        None => -1,
    }
}

/// Run worker `id`'s next task. Returns what it handed its parent since the last run, as a JSON array
/// text (each item a worker's JSON text, in order).
#[unsafe(no_mangle)]
extern "C" fn mrt_worker_run(h: *mut c_void, id: f64, out: *mut *const c_char, out_len: *mut usize) {
    let h = unsafe { host(h) };
    let id = id as usize;
    if let Some(w) = h.workers.get(id) {
        if let Some(reply) = w.ask(Command::Run) {
            h.console.extend(reply.console);
            h.worker_pending.push((id, reply.out));
            if reply.closed {
                if let Some(w) = h.workers.get(id) {
                    w.join();
                    w.next = None;
                }
            }
        } else {
            // its thread ended (a panic): nothing more from it
            h.workers.end(id);
        }
    }
    let mut items = Vec::new();
    for (from, list) in std::mem::take(&mut h.worker_pending) {
        if from == id {
            items.extend(list);
        } else {
            h.worker_pending.push((from, list));
        }
    }
    let s = format!("[{}]", items.join(","));
    h.give_string(s, out, out_len);
}

/// `terminate()`: the worker's thread ends; what it handed over and was not delivered is dropped.
#[unsafe(no_mangle)]
extern "C" fn mrt_worker_terminate(h: *mut c_void, id: f64) {
    let h = unsafe { host(h) };
    let id = id as usize;
    h.workers.end(id);
    h.worker_pending.retain(|(from, _)| *from != id);
}

/// In a worker: an item for its parent (bridge.js's JSON text), handed over when the task ends.
#[unsafe(no_mangle)]
extern "C" fn mrt_worker_out(h: *mut c_void, s: *const c_char, len: usize) {
    let h = unsafe { host(h) };
    h.worker_out.push(unsafe { text(s, len) }.to_string());
}

/// A worker's name from its config JSON (`"name":"..."`), for a file name; "worker" when it has none.
fn worker_file_name(config: &str) -> String {
    let name = config.split("\"name\":\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("");
    let clean: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    if clean.is_empty() { "worker".into() } else { clean }
}
