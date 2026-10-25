//! The arena DOM: a page's tree kept in a few flat arrays, with 32-bit node ids instead of pointers.
//! Firefox's HTML parser builds it through the `MrtSink` callbacks in `sink.rs`.

mod sink;
pub use sink::{DocumentParser, ParserObserver};

use std::borrow::Cow;
use std::collections::HashMap;

/// Node id. 0 means "no node"; 1 is the document.
pub type NodeId = u32;
pub const NONE: NodeId = 0;
pub const DOCUMENT: NodeId = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Kind {
    Document = 0,
    Element = 1,
    Text = 2,
    Comment = 3,
    Doctype = 4,
    /// Holds the contents of a `<template>`; never a child of anything.
    Fragment = 5,
    /// A shadow root (Gecko's `ShadowRoot`, a `DocumentFragment`): no parent; `Node::data` is its
    /// host and `Node::flags` its `SR_*` options. The host finds it through `Dom::shadow_roots`.
    ShadowRoot = 6,
}

/// Namespace numbers match the parser's (`MrtAttr::ns`).
pub const NS_HTML: u8 = 3;
/// Attribute namespaces the HTML parser sets on foreign attributes (the shim's numbers).
pub const NS_XMLNS: u8 = 1;
pub const NS_XML: u8 = 2;
pub const NS_XLINK: u8 = 4;

/// An element's custom element state, Gecko's `CustomElementData::State`, in the low three bits of
/// `Node::flags`. Uncustomized (0) is every element whose name is not a valid custom element name;
/// an HTML element whose name is one starts undefined and becomes custom when it is upgraded.
pub const CE_UNCUSTOMIZED: u8 = 0;
pub const CE_UNDEFINED: u8 = 1;
pub const CE_FAILED: u8 = 2;
pub const CE_PRECUSTOMIZED: u8 = 3;
pub const CE_CUSTOM: u8 = 4;
const CE_STATE_MASK: u8 = 0b111;

/// A shadow root's options, in `Node::flags` (`ShadowRootInit` and the root's declarative bit).
pub const SR_CLOSED: u8 = 1;
pub const SR_CLONABLE: u8 = 2;
pub const SR_SERIALIZABLE: u8 = 4;
pub const SR_DELEGATES_FOCUS: u8 = 8;
/// `slotAssignment: "manual"`; named otherwise.
pub const SR_MANUAL_SLOTS: u8 = 16;
/// Attached by the parser from `<template shadowrootmode>` and not yet taken over by
/// `attachShadow` (`ShadowRoot::IsDeclarative`).
pub const SR_DECLARATIVE: u8 = 32;

/// The local names `attachShadow` accepts besides custom element names
/// (`nsContentUtils::IsValidShadowHostName`).
pub fn is_valid_shadow_host_name(name: &str) -> bool {
    is_custom_element_name(name)
        || matches!(
            name,
            "article" | "aside" | "blockquote" | "body" | "div" | "footer" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "header" | "main" | "nav" | "p" | "section" | "span"
        )
}

/// Whether `name` matches PotentialCustomElementName: an ASCII lower alpha first, a dash, no
/// ASCII upper alpha, nothing that cannot be in an element's local name (`nsContentUtils::IsNameWithDash`).
pub fn is_name_with_dash(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() || !b[0].is_ascii_lowercase() {
        return false;
    }
    let mut has_dash = false;
    for &c in &b[1..] {
        if c.is_ascii_uppercase() || matches!(c, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ' | b'/' | b'>') {
            return false;
        }
        if c == b'-' {
            has_dash = true;
        }
    }
    has_dash
}

/// A valid custom element name in the HTML namespace (`nsContentUtils::IsCustomElementName`): a
/// name with a dash, except the eight names SVG and MathML already use.
pub fn is_custom_element_name(name: &str) -> bool {
    is_name_with_dash(name)
        && !matches!(
            name,
            "annotation-xml" | "color-profile" | "font-face" | "font-face-src" | "font-face-uri" | "font-face-format" | "font-face-name" | "missing-glyph"
        )
}

/// 36 bytes. Nothing here points at heap memory.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Node {
    pub kind: Kind,
    pub flags: u8,
    pub ns: u8,
    /// Elements: interned tag name. Doctypes: interned doctype name.
    pub name: u32,
    pub parent: NodeId,
    pub first_child: NodeId,
    pub last_child: NodeId,
    pub next_sibling: NodeId,
    pub prev_sibling: NodeId,
    /// Elements: index of the first attribute. Text and comments: offset into the text arena.
    pub data: u32,
    /// Elements: attribute count. Text and comments: byte length.
    pub len: u32,
}

/// An attribute: interned name, value in the text arena. 16 bytes.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Attr {
    pub name: u32,
    pub value_off: u32,
    pub value_len: u32,
    pub ns: u8,
}

#[derive(Default, Clone)]
pub struct Names {
    list: Vec<Box<str>>,
    index: HashMap<Box<str>, u32>,
}

impl Names {
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.index.get(s) {
            return id;
        }
        let id = self.list.len() as u32;
        self.list.push(s.into());
        self.index.insert(s.into(), id);
        id
    }
    pub fn get(&self, id: u32) -> &str {
        &self.list[id as usize]
    }
    pub fn len(&self) -> usize {
        self.list.len()
    }
}

/// `Clone` is for a layout flush while script runs (mrt-js), which lays out a copy.
#[derive(Clone)]
pub struct Dom {
    pub nodes: Vec<Node>,
    pub attrs: Vec<Attr>,
    /// UTF-8 bytes of all text, comment and attribute values.
    pub text: Vec<u8>,
    pub names: Names,
    /// `<template>` element id to the fragment node that holds its contents.
    pub template_contents: HashMap<NodeId, NodeId>,
    /// Shadow host to its shadow root (Gecko keeps it on the host, `Element::GetShadowRoot`); the
    /// root keeps its host in `Node::data`.
    pub shadow_roots: HashMap<NodeId, NodeId>,
    /// 0 no-quirks, 1 almost-standards, 2 quirks.
    pub mode: i32,
    /// Constructed style sheets (`new CSSStyleSheet()`, Gecko's StyleSheet with a constructor
    /// document), by sheet id. A page's script owns them; they are page state.
    pub constructed_sheets: Vec<ConstructedSheet>,
    /// What script made of the sheet of a `<style>` or stylesheet `<link>` element that its text no
    /// longer says, by element: the rules after an `insertRule` or `deleteRule`, and the `disabled`
    /// flag. Page state (SPEC decision 13); an element whose sheet is untouched has no entry.
    pub owner_sheets: HashMap<NodeId, OwnerSheet>,
    /// `adoptedStyleSheets` of the document (`DOCUMENT`) and of shadow roots, as sheet ids in order
    /// (Gecko's DocumentOrShadowRoot::mAdoptedStyleSheets).
    pub adopted_sheets: HashMap<NodeId, Vec<u32>>,
    /// The node document of each node that does not belong to the page's document (`DOCUMENT`): the
    /// nodes of a frame's document, of `createHTMLDocument` and `DOMParser` documents, and the nodes
    /// they made (Gecko's `nsINode::OwnerDoc`, a pointer in every node). The page's nodes, nearly
    /// all of them, have no entry. A document is its own node document.
    pub node_documents: HashMap<NodeId, NodeId>,
    /// The compatibility mode of each document other than the page's (`mode` is the page's): a
    /// frame's initial `about:blank` is in quirks mode, a parsed one has its doctype's.
    pub document_modes: HashMap<NodeId, i32>,
    /// A parser-made `<script>`'s place in its document's source, the line and the one-based column
    /// its text starts at (Gecko's `nsIScriptElement::mLineNumber` and `mColumnNumber`), until the
    /// script runs and takes it (`take_script_position`). Not kept through hibernation: by then the
    /// page's scripts have run.
    pub script_positions: HashMap<NodeId, (u32, u32)>,
}

/// A change a script made to a sheet's rules through the CSSOM, kept as the script gave it: replayed
/// in order over the sheet's text, each insert parsed in the context of the rules before it, it
/// gives the rules Gecko's sheet has. (A rule's serialization is not kept instead: it does not
/// always parse back to the same rule; `font: var(--x); font-weight: bold` serializes the other
/// font longhands with empty values.)
#[derive(Clone, Debug, PartialEq)]
pub enum RuleOp {
    /// `insertRule(text, index)` that succeeded.
    Insert(u32, String),
    /// `deleteRule(index)` that succeeded.
    Delete(u32),
    /// A declaration edit on a style rule, addressed after the preceding edits.
    Declaration(Box<DeclarationOp>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeclarationOp {
    pub path: Vec<u32>,
    pub kind: u8,
    pub name: String,
    pub value: String,
    pub important: bool,
}

/// A constructed style sheet: the text it was last given (`replace`, `replaceSync`), the changes
/// made to its rules since, the media list it was made with (`new CSSStyleSheet({media})`) and
/// whether it is disabled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConstructedSheet {
    pub text: String,
    pub ops: Vec<RuleOp>,
    pub media: String,
    pub disabled: bool,
}

/// The CSSOM's state of a `<style>` or `<link>` element's sheet (Gecko keeps it in the element's
/// StyleSheet object, which the mini runtime does not keep).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OwnerSheet {
    /// The changes a script made to the sheet's rules, in order (Gecko's StyleSheet::EnsureUniqueInner
    /// copies the rules out of the sheet the text made before the first change, so from then on
    /// the text does not say what the sheet holds).
    pub ops: Vec<RuleOp>,
    /// A link's sheet text once a script changed its rules: the ops apply to it, and a link's text
    /// is otherwise not page state (SPEC decision 13). None for a `<style>`, whose own text it is.
    pub base: Option<String>,
    pub disabled: bool,
}

/// A subtree outside the arenas (`Dom::snapshot`), for cloning and for moving nodes between DOMs.
enum Owned {
    /// `contents`: a `<template>`'s contents, copied with it in a deep copy (as Gecko's
    /// HTMLTemplateElement clones them).
    /// `shadow`: a clonable shadow root's options and children, copied with its host whether the
    /// copy is deep or not (the DOM's "clone a node", step 7).
    Element { ns: u8, tag: String, attrs: Vec<(String, String, u8)>, children: Vec<Owned>, contents: Vec<Owned>, shadow: Option<(u8, Vec<Owned>)> },
    Text(String),
    Comment(String),
    Fragment(Vec<Owned>),
}

fn empty_node(kind: Kind) -> Node {
    Node {
        kind,
        flags: 0,
        ns: 0,
        name: 0,
        parent: NONE,
        first_child: NONE,
        last_child: NONE,
        next_sibling: NONE,
        prev_sibling: NONE,
        data: 0,
        len: 0,
    }
}

