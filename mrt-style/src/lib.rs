//! Stylo's element traits implemented over the arena DOM (`mrt-dom`).
//!
//! A handle (`H`) is a shared reference to the `StyleDom` plus a node id; Stylo gets handles, and
//! the arena stays free of Stylo types. Per-element style data lives in a side table that only
//! has an entry for elements.

mod traits;
pub mod cssom;
pub mod computed;
pub mod restyle;

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::Relaxed};
use std::marker::PhantomData;

use web_atoms::{LocalName, Namespace};
use mrt_dom::{Dom, Kind, NodeId, DOCUMENT, NONE};
use selectors::matching::QuirksMode;
use selectors::Element;
use style::Atom;
use cssparser::SourceLocation;
use style::error_reporting::{ContextualParseError, ParseErrorReporter};
use style::context::{RegisteredSpeculativePainter, RegisteredSpeculativePainters, SharedStyleContext};
use style::data::ElementDataWrapper;
use style::dom::{TElement, TNode};
use style::font_metrics::FontMetrics;
use style::global_style_data::GLOBAL_STYLE_DATA;
use style::device::Device;
use style::media_queries::{MediaList, MediaType};
use style::properties::style_structs::Font;
use style::properties::{parse_style_attribute, ComputedValues, PropertyDeclarationBlock};
use style::shared_lock::Locked;
use style::stylesheets::CssRuleType;
use style::selector_parser::SnapshotMap;
use style::queries::values::PrefersColorScheme;
use style::device::servo::FontMetricsProvider;
use style::servo_arc::Arc as ServoArc;
use style::shared_lock::{SharedRwLock, StylesheetGuards};
use style::stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet, UrlExtraData};
use style::stylist::Stylist;
use style::thread_state::{self, ThreadState};
use style::traversal::DomTraversal;
use style::traversal_flags::TraversalFlags;
use style::values::computed::font::{GenericFontFamily, QueryFontMetricsFlags};
use style::values::computed::{CSSPixelLength, Length};

pub use mrt_dom;
pub use style;
pub use style_traits;

/// Firefox's own user-agent sheets (`assets/firefox-ua/`, copied by `vendor-ua.sh`), in the order
/// `Document::FillStyleSetUserAndUASheets` adds them.
///
/// `quirk.css` is last and applies in quirks mode only.
const FIREFOX_UA: [(&str, &str); 8] = [
    ("ua.css", include_str!("../assets/firefox-ua/ua.css")),
    ("mathml.css", include_str!("../assets/firefox-ua/mathml.css")),
    ("svg.css", include_str!("../assets/firefox-ua/svg.css")),
    ("html.css", include_str!("../assets/firefox-ua/html.css")),
    ("counterstyles.css", include_str!("../assets/firefox-ua/counterstyles.css")),
    ("forms.css", include_str!("../assets/firefox-ua/forms.css")),
    ("scrollbars.css", include_str!("../assets/firefox-ua/scrollbars.css")),
    ("quirk.css", include_str!("../assets/firefox-ua/quirk.css")),
];

/// Counts the errors Stylo reports while parsing a sheet (rules and declarations it drops).
#[derive(Default)]
struct ErrorCount(AtomicUsize);

impl ParseErrorReporter for ErrorCount {
    fn report_error(&self, _url: &UrlExtraData, location: SourceLocation, error: ContextualParseError) {
        if std::env::var_os("MRT_PARSE_ERRORS").is_some() {
            eprintln!("  {}:{}: {}", location.line, location.column, error);
        }
        self.0.fetch_add(1, Relaxed);
    }
}

/// Per-element style data. One per element, found through `Tree::slot`.
///
/// Only what every element has lives here. The `id` atom and the parsed `style=""` block, which
/// most elements lack, are in the sorted side tables `Tree::id_atoms` and `Tree::style_attrs`.
pub struct Slot {
    /// Meaningful only when `has_data` says so (an `Option` here would cost 8 more bytes).
    pub data: ElementDataWrapper,
    /// `ElementSelectorFlags` bits (bits 0 to 30; Stylo sets them through `&self`), and
    /// `HAS_DATA` in bit 31.
    pub flags: AtomicU32,
}

/// Set in `Slot::flags` once `data` has been created for the element.
pub const HAS_DATA: u32 = 1 << 31;
/// Set in `Slot::flags` when a descendant in the flattened tree needs styling (Gecko:
/// ELEMENT_HAS_DIRTY_DESCENDANTS_FOR_SERVO).
pub const HAS_DIRTY_DESCENDANTS: u32 = 1 << 30;
/// Set in `Slot::flags` when the element has a snapshot in `Tree::snapshots` (Gecko:
/// ELEMENT_HAS_SNAPSHOT), and when the traversal has handled it (ELEMENT_HANDLED_SNAPSHOT).
pub const HAS_SNAPSHOT: u32 = 1 << 29;
pub const HANDLED_SNAPSHOT: u32 = 1 << 28;
/// The bits of `Slot::flags` that are not selector flags.
pub const NOT_SELECTOR_FLAGS: u32 = HAS_DATA | HAS_DIRTY_DESCENDANTS | HAS_SNAPSHOT | HANDLED_SNAPSHOT;

/// The `ElementState` bits that selectors read here, from the DOM alone (Gecko: Element::State): a
/// link is unvisited (there is no history); `:defined` is every element but a custom element not
/// (yet) upgraded. Shared by the style traits and the restyle recorder, which snapshots it.
pub fn element_state_bits(dom: &Dom, id: NodeId) -> u64 {
    use style_dom::ElementState;
    let n = dom.node(id);
    let mut state = ElementState::empty();
    if n.kind == Kind::Element && matches!(dom.tag(id), "a" | "area") && dom.get_attribute(id, "href").is_some() {
        state |= ElementState::UNVISITED;
    }
    if dom.is_defined(id) {
        state |= ElementState::DEFINED;
    }
    state.bits()
}

impl Slot {
    pub fn has_data(&self) -> bool {
        self.flags.load(Relaxed) & HAS_DATA != 0
    }
}

/// Everything a handle can reach. The stylist is kept outside so it can be flushed while handles exist.
pub struct Tree {
    pub dom: Dom,
    /// The document styled: the page's (`DOCUMENT`), or a frame's (DESIGN, "The frames' layout (S5)").
    /// Only its elements get style data.
    pub doc: NodeId,
    /// Node id to index in `slots`; `u32::MAX` for nodes that are not elements.
    slot_of: Vec<u32>,
    pub slots: Vec<Slot>,
    /// (slot index, `id` attribute as an atom), sorted by slot index. Stylo's `TElement::id`
    /// returns a reference, so the atoms must be stored.
    id_atoms: Vec<(u32, Atom)>,
    /// (slot index, parsed `style=""`), sorted by slot index. Elements with the same attribute
    /// text share one block.
    style_attrs: Vec<(u32, ServoArc<Locked<PropertyDeclarationBlock>>)>,
    /// Presentational-hint blocks by their CSS text, so that elements with the same mapped
    /// attributes share one block (Gecko: `AttributeStyles` uniques its mapped attributes the same
    /// way). Stylo compares hint blocks by pointer, both for rule nodes and for style sharing.
    pub(crate) pres_hint_blocks: Mutex<HashMap<String, ServoArc<Locked<PropertyDeclarationBlock>>>>,
    /// Atom for each interned name of the DOM (tag names and attribute names), same index.
    name_atoms: Vec<LocalName>,
    /// Atom for each namespace number the parser uses (0 none, 1 xmlns, 2 xml, 3 html, 4 xlink,
    /// 5 mathml, 6 svg).
    ns_atoms: [Namespace; 7],
    pub guard: SharedRwLock,
    pub snapshots: SnapshotMap,
    pub url_data: UrlExtraData,
    /// The parsed `style=""` blocks by their text, kept between style builds (a text no element has
    /// any more is dropped by `sync_tables`).
    style_blocks: HashMap<String, ServoArc<Locked<PropertyDeclarationBlock>>>,
    /// A slot was made for an element older than others with slots (adopted from another document):
    /// the side tables sorted by slot need sorting.
    slots_out_of_order: bool,
    /// Each connected shadow root's author styles: the `<style>` elements of its tree, in tree
    /// order, and the cascade data built from them (Gecko: the ShadowRoot's ServoStyleSet data,
    /// `ShadowRoot::mServoStyles`). `TShadowRoot::style_data` reads it.
    pub shadow_styles: HashMap<NodeId, style::author_styles::AuthorStyles<DocumentStyleSheet>>,
}

