// Gecko: layout/base/nsCounterManager.h, nsCounterManager.cpp, nsQuoteList.h, nsQuoteList.cpp
// (NOT A TRANSLATION: a different structure that computes the same values)
//
// The values of CSS counters (`counter-reset`, `counter-increment`, `counter-set`, the implicit
// `list-item` counter), the text of `counter()` and `counters()`, the marker text of list items, and the
// quote characters of `open-quote` and `close-quote`, for all the generated content of a document.
//
// Gecko keeps nodes in nsCounterList and nsQuoteList, inserted as frames are constructed in any order
// and recalculated when the lists are dirty (RecalcAll). The frame constructor of this runtime builds the
// frames of a page once, in document order, so one walk over the elements in document order gives each
// piece of generated content its text before any frame exists, and there are no lists to keep sorted.
// The scope rules are CSS Lists 3 (a counter made by an element is visible to the element, its following
// siblings and their descendants), which is what nsCounterList::SetScope finds.
//
// Gecko's rules that are kept: the order of the operations on one element (reset, increment, set),
// `display: list-item` increments `list-item` by 1 (by -1 in a reversed counter) unless
// `counter-increment` names it, a reset by a sibling's counter replaces it, `<ol start>` and
// `<ol reversed>` and `<li value>` (HTMLSharedListElement::MapAttributesIntoRule and
// HTMLLIElement::MapAttributesIntoRule), and `display: none` boxes change no counter.
// NOT PORTED: `contain: style` scopes, counters in the shadow tree and flat tree, `counter-reset:
// reversed(name)` without `start` for counters other than `list-item` (it counts from 0), the content
// of absolutely positioned and `details` boxes that have no frame (they still count), and counter styles
// other than the builtin ones that CounterStyleManager.rs has.

use crate::*;
use mrt_style::mrt_dom::{Kind, NodeId, DOCUMENT, NONE};
use mrt_style::style::properties::ComputedValues;
use mrt_style::style::selector_parser::PseudoElement;
use mrt_style::style::values::generics::counters::{Content, ContentItem};
use mrt_style::style::values::computed::list::Quotes;
use mrt_style::style::Atom;
use mrt_style::StyleDom;
use std::collections::HashMap;

// ADAPTER: which generated content of an element a text belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GenConKind {
    Before,
    After,
    Marker,
}

// Gecko: layout/generic/nsCounterManager.h nsCounterNode (the RESET node, as the instance of a counter)
struct CounterInstance {
    value: i32,
    reversed: bool,
    // the parent of the element that made the counter (the element itself for a ::before)
    mScopeParent: NodeId,
}

// Gecko: layout/base/nsCounterManager.h nsCounterManager
pub struct nsCounterManager {
    // the text of each piece of generated content that depends on the order: one string for each
    // item of `content` (an item that gives no text, an image, is empty); for a ::marker made from
    // `list-style`, one string
    mTexts: HashMap<(NodeId, GenConKind), Vec<String>>,
}

// Gecko: dom/html/HTMLSharedListElement.cpp's start for a reversed counter without `start`
const kContentBasedReversed: i32 = i32::MIN;

struct Walk<'a> {
    sd: &'a StyleDom,
    out: HashMap<(NodeId, GenConKind), Vec<String>>,
    // counters by name, innermost last; and for each level of the walk the names made by the children
    // of its element, which end when it does
    counters: HashMap<Atom, Vec<CounterInstance>>,
    scopes: Vec<Vec<Atom>>,
    // the nesting depth of quotes (nsQuoteList)
    quoteDepth: i32,
}

enum Event {
    Enter(NodeId),
    Leave(NodeId),
}

impl nsCounterManager {
    // ADAPTER: walks the whole document in order. Cost is one pass over the elements.
    pub fn new(aStyleDom: &StyleDom) -> nsCounterManager {
        let mut walk = Walk { sd: aStyleDom, out: HashMap::new(), counters: HashMap::new(), scopes: vec![Vec::new()], quoteDepth: 0 };
        let mut events = Vec::new();
        walk.PushChildren(aStyleDom.tree.doc, &mut events);
        while let Some(e) = events.pop() {
            match e {
                Event::Enter(n) => walk.Enter(n, &mut events),
                Event::Leave(n) => walk.Leave(n),
            }
        }
        nsCounterManager { mTexts: walk.out }
    }

