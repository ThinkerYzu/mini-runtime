//! The DOM side of the bindings: the `mrt_dom_*` functions the C++ layer calls (glue/mrt_js.h).
//! Every pointer they get or give is valid only for the call (strings out live in `scratch`).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, Mutex};
use std::ffi::{c_char, c_void};

use crate::flushrec::FrameOp;
use mrt_layout::driver::{BoundingClientRectInCSSPixels, ClientAreaAndScrollSize, ConstructFramesTrackingMode, DestroyFrameTree, IntersectionMargin, PresShellFrames, ReflowFrames, ResolveIntersectionMargin};
use mrt_layout::{nsPoint, nsRect, nsSize};
use mrt_style::mrt_dom::{Dom, Kind, NodeId, NONE};
use mrt_style::restyle::{RestyleOp, RestyleRecorder};
use mrt_style::{element_state_bits, match_selector, query_selector, SheetCache, StyleDom};

/// The Rust half of a page: the DOM and what the callbacks need between calls.
pub struct Host {
    pub dom: Dom,
    /// The string handed out last.
    scratch: String,
    /// The result of the last query.
    results: Vec<NodeId>,
    pub console: Vec<String>,
    pub echo_console: bool,
    /// Bumped by every change of the DOM; a document's boxes are for one generation.
    generation: u64,
    pub flushes: usize,
    /// Layout reads answered with the boxes of an older DOM because `max_flushes` was used up.
    pub stale_reads: usize,
    /// Flushes allowed before layout reads get the last boxes (each flush lays out the whole page,
    /// GAPS B9; a real site reads layout after nearly every change).
    pub max_flushes: usize,
    /// Flushes whose layout panicked (a feature that is not ported, GAPS C); they answer no boxes.
    pub layout_panics: usize,
    /// Layout threads started: one per document laid out (the page's and each frame's), and another
    /// after a panic.
    pub layout_threads: usize,
    /// The sheet of each stylesheet `<link>` whose load succeeded, by element: an entry of
    /// `LINKED_SHEETS`. Node-id state outside the arena (DESIGN, "Page state outside the arena").
    pub link_sheets: HashMap<NodeId, Arc<str>>,
    /// The parsed rules of the sheets a script changed through the CSSOM (insertRule, deleteRule), by
    /// sheet (`SheetKey`): Gecko keeps a StyleSheet's parsed rules, and so insertRule parses one rule.
    /// Computed from the page state (the owner's text or linked sheet, the rules in the arena, a
    /// constructed sheet's text), so dropped with the kept style and parsed again when read.
    cssom: HashMap<SheetKey, mrt_style::cssom::CssomSheet>,
    /// For each `<style>` or `<link>` whose sheet bridge.js made an object for, how many times the
    /// element got a new sheet since (`sheet_replaced`): its CSSStyleSheet object is made again then,
    /// as Gecko's `sheet` is a new object.
    sheet_generations: HashMap<NodeId, u32>,
    /// The layout of each document with a window, by document: the page's (`DOCUMENT`), and one per
    /// frame window's document (DESIGN, "The frames' layout (S5)"), as Gecko has a PresShell per
    /// document. Computed results, not page state.
    shells: BTreeMap<NodeId, PresShell>,
    /// The page document's root scroll offset in app units: the bridge tab's (realsite --interactive),
    /// or where the page's scrollTo put it (bridge.js). Frame documents do not scroll.
    root_scroll: (i32, i32),
    /// Each scrolled element scroll container's offset in app units (bridge.js, task 0bec4), and a
    /// number that changes with any of them, for the boxes read at them.
    element_scroll: std::collections::BTreeMap<NodeId, (i32, i32)>,
    element_scroll_generation: u64,
    /// The frame documents whose size changed since the last resize steps (Gecko's
    /// PresShell::mResizeEventPending), in the order the changes were found.
    resizes: Vec<NodeId>,
    /// Stage 6's bridge: requests out on stdout, replies in on stdin (one JSON line each).
    pub bridge: bool,
    /// Bridge requests sent.
    pub bridge_calls: usize,
    /// The page's dedicated workers (worker.rs), each on a thread of its own.
    pub workers: crate::worker::Workers,
    /// What workers handed over and the page has not taken yet, by worker id (worker.rs).
    pub worker_pending: Vec<(usize, Vec<String>)>,
    /// In a worker's own Host: what it hands its parent at the end of the task (`__mrtWorkerOut`).
    pub worker_out: Vec<String>,
}

unsafe extern "C" {
    fn mrt_js_parser_inserted(parent: u32, child: u32);
    fn mrt_js_parser_will_remove(parent: u32, child: u32);
}

/// The document parser's changes to the page's tree (mrt-dom's ParserObserver), told to the page as
/// the DOM funnels below tell it of a script's (`mrt_dom_insert`, `mrt_dom_remove`, the text and
/// attribute setters): the sheets, the restyle log, the frame ops and the generation here, the
/// mutation observers and what waits for the parser's stop in the C++ layer. Gecko's tree operations
/// notify the same way (nsHtml5TreeOperation::Append and the others).
pub(crate) struct ParserNotifier {
    pub(crate) host: *mut Host,
}

impl mrt_style::mrt_dom::ParserObserver for ParserNotifier {
    fn inserted(&mut self, node: NodeId) {
        let h = unsafe { &mut *self.host };
        let parent = h.dom.node(node).parent;
        h.sheets_moved(parent, node);
        h.restyle(node, |r, d| r.content_inserted(d, node));
        h.frame_op(node, FrameOp::Inserted(node));
        h.changed();
        unsafe { mrt_js_parser_inserted(parent, node) };
    }

    fn will_remove(&mut self, node: NodeId) {
        let h = unsafe { &mut *self.host };
        let parent = h.dom.node(node).parent;
        unsafe { mrt_js_parser_will_remove(parent, node) };
        h.restyle(node, |r, d| r.content_will_be_removed(d, node));
        h.frame_will_be_removed(node);
        h.sheets_moved(parent, node);
        h.changed();
    }

    fn text_changed(&mut self, node: NodeId) {
        let h = unsafe { &mut *self.host };
        let parent = h.dom.node(node).parent;
        if parent != NONE && h.dom.node(parent).kind == Kind::Element && h.dom.tag(parent) == "style" {
            h.sheet_replaced(parent);
        }
        h.restyle(node, |r, d| r.character_data_changed(d, node));
        h.frame_op(node, FrameOp::TextChanged(node));
        h.changed();
    }

    fn attributes_added(&mut self, node: NodeId, names: &[String]) {
        // (after the change, so the restyle log has no snapshot of the element before it: everything
        // is restyled; only a second <html> or <body> tag does this)
        let h = unsafe { &mut *self.host };
        h.restyle(node, |r, _| r.everything());
        for name in names {
            h.sheet_attribute_changed(node, name);
            h.frame_op(node, FrameOp::AttrChanged(node, name.clone()));
        }
        h.changed();
    }
}

/// A style sheet the CSSOM reaches: a `<style>` or `<link>` element's (by element), or a constructed
/// one (by sheet id).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum SheetKey {
    Owner(NodeId),
    Constructed(u32),
}

/// Gecko's initial size of a frame's document before the parent's layout gives it one: a frame laid
/// out at another size gets a `resize` at the next refresh tick (measured in release Firefox, bridge
/// case frame-layout).
const INITIAL_FRAME_SIZE: (i32, i32) = (300, 150);

/// A document's layout, Gecko's PresShell: its style data and frames on a layout thread of its own
/// (mrt-style and mrt-layout keep one document per thread), its viewport, and what the DOM's changes
/// since its last flush invalidate.
struct PresShell {
    /// The frame element whose document this is: NONE for the page, and for a root window (its
    /// embedder is in another runtime, and its size is the one it was opened with).
    frame: NodeId,
    /// Its viewport in CSS px. A frame's comes from its parent's last flush (the frame element's
    /// content box, nsSubDocumentFrame); None while the frame element has no frame (display: none),
    /// when Gecko's document has no pres shell: no boxes, and `innerWidth` 0.
    viewport: Option<(i32, i32)>,
    /// The size a frame's document has for its `resize` events: INITIAL_FRAME_SIZE when its window
    /// opens, None after its frame element had no frame (its next size comes with a new pres shell,
    /// without an event).
    resize_size: Option<(i32, i32)>,
    /// What the DOM's changes since the last flush invalidate in its styles (Gecko's RestyleManager,
    /// mrt_style::restyle); the next flush hands the ops to the layout thread.
    restyle: RestyleRecorder,
    /// What the DOM's changes since the last flush do to the frames (FrameOp); the next flush hands them
    /// to the layout thread with the restyle ops.
    frame_ops: Vec<FrameOp>,
    /// The thread that styles and lays the document out, keeping its style data (and the parsed sheets,
    /// GAPS B41) from one flush to the next: computed results, not page state, dropped by
    /// `drop_kept_style` but for the parsed sheets (SPEC decision 22). Started by the first flush; a
    /// flush whose layout panicked ends it.
    layout_thread: Option<LayoutThread>,
    /// The parsed sheets kept through hibernation (SPEC decision 22), for the next layout thread.
    kept_sheets: Option<SheetCache>,
    /// Per element with a frame: its bounding client rect (x, y, width, height), then its client area and
    /// scroll size (clientWidth, clientHeight, scrollWidth, scrollHeight), in CSS px; for one DOM
    /// generation and viewport.
    boxes: Option<(u64, Option<(i32, i32)>, HashMap<NodeId, [f64; 8]>)>,
    /// The root scroll offset the boxes were read at (app units): they are in client coordinates.
    boxes_scroll: (i32, i32),
    /// The element scroll generation the boxes were read at.
    boxes_element_scroll: u64,
    /// The size each frame element of the document gives its own document, from the last flush.
    subdocument_sizes: HashMap<NodeId, (i32, i32)>,
}

impl PresShell {
    fn new(frame: NodeId, viewport: Option<(i32, i32)>) -> PresShell {
        PresShell {
            frame,
            viewport,
            resize_size: Some(INITIAL_FRAME_SIZE),
            restyle: RestyleRecorder::default(),
            frame_ops: Vec::new(),
            layout_thread: None,
            kept_sheets: None,
            boxes: None,
            boxes_scroll: (0, 0),
            boxes_element_scroll: 0,
            subdocument_sizes: HashMap::new(),
        }
    }
}

impl Host {
    pub fn new(dom: Dom) -> Host {
        let mut shells = BTreeMap::new();
        shells.insert(mrt_style::mrt_dom::DOCUMENT, PresShell::new(NONE, Some(crate::viewport())));
        Host {
            dom,
            scratch: String::new(),
            results: Vec::new(),
            console: Vec::new(),
            echo_console: false,
            generation: 0,
            flushes: 0,
            stale_reads: 0,
            max_flushes: usize::MAX,
            layout_panics: 0,
            layout_threads: 0,
            link_sheets: HashMap::new(),
            cssom: HashMap::new(),
            sheet_generations: HashMap::new(),
            shells,
            root_scroll: (0, 0),
            element_scroll: Default::default(),
            element_scroll_generation: 0,
            resizes: Vec::new(),
            bridge: false,
            bridge_calls: 0,
            workers: Default::default(),
            worker_pending: Vec::new(),
            worker_out: Vec::new(),
        }
    }

    pub(crate) fn give_string(&mut self, s: String, out: *mut *const c_char, len: *mut usize) {
        self.give(s, out, len)
    }

    fn give(&mut self, s: String, out: *mut *const c_char, len: *mut usize) {
        self.scratch = s;
        unsafe {
            *out = self.scratch.as_ptr() as *const c_char;
            *len = self.scratch.len();
        }
    }

    /// Drop the style data the layout threads keep (see `layout_thread`) but for the parsed sheets
    /// (`KeepStyle`); the next flush of each document styles everything.
    pub fn drop_kept_style(&mut self) {
        self.cssom.clear();
        for shell in self.shells.values_mut() {
            if let Some(thread) = &shell.layout_thread {
                // (the frames read the DOM while they are destroyed: it goes to the thread and back)
                self.dom = thread.drop_style(std::mem::take(&mut self.dom));
            }
            shell.restyle.reset(HashMap::new(), Default::default());
            shell.frame_ops.clear();
        }
    }

    /// Hibernate every document's layout (DESIGN, "Hibernation of a frame (S6)"): the page's and each
    /// frame document's style data, frames and boxes dropped and its layout thread ended, as Gecko's
    /// hidden document viewer destroys its PresShell and, through nsSubDocumentFrame::Destroy's
    /// nsHideViewer, those of the frames in the same process. Each document keeps its size (the
    /// docshell's) and the size it had for `resize` events, so the wake fires none, and its parsed
    /// sheets (SPEC decision 22), so the wake does not parse them again.
    pub fn hibernate(&mut self) {
        self.cssom.clear();
        for shell in self.shells.values_mut() {
            // (MRT_KEEP_STYLE=rules keeps the thread, with its rule tables)
            if keep_style() == KeepStyle::Rules {
                if let Some(thread) = &shell.layout_thread {
                    self.dom = thread.drop_style(std::mem::take(&mut self.dom));
                }
            } else if let Some(thread) = shell.layout_thread.take() {
                self.dom = thread.drop_style(std::mem::take(&mut self.dom));
                shell.kept_sheets = thread.join();
            }
            shell.restyle.reset(HashMap::new(), Default::default());
            shell.frame_ops.clear();
            shell.boxes = None;
        }
    }

    /// The documents whose layout thread is running now (hibernate stops them all).
    pub fn layouts_live(&self) -> usize {
        self.shells.values().filter(|shell| shell.layout_thread.is_some()).count()
    }

    /// Every document's layout brought up to the DOM (each flushed after its parent), and each one's
    /// boxes: the documents with a layout, by document id.
    pub fn flush_all(&mut self) -> BTreeMap<NodeId, HashMap<NodeId, [f64; 8]>> {
        let docs: Vec<NodeId> = self.shells.keys().copied().collect();
        let mut all = BTreeMap::new();
        for doc in docs {
            if self.flush_document(doc) {
                if let Some(b) = &self.shells[&doc].boxes {
                    all.insert(doc, b.2.clone());
                }
            }
        }
        all
    }

    /// The page document's root scroll offset, in CSS px (bridge.js's scroll state): the next layout
    /// read flushes with it, and reads in client coordinates (PresShellFrames::SetRootScrollPosition).
    pub fn set_root_scroll(&mut self, x: f64, y: f64) {
        // CSSPixel::ToAppUnits: NSToCoordRoundWithClamp
        let au = |v: f64| (v * 60.0 + 0.5).floor().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        self.root_scroll = (au(x), au(y));
    }

    /// An element scroll container's scroll offset, in CSS px (bridge.js's element scroll state): the
    /// next layout read flushes with it (PresShellFrames::ApplyElementScroll); (0, 0) forgets it.
    pub fn set_element_scroll(&mut self, id: NodeId, x: f64, y: f64) {
        let au = |v: f64| (v * 60.0 + 0.5).floor().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        let offset = (au(x), au(y));
        let old = if offset == (0, 0) { self.element_scroll.remove(&id) } else { self.element_scroll.insert(id, offset) };
        if old.unwrap_or((0, 0)) != offset {
            self.element_scroll_generation += 1;
        }
    }

