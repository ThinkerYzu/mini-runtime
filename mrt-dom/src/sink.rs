//! The `MrtSink` callbacks: Firefox's tree builder calls these and they fill a `Dom`.
//! The C declaration is in `parser/shim/mrt_parser.h`.

use crate::{Attr, Dom, Kind, Node, NodeId, DOCUMENT, NONE};
use std::ffi::c_void;

#[repr(C)]
struct MrtAttr {
    ns: i32,
    name: *const u16,
    name_len: u32,
    value: *const u16,
    value_len: u32,
}

#[repr(C)]
struct MrtSink {
    ctx: *mut c_void,
    create_element: extern "C" fn(*mut c_void, i32, *const u16, u32, *const MrtAttr, u32, u32) -> u32,
    create_root: extern "C" fn(*mut c_void, *const u16, u32, *const MrtAttr, u32) -> u32,
    append_child: extern "C" fn(*mut c_void, u32, u32),
    append_children_to_new_parent: extern "C" fn(*mut c_void, u32, u32),
    detach_from_parent: extern "C" fn(*mut c_void, u32),
    insert_foster_parented: extern "C" fn(*mut c_void, u32, u32, u32),
    append_text: extern "C" fn(*mut c_void, u32, *const u16, u32),
    insert_foster_text: extern "C" fn(*mut c_void, *const u16, u32, u32, u32),
    append_comment: extern "C" fn(*mut c_void, u32, *const u16, u32),
    add_attributes: extern "C" fn(*mut c_void, u32, *const MrtAttr, u32),
    append_doctype: extern "C" fn(*mut c_void, *const u16, u32, *const u16, u32, *const u16, u32),
    set_document_mode: extern "C" fn(*mut c_void, i32),
    has_children: extern "C" fn(*mut c_void, u32) -> i32,
    template_contents: extern "C" fn(*mut c_void, u32) -> u32,
    attach_shadow_root: extern "C" fn(*mut c_void, u32, u32, u32) -> u32,
    set_script_position: extern "C" fn(*mut c_void, u32, i32, i32),
}

#[repr(C)]
struct MrtParser {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn mrt_parser_init();
    fn mrt_parse_document(sink: *const MrtSink, text: *const u16, len: u32, declarative_shadow_roots: i32, scripting_enabled: i32) -> *mut MrtParser;
    fn mrt_parse_fragment(
        sink: *const MrtSink,
        text: *const u16,
        len: u32,
        scripting_enabled: i32,
        context_name: *const u16,
        context_len: u32,
        context_ns: i32,
        quirks: i32,
    ) -> *mut MrtParser;
    fn mrt_parser_begin_document(sink: *const MrtSink, text: *const u16, len: u32) -> *mut MrtParser;
    fn mrt_parser_resume(p: *mut MrtParser, ctx: *mut c_void) -> u32;
    fn mrt_parser_free(p: *mut MrtParser);
}

/// What a resumed document parse tells the page about each change it makes to the tree, as Gecko's
/// tree operations notify (nsHtml5TreeOperation::Append and the others): the node is already in the
/// DOM when `inserted` and `text_changed` are called, and still in it when `will_remove` is. The
/// methods must not touch the parser's DOM borrow: they get the node only.
pub trait ParserObserver {
    /// `node` was put into a parent (a new node, or one moved by the adoption agency).
    fn inserted(&mut self, node: NodeId);
    /// `node` is about to leave its parent.
    fn will_remove(&mut self, node: NodeId);
    /// The parser added text at the end of text node `node`.
    fn text_changed(&mut self, node: NodeId);
    /// The parser gave element `node` attributes it did not have (a second `<html>` or `<body>` tag).
    fn attributes_added(&mut self, node: NodeId, names: &[String]);
}

/// The sink's context: the DOM the parser fills, and who is told of each change.
struct Ctx {
    dom: *mut Dom,
    observer: Option<*mut dyn ParserObserver>,
}

/// A document parsed at its scripts' pace, as a document loaded in a tab whose scripts run (scripting
/// on, declarative shadow roots attached): `resume` parses until a `<script>` is popped, which the
/// caller runs before resuming (Gecko's nsHtml5TreeOpExecutor runs it, then the parser goes on).
pub struct DocumentParser {
    parser: *mut MrtParser,
    /// (the C++ tree builder keeps a copy of it; held for as long as the parser, as the context is)
    _sink: Box<MrtSink>,
    ctx: Box<Ctx>,
    done: bool,
}