    // ADAPTER: the texts of the pseudo-element aKind of aContent, if the walk made some.
    pub fn GetTexts(&self, aContent: NodeId, aKind: GenConKind) -> Option<&Vec<String>> {
        self.mTexts.get(&(aContent, aKind))
    }
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreateGeneratedContent, the String
// case (the items that need no counter manager; `attr()` is substituted by Stylo through
// TElement::get_attr, so it arrives here as a String)
// ADAPTER: the text of one item of `content`, or None for the kinds that need the walk.
pub fn GetStaticItemText(aStyleDom: &StyleDom, aOriginatingElement: NodeId, aItem: &ContentItem<mrt_style::style::values::computed::Image>) -> Option<String> {
    match aItem {
        ContentItem::String(s) => Some(s.to_string()),
        // images: NOT PORTED
        ContentItem::Image(_) => Some(String::new()),
        _ => None,
    }
}

// ADAPTER: does aItem need the walk to be rendered (counter, counters, quotes)?
pub fn ItemNeedsCounterManager(aItem: &ContentItem<mrt_style::style::values::computed::Image>) -> bool {
    matches!(aItem, ContentItem::Counter(..) | ContentItem::Counters(..) | ContentItem::OpenQuote | ContentItem::CloseQuote | ContentItem::NoOpenQuote | ContentItem::NoCloseQuote)
}

// ADAPTER: the items of `content` that are not alternative text (StyleContent::NonAltContentItems);
// empty for `normal` and `none`.
pub fn NonAltContentItems(aStyle: &ComputedValues) -> &[ContentItem<mrt_style::style::values::computed::Image>] {
    match &aStyle.get_counters().content {
        Content::Items(items) => &items.items[..items.alt_start.min(items.items.len())],
        _ => &[],
    }
}

// ADAPTER: the name the style of a counter has in CounterStyleManager.rs: the CSS name without
// hyphens (`lower-alpha` is "loweralpha"). NOT PORTED: `symbols()` and a string as the counter
// style; they fall back to decimal, as an unknown name does.
pub fn CounterStyleName(aStyle: &mrt_style::style::counter_style::CounterStyle) -> String {
    use mrt_style::style::counter_style::CounterStyle;
    match aStyle {
        CounterStyle::None => "none".to_string(),
        CounterStyle::Name(ident) => ident.0.to_string().replace('-', "").to_ascii_lowercase(),
        _ => "decimal".to_string(),
    }
}

impl<'a> Walk<'a> {
    fn PushChildren(&self, aParent: NodeId, aEvents: &mut Vec<Event>) {
        // pushed in reverse so that the first child is popped first
        // the flattened tree's children (Gecko counts in frame order, which follows it)
        let dom = &self.sd.tree.dom;
        let kids: Vec<NodeId> = dom.flattened_children(aParent).into_iter().filter(|&c| dom.node(c).kind == Kind::Element).collect();
        for &k in kids.iter().rev() {
            aEvents.push(Event::Leave(k));
            aEvents.push(Event::Enter(k));
        }
    }

    // Gecko: layout/base/nsCounterManager.cpp nsCounterManager::AddCounterChanges for the element,
    // then its ::marker and ::before; the children are pushed on the event stack above the element's
    // Leave, which renders the ::after.
    fn Enter(&mut self, aElement: NodeId, aEvents: &mut Vec<Event>) {
        // the counters the children of aElement make end in the Leave of aElement
        self.scopes.push(Vec::new());
        let Some(style) = self.sd.computed(aElement) else {
            return;
        };
        use mrt_style::style::values::specified::box_::Display;
        let display = style.get_box().clone_display();
        if display == Display::None {
            // 'display:none' boxes make no counters and have no children that do
            return;
        }
        let parentFrame = self.scopes.len() - 2;
        let parent = self.sd.tree.dom.flattened_parent(aElement);
        if display != Display::Contents {
            self.ApplyCounterChanges(aElement, &style, parentFrame, display.is_list_item(), parent);
            if display.is_list_item() {
                if let Some(marker) = self.sd.pseudo_style(aElement, PseudoElement::Marker) {
                    self.RenderMarker(aElement, &marker);
                }
            }
        }
        // the ::before and the ::after of an element are children of it: the counters they make belong
        // to the scope of the element's children
        let me = self.scopes.len() - 1;
        if let Some(before) = self.sd.pseudo_style(aElement, PseudoElement::Before) {
            self.ApplyCounterChanges(aElement, &before, me, false, aElement);
            self.RenderItems(aElement, &before, GenConKind::Before);
        }
        self.PushChildren(aElement, aEvents);
    }