    /// Forget the boxes of the last flushes.
    pub fn drop_layout_cache(&mut self) {
        for shell in self.shells.values_mut() {
            shell.boxes = None;
        }
    }

    /// A frame window's document `doc` was made (the window of frame element `frame`, or a root window
    /// when `frame` is NONE, laid out at `size`): it gets its layout.
    pub fn window_opened(&mut self, doc: NodeId, frame: NodeId, size: (i32, i32)) {
        let viewport = if frame == NONE { Some(size) } else { None };
        let mut shell = PresShell::new(frame, viewport);
        // a frame navigated to another origin gets a new window (its old one closes after): the
        // docshell, and so the size its document had, are the frame's
        if let Some(old) = self.shells.values().find(|s| frame != NONE && s.frame == frame) {
            shell.resize_size = old.resize_size;
        }
        self.shells.insert(doc, shell);
    }

    /// A frame's window got a new document (a `src` or `srcdoc` loaded into it): the old document's
    /// layout goes, the size its window had stays (Gecko makes the new document's PresShell at the
    /// docshell's size).
    pub fn window_document_changed(&mut self, old: NodeId, doc: NodeId) {
        let Some(shell) = self.shells.get(&old) else { return };
        let (frame, resize_size, viewport) = (shell.frame, shell.resize_size, if shell.frame == NONE { shell.viewport } else { None });
        self.window_closed(old);
        let mut shell = PresShell::new(frame, viewport);
        shell.resize_size = resize_size;
        self.shells.insert(doc, shell);
    }

    /// A frame window closed: its document's layout goes (Gecko destroys its PresShell).
    pub fn window_closed(&mut self, doc: NodeId) {
        if doc == mrt_style::mrt_dom::DOCUMENT {
            return;
        }
        if let Some(mut shell) = self.shells.remove(&doc) {
            if let Some(thread) = shell.layout_thread.take() {
                self.dom = thread.drop_style(std::mem::take(&mut self.dom));
                thread.join();
            }
        }
        self.resizes.retain(|&d| d != doc);
    }

    /// The document whose layout a change of `node` goes to: the one whose tree has it (a node outside
    /// every document's tree changes no layout). With no frame documents, always the page's, as before
    /// frames were laid out.
    fn shell_document(&self, node: NodeId) -> Option<NodeId> {
        if self.shells.len() == 1 {
            return Some(mrt_style::mrt_dom::DOCUMENT);
        }
        let root = self.dom.shadow_including_root(node);
        self.shells.contains_key(&root).then_some(root)
    }

    /// An attribute of element `id` changed: the attributes that make a `<style>` a new sheet
    /// (HTMLStyleElement::AfterSetAttr: `title`, `media`, `type`) or a link load again
    /// (HTMLLinkElement::AfterSetAttr; bridge.js loads it, `mrt_dom_link_sheet`).
    fn sheet_attribute_changed(&mut self, id: NodeId, name: &str) {
        let replaces = match self.dom.tag(id) {
            "style" => matches!(name, "title" | "media" | "type"),
            "link" => matches!(name, "href" | "rel" | "title" | "media" | "type" | "disabled" | "crossorigin"),
            _ => false,
        };
        if replaces && self.dom.node(id).ns == mrt_style::mrt_dom::NS_HTML {
            self.sheet_replaced(id);
        }
    }

    /// The CSSOM's parsed rules of `key`'s sheet, parsed now from the page state if they are not kept.
    fn cssom_sheet(&mut self, key: SheetKey) -> &mut mrt_style::cssom::CssomSheet {
        let dom = &self.dom;
        let link_sheets = &self.link_sheets;
        self.cssom.entry(key).or_insert_with(|| {
            use mrt_style::cssom::CssomSheet;
            match key {
                SheetKey::Constructed(id) => {
                    let sheet = &dom.constructed_sheets[id as usize];
                    CssomSheet::parse(&sheet.text, &sheet.ops, dom.mode, true)
                }
                SheetKey::Owner(owner) => {
                    let state = dom.owner_sheet(owner);
                    let ops = state.map_or(&[][..], |s| &s.ops[..]);
                    // (the text as mrt_style's owner_sheet reads it)
                    match state.and_then(|s| s.base.as_deref()) {
                        Some(base) => CssomSheet::parse(base, ops, dom.mode, false),
                        None if dom.tag(owner) == "link" => CssomSheet::parse(link_sheets.get(&owner).map_or("", |s| s), ops, dom.mode, false),
                        None => {
                            let text: String = dom.children(owner).filter(|&c| dom.node(c).kind == Kind::Text).map(|c| dom.text_of(c)).collect();
                            CssomSheet::parse(&text, ops, dom.mode, false)
                        }
                    }
                }
            }
        })
    }

    /// `owner` got a new sheet, as Gecko's LinkStyle::UpdateStyleSheetInternal makes one when a
    /// `<style>`'s text, `media` or `type` changes, when the element enters or leaves a document,
    /// and when a link loads again: what script did to the old sheet (its rules, `disabled`) goes,
    /// and bridge.js's next `sheet` is a new object.
    fn sheet_replaced(&mut self, owner: NodeId) {
        self.dom.drop_owner_sheet(owner);
        self.cssom.remove(&SheetKey::Owner(owner));
        if let Some(g) = self.sheet_generations.get_mut(&owner) {
            *g += 1;
        }
    }

    /// A child list change under `parent` that moves `child`'s subtree into or out of a tree: the
    /// sheet of a `<style>` parent (its text) and of every sheet owner in the subtree is made again.
    /// Only owners whose sheet script reached are looked for (the others have no state to drop).
    fn sheets_moved(&mut self, parent: NodeId, child: NodeId) {
        if parent != NONE && self.dom.node(parent).kind == Kind::Element && self.dom.tag(parent) == "style" {
            self.sheet_replaced(parent);
        }
        if self.sheet_generations.is_empty() && self.dom.owner_sheets.is_empty() {
            return;
        }
        let inside = |dom: &Dom, mut n: NodeId| {
            while n != NONE {
                if n == child {
                    return true;
                }
                n = dom.node(n).parent;
            }
            false
        };
        let mut owners: Vec<NodeId> = self.sheet_generations.keys().chain(self.dom.owner_sheets.keys()).copied().filter(|&o| inside(&self.dom, o)).collect();
        owners.sort();
        owners.dedup();
        for owner in owners {
            self.sheet_replaced(owner);
        }
    }

    /// Tell the restyle log of `node`'s document about a change (`record` gets it and the DOM).
    fn restyle(&mut self, node: NodeId, record: impl FnOnce(&mut RestyleRecorder, &Dom)) {
        if let Some(doc) = self.shell_document(node) {
            record(&mut self.shells.get_mut(&doc).unwrap().restyle, &self.dom);
        }
    }

    /// Every change of the DOM goes through here: the boxes are out of date. (The restyle recorder
    /// was told what changed, as Gecko's PresShell gets its notifications.)
    fn changed(&mut self) {
        // a log no flush will read, once the budget is used up, is kept as one op
        if self.flushes >= self.max_flushes {
            for shell in self.shells.values_mut() {
                shell.restyle.everything();
                shell.frame_ops.clear();
            }
        }
        self.generation += 1;
    }

    /// A connected node is about to leave its parent: what the frames need to know of the DOM then
    /// (mrt_layout::RemovedContent).
    fn frame_will_be_removed(&mut self, child: NodeId) {
        let n = self.dom.node(child);
        if n.parent == NONE {
            return;
        }
        let Some(doc) = self.shell_document(child) else { return };
        if !self.dom.is_connected_to(child, doc) {
            return;
        }
        let (prev, next) = (n.prev_sibling, n.next_sibling);
        let removed = FrameOp::Removed(mrt_layout::RemovedContent {
            mChild: child,
            mContainer: n.parent,
            mPrevSibling: prev,
            mNextSibling: next,
            mPrevSiblingHasPrevSibling: prev != NONE && self.dom.node(prev).prev_sibling != NONE,
            mNextSiblingHasNextSibling: next != NONE && self.dom.node(next).next_sibling != NONE,
        });
        self.shells.get_mut(&doc).unwrap().frame_ops.push(removed);
    }

    /// A node was inserted, or another frame op, if the node is connected (the frames of a detached
    /// subtree are built when it is inserted).
    fn frame_op(&mut self, node: NodeId, op: FrameOp) {
        if let Some(doc) = self.shell_document(node) {
            if self.dom.is_connected_to(node, doc) {
                self.shells.get_mut(&doc).unwrap().frame_ops.push(op);
            }
        }
    }

    /// The document whose layout answers a layout read of `node`: the one whose tree has it.
    fn layout_document(&self, node: NodeId) -> Option<NodeId> {
        let root = self.dom.shadow_including_root(node);
        self.shells.contains_key(&root).then_some(root)
    }

    /// The layout of document `doc` brought up to the DOM, as Gecko's FlushPendingNotifications(Layout):
    /// a frame's parent document first (Document::FlushPendingNotifications flushes it, so the frame's
    /// size is current), then the document if it has a viewport. Whether it has boxes.
    fn flush_document(&mut self, doc: NodeId) -> bool {
        let Some(shell) = self.shells.get(&doc) else { return false };
        let frame = shell.frame;
        if frame != NONE {
            let parent = self.dom.shadow_including_root(frame);
            if parent != doc && self.shells.contains_key(&parent) {
                self.flush_document(parent);
            } else {
                // a frame element out of every laid-out document: no frame, no pres shell
                self.set_frame_size(doc, None);
            }
        }
        let shell = self.shells.get(&doc).unwrap();
        let Some(viewport) = shell.viewport else {
            return false;
        };
        let scroll = if doc == mrt_style::mrt_dom::DOCUMENT { self.root_scroll } else { (0, 0) };
        let stale = shell.boxes.as_ref().is_none_or(|b| b.0 != self.generation || b.1 != Some(viewport))
            || shell.boxes_scroll != scroll
            || shell.boxes_element_scroll != self.element_scroll_generation;
        if stale && shell.boxes.is_some() && self.flushes >= self.max_flushes {
            self.stale_reads += 1;
        } else if stale {
            self.flushes += 1;
            let shell = self.shells.get_mut(&doc).unwrap();
            // the DOM goes to the layout thread for the flush and comes back with the boxes (nothing
            // else runs meanwhile); the frames read it while they are built and reflowed
            let request = LayoutRequest {
                doc,
                viewport,
                dom: std::mem::take(&mut self.dom),
                link_sheets: self.link_sheets.clone(),
                ops: shell.restyle.take_ops(),
                frame_ops: std::mem::take(&mut shell.frame_ops),
                flush: self.flushes,
                scroll,
                element_scroll: self.element_scroll.iter().map(|(&n, &o)| (n, o)).collect(),
            };
            if shell.layout_thread.is_none() {
                self.layout_threads += 1;
            }
            let thread = shell.layout_thread.get_or_insert_with(|| LayoutThread::start(shell.kept_sheets.take()));
            let reply = thread.flush(request);
            self.dom = reply.dom;
            let (boxes, subdocuments) = match reply.boxes {
                Some(boxes) => {
                    let (flags, styled) = reply.restyle_state;
                    shell.restyle.reset(flags, styled);
                    (boxes, reply.subdocuments)
                }
                // the layout panicked (a feature that is not ported, GAPS C): the thread has ended,
                // with the frames it had; the next flush starts another
                None => {
                    self.layout_panics += 1;
                    shell.layout_thread.take().unwrap().join();
                    shell.restyle.reset(HashMap::new(), Default::default());
                    (HashMap::new(), HashMap::new())
                }
            };
            shell.boxes = Some((self.generation, Some(viewport), boxes));
            shell.boxes_scroll = scroll;
            shell.boxes_element_scroll = self.element_scroll_generation;
            shell.subdocument_sizes = subdocuments;
            self.update_frame_sizes(doc);
        }
        true
    }

    /// After a flush of `parent`: each frame document whose frame element is in it gets the size the
    /// flush gave it (nsSubDocumentFrame::Reflow's SetPositionAndSize).
    fn update_frame_sizes(&mut self, parent: NodeId) {
        let frames: Vec<(NodeId, NodeId)> = self.shells.iter().filter(|(_, s)| s.frame != NONE).map(|(&d, s)| (d, s.frame)).collect();
        for (doc, frame) in frames {
            if doc != parent && self.dom.shadow_including_root(frame) == parent {
                let size = self.shells[&parent].subdocument_sizes.get(&frame).copied();
                self.set_frame_size(doc, size);
            }
        }
    }

    /// Frame document `doc` gets `size` from its frame element (None: the element has no frame). A
    /// size other than the one it had queues a `resize` for the next refresh tick, as
    /// PresShell::ResizeReflow does for a document that has had its initial reflow.
    fn set_frame_size(&mut self, doc: NodeId, size: Option<(i32, i32)>) {
        let shell = self.shells.get_mut(&doc).unwrap();
        shell.viewport = size;
        match (shell.resize_size, size) {
            (_, None) => shell.resize_size = None,
            (None, Some(s)) => shell.resize_size = Some(s),
            (Some(old), Some(s)) if old != s => {
                shell.resize_size = Some(s);
                if !self.resizes.contains(&doc) {
                    self.resizes.push(doc);
                }
            }
            _ => {}
        }
    }

    /// The boxes of `node` (see PresShell::boxes): its document's layout flushed first. None when it
    /// has no frame, or its document has no layout.
    fn layout_box(&mut self, node: NodeId) -> Option<[f64; 8]> {
        let doc = self.layout_document(node)?;
        if !self.flush_document(doc) {
            return None;
        }
        self.shells[&doc].boxes.as_ref().and_then(|b| b.2.get(&node).copied())
    }

    /// The attached viewer resized the top-level viewport. The next flush updates
    /// media queries and frames; resize delivery uses the existing refresh path.
    pub fn resize_page(&mut self, width: i32, height: i32) {
        crate::set_viewport(width, height);
        self.set_frame_size(mrt_style::mrt_dom::DOCUMENT, Some((width, height)));
    }

    /// The viewport of document `doc` in CSS px, its layout flushed (a frame's size depends on its
    /// parent's layout): what `innerWidth` and `innerHeight` give (nsGlobalWindowOuter::GetInnerSize).
    /// None when it has no pres shell (a frame element without a frame, or no layout at all). For a
    /// frame element, the size it gives its document (its content box), its document flushed.
    pub fn viewport_of(&mut self, doc: NodeId) -> Option<(i32, i32)> {
        if self.dom.node(doc).kind == Kind::Element {
            let parent = self.layout_document(doc)?;
            self.flush_document(parent);
            return self.shells[&parent].subdocument_sizes.get(&doc).copied();
        }
        if doc == mrt_style::mrt_dom::DOCUMENT {
            return Some(crate::viewport());
        }
        self.shells.get(&doc)?;
        let frame = self.shells[&doc].frame;
        if frame != NONE {
            let parent = self.dom.shadow_including_root(frame);
            if parent != doc && self.shells.contains_key(&parent) {
                self.flush_document(parent);
            } else {
                self.set_frame_size(doc, None);
            }
        }
        self.shells[&doc].viewport
    }

