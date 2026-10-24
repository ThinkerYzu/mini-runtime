//! What a change of the DOM invalidates in the styles of the last style build, decided when the change
//! is made, as Gecko's RestyleManager decides it from its DOM notifications (layout/style/
//! RestyleManager.cpp: ContentInserted, ContentAppended, ContentWillBeRemoved, CharacterDataChanged,
//! and the snapshots of ServoElementSnapshot taken in AttributeWillChange and ElementStateChanged).
//!
//! The runtime's DOM funnels call a `RestyleRecorder` as Gecko's PresShell gets its notifications,
//! with the DOM before or after the change as Gecko has it. The recorder writes `RestyleOp`s, which
//! the next style build applies in order (`StyleDom::attach_dom`). The selector flags it reads are
//! the ones the last style build left on the elements (`StyleDom::restyle_flags`): an element added
//! since has none, as in Gecko, where it has no style data yet.
//!
//! NOT PORTED: the invalidation for relative selectors (`:has()`: StyleSet()->MaybeInvalidateFor*),
//! tree-counting functions (`sibling-index()`), native anonymous content, the selector flags of a
//! shadow root (Stylo's flags for the parent of a shadow tree's top-level element are not kept: the
//! root has no slot), animations.
use mrt_dom::{Dom, Kind, NONE, NodeId};
use selectors::matching::ElementSelectorFlags;
use std::collections::{HashMap, HashSet};
use style::invalidation::element::restyle_hints::RestyleHint;

/// An element's attributes and state before the first change since the last style build
/// (ServoElementSnapshot, as Gecko takes it in Element::AttributeWillChange / ElementStateChanged).
#[derive(Clone, Debug, Default)]
pub struct ElementSnapshotData {
    /// (local name, value, namespace number) of every attribute, if an attribute changed.
    pub attrs: Option<Vec<(String, String, u8)>>,
    /// `ElementState` bits, if the state changed.
    pub state: Option<u64>,
    pub changed_attrs: Vec<String>,
    pub id_changed: bool,
    pub class_changed: bool,
    pub other_attributes_changed: bool,
}

/// One step for the next style build, in the order of the changes.
#[derive(Clone, Debug)]
pub enum RestyleOp {
    /// Drop the style data of the element and its flattened subtree (RestyleManager::
    /// ClearServoDataFromSubtree: the node left the tree).
    ClearSubtree(NodeId),
    /// RestyleManager::PostRestyleEvent: this hint on the element, if it has style data.
    Hint(NodeId, RestyleHint),
    /// The node (and its subtree) was inserted: new elements have no style data, so the traversal
    /// must reach them (Gecko styles them from the frame constructor, StyleNewChildRange).
    Inserted(NodeId),
    /// The element's snapshot, for Stylo's invalidation.
    Snapshot(NodeId, ElementSnapshotData),
    /// Everything is to be restyled: a sheet changed, or the log was too long.
    Everything,
}

/// Records what the DOM's changes invalidate (see the module's comment).
#[derive(Default)]
pub struct RestyleRecorder {
    /// The selector flags the last style build left, for the elements that have any of the ones the
    /// recorder reads (AllSimpleRestyleFlags), and whether an element had style data then.
    flags: HashMap<NodeId, u32>,
    styled: HashSet<NodeId>,
    ops: Vec<RestyleOp>,
    /// The elements snapshotted since the last style build, and where their snapshot is in `ops`.
    snapshots: HashMap<NodeId, usize>,
    /// RestyleManager::mRestyledAsWholeContainer.
    restyled_as_whole_container: HashSet<NodeId>,
}

/// The ops kept before the log becomes `Everything`.
const MAX_OPS: usize = 1 << 16;

impl RestyleRecorder {
    /// The state the last style build left: `flags` by element, and the elements with style data.
    pub fn reset(&mut self, flags: HashMap<NodeId, u32>, styled: HashSet<NodeId>) {
        self.flags = flags;
        self.styled = styled;
        self.ops.clear();
        self.snapshots.clear();
        self.restyled_as_whole_container.clear();
    }