pub struct StyleDom {
    pub tree: Tree,
    pub stylist: Stylist,
    pub author_sheets: Vec<DocumentStyleSheet>,
    /// The sheet of each stylesheet `<link>` that loaded, by element: its text, from the runtime's
    /// cache of linked sheets (SPEC decision 13: not page state, so not in the arena).
    pub link_sheets: HashMap<NodeId, std::sync::Arc<str>>,
    /// (sheet name, parse errors) for each sheet added, user-agent sheets first.
    pub parse_errors: Vec<(String, usize)>,
    /// The parsed sheets of earlier style builds of the same page, given back by `take_sheet_cache`.
    sheet_cache: SheetCache,
    /// The viewport the device was made for (a new stylist needs a new device).
    viewport: (f32, f32),
    /// Which sheets the stylist and the shadow trees' author styles were built from, by tree (the
    /// document is `DOCUMENT`): a style build whose sheets are the same keeps them.
    sheets_signature: Vec<(NodeId, usize)>,
    /// Lazily computed `::marker` styles, keyed by node id: only list items ever get an entry.
    /// (`::before` and `::after` are not here: Stylo computes those eagerly into
    /// `ElementData::styles.pseudos`, which costs 8 bytes per element and a small array only for
    /// elements that have one.)
    markers: Mutex<Markers>,
}

/// The `::marker` styles made so far. Two list items whose marker matched the same rules and
/// whose element has the same style get the same `Arc` (`by_rules`), so a long list costs one
/// style plus 16 bytes per item, not one style per item.
#[derive(Default)]
struct Markers {
    by_node: HashMap<NodeId, ServoArc<ComputedValues>>,
    by_rules: HashMap<(style::rule_tree::StrongRuleNode, usize), ServoArc<ComputedValues>>,
}

/// No fonts yet: every metric the stylist asks for gets the default. Layout (stage 2b) replaces it.
#[derive(Debug)]
struct NoFontMetrics;

impl FontMetricsProvider for NoFontMetrics {
    fn query_font_metrics(&self, _vertical: bool, _font: &Font, _size: CSSPixelLength, _flags: QueryFontMetricsFlags) -> FontMetrics {
        FontMetrics::default()
    }
    fn base_size_for_generic(&self, generic: GenericFontFamily) -> Length {
        // The default font sizes of Firefox on Linux: modules/libpref/init/all.js sets
        // `font.size.monospace.x-western` (and x-unicode) to 12 in the XP_UNIX block (13 on other
        // platforms), and `font.size.variable.*` to 16. Firefox measures 12 by 10 `0`s (72.17 px).
        let px = if generic == GenericFontFamily::Monospace { 12.0 } else { 16.0 };
        Length::from(app_units::Au::from_f32_px(px))
    }
}

struct NoPainters;
impl RegisteredSpeculativePainters for NoPainters {
    fn get(&self, _name: &Atom) -> Option<&dyn RegisteredSpeculativePainter> {
        None
    }
}

/// Makes a user-agent sheet readable by this Stylo, which is built without Gecko's media features.
///
/// html.css gives `h1` to `h6` their display, size and margins inside
/// `@media not (-moz-pref("dom.headingoffset.enabled"))`. The preference is false in the Firefox we
/// compare with (modules/libpref/init/StaticPrefList.yaml), so the condition is true, but `-moz-pref`
/// is a Gecko-only media feature: the media rule fails to parse, the sheet drops it, and headings
/// came out as plain inline text. The condition is replaced by `all`. The other `-moz-pref` and
/// `-moz-platform` rules in the sheets are conditions that are false here (or do not matter to
/// layout), which is what a dropped rule amounts to.
///
/// html.css also writes `table:-moz-table-border-nonzero` (a table with a `border` attribute other than
/// 0, which gives the table and its cells their borders). Stylo's servo configuration names that
/// pseudo-class `:-servo-nonzero-border`, so the sheet is rewritten to it (the match is in traits.rs).
///
/// ua.css gives `::marker` its `white-space: pre`, `text-align: end` and `font-variant-numeric` in one rule
/// for `::marker, ::before::marker, ::after::marker`. Stylo's servo configuration cannot parse a
/// pseudo-element after another one, and a selector list with one bad selector is dropped whole,
/// so every marker lost the rule. The two compound selectors are removed (a `::marker` of a
/// generated box is not made here).
///
/// forms.css needs four more rewrites (the pseudo-class one is last), because Stylo's servo configuration drops the whole rule or
/// declaration that has these (and the button, input and select boxes lost their font, borders and
/// padding):
///   - `font: -moz-field`, `-moz-button` and `-moz-list` (system fonts) become `13.3333px system-ui`,
///     which is what Gecko's GTK look-and-feel gives them when no GTK font is set: 10pt in the
///     system UI font. The rest of the `font` shorthand resets (line-height, weight, style) as for
///     a system font. (`-moz-fixed` is understood and stays.)
///   - the system colors in `border` shorthands (`ButtonBorder`, `ThreeDFace`, `ThreeDShadow`) become
///     a plain gray: a `border` with an invalid color is dropped whole, and the border width with it.
///   - the pseudo-class `:-moz-select-list-box` becomes the attribute test of a list box: `select:is([multiple], [size]:not([size="0"], [size="1"], [size=""]))`
///     (HTMLSelectElement::Size is the `size` attribute as an unsigned integer, 0 when missing or malformed).
///     The rules for it had `padding-inline: 0` and the forced `line-height: normal` of the options.
///   - the selector `::file-selector-button` (unknown here) is removed from the selector lists that
///     have `button` (a selector list with one bad selector is dropped whole, which dropped the
///     padding, border and font of every button and button-type input).
fn patch_ua_sheet(css: &str) -> std::borrow::Cow<'_, str> {
    const HEADINGS: &str = r#"@media not (-moz-pref("dom.headingoffset.enabled")) {"#;
    const BORDER: &str = ":-moz-table-border-nonzero";
    const MARKER: &str = "::marker,\n::before::marker,\n::after::marker {";
    const SYSTEM_FONTS: [&str; 3] = ["font: -moz-field;", "font: -moz-button;", "font: -moz-list;"];
    const SYSTEM_COLOR_BORDERS: [(&str, &str); 4] = [
        ("border: 2px inset ButtonBorder;", "border: 2px inset gray;"),
        ("border: 2px outset ButtonBorder;", "border: 2px outset gray;"),
        ("border: 2px groove ThreeDFace;", "border: 2px groove gray;"),
        ("border: 1px solid ThreeDShadow;", "border: 1px solid gray;"),
    ];
    const FILE_SELECTOR_LINES: [&str; 5] = [
        "::file-selector-button,\n",
        "::file-selector-button:hover,\n",
        "::file-selector-button:active:hover,\n",
        ":is(:disabled, :disabled:active)::file-selector-button,\n",
        "  ::file-selector-button,\n",
    ];
    // A select with `multiple` or a size above 1 is a list box (the pseudo-class :-moz-select-list-box).
    const SELECT_LIST_BOX: &str = ":-moz-select-list-box";
    const SELECT_LIST_BOX_SELECTOR: &str = ":is([multiple], [size]:not([size=\"0\"], [size=\"1\"], [size=\"\"]))";
    // The states of a dir attribute (Gecko: nsGenericHTMLElement::AfterSetAttr, HTMLElement::HTMLElement):
    // HAS_DIR_ATTR for any dir attribute (valid or not), HAS_DIR_ATTR_LTR and _RTL for dir=ltr and
    // dir=rtl (any case), HAS_DIR_ATTR_LIKE_AUTO for dir=auto and for a bdi without a valid dir. Stylo
    // without the Gecko configuration has no pseudo-classes for them, so html.css's rules for them
    // (`unicode-bidi: isolate`, `direction`) are written with the attribute selectors that mean the
    // same. The rules that also use `:dir()` (the directionality found from the content of a dir=auto
    // element) stay unparsable and dropped: NOT PORTED.
    const DIR_STATES: [(&str, &str); 4] = [
        (":-moz-has-dir-attr", ":is([dir])"),
        (":-moz-dir-attr-ltr", ":is([dir=\"ltr\" i])"),
        (":-moz-dir-attr-rtl", ":is([dir=\"rtl\" i])"),
        (":-moz-dir-attr-like-auto", ":is([dir=\"auto\" i], bdi:not([dir=\"ltr\" i], [dir=\"rtl\" i]))"),
    ];
    let needs_patch = DIR_STATES.iter().any(|(s, _)| css.contains(s))
        || css.contains(SELECT_LIST_BOX)
        || css.contains(HEADINGS)
        || css.contains(BORDER)
        || css.contains(MARKER)
        || SYSTEM_FONTS.iter().any(|f| css.contains(f))
        || SYSTEM_COLOR_BORDERS.iter().any(|(b, _)| css.contains(b))
        || css.contains("button,\n::file-selector-button {")
        || FILE_SELECTOR_LINES.iter().any(|l| css.contains(l));
    if !needs_patch {
        return std::borrow::Cow::Borrowed(css);
    }
    let mut out = css.replace(HEADINGS, "@media all {");
    for (from, to) in DIR_STATES {
        out = out.replace(from, to);
    }
    let mut out = out.replace(BORDER, ":-servo-nonzero-border").replace(MARKER, "::marker {");
    for f in SYSTEM_FONTS {
        out = out.replace(f, "font: 13.3333px system-ui;");
    }
    // only the `border:` shorthands (a `border-color: ButtonBorder` only loses its color)
    for (from, to) in SYSTEM_COLOR_BORDERS {
        out = out.replace(from, to);
    }
    for l in FILE_SELECTOR_LINES {
        out = out.replace(l, "");
    }
    // the rule that ends the list with it (`button, ::file-selector-button {`: `white-space: unset`, ...)
    out = out.replace("button,\n::file-selector-button {", "button {");
    out = out.replace(SELECT_LIST_BOX, SELECT_LIST_BOX_SELECTOR);
    std::borrow::Cow::Owned(out)
}