impl Dom {
    pub fn new() -> Dom {
        // Index 0 is the "no node" placeholder, index 1 the document.
        Dom {
            nodes: vec![empty_node(Kind::Document), empty_node(Kind::Document)],
            attrs: Vec::new(),
            text: Vec::new(),
            names: Names::default(),
            template_contents: HashMap::new(),
            shadow_roots: HashMap::new(),
            mode: 0,
            constructed_sheets: Vec::new(),
            owner_sheets: HashMap::new(),
            adopted_sheets: HashMap::new(),
            node_documents: HashMap::new(),
            document_modes: HashMap::new(),
            script_positions: HashMap::new(),
        }
    }

    /// The compatibility mode of document `doc` (0 no-quirks, 1 almost-standards, 2 quirks).
    pub fn document_mode(&self, doc: NodeId) -> i32 {
        if doc == DOCUMENT { self.mode } else { self.document_modes.get(&doc).copied().unwrap_or(0) }
    }

    /// A frame's initial document: `about:blank` as Gecko's docshell makes it (nsDocShell's initial
    /// document, `<html><head></head><body></body></html>` with no doctype, so in quirks mode). Not
    /// connected to the page's document; its nodes belong to it.
    pub fn create_blank_document(&mut self) -> NodeId {
        let doc = self.add_node(empty_node(Kind::Document));
        self.document_modes.insert(doc, 2);
        let html = self.create_element("html");
        self.insert_child(doc, html, NONE);
        let head = self.create_element("head");
        self.insert_child(html, head, NONE);
        let body = self.create_element("body");
        self.insert_child(html, body, NONE);
        doc
    }

    /// A frame's document from its HTML (a `src` or `srcdoc` load): parsed as a document whose
    /// scripts run (scripting on, declarative shadow roots attached) into a new document of this
    /// arena, with its compatibility mode.
    pub fn parse_frame_document(&mut self, html: &str) -> NodeId {
        let tmp = sink::parse(html, true, true);
        self.adopt_parsed_document(tmp)
    }

    /// The node document of `id` (Gecko's `OwnerDoc`): itself for a document, else the document that
    /// made it or that it was adopted into.
    pub fn node_document(&self, id: NodeId) -> NodeId {
        if self.node(id).kind == Kind::Document {
            return id;
        }
        self.node_documents.get(&id).copied().unwrap_or(DOCUMENT)
    }

    /// Make `doc` the node document of `id` alone (a node just made by `doc`).
    pub fn set_node_document(&mut self, id: NodeId, doc: NodeId) {
        if doc == DOCUMENT {
            self.node_documents.remove(&id);
        } else {
            self.node_documents.insert(id, doc);
        }
    }

