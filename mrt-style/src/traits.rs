//! `TDocument`, `TNode`, `TElement` and the selector matching trait for `H`.
//! The shape of this follows Blitz's `stylo.rs` (blitz-dom 0.2.4), which was read as a reference.

use std::ptr::NonNull;
use std::sync::atomic::Ordering::Relaxed;

use web_atoms::{local_name, LocalName, LocalNameStaticSet, Namespace, NamespaceStaticSet};
use mrt_dom::{Kind, NodeId, NONE};
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::matching::{ElementSelectorFlags, MatchingContext, QuirksMode, VisitedHandlingMode};
use selectors::sink::Push;
use selectors::{Element, OpaqueElement};
use style::applicable_declarations::ApplicableDeclarationBlock;
use style::context::SharedStyleContext;
use style::dom::{LayoutIterator, NodeInfo, OpaqueNode, TDocument, TElement, TNode, TShadowRoot};
use style::properties::{ComputedValues, Importance, PropertyDeclaration, PropertyDeclarationBlock};
use style::rule_tree::CascadeLevel;
use style::selector_parser::{NonTSPseudoClass, PseudoElement, SelectorImpl};
use style::servo_arc::{Arc, ArcBorrow};
use style::shared_lock::{Locked, SharedRwLock};
use style::stylesheets::layer_rule::LayerOrder;
use style::stylesheets::scope_rule::ImplicitScopeRoot;
use style::values::{AtomIdent, AtomString, GenericAtomIdent};
use style::{Atom, CaseSensitivityExt};
use style_dom::ElementState;

use crate::{H, HAS_DATA};

impl<'a> H<'a> {
    fn with(&self, id: NodeId) -> Option<H<'a>> {
        (id != NONE).then_some(H::new(id))
    }

    fn node(&self) -> &'a mrt_dom::Node {
        self.t().dom.node(self.id)
    }

    fn selector_flags_now(&self) -> ElementSelectorFlags {
        ElementSelectorFlags::from_bits_truncate((self.slot_flags() & !crate::NOT_SELECTOR_FLAGS) as usize)
    }

    fn slot_flags(&self) -> u32 {
        self.t().slot(self.id).map_or(0, |s| s.flags.load(Relaxed))
    }

    fn is_el(&self) -> bool {
        self.node().kind == Kind::Element
    }

    fn name_atom(&self, name: u32) -> &'a LocalName {
        &self.t().name_atoms[name as usize]
    }

    /// The value of the attribute with this local name.
    fn attr(&self, name: &LocalName) -> Option<&'a str> {
        let dom = &self.t().dom;
        dom.attrs_of(self.id).iter().find(|a| self.name_atom(a.name) == name).map(|a| dom.attr_value(a))
    }

    fn has_tag(&self, name: &LocalName) -> bool {
        self.is_el() && self.name_atom(self.node().name) == name
    }

    fn classes(&self) -> impl Iterator<Item = &'a str> {
        self.attr(&local_name!("class")).unwrap_or("").split_ascii_whitespace()
    }

    fn is_link_element(&self) -> bool {
        (self.has_tag(&local_name!("a")) || self.has_tag(&local_name!("area"))) && self.attr(&local_name!("href")).is_some()
    }

    /// The `<html>` element: the document's first element child.
    fn root_element(&self) -> Option<H<'a>> {
        H::new(self.t().doc).first_element_child()
    }
}

impl<'a> TDocument for H<'a> {
    type ConcreteNode = H<'a>;
    fn as_node(&self) -> H<'a> {
        *self
    }
    fn is_html_document(&self) -> bool {
        true
    }
    fn quirks_mode(&self) -> QuirksMode {
        crate::quirks_of(self.t().mode())
    }
    fn shared_lock(&self) -> &SharedRwLock {
        &self.t().guard
    }
}

impl NodeInfo for H<'_> {
    fn is_element(&self) -> bool {
        self.is_el()
    }
    fn is_text_node(&self) -> bool {
        self.node().kind == Kind::Text
    }
}