impl DocumentParser {
    pub fn new(html: &str) -> DocumentParser {
        let units: Vec<u16> = html.encode_utf16().collect();
        let mut ctx = Box::new(Ctx { dom: std::ptr::null_mut(), observer: None });
        let sink = Box::new(make_sink(&mut *ctx as *mut Ctx as *mut c_void));
        let parser = unsafe {
            mrt_parser_init();
            mrt_parser_begin_document(&*sink, units.as_ptr(), units.len() as u32)
        };
        DocumentParser { parser, _sink: sink, ctx, done: false }
    }

    /// Parse on into `dom` (the page's, which must stay where it is until this returns), telling
    /// `observer` of every change: the `<script>` element popped, or None once the document is parsed.
    pub fn resume(&mut self, dom: &mut Dom, observer: Option<&mut dyn ParserObserver>) -> Option<NodeId> {
        if self.done {
            return None;
        }
        self.ctx.dom = dom as *mut Dom;
        // (the observer outlives the call; the lifetime is erased for the raw pointer)
        self.ctx.observer = observer.map(|o| unsafe {
            std::mem::transmute::<*mut (dyn ParserObserver + '_), *mut (dyn ParserObserver + 'static)>(o as *mut dyn ParserObserver)
        });
        let script = unsafe { mrt_parser_resume(self.parser, &mut *self.ctx as *mut Ctx as *mut c_void) };
        self.ctx.dom = std::ptr::null_mut();
        self.ctx.observer = None;
        if script == NONE {
            self.done = true;
            None
        } else {
            Some(script)
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }
}

impl Drop for DocumentParser {
    fn drop(&mut self) {
        unsafe { mrt_parser_free(self.parser) };
    }
}

fn make_sink(ctx: *mut c_void) -> MrtSink {
    MrtSink {
        ctx,
        create_element,
        create_root,
        append_child,
        append_children_to_new_parent,
        detach_from_parent,
        insert_foster_parented,
        append_text,
        insert_foster_text,
        append_comment,
        add_attributes,
        append_doctype,
        set_document_mode,
        has_children,
        template_contents,
        attach_shadow_root,
        set_script_position,
    }
}

/// `declarative_shadow_roots`: whether `<template shadowrootmode>` attaches a shadow root, as for a
/// document loaded in a tab (`Document::AllowsDeclarativeShadowRoots`); `innerHTML` does not.
/// `scripting_enabled`: the tree builder's scripting flag (`nsHtml5TreeBuilder::setScriptingEnabled`):
/// on, `<noscript>` is parsed as raw text, as in a document whose scripts run and in every fragment.
pub fn parse(html: &str, declarative_shadow_roots: bool, scripting_enabled: bool) -> Dom {
    parse_with(html, declarative_shadow_roots, scripting_enabled, None)
}

/// A fragment parsed in the context of an element with this local name and namespace (the parser's
/// numbers: 3 HTML, 5 MathML, 6 SVG), in quirks mode or not, with scripting on (innerHTML's
/// nsContentUtils::ParseFragmentHTML). The fragment's nodes are the children of the returned DOM's
/// root element.
pub fn parse_fragment(html: &str, context: &str, context_ns: i32, quirks: bool) -> Dom {
    parse_with(html, false, true, Some((context, context_ns, quirks)))
}

fn parse_with(html: &str, declarative_shadow_roots: bool, scripting_enabled: bool, context: Option<(&str, i32, bool)>) -> Dom {
    let units: Vec<u16> = html.encode_utf16().collect();
    let mut dom = Dom::new();
    let mut ctx = Ctx { dom: &mut dom as *mut Dom, observer: None };
    let sink = make_sink(&mut ctx as *mut Ctx as *mut c_void);
    unsafe {
        mrt_parser_init();
        let p = match context {
            None => mrt_parse_document(&sink, units.as_ptr(), units.len() as u32, declarative_shadow_roots as i32, scripting_enabled as i32),
            Some((name, ns, quirks)) => {
                let name: Vec<u16> = name.encode_utf16().collect();
                mrt_parse_fragment(&sink, units.as_ptr(), units.len() as u32, scripting_enabled as i32, name.as_ptr(), name.len() as u32, ns, quirks as i32)
            }
        };
        mrt_parser_free(p);
    }
    dom
}

unsafe fn dom<'a>(ctx: *mut c_void) -> &'a mut Dom {
    unsafe { &mut *(*(ctx as *mut Ctx)).dom }
}

/// The observer of a resumed document parse, if any; called after the DOM borrow of the callback
/// has ended.
fn notify(ctx: *mut c_void, f: impl FnOnce(&mut dyn ParserObserver)) {
    if let Some(o) = unsafe { (*(ctx as *mut Ctx)).observer } {
        f(unsafe { &mut *o });
    }
}

unsafe fn units<'a>(p: *const u16, len: u32) -> &'a [u16] {
    if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(p, len as usize) } }
}