    /// The ops since the last style build; the recorder starts a new log.
    pub fn take_ops(&mut self) -> Vec<RestyleOp> {
        self.snapshots.clear();
        self.restyled_as_whole_container.clear();
        std::mem::take(&mut self.ops)
    }

    fn push(&mut self, op: RestyleOp) {
        if matches!(self.ops.as_slice(), [RestyleOp::Everything]) {
            return;
        }
        if self.ops.len() >= MAX_OPS || matches!(op, RestyleOp::Everything) {
            self.ops.clear();
            self.snapshots.clear();
            self.ops.push(RestyleOp::Everything);
            return;
        }
        self.ops.push(op);
    }

    /// Everything is to be restyled (a sheet loaded or changed, or the log is not wanted).
    pub fn everything(&mut self) {
        self.push(RestyleOp::Everything);
    }

    fn selector_flags(&self, node: NodeId) -> ElementSelectorFlags {
        ElementSelectorFlags::from_bits_truncate(
            self.flags.get(&node).copied().unwrap_or(0) as usize
        )
    }

    // Gecko: layout/style/RestyleManager.cpp RestyleManager::PostRestyleEvent
    fn post_restyle_event(&mut self, element: NodeId, hint: RestyleHint) {
        if hint.is_empty() {
            return;
        }
        self.push(RestyleOp::Hint(element, hint));
    }

    // Gecko: layout/style/RestyleManager.cpp RestyleManager::RestyleSiblingsStartingWith
    fn restyle_siblings_starting_with(&mut self, dom: &Dom, starting_sibling: NodeId) {
        let mut sibling = starting_sibling;
        while sibling != NONE {
            if dom.node(sibling).kind == Kind::Element {
                self.post_restyle_event(sibling, RestyleHint::restyle_subtree());
            }
            sibling = dom.node(sibling).next_sibling;
        }
    }

    // Gecko: layout/style/RestyleManager.cpp RestyleManager::RestyleWholeContainer
    // (the relative-selector invalidation for :nth-child: NOT PORTED, see the module's comment)
    fn restyle_whole_container(&mut self, dom: &Dom, container: NodeId) {
        if !self.restyled_as_whole_container.insert(container) {
            return;
        }
        if dom.node(container).kind == Kind::Element {
            self.post_restyle_event(container, RestyleHint::restyle_subtree());
        } else {
            self.restyle_siblings_starting_with(dom, dom.node(container).first_child);
        }
    }

    // Gecko: layout/style/RestyleManager.cpp RestyleManager::RestyleForEmptyChange
    fn restyle_for_empty_change(&mut self, dom: &Dom, container: NodeId) {
        self.post_restyle_event(container, RestyleHint::restyle_subtree());

        // In some cases (:empty + E, :empty ~ E), a change in the content of
        // an element requires restyling its parent's siblings.
        let grandparent = dom.node(container).parent;
        if grandparent == NONE
            || !self
                .selector_flags(grandparent)
                .contains(ElementSelectorFlags::HAS_SLOW_SELECTOR_LATER_SIBLINGS)
        {
            return;
        }
        self.restyle_siblings_starting_with(dom, dom.node(container).next_sibling);
    }

    // Gecko: layout/style/RestyleManager.cpp RestyleManager::MaybeRestyleForEdgeChildChange
    fn maybe_restyle_for_edge_child_change(
        &mut self,
        dom: &Dom,
        container: NodeId,
        changed_child: NodeId,
    ) {
        // restyle the previously-first element child if it is after this node
        let mut passed_child = false;
        let mut content = dom.node(container).first_child;
        while content != NONE {
            if content == changed_child {
                passed_child = true;
            } else if dom.node(content).kind == Kind::Element {
                if passed_child {
                    self.post_restyle_event(content, RestyleHint::restyle_subtree());
                }
                break;
            }
            content = dom.node(content).next_sibling;
        }
        // restyle the previously-last element child if it is before this node
        passed_child = false;
        let mut content = dom.node(container).last_child;
        while content != NONE {
            if content == changed_child {
                passed_child = true;
            } else if dom.node(content).kind == Kind::Element {
                if passed_child {
                    self.post_restyle_event(content, RestyleHint::restyle_subtree());
                }
                break;
            }
            content = dom.node(content).prev_sibling;
        }
    }