/// Turn on the Stylo prefs this runtime styles with, once per process (they are global): grid,
/// multi-column, and the properties Servo marks unimplemented, which Firefox parses
/// (`content-visibility`, `container-type`, `backdrop-filter`, ...). Every entry point that parses
/// CSS calls it. Until session 75 only a style build did, so `element.style` and `CSS.supports`
/// refused those properties until the page's first layout flush.
pub fn ensure_prefs() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // Stylo 0.22 has no `layout.flexbox.enabled` or `layout.legacy_layout` (flex and the Gecko
        // values are always on).
        static_prefs::set_pref!("layout.grid.enabled", true);
        static_prefs::set_pref!("layout.unimplemented", true);
        static_prefs::set_pref!("layout.columns.enabled", true);
        // `font-format()` and `font-tech()` in @supports and CSS.supports: always parsed in Gecko
        // (pref!(..., gecko = true)); answered as Gecko's gfxPlatform does (vendor/stylo)
        static_prefs::set_pref!("layout.css.font-tech.enabled", true);
    });
}

pub(crate) fn quirks_of(mode: i32) -> QuirksMode {
    match mode {
        2 => QuirksMode::Quirks,
        1 => QuirksMode::LimitedQuirks,
        _ => QuirksMode::NoQuirks,
    }
}

impl Tree {
    /// The element side tables for `dom`: a slot per element, the atoms of its names, and the
    /// `id` and `style=""` attributes parsed. No style is computed.
    pub fn new(dom: Dom) -> Tree {
        Self::with_guard(dom, SharedRwLock::new())
    }

    /// As `new`, with the lock that guards the sheets and declaration blocks (a `SheetCache`'s, so
    /// that sheets parsed for an earlier style build can be used again).
    pub fn with_guard(dom: Dom, guard: SharedRwLock) -> Tree {
        Self::for_document(DOCUMENT, dom, guard)
    }

    /// As `with_guard`, for document `doc` of the DOM (a frame's): only its elements get slots.
    pub fn for_document(doc: NodeId, dom: Dom, guard: SharedRwLock) -> Tree {
        let ns_atoms = ["", "http://www.w3.org/2000/xmlns/", "http://www.w3.org/XML/1998/namespace", "http://www.w3.org/1999/xhtml", "http://www.w3.org/1999/xlink", "http://www.w3.org/1998/Math/MathML", "http://www.w3.org/2000/svg"]
            .map(Namespace::from);
        let url_data = UrlExtraData(ServoArc::new(url::Url::parse("data:text/css;charset=utf-8;base64,").unwrap()));
        let mut tree = Tree {
            dom,
            doc,
            slot_of: Vec::new(),
            slots: Vec::new(),
            id_atoms: Vec::new(),
            style_attrs: Vec::new(),
            pres_hint_blocks: Mutex::default(),
            name_atoms: Vec::new(),
            ns_atoms,
            guard,
            snapshots: SnapshotMap::new(),
            url_data,
            style_blocks: HashMap::new(),
            slots_out_of_order: false,
            shadow_styles: HashMap::new(),
        };
        tree.sync_tables();
        // The arrays grew by doubling; a finished DOM gets no more elements (the DOM arrays are trimmed
        // the same way after parsing). Up to half of `slots` was spare: 0.27 MB on synthetic.
        tree.slots.shrink_to_fit();
        tree.id_atoms.shrink_to_fit();
        tree.style_attrs.shrink_to_fit();
        tree
    }

    /// Bring the side tables up to the DOM: a slot for each element made since the last call (a slot
    /// is never taken back, as a node id is not, GAPS B10; slots are in node id order), the atoms of
    /// new names, and the `id` and `style=""` tables of every element, the parsed blocks kept by text.
    pub fn sync_tables(&mut self) {
        let dom = &self.dom;
        let quirks = quirks_of(dom.document_mode(self.doc));
        for i in self.name_atoms.len() as u32..dom.names.len() as u32 {
            self.name_atoms.push(LocalName::from(dom.names.get(i)));
        }
        let first_new = self.slot_of.len();
        self.slot_of.resize(dom.nodes.len(), u32::MAX);
        // the elements of this document only; one adopted from another document since the last call
        // gets its slot now (so slots are not always in node id order: the side tables are sorted below)
        let doc = self.doc;
        let mine = |id: NodeId| dom.node_documents.is_empty() || dom.node_document(id) == doc;
        let mut out_of_order = false;
        let rescan = if dom.node_documents.is_empty() { first_new } else { 0 };
        for id in rescan..dom.nodes.len() {
            if self.slot_of[id] == u32::MAX && dom.node(id as NodeId).kind == Kind::Element && id as NodeId != NONE && mine(id as NodeId) {
                out_of_order |= id < first_new;
                self.slot_of[id] = self.slots.len() as u32;
                self.slots.push(Slot { data: ElementDataWrapper::default(), flags: AtomicU32::new(0) });
            }
        }
        self.slots_out_of_order |= out_of_order;
        self.id_atoms.clear();
        self.style_attrs.clear();
        let mut blocks = std::mem::take(&mut self.style_blocks);
        let mut used: HashMap<String, ServoArc<Locked<PropertyDeclarationBlock>>> = HashMap::new();
        for id in 0..dom.nodes.len() as NodeId {
            let slot = self.slot_of[id as usize];
            if slot == u32::MAX {
                continue;
            }
            for a in dom.attrs_of(id) {
                match dom.names.get(a.name) {
                    "id" => self.id_atoms.push((slot, Atom::from(dom.attr_value(a)))),
                    "style" => {
                        let text = dom.attr_value(a);
                        let block = match used.get(text) {
                            Some(b) => b.clone(),
                            None => {
                                let b = blocks.remove(text).unwrap_or_else(|| ServoArc::new(self.guard.wrap(parse_style_attribute(text, &self.url_data, None, quirks, CssRuleType::Style))));
                                used.insert(text.to_string(), b.clone());
                                b
                            }
                        };
                        self.style_attrs.push((slot, block));
                    }
                    _ => {}
                }
            }
        }
        self.style_blocks = used;
        if self.slots_out_of_order {
            self.id_atoms.sort_by_key(|e| e.0);
            self.style_attrs.sort_by_key(|e| e.0);
        }
    }

    /// The compatibility mode of the document styled.
    pub fn mode(&self) -> i32 {
        self.dom.document_mode(self.doc)
    }

    /// Whether `node` is in the document styled (Dom::is_connected_to).
    pub fn is_connected(&self, node: NodeId) -> bool {
        self.dom.is_connected_to(node, self.doc)
    }

    /// Give back the DOM.
    pub fn into_dom(self) -> Dom {
        self.dom
    }
}

/// `querySelector` (`max` 1) and `querySelectorAll` (`max` 0) below `root`, by Stylo's
/// `dom_apis::query_selector`, which Firefox's `Servo_SelectorList_QueryFirst` and `QueryAll` call.
/// The element side tables are built for the query and dropped after it (the DOM has no styles while
/// script runs), so a query costs a pass over the whole DOM. `None` when the selector does not parse.
pub fn query_selector(dom: Dom, root: NodeId, selector: &str, max: usize) -> (Dom, Option<Vec<NodeId>>) {
    ensure_prefs();
    use style::dom_apis::{self, MayUseInvalidation, QueryAll, QueryFirst};
    use style::selector_parser::SelectorParser;
    let tree = Tree::new(dom);
    let result = match SelectorParser::parse_author_origin_no_namespace(selector, &tree.url_data) {
        Err(_) => None,
        Ok(list) => {
            let _scope = TreeScope::enter(&tree);
            let root = tree.handle(root).as_node();
            if max == 1 {
                let mut first: Option<H> = None;
                dom_apis::query_selector::<H, QueryFirst>(root, &list, &mut first, MayUseInvalidation::Yes);
                Some(first.map(|e| e.id).into_iter().collect())
            } else {
                let mut all = Default::default();
                dom_apis::query_selector::<H, QueryAll>(root, &list, &mut all, MayUseInvalidation::Yes);
                Some(all.iter().map(|e: &H| e.id).collect())
            }
        }
    };
    (tree.into_dom(), result)
}