/// The parser uses 0 for the document where the DOM uses 1.
fn parent_or_document(id: u32) -> NodeId {
    if id == NONE { DOCUMENT } else { id }
}

/// Converts the parser's attributes, storing values in the text arena.
unsafe fn convert_attrs(d: &mut Dom, attrs: *const MrtAttr, n: u32) -> Vec<Attr> {
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n as usize {
        let a = unsafe { &*attrs.add(i) };
        let name = String::from_utf16_lossy(unsafe { units(a.name, a.name_len) });
        let (value_off, value_len) = d.push_text(unsafe { units(a.value, a.value_len) });
        out.push(Attr { name: d.names.intern(&name), value_off, value_len, ns: a.ns as u8 });
    }
    out
}

unsafe fn make_element(d: &mut Dom, ns: i32, name: *const u16, name_len: u32, attrs: *const MrtAttr, n: u32) -> NodeId {
    let tag = String::from_utf16_lossy(unsafe { units(name, name_len) });
    let attrs = unsafe { convert_attrs(d, attrs, n) };
    let name = d.names.intern(&tag);
    let id = d.add_node(Node {
        kind: Kind::Element,
        ns: ns as u8,
        name,
        data: d.attrs.len() as u32,
        len: attrs.len() as u32,
        ..d.nodes[0]
    });
    d.attrs.extend(attrs);
    d.init_ce_state(id);
    id
}

extern "C" fn create_element(ctx: *mut c_void, ns: i32, name: *const u16, name_len: u32, attrs: *const MrtAttr, n: u32, _form: u32) -> u32 {
    let d = unsafe { dom(ctx) };
    let id = unsafe { make_element(d, ns, name, name_len, attrs, n) };
    // A <template> gets its own contents node, as the DOM spec has it.
    if ns == crate::NS_HTML as i32 && d.tag(id) == "template" {
        let frag = d.add_node(Node { kind: Kind::Fragment, ..d.nodes[0] });
        d.template_contents.insert(id, frag);
    }
    id
}

extern "C" fn create_root(ctx: *mut c_void, name: *const u16, name_len: u32, attrs: *const MrtAttr, n: u32) -> u32 {
    let d = unsafe { dom(ctx) };
    let id = unsafe { make_element(d, crate::NS_HTML as i32, name, name_len, attrs, n) };
    d.append(id, DOCUMENT);
    notify(ctx, |o| o.inserted(id));
    id
}

extern "C" fn append_child(ctx: *mut c_void, child: u32, parent: u32) {
    unsafe { dom(ctx) }.append(child, parent_or_document(parent));
    notify(ctx, |o| o.inserted(child));
}

extern "C" fn append_children_to_new_parent(ctx: *mut c_void, old: u32, new: u32) {
    let d = unsafe { dom(ctx) };
    let kids: Vec<NodeId> = d.children(old).collect();
    // (nsHtml5TreeOperation::AppendChildrenToNewParent: each child removed, then appended, notified)
    for k in kids {
        notify(ctx, |o| o.will_remove(k));
        unsafe { dom(ctx) }.append(k, new);
        notify(ctx, |o| o.inserted(k));
    }
}

extern "C" fn detach_from_parent(ctx: *mut c_void, node: u32) {
    if unsafe { dom(ctx) }.node(node).parent != NONE {
        notify(ctx, |o| o.will_remove(node));
    }
    unsafe { dom(ctx) }.detach(node);
}

extern "C" fn insert_foster_parented(ctx: *mut c_void, child: u32, table: u32, stack_parent: u32) {
    let d = unsafe { dom(ctx) };
    if d.node(table).parent != NONE {
        d.insert_before(child, table);
    } else {
        d.append(child, stack_parent);
    }
    notify(ctx, |o| o.inserted(child));
}