    /// The frame documents whose size changed since the last call (the resize steps of a refresh tick).
    pub fn take_resizes(&mut self) -> Vec<NodeId> {
        std::mem::take(&mut self.resizes)
    }

    /// Whether a resize is waiting for the resize steps.
    pub fn resizes_pending(&self) -> bool {
        !self.resizes.is_empty()
    }

    /// The DOM's generation (see `generation`).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The layout phase of a refresh tick for the frames: every document that has a frame document of
    /// this runtime flushed, so that their sizes are current (Gecko's refresh driver flushes layout on
    /// each tick; here only the parents of frames are, for their `resize` events).
    pub fn flush_frame_parents(&mut self) {
        let parents: std::collections::BTreeSet<NodeId> = self
            .shells
            .values()
            .filter(|s| s.frame != NONE)
            .map(|s| self.dom.shadow_including_root(s.frame))
            .filter(|p| self.shells.contains_key(p))
            .collect();
        for parent in parents {
            self.flush_document(parent);
        }
    }

    /// `getComputedStyle(node, pseudo).getPropertyValue(name)`: style and layout flushed (as a layout
    /// read), then the value from its document's layout thread (mrt_layout::GetComputedPropertyValue).
    /// "" when the last flush's layout panicked, or the node's document has no layout.
    fn computed_value(&mut self, node: NodeId, pseudo: Option<mrt_style::style::selector_parser::PseudoElement>, name: &str) -> String {
        let doc = self.layout_document(node).unwrap_or(mrt_style::mrt_dom::DOCUMENT);
        if !self.flush_document(doc) {
            return String::new();
        }
        let Some(thread) = &self.shells[&doc].layout_thread else {
            return String::new();
        };
        let (dom, value) = thread.computed_value(std::mem::take(&mut self.dom), node, pseudo, name);
        self.dom = dom;
        value
    }

    /// What a ResizeObserver reads of each of `nodes` (PresShellFrames::ResizeObserverBoxes), in the
    /// order asked, from each node's document's layout thread: with `flush`, the document flushed
    /// once first; without, its frames as the last flush left them, whatever the DOM did since (the
    /// thread reads only the frames and their style, which a later flush has not touched yet). Zeros
    /// for a node without a frame or whose document has no layout.
    pub fn resize_observer_boxes(&mut self, nodes: &[NodeId], flush: bool) -> Vec<[f64; 11]> {
        let mut out = vec![[0.0; 11]; nodes.len()];
        // by document: each is flushed once, then asked for its nodes
        let mut by_doc: std::collections::BTreeMap<NodeId, Vec<usize>> = Default::default();
        for (i, &node) in nodes.iter().enumerate() {
            if let Some(doc) = self.layout_document(node) {
                by_doc.entry(doc).or_default().push(i);
            }
        }
        for (doc, indices) in by_doc {
            if flush && !self.flush_document(doc) {
                continue;
            }
            let Some(thread) = &self.shells[&doc].layout_thread else { continue };
            let asked: Vec<NodeId> = indices.iter().map(|&i| nodes[i]).collect();
            let (dom, boxes) = thread.resize_observer_boxes(std::mem::take(&mut self.dom), asked);
            self.dom = dom;
            for (&i, b) in indices.iter().zip(boxes) {
                out[i] = b;
            }
        }
        out
    }

    /// `elementFromPoint` (`multiple` false) and `elementsFromPoint` of `root`, a document or a shadow
    /// root, at (x, y) in CSS px: the elements under the point, the topmost first.
    ///
    /// Gecko: dom/base/DocumentOrShadowRoot.cpp QueryNodesFromPoint and QueryNodesFromRect: none for
    /// a negative coordinate; the owner document's layout flushed; the frames' nodes
    /// (PresShellFrames::NodesFromPoint), a Text node answered by its parent (by the host when that is
    /// a shadow root) for elementFromPoint and skipped for elementsFromPoint; each retargeted against
    /// `root` (DocumentOrShadowRoot::Retarget: up through the shadow hosts until the node is in
    /// `root`'s tree; a node never in it is skipped); a node equal to the one before dropped.
    /// ADAPTER: no SVG text frames (QueryNodesFromRect's exception for them).
    pub fn nodes_from_point(&mut self, root: NodeId, x: f32, y: f32, multiple: bool) -> Vec<NodeId> {
        let mut out = Vec::new();
        // As per the spec, we return null if either coord is negative.
        if x < 0.0 || y < 0.0 {
            return out;
        }
        // CSSPixelsToAppUnits: NSToCoordRoundWithClamp
        let au = |v: f32| (v * 60.0 + 0.5).floor().min(i32::MAX as f32) as i32;
        let point = (au(x), au(y));
        let doc = self.dom.shadow_including_root(root);
        if !self.shells.contains_key(&doc) || !self.flush_document(doc) {
            return out;
        }
        let Some((w, h)) = self.viewport_of(doc) else { return out };
        let Some(thread) = self.shells.get(&doc).and_then(|s| s.layout_thread.as_ref()) else { return out };
        let (dom, hits) = thread.nodes_from_point(std::mem::take(&mut self.dom), point, (w * 60, h * 60));
        self.dom = dom;
        for node in hits {
            let mut node = node;
            if self.dom.node(node).kind != Kind::Element {
                // If this helper is called via ElementsFromPoint, we need to make sure
                // our frame is an element. Otherwise return whatever the top frame is
                // even if it isn't the top-painted element.
                if multiple {
                    continue;
                }
                node = self.dom.node(node).parent;
                if node != NONE && self.dom.node(node).kind == Kind::ShadowRoot {
                    node = self.dom.host(node);
                }
            }
            // DocumentOrShadowRoot::Retarget
            let mut cur = node;
            while cur != NONE && self.dom.tree_root(cur) != root {
                let shadow = self.dom.containing_shadow(cur);
                cur = if shadow == NONE { NONE } else { self.dom.host(shadow) };
            }
            if cur != NONE && out.last() != Some(&cur) {
                out.push(cur);
                if !multiple {
                    break;
                }
            }
        }
        out
    }

    /// `innerText` of element `node`: its rendered text from its composed document's frames, or its
    /// textContent when it is not rendered or that document has no layout.
    ///
    /// Gecko: dom/html/nsGenericHTMLElement.cpp nsGenericHTMLElement::GetInnerText: style flushed,
    /// layout flushed when a frame is dirty, then `IsRendered()` chooses between
    /// nsRange::GetInnerTextNoFlush (PresShellFrames::InnerText) and GetTextContentInternal.
    /// ADAPTER: the flush is the runtime's whole-document flush (flush_document), which does nothing
    /// when the DOM has not changed since the last one; Gecko reflows only when this element's frames
    /// or their ancestors are dirty, so a read after a change elsewhere flushes here and not there
    /// (GAPS B116).
    pub fn inner_text(&mut self, node: NodeId) -> String {
        let text = (|| {
            let doc = self.layout_document(node)?;
            if !self.flush_document(doc) {
                return None;
            }
            let thread = self.shells.get(&doc)?.layout_thread.as_ref()?;
            let (dom, text) = thread.inner_text(std::mem::take(&mut self.dom), node);
            self.dom = dom;
            text
        })();
        text.unwrap_or_else(|| self.dom.text_content(node))
    }

    /// Asks document `doc`'s frames the questions of an IntersectionObserver's update, after its
    /// layout was flushed: one answer per question, empty when it has no layout.
    fn ask_intersections(&mut self, doc: NodeId, asks: Vec<IntersectionAsk>, scroll_margin: &IntersectionMargin) -> Vec<Vec<i32>> {
        let n = asks.len();
        let Some(thread) = self.shells.get(&doc).and_then(|s| s.layout_thread.as_ref()) else { return vec![Vec::new(); n] };
        let (dom, answers) = thread.intersections(std::mem::take(&mut self.dom), asks, *scroll_margin);
        self.dom = dom;
        answers
    }

    /// The geometry of an IntersectionObserver's update for each of `targets`, as
    /// DOMIntersectionObserver::ComputeInput and Intersect compute it, in app units: 14 numbers each
    /// (whether it was computed, rootBounds x, y, width, height, the target's rectangle, whether it
    /// intersects, the intersection rectangle). `root` is NONE for the implicit root (the viewport of
    /// the top document of `owner`, the observer's document, in this runtime), or an element or a
    /// document. Not computed (0, then zeros) where Intersect returns early: no root frame, no target
    /// frame, a target in another document than an explicit root, or not inside an element root.
    /// Layout is flushed first, as the refresh tick's layout phase does before the update. A target
    /// in a frame document with the implicit root has its intersection clipped by its viewport and
    /// the ancestors of its frame element, document by document (ComputeTheIntersection's walk across
    /// documents), and given back relative to its own viewport (step 6).
    /// ADAPTER: a frame of another origin runs in a runtime of its own, whose top document is its
    /// own; Firefox would clip it by the visible rect its parent process gives it.
    pub fn intersections(&mut self, owner: NodeId, root: NodeId, root_margin: &IntersectionMargin, scroll_margin: &IntersectionMargin, targets: &[NodeId]) -> Vec<[f64; 14]> {
        let mut out = vec![[0.0; 14]; targets.len()];
        let au = |v: i32| v * 60;
        let edge_inclusive = |a: [i32; 4], b: [i32; 4]| -> Option<[i32; 4]> {
            nsRect::new(a[0], a[1], a[2], a[3]).EdgeInclusiveIntersection(nsRect::new(b[0], b[1], b[2], b[3])).map(|r| [r.x, r.y, r.width, r.height])
        };
        // the root's document and rectangle (ComputeInput)
        let implicit = root == NONE;
        let root_doc = if implicit {
            let mut doc = self.layout_document(owner);
            while let Some(d) = doc {
                let frame = self.shells[&d].frame;
                if frame == NONE {
                    break;
                }
                match self.layout_document(frame) {
                    Some(parent) if parent != d => doc = Some(parent),
                    _ => break,
                }
            }
            doc
        } else if self.dom.node(root).kind == Kind::Element {
            self.layout_document(root)
        } else {
            self.shells.contains_key(&root).then_some(root)
        };
        let Some(root_doc) = root_doc else { return out };
        if !self.flush_document(root_doc) {
            return out;
        }
        let root_rect = if !implicit && self.dom.node(root).kind == Kind::Element {
            let answer = self.ask_intersections(root_doc, vec![IntersectionAsk::RootRect(root)], scroll_margin).remove(0);
            if answer.is_empty() {
                return out;
            }
            [answer[0], answer[1], answer[2], answer[3]]
        } else {
            let Some((w, h)) = self.viewport_of(root_doc) else { return out };
            [0, 0, au(w), au(h)]
        };
        let mut bounds = nsRect::new(root_rect[0], root_rect[1], root_rect[2], root_rect[3]);
        bounds.Inflate__nsMargin(ResolveIntersectionMargin(root_margin, nsSize::new(root_rect[2], root_rect[3])));
        if implicit {
            bounds.Inflate__nsMargin(ResolveIntersectionMargin(scroll_margin, nsSize::new(root_rect[2], root_rect[3])));
        }
        let root_bounds = [bounds.x, bounds.y, bounds.width, bounds.height];
        let root_element = if !implicit && self.dom.node(root).kind == Kind::Element { root } else { NONE };
        // the targets, by document (Intersect)
        let mut by_doc: BTreeMap<NodeId, Vec<usize>> = BTreeMap::new();
        for (i, &target) in targets.iter().enumerate() {
            let Some(doc) = self.layout_document(target) else { continue };
            // 2.2: an explicit root's targets are in its document
            if !implicit && doc != root_doc {
                continue;
            }
            by_doc.entry(doc).or_default().push(i);
        }
        for (doc, indices) in by_doc {
            if !self.flush_document(doc) {
                continue;
            }
            let asks = indices.iter().map(|&i| IntersectionAsk::Target(targets[i], if doc == root_doc { root_element } else { NONE })).collect();
            let answers = self.ask_intersections(doc, asks, scroll_margin);
            for (&i, answer) in indices.iter().zip(answers) {
                if answer.is_empty() {
                    continue;
                }
                let target_rect = [answer[0], answer[1], answer[2], answer[3]];
                let mut rect = (answer.len() == 8).then(|| [answer[4], answer[5], answer[6], answer[7]]);
                // up through the frame documents to the root's (implicit root only)
                let (mut d, mut origin) = (doc, (0, 0));
                while d != root_doc {
                    let Some(r) = rect else { break };
                    let frame = self.shells[&d].frame;
                    let parent = if frame == NONE { None } else { self.layout_document(frame) };
                    let (Some(parent), Some((w, h))) = (parent, self.viewport_of(d)) else {
                        rect = None;
                        break;
                    };
                    // the document's viewport (its root scroll frame's port, with the scroll margin)
                    let mut port = nsRect::new(0, 0, au(w), au(h));
                    port.Inflate__nsMargin(ResolveIntersectionMargin(scroll_margin, nsSize::new(au(w), au(h))));
                    rect = edge_inclusive(r, [port.x, port.y, port.width, port.height]);
                    let Some(r) = rect else { break };
                    let mut answers = self.ask_intersections(parent, vec![IntersectionAsk::Origin(frame)], scroll_margin);
                    let o = answers.remove(0);
                    if o.is_empty() {
                        rect = None;
                        break;
                    }
                    origin = (origin.0 + o[0], origin.1 + o[1]);
                    let moved = [r[0] + o[0], r[1] + o[1], r[2], r[3]];
                    let clipped = self.ask_intersections(parent, vec![IntersectionAsk::ClipFrom(frame, moved)], scroll_margin).remove(0);
                    rect = (!clipped.is_empty()).then(|| [clipped[0], clipped[1], clipped[2], clipped[3]]);
                    d = parent;
                }
                // 5: the root bounds; 6: back to the target's document
                let intersection = rect.and_then(|r| edge_inclusive(r, root_bounds)).map(|r| [r[0] - origin.0, r[1] - origin.1, r[2], r[3]]);
                let o = &mut out[i];
                o[0] = 1.0;
                for k in 0..4 {
                    o[1 + k] = root_bounds[k] as f64;
                    o[5 + k] = target_rect[k] as f64;
                }
                if let Some(r) = intersection {
                    o[9] = 1.0;
                    for k in 0..4 {
                        o[10 + k] = r[k] as f64;
                    }
                }
            }
        }
        out
    }

    /// `offsetParent` and the offset rectangle of an element (Element::GetOffsetRect): style and
    /// layout flushed, then the answer from its document's layout thread. None when it has no frame
    /// or its document no layout.
    fn offset_rect(&mut self, node: NodeId) -> Option<(NodeId, [i32; 4])> {
        let doc = self.layout_document(node)?;
        if !self.flush_document(doc) {
            return None;
        }
        let thread = self.shells[&doc].layout_thread.as_ref()?;
        let (dom, offsets) = thread.offset_rect(std::mem::take(&mut self.dom), node);
        self.dom = dom;
        offsets
    }
}