    /// Adopt `root` and everything under it, its template contents and shadow trees included, into
    /// `doc` (the DOM's "adopt", from Gecko's `nsNodeUtils::Adopt`; the reflectors keep their
    /// prototypes, as Gecko's do between same-origin windows).
    pub fn adopt(&mut self, root: NodeId, doc: NodeId) {
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            if self.node(n).kind == Kind::Document {
                continue;
            }
            self.set_node_document(n, doc);
            stack.extend(self.children(n));
            if let Some(&contents) = self.template_contents.get(&n) {
                // a template's contents stay in the template's own inert document in Gecko; here
                // they follow the template
                stack.push(contents);
            }
            if let Some(&shadow) = self.shadow_roots.get(&n) {
                stack.push(shadow);
            }
        }
    }

    /// Parse a whole document with Firefox's HTML parser, as a document loaded in a tab is
    /// (declarative shadow roots attached).
    pub fn parse(html: &str) -> Dom {
        let mut dom = sink::parse(html, true, false);
        dom.shrink_to_fit();
        dom
    }

    /// Like `parse`, for a document whose scripts run: the parser's scripting flag on, so `<noscript>`
    /// holds its content as text (Gecko's nsHtml5StreamParser sets it from the executor's
    /// `IsScriptEnabled`).
    pub fn parse_with_scripting(html: &str) -> Dom {
        let mut dom = sink::parse(html, true, true);
        dom.shrink_to_fit();
        dom
    }

    /// Like `parse`, with `<template shadowrootmode>` left a plain template (Firefox's `innerHTML`
    /// and `DOMParser` without `parseHTMLUnsafe`).
    pub fn parse_without_shadow_roots(html: &str) -> Dom {
        let mut dom = sink::parse(html, false, false);
        dom.shrink_to_fit();
        dom
    }

    /// Vec growth leaves up to half of each array unused; a finished page does not need it. Called
    /// after parsing, and by mrt-js after the page's scripts and timers have run.
    pub fn shrink_to_fit(&mut self) {
        self.nodes.shrink_to_fit();
        self.attrs.shrink_to_fit();
        self.text.shrink_to_fit();
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut next = self.node(id).first_child;
        std::iter::from_fn(move || {
            if next == NONE {
                return None;
            }
            let cur = next;
            next = self.node(cur).next_sibling;
            Some(cur)
        })
    }

    pub fn tag(&self, id: NodeId) -> &str {
        self.names.get(self.node(id).name)
    }

    pub fn text_of(&self, id: NodeId) -> &str {
        let n = self.node(id);
        std::str::from_utf8(&self.text[n.data as usize..(n.data + n.len) as usize]).unwrap()
    }

    pub fn attrs_of(&self, id: NodeId) -> &[Attr] {
        let n = self.node(id);
        &self.attrs[n.data as usize..(n.data + n.len) as usize]
    }

    pub fn attr_value(&self, a: &Attr) -> &str {
        std::str::from_utf8(&self.text[a.value_off as usize..(a.value_off + a.value_len) as usize]).unwrap()
    }

    // ---- building ----

    pub(crate) fn add_node(&mut self, node: Node) -> NodeId {
        self.nodes.push(node);
        (self.nodes.len() - 1) as NodeId
    }

    /// Store UTF-16 as UTF-8 at the end of the text arena; returns (offset, length).
    pub(crate) fn push_text(&mut self, units: &[u16]) -> (u32, u32) {
        let off = self.text.len();
        for c in char::decode_utf16(units.iter().copied()) {
            let c = c.unwrap_or('\u{FFFD}');
            let mut buf = [0u8; 4];
            self.text.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        (off as u32, (self.text.len() - off) as u32)
    }

    pub(crate) fn detach(&mut self, id: NodeId) {
        let Node { parent, prev_sibling: prev, next_sibling: next, .. } = *self.node(id);
        if parent == NONE {
            return;
        }
        if prev != NONE {
            self.nodes[prev as usize].next_sibling = next;
        } else {
            self.nodes[parent as usize].first_child = next;
        }
        if next != NONE {
            self.nodes[next as usize].prev_sibling = prev;
        } else {
            self.nodes[parent as usize].last_child = prev;
        }
        let n = &mut self.nodes[id as usize];
        n.parent = NONE;
        n.prev_sibling = NONE;
        n.next_sibling = NONE;
    }

    pub(crate) fn append(&mut self, child: NodeId, parent: NodeId) {
        self.detach(child);
        let last = self.node(parent).last_child;
        {
            let c = &mut self.nodes[child as usize];
            c.parent = parent;
            c.prev_sibling = last;
        }
        if last != NONE {
            self.nodes[last as usize].next_sibling = child;
        } else {
            self.nodes[parent as usize].first_child = child;
        }
        self.nodes[parent as usize].last_child = child;
    }

    /// Put `child` just before `before`, which must have a parent.
    pub(crate) fn insert_before(&mut self, child: NodeId, before: NodeId) {
        self.detach(child);
        let parent = self.node(before).parent;
        let prev = self.node(before).prev_sibling;
        {
            let c = &mut self.nodes[child as usize];
            c.parent = parent;
            c.prev_sibling = prev;
            c.next_sibling = before;
        }
        self.nodes[before as usize].prev_sibling = child;
        if prev != NONE {
            self.nodes[prev as usize].next_sibling = child;
        } else {
            self.nodes[parent as usize].first_child = child;
        }
    }

    /// Add text at the end of `prev` (a text node): in place when its bytes are the last in the
    /// arena, otherwise by moving them to the end first.
    pub(crate) fn extend_text(&mut self, prev: NodeId, units: &[u16]) {
        let n = *self.node(prev);
        if (n.data + n.len) as usize != self.text.len() {
            let start = n.data as usize;
            let end = start + n.len as usize;
            let new_off = self.text.len();
            self.text.extend_from_within(start..end);
            self.nodes[prev as usize].data = new_off as u32;
        }
        let (_, added) = self.push_text(units);
        self.nodes[prev as usize].len += added;
    }

    pub(crate) fn new_text(&mut self, units: &[u16]) -> NodeId {
        let (off, len) = self.push_text(units);
        let mut n = empty_node(Kind::Text);
        n.data = off;
        n.len = len;
        self.add_node(n)
    }

    /// Append text under `parent`, merging into a trailing text node as the DOM does.
    pub(crate) fn append_text(&mut self, parent: NodeId, units: &[u16]) {
        let last = self.node(parent).last_child;
        if last != NONE && self.node(last).kind == Kind::Text {
            self.extend_text(last, units);
        } else {
            let t = self.new_text(units);
            self.append(t, parent);
        }
    }

    /// Text that belongs before `table` (foster parenting), merging with a text node just before it.
    pub(crate) fn insert_text_before(&mut self, table: NodeId, units: &[u16]) {
        let prev = self.node(table).prev_sibling;
        if prev != NONE && self.node(prev).kind == Kind::Text {
            self.extend_text(prev, units);
        } else {
            let t = self.new_text(units);
            self.insert_before(t, table);
        }
    }

    /// Add attributes to an element, skipping names it already has.
    pub(crate) fn add_attributes(&mut self, id: NodeId, new: &[Attr]) {
        let n = *self.node(id);
        let mut fresh: Vec<Attr> = Vec::new();
        for a in new {
            let exists = self.attrs_of(id).iter().any(|e| e.name == a.name && e.ns == a.ns);
            if !exists && !fresh.iter().any(|e| e.name == a.name && e.ns == a.ns) {
                fresh.push(*a);
            }
        }
        if fresh.is_empty() {
            return;
        }
        // The attributes of one element must stay contiguous; if another element's come after,
        // move this element's to the end (this happens only for repeated <html> or <body> tags).
        if (n.data + n.len) as usize != self.attrs.len() {
            let start = n.data as usize;
            let end = start + n.len as usize;
            let new_start = self.attrs.len();
            self.attrs.extend_from_within(start..end);
            self.nodes[id as usize].data = new_start as u32;
        }
        self.attrs.extend_from_slice(&fresh);
        self.nodes[id as usize].len += fresh.len() as u32;
    }

    // ---- changes made by script (stage 3) ----
    //
    // Nothing here frees a node or arena bytes: a removed node stays in `nodes`, and replaced text or
    // moved attributes leave their old bytes behind. Node ids therefore stay valid for as long as the
    // `Dom` lives, which is what lets a JS reflector hold one.

    /// `document.createElement(tag)`: an HTML element with no attributes and no parent. The caller
    /// lowercases the name, as `Document::CreateElement` does for HTML documents.
    pub fn create_element(&mut self, tag: &str) -> NodeId {
        self.create_element_ns(NS_HTML, tag)
    }

    /// `document.createElementNS(namespace, qualifiedName)`: an element in namespace `ns` (the
    /// parser's numbers) with local name `local`; the prefix is not kept.
    pub fn create_element_ns(&mut self, ns: u8, local: &str) -> NodeId {
        let mut n = empty_node(Kind::Element);
        n.ns = ns;
        n.name = self.names.intern(local);
        n.data = self.attrs.len() as u32;
        let id = self.add_node(n);
        self.init_ce_state(id);
        if ns == NS_HTML && local == "template" {
            self.add_template_contents(id);
        }
        id
    }

    /// A `<template>`'s contents node, made with the element as the DOM spec has it (the parser's sink
    /// does the same for parsed templates).
    pub(crate) fn add_template_contents(&mut self, template: NodeId) -> NodeId {
        let frag = self.add_node(empty_node(Kind::Fragment));
        self.template_contents.insert(template, frag);
        frag
    }

    /// Sets an element's custom element state as it is created: undefined for an HTML element with
    /// a valid custom element name (`nsContentUtils::NewXULOrHTMLElement`, step 6.3), uncustomized
    /// for any other. Call once the namespace is set.
    pub(crate) fn init_ce_state(&mut self, id: NodeId) {
        let n = &self.nodes[id as usize];
        let state = if n.ns == NS_HTML && is_custom_element_name(self.names.get(n.name)) { CE_UNDEFINED } else { CE_UNCUSTOMIZED };
        self.set_ce_state(id, state);
    }

    /// The custom element state of an element (`CE_*`).
    pub fn ce_state(&self, id: NodeId) -> u8 {
        self.nodes[id as usize].flags & CE_STATE_MASK
    }

    pub fn set_ce_state(&mut self, id: NodeId, state: u8) {
        let f = &mut self.nodes[id as usize].flags;
        *f = (*f & !CE_STATE_MASK) | (state & CE_STATE_MASK);
    }

    /// Gecko's `ElementState::DEFINED` (`:defined`): set on every element except a custom element
    /// that is not yet upgraded or failed to upgrade.
    pub fn is_defined(&self, id: NodeId) -> bool {
        matches!(self.ce_state(id), CE_UNCUSTOMIZED | CE_CUSTOM)
    }

    /// `document.createTextNode(text)`.
    pub fn create_text(&mut self, text: &str) -> NodeId {
        let off = self.text.len() as u32;
        self.text.extend_from_slice(text.as_bytes());
        let mut n = empty_node(Kind::Text);
        n.data = off;
        n.len = text.len() as u32;
        self.add_node(n)
    }

    /// `document.createComment(text)`.
    pub fn create_comment(&mut self, text: &str) -> NodeId {
        let id = self.create_text(text);
        self.nodes[id as usize].kind = Kind::Comment;
        id
    }

    /// `document.implementation.createHTMLDocument(title)`: another document in this arena, with the
    /// skeleton Gecko's DOMImplementation::CreateHTMLDocument gives it (a doctype, html, head, a title
    /// if one is given, body). Its nodes are not connected: only node 1 is the page's document.
    pub fn create_html_document(&mut self, title: Option<&str>) -> NodeId {
        let doc = self.add_node(empty_node(Kind::Document));
        let mut doctype = empty_node(Kind::Doctype);
        doctype.name = self.names.intern("html");
        let doctype = self.add_node(doctype);
        self.insert_child(doc, doctype, NONE);
        let html = self.create_element("html");
        self.insert_child(doc, html, NONE);
        let head = self.create_element("head");
        self.insert_child(html, head, NONE);
        if let Some(title) = title {
            let t = self.create_element("title");
            self.insert_child(head, t, NONE);
            let text = self.create_text(title);
            self.insert_child(t, text, NONE);
        }
        let body = self.create_element("body");
        self.insert_child(html, body, NONE);
        doc
    }

    /// `DOMParser.parseFromString(html, "text/html")`: `html` parsed as a whole document into a new
    /// document of this arena, as Gecko's DOMParser does: scripting off (scripts are not run, a
    /// `<noscript>` is parsed as elements) and no declarative shadow roots. Not connected.
    pub fn parse_document(&mut self, html: &str) -> NodeId {
        let tmp = Dom::parse_without_shadow_roots(html);
        self.adopt_parsed_document(tmp)
    }

    /// The document `tmp` parsed, copied into a new document node of this arena.
    fn adopt_parsed_document(&mut self, tmp: Dom) -> NodeId {
        let doc = self.add_node(empty_node(Kind::Document));
        self.document_modes.insert(doc, tmp.mode);
        for c in tmp.children(DOCUMENT).collect::<Vec<_>>() {
            let id = if tmp.node(c).kind == Kind::Doctype {
                let mut doctype = empty_node(Kind::Doctype);
                doctype.name = self.names.intern(tmp.names.get(tmp.node(c).name));
                self.add_node(doctype)
            } else {
                let owned = tmp.snapshot(c, true);
                let id = self.build(&owned);
                self.copy_script_positions(&tmp, c, id);
                id
            };
            self.insert_child(doc, id, NONE);
        }
        doc
    }

    /// The script positions of `from` and its descendants in `tmp`, given to their copies under `to`
    /// (`build` made them in the same order).
    fn copy_script_positions(&mut self, tmp: &Dom, from: NodeId, to: NodeId) {
        if tmp.script_positions.is_empty() {
            return;
        }
        let mut stack = vec![(from, to)];
        while let Some((a, b)) = stack.pop() {
            if let Some(&pos) = tmp.script_positions.get(&a) {
                self.script_positions.insert(b, pos);
            }
            stack.extend(tmp.children(a).zip(self.children(b)));
        }
    }

    /// The place in its document's source of a parser-made `<script>` that has not run yet, taken
    /// out (a script runs once): its line and one-based column, or None for any other script.
    pub fn take_script_position(&mut self, script: NodeId) -> Option<(u32, u32)> {
        self.script_positions.remove(&script)
    }

    // ---- constructed and adopted style sheets ----

    /// `new CSSStyleSheet()`: a sheet with this text, enabled, with no media list; returns its id.
    pub fn new_constructed_sheet(&mut self, css: &str) -> u32 {
        self.new_constructed_sheet_with(css, "", false)
    }

    /// `new CSSStyleSheet({media, disabled})`: as `new_constructed_sheet`, with the options.
    pub fn new_constructed_sheet_with(&mut self, css: &str, media: &str, disabled: bool) -> u32 {
        self.constructed_sheets.push(ConstructedSheet { text: css.to_string(), ops: Vec::new(), media: media.to_string(), disabled });
        (self.constructed_sheets.len() - 1) as u32
    }

    /// `replace` and `replaceSync` on a constructed sheet: its new text, and no changes since.
    pub fn set_constructed_sheet(&mut self, id: u32, css: &str) {
        if let Some(s) = self.constructed_sheets.get_mut(id as usize) {
            s.text.clear();
            s.text.push_str(css);
            s.ops.clear();
        }
    }

    /// What a script did to the sheet of `<style>` or `<link>` element `owner`, if anything.
    pub fn owner_sheet(&self, owner: NodeId) -> Option<&OwnerSheet> {
        self.owner_sheets.get(&owner)
    }

    /// A change to the rules of `owner`'s sheet; `base` is a link's sheet text, kept with the first
    /// change.
    pub fn push_owner_sheet_op(&mut self, owner: NodeId, base: Option<&str>, op: RuleOp) {
        let entry = self.owner_sheets.entry(owner).or_default();
        if entry.ops.is_empty() && entry.base.is_none() {
            entry.base = base.map(str::to_string);
        }
        entry.ops.push(op);
    }

    /// A change to the rules of constructed sheet `id`.
    pub fn push_constructed_sheet_op(&mut self, id: u32, op: RuleOp) {
        if let Some(s) = self.constructed_sheets.get_mut(id as usize) {
            s.ops.push(op);
        }
    }

    /// Whether a script disabled `owner`'s sheet (`sheet.disabled = true`).
    pub fn owner_sheet_disabled(&self, owner: NodeId) -> bool {
        self.owner_sheets.get(&owner).is_some_and(|s| s.disabled)
    }

    pub fn set_owner_sheet_disabled(&mut self, owner: NodeId, disabled: bool) {
        let entry = self.owner_sheets.entry(owner).or_default();
        entry.disabled = disabled;
        if *entry == OwnerSheet::default() {
            self.owner_sheets.remove(&owner);
        }
    }

    /// `owner` got a new sheet (Gecko's LinkStyle::UpdateStyleSheet made one: its text or media
    /// changed, it left the document, its link loaded again): what script did to the old one goes.
    /// Whether it had any.
    pub fn drop_owner_sheet(&mut self, owner: NodeId) -> bool {
        self.owner_sheets.remove(&owner).is_some()
    }

    /// Setting `adoptedStyleSheets` on the document or a shadow root. Ids out of range are dropped.
    pub fn set_adopted_sheets(&mut self, node: NodeId, ids: &[u32]) {
        let ids: Vec<u32> = ids.iter().copied().filter(|&i| (i as usize) < self.constructed_sheets.len()).collect();
        if ids.is_empty() {
            self.adopted_sheets.remove(&node);
        } else {
            self.adopted_sheets.insert(node, ids);
        }
    }

    /// The sheets a document or shadow root adopted, in order.
    pub fn adopted_sheets(&self, node: NodeId) -> &[u32] {
        self.adopted_sheets.get(&node).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// `document.createDocumentFragment()`.
    pub fn create_fragment(&mut self) -> NodeId {
        self.add_node(empty_node(Kind::Fragment))
    }

    // ---- shadow trees ----

    /// A shadow root for `host`, with no checks (Gecko's `AttachShadowWithoutNameChecks`).
    fn new_shadow_root(&mut self, host: NodeId, flags: u8) -> NodeId {
        let mut n = empty_node(Kind::ShadowRoot);
        n.flags = flags;
        n.data = host;
        let root = self.add_node(n);
        self.shadow_roots.insert(host, root);
        root
    }

    /// The checks of `attachShadow` that the DOM can make (`Element::CanAttachShadowDOM`, steps 1
    /// and 2): an HTML element with a valid shadow host name. A custom element definition's
    /// `disabledFeatures: ["shadow"]` is the JS layer's to check.
    pub fn can_attach_shadow(&self, host: NodeId) -> bool {
        let n = self.node(host);
        n.kind == Kind::Element && n.ns == NS_HTML && is_valid_shadow_host_name(self.tag(host))
    }

    /// `element.attachShadow(init)` after `can_attach_shadow` (`Element::AttachShadow`, step 4 on):
    /// a host whose root is declarative and of the same mode gets that root back emptied and no longer
    /// declarative; any other host throws NotSupportedError. `flags` are `SR_*` without
    /// `SR_DECLARATIVE`.
    pub fn attach_shadow(&mut self, host: NodeId, flags: u8) -> Result<NodeId, &'static str> {
        if let Some(&root) = self.shadow_roots.get(&host) {
            let f = self.node(root).flags;
            if f & SR_DECLARATIVE == 0 || (f & SR_CLOSED) != (flags & SR_CLOSED) {
                return Err("NotSupportedError");
            }
            while self.node(root).first_child != NONE {
                let c = self.node(root).first_child;
                self.detach(c);
            }
            self.nodes[root as usize].flags = f & !SR_DECLARATIVE;
            return Ok(root);
        }
        Ok(self.new_shadow_root(host, flags & !SR_DECLARATIVE))
    }

    /// The parser's declarative shadow root (`nsContentUtils::AttachDeclarativeShadowRoot`): none
    /// when the host cannot have one or already has one.
    pub(crate) fn attach_declarative_shadow(&mut self, host: NodeId, flags: u8) -> NodeId {
        if !self.can_attach_shadow(host) || self.shadow_roots.contains_key(&host) {
            return NONE;
        }
        self.new_shadow_root(host, flags | SR_DECLARATIVE)
    }

    /// An element's shadow root, or `NONE`.
    pub fn shadow_root(&self, host: NodeId) -> NodeId {
        self.shadow_roots.get(&host).copied().unwrap_or(NONE)
    }

    /// A shadow root's host, `NONE` for any other node.
    pub fn host(&self, root: NodeId) -> NodeId {
        let n = self.node(root);
        if n.kind == Kind::ShadowRoot { n.data } else { NONE }
    }

    /// The root of `node`'s tree (`getRootNode()`): the document, a shadow root, or a node with no
    /// parent.
    pub fn tree_root(&self, mut node: NodeId) -> NodeId {
        while self.node(node).parent != NONE {
            node = self.node(node).parent;
        }
        node
    }

    /// The shadow root `node` is in, or `NONE` (`nsINode::GetContainingShadow`).
    pub fn containing_shadow(&self, node: NodeId) -> NodeId {
        let root = self.tree_root(node);
        if self.node(root).kind == Kind::ShadowRoot { root } else { NONE }
    }

    /// The root of `node`'s tree after leaving every shadow tree through its host
    /// (`getRootNode({composed: true})`, `SubtreeRootOrShadowIncludingRoot`).
    pub fn shadow_including_root(&self, node: NodeId) -> NodeId {
        let mut root = self.tree_root(node);
        while self.node(root).kind == Kind::ShadowRoot {
            root = self.tree_root(self.node(root).data);
        }
        root
    }

    /// Whether `node` is in document `doc`'s tree: its shadow-including root is `doc` (a frame's
    /// document is laid out on its own, DESIGN "The frames' layout (S5)").
    pub fn is_connected_to(&self, node: NodeId, doc: NodeId) -> bool {
        self.shadow_including_root(node) == doc
    }

    /// `isConnected`: the shadow-including root is the page's document.
    pub fn is_connected(&self, node: NodeId) -> bool {
        self.shadow_including_root(node) == DOCUMENT
    }

    /// The parent in the shadow-including tree: a node's parent, a shadow root's host.
    pub fn shadow_including_parent(&self, node: NodeId) -> NodeId {
        let n = self.node(node);
        if n.kind == Kind::ShadowRoot { n.data } else { n.parent }
    }

    /// `node` and every node below it in shadow-including tree order: a host's shadow root comes
    /// right after the host, before its children (the DOM's shadow-including preorder).
    pub fn shadow_including_descendants(&self, node: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![node];
        while let Some(id) = stack.pop() {
            out.push(id);
            let first = stack.len();
            if let Some(&root) = self.shadow_roots.get(&id) {
                stack.push(root);
            }
            stack.extend(self.children(id));
            stack[first..].reverse();
        }
        out
    }

    /// A slottable's slot name (`ShadowRoot::GetSlotNameFor`): an element's `slot` attribute, the
    /// empty string for text.
    pub fn slot_name(&self, node: NodeId) -> &str {
        match self.node(node).kind {
            Kind::Element => self.get_attribute(node, "slot").unwrap_or(""),
            _ => "",
        }
    }

    /// Whether `node` is a `<slot>` (an HTML `slot` element).
    pub fn is_slot(&self, node: NodeId) -> bool {
        let n = self.node(node);
        n.kind == Kind::Element && n.ns == NS_HTML && self.tag(node) == "slot"
    }

    /// A slot's name: its `name` attribute (`HTMLSlotElement::GetName`).
    fn slot_element_name(&self, slot: NodeId) -> &str {
        self.get_attribute(slot, "name").unwrap_or("")
    }

    /// The slots of a shadow tree in tree order, not entering nested shadow trees (Gecko's
    /// `ShadowRoot::mSlotMap` lists them per name in this order).
    pub fn slots(&self, root: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.children(root).collect();
        stack.reverse();
        while let Some(id) = stack.pop() {
            if self.is_slot(id) {
                out.push(id);
            }
            let first = stack.len();
            stack.extend(self.children(id));
            stack[first..].reverse();
        }
        out
    }

    /// The slot `node` is assigned to, or `NONE` (`assignedSlot`, before the closed-root check;
    /// `ShadowRoot::SlotInsertionPointFor`): for a child of a shadow host, with named slot assignment,
    /// the first slot of the host's shadow tree whose name is the child's slot name. Only elements
    /// and text are slottable. Manual assignment is not built (GAPS).
    pub fn assigned_slot(&self, node: NodeId) -> NodeId {
        if !matches!(self.node(node).kind, Kind::Element | Kind::Text) {
            return NONE;
        }
        let root = self.shadow_root(self.node(node).parent);
        if root == NONE || self.node(root).flags & SR_MANUAL_SLOTS != 0 {
            return NONE;
        }
        let name = self.slot_name(node);
        self.slots(root).into_iter().find(|&s| self.slot_element_name(s) == name).unwrap_or(NONE)
    }

    /// A slot's assigned nodes in tree order (`HTMLSlotElement::AssignedNodes`, not flattened).
    pub fn assigned_nodes(&self, slot: NodeId) -> Vec<NodeId> {
        let root = self.containing_shadow(slot);
        if root == NONE || !self.is_slot(slot) {
            return Vec::new();
        }
        let host = self.node(root).data;
        self.children(host).filter(|&c| self.assigned_slot(c) == slot).collect()
    }

    /// The children in the flattened tree (Gecko's `FlattenedChildIterator`): a host's are its shadow
    /// root's children; a slot's are its assigned nodes, or its own children when it has none; any
    /// other node's are its children.
    pub fn flattened_children(&self, node: NodeId) -> Vec<NodeId> {
        if let Some(&root) = self.shadow_roots.get(&node) {
            return self.children(root).collect();
        }
        if self.is_slot(node) && self.containing_shadow(node) != NONE {
            let assigned = self.assigned_nodes(node);
            if !assigned.is_empty() {
                return assigned;
            }
        }
        self.children(node).collect()
    }

    /// The parent in the flattened tree (`nsINode::GetFlattenedTreeParentNode`): a slotted node's
    /// slot; `NONE` for a host's child that no slot takes (it is not in the flattened tree); a shadow
    /// root's children's parent is the host.
    pub fn flattened_parent(&self, node: NodeId) -> NodeId {
        let parent = self.node(node).parent;
        if parent == NONE {
            return NONE;
        }
        if self.shadow_roots.contains_key(&parent) {
            return self.assigned_slot(node);
        }
        match self.node(parent).kind {
            Kind::ShadowRoot => self.node(parent).data,
            _ if self.is_slot(parent) && self.containing_shadow(parent) != NONE && !self.assigned_nodes(parent).is_empty() => NONE,
            _ => parent,
        }
    }

    /// `element.getAttributeNames()`, in the element's order.
    pub fn attribute_names(&self, id: NodeId) -> Vec<Cow<'_, str>> {
        if self.node(id).kind != Kind::Element {
            return Vec::new();
        }
        self.attrs_of(id).iter().map(|a| self.attr_qualified_name(a)).collect()
    }

    /// An attribute's qualified name. The shim passes local names only, but the HTML parser's
    /// prefixes are fixed by namespace and local name (nsHtml5AttributeName's prefix tables:
    /// `xlink:href`, `xml:lang`, `xmlns`, `xmlns:xlink`), and script-set attributes have no
    /// namespace (GAPS B15), so the prefix follows from the pair.
    pub fn attr_qualified_name(&self, a: &Attr) -> Cow<'_, str> {
        let local = self.names.get(a.name);
        match a.ns {
            NS_XLINK => Cow::Owned(format!("xlink:{local}")),
            NS_XML => Cow::Owned(format!("xml:{local}")),
            NS_XMLNS if local != "xmlns" => Cow::Owned(format!("xmlns:{local}")),
            _ => Cow::Borrowed(local),
        }
    }

    /// The index in `attrs` of the element's first attribute whose qualified name is `name`, in any
    /// namespace (AttrArray::GetAttr(const nsAString&), GetExistingAttrNameFromQName).
    fn find_attr_by_qualified_name(&self, id: NodeId, name: &str) -> Option<usize> {
        let n = self.node(id);
        (n.data as usize..(n.data + n.len) as usize).find(|&i| self.attr_qualified_name(&self.attrs[i]) == name)
    }

    /// `node.cloneNode(deep)`: a copy with no parent; a deep copy of a `<template>` copies its contents.
    pub fn clone_node(&mut self, id: NodeId, deep: bool) -> NodeId {
        let owned = self.snapshot(id, deep);
        let copy = self.build(&owned);
        // the copy belongs to the original's document (nsNodeUtils::CloneNodeImpl)
        let doc = self.node_document(id);
        if doc != DOCUMENT && self.node(copy).kind != Kind::Document {
            self.adopt(copy, doc);
        }
        copy
    }

    /// `innerHTML`, `outerHTML` and `insertAdjacentHTML`: `html` parsed in the context of `context`
    /// (an element of this DOM, or NONE for a `<body>`), as nsContentUtils::ParseFragmentHTML does:
    /// the context's local name and namespace set the tokenizer's state and the insertion mode (raw
    /// text in a `<style>`, rows in a `<tbody>`), the document's quirks mode is kept. The result is a
    /// new fragment of this DOM. Scripts in it do not run. Scripting is on, so a `<noscript>` holds
    /// text, as nsHtml5StringParser::ParseFragment parses every fragment.
    pub fn parse_fragment(&mut self, html: &str, context: NodeId) -> NodeId {
        let (name, ns) = if context != NONE && self.node(context).kind == Kind::Element {
            (self.tag(context).to_string(), self.node(context).ns as i32)
        } else {
            ("body".to_string(), NS_HTML as i32)
        };
        // eCompatibility_NavQuirks: quirks mode, not almost-standards
        let quirks = self.document_mode(self.node_document(if context != NONE { context } else { DOCUMENT })) == 2;
        let mut tmp = sink::parse_fragment(html, &name, ns, quirks);
        tmp.shrink_to_fit();
        let frag = self.create_fragment();
        let root = tmp.children(DOCUMENT).find(|&c| tmp.node(c).kind == Kind::Element);
        if let Some(root) = root {
            for c in tmp.children(root).collect::<Vec<_>>() {
                let owned = tmp.snapshot(c, true);
                let id = self.build(&owned);
                self.insert_child(frag, id, NONE);
            }
        }
        frag
    }

    /// A node and (with `deep`) its subtree, out of the arenas, so it can be built again here or in
    /// another `Dom`.
    fn snapshot(&self, id: NodeId, deep: bool) -> Owned {
        let n = self.node(id);
        let children = if deep { self.children(id).map(|c| self.snapshot(c, true)).collect() } else { Vec::new() };
        match n.kind {
            Kind::Element => Owned::Element {
                ns: n.ns,
                tag: self.tag(id).to_string(),
                attrs: self.attrs_of(id).iter().map(|a| (self.names.get(a.name).to_string(), self.attr_value(a).to_string(), a.ns)).collect(),
                children,
                contents: match self.template_contents.get(&id) {
                    Some(&frag) if deep => self.children(frag).map(|c| self.snapshot(c, true)).collect(),
                    _ => Vec::new(),
                },
                shadow: match self.shadow_roots.get(&id) {
                    Some(&root) if self.node(root).flags & SR_CLONABLE != 0 => {
                        Some((self.node(root).flags, self.children(root).map(|c| self.snapshot(c, true)).collect()))
                    }
                    _ => None,
                },
            },
            Kind::Text => Owned::Text(self.text_of(id).to_string()),
            Kind::Comment => Owned::Comment(self.text_of(id).to_string()),
            _ => Owned::Fragment(children),
        }
    }

    fn build(&mut self, o: &Owned) -> NodeId {
        match o {
            Owned::Element { ns, tag, attrs, children, contents, shadow } => {
                let id = self.create_element(tag);
                self.nodes[id as usize].ns = *ns;
                self.init_ce_state(id);
                if *ns != NS_HTML {
                    // only an HTML <template> has contents (create_element assumed HTML)
                    self.template_contents.remove(&id);
                }
                if let Some(&frag) = self.template_contents.get(&id) {
                    for c in contents {
                        let cid = self.build(c);
                        self.insert_child(frag, cid, NONE);
                    }
                }
                let list: Vec<Attr> = attrs
                    .iter()
                    .map(|(name, value, ans)| {
                        let off = self.text.len() as u32;
                        self.text.extend_from_slice(value.as_bytes());
                        Attr { name: self.names.intern(name), value_off: off, value_len: value.len() as u32, ns: *ans }
                    })
                    .collect();
                if !list.is_empty() {
                    self.add_attributes(id, &list);
                }
                if let Some((flags, shadow_children)) = shadow {
                    let root = self.new_shadow_root(id, *flags);
                    for c in shadow_children {
                        let cid = self.build(c);
                        self.insert_child(root, cid, NONE);
                    }
                }
                for c in children {
                    let cid = self.build(c);
                    self.insert_child(id, cid, NONE);
                }
                id
            }
            Owned::Text(t) => self.create_text(t),
            Owned::Comment(t) => self.create_comment(t),
            Owned::Fragment(children) => {
                let id = self.create_fragment();
                for c in children {
                    let cid = self.build(c);
                    self.insert_child(id, cid, NONE);
                }
                id
            }
        }
    }

    /// Whether `node` is `ancestor` or below it.
    pub fn is_inclusive_descendant(&self, mut node: NodeId, ancestor: NodeId) -> bool {
        while node != NONE {
            if node == ancestor {
                return true;
            }
            node = self.node(node).parent;
        }
        false
    }

    /// Whether `node` is `ancestor` or below it, going from a shadow root to its host
    /// (`nsContentUtils::ContentIsHostIncludingDescendantOf`; template contents are not crossed here).
    pub fn is_host_including_inclusive_descendant(&self, mut node: NodeId, ancestor: NodeId) -> bool {
        while node != NONE {
            if node == ancestor {
                return true;
            }
            node = self.shadow_including_parent(node);
        }
        false
    }

    /// The checks of the DOM's "ensure pre-insertion validity", reduced to what can go wrong here:
    /// `Err` with the `DOMException` name to throw.
    pub fn check_insert(&self, parent: NodeId, child: NodeId, before: NodeId) -> Result<(), &'static str> {
        if !matches!(self.node(parent).kind, Kind::Document | Kind::Element | Kind::Fragment | Kind::ShadowRoot) {
            return Err("HierarchyRequestError");
        }
        if self.is_host_including_inclusive_descendant(parent, child) || self.node(child).kind == Kind::Document {
            return Err("HierarchyRequestError");
        }
        if before != NONE && self.node(before).parent != parent {
            return Err("NotFoundError");
        }
        Ok(())
    }

    /// `parent.insertBefore(child, before)`, or `appendChild` when `before` is `NONE`. Check with
    /// `check_insert` first.
    pub fn insert_child(&mut self, parent: NodeId, child: NodeId, before: NodeId) {
        // a node inserted under another document's node is adopted into it first (nsINode::InsertChildBefore)
        let doc = self.node_document(parent);
        if self.node_document(child) != doc {
            self.adopt(child, doc);
        }
        if before == NONE {
            self.append(child, parent);
        } else if before != child {
            self.insert_before(child, before);
        }
    }

    /// `parent.removeChild(child)`; false when `child` is not a child of `parent`.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        if self.node(child).parent != parent || parent == NONE {
            return false;
        }
        self.detach(child);
        true
    }

    /// The value of the attribute whose qualified name is `name`, if the element has one
    /// (Element::GetAttribute; the caller lowercases `name` for an HTML element).
    pub fn get_attribute(&self, id: NodeId, name: &str) -> Option<&str> {
        self.find_attr_by_qualified_name(id, name).map(|i| self.attr_value(&self.attrs[i]))
    }

    /// `element.setAttribute(name, value)`: replaces the value in place, or adds the attribute at the
    /// end of the element's list (moving the list to the end of `attrs` when another element's
    /// attributes follow it).
    pub fn set_attribute(&mut self, id: NodeId, name: &str, value: &str) {
        let off = self.text.len() as u32;
        self.text.extend_from_slice(value.as_bytes());
        // an existing attribute of that qualified name keeps its namespace (Element::SetAttribute)
        if let Some(i) = self.find_attr_by_qualified_name(id, name) {
            self.attrs[i].value_off = off;
            self.attrs[i].value_len = value.len() as u32;
            return;
        }
        let attr = Attr { name: self.names.intern(name), value_off: off, value_len: value.len() as u32, ns: 0 };
        self.add_attributes(id, &[attr]);
    }

    /// `element.removeAttribute(name)`; the list stays contiguous by shifting its tail down.
    pub fn remove_attribute(&mut self, id: NodeId, name: &str) {
        let n = *self.node(id);
        let end = (n.data + n.len) as usize;
        if let Some(i) = self.find_attr_by_qualified_name(id, name) {
            self.attrs.copy_within(i + 1..end, i);
            self.nodes[id as usize].len -= 1;
        }
    }

    /// `textContent`: the text of a text or comment node, or the text of every text node below an
    /// element or document, in tree order.
    pub fn text_content(&self, id: NodeId) -> String {
        match self.node(id).kind {
            Kind::Text | Kind::Comment => self.text_of(id).to_string(),
            Kind::Doctype => String::new(),
            _ => {
                let mut out = String::new();
                let mut stack: Vec<NodeId> = self.children(id).collect();
                stack.reverse();
                while let Some(c) = stack.pop() {
                    if self.node(c).kind == Kind::Text {
                        out += self.text_of(c);
                    }
                    let first = stack.len();
                    stack.extend(self.children(c));
                    stack[first..].reverse();
                }
                out
            }
        }
    }

    /// Set `textContent`: a text or comment node gets the new text; an element loses its children and
    /// gets one text node (none for the empty string), as the DOM specifies.
    pub fn set_text_content(&mut self, id: NodeId, text: &str) {
        match self.node(id).kind {
            Kind::Text | Kind::Comment => {
                let off = self.text.len() as u32;
                self.text.extend_from_slice(text.as_bytes());
                let n = &mut self.nodes[id as usize];
                n.data = off;
                n.len = text.len() as u32;
            }
            Kind::Element | Kind::Fragment => {
                while self.node(id).first_child != NONE {
                    let c = self.node(id).first_child;
                    self.detach(c);
                }
                if !text.is_empty() {
                    let t = self.create_text(text);
                    self.append(t, id);
                }
            }
            _ => {}
        }
    }

    // ---- save and restore (stage 4) ----

    /// The DOM as bytes: every array field by field, little-endian, after a magic word and the
    /// counts. Node ids are indices, so they survive the round trip and JS reflectors stay valid.
    pub fn to_bytes(&self) -> Vec<u8> {
        let names: usize = self.names.list.iter().map(|s| 4 + s.len()).sum();
        let mut out = Vec::with_capacity(28 + self.nodes.len() * 35 + self.attrs.len() * 13 + self.text.len() + names + (self.template_contents.len() + self.shadow_roots.len()) * 8);
        let u32s = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(SAVE_MAGIC);
        for n in [self.nodes.len(), self.attrs.len(), self.text.len(), self.names.len(), self.template_contents.len(), self.shadow_roots.len()] {
            u32s(&mut out, n as u32);
        }
        u32s(&mut out, self.mode as u32);
        for n in &self.nodes {
            out.extend_from_slice(&[n.kind as u8, n.flags, n.ns]);
            for v in [n.name, n.parent, n.first_child, n.last_child, n.next_sibling, n.prev_sibling, n.data, n.len] {
                u32s(&mut out, v);
            }
        }
        for a in &self.attrs {
            for v in [a.name, a.value_off, a.value_len] {
                u32s(&mut out, v);
            }
            out.push(a.ns);
        }
        out.extend_from_slice(&self.text);
        for s in &self.names.list {
            u32s(&mut out, s.len() as u32);
            out.extend_from_slice(s.as_bytes());
        }
        let mut templates: Vec<_> = self.template_contents.iter().collect();
        templates.sort();
        for (&t, &f) in templates {
            u32s(&mut out, t);
            u32s(&mut out, f);
        }
        let mut hosts: Vec<_> = self.shadow_roots.iter().collect();
        hosts.sort();
        for (&h, &r) in hosts {
            u32s(&mut out, h);
            u32s(&mut out, r);
        }
        // constructed sheets (their media and disabled flag since format 05), then each adopter's list
        // (format 03)
        let string = |out: &mut Vec<u8>, s: &str| {
            u32s(out, s.len() as u32);
            out.extend_from_slice(s.as_bytes());
        };
        u32s(&mut out, self.constructed_sheets.len() as u32);
        let ops = |out: &mut Vec<u8>, ops: &[RuleOp]| {
            u32s(out, ops.len() as u32);
            for op in ops {
                match op {
                    RuleOp::Insert(i, text) => {
                        out.push(0);
                        u32s(out, *i);
                        string(out, text);
                    }
                    RuleOp::Delete(i) => {
                        out.push(1);
                        u32s(out, *i);
                    }
                    RuleOp::Declaration(edit) => {
                        out.push(2);
                        u32s(out, edit.path.len() as u32);
                        for i in &edit.path { u32s(out, *i); }
                        out.push(edit.kind);
                        string(out, &edit.name);
                        string(out, &edit.value);
                        out.push(u8::from(edit.important));
                    }
                }
            }
        };
        for s in &self.constructed_sheets {
            string(&mut out, &s.text);
            ops(&mut out, &s.ops);
            string(&mut out, &s.media);
            out.push(s.disabled as u8);
        }
        let mut adopters: Vec<_> = self.adopted_sheets.iter().collect();
        adopters.sort();
        u32s(&mut out, adopters.len() as u32);
        for (&node, ids) in adopters {
            u32s(&mut out, node);
            u32s(&mut out, ids.len() as u32);
            for &i in ids {
                u32s(&mut out, i);
            }
        }
        // the node documents of the nodes outside the page's document (format 04)
        let mut owned: Vec<_> = self.node_documents.iter().collect();
        owned.sort();
        u32s(&mut out, owned.len() as u32);
        for (&node, &doc) in owned {
            u32s(&mut out, node);
            u32s(&mut out, doc);
        }
        let mut modes: Vec<_> = self.document_modes.iter().collect();
        modes.sort();
        u32s(&mut out, modes.len() as u32);
        for (&doc, &mode) in modes {
            u32s(&mut out, doc);
            u32s(&mut out, mode as u32);
        }
        // the sheets of <style> and <link> elements that script changed (format 05)
        let mut owners: Vec<_> = self.owner_sheets.iter().collect();
        owners.sort_by_key(|&(&n, _)| n);
        u32s(&mut out, owners.len() as u32);
        for (&node, sheet) in owners {
            u32s(&mut out, node);
            out.push(sheet.disabled as u8);
            match &sheet.base {
                None => u32s(&mut out, u32::MAX),
                Some(base) => string(&mut out, base),
            }
            ops(&mut out, &sheet.ops);
        }
        out
    }

    /// Read back what `to_bytes` wrote; `None` for bytes that are not a saved DOM (wrong magic, short,
    /// a bad kind, a link, offset or name out of range, text that is not UTF-8).
    pub fn from_bytes(b: &[u8]) -> Option<Dom> {
        let mut r = Reader { b, at: 0 };
        if r.take(SAVE_MAGIC.len())? != SAVE_MAGIC {
            return None;
        }
        let (nn, na, nt, nnames, ntemplates, nshadows, mode) =
            (r.u32()? as usize, r.u32()? as usize, r.u32()? as usize, r.u32()? as usize, r.u32()? as usize, r.u32()? as usize, r.u32()?);
        let mut nodes = Vec::with_capacity(nn);
        for _ in 0..nn {
            let h = r.take(3)?;
            let kind = match h[0] {
                0 => Kind::Document,
                1 => Kind::Element,
                2 => Kind::Text,
                3 => Kind::Comment,
                4 => Kind::Doctype,
                5 => Kind::Fragment,
                6 => Kind::ShadowRoot,
                _ => return None,
            };
            let mut v = [0u32; 8];
            for x in &mut v {
                *x = r.u32()?;
            }
            nodes.push(Node { kind, flags: h[1], ns: h[2], name: v[0], parent: v[1], first_child: v[2], last_child: v[3], next_sibling: v[4], prev_sibling: v[5], data: v[6], len: v[7] });
        }
        let mut attrs = Vec::with_capacity(na);
        for _ in 0..na {
            let (name, value_off, value_len) = (r.u32()?, r.u32()?, r.u32()?);
            attrs.push(Attr { name, value_off, value_len, ns: r.take(1)?[0] });
        }
        let text = r.take(nt)?.to_vec();
        let mut names = Names::default();
        for _ in 0..nnames {
            let len = r.u32()? as usize;
            let s = std::str::from_utf8(r.take(len)?).ok()?;
            names.intern(s);
        }
        let mut template_contents = HashMap::with_capacity(ntemplates);
        for _ in 0..ntemplates {
            template_contents.insert(r.u32()?, r.u32()?);
        }
        let mut shadow_roots = HashMap::with_capacity(nshadows);
        for _ in 0..nshadows {
            shadow_roots.insert(r.u32()?, r.u32()?);
        }
        let nsheets = r.u32()? as usize;
        let mut constructed_sheets = Vec::with_capacity(nsheets.min(1 << 16));
        for _ in 0..nsheets {
            let (text, ops, media) = (r.string()?, r.rule_ops()?, r.string()?);
            constructed_sheets.push(ConstructedSheet { text, ops, media, disabled: r.take(1)?[0] != 0 });
        }
        let nadopters = r.u32()? as usize;
        let mut adopted_sheets = HashMap::with_capacity(nadopters.min(1 << 16));
        for _ in 0..nadopters {
            let node = r.u32()?;
            let count = r.u32()? as usize;
            let mut ids = Vec::with_capacity(count.min(1 << 16));
            for _ in 0..count {
                ids.push(r.u32()?);
            }
            adopted_sheets.insert(node, ids);
        }
        let nowned = r.u32()? as usize;
        let mut node_documents = HashMap::with_capacity(nowned.min(1 << 16));
        for _ in 0..nowned {
            let node = r.u32()?;
            node_documents.insert(node, r.u32()?);
        }
        let nmodes = r.u32()? as usize;
        let mut document_modes = HashMap::with_capacity(nmodes.min(1 << 16));
        for _ in 0..nmodes {
            let doc = r.u32()?;
            document_modes.insert(doc, r.u32()? as i32);
        }
        let nowners = r.u32()? as usize;
        let mut owner_sheets = HashMap::with_capacity(nowners.min(1 << 16));
        for _ in 0..nowners {
            let node = r.u32()?;
            let disabled = r.take(1)?[0] != 0;
            let base = match r.u32()? {
                u32::MAX => None,
                len => Some(std::str::from_utf8(r.take(len as usize)?).ok()?.to_string()),
            };
            owner_sheets.insert(node, OwnerSheet { ops: r.rule_ops()?, base, disabled });
        }
        if r.at != b.len() || names.len() != nnames {
            return None;
        }
        // every link, name and text range must point inside the arrays
        let in_nodes = |id: u32| (id as usize) < nn;
        for n in &nodes {
            let links_ok = [n.parent, n.first_child, n.last_child, n.next_sibling, n.prev_sibling].iter().all(|&l| in_nodes(l));
            let data_ok = match n.kind {
                Kind::Element => (n.data as usize + n.len as usize) <= na && (n.name as usize) < nnames.max(1),
                Kind::Text | Kind::Comment => (n.data as usize + n.len as usize) <= nt && std::str::from_utf8(&text[n.data as usize..(n.data + n.len) as usize]).is_ok(),
                Kind::Doctype => (n.name as usize) < nnames.max(1),
                Kind::ShadowRoot => in_nodes(n.data),
                _ => true,
            };
            if !links_ok || !data_ok {
                return None;
            }
        }
        for a in &attrs {
            if (a.name as usize) >= nnames || a.value_off as usize + a.value_len as usize > nt || std::str::from_utf8(&text[a.value_off as usize..(a.value_off + a.value_len) as usize]).is_err() {
                return None;
            }
        }
        if template_contents.iter().any(|(&t, &f)| !in_nodes(t) || !in_nodes(f)) {
            return None;
        }
        // a host and its root must name each other
        if shadow_roots.iter().any(|(&h, &s)| !in_nodes(h) || !in_nodes(s) || nodes[s as usize].kind != Kind::ShadowRoot || nodes[s as usize].data != h) {
            return None;
        }
        // an adopter is a document or a shadow root; its sheets exist
        if adopted_sheets.iter().any(|(&n, ids)| {
            !in_nodes(n) || !matches!(nodes[n as usize].kind, Kind::Document | Kind::ShadowRoot) || ids.iter().any(|&i| i as usize >= nsheets)
        }) {
            return None;
        }
        // a changed sheet's owner is an element
        if owner_sheets.keys().any(|&n| !in_nodes(n) || nodes[n as usize].kind != Kind::Element) {
            return None;
        }
        // a node's document is a document node
        if node_documents.iter().chain(document_modes.keys().map(|d| (d, d))).any(|(&n, &d)| !in_nodes(n) || !in_nodes(d) || nodes[d as usize].kind != Kind::Document)
            || document_modes.values().any(|&m| !(0..=2).contains(&m))
        {
            return None;
        }
        Some(Dom { nodes, attrs, text, names, template_contents, shadow_roots, mode: mode as i32, constructed_sheets, owner_sheets, adopted_sheets, node_documents, document_modes, script_positions: HashMap::new() })
    }

    // ---- inspection ----

    /// Elements reachable from the document, not counting template contents (what
    /// `querySelectorAll('*')` sees).
    pub fn count(&self, kind: Kind) -> usize {
        let mut count = 0;
        let mut stack = vec![DOCUMENT];
        while let Some(id) = stack.pop() {
            count += (self.node(id).kind == kind) as usize;
            stack.extend(self.children(id));
        }
        count
    }

    /// Like `count`, but also inside template contents and shadow trees: a page written out with
    /// its shadow roots as declarative shadow DOM gets them back as shadow roots, except where a host
    /// cannot take one (the tree then stays in the template's contents).
    pub fn count_with_templates(&self, kind: Kind) -> usize {
        let mut count = 0;
        let mut stack = vec![DOCUMENT];
        while let Some(id) = stack.pop() {
            count += (self.node(id).kind == kind) as usize;
            stack.extend(self.children(id));
            if let Some(&f) = self.template_contents.get(&id) {
                stack.push(f);
            }
            if let Some(&r) = self.shadow_roots.get(&id) {
                stack.push(r);
            }
        }
        count
    }

    /// Nodes of `kind` inside the shadow trees of connected hosts, nested ones included (what
    /// Firefox's DOM dump counts as `shadowElements`), and the number of those shadow roots.
    pub fn count_in_shadow_trees(&self, kind: Kind) -> (usize, usize) {
        let (mut count, mut roots) = (0, 0);
        for id in self.shadow_including_descendants(DOCUMENT) {
            if self.node(id).kind == Kind::ShadowRoot {
                roots += 1;
            } else if self.node(id).kind == kind && self.containing_shadow(id) != NONE {
                count += 1;
            }
        }
        (count, roots)
    }

    /// Same text format as the C++ test sink (`parser/test_sink.cpp`), plus template contents.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.dump_node(DOCUMENT, 0, &mut out);
        out
    }

    fn dump_node(&self, id: NodeId, depth: usize, out: &mut String) {
        use std::fmt::Write;
        let pad = "  ".repeat(depth);
        let n = self.node(id);
        match n.kind {
            Kind::Document => writeln!(out, "{pad}#document").unwrap(),
            Kind::Fragment => writeln!(out, "{pad}#template-contents").unwrap(),
            Kind::ShadowRoot => writeln!(out, "{pad}#shadow-root ({})", if n.flags & SR_CLOSED != 0 { "closed" } else { "open" }).unwrap(),
            Kind::Element => {
                write!(out, "{pad}<{}", self.tag(id)).unwrap();
                for a in self.attrs_of(id) {
                    write!(out, " {}=\"{}\"", self.names.get(a.name), self.attr_value(a)).unwrap();
                }
                writeln!(out, ">").unwrap();
            }
            Kind::Text => writeln!(out, "{pad}\"{}\"", self.text_of(id)).unwrap(),
            Kind::Comment => writeln!(out, "{pad}<!--{}-->", self.text_of(id)).unwrap(),
            Kind::Doctype => writeln!(out, "{pad}<!DOCTYPE {}>", self.names.get(n.name)).unwrap(),
        }
        if let Some(&frag) = self.template_contents.get(&id) {
            self.dump_node(frag, depth + 1, out);
        }
        if let Some(&root) = self.shadow_roots.get(&id) {
            self.dump_node(root, depth + 1, out);
        }
        for c in self.children(id) {
            self.dump_node(c, depth + 1, out);
        }
    }

    /// Bytes held by the arrays (capacity, not just length).
    pub fn heap_bytes(&self) -> HeapBytes {
        HeapBytes {
            nodes: self.nodes.capacity() * size_of::<Node>(),
            attrs: self.attrs.capacity() * size_of::<Attr>(),
            text: self.text.capacity(),
            names: self.names.list.iter().map(|s| s.len()).sum::<usize>() + self.names.len() * 40,
            sheets: self.constructed_sheets.iter().map(|s| s.text.capacity() + s.media.capacity() + 80 + ops_bytes(&s.ops)).sum::<usize>()
                + self.owner_sheets.values().map(|s| 64 + s.base.as_ref().map_or(0, |b| b.capacity()) + ops_bytes(&s.ops)).sum::<usize>()
                + self.adopted_sheets.values().map(|v| v.capacity() * 4 + 48).sum::<usize>()
                + self.node_documents.capacity() * 9,
        }
    }
}

