// ADAPTER (not a translation of a whole file): the parts of layout/generic/nsIFrame.h that
// translated layout code uses, on top of flat frame arrays. See CONVENTIONS.md, "Frames and styles".
//
// In Gecko a frame is a heap object reached through `nsIFrame*`, with virtual methods per frame
// class. Here a frame is a `Copy` handle (`nsIFrame`, an index) and its data lives in the arrays of
// a `FrameTree` kept in a thread-local, so translated bodies call `aFrame.GetParent()`,
// `aFrame.StyleDisplay()` and so on exactly as in C++. A C++ `nsIFrame*` that may be null is
// `Option<nsIFrame>`.
use crate::*;
use mrt_style::style::properties::ComputedValues;
use mrt_style::style::servo_arc::Arc as ServoArc;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;

// ADAPTER: Gecko's nsIContent, reduced to what layout reads of a frame's content. A frame does not
// own a copy of its node, as in Gecko, where `mContent` is one pointer into the DOM: `nsIContent` is a
// `Copy` handle of 4 bytes, and the methods below read the node from the arena DOM (mrt-dom).
//
// The DOM is reached through a thread-local (`ContentContext`), like the frame tree: the program
// gives it with `nsIContent::SetDocument` (nsCSSFrameConstructor::new does it), and the DOM must
// stay alive and unmoved for as long as frames of this thread are used. The driver's callers hold
// the `StyleDom` for the whole layout; a test whose helper returns before its test body reads the
// frames makes the `StyleDom` last with `KeepDocumentAlive` (driver.rs).
//
// Two kinds of ids:
//  - below kSyntheticContentBase: the node id of the arena DOM (0, `NONE`, is not a node; a frame
//    without content has `None`);
//  - from kSyntheticContentBase up: content that has no DOM node, an index into the side store of
//    synthetic contents (the ::marker container and its text, tests): Gecko makes those as real
//    nodes, here each is a small record of its own (`SyntheticContent`).
//
// Identity is the id, as Gecko compares `nsIContent*`: two frames made for the same DOM node have
// the same content.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct nsIContent(u32);

pub const kSyntheticContentBase: u32 = 1 << 31;

// ADAPTER: what a synthetic content is.
enum SyntheticContentKind {
    // an element: the namespace id (3 is HTML, 6 SVG, 5 MathML, as in mrt-dom) and, for an HTML
    // element only, the local name (IsHTMLElement(body) is not true for an SVG body)
    Element { mNameSpaceID: u8, mHTMLLocalName: Option<String> },
    // a Text node
    Text(std::rc::Rc<CharacterDataBuffer>),
}

struct SyntheticContent {
    mKind: SyntheticContentKind,
    // see nsIContent::mLanguage
    mLanguage: Option<nsAtom>,
}

// ADAPTER: the number of text buffers kept (see ContentContext::mBuffers).
const kContentBufferCacheSize: usize = 128;

// ADAPTER: what the contents of this thread's frames are read from.
struct ContentContext {
    // the arena DOM (null until SetDocument)
    mDom: *const mrt_style::mrt_dom::Dom,
    mSynthetic: Vec<SyntheticContent>,
    // The language of each arena node, found on first use: 0 is "not found yet", 1 is "none", and
    // n + 2 is mLanguageAtoms[n] (Gecko keeps it in the `-x-lang` style property, nsStyleFont::mLanguage).
    mLanguages: Vec<u16>,
    mLanguageAtoms: Vec<nsAtom>,
    // The text of the DOM is UTF-8 and layout reads it as 8-bit or 16-bit units by offset
    // (CharacterDataBuffer), so a Text node's buffer is made when asked for. A direct-mapped cache
    // of the last ones (node id modulo the size) keeps the cost of the lines of one text node to one
    // conversion; ClearContentCaches empties it when layout is done (Gecko's DOM holds the text once).
    mBuffers: Vec<Option<(u32, std::rc::Rc<CharacterDataBuffer>)>>,
    // Gecko: the `nsGkAtoms::flowlength` property of a text node (nsTextFrame::GetInFlowContentLength's
    // cache); a node has an entry exactly when it has NS_HAS_FLOWLENGTH_PROPERTY.
    mFlowLengths: std::collections::HashMap<u32, FlowLengthProperty>,
    // Gecko: nsIContent::mPrimaryFrame of the arena's nodes, indexed by node id: the frame id plus
    // one, 0 for none (generated content keeps its own, in the frame constructor).
    mPrimaryFrames: Vec<u32>,
    // Gecko: the frame constructor's flags on nsINode (NS_CREATE_FRAME_IF_NON_WHITESPACE,
    // NS_REFRAME_IF_WHITESPACE) of the arena's nodes, indexed by node id.
    mNodeFlags: Vec<u8>,
}

// Gecko: dom/base/nsINode.h NODE_TYPE_SPECIFIC_BITS used by layout (nsTextFrame.h, nsCSSFrameConstructor):
// This bit is set on text nodes (and text frames) whose frame was suppressed because they are only
// whitespace next to a line boundary.
pub const NS_CREATE_FRAME_IF_NON_WHITESPACE: u8 = 1 << 0;
// This bit is set on text nodes whose text frame should be reconstructed if the text becomes only
// whitespace.
pub const NS_REFRAME_IF_WHITESPACE: u8 = 1 << 1;

thread_local! {
    static CONTENT: RefCell<ContentContext> = RefCell::new(ContentContext {
        mDom: std::ptr::null(),
        mSynthetic: Vec::new(),
        mLanguages: Vec::new(),
        mLanguageAtoms: Vec::new(),
        mBuffers: Vec::new(),
        mFlowLengths: std::collections::HashMap::new(),
        mPrimaryFrames: Vec::new(),
        mNodeFlags: Vec::new(),
    });
}

impl ContentContext {
    fn dom(&self) -> &mrt_style::mrt_dom::Dom {
        assert!(!self.mDom.is_null(), "frame content read before nsIContent::SetDocument");
        // SAFETY: SetDocument's caller keeps the DOM alive and in place while frames are used
        unsafe { &*self.mDom }
    }

    // ADAPTER: the language of an arena node (nsIContent::mLanguage): the lower-cased value of the
    // nearest `lang` attribute of the element or its ancestors (a Text node has its parent's); an
    // empty `lang` means "no language", as in HTML, and so does none at all. The document's language
    // (`Content-Language`, the locale) is NOT PORTED.
    fn LanguageOfNode(&mut self, aNode: u32) -> Option<nsAtom> {
        use mrt_style::mrt_dom::{Kind, NONE};
        let dom = self.dom();
        let element = if dom.node(aNode).kind == Kind::Text {
            let parent = dom.flattened_parent(aNode);
            if parent == NONE {
                return None;
            }
            parent
        } else {
            aNode
        };
        if self.mLanguages.len() < dom.nodes.len() {
            self.mLanguages.resize(dom.nodes.len(), 0);
        }
        // Walk up to the first element whose language is known or that sets one, then give the
        // elements passed on the way the same answer.
        let mut chain: Vec<u32> = Vec::new();
        let mut cursor = element;
        let result: Option<nsAtom> = loop {
            let cached = self.mLanguages[cursor as usize];
            if cached != 0 {
                break if cached == 1 { None } else { Some(self.mLanguageAtoms[cached as usize - 2]) };
            }
            chain.push(cursor);
            let dom = self.dom();
            let lang = dom.attrs_of(cursor).iter().find(|a| dom.names.get(a.name) == "lang").map(|a| dom.attr_value(a).trim().to_ascii_lowercase());
            if let Some(lang) = lang {
                break if lang.is_empty() { None } else { Some(InternContentAtom(&lang)) };
            }
            let parent = dom.flattened_parent(cursor);
            if parent == NONE || dom.node(parent).kind != Kind::Element {
                break None;
            }
            cursor = parent;
        };
        let code: u16 = match result {
            None => 1,
            Some(atom) => {
                let index = match self.mLanguageAtoms.iter().position(|a| *a == atom) {
                    Some(i) => i,
                    None => {
                        self.mLanguageAtoms.push(atom);
                        self.mLanguageAtoms.len() - 1
                    }
                };
                index as u16 + 2
            }
        };
        for n in chain {
            self.mLanguages[n as usize] = code;
        }
        result
    }
}

// ADAPTER: Gecko's nsAtom for a string known only at run time (the atom table); the strings are kept for the
// life of the thread, as the atoms of the table are.
fn InternContentAtom(aString: &str) -> nsAtom {
    thread_local! {
        static sContentAtoms: RefCell<std::collections::HashSet<&'static str>> = RefCell::new(std::collections::HashSet::new());
    }
    sContentAtoms.with(|atoms| {
        let mut atoms = atoms.borrow_mut();
        if let Some(a) = atoms.get(aString) {
            return *a;
        }
        let a: &'static str = Box::leak(aString.to_string().into_boxed_str());
        atoms.insert(a);
        a
    })
}

impl nsIContent {
    // ADAPTER: says which arena DOM the contents of this thread's frames are read from (see the top
    // of nsIContent). Forgets what was found about the previous one.
    pub fn SetDocument(aDom: &mrt_style::mrt_dom::Dom) {
        CONTENT.with(|c| {
            let mut c = c.borrow_mut();
            c.mDom = aDom as *const _;
            c.mLanguages.clear();
            c.mBuffers.clear();
            c.mFlowLengths.clear();
        })
    }

    // ADAPTER: forgets the document and everything read from it (stage 4: the frame tree is dropped and
    // may be built again, possibly over a DOM that changed). The atom tables stay, as Gecko's do.
    pub fn ResetContentContext() {
        CONTENT.with(|c| {
            let mut c = c.borrow_mut();
            c.mDom = std::ptr::null();
            c.mSynthetic = Vec::new();
            c.mLanguages = Vec::new();
            c.mLanguageAtoms = Vec::new();
            c.mBuffers = Vec::new();
            c.mFlowLengths = std::collections::HashMap::new();
            c.mPrimaryFrames = Vec::new();
            c.mNodeFlags = Vec::new();
        })
    }

    // ADAPTER: forgets what was read from the DOM that a change may have made stale (the languages,
    // which follow `lang` attributes, and the text buffers); the frames and the primary frames stay.
    // The layout thread calls it before a flush applies the DOM's changes to a kept frame tree.
    pub fn ForgetDocumentReads() {
        CONTENT.with(|c| {
            let mut c = c.borrow_mut();
            c.mLanguages.clear();
            c.mBuffers = Vec::new();
        })
    }

    // Gecko: dom/base/nsIContent.h nsIContent::GetPrimaryFrame (an arena node; a synthetic content
    // has none here, the frame constructor keeps those of generated content)
    pub fn GetPrimaryFrame(&self) -> Option<nsIFrame> {
        if self.IsSynthetic() {
            return None;
        }
        CONTENT.with(|c| match c.borrow().mPrimaryFrames.get(self.0 as usize) {
            Some(&f) if f != 0 => Some(nsIFrame(f - 1)),
            _ => None,
        })
    }

    // Gecko: dom/base/nsINode.h nsINode::HasFlag (the layout flags above; a synthetic content has none)
    pub fn HasFlag(&self, aFlag: u8) -> bool {
        !self.IsSynthetic() && CONTENT.with(|c| c.borrow().mNodeFlags.get(self.0 as usize).is_some_and(|f| f & aFlag != 0))
    }

    // Gecko: dom/base/nsINode.h nsINode::SetFlags
    pub fn SetFlags(&self, aFlags: u8) {
        if self.IsSynthetic() {
            return;
        }
        CONTENT.with(|c| {
            let mut c = c.borrow_mut();
            let i = self.0 as usize;
            if c.mNodeFlags.len() <= i {
                c.mNodeFlags.resize(i + 1, 0);
            }
            c.mNodeFlags[i] |= aFlags;
        })
    }

    // Gecko: dom/base/nsINode.h nsINode::UnsetFlags
    pub fn UnsetFlags(&self, aFlags: u8) {
        if self.IsSynthetic() {
            return;
        }
        CONTENT.with(|c| {
            if let Some(f) = c.borrow_mut().mNodeFlags.get_mut(self.0 as usize) {
                *f &= !aFlags;
            }
        })
    }

    // Gecko: dom/base/nsIContent.h nsIContent::SetPrimaryFrame
    pub fn SetPrimaryFrame(&self, aFrame: Option<nsIFrame>) {
        if self.IsSynthetic() {
            return;
        }
        CONTENT.with(|c| {
            let mut c = c.borrow_mut();
            let i = self.0 as usize;
            if c.mPrimaryFrames.len() <= i {
                if aFrame.is_none() {
                    return;
                }
                c.mPrimaryFrames.resize(i + 1, 0);
            }
            c.mPrimaryFrames[i] = aFrame.map_or(0, |f| f.0 + 1);
        })
    }

    // ADAPTER: drops the text buffers made so far (see ContentContext::mBuffers) and gives back the
    // spare capacity of the other tables. Layout does not need the buffers once it is done.
    pub fn ClearContentCaches() {
        CONTENT.with(|c| {
            let mut c = c.borrow_mut();
            c.mBuffers = Vec::new();
            c.mSynthetic.shrink_to_fit();
            c.mLanguages.shrink_to_fit();
            c.mLanguageAtoms.shrink_to_fit();
        })
    }

    // ADAPTER: the content of a node of the arena DOM.
    pub fn FromNodeId(aNode: u32) -> nsIContent {
        debug_assert!(aNode != 0 && aNode < kSyntheticContentBase);
        nsIContent(aNode)
    }

    fn NewSynthetic(aKind: SyntheticContentKind) -> nsIContent {
        CONTENT.with(|c| {
            let mut c = c.borrow_mut();
            c.mSynthetic.push(SyntheticContent { mKind: aKind, mLanguage: None });
            nsIContent(kSyntheticContentBase + (c.mSynthetic.len() - 1) as u32)
        })
    }

    // ADAPTER: content of an HTML element with this local name that has no DOM node.
    pub fn HTMLElement(aLocalName: &str) -> nsIContent {
        nsIContent::NewSynthetic(SyntheticContentKind::Element { mNameSpaceID: 3, mHTMLLocalName: Some(aLocalName.to_string()) })
    }

    // ADAPTER: content of an element of another namespace (SVG, MathML) that has no DOM node.
    pub fn ForeignElement(aNameSpaceID: u8) -> nsIContent {
        nsIContent::NewSynthetic(SyntheticContentKind::Element { mNameSpaceID: aNameSpaceID, mHTMLLocalName: None })
    }

    // ADAPTER: a Text node with this text that has no DOM node.
    pub fn TextNode(aText: &str) -> nsIContent {
        nsIContent::NewSynthetic(SyntheticContentKind::Text(std::rc::Rc::new(CharacterDataBuffer::from_str(aText))))
    }

    // ADAPTER: this synthetic content with the language of the text it is in (an arena node finds its own).
    pub fn WithLanguage(self, aLanguage: Option<nsAtom>) -> nsIContent {
        MOZ_ASSERT!(self.0 >= kSyntheticContentBase, "the language of a DOM node is found from the DOM");
        CONTENT.with(|c| c.borrow_mut().mSynthetic[(self.0 - kSyntheticContentBase) as usize].mLanguage = aLanguage);
        self
    }

    // Gecko: mContent->HasFlag(NS_HAS_FLOWLENGTH_PROPERTY)
    pub fn HasFlowLengthProperty(&self) -> bool {
        CONTENT.with(|c| c.borrow().mFlowLengths.contains_key(&self.0))
    }
    // Gecko: mContent->GetProperty(nsGkAtoms::flowlength)
    pub fn GetFlowLengthProperty(&self) -> Option<FlowLengthProperty> {
        CONTENT.with(|c| c.borrow().mFlowLengths.get(&self.0).copied())
    }
    // Gecko: mContent->SetProperty(nsGkAtoms::flowlength, ...) and SetFlags(NS_HAS_FLOWLENGTH_PROPERTY)
    pub fn SetFlowLengthProperty(&self, aFlowLength: FlowLengthProperty) {
        CONTENT.with(|c| {
            c.borrow_mut().mFlowLengths.insert(self.0, aFlowLength);
        });
    }
    // Gecko: mContent->RemoveProperty(nsGkAtoms::flowlength) and UnsetFlags(NS_HAS_FLOWLENGTH_PROPERTY)
    pub fn RemoveFlowLengthProperty(&self) {
        CONTENT.with(|c| {
            c.borrow_mut().mFlowLengths.remove(&self.0);
        });
    }

    // ADAPTER: the node id (arena DOM nodes) or synthetic id, for `Option<nsIContent>` stored as a number.
    pub fn Id(&self) -> u32 {
        self.0
    }

    // ADAPTER: the inverse of Id; 0 is "no content".
    pub fn FromId(aId: u32) -> Option<nsIContent> {
        if aId == 0 { None } else { Some(nsIContent(aId)) }
    }

    fn IsSynthetic(&self) -> bool {
        self.0 >= kSyntheticContentBase
    }

    // ADAPTER: runs `f` on the synthetic content of this id.
    fn with_synthetic<R>(&self, f: impl FnOnce(&SyntheticContent) -> R) -> R {
        CONTENT.with(|c| f(&c.borrow().mSynthetic[(self.0 - kSyntheticContentBase) as usize]))
    }

    // ADAPTER: the HTML local name (Gecko: NodeInfo::NameAtom of an HTML element); None for text
    // and for elements of other namespaces.
    pub fn mHTMLLocalName(&self) -> Option<String> {
        self.with_HTMLLocalName(|n| n.map(|s| s.to_string()))
    }

    // ADAPTER: `f` is given the HTML local name without a copy (see mHTMLLocalName).
    pub fn with_HTMLLocalName<R>(&self, f: impl FnOnce(Option<&str>) -> R) -> R {
        if self.IsSynthetic() {
            return self.with_synthetic(|s| match &s.mKind {
                SyntheticContentKind::Element { mHTMLLocalName, .. } => f(mHTMLLocalName.as_deref()),
                SyntheticContentKind::Text(_) => f(None),
            });
        }
        CONTENT.with(|c| {
            let c = c.borrow();
            let dom = c.dom();
            let n = dom.node(self.0);
            if n.kind == mrt_style::mrt_dom::Kind::Element && n.ns == 3 { f(Some(dom.tag(self.0))) } else { f(None) }
        })
    }

    // ADAPTER: Gecko's NodeInfo namespace id (3 is HTML, 6 SVG, 5 MathML, as in mrt-dom); 0 for text.
    pub fn mNameSpaceID(&self) -> u8 {
        if self.IsSynthetic() {
            return self.with_synthetic(|s| match &s.mKind {
                SyntheticContentKind::Element { mNameSpaceID, .. } => *mNameSpaceID,
                SyntheticContentKind::Text(_) => 0,
            });
        }
        CONTENT.with(|c| {
            let c = c.borrow();
            let n = c.dom().node(self.0);
            if n.kind == mrt_style::mrt_dom::Kind::Element { n.ns } else { 0 }
        })
    }

    // ADAPTER: the text of a Text node (Gecko: CharacterData::mBuffer, nsTextFrame reads it through
    // `mContent->AsText()->DataBuffer()`). None for elements. See ContentContext::mBuffers.
    pub fn mCharacterDataBuffer(&self) -> Option<std::rc::Rc<CharacterDataBuffer>> {
        if self.IsSynthetic() {
            return self.with_synthetic(|s| match &s.mKind {
                SyntheticContentKind::Text(b) => Some(b.clone()),
                SyntheticContentKind::Element { .. } => None,
            });
        }
        CONTENT.with(|c| {
            let mut c = c.borrow_mut();
            if c.dom().node(self.0).kind != mrt_style::mrt_dom::Kind::Text {
                return None;
            }
            if c.mBuffers.is_empty() {
                c.mBuffers.resize(kContentBufferCacheSize, None);
            }
            let slot = self.0 as usize % kContentBufferCacheSize;
            if let Some((id, buffer)) = &c.mBuffers[slot] {
                if *id == self.0 {
                    return Some(buffer.clone());
                }
            }
            let buffer = std::rc::Rc::new(CharacterDataBuffer::from_str(c.dom().text_of(self.0)));
            c.mBuffers[slot] = Some((self.0, buffer.clone()));
            Some(buffer)
        })
    }

    // ADAPTER: the language of the node, the value of the nearest `lang` attribute of the element or its
    // ancestors (lower case), which Gecko keeps in the `-x-lang` property of the style (nsStyleFont::mLanguage)
    // and the font lookup reads. None when no element sets one. See nsLayoutUtils::GetFontMetricsForFrame.
    pub fn mLanguage(&self) -> Option<nsAtom> {
        if self.IsSynthetic() {
            return self.with_synthetic(|s| s.mLanguage);
        }
        CONTENT.with(|c| c.borrow_mut().LanguageOfNode(self.0))
    }

    // Gecko: dom/base/nsINode.h nsINode::IsSVGElement
    pub fn IsSVGElement(&self) -> bool {
        self.mNameSpaceID() == 6
    }

    // Gecko: dom/base/nsINode.h nsINode::IsText
    pub fn IsText(&self) -> bool {
        if self.IsSynthetic() {
            return self.with_synthetic(|s| matches!(s.mKind, SyntheticContentKind::Text(_)));
        }
        CONTENT.with(|c| c.borrow().dom().node(self.0).kind == mrt_style::mrt_dom::Kind::Text)
    }

    // Gecko: dom/base/CharacterData.h CharacterData::TextLength (in UTF-16 code units)
    pub fn TextLength(&self) -> u32 {
        if self.IsSynthetic() {
            return self.with_synthetic(|s| match &s.mKind {
                SyntheticContentKind::Text(b) => b.GetLength(),
                SyntheticContentKind::Element { .. } => 0,
            });
        }
        CONTENT.with(|c| {
            let c = c.borrow();
            let dom = c.dom();
            if dom.node(self.0).kind != mrt_style::mrt_dom::Kind::Text {
                return 0;
            }
            dom.text_of(self.0).encode_utf16().count() as u32
        })
    }

    // Gecko: dom/base/CharacterData.h CharacterData::TextIsOnlyWhitespace
    // (whitespace is the HTML one: space, tab, line feed, form feed, carriage return)
    pub fn TextIsOnlyWhitespace(&self) -> bool {
        if self.IsSynthetic() {
            return self.with_synthetic(|s| match &s.mKind {
                SyntheticContentKind::Text(b) => b.IsOnlyWhitespace(),
                SyntheticContentKind::Element { .. } => false,
            });
        }
        CONTENT.with(|c| {
            let c = c.borrow();
            let dom = c.dom();
            dom.node(self.0).kind == mrt_style::mrt_dom::Kind::Text && dom.text_of(self.0).chars().all(|ch| matches!(ch, ' ' | '\t' | '\n' | '\x0c' | '\r'))
        })
    }

    // ADAPTER: `content1 == content2` (pointer identity in Gecko).
    pub fn IsSameNode(aContent1: &Option<nsIContent>, aContent2: &Option<nsIContent>) -> bool {
        aContent1 == aContent2
    }

    // Gecko: dom/base/nsIContent.h (NodeInfo) IsHTMLElement(nsAtom*)
    pub fn IsHTMLElement(&self, aName: nsAtom) -> bool {
        self.with_HTMLLocalName(|n| n == Some(aName))
    }

    // Gecko: dom/base/nsIContent.h IsAnyOfHTMLElements
    pub fn IsAnyOfHTMLElements(&self, aNames: &[nsAtom]) -> bool {
        self.with_HTMLLocalName(|n| n.map_or(false, |n| aNames.iter().any(|a| *a == n)))
    }
}

// ADAPTER: Gecko's nsAtom* for the static atoms of nsGkAtoms; here the string itself.
pub type nsAtom = &'static str;

// Gecko: xpcom/ds/nsGkAtomList.h (the atoms layout code compares against; add one when needed)
pub mod nsGkAtoms {
    use super::nsAtom;
    pub const body: nsAtom = "body";
    pub const html: nsAtom = "html";
    pub const button: nsAtom = "button";
    pub const br: nsAtom = "br";
    pub const p: nsAtom = "p";
    pub const select: nsAtom = "select";
}