/// `element.matches(selector)` and `element.closest(selector)` (Gecko's Servo_SelectorList_Matches
/// and Servo_SelectorList_Closest): with `closest` false, `element` if it matches and `NONE` if not;
/// with `closest` true, the nearest inclusive ancestor element that matches, or `NONE`. `None` for a
/// selector that does not parse. Like `query_selector`, it builds the side tables for each call.
pub fn match_selector(dom: Dom, element: NodeId, selector: &str, closest: bool) -> (Dom, Option<NodeId>) {
    ensure_prefs();
    use style::dom_apis;
    use style::selector_parser::SelectorParser;
    let tree = Tree::new(dom);
    let quirks = quirks_of(tree.dom.document_mode(tree.dom.node_document(element)));
    let result = match SelectorParser::parse_author_origin_no_namespace(selector, &tree.url_data) {
        Err(_) => None,
        Ok(list) => {
            let _scope = TreeScope::enter(&tree);
            let e = tree.handle(element);
            Some(if closest {
                dom_apis::element_closest(e, &list, quirks).map_or(NONE, |m| m.id)
            } else if dom_apis::element_matches(&e, &list, quirks) {
                element
            } else {
                NONE
            })
        }
    };
    (tree.into_dom(), result)
}

impl StyleDom {
    /// Drop the style data (slots, computed styles, the page's sheets and stylist) and give back the
    /// DOM (stage 4: hibernation). Stylo's global cache of the user-agent cascade data stays, as it is
    /// shared between pages in Gecko too.
    pub fn into_dom(self) -> Dom {
        self.tree.into_dom()
    }

    /// Wrap a parsed DOM: set up the stylist with the user-agent sheet and the page's `<style>`
    /// elements. Call `resolve` afterwards to compute styles.
    pub fn new(dom: Dom, viewport: (f32, f32)) -> StyleDom {
        Self::with_link_sheets(dom, viewport, HashMap::new())
    }

    /// As `new`, with the sheets of the page's stylesheet links (`link_sheets`).
    pub fn with_link_sheets(dom: Dom, viewport: (f32, f32), link_sheets: HashMap<NodeId, std::sync::Arc<str>>) -> StyleDom {
        Self::with_sheet_cache(dom, viewport, link_sheets, SheetCache::default())
    }

    /// As `with_link_sheets`, parsing only the sheets that `cache` does not have from an earlier
    /// style build of the page (Gecko parses a sheet once and keeps it; a style build here starts
    /// from a new stylist, GAPS B9). Take the cache back with `take_sheet_cache` for the next build.
    pub fn with_sheet_cache(dom: Dom, viewport: (f32, f32), link_sheets: HashMap<NodeId, std::sync::Arc<str>>, sheet_cache: SheetCache) -> StyleDom {
        Self::for_document(DOCUMENT, dom, viewport, link_sheets, sheet_cache)
    }

    /// As `with_sheet_cache`, for document `doc` of the DOM: a frame's document, styled at its
    /// viewport (DESIGN, "The frames' layout (S5)"; Gecko gives each document its own style set).
    pub fn for_document(doc: NodeId, dom: Dom, viewport: (f32, f32), link_sheets: HashMap<NodeId, std::sync::Arc<str>>, mut sheet_cache: SheetCache) -> StyleDom {
        ensure_prefs();
        let quirks = quirks_of(dom.document_mode(doc));
        sheet_cache.used.clear();
        let tree = Tree::for_document(doc, dom, sheet_cache.guard.clone());
        let device = make_device(quirks, viewport);
        let mut sd = StyleDom {
            tree,
            stylist: Stylist::new(device, quirks),
            author_sheets: Vec::new(),
            link_sheets,
            parse_errors: Vec::new(),
            sheet_cache,
            viewport,
            sheets_signature: Vec::new(),
            markers: Mutex::default(),
        };
        sd.update_style_set();
        sd
    }

    /// Collect the page's sheets (from the cache when they were parsed before) and, if they are not
    /// the ones the stylist has, build a new stylist and the shadow trees' author styles from them.
    /// Returns whether it did (every element's style must then be computed again).
    fn update_style_set(&mut self) -> bool {
        self.parse_errors.clear();
        let mut adopted = HashMap::new();
        let page = self.page_sheets(&mut adopted);
        let shadow = self.shadow_sheets(&mut adopted);
        let id = |s: &DocumentStyleSheet| (&*s.0 as *const Stylesheet) as usize;
        let mut signature: Vec<(NodeId, usize)> = page.iter().map(|s| (DOCUMENT, id(s))).collect();
        for (root, sheets) in &shadow {
            signature.extend(sheets.iter().map(|s| (*root, id(s))));
            // an empty shadow tree's styles still exist (it has a cascade data)
            signature.push((*root, 0));
        }
        if signature == self.sheets_signature && !self.sheets_signature.is_empty() {
            return false;
        }
        self.sheets_signature = signature;

        let quirks = quirks_of(self.tree.mode());
        self.stylist = Stylist::new(make_device(quirks, self.viewport), quirks);
        let ua_sheets = FIREFOX_UA.iter().filter(|(name, _)| *name != "quirk.css" || quirks == QuirksMode::Quirks);
        for (name, sheet, errors) in ua_sheets_for(&self.tree, ua_sheets) {
            self.stylist.append_stylesheet(sheet, &UA_LOCK.read());
            self.parse_errors.push((name.to_string(), errors));
        }
        for sheet in &page {
            self.stylist.append_stylesheet(sheet.clone(), &self.tree.guard.read());
        }
        self.author_sheets = page;
        self.tree.shadow_styles.clear();
        for (root, sheets) in shadow {
            let mut styles = style::author_styles::AuthorStyles::<DocumentStyleSheet>::new();
            let guard = self.tree.guard.read();
            for sheet in sheets {
                styles.stylesheets.append_stylesheet(None, &style::stylesheets::CustomMediaMap::default(), sheet, &guard);
            }
            drop(guard);
            self.tree.shadow_styles.insert(root, styles);
        }
        true
    }

    /// Put the DOM back (`detach_dom` took it) after it changed as `ops` say, and make the next
    /// `resolve` style what the changes invalidated (Gecko: the RestyleManager's posted restyles and
    /// snapshots, processed by ServoStyleSet::StyleDocument). A change of the page's sheets restyles
    /// everything.
    pub fn attach_dom(&mut self, dom: Dom, link_sheets: HashMap<NodeId, std::sync::Arc<str>>, ops: &[restyle::RestyleOp]) -> bool {
        self.attach_dom_only(dom, link_sheets);
        self.apply_restyle_ops(ops)
    }

    /// The first half of `attach_dom`: the DOM back in place with its side tables brought up to date,
    /// the style data as the last build left it. Layout reads the DOM and the old styles in between
    /// (the frames of removed nodes are destroyed then, as Gecko destroys them at the removal).
    pub fn attach_dom_only(&mut self, dom: Dom, link_sheets: HashMap<NodeId, std::sync::Arc<str>>) {
        self.tree.dom = dom;
        self.link_sheets = link_sheets;
        self.tree.sync_tables();
        *self.markers.lock().unwrap() = Markers::default();
    }

    /// The second half of `attach_dom`: what the changes invalidate, for the next `resolve`. True when
    /// everything is to be restyled (the sheets changed, or an `Everything` op).
    pub fn apply_restyle_ops(&mut self, ops: &[restyle::RestyleOp]) -> bool {
        self.sheet_cache.used.clear();
        let rebuilt = self.update_style_set();
        // the sheets no tree has any more
        let used = std::mem::take(&mut self.sheet_cache.used);
        self.sheet_cache.sheets.retain(|k, _| used.contains(k));
        // (kept: `take_sheet_cache` keeps the sheets this build used)
        self.sheet_cache.used = used;
        if rebuilt || ops.iter().any(|op| matches!(op, restyle::RestyleOp::Everything)) {
            self.clear_all_style_data();
            return true;
        }
        for op in ops {
            self.apply_restyle_op(op);
        }
        false
    }