/// The heap bytes of a list of RuleOps.
fn ops_bytes(ops: &Vec<RuleOp>) -> usize {
    ops.capacity() * size_of::<RuleOp>() + ops.iter().map(|op| match op {
        RuleOp::Insert(_, t) => t.capacity(),
        RuleOp::Delete(_) => 0,
        RuleOp::Declaration(e) => size_of::<DeclarationOp>() + e.path.capacity() * size_of::<u32>() + e.name.capacity() + e.value.capacity(),
    }).sum::<usize>()
}

/// The first bytes of a saved DOM (`Dom::to_bytes`): a name and a format version.
const SAVE_MAGIC: &[u8; 8] = b"MRTDOM06";

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.b.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    /// A count, then that many RuleOps (a kind byte, the index, an insert's text).
    fn rule_ops(&mut self) -> Option<Vec<RuleOp>> {
        let n = self.u32()? as usize;
        let mut ops = Vec::with_capacity(n.min(1 << 16));
        for _ in 0..n {
            ops.push(match self.take(1)?[0] {
                0 => {
                    let i = self.u32()?;
                    RuleOp::Insert(i, self.string()?)
                }
                1 => RuleOp::Delete(self.u32()?),
                2 => {
                    let n = self.u32()? as usize;
                    if n > 1024 { return None; }
                    let path = (0..n).map(|_| self.u32()).collect::<Option<Vec<_>>>()?;
                    let kind = self.take(1)?[0];
                    if path.is_empty() || !matches!(kind, 2 | 3 | 7) { return None; }
                    RuleOp::Declaration(Box::new(DeclarationOp { path, kind,
                        name: self.string()?, value: self.string()?, important: self.take(1)?[0] != 0 }))
                }
                _ => return None,
            });
        }
        Some(ops)
    }
    /// A length, then that many bytes of UTF-8.
    fn string(&mut self) -> Option<String> {
        let len = self.u32()? as usize;
        Some(std::str::from_utf8(self.take(len)?).ok()?.to_string())
    }
}