// ADAPTER: what is stored for one frame.
pub struct FrameData {
    pub mParent: Option<nsIFrame>,
    pub mNextSibling: Option<nsIFrame>,
    // Gecko: nsIFrame::mPrevSibling ("Do not touch outside SetNextSibling!", see nsIFrameLists.rs)
    pub mPrevSibling: Option<nsIFrame>,
    // Gecko: nsContainerFrame::mFrames, the principal child list, inline in the frame as in Gecko
    // (the address stays the same while the frame arrays grow, see FrameStore, so nsIFrame::mFrames
    // can hand out a pointer to it). The sibling links of the children are in their own FrameData.
    pub mFrames: nsFrameList,
    pub mRect: nsRect,
    // Gecko: nsIFrame::mOverflow (see "Overflow areas" below)
    pub mOverflow: nsIFrame_Overflow,
    pub mWritingMode: WritingMode,
    pub mStyle: Option<ServoArc<ComputedValues>>,
    // Gecko: nsIFrame::mClass, which maps to the LayoutFrameType (Type()). Block unless set.
    pub mType: LayoutFrameType,
    // Gecko: nsIFrame::mState
    pub mState: nsFrameState,
    // Gecko's nsIFrame::mHasBSizeChange and mHasPaddingChange bit fields
    pub mHasBSizeChange: bool,
    pub mHasPaddingChange: bool,
    // Gecko: nsIFrame::mContent. The id of the content (see nsIContent::Id); 0 is no content.
    pub mContent: u32,
    // Gecko: Style()->GetPseudoType() is one of the block-wrapper anonymous boxes
    // (MozBlockInsideInlineWrapper, MozCellContent, MozColumnSpanWrapper); the servo configuration of
    // Stylo has no such pseudo types, so the frame constructor says so.
    pub mIsBlockWrapper: bool,
    // Gecko: nsSplittableFrame::mPrevContinuation and mNextContinuation. A link is an in-flow (fluid)
    // when the later frame has NS_FRAME_IS_FLUID_CONTINUATION. (mFirstContinuation and mFirstInFlow,
    // Gecko's caches of the walk back, are not kept: the walk is short and a cache is 8 bytes a frame.)
    pub mPrevContinuation: Option<nsIFrame>,
    pub mNextContinuation: Option<nsIFrame>,
    // Gecko: nsInlineFrame::mBaseline (initially NS_INTRINSIC_ISIZE_UNKNOWN), inline in the frame
    // as in Gecko; nsLineLayout keeps a pointer to it for the whole line (BeginSpan), which stays
    // valid because the frames do not move (see FrameStore). Only inline frames use it.
    pub mBaseline: nscoord,
    // Gecko: nsIFrame::mProperties (FrameProperties, layout/base/FrameProperties.rs): a sparse list,
    // 8 bytes while the frame has no property.
    mProperties: FrameProperties,
}

// ADAPTER: the frames of a thread, in chunks of kFrameChunkSize. A frame never moves while the tree
// grows (a chunk is allocated at full capacity and never reallocated), as a frame object in Gecko's
// arena does not, so a reference into a FrameData (nsIFrame::mFrames, nsInlineFrame::mBaseline)
// stays valid when more frames are made. A single Vec would also keep up to half of its capacity
// unused after the doubling; here the unused part is at most the end of the last chunk, which
// `shrink_to_fit` gives back when layout is done.
#[derive(Default)]
pub struct FrameStore {
    mChunks: Vec<Vec<FrameData>>,
    mLen: usize,
}

const kFrameChunkShift: usize = 8;
const kFrameChunkSize: usize = 1 << kFrameChunkShift;

impl FrameStore {
    pub fn len(&self) -> usize {
        self.mLen
    }
    pub fn capacity(&self) -> usize {
        self.mChunks.len() << kFrameChunkShift
    }
    pub fn push(&mut self, aData: FrameData) {
        if self.mChunks.last().map_or(true, |c| c.len() == kFrameChunkSize) {
            self.mChunks.push(Vec::with_capacity(kFrameChunkSize));
        } else if let Some(last) = self.mChunks.last_mut() {
            // ShrinkFrameStorage cut the last chunk to its length: give it its capacity back now, so
            // that the pushes after this one never move a frame (a second layout on the same thread
            // gets here; the frames of the first one are done, nothing points into them)
            if last.capacity() < kFrameChunkSize {
                last.reserve_exact(kFrameChunkSize - last.len());
            }
        }
        self.mChunks.last_mut().unwrap().push(aData);
        self.mLen += 1;
    }
    pub fn iter(&self) -> impl Iterator<Item = &FrameData> {
        self.mChunks.iter().flatten()
    }
    // ADAPTER (measurement only): each frame rebuilt field by field at its own offset, padding zeroed,
    // as the frames would be saved for compression (DESIGN, "Idea: keep the layout result"). `aIds`:
    // the two pointers (mStyle, mProperties) become numbers in order of first use (0 for none);
    // `aRelative`: the links to other frames (parent, siblings, continuations, first and last child)
    // become the difference from this frame's index. Field values are copied, not their meaning
    // checked: a field's bytes are what the type holds.
    pub fn field_bytes(&self, aIds: bool, aRelative: bool) -> Vec<u8> {
        use std::collections::HashMap;
        use std::mem::{offset_of, size_of, size_of_val};
        let size = size_of::<FrameData>();
        // where the u32 sits inside an Option<nsIFrame> (the rest is the discriminant)
        let probe: Option<nsIFrame> = Some(nsIFrame(0xA5A5_5A5A));
        let pb: [u8; 8] = unsafe { std::mem::transmute(probe) };
        let valueAt = (0..=4).step_by(4).find(|&o| pb[o..o + 4] == 0xA5A5_5A5Au32.to_le_bytes()).unwrap();
        let mut styles: HashMap<usize, u64> = HashMap::new();
        let mut props: HashMap<usize, u64> = HashMap::new();
        let mut out = vec![0u8; self.mLen * size];
        for (i, f) in self.iter().enumerate() {
            let dst = &mut out[i * size..(i + 1) * size];
            let src = unsafe { std::slice::from_raw_parts(f as *const FrameData as *const u8, size) };
            macro_rules! copy {
                ($field:ident) => {{
                    let o = offset_of!(FrameData, $field);
                    let n = size_of_val(&f.$field);
                    dst[o..o + n].copy_from_slice(&src[o..o + n]);
                }};
            }
            copy!(mParent); copy!(mNextSibling); copy!(mPrevSibling); copy!(mFrames); copy!(mRect);
            copy!(mOverflow); copy!(mWritingMode); copy!(mStyle); copy!(mType); copy!(mState);
            copy!(mHasBSizeChange); copy!(mHasPaddingChange); copy!(mContent); copy!(mIsBlockWrapper);
            copy!(mPrevContinuation); copy!(mNextContinuation); copy!(mBaseline); copy!(mProperties);
            if aIds {
                let o = offset_of!(FrameData, mStyle);
                let ptr = usize::from_le_bytes(src[o..o + 8].try_into().unwrap());
                let n = styles.len() as u64 + 1;
                let id = if ptr == 0 { 0 } else { *styles.entry(ptr).or_insert(n) };
                dst[o..o + 8].copy_from_slice(&id.to_le_bytes());
                let o = offset_of!(FrameData, mProperties);
                let ptr = usize::from_le_bytes(src[o..o + 8].try_into().unwrap());
                let n = props.len() as u64 + 1;
                let id = if ptr == 0 { 0 } else { *props.entry(ptr).or_insert(n) };
                dst[o..o + 8].copy_from_slice(&id.to_le_bytes());
            }
            if aRelative {
                let mut rel = |o: usize, link: Option<nsIFrame>| {
                    if let Some(l) = link {
                        let d = (l.0 as i64 - i as i64) as i32;
                        dst[o + valueAt..o + valueAt + 4].copy_from_slice(&d.to_le_bytes());
                    }
                };
                rel(offset_of!(FrameData, mParent), f.mParent);
                rel(offset_of!(FrameData, mNextSibling), f.mNextSibling);
                rel(offset_of!(FrameData, mPrevSibling), f.mPrevSibling);
                rel(offset_of!(FrameData, mPrevContinuation), f.mPrevContinuation);
                rel(offset_of!(FrameData, mNextContinuation), f.mNextContinuation);
                // nsFrameList is two Option<nsIFrame>; which is first is found from the values
                let o = offset_of!(FrameData, mFrames);
                let at = |k: usize| -> Option<u32> {
                    let b = &src[o + k * 8..o + k * 8 + 8];
                    let v = u32::from_le_bytes(b[valueAt..valueAt + 4].try_into().unwrap());
                    Some(v)
                };
                let (first, last) = (f.mFrames.FirstChild(), f.mFrames.LastChild());
                let firstSlot = if first.map(|x| x.0) == at(0) { 0 } else { 1 };
                rel(o + firstSlot * 8, first);
                rel(o + (1 - firstSlot) * 8, last);
            }
        }
        out
    }

    // ADAPTER (measurement only): each FrameData field's name, offset and size, for the field_bytes files
    pub fn field_layout() -> Vec<(&'static str, usize, usize)> {
        use std::mem::{offset_of, size_of};
        macro_rules! f {
            ($field:ident, $t:ty) => {
                (stringify!($field), offset_of!(FrameData, $field), size_of::<$t>())
            };
        }
        vec![
            f!(mParent, Option<nsIFrame>), f!(mNextSibling, Option<nsIFrame>), f!(mPrevSibling, Option<nsIFrame>),
            f!(mFrames, nsFrameList), f!(mRect, nsRect), f!(mOverflow, nsIFrame_Overflow),
            f!(mWritingMode, WritingMode), f!(mStyle, Option<ServoArc<ComputedValues>>), f!(mType, LayoutFrameType),
            f!(mState, nsFrameState), f!(mHasBSizeChange, bool), f!(mHasPaddingChange, bool), f!(mContent, u32),
            f!(mIsBlockWrapper, bool), f!(mPrevContinuation, Option<nsIFrame>), f!(mNextContinuation, Option<nsIFrame>),
            f!(mBaseline, nscoord), f!(mProperties, FrameProperties),
        ]
    }

    // ADAPTER (measurement only): the frames' bytes as they sit in memory, chunk after chunk, to see
    // how well the frame storage compresses as it is (layoutcount, MRT_DUMP_FRAME_BYTES). Padding
    // bytes are copied too; their values are whatever the allocator left.
    pub fn raw_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.mLen * std::mem::size_of::<FrameData>());
        for chunk in &self.mChunks {
            let len = chunk.len() * std::mem::size_of::<FrameData>();
            // SAFETY: reads `len` bytes of the chunk's initialized frames as plain bytes
            out.extend_from_slice(unsafe { std::slice::from_raw_parts(chunk.as_ptr() as *const u8, len) });
        }
        out
    }
    // ADAPTER: gives back the unused end of the last chunk and of the list of chunks. This moves the
    // frames of the last chunk, so no reference into a FrameData may be held (layout is done).
    pub fn shrink_to_fit(&mut self) {
        if let Some(last) = self.mChunks.last_mut() {
            last.shrink_to_fit();
        }
        self.mChunks.shrink_to_fit();
    }
}

impl std::ops::Index<usize> for FrameStore {
    type Output = FrameData;
    fn index(&self, aIndex: usize) -> &FrameData {
        &self.mChunks[aIndex >> kFrameChunkShift][aIndex & (kFrameChunkSize - 1)]
    }
}

impl std::ops::IndexMut<usize> for FrameStore {
    fn index_mut(&mut self, aIndex: usize) -> &mut FrameData {
        &mut self.mChunks[aIndex >> kFrameChunkShift][aIndex & (kFrameChunkSize - 1)]
    }
}

impl<'a> IntoIterator for &'a FrameStore {
    type Item = &'a FrameData;
    type IntoIter = std::iter::Flatten<std::slice::Iter<'a, Vec<FrameData>>>;
    fn into_iter(self) -> Self::IntoIter {
        self.mChunks.iter().flatten()
    }
}

// ADAPTER: all frames of the page.
#[derive(Default)]
pub struct FrameTree {
    pub mFrames: FrameStore,
    // The slots of destroyed frames, given to new frames first (Gecko's pres arena reuses the memory
    // of a freed frame the same way). A slot gets here from mDestroyedFrames once the change that
    // destroyed it is done (ReleaseDestroyedFrames in driver.rs), so that a frame id is never reused
    // while code of that change may still hold it.
    mFreeList: Vec<u32>,
    // Frames whose Destroy ran since the last ReleaseDestroyedFrames.
    mDestroyedFrames: Vec<u32>,
}

thread_local! {
    static FRAME_TREE: RefCell<FrameTree> = RefCell::new(FrameTree::default());
}

// ADAPTER: drops every frame of this thread (stage 4; see DestroyFrameTree in driver.rs).
pub fn ResetFrameTree() {
    FRAME_TREE.with(|t| *t.borrow_mut() = FrameTree::default());
}

// ADAPTER: runs `f` on the frame tree of this thread.
pub fn WithFrameTree<R>(f: impl FnOnce(&mut FrameTree) -> R) -> R {
    FRAME_TREE.with(|t| f(&mut t.borrow_mut()))
}

impl FrameTree {
    // ADAPTER: creates a frame; `aParent` gets it as its last child.
    pub fn AddFrame(&mut self, aParent: Option<nsIFrame>, aRect: nsRect, aWritingMode: WritingMode, aStyle: Option<ServoArc<ComputedValues>>) -> nsIFrame {
        // A new frame is dirty and has not been reflowed yet, as in Gecko (nsIFrame::Init).
        let data = FrameData {
            mParent: aParent,
            mNextSibling: None,
            mPrevSibling: None,
            mFrames: nsFrameList::default(),
            mRect: aRect,
            mOverflow: nsIFrame_Overflow::default(),
            mWritingMode: aWritingMode,
            mStyle: aStyle,
            mType: LayoutFrameType::Block,
            mState: NS_FRAME_FIRST_REFLOW | NS_FRAME_IS_DIRTY,
            mHasBSizeChange: false,
            mHasPaddingChange: false,
            mContent: 0,
            mIsBlockWrapper: false,
            mPrevContinuation: None,
            mNextContinuation: None,
            mBaseline: NS_INTRINSIC_ISIZE_UNKNOWN,
            mProperties: FrameProperties::default(),
        };
        let frame = match self.mFreeList.pop() {
            Some(slot) => {
                self.mFrames[slot as usize] = data;
                nsIFrame(slot)
            }
            None => {
                self.mFrames.push(data);
                nsIFrame(self.mFrames.len() as u32 - 1)
            }
        };
        if let Some(parent) = aParent {
            // The new frame is the last child of the parent's principal child list.
            let list = &mut self.mFrames[parent.0 as usize].mFrames;
            match list.LastChild() {
                None => *list = nsFrameList::new(Some(frame), Some(frame)),
                Some(l) => {
                    let first = list.FirstChild();
                    *list = nsFrameList::new(first, Some(frame));
                    self.mFrames[l.0 as usize].mNextSibling = Some(frame);
                    self.mFrames[frame.0 as usize].mPrevSibling = Some(l);
                }
            }
        }
        frame
    }

    // ADAPTER: notes that aFrame's Destroy ran (nsIFrame::Destroy, see mDestroyedFrames).
    pub fn NoteDestroyed(&mut self, aFrame: nsIFrame) {
        self.mDestroyedFrames.push(aFrame.0);
    }

    // ADAPTER: the frames destroyed since the last call; their slots are not reused until
    // FreeSlots gets them.
    pub fn TakeDestroyedFrames(&mut self) -> Vec<nsIFrame> {
        std::mem::take(&mut self.mDestroyedFrames).into_iter().map(nsIFrame).collect()
    }

    // ADAPTER: gives the slots of destroyed frames back (what PresShell::FreeFrame does with the
    // memory): the style and the links are dropped, the slot goes on the free list.
    pub fn FreeSlots(&mut self, aFrames: &[nsIFrame]) {
        for f in aFrames {
            let d = &mut self.mFrames[f.0 as usize];
            d.mStyle = None;
            d.mParent = None;
            d.mNextSibling = None;
            d.mPrevSibling = None;
            d.mFrames = nsFrameList::default();
            d.mPrevContinuation = None;
            d.mNextContinuation = None;
            d.mContent = 0;
            d.mOverflow = nsIFrame_Overflow::default();
            d.mState = 0;
            self.mFreeList.push(f.0);
        }
    }

    // ADAPTER: the number of frames alive (made and not freed).
    pub fn LiveFrameCount(&self) -> usize {
        self.mFrames.len() - self.mFreeList.len() - self.mDestroyedFrames.len()
    }
}

// Gecko: layout/generic/nsIFrame.h nsIFrame
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct nsIFrame(pub u32);

// ADAPTER: Gecko's FramePropertyDescriptor<T>; the id stands for the address of the static
// descriptor.
pub struct FramePropertyDescriptor<T> {
    pub mId: u32,
    mMarker: std::marker::PhantomData<T>,
}

impl<T> FramePropertyDescriptor<T> {
    pub const fn new(aId: u32) -> FramePropertyDescriptor<T> {
        FramePropertyDescriptor { mId: aId, mMarker: std::marker::PhantomData }
    }
}

// Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_DELETABLE
// ADAPTER: `NS_DECLARE_FRAME_PROPERTY_DELETABLE!(FloatRegionProperty, nsMargin, 1)` defines
// `FloatRegionProperty()` returning the descriptor. The last argument is a number unique per
// property (Gecko uses the descriptor's address).
#[macro_export]
macro_rules! NS_DECLARE_FRAME_PROPERTY_DELETABLE {
    ($name:ident, $type:ty, $id:expr) => {
        fn $name() -> $crate::FramePropertyDescriptor<$type> {
            $crate::FramePropertyDescriptor::new($id)
        }
    };
}

impl nsIFrame {
    // ADAPTER: Gecko's frame data is reached through `this`; here through the thread's frame tree.
    fn with<R>(&self, f: impl FnOnce(&FrameData) -> R) -> R {
        FRAME_TREE.with(|t| f(&t.borrow().mFrames[self.0 as usize]))
    }

