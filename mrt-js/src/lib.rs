//! Stage 3: script over the arena DOM. SpiderMonkey from the Firefox tree runs the page's scripts;
//! the C++ layer in `glue/` holds the reflectors and calls back into `host.rs` for every DOM read or
//! change. See DESIGN "Stage 3: JS" in the project docs.

pub mod flushrec;
// zlib-rs's C API (the MOZ_Z_ symbols the C++ layer's CompressionStream calls), linked in
extern crate libz_rs_sys;
mod host;
pub mod procmem;
mod worker;

use std::ffi::{c_char, c_void, CString};

pub use host::{linked_sheets_bytes, replay_flushes, take_task_times, Host, TaskTimes};
pub use mrt_layout;
pub use mrt_style;
use mrt_style::mrt_dom::{Dom, Kind, NodeId, DOCUMENT, NS_HTML};

/// The default viewport, as in `layoutcount` and the Firefox runs of the test pages.
pub const WIDTH: i32 = 1489;
pub const HEIGHT: i32 = 844;

static VIEWPORT: [std::sync::atomic::AtomicI32; 2] = [std::sync::atomic::AtomicI32::new(WIDTH), std::sync::atomic::AtomicI32::new(HEIGHT)];

/// The viewport the page is laid out at, in CSS px (`WIDTH` by `HEIGHT` unless `set_viewport`).
pub fn viewport() -> (i32, i32) {
    use std::sync::atomic::Ordering::Relaxed;
    (VIEWPORT[0].load(Relaxed), VIEWPORT[1].load(Relaxed))
}

/// Lay pages out at `width` by `height` CSS px: `realsite` takes the bridge tab's viewport, so a
/// site sees the same window size on both sides (GAPS B28). Set before the first layout.
pub fn set_viewport(width: i32, height: i32) {
    use std::sync::atomic::Ordering::Relaxed;
    VIEWPORT[0].store(width, Relaxed);
    VIEWPORT[1].store(height, Relaxed);
}

#[repr(C)]
struct MrtJs {
    _private: [u8; 0],
}

/// Bytes of one realm or zone, grouped as `JS::ServoSizes` groups them.
#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct JsSizes {
    pub gc_used: usize,
    pub gc_unused: usize,
    pub gc_admin: usize,
    pub malloc_heap: usize,
    pub non_heap: usize,
}

impl JsSizes {
    pub fn total(&self) -> usize {
        self.gc_used + self.gc_unused + self.gc_admin + self.malloc_heap + self.non_heap
    }
}

/// The page's JS memory: its realm (Firefox's `js-realm` under the window) and its zone (`js-zone`).
#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct JsMemory {
    pub realm: JsSizes,
    pub zone: JsSizes,
    /// Free and admin bytes of the zone's GC buffer allocator, not under the window in Firefox's
    /// report and so not in `zone`.
    pub zone_buffer_slack: usize,
    /// The C++ layer's wrapper cache, 8 bytes per node id up to the highest one wrapped (Gecko keeps
    /// the same pointer in each node, so it belongs with the DOM).
    pub wrapper_cache: usize,
    pub reflectors: usize,
    pub preserved: usize,
}

#[repr(C)]
struct MrtGlobalOptions {
    origin: *const c_char,
    origin_len: usize,
    secure_context: i32,
    cross_origin_isolated: i32,
}

/// What Gecko's window global is made from (`CreateNativeGlobalForInner`): the document's origin
/// ("null" for an opaque one), whether it is a secure context, whether it is cross-origin isolated
/// (only then is there a `SharedArrayBuffer` constructor).
#[derive(Debug, Clone)]
pub struct GlobalOptions {
    pub origin: String,
    pub secure_context: bool,
    pub cross_origin_isolated: bool,
}

impl GlobalOptions {
    /// A page served from `http://localhost`, as Firefox's side of the test pages loads them
    /// (`bench/firefox_js.py`): loopback is a secure context; not cross-origin isolated.
    pub fn localhost() -> GlobalOptions {
        GlobalOptions { origin: "http://localhost".into(), secure_context: true, cross_origin_isolated: false }
    }