    fn Leave(&mut self, aElement: NodeId) {
        if let Some(style) = self.sd.computed(aElement) {
            use mrt_style::style::values::specified::box_::Display;
            if style.get_box().clone_display() != Display::None {
                if let Some(after) = self.sd.pseudo_style(aElement, PseudoElement::After) {
                    let me = self.scopes.len() - 1;
                    self.ApplyCounterChanges(aElement, &after, me, false, aElement);
                    self.RenderItems(aElement, &after, GenConKind::After);
                }
            }
        }
        // the counters made by the children of aElement end here
        for name in self.scopes.pop().unwrap_or_default() {
            if let Some(stack) = self.counters.get_mut(&name) {
                stack.pop();
                if stack.is_empty() {
                    self.counters.remove(&name);
                }
            }
        }
    }

    // ADAPTER: make the counter aName at the element, by a reset: a counter that a sibling made
    // (same parent) is replaced, any other is hidden by a nested one.
    fn Reset(&mut self, aName: &Atom, aValue: i32, aReversed: bool, aScopeFrame: usize, aParent: NodeId) {
        let stack = self.counters.entry(aName.clone()).or_default();
        if let Some(top) = stack.last_mut() {
            if top.mScopeParent == aParent {
                top.value = aValue;
                top.reversed = aReversed;
                return;
            }
        }
        stack.push(CounterInstance { value: aValue, reversed: aReversed, mScopeParent: aParent });
        self.scopes[aScopeFrame].push(aName.clone());
    }

    // Gecko: layout/base/nsCounterManager.cpp nsCounterManager::AddCounterChanges
    // aScopeFrame: the level of `scopes` whose end is the end of the counters this element makes.
    fn ApplyCounterChanges(&mut self, aElement: NodeId, aStyle: &ComputedValues, aScopeFrame: usize, aRequiresListItemIncrement: bool, aParent: NodeId) {
        let content = aStyle.get_counters();
        let isElementItself = aParent != aElement;
        let list_item = Atom::from("list-item");

        // Resets first, ...
        for pair in content.counter_reset.iter() {
            let mut value = pair.value;
            let mut reversed = pair.is_reversed;
            if isElementItself && pair.name.0 == list_item {
                // <ol start> and <ol reversed>: HTMLSharedListElement::MapAttributesIntoRule
                if let Some((v, r)) = self.ListStartFromAttributes(aElement) {
                    value = v;
                    reversed = r;
                }
            }
            if reversed && value == kContentBasedReversed {
                // content-based reversed(): counts the list items in the scope
                value = self.CountListItems(aElement) + 1;
            }
            self.Reset(&pair.name.0, value, reversed, aScopeFrame, aParent);
        }
        // ... then increments ...
        let mut hasListItemIncrement = false;
        for pair in content.counter_increment.iter() {
            hasListItemIncrement |= pair.name.0 == list_item;
            if pair.value != 0 {
                self.Increment(&pair.name.0, pair.value, aScopeFrame, aParent, false);
            }
        }
        // For elements with 'display:list-item' we add a default 'counter-increment:list-item'
        // unless 'counter-increment' already has a value for 'list-item'.
        if aRequiresListItemIncrement && !hasListItemIncrement {
            self.Increment(&list_item, 1, aScopeFrame, aParent, true);
        }
        // ... then sets (`counter-set` itself: NOT PORTED, Stylo's servo configuration does not have it)
        // <li value>: HTMLLIElement::MapAttributesIntoRule maps it to `counter-set: list-item INTEGER`
        if isElementItself && aRequiresListItemIncrement && self.sd.tree.dom.tag(aElement) == "li" {
            if let Some(v) = self.IntegerAttr(aElement, "value") {
                self.Set(&list_item, v, aScopeFrame, aParent);
            }
        }
    }