// A shadow root: its host, and the cascade data of the `<style>` elements in its tree
// (`StyleDom::add_shadow_stylesheets`; Gecko's ShadowRoot::ServoStyleRuleMap and its AuthorStyles).
impl<'a> TShadowRoot for H<'a> {
    type ConcreteNode = H<'a>;
    fn as_node(&self) -> H<'a> {
        *self
    }
    fn host(&self) -> H<'a> {
        H::new(self.t().dom.host(self.id))
    }
    fn style_data<'b>(&self) -> Option<&'b style::stylist::CascadeData>
    where
        Self: 'b,
    {
        self.t().shadow_styles.get(&self.id).map(|styles| &*styles.data)
    }
}

impl<'a> TNode for H<'a> {
    type ConcreteElement = H<'a>;
    type ConcreteDocument = H<'a>;
    type ConcreteShadowRoot = H<'a>;

    fn parent_node(&self) -> Option<Self> {
        self.with(self.node().parent)
    }
    fn first_child(&self) -> Option<Self> {
        self.with(self.node().first_child)
    }
    fn last_child(&self) -> Option<Self> {
        self.with(self.node().last_child)
    }
    fn prev_sibling(&self) -> Option<Self> {
        self.with(self.node().prev_sibling)
    }
    fn next_sibling(&self) -> Option<Self> {
        self.with(self.node().next_sibling)
    }
    fn owner_doc(&self) -> Self::ConcreteDocument {
        H::new(self.t().doc)
    }
    fn is_in_document(&self) -> bool {
        true
    }
    // The parent in the flattened tree (Gecko: nsINode::GetFlattenedTreeParentElementForStyle): a
    // slotted node's slot, a shadow tree's top-level node's host; none for a host's child that no slot
    // takes, which is not styled.
    fn traversal_parent(&self) -> Option<Self::ConcreteElement> {
        self.with(self.t().dom.flattened_parent(self.id)).and_then(|p| p.as_element())
    }
    fn opaque(&self) -> OpaqueNode {
        OpaqueNode(self.id as usize)
    }
    fn debug_id(self) -> usize {
        self.id as usize
    }
    fn as_element(&self) -> Option<Self::ConcreteElement> {
        self.is_el().then_some(*self)
    }
    fn as_document(&self) -> Option<Self::ConcreteDocument> {
        (self.id == self.t().doc).then_some(*self)
    }
    fn as_shadow_root(&self) -> Option<Self::ConcreteShadowRoot> {
        (self.node().kind == Kind::ShadowRoot).then_some(*self)
    }
}