impl Default for Dom {
    fn default() -> Self {
        Dom::new()
    }
}

#[derive(Debug)]
pub struct HeapBytes {
    pub nodes: usize,
    pub attrs: usize,
    pub text: usize,
    pub names: usize,
    /// constructed style sheets, the sheets of `<style>` and `<link>` elements that script changed, and
    /// the adopted lists
    pub sheets: usize,
}

const _: () = assert!(size_of::<Node>() == 36);
const _: () = assert!(size_of::<Attr>() == 16);

#[cfg(test)]
mod tests {
    use super::*;

    fn body(d: &Dom) -> NodeId {
        let html = d.children(DOCUMENT).find(|&c| d.node(c).kind == Kind::Element).unwrap();
        d.children(html).find(|&c| d.tag(c) == "body").unwrap()
    }

    /// Counts what a resumed parse reports.
    #[derive(Default)]
    struct Count {
        inserted: usize,
        removed: usize,
    }
    impl ParserObserver for Count {
        fn inserted(&mut self, _: NodeId) {
            self.inserted += 1;
        }
        fn will_remove(&mut self, _: NodeId) {
            self.removed += 1;
        }
        fn text_changed(&mut self, _: NodeId) {}
        fn attributes_added(&mut self, _: NodeId, _: &[String]) {}
    }