/// A change of the DOM, as the layout thread gets it (DESIGN, "Incremental style and layout"). Node ids
/// stay valid after a removal (GAPS B10), so a change can name a node that is no longer in the tree.
/// What a flush hands the layout thread.
struct LayoutRequest {
    /// The document laid out (Tree::doc) and its viewport.
    doc: NodeId,
    viewport: (i32, i32),
    dom: Dom,
    link_sheets: HashMap<NodeId, Arc<str>>,
    /// What the changes since the last flush invalidate (mrt_style::restyle).
    ops: Vec<RestyleOp>,
    /// What they do to the frames.
    frame_ops: Vec<FrameOp>,
    /// The flush's number, for MRT_VERIFY_INCREMENTAL.
    flush: usize,
    /// The root scroll offset in app units (PresShellFrames::SetRootScrollPosition).
    scroll: (i32, i32),
    /// The element scroll containers' offsets in app units (PresShellFrames::ApplyElementScroll).
    element_scroll: Vec<(NodeId, (i32, i32))>,
}

/// What it gives back: the DOM, and unless the layout panicked the boxes and what the restyle
/// recorder reads (the selector flags, the styled elements).
struct LayoutReply {
    dom: Dom,
    boxes: Option<HashMap<NodeId, [f64; 8]>>,
    restyle_state: (HashMap<NodeId, u32>, std::collections::HashSet<NodeId>),
    /// The answer to a ComputedValue query.
    computed: String,
    /// The size each frame element gives its document (nsSubDocumentFrame::GetSubdocumentSize).
    subdocuments: HashMap<NodeId, (i32, i32)>,
    /// The answer to an OffsetRect query.
    offsets: Option<(NodeId, [i32; 4])>,
    /// The answer to a ResizeObserverBoxes query, one per node asked.
    resize_boxes: Vec<[f64; 11]>,
    /// The answers to an Intersections query, one per question (see IntersectionAsk).
    intersections: Vec<Vec<i32>>,
    /// The answer to a NodesFromPoint query.
    hits: Vec<NodeId>,
    /// The answer to an InnerText query: None when the element is not rendered.
    inner_text: Option<String>,
}

enum LayoutMessage {
    Flush(LayoutRequest),
    /// Drop the style data kept between flushes (it is computed: the next flush styles everything).
    /// It comes with the page's DOM, which the frames read while they are destroyed, and which goes back.
    DropStyle(Dom),
    /// `getComputedStyle(node, pseudo).getPropertyValue(name)` right after a flush: the DOM is the one
    /// the flush gave back, unchanged since, and goes back with the value.
    ComputedValue { dom: Dom, node: NodeId, pseudo: Option<mrt_style::style::selector_parser::PseudoElement>, name: String },
    /// An element's offset parent and offset rectangle right after a flush (PresShellFrames::GetOffsetRect),
    /// with the DOM as for ComputedValue.
    OffsetRect { dom: Dom, node: NodeId },
    /// What ResizeObserver reads of each of `nodes` right after a flush
    /// (PresShellFrames::ResizeObserverBoxes), with the DOM as for ComputedValue.
    ResizeObserverBoxes { dom: Dom, nodes: Vec<NodeId> },
    /// What IntersectionObserver asks of the frames right after a flush, with the DOM as for
    /// ComputedValue.
    Intersections { dom: Dom, asks: Vec<IntersectionAsk>, scroll_margin: IntersectionMargin },
    /// The nodes under a point of the viewport, in app units, the topmost first
    /// (PresShellFrames::NodesFromPoint), with the visible area's size and the DOM as for ComputedValue.
    NodesFromPoint { dom: Dom, point: (i32, i32), port: (i32, i32) },
    /// An element's innerText (PresShellFrames::InnerText), with the DOM as for ComputedValue.
    InnerText { dom: Dom, node: NodeId },
}

/// A question an IntersectionObserver's update asks a document's frames, in app units relative to
/// its viewport. Each answer is a list of numbers, empty for none.
#[derive(Clone, Copy)]
enum IntersectionAsk {
    /// An element root's rectangle (PresShellFrames::IntersectionRootRect): x, y, width, height.
    RootRect(NodeId),
    /// A target against a root element of the document, or NONE for its viewport
    /// (PresShellFrames::IntersectionTarget): the target's rectangle, then the clipped one if any.
    Target(NodeId, NodeId),
    /// A rectangle clipped from a frame element up (PresShellFrames::IntersectionClipFrom).
    ClipFrom(NodeId, [i32; 4]),
    /// Where a frame element's document's viewport is (PresShellFrames::SubdocumentOrigin): x, y.
    Origin(NodeId),
}

/// The page's layout thread: it keeps the style data (StyleDom) and the frame tree's thread-local
/// stores from one flush to the next. A page nests deeply (layoutcount's 2 GB stack).
struct LayoutThread {
    requests: std::sync::mpsc::Sender<LayoutMessage>,
    replies: std::sync::mpsc::Receiver<LayoutReply>,
    /// Ends with the parsed sheets its last dropped style data kept, for the document's next thread.
    thread: std::thread::JoinHandle<Option<SheetCache>>,
}

impl LayoutThread {
    /// Start a document's layout thread, with the parsed sheets its last thread kept (`join`).
    fn start(sheets: Option<SheetCache>) -> LayoutThread {
        let (requests, request_rx) = std::sync::mpsc::channel::<LayoutMessage>();
        let (reply_tx, replies) = std::sync::mpsc::channel::<LayoutReply>();
        let thread = std::thread::Builder::new()
            .name("layout".into())
            .stack_size(2 << 30)
            .spawn(move || {
                let mut state = LayoutState { sheets, ..Default::default() };
                let mut recorder = crate::flushrec::FlushRecorder::from_env();
                for message in request_rx {
                    match message {
                        LayoutMessage::DropStyle(dom) => {
                            let dom = state.clear(dom);
                            let _ = reply_tx.send(LayoutReply { dom, boxes: None, restyle_state: Default::default(), computed: String::new(), subdocuments: HashMap::new(), offsets: None, resize_boxes: Vec::new(), intersections: Vec::new(), hits: Vec::new(), inner_text: None });
                        }
                        LayoutMessage::ComputedValue { dom, node, pseudo, name } => {
                            let (dom, computed) = computed_value(&mut state, dom, node, pseudo, &name);
                            let _ = reply_tx.send(LayoutReply { dom, boxes: None, restyle_state: Default::default(), computed, subdocuments: HashMap::new(), offsets: None, resize_boxes: Vec::new(), intersections: Vec::new(), hits: Vec::new(), inner_text: None });
                        }
                        LayoutMessage::OffsetRect { dom, node } => {
                            let (dom, offsets) = offset_rect(&mut state, dom, node);
                            let _ = reply_tx.send(LayoutReply { dom, boxes: None, restyle_state: Default::default(), computed: String::new(), subdocuments: HashMap::new(), offsets, resize_boxes: Vec::new(), intersections: Vec::new(), hits: Vec::new(), inner_text: None });
                        }
                        LayoutMessage::ResizeObserverBoxes { dom, nodes } => {
                            let (dom, resize_boxes) = resize_observer_boxes(&mut state, dom, &nodes);
                            let _ = reply_tx.send(LayoutReply { dom, boxes: None, restyle_state: Default::default(), computed: String::new(), subdocuments: HashMap::new(), offsets: None, resize_boxes, intersections: Vec::new(), hits: Vec::new(), inner_text: None });
                        }
                        LayoutMessage::Intersections { dom, asks, scroll_margin } => {
                            let (dom, intersections) = intersection_answers(&mut state, dom, &asks, &scroll_margin);
                            let _ = reply_tx.send(LayoutReply { dom, boxes: None, restyle_state: Default::default(), computed: String::new(), subdocuments: HashMap::new(), offsets: None, resize_boxes: Vec::new(), intersections, hits: Vec::new(), inner_text: None });
                        }
                        LayoutMessage::NodesFromPoint { dom, point, port } => {
                            let (dom, hits) = nodes_from_point(&mut state, dom, point, port);
                            let _ = reply_tx.send(LayoutReply { dom, boxes: None, restyle_state: Default::default(), computed: String::new(), subdocuments: HashMap::new(), offsets: None, resize_boxes: Vec::new(), intersections: Vec::new(), hits, inner_text: None });
                        }
                        LayoutMessage::InnerText { dom, node } => {
                            let (dom, inner_text) = inner_text(&mut state, dom, node);
                            let _ = reply_tx.send(LayoutReply { dom, boxes: None, restyle_state: Default::default(), computed: String::new(), subdocuments: HashMap::new(), offsets: None, resize_boxes: Vec::new(), intersections: Vec::new(), hits: Vec::new(), inner_text });
                        }
                        LayoutMessage::Flush(request) => {
                            if let Some(recorder) = recorder.as_mut().filter(|_| request.doc == mrt_style::mrt_dom::DOCUMENT) {
                                recorder.record(request.flush, &request.dom, &request.link_sheets, &request.ops, &request.frame_ops);
                            }
                            let (reply, panicked) = layout_flush(&mut state, request);
                            let _ = reply_tx.send(reply);
                            if panicked {
                                // the frame tree's stores may be half built: end the thread with them
                                break;
                            }
                        }
                    }
                }
                state.sheets.take()
            })
            .unwrap();
        LayoutThread { requests, replies, thread }
    }

    fn flush(&self, request: LayoutRequest) -> LayoutReply {
        self.requests.send(LayoutMessage::Flush(request)).expect("the layout thread ended");
        self.replies.recv().expect("the layout thread ended without giving the DOM back")
    }

    fn computed_value(&self, dom: Dom, node: NodeId, pseudo: Option<mrt_style::style::selector_parser::PseudoElement>, name: &str) -> (Dom, String) {
        self.requests.send(LayoutMessage::ComputedValue { dom, node, pseudo, name: name.to_string() }).expect("the layout thread ended");
        let reply = self.replies.recv().expect("the layout thread ended without giving the DOM back");
        (reply.dom, reply.computed)
    }

    fn offset_rect(&self, dom: Dom, node: NodeId) -> (Dom, Option<(NodeId, [i32; 4])>) {
        self.requests.send(LayoutMessage::OffsetRect { dom, node }).expect("the layout thread ended");
        let reply = self.replies.recv().expect("the layout thread ended without giving the DOM back");
        (reply.dom, reply.offsets)
    }

    fn resize_observer_boxes(&self, dom: Dom, nodes: Vec<NodeId>) -> (Dom, Vec<[f64; 11]>) {
        self.requests.send(LayoutMessage::ResizeObserverBoxes { dom, nodes }).expect("the layout thread ended");
        let reply = self.replies.recv().expect("the layout thread ended without giving the DOM back");
        (reply.dom, reply.resize_boxes)
    }

    fn intersections(&self, dom: Dom, asks: Vec<IntersectionAsk>, scroll_margin: IntersectionMargin) -> (Dom, Vec<Vec<i32>>) {
        self.requests.send(LayoutMessage::Intersections { dom, asks, scroll_margin }).expect("the layout thread ended");
        let reply = self.replies.recv().expect("the layout thread ended without giving the DOM back");
        (reply.dom, reply.intersections)
    }

    fn nodes_from_point(&self, dom: Dom, point: (i32, i32), port: (i32, i32)) -> (Dom, Vec<NodeId>) {
        self.requests.send(LayoutMessage::NodesFromPoint { dom, point, port }).expect("the layout thread ended");
        let reply = self.replies.recv().expect("the layout thread ended without giving the DOM back");
        (reply.dom, reply.hits)
    }

    fn inner_text(&self, dom: Dom, node: NodeId) -> (Dom, Option<String>) {
        self.requests.send(LayoutMessage::InnerText { dom, node }).expect("the layout thread ended");
        let reply = self.replies.recv().expect("the layout thread ended without giving the DOM back");
        (reply.dom, reply.inner_text)
    }

    fn drop_style(&self, dom: Dom) -> Dom {
        self.requests.send(LayoutMessage::DropStyle(dom)).expect("the layout thread ended");
        self.replies.recv().expect("the layout thread ended without giving the DOM back").dom
    }

    /// End the thread; the parsed sheets it kept come back (None if it kept none, or panicked).
    fn join(self) -> Option<SheetCache> {
        drop(self.requests);
        self.thread.join().ok().flatten()
    }
}

/// The boxes of a styled page: frames built and reflowed, every element's box read, the frames
/// destroyed (frame construction is not incremental yet). Also the time of each phase in ms:
/// frame construction, reflow, reading the boxes, destroying the frames.
fn lay_out(sd: &StyleDom) -> (HashMap<NodeId, [f64; 8]>, [f64; 4]) {
    let t0 = std::time::Instant::now();
    let mut layout = ConstructFramesTrackingMode(sd, crate::viewport().0, crate::viewport().1, false, true);
    let t1 = std::time::Instant::now();
    ReflowFrames(&mut layout);
    let t2 = std::time::Instant::now();
    let boxes = layout
        .mFramesOfElementsWithId
        .iter()
        .map(|&(node, frame)| {
            let (x, y, w, h) = BoundingClientRectInCSSPixels(frame);
            let (cw, ch, sw, sh) = ClientAreaAndScrollSize(frame);
            (node, [x, y, w, h, cw, ch, sw, sh])
        })
        .collect::<HashMap<_, _>>();
    let t3 = std::time::Instant::now();
    DestroyFrameTree(layout);
    let t4 = std::time::Instant::now();
    let ms = |a: std::time::Instant, b: std::time::Instant| (b - a).as_secs_f64() * 1000.0;
    (boxes, [ms(t0, t1), ms(t1, t2), ms(t2, t3), ms(t3, t4)])
}

/// What the layout thread keeps from one flush to the next: the page's style data, and the frames
/// made from it (dropped first: they read the StyleDom).
#[derive(Default)]
struct LayoutState {
    frames: Option<PresShellFrames>,
    style: Option<Box<StyleDom>>,
    /// The parsed sheets kept when the style data was dropped (SPEC decision 22), for the next style
    /// build.
    sheets: Option<SheetCache>,
}

/// What dropping a document's style data keeps (under pressure and at hibernation). By default the
/// parsed sheets, so a wake does not parse them again (SPEC decision 22, GAPS A13). MRT_KEEP_STYLE
/// changes it, to measure the trade: `none` keeps nothing, as before decision 22; `rules` keeps the
/// stylist too (the rule tables), with every element unstyled, and the layout thread is then not
/// ended at hibernation (decision 22 did not choose it: about 58 ms on Gmail for 12.8 MB more).
#[derive(Clone, Copy, PartialEq)]
enum KeepStyle {
    Nothing,
    Sheets,
    Rules,
}

fn keep_style() -> KeepStyle {
    match std::env::var("MRT_KEEP_STYLE").as_deref() {
        Ok("none") => KeepStyle::Nothing,
        Ok("rules") => KeepStyle::Rules,
        _ => KeepStyle::Sheets,
    }
}