    /// Call after `child` was inserted (Gecko: ContentInserted, and ContentAppended, which gives the
    /// same restyles for an append).
    // Gecko: layout/style/RestyleManager.cpp RestyleManager::ContentInserted
    pub fn content_inserted(&mut self, dom: &Dom, child: NodeId) {
        // (StyleSet()->MaybeInvalidateForElementInsertion: NOT PORTED, :has())
        self.push(RestyleOp::Inserted(child));
        self.restyle_for_insert_or_change(dom, child);
    }

    // Restyling for a ContentInserted or CharacterDataChanged notification.
    // Gecko: layout/style/RestyleManager.cpp RestyleManager::RestyleForInsertOrChange
    fn restyle_for_insert_or_change(&mut self, dom: &Dom, child: NodeId) {
        let container = dom.node(child).parent;
        if container == NONE {
            return;
        }
        let selector_flags = self.selector_flags(container) & all_simple_restyle_flags();
        if selector_flags.is_empty() {
            return;
        }

        if selector_flags.contains(ElementSelectorFlags::HAS_EMPTY_SELECTOR)
            && dom.node(container).kind == Kind::Element
        {
            // See whether we need to restyle the container due to :empty /
            // :-moz-only-whitespace.
            let was_empty = !has_any_significant_sibling(dom, container, child);
            if was_empty {
                self.restyle_for_empty_change(dom, container);
                return;
            }
        }

        if selector_flags.contains(ElementSelectorFlags::HAS_SLOW_SELECTOR) {
            self.restyle_whole_container(dom, container);
            // Restyling the container is the most we can do here, so we're done.
            return;
        }

        if selector_flags.contains(ElementSelectorFlags::HAS_SLOW_SELECTOR_LATER_SIBLINGS) {
            // Restyle all later siblings.
            self.restyle_siblings_starting_with(dom, dom.node(child).next_sibling);
        }

        if selector_flags.contains(ElementSelectorFlags::HAS_EDGE_CHILD_SELECTOR) {
            self.maybe_restyle_for_edge_child_change(dom, container, child);
        }
    }

    /// Call before `old_child` is removed from its parent.
    // Gecko: layout/style/RestyleManager.cpp RestyleManager::ContentWillBeRemoved
    pub fn content_will_be_removed(&mut self, dom: &Dom, old_child: NodeId) {
        let container = dom.node(old_child).parent;
        if container == NONE {
            return;
        }

        // Computed style data isn't useful for detached nodes, and we'll need to
        // recompute it anyway if we ever insert the nodes back into a document.
        if dom.node(old_child).kind == Kind::Element {
            self.push(RestyleOp::ClearSubtree(old_child));
        }

        let selector_flags = self.selector_flags(container) & all_simple_restyle_flags();
        if selector_flags.is_empty() {
            return;
        }

        let container_is_element = dom.node(container).kind == Kind::Element;

        if selector_flags.contains(ElementSelectorFlags::HAS_EMPTY_SELECTOR) && container_is_element
        {
            // see whether we need to restyle the container
            let mut is_empty = true; // :empty or :-moz-only-whitespace
            let mut child = dom.node(container).first_child;
            while child != NONE {
                // We don't know whether we're testing :empty or :-moz-only-whitespace,
                // so be conservative and assume :-moz-only-whitespace (i.e., make
                // IsSignificantChild less likely to be true, and thus make us more
                // likely to restyle).
                if child != old_child && is_significant_child(dom, child) {
                    is_empty = false;
                    break;
                }
                child = dom.node(child).next_sibling;
            }
            if is_empty {
                self.restyle_for_empty_change(dom, container);
                return;
            }
        }

        // It is somewhat common to remove all nodes in a container from the
        // beginning. If we're doing that, going through the
        // HasSlowSelectorLaterSiblings code-path would be quadratic, so that's not
        // amazing. Instead, we take the slower path (which also restyles the
        // container) in that case. It restyles one more element, but it avoids the
        // quadratic behavior.
        let restyle_whole_container = selector_flags
            .contains(ElementSelectorFlags::HAS_SLOW_SELECTOR)
            || (selector_flags.contains(ElementSelectorFlags::HAS_SLOW_SELECTOR_LATER_SIBLINGS)
                && dom.node(old_child).prev_sibling == NONE);

        if restyle_whole_container {
            self.restyle_whole_container(dom, container);
            // Restyling the container is the most we can do here, so we're done.
            return;
        }

        if selector_flags.contains(ElementSelectorFlags::HAS_SLOW_SELECTOR_LATER_SIBLINGS) {
            // Restyle all later siblings.
            self.restyle_siblings_starting_with(dom, dom.node(old_child).next_sibling);
        }

        if selector_flags.contains(ElementSelectorFlags::HAS_EDGE_CHILD_SELECTOR) {
            let next_sibling = dom.node(old_child).next_sibling;
            // restyle the now-first element child if it was after aOldChild
            let mut reached_following_sibling = false;
            let mut content = dom.node(container).first_child;
            while content != NONE {
                if content != old_child {
                    if content == next_sibling {
                        reached_following_sibling = true;
                        // do NOT continue here; we might want to restyle this node
                    }
                    if dom.node(content).kind == Kind::Element {
                        if reached_following_sibling {
                            self.post_restyle_event(content, RestyleHint::restyle_subtree());
                        }
                        break;
                    }
                }
                content = dom.node(content).next_sibling;
            }
            // restyle the now-last element child if it was before aOldChild
            reached_following_sibling = next_sibling == NONE;
            let mut content = dom.node(container).last_child;
            while content != NONE {
                if content != old_child {
                    if dom.node(content).kind == Kind::Element {
                        if reached_following_sibling {
                            self.post_restyle_event(content, RestyleHint::restyle_subtree());
                        }
                        break;
                    }
                    if content == next_sibling {
                        reached_following_sibling = true;
                    }
                }
                content = dom.node(content).prev_sibling;
            }
        }
    }