    fn raw(&self) -> MrtGlobalOptions {
        MrtGlobalOptions {
            origin: self.origin.as_ptr() as *const c_char,
            origin_len: self.origin.len(),
            secure_context: self.secure_context as i32,
            cross_origin_isolated: self.cross_origin_isolated as i32,
        }
    }
}

/// How a frame's global sits relative to the page's (`Page::frame_global_relation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameGlobalRelation {
    pub same_zone_as_page: bool,
    pub same_compartment_as_page: bool,
    pub secure_context: bool,
    pub same_compartment_as_earlier_frame: bool,
}

unsafe extern "C" {
    fn mrt_js_new(host: *mut c_void, page: *const MrtGlobalOptions) -> *mut MrtJs;
    fn mrt_js_new_frame_global(js: *mut MrtJs, frame: *const MrtGlobalOptions) -> i32;
    fn mrt_js_new_worker(host: *mut c_void, worker: *const MrtGlobalOptions, shared: i32) -> *mut MrtJs;
    fn mrt_js_frame_global_relation(js: *mut MrtJs, i: i32) -> i32;
    fn mrt_js_free(js: *mut MrtJs);
    fn mrt_js_set_window_script(js: *mut MrtJs, which: i32, src: *const c_char, len: usize);
    fn mrt_js_shutdown();
    fn mrt_js_eval(js: *mut MrtJs, src: *const c_char, len: usize, filename: *const c_char) -> i32;
    fn mrt_js_eval_number(js: *mut MrtJs, src: *const c_char, len: usize, filename: *const c_char, out: *mut f64) -> i32;
    fn mrt_js_eval_string(js: *mut MrtJs, src: *const c_char, len: usize, filename: *const c_char, out_len: *mut usize) -> *const c_char;
    fn mrt_js_errors(js: *mut MrtJs) -> usize;
    fn mrt_js_parser_stopped();
    fn mrt_js_parser_script(script: u32);
    fn mrt_js_gc(js: *mut MrtJs);
    fn mrt_js_idle_gc(js: *mut MrtJs) -> usize;
    fn mrt_js_shrink_gc(js: *mut MrtJs);
    fn mrt_js_memory(js: *mut MrtJs, out: *mut JsMemory);
    fn mrt_js_memory_detail(js: *mut MrtJs, out: *mut c_char, cap: usize) -> usize;
    fn mrt_js_session_storage_bytes(js: *mut MrtJs) -> usize;
}

const PRELUDE_SOURCE: &str = include_str!("../glue/prelude.js");
/// Generated by tools/gen_css_properties.py; put into prelude.js's scope at its `@CSS_PROPERTIES@` line.
const CSS_PROPERTIES: &str = include_str!("../glue/css_properties.js");
/// Generated by tools/gen_event_attribute_names.py; put into prelude.js's scope at its
/// `@EVENT_ATTRIBUTE_NAMES@` line (Trusted Types' event handler attributes).
const EVENT_ATTRIBUTE_NAMES: &str = include_str!("../glue/event_attribute_names.js");
/// The prelude with its generated parts in place.
static PRELUDE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    PRELUDE_SOURCE.replacen("// @CSS_PROPERTIES@", CSS_PROPERTIES, 1).replacen("  // @EVENT_ATTRIBUTE_NAMES@", EVENT_ATTRIBUTE_NAMES, 1)
});
const BRIDGE: &str = include_str!("../glue/bridge.js");
/// Generated by tools/gen_html_interfaces.py; put into bridge.js's scope at its `@HTML_INTERFACES@` line.
const HTML_INTERFACES: &str = include_str!("../glue/html_interfaces.js");
/// The structured-clone codec shared with the bridge tab's IndexedDB side (bench/idb_bridge.js); put
/// into bridge.js at its `@CLONE_CODEC@` line.
const CLONE_CODEC: &str = include_str!("../glue/clone_codec.js");
/// Where release Firefox puts each name of a dedicated worker's global (tools/gen_worker_names.py); put
/// into bridge.js's worker section at its `@WORKER_NAMES@` line.
const WORKER_NAMES: &str = include_str!("../glue/worker_names.js");
/// WebGL forwarded to the bridge tab (task 23bb); put into bridge.js at its `@WEBGL@` line, with the
/// tables tools/gen_webgl_idl.py generates from Gecko's WebIDL at webgl.js's `@WEBGL_IDL@` line.
const WEBGL: &str = include_str!("../glue/webgl.js");
const WEBGL_IDL: &str = include_str!("../glue/webgl_idl.js");
/// bridge.js with its generated and separate parts in place.
static BRIDGE_FULL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    BRIDGE.replacen("// @HTML_INTERFACES@", HTML_INTERFACES, 1).replacen("// @CLONE_CODEC@", CLONE_CODEC, 1).replacen("// @WORKER_NAMES@", WORKER_NAMES, 1)
        .replacen("  // @WEBGL@\n", &WEBGL.replacen("  // @WEBGL_IDL@\n", WEBGL_IDL, 1), 1)
});