extern "C" fn append_text(ctx: *mut c_void, parent: u32, text: *const u16, len: u32) {
    let d = unsafe { dom(ctx) };
    let parent = parent_or_document(parent);
    let last = d.node(parent).last_child;
    let merged = last != NONE && d.node(last).kind == Kind::Text;
    unsafe { d.append_text(parent, units(text, len)) };
    let node = d.node(parent).last_child;
    text_notify(ctx, node, merged);
}

/// The observer's call for text the parser added: to an existing text node, or as a new one.
fn text_notify(ctx: *mut c_void, node: NodeId, merged: bool) {
    if merged {
        notify(ctx, |o| o.text_changed(node));
    } else {
        notify(ctx, |o| o.inserted(node));
    }
}

extern "C" fn insert_foster_text(ctx: *mut c_void, text: *const u16, len: u32, table: u32, stack_parent: u32) {
    let d = unsafe { dom(ctx) };
    if d.node(table).parent != NONE {
        let prev = d.node(table).prev_sibling;
        let merged = prev != NONE && d.node(prev).kind == Kind::Text;
        unsafe { d.insert_text_before(table, units(text, len)) };
        let node = d.node(table).prev_sibling;
        text_notify(ctx, node, merged);
    } else {
        let last = d.node(stack_parent).last_child;
        let merged = last != NONE && d.node(last).kind == Kind::Text;
        unsafe { d.append_text(stack_parent, units(text, len)) };
        let node = d.node(stack_parent).last_child;
        text_notify(ctx, node, merged);
    }
}

extern "C" fn append_comment(ctx: *mut c_void, parent: u32, text: *const u16, len: u32) {
    let d = unsafe { dom(ctx) };
    let (off, n) = d.push_text(unsafe { units(text, len) });
    let id = d.add_node(Node { kind: Kind::Comment, data: off, len: n, ..d.nodes[0] });
    d.append(id, parent_or_document(parent));
    notify(ctx, |o| o.inserted(id));
}

extern "C" fn add_attributes(ctx: *mut c_void, node: u32, attrs: *const MrtAttr, n: u32) {
    let d = unsafe { dom(ctx) };
    let attrs = unsafe { convert_attrs(d, attrs, n) };
    let names: Vec<String> = attrs
        .iter()
        .filter(|a| !d.attrs_of(node).iter().any(|e| e.name == a.name && e.ns == a.ns))
        .map(|a| d.names.get(a.name).to_string())
        .collect();
    d.add_attributes(node, &attrs);
    if !names.is_empty() {
        notify(ctx, |o| o.attributes_added(node, &names));
    }
}

// The public and system ids are only used to pick the document mode, which the parser already did.
extern "C" fn append_doctype(ctx: *mut c_void, name: *const u16, name_len: u32, _p: *const u16, _pl: u32, _s: *const u16, _sl: u32) {
    let d = unsafe { dom(ctx) };
    let name = String::from_utf16_lossy(unsafe { units(name, name_len) });
    let name = d.names.intern(&name);
    let id = d.add_node(Node { kind: Kind::Doctype, name, ..d.nodes[0] });
    d.append(id, DOCUMENT);
    notify(ctx, |o| o.inserted(id));
}

extern "C" fn set_document_mode(ctx: *mut c_void, mode: i32) {
    unsafe { dom(ctx) }.mode = mode;
}

extern "C" fn has_children(ctx: *mut c_void, node: u32) -> i32 {
    (unsafe { dom(ctx) }.node(node).first_child != NONE) as i32
}

extern "C" fn template_contents(ctx: *mut c_void, template: u32) -> u32 {
    *unsafe { dom(ctx) }.template_contents.get(&template).unwrap_or(&template)
}

/// The tree builder's `getShadowRootFromHost` and `setDocumentFragmentForTemplate` in one: the
/// template's children go into the shadow root, and the template itself is never inserted.
extern "C" fn attach_shadow_root(ctx: *mut c_void, host: u32, template: u32, flags: u32) -> u32 {
    let d = unsafe { dom(ctx) };
    let root = d.attach_declarative_shadow(host, flags as u8);
    if root != NONE {
        d.template_contents.insert(template, root);
    }
    root
}

/// A `<script>`'s line and column in the document (`Dom::script_positions`).
extern "C" fn set_script_position(ctx: *mut c_void, script: u32, line: i32, column: i32) {
    unsafe { dom(ctx) }.script_positions.insert(script, (line.max(1) as u32, column.max(1) as u32));
}