    /// The elements whose style changed in the last `resolve`, with Stylo's restyle damage (the bits of
    /// `ServoRestyleDamage`: 0b1111 is `RELAYOUT`), and the damage cleared: what Gecko's
    /// RestyleManager::ProcessPostTraversal reads as each element's change hint. An element styled for
    /// the first time has all bits.
    pub fn take_damage(&self) -> Vec<(NodeId, u16)> {
        let mut out = Vec::new();
        for (node, &slot) in self.tree.slot_of.iter().enumerate() {
            if slot == u32::MAX {
                continue;
            }
            let slot = &self.tree.slots[slot as usize];
            if !slot.has_data() {
                continue;
            }
            let mut data = slot.data.borrow_mut();
            if !data.damage.is_empty() {
                out.push((node as NodeId, data.damage.bits()));
                data.damage = Default::default();
            }
        }
        out
    }

    /// Put back the DOM `detach_dom` took, unchanged since: the side tables and the caches are as
    /// they were, so nothing is brought up to date (a style query between two flushes).
    /// A new viewport (a frame resized, DESIGN "The frames' layout (S5)"): the next restyle makes the
    /// stylist again for it, which restyles everything (Gecko re-evaluates the media queries, and
    /// restyles what they change).
    pub fn set_viewport(&mut self, viewport: (f32, f32)) {
        if self.viewport != viewport {
            self.viewport = viewport;
            self.sheets_signature.clear();
        }
    }

    pub fn attach_dom_unchanged(&mut self, dom: Dom) {
        self.tree.dom = dom;
    }

    /// Take the DOM out (the runtime changes it between style builds); the style data stays.
    pub fn detach_dom(&mut self) -> Dom {
        std::mem::take(&mut self.tree.dom)
    }

    /// Every element unstyled: the next `resolve` styles the whole page.
    pub fn clear_all_style_data(&mut self) {
        for slot in &self.tree.slots {
            *slot.data.borrow_mut() = Default::default();
            slot.flags.fetch_and(!NOT_SELECTOR_FLAGS, Relaxed);
        }
        self.tree.snapshots.clear();
    }

    fn apply_restyle_op(&mut self, op: &restyle::RestyleOp) {
        use restyle::RestyleOp;
        let dom = &self.tree.dom;
        match op {
            RestyleOp::Everything => unreachable!(),
            // Gecko: RestyleManager::ClearServoDataFromSubtree (over the flattened tree)
            RestyleOp::ClearSubtree(root) => {
                let mut stack = vec![*root];
                while let Some(n) = stack.pop() {
                    let Some(slot) = self.tree.slot(n) else { continue };
                    if !slot.has_data() {
                        continue;
                    }
                    *slot.data.borrow_mut() = Default::default();
                    slot.flags.fetch_and(!NOT_SELECTOR_FLAGS, Relaxed);
                    stack.extend(flattened_children(dom, n));
                }
            }
            // Gecko: Servo_NoteExplicitHints
            RestyleOp::Hint(element, hint) => {
                if dom.is_connected_to(*element, self.tree.doc) && self.tree.slot(*element).is_some_and(|s| s.has_data()) {
                    self.tree.slot(*element).unwrap().data.borrow_mut().hint.insert(*hint);
                    self.note_dirty_ancestors(*element);
                }
            }
            // the new elements have no style data: the traversal must reach them
            RestyleOp::Inserted(node) => {
                if dom.is_connected_to(*node, self.tree.doc) {
                    self.note_dirty_ancestors(*node);
                }
            }
            // Gecko: ServoElementSnapshot, kept by the RestyleManager until the traversal
            RestyleOp::Snapshot(element, data) => {
                if !dom.is_connected_to(*element, self.tree.doc) || !self.tree.slot(*element).is_some_and(|s| s.has_data()) {
                    return;
                }
                let snapshot = self.servo_snapshot(data);
                self.tree.snapshots.insert(style::dom::OpaqueNode(*element as usize), snapshot);
                let slot = self.tree.slot(*element).unwrap();
                slot.flags.fetch_and(!HANDLED_SNAPSHOT, Relaxed);
                slot.flags.fetch_or(HAS_SNAPSHOT, Relaxed);
                self.note_dirty_ancestors(*element);
            }
        }
    }

    /// Set the dirty-descendants bit up the flattened tree from `node`'s parent, to the first element
    /// that has it or has no style data (Gecko: Element::NoteDirtyForServo's propagation).
    fn note_dirty_ancestors(&self, node: NodeId) {
        let dom = &self.tree.dom;
        let mut parent = dom.flattened_parent(node);
        while parent != NONE {
            let Some(slot) = self.tree.slot(parent) else { break };
            if !slot.has_data() || slot.flags.load(Relaxed) & HAS_DIRTY_DESCENDANTS != 0 {
                break;
            }
            slot.flags.fetch_or(HAS_DIRTY_DESCENDANTS, Relaxed);
            parent = dom.flattened_parent(parent);
        }
    }

    /// Stylo's snapshot from the recorder's (the attribute values as Servo's AttrValue: `id` an atom,
    /// `class` and `part` token lists).
    fn servo_snapshot(&self, data: &restyle::ElementSnapshotData) -> style::selector_parser::ServoElementSnapshot {
        use style::attr::{AttrIdentifier, AttrValue};
        use style::values::GenericAtomIdent;
        let mut snapshot = style::selector_parser::ServoElementSnapshot::new();
        snapshot.state = data.state.map(style_dom::ElementState::from_bits_retain);
        snapshot.attrs = data.attrs.as_ref().map(|attrs| {
            attrs
                .iter()
                .map(|(name, value, ns)| {
                    let local_name = LocalName::from(name.as_str());
                    let ident = AttrIdentifier { local_name: GenericAtomIdent(local_name.clone()), name: GenericAtomIdent(local_name), namespace: GenericAtomIdent(self.tree.ns_atoms[*ns as usize].clone()), prefix: None };
                    let value = match name.as_str() {
                        "id" => AttrValue::from_atomic(value.clone()),
                        "class" | "part" => AttrValue::from_serialized_tokenlist(value.clone()),
                        _ => AttrValue::String(value.clone()),
                    };
                    (ident, value)
                })
                .collect()
        });
        snapshot.changed_attrs = data.changed_attrs.iter().map(|n| GenericAtomIdent(LocalName::from(n.as_str()))).collect();
        snapshot.id_changed = data.id_changed;
        snapshot.class_changed = data.class_changed;
        snapshot.other_attributes_changed = data.other_attributes_changed;
        snapshot
    }

    /// What the restyle recorder reads from this style build: the selector flags of the elements that
    /// have any (the ones RestyleManager reads), and the elements that have style data.
    pub fn restyle_state(&self) -> (HashMap<NodeId, u32>, std::collections::HashSet<NodeId>) {
        let mut flags = HashMap::new();
        let mut styled = std::collections::HashSet::new();
        for (node, &slot) in self.tree.slot_of.iter().enumerate() {
            if slot == u32::MAX {
                continue;
            }
            let f = self.tree.slots[slot as usize].flags.load(Relaxed);
            if f & HAS_DATA != 0 {
                styled.insert(node as NodeId);
            }
            if f & !NOT_SELECTOR_FLAGS != 0 {
                flags.insert(node as NodeId, f & !NOT_SELECTOR_FLAGS);
            }
        }
        (flags, styled)
    }

    /// A constructed sheet, parsed once per style build however many trees adopted it, so that its
    /// adopters share one sheet (and Stylo's author data cache, one cascade data for the same sheet
    /// list, as Gecko's adopters share a StyleSheet).
    fn adopted_sheet(&mut self, id: u32, parsed: &mut HashMap<u32, DocumentStyleSheet>) -> DocumentStyleSheet {
        if let Some(sheet) = parsed.get(&id) {
            return sheet.clone();
        }
        let sheet = &self.tree.dom.constructed_sheets[id as usize];
        let (css, ops, media) = (sheet.text.clone(), sheet.ops.clone(), (!sheet.media.is_empty()).then(|| sheet.media.clone()));
        let (sheet, errors) = self.cached_sheet(sheet_key_text(&css, &ops), Origin::Author, media.clone(), |tree| tree.make_sheet_with_ops(&css, &ops, true, media.as_deref()));
        self.parse_errors.push((format!("constructed sheet {id}"), errors));
        parsed.insert(id, sheet.clone());
        sheet
    }

    /// The `<style>` elements of a tree in tree order, not entering shadow trees below it (a shadow
    /// tree's styles are its own).
    fn style_elements(dom: &Dom, root: NodeId) -> Vec<NodeId> {
        Self::sheet_owners(dom, root, &HashMap::new())
    }