impl LayoutState {
    /// Drop the frames and the style data (computed results: the next flush makes them again), keeping
    /// what `keep_style` says. The frames read `dom`, the page's DOM, while they are destroyed; it is
    /// given back.
    fn clear(&mut self, dom: Dom) -> Dom {
        let Some(sd) = self.style.as_mut() else {
            return dom;
        };
        let links = sd.link_sheets.clone();
        sd.attach_dom_only(dom, links);
        if let Some(frames) = self.frames.take() {
            frames.Destroy();
        }
        let sd = self.style.as_mut().unwrap();
        let dom = sd.detach_dom();
        match keep_style() {
            KeepStyle::Nothing => self.style = None,
            KeepStyle::Sheets => self.sheets = self.style.take().map(|mut sd| sd.take_sheet_cache()),
            KeepStyle::Rules => sd.clear_all_style_data(),
        }
        dom
    }
}

/// The attributes whose change the frames read directly, not through the style (Gecko: the frame
/// classes' AttributeChanged): the element is reframed. A <select>'s or <textarea>'s frame copies
/// what is inside it (FormControlData), so any change inside one reframes the control.
fn attribute_reframes(dom: &Dom, node: NodeId, name: &str) -> Option<NodeId> {
    let tag = dom.tag(node);
    let reframe = match name {
        "colspan" | "rowspan" => matches!(tag, "td" | "th"),
        "span" => matches!(tag, "col" | "colgroup"),
        "alt" | "src" | "width" | "height" => matches!(tag, "img" | "video" | "audio" | "iframe" | "canvas" | "embed" | "object" | "svg" | "input"),
        "viewBox" | "viewbox" => tag == "svg",
        "type" | "value" | "size" | "placeholder" | "checked" => matches!(tag, "input" | "button"),
        "cols" | "rows" => tag == "textarea",
        "multiple" => tag == "select",
        "open" => tag == "details",
        "lang" => true,
        _ => false,
    };
    if reframe {
        return Some(node);
    }
    // slot assignment: the host's flattened children change
    if name == "slot" {
        let parent = dom.node(node).parent;
        if parent != NONE && dom.shadow_root(parent) != NONE {
            return Some(parent);
        }
    }
    if name == "name" && dom.is_slot(node) {
        let root = dom.containing_shadow(node);
        if root != NONE {
            return Some(dom.host(root));
        }
    }
    None
}

/// The nearest <select> or <textarea> that has `node` inside (or is it), see attribute_reframes.
fn form_control_ancestor(dom: &Dom, node: NodeId) -> Option<NodeId> {
    let mut n = node;
    while n != NONE {
        if dom.node(n).kind == Kind::Element && matches!(dom.tag(n), "select" | "textarea") {
            return Some(n);
        }
        n = dom.node(n).parent;
    }
    None
}

/// The changes of a flush applied to the kept frames, after the restyle: what RestyleManager::
/// ProcessPostTraversal gives Gecko's frame constructor, in tree order, the descendants of a node that
/// is reframed left out (its reframe rebuilds them). `damaged`: the elements whose style changed.
fn apply_frame_changes(frames: &mut PresShellFrames, dom: &Dom, frame_ops: &[FrameOp], damaged: &[(NodeId, u16)]) {
    let mut reframe: std::collections::HashSet<NodeId> = damaged.iter().map(|d| d.0).collect();
    let mut text_changed: Vec<NodeId> = Vec::new();
    for op in frame_ops {
        match op {
            FrameOp::Inserted(n) | FrameOp::Reframe(n) => {
                reframe.insert(*n);
                if let Some(c) = form_control_ancestor(dom, dom.node(*n).parent) {
                    reframe.insert(c);
                }
            }
            FrameOp::TextChanged(n) => {
                text_changed.push(*n);
                if let Some(c) = form_control_ancestor(dom, *n) {
                    reframe.insert(c);
                }
            }
            FrameOp::AttrChanged(n, name) => {
                if let Some(target) = attribute_reframes(dom, *n, name) {
                    reframe.insert(target);
                }
                if let Some(c) = form_control_ancestor(dom, dom.node(*n).parent) {
                    reframe.insert(c);
                }
            }
            FrameOp::Removed(r) => {
                if r.mContainer != NONE && dom.is_connected(r.mContainer) {
                    if let Some(c) = form_control_ancestor(dom, r.mContainer) {
                        reframe.insert(c);
                    }
                }
            }
        }
    }
    let covered = |n: NodeId| {
        let mut p = dom.flattened_parent(n);
        while p != NONE {
            if reframe.contains(&p) {
                return true;
            }
            p = dom.flattened_parent(p);
        }
        false
    };
    let mut work: Vec<(NodeId, bool)> = reframe.iter().filter(|&&n| dom.is_connected(n) && !covered(n)).map(|&n| (n, true)).collect();
    text_changed.sort();
    text_changed.dedup();
    work.extend(text_changed.into_iter().filter(|&n| dom.is_connected(n) && !reframe.contains(&n) && !covered(n)).map(|n| (n, false)));
    if work.is_empty() {
        frames.ProcessPendingReconstructs();
        return;
    }
    // tree order (a shadow tree before the light children, as the flattened tree has them)
    let mut order: HashMap<NodeId, usize> = HashMap::with_capacity(dom.nodes.len());
    let mut stack = vec![mrt_style::mrt_dom::DOCUMENT];
    while let Some(n) = stack.pop() {
        order.insert(n, order.len());
        let kids: Vec<NodeId> = dom.children(n).collect();
        stack.extend(kids.into_iter().rev());
        let root = dom.shadow_root(n);
        if root != NONE {
            stack.push(root);
        }
    }
    work.sort_by_key(|w| order.get(&w.0).copied().unwrap_or(usize::MAX));
    for (n, is_reframe) in work {
        if is_reframe {
            frames.RecreateFramesForContent(n);
        } else {
            frames.CharacterDataChanged(n);
        }
    }
    frames.ProcessPendingReconstructs();
}

/// The boxes of every element with a frame, from the kept frames: what getBoundingClientRect gives
/// (GetBoundingClientRect: continuations and transforms included), then the client and scroll sizes.
fn read_boxes(frames: &PresShellFrames) -> HashMap<NodeId, [f64; 8]> {
    frames
        .ElementFrames()
        .into_iter()
        .map(|(node, frame)| {
            let (x, y, w, h) = BoundingClientRectInCSSPixels(frame);
            // (a scrolled container's, from before its children moved)
            let (cw, ch, sw, sh) = frames.mElementScrollSizes.get(&node).copied().unwrap_or_else(|| ClientAreaAndScrollSize(frame));
            (node, [x, y, w, h, cw, ch, sw, sh])
        })
        .collect()
}

/// One flush on the layout thread: the frames' removals applied to the kept frames, the page restyled
/// where its changes invalidated it, the frames rebuilt where the changes say (from scratch the first
/// time, or when everything was restyled), the dirty frames reflowed, and the DOM given back. A panic
/// in style or layout is caught so that the DOM comes back (they only read it); the style data and the
/// frames are dropped then.
fn layout_flush(state: &mut LayoutState, request: LayoutRequest) -> (LayoutReply, bool) {
    // MRT_FLUSH_TIMES=1: each flush's phases on stderr, in ms (attaching the DOM and its sheets, the
    // removals, rule tables, style resolution, frame changes, reflow, reading the boxes), the ops it was
    // given and what the frame constructor did.
    // MRT_VERIFY_INCREMENTAL=1: each flush also styles and lays out a copy of the DOM from scratch and
    // reports the boxes that differ.
    // MRT_FULL_REFLOW=1: the kept frames are all reflowed at each flush (to check frame construction
    // alone).
    let times = std::env::var_os("MRT_FLUSH_TIMES").is_some();
    let verify = std::env::var_os("MRT_VERIFY_INCREMENTAL").is_some();
    let full_reflow = std::env::var_os("MRT_FULL_REFLOW").is_some();
    let ops = request.ops.len();
    let fops = request.frame_ops.len();
    let copy = verify.then(|| (request.dom.clone(), request.link_sheets.clone()));
    let t0 = std::time::Instant::now();
    let first = state.style.is_none();
    let (doc, (width, height)) = (request.doc, request.viewport);
    match &mut state.style {
        Some(sd) => {
            sd.attach_dom_only(request.dom, request.link_sheets);
            // a frame resized: its media queries again, and everything restyled and reframed
            sd.set_viewport((width as f32, height as f32));
        }
        None => {
            let sheets = state.sheets.take().unwrap_or_default();
            state.style = Some(Box::new(StyleDom::for_document(doc, request.dom, (width as f32, height as f32), request.link_sheets, sheets)))
        }
    }
    let LayoutState { frames, style, .. } = state;
    let sd = style.as_mut().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ms = |a: std::time::Instant, b: std::time::Instant| (b - a).as_secs_f64() * 1000.0;
        let t1 = std::time::Instant::now();
        // the removals, with the styles and frames as they were (Gecko destroys frames at the removal)
        if let Some(f) = frames.as_mut() {
            // (the scrolled children back in place while their frames are still those it moved)
            f.UndoElementScroll();
            f.BeginChanges();
            for op in &request.frame_ops {
                if let FrameOp::Removed(r) = op {
                    f.ContentRemoved(*r);
                }
            }
        }
        let everything = if first { true } else { sd.apply_restyle_ops(&request.ops) };
        let t2 = std::time::Instant::now();
        sd.flush_stylist();
        let t3 = std::time::Instant::now();
        sd.resolve();
        let damaged = sd.take_damage();
        let t4 = std::time::Instant::now();
        let rebuilt = everything || frames.is_none();
        if rebuilt {
            if let Some(f) = frames.take() {
                f.Destroy();
            }
            *frames = Some(PresShellFrames::new(sd, width, height));
        } else {
            apply_frame_changes(frames.as_mut().unwrap(), &sd.tree.dom, &request.frame_ops, &damaged);
        }
        let f = frames.as_mut().unwrap();
        // (applied by ReflowDirty, after the reflow)
        f.mLayout.mRootScrollPosition = nsPoint::new(request.scroll.0, request.scroll.1);
        f.mElementScrollPositions = request.element_scroll.iter().map(|&(n, (x, y))| (n, nsPoint::new(x, y))).collect();
        let counts = f.TakeDynamicChangeCounts();
        let t5 = std::time::Instant::now();
        f.ReflowDirty(full_reflow && !rebuilt);
        let t6 = std::time::Instant::now();
        let boxes = read_boxes(f);
        let subdocuments = f.SubdocumentSizes().into_iter().collect::<HashMap<_, _>>();
        let t7 = std::time::Instant::now();
        if times {
            eprintln!(
                "FLUSHTIME sheets {:.1} removals {:.1} stylist {:.1} resolve {:.1} construct {:.1} reflow {:.1} read {:.1} layout {:.1} ops {ops} frameops {fops} damaged {} rebuilt {} removed {} reconstructs {} inserted {} containers {}",
                ms(t0, t1),
                ms(t1, t2),
                ms(t2, t3),
                ms(t3, t4),
                ms(t4, t5),
                ms(t5, t6),
                ms(t6, t7),
                ms(t4, t7),
                damaged.len(),
                rebuilt as u8,
                counts.mRemovals,
                counts.mReconstructs,
                counts.mInsertions,
                counts.mContainerReframes,
            );
        }
        (boxes, subdocuments)
    }));
    match result {
        Ok((boxes, subdocuments)) => {
            if let Some((dom, link_sheets)) = copy {
                let depth: usize = std::env::var("MRT_VERIFY_DUMP").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
                let frames = state.frames.as_ref();
                let dump = |n: NodeId| {
                    frames.and_then(|f| f.ElementFrames().into_iter().find(|e| e.0 == n)).map(|e| mrt_layout::driver::DumpFrameTree(e.1, depth)).unwrap_or_default()
                };
                verify_flush(request.flush, doc, (width, height), &boxes, dom, link_sheets, &dump);
            }
            let sd = state.style.as_mut().unwrap();
            let restyle_state = sd.restyle_state();
            (LayoutReply { dom: sd.detach_dom(), boxes: Some(boxes), restyle_state, computed: String::new(), subdocuments, offsets: None, resize_boxes: Vec::new(), intersections: Vec::new(), hits: Vec::new(), inner_text: None }, false)
        }
        Err(_) => {
            // (the frames are left to the thread's end: their stores may be half built)
            std::mem::forget(state.frames.take());
            let dom = state.style.take().unwrap().detach_dom();
            (LayoutReply { dom, boxes: None, restyle_state: Default::default(), computed: String::new(), subdocuments: HashMap::new(), offsets: None, resize_boxes: Vec::new(), intersections: Vec::new(), hits: Vec::new(), inner_text: None }, true)
        }
    }
}

/// A computed value on the layout thread, from the style data and frames of the last flush (which the
/// DOM has not changed since). A panic (a feature that is not ported) answers "".
fn computed_value(state: &mut LayoutState, dom: Dom, node: NodeId, pseudo: Option<mrt_style::style::selector_parser::PseudoElement>, name: &str) -> (Dom, String) {
    let Some(sd) = state.style.as_mut() else {
        return (dom, String::new());
    };
    sd.attach_dom_unchanged(dom);
    let sd = &**sd;
    let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mrt_layout::GetComputedPropertyValue(sd, node, pseudo, name))).unwrap_or_default();
    (state.style.as_mut().unwrap().detach_dom(), value)
}

/// An element's offset parent and rectangle on the layout thread, from the frames of the last flush
/// (which the DOM has not changed since). None without a frame, or after a panic.
fn offset_rect(state: &mut LayoutState, dom: Dom, node: NodeId) -> (Dom, Option<(NodeId, [i32; 4])>) {
    let (Some(sd), Some(frames)) = (state.style.as_mut(), state.frames.as_ref()) else {
        return (dom, None);
    };
    sd.attach_dom_unchanged(dom);
    let offsets = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| frames.GetOffsetRect(node))).unwrap_or_default();
    (state.style.as_mut().unwrap().detach_dom(), offsets)
}

/// What ResizeObserver reads of each of `nodes` on the layout thread, from the frames of the last
/// flush: PresShellFrames::ResizeObserverBoxes, zeros (no frame) after a panic.
fn resize_observer_boxes(state: &mut LayoutState, dom: Dom, nodes: &[NodeId]) -> (Dom, Vec<[f64; 11]>) {
    let (Some(sd), Some(frames)) = (state.style.as_mut(), state.frames.as_ref()) else {
        return (dom, vec![[0.0; 11]; nodes.len()]);
    };
    sd.attach_dom_unchanged(dom);
    let boxes = nodes
        .iter()
        .map(|&node| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| frames.ResizeObserverBoxes(node))).unwrap_or([0.0; 11]))
        .collect();
    (state.style.as_mut().unwrap().detach_dom(), boxes)
}