    /// Call after a text node's data changed (not an append: the runtime's text changes replace the
    /// data).
    // Gecko: layout/style/RestyleManager.cpp RestyleManager::CharacterDataChanged (the !aInfo.mAppend
    // branch)
    pub fn character_data_changed(&mut self, dom: &Dom, content: NodeId) {
        let parent = dom.node(content).parent;
        if parent == NONE {
            return;
        }

        let slow_selector_flags = self.selector_flags(parent) & all_simple_restyle_flags();
        if !slow_selector_flags.intersects(
            ElementSelectorFlags::HAS_EMPTY_SELECTOR
                | ElementSelectorFlags::HAS_EDGE_CHILD_SELECTOR,
        ) {
            // Nothing to do, no other slow selector can change as a result of this.
            return;
        }

        if dom.node(content).kind != Kind::Text {
            // Doesn't matter to styling (could be a processing instruction or a
            // comment), it can't change whether any selectors match or don't.
            return;
        }

        if dom.node(parent).kind != Kind::Element {
            return;
        }

        // FIXME(emilio): This restyles unnecessarily if the text node is the only
        // child of the parent element. Fortunately, it's uncommon to have such
        // nodes and this not being an append.
        self.restyle_for_insert_or_change(dom, content);
    }

    /// Call before an attribute of `element` changes: the first change since the last style build
    /// takes the element's snapshot (Gecko: Element::AttributeWillChange -> ServoElementSnapshot,
    /// ServoRestyleState). `name` is the attribute's local name.
    pub fn attribute_will_change(&mut self, dom: &Dom, element: NodeId, name: &str) {
        if !self.styled.contains(&element) {
            return;
        }
        let Some(snapshot) = self.snapshot_for(element) else {
            return;
        };
        if snapshot.attrs.is_none() {
            snapshot.attrs = Some(
                dom.attrs_of(element)
                    .iter()
                    .map(|a| {
                        (
                            dom.names.get(a.name).to_string(),
                            dom.attr_value(a).to_string(),
                            a.ns,
                        )
                    })
                    .collect(),
            );
        }
        match name {
            "id" => snapshot.id_changed = true,
            "class" => snapshot.class_changed = true,
            _ => snapshot.other_attributes_changed = true,
        }
        if !snapshot.changed_attrs.iter().any(|n| n == name) {
            snapshot.changed_attrs.push(name.to_string());
        }
    }