    /// The `<style>` elements of a tree, and its `<link>` elements whose sheet loaded (`links`), in
    /// tree order, not entering shadow trees below it: the order of their sheets in the cascade, as
    /// Gecko inserts each sheet at its owner's tree position (DocumentOrShadowRoot), whenever it
    /// loaded.
    fn sheet_owners(dom: &Dom, root: NodeId, links: &HashMap<NodeId, std::sync::Arc<str>>) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let n = dom.node(id);
            if n.kind == Kind::Element && n.ns == mrt_dom::NS_HTML {
                let tag = dom.tag(id);
                if tag == "style" || (tag == "link" && links.contains_key(&id)) {
                    out.push(id);
                    continue;
                }
            }
            let kids: Vec<NodeId> = dom.children(id).collect();
            stack.extend(kids.into_iter().rev());
        }
        out
    }

    /// The sheet of a `<style>` (its text) or of a loaded `<link>`, with the element's `media`
    /// attribute as its media list (LinkStyle::GetStyleSheetInfo).
    fn owner_sheet(&mut self, id: NodeId) -> DocumentStyleSheet {
        let dom = &self.tree.dom;
        let (css, name): (std::sync::Arc<str>, String) = if let Some(base) = dom.owner_sheet(id).and_then(|s| s.base.as_deref()) {
            // (a link whose rules a script changed: the text they were changed from)
            (base.into(), format!("<link> #{id}"))
        } else if dom.tag(id) == "link" {
            (self.link_sheets.get(&id).expect("a loaded link").clone(), format!("<link> #{id}"))
        } else {
            (dom.children(id).filter(|&c| dom.node(c).kind == Kind::Text).map(|c| dom.text_of(c)).collect::<String>().into(), format!("<style> #{id}"))
        };
        let media = dom.get_attribute(id, "media").map(str::to_string);
        // the changes a script made to its rules, replayed over the text
        let ops = dom.owner_sheet(id).map(|s| s.ops.clone()).unwrap_or_default();
        let (sheet, errors) = if ops.is_empty() {
            self.cached_sheet(css.clone(), Origin::Author, media.clone(), |tree| tree.make_sheet(&css, Origin::Author, media.as_deref()))
        } else {
            self.cached_sheet(sheet_key_text(&css, &ops), Origin::Author, media.clone(), |tree| tree.make_sheet_with_ops(&css, &ops, false, media.as_deref()))
        };
        self.parse_errors.push((name, errors));
        sheet
    }

    /// The sheet for (text, origin, media) from the cache, or `parse`d now and kept. A parsed sheet is
    /// never changed here (a sheet the CSSOM changed comes as the text of its new rules), so a sheet
    /// of the same text, origin and media is the same sheet.
    fn cached_sheet(&mut self, text: std::sync::Arc<str>, origin: Origin, media: Option<String>, parse: impl FnOnce(&Tree) -> (DocumentStyleSheet, usize)) -> (DocumentStyleSheet, usize) {
        let key = SheetKey { text, origin: origin as u8, media, quirks: self.tree.mode() };
        let cache = &mut self.sheet_cache;
        // (on a hit, `used` takes the cached key, whose text is the cached entry's: the caller's
        // text is a new copy of the sheet's, which `used` would otherwise keep beside it, 4 MB on
        // Gmail, GAPS B94)
        let (key, found) = match cache.sheets.get_key_value(&key) {
            Some((cached, found)) => (cached.clone(), found.clone()),
            None => {
                let parsed = parse(&self.tree);
                cache.sheets.insert(key.clone(), parsed.clone());
                (key, parsed)
            }
        };
        cache.used.insert(key);
        found
    }

    /// The sheet cache for the next style build of the page, without the sheets this build did not
    /// use (a removed `<style>` or link, an old text of a `<style>` that script changed).
    pub fn take_sheet_cache(&mut self) -> SheetCache {
        let mut cache = std::mem::take(&mut self.sheet_cache);
        let used = std::mem::take(&mut cache.used);
        cache.sheets.retain(|k, _| used.contains(k));
        cache
    }

    /// The sheets of each connected shadow tree: its `<style>` elements in tree order, then its
    /// adopted sheets (Gecko: ShadowRoot::AppendStyleSheet when a `<style>` binds into the tree, and
    /// the adopted ones after them, DocumentOrShadowRoot::StyleOrderIndexOfSheet), by shadow root.
    fn shadow_sheets(&mut self, adopted: &mut HashMap<u32, DocumentStyleSheet>) -> Vec<(NodeId, Vec<DocumentStyleSheet>)> {
        let dom = &self.tree.dom;
        let doc = self.tree.doc;
        let mut roots: Vec<NodeId> = dom.shadow_roots.values().copied().filter(|&r| dom.is_connected_to(r, doc)).collect();
        roots.sort();
        let mut out = Vec::new();
        for root in roots {
            let mut sheets = Vec::new();
            // links in shadow trees are not loaded (GAPS B25)
            for id in Self::style_elements(&self.tree.dom, root) {
                if !self.tree.dom.owner_sheet_disabled(id) {
                    sheets.push(self.owner_sheet(id));
                }
            }
            for id in self.tree.dom.adopted_sheets(root).to_vec() {
                if !self.tree.dom.constructed_sheets[id as usize].disabled {
                    sheets.push(self.adopted_sheet(id, adopted));
                }
            }
            out.push((root, sheets));
        }
        out
    }

    /// Every `<style>` element and loaded stylesheet `<link>` of the document's tree, in document
    /// order; then the document's adopted sheets, which come after them.
    fn page_sheets(&mut self, adopted: &mut HashMap<u32, DocumentStyleSheet>) -> Vec<DocumentStyleSheet> {
        let mut out = Vec::new();
        // (a disabled sheet stays in the document's list, and out of the cascade: StyleSheet::SetDisabled)
        for id in Self::sheet_owners(&self.tree.dom, self.tree.doc, &self.link_sheets) {
            if !self.tree.dom.owner_sheet_disabled(id) {
                out.push(self.owner_sheet(id));
            }
        }
        for id in self.tree.dom.adopted_sheets(self.tree.doc).to_vec() {
            if !self.tree.dom.constructed_sheets[id as usize].disabled {
                out.push(self.adopted_sheet(id, adopted));
            }
        }
        out
    }

    /// Build the stylist's rule tables from the sheets. `resolve` does this too; it is public so
    /// its memory can be measured on its own.
    pub fn flush_stylist(&mut self) {
        {
            let tree = &self.tree;
            let _scope = TreeScope::enter(tree);
            let guards = StylesheetGuards { author: &tree.guard.read(), ua_or_user: &UA_LOCK.read() };
            self.stylist.flush(&guards);
        }
        // each shadow tree's cascade data, after the document's (Gecko: ServoStyleSet::UpdateStylist
        // flushes the shadow roots' author styles with the stylist's)
        let guard = self.tree.guard.read();
        for styles in self.tree.shadow_styles.values_mut() {
            styles.flush(&mut self.stylist, &guard);
        }
    }

    /// Compute the style of every element.
    pub fn resolve(&mut self) {
        self.flush_stylist();
        // (a block: the traversal borrows the tree, which the snapshots below are cleared from)
        {
            thread_state::enter(ThreadState::LAYOUT);
            let tree = &self.tree;
            let _scope = TreeScope::enter(tree);
            let guards = StylesheetGuards { author: &tree.guard.read(), ua_or_user: &UA_LOCK.read() };
            let root = tree.handle(tree.doc).first_element_child().expect("document has no root element");

            let context = SharedStyleContext {
                traversal_flags: TraversalFlags::empty(),
                stylist: &self.stylist,
                options: GLOBAL_STYLE_DATA.options.clone(),
                guards,
                visited_styles_enabled: false,
                animations: Default::default(),
                current_time_for_animations: 0.0,
                snapshot_map: &tree.snapshots,
                registered_speculative_painters: &NoPainters,
            };
            let token = RecalcStyle::pre_traverse(root, &context);
            if token.should_traverse() {
                let traverser = RecalcStyle { context };
                style::driver::traverse_dom(&traverser, token, None);
            }
            thread_state::exit(ThreadState::LAYOUT);
        }
        // Gecko: RestyleManager::ClearSnapshots
        let snapshotted: Vec<NodeId> = self.tree.snapshots.keys().map(|n| n.0 as NodeId).collect();
        for n in snapshotted {
            if let Some(slot) = self.tree.slot(n) {
                slot.flags.fetch_and(!(HAS_SNAPSHOT | HANDLED_SNAPSHOT), Relaxed);
            }
        }
        self.tree.snapshots.clear();
    }

    /// The style of an anonymous box (one with no element, such as the box around a text node, a list
    /// marker, the viewport or the canvas): the inherited properties of `parent` and the initial
    /// values for the rest, as Gecko's `ResolveInheritingAnonymousBoxStyle` gives. `display` replaces
    /// the initial `inline` when given. For the viewport and the canvas, which have no parent, pass
    /// `None` for `parent` to inherit from the initial values.
    pub fn anonymous_style(&self, parent: Option<&ComputedValues>, display: Option<style::values::specified::box_::Display>) -> ServoArc<ComputedValues> {
        let tree = &self.tree;
        let guards = StylesheetGuards { author: &tree.guard.read(), ua_or_user: &UA_LOCK.read() };
        let parent = parent.unwrap_or_else(|| self.stylist.device().default_computed_values());
        let mut style = self.stylist.style_for_anonymous::<H>(&guards, &style::selector_parser::PseudoElement::ServoAnonymousBox, parent);
        if let Some(display) = display {
            ServoArc::make_mut(&mut style).mutate_box().set_display(display);
        }
        style
    }

    /// The style of the `::before`, `::after` or `::marker` of an element, once `resolve` has run;
    /// `None` when the element has no such pseudo-element (no rules matched, `display: none`, or for
    /// `::before` and `::after` a `content` of `normal`, `none` or nothing).
    ///
    /// `::before` and `::after` are read from what Stylo's traversal stored in the element's data.
    /// `::marker` is a lazy pseudo-element in Stylo: it is computed here on the first request (it
    /// exists for every element, like Gecko's `ProbePseudoElementStyle` for it) and kept in a side
    /// table. Asking for `display: list-item` elements only is the caller's business.
    pub fn pseudo_style(&self, id: NodeId, pseudo: style::selector_parser::PseudoElement) -> Option<ServoArc<ComputedValues>> {
        use style::selector_parser::PseudoElement;
        let slot = self.tree.slot(id).filter(|s| s.has_data())?;
        match pseudo {
            PseudoElement::Before | PseudoElement::After => {
                let data = slot.data.borrow();
                let style = data.styles.pseudos.get(&pseudo)?.clone();
                pseudo.should_exist(&style).then_some(style)
            }
            PseudoElement::Marker => {
                if let Some(s) = self.markers.lock().unwrap().by_node.get(&id) {
                    return Some(s.clone());
                }
                let primary = slot.data.borrow().styles.get_primary().cloned()?;
                let tree = &self.tree;
                let _scope = TreeScope::enter(tree);
                let guards = StylesheetGuards { author: &tree.guard.read(), ua_or_user: &UA_LOCK.read() };
                let style = self.stylist.lazily_compute_pseudo_element_style(
                    &guards,
                    tree.handle(id),
                    &pseudo,
                    style::stylist::RuleInclusion::All,
                    &primary,
                    false,
                    None,
                )?;
                if !pseudo.should_exist(&style) {
                    return None;
                }
                let mut markers = self.markers.lock().unwrap();
                let style = match style.rules.clone() {
                    Some(rules) => markers.by_rules.entry((rules, &*primary as *const ComputedValues as usize)).or_insert(style).clone(),
                    None => style,
                };
                markers.by_node.insert(id, style.clone());
                Some(style)
            }
            _ => None,
        }
    }

    /// The style `getComputedStyle(element, pseudo)` reads: the pseudo-element's style when it exists
    /// (`pseudo_style`), else the style it would have, computed and not kept, as Gecko's
    /// `ResolveStyleLazily` gives it for a pseudo-element that generates no box (an `::after` with no
    /// `content` reads `content: none`).
    pub fn pseudo_style_for_query(&self, id: NodeId, pseudo: style::selector_parser::PseudoElement) -> Option<ServoArc<ComputedValues>> {
        if let Some(style) = self.pseudo_style(id, pseudo.clone()) {
            return Some(style);
        }
        let primary = self.style_for_query(id)?;
        let tree = &self.tree;
        let _scope = TreeScope::enter(tree);
        let guards = StylesheetGuards { author: &tree.guard.read(), ua_or_user: &UA_LOCK.read() };
        self.stylist.lazily_compute_pseudo_element_style(&guards, tree.handle(id), &pseudo, style::stylist::RuleInclusion::All, &primary, false, None)
    }

    /// (elements with an eager pseudo-element style, `::before` styles, `::after` styles), for
    /// the memory report.
    pub fn eager_pseudo_stats(&self) -> (usize, usize, usize) {
        use style::selector_parser::PseudoElement;
        let (mut elements, mut before, mut after) = (0, 0, 0);
        for slot in self.tree.slots.iter().filter(|s| s.has_data()) {
            let data = slot.data.borrow();
            let p = &data.styles.pseudos;
            if p.is_empty() {
                continue;
            }
            elements += 1;
            before += p.get(&PseudoElement::Before).is_some() as usize;
            after += p.get(&PseudoElement::After).is_some() as usize;
        }
        (elements, before, after)
    }

    /// The computed style of an element, once `resolve` has run.
    pub fn computed(&self, id: NodeId) -> Option<ServoArc<ComputedValues>> {
        let slot = self.tree.slot(id).filter(|s| s.has_data())?;
        let data = slot.data.borrow();
        data.styles.get_primary().cloned()
    }

    /// The style `getComputedStyle` reads for an element, once `resolve` has run: its computed style,
    /// or for an element the traversal did not style (inside a `display: none` subtree) a style
    /// resolved for it alone and not kept, as Gecko's `ServoStyleSet::ResolveStyleLazily` does.
    /// `None` for a node that is not an element.
    ///
    /// Gecko: layout/style/nsComputedDOMStyle.cpp DoGetComputedStyleNoFlush.
    pub fn style_for_query(&self, id: NodeId) -> Option<ServoArc<ComputedValues>> {
        if let Some(style) = self.computed(id) {
            return Some(style);
        }
        self.tree.slot(id)?;
        let tree = &self.tree;
        let _scope = TreeScope::enter(tree);
        let guards = StylesheetGuards { author: &tree.guard.read(), ua_or_user: &UA_LOCK.read() };
        let shared = SharedStyleContext {
            traversal_flags: TraversalFlags::empty(),
            stylist: &self.stylist,
            options: GLOBAL_STYLE_DATA.options.clone(),
            guards,
            visited_styles_enabled: false,
            animations: Default::default(),
            current_time_for_animations: 0.0,
            snapshot_map: &tree.snapshots,
            registered_speculative_painters: &NoPainters,
        };
        let mut thread_local = style::context::ThreadLocalStyleContext::new();
        let mut context = style::context::StyleContext { shared: &shared, thread_local: &mut thread_local };
        thread_state::enter(ThreadState::LAYOUT);
        let styles = style::traversal::resolve_style(&mut context, tree.handle(id), style::stylist::RuleInclusion::All, None, None);
        thread_state::exit(ThreadState::LAYOUT);
        Some(styles.primary().clone())
    }
}