    #[test]
    fn parse_at_scripts_pace() {
        // a script in the head, one in a table (foster parenting around it), one in SVG, and an
        // unclosed one at the end of the file; the adoption agency moves nodes (<b><p>)
        let html = "<!doctype html><head><script>a</script></head><body><b><p>x</b>y</p><table><tr><td>1<script>b</script></td></tr>z</table><svg><script>c</script></svg><p>after<script>d";
        let mut dom = Dom::new();
        let mut parser = DocumentParser::new(html);
        let mut count = Count::default();
        let mut seen = Vec::new();
        while let Some(script) = parser.resume(&mut dom, Some(&mut count)) {
            // the script is in the tree, and nothing after it is
            let text: String = dom.children(script).map(|t| dom.text_content(t)).collect();
            seen.push((text, dom.dump().contains("after")));
        }
        assert_eq!(seen, vec![("a".into(), false), ("b".into(), false), ("c".into(), false), ("d".into(), true)]);
        assert!(parser.resume(&mut dom, None).is_none());
        // the same tree as the one-shot parse
        assert_eq!(dom.dump(), Dom::parse_with_scripting(html).dump());
        assert!(count.inserted > 20 && count.removed > 0);
    }

    #[test]
    fn build_tree_from_script() {
        let mut d = Dom::parse("<!doctype html><body><p id=a>x</p></body>");
        let b = body(&d);
        let div = d.create_element("div");
        let t = d.create_text("hello");
        d.insert_child(div, t, NONE);
        d.insert_child(b, div, NONE);
        assert_eq!(d.text_content(b), "xhello");
        let p = d.node(b).first_child;
        d.check_insert(b, div, p).unwrap();
        d.insert_child(b, div, p);
        assert_eq!(d.text_content(b), "hellox");
        assert!(d.remove_child(b, div));
        assert!(!d.remove_child(b, div));
        assert_eq!(d.text_content(b), "x");
        assert_eq!(d.check_insert(div, b, NONE), Ok(()));
        d.insert_child(div, b, NONE);
        assert_eq!(d.check_insert(b, div, NONE), Err("HierarchyRequestError"));
    }