impl Element for H<'_> {
    type Impl = SelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        // The id plus one, as a fake pointer: ids are stable and never 0.
        OpaqueElement::from_non_null_ptr(NonNull::new((self.id + 1) as usize as *mut ()).unwrap())
    }
    // Selectors match in the DOM tree (not the flattened one): a shadow tree's top-level element has no
    // parent element.
    fn parent_element(&self) -> Option<Self> {
        self.parent_node().and_then(|p| p.as_element())
    }
    fn parent_node_is_shadow_root(&self) -> bool {
        self.parent_node().is_some_and(|p| p.node().kind == Kind::ShadowRoot)
    }
    fn containing_shadow_host(&self) -> Option<Self> {
        self.with(self.t().dom.containing_shadow(self.id)).map(|root| TShadowRoot::host(&root))
    }
    fn is_pseudo_element(&self) -> bool {
        false
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        let mut cur = self.prev_sibling();
        while let Some(n) = cur {
            if n.is_el() {
                return Some(n);
            }
            cur = n.prev_sibling();
        }
        None
    }
    fn next_sibling_element(&self) -> Option<Self> {
        let mut cur = self.next_sibling();
        while let Some(n) = cur {
            if n.is_el() {
                return Some(n);
            }
            cur = n.next_sibling();
        }
        None
    }
    fn first_element_child(&self) -> Option<Self> {
        let mut cur = self.first_child();
        while let Some(n) = cur {
            if n.is_el() {
                return Some(n);
            }
            cur = n.next_sibling();
        }
        None
    }
    fn is_html_element_in_html_document(&self) -> bool {
        self.node().ns == mrt_dom::NS_HTML
    }
    fn has_local_name(&self, local_name: &LocalName) -> bool {
        self.has_tag(local_name)
    }
    fn has_namespace(&self, ns: &Namespace) -> bool {
        self.t().ns_atoms[self.node().ns as usize] == *ns
    }
    fn is_same_type(&self, other: &Self) -> bool {
        self.node().name == other.node().name && self.node().ns == other.node().ns
    }

    fn attr_matches(
        &self,
        _ns: &NamespaceConstraint<&GenericAtomIdent<NamespaceStaticSet>>,
        local_name: &GenericAtomIdent<LocalNameStaticSet>,
        operation: &AttrSelectorOperation<&AtomString>,
    ) -> bool {
        let Some(value) = self.attr(&local_name.0) else { return false };
        match operation {
            AttrSelectorOperation::Exists => true,
            AttrSelectorOperation::WithValue { operator, case_sensitivity, value: wanted } => {
                operator.eval_str(value, wanted.as_ref(), *case_sensitivity)
            }
        }
    }

    fn match_non_ts_pseudo_class(&self, pseudo_class: &NonTSPseudoClass, _context: &mut MatchingContext<Self::Impl>) -> bool {
        match *pseudo_class {
            NonTSPseudoClass::AnyLink | NonTSPseudoClass::Link => self.is_link_element(),
            NonTSPseudoClass::Checked => self.has_tag(&local_name!("input")) && self.attr(&local_name!("checked")).is_some(),
            // the DEFINED state bit (see `state`)
            NonTSPseudoClass::Defined => self.t().dom.is_defined(self.id),
            // Gecko: Gecko_IsTableBorderNonzero (html.css writes `:-moz-table-border-nonzero`, see patch_ua_sheet)
            NonTSPseudoClass::ServoNonZeroBorder => {
                self.has_tag(&local_name!("table"))
                    && self.attr(&local_name!("border")).is_some_and(|v| {
                        let t = v.trim_start_matches([' ', '\t', '\n', '\x0c', '\r']);
                        let t = t.strip_prefix('+').unwrap_or(t);
                        let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
                        // not an integer, or an integer other than zero
                        digits.is_empty() || digits.parse::<u64>().map_or(true, |n| n != 0)
                    })
            }
            // Nothing is hovered, active or focused in a page nobody interacts with.
            _ => false,
        }
    }

    fn match_pseudo_element(&self, _pe: &PseudoElement, _context: &mut MatchingContext<Self::Impl>) -> bool {
        false
    }

    fn apply_selector_flags(&self, flags: ElementSelectorFlags) {
        let own = flags.for_self();
        if !own.is_empty() {
            if let Some(slot) = self.t().slot(self.id) {
                slot.flags.fetch_or(own.bits() as u32, Relaxed);
            }
        }
        let parent = flags.for_parent();
        if !parent.is_empty() {
            if let Some(slot) = self.parent_node().and_then(|p| self.t().slot(p.id)) {
                slot.flags.fetch_or(parent.bits() as u32, Relaxed);
            }
        }
    }

    fn is_link(&self) -> bool {
        self.has_tag(&local_name!("a"))
    }
    fn is_html_slot_element(&self) -> bool {
        self.t().dom.is_slot(self.id)
    }
    // The slot whatever the shadow root's mode (::slotted, Gecko's GetAssignedSlot).
    fn assigned_slot(&self) -> Option<Self> {
        self.with(self.t().dom.assigned_slot(self.id))
    }
    fn has_id(&self, id: &AtomIdent, case_sensitivity: CaseSensitivity) -> bool {
        if self.t().selector_only {
            return self.attr(&local_name!("id")).is_some_and(|mine| eq_text(case_sensitivity, mine, id));
        }
        self.t().id_atom(self.id).is_some_and(|mine| case_sensitivity.eq_atom(mine, id))
    }
    fn has_class(&self, name: &AtomIdent, case_sensitivity: CaseSensitivity) -> bool {
        self.classes().any(|c| eq_text(case_sensitivity, c, name))
    }
    // `exportparts="inner: outer, name"` (Gecko: Element::GetExportedParts): the inner name for an
    // outer one.
    fn imported_part(&self, name: &AtomIdent) -> Option<AtomIdent> {
        let list = self.attr(&local_name!("exportparts"))?;
        list.split(',').find_map(|entry| {
            let mut it = entry.splitn(2, ':').map(str::trim);
            let inner = it.next().filter(|s| !s.is_empty())?;
            let outer = it.next().unwrap_or(inner);
            (outer == &**name).then(|| AtomIdent::from(inner))
        })
    }
    fn is_part(&self, name: &AtomIdent) -> bool {
        self.attr(&local_name!("part")).is_some_and(|p| p.split_ascii_whitespace().any(|n| n == &**name))
    }
    fn is_empty(&self) -> bool {
        self.first_child().is_none()
    }
    fn is_root(&self) -> bool {
        self.node().parent == self.t().doc
    }
    fn has_custom_state(&self, _name: &AtomIdent) -> bool {
        false
    }
    fn add_element_unique_hashes(&self, _filter: &mut selectors::bloom::BloomFilter) -> bool {
        false
    }
}