    fn with_mut<R>(&self, f: impl FnOnce(&mut FrameData) -> R) -> R {
        FRAME_TREE.with(|t| f(&mut t.borrow_mut().mFrames[self.0 as usize]))
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetParent
    pub fn GetParent(&self) -> Option<nsIFrame> {
        self.with(|d| d.mParent)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetNextSibling
    pub fn GetNextSibling(&self) -> Option<nsIFrame> {
        self.with(|d| d.mNextSibling)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetRect
    pub fn GetRect(&self) -> nsRect {
        self.with(|d| d.mRect)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetPosition
    pub fn GetPosition(&self) -> nsPoint {
        let r = self.GetRect();
        nsPoint::new(r.x, r.y)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetSize
    pub fn GetSize(&self) -> nsSize {
        let r = self.GetRect();
        nsSize::new(r.width, r.height)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetWritingMode
    pub fn GetWritingMode(&self) -> WritingMode {
        self.with(|d| d.mWritingMode)
    }

    // Gecko: layout/generic/nsIFrameInlines.h nsIFrame::GetNormalPosition
    // The position without a relative-position offset. Relative positioning is not supported
    // (it does not change page height), so there is never an offset.
    pub fn GetNormalPosition(&self) -> nsPoint {
        self.GetPosition()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::StyleDisplay
    pub fn StyleDisplay(&self) -> nsStyleDisplay {
        nsStyleDisplay::new(self.with(|d| d.mStyle.clone()).expect("frame without a style"))
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetProperty
    // ADAPTER: returns a copy of the value (Gecko returns a pointer into the property list).
    pub fn GetProperty<T: Clone + 'static>(&self, aProperty: FramePropertyDescriptor<T>) -> Option<T> {
        self.with(|d| d.mProperties.Get::<T>(aProperty.mId))
    }

    // ADAPTER: SetProperty for a value kept as a raw pointer (`aValue` is `Box::<T>::into_raw` as a
    // usize): the frame owns it from now on, and destroying the frame frees it, as Gecko's deletable
    // descriptors do (see FrameProperties.rs).
    pub fn SetOwnedPointerProperty<T: 'static>(&self, aProperty: FramePropertyDescriptor<usize>, aValue: usize) {
        crate::FrameProperties_rs::RegisterOwnedPointer::<T>(aProperty.mId);
        self.SetProperty(aProperty, aValue);
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::RemoveAllProperties
    pub fn RemoveAllProperties(&self) {
        // (the entries are taken out first: a destructor may read the frame tree)
        let mut props = self.with_mut(|d| std::mem::take(&mut d.mProperties));
        props.RemoveAll();
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::RemoveProperty
    pub fn RemoveProperty<T: 'static>(&self, aProperty: FramePropertyDescriptor<T>) {
        self.with_mut(|d| {
            d.mProperties.Remove(aProperty.mId);
        })
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetOrCreateDeletableProperty
    // ADAPTER: Gecko returns a pointer to the (possibly new) stored object and the caller assigns
    // through it; here the caller gives the value to store.
    pub fn SetProperty<T: 'static>(&self, aProperty: FramePropertyDescriptor<T>, aValue: T) {
        self.with_mut(|d| {
            d.mProperties.Set(aProperty.mId, aValue);
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Overflow areas: how a frame stores them (layout/generic/nsIFrame.h `mOverflow`, nsIFrame.cpp
// GetOverflowRect, GetOverflowAreas, ClearOverflowRects, SetOverflowAreas). The areas are not kept
// for every frame: a frame whose overflow areas are both its own border box stores nothing, one whose
// ink overflow is only slightly larger than its rect stores four bytes in the frame, and only the
// rest keep an OverflowAreas in a frame property. The same three kinds as Gecko.
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsIFrame.h nsIFrame::InkOverflowDeltas
// When there is no scrollable overflow area, and the ink overflow area only slightly larger than
// mRect, the ink overflow area may be stored a set of four 1-byte deltas from the edges of mRect
// rather than allocating a whole separate rectangle property. If all four deltas are zero, this means
// that no overflow area has actually been set (this is the initial state of newly-created frames).
//
// Note that these are unsigned values, all measured "outwards" from the edges of mRect, so mLeft and
// mTop are reversed from our normal coordinate system.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct nsIFrame_InkOverflowDeltas {
    pub mLeft: u8,
    pub mTop: u8,
    pub mRight: u8,
    pub mBottom: u8,
}

impl nsIFrame_InkOverflowDeltas {
    // The maximum delta value we can store in any of the four edges.
    pub const kMax: u8 = 0xfe;
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::OverflowStorageType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum nsIFrame_OverflowStorageType {
    // No overflow area; code relies on this being an all-zero value.
    None = 0x00000000,
    // Ink overflow is too large to stored in InkOverflowDeltas.
    Large = 0x000000ff,
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::mOverflow
// If mOverflow.mType is OverflowStorageType::Large, then the delta values are not meaningful and the
// overflow area is stored in OverflowAreasProperty() instead.
// ADAPTER: Gecko's `union { OverflowStorageType mType; InkOverflowDeltas mInkOverflowDeltas; }`, kept
// as the 32 bits both views share (mLeft is the lowest byte, so Large is mLeft = 0xff).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct nsIFrame_Overflow(u32);

impl nsIFrame_Overflow {
    // mOverflow.mType == OverflowStorageType::None
    pub fn IsNone(&self) -> bool {
        self.0 == nsIFrame_OverflowStorageType::None as u32
    }
    // mOverflow.mType == OverflowStorageType::Large
    pub fn IsLarge(&self) -> bool {
        self.0 == nsIFrame_OverflowStorageType::Large as u32
    }
    pub fn SetType(&mut self, aType: nsIFrame_OverflowStorageType) {
        self.0 = aType as u32;
    }
    // mOverflow.mInkOverflowDeltas
    pub fn InkOverflowDeltas(&self) -> nsIFrame_InkOverflowDeltas {
        let b = self.0.to_le_bytes();
        nsIFrame_InkOverflowDeltas { mLeft: b[0], mTop: b[1], mRight: b[2], mBottom: b[3] }
    }
    pub fn SetInkOverflowDeltas(&mut self, aDeltas: nsIFrame_InkOverflowDeltas) {
        self.0 = u32::from_le_bytes([aDeltas.mLeft, aDeltas.mTop, aDeltas.mRight, aDeltas.mBottom]);
    }
}

const kOverflowAreasPropertyId: u32 = 120;
const kIntrinsicISizesPropertyId: u32 = 127;

impl nsIFrame {
    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_DELETABLE(OverflowAreasProperty, mozilla::OverflowAreas)
    pub fn OverflowAreasProperty() -> FramePropertyDescriptor<OverflowAreas> {
        FramePropertyDescriptor::new(kOverflowAreasPropertyId)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetOverflowAreasProperty
    // Get the overflow areas property attached to the frame (ADAPTER: a copy).
    fn GetOverflowAreasProperty(&self) -> OverflowAreas {
        MOZ_ASSERT!(self.with(|d| d.mOverflow.IsLarge()));
        let overflow = self.GetProperty(nsIFrame::OverflowAreasProperty());
        MOZ_ASSERT!(overflow.is_some());
        overflow.unwrap()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::InkOverflowFromDeltas
    fn InkOverflowFromDeltas(&self) -> nsRect {
        self.with(|d| d.InkOverflowFromDeltas())
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::HasOverflowAreas
    // Returns whether the frame has an overflow rect that is different from its border-box.
    pub fn HasOverflowAreas(&self) -> bool {
        self.with(|d| !d.mOverflow.IsNone())
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetOverflowRect
    fn GetOverflowRect(&self, aType: OverflowType) -> nsRect {
        // Note that in some cases the overflow area might not have been
        // updated (yet) to reflect any outline set on the frame or the area
        // of child frames. That's OK because any reflow that updates these
        // areas will invalidate the appropriate area, so any (mis)uses of
        // this method will be fixed up.

        let overflow = self.with(|d| d.mOverflow);
        if overflow.IsLarge() {
            // there is an overflow rect, and it's not stored as deltas but as
            // a separately-allocated rect
            return *self.GetOverflowAreasProperty().Overflow(aType);
        }

        if aType == OverflowType::Ink && !overflow.IsNone() {
            return self.InkOverflowFromDeltas();
        }

        self.GetRectRelativeToSelf()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::InkOverflowRect
    pub fn InkOverflowRect(&self) -> nsRect {
        self.GetOverflowRect(OverflowType::Ink)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::ScrollableOverflowRect
    pub fn ScrollableOverflowRect(&self) -> nsRect {
        self.GetOverflowRect(OverflowType::Scrollable)
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetOverflowAreas
    pub fn GetOverflowAreas(&self) -> OverflowAreas {
        if self.with(|d| d.mOverflow.IsLarge()) {
            // there is an overflow rect, and it's not stored as deltas but as
            // a separately-allocated rect
            return self.GetOverflowAreasProperty();
        }

        OverflowAreas::new(self.InkOverflowFromDeltas(), nsRect::new(0, 0, self.GetSize().width, self.GetSize().height))
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetOverflowAreasRelativeToParent
    pub fn GetOverflowAreasRelativeToParent(&self) -> OverflowAreas {
        self.GetOverflowAreas() + self.GetPosition()
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::ClearOverflowRects
    // Removes any stored overflow rects (visual and scrollable) from the frame. Returns true if the
    // overflow changed.
    pub fn ClearOverflowRects(&self) -> bool {
        let overflow = self.with(|d| d.mOverflow);
        if overflow.IsNone() {
            return false;
        }
        if overflow.IsLarge() {
            self.RemoveProperty(nsIFrame::OverflowAreasProperty());
        }
        self.with_mut(|d| d.mOverflow.SetType(nsIFrame_OverflowStorageType::None));
        true
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::SetOverflowAreas
    // Set the OverflowArea rect, storing it as deltas or a separate rect depending on its size in
    // relation to the primary frame rect.
    //
    // @return true if any overflow changed.
    pub fn SetOverflowAreas(&self, aOverflowAreas: &OverflowAreas) -> bool {
        if self.with(|d| d.mOverflow.IsLarge()) {
            let overflow = self.GetOverflowAreasProperty();
            let changed = overflow != *aOverflowAreas;
            // ADAPTER: Gecko assigns through the pointer to the property's value
            self.SetProperty(nsIFrame::OverflowAreasProperty(), *aOverflowAreas);

            // Don't bother with converting to the deltas form if we already
            // have a property.
            return changed;
        }

        let rect = self.GetRect();
        let vis = aOverflowAreas.InkOverflow();
        let l: u32 = vis.x.wrapping_neg() as u32; // left edge: positive delta is leftwards
        let t: u32 = vis.y.wrapping_neg() as u32; // top: positive is upwards
        let r: u32 = vis.XMost().wrapping_sub(rect.width) as u32; // right: positive is rightwards
        let b: u32 = vis.YMost().wrapping_sub(rect.height) as u32; // bottom: positive is downwards
        let kMax = nsIFrame_InkOverflowDeltas::kMax as u32;
        if aOverflowAreas.ScrollableOverflow().IsEqualEdges(nsRect::new(0, 0, self.GetSize().width, self.GetSize().height))
            && l <= kMax
            && t <= kMax
            && r <= kMax
            && b <= kMax
            // we have to check these against zero because we *never* want to
            // set a frame as having no overflow in this function.  This is
            // because FinishAndStoreOverflow calls this function prior to
            // SetRect based on whether the overflow areas match aNewSize.
            // In the case where the overflow areas exactly match mRect but
            // do not match aNewSize, we need to store overflow in a property
            // so that our eventual SetRect/SetSize will know that it has to
            // reset our overflow areas.
            && (l | t | r | b) != 0
        {
            let oldDeltas = self.with(|d| d.mOverflow.InkOverflowDeltas());
            // It's a "small" overflow area so we store the deltas for each edge
            // directly in the frame, rather than allocating a separate rect.
            // If they're all zero, that's fine; we're setting things to
            // no-overflow.
            let newDeltas = nsIFrame_InkOverflowDeltas { mLeft: l as u8, mTop: t as u8, mRight: r as u8, mBottom: b as u8 };
            self.with_mut(|d| d.mOverflow.SetInkOverflowDeltas(newDeltas));
            // There was no scrollable overflow before, and there isn't now.
            oldDeltas != newDeltas
        } else {
            let changed = !aOverflowAreas.ScrollableOverflow().IsEqualEdges(nsRect::new(0, 0, self.GetSize().width, self.GetSize().height)) || !aOverflowAreas.InkOverflow().IsEqualEdges(self.InkOverflowFromDeltas());

            // it's a large overflow area that we need to store as a property
            self.with_mut(|d| d.mOverflow.SetType(nsIFrame_OverflowStorageType::Large));
            self.SetProperty(nsIFrame::OverflowAreasProperty(), *aOverflowAreas);
            changed
        }
    }
}

impl FrameData {
    // Gecko: layout/generic/nsIFrame.h nsIFrame::InkOverflowFromDeltas
    fn InkOverflowFromDeltas(&self) -> nsRect {
        MOZ_ASSERT!(!self.mOverflow.IsLarge(), "should not be called when overflow is in a property");
        // Calculate the rect using deltas from the frame's border rect.
        // Note that the mOverflow.mInkOverflowDeltas fields are unsigned, but we
        // will often need to return negative values for the left and top, so take
        // care to cast away the unsigned-ness.
        let deltas = self.mOverflow.InkOverflowDeltas();
        nsRect::new(
            -(deltas.mLeft as i32),
            -(deltas.mTop as i32),
            self.mRect.width + deltas.mRight as i32 + deltas.mLeft as i32,
            self.mRect.height + deltas.mBottom as i32 + deltas.mTop as i32,
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Added for ReflowInput (layout/generic/ReflowInput.h). Same scheme as above: methods of
// nsIFrame with Gecko's names; what is virtual in C++ is a `match` on the frame type, with a
// comment. The bodies of nsIFrame::ComputeSize and the functions around it are translations of
// layout/generic/nsIFrame.cpp (with `// Gecko:` markers), because ReflowInput's InitConstraints
// calls them to compute a frame's size.
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsIFrame.h kFrameClassCount etc. are not needed; the frame type is stored.

// Gecko: layout/generic/nsIFrame.h nsIFrame::AspectRatioUsage
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum nsIFrame_AspectRatioUsage {
    #[default]
    None,
    ToComputeISize,
    ToComputeBSize,
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::SizeComputationResult
#[derive(Clone, Copy, Debug)]
pub struct nsIFrame_SizeComputationResult {
    pub mLogicalSize: LogicalSize,
    pub mAspectRatioUsage: nsIFrame_AspectRatioUsage,
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::ISizeComputationResult
#[derive(Clone, Copy, Debug, Default)]
pub struct nsIFrame_ISizeComputationResult {
    pub mISize: nscoord,
    pub mAspectRatioUsage: nsIFrame_AspectRatioUsage,
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::ExtremumLength
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsIFrame_ExtremumLength {
    MinContent,
    MaxContent,
    MozAvailable,
    Stretch,
    FitContent,
    FitContentFunction,
}

// Type of preferred size/min size/max size.
// Gecko: layout/generic/nsIFrame.h nsIFrame::SizeProperty
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsIFrame_SizeProperty {
    Size,
    MinSize,
    MaxSize,
}

/**
 * Intrinsic size of a frame in a single axis.
 *
 * This can represent either isize or bsize.
 */
// Gecko: layout/generic/nsIFrame.h nsIFrame::IntrinsicSizeOffsetData
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct nsIFrame_IntrinsicSizeOffsetData {
    pub padding: nscoord,
    pub border: nscoord,
    pub margin: nscoord,
}

impl nsIFrame_IntrinsicSizeOffsetData {
    // Gecko: layout/generic/nsIFrame.h nsIFrame::IntrinsicSizeOffsetData::BorderPadding
    pub fn BorderPadding(&self) -> nscoord {
        self.border + self.padding
    }
    // Gecko: layout/generic/nsIFrame.h nsIFrame::IntrinsicSizeOffsetData::MarginBorderPadding
    pub fn MarginBorderPadding(&self) -> nscoord {
        self.margin + self.border + self.padding
    }
}

//----------------------------------------------------------------------
// Reflow status returned by the Reflow() methods.
// Gecko: layout/generic/nsIFrame.h nsReflowStatus
#[derive(Clone, Copy, Debug)]
pub struct nsReflowStatus {
    // private:
    mFloatClearType: UsedClear,
    mInlineBreak: nsReflowStatus_InlineBreak,
    mCompletion: nsReflowStatus_Completion,
    // (C++ bit fields, `bool x : 1`, are bools here)
    mNextInFlowNeedsReflow: bool,
    mFirstLetterComplete: bool,
}

// There are three possible completion statuses, represented by
// mCompletion.
//
// Incomplete means the frame does *not* map all its content, and the
// parent frame should create a continuing frame.
//
// OverflowIncomplete means that the frame has an overflow that is not
// complete, but its own box is complete. (This happens when the content
// overflows a fixed-height box.) The reflower should place and size the
// frame and continue its reflow, but it needs to create an overflow
// container as a continuation for this frame. See "Overflow containers"
// documentation in nsContainerFrame.h for more information.
//
// FullyComplete means the frame is neither Incomplete nor
// OverflowIncomplete. This is the default state for a nsReflowStatus.
//
// Gecko: layout/generic/nsIFrame.h nsReflowStatus::Completion
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum nsReflowStatus_Completion {
    // The order of the enum values is important, which represents the
    // precedence when merging.
    FullyComplete,
    OverflowIncomplete,
    Incomplete,
}

// There are three possible inline-break statuses, represented by
// mInlineBreak.
//
// "None" means no break is requested.
// "Before" means the break should occur before the frame.
// "After" means the break should occur after the frame.
// (Here, "the frame" is the frame whose reflow results are being reported by
// this nsReflowStatus.)
//
// Gecko: layout/generic/nsIFrame.h nsReflowStatus::InlineBreak
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum nsReflowStatus_InlineBreak {
    None,
    Before,
    After,
}

impl Default for nsReflowStatus {
    fn default() -> nsReflowStatus {
        nsReflowStatus::new()
    }
}

impl nsReflowStatus {
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::nsReflowStatus
    pub fn new() -> nsReflowStatus {
        nsReflowStatus {
            mFloatClearType: UsedClear::None,
            mInlineBreak: nsReflowStatus_InlineBreak::None,
            mCompletion: nsReflowStatus_Completion::FullyComplete,
            mNextInFlowNeedsReflow: false,
            mFirstLetterComplete: false,
        }
    }

    // Reset all the member variables.
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::Reset
    pub fn Reset(&mut self) {
        self.mFloatClearType = UsedClear::None;
        self.mInlineBreak = nsReflowStatus_InlineBreak::None;
        self.mCompletion = nsReflowStatus_Completion::FullyComplete;
        self.mNextInFlowNeedsReflow = false;
        self.mFirstLetterComplete = false;
    }

    // Return true if all member variables have their default values.
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        self.IsFullyComplete() && !self.IsInlineBreak() && !self.mNextInFlowNeedsReflow && !self.mFirstLetterComplete
    }

    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::IsIncomplete
    pub fn IsIncomplete(&self) -> bool {
        self.mCompletion == nsReflowStatus_Completion::Incomplete
    }
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::IsOverflowIncomplete
    pub fn IsOverflowIncomplete(&self) -> bool {
        self.mCompletion == nsReflowStatus_Completion::OverflowIncomplete
    }
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::IsFullyComplete
    pub fn IsFullyComplete(&self) -> bool {
        self.mCompletion == nsReflowStatus_Completion::FullyComplete
    }
    // Just for convenience; not a distinct state.
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::IsComplete
    pub fn IsComplete(&self) -> bool {
        !self.IsIncomplete()
    }

    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::SetIncomplete
    pub fn SetIncomplete(&mut self) {
        self.mCompletion = nsReflowStatus_Completion::Incomplete;
    }
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::SetOverflowIncomplete
    pub fn SetOverflowIncomplete(&mut self) {
        self.mCompletion = nsReflowStatus_Completion::OverflowIncomplete;
    }

    // mNextInFlowNeedsReflow bit flag means that the next-in-flow is dirty,
    // and also needs to be reflowed. This status only makes sense for a frame
    // that is not complete, i.e. you wouldn't set mNextInFlowNeedsReflow when
    // IsComplete() is true.
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::NextInFlowNeedsReflow
    pub fn NextInFlowNeedsReflow(&self) -> bool {
        self.mNextInFlowNeedsReflow
    }
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::SetNextInFlowNeedsReflow
    pub fn SetNextInFlowNeedsReflow(&mut self) {
        self.mNextInFlowNeedsReflow = true;
    }

    // Merge the frame completion status bits from aStatus into this.
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::MergeCompletionStatusFrom
    pub fn MergeCompletionStatusFrom(&mut self, aStatus: &nsReflowStatus) {
        if self.mCompletion < aStatus.mCompletion {
            self.mCompletion = aStatus.mCompletion;
        }

        // These asserts ensure that the mCompletion merging works as we expect.
        // (Incomplete beats OverflowIncomplete, which beats FullyComplete.)
        // (the static_assert is the derived Ord of nsReflowStatus_Completion, in declaration order)

        self.mNextInFlowNeedsReflow |= aStatus.mNextInFlowNeedsReflow;
    }

    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::IsInlineBreak
    pub fn IsInlineBreak(&self) -> bool {
        self.mInlineBreak != nsReflowStatus_InlineBreak::None
    }
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::IsInlineBreakBefore
    pub fn IsInlineBreakBefore(&self) -> bool {
        self.mInlineBreak == nsReflowStatus_InlineBreak::Before
    }
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::IsInlineBreakAfter
    pub fn IsInlineBreakAfter(&self) -> bool {
        self.mInlineBreak == nsReflowStatus_InlineBreak::After
    }
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::FloatClearType
    pub fn FloatClearType(&self) -> UsedClear {
        self.mFloatClearType
    }

    // Set the inline line-break-before status, and reset other bit flags. Note
    // that other frame completion status isn't expected to matter after calling
    // this method.
    //
    // Here's one scenario where a child frame would report this status. Suppose
    // the child has "break-inside:avoid" in its style, and the child (and its
    // content) won't fit in the available block-size. This child would want to
    // report this status so that it gets pushed (in its entirety) to the next
    // column/page where it will hopefully fit.
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::SetInlineLineBreakBeforeAndReset
    pub fn SetInlineLineBreakBeforeAndReset(&mut self) {
        self.Reset();
        self.mFloatClearType = UsedClear::None;
        self.mInlineBreak = nsReflowStatus_InlineBreak::Before;
    }

    // Set the inline line-break-after status. The clear type can be changed
    // via the optional aClearType param.
    // (the C++ default argument is `UsedClear::None`; callers without one pass it)
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::SetInlineLineBreakAfter
    pub fn SetInlineLineBreakAfter(&mut self, aClearType: UsedClear) {
        self.mFloatClearType = aClearType;
        self.mInlineBreak = nsReflowStatus_InlineBreak::After;
    }

    // mFirstLetterComplete bit flag means the break was induced by
    // completion of a first-letter.
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::FirstLetterComplete
    pub fn FirstLetterComplete(&self) -> bool {
        self.mFirstLetterComplete
    }
    // Gecko: layout/generic/nsIFrame.h nsReflowStatus::SetFirstLetterComplete
    pub fn SetFirstLetterComplete(&mut self) {
        self.mFirstLetterComplete = true;
    }
}
// NOT PORTED: operator<< (debug string of an nsReflowStatus).

/*
 * For replaced elements only. Gets the intrinsic dimensions of this element,
 * which can be specified on a per-axis basis.
 */
// Gecko: layout/generic/nsIFrame.h mozilla::IntrinsicSize
// (the width and height only; ToSize/operator== and friends are added when translated code calls them)
#[derive(Clone, Copy, Debug, Default)]
pub struct IntrinsicSize {
    pub width: Option<nscoord>,
    pub height: Option<nscoord>,
}

impl IntrinsicSize {
    // Gecko: layout/generic/nsIFrame.h IntrinsicSize::ToSize
    pub fn ToSize(&self) -> Option<nsSize> {
        match (self.width, self.height) {
            (Some(w), Some(h)) => Some(nsSize::new(w, h)),
            _ => None,
        }
    }
}

// A struct aggregates necessary data to compute the intrinsic sizes for a
// frame, typically the frame whose intrinsic size contribution is being
// requested. This struct is used as an input for GetMinISize(), GetPrefISize(),
// IntrinsicISize(), and others.
// Gecko: layout/generic/nsIFrame.h mozilla::IntrinsicSizeInput
#[derive(Clone, Copy, Debug)]
pub struct IntrinsicSizeInput {
    pub mContext: Option<gfxContext>,

    // The content-box size of a frame's containing block (in the frame's own
    // writing mode), used as a percentage basis for percentage-based sizes on the
    // frame itself that contribute to its intrinsic size. For example, in grid
    // layout, a percentage value of min-height be can transferred through the
    // aspect-ratio to determine auto repeat columns specified in
    // grid-template-columns.
    //
    // Note: it is acceptable for mContainingBlockSize to be Nothing() as long as
    // the frame doesn't have percentage-based value for properties that need to
    // be resolved in order to compute its intrinsic size.
    pub mContainingBlockSize: Option<LogicalSize>,

    // The content-box size of a frame (in the frame's own writing mode), served
    // as a percentage basis when computing the children's intrinsic
    // contributions. If the basis is indefinite in a given axis, use
    // NS_UNCONSTRAINEDSIZE for that component. If the value is Nothing, it is
    // semantically equivalent to NS_UNCONSTRAINEDSIZE in both axes.
    //
    // In most scenarios, this struct is used when computing the inline size
    // contribution, so the inline component of the percentage basis should be set
    // to NS_UNCONSTRAINEDSIZE.
    pub mPercentageBasisForChildren: Option<LogicalSize>,
}

impl IntrinsicSizeInput {
    // Gecko: layout/generic/nsIFrame.h IntrinsicSizeInput::HasSomePercentageBasisForChildren
    pub fn HasSomePercentageBasisForChildren(&self) -> bool {
        self.mPercentageBasisForChildren.map_or(false, |pb| !pb.IsAllValues(NS_UNCONSTRAINEDSIZE))
    }

    // Gecko: layout/generic/nsIFrame.h IntrinsicSizeInput::IntrinsicSizeInput(gfxContext*, ...)
    pub fn new(aContext: Option<gfxContext>, aContainingBlockSize: Option<LogicalSize>, aPercentageBasisForChildren: Option<LogicalSize>) -> IntrinsicSizeInput {
        MOZ_ASSERT!(aContext.is_some());
        IntrinsicSizeInput { mContext: aContext, mContainingBlockSize: aContainingBlockSize, mPercentageBasisForChildren: aPercentageBasisForChildren }
    }

    // Construct a new IntrinsicSizeInput for children by copying from
    // aParentInput.
    //
    // Note: since this constructor creates an IntrinsicSizeInput for the
    // children, it does not copy mContainingBlockSize from aParentInput.
    //
    // This constructor converts mPercentageBasisForChildren's writing mode, if it
    // exists. The original mPercentageBasis in aSource is expected to be in the
    // writing mode aFromWM, and it will be converted to the writing mode aToWM.
    // Gecko: layout/generic/nsIFrame.h IntrinsicSizeInput::IntrinsicSizeInput(const IntrinsicSizeInput&, WritingMode, WritingMode)
    pub fn new__IntrinsicSizeInput_WritingMode_WritingMode(aParentInput: &IntrinsicSizeInput, aToWM: WritingMode, aFromWM: WritingMode) -> IntrinsicSizeInput {
        IntrinsicSizeInput::new(aParentInput.mContext, None, aParentInput.mPercentageBasisForChildren.map(|aPB| aPB.ConvertTo(aToWM, aFromWM)))
    }
}

// Gecko: layout/style/nsStyleStruct.h mozilla::ContainSizeAxes
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ContainSizeAxes {
    pub mIContained: bool,
    pub mBContained: bool,
}

impl ContainSizeAxes {
    // Gecko: layout/generic/ContainSizeAxes.h ContainSizeAxes::ContainSizeAxes(bool, bool)
    pub fn new(aIContained: bool, aBContained: bool) -> ContainSizeAxes {
        ContainSizeAxes { mIContained: aIContained, mBContained: aBContained }
    }

    // Gecko: layout/style/nsStyleStruct.h ContainSizeAxes::IsBoth
    pub fn IsBoth(&self) -> bool {
        self.mIContained && self.mBContained
    }

    // Gecko: layout/style/nsStyleStruct.h ContainSizeAxes::IsAny
    pub fn IsAny(&self) -> bool {
        self.mIContained || self.mBContained
    }
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::GetContainingBlock flags
pub const SKIP_SCROLLED_FRAME: u32 = 0x01;

// ADAPTER: ids of the frame properties used below (Gecko uses the address of the static
// descriptor). 1 is FloatRegionProperty in nsFloatManager.rs.
const kNormalPositionPropertyId: u32 = 100;
const kComputedOffsetPropertyId: u32 = 101;
const kUsedMarginPropertyId: u32 = 102;
const kUsedPaddingPropertyId: u32 = 103;
const kIBaselinePadPropertyId: u32 = 104;
const kBBaselinePadPropertyId: u32 = 105;
const kFragStretchBSizePropertyId: u32 = 106;
const kBClampMarginBoxMinSizePropertyId: u32 = 107;
const kIBSplitSiblingPropertyId: u32 = 110;
const kIBSplitPrevSiblingPropertyId: u32 = 111;

// Pseudo bidi embedding level indicating nonexistence.
// Gecko: layout/generic/nsIFrame.h kBidiLevelNone
pub const kBidiLevelNone: BidiEmbeddingLevel = BidiEmbeddingLevel::new(0xff);

// Gecko: layout/generic/nsIFrame.h mozilla::FrameBidiData
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FrameBidiData {
    pub baseLevel: BidiEmbeddingLevel,
    pub embeddingLevel: BidiEmbeddingLevel,
    // The embedding level of virtual bidi formatting character before
    // this frame if any. kBidiLevelNone is used to indicate nonexistence
    // or unnecessity of such virtual character.
    pub precedingControl: BidiEmbeddingLevel,
}

const kBidiDataPropertyId: u32 = 130;

impl nsIFrame {
    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE(NormalPositionProperty, nsPoint)
    pub fn NormalPositionProperty() -> FramePropertyDescriptor<nsPoint> {
        FramePropertyDescriptor::new(kNormalPositionPropertyId)
    }
    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_DELETABLE(ComputedOffsetProperty, nsMargin)
    pub fn ComputedOffsetProperty() -> FramePropertyDescriptor<nsMargin> {
        FramePropertyDescriptor::new(kComputedOffsetPropertyId)
    }
    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_DELETABLE(UsedMarginProperty, nsMargin)
    pub fn UsedMarginProperty() -> FramePropertyDescriptor<nsMargin> {
        FramePropertyDescriptor::new(kUsedMarginPropertyId)
    }
    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_DELETABLE(UsedPaddingProperty, nsMargin)
    pub fn UsedPaddingProperty() -> FramePropertyDescriptor<nsMargin> {
        FramePropertyDescriptor::new(kUsedPaddingPropertyId)
    }
    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE(IBaselinePadProperty, nscoord)
    pub fn IBaselinePadProperty() -> FramePropertyDescriptor<nscoord> {
        FramePropertyDescriptor::new(kIBaselinePadPropertyId)
    }
    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE(BBaselinePadProperty, nscoord)
    pub fn BBaselinePadProperty() -> FramePropertyDescriptor<nscoord> {
        FramePropertyDescriptor::new(kBBaselinePadPropertyId)
    }

    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE(FragStretchBSizeProperty, nscoord)
    pub fn FragStretchBSizeProperty() -> FramePropertyDescriptor<nscoord> {
        FramePropertyDescriptor::new(kFragStretchBSizePropertyId)
    }
    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE(BClampMarginBoxMinSizeProperty, nscoord)
    pub fn BClampMarginBoxMinSizeProperty() -> FramePropertyDescriptor<nscoord> {
        FramePropertyDescriptor::new(kBClampMarginBoxMinSizePropertyId)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::HasProperty
    pub fn HasProperty<T: Clone + 'static>(&self, aProperty: FramePropertyDescriptor<T>) -> bool {
        self.with(|d| d.mProperties.Has(aProperty.mId))
    }

    // ADAPTER: Gecko's frame class is fixed when the C++ object is created; here the frame
    // constructor (or a test) sets the type.
    pub fn SetType(&self, aType: LayoutFrameType) {
        self.with_mut(|d| d.mType = aType)
    }

    // ADAPTER: gives the frame the content it was created for (see nsIContent).
    pub fn SetContent(&self, aContent: Option<nsIContent>) {
        self.with_mut(|d| d.mContent = aContent.map_or(0, |c| c.Id()))
    }

    // ADAPTER: says that this is one of the anonymous block wrappers (see FrameData::mIsBlockWrapper).
    pub fn SetIsBlockWrapper(&self, aIsBlockWrapper: bool) {
        self.with_mut(|d| d.mIsBlockWrapper = aIsBlockWrapper)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::SetRect
    // When we change the size of the frame's border-box rect, we may need to reset the overflow rect
    // if it was previously stored as deltas. (If it is currently a "large" overflow and could be
    // re-packed as deltas, we don't bother as the cost of the allocation has already been paid.)
    // (NOT PORTED: aRebuildDisplayItems)
    pub fn SetRect(&self, aRect: nsRect) {
        if aRect == self.GetRect() {
            return;
        }
        let overflow = self.with(|d| d.mOverflow);
        if !overflow.IsLarge() && !overflow.IsNone() {
            let overflow = self.GetOverflowAreas();
            self.with_mut(|d| d.mRect = aRect);
            self.SetOverflowAreas(&overflow);
        } else {
            self.with_mut(|d| d.mRect = aRect);
        }
    }

    // Gecko: layout/generic/nsSplittableFrame.cpp nsSplittableFrame::SetPrevContinuation
    // (a non-fluid link: the frame stops being a fluid continuation)
    pub fn SetPrevContinuation(&self, aFrame: Option<nsIFrame>) {
        NS_ASSERTION!(aFrame.map_or(true, |f| self.Type() == f.Type()), "setting a prev continuation with incorrect type!");
        self.with_mut(|d| d.mPrevContinuation = aFrame);
        self.RemoveStateBits(NS_FRAME_IS_FLUID_CONTINUATION);
    }

    // Gecko: layout/generic/nsSplittableFrame.cpp nsSplittableFrame::SetNextContinuation
    pub fn SetNextContinuation(&self, aFrame: Option<nsIFrame>) {
        NS_ASSERTION!(aFrame.map_or(true, |f| self.Type() == f.Type()), "setting a next continuation with incorrect type!");
        self.with_mut(|d| d.mNextContinuation = aFrame);
        if let Some(next) = aFrame {
            next.RemoveStateBits(NS_FRAME_IS_FLUID_CONTINUATION);
        }
    }

    // Gecko: layout/generic/nsSplittableFrame.cpp nsSplittableFrame::SetPrevInFlow
    pub fn SetPrevInFlow(&self, aFrame: Option<nsIFrame>) {
        NS_ASSERTION!(aFrame.map_or(true, |f| self.Type() == f.Type()), "setting a prev in flow with incorrect type!");
        self.with_mut(|d| d.mPrevContinuation = aFrame);
        self.AddStateBits(NS_FRAME_IS_FLUID_CONTINUATION);
    }

    // Gecko: layout/generic/nsSplittableFrame.cpp nsSplittableFrame::SetNextInFlow
    pub fn SetNextInFlow(&self, aFrame: Option<nsIFrame>) {
        NS_ASSERTION!(aFrame.map_or(true, |f| self.Type() == f.Type()), "setting a next in flow with incorrect type!");
        self.with_mut(|d| d.mNextContinuation = aFrame);
        if let Some(next) = aFrame {
            next.AddStateBits(NS_FRAME_IS_FLUID_CONTINUATION);
        }
    }

    // ADAPTER: what nsSplittableFrame::Init does with a previous in-flow, as one call (Gecko:
    // SetPrevInFlow(aPrevInFlow); aPrevInFlow->SetNextInFlow(this)); `self` is the earlier frame.
    pub fn HookNextInFlow(&self, aNextInFlow: nsIFrame) {
        aNextInFlow.SetPrevInFlow(Some(*self));
        self.SetNextInFlow(Some(aNextInFlow));
    }

    // Gecko: layout/generic/nsSplittableFrame.cpp nsSplittableFrame::RemoveFromFlow
    pub fn RemoveFromFlow(aFrame: nsIFrame) {
        let prevContinuation = aFrame.GetPrevContinuation();
        let nextContinuation = aFrame.GetNextContinuation();

        // The new continuation is fluid only if the continuation on both sides
        // of the removed frame was fluid
        if aFrame.GetPrevInFlow().is_some() && aFrame.GetNextInFlow().is_some() {
            if let Some(p) = prevContinuation {
                p.SetNextInFlow(nextContinuation);
            }
            if let Some(n) = nextContinuation {
                n.SetPrevInFlow(prevContinuation);
            }
        } else {
            if let Some(p) = prevContinuation {
                p.SetNextContinuation(nextContinuation);
            }
            if let Some(n) = nextContinuation {
                n.SetPrevContinuation(prevContinuation);
            }
        }

        // **Note: it is important here that we clear the Next link from aFrame
        // BEFORE clearing its Prev link, because in nsContinuingTextFrame,
        // SetPrevInFlow() would follow the Next pointers, wiping out the cached
        // mFirstContinuation field from each following frame in the list.
        aFrame.SetNextInFlow(None);
        aFrame.SetPrevInFlow(None);
    }

    // ADAPTER (TEMP): an override of the result of GetMinISize / GetPrefISize, kept as a frame property
    // (only the few frames that need it have one). Gecko computes them on demand through the virtual
    // IntrinsicISize (a `match` on the frame type, see nsIFrame::IntrinsicISize); this is the leaf
    // stand-in for frame classes that are not translated (images, form controls) and for tests.
    pub fn IntrinsicISizesProperty() -> FramePropertyDescriptor<(nscoord, nscoord)> {
        FramePropertyDescriptor::new(kIntrinsicISizesPropertyId)
    }

    // ADAPTER: stores the intrinsic inline-sizes (see IntrinsicISizesProperty).
    pub fn SetIntrinsicISizes(&self, aMinISize: nscoord, aPrefISize: nscoord) {
        self.SetProperty(nsIFrame::IntrinsicISizesProperty(), (aMinISize, aPrefISize))
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::Type
    pub fn Type(&self) -> LayoutFrameType {
        self.with(|d| d.mType)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetContent
    pub fn GetContent(&self) -> Option<nsIContent> {
        self.with(|d| nsIContent::FromId(d.mContent))
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::PresContext
    pub fn PresContext(&self) -> nsPresContext {
        nsPresContext::Current()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetStateBits, HasAnyStateBits, AddStateBits,
    // RemoveStateBits
    pub fn GetStateBits(&self) -> nsFrameState {
        self.with(|d| d.mState)
    }
    pub fn HasAnyStateBits(&self, aBitMask: nsFrameState) -> bool {
        self.with(|d| d.mState & aBitMask != 0)
    }
    pub fn AddStateBits(&self, aBits: nsFrameState) {
        self.with_mut(|d| d.mState |= aBits)
    }
    pub fn RemoveStateBits(&self, aBits: nsFrameState) {
        self.with_mut(|d| d.mState &= !aBits)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::HasBSizeChange, SetHasBSizeChange,
    // HasPaddingChange, SetHasPaddingChange
    pub fn HasBSizeChange(&self) -> bool {
        self.with(|d| d.mHasBSizeChange)
    }
    pub fn SetHasBSizeChange(&self, aHasBSizeChange: bool) {
        self.with_mut(|d| d.mHasBSizeChange = aHasBSizeChange)
    }
    pub fn HasPaddingChange(&self) -> bool {
        self.with(|d| d.mHasPaddingChange)
    }
    pub fn SetHasPaddingChange(&self, aHasPaddingChange: bool) {
        self.with_mut(|d| d.mHasPaddingChange = aHasPaddingChange)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::Style
    pub fn Style(&self) -> ServoArc<ComputedValues> {
        self.with(|d| d.mStyle.clone()).expect("frame without a style")
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::StylePosition, StyleMargin, StylePadding,
    // StyleBorder, StyleText, StyleList, StyleFont
    pub fn StylePosition(&self) -> nsStylePosition {
        nsStylePosition::new(self.Style())
    }
    pub fn StyleMargin(&self) -> nsStyleMargin {
        nsStyleMargin::new(self.Style())
    }
    pub fn StylePadding(&self) -> nsStylePadding {
        nsStylePadding::new(self.Style())
    }
    pub fn StyleBorder(&self) -> nsStyleBorder {
        nsStyleBorder::new(self.Style())
    }
    pub fn StyleText(&self) -> nsStyleText {
        nsStyleText::new(self.Style())
    }
    pub fn StyleList(&self) -> nsStyleList {
        nsStyleList::new(self.Style())
    }
    pub fn StyleFont(&self) -> nsStyleFont {
        nsStyleFont::new(self.Style())
    }

    // Gecko generates StyleDisplayWithOptionalParam with the style struct accessor macros
    // (layout/style/nsStyleStructList.h); it returns aDisplay when given, to skip the lookup. Here
    // the lookup is cheap.
    pub fn StyleDisplayWithOptionalParam(&self, aDisplay: Option<&nsStyleDisplay>) -> nsStyleDisplay {
        match aDisplay {
            Some(d) => d.clone(),
            None => self.StyleDisplay(),
        }
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsThemed
    // NOT PORTED: native theming (nsITheme); frames are never themed here.
    pub fn IsThemed(&self) -> bool {
        false
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsInSVGTextSubtree
    // NOT PORTED: SVG text; always false.
    pub fn IsInSVGTextSubtree(&self) -> bool {
        false
    }

    // Gecko: layout/generic/nsIFrame.h Is##name_##Frame() (generated from FrameTypeList.h), for
    // the types that translated layout code asks about.
    pub fn IsTableFrame(&self) -> bool {
        self.Type() == LayoutFrameType::Table
    }
    pub fn IsTableWrapperFrame(&self) -> bool {
        self.Type() == LayoutFrameType::TableWrapper
    }
    pub fn IsTableCellFrame(&self) -> bool {
        self.Type() == LayoutFrameType::TableCell
    }
    pub fn IsTableRowFrame(&self) -> bool {
        self.Type() == LayoutFrameType::TableRow
    }
    pub fn IsTableRowGroupFrame(&self) -> bool {
        self.Type() == LayoutFrameType::TableRowGroup
    }
    pub fn IsTableColFrame(&self) -> bool {
        self.Type() == LayoutFrameType::TableCol
    }
    pub fn IsTableColGroupFrame(&self) -> bool {
        self.Type() == LayoutFrameType::TableColGroup
    }
    pub fn IsPlaceholderFrame(&self) -> bool {
        self.Type() == LayoutFrameType::Placeholder
    }
    pub fn IsCanvasFrame(&self) -> bool {
        self.Type() == LayoutFrameType::Canvas
    }
    pub fn IsScrollContainerFrame(&self) -> bool {
        self.Type() == LayoutFrameType::ScrollContainer
    }
    pub fn IsFlexContainerFrame(&self) -> bool {
        self.Type() == LayoutFrameType::FlexContainer
    }
    pub fn IsGridContainerFrame(&self) -> bool {
        self.Type() == LayoutFrameType::GridContainer
    }
    pub fn IsPageContentFrame(&self) -> bool {
        self.Type() == LayoutFrameType::PageContent
    }
    pub fn IsSVGForeignObjectFrame(&self) -> bool {
        self.Type() == LayoutFrameType::SVGForeignObject
    }

    // Gecko: layout/generic/nsIFrame.h CLASS_FLAG_METHOD(IsLineParticipant, LineParticipant)
    // The flag is set per frame class in FrameClasses.py; the classes with it are Inline, Text, Br,
    // Line and the ruby frames. (nsFirstLetterFrame has it, nsFloatingFirstLetterFrame does not, and
    // both are of type Letter; a floating first-letter is not line participant, so this says no.)
    pub fn IsLineParticipant(&self) -> bool {
        matches!(
            self.Type(),
            LayoutFrameType::Inline
                | LayoutFrameType::Text
                | LayoutFrameType::Br
                | LayoutFrameType::Line
                | LayoutFrameType::RubyBaseContainer
                | LayoutFrameType::RubyBase
                | LayoutFrameType::Ruby
                | LayoutFrameType::RubyText
        )
    }

    // Gecko: layout/generic/nsIFrame.h CLASS_FLAG_METHOD(HasReplacedSizing, ReplacedSizing)
    pub fn HasReplacedSizing(&self) -> bool {
        matches!(
            self.Type(),
            LayoutFrameType::HTMLVideo
                | LayoutFrameType::SVGOuterSVG
                | LayoutFrameType::HTMLCanvas
                | LayoutFrameType::ImageControl
                | LayoutFrameType::Image
                | LayoutFrameType::SubDocument
        )
    }

    // Gecko: layout/generic/nsIFrame.h CLASS_FLAG_METHOD0(SupportsAspectRatio)
    // (the frame classes without the flag: the table parts, inline, letter and the ruby frames)
    pub fn SupportsAspectRatio(&self) -> bool {
        !matches!(
            self.Type(),
            LayoutFrameType::Inline
                | LayoutFrameType::Letter
                | LayoutFrameType::TableCell
                | LayoutFrameType::TableRow
                | LayoutFrameType::TableRowGroup
                | LayoutFrameType::TableCol
                | LayoutFrameType::TableColGroup
                | LayoutFrameType::RubyBaseContainer
                | LayoutFrameType::RubyBase
                | LayoutFrameType::Ruby
                | LayoutFrameType::RubyTextContainer
                | LayoutFrameType::RubyText
        )
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IsReplaced
    pub fn IsReplaced(&self) -> bool {
        // HasAnyClassFlag(ClassFlags::Replaced)
        if matches!(
            self.Type(),
            LayoutFrameType::HTMLVideo
                | LayoutFrameType::Text
                | LayoutFrameType::Br
                | LayoutFrameType::InputButtonControl
                | LayoutFrameType::SVGOuterSVG
                | LayoutFrameType::CheckboxRadio
                | LayoutFrameType::ColorControl
                | LayoutFrameType::ComboboxControl
                | LayoutFrameType::DateTimeControl
                | LayoutFrameType::FileControl
                | LayoutFrameType::HTMLCanvas
                | LayoutFrameType::ImageControl
                | LayoutFrameType::Image
                | LayoutFrameType::ListControl
                | LayoutFrameType::Progress
                | LayoutFrameType::Range
                | LayoutFrameType::SubDocument
                | LayoutFrameType::TextInput
        ) {
            return true;
        }
        // NOT PORTED: `!Style()->IsAnonBox()`; a frame whose content is a <button> is not an
        // anonymous box here, because anonymous boxes have no content.
        if self.GetContent().map_or(false, |c| c.IsHTMLElement(nsGkAtoms::button)) {
            // Button always behaves as a replaced element.
            return true;
        }
        false
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IsBlockWrapper
    pub fn IsBlockWrapper(&self) -> bool {
        self.with(|d| d.mIsBlockWrapper)
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IsBlockFrameOrSubclass
    // (the classes derived from nsBlockFrame have type Block too, except the column-set wrapper
    // and the MathML block, which have their own types or are not supported)
    pub fn IsBlockFrameOrSubclass(&self) -> bool {
        matches!(self.Type(), LayoutFrameType::Block | LayoutFrameType::ColumnSetWrapper)
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IsBlockContainer
    pub fn IsBlockContainer(&self) -> bool {
        // The block wrappers we use to wrap blocks inside inlines aren't
        // described in the CSS spec.  We need to make them not be containing
        // blocks.
        // Since the parent of such a block is either a normal block or
        // another such pseudo, this shouldn't cause anything bad to happen.
        // Also the anonymous blocks inside table cells are not containing blocks.
        //
        // If we ever start skipping table row groups from being containing blocks,
        // you need to remove the StickyScrollContainer hack referencing bug 1421660.
        // Table rows are not containing blocks either
        !self.IsLineParticipant() && !self.IsBlockWrapper() && !self.IsTableRowFrame()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetPrevInFlow, GetNextInFlow, FirstInFlow
    pub fn GetPrevInFlow(&self) -> Option<nsIFrame> {
        if self.HasAnyStateBits(NS_FRAME_IS_FLUID_CONTINUATION) { self.with(|d| d.mPrevContinuation) } else { None }
    }
    pub fn GetNextInFlow(&self) -> Option<nsIFrame> {
        self.with(|d| d.mNextContinuation).filter(|n| n.HasAnyStateBits(NS_FRAME_IS_FLUID_CONTINUATION))
    }
    pub fn FirstInFlow(&self) -> nsIFrame {
        let mut f = *self;
        while let Some(p) = f.GetPrevInFlow() {
            f = p;
        }
        f
    }

    // Gecko: layout/generic/nsIFrameInlines.h nsIFrame::IsFlexItem, IsGridItem,
    // IsFlexOrGridContainer, IsFlexOrGridItem
    pub fn IsFlexItem(&self) -> bool {
        self.GetParent().map_or(false, |p| p.IsFlexContainerFrame()) && !self.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW)
    }
    pub fn IsGridItem(&self) -> bool {
        self.GetParent().map_or(false, |p| p.IsGridContainerFrame()) && !self.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW)
    }
    pub fn IsFlexOrGridContainer(&self) -> bool {
        self.IsFlexContainerFrame() || self.IsGridContainerFrame()
    }
    pub fn IsFlexOrGridItem(&self) -> bool {
        !self.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) && self.GetParent().map_or(false, |p| p.IsFlexOrGridContainer())
    }
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetMarginRect
    pub fn GetMarginRect(&self) -> nsRect {
        self.GetMarginRectRelativeToSelf() + self.GetPosition()
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetMarginRectRelativeToSelf
    pub fn GetMarginRectRelativeToSelf(&self) -> nsRect {
        let mut m = self.GetUsedMargin();
        m.ApplySkipSides(self.GetSkipSides());
        let mut r = nsRect::new(0, 0, self.GetRect().width, self.GetRect().height);
        r.Inflate__nsMargin(m);
        r
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::ShouldBreakBefore, ShouldBreakAfter
    // NOT PORTED: forced page and column breaks (nsStyleDisplay::BreakBefore/BreakAfter are always
    // false here, see nsStyleStruct.rs), so there is never a break to honor.
    pub fn ShouldBreakBefore(&self, aBreakType: BreakType) -> bool {
        false
    }
    pub fn ShouldBreakAfter(&self, aBreakType: BreakType) -> bool {
        false
    }

    // Gecko: layout/generic/nsIFrameInlines.h nsIFrame::IsLegacyWebkitBox
    // (a state bit; NOT PORTED: the frame constructor never sets it, since -webkit-box needs nsStyleXUL)
    pub fn IsLegacyWebkitBox(&self) -> bool {
        MOZ_ASSERT!(self.IsFlexContainerFrame(), "The state-bit is meaningful when this is a nsFlexContainerFrame!");
        self.HasAnyStateBits(NS_STATE_FLEX_IS_EMULATING_LEGACY_WEBKIT_BOX)
    }

    // Gecko: layout/generic/nsIFrameInlines.h nsIFrame::IsRelativelyOrStickyPositioned,
    // IsRelativelyPositioned, IsStickyPositioned
    pub fn IsRelativelyOrStickyPositioned(&self) -> bool {
        self.StyleDisplay().IsRelativelyOrStickyPositioned(*self)
    }
    pub fn IsRelativelyPositioned(&self) -> bool {
        self.StyleDisplay().IsRelativelyPositioned(*self)
    }
    pub fn IsStickyPositioned(&self) -> bool {
        self.StyleDisplay().IsStickyPositioned(*self)
    }

    // Gecko: layout/generic/nsIFrameInlines.h nsIFrame::IsAbsolutelyPositioned
    pub fn IsAbsolutelyPositioned(&self, aStyleDisplay: Option<&nsStyleDisplay>) -> bool {
        self.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) && self.StyleDisplayWithOptionalParam(aStyleDisplay).IsAbsolutelyPositioned(*self)
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetContainingBlock(uint32_t, const nsStyleDisplay*)
    pub fn GetContainingBlock__uint32_t_nsStyleDisplay(&self, aFlags: u32, aStyleDisplay: &nsStyleDisplay) -> Option<nsIFrame> {
        // Keep this in sync with MightBeContainingBlockFor in ReflowInput.cpp.

        self.GetParent()?;
        // MathML frames might have absolute positioning style, but they would
        // still be in-flow.  So we have to check to make sure that the frame
        // is really out-of-flow too.
        let mut f: Option<nsIFrame>;
        if self.IsAbsolutelyPositioned(Some(aStyleDisplay)) {
            f = self.GetParent(); // the parent is always the containing block
        } else {
            f = Some(GetNearestBlockContainer(self.GetParent().unwrap()));
        }

        // NOT PORTED: `aFlags & SKIP_SCROLLED_FRAME` with a ::-moz-scrolled-content frame (scroll
        // containers are not translated; there is no such pseudo type).
        f
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetContainingBlock(uint32_t aFlags = 0)
    // (the C++ call without an argument is `GetContainingBlock__uint32_t(0)` here)
    pub fn GetContainingBlock__uint32_t(&self, aFlags: u32) -> Option<nsIFrame> {
        self.GetContainingBlock__uint32_t_nsStyleDisplay(aFlags, &self.StyleDisplay())
    }
}

// Gecko: layout/generic/nsIFrame.cpp GetNearestBlockContainer
fn GetNearestBlockContainer(aFrame: nsIFrame) -> nsIFrame {
    let mut frame = aFrame;
    while !frame.IsBlockContainer() {
        match frame.GetParent() {
            Some(p) => frame = p,
            None => {
                NS_ASSERTION!(false, "How come we got to the root frame without seeing a containing block?");
                break;
            }
        }
    }
    frame
}

impl nsIFrame {
    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalSize(WritingMode)
    pub fn GetLogicalSize__WritingMode(&self, aWritingMode: WritingMode) -> LogicalSize {
        LogicalSize::new__WritingMode_nsSize(aWritingMode, self.GetSize())
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalSize()
    pub fn GetLogicalSize__(&self) -> LogicalSize {
        self.GetLogicalSize__WritingMode(self.GetWritingMode())
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::ISize(WritingMode)
    pub fn ISize(&self, aWritingMode: WritingMode) -> nscoord {
        self.GetLogicalSize__WritingMode(aWritingMode).ISize(aWritingMode)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::BSize(WritingMode)
    pub fn BSize(&self, aWritingMode: WritingMode) -> nscoord {
        self.GetLogicalSize__WritingMode(aWritingMode).BSize(aWritingMode)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::ContentSize(WritingMode)
    pub fn ContentSize(&self, aWritingMode: WritingMode) -> LogicalSize {
        self.SizeReducedBy(aWritingMode, self.GetLogicalUsedBorderAndPadding(self.GetWritingMode()))
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::SizeReducedBy
    // (GetLogicalSkipSides is in nsInlineFrame.rs)
    pub fn SizeReducedBy(&self, aWritingMode: WritingMode, mut aMargin: LogicalMargin) -> LogicalSize {
        let wm = self.GetWritingMode();
        // aMargin assumed to be in `wm`.
        let m = aMargin.ApplySkipSides(self.GetLogicalSkipSides()).ConvertTo(aWritingMode, wm);
        let size = self.GetLogicalSize__WritingMode(aWritingMode);
        LogicalSize::new(aWritingMode, std::cmp::max(0, size.ISize(aWritingMode) - m.IStartEnd(aWritingMode)), std::cmp::max(0, size.BSize(aWritingMode) - m.BStartEnd(aWritingMode)))
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetUsedMargin (virtual)
    // ADAPTER: the virtual call is a `match` on the frame type.
    pub fn GetUsedMargin(&self) -> nsMargin {
        match self.Type() {
            // Table => nsTableFrame::GetUsedMargin
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).GetUsedMargin(),
            // TableRowGroup, TableRow, TableCell => their GetUsedMargin (all zero)
            LayoutFrameType::TableRowGroup => nsTableRowGroupFrame::from_frame(*self).GetUsedMargin(),
            LayoutFrameType::TableRow => nsTableRowFrame::from_frame(*self).GetUsedMargin(),
            LayoutFrameType::TableCell => nsTableCellFrame::from_frame(*self).GetUsedMargin(),
            _ => self.nsIFrame_GetUsedMargin(),
        }
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetUsedMargin
    pub fn nsIFrame_GetUsedMargin(&self) -> nsMargin {
        let mut margin = nsMargin::default();
        let mState = self.GetStateBits();
        if ((mState & NS_FRAME_FIRST_REFLOW != 0) && (mState & NS_FRAME_IN_REFLOW == 0)) || self.IsInSVGTextSubtree() {
            return margin;
        }

        if let Some(m) = self.GetProperty(nsIFrame::UsedMarginProperty()) {
            margin = m;
        } else if !self.StyleMargin().GetMargin__nsMargin(&mut margin) {
            // If we get here, our caller probably shouldn't be calling us...
            NS_WARNING!("Returning bogus 0-sized margin, because this margin depends on layout & isn't cached!");
        }
        margin
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalUsedMargin
    pub fn GetLogicalUsedMargin(&self, aWritingMode: WritingMode) -> LogicalMargin {
        LogicalMargin::new__WritingMode_nsMargin(aWritingMode, self.GetUsedMargin())
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetUsedBorder (virtual)
    // ADAPTER: the virtual call is a `match` on the frame type.
    pub fn GetUsedBorder(&self) -> nsMargin {
        match self.Type() {
            // Table => nsTableFrame::GetUsedBorder
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).GetUsedBorder(),
            // TableRowGroup, TableRow => their GetUsedBorder (zero)
            LayoutFrameType::TableRowGroup => nsTableRowGroupFrame::from_frame(*self).GetUsedBorder(),
            LayoutFrameType::TableRow => nsTableRowFrame::from_frame(*self).GetUsedBorder(),
            _ => self.nsIFrame_GetUsedBorder(),
        }
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetUsedBorder
    // NOT PORTED: themed frames (IsThemed() is always false).
    pub fn nsIFrame_GetUsedBorder(&self) -> nsMargin {
        let mState = self.GetStateBits();
        if ((mState & NS_FRAME_FIRST_REFLOW != 0) && (mState & NS_FRAME_IN_REFLOW == 0)) || self.IsInSVGTextSubtree() {
            return nsMargin::default();
        }

        self.StyleBorder().GetComputedBorder()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalUsedBorder
    pub fn GetLogicalUsedBorder(&self, aWritingMode: WritingMode) -> LogicalMargin {
        LogicalMargin::new__WritingMode_nsMargin(aWritingMode, self.GetUsedBorder())
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetUsedPadding (virtual)
    // ADAPTER: the virtual call is a `match` on the frame type.
    pub fn GetUsedPadding(&self) -> nsMargin {
        match self.Type() {
            // Table => nsTableFrame::GetUsedPadding
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).GetUsedPadding(),
            // TableRowGroup, TableRow => their GetUsedPadding (zero)
            LayoutFrameType::TableRowGroup => nsTableRowGroupFrame::from_frame(*self).GetUsedPadding(),
            LayoutFrameType::TableRow => nsTableRowFrame::from_frame(*self).GetUsedPadding(),
            _ => self.nsIFrame_GetUsedPadding(),
        }
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetUsedPadding
    // NOT PORTED: themed frames (IsThemed() is always false).
    pub fn nsIFrame_GetUsedPadding(&self) -> nsMargin {
        let mut padding = nsMargin::default();
        let mState = self.GetStateBits();
        if ((mState & NS_FRAME_FIRST_REFLOW != 0) && (mState & NS_FRAME_IN_REFLOW == 0)) || self.IsInSVGTextSubtree() {
            return padding;
        }

        if let Some(p) = self.GetProperty(nsIFrame::UsedPaddingProperty()) {
            padding = p;
        } else if !self.StylePadding().GetPadding(&mut padding) {
            // If we get here, our caller probably shouldn't be calling us...
            NS_WARNING!("Returning bogus 0-sized padding, because this padding depends on layout & isn't cached!");
        }
        padding
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalUsedPadding
    pub fn GetLogicalUsedPadding(&self, aWritingMode: WritingMode) -> LogicalMargin {
        LogicalMargin::new__WritingMode_nsMargin(aWritingMode, self.GetUsedPadding())
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetUsedBorderAndPadding
    pub fn GetUsedBorderAndPadding(&self) -> nsMargin {
        self.GetUsedBorder() + self.GetUsedPadding()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalUsedBorderAndPadding
    pub fn GetLogicalUsedBorderAndPadding(&self, aWritingMode: WritingMode) -> LogicalMargin {
        LogicalMargin::new__WritingMode_nsMargin(aWritingMode, self.GetUsedBorderAndPadding())
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetContainSizeAxes, which calls
    // nsStyleDisplay::GetContainSizeAxes (layout/style/nsStyleStruct.cpp)
    // NOT PORTED: content-visibility (the "hides content" case), contain-intrinsic-size.
    pub fn GetContainSizeAxes(&self) -> ContainSizeAxes {
        use mrt_style::style::values::specified::box_::Contain;
        let contain = self.Style().get_box().contain;
        // Short circuit for no containment whatsoever
        if contain.is_empty() {
            return ContainSizeAxes::new(false, false);
        }

        // PrecludesSizeContainmentOrContentVisibilityWithFrame: size containment does not apply to
        // non-replaced inlines and internal table elements (other than cells) or tables.
        let disp = self.StyleDisplay();
        let isNonReplacedInline = self.IsLineParticipant() && !self.IsReplaced();
        if isNonReplacedInline || disp.DisplayInside() == StyleDisplayInside::Table || disp.IsInnerTableStyle() {
            return ContainSizeAxes::new(false, false);
        }

        ContainSizeAxes::new(contain.contains(Contain::INLINE_SIZE), contain.contains(Contain::BLOCK_SIZE))
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetIntrinsicRatio
    // NOT PORTED: the intrinsic ratio of images and canvas (they override this); every other frame
    // has none, but video's (nsVideoFrame::GetIntrinsicRatio).
    pub fn GetIntrinsicRatio(&self) -> AspectRatio {
        // Gecko: layout/svg/SVGOuterSVGFrame.cpp SVGOuterSVGFrame::GetIntrinsicRatio
        // ADAPTER: the leaf block of an <svg> element (no frame class, see SVGOuterSVGFrame.rs).
        if let Some(svgData) = self.GetProperty(SVGOuterSVGDataProperty()) {
            return SVGOuterSVGFrame_GetIntrinsicRatio(&svgData);
        }
        match self.Type() {
            LayoutFrameType::HTMLVideo => nsVideoFrame::from_frame(*self).GetIntrinsicRatio(),
            LayoutFrameType::SubDocument => nsSubDocumentFrame::from_frame(*self).GetIntrinsicRatio(),
            _ => AspectRatio::default(),
        }
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetAspectRatio
    pub fn GetAspectRatio(&self) -> AspectRatio {
        use mrt_style::style::values::generics::position::PreferredRatio;
        // Per spec, 'aspect-ratio' property applies to all elements except inline
        // boxes and internal ruby or table boxes.
        // https://drafts.csswg.org/css-sizing-4/#aspect-ratio
        // For those frame types that don't support aspect-ratio, they must not have
        // the natural ratio, so this early return is fine.
        if !self.SupportsAspectRatio() {
            return AspectRatio::default();
        }

        let ar = self.StylePosition().mAspectRatio();
        // StyleRatio<StyleNonNegativeNumber>::ToLayoutRatio(aUseBoxSizing)
        let ToLayoutRatio = |aUseBoxSizing: UseBoxSizing| -> AspectRatio {
            match &ar.ratio {
                PreferredRatio::Ratio(r) => {
                    // 0/1, 1/0, and 0/0 are all degenerate ratios (which behave as auto), and we
                    // always return 0.0f.
                    // https://drafts.csswg.org/css-values-4/#degenerate-ratio
                    AspectRatio::FromSize__float_float_UseBoxSizing(r.0.0, r.1.0, aUseBoxSizing)
                }
                PreferredRatio::None => AspectRatio::default(),
            }
        };
        let hasRatio = matches!(ar.ratio, PreferredRatio::Ratio(_));
        // If aspect-ratio is zero or infinite, it's a degenerate ratio and behaves
        // as auto.
        // https://drafts.csswg.org/css-sizing-4/#valdef-aspect-ratio-ratio
        // Non-auto. Return the preferred aspect ratio from the aspect-ratio style.
        if hasRatio && !ar.auto {
            let ratio = ToLayoutRatio(UseBoxSizing::Yes);
            if ratio.operator_bool() {
                return ratio;
            }
        }

        let intrinsicRatio = self.GetIntrinsicRatio();
        if intrinsicRatio.operator_bool() {
            return intrinsicRatio;
        }

        if hasRatio {
            // This returns a 0 ratio for degenerate rations, like the auto case below.
            return ToLayoutRatio(UseBoxSizing::No);
        }

        AspectRatio::default()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetMinISize
    pub fn GetMinISize(&self, aInput: &IntrinsicSizeInput) -> nscoord {
        self.IntrinsicISize(aInput, IntrinsicISizeType::MinISize)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetPrefISize
    pub fn GetPrefISize(&self, aInput: &IntrinsicSizeInput) -> nscoord {
        self.IntrinsicISize(aInput, IntrinsicISizeType::PrefISize)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IntrinsicISize (virtual; the base returns 0)
    // ADAPTER: the virtual call is a `match` on the frame type. TEMP: a frame that was given its
    // sizes with SetIntrinsicISizes() answers with those (the leaf stand-in for frame classes whose
    // content is not translated: images, form controls); it is checked first.
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        if let Some((min, pref)) = self.GetProperty(nsIFrame::IntrinsicISizesProperty()) {
            return if aType == IntrinsicISizeType::MinISize { min } else { pref };
        }
        // Gecko: layout/svg/SVGOuterSVGFrame.cpp SVGOuterSVGFrame::IntrinsicISize
        // ADAPTER: the leaf block of an <svg> element (no frame class, see SVGOuterSVGFrame.rs).
        if let Some(svgData) = self.GetProperty(SVGOuterSVGDataProperty()) {
            return SVGOuterSVGFrame_IntrinsicISize(*self, &svgData, aInput, aType);
        }
        match self.Type() {
            // The base nsIFrame::IntrinsicISize: nsInlineFrame (and the other inline containers),
            // nsPlaceholderFrame, BRFrame, WBRFrame and nsIFrame itself have no override.
            LayoutFrameType::None | LayoutFrameType::Placeholder | LayoutFrameType::Inline | LayoutFrameType::Line | LayoutFrameType::Br | LayoutFrameType::Wbr => 0,
            // Block => nsBlockFrame::IntrinsicISize
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            // Table => nsTableFrame::IntrinsicISize
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            // TableWrapper => nsTableWrapperFrame::IntrinsicISize, TableCell => nsTableCellFrame::IntrinsicISize
            // (TableRowGroup and TableRow have no override: the base class answers 0)
            LayoutFrameType::TableWrapper => nsTableWrapperFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            LayoutFrameType::TableCell => nsTableCellFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            // (TableCol and TableColGroup have no override either)
            LayoutFrameType::TableRowGroup | LayoutFrameType::TableRow | LayoutFrameType::TableCol | LayoutFrameType::TableColGroup => 0,
            // FlexContainer => nsFlexContainerFrame::IntrinsicISize
            LayoutFrameType::FlexContainer => nsFlexContainerFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            // GridContainer => nsGridContainerFrame::IntrinsicISize
            LayoutFrameType::GridContainer => nsGridContainerFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            // Text => nsTextFrame::IntrinsicISize (nsTextFrame.cpp)
            LayoutFrameType::Text => self.IntrinsicISizeFromInline(aInput, aType),
            // the form controls (layout/forms)
            LayoutFrameType::CheckboxRadio => nsCheckboxRadioFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            LayoutFrameType::TextInput => nsTextControlFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            LayoutFrameType::ComboboxControl => nsComboboxControlFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            LayoutFrameType::ListControl => nsListControlFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            // HTMLVideo => nsVideoFrame::IntrinsicISize
            LayoutFrameType::HTMLVideo => nsVideoFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            // SubDocument => nsSubDocumentFrame::IntrinsicISize
            LayoutFrameType::SubDocument => nsSubDocumentFrame::from_frame(*self).IntrinsicISize(aInput, aType),
            _ => unimplemented!("NOT PORTED: intrinsic inline-sizes of the other frame classes (flex, grid, table, replaced and form control frames, scroll containers, ...)"),
        }
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IntrinsicISizeFromInline
    pub fn IntrinsicISizeFromInline(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        // NOT PORTED: MOZ_ASSERT(!IsContainerForFontSizeInflation()) (font size inflation is off).

        if aType == IntrinsicISizeType::MinISize {
            let mut data = InlineMinISizeData::default();
            self.AddInlineMinISize(aInput, &mut data);
            data.ForceBreak();
            return data.mPrevLines;
        }

        let mut data = InlinePrefISizeData::default();
        self.AddInlinePrefISize(aInput, &mut data);
        data.ForceBreak(UsedClear::Both);
        data.mPrevLines
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsIntrinsicKeyword
    pub fn IsIntrinsicKeyword<SizeOrMaxSize: StyleSizeLike>(aSize: &SizeOrMaxSize) -> bool {
        // All keywords other than auto/none/-moz-available depend on intrinsic
        // sizes.
        aSize.IsMaxContent() || aSize.IsMinContent() || aSize.IsFitContent() || aSize.IsFitContentFunction()
    }

    // Gecko: layout/generic/nsIFrame.cpp FormControlShrinksForPercentSize
    //
    // Return true if this form control element's preferred size property (but not
    // percentage max size property) contains a percentage value that should be
    // resolved against zero when calculating its min-content contribution in the
    // corresponding axis.
    //
    // For proper replaced elements, the percentage value in both their max size
    // property or preferred size property should be resolved against zero. This is
    // handled in IsPercentageResolvedAgainstZero().
    fn FormControlShrinksForPercentSize(aFrame: &nsIFrame) -> bool {
        if !aFrame.IsReplaced() {
            // Quick test to reject most frames.
            return false;
        }

        match aFrame.Type() {
            LayoutFrameType::Progress
            | LayoutFrameType::Range
            | LayoutFrameType::TextInput
            | LayoutFrameType::ColorControl
            | LayoutFrameType::ComboboxControl
            | LayoutFrameType::ListControl
            | LayoutFrameType::CheckboxRadio
            | LayoutFrameType::FileControl
            | LayoutFrameType::ImageControl => true,
            // True buttons (<button>, backed by block/grid/flex frame) and most
            // button-flavored <inputs> (those backed by InputButtonControlFrame)
            // don't have this shrinking behavior. But color-inputs and comboboxes do;
            // and both of those derive from ButtonControlFrame. So: we can't easily
            // use do_QueryFrame to differentiate the buttons-that-do vs. the
            // buttons-that-don't. So we explicitly list the buttons-that-do by
            // LayoutFrameType above, and the others fall into this catch-all.
            _ => false,
        }
    }

    /**
     * Return true if this frame's preferred size property or max size property
     * contains a percentage value that should be resolved against zero when
     * calculating its min-content contribution in the corresponding axis.
     *
     * This is a special case for webcompat required by CSS Sizing 3 §5.2.1c
     * https://drafts.csswg.org/css-sizing-3/#replaced-percentage-min-contribution,
     * and applies only to some replaced elements and form control elements. See
     * CSS Sizing 3 §5.2.2 for the list of elements this rule applies to.
     * https://drafts.csswg.org/css-sizing-3/#min-content-zero
     *
     * Bug 1463700: some callers may not match the spec by resolving the entire
     * preferred size property or max size property against zero.
     */
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IsPercentageResolvedAgainstZero(const StyleSize&, const StyleMaxSize&)
    pub fn IsPercentageResolvedAgainstZero__StyleSize_StyleMaxSize(&self, aStyleSize: &StyleSize, aStyleMaxSize: &StyleMaxSize) -> bool {
        let sizeHasPercent = aStyleSize.HasPercent();
        ((sizeHasPercent || aStyleMaxSize.HasPercent()) && self.HasReplacedSizing()) || (sizeHasPercent && nsIFrame::FormControlShrinksForPercentSize(self))
    }

    // Summary of the Cyclic-Percentage Intrinsic Size Contribution Rules:
    //
    // Element Type         |       Replaced           |        Non-replaced
    // Contribution Type    | min-content  max-content | min-content  max-content
    // ---------------------------------------------------------------------------
    // min size             | zero         zero        | zero         zero
    // max & preferred size | zero         initial     | initial      initial
    //
    // https://drafts.csswg.org/css-sizing-3/#cyclic-percentage-contribution
    /**
     * This is simliar to the above method but accepts LengthPercentage. Return
     * true if the frame's preferred size property or max size property contains
     * a percentage value that should be resolved against zero. For min size, it
     * always returns true.
     */
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IsPercentageResolvedAgainstZero(const LengthPercentage&, SizeProperty)
    pub fn IsPercentageResolvedAgainstZero__LengthPercentage_SizeProperty(&self, aSize: &LengthPercentage, aProperty: nsIFrame_SizeProperty) -> bool {
        // Early return to avoid calling the virtual function, IsFrameOfType().
        if aProperty == nsIFrame_SizeProperty::MinSize {
            return true;
        }

        let hasPercentOnReplaced = aSize.HasPercent() && self.HasReplacedSizing();
        if aProperty == nsIFrame_SizeProperty::MaxSize {
            return hasPercentOnReplaced;
        }

        MOZ_ASSERT!(aProperty == nsIFrame_SizeProperty::Size);
        hasPercentOnReplaced || (aSize.HasPercent() && nsIFrame::FormControlShrinksForPercentSize(self))
    }

    // Gecko: layout/generic/nsIFrame.cpp ResolvePadding
    fn ResolvePadding(aStyle: &LengthPercentage, aPercentageBasis: nscoord) -> nscoord {
        nsLayoutUtils::ResolveToLength::<true>(aStyle, aPercentageBasis)
    }

    // Gecko: layout/generic/nsIFrame.cpp ResolveMargin
    fn ResolveMargin(aStyle: &StyleMargin, aPercentageBasis: nscoord) -> nscoord {
        if !aStyle.IsLengthPercentage() {
            return 0;
        }
        nsLayoutUtils::ResolveToLength::<false>(aStyle.AsLengthPercentage(), aPercentageBasis)
    }

    // Gecko: layout/generic/nsIFrame.cpp IntrinsicSizeOffsets (the static function)
    fn IntrinsicSizeOffsets__nsIFrame_nscoord_bool(aFrame: nsIFrame, aPercentageBasis: nscoord, aForISize: bool) -> nsIFrame_IntrinsicSizeOffsetData {
        let mut result = nsIFrame_IntrinsicSizeOffsetData::default();
        let wm = aFrame.GetWritingMode();
        let verticalAxis = aForISize == wm.IsVertical();
        let styleMargin = aFrame.StyleMargin();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aFrame, std::ptr::null_mut());
        if verticalAxis {
            result.margin += nsIFrame::ResolveMargin(&styleMargin.GetMargin__Side_AnchorPosResolutionParams(eSideTop, &anchorResolutionParams), aPercentageBasis);
            result.margin += nsIFrame::ResolveMargin(&styleMargin.GetMargin__Side_AnchorPosResolutionParams(eSideBottom, &anchorResolutionParams), aPercentageBasis);
        } else {
            result.margin += nsIFrame::ResolveMargin(&styleMargin.GetMargin__Side_AnchorPosResolutionParams(eSideLeft, &anchorResolutionParams), aPercentageBasis);
            result.margin += nsIFrame::ResolveMargin(&styleMargin.GetMargin__Side_AnchorPosResolutionParams(eSideRight, &anchorResolutionParams), aPercentageBasis);
        }

        let padding = aFrame.StylePadding().mPadding();
        if verticalAxis {
            result.padding += nsIFrame::ResolvePadding(padding.Get__Side(eSideTop), aPercentageBasis);
            result.padding += nsIFrame::ResolvePadding(padding.Get__Side(eSideBottom), aPercentageBasis);
        } else {
            result.padding += nsIFrame::ResolvePadding(padding.Get__Side(eSideLeft), aPercentageBasis);
            result.padding += nsIFrame::ResolvePadding(padding.Get__Side(eSideRight), aPercentageBasis);
        }

        let styleBorder = aFrame.StyleBorder();
        // (GetComputedBorderWidth(aSide) is GetComputedBorder().Side(aSide))
        if verticalAxis {
            result.border += styleBorder.GetComputedBorder().Side(eSideTop);
            result.border += styleBorder.GetComputedBorder().Side(eSideBottom);
        } else {
            result.border += styleBorder.GetComputedBorder().Side(eSideLeft);
            result.border += styleBorder.GetComputedBorder().Side(eSideRight);
        }

        // NOT PORTED: native theming (aFrame->IsThemed(disp): the widget border and padding from
        // nsITheme replace result.border and result.padding); frames are never themed here.
        result
    }

    /**
     * Return the isize components of padding, border, and margin
     * that contribute to the intrinsic width that applies to the parent.
     * @param aPercentageBasis the percentage basis to use for padding/margin -
     *   i.e. the Containing Block's inline-size
     */
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IntrinsicISizeOffsets (virtual)
    // ADAPTER: the virtual call is a `match` on the frame type; the frame classes that override it
    // (nsTableCellFrame, nsFieldSetFrame, ScrollContainerFrame, nsImageFrame, ...) are not ported,
    // so every frame answers with the base class.
    pub fn IntrinsicISizeOffsets(&self, aPercentageBasis: nscoord) -> nsIFrame_IntrinsicSizeOffsetData {
        match self.Type() {
            // Table => nsTableFrame::IntrinsicISizeOffsets
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).IntrinsicISizeOffsets(aPercentageBasis),
            // TableCell => nsTableCellFrame::IntrinsicISizeOffsets
            LayoutFrameType::TableCell => nsTableCellFrame::from_frame(*self).IntrinsicISizeOffsets(aPercentageBasis),
            _ => self.nsIFrame_IntrinsicISizeOffsets(aPercentageBasis),
        }
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IntrinsicISizeOffsets (the base class's)
    pub fn nsIFrame_IntrinsicISizeOffsets(&self, aPercentageBasis: nscoord) -> nsIFrame_IntrinsicSizeOffsetData {
        nsIFrame::IntrinsicSizeOffsets__nsIFrame_nscoord_bool(*self, aPercentageBasis, true)
    }

    /**
     * Return the bsize components of padding, border, and margin
     * that contribute to the intrinsic width that applies to the parent.
     * @param aPercentageBasis the percentage basis to use for padding/margin -
     *   i.e. the Containing Block's inline-size
     */
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IntrinsicBSizeOffsets
    pub fn IntrinsicBSizeOffsets(&self, aPercentageBasis: nscoord) -> nsIFrame_IntrinsicSizeOffsetData {
        nsIFrame::IntrinsicSizeOffsets__nsIFrame_nscoord_bool(*self, aPercentageBasis, false)
    }

    // Gecko: layout/generic/nsIFrame.h ToExtremumLength
    pub fn ToExtremumLength<SizeOrMaxSize: StyleSizeLike>(aSize: &SizeOrMaxSize) -> Option<nsIFrame_ExtremumLength> {
        if aSize.IsMinContent() {
            Some(nsIFrame_ExtremumLength::MinContent)
        } else if aSize.IsMaxContent() {
            Some(nsIFrame_ExtremumLength::MaxContent)
        } else if aSize.IsMozAvailable() {
            Some(nsIFrame_ExtremumLength::MozAvailable)
        } else if aSize.IsWebkitFillAvailable() || aSize.IsStretch() {
            Some(nsIFrame_ExtremumLength::Stretch)
        } else if aSize.IsFitContent() {
            Some(nsIFrame_ExtremumLength::FitContent)
        } else if aSize.IsFitContentFunction() {
            Some(nsIFrame_ExtremumLength::FitContentFunction)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------------------------
// nsIFrame::ComputeSize and the functions it calls (layout/generic/nsIFrame.cpp, nsContainerFrame.cpp)
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsIFrame.cpp ShouldApplyAutomaticMinimumOnInlineAxis
fn ShouldApplyAutomaticMinimumOnInlineAxis(aWM: WritingMode, aIsScrollableOverflow: bool, aParams: &AnchorPosResolutionParams, aPosition: &nsStylePosition) -> bool {
    // Apply the automatic minimum size for aspect ratio:
    // Note: The replaced elements shouldn't be here, so we only check the scroll
    // container.
    // https://drafts.csswg.org/css-sizing-4/#aspect-ratio-minimum
    !aIsScrollableOverflow && aPosition.MinISize(aWM, aParams).IsAuto()
}

impl nsIFrame {
    /**
     * Compute the size that a frame will occupy.  Called while
     * constructing the ReflowInput to produce the ComputedWidth()
     * and ComputedHeight() members.  If the frame's desired size is the size
     * of the frame's content area (excluding borders and padding), then
     * this function should return the content area size.
     * (see the long comment in nsIFrame.h for the parameters and the return value)
     */
    // Gecko: layout/generic/nsIFrame.h nsIFrame::ComputeSize (virtual)
    // ADAPTER: the dispatch over the classes that override it. nsContainerFrame, nsBlockFrame and
    // the other block-level classes use nsIFrame's.
    pub fn ComputeSize(
        &self,
        aSizingInput: &SizeComputationInput,
        aWM: WritingMode,
        aCBSize: LogicalSize,
        aAvailableISize: nscoord,
        aMargin: LogicalSize,
        aBorderPadding: LogicalSize,
        aSizeOverrides: &StyleSizeOverrides,
        aFlags: ComputeSizeFlags,
    ) -> nsIFrame_SizeComputationResult {
        // Gecko: layout/svg/SVGOuterSVGFrame.cpp SVGOuterSVGFrame::ComputeSize
        // ADAPTER: the leaf block of an <svg> element (no frame class, see SVGOuterSVGFrame.rs).
        if let Some(svgData) = self.GetProperty(SVGOuterSVGDataProperty()) {
            return SVGOuterSVGFrame_ComputeSize(*self, &svgData, aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags);
        }
        match self.Type() {
            // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::ComputeSize
            LayoutFrameType::Inline | LayoutFrameType::Line => nsInlineFrame::from_frame(*self).ComputeSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ComputeSize
            LayoutFrameType::Text => {
                // Inlines and text don't compute size before reflow.
                nsIFrame_SizeComputationResult { mLogicalSize: LogicalSize::new(aWM, NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE), mAspectRatioUsage: nsIFrame_AspectRatioUsage::None }
            }
            // Gecko: layout/generic/nsFirstLetterFrame.cpp nsFirstLetterFrame::ComputeSize
            LayoutFrameType::Letter => {
                if self.GetPrevInFlow().is_some() {
                    // We're wrapping the text *after* the first letter, so behave like an
                    // inline frame.
                    return nsIFrame_SizeComputationResult { mLogicalSize: LogicalSize::new(aWM, NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE), mAspectRatioUsage: nsIFrame_AspectRatioUsage::None };
                }
                self.nsIFrame_ComputeSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags)
            }
            // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::ComputeSize
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).ComputeSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::ComputeSize
            LayoutFrameType::TableWrapper => nsTableWrapperFrame::from_frame(*self).ComputeSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            // Gecko: layout/generic/nsVideoFrame.cpp nsVideoFrame::ComputeSize
            LayoutFrameType::HTMLVideo => nsVideoFrame::from_frame(*self).ComputeSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            // Gecko: layout/generic/nsSubDocumentFrame.cpp nsSubDocumentFrame::ComputeSize
            LayoutFrameType::SubDocument => nsSubDocumentFrame::from_frame(*self).ComputeSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            // NOT PORTED: the ComputeSize overrides of
            // nsImageFrame, nsHTMLCanvasFrame, SVGOuterSVGFrame and
            // nsRubyBaseContainerFrame.
            LayoutFrameType::Image
            | LayoutFrameType::HTMLCanvas
            | LayoutFrameType::SubDocument
            | LayoutFrameType::SVGOuterSVG
            | LayoutFrameType::RubyBaseContainer => {
                unimplemented!("NOT PORTED: ComputeSize of tables, replaced elements and ruby")
            }
            _ => self.nsIFrame_ComputeSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
        }
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::ComputeSize
    pub fn nsIFrame_ComputeSize(
        &self,
        aSizingInput: &SizeComputationInput,
        aWM: WritingMode,
        aCBSize: LogicalSize,
        aAvailableISize: nscoord,
        aMargin: LogicalSize,
        aBorderPadding: LogicalSize,
        aSizeOverrides: &StyleSizeOverrides,
        aFlags: ComputeSizeFlags,
    ) -> nsIFrame_SizeComputationResult {
        MOZ_ASSERT!(!self.GetIntrinsicRatio().operator_bool(), "Please override this method and call nsContainerFrame::ComputeSizeWithIntrinsicDimensions instead.");
        let mut result = self.ComputeAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags);
        let stylePos = self.StylePosition();
        let disp = self.StyleDisplay();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(*self, aSizingInput.mAnchorPosResolutionCache);
        let mut aspectRatioUsage = nsIFrame_AspectRatioUsage::None;

        let boxSizingAdjust = if stylePos.mBoxSizing() == StyleBoxSizing::BorderBox { aBorderPadding } else { LogicalSize::new__WritingMode(aWM) };
        let boxSizingToMarginEdgeISize = aMargin.ISize(aWM) + aBorderPadding.ISize(aWM) - boxSizingAdjust.ISize(aWM);

        let aspectRatio = aSizeOverrides.mAspectRatio.unwrap_or_else(|| self.GetAspectRatio());
        let styleISize = match &aSizeOverrides.mStyleISize {
            Some(s) => AnchorResolvedSizeHelper::Overridden(s),
            None => stylePos.ISize(aWM, &anchorResolutionParams),
        };
        // For bsize, we consider overrides *and then* we resolve 'stretch' to a
        // nscoord value, for convenience (so that we can assume that either
        // isAutoBSize is true, or styleBSize is of type LengthPercentage()).
        let styleBSize = {
            let styleBSizeConsideringOverrides = match &aSizeOverrides.mStyleBSize {
                Some(s) => AnchorResolvedSizeHelper::Overridden(s),
                None => stylePos.BSize(aWM, &anchorResolutionParams),
            };
            if styleBSizeConsideringOverrides.BehavesLikeStretchOnBlockAxis() && aCBSize.BSize(aWM) != NS_UNCONSTRAINEDSIZE {
                // We've got a 'stretch' BSize; resolve it to a length:
                let stretchBSize = nsLayoutUtils::ComputeStretchBSize(aCBSize.BSize(aWM), aMargin.BSize(aWM), aBorderPadding.BSize(aWM), stylePos.mBoxSizing());
                // Note(dshin): This allocates.
                AnchorResolvedSizeHelper::LengthPercentage(LengthPercentage::FromAppUnits(stretchBSize))
            } else {
                styleBSizeConsideringOverrides
            }
        };

        let parentFrame = self.GetParent();
        let mut alignCB = parentFrame;
        let mut isGridItem = self.IsGridItem();
        let isSubgrid = false; // NOT PORTED: subgrid (IsSubgrid() is always false here)
        if let Some(parent) = parentFrame {
            if parent.IsTableWrapperFrame() && self.IsTableFrame() {
                // An inner table frame is sized as a grid item if its table wrapper is,
                // because they actually have the same CB (the wrapper's CB).
                // @see ReflowInput::InitCBReflowInput
                let tableWrapper = parent;
                let grandParent = tableWrapper.GetParent();
                isGridItem = grandParent.map_or(false, |g| g.IsGridContainerFrame()) && !tableWrapper.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW);
                if isGridItem {
                    // When resolving justify/align-self below, we want to use the grid
                    // container's justify/align-items value and WritingMode.
                    alignCB = grandParent;
                }
            }
        }

        // flexItemMainAxis is set if this frame is a flex item in a modern flexbox
        // layout. It indicates which logical axis (in this frame's own WM)
        // corresponds to its flex container's main axis.
        let mut flexItemMainAxis: Option<LogicalAxis> = None;
        if self.IsFlexItem() && !parentFrame.unwrap().HasAnyStateBits(NS_STATE_FLEX_IS_EMULATING_LEGACY_WEBKIT_BOX) {
            flexItemMainAxis = Some(if nsFlexContainerFrame::IsItemInlineAxisMainAxis(*self) { LogicalAxis::Inline } else { LogicalAxis::Block });
        }

        let isAutoISize = styleISize.IsAuto();
        let isAutoBSize = nsLayoutUtils::IsAutoBSize(&styleBSize, aCBSize.BSize(aWM));

        MOZ_ASSERT!(isAutoBSize || styleBSize.IsLengthPercentage(), "We should have resolved away any non-'auto'-like flavors of styleBSize into a LengthPercentage. (If this fails, we might run afoul of some AsLengthPercentage() call below.)");

        // Compute inline-axis size
        let isSubgriddedInInlineAxis = isSubgrid; // && IsColSubgrid()

        // Per https://drafts.csswg.org/css-grid/#subgrid-box-alignment, if we are
        // subgridded in the inline-axis, ignore our style inline-size, and stretch to
        // fill the CB.
        let shouldComputeISize = !isAutoISize && !isSubgriddedInInlineAxis;
        if shouldComputeISize {
            let iSizeResult = self.ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_SizeOrMaxSize_StyleSize_AspectRatio_ComputeSizeFlags(
                aSizingInput.mRenderingContext,
                aWM,
                aCBSize,
                boxSizingAdjust,
                boxSizingToMarginEdgeISize,
                &styleISize,
                &styleBSize,
                aspectRatio,
                aFlags,
            );
            *result.ISize_mut(aWM) = iSizeResult.mISize;
            aspectRatioUsage = iSizeResult.mAspectRatioUsage;
        } else if isGridItem && !self.IsTrueOverflowContainer() {
            // 'auto' inline-size for grid-level box - fill the CB for 'stretch' /
            // 'normal' and clamp it to the CB if requested:
            let alignCB = alignCB.unwrap();
            let mut isStretchAligned = false;
            let mayUseAspectRatio = aspectRatio.operator_bool() && !isAutoBSize;
            // NOT PORTED: masonry (!alignCB->IsMasonry(aWM, LogicalAxis::Inline) is always true)
            if !aFlags.contains(ComputeSizeFlag::ShrinkWrap) && !self.StyleMargin().HasInlineAxisAuto(aWM, &anchorResolutionParams) {
                let inlineAxisAlignment = stylePos.UsedSelfAlignment__WritingMode_LogicalAxis_WritingMode_ComputedStyle(aWM, LogicalAxis::Inline, alignCB.GetWritingMode(), Some(alignCB.Style()));
                isStretchAligned = inlineAxisAlignment == StyleAlignFlags::STRETCH || (inlineAxisAlignment == StyleAlignFlags::NORMAL && !mayUseAspectRatio);
            }

            // Apply the preferred aspect ratio for alignments other than *stretch* and
            // *normal without aspect ratio*.
            // The spec says all other values should size the items as fit-content, and
            // the intrinsic size should respect the preferred aspect ratio, so we also
            // apply aspect ratio for all other values.
            // https://drafts.csswg.org/css-grid/#grid-item-sizing
            if !isStretchAligned && mayUseAspectRatio {
                *result.ISize_mut(aWM) = self.ComputeISizeValueFromAspectRatio(aWM, aCBSize, boxSizingAdjust, styleBSize.AsLengthPercentage(), aspectRatio);
                aspectRatioUsage = nsIFrame_AspectRatioUsage::ToComputeISize;
            }

            if isStretchAligned || aFlags.contains(ComputeSizeFlag::IClampMarginBoxMinSize) {
                let iSizeToFillCB = std::cmp::max(0 as nscoord, aCBSize.ISize(aWM) - aBorderPadding.ISize(aWM) - aMargin.ISize(aWM));
                if isStretchAligned || result.ISize(aWM) > iSizeToFillCB {
                    *result.ISize_mut(aWM) = iSizeToFillCB;
                }
            }
        } else if aspectRatio.operator_bool() && !isAutoBSize {
            // Note: if both the inline size and the block size are auto, the block axis
            // is the ratio-dependent axis by default. That means we only need to
            // transfer the resolved inline size via aspect-ratio to block axis later in
            // this method, but not the other way around.
            //
            // In this branch, we transfer the non-auto block size via aspect-ration to
            // inline axis.
            *result.ISize_mut(aWM) = self.ComputeISizeValueFromAspectRatio(aWM, aCBSize, boxSizingAdjust, styleBSize.AsLengthPercentage(), aspectRatio);
            aspectRatioUsage = nsIFrame_AspectRatioUsage::ToComputeISize;
        }

        // Calculate and apply transferred min & max size contraints.
        // https://drafts.csswg.org/css-sizing-4/#aspect-ratio-size-transfers
        //
        // Note: The basic principle is that sizing constraints transfer through the
        // aspect-ratio to the other side to preserve the aspect ratio to the extent
        // that they can without violating any sizes specified explicitly on that
        // affected axis.
        //
        // FIXME: The spec words may not be correct, so we may have to update this
        // tentative solution once this spec issue gets resolved. Here, we clamp the
        // flex base size by the transferred min and max sizes, and don't include
        // the transferred min & max sizes into its used min & max sizes. So this
        // lets us match other browsers' current behaviors.
        // https://github.com/w3c/csswg-drafts/issues/6071
        //
        // Note: This may make more sense if we clamp the flex base size in
        // FlexItem::ResolveFlexBaseSizeFromAspectRatio(). However, the result should
        // be identical. FlexItem::ResolveFlexBaseSizeFromAspectRatio() only handles
        // the case of the definite cross size, and the definite cross size is clamped
        // by the min & max cross sizes below in this function. This means its flex
        // base size has been clamped by the transferred min & max size already after
        // generating the flex items. So here we make the code more general for both
        // definite cross size and indefinite cross size.
        let isDefiniteISize = styleISize.IsLengthPercentage();
        let minBSizeCoord = stylePos.MinBSize(aWM, &anchorResolutionParams);
        let maxBSizeCoord = stylePos.MaxBSize(aWM, &anchorResolutionParams);
        let isAutoMinBSize = nsLayoutUtils::IsAutoBSize(&minBSizeCoord, aCBSize.BSize(aWM));
        let isAutoMaxBSize = nsLayoutUtils::IsAutoBSize(&maxBSizeCoord, aCBSize.BSize(aWM));
        if aspectRatio.operator_bool() && !isDefiniteISize {
            // Note: the spec mentions that
            // 1. This transferred minimum is capped by any definite preferred or
            //    maximum size in the destination axis.
            // 2. This transferred maximum is floored by any definite preferred or
            //    minimum size in the destination axis.
            //
            // https://drafts.csswg.org/css-sizing-4/#aspect-ratio-size-transfers
            //
            // The spec requires us to clamp these by the specified size (it calls it
            // the preferred size). However, we actually don't need to worry about that,
            // because we are here only if the inline size is indefinite.
            //
            // We do not need to clamp the transferred minimum and maximum as long as we
            // always apply the transferred min/max size before the explicit min/max
            // size; the result will be identical.
            let transferredMinISize = if isAutoMinBSize { 0 } else { self.ComputeISizeValueFromAspectRatio(aWM, aCBSize, boxSizingAdjust, minBSizeCoord.AsLengthPercentage(), aspectRatio) };
            let transferredMaxISize = if isAutoMaxBSize { nscoord_MAX } else { self.ComputeISizeValueFromAspectRatio(aWM, aCBSize, boxSizingAdjust, maxBSizeCoord.AsLengthPercentage(), aspectRatio) };

            *result.ISize_mut(aWM) = CSSMinMax(result.ISize(aWM), transferredMinISize, transferredMaxISize);
        }

        // Flex items ignore their min & max sizing properties in their flex
        // container's main-axis. (Those properties get applied later in the flexbox
        // algorithm.)
        let isFlexItemInlineAxisMainAxis = flexItemMainAxis == Some(LogicalAxis::Inline);
        // Grid items that are subgridded in inline-axis also ignore their min & max
        // sizing properties in that axis.
        let shouldIgnoreMinMaxISize = isFlexItemInlineAxisMainAxis || isSubgriddedInInlineAxis;
        let maxISizeCoord = stylePos.MaxISize(aWM, &anchorResolutionParams);
        let mut maxISize = NS_UNCONSTRAINEDSIZE;
        if !maxISizeCoord.IsNone() && !shouldIgnoreMinMaxISize {
            maxISize = self
                .ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_SizeOrMaxSize_StyleSize_AspectRatio_ComputeSizeFlags(
                    aSizingInput.mRenderingContext,
                    aWM,
                    aCBSize,
                    boxSizingAdjust,
                    boxSizingToMarginEdgeISize,
                    &maxISizeCoord,
                    &styleBSize,
                    aspectRatio,
                    aFlags,
                )
                .mISize;
            *result.ISize_mut(aWM) = std::cmp::min(maxISize, result.ISize(aWM));
        }

        let bSizeAsPercentageBasis = nsIFrame::ComputeBSizeValueAsPercentageBasis(&styleBSize, &minBSizeCoord, &maxBSizeCoord, aCBSize.BSize(aWM), boxSizingAdjust.BSize(aWM));
        let input = IntrinsicSizeInput::new(
            aSizingInput.mRenderingContext,
            Some(aCBSize.ConvertTo(self.GetWritingMode(), aWM)),
            Some(LogicalSize::new(aWM, NS_UNCONSTRAINEDSIZE, bSizeAsPercentageBasis).ConvertTo(self.GetWritingMode(), aWM)),
        );
        let minISizeCoord = stylePos.MinISize(aWM, &anchorResolutionParams);
        let minISize: nscoord;
        if !minISizeCoord.IsAuto() && !shouldIgnoreMinMaxISize {
            minISize = self
                .ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_SizeOrMaxSize_StyleSize_AspectRatio_ComputeSizeFlags(
                    aSizingInput.mRenderingContext,
                    aWM,
                    aCBSize,
                    boxSizingAdjust,
                    boxSizingToMarginEdgeISize,
                    &minISizeCoord,
                    &styleBSize,
                    aspectRatio,
                    aFlags,
                )
                .mISize;
        } else if aFlags.contains(ComputeSizeFlag::IApplyAutoMinSize) {
            // This implements "Implied Minimum Size of Grid Items".
            // https://drafts.csswg.org/css-grid/#min-size-auto
            let mut m = std::cmp::min(maxISize, self.GetMinISize(&input));
            if styleISize.IsLengthPercentage() {
                m = std::cmp::min(m, result.ISize(aWM));
            } else if aFlags.contains(ComputeSizeFlag::IClampMarginBoxMinSize) {
                // "if the grid item spans only grid tracks that have a fixed max track
                // sizing function, its automatic minimum size in that dimension is
                // further clamped to less than or equal to the size necessary to fit
                // its margin box within the resulting grid area (flooring at zero)"
                // https://drafts.csswg.org/css-grid/#min-size-auto
                let maxMinISize = std::cmp::max(0, aCBSize.ISize(aWM) - aBorderPadding.ISize(aWM) - aMargin.ISize(aWM));
                m = std::cmp::min(m, maxMinISize);
            }
            minISize = m;
        } else if aspectRatioUsage == nsIFrame_AspectRatioUsage::ToComputeISize && ShouldApplyAutomaticMinimumOnInlineAxis(aWM, disp.IsScrollableOverflow(), &anchorResolutionParams, &stylePos) {
            // This means we successfully applied aspect-ratio and now need to check
            // if we need to apply the automatic content-based minimum size:
            // https://drafts.csswg.org/css-sizing-4/#aspect-ratio-minimum
            MOZ_ASSERT!(!self.HasReplacedSizing(), "aspect-ratio minimums should not apply to replaced elements");
            // The inline size computed by aspect-ratio shouldn't less than the
            // min-content size, which should be capped by its maximum inline size.
            minISize = std::cmp::min(self.GetMinISize(&input), maxISize);
        } else {
            // Treat "min-width: auto" as 0.
            // NOTE: Technically, "auto" is supposed to behave like "min-content" on
            // flex items. However, we don't need to worry about that here, because
            // flex items' min-sizes are intentionally ignored until the flex
            // container explicitly considers them during space distribution.
            minISize = 0;
        }
        *result.ISize_mut(aWM) = std::cmp::max(minISize, result.ISize(aWM));

        // Compute block-axis size
        // (but not if we have auto bsize  -- then, we'll just stick with the bsize
        // that we already calculated in the initial ComputeAutoSize() call. However,
        // if we have a valid preferred aspect ratio, we still have to compute the
        // block size because aspect ratio affects the intrinsic content size.)
        let isSubgriddedInBlockAxis = isSubgrid; // && IsRowSubgrid()

        // Per https://drafts.csswg.org/css-grid/#subgrid-box-alignment, if we are
        // subgridded in the block-axis, ignore our style block-size, and stretch to
        // fill the CB.
        let shouldComputeBSize = !isAutoBSize && !isSubgriddedInBlockAxis;
        if shouldComputeBSize {
            *result.BSize_mut(aWM) = nsLayoutUtils::ComputeBSizeValue(aCBSize.BSize(aWM), boxSizingAdjust.BSize(aWM), styleBSize.AsLengthPercentage());
        } else if isGridItem && styleBSize.IsAuto() && !aFlags.contains(ComputeSizeFlag::IsGridMeasuringReflow) && !self.IsTrueOverflowContainer() {
            // NOT PORTED: masonry (!alignCB->IsMasonry(aWM, LogicalAxis::Block) is always true)
            let alignCB = alignCB.unwrap();
            let cbSize = aCBSize.BSize(aWM);
            if cbSize != NS_UNCONSTRAINEDSIZE {
                // 'auto' block-size for grid-level box - fill the CB for 'stretch' /
                // 'normal' and clamp it to the CB if requested:
                let mut isStretchAligned = false;
                let mayUseAspectRatio = aspectRatio.operator_bool() && result.ISize(aWM) != NS_UNCONSTRAINEDSIZE;
                if !self.StyleMargin().HasBlockAxisAuto(aWM, &anchorResolutionParams) {
                    let blockAxisAlignment = stylePos.UsedSelfAlignment__WritingMode_LogicalAxis_WritingMode_ComputedStyle(aWM, LogicalAxis::Block, alignCB.GetWritingMode(), Some(alignCB.Style()));
                    isStretchAligned = blockAxisAlignment == StyleAlignFlags::STRETCH || (blockAxisAlignment == StyleAlignFlags::NORMAL && !mayUseAspectRatio);
                }

                // Apply the preferred aspect ratio for alignments other than *stretch*
                // and *normal without aspect ratio*.
                // The spec says all other values should size the items as fit-content,
                // and the intrinsic size should respect the preferred aspect ratio, so
                // we also apply aspect ratio for all other values.
                // https://drafts.csswg.org/css-grid/#grid-item-sizing
                if !isStretchAligned && mayUseAspectRatio {
                    *result.BSize_mut(aWM) = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Block, aWM, result.ISize(aWM), boxSizingAdjust);
                    MOZ_ASSERT!(aspectRatioUsage == nsIFrame_AspectRatioUsage::None);
                    aspectRatioUsage = nsIFrame_AspectRatioUsage::ToComputeBSize;
                }

                if isStretchAligned || aFlags.contains(ComputeSizeFlag::BClampMarginBoxMinSize) {
                    let bSizeToFillCB = nsLayoutUtils::ComputeStretchContentBoxBSize(cbSize, aMargin.BSize(aWM), aBorderPadding.BSize(aWM));
                    if isStretchAligned || (result.BSize(aWM) != NS_UNCONSTRAINEDSIZE && result.BSize(aWM) > bSizeToFillCB) {
                        *result.BSize_mut(aWM) = bSizeToFillCB;
                    }
                }
            }
        } else if aspectRatio.operator_bool() {
            // If both inline and block dimensions are auto, the block axis is the
            // ratio-dependent axis by default.
            // If we have a super large inline size, aspect-ratio should still be
            // applied (so aspectRatioUsage flag is set as expected). That's why we
            // apply aspect-ratio unconditionally for auto block size here.
            *result.BSize_mut(aWM) = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Block, aWM, result.ISize(aWM), boxSizingAdjust);
            MOZ_ASSERT!(aspectRatioUsage == nsIFrame_AspectRatioUsage::None);
            aspectRatioUsage = nsIFrame_AspectRatioUsage::ToComputeBSize;
        }

        if result.BSize(aWM) != NS_UNCONSTRAINEDSIZE {
            // Flex items ignore their min & max sizing properties in their flex
            // container's main-axis. (Those properties get applied later in the flexbox
            // algorithm.)
            let isFlexItemBlockAxisMainAxis = flexItemMainAxis == Some(LogicalAxis::Block);
            // Grid items that are subgridded in block-axis also ignore their min & max
            // sizing properties in that axis.
            let shouldIgnoreMinMaxBSize = isFlexItemBlockAxisMainAxis || isSubgriddedInBlockAxis;
            if !isAutoMaxBSize && !shouldIgnoreMinMaxBSize {
                let maxBSize = nsLayoutUtils::ComputeBSizeValueHandlingStretch(aCBSize.BSize(aWM), aMargin.BSize(aWM), aBorderPadding.BSize(aWM), boxSizingAdjust.BSize(aWM), &maxBSizeCoord);
                *result.BSize_mut(aWM) = std::cmp::min(maxBSize, result.BSize(aWM));
            }

            if !isAutoMinBSize && !shouldIgnoreMinMaxBSize {
                let minBSize = nsLayoutUtils::ComputeBSizeValueHandlingStretch(aCBSize.BSize(aWM), aMargin.BSize(aWM), aBorderPadding.BSize(aWM), boxSizingAdjust.BSize(aWM), &minBSizeCoord);
                *result.BSize_mut(aWM) = std::cmp::max(minBSize, result.BSize(aWM));
            }
        }

        // NOT PORTED: `if (IsThemed(disp))` raises the size to the native widget's minimum size
        // (nsIFrame.cpp lines 7131-7149); frames are never themed here.

        *result.ISize_mut(aWM) = std::cmp::max(0, result.ISize(aWM));
        *result.BSize_mut(aWM) = std::cmp::max(0, result.BSize(aWM));

        nsIFrame_SizeComputationResult { mLogicalSize: result, mAspectRatioUsage: aspectRatioUsage }
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::ComputeBSizeValueAsPercentageBasis
    pub fn ComputeBSizeValueAsPercentageBasis(aStyleBSize: &StyleSize, aStyleMinBSize: &StyleSize, aStyleMaxBSize: &StyleMaxSize, aCBBSize: nscoord, aContentEdgeToBoxSizingBSize: nscoord) -> nscoord {
        if nsLayoutUtils::IsAutoBSize(aStyleBSize, aCBBSize) {
            return NS_UNCONSTRAINEDSIZE;
        }

        // TODO(dholbert): This is a temporary hack, to be fixed up in bug 1933604.
        // We don't know have aMargin or aBorderPadding args available,
        // so we use these dummy zero-valued variables as placeholders in
        // our call to ComputeBSizeValueHandlingStretch. (This might mean we
        // end up resolving 'stretch' to something slighlty-too-large for the
        // purposes of this call, if there's actually nonzero margin/border/padding).
        let dummyMargin = 0;
        let dummyBorderPadding = 0;

        let bSize = nsLayoutUtils::ComputeBSizeValueHandlingStretch(aCBBSize, dummyMargin, dummyBorderPadding, aContentEdgeToBoxSizingBSize, aStyleBSize);

        let minBSize = if nsLayoutUtils::IsAutoBSize(aStyleMinBSize, aCBBSize) {
            0
        } else {
            nsLayoutUtils::ComputeBSizeValueHandlingStretch(aCBBSize, dummyMargin, dummyBorderPadding, aContentEdgeToBoxSizingBSize, aStyleMinBSize)
        };

        let maxBSize = if nsLayoutUtils::IsAutoBSize(aStyleMaxBSize, aCBBSize) {
            NS_UNCONSTRAINEDSIZE
        } else {
            nsLayoutUtils::ComputeBSizeValueHandlingStretch(aCBBSize, dummyMargin, dummyBorderPadding, aContentEdgeToBoxSizingBSize, aStyleMaxBSize)
        };

        CSSMinMax(bSize, minBSize, maxBSize)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsTrueOverflowContainer
    // NOT PORTED: overflow containers (fragmentation); never true here.
    pub fn IsTrueOverflowContainer(&self) -> bool {
        false
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ComputeAutoSize and
    // layout/generic/nsIFrame.cpp nsIFrame::ComputeAutoSize (virtual)
    // ADAPTER: the dispatch. The block-level container classes use nsContainerFrame's; the leaf
    // frames (nsLeafFrame::ComputeAutoSize, nsCheckboxRadioFrame) and the table frames are NOT PORTED.
    pub fn ComputeAutoSize(
        &self,
        aSizingInput: &SizeComputationInput,
        aWM: WritingMode,
        aCBSize: LogicalSize,
        aAvailableISize: nscoord,
        aMargin: LogicalSize,
        aBorderPadding: LogicalSize,
        aSizeOverrides: &StyleSizeOverrides,
        aFlags: ComputeSizeFlags,
    ) -> LogicalSize {
        match self.Type() {
            LayoutFrameType::Block
            | LayoutFrameType::Canvas
            | LayoutFrameType::Viewport
            | LayoutFrameType::ScrollContainer
            | LayoutFrameType::ColumnSet
            | LayoutFrameType::ColumnSetWrapper
            | LayoutFrameType::FlexContainer
            | LayoutFrameType::GridContainer
            | LayoutFrameType::FieldSet
            | LayoutFrameType::Letter
            | LayoutFrameType::Inline => self.nsContainerFrame_ComputeAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::ComputeAutoSize
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).ComputeAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::ComputeAutoSize
            LayoutFrameType::TableWrapper => nsTableWrapperFrame::from_frame(*self).ComputeAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            // the cell, row and row group have no override (nsContainerFrame::ComputeAutoSize)
            LayoutFrameType::TableCell | LayoutFrameType::TableRow | LayoutFrameType::TableRowGroup | LayoutFrameType::TableColGroup => self.nsContainerFrame_ComputeAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            // Gecko: layout/forms/nsCheckboxRadioFrame.cpp nsCheckboxRadioFrame::ComputeAutoSize
            LayoutFrameType::CheckboxRadio => nsCheckboxRadioFrame::from_frame(*self).ComputeAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            // the text control (a ScrollContainerFrame), the combobox (a ButtonControlFrame, an nsBlockFrame) and
            // the list control (a ScrollContainerFrame) have no override (nsContainerFrame::ComputeAutoSize)
            // (nsVideoFrame and nsSubDocumentFrame have none either)
            LayoutFrameType::TextInput | LayoutFrameType::ComboboxControl | LayoutFrameType::ListControl | LayoutFrameType::HTMLVideo | LayoutFrameType::SubDocument => self.nsContainerFrame_ComputeAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            LayoutFrameType::None | LayoutFrameType::Br | LayoutFrameType::Wbr | LayoutFrameType::Placeholder | LayoutFrameType::Text | LayoutFrameType::TableCol => self.nsIFrame_ComputeAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            _ => unimplemented!("NOT PORTED: ComputeAutoSize of leaf, table and form control frames"),
        }
    }

    // nsContainerFrame::ComputeAutoSize (nsContainerFrame_ComputeAutoSize) moved to nsContainerFrame.rs.

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IsAbsolutelyPositionedWithDefiniteContainingBlock
    pub fn IsAbsolutelyPositionedWithDefiniteContainingBlock(&self) -> bool {
        // TODO(dshin, Bug 1927861): Even if an absolute container should have a
        // definite size, in a continuation context, the full extent of the containing
        // block is not known.
        self.IsAbsolutelyPositioned(None) && self.GetPrevInFlow().is_none()
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::ComputeAutoSize
    pub fn nsIFrame_ComputeAutoSize(
        &self,
        aSizingInput: &SizeComputationInput,
        aWM: WritingMode,
        aCBSize: LogicalSize,
        aAvailableISize: nscoord,
        aMargin: LogicalSize,
        aBorderPadding: LogicalSize,
        aSizeOverrides: &StyleSizeOverrides,
        aFlags: ComputeSizeFlags,
    ) -> LogicalSize {
        if self.IsAbsolutelyPositionedWithDefiniteContainingBlock() {
            return self.ComputeAbsolutePosAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags);
        }

        // Use basic shrink-wrapping as a default implementation.
        let mut result = LogicalSize::new(aWM, 0xdeadbeef_u32 as i32, NS_UNCONSTRAINEDSIZE);

        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(*self, std::ptr::null_mut());
        // don't bother setting it if the result won't be used
        let styleISize = match &aSizeOverrides.mStyleISize {
            Some(s) => AnchorResolvedSizeHelper::Overridden(s),
            None => self.StylePosition().ISize(aWM, &anchorResolutionParams),
        };
        if styleISize.IsAuto() {
            let availBased = nsLayoutUtils::ComputeStretchContentBoxISize(aAvailableISize, aMargin.ISize(aWM), aBorderPadding.ISize(aWM));
            let stylePos = self.StylePosition();
            let styleBSize = match &aSizeOverrides.mStyleBSize {
                Some(s) => AnchorResolvedSizeHelper::Overridden(s),
                None => stylePos.BSize(aWM, &anchorResolutionParams),
            };
            let contentEdgeToBoxSizing = if stylePos.mBoxSizing() == StyleBoxSizing::BorderBox { aBorderPadding } else { LogicalSize::new__WritingMode(aWM) };
            let bSize = nsIFrame::ComputeBSizeValueAsPercentageBasis(
                &styleBSize,
                &stylePos.MinBSize(aWM, &anchorResolutionParams),
                &stylePos.MaxBSize(aWM, &anchorResolutionParams),
                aCBSize.BSize(aWM),
                contentEdgeToBoxSizing.BSize(aWM),
            );
            let input = IntrinsicSizeInput::new(
                aSizingInput.mRenderingContext,
                Some(aCBSize.ConvertTo(self.GetWritingMode(), aWM)),
                Some(LogicalSize::new(aWM, NS_UNCONSTRAINEDSIZE, bSize).ConvertTo(self.GetWritingMode(), aWM)),
            );
            *result.ISize_mut(aWM) = self.ShrinkISizeToFit(&input, availBased, aFlags);
        }
        result
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::ComputeAbsolutePosAutoSize
    // NOT PORTED: anchor positioning (position-area, the anchor-resolved sizes and insets are the
    // style's).
    pub fn ComputeAbsolutePosAutoSize(
        &self,
        aSizingInput: &SizeComputationInput,
        aWM: WritingMode,
        aCBSize: LogicalSize,
        aAvailableISize: nscoord,
        aMargin: LogicalSize,
        aBorderPadding: LogicalSize,
        aSizeOverrides: &StyleSizeOverrides,
        aFlags: ComputeSizeFlags,
    ) -> LogicalSize {
        MOZ_ASSERT!(self.IsAbsolutelyPositionedWithDefiniteContainingBlock(), "Asking for absolute auto size when not absolute");
        // Ideally, this is an assertion, but the containing block could just be
        // really big.
        NS_WARNING_ASSERTION!(aCBSize.ISize(aWM) != NS_UNCONSTRAINEDSIZE && aCBSize.BSize(aWM) != NS_UNCONSTRAINEDSIZE, "Absolute containing block size not definite?");
        let mut result = LogicalSize::new(aWM, 0xdeadbeef_u32 as i32, NS_UNCONSTRAINEDSIZE);

        let stylePos = self.StylePosition();
        let anchorResolutionParams = AnchorPosOffsetResolutionParams::UseCBFrameSize(AnchorPosResolutionParams::From__SizeComputationInput_bool(aSizingInput, false));
        let styleISize = match &aSizeOverrides.mStyleISize {
            Some(s) => AnchorResolvedSizeHelper::Overridden(s),
            None => stylePos.ISize(aWM, &anchorResolutionParams.mBaseParams),
        };
        let styleBSize = match &aSizeOverrides.mStyleBSize {
            Some(s) => AnchorResolvedSizeHelper::Overridden(s),
            None => stylePos.BSize(aWM, &anchorResolutionParams.mBaseParams),
        };
        let iStartOffsetIsAuto = stylePos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::IStart, aWM, &anchorResolutionParams).IsAuto();
        let iEndOffsetIsAuto = stylePos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::IEnd, aWM, &anchorResolutionParams).IsAuto();
        let bStartOffsetIsAuto = stylePos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BStart, aWM, &anchorResolutionParams).IsAuto();
        let bEndOffsetIsAuto = stylePos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BEnd, aWM, &anchorResolutionParams).IsAuto();
        let boxSizingAdjust = if stylePos.mBoxSizing() == StyleBoxSizing::BorderBox { aBorderPadding } else { LogicalSize::new__WritingMode(aWM) };
        let shouldStretch = |mut aAlignment: StyleAlignFlags, aFrame: nsIFrame, aStartIsAuto: bool, aEndIsAuto: bool| -> bool {
            if aStartIsAuto || aEndIsAuto {
                // Note(dshin, bug 1930427): This is not part of the current spec [1];
                // however, no one implements the new inset behaviour [2], and the old
                // behaviour [3] ends up computing the static size if both or one inset is
                // auto.
                //
                // [1]: https://drafts.csswg.org/css-position-3/#abspos-auto-size
                // [2]: https://drafts.csswg.org/css-position-3/#resolving-insets
                // [3]: https://drafts.csswg.org/css-position-3/#abspos-old
                return false;
            }
            // Don't care about flag bits for auto-sizing.
            aAlignment &= !StyleAlignFlags::FLAG_BITS;

            if aAlignment == StyleAlignFlags::STRETCH {
                return true;
            }

            if aAlignment == StyleAlignFlags::NORMAL {
                // Some replaced elements behave as semi-replaced elements - we want them
                // to stretch (See bug 1740580).
                return !aFrame.HasReplacedSizing() && !aFrame.IsTableWrapperFrame();
            }

            false
        };

        // i.e. Absolute containing block

        // Self alignment properties translate `auto` to normal for this purpose.
        // https://drafts.csswg.org/css-align-3/#valdef-justify-self-auto
        let contFrame = nsContainerFrame::from_frame(*self);
        // (posArea: NOT PORTED, anchor positioning.)
        let containerWM = self.GetParent().unwrap().GetWritingMode();
        let containerAxis = |aSubjectAxis: LogicalAxis| aWM.ConvertAxisTo(aSubjectAxis, containerWM);
        let inlineSelfAlign = contFrame.CSSAlignmentForAbsPosChildWithinContainingBlock(aSizingInput, containerAxis(LogicalAxis::Inline), aCBSize);
        let blockSelfAlign = contFrame.CSSAlignmentForAbsPosChildWithinContainingBlock(aSizingInput, containerAxis(LogicalAxis::Block), aCBSize);
        let iShouldStretch = shouldStretch(inlineSelfAlign, *self, iStartOffsetIsAuto, iEndOffsetIsAuto);
        let bShouldStretch = shouldStretch(blockSelfAlign, *self, bStartOffsetIsAuto, bEndOffsetIsAuto);
        let iSizeIsAuto = styleISize.IsAuto();
        // Note(dshin, bug 1789477): `auto` in the context of abs-element uses
        // stretch-fit sizing, given specific alignment conditions [1]. Effectively,
        // `auto` is `stretch`. `nsLayoutUtils::IsAutoBSize` is not the right tool
        // here, since the mapping is explicit, and it's incorrect to e.g. map
        // `fit-content` to `stretch`.
        // `-moz-available` behaves like `auto` in general, so map the same way.
        // When Bug 567039 brings `-moz-available` into alignment with `stretch`, this
        // special check can be removed. TODO(dshin): we're probably duplicating the
        // `stretch` logic here, since `stretch` is `stretch-fit` sizing [2].
        //
        // [1]: https://drafts.csswg.org/css-position/#abspos-auto-size
        // [2]: https://drafts.csswg.org/css-sizing-4/#valdef-width-stretch
        let bSizeIsAuto = styleBSize.IsAuto() || styleBSize.IsMozAvailable();
        if bSizeIsAuto && bShouldStretch {
            *result.BSize_mut(aWM) = nsLayoutUtils::ComputeStretchContentBoxBSize(aCBSize.BSize(aWM), aMargin.BSize(aWM), aBorderPadding.BSize(aWM));
        }
        if iSizeIsAuto {
            if iShouldStretch {
                // inline-size to make our margin-box fill the containing block:
                *result.ISize_mut(aWM) = nsLayoutUtils::ComputeStretchContentBoxISize(aCBSize.ISize(aWM), aMargin.ISize(aWM), aBorderPadding.ISize(aWM));
            } else {
                // inline-size to make our margin-box fill aAvailableISize:
                let availBased = nsLayoutUtils::ComputeStretchContentBoxISize(aAvailableISize, aMargin.ISize(aWM), aBorderPadding.ISize(aWM));

                let bSize = nsIFrame::ComputeBSizeValueAsPercentageBasis(
                    &(if styleBSize.IsAuto() && result.BSize(aWM) != NS_UNCONSTRAINEDSIZE { StyleSize::FromAppUnits(result.BSize(aWM)) } else { styleBSize.clone() }),
                    &stylePos.MinBSize(aWM, &anchorResolutionParams.mBaseParams),
                    &stylePos.MaxBSize(aWM, &anchorResolutionParams.mBaseParams),
                    aCBSize.BSize(aWM),
                    boxSizingAdjust.BSize(aWM),
                );

                let input = IntrinsicSizeInput::new(
                    aSizingInput.mRenderingContext,
                    Some(aCBSize.ConvertTo(self.GetWritingMode(), aWM)),
                    Some(LogicalSize::new(aWM, NS_UNCONSTRAINEDSIZE, bSize).ConvertTo(self.GetWritingMode(), aWM)),
                );
                *result.ISize_mut(aWM) = self.ShrinkISizeToFit(&input, availBased, aFlags);
            }
        }

        let aspectRatio = match aSizeOverrides.mAspectRatio {
            Some(a) => a,
            None => self.GetAspectRatio(),
        };
        if aspectRatio.operator_bool() {
            let mut aspectRatioUsage = nsIFrame_AspectRatioUsage::None;
            if iSizeIsAuto != bSizeIsAuto {
                // Auto axis is dependent.
                if iSizeIsAuto {
                    aspectRatioUsage = nsIFrame_AspectRatioUsage::ToComputeBSize;
                } else {
                    aspectRatioUsage = nsIFrame_AspectRatioUsage::ToComputeISize;
                }
            } else if iSizeIsAuto {
                // Both axes are `auto`.
                if iShouldStretch != bShouldStretch {
                    // If an axis has stretch, that behaves like a definite size.
                    aspectRatioUsage = if iShouldStretch { nsIFrame_AspectRatioUsage::ToComputeBSize } else { nsIFrame_AspectRatioUsage::ToComputeISize };
                } else if !iShouldStretch {
                    // If one axis has `auto` inset, that is the ratio dependent axis,
                    // otherwise the block axis is.
                    let inlineInsetHasAuto = iStartOffsetIsAuto || iEndOffsetIsAuto;
                    let blockInsetHasAuto = bStartOffsetIsAuto || bEndOffsetIsAuto;
                    aspectRatioUsage = if inlineInsetHasAuto && !blockInsetHasAuto { nsIFrame_AspectRatioUsage::ToComputeISize } else { nsIFrame_AspectRatioUsage::ToComputeBSize };
                }
            }

            if aspectRatioUsage == nsIFrame_AspectRatioUsage::ToComputeBSize && !bShouldStretch {
                *result.BSize_mut(aWM) = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Block, aWM, result.ISize(aWM), boxSizingAdjust);
            } else if aspectRatioUsage == nsIFrame_AspectRatioUsage::ToComputeISize && !iShouldStretch && result.BSize(aWM) != NS_UNCONSTRAINEDSIZE {
                *result.ISize_mut(aWM) = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Inline, aWM, result.BSize(aWM), boxSizingAdjust);
            }
        }

        result
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::ShrinkISizeToFit
    // NOT PORTED: AutoMaybeDisableFontInflation (font size inflation is off on desktop).
    pub fn ShrinkISizeToFit(&self, aInput: &IntrinsicSizeInput, aISizeInCB: nscoord, aFlags: ComputeSizeFlags) -> nscoord {
        let result: nscoord;
        let minISize = self.GetMinISize(aInput);
        if minISize > aISizeInCB {
            let clamp = aFlags.contains(ComputeSizeFlag::IClampMarginBoxMinSize);
            result = if clamp { aISizeInCB } else { minISize };
        } else {
            let prefISize = self.GetPrefISize(aInput);
            if prefISize > aISizeInCB {
                result = aISizeInCB;
            } else {
                result = prefISize;
            }
        }
        result
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::ComputeISizeValueFromAspectRatio
    pub fn ComputeISizeValueFromAspectRatio(&self, aWM: WritingMode, aCBSize: LogicalSize, aContentEdgeToBoxSizing: LogicalSize, aBSize: &LengthPercentage, aAspectRatio: AspectRatio) -> nscoord {
        MOZ_ASSERT!(aAspectRatio.operator_bool(), "Must have a valid AspectRatio!");
        let bSize = nsLayoutUtils::ComputeBSizeValue(aCBSize.BSize(aWM), aContentEdgeToBoxSizing.BSize(aWM), aBSize);
        aAspectRatio.ComputeRatioDependentSize(LogicalAxis::Inline, aWM, bSize, aContentEdgeToBoxSizing)
    }

    // Helper function - computes the content-box inline size for aSize, which is
    // a more complex version to resolve a StyleExtremumLength.
    //
    // @param aAvailableISizeOverride If this has a value, it is used as the
    // available inline-size instead of aCBSize.ISize(aWM) when resolving
    // fit-content.
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::ComputeISizeValue(gfxContext*, WritingMode, ..., ExtremumLength, ...)
    pub fn ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_ExtremumLength_Maybe_StyleSize_AspectRatio_ComputeSizeFlags(
        &self,
        aRenderingContext: Option<gfxContext>,
        aWM: WritingMode,
        aCBSize: LogicalSize,
        aContentEdgeToBoxSizing: LogicalSize,
        aBoxSizingToMarginEdge: nscoord,
        aSize: nsIFrame_ExtremumLength,
        aAvailableISizeOverride: Option<nscoord>,
        aStyleBSize: &StyleSize,
        aAspectRatio: AspectRatio,
        aFlags: ComputeSizeFlags,
    ) -> nsIFrame_ISizeComputationResult {
        let GetAvailableISize = || aCBSize.ISize(aWM) - aBoxSizingToMarginEdge - aContentEdgeToBoxSizing.ISize(aWM);

        // NOT PORTED: AutoMaybeDisableFontInflation (font size inflation is off on desktop).
        // If we have an aspect-ratio and a definite block size, we should use them to
        // resolve the sizes with intrinsic keywords.
        // https://github.com/w3c/csswg-drafts/issues/5032
        let iSizeFromAspectRatio: Option<nscoord> = (|| -> Option<nscoord> {
            if aSize == nsIFrame_ExtremumLength::MozAvailable || aSize == nsIFrame_ExtremumLength::Stretch {
                return None;
            }
            if !aAspectRatio.operator_bool() {
                return None;
            }
            if nsLayoutUtils::IsAutoBSize(aStyleBSize, aCBSize.BSize(aWM)) {
                return None;
            }

            // Helper used below to resolve aStyleBSize if it's 'stretch' or an alias.
            // XXXdholbert Really we should be resolving 'stretch' and its aliases
            // sooner; see bug 2000035.
            let ResolveStretchBSize = || -> LengthPercentage {
                MOZ_ASSERT!(aStyleBSize.BehavesLikeStretchOnBlockAxis(), "Only call me for 'stretch'-like BSizes");
                MOZ_ASSERT!(
                    aCBSize.BSize(aWM) != NS_UNCONSTRAINEDSIZE,
                    "If aStyleBSize is stretch-like, then unconstrained aCBSize.BSize should make us return via the IsAutoBSize check above"
                );

                // NOTE: the borderPadding and margin variables might be zero-filled
                // instead of having the true values, if those values haven't been
                // stashed in our property-table yet (e.g. if we're in the midst of
                // setting up a ReflowInput for our first reflow). So ideally, we should
                // be resolving 'stretch' **in our callers** rather than here, if those
                // callers have more up-to-date resolved margin/border/padding values.
                // We'll still make a best-effort attempt to resolve 'stretch' here,
                // though, for the benefit of callers that might not have handled it, to
                // be sure we don't abort in aStyleBSize.AsLengthPercentage(). Ultimately
                // this all can be removed when we fix bug 2000035.
                let borderPadding = self.GetLogicalUsedBorderAndPadding(aWM);
                let margin = self.GetLogicalUsedMargin(aWM);
                let stretchBSize = nsLayoutUtils::ComputeStretchBSize(aCBSize.BSize(aWM), margin.BStartEnd(aWM), borderPadding.BStartEnd(aWM), self.StylePosition().mBoxSizing());
                LengthPercentage::FromAppUnits(stretchBSize)
            };

            if aStyleBSize.BehavesLikeStretchOnBlockAxis() {
                Some(self.ComputeISizeValueFromAspectRatio(aWM, aCBSize, aContentEdgeToBoxSizing, &ResolveStretchBSize(), aAspectRatio))
            } else {
                Some(self.ComputeISizeValueFromAspectRatio(aWM, aCBSize, aContentEdgeToBoxSizing, aStyleBSize.AsLengthPercentage(), aAspectRatio))
            }
        })();

        let stylePos = self.StylePosition();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(*self, std::ptr::null_mut());
        let bSize = nsIFrame::ComputeBSizeValueAsPercentageBasis(
            aStyleBSize,
            &stylePos.MinBSize(aWM, &anchorResolutionParams),
            &stylePos.MaxBSize(aWM, &anchorResolutionParams),
            aCBSize.BSize(aWM),
            aContentEdgeToBoxSizing.BSize(aWM),
        );
        let input = IntrinsicSizeInput::new(
            aRenderingContext,
            Some(aCBSize.ConvertTo(self.GetWritingMode(), aWM)),
            Some(LogicalSize::new(aWM, NS_UNCONSTRAINEDSIZE, bSize).ConvertTo(self.GetWritingMode(), aWM)),
        );
        let result: nscoord;
        match aSize {
            nsIFrame_ExtremumLength::MaxContent => {
                result = match iSizeFromAspectRatio {
                    Some(v) => v,
                    None => self.GetPrefISize(&input),
                };
                NS_ASSERTION!(result >= 0, "inline-size less than zero");
                nsIFrame_ISizeComputationResult {
                    mISize: result,
                    mAspectRatioUsage: if iSizeFromAspectRatio.is_some() { nsIFrame_AspectRatioUsage::ToComputeISize } else { nsIFrame_AspectRatioUsage::None },
                }
            }
            nsIFrame_ExtremumLength::MinContent => {
                let mut r = match iSizeFromAspectRatio {
                    Some(v) => v,
                    None => self.GetMinISize(&input),
                };
                NS_ASSERTION!(r >= 0, "inline-size less than zero");
                if aFlags.contains(ComputeSizeFlag::IClampMarginBoxMinSize) {
                    r = std::cmp::min(GetAvailableISize(), r);
                }
                nsIFrame_ISizeComputationResult {
                    mISize: r,
                    mAspectRatioUsage: if iSizeFromAspectRatio.is_some() { nsIFrame_AspectRatioUsage::ToComputeISize } else { nsIFrame_AspectRatioUsage::None },
                }
            }
            nsIFrame_ExtremumLength::FitContentFunction | nsIFrame_ExtremumLength::FitContent => {
                let mut pref = NS_UNCONSTRAINEDSIZE;
                let mut min = 0;
                if let Some(v) = iSizeFromAspectRatio {
                    // The min-content and max-content size are identical and equal to the
                    // size computed from the block size and the aspect ratio.
                    pref = v;
                    min = v;
                } else {
                    pref = self.GetPrefISize(&input);
                    min = self.GetMinISize(&input);
                }

                let fill = match aAvailableISizeOverride {
                    Some(v) => v,
                    None => GetAvailableISize(),
                };
                if aFlags.contains(ComputeSizeFlag::IClampMarginBoxMinSize) {
                    min = std::cmp::min(min, fill);
                }
                let r = std::cmp::max(min, std::cmp::min(pref, fill));
                NS_ASSERTION!(r >= 0, "inline-size less than zero");
                nsIFrame_ISizeComputationResult { mISize: r, mAspectRatioUsage: nsIFrame_AspectRatioUsage::None }
            }
            nsIFrame_ExtremumLength::MozAvailable | nsIFrame_ExtremumLength::Stretch => nsIFrame_ISizeComputationResult { mISize: GetAvailableISize(), mAspectRatioUsage: nsIFrame_AspectRatioUsage::None },
        }
    }

    // Helper function - computes the content-box inline size for aSize, which is
    // a simpler version to resolve a LengthPercentage.
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::ComputeISizeValue(WritingMode, LogicalSize, LogicalSize, LengthPercentage)
    pub fn ComputeISizeValue__WritingMode_LogicalSize_LogicalSize_LengthPercentage(&self, aWM: WritingMode, aCBSize: LogicalSize, aContentEdgeToBoxSizing: LogicalSize, aSize: &LengthPercentage) -> nscoord {
        NS_WARNING_ASSERTION!(
            aCBSize.ISize(aWM) != NS_UNCONSTRAINEDSIZE,
            "have unconstrained inline-size; this should only result from very large sizes, not attempts at intrinsic inline-size calculation"
        );
        NS_ASSERTION!(aCBSize.ISize(aWM) >= 0, "inline-size less than zero");

        let mut result = aSize.Resolve(aCBSize.ISize(aWM));
        // The result of a calc() expression might be less than 0; we
        // should clamp at runtime (below).  (Percentages and coords that
        // are less than 0 have already been dropped by the parser.)
        result -= aContentEdgeToBoxSizing.ISize(aWM);
        std::cmp::max(0, result)
    }

    // Compute content-box inline size for aSize.
    //
    // This method doesn't handle 'auto' when aSize is of type StyleSize,
    // nor does it handle 'none' when aSize is of type StyleMaxSize.
    //
    // @param aStyleBSize the style block size of the frame, used to compute
    // intrinsic inline size with aAspectRatio.
    //
    // @param aAspectRatio the preferred aspect-ratio of the frame.
    // Gecko: layout/generic/nsIFrame.h nsIFrame::ComputeISizeValue<SizeOrMaxSize>
    pub fn ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_SizeOrMaxSize_StyleSize_AspectRatio_ComputeSizeFlags<SizeOrMaxSize: StyleSizeLike>(
        &self,
        aRenderingContext: Option<gfxContext>,
        aWM: WritingMode,
        aCBSize: LogicalSize,
        aContentEdgeToBoxSizing: LogicalSize,
        aBoxSizingToMarginEdge: nscoord,
        aSize: &SizeOrMaxSize,
        aStyleBSize: &StyleSize,
        aAspectRatio: AspectRatio,
        aFlags: ComputeSizeFlags,
    ) -> nsIFrame_ISizeComputationResult {
        if aSize.IsLengthPercentage() {
            return nsIFrame_ISizeComputationResult {
                mISize: self.ComputeISizeValue__WritingMode_LogicalSize_LogicalSize_LengthPercentage(aWM, aCBSize, aContentEdgeToBoxSizing, aSize.AsLengthPercentage()),
                mAspectRatioUsage: nsIFrame_AspectRatioUsage::None,
            };
        }
        let length = nsIFrame::ToExtremumLength(aSize);
        MOZ_ASSERT!(length.is_some(), "This doesn't handle none / auto");
        let mut availbleISizeOverride: Option<nscoord> = None;
        if aSize.IsFitContentFunction() {
            availbleISizeOverride = Some(aSize.AsFitContentFunction().Resolve(aCBSize.ISize(aWM)));
        }
        self.ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_ExtremumLength_Maybe_StyleSize_AspectRatio_ComputeSizeFlags(
            aRenderingContext,
            aWM,
            aCBSize,
            aContentEdgeToBoxSizing,
            aBoxSizingToMarginEdge,
            length.unwrap_or(nsIFrame_ExtremumLength::MinContent),
            availbleISizeOverride,
            aStyleBSize,
            aAspectRatio,
            aFlags,
        )
    }
}

impl nsIFrame {
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::CanBeDynamicReflowRoot
    // NOT PORTED: the dynamic reflow root optimization (nsIFrame.cpp lines 2407-2465): it lets a
    // later incremental reflow start below the root. A first, complete reflow never reads the bit,
    // and "false" (no frame is a reflow root) is always correct.
    pub fn CanBeDynamicReflowRoot(&self) -> bool {
        false
    }
}

impl nsIFrame {
    // ADAPTER: Gecko's `PrincipalChildList().FirstChild()`; the child lists are not modelled, the
    // frame tree has one list of children per frame.
    pub fn PrincipalChildList_FirstChild(&self) -> Option<nsIFrame> {
        self.with(|d| d.mFrames.FirstChild())
    }
}

// Gecko: layout/generic/nsIFrame.h mozilla::BaselineSharingGroup
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaselineSharingGroup {
    First,
    Last,
}

// Gecko: layout/generic/nsIFrame.h mozilla::BaselineExportContext
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaselineExportContext {
    LineLayout,
    Other,
}

// ADAPTER: nsIFrame members that nsLineLayout.rs and BlockReflowState.rs call, moved here from the
// adapters (Gecko: layout/generic/nsIFrame.h, nsIFrame.cpp, nsSplittableFrame.h, nsSplittableFrame.cpp).
impl nsIFrame {

    // Gecko: layout/generic/nsIFrame.h nsIFrame::SetRect(WritingMode, const LogicalRect&, const nsSize&)
    pub fn SetRect__WritingMode_LogicalRect_nsSize(&self, aWritingMode: WritingMode, aRect: LogicalRect, aContainerSize: nsSize) {
        self.SetRect(aRect.GetPhysicalRect(aWritingMode, aContainerSize));
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::SetPosition(const nsPoint&)
    pub fn SetPosition__nsPoint(&self, aPt: nsPoint) {
        let mut rect = self.GetRect();
        if nsPoint::new(rect.x, rect.y) == aPt {
            return;
        }
        rect.MoveTo__nscoord_nscoord(aPt.x, aPt.y);
        // MarkNeedsDisplayItemRebuild(): NOT PORTED, display lists.
        self.SetRect(rect);
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::SetPosition(WritingMode, const LogicalPoint&, const nsSize&)
    pub fn SetPosition__WritingMode_LogicalPoint_nsSize(&self, aWritingMode: WritingMode, aPt: LogicalPoint, aContainerSize: nsSize) {
        // We subtract mRect.Size() from the container size to account for
        // the fact that logical origins in RTL coordinate systems are at
        // the top right of the frame instead of the top left.
        self.SetPosition__nsPoint(aPt.GetPhysicalPoint(aWritingMode, aContainerSize - self.GetSize()));
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::SetSize(const nsSize&)
    pub fn SetSize__nsSize(&self, aSize: nsSize) {
        let r = self.GetRect();
        self.SetRect(nsRect::new(r.x, r.y, aSize.width, aSize.height));
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::SetSize(WritingMode, const LogicalSize&)
    pub fn SetSize__WritingMode_LogicalSize(&self, aWritingMode: WritingMode, aSize: LogicalSize) {
        if aWritingMode.IsPhysicalRTL() {
            let oldWidth = self.GetRect().width;
            self.SetSize__nsSize(aSize.GetPhysicalSize(aWritingMode));
            let mut r = self.GetRect();
            r.x -= r.width - oldWidth;
            self.SetRect(r);
        } else {
            self.SetSize__nsSize(aSize.GetPhysicalSize(aWritingMode));
        }
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalRect(WritingMode, const nsSize&)
    pub fn GetLogicalRect__WritingMode_nsSize(&self, aWritingMode: WritingMode, aContainerSize: nsSize) -> LogicalRect {
        LogicalRect::new__WritingMode_nsRect_nsSize(aWritingMode, self.GetRect(), aContainerSize)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalPosition(const nsSize&)
    pub fn GetLogicalPosition__nsSize(&self, aContainerSize: nsSize) -> LogicalPoint {
        let wm = self.GetWritingMode();
        self.GetLogicalRect__WritingMode_nsSize(wm, aContainerSize).Origin(wm)
    }

    pub fn IsBrFrame(&self) -> bool {
        self.Type() == LayoutFrameType::Br
    }
    pub fn IsLetterFrame(&self) -> bool {
        self.Type() == LayoutFrameType::Letter
    }
    pub fn IsLineFrame(&self) -> bool {
        self.Type() == LayoutFrameType::Line
    }
    pub fn IsInlineFrame(&self) -> bool {
        self.Type() == LayoutFrameType::Inline
    }
    pub fn IsRubyFrame(&self) -> bool {
        self.Type() == LayoutFrameType::Ruby
    }
    pub fn IsRubyBaseFrame(&self) -> bool {
        self.Type() == LayoutFrameType::RubyBase
    }
    pub fn IsRubyBaseContainerFrame(&self) -> bool {
        self.Type() == LayoutFrameType::RubyBaseContainer
    }
    pub fn IsRubyTextContainerFrame(&self) -> bool {
        self.Type() == LayoutFrameType::RubyTextContainer
    }
    pub fn IsTextFrame(&self) -> bool {
        self.Type() == LayoutFrameType::Text
    }

    // Gecko: layout/generic/nsSplittableFrame.h nsSplittableFrame::GetPrevContinuation
    // (fluid or not)
    pub fn GetPrevContinuation(&self) -> Option<nsIFrame> {
        self.with(|d| d.mPrevContinuation)
    }

    // Gecko: layout/generic/nsSplittableFrame.h nsSplittableFrame::GetNextContinuation
    pub fn GetNextContinuation(&self) -> Option<nsIFrame> {
        self.with(|d| d.mNextContinuation)
    }

    // Gecko: layout/generic/nsSplittableFrame.cpp nsSplittableFrame::LastContinuation
    pub fn LastContinuation(&self) -> nsIFrame {
        let mut lastContinuation = *self;
        while let Some(next) = lastContinuation.GetNextContinuation() {
            lastContinuation = next;
        }
        lastContinuation
    }

    // Gecko: layout/generic/nsSplittableFrame.cpp nsSplittableFrame::LastInFlow
    pub fn LastInFlow(&self) -> nsIFrame {
        let mut lastInFlow = *self;
        while let Some(next) = lastInFlow.GetNextInFlow() {
            lastInFlow = next;
        }
        lastInFlow
    }

    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_WITHOUT_DTOR(IBSplitSibling, nsContainerFrame)
    pub fn IBSplitSibling() -> FramePropertyDescriptor<nsIFrame> {
        FramePropertyDescriptor::new(kIBSplitSiblingPropertyId)
    }
    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_WITHOUT_DTOR(IBSplitPrevSibling, nsContainerFrame)
    pub fn IBSplitPrevSibling() -> FramePropertyDescriptor<nsIFrame> {
        FramePropertyDescriptor::new(kIBSplitPrevSiblingPropertyId)
    }

    // Gecko: layout/generic/nsIFrame.h NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE(BidiDataProperty, mozilla::FrameBidiData)
    pub fn BidiDataProperty() -> FramePropertyDescriptor<FrameBidiData> {
        FramePropertyDescriptor::new(kBidiDataPropertyId)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetBidiData
    pub fn GetBidiData(&self) -> FrameBidiData {
        match self.GetProperty(nsIFrame::BidiDataProperty()) {
            Some(bidiData) => bidiData,
            None => FrameBidiData { precedingControl: kBidiLevelNone, ..Default::default() },
        }
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetBaseLevel
    pub fn GetBaseLevel(&self) -> BidiEmbeddingLevel {
        self.GetBidiData().baseLevel
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetEmbeddingLevel
    pub fn GetEmbeddingLevel(&self) -> BidiEmbeddingLevel {
        self.GetBidiData().embeddingLevel
    }

    // Gecko: layout/generic/nsIFrame.h CLASS_FLAG_METHOD(IsBidiInlineContainer, BidiInlineContainer)
    // (the class flag is set by nsInlineFrame and its subclass nsFirstLineFrame, nsFirstLetterFrame and
    // the ruby frames)
    pub fn IsBidiInlineContainer(&self) -> bool {
        matches!(
            self.Type(),
            LayoutFrameType::Inline | LayoutFrameType::Line | LayoutFrameType::Letter | LayoutFrameType::Ruby | LayoutFrameType::RubyBase | LayoutFrameType::RubyBaseContainer | LayoutFrameType::RubyText | LayoutFrameType::RubyTextContainer
        )
    }

    // Gecko: layout/generic/nsIFrameInlines.h nsIFrame::IsInlineOutside
    pub fn IsInlineOutside(&self) -> bool {
        self.StyleDisplay().IsInlineOutside(*self)
    }

    /**
     * Return true if aFrame is in an {ib} split and is NOT one of the
     * continuations of the first inline in it.
     */
    // Gecko: layout/generic/nsIFrame.h nsIFrame::FrameIsNonFirstInIBSplit
    pub fn FrameIsNonFirstInIBSplit(&self) -> bool {
        self.HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT) && self.FirstContinuation().GetProperty(nsIFrame::IBSplitPrevSibling()).is_some()
    }

    /**
     * Return true if aFrame is in an {ib} split and is NOT one of the
     * continuations of the last inline in it.
     */
    // Gecko: layout/generic/nsIFrame.h nsIFrame::FrameIsNonLastInIBSplit
    pub fn FrameIsNonLastInIBSplit(&self) -> bool {
        self.HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT) && self.FirstContinuation().GetProperty(nsIFrame::IBSplitSibling()).is_some()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::CanContinueTextRun (virtual; text frames and inline
    // containers return true, nsIFrame false)
    pub fn CanContinueTextRun(&self) -> bool {
        matches!(self.Type(), LayoutFrameType::Text | LayoutFrameType::Inline | LayoutFrameType::Line | LayoutFrameType::Letter)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalPosition(WritingMode, const nsSize&)
    pub fn GetLogicalPosition__WritingMode_nsSize(&self, aWritingMode: WritingMode, aContainerSize: nsSize) -> LogicalPoint {
        self.GetLogicalRect__WritingMode_nsSize(aWritingMode, aContainerSize).Origin(aWritingMode)
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::MovePositionBy(const nsPoint&)
    // NOT PORTED: the relative-positioning offsets (ReflowInput::ApplyRelativePositioning); see
    // GetNormalPosition, there are none.
    pub fn MovePositionBy(&self, aTranslation: nsPoint) {
        let position = self.GetNormalPosition() + aTranslation;
        self.SetPosition__nsPoint(position);
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::HasColumnSpanSiblings
    // NOT PORTED: column-span siblings (columns); the bit is never set.
    pub fn HasColumnSpanSiblings(&self) -> bool {
        false
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::HasAbsolutelyPositionedChildren
    pub fn HasAbsolutelyPositionedChildren(&self) -> bool {
        match self.GetAbsoluteContainingBlock() {
            Some(absCB) => absCB.HasAbsoluteFrames(),
            None => false,
        }
    }

    /**
     * Accessors for the absolute containing block.
     */
    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsAbsoluteContainer
    pub fn IsAbsoluteContainer(&self) -> bool {
        self.HasAnyStateBits(NS_FRAME_HAS_ABSPOS_CHILDREN)
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetAbsoluteContainingBlock
    // ADAPTER: the containing block is a Box owned by the frame property (Gecko's deletable
    // property); the reference lives as long as the property, like the float lists of
    // nsBlockFrame.
    pub fn GetAbsoluteContainingBlock(&self) -> Option<&'static mut AbsoluteContainingBlock> {
        if !self.IsAbsoluteContainer() {
            return None;
        }
        let absCB = self.GetProperty(AbsoluteContainingBlockProperty());
        NS_ASSERTION!(absCB.is_some(), "The frame is marked as an abspos container but doesn't have the property");
        absCB.map(|p| unsafe { &mut *p })
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::MarkAsAbsoluteContainingBlock
    pub fn MarkAsAbsoluteContainingBlock(&self) {
        MOZ_ASSERT!(self.HasAnyStateBits(NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN));
        NS_ASSERTION!(self.GetProperty(AbsoluteContainingBlockProperty()).is_none(), "Already has an abs-pos containing block property?");
        NS_ASSERTION!(!self.HasAnyStateBits(NS_FRAME_HAS_ABSPOS_CHILDREN), "Already has NS_FRAME_HAS_ABSPOS_CHILDREN state bit?");
        self.AddStateBits(NS_FRAME_HAS_ABSPOS_CHILDREN);
        self.SetProperty(AbsoluteContainingBlockProperty(), Box::into_raw(Box::new(AbsoluteContainingBlock::default())));
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::MarkAsNotAbsoluteContainingBlock
    pub fn MarkAsNotAbsoluteContainingBlock(&self) {
        NS_ASSERTION!(!self.HasAbsolutelyPositionedChildren(), "Think of the children!");
        NS_ASSERTION!(self.GetProperty(AbsoluteContainingBlockProperty()).is_some(), "Should have an abs-pos containing block property");
        NS_ASSERTION!(self.HasAnyStateBits(NS_FRAME_HAS_ABSPOS_CHILDREN), "Should have NS_FRAME_HAS_ABSPOS_CHILDREN state bit");
        MOZ_ASSERT!(self.HasAnyStateBits(NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN));
        self.RemoveStateBits(NS_FRAME_HAS_ABSPOS_CHILDREN);
        // (the deletable property's destructor)
        if let Some(p) = self.GetProperty(AbsoluteContainingBlockProperty()) {
            drop(unsafe { Box::from_raw(p) });
        }
        self.RemoveProperty(AbsoluteContainingBlockProperty());
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetChildList (the base class's: only the
    // absolute lists)
    pub fn nsIFrame_GetChildList(&self, aListID: FrameChildListID) -> &'static nsFrameList {
        if self.IsAbsoluteContainer() {
            if aListID == FrameChildListID::Absolute {
                return &self.GetAbsoluteContainingBlock().unwrap().mAbsoluteFrames;
            }
            if aListID == FrameChildListID::PushedAbsolute {
                return &self.GetAbsoluteContainingBlock().unwrap().mPushedAbsoluteFrames;
            }
        }
        nsFrameList::EmptyList()
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetChildLists (the base class's)
    pub fn nsIFrame_GetChildLists(&self, aLists: &mut Vec<FrameChildList>) {
        if let Some(absCB) = self.GetAbsoluteContainingBlock() {
            let absoluteList = absCB.GetChildList();
            absoluteList.AppendIfNonempty(aLists, FrameChildListID::Absolute);
            let pushedAbsoluteList = absCB.GetPushedChildList();
            pushedAbsoluteList.AppendIfNonempty(aLists, FrameChildListID::PushedAbsolute);
        }
    }

    // Gecko: layout/generic/nsIFrame.cpp OffsetCalculator
    // (the template over the position getter; GetPosition and GetPositionIgnoringScrolling are the
    // same here, there are no scroll containers)
    fn OffsetCalculator(aThis: nsIFrame, aOther: nsIFrame) -> nsPoint {
        let mut offset = nsPoint::new(0, 0);
        let mut f = Some(aThis);
        while let Some(frame) = f {
            if frame == aOther {
                break;
            }
            offset += frame.GetPosition();
            f = frame.GetParent();
        }

        if f != Some(aOther) {
            // Looks like aOther wasn't an ancestor of |this|.  So now we have
            // the root-frame-relative position of |this| in |offset|.  Convert back
            // to the coordinates of aOther
            let mut other = Some(aOther);
            while let Some(o) = other {
                offset -= o.GetPosition();
                other = o.GetParent();
            }
        }

        offset
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetOffsetTo
    pub fn GetOffsetTo(&self, aOther: nsIFrame) -> nsPoint {
        nsIFrame::OffsetCalculator(*self, aOther)
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetOffsetToIgnoringScrolling
    // (GetPositionIgnoringScrolling is GetPosition: no frame is a scroll container's scrolled frame)
    pub fn GetOffsetToIgnoringScrolling(&self, aOther: nsIFrame) -> nsPoint {
        nsIFrame::OffsetCalculator(*self, aOther)
    }

    // Gecko: layout/generic/nsIFrameInlines.h nsIFrame::IsAbsPosContainingBlock
    // (Style()->IsAbsPosContainingBlock(this), layout/style/ComputedStyleInlines.h)
    pub fn IsAbsPosContainingBlock(&self) -> bool {
        if self.IsFixedPosContainingBlock() {
            return true;
        }
        // NOTE: Any CSS properties that influence the output of this function
        // should also handle will-change appropriately.
        self.StyleDisplay().IsPositionedStyle() && !self.IsInSVGTextSubtree()
    }

    // Gecko: layout/generic/nsIFrameInlines.h nsIFrame::IsFixedPosContainingBlock
    // (Style()->IsFixedPosContainingBlock(this), layout/style/ComputedStyleInlines.h)
    pub fn IsFixedPosContainingBlock(&self) -> bool {
        // NOTE: Any CSS properties that influence the output of this function
        // should also handle will-change appropriately.
        if self.IsInSVGTextSubtree() {
            return false;
        }
        if self.IsFixedPosContainingBlockForNonSVGTextFrames() {
            return true;
        }
        let disp = self.StyleDisplay();
        if disp.IsFixedPosContainingBlockForContainLayoutAndPaintSupportingFrames() && self.SupportsContainLayoutAndPaint() {
            return true;
        }
        if disp.IsFixedPosContainingBlockForTransformSupportingFrames() && self.SupportsCSSTransforms() {
            return true;
        }
        false
    }

    // Gecko: layout/style/ComputedStyleInlines.h ComputedStyle::IsFixedPosContainingBlockForNonSVGTextFrames
    // (a method of the frame here: IsRootElementStyle is answered by the frame)
    pub fn IsFixedPosContainingBlockForNonSVGTextFrames(&self) -> bool {
        // NOTE: Any CSS properties that influence the output of this function
        // should return FIXPOS_CB_NON_SVG for will-change.
        if self.IsRootElementStyle() {
            return false;
        }

        let disp = self.StyleDisplay();
        if disp.mWillChangeBits().contains(StyleWillChangeBits::FIXPOS_CB_NON_SVG) {
            return true;
        }

        disp.HasFiltersOrBackdropFilters()
    }

    // Gecko: layout/generic/nsIFrame.h CLASS_FLAG_METHOD0(SupportsCSSTransforms)
    // (layout/generic/FrameClasses.py: every class but the inline and ruby frames (INLINE,
    // RUBY_CONTENT) and nsTableFrame (TABLE))
    pub fn SupportsCSSTransforms(&self) -> bool {
        !matches!(self.Type(), LayoutFrameType::Inline | LayoutFrameType::Line | LayoutFrameType::Table)
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::IsTransformed (IsCSSTransformed: a transform style
    // on a frame that supports transforms)
    // ADAPTER: NS_FRAME_MAY_BE_TRANSFORMED is the style itself; no transform animations, no SVG
    // transforms.
    pub fn IsTransformed(&self) -> bool {
        self.StyleDisplay().HasTransformStyle() && self.SupportsCSSTransforms()
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::Combines3DTransformWithAncestors
    // NOT PORTED: 3D rendering contexts (transform-style: preserve-3d): every transformed frame is
    // flattened into its parent's plane, as Gecko does for a frame whose parent does not extend a 3D
    // context.
    pub fn Combines3DTransformWithAncestors(&self) -> bool {
        false
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetTransformMatrix (ViewportType::Layout, no
    // flags): the matrix from this frame's coordinates (device pixels) to those of the ancestor it
    // returns: its parent when it is transformed (its transform, its perspective, then its
    // position), else the nearest ancestor that is transformed or is `aStopAtAncestor` (a
    // translation by the offset to it). The ancestor is None at the root (the identity then).
    // ADAPTER: one document: no cross-document parent; no zoomed content root.
    pub fn GetTransformMatrix(&self, aStopAtAncestor: nsIFrame) -> (Matrix4x4, Option<nsIFrame>) {
        /* If we're transformed, we want to hand back the combination
         * transform/translate matrix that will apply our current transform, then
         * shift us to our parent.
         */
        let isTransformed = self.IsTransformed();
        let scaleFactor = AppUnitsPerCSSPixel() as f32;

        if isTransformed {
            MOZ_ASSERT!(self.GetParent().is_some());
            /* Compute the delta to the parent, which we need because we are converting
             * coordinates to our parent.
             */
            let mut result = nsDisplayTransform::GetResultingTransformMatrix(*self, nsPoint::new(0, 0), scaleFactor, nsDisplayTransform::INCLUDE_PERSPECTIVE);

            let aOutAncestor = self.GetParent();
            let delta = self.GetPosition();
            /* Combine the raw transform with a translation to our parent. */
            result.PostTranslate(NSAppUnitsToFloatPixels(delta.x, scaleFactor), NSAppUnitsToFloatPixels(delta.y, scaleFactor), 0.0);

            return (result, aOutAncestor);
        }

        // We are not transformed, so the returned transform is just going to be a
        // translation up to whatever ancestor we decide to stop at.
        let mut aOutAncestor = self.GetParent();

        /* Otherwise, we're not transformed.  In that case, we'll walk up the frame
         * tree until we either hit the root frame or something that may be
         * transformed.  We'll then change coordinates into that frame, since we're
         * guaranteed that nothing in-between can be transformed.  First, however,
         * we have to check to see if we have a parent.  If not, we'll set the
         * outparam to null (indicating that there's nothing left) and will hand back
         * the identity matrix.
         */
        let Some(mut ancestor) = aOutAncestor else {
            return (Matrix4x4::default(), None);
        };

        /* Keep iterating while the frame can't possibly be transformed. */
        let mut offset = self.GetPosition();
        while ancestor != aStopAtAncestor && !ancestor.IsTransformed() {
            offset += ancestor.GetPosition();

            // (at the root, the root stays the ancestor)
            let Some(parent) = ancestor.GetParent() else { break };
            ancestor = parent;
            aOutAncestor = Some(ancestor);
        }

        let mut result = Matrix4x4::default();
        result.PostTranslate(NSAppUnitsToFloatPixels(offset.x, scaleFactor), NSAppUnitsToFloatPixels(offset.y, scaleFactor), 0.0);
        (result, aOutAncestor)
    }

    // Gecko: layout/generic/nsIFrame.h CLASS_FLAG_METHOD0(SupportsContainLayoutAndPaint)
    // (layout/generic/FrameClasses.py: every class but the inline and ruby frames and the table
    // parts other than the cell)
    pub fn SupportsContainLayoutAndPaint(&self) -> bool {
        !matches!(
            self.Type(),
            LayoutFrameType::Inline | LayoutFrameType::Line | LayoutFrameType::TableRow | LayoutFrameType::TableRowGroup | LayoutFrameType::TableCol | LayoutFrameType::TableColGroup
        )
    }
}

// Gecko: layout/generic/nsIFrame.cpp AbsoluteContainingBlockProperty
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(AbsoluteContainingBlockProperty, *mut AbsoluteContainingBlock, 8000);