    #[test]
    fn attributes_from_script() {
        let mut d = Dom::parse("<!doctype html><body><p id=a class=c>x</p><i title=t></i></body>");
        let b = body(&d);
        let p = d.node(b).first_child;
        let i = d.node(p).next_sibling;
        // p's attributes are followed by i's: adding one moves p's list to the end
        d.set_attribute(p, "lang", "en");
        assert_eq!(d.get_attribute(p, "id"), Some("a"));
        assert_eq!(d.get_attribute(p, "lang"), Some("en"));
        assert_eq!(d.get_attribute(i, "title"), Some("t"));
        d.set_attribute(p, "class", "other");
        assert_eq!(d.get_attribute(p, "class"), Some("other"));
        d.remove_attribute(p, "id");
        assert_eq!(d.get_attribute(p, "id"), None);
        assert_eq!(d.get_attribute(p, "lang"), Some("en"));
        let e = d.create_element("span");
        d.set_attribute(e, "class", "c");
        d.set_attribute(e, "id", "s");
        assert_eq!(d.attrs_of(e).len(), 2);
        assert_eq!(d.get_attribute(i, "title"), Some("t"));
    }

    #[test]
    fn save_and_restore() {
        let mut d = Dom::parse("<!doctype html><html lang=en><body><p id=a class=c>x &amp; é</p><template><b>t</b></template><!--c--></body></html>");
        let p = d.create_element("span");
        d.set_attribute(p, "title", "made");
        let b = body(&d);
        d.insert_child(b, p, NONE);
        let bytes = d.to_bytes();
        let back = Dom::from_bytes(&bytes).expect("restores");
        assert_eq!(back.dump(), d.dump());
        assert_eq!(back.nodes.len(), d.nodes.len());
        assert_eq!(back.mode, d.mode);
        // anything else is refused, not trusted
        assert!(Dom::from_bytes(&bytes[..bytes.len() - 1]).is_none());
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert!(Dom::from_bytes(&bad).is_none());
        let mut bad_link = bytes.clone();
        // the parent link of node 2 (after the 8-byte magic, 7 counts, and two 35-byte nodes; 3 header bytes, then the name)
        let at = 8 + 28 + 2 * 35 + 3 + 4;
        bad_link[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Dom::from_bytes(&bad_link).is_none());
    }

    #[test]
    fn set_text_content() {
        let mut d = Dom::parse("<!doctype html><body><p>a<b>b</b></p></body>");
        let p = d.node(body(&d)).first_child;
        assert_eq!(d.text_content(p), "ab");
        d.set_text_content(p, "new");
        assert_eq!(d.children(p).count(), 1);
        assert_eq!(d.text_content(p), "new");
        let t = d.node(p).first_child;
        d.set_text_content(t, "again");
        assert_eq!(d.text_content(p), "again");
        d.set_text_content(p, "");
        assert_eq!(d.children(p).count(), 0);
    }

    fn by_id(d: &Dom, id: &str) -> NodeId {
        d.shadow_including_descendants(DOCUMENT).into_iter().find(|&n| d.node(n).kind == Kind::Element && d.get_attribute(n, "id") == Some(id)).unwrap()
    }