/// The children of an element in the flattened tree: its own children by their sibling links, or,
/// for a shadow host or a slot with assigned nodes, the list `Dom::flattened_children` gives.
pub enum Children<'a> {
    Siblings(Option<H<'a>>),
    List(std::vec::IntoIter<NodeId>),
}

impl<'a> Iterator for Children<'a> {
    type Item = H<'a>;
    fn next(&mut self) -> Option<H<'a>> {
        match self {
            Children::Siblings(next) => {
                let cur = (*next)?;
                *next = cur.next_sibling();
                Some(cur)
            }
            Children::List(ids) => ids.next().map(H::new),
        }
    }
}

impl<'a> TElement for H<'a> {
    type ConcreteNode = H<'a>;
    type TraversalChildrenIterator = Children<'a>;

    fn as_node(&self) -> H<'a> {
        *self
    }
    // `@scope` without a prelude in a shadow tree's sheet: NOT PORTED (no scope root).
    fn implicit_scope_for_sheet_in_shadow_root(_host: OpaqueElement, _sheet_index: usize) -> Option<ImplicitScopeRoot> {
        None
    }
    // Inherit from the parent in the flattened tree: the host for a shadow tree's top-level element,
    // the slot for a slotted one (Gecko's wrapper: `flattened_tree_parent`; Stylo's default is the DOM
    // parent).
    fn inheritance_parent(&self) -> Option<Self> {
        TElement::traversal_parent(self)
    }
    // Gecko: StyleChildrenIterator, the flattened tree's children.
    fn traversal_children(&self) -> LayoutIterator<Children<'a>> {
        let dom = &self.t().dom;
        if dom.shadow_root(self.id) != NONE || dom.is_slot(self.id) {
            return LayoutIterator(Children::List(dom.flattened_children(self.id).into_iter()));
        }
        LayoutIterator(Children::Siblings(self.first_child()))
    }
    fn is_html_element(&self) -> bool {
        self.node().ns == mrt_dom::NS_HTML
    }
    fn is_mathml_element(&self) -> bool {
        self.node().ns == 5
    }
    fn is_svg_element(&self) -> bool {
        self.node().ns == 6
    }
    fn style_attribute(&self) -> Option<ArcBorrow<'_, Locked<PropertyDeclarationBlock>>> {
        self.t().style_attr(self.id).map(|b| b.borrow_arc())
    }
    // Gecko: Element::State (nsIContent state flags). A link is unvisited here (there is no history);
    // Stylo puts the rules with :link and :any-link in a bucket that it reads only for an element
    // whose state has VISITED_OR_UNVISITED, so a rule like `:any-link { ... }` (html.css, ua.css) needs
    // this to match anything.
    fn state(&self) -> ElementState {
        // (`crate::element_state_bits`, which the restyle recorder snapshots too)
        ElementState::from_bits_retain(crate::element_state_bits(&self.t().dom, self.id))
    }
    fn has_part_attr(&self) -> bool {
        self.attr(&local_name!("part")).is_some()
    }
    fn exports_any_part(&self) -> bool {
        self.attr(&local_name!("exportparts")).is_some()
    }
    fn id(&self) -> Option<&Atom> {
        self.t().id_atom(self.id)
    }
    fn each_class<F: FnMut(&AtomIdent)>(&self, mut callback: F) {
        for c in self.classes() {
            let atom = Atom::from(c);
            callback(AtomIdent::cast(&atom));
        }
    }
    fn each_attr_name<F: FnMut(&style::LocalName)>(&self, mut callback: F) {
        for a in self.t().dom.attrs_of(self.id) {
            callback(&GenericAtomIdent(self.name_atom(a.name).clone()));
        }
    }
    // Gecko: Element::HasDirtyDescendantsForServo and the snapshot flags (ELEMENT_HAS_SNAPSHOT,
    // ELEMENT_HANDLED_SNAPSHOT), in the slot's flags
    fn has_dirty_descendants(&self) -> bool {
        self.slot_flags() & crate::HAS_DIRTY_DESCENDANTS != 0
    }
    fn has_snapshot(&self) -> bool {
        self.slot_flags() & crate::HAS_SNAPSHOT != 0
    }
    fn handled_snapshot(&self) -> bool {
        self.slot_flags() & crate::HANDLED_SNAPSHOT != 0
    }
    unsafe fn set_handled_snapshot(&self) {
        if let Some(slot) = self.t().slot(self.id) {
            slot.flags.fetch_or(crate::HANDLED_SNAPSHOT, Relaxed);
        }
    }
    unsafe fn set_dirty_descendants(&self) {
        if let Some(slot) = self.t().slot(self.id) {
            slot.flags.fetch_or(crate::HAS_DIRTY_DESCENDANTS, Relaxed);
        }
    }
    unsafe fn unset_dirty_descendants(&self) {
        if let Some(slot) = self.t().slot(self.id) {
            slot.flags.fetch_and(!crate::HAS_DIRTY_DESCENDANTS, Relaxed);
        }
    }
    fn store_children_to_process(&self, _n: isize) {
        unimplemented!()
    }
    fn did_process_child(&self) -> isize {
        unimplemented!()
    }

    unsafe fn ensure_data(&self) -> style::data::ElementDataMut<'_> {
        let slot = self.t().slot(self.id).expect("not an element");
        let mut data = slot.data.borrow_mut();
        if !slot.has_data() {
            *data = style::data::ElementData { damage: style::selector_parser::RestyleDamage::all(), ..Default::default() };
            slot.flags.fetch_or(HAS_DATA, Relaxed);
        }
        data
    }
    unsafe fn clear_data(&self) {
        let slot = self.t().slot(self.id).expect("not an element");
        *slot.data.borrow_mut() = Default::default();
        slot.flags.fetch_and(!HAS_DATA, Relaxed);
    }
    fn has_data(&self) -> bool {
        self.t().slot(self.id).is_some_and(|s| s.has_data())
    }
    fn borrow_data(&self) -> Option<style::data::ElementDataRef<'_>> {
        let slot = self.t().slot(self.id).filter(|s| s.has_data())?;
        Some(slot.data.borrow())
    }
    fn mutate_data(&self) -> Option<style::data::ElementDataMut<'_>> {
        let slot = self.t().slot(self.id).filter(|s| s.has_data())?;
        Some(slot.data.borrow_mut())
    }
    fn skip_item_display_fixup(&self) -> bool {
        false
    }
    fn may_have_animations(&self) -> bool {
        false
    }
    fn has_animations(&self, _context: &SharedStyleContext) -> bool {
        false
    }
    fn has_css_animations(&self, _context: &SharedStyleContext, _pe: Option<PseudoElement>) -> bool {
        false
    }
    fn has_css_transitions(&self, _context: &SharedStyleContext, _pe: Option<PseudoElement>) -> bool {
        false
    }
    fn animation_rule(&self, _context: &SharedStyleContext) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }
    fn transition_rule(&self, _context: &SharedStyleContext) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }
    fn shadow_root(&self) -> Option<H<'a>> {
        self.with(self.t().dom.shadow_root(self.id))
    }
    fn containing_shadow(&self) -> Option<H<'a>> {
        self.with(self.t().dom.containing_shadow(self.id))
    }
    fn lang_attr(&self) -> Option<style::selector_parser::AttrValue> {
        None
    }
    fn match_element_lang(&self, _override_lang: Option<Option<style::selector_parser::AttrValue>>, _value: &style::selector_parser::Lang) -> bool {
        false
    }
    fn is_html_document_body_element(&self) -> bool {
        self.has_tag(&local_name!("body")) && self.root_element().is_some_and(|r| r.id == self.node().parent)
    }

    fn synthesize_presentational_hints_for_legacy_attributes<V>(&self, _visited_handling: VisitedHandlingMode, hints: &mut V)
    where
        V: Push<ApplicableDeclarationBlock>,
    {
        use style::values::generics::{length::Size, NonNegative};
        use style::values::specified::{LengthPercentage, NoCalcLength};

        // All of the element's hints go into one block (Gecko: one mapped-attribute declaration block per
        // element), interned below so that elements with the same hints share it.
        let combined = std::cell::RefCell::new(PropertyDeclarationBlock::new());
        let push_block = |block: PropertyDeclarationBlock| {
            let mut combined = combined.borrow_mut();
            for (decl, importance) in block.declaration_importance_iter() {
                combined.push(decl.clone(), importance);
            }
        };
        let push = |decl: PropertyDeclaration| {
            combined.borrow_mut().push(decl, Importance::Normal);
        };
        // A presentational hint written as CSS text (Gecko builds these with MappedDeclarationsBuilder).
        let quirks = crate::quirks_of(self.t().mode());
        let push_css = |css: &str| {
            push_block(style::properties::parse_style_attribute(css, &self.t().url_data, None, quirks, style::stylesheets::CssRuleType::Style))
        };
        // HTML's rules for parsing a non-negative integer: leading digits (after white space and an optional
        // plus sign), whatever follows is ignored.
        fn html_uint(value: &str) -> Option<u32> {
            let v = value.trim_start_matches([' ', '\t', '\n', '\x0c', '\r']);
            let v = v.strip_prefix('+').unwrap_or(v);
            let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                return None;
            }
            Some(digits.parse::<u32>().unwrap_or(u32::MAX).min(1_000_000))
        }
        let size = |value: &str, keep: &dyn Fn(f32) -> bool| -> Option<LengthPercentage> {
            if let Some(v) = value.strip_suffix("px") {
                return Some(LengthPercentage::Length(NoCalcLength::from_px(v.parse().ok()?)));
            }
            if let Some(v) = value.strip_suffix('%') {
                let v: f32 = v.parse().ok()?;
                return Some(LengthPercentage::Percentage(style::values::specified::NoCalcPercentage::new(v / 100.0)));
            }
            let v: f32 = value.parse().ok().filter(|v| keep(*v))?;
            Some(LengthPercentage::Length(NoCalcLength::from_px(v)))
        };
        fn color(value: &str) -> Option<(u8, u8, u8)> {
            let v = value.strip_prefix('#')?;
            let h = |s: &str| u8::from_str_radix(s, 16).ok();
            match v.len() {
                3 => Some((h(&v[0..1])?, h(&v[1..2])?, h(&v[2..3])?)),
                6 => Some((h(&v[0..2])?, h(&v[2..4])?, h(&v[4..6])?)),
                _ => None,
            }
        }

        let tag = self.node().name;
        let tag = self.name_atom(tag);
        for a in self.t().dom.attrs_of(self.id) {
            let name = self.name_atom(a.name);
            let value = self.t().dom.attr_value(a);
            if *name == local_name!("align") {
                let lower = value.to_ascii_lowercase();
                if *tag == local_name!("table") {
                    // Gecko: nsGenericHTMLElement::MapTableHAlignAttributeInto
                    match lower.as_str() {
                        "center" | "middle" => push_css("margin-left:auto;margin-right:auto"),
                        "left" => push_css("float:left"),
                        "right" => push_css("float:right"),
                        _ => {}
                    }
                } else {
                    // Gecko: nsGenericHTMLElement::MapDivAlignAttributeInto and
                    // MapTableCellHAlignAttributeInto
                    use style::values::computed::text::TextAlign as Keyword;
                    use style::values::specified::TextAlign;
                    let k = match lower.as_str() {
                        "left" => Some(Keyword::MozLeft),
                        "right" => Some(Keyword::MozRight),
                        "center" | "middle" => Some(Keyword::MozCenter),
                        "justify" => Some(Keyword::Justify),
                        _ => None,
                    };
                    if let Some(k) = k {
                        push(PropertyDeclaration::TextAlign(TextAlign::Keyword(k)));
                    }
                }
            }
            // Gecko: nsGenericHTMLElement::MapTableVAlignAttributeInto (here as `vertical-align`, which
            // is what Gecko's baseline-shift and alignment-baseline stand for in a table cell)
            if *name == local_name!("valign") && [local_name!("td"), local_name!("th"), local_name!("tr"), local_name!("thead"), local_name!("tbody"), local_name!("tfoot")].contains(tag) {
                match value.to_ascii_lowercase().as_str() {
                    v @ ("top" | "middle" | "bottom" | "baseline") => push_css(&format!("vertical-align:{v}")),
                    _ => {}
                }
            }
            // Gecko: HTMLTableElement::MapAttributesIntoRule (cellspacing, border)
            if *tag == local_name!("table") {
                if *name == local_name!("cellspacing") {
                    if let Some(n) = html_uint(value) {
                        push_css(&format!("border-spacing:{n}px"));
                    }
                }
                if *name == local_name!("border") {
                    // border = 1 pixel default
                    let n = html_uint(value).unwrap_or(1);
                    push_css(&format!("border-top-width:{n}px;border-right-width:{n}px;border-bottom-width:{n}px;border-left-width:{n}px"));
                }
            }
            // Gecko: HTMLTableCellElement::MapAttributesIntoRule (nowrap)
            if *name == local_name!("nowrap") && [local_name!("td"), local_name!("th")].contains(tag) {
                push_css("text-wrap-mode:nowrap");
            }
            // https://html.spec.whatwg.org/multipage/rendering.html#dimRendering
            if *name == local_name!("width") && [local_name!("table"), local_name!("col"), local_name!("tr"), local_name!("td"), local_name!("th"), local_name!("hr")].contains(tag) {
                let is_table = *tag == local_name!("table");
                if let Some(w) = size(value, &|v| !is_table || v != 0.0) {
                    push(PropertyDeclaration::Width(Size::LengthPercentage(NonNegative(w))));
                }
            }
            if *name == local_name!("height") && [local_name!("table"), local_name!("thead"), local_name!("tbody"), local_name!("tfoot"), local_name!("tr"), local_name!("td"), local_name!("th")].contains(tag) {
                if let Some(h) = size(value, &|_| true) {
                    push(PropertyDeclaration::Height(Size::LengthPercentage(NonNegative(h))));
                }
            }
            // https://html.spec.whatwg.org/multipage/rendering.html#attributes-for-embedded-content-and-images
            // (Gecko: nsGenericHTMLElement::MapImageSizeAttributesInto): the width and height attributes of
            // embedded content are the CSS width and height. The aspect ratio the attributes give
            // (`aspect-ratio: auto w / h`) is not applied.
            if (*name == local_name!("width") || *name == local_name!("height"))
                && [local_name!("img"), local_name!("iframe"), local_name!("embed"), local_name!("object"), local_name!("video"), local_name!("canvas")].contains(tag)
            {
                if let Some(d) = size(value, &|v| v >= 0.0) {
                    if *name == local_name!("width") {
                        push(PropertyDeclaration::Width(Size::LengthPercentage(NonNegative(d))));
                    } else {
                        push(PropertyDeclaration::Height(Size::LengthPercentage(NonNegative(d))));
                    }
                }
            }
            // Gecko: SVGSVGElement (mapped attributes `width` and `height` of the outer <svg>, SVG 2 section on
            // geometry properties): the attributes are the CSS width and height. A number without a unit is
            // a length in px; other units (em, pt, ...) are CSS lengths as written. An invalid value is ignored.
            if (*name == local_name!("width") || *name == local_name!("height")) && *tag == local_name!("svg") {
                let value = value.trim();
                let is_number = value.parse::<f32>().is_ok();
                let css_name = if *name == local_name!("width") { "width" } else { "height" };
                if is_number && value.parse::<f32>().map_or(false, |v| v >= 0.0) {
                    push_css(&format!("{css_name}:{value}px"));
                } else if !is_number && value.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
                    push_css(&format!("{css_name}:{value}"));
                }
            }
            if *name == local_name!("bgcolor") {
                if let Some((r, g, b)) = color(value) {
                    use style::color::AbsoluteColor;
                    use style::values::specified::Color;
                    push(PropertyDeclaration::BackgroundColor(Color::from_absolute_color(AbsoluteColor::srgb_legacy(r, g, b, 1.0))));
                }
            }
            if *name == local_name!("hidden") {
                push(PropertyDeclaration::Display(style::values::specified::Display::None));
            }
        }

        // Gecko: HTMLTableCellElement::WalkContentStyleRules and HTMLTableElement::BuildInheritedAttributes: a
        // cell takes its padding from the `cellpadding` of its table (the cell's parent is a row, whose
        // parent is the table or a row group).
        if *tag == local_name!("td") || *tag == local_name!("th") {
            let is = |h: &H<'_>, n: LocalName| h.has_tag(&n);
            let row = self.with(self.node().parent).filter(|r| is(r, local_name!("tr")));
            let table = row.and_then(|r| self.with(r.node().parent)).and_then(|g| {
                if is(&g, local_name!("table")) {
                    Some(g)
                } else if is(&g, local_name!("thead")) || is(&g, local_name!("tbody")) || is(&g, local_name!("tfoot")) {
                    self.with(g.node().parent).filter(|t| is(t, local_name!("table")))
                } else {
                    None
                }
            });
            if let Some(n) = table.and_then(|t| t.attr(&local_name!("cellpadding"))).and_then(html_uint) {
                push_css(&format!("padding:{n}px"));
            }
        }

        // Intern the block by its CSS text (Gecko: AttributeStyles::UniqueMappedAttributes). Stylo
        // compares hint blocks by pointer, so a fresh block per element would give every such element
        // its own rule node and keep it out of the style sharing cache.
        let combined = combined.into_inner();
        if combined.is_empty() {
            return;
        }
        let mut key = String::new();
        let _ = combined.to_css(&mut key);
        let block = self
            .t()
            .pres_hint_blocks
            .lock()
            .unwrap()
            .entry(key)
            .or_insert_with(|| Arc::new(self.t().guard.wrap(combined)))
            .clone();
        hints.push(ApplicableDeclarationBlock::from_declarations(block, CascadeLevel::new(style::rule_tree::CascadeOrigin::PresHints), LayerOrder::root()));
    }

    /// The value of an attribute, for CSS `attr()`. Attributes in a namespace are not kept apart in the
    /// arena (the parser gives them prefixed names), so only the null namespace is found.
    fn get_attr(&self, attr: &style::LocalName, namespace: &style::Namespace) -> Option<String> {
        if !namespace.is_empty() {
            return None;
        }
        self.attr(&LocalName::from(&**attr)).map(str::to_owned)
    }

    fn local_name(&self) -> &LocalName {
        self.name_atom(self.node().name)
    }
    fn namespace(&self) -> &Namespace {
        &self.t().ns_atoms[self.node().ns as usize]
    }
    fn query_container_size(&self, _display: &style::values::specified::Display) -> euclid::default::Size2D<Option<app_units::Au>> {
        // Container queries are off for now: no sizes known.
        Default::default()
    }
    fn each_custom_state<F: FnMut(&AtomIdent)>(&self, _callback: F) {}
    fn has_selector_flags(&self, flags: ElementSelectorFlags) -> bool {
        self.selector_flags_now().contains(flags)
    }
    fn relative_selector_search_direction(&self) -> ElementSelectorFlags {
        let flags = self.selector_flags_now();
        for dir in [
            ElementSelectorFlags::RELATIVE_SELECTOR_SEARCH_DIRECTION_ANCESTOR_SIBLING,
            ElementSelectorFlags::RELATIVE_SELECTOR_SEARCH_DIRECTION_ANCESTOR,
            ElementSelectorFlags::RELATIVE_SELECTOR_SEARCH_DIRECTION_SIBLING,
        ] {
            if flags.contains(dir) {
                return dir;
            }
        }
        ElementSelectorFlags::empty()
    }
}

#[allow(dead_code)]
fn _unused(_: &ComputedValues, _: VisitedHandlingMode) {}

/// `case_sensitivity.eq_atom` of an attribute's text and an atom, without interning the text (an
/// atom of the servo build is its string): querySelectorAll tests every element's classes, and an
/// atom for each took a lock and a hash (task 9caa0).
fn eq_text(case_sensitivity: CaseSensitivity, text: &str, atom: &Atom) -> bool {
    match case_sensitivity {
        CaseSensitivity::CaseSensitive => text == &**atom,
        CaseSensitivity::AsciiCaseInsensitive => text.eq_ignore_ascii_case(atom),
    }
}