fn intersection_answers(state: &mut LayoutState, dom: Dom, asks: &[IntersectionAsk], scroll_margin: &IntersectionMargin) -> (Dom, Vec<Vec<i32>>) {
    let (Some(sd), Some(frames)) = (state.style.as_mut(), state.frames.as_ref()) else {
        return (dom, vec![Vec::new(); asks.len()]);
    };
    sd.attach_dom_unchanged(dom);
    let rect = |r: nsRect| vec![r.x, r.y, r.width, r.height];
    let answers = asks
        .iter()
        .map(|&ask| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match ask {
                IntersectionAsk::RootRect(root) => frames.IntersectionRootRect(root).map_or(Vec::new(), rect),
                IntersectionAsk::Target(target, root) => match frames.IntersectionTarget(target, root, scroll_margin) {
                    None => Vec::new(),
                    Some((target_rect, clipped)) => {
                        let mut out = rect(target_rect);
                        if let Some(c) = clipped {
                            out.extend(rect(c));
                        }
                        out
                    }
                },
                IntersectionAsk::ClipFrom(element, [x, y, w, h]) => {
                    frames.IntersectionClipFrom(element, nsRect::new(x, y, w, h), scroll_margin).map_or(Vec::new(), rect)
                }
                IntersectionAsk::Origin(element) => frames.SubdocumentOrigin(element).map_or(Vec::new(), |p| vec![p.x, p.y]),
            }))
            .unwrap_or_default()
        })
        .collect();
    (state.style.as_mut().unwrap().detach_dom(), answers)
}

/// The nodes under a point on the layout thread, from the frames of the last flush:
/// PresShellFrames::NodesFromPoint, none after a panic.
fn nodes_from_point(state: &mut LayoutState, dom: Dom, point: (i32, i32), port: (i32, i32)) -> (Dom, Vec<NodeId>) {
    let (Some(sd), Some(frames)) = (state.style.as_mut(), state.frames.as_ref()) else {
        return (dom, Vec::new());
    };
    sd.attach_dom_unchanged(dom);
    let hits = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        frames.NodesFromPoint(nsPoint::new(point.0, point.1), nsRect::new(0, 0, port.0, port.1))
    }))
    .unwrap_or_default();
    (state.style.as_mut().unwrap().detach_dom(), hits)
}

/// An element's innerText on the layout thread, from the frames of the last flush:
/// PresShellFrames::InnerText, None (not rendered) after a panic.
fn inner_text(state: &mut LayoutState, dom: Dom, node: NodeId) -> (Dom, Option<String>) {
    let (Some(sd), Some(frames)) = (state.style.as_mut(), state.frames.as_ref()) else {
        return (dom, None);
    };
    sd.attach_dom_unchanged(dom);
    let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| frames.InnerText(node))).unwrap_or_default();
    (state.style.as_mut().unwrap().detach_dom(), text.map(|t| String::from_utf16_lossy(&t)))
}

/// `flushreplay`: runs the flushes recorded in `dir` (MRT_RECORD_FLUSHES, see flushrec.rs) whose
/// numbers are in `range`, in order, as the page's layout thread ran them: one thread that keeps the
/// style data, ended by a flush whose layout panicked (the next flush starts another).
/// MRT_FLUSH_TIMES and MRT_VERIFY_INCREMENTAL apply as in `realsite`. Returns the panicked flushes.
pub fn replay_flushes(dir: &std::path::Path, range: std::ops::RangeInclusive<usize>) -> Vec<usize> {
    let flushes: Vec<usize> = crate::flushrec::recorded_flushes(dir).into_iter().filter(|n| range.contains(n)).collect();
    let mut panicked = Vec::new();
    let mut next = 0;
    while next < flushes.len() {
        let dir = dir.to_path_buf();
        let list = flushes[next..].to_vec();
        // (how many flushes ran, and whether the last one panicked)
        let (ran, last_panicked) = std::thread::Builder::new()
            .name("layout".into())
            .stack_size(2 << 30)
            .spawn(move || {
                let mut state = LayoutState::default();
                for (i, &flush) in list.iter().enumerate() {
                    let r = crate::flushrec::read_flush(&dir, flush);
                    let request = LayoutRequest { doc: mrt_style::mrt_dom::DOCUMENT, viewport: crate::viewport(), dom: r.dom, link_sheets: r.sheets, ops: r.ops, frame_ops: r.frame_ops, flush, scroll: (0, 0), element_scroll: Vec::new() };
                    let (_, panicked) = layout_flush(&mut state, request);
                    if panicked {
                        return (i + 1, true);
                    }
                }
                // (as `realsite` drops the kept style data and frames before its report)
                if let Some(&last) = list.last() {
                    state.clear(crate::flushrec::read_flush(&dir, last).dom);
                }
                (list.len(), false)
            })
            .unwrap()
            .join()
            .unwrap();
        if last_panicked {
            panicked.push(flushes[next + ran - 1]);
        }
        next += ran;
    }
    panicked
}

/// MRT_VERIFY_INCREMENTAL: the boxes of `dom` styled and laid out from scratch, against the flush's.
/// The layout from scratch runs on a thread of its own: the frame tree is per thread, and this one keeps
/// the page's.
fn verify_flush(flush: usize, doc: NodeId, viewport: (i32, i32), boxes: &HashMap<NodeId, [f64; 8]>, dom: Dom, link_sheets: HashMap<NodeId, Arc<str>>, dump_incremental: &dyn Fn(NodeId) -> String) {
    // MRT_VERIFY_DUMP=N: the frames under the first element that differs, N levels deep, both ways
    let dump_depth: Option<usize> = std::env::var("MRT_VERIFY_DUMP").ok().and_then(|v| v.parse().ok());
    let boxes_for_thread = boxes.clone();
    let (expected, tags, scratch_dump) = std::thread::Builder::new()
        .name("verify".into())
        .stack_size(2 << 30)
        .spawn(move || {
            let mut fresh = StyleDom::for_document(doc, dom, (viewport.0 as f32, viewport.1 as f32), link_sheets, SheetCache::default());
            fresh.resolve();
            let mut layout = ConstructFramesTrackingMode(&fresh, viewport.0, viewport.1, false, true);
            ReflowFrames(&mut layout);
            let expected: HashMap<NodeId, [f64; 8]> = layout
                .mFramesOfElementsWithId
                .iter()
                .map(|&(node, frame)| {
                    let (x, y, w, h) = BoundingClientRectInCSSPixels(frame);
                    let (cw, ch, sw, sh) = ClientAreaAndScrollSize(frame);
                    (node, [x, y, w, h, cw, ch, sw, sh])
                })
                .collect();
            let mut differ: Vec<NodeId> = expected.keys().chain(boxes_for_thread.keys()).copied().filter(|n| expected.get(n) != boxes_for_thread.get(n)).collect();
            differ.sort();
            // (MRT_VERIFY_NODE=N: dump that element instead of the first that differs)
            let pick: Option<NodeId> = std::env::var("MRT_VERIFY_NODE").ok().and_then(|v| v.parse().ok());
            let scratch_dump = match (dump_depth, pick.as_ref().or(differ.first())) {
                (Some(depth), Some(&n)) => layout.mFramesOfElementsWithId.iter().find(|e| e.0 == n).map(|e| mrt_layout::driver::DumpFrameTree(e.1, depth)).unwrap_or_default(),
                _ => String::new(),
            };
            let mut tags: HashMap<NodeId, String> = expected.keys().map(|&n| (n, fresh.tree.dom.tag(n).to_string())).collect();
            if let (Some(_), Some(&n)) = (dump_depth, pick.as_ref().filter(|&&p| (p as usize) < fresh.tree.dom.nodes.len()).or(differ.first())) {
                // the element's ancestors, for choosing what to dump next
                let dom = &fresh.tree.dom;
                let mut chain = Vec::new();
                let mut a = dom.flattened_parent(n);
                while a != NONE && chain.len() < 8 {
                    chain.push(format!("{}#{a}", if dom.node(a).kind == Kind::Element { dom.tag(a) } else { "?" }));
                    a = dom.flattened_parent(a);
                }
                tags.insert(u32::MAX, chain.join(" < "));
            }
            DestroyFrameTree(layout);
            (expected, tags, scratch_dump)
        })
        .unwrap()
        .join()
        .unwrap();
    let mut differ: Vec<NodeId> = expected.keys().chain(boxes.keys()).copied().filter(|n| expected.get(n) != boxes.get(n)).collect();
    differ.sort();
    differ.dedup();
    if differ.is_empty() {
        eprintln!("VERIFY flush {flush}: {} boxes, the same", expected.len());
    } else {
        eprintln!("VERIFY flush {flush}: {} of {} boxes differ", differ.len(), expected.len());
        for n in differ.iter().take(5) {
            eprintln!("VERIFY   <{}> #{n}: incremental {:?}, from scratch {:?}", tags.get(n).map_or("?", |t| t.as_str()), boxes.get(n), expected.get(n));
        }
        if dump_depth.is_some() {
            let pick: NodeId = std::env::var("MRT_VERIFY_NODE").ok().and_then(|v| v.parse().ok()).unwrap_or(differ[0]);
            eprintln!("VERIFY ancestors of #{pick}: {}", tags.get(&u32::MAX).map_or("", |s| s.as_str()));
            eprintln!("VERIFY frames of #{pick} incremental:\n{}VERIFY from scratch:\n{}", dump_incremental(pick), scratch_dump);
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
    // the C++ layer passes UTF-8 made by SpiderMonkey (lone surrogates become U+FFFD)
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(p as *const u8, len) }).unwrap_or("\u{FFFD}")
}

fn valid(h: &Host, id: NodeId) -> bool {
    id != NONE && (id as usize) < h.dom.nodes.len()
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_kind(h: *mut c_void, id: u32) -> i32 {
    let h = unsafe { host(h) };
    if valid(h, id) { h.dom.node(id).kind as i32 } else { -1 }
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_link(h: *mut c_void, id: u32, which: i32) -> u32 {
    let h = unsafe { host(h) };
    if !valid(h, id) {
        return NONE;
    }
    let n = h.dom.node(id);
    match which {
        0 => n.parent,
        1 => n.first_child,
        2 => n.last_child,
        3 => n.next_sibling,
        4 => n.prev_sibling,
        5 => h.dom.host(id),
        6 => h.dom.shadow_root(id),
        // the parent in the shadow-including tree, then the assigned slot
        7 => h.dom.shadow_including_parent(id),
        _ => h.dom.assigned_slot(id),
    }
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_name(h: *mut c_void, id: u32, out: *mut *const c_char, len: *mut usize) -> i32 {
    let h = unsafe { host(h) };
    let ns = h.dom.node(id).ns as i32;
    let name = h.dom.tag(id).to_string();
    h.give(name, out, len);
    ns
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_quirks_mode(h: *mut c_void) -> i32 {
    unsafe { host(h) }.dom.mode
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_text_content(h: *mut c_void, id: u32, out: *mut *const c_char, len: *mut usize) {
    let h = unsafe { host(h) };
    let s = h.dom.text_content(id);
    h.give(s, out, len);
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_set_text_content(h: *mut c_void, id: u32, s: *const c_char, len: usize) {
    let h = unsafe { host(h) };
    let is_element = h.dom.node(id).kind != Kind::Text && h.dom.node(id).kind != Kind::Comment;
    // a <style>'s text: a new sheet
    let style = if is_element { id } else { h.dom.node(id).parent };
    if style != NONE && h.dom.node(style).kind == Kind::Element && h.dom.tag(style) == "style" {
        h.sheet_replaced(style);
    }
    if is_element {
        // (nsContentUtils::SetNodeTextContent: the children removed, then a text appended)
        let children: Vec<NodeId> = h.dom.children(id).collect();
        for c in children {
            h.restyle(c, |r, d| r.content_will_be_removed(d, c));
            h.frame_will_be_removed(c);
        }
    }
    h.dom.set_text_content(id, unsafe { text(s, len) });
    if is_element {
        let children: Vec<NodeId> = h.dom.children(id).collect();
        for c in children {
            h.restyle(c, |r, d| r.content_inserted(d, c));
            h.frame_op(c, FrameOp::Inserted(c));
        }
    } else {
        h.restyle(id, |r, d| r.character_data_changed(d, id));
        h.frame_op(id, FrameOp::TextChanged(id));
    }
    h.changed();
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_create_element(h: *mut c_void, tag: *const c_char, len: usize) -> u32 {
    let h = unsafe { host(h) };
    h.dom.create_element(unsafe { text(tag, len) })
}

/// An element in namespace `ns` (the parser's numbers), for `createElementNS`.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_create_element_ns(h: *mut c_void, ns: i32, tag: *const c_char, len: usize) -> u32 {
    let h = unsafe { host(h) };
    h.dom.create_element_ns(ns as u8, unsafe { text(tag, len) })
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_create_text(h: *mut c_void, s: *const c_char, len: usize) -> u32 {
    let h = unsafe { host(h) };
    h.dom.create_text(unsafe { text(s, len) })
}

/// The checks of `mrt_dom_insert` alone: 0, or the error it would return (nsINode::EnsurePreInsertionValidity,
/// which runs before a node is taken out of its old parent).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_check_insert(h: *mut c_void, parent: u32, child: u32, before: u32) -> i32 {
    let h = unsafe { host(h) };
    match h.dom.check_insert(parent, child, before) {
        Ok(()) => 0,
        Err("NotFoundError") => 2,
        Err(_) => 1,
    }
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_insert(h: *mut c_void, parent: u32, child: u32, before: u32) -> i32 {
    let h = unsafe { host(h) };
    match h.dom.check_insert(parent, child, before) {
        Ok(()) => {
            // (a node that has a parent is removed from it first: nsINode::InsertChildBefore)
            let old_parent = h.dom.node(child).parent;
            if old_parent != NONE {
                h.restyle(child, |r, d| r.content_will_be_removed(d, child));
                h.frame_will_be_removed(child);
                h.sheets_moved(old_parent, child);
            }
            h.dom.insert_child(parent, child, before);
            h.sheets_moved(parent, child);
            h.restyle(child, |r, d| r.content_inserted(d, child));
            // (a fragment's children were inserted one by one: each is its own op)
            if h.dom.node(child).kind != Kind::Fragment {
                h.frame_op(child, FrameOp::Inserted(child));
            }
            h.changed();
            0
        }
        Err("NotFoundError") => 2,
        Err(_) => 1,
    }
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_remove(h: *mut c_void, parent: u32, child: u32) -> i32 {
    let h = unsafe { host(h) };
    if h.dom.node(child).parent == parent && parent != NONE {
        h.restyle(child, |r, d| r.content_will_be_removed(d, child));
        h.frame_will_be_removed(child);
        h.sheets_moved(parent, child);
    }
    let removed = h.dom.remove_child(parent, child);
    h.changed();
    removed as i32
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_get_attr(h: *mut c_void, id: u32, name: *const c_char, name_len: usize, out: *mut *const c_char, len: *mut usize) -> i32 {
    let h = unsafe { host(h) };
    if h.dom.node(id).kind != Kind::Element {
        return 0;
    }
    match h.dom.get_attribute(id, unsafe { text(name, name_len) }).map(str::to_string) {
        Some(v) => {
            h.give(v, out, len);
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_set_attr(h: *mut c_void, id: u32, name: *const c_char, name_len: usize, value: *const c_char, value_len: usize) {
    let h = unsafe { host(h) };
    if h.dom.node(id).kind == Kind::Element {
        let name = unsafe { text(name, name_len) };
        h.restyle(id, |r, d| r.attribute_will_change(d, id, name));
        h.dom.set_attribute(id, name, unsafe { text(value, value_len) });
        h.sheet_attribute_changed(id, name);
        h.restyle(id, |r, _| r.attribute_changed(id, name));
        h.frame_op(id, FrameOp::AttrChanged(id, name.to_string()));
        h.changed();
    }
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_remove_attr(h: *mut c_void, id: u32, name: *const c_char, name_len: usize) {
    let h = unsafe { host(h) };
    if h.dom.node(id).kind == Kind::Element {
        let name = unsafe { text(name, name_len) };
        h.restyle(id, |r, d| r.attribute_will_change(d, id, name));
        h.dom.remove_attribute(id, name);
        h.sheet_attribute_changed(id, name);
        h.restyle(id, |r, _| r.attribute_changed(id, name));
        h.frame_op(id, FrameOp::AttrChanged(id, name.to_string()));
        h.changed();
    }
}

/// The first element in tree order whose `id` is `s` (a walk: Firefox keeps an id table).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_get_element_by_id(h: *mut c_void, root: u32, s: *const c_char, len: usize) -> u32 {
    let h = unsafe { host(h) };
    let want = unsafe { text(s, len) };
    if want.is_empty() || !valid(h, root) {
        return NONE;
    }
    let dom = &h.dom;
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if dom.node(id).kind == Kind::Element && dom.get_attribute(id, "id") == Some(want) {
            return id;
        }
        let first = stack.len();
        stack.extend(dom.children(id));
        stack[first..].reverse();
    }
    NONE
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_query(h: *mut c_void, root: u32, sel: *const c_char, len: usize, max: u32) -> i32 {
    let h = unsafe { host(h) };
    let dom = std::mem::take(&mut h.dom);
    let (dom, result) = query_selector(dom, root, unsafe { text(sel, len) }, max as usize);
    h.dom = dom;
    match result {
        Some(ids) => {
            h.results = ids;
            h.results.len() as i32
        }
        None => -1,
    }
}

/// `matches` (closest 0: `id` or 0) and `closest` (1: the nearest inclusive ancestor matching, or 0);
/// -1 for a selector that does not parse.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_match(h: *mut c_void, id: u32, sel: *const c_char, len: usize, closest: i32) -> i64 {
    let h = unsafe { host(h) };
    let dom = std::mem::take(&mut h.dom);
    let (dom, result) = match_selector(dom, id, unsafe { text(sel, len) }, closest != 0);
    h.dom = dom;
    result.map_or(-1, |n| n as i64)
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_result(h: *mut c_void, i: u32) -> u32 {
    let h = unsafe { host(h) };
    h.results.get(i as usize).copied().unwrap_or(NONE)
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_box(h: *mut c_void, id: u32, out: *mut f64) -> i32 {
    let h = unsafe { host(h) };
    match h.layout_box(id) {
        Some(b) => {
            unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), out, 4) };
            1
        }
        None => 0,
    }
}

/// An element's offset parent (the return value, 0 for none) and its offsetLeft, offsetTop,
/// offsetWidth and offsetHeight in `out` (zeros without a frame): Element::GetOffsetRect.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_offset_rect(h: *mut c_void, id: u32, out: *mut i32) -> u32 {
    let h = unsafe { host(h) };
    let (parent, rect) = h.offset_rect(id).unwrap_or((NONE, [0; 4]));
    unsafe { std::ptr::copy_nonoverlapping(rect.as_ptr(), out, 4) };
    if parent == NONE { 0 } else { parent }
}

/// What a ResizeObserver reads of each of the `n` elements in `ids` (Host::resize_observer_boxes): 11
/// numbers each into `out`.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_resize_boxes(h: *mut c_void, ids: *const u32, n: usize, flush: bool, out: *mut f64) {
    let h = unsafe { host(h) };
    let nodes = unsafe { std::slice::from_raw_parts(ids, n) };
    for (i, b) in h.resize_observer_boxes(nodes, flush).iter().enumerate() {
        unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), out.add(i * 11), 11) };
    }
}

/// IntersectionObserver's geometry (see Host::intersections): `margins` holds the rootMargin and
/// then the scrollMargin, each side as a value and 1 for a percentage (16 numbers); 14 numbers out per
/// target.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_intersections(h: *mut c_void, owner: u32, root: u32, margins: *const f64, ids: *const u32, n: usize, out: *mut f64) {
    let h = unsafe { host(h) };
    let m = unsafe { std::slice::from_raw_parts(margins, 16) };
    let margin = |at: usize| -> IntersectionMargin { std::array::from_fn(|k| (m[at + 2 * k] as f32, m[at + 2 * k + 1] != 0.0)) };
    let nodes = unsafe { std::slice::from_raw_parts(ids, n) };
    for (i, b) in h.intersections(owner, root, &margin(0), &margin(8), nodes).iter().enumerate() {
        unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), out.add(i * 14), 14) };
    }
}

/// `elementFromPoint` and `elementsFromPoint` of `root` (Host::nodes_from_point): the number of
/// elements, the first `cap` of them written to `out`.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_nodes_from_point(h: *mut c_void, root: u32, x: f32, y: f32, multiple: i32, out: *mut u32, cap: usize) -> usize {
    let h = unsafe { host(h) };
    let nodes = h.nodes_from_point(root, x, y, multiple != 0);
    let n = nodes.len().min(cap);
    unsafe { std::ptr::copy_nonoverlapping(nodes.as_ptr(), out, n) };
    nodes.len()
}