/// Shut SpiderMonkey down. Call once, after every `Page` is dropped, before the process exits.
pub fn shutdown() {
    unsafe { mrt_js_shutdown() }
}

/// What `run_scripts` did.
#[derive(Default, Debug)]
pub struct ScriptRun {
    pub run: usize,
    /// Classic `<script src>` and scripts of other types: not run (no loading here).
    pub skipped: usize,
}

/// A document with a JS global. One per thread: SpiderMonkey's context, and the layout a flush
/// does, belong to the thread that made it.
pub struct Page {
    host: Box<Host>,
    js: *mut MrtJs,
}

impl Page {
    /// Make the global for `dom` and run the prelude.
    /// A page with the global Firefox gives a page served from localhost (`GlobalOptions::localhost`).
    pub fn new(dom: Dom) -> Page {
        Page::with_global(dom, &GlobalOptions::localhost())
    }

    /// A page whose global is made as Gecko makes a content window's from `global`.
    pub fn with_global(dom: Dom, global: &GlobalOptions) -> Page {
        let mut host = Box::new(Host::new(dom));
        let raw = global.raw();
        let js = unsafe { mrt_js_new(&mut *host as *mut Host as *mut c_void, &raw) };
        assert!(!js.is_null(), "SpiderMonkey failed to start");
        let mut page = Page { host, js };
        assert!(page.eval(&PRELUDE, "prelude.js"), "the prelude threw");
        // every frame's window evaluates it too (DESIGN, "Same-origin frames: the build plan", S3)
        unsafe { mrt_js_set_window_script(page.js, 0, PRELUDE.as_ptr() as *const c_char, PRELUDE.len()) };
        page
    }

    /// A worker's global (worker.rs), made on the calling thread, which is the worker's own: a
    /// SharedWorkerGlobalScope when `shared`, else a DedicatedWorkerGlobalScope. `load_worker_bridge`
    /// gives it its JS half.
    pub(crate) fn new_worker(global: &GlobalOptions, shared: bool) -> Page {
        let mut host = Box::new(Host::new(Dom::default()));
        let raw = global.raw();
        let js = unsafe { mrt_js_new_worker(&mut *host as *mut Host as *mut c_void, &raw, shared as i32) };
        assert!(!js.is_null(), "SpiderMonkey failed to start a worker");
        Page { host, js }
    }

    /// A worker's JS half: prelude.js and bridge.js, which leave out what is [Exposed=Window] only.
    pub(crate) fn load_worker_bridge(&mut self) {
        assert!(self.eval(&PRELUDE, "prelude.js"), "the prelude threw in a worker");
        let bridge: &str = &BRIDGE_FULL;
        assert!(self.eval(bridge, "bridge.js"), "bridge.js threw in a worker");
    }

    pub(crate) fn host_mut(&mut self) -> &mut Host {
        &mut self.host
    }

    /// Run a script in the page's global; false if it threw (reported on stderr).
    pub fn eval(&mut self, src: &str, filename: &str) -> bool {
        let name = CString::new(filename).unwrap();
        unsafe { mrt_js_eval(self.js, src.as_ptr() as *const c_char, src.len(), name.as_ptr()) != 0 }
    }

    /// Run a script and return its value as a number (NaN if it threw or is not a number).
    pub fn eval_number(&mut self, src: &str, filename: &str) -> f64 {
        let name = CString::new(filename).unwrap();
        let mut out = f64::NAN;
        unsafe { mrt_js_eval_number(self.js, src.as_ptr() as *const c_char, src.len(), name.as_ptr(), &mut out) };
        out
    }