impl Tree {
    pub fn handle(&self, id: NodeId) -> H<'_> {
        H::new(id)
    }

    pub fn slot(&self, id: NodeId) -> Option<&Slot> {
        self.slot_index(id).map(|i| &self.slots[i as usize])
    }

    /// Parse a sheet; also returns how many errors the parser reported.
    /// The `id` attribute of an element as an atom.
    pub fn id_atom(&self, node: NodeId) -> Option<&Atom> {
        let slot = self.slot_index(node)?;
        let i = self.id_atoms.binary_search_by_key(&slot, |e| e.0).ok()?;
        Some(&self.id_atoms[i].1)
    }

    /// The parsed `style=""` of an element.
    pub fn style_attr(&self, node: NodeId) -> Option<&ServoArc<Locked<PropertyDeclarationBlock>>> {
        let slot = self.slot_index(node)?;
        let i = self.style_attrs.binary_search_by_key(&slot, |e| e.0).ok()?;
        Some(&self.style_attrs[i].1)
    }

    // None as well for a node made after the tree's last attach: once `--max-flushes` is used up,
    // layout reads are answered from an older DOM that never saw it (GAPS B43's runs)
    fn slot_index(&self, node: NodeId) -> Option<u32> {
        match self.slot_of.get(node as usize).copied() {
            None | Some(u32::MAX) => None,
            Some(i) => Some(i),
        }
    }

    /// A sheet parsed from `css`; `media` is its owner's `media` attribute, parsed as Gecko's
    /// Servo_MediaList_SetText parses it (author origin, no quirks, no error reporting).
    fn make_sheet(&self, css: &str, origin: Origin, media: Option<&str>) -> (DocumentStyleSheet, usize) {
        self.make_sheet_with_lock(css, origin, media, &self.guard)
    }

    /// A sheet's media list from its text (None: no list).
    fn media_list(&self, media: Option<&str>) -> MediaList {
        match media {
            None => MediaList::empty(),
            Some(text) => {
                let mut parser = cssparser::Parser::new(text);
                let mut context = style::parser::ParserContext::new(
                    Origin::Author,
                    &self.url_data,
                    Some(CssRuleType::Media),
                    style_traits::ParsingMode::DEFAULT,
                    QuirksMode::NoQuirks,
                    /* namespaces = */ Default::default(),
                    None,
                    None,
                    /* attr_taint */ Default::default(),
                );
                MediaList::parse(&mut context, &mut parser)
            }
        }
    }

    /// An author sheet from `css` changed by a script's CSSOM calls (`ops`), as `cssom::parse_with_ops`.
    fn make_sheet_with_ops(&self, css: &str, ops: &[mrt_dom::RuleOp], constructed: bool, media: Option<&str>) -> (DocumentStyleSheet, usize) {
        let errors = ErrorCount::default();
        let contents = cssom::parse_with_ops(css, ops, self.url_data.clone(), &self.guard, Some(&errors), self.mode(), constructed);
        let media = self.media_list(media);
        let sheet = Stylesheet { contents: self.guard.wrap(contents), shared_lock: self.guard.clone(), media: ServoArc::new(self.guard.wrap(media)), disabled: Default::default() };
        (DocumentStyleSheet(ServoArc::new(sheet)), errors.0.load(Relaxed))
    }

    /// As `make_sheet`, guarded by `lock` (the user-agent sheets' is the process's, UA_LOCK).
    fn make_sheet_with_lock(&self, css: &str, origin: Origin, media: Option<&str>, lock: &SharedRwLock) -> (DocumentStyleSheet, usize) {
        let errors = ErrorCount::default();
        let media = self.media_list(media);
        let sheet = DocumentStyleSheet(ServoArc::new(Stylesheet::from_str(
            css,
            self.url_data.clone(),
            origin,
            ServoArc::new(lock.wrap(media)),
            lock.clone(),
            None,
            Some(&errors),
            quirks_of(self.mode()),
            AllowImportRules::No,
        )));
        (sheet, errors.0.load(Relaxed))
    }
}