    // Instantiates the counter (a reset to 0 on the element) when none is in scope.
    fn Instance(&mut self, aName: &Atom, aScopeFrame: usize, aParent: NodeId) -> &mut CounterInstance {
        if !self.counters.get(aName).is_some_and(|s| !s.is_empty()) {
            self.Reset(aName, 0, false, aScopeFrame, aParent);
        }
        self.counters.get_mut(aName).unwrap().last_mut().unwrap()
    }

    fn Increment(&mut self, aName: &Atom, aValue: i32, aScopeFrame: usize, aParent: NodeId, aImplicit: bool) {
        let c = self.Instance(aName, aScopeFrame, aParent);
        // the implicit list-item increment is -1 in a reversed counter
        let by = if aImplicit && c.reversed { -aValue } else { aValue };
        c.value = c.value.wrapping_add(by);
    }

    fn Set(&mut self, aName: &Atom, aValue: i32, aScopeFrame: usize, aParent: NodeId) {
        self.Instance(aName, aScopeFrame, aParent).value = aValue;
    }

    fn IntegerAttr(&self, aElement: NodeId, aName: &str) -> Option<i32> {
        let dom = &self.sd.tree.dom;
        let a = dom.attrs_of(aElement).iter().find(|a| dom.names.get(a.name) == aName)?;
        // HTML's rules for parsing integers: optional white space, a sign, digits
        let v = dom.attr_value(a).trim_start_matches([' ', '\t', '\n', '\x0c', '\r']);
        let (neg, v) = match v.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, v.strip_prefix('+').unwrap_or(v)),
        };
        let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }
        let n = digits.parse::<i64>().ok()?;
        i32::try_from(if neg { -n } else { n }).ok()
    }

    // Gecko: dom/html/HTMLSharedListElement.cpp HTMLSharedListElement::MapAttributesIntoRule
    // (the value to reset `list-item` to, and whether it is reversed) for an <ol>
    fn ListStartFromAttributes(&self, aElement: NodeId) -> Option<(i32, bool)> {
        if self.sd.tree.dom.tag(aElement) != "ol" {
            return None;
        }
        let dom = &self.sd.tree.dom;
        let haveReversed = dom.attrs_of(aElement).iter().any(|a| dom.names.get(a.name) == "reversed");
        let start = self.IntegerAttr(aElement, "start");
        if start.is_none() && !haveReversed {
            return None;
        }
        let mut value = start.map_or(0, |s| s.wrapping_sub(1));
        if haveReversed {
            if start.is_some() {
                value = value.wrapping_add(2); // i.e. the attr value + 1
            } else {
                value = kContentBasedReversed;
            }
        }
        Some((value, haveReversed))
    }

    // Gecko: layout/base/nsCounterManager.cpp nsCounterList::SetScope (the "content-based reset" part: the
    // start of a reversed counter is the number of increments in its scope)
    // ADAPTER: the display:list-item elements below aList, not counting those of nested lists.
    fn CountListItems(&self, aList: NodeId) -> i32 {
        let dom = &self.sd.tree.dom;
        let mut count = 0;
        let mut stack = vec![dom.node(aList).first_child];
        while let Some(mut c) = stack.pop() {
            while c != NONE {
                let n = dom.node(c);
                if n.kind == Kind::Element {
                    let tag = dom.tag(c);
                    if let Some(style) = self.sd.computed(c) {
                        use mrt_style::style::values::specified::box_::Display;
                        let display = style.get_box().clone_display();
                        if display != Display::None {
                            if display.is_list_item() {
                                count += 1;
                            }
                            if !matches!(tag, "ol" | "ul" | "menu") {
                                stack.push(n.first_child);
                            }
                        }
                    }
                }
                c = n.next_sibling;
            }
        }
        count
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreateGeneratedContentFromListStyle
    // (the text of the marker of a list item, with the value of its `list-item` counter now)
    fn RenderMarker(&mut self, aElement: NodeId, aMarker: &ComputedValues) {
        if matches!(aMarker.get_counters().content, Content::Items(_)) {
            self.RenderItems(aElement, aMarker, GenConKind::Marker);
            return;
        }
        let value = self.counters.get(&Atom::from("list-item")).and_then(|s| s.last()).map_or(0, |c| c.value);
        let list = aMarker.get_list();
        // list-style-image: NOT PORTED (the image is left out and the text is the space that follows it)
        let text = if !matches!(list.list_style_image, mrt_style::style::values::computed::Image::None) {
            " ".to_string()
        } else {
            CounterStyle_GetMarkerText(&CounterStyleName(&list.list_style_type.0), value)
        };
        self.out.insert((aElement, GenConKind::Marker), vec![text]);
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreateGeneratedContent
    // (the Counter, Counters and quote cases, rendered at their place in document order)
    // Only content that has such an item is stored; the constructor renders the rest itself.
    fn RenderItems(&mut self, aElement: NodeId, aPseudoStyle: &ComputedValues, aKind: GenConKind) {
        let items = NonAltContentItems(aPseudoStyle);
        if !items.iter().any(ItemNeedsCounterManager) {
            return;
        }
        let mut texts = Vec::with_capacity(items.len());
        for item in items {
            let text = match item {
                ContentItem::Counter(name, style) => {
                    let value = self.counters.get(&name.0).and_then(|s| s.last()).map_or(0, |c| c.value);
                    CounterStyle_GetCounterText(&CounterStyleName(style), value)
                }
                ContentItem::Counters(name, separator, style) => {
                    let style = CounterStyleName(style);
                    let parts: Vec<String> = self.counters.get(&name.0).map_or_else(|| vec![CounterStyle_GetCounterText(&style, 0)], |s| s.iter().map(|c| CounterStyle_GetCounterText(&style, c.value)).collect());
                    parts.join(&separator.to_string())
                }
                ContentItem::OpenQuote | ContentItem::NoOpenQuote => {
                    // Gecko: nsQuoteNode::Text
                    let text = if matches!(item, ContentItem::OpenQuote) { self.QuoteText(aPseudoStyle, true) } else { String::new() };
                    self.quoteDepth += 1;
                    text
                }
                ContentItem::CloseQuote | ContentItem::NoCloseQuote => {
                    if self.quoteDepth == 0 {
                        String::new()
                    } else {
                        self.quoteDepth -= 1;
                        if matches!(item, ContentItem::CloseQuote) { self.QuoteText(aPseudoStyle, false) } else { String::new() }
                    }
                }
                other => GetStaticItemText(self.sd, aElement, other).unwrap_or_default(),
            };
            texts.push(text);
        }
        self.out.insert((aElement, aKind), texts);
    }

    // Gecko: layout/base/nsQuoteList.cpp nsQuoteNode::Text
    // The quote character of the pair for the current depth: the last pair serves all deeper levels.
    // For an opening quote the depth is the one before it opens; for a closing quote, after it closed.
    fn QuoteText(&self, aStyle: &ComputedValues, aOpen: bool) -> String {
        let depth = self.quoteDepth.max(0) as usize;
        match &aStyle.get_list().quotes {
            Quotes::QuoteList(list) => {
                let pairs = &list.0;
                if pairs.is_empty() {
                    return String::new();
                }
                let pair = &pairs[depth.min(pairs.len() - 1)];
                (if aOpen { &pair.opening } else { &pair.closing }).to_string()
            }
            // `auto`: the quotes of the language; the language is not known here, English
            // (U+201C U+201D, U+2018 U+2019)
            Quotes::Auto => {
                let (open, close) = if depth == 0 { ('\u{201C}', '\u{201D}') } else { ('\u{2018}', '\u{2019}') };
                (if aOpen { open } else { close }).to_string()
            }
        }
    }
}