    /// Run a script and return its value as text (empty if it threw).
    pub fn eval_string(&mut self, src: &str, filename: &str) -> String {
        let name = CString::new(filename).unwrap();
        let mut len = 0;
        let p = unsafe { mrt_js_eval_string(self.js, src.as_ptr() as *const c_char, src.len(), name.as_ptr(), &mut len) };
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(p as *const u8, len) }).into_owned()
    }

    /// Run the timers due before `limit_ms` on the virtual clock one per evaluation, so that the
    /// promise jobs each one queues run before the next (a microtask checkpoint, as in Firefox).
    /// Stops after `max` callbacks (a page polling with a short interval). Returns how many ran.
    pub fn run_timers_stepwise(&mut self, limit_ms: u32, max: usize) -> usize {
        let mut ran = 0;
        while ran < max && self.eval_number(&format!("__mrt.runNextTimer({limit_ms})"), "driver") == 1.0 {
            ran += 1;
        }
        ran
    }

    /// A request of the runtime's own to the driver (not bridge.js's), and the reply line.
    pub fn bridge_raw(&mut self, req: &str) -> String {
        self.host.bridge_request(req)
    }

    /// Stage 6: send `__mrtBridge` requests to the driver on stdout and read replies on stdin.
    pub fn enable_bridge(&mut self) {
        self.host.bridge = true;
    }

    /// Load the bridge's JS side (glue/bridge.js) and point the page at `url`. Needs `enable_bridge`:
    /// it asks Firefox for the browser's identity and API names at once.
    pub fn load_bridge(&mut self, url: &str) -> bool {
        let bridge: &str = &BRIDGE_FULL;
        unsafe { mrt_js_set_window_script(self.js, 1, bridge.as_ptr() as *const c_char, bridge.len()) };
        self.eval(bridge, "bridge.js") && self.eval(&format!("__mrt.setURL({})", js_string(url)), "driver")
    }

    /// Trim the DOM's arrays and drop the boxes of the last layout flush, as `finish_loading` does.
    pub fn trim(&mut self) {
        self.host.dom.shrink_to_fit();
        self.host.drop_layout_cache();
    }

    /// Layout flushes allowed before layout reads are answered from the last flush (see `Host`).
    pub fn set_max_flushes(&mut self, n: usize) {
        self.host.max_flushes = n;
    }

    /// (bridge calls, stale layout reads, layout panics)
    pub fn bridge_stats(&self) -> (usize, usize, usize) {
        (self.host.bridge_calls, self.host.stale_reads, self.host.layout_panics)
    }

    /// Layout threads running now, one per document with a layout (Host::layouts_live).
    pub fn layouts_live(&self) -> usize {
        self.host.layouts_live()
    }

    /// Layout threads started (Host::layout_threads).
    pub fn layout_threads(&self) -> usize {
        self.host.layout_threads
    }

    /// Drop the style data kept between flushes (the parsed sheets, GAPS B41, the rule tables and
    /// every element's style): computed results; the next flush styles the whole page.
    pub fn drop_kept_style(&mut self) {
        self.host.drop_kept_style();
    }

    /// Hibernate the layout of the page and of every frame document in this runtime (Host::hibernate):
    /// style data, frames, boxes and layout threads go; the DOM, the JS heap and the windows stay.
    /// The page is then a background tab (SPEC decision 22, prelude.js's `hide`): each document
    /// gets `visibilitychange` in a task and is hidden, animation frames wait, timeouts are throttled.
    pub fn hibernate(&mut self) {
        self.host.hibernate();
        self.eval("__mrt.hide()", "driver");
    }

    /// The page shown again after `hibernate`: every document visible, its timeouts and animation
    /// frames as in a visible tab. Layout comes back at the next read, from scratch.
    pub fn wake(&mut self) {
        self.eval("__mrt.show()", "driver");
    }

    /// An HTTP viewer's viewport, synchronized before its first input is accepted.
    pub fn resize_viewport(&mut self, width: i32, height: i32) {
        if viewport() != (width, height) {
            self.host.resize_page(width, height);
            self.eval(&format!("__mrt.viewerViewport({width}, {height})"), "driver");
        }
    }

    /// Every document laid out (Host::flush_all): each one's boxes by element, by document.
    pub fn flush_all(&mut self) -> std::collections::BTreeMap<NodeId, std::collections::HashMap<NodeId, [f64; 8]>> {
        self.host.flush_all()
    }

    /// Whether a frame document's `resize` waits for the next refresh tick.
    pub fn resizes_pending(&self) -> bool {
        self.host.resizes_pending()
    }

    /// Run the page's inline classic scripts in document order, as the parser would have when it
    /// reached each one, except that the whole document is already there (DESIGN "Stage 3"); then
    /// its module scripts, in document order after the classic ones as Firefox defers them (async
    /// ones too: they are ready at once). External classic scripts are skipped (no loading here).
    pub fn run_scripts(&mut self, name: &str) -> ScriptRun {
        let mut run = ScriptRun::default();
        let mut modules = Vec::new();
        for (i, id) in script_elements(&self.host.dom).into_iter().enumerate() {
            let dom = &self.host.dom;
            let kind = dom.get_attribute(id, "type").unwrap_or("").trim().to_ascii_lowercase();
            if kind == "module" {
                modules.push(id);
                continue;
            }
            let classic = matches!(kind.as_str(), "" | "text/javascript" | "application/javascript");
            if dom.get_attribute(id, "src").is_some() || !classic {
                run.skipped += 1;
                continue;
            }
            let src = dom.text_content(id);
            self.eval(&src, &format!("{name}#script{i}"));
            run.run += 1;
        }
        for id in modules {
            // the element by its place among the document's scripts now (earlier scripts may have
            // added some)
            let Some(k) = script_elements(&self.host.dom).iter().position(|&s| s == id) else { continue };
            self.eval(&format!("__mrt.runModuleScript(document.querySelectorAll('script')[{k}])"), "driver");
            run.run += 1;
        }
        run
    }

    /// DOMContentLoaded and load, then the timers until `limit_ms` on the virtual clock.
    /// Then the DOM's arrays are trimmed, as after parsing, and the boxes of the last layout flush are
    /// dropped (the page's own layout makes them again).
    pub fn finish_loading(&mut self, limit_ms: u32) {
        self.eval("__mrt.fireLoadEvents()", "driver");
        // one timer per task, each with its microtask checkpoint and rejection flush, as the event
        // loop runs them
        self.run_timers_stepwise(limit_ms, 50_000);
        self.host.dom.shrink_to_fit();
        self.host.drop_layout_cache();
    }

    /// Exceptions that reached the top (scripts, timers, listeners are caught and logged separately).
    pub fn errors(&self) -> usize {
        unsafe { mrt_js_errors(self.js) }
    }

    /// Heap bytes of the page's sessionStorage, which is page state (SPEC decision 14).
    pub fn session_storage_bytes(&self) -> usize {
        unsafe { mrt_js_session_storage_bytes(self.js) }
    }

    pub fn console(&self) -> &[String] {
        &self.host.console
    }

    /// Print console messages to stderr as they come.
    pub fn echo_console(&mut self, on: bool) {
        self.host.echo_console = on;
    }

    /// Layout flushes the script caused (offsetWidth and the others).
    pub fn flushes(&self) -> usize {
        self.host.flushes
    }

    /// Two full, non-shrinking GCs, as `bench/firefox_js.py` runs `windowUtils.garbageCollect()` twice
    /// before Firefox's report (see `mrt_js_gc`).
    /// The page's dedicated workers too (W5), as Gecko's memory-pressure GC reaches workers.
    pub fn gc(&mut self) {
        unsafe { mrt_js_gc(self.js) }
        self.host.workers.gc(false);
    }

    /// The page going idle, as Firefox's content process does once the user is inactive: a
    /// shrinking GC (the nursery to its minimum, script sources compressed on the helper threads).
    /// Returns the milliseconds spent waiting for the compressions.
    /// The page's dedicated workers go idle with it (W5); the longest wait is returned.
    pub fn idle_gc(&mut self) -> usize {
        let page = unsafe { mrt_js_idle_gc(self.js) };
        page.max(self.host.workers.gc(true))
    }

    /// One shrinking GC of the page and its dedicated workers, for a page in use that has been idle
    /// a while (realsite --interactive, GAPS A22): `idle_gc` without its wait for the source
    /// compressions.
    pub fn shrink_gc(&mut self) {
        unsafe { mrt_js_shrink_gc(self.js) }
        self.host.workers.shrink_gc();
    }

    /// Each dedicated worker's JS runtime (W5): `{"config": {...}, "js": {...}}` with the same fields as
    /// `memory_detail`.
    pub fn workers_memory(&mut self) -> Vec<String> {
        self.host.workers.memory()
    }

    pub fn memory(&self) -> JsMemory {
        let mut m = JsMemory::default();
        unsafe { mrt_js_memory(self.js, &mut m) };
        m
    }

    /// The whole JS runtime's memory as a JSON object in bytes (`mrt_js_memory_detail`): malloc by
    /// owner, the GC chunks, the nursery and JIT code, and the page's realm and zone by category.
    pub fn memory_detail(&self) -> String {
        let mut buf = vec![0u8; 8192];
        let n = unsafe { mrt_js_memory_detail(self.js, buf.as_mut_ptr() as *mut c_char, buf.len()) };
        if n >= buf.len() {
            buf = vec![0u8; n + 1];
            unsafe { mrt_js_memory_detail(self.js, buf.as_mut_ptr() as *mut c_char, buf.len()) };
        }
        buf.truncate(n);
        String::from_utf8(buf).unwrap()
    }

    /// A frame's global, placed by Gecko's zone and compartment rule for an in-process frame of
    /// this page (`SelectZone`). Frames are not run yet (stage 6 phase 3); this exercises the rule.
    /// Returns the frame's index.
    pub fn new_frame_global(&mut self, frame: &GlobalOptions) -> Option<i32> {
        let raw = frame.raw();
        let i = unsafe { mrt_js_new_frame_global(self.js, &raw) };
        (i >= 0).then_some(i)
    }

    pub fn frame_global_relation(&self, i: i32) -> FrameGlobalRelation {
        let r = unsafe { mrt_js_frame_global_relation(self.js, i) };
        assert!(r >= 0, "no frame global {i}");
        FrameGlobalRelation {
            same_zone_as_page: r & 1 != 0,
            same_compartment_as_page: r & 2 != 0,
            secure_context: r & 4 != 0,
            same_compartment_as_earlier_frame: r & 8 != 0,
        }
    }

    /// Parse the page's document on with `parser` until it pops a `<script>`, which bridge.js then runs
    /// (or keeps, a deferred one): false once the document is parsed. The changes the parser makes
    /// are notified as a script's are (host.rs's ParserNotifier); what they start (custom element
    /// reactions, frames' windows, sheet loads) runs when the parser stops, before the script.
    pub fn parse_step(&mut self, parser: &mut mrt_style::mrt_dom::DocumentParser) -> bool {
        let mut notifier = host::ParserNotifier { host: &mut *self.host as *mut Host };
        let dom: *mut Dom = &mut self.host.dom;
        let script = parser.resume(unsafe { &mut *dom }, Some(&mut notifier));
        unsafe { mrt_js_parser_stopped() };
        match script {
            Some(script) => {
                unsafe { mrt_js_parser_script(script) };
                true
            }
            None => false,
        }
    }

    pub fn dom(&self) -> &Dom {
        &self.host.dom
    }

    /// Move the DOM out to style and lay it out. Script must not run until `put_dom` gives it back.
    pub fn take_dom(&mut self) -> Dom {
        std::mem::take(&mut self.host.dom)
    }

    /// Give the DOM back (after hibernation, or a restore from bytes: node ids are unchanged, so the
    /// reflectors still point at the same nodes).
    pub fn put_dom(&mut self, dom: Dom) {
        self.host.dom = dom;
        self.host.drop_layout_cache();
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        unsafe { mrt_js_free(self.js) }
    }
}

/// The HTML `<script>` elements of the document, in tree order.
fn script_elements(dom: &Dom) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![DOCUMENT];
    while let Some(id) = stack.pop() {
        let n = dom.node(id);
        if n.kind == Kind::Element && n.ns == NS_HTML && dom.tag(id) == "script" {
            out.push(id);
        }
        let first = stack.len();
        stack.extend(dom.children(id));
        stack[first..].reverse();
    }
    out
}

/// `s` as a JS string literal.
pub fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