    #[test]
    fn declarative_shadow_root_and_slots() {
        let d = Dom::parse(
            "<!doctype html><body><div id=h><template shadowrootmode=open shadowrootclonable><slot id=sa name=a></slot><slot id=sd><i>fallback</i></slot><p>s</p></template><span id=x slot=a>A</span>t<b id=y>B</b></div></body>",
        );
        let h = by_id(&d, "h");
        let root = d.shadow_root(h);
        assert_ne!(root, NONE);
        assert_eq!(d.node(root).kind, Kind::ShadowRoot);
        assert_eq!(d.host(root), h);
        assert_eq!(d.node(root).flags, SR_CLONABLE | SR_DECLARATIVE);
        // the template is not inserted; its children are the shadow root's
        assert!(d.children(h).all(|c| d.node(c).kind != Kind::Element || d.tag(c) != "template"));
        assert_eq!(d.children(root).count(), 3);
        let (sa, sd, x, y) = (by_id(&d, "sa"), by_id(&d, "sd"), by_id(&d, "x"), by_id(&d, "y"));
        assert_eq!(d.containing_shadow(sa), root);
        assert!(d.is_connected(sa));
        assert_eq!(d.assigned_slot(x), sa);
        assert_eq!(d.assigned_slot(y), sd);
        let t = d.node(x).next_sibling;
        assert_eq!(d.assigned_slot(t), sd);
        assert_eq!(d.assigned_nodes(sd), vec![t, y]);
        assert_eq!(d.flattened_children(h), d.children(root).collect::<Vec<_>>());
        assert_eq!(d.flattened_children(sd), vec![t, y]);
        assert_eq!(d.flattened_parent(x), sa);
        assert_eq!(d.flattened_parent(sa), h);
        // the fallback content of a slot that has assigned nodes is not in the flattened tree
        let fallback = d.node(sd).first_child;
        assert_eq!(d.flattened_parent(fallback), NONE);
        assert_eq!(d.count_in_shadow_trees(Kind::Element), (4, 1));
        assert_eq!(d.count(Kind::Element), 6); // html head body div span b, and nothing inside the root
    }

    #[test]
    fn declarative_shadow_root_refused() {
        // not a valid host, a bad mode, a second root, and innerHTML: all stay plain templates
        let mut d = Dom::parse(
            "<!doctype html><body><a id=a><template shadowrootmode=open><b></b></template></a><div id=m><template shadowrootmode=other></template></div><div id=two><template shadowrootmode=closed></template><template shadowrootmode=open><i></i></template></div></body>",
        );
        assert_eq!(d.shadow_root(by_id(&d, "a")), NONE);
        assert_eq!(d.shadow_root(by_id(&d, "m")), NONE);
        let two = by_id(&d, "two");
        let root = d.shadow_root(two);
        assert_eq!(d.node(root).flags, SR_CLOSED | SR_DECLARATIVE);
        assert_eq!(d.children(two).filter(|&c| d.tag(c) == "template").count(), 1);
        let f = d.parse_fragment("<div><template shadowrootmode=open><b></b></template></div>", NONE);
        let div = d.node(f).first_child;
        assert_eq!(d.shadow_root(div), NONE);
        assert_eq!(d.tag(d.node(div).first_child), "template");
    }

    #[test]
    fn attach_shadow() {
        let mut d = Dom::parse("<!doctype html><body><div id=h><template shadowrootmode=open><b></b></template></div><span id=s></span><a id=a></a></body>");
        let h = by_id(&d, "h");
        let root = d.shadow_root(h);
        assert_eq!(d.attach_shadow(h, SR_CLOSED), Err("NotSupportedError"));
        // a declarative root of the same mode is taken over, emptied
        assert_eq!(d.attach_shadow(h, 0), Ok(root));
        assert_eq!(d.children(root).count(), 0);
        assert_eq!(d.node(root).flags & SR_DECLARATIVE, 0);
        assert_eq!(d.attach_shadow(h, 0), Err("NotSupportedError"));
        assert!(!d.can_attach_shadow(by_id(&d, "a")));
        let s = by_id(&d, "s");
        let sr = d.attach_shadow(s, SR_CLONABLE).unwrap();
        let p = d.create_element("p");
        d.insert_child(sr, p, NONE);
        // a host can't go into its own shadow tree
        assert_eq!(d.check_insert(p, s, NONE), Err("HierarchyRequestError"));
        // a clonable root is copied with its host, deep or not; others are not
        let c = d.clone_node(s, false);
        assert_eq!(d.children(d.shadow_root(c)).count(), 1);
        let c = d.clone_node(h, true);
        assert_eq!(d.shadow_root(c), NONE);
        // a detached host's shadow tree is not connected
        assert!(d.is_connected(p));
        let b = d.node(s).parent;
        d.remove_child(b, s);
        assert!(!d.is_connected(p));
        assert_eq!(d.shadow_including_root(p), s);
    }

    #[test]
    fn save_and_restore_shadow_roots() {
        let d = Dom::parse("<!doctype html><body><div><template shadowrootmode=closed><slot></slot></template>x</div></body>");
        let back = Dom::from_bytes(&d.to_bytes()).expect("restores");
        assert_eq!(back.dump(), d.dump());
        assert!(back.dump().contains("#shadow-root (closed)"));
        assert_eq!(back.shadow_roots, d.shadow_roots);
    }

    #[test]
    fn node_documents_adopt_and_save() {
        let mut d = Dom::parse("<p>x</p>");
        let other = d.create_html_document(None);
        let html = d.children(other).find(|&c| d.node(c).kind == Kind::Element).unwrap();
        assert_eq!(d.node_document(html), other);
        // made by the other document, then moved into the page's
        let el = d.create_element("div");
        d.set_node_document(el, other);
        let t = d.create_text("t");
        d.insert_child(el, t, NONE);
        assert_eq!(d.node_document(t), other);
        let body = d.children(DOCUMENT).find(|&c| d.node(c).kind == Kind::Element).unwrap();
        d.insert_child(body, el, NONE);
        assert_eq!((d.node_document(el), d.node_document(t)), (DOCUMENT, DOCUMENT));
        let copy = d.clone_node(html, true);
        assert_eq!(d.node_document(copy), other);
        let back = Dom::from_bytes(&d.to_bytes()).unwrap();
        assert_eq!(back.node_document(html), other);
        assert_eq!(back.node_document(copy), other);
        assert_eq!(back.node_document(el), DOCUMENT);
    }

    #[test]
    fn save_and_restore_adopted_sheets() {
        let mut d = Dom::parse("<!doctype html><body><div><template shadowrootmode=open></template></div></body>");
        let root = *d.shadow_roots.values().next().unwrap();
        let a = d.new_constructed_sheet("p { color: red }");
        let b = d.new_constructed_sheet_with("", "print", true);
        d.set_constructed_sheet(b, ":host { display: block }");
        d.set_adopted_sheets(DOCUMENT, &[a]);
        d.set_adopted_sheets(root, &[a, b, 99]);
        assert_eq!(d.adopted_sheets(root), &[a, b]);
        let back = Dom::from_bytes(&d.to_bytes()).expect("restores");
        assert_eq!(back.constructed_sheets, vec![
            ConstructedSheet { text: "p { color: red }".into(), ops: vec![], media: "".into(), disabled: false },
            ConstructedSheet { text: ":host { display: block }".into(), ops: vec![], media: "print".into(), disabled: true },
        ]);
        assert_eq!(back.adopted_sheets, d.adopted_sheets);
        // an adopter that is not a document or a shadow root is refused
        let mut bad = d.clone();
        bad.adopted_sheets.insert(2, vec![a]);
        assert!(Dom::from_bytes(&bad.to_bytes()).is_none());
        d.set_adopted_sheets(root, &[]);
        assert!(d.adopted_sheets(root).is_empty());
    }

    #[test]
    fn save_and_restore_rule_declarations() {
        let mut dom = Dom::parse("<!doctype html><style id=s>p { color: red }</style>");
        let style = by_id(&dom, "s");
        let op = RuleOp::Declaration(Box::new(DeclarationOp {
            path: vec![1, 0], kind: 2, name: "color".into(), value: "blue".into(), important: true,
        }));
        dom.push_owner_sheet_op(style, None, op.clone());
        let constructed = dom.new_constructed_sheet("@media all { p {} }");
        dom.push_constructed_sheet_op(constructed, op.clone());
        let restored = Dom::from_bytes(&dom.to_bytes()).expect("restores declaration operations");
        assert_eq!(restored.owner_sheet(style).unwrap().ops, vec![op.clone()]);
        assert_eq!(restored.constructed_sheets[constructed as usize].ops, vec![op]);
    }

    #[test]
    fn save_and_restore_owner_sheets() {
        let mut d = Dom::parse("<!doctype html><head><style id=s>p { color: red }</style><style id=t></style><link id=l></head><body></body>");
        let (st, tt, lt) = (by_id(&d, "s"), by_id(&d, "t"), by_id(&d, "l"));
        d.push_owner_sheet_op(st, None, RuleOp::Insert(0, "b { color: blue }".into()));
        d.push_owner_sheet_op(st, None, RuleOp::Delete(1));
        d.set_owner_sheet_disabled(tt, true);
        // a link's text is kept with its first change only
        d.push_owner_sheet_op(lt, Some("i {}"), RuleOp::Insert(1, "u {}".into()));
        d.push_owner_sheet_op(lt, Some("other"), RuleOp::Delete(0));
        let c = d.new_constructed_sheet("a {}");
        d.push_constructed_sheet_op(c, RuleOp::Insert(1, "q {}".into()));
        let back = Dom::from_bytes(&d.to_bytes()).expect("restores");
        assert_eq!(back.owner_sheet(st).unwrap().ops, vec![RuleOp::Insert(0, "b { color: blue }".into()), RuleOp::Delete(1)]);
        assert_eq!(back.owner_sheet(st).unwrap().base, None);
        assert_eq!(back.owner_sheet(lt).unwrap().base.as_deref(), Some("i {}"));
        assert!(back.owner_sheet_disabled(tt) && !back.owner_sheet_disabled(st));
        assert_eq!(back.constructed_sheets[c as usize].ops, vec![RuleOp::Insert(1, "q {}".into())]);
        // replaceSync forgets the changes; enabling again leaves nothing to keep
        d.set_constructed_sheet(c, "z {}");
        assert!(d.constructed_sheets[c as usize].ops.is_empty());
        d.set_owner_sheet_disabled(tt, false);
        assert!(!d.owner_sheets.contains_key(&tt));
        assert!(d.drop_owner_sheet(st));
        assert!(d.owner_sheet(st).is_none());
        // an owner that is not an element is refused
        let mut bad = d.clone();
        bad.owner_sheets.insert(DOCUMENT, OwnerSheet::default());
        assert!(Dom::from_bytes(&bad.to_bytes()).is_none());
    }
}