/// `innerText` of element `id` (Host::inner_text).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_inner_text(h: *mut c_void, id: u32, out: *mut *const c_char, len: *mut usize) {
    let h = unsafe { host(h) };
    let s = if valid(h, id) { h.inner_text(id) } else { String::new() };
    h.give(s, out, len);
}

/// An element's client area and scroll size (see `boxes`): 0 when it has no frame.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_client_scroll(h: *mut c_void, id: u32, out: *mut f64) -> i32 {
    let h = unsafe { host(h) };
    match h.layout_box(id) {
        Some(b) => {
            unsafe { std::ptr::copy_nonoverlapping(b[4..].as_ptr(), out, 4) };
            1
        }
        None => 0,
    }
}

/// The text of every stylesheet a page linked, by URL, shared by the pages of the process, as
/// Gecko's SharedStyleSheetCache shares complete sheets. Not page state (SPEC decision 13): it can be
/// fetched again; it is outside the arena and counted apart (`linked_sheets_bytes`).
static LINKED_SHEETS: LazyLock<Mutex<HashMap<String, Arc<str>>>> = LazyLock::new(Default::default);

/// The bytes of text in the linked-sheet cache, and the number of sheets.
pub fn linked_sheets_bytes() -> (usize, usize) {
    let cache = LINKED_SHEETS.lock().unwrap();
    (cache.values().map(|s| s.len()).sum(), cache.len())
}

/// A stylesheet `<link>`'s load ended: with the sheet's URL and text, its sheet (from the cache, the
/// same text shared); without, it has none (failed, or no longer loads one).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_link_sheet(h: *mut c_void, id: u32, url: *const c_char, url_len: usize, css: *const c_char, css_len: usize, loaded: i32) {
    let h = unsafe { host(h) };
    if loaded != 0 {
        let (url, css) = (unsafe { text(url, url_len) }, unsafe { text(css, css_len) });
        let mut cache = LINKED_SHEETS.lock().unwrap();
        let sheet = match cache.get(url) {
            Some(s) if &**s == css => s.clone(),
            _ => {
                let s: Arc<str> = Arc::from(css);
                cache.insert(url.to_string(), s.clone());
                s
            }
        };
        h.link_sheets.insert(id, sheet);
    } else {
        h.link_sheets.remove(&id);
    }
    // (a load makes a new sheet; what script did to the old one goes)
    h.sheet_replaced(id);
    // (the next style build sees that its sheets changed: StyleDom::attach_dom)
    h.changed();
}

#[unsafe(no_mangle)]
extern "C" fn mrt_console(h: *mut c_void, s: *const c_char, len: usize) {
    let h = unsafe { host(h) };
    let line = unsafe { text(s, len) }.to_string();
    if h.echo_console {
        eprintln!("console: {line}");
    }
    h.console.push(line);
}

#[unsafe(no_mangle)]
extern "C" fn mrt_bridge(h: *mut c_void, req: *const c_char, len: usize, out: *mut *const c_char, out_len: *mut usize) -> i32 {
    let h = unsafe { host(h) };
    if !h.bridge {
        return 0;
    }
    let req = unsafe { text(req, len) };
    let reply = h.bridge_request(req);
    h.give(reply, out, out_len);
    1
}

impl Host {
    /// One request to the driver and its reply (one JSON line each way: the request has no raw
    /// newline, JSON escapes them): the bridge's channel, for bridge.js and for realsite's own requests
    /// (the steps of a merged event loop, DESIGN "Cross-origin frames in runtimes of their own").
    pub fn bridge_request(&mut self, req: &str) -> String {
        use std::io::{BufRead, Write};
        self.bridge_calls += 1;
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(req.as_bytes()).and_then(|_| stdout.write_all(b"\n")).and_then(|_| stdout.flush()).expect("bridge: the driver went away");
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).expect("bridge: the driver went away");
        if line.is_empty() {
            panic!("bridge: the driver closed the pipe");
        }
        line.trim_end_matches('\n').to_string()
    }
}

/// `createHTMLDocument`: a new document; `has_title` 0 for no title element.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_create_html_document(h: *mut c_void, title: *const c_char, len: usize, has_title: i32) -> u32 {
    let h = unsafe { host(h) };
    let title = if has_title != 0 { Some(unsafe { text(title, len) }) } else { None };
    h.dom.create_html_document(title)
}

/// `new CSSStyleSheet()`: a constructed sheet with this text; returns its id.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_new_sheet(h: *mut c_void, s: *const c_char, len: usize) -> u32 {
    let h = unsafe { host(h) };
    h.dom.new_constructed_sheet(unsafe { text(s, len) })
}

/// A constructed sheet's new text (replace, replaceSync, insertRule, deleteRule).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_set_sheet(h: *mut c_void, id: u32, s: *const c_char, len: usize) {
    let h = unsafe { host(h) };
    h.dom.set_constructed_sheet(id, unsafe { text(s, len) });
    if h.dom.adopted_sheets.values().any(|ids| ids.contains(&id)) {
        h.changed();
    }
}

/// The CSSOM of a style sheet for bridge.js's CSSStyleSheet: the sheet of element `id` when `owner`
/// is 1, else constructed sheet `id`. `op`: 0 the element's sheet generation (`sheet_generations`,
/// from then on counted), 1 the rules as JSON (mrt_style::cssom's `rule_json`), 2 insertRule(`text`,
/// `n`), the new rule's JSON, 3 deleteRule(`n`), "", 4 set `disabled` to `n`, "", 5 `disabled` ("1"
/// or ""), 6 replaceSync(`text`) on a constructed sheet, "", 7 a new constructed sheet
/// with media list `text`, disabled when `n` is 1, its id. 8 reads/edits a rule declaration using
/// a form-encoded path/name/value and `n`'s operation/importance flags. A refused operation answers
/// "!" and the DOMException's name. The result string goes to `out`.
#[unsafe(no_mangle)]
extern "C" fn mrt_cssom_op(h: *mut c_void, op: i32, owner: i32, id: u32, s: *const c_char, len: usize, n: f64, out: *mut *const c_char, out_len: *mut usize) {
    let h = unsafe { host(h) };
    let text = unsafe { text(s, len) };
    let key = if owner != 0 { SheetKey::Owner(id) } else { SheetKey::Constructed(id) };
    let valid_key = match key {
        SheetKey::Owner(id) => valid(h, id) && h.dom.node(id).kind == Kind::Element,
        SheetKey::Constructed(id) => (id as usize) < h.dom.constructed_sheets.len() || op == 7,
    };
    if !valid_key {
        h.give(String::new(), out, out_len);
        return;
    }
    // whether a change to the sheet can change a document's style: an owner's (the style build
    // takes the sheets of connected owners) or an adopted constructed sheet's
    let applies = |h: &Host| match key {
        SheetKey::Owner(_) => true,
        SheetKey::Constructed(id) => h.dom.adopted_sheets.values().any(|ids| ids.contains(&id)),
    };
    let result = match op {
        0 => {
            let SheetKey::Owner(id) = key else { return h.give(String::new(), out, out_len) };
            h.sheet_generations.entry(id).or_insert(0).to_string()
        }
        // (parsed for the answer and not kept unless the sheet is already kept: a page that reads
        // its sheets' rules (Amazon reads 24,109 rules' cssText) would otherwise hold a second copy
        // of every sheet beside the style build's)
        1 => {
            let kept = h.cssom.contains_key(&key);
            let json = h.cssom_sheet(key).rules_json();
            if !kept {
                h.cssom.remove(&key);
            }
            json
        }
        2 | 3 => {
            let index = if n.is_finite() && n >= 0.0 && n <= u32::MAX as f64 { n as u32 } else { u32::MAX };
            let op = if op == 2 { mrt_style::mrt_dom::RuleOp::Insert(index, text.to_string()) } else { mrt_style::mrt_dom::RuleOp::Delete(index) };
            match h.cssom_sheet(key).apply(&op) {
                Err(name) => format!("!{name}"),
                Ok(json) => {
                    // the page state: the op, as the script gave it (and a link's text, with the
                    // first change: Gecko's EnsureUniqueInner copy)
                    match key {
                        SheetKey::Owner(id) => {
                            let base = (h.dom.tag(id) == "link").then(|| h.link_sheets.get(&id).map_or("", |s| s).to_string());
                            h.dom.push_owner_sheet_op(id, base.as_deref(), op);
                        }
                        SheetKey::Constructed(id) => h.dom.push_constructed_sheet_op(id, op),
                    }
                    if applies(h) {
                        h.changed();
                    }
                    json
                }
            }
        }
        4 => {
            let disabled = n != 0.0;
            match key {
                SheetKey::Owner(id) => h.dom.set_owner_sheet_disabled(id, disabled),
                SheetKey::Constructed(id) => h.dom.constructed_sheets[id as usize].disabled = disabled,
            }
            if applies(h) {
                h.changed();
            }
            String::new()
        }
        5 => {
            let disabled = match key {
                SheetKey::Owner(id) => h.dom.owner_sheet_disabled(id),
                SheetKey::Constructed(id) => h.dom.constructed_sheets[id as usize].disabled,
            };
            if disabled { "1" } else { "" }.to_string()
        }
        6 => {
            let SheetKey::Constructed(id) = key else { return h.give(String::new(), out, out_len) };
            // the text as given is the page state (its rules are parsed when read)
            h.dom.set_constructed_sheet(id, text);
            h.cssom.remove(&key);
            if applies(h) {
                h.changed();
            }
            String::new()
        }
        8 => {
            let kind = n as u8 & 15;
            if let Some(edit) = mrt_style::cssom::declaration_request(text, n as u8) {
                let kept = h.cssom.contains_key(&key);
                let answer = h.cssom_sheet(key).declaration(&edit);
                if !kept && !matches!(kind, 2 | 3 | 7) { h.cssom.remove(&key); }
                match answer {
                    Err(name) => format!("!{name}"),
                    Ok(result) => {
                        if matches!(kind, 2 | 3 | 7) {
                            let op = mrt_style::mrt_dom::RuleOp::Declaration(Box::new(edit));
                            match key {
                                SheetKey::Owner(id) => {
                                    let base = (h.dom.tag(id) == "link").then(|| h.link_sheets.get(&id).map_or("", |s| s).to_string());
                                    h.dom.push_owner_sheet_op(id, base.as_deref(), op);
                                }
                                SheetKey::Constructed(id) => h.dom.push_constructed_sheet_op(id, op),
                            }
                            if applies(h) { h.changed(); }
                        }
                        result
                    }
                }
            } else { "!IndexSizeError".into() }
        }
        7 => h.dom.new_constructed_sheet_with("", text, n != 0.0).to_string(),
        _ => String::new(),
    };
    h.give(result, out, out_len);
}