    /// Call after an attribute of `element` changed: `style` and attributes that map to style
    /// (presentational hints) restyle the element itself (Gecko: Element::AttributeChanged ->
    /// RestyleManager::AttributeChanged: RESTYLE_STYLE_ATTRIBUTE, and RESTYLE_SELF for a mapped
    /// attribute; here every attribute other than id and class counts as mapped, which restyles more
    /// than Gecko, never less).
    pub fn attribute_changed(&mut self, element: NodeId, name: &str) {
        if !self.styled.contains(&element) {
            return;
        }
        match name {
            "id" | "class" => {}
            "style" => self.post_restyle_event(element, RestyleHint::RESTYLE_STYLE_ATTRIBUTE),
            _ => self.post_restyle_event(element, RestyleHint::RESTYLE_SELF),
        }
    }

    /// Call before the state that selectors read changes (`:defined`), with the old state bits
    /// (Gecko: ElementStateChanged takes the snapshot's state).
    pub fn state_will_change(&mut self, element: NodeId, old_state: u64) {
        if !self.styled.contains(&element) {
            return;
        }
        let Some(snapshot) = self.snapshot_for(element) else {
            return;
        };
        if snapshot.state.is_none() {
            snapshot.state = Some(old_state);
        }
    }

    /// Call after a shadow root was attached to `host`: its children leave the flattened tree unless
    /// a slot takes them, so the host's subtree is restyled (Gecko reframes and restyles the host).
    pub fn shadow_attached(&mut self, host: NodeId) {
        if self.styled.contains(&host) {
            self.push(RestyleOp::ClearSubtree(host));
            self.push(RestyleOp::Inserted(host));
        }
    }

    fn is_everything(&self) -> bool {
        matches!(self.ops.as_slice(), [RestyleOp::Everything])
    }

    /// The element's snapshot in the log, made on its first change since the last style build;
    /// `None` once the log is `Everything` (no build will read a snapshot then).
    fn snapshot_for(&mut self, element: NodeId) -> Option<&mut ElementSnapshotData> {
        let index = match self.snapshots.get(&element) {
            Some(&i) => i,
            None => {
                self.push(RestyleOp::Snapshot(element, ElementSnapshotData::default()));
                if self.is_everything() {
                    return None;
                }
                let i = self.ops.len() - 1;
                self.snapshots.insert(element, i);
                i
            }
        };
        match &mut self.ops[index] {
            RestyleOp::Snapshot(_, data) => Some(data),
            _ => None,
        }
    }
}

// Gecko: dom/base/nsINode.h NodeSelectorFlags::AllSimpleRestyleFlags
fn all_simple_restyle_flags() -> ElementSelectorFlags {
    ElementSelectorFlags::HAS_SLOW_SELECTOR
        | ElementSelectorFlags::HAS_SLOW_SELECTOR_LATER_SIBLINGS
        | ElementSelectorFlags::HAS_SLOW_SELECTOR_NTH
        | ElementSelectorFlags::HAS_SLOW_SELECTOR_NTH_OF
        | ElementSelectorFlags::HAS_EDGE_CHILD_SELECTOR
        | ElementSelectorFlags::HAS_EMPTY_SELECTOR
}

// Gecko: layout/style/nsStyleUtil.cpp nsStyleUtil::IsSignificantChild (aWhitespaceIsSignificant false)
fn is_significant_child(dom: &Dom, child: NodeId) -> bool {
    match dom.node(child).kind {
        Kind::Element => true,
        Kind::Text => !dom
            .text_of(child)
            .chars()
            .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')),
        _ => false,
    }
}

// Gecko: layout/style/RestyleManager.cpp HasAnySignificantSibling
fn has_any_significant_sibling(dom: &Dom, container: NodeId, child: NodeId) -> bool {
    let mut c = dom.node(container).first_child;
    while c != NONE {
        if c != child && is_significant_child(dom, c) {
            return true;
        }
        c = dom.node(c).next_sibling;
    }
    false
}