/// Stylo's style traversal, over elements only.
struct RecalcStyle<'a> {
    context: SharedStyleContext<'a>,
}

impl<E: TElement> DomTraversal<E> for RecalcStyle<'_> {
    fn process_preorder<F: FnMut(E::ConcreteNode)>(
        &self,
        context: &mut style::context::StyleContext<E>,
        node: E::ConcreteNode,
        note_child: F,
    ) {
        let Some(el) = node.as_element() else { return };
        let mut data = unsafe { el.ensure_data() };
        style::traversal::recalc_style_at(self, context, el, &mut data, note_child);
        unsafe { el.unset_dirty_descendants() };
    }

    fn needs_postorder_traversal() -> bool {
        false
    }

    fn process_postorder(&self, _context: &mut style::context::StyleContext<E>, _node: E::ConcreteNode) {
        unreachable!()
    }

    fn shared_context(&self) -> &SharedStyleContext<'_> {
        &self.context
    }
}

thread_local! {
    /// The tree that handles on this thread refer to; set while styles are being resolved.
    static CURRENT_TREE: Cell<*const Tree> = const { Cell::new(std::ptr::null()) };
}

/// Makes `tree` the one handles refer to, until dropped.
struct TreeScope(*const Tree);

impl TreeScope {
    fn enter(tree: &Tree) -> TreeScope {
        TreeScope(CURRENT_TREE.with(|c| c.replace(tree)))
    }
}

impl Drop for TreeScope {
    fn drop(&mut self) {
        CURRENT_TREE.with(|c| c.set(self.0));
    }
}

/// A node of a `Tree`, as Stylo sees it: just the id, 8 bytes with padding.
///
/// Stylo's style-sharing cache requires an element type the size of a pointer, so the handle
/// cannot carry a reference to the tree next to the id. It finds the tree through
/// `CURRENT_TREE` instead, which `StyleDom::resolve` sets for the duration of the traversal.
#[derive(Clone, Copy)]
#[repr(C, align(8))]
pub struct H<'a> {
    pub(crate) id: NodeId,
    _tree: PhantomData<&'a Tree>,
}

const _: () = assert!(size_of::<H>() == 8);

impl<'a> H<'a> {
    pub(crate) fn new(id: NodeId) -> H<'a> {
        H { id, _tree: PhantomData }
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    pub(crate) fn t(&self) -> &'a Tree {
        let p = CURRENT_TREE.with(|c| c.get());
        assert!(!p.is_null(), "node handle used outside StyleDom::resolve");
        unsafe { &*p }
    }
}

impl std::fmt::Debug for H<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "H({})", self.id)
    }
}
impl PartialEq for H<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for H<'_> {}
impl std::hash::Hash for H<'_> {
    fn hash<S: std::hash::Hasher>(&self, state: &mut S) {
        state.write_u32(self.id)
    }
}

/// The parsed sheets of a page, kept from one style build to the next (Gecko keeps a sheet parsed for
/// as long as its owner has it). Sheets are guarded by the cache's lock, so every build that uses
/// them shares it (`Tree::with_guard`).
pub struct SheetCache {
    guard: SharedRwLock,
    sheets: HashMap<SheetKey, (DocumentStyleSheet, usize)>,
    used: std::collections::HashSet<SheetKey>,
}

impl Default for SheetCache {
    fn default() -> SheetCache {
        SheetCache { guard: SharedRwLock::new(), sheets: HashMap::new(), used: Default::default() }
    }
}

impl SheetCache {
    /// The number of sheets kept, and the bytes of their text.
    pub fn stats(&self) -> (usize, usize) {
        (self.sheets.len(), self.sheets.keys().map(|k| k.text.len()).sum())
    }
}

/// The cache key's text of a sheet whose rules a script changed: its text and the ops, so that a
/// sheet is parsed again only when either changes.
fn sheet_key_text(css: &str, ops: &[mrt_dom::RuleOp]) -> std::sync::Arc<str> {
    let mut key = String::with_capacity(css.len() + ops.len() * 16);
    key.push_str(css);
    for op in ops {
        match op {
            mrt_dom::RuleOp::Insert(i, rule) => key.push_str(&format!("\u{0}I{i}:{rule}")),
            mrt_dom::RuleOp::Delete(i) => key.push_str(&format!("\u{0}D{i}")),
        }
    }
    key.into()
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct SheetKey {
    /// (a link's text is the runtime's cache entry, shared)
    text: std::sync::Arc<str>,
    origin: u8,
    media: Option<String>,
    quirks: i32,
}

/// The lock of the user-agent sheets, shared by every page of the process (Gecko: the global
/// shared lock, `GLOBAL_STYLE_DATA.shared_lock`, which `StylesheetGuards::ua_or_user` reads).
static UA_LOCK: std::sync::LazyLock<SharedRwLock> = std::sync::LazyLock::new(SharedRwLock::new);

/// Firefox's user-agent sheets, parsed once per process for each quirks mode a page has (Gecko:
/// `GlobalStyleSheetCache`, shared by the documents of a process), with their parse error counts.
static UA_SHEETS: std::sync::LazyLock<Mutex<HashMap<i32, Vec<(&'static str, DocumentStyleSheet, usize)>>>> = std::sync::LazyLock::new(Default::default);

/// The user-agent sheets for `tree`'s quirks mode, parsed on the first call for that mode.
fn ua_sheets_for<'a>(tree: &Tree, sheets: impl Iterator<Item = &'a (&'static str, &'static str)>) -> Vec<(&'static str, DocumentStyleSheet, usize)> {
    UA_SHEETS
        .lock()
        .unwrap()
        .entry(tree.mode())
        .or_insert_with(|| sheets.map(|&(name, css)| {
            let (sheet, errors) = tree.make_sheet_with_lock(&patch_ua_sheet(css), Origin::UserAgent, None, &UA_LOCK);
            (name, sheet, errors)
        }).collect())
        .clone()
}

/// The device for a style build (Gecko's nsPresContext's): a screen of `viewport` CSS px at a device
/// pixel ratio of 1, for a desktop.
fn make_device(quirks: QuirksMode, viewport: (f32, f32)) -> Device {
    Device::new(
        MediaType::screen(),
        quirks,
        euclid::Size2D::new(viewport.0, viewport.1),
        // device pixels: the same, at a ratio of 1
        euclid::Size2D::new(viewport.0, viewport.1),
        euclid::Scale::new(1.0),
        Box::new(NoFontMetrics),
        ComputedValues::initial_values_with_font_override(Font::initial_values()),
        PrefersColorScheme::Light,
        // a desktop: a mouse
        Default::default(),
        Default::default(),
    )
}

/// The children of `node` in the flattened tree (Gecko: StyleChildrenIterator): a host's shadow
/// tree, a slot's assigned nodes (or its own children when none), anything else's children.
fn flattened_children(dom: &Dom, node: NodeId) -> Vec<NodeId> {
    let root = dom.shadow_root(node);
    if root != NONE {
        return dom.children(root).collect();
    }
    if dom.is_slot(node) && dom.containing_shadow(node) != NONE {
        let assigned = dom.assigned_nodes(node);
        if !assigned.is_empty() {
            return assigned;
        }
    }
    dom.children(node).collect()
}