/// `adoptedStyleSheets` set on a document or a shadow root.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_set_adopted(h: *mut c_void, node: u32, ids: *const u32, n: usize) {
    let h = unsafe { host(h) };
    if !valid(h, node) {
        return;
    }
    let ids = if n == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(ids, n) } };
    h.dom.set_adopted_sheets(node, ids);
    h.changed();
}

/// DOMParser: `html` parsed as a new document; returns the document.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_parse_document(h: *mut c_void, s: *const c_char, len: usize) -> u32 {
    let h = unsafe { host(h) };
    h.dom.parse_document(unsafe { text(s, len) })
}

/// A `<template>`'s contents fragment, 0 for any other node.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_template_contents(h: *mut c_void, id: u32) -> u32 {
    let h = unsafe { host(h) };
    if valid(h, id) { h.dom.template_contents.get(&id).copied().unwrap_or(NONE) } else { NONE }
}

/// A parser-made `<script>`'s line (high 32 bits) and one-based column in its document, taken out
/// as the script runs; 0 for a script the parser did not make, or one already taken.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_take_script_position(h: *mut c_void, id: u32) -> u64 {
    let h = unsafe { host(h) };
    if !valid(h, id) {
        return 0;
    }
    h.dom.take_script_position(id).map_or(0, |(line, column)| (line as u64) << 32 | column as u64)
}

/// `attachShadow`: the shadow root, or -1 for NotSupportedError (`Dom::can_attach_shadow` and
/// `Dom::attach_shadow`). `flags` are mrt_dom's `SR_*`.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_attach_shadow(h: *mut c_void, id: u32, flags: u32) -> i64 {
    let h = unsafe { host(h) };
    if !valid(h, id) || !h.dom.can_attach_shadow(id) {
        return -1;
    }
    let r = h.dom.attach_shadow(id, flags as u8);
    h.restyle(id, |r, _| r.shadow_attached(id));
    h.frame_op(id, FrameOp::Reframe(id));
    h.changed();
    r.map_or(-1, |root| root as i64)
}

/// A shadow root's `SR_*` flags, -1 for any other node.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_shadow_flags(h: *mut c_void, id: u32) -> i32 {
    let h = unsafe { host(h) };
    if valid(h, id) && h.dom.node(id).kind == Kind::ShadowRoot { h.dom.node(id).flags as i32 } else { -1 }
}

/// The slots of a shadow tree in tree order into the result list (`mrt_dom_result`); returns the count.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_slots(h: *mut c_void, root: u32) -> i32 {
    let h = unsafe { host(h) };
    h.results = if valid(h, root) { h.dom.slots(root) } else { Vec::new() };
    h.results.len() as i32
}

/// A slot's assigned nodes into the result list (`mrt_dom_result`); returns the count.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_assigned_nodes(h: *mut c_void, slot: u32) -> i32 {
    let h = unsafe { host(h) };
    h.results = if valid(h, slot) { h.dom.assigned_nodes(slot) } else { Vec::new() };
    h.results.len() as i32
}

/// Whether `name` is a valid custom element name (`nsContentUtils::IsCustomElementName`).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_is_custom_element_name(s: *const c_char, len: usize) -> i32 {
    mrt_style::mrt_dom::is_custom_element_name(unsafe { text(s, len) }) as i32
}

/// An element's custom element state (`CE_*` in mrt-dom), -1 for any other node.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_ce_state(h: *mut c_void, id: u32) -> i32 {
    let h = unsafe { host(h) };
    if valid(h, id) && h.dom.node(id).kind == Kind::Element { h.dom.ce_state(id) as i32 } else { -1 }
}

/// Sets an element's custom element state; `:defined` follows it, so it is a change of the DOM.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_set_ce_state(h: *mut c_void, id: u32, state: i32) {
    let h = unsafe { host(h) };
    if valid(h, id) && h.dom.node(id).kind == Kind::Element {
        let bits = element_state_bits(&h.dom, id);
        h.restyle(id, |r, _| r.state_will_change(id, bits));
        h.dom.set_ce_state(id, state as u8);
        h.changed();
    }
}

/// `element.style` over Stylo's declaration block (mrt_style::cssom, as Gecko's
/// nsDOMCSSDeclaration): `op` 0 getPropertyValue, 1 getPropertyPriority, 2 setProperty, 3
/// removeProperty, 4 cssText, 5 length, 6 item (`value` is the index), 7 whether `name` is a CSS
/// property ("1" or ""), 8 `CSS.supports(name, value)`, 9 `CSS.supports(css)` as a condition (both
/// "1" or ""). The result string goes to `out`. For 2 and 3 it is the attribute's new
/// text, and the return is 0 when nothing changed (the caller reads the old value first with 0);
/// every other op returns 1.
#[unsafe(no_mangle)]
extern "C" fn mrt_style_op(
    h: *mut c_void,
    op: i32,
    css: *const c_char,
    css_len: usize,
    name: *const c_char,
    name_len: usize,
    value: *const c_char,
    value_len: usize,
    important: i32,
    out: *mut *const c_char,
    out_len: *mut usize,
) -> i32 {
    use mrt_style::cssom;
    let h = unsafe { host(h) };
    let (css, name, value) = unsafe { (text(css, css_len), text(name, name_len), text(value, value_len)) };
    let mode = h.dom.mode;
    let (result, ret) = match op {
        0 => (cssom::property_value(css, name, mode), 1),
        1 => (cssom::property_priority(css, name, mode).to_string(), 1),
        2 => match cssom::set_property(css, name, value, important != 0, mode) {
            Some(new) => (new, 1),
            None => (String::new(), 0),
        },
        3 => match cssom::remove_property(css, name, mode).1 {
            Some(new) => (new, 1),
            None => (String::new(), 0),
        },
        4 => (cssom::css_text(css, mode), 1),
        5 => (cssom::length(css, mode).to_string(), 1),
        6 => (cssom::item(css, value.parse().unwrap_or(usize::MAX), mode), 1),
        7 => (if cssom::is_property(name) { "1" } else { "" }.to_string(), 1),
        8 => (if cssom::supports_declaration(name, value) { "1" } else { "" }.to_string(), 1),
        9 => (if cssom::supports_condition(css) { "1" } else { "" }.to_string(), 1),
        _ => (String::new(), 0),
    };
    h.give(result, out, out_len);
    ret
}

/// `createComment` (kind 3) and `createDocumentFragment` (kind 5).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_create_node(h: *mut c_void, kind: i32, s: *const c_char, len: usize) -> u32 {
    let h = unsafe { host(h) };
    match kind {
        3 => h.dom.create_comment(unsafe { text(s, len) }),
        _ => h.dom.create_fragment(),
    }
}

#[unsafe(no_mangle)]
extern "C" fn mrt_dom_clone(h: *mut c_void, id: u32, deep: i32) -> u32 {
    let h = unsafe { host(h) };
    if !valid(h, id) {
        return NONE;
    }
    h.dom.clone_node(id, deep != 0)
}

/// Markup parsed into a new fragment in the context of element `context` (0: a `<body>`).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_parse_fragment(h: *mut c_void, s: *const c_char, len: usize, context: u32) -> u32 {
    let h = unsafe { host(h) };
    let context = if valid(h, context) { context } else { NONE };
    h.dom.parse_fragment(unsafe { text(s, len) }, context)
}

/// The attribute names of an element, one per line (names have no whitespace).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_attr_names(h: *mut c_void, id: u32, out: *mut *const c_char, len: *mut usize) {
    let h = unsafe { host(h) };
    let names = if valid(h, id) { h.dom.attribute_names(id).join("\n") } else { String::new() };
    h.give(names, out, len);
}

/// `getComputedStyle`'s declaration (prelude.js): op 0 the value of the property `name` of element
/// `id` with pseudo-element `pseudo` ("" for none; Gecko's parse: `::before`, `:before`, `::after`,
/// `:after`, `::marker`; any other gives an empty style); op 1 `length`; op 2 the name at index
/// `name` (`item`). Returns 1.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_computed_style(
    h: *mut c_void,
    id: u32,
    op: i32,
    pseudo: *const c_char,
    pseudo_len: usize,
    name: *const c_char,
    name_len: usize,
    out: *mut *const c_char,
    out_len: *mut usize,
) -> i32 {
    use mrt_style::style::selector_parser::PseudoElement;
    let h = unsafe { host(h) };
    let (pseudo, name) = unsafe { (text(pseudo, pseudo_len), text(name, name_len)) };
    let names = mrt_layout::ComputedPropertyNames();
    let result = match op {
        0 => {
            let pseudo = match pseudo.to_ascii_lowercase().as_str() {
                "" => Ok(None),
                "::before" | ":before" => Ok(Some(PseudoElement::Before)),
                "::after" | ":after" => Ok(Some(PseudoElement::After)),
                "::marker" => Ok(Some(PseudoElement::Marker)),
                _ => Err(()),
            };
            match pseudo {
                Ok(pseudo) if valid(h, id) => h.computed_value(id, pseudo, name),
                _ => String::new(),
            }
        }
        1 => names.len().to_string(),
        2 => name.parse::<usize>().ok().and_then(|i| names.get(i)).map(|n| n.to_string()).unwrap_or_default(),
        _ => String::new(),
    };
    h.give(result, out, out_len);
    1
}

/// A node's document (Gecko's `nsINode::OwnerDoc`), 0 for a node id out of range.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_node_document(h: *mut c_void, id: u32) -> u32 {
    let h = unsafe { host(h) };
    if !valid(h, id) { return 0; }
    h.dom.node_document(id)
}

/// A node just made by document `doc` belongs to it.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_set_node_document(h: *mut c_void, id: u32, doc: u32) {
    let h = unsafe { host(h) };
    if valid(h, id) && valid(h, doc) {
        h.dom.set_node_document(id, doc);
    }
}

/// `importNode`'s copy and `adoptNode`'s node, with their subtrees, into document `doc`.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_adopt(h: *mut c_void, id: u32, doc: u32) {
    let h = unsafe { host(h) };
    if valid(h, id) && valid(h, doc) {
        h.dom.adopt(id, doc);
    }
}

/// A document's compatibility mode (0 no quirks, 1 limited quirks, 2 quirks).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_document_mode(h: *mut c_void, doc: u32) -> i32 {
    let h = unsafe { host(h) };
    if !valid(h, doc) { return 0; }
    h.dom.document_mode(doc)
}

/// A frame's initial about:blank document (mrt_dom::Dom::create_blank_document).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_create_blank_document(h: *mut c_void) -> u32 {
    let h = unsafe { host(h) };
    h.dom.create_blank_document()
}

/// A frame's document parsed from its HTML, scripting on (mrt_dom::Dom::parse_frame_document).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_parse_frame_document(h: *mut c_void, html: *const c_char, len: usize) -> u32 {
    let h = unsafe { host(h) };
    let html = unsafe { text(html, len) };
    h.dom.parse_frame_document(html)
}

/// Set a document's compatibility mode (document.write's doctype).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_set_document_mode(h: *mut c_void, doc: u32, mode: i32) {
    let h = unsafe { host(h) };
    if !valid(h, doc) { return; }
    if doc == mrt_style::mrt_dom::DOCUMENT { h.dom.mode = mode; } else { h.dom.document_modes.insert(doc, mode); }
}

/// A frame window's document was made (`frame` its frame element, or 0 for a root window laid out at
/// `width` by `height`): it gets its layout (DESIGN, "The frames' layout (S5)").
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_window_opened(h: *mut c_void, doc: u32, frame: u32, width: i32, height: i32) {
    let h = unsafe { host(h) };
    h.window_opened(doc, if frame == 0 { NONE } else { frame }, (width, height));
}

/// A frame window got a new document (Host::window_document_changed).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_window_document_changed(h: *mut c_void, old_doc: u32, doc: u32) {
    let h = unsafe { host(h) };
    h.window_document_changed(old_doc, doc);
}

/// A frame window closed: its document's layout goes.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_window_closed(h: *mut c_void, doc: u32) {
    let h = unsafe { host(h) };
    h.window_closed(doc);
}

/// An element scroll container's offset in CSS px (Host::set_element_scroll).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_set_element_scroll(h: *mut c_void, id: u32, x: f64, y: f64) {
    let h = unsafe { host(h) };
    if valid(h, id) {
        h.set_element_scroll(id, x, y);
    }
}

/// The page's root scroll offset in CSS px (Host::set_root_scroll).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_set_root_scroll(h: *mut c_void, x: f64, y: f64) {
    let h = unsafe { host(h) };
    h.set_root_scroll(x, y);
}

/// The viewport of document `doc` (Host::viewport_of) in `out`: 1, or 0 when it has none.
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_viewport(h: *mut c_void, doc: u32, out: *mut i32) -> i32 {
    let h = unsafe { host(h) };
    match h.viewport_of(doc) {
        Some((width, height)) => {
            unsafe {
                *out = width;
                *out.add(1) = height;
            }
            1
        }
        None => 0,
    }
}

/// The refresh tick's state for the frames: `op` 0 whether a resize is pending, 1 the DOM's generation,
/// 2 flush the documents that have frames (Host::flush_frame_parents), 3 take the documents whose size
/// changed into `out` (at most `max`; returns how many).
#[unsafe(no_mangle)]
extern "C" fn mrt_dom_refresh(h: *mut c_void, op: i32, out: *mut u32, max: usize) -> f64 {
    let h = unsafe { host(h) };
    match op {
        0 => h.resizes_pending() as i32 as f64,
        1 => h.generation() as f64,
        2 => {
            h.flush_frame_parents();
            0.0
        }
        3 => {
            let docs = h.take_resizes();
            let n = docs.len().min(max);
            unsafe { std::ptr::copy_nonoverlapping(docs.as_ptr(), out, n) };
            n as f64
        }
        _ => 0.0,
    }
}
