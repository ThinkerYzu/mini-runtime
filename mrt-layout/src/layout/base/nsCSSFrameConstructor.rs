/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/*
 * construction of a frame tree that is nearly isomorphic to the content
 * tree and updating of that tree in response to dynamic changes
 */
// Translated from layout/base/nsCSSFrameConstructor.h and nsCSSFrameConstructor.cpp (GECKO_REV), REDUCED: it
// builds the frames of the initial document, nothing dynamic. Only what block, inline, text, list-item
// and float frames need is translated, in the order of the C++ file (class members first, as in the
// header; then the .cpp).
//
// What is translated:
//   - nsFrameConstructorState with the float containing block scope (nsFrameConstructorSaveState,
//     AbsoluteFrameList), AddChild, ProcessFrameInsertions
//   - ConstructDocElementFrame, ConstructRootFrame, SetUpDocElementContainingBlock (viewport > canvas > root frame)
//   - FrameConstructionData / FrameConstructionItem / FrameConstructionItemList and their iteration
//   - AddFrameConstructionItems(Internal), FindDataForContent, FindElementData, FindTextData, FindHTMLData,
//     FindDisplayData, ShouldCreateItemsForChild, display:none and display:contents
//   - the table section: ConstructTable, ConstructTableRowOrRowGroup, ConstructTableCol, ConstructTableCell,
//     the pseudo parents (sPseudoParentData, CreateNeededPseudoContainers, WrapItemsInPseudoParent), and the
//     anonymous box styles they need
//   - ConstructFramesFromItem(List), ConstructFrameFromItemInternal, ProcessChildren, InitAndRestoreFrame,
//     ConstructTextFrame (with the whitespace-only text rules: AtLineBoundary, IsWhitespace)
//   - ConstructNonScrollableBlock, ConstructBlock, ConstructInline with the {ib} split (CreateIBSiblings,
//     SetFrameIsIBSplit, MoveChildrenTo, BuildInlineChildItems)
//   - the outside and inside ::marker of a list item, CreatePlaceholderFrameFor, GetGeometricParent,
//     GetFloatContainingBlock, CreateContinuingFrame
//
// ADAPTERS (marked `ADAPTER:` where they are):
//   - `nsIContent*` is a ContentId: a node id of the arena DOM (mrt-dom), or a generated-content id
//     (the ::marker container and its text). The frame's own nsIContent is the handle MakeContent gives:
//     the node id itself for a DOM node, a synthetic content for generated content (see nsIContent).
//   - Frames are allocated by the frame tree (FrameTree::AddFrame); a FrameCreationFunc gives the
//     fresh frame its class (the NS_New*Frame functions).
//   - Styles come from mrt-style (`StyleDom`): the element's computed style; the style of text and of
//     other anonymous boxes is made by StyleDom::anonymous_style and cached per parent style.
//   - Lists of items are Vecs and iterators are indexes; RAII save states are restored explicitly
//     (`Restore`, marked `~nsFrameConstructorSaveState`).
//
// NOT PORTED (each stands where it would be, with the reason):
//   - the table pseudo-frame wrapping of the Ruby kind (ruby, ruby bases and texts), MathML tables,
//     scrollable table cells, table parts as absolute containing blocks, and the page breaks and
//     pagination of table parts
//   - flex and grid containers are real (nsFlexContainerFrame, nsGridContainerFrame), without scroll frames,
//     legacy -webkit-box and abspos children
//   - absolutely positioned and fixed positioned boxes: no frames at all (no placeholder, no frame)
//   - replaced elements and form controls (img, input, textarea, select, video, audio, iframe,
//     progress, meter, svg): a block or inline-block frame with no children, sized by its CSS
//   - scroll frames (overflow other than visible gets a plain block), columns, ruby, MathML, XUL, SVG
//   - ::before, ::after, ::backdrop (mrt-style computes no pseudo-element styles), ::first-letter,
//     ::first-line, anonymous content creators, fieldset legends, counters and quotes, page breaks
//   - dynamic changes (ContentInserted, ContentRemoved, restyle, reframing), lazy frame construction,
//     frame state restoration, accessibility, paginated documents
use crate::nsLineLayoutAdapter::*;
use crate::*;
use mrt_style::mrt_dom::{Kind, NodeId, NONE};
use mrt_style::style::properties::ComputedValues;
use mrt_style::style::servo_arc::Arc as ServoArc;
use mrt_style::style::values::specified::box_::Display as StyleDisplayValue;
use mrt_style::style::values::computed::Image;
use mrt_style::style::values::generics::counters::ContentItem;
use mrt_style::StyleDom;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

// ADAPTER: Gecko's `nsIContent*` while frames are constructed. A node id of the arena DOM, or, from
// kGeneratedContentBase up, an index into the constructor's generated content (the ::marker
// container and its text node). 0 is "no content".
pub type ContentId = u32;
pub const kGeneratedContentBase: ContentId = 1 << 31;

// ADAPTER: namespace ids of mrt-dom (the ones the frame constructor looks at).
const kNameSpaceID_XHTML: u8 = 3;
const kNameSpaceID_MathML: u8 = 5;
const kNameSpaceID_SVG: u8 = 6;

// Gecko: layout/base/nsCSSFrameConstructor.cpp MAX_REFLOW_DEPTH (nsIFrame.h)
const MAX_REFLOW_DEPTH: u32 = 200;

// ADAPTER: ComputedStyle is Stylo's ComputedValues.
pub type ComputedStyle = ComputedValues;

//----------------------------------------------------------------------

// Block/inline frame construction logic. We maintain a few invariants here:
//
// 1. Block frames contain block and inline frames.
//
// 2. Inline frames only contain inline frames. If an inline parent has a block
// child then the block child is migrated upward until it lands in a block
// parent (the inline frames containing block is where it will end up).

// Gecko: layout/base/nsCSSFrameConstructor.cpp SetInitialSingleChild
pub fn SetInitialSingleChild(aParent: nsIFrame, aFrame: nsIFrame) {
    MOZ_ASSERT!(aFrame.GetNextSibling().is_none(), "Should be using a frame list");
    nsContainerFrame::from_frame(aParent).SetInitialChildList(FrameChildListID::Principal, nsFrameList::new(Some(aFrame), Some(aFrame)));
}

//----------------------------------------------------------------------
//
// When inline frames get weird and have block frames in them, we
// annotate them to help us respond to incremental content changes
// more easily.

// Gecko: layout/base/nsCSSFrameConstructor.cpp IsFramePartOfIBSplit
pub fn IsFramePartOfIBSplit(aFrame: nsIFrame) -> bool {
    let result = aFrame.HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT);
    MOZ_ASSERT!(!result || aFrame.IsBlockFrameOrSubclass() || aFrame.IsInlineFrame(), "only block/inline frames can have NS_FRAME_PART_OF_IBSPLIT");
    result
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp SetFrameIsIBSplit
pub fn SetFrameIsIBSplit(aFrame: nsIFrame, aIBSplitSibling: Option<nsIFrame>) {
    // We should be the only continuation
    NS_ASSERTION!(aFrame.GetPrevContinuation().is_none(), "assigning ib-split sibling to other than first continuation!");
    NS_ASSERTION!(aFrame.GetNextContinuation().map_or(true, IsFramePartOfIBSplit), "should have no non-ib-split continuations here");

    // Mark the frame as ib-split.
    aFrame.AddStateBits(NS_FRAME_PART_OF_IBSPLIT);

    if let Some(aIBSplitSibling) = aIBSplitSibling {
        NS_ASSERTION!(aIBSplitSibling.GetPrevContinuation().is_none(), "assigning something other than the first continuation as the ib-split sibling");

        // Store the ib-split sibling (if we were given one) with the
        // first frame in the flow.
        aFrame.SetProperty(nsIFrame::IBSplitSibling(), aIBSplitSibling);
        aIBSplitSibling.SetProperty(nsIFrame::IBSplitPrevSibling(), aFrame);
    }
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp ShouldSuppressFloatingOfDescendants
// (Return true if aFrame prevents its descendants from floating.)
// (MathML: NOT PORTED)
fn ShouldSuppressFloatingOfDescendants(aFrame: nsIFrame) -> bool {
    aFrame.IsFlexOrGridContainer() || aFrame.IsMathMLFrame()
}

/**
 * Moves aFrameList from aOldParent to aNewParent.  This updates the parent
 * pointer of the frames in the list, and reparents their views as needed.
 * nsIFrame::SetParent sets the NS_FRAME_HAS_VIEW bit on aNewParent and its
 * ancestors as needed. Then it sets the list as the initial child list
 * on aNewParent, unless aNewParent either already has kids or has been
 * reflowed; in that case it appends the new frames.  Note that this
 * method differs from ReparentFrames in that it doesn't change the kids'
 * style.
 */
// Gecko: layout/base/nsCSSFrameConstructor.cpp MoveChildrenTo
fn MoveChildrenTo(aOldParent: nsIFrame, aNewParent: nsIFrame, aFrameList: nsFrameList) {
    aFrameList.ApplySetParent(aNewParent);

    if nsContainerFrame::from_frame(aNewParent).mFrames().IsEmpty() && aNewParent.HasAnyStateBits(NS_FRAME_FIRST_REFLOW) {
        nsContainerFrame::from_frame(aNewParent).SetInitialChildList(FrameChildListID::Principal, aFrameList);
    } else {
        nsContainerFrame::from_frame(aNewParent).AppendFrames(FrameChildListID::Principal, aFrameList);
    }
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp GetFloatContainingBlock
// (nsCSSFrameConstructor::GetFloatContainingBlock; a static function of the class)
pub fn nsCSSFrameConstructor_GetFloatContainingBlock(aFrame: Option<nsIFrame>) -> Option<nsIFrame> {
    // Starting with aFrame, look for a frame that is a float containing block.
    // If we hit a frame which prevents its descendants from floating, bail out.
    // The logic here needs to match the logic in MaybePushFloatContainingBlock().
    let mut containingBlock = aFrame;
    while let Some(cb) = containingBlock {
        if ShouldSuppressFloatingOfDescendants(cb) {
            break;
        }
        if cb.IsFloatContainingBlock() {
            return Some(cb);
        }
        containingBlock = cb.GetParent();
    }

    // If we didn't find a containing block, then there just isn't
    // one.... return null
    None
}

//----------------------------------------------------------------------

// Structure used when constructing formatting object trees. Contains
// state information needed for absolutely positioned elements
// Gecko: layout/base/nsCSSFrameConstructor.cpp mozilla::AbsoluteFrameList
// (Rust has no inheritance from nsFrameList: it holds the list and Derefs to it.)
#[derive(Default)]
pub struct AbsoluteFrameList {
    pub mList: nsFrameList,
    // Containing block for absolutely positioned elements.
    pub mContainingBlock: Option<nsIFrame>,
}

impl AbsoluteFrameList {
    // Gecko: AbsoluteFrameList::AbsoluteFrameList(nsContainerFrame*)
    pub fn new(aContainingBlock: Option<nsIFrame>) -> AbsoluteFrameList {
        AbsoluteFrameList { mList: nsFrameList::default(), mContainingBlock: aContainingBlock }
    }
}

impl Deref for AbsoluteFrameList {
    type Target = nsFrameList;
    fn deref(&self) -> &nsFrameList {
        &self.mList
    }
}

impl DerefMut for AbsoluteFrameList {
    fn deref_mut(&mut self) -> &mut nsFrameList {
        &mut self.mList
    }
}

// -----------------------------------------------------------

// Structure for saving the existing state when pushing/poping containing
// blocks. The destructor restores the state to its previous state
// Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorSaveState
// ADAPTER: the destructor is `Restore`, which the code that made the scope calls where the C++ scope
// ends. `mList` names which list of the state the scope saved (Gecko keeps a pointer to it), and
// the saved list is boxed so that mFixedList can point into it (FixedListRef::Saved) while the
// scope is open, as Gecko's mFixedList may point to aSaveState.mSavedList.
#[derive(Default)]
pub struct nsFrameConstructorSaveState {
    mList: Option<nsFrameConstructorSaveState_List>,
    // Copy of original frame list. This can be the original absolute list or a
    // float list.
    mSavedList: Box<AbsoluteFrameList>,
    // The name of the child list in which our frames would belong.
    mChildListID: Option<FrameChildListID>,
    // The fixed list of the state before the scope (only for the absolute list).
    mSavedFixedList: FixedListRef,
}

// ADAPTER: which list of nsFrameConstructorState a save state holds (Gecko's `mList` pointer).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum nsFrameConstructorSaveState_List {
    Floated,
    Absolute,
}

// ADAPTER: Gecko's `AbsoluteFrameList* mFixedList`, which points to one of the state's lists or to the
// saved list of an open save state (see PushAbsoluteContainingBlock).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FixedListRef {
    Absolute,
    #[default]
    AncestorFixed,
    RealFixed,
    Saved(*mut AbsoluteFrameList),
}

impl nsFrameConstructorSaveState {
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorSaveState::~nsFrameConstructorSaveState
    pub fn Restore(&mut self, aState: &mut nsFrameConstructorState) {
        // Restore the state
        if let Some(list) = self.mList.take() {
            match list {
                nsFrameConstructorSaveState_List::Absolute => {
                    let mut current = std::mem::take(&mut aState.mAbsoluteList);
                    aState.ProcessFrameInsertions(&mut current, self.mChildListID.unwrap());
                    aState.mAbsoluteList = std::mem::take(&mut *self.mSavedList);
                    aState.mFixedList = self.mSavedFixedList;
                }
                nsFrameConstructorSaveState_List::Floated => {
                    let mut current = std::mem::take(&mut aState.mFloatedList);
                    aState.ProcessFrameInsertions(&mut current, self.mChildListID.unwrap());
                    aState.mFloatedList = std::mem::take(&mut *self.mSavedList);
                }
            }

            MOZ_ASSERT!(self.mSavedList.IsEmpty(), "Frames in mSavedList should've moved back into mState!");
        }
    }
}

// Structure used for maintaining state information during the
// frame construction process
// Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorState
pub struct nsFrameConstructorState {
    // Containing block information for out-of-flow frames.
    //
    // Floats are easy. Whatever is our float CB.
    //
    // Regular abspos elements are easy too. Its containing block can be the
    // nearest abspos element, or the ICB (the canvas frame).
    //
    // Top layer abspos elements are always children of the ICB, but we can get
    // away with having two different lists (mAbsoluteList and
    // mTopLayerAbsoluteList), because because top layer frames cause
    // non-top-layer frames to be contained inside (so any descendants of a top
    // layer abspos can never share containing block with it, unless they're also
    // in the top layer).
    //
    // Regular fixed elements however are trickier. Fixed elements can be
    // contained in one of three lists:
    //
    //  * mAbsoluteList, if our abspos cb is also a fixpos cb (e.g., is
    //                   transformed or has a filter).
    //
    //  * mAncestorFixedList, if the fixpos cb is an ancestor element other than
    //                        the viewport frame, (so, a transformed / filtered
    //                        ancestor).
    //
    //  * mRealFixedList, which is also the fixed list used for the top layer
    //                    fixed items, which is the fixed list of the viewport
    //                    frame.
    //
    // It is important that mRealFixedList is shared between regular and top layer
    // fixpos elements, since no-top-layer descendants of top layer fixed elements
    // could share ICB and vice versa, so without that there would be no guarantee
    // of layout ordering between them.
    // (mTopLayerAbsoluteList: NOT PORTED, the top layer: Stylo's servo configuration has no
    // -moz-top-layer, so no box is in it.)
    pub mFloatedList: AbsoluteFrameList,
    pub mAbsoluteList: AbsoluteFrameList,
    pub mAncestorFixedList: AbsoluteFrameList,
    pub mRealFixedList: AbsoluteFrameList,

    // Never null, always pointing to one of the lists documented above.
    pub mFixedList: FixedListRef,

    // These bits will be added to the state bits of any frame we construct
    // using this state.
    pub mAdditionalStateBits: nsFrameState,

    // If false (which is the default) then call SetPrimaryFrame() as needed
    // during frame construction.  If true, don't make any SetPrimaryFrame()
    // calls, except for generated content which doesn't have a primary frame
    // yet.  The mCreatingExtraFrames == true mode is meant to be used for
    // construction of random "extra" frames for elements via normal frame
    // construction APIs (e.g. replication of things across pages in paginated
    // mode).
    pub mCreatingExtraFrames: bool,

    // This keeps track of whether we have found a "rendered legend" for
    // the current FieldSetFrame.
    pub mHasRenderedLegend: bool,
}

impl nsFrameConstructorState {
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorState::nsFrameConstructorState
    // (mPresContext, mPresShell, mFrameConstructor and the history state: NOT PORTED; the root frame,
    // the containing block of mRealFixedList, is passed in, as the frame constructor is not
    // reachable from here)
    pub fn new(aFixedContainingBlock: Option<nsIFrame>, aAbsoluteContainingBlock: Option<nsIFrame>, aFloatContainingBlock: Option<nsIFrame>, aRootFrame: Option<nsIFrame>) -> nsFrameConstructorState {
        let mut state = nsFrameConstructorState {
            mFloatedList: AbsoluteFrameList::new(aFloatContainingBlock),
            mAbsoluteList: AbsoluteFrameList::new(aAbsoluteContainingBlock),
            mAncestorFixedList: AbsoluteFrameList::new(aFixedContainingBlock),
            mRealFixedList: AbsoluteFrameList::new(aRootFrame),
            mFixedList: FixedListRef::AncestorFixed,
            mAdditionalStateBits: 0,
            mCreatingExtraFrames: false,
            mHasRenderedLegend: false,
        };
        state.mFixedList = if aFixedContainingBlock == aAbsoluteContainingBlock {
            FixedListRef::Absolute
        } else if aAbsoluteContainingBlock == state.mRealFixedList.mContainingBlock {
            FixedListRef::RealFixed
        } else {
            FixedListRef::AncestorFixed
        };
        state
    }

    // ADAPTER: `*mFixedList`.
    fn FixedList(&mut self) -> &mut AbsoluteFrameList {
        match self.mFixedList {
            FixedListRef::Absolute => &mut self.mAbsoluteList,
            FixedListRef::AncestorFixed => &mut self.mAncestorFixedList,
            FixedListRef::RealFixed => &mut self.mRealFixedList,
            FixedListRef::Saved(p) => unsafe { &mut *p },
        }
    }

    // Process the frame insertions for all the out-of-flow nsAbsoluteItems.
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorState::ProcessFrameInsertionsForAllLists
    // (called where Gecko runs ~nsFrameConstructorState)
    pub fn ProcessFrameInsertionsForAllLists(&mut self) {
        let mut list = std::mem::take(&mut self.mFloatedList);
        self.ProcessFrameInsertions(&mut list, FrameChildListID::Float);
        self.mFloatedList = list;
        let mut list = std::mem::take(&mut self.mAbsoluteList);
        self.ProcessFrameInsertions(&mut list, FrameChildListID::Absolute);
        self.mAbsoluteList = list;
        // (mTopLayerAbsoluteList: NOT PORTED, see above.)
        let mut list = std::mem::take(self.FixedList());
        self.ProcessFrameInsertions(&mut list, FrameChildListID::Absolute);
        *self.FixedList() = list;
        let mut list = std::mem::take(&mut self.mRealFixedList);
        self.ProcessFrameInsertions(&mut list, FrameChildListID::Absolute);
        self.mRealFixedList = list;
    }

    // Function to push the existing absolute containing block state and
    // create a new scope. Code that uses this function should get matching
    // logic in GetAbsoluteContainingBlock.
    // Also makes aNewAbsoluteContainingBlock the containing block for
    // fixed-pos elements if necessary.
    // aPositionedFrame is the frame whose style actually makes
    // aNewAbsoluteContainingBlock a containing block. E.g. for a scrollable
    // element aPositionedFrame is the element's primary frame and
    // aNewAbsoluteContainingBlock is the scrolled frame.
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorState::PushAbsoluteContainingBlock
    // NOT PORTED: the top layer branch (mTopLayer == Auto; no box is in the top layer here).
    pub fn PushAbsoluteContainingBlock(&mut self, aNewAbsoluteContainingBlock: Option<nsIFrame>, aPositionedFrame: Option<nsIFrame>, aSaveState: &mut nsFrameConstructorSaveState) {
        MOZ_ASSERT!(aNewAbsoluteContainingBlock.is_some() == aPositionedFrame.is_some(), "We should have both or none");
        aSaveState.mList = Some(nsFrameConstructorSaveState_List::Absolute);
        aSaveState.mChildListID = Some(FrameChildListID::Absolute);
        *aSaveState.mSavedList = std::mem::take(&mut self.mAbsoluteList);
        aSaveState.mSavedFixedList = self.mFixedList;
        self.mAbsoluteList = AbsoluteFrameList::new(aNewAbsoluteContainingBlock);
        self.mFixedList = (|| {
            if aPositionedFrame.map_or(true, |f| f.IsFixedPosContainingBlock()) {
                // See if we need to treat abspos and fixedpos the same. This happens if
                // we're a transformed/filtered/etc element, or if we force a null abspos
                // containing block (for mathml for example).
                return FixedListRef::Absolute;
            }
            // if (aPositionedFrame->StyleDisplay()->mTopLayer == StyleTopLayer::Auto) {...}
            // NOT PORTED: the top layer.
            if self.mFixedList == FixedListRef::Absolute {
                // If we were pointing to our old absolute list, keep pointing to it.
                return FixedListRef::Saved(&mut *aSaveState.mSavedList as *mut AbsoluteFrameList);
            }
            // Otherwise keep pointing to the current thing (another ancestor's
            // absolute list, or the real fixed list, doesn't matter).
            self.mFixedList
        })();

        if let Some(cb) = aNewAbsoluteContainingBlock {
            if !cb.IsAbsoluteContainer() {
                cb.MarkAsAbsoluteContainingBlock();
            }
        }
    }

    // Function to forbid floats descendants under aFloatCBCandidate, or open a
    // new float containing block scope for aFloatCBCandidate. The current
    // state is saved in aSaveState if a new scope is pushed.
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorState::MaybePushFloatContainingBlock
    pub fn MaybePushFloatContainingBlock(&mut self, aFloatCBCandidate: nsIFrame, aSaveState: &mut nsFrameConstructorSaveState) {
        // The logic here needs to match the logic in GetFloatContainingBlock().
        if ShouldSuppressFloatingOfDescendants(aFloatCBCandidate) {
            // Pushing a null float containing block forbids any frames from being
            // floated until a new float containing block is pushed. See implementation
            // of nsFrameConstructorState::AddChild().
            //
            // XXX we should get rid of null float containing blocks and teach the
            // various frame classes to deal with floats instead.
            self.PushFloatContainingBlock(None, aSaveState);
        } else if aFloatCBCandidate.IsFloatContainingBlock() {
            self.PushFloatContainingBlock(Some(aFloatCBCandidate), aSaveState);
        }
        // #ifdef DEBUG: mFloatCBCandidate. NOT PORTED: debug-only.
    }

    // Helper function for MaybePushFloatContainingBlock().
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorState::PushFloatContainingBlock
    pub fn PushFloatContainingBlock(&mut self, aNewFloatContainingBlock: Option<nsIFrame>, aSaveState: &mut nsFrameConstructorSaveState) {
        MOZ_ASSERT!(aNewFloatContainingBlock.map_or(true, |f| f.IsFloatContainingBlock()), "Please push a real float containing block!");
        NS_ASSERTION!(aNewFloatContainingBlock.map_or(true, |f| !ShouldSuppressFloatingOfDescendants(f)), "We should not push a frame that is supposed to _suppress_ floats as a float containing block!");
        aSaveState.mList = Some(nsFrameConstructorSaveState_List::Floated);
        *aSaveState.mSavedList = std::mem::take(&mut self.mFloatedList);
        aSaveState.mChildListID = Some(FrameChildListID::Float);
        self.mFloatedList = AbsoluteFrameList::new(aNewFloatContainingBlock);
    }

    // Function to return the proper geometric parent for a frame with display
    // struct given by aStyleDisplay and parent's frame given by
    // aContentParentFrame.
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorState::GetGeometricParent
    pub fn GetGeometricParent(&self, aStyleDisplay: &nsStyleDisplay, aContentParentFrame: Option<nsIFrame>) -> Option<nsIFrame> {
        // If there is no container for a fixed, absolute, or floating root
        // frame, we will ignore the positioning.  This hack is originally
        // brought to you by the letter T: tables, since other roots don't
        // even call into this code.  See bug 178855.
        //
        // XXX Disabling positioning in this case is a hack.  If one was so inclined,
        // one could support this either by (1) inserting a dummy block between the
        // table and the canvas or (2) teaching the canvas how to reflow positioned
        // elements. (1) has the usual problems when multiple frames share the same
        // content (notice all the special cases in this file dealing with inner
        // tables and table wrappers which share the same content). (2) requires some
        // work and possible factoring.
        //
        // XXXbz couldn't we just force position to "static" on roots and
        // float to "none"?  That's OK per CSS 2.1, as far as I can tell.

        if let Some(p) = aContentParentFrame {
            if p.IsInSVGTextSubtree() {
                return aContentParentFrame;
            }
        }

        if aStyleDisplay.IsFloatingStyle() && self.mFloatedList.mContainingBlock.is_some() {
            NS_ASSERTION!(!aStyleDisplay.IsAbsolutelyPositionedStyle(), "Absolutely positioned _and_ floating?");
            return self.mFloatedList.mContainingBlock;
        }

        // if (aStyleDisplay.mTopLayer != StyleTopLayer::None) {...}
        // NOT PORTED: the top layer.

        if aStyleDisplay.mPosition() == StylePositionProperty::Absolute && self.mAbsoluteList.mContainingBlock.is_some() {
            return self.mAbsoluteList.mContainingBlock;
        }

        if aStyleDisplay.mPosition() == StylePositionProperty::Fixed {
            let fixedCB = match self.mFixedList {
                FixedListRef::Absolute => self.mAbsoluteList.mContainingBlock,
                FixedListRef::AncestorFixed => self.mAncestorFixedList.mContainingBlock,
                FixedListRef::RealFixed => self.mRealFixedList.mContainingBlock,
                FixedListRef::Saved(p) => unsafe { (*p).mContainingBlock },
            };
            if fixedCB.is_some() {
                return fixedCB;
            }
        }

        aContentParentFrame
    }

    /**
     * Function to add a new frame to the right frame list.  This MUST be called
     * on frames before their children have been processed if the frames might
     * conceivably be out-of-flow; otherwise cleanup in error cases won't work
     * right.  Also, this MUST be called on frames after they have been
     * initialized.
     * @param aNewFrame the frame to add
     * @param aFrameList the list to add in-flow frames to
     * @param aContent the content pointer for aNewFrame
     * @param aParentFrame the parent frame for the content if it were in-flow
     * @param aCanBePositioned pass false if the frame isn't allowed to be
     *        positioned
     * @param aCanBeFloated pass false if the frame isn't allowed to be
     *        floated
     */
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorState::AddChild
    pub fn AddChild(
        &mut self,
        aFC: &mut nsCSSFrameConstructor,
        aNewFrame: nsIFrame,
        aFrameList: &mut nsFrameList,
        aContent: ContentId,
        aParentFrame: Option<nsIFrame>,
        aCanBePositioned: bool,
        aCanBeFloated: bool,
        aInsertAfter: bool,
        aInsertAfterFrame: Option<nsIFrame>,
    ) {
        MOZ_ASSERT!(aNewFrame.GetNextSibling().is_none(), "Shouldn't happen");

        let mut placeholderType: nsFrameState = 0;
        let outOfFlowFrameList = self.GetOutOfFlowFrameList(aNewFrame, aCanBePositioned, aCanBeFloated, &mut placeholderType);

        // The comments in GetGeometricParent regarding root table frames
        // all apply here, unfortunately. Thus, we need to check whether
        // the returned frame items really has containing block.
        let useOutOfFlowList = match outOfFlowFrameList {
            Some(list) => self.OutOfFlowList(list).mContainingBlock.is_some(),
            None => false,
        };
        if !useOutOfFlowList {
            placeholderType = 0;
        } else {
            MOZ_ASSERT!(aNewFrame.GetParent() == self.OutOfFlowList(outOfFlowFrameList.unwrap()).mContainingBlock, "Parent of the frame is not the containing block?");
        }

        if placeholderType != 0 {
            let placeholderFrame = aFC.CreatePlaceholderFrameFor(aContent, aNewFrame, aParentFrame, None, placeholderType);

            placeholderFrame.AddStateBits(self.mAdditionalStateBits);
            // Add the placeholder frame to the flow
            aFrameList.AppendFrame(None, placeholderFrame);
        } else {
            NS_ASSERTION!(aNewFrame.GetParent() == aParentFrame, "In-flow frame has wrong parent");
        }

        let frameList: &mut nsFrameList = if useOutOfFlowList { &mut self.OutOfFlowList(outOfFlowFrameList.unwrap()).mList } else { aFrameList };
        if aInsertAfter {
            frameList.InsertFrame(None, aInsertAfterFrame, aNewFrame);
        } else {
            frameList.AppendFrame(None, aNewFrame);
        }
    }

    // ADAPTER: the list GetOutOfFlowFrameList names (Gecko returns a pointer to it).
    fn OutOfFlowList(&mut self, aList: nsFrameConstructorState_OutOfFlowList) -> &mut AbsoluteFrameList {
        match aList {
            nsFrameConstructorState_OutOfFlowList::Floated => &mut self.mFloatedList,
            nsFrameConstructorState_OutOfFlowList::Absolute => &mut self.mAbsoluteList,
            nsFrameConstructorState_OutOfFlowList::Fixed => self.FixedList(),
        }
    }

    /**
     * ProcessFrameInsertions takes the frames in aFrameList and adds them as
     * kids to the aChildListID child list of |aFrameList.containingBlock|.
     */
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorState::ProcessFrameInsertions
    pub fn ProcessFrameInsertions(&mut self, aFrameList: &mut AbsoluteFrameList, aChildListID: FrameChildListID) {
        MOZ_ASSERT!(aChildListID == FrameChildListID::Float || aChildListID == FrameChildListID::Absolute);

        if aFrameList.IsEmpty() {
            return;
        }

        let containingBlock = aFrameList.mContainingBlock;

        NS_ASSERTION!(containingBlock.is_some(), "Child list without containing block?");
        let containingBlock = containingBlock.unwrap();

        // Insert the frames hanging out in aItems.  We can use SetInitialChildList()
        // if the containing block hasn't been reflowed yet (so NS_FRAME_FIRST_REFLOW
        // is set) and doesn't have any frames in the aChildListID child list yet.
        let list = std::mem::take(&mut aFrameList.mList);
        let childListIsEmpty = containingBlock.GetChildList(aChildListID).IsEmpty();
        if childListIsEmpty && containingBlock.HasAnyStateBits(NS_FRAME_FIRST_REFLOW) {
            // If we're injecting absolutely positioned frames, inject them on the
            // absolute containing block
            if aChildListID == FrameChildListID::Absolute {
                containingBlock.GetAbsoluteContainingBlock().unwrap().SetInitialChildList(containingBlock, aChildListID, list);
            } else {
                nsContainerFrame::from_frame(containingBlock).SetInitialChildList(aChildListID, list);
            }
        } else if childListIsEmpty || aChildListID == FrameChildListID::Absolute {
            // The order is not important for abs-pos/fixed-pos frame list, just
            // append the frame items to the list directly.
            // (mFrameConstructor->AppendFrames: for the absolute list nsFrameManager::AppendFrames
            // hands the frames to the absolute containing block)
            if aChildListID == FrameChildListID::Absolute {
                containingBlock.GetAbsoluteContainingBlock().unwrap().AppendFrames(containingBlock, aChildListID, list);
            } else {
                nsContainerFrame::from_frame(containingBlock).AppendFrames(aChildListID, list);
            }
        } else {
            MOZ_ASSERT!(aChildListID == FrameChildListID::Float);
            // NOT PORTED: keeping the new floats in the order of their placeholders when the
            // containing block already has floats (CompareTreePosition and the binary search); the
            // construction of the initial document appends.
            nsContainerFrame::from_frame(containingBlock).AppendFrames(aChildListID, list);
        }
    }

    /**
     * GetOutOfFlowFrameList selects the out-of-flow frame list the new
     * frame should be added to. If the frame shouldn't be added to any
     * out-of-flow list, it returns nullptr. The corresponding type of
     * placeholder is also returned via the aPlaceholderType parameter
     * if this method doesn't return nullptr. The caller should check
     * whether the returned list really has a containing block.
     */
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsFrameConstructorState::GetOutOfFlowFrameList
    // NOT PORTED: the top layer lists.
    fn GetOutOfFlowFrameList(&self, aNewFrame: nsIFrame, aCanBePositioned: bool, aCanBeFloated: bool, aPlaceholderType: &mut nsFrameState) -> Option<nsFrameConstructorState_OutOfFlowList> {
        let disp = aNewFrame.StyleDisplay();
        if aCanBeFloated && disp.IsFloatingStyle() {
            *aPlaceholderType = PLACEHOLDER_FOR_FLOAT;
            return Some(nsFrameConstructorState_OutOfFlowList::Floated);
        }

        if aCanBePositioned {
            // if (disp->mTopLayer != StyleTopLayer::None) {...}  NOT PORTED: the top layer.
            if disp.mPosition() == StylePositionProperty::Absolute {
                *aPlaceholderType = PLACEHOLDER_FOR_ABSPOS;
                return Some(nsFrameConstructorState_OutOfFlowList::Absolute);
            }
            if disp.mPosition() == StylePositionProperty::Fixed {
                *aPlaceholderType = PLACEHOLDER_FOR_FIXEDPOS;
                return Some(nsFrameConstructorState_OutOfFlowList::Fixed);
            }
        }
        None
    }
}

// ADAPTER: which list GetOutOfFlowFrameList returns (`&mFloatedList`, `&mAbsoluteList` or
// `mFixedList`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum nsFrameConstructorState_OutOfFlowList {
    Floated,
    Absolute,
    Fixed,
}

//----------------------------------------------------------------------

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::ParentType
// NOT PORTED: the ruby types are there, no frame has them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParentType {
    eTypeBlock = 0, /* This includes all non-table-related frames */
    eTypeRow,
    eTypeRowGroup,
    eTypeColGroup,
    eTypeTable,
    eTypeRuby,
    eTypeRubyBase,
    eTypeRubyBaseContainer,
    eTypeRubyText,
    eTypeRubyTextContainer,
    eParentTypeCount,
}
pub use ParentType::*;

/* 4 bits is enough to handle our ParentType values */
pub const FCDATA_PARENT_TYPE_OFFSET: u32 = 28;
/* Macro to get the desired parent type out of an mBits member of
   FrameConstructionData */
pub fn FCDATA_DESIRED_PARENT_TYPE(_bits: u32) -> ParentType {
    match _bits >> FCDATA_PARENT_TYPE_OFFSET {
        0 => eTypeBlock,
        1 => eTypeRow,
        2 => eTypeRowGroup,
        3 => eTypeColGroup,
        4 => eTypeTable,
        5 => eTypeRuby,
        6 => eTypeRubyBase,
        7 => eTypeRubyBaseContainer,
        8 => eTypeRubyText,
        9 => eTypeRubyTextContainer,
        _ => unreachable!("not a ParentType"),
    }
}
/* Macro to create FrameConstructionData bits out of a desired parent type */
pub const fn FCDATA_DESIRED_PARENT_TYPE_TO_BITS(_type: ParentType) -> u32 {
    (_type as u32) << FCDATA_PARENT_TYPE_OFFSET
}

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::IsRubyParentType
pub fn IsRubyParentType(aParentType: ParentType) -> bool {
    aParentType == eTypeRuby || aParentType == eTypeRubyBase || aParentType == eTypeRubyBaseContainer || aParentType == eTypeRubyText || aParentType == eTypeRubyTextContainer
}

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::IsTableParentType
pub fn IsTableParentType(aParentType: ParentType) -> bool {
    aParentType == eTypeTable || aParentType == eTypeRow || aParentType == eTypeRowGroup || aParentType == eTypeColGroup
}

/* Bits that modify the way a FrameConstructionData is handled */

/* If the FCDATA_SKIP_FRAMESET bit is set, then the frame created should not
   be set as the primary frame on the content node.  This should only be used
   in very rare cases when we create more than one frame for a given content
   node. */
pub const FCDATA_SKIP_FRAMESET: u32 = 0x1;
/* If the FCDATA_FUNC_IS_DATA_GETTER bit is set, then the mFunc of the
   FrameConstructionData is a getter function that can be used to get the
   actual FrameConstructionData to use. */
pub const FCDATA_FUNC_IS_DATA_GETTER: u32 = 0x2;
/* If the FCDATA_FUNC_IS_FULL_CTOR bit is set, then the FrameConstructionData
   has an mFullConstructor.  In this case, there is no relevant mData or
   mFunc */
pub const FCDATA_FUNC_IS_FULL_CTOR: u32 = 0x4;
/* If FCDATA_DISALLOW_OUT_OF_FLOW is set, do not allow the frame to
   float or be absolutely positioned.  This can also be used with
   FCDATA_FUNC_IS_FULL_CTOR to indicate what the full-constructor
   function will do. */
pub const FCDATA_DISALLOW_OUT_OF_FLOW: u32 = 0x8;
/* If FCDATA_FORCE_NULL_ABSPOS_CONTAINER is set, make sure to push a
   null absolute containing block before processing children for this
   frame.  If this is not set, the frame will be pushed as the
   absolute containing block as needed, based on its style */
pub const FCDATA_FORCE_NULL_ABSPOS_CONTAINER: u32 = 0x10;
/* If FCDATA_WRAP_KIDS_IN_BLOCKS is set, the inline kids of the frame
   will be wrapped in blocks.  This is only usable for MathML at the
   moment. */
pub const FCDATA_WRAP_KIDS_IN_BLOCKS: u32 = 0x20;
/* If FCDATA_SUPPRESS_FRAME is set, no frame should be created for the
   content.  If this bit is set, nothing else in the struct needs to be
   set. */
pub const FCDATA_SUPPRESS_FRAME: u32 = 0x40;
/* If FCDATA_MAY_NEED_SCROLLFRAME is set, the new frame should be wrapped in
   a scrollframe if its overflow type so requires. */
pub const FCDATA_MAY_NEED_SCROLLFRAME: u32 = 0x80;
/* If FCDATA_SKIP_ABSPOS_PUSH is set, don't push this frame as an
   absolute containing block, no matter what its style says. */
pub const FCDATA_SKIP_ABSPOS_PUSH: u32 = 0x200;
/* If FCDATA_DISALLOW_GENERATED_CONTENT is set, then don't allow generated
   content when processing kids of this frame.  This should not be used with
   FCDATA_FUNC_IS_FULL_CTOR */
pub const FCDATA_DISALLOW_GENERATED_CONTENT: u32 = 0x400;
/* If FCDATA_IS_TABLE_PART is set, then the frame is some sort of
   table-related thing and we should not attempt to fetch a table-cell parent
   for it if it's inside another table-related frame. */
pub const FCDATA_IS_TABLE_PART: u32 = 0x800;
/* If FCDATA_IS_INLINE is set, then the frame is a non-replaced CSS
   inline box. */
pub const FCDATA_IS_INLINE: u32 = 0x1000;
/* If FCDATA_IS_LINE_PARTICIPANT is set, the frame is something that will
   return true for IsLineParticipant() */
pub const FCDATA_IS_LINE_PARTICIPANT: u32 = 0x2000;
/* If FCDATA_IS_LINE_BREAK is set, the frame is something that will
   induce a line break boundary before and after itself. */
pub const FCDATA_IS_LINE_BREAK: u32 = 0x4000;
/* If FCDATA_ALLOW_BLOCK_STYLES is set, allow block styles when processing
   children of a block (i.e. allow ::first-letter/line).
   This should not be used with FCDATA_FUNC_IS_FULL_CTOR. */
pub const FCDATA_ALLOW_BLOCK_STYLES: u32 = 0x8000;
/* If FCDATA_USE_CHILD_ITEMS is set, then use the mChildItems in the relevant
   FrameConstructionItem instead of trying to process the content's children.
   This can be used with or without FCDATA_FUNC_IS_FULL_CTOR.
   The child items might still need table pseudo processing. */
pub const FCDATA_USE_CHILD_ITEMS: u32 = 0x10000;
// 0x200000 is free
/* If FCDATA_CREATE_BLOCK_WRAPPER_FOR_ALL_KIDS is set, then create a
   block formatting context wrapper around the kids of this frame
   using the FrameConstructionData's mPseudoAtom for its anonymous
   box type. */
pub const FCDATA_CREATE_BLOCK_WRAPPER_FOR_ALL_KIDS: u32 = 0x40000;
/* If FCDATA_IS_SVG_TEXT is set, then this text frame is a descendant of
   an SVG text frame. */
pub const FCDATA_IS_SVG_TEXT: u32 = 0x80000;
/**
 * Whether the kids of this FrameConstructionData should be flagged as having
 * a wrapper anon box parent.  This should only be set if
 * FCDATA_USE_CHILD_ITEMS is set.
 */
pub const FCDATA_IS_WRAPPER_ANON_BOX: u32 = 0x400000;
/* ADAPTER: the frame has no children (a replaced element or a form control, whose frame
   classes are not ported: a block with no children stands for it), so ProcessChildren is
   not run for it. Gecko's nsIFrame::IsLeaf() answers this from the frame class. */
pub const FCDATA_ADAPTER_LEAF: u32 = 0x1000000;

/* A constructor function that just creates an nsIFrame object.  The caller
   is responsible for initializing the object, adding it to frame lists,
   constructing frames for the children, etc.

   ADAPTER: the frame is allocated by the frame tree already (FrameTree::AddFrame); the function
   gives it its class and returns it. In Gecko it takes the PresShell and the ComputedStyle. */
pub type FrameCreationFunc = fn(nsIFrame) -> nsIFrame;

/* A function that can be used to get a FrameConstructionData.  Such
   a function is allowed to return null.

   @param nsIContent the node for which the frame is being constructed.
   @param ComputedStyle the style to be used for the frame.
*/
pub type FrameConstructionDataGetter = fn(&nsCSSFrameConstructor, ContentId, &ServoArc<ComputedStyle>) -> Option<&'static FrameConstructionData>;

/* A constructor function that's used for complicated construction tasks.
   This is expected to create the new frame, initialize it, add whatever
   needs to be added to aFrameList, process children as
   needed, etc.  It is NOT expected to deal with setting the frame on the
   content.

   @param aState the frame construction state to use.
   @param aItem the frame construction item to use
   @param aParentFrame the frame to set as the parent of the
                       newly-constructed frame.
   @param aStyleDisplay the display struct from aItem's mComputedStyle
   @param aFrameList the frame list to add the new frame (or its
                      placeholder) to.
   @return the frame that was constructed.  This frame is what the caller
           will set as the frame on the content.  Guaranteed non-null.
*/
pub type FrameFullConstructor = fn(&mut nsCSSFrameConstructor, &mut nsFrameConstructorState, &mut FrameConstructionItem, Option<nsIFrame>, &nsStyleDisplay, &mut nsFrameList) -> nsIFrame;

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::FrameConstructionData::Func
// (a C++ union of the three function types)
#[derive(Clone, Copy)]
pub enum FrameConstructionData_Func {
    mCreationFunc(Option<FrameCreationFunc>),
    mDataGetter(FrameConstructionDataGetter),
    mFullConstructor(FrameFullConstructor),
}

/* Structure representing information about how a frame should be
   constructed.  */
// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::FrameConstructionData
pub struct FrameConstructionData {
    // We have exactly one of three types of functions
    pub mFunc: FrameConstructionData_Func,
    // Flag bits that can modify the way the construction happens
    pub mBits: u32,
    // For cases when FCDATA_CREATE_BLOCK_WRAPPER_FOR_ALL_KIDS is set, the
    // anonymous box type to use for that wrapper.
    pub mAnonBoxPseudo: PseudoStyleType,
}

impl FrameConstructionData {
    // Gecko: FrameConstructionData(FrameCreationFunc aCreationFunc, uint32_t aBits = 0)
    pub const fn new(aCreationFunc: Option<FrameCreationFunc>, aBits: u32) -> FrameConstructionData {
        FrameConstructionData { mFunc: FrameConstructionData_Func::mCreationFunc(aCreationFunc), mBits: aBits, mAnonBoxPseudo: PseudoStyleType::NotPseudo }
    }
    // Gecko: FrameConstructionData(FrameConstructionDataGetter aDataGetter, uint32_t aBits = 0)
    pub const fn new__FrameConstructionDataGetter(aDataGetter: FrameConstructionDataGetter, aBits: u32) -> FrameConstructionData {
        FrameConstructionData { mFunc: FrameConstructionData_Func::mDataGetter(aDataGetter), mBits: aBits | FCDATA_FUNC_IS_DATA_GETTER, mAnonBoxPseudo: PseudoStyleType::NotPseudo }
    }
    // Gecko: FrameConstructionData(FrameFullConstructor aCtor, uint32_t aBits = 0)
    pub const fn new__FrameFullConstructor(aCtor: FrameFullConstructor, aBits: u32) -> FrameConstructionData {
        FrameConstructionData { mFunc: FrameConstructionData_Func::mFullConstructor(aCtor), mBits: aBits | FCDATA_FUNC_IS_FULL_CTOR, mAnonBoxPseudo: PseudoStyleType::NotPseudo }
    }
}

/* Structure representing a mapping of an atom to a FrameConstructionData.
   This can be used with non-static atoms, assuming that the nsAtom* is
   stored somewhere that this struct can point to (that is, a static
   nsAtom*) and that it's allocated before the struct is ever used. */
// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::FrameConstructionDataByTag
pub struct FrameConstructionDataByTag {
    pub mTag: nsAtom,
    pub mData: FrameConstructionData,
}

// ADAPTER: the ItemFlag enum of nsCSSFrameConstructor.h and EnumSet<ItemFlag>, as bits.
// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::ItemFlag
MOZ_ENUM_CLASS_BITS! {
    pub struct nsCSSFrameConstructor_ItemFlag: u8 {
        None = 0,
        // Allow page-break before and after items to be created if the
        // style asks for them.
        AllowPageBreak = 1 << 0,
        IsGeneratedContent = 1 << 1,
        IsWithinSVGText = 1 << 2,
        // The item allows items to be created for SVG <textPath> children.
        AllowTextPathChild = 1 << 3,
        // The item is content created by an nsIAnonymousContentCreator frame.
        IsAnonymousContentCreatorContent = 1 << 4,
        // The item will be the rendered legend of a <fieldset>.
        IsForRenderedLegend = 1 << 5,
        // This will be an outside ::marker.
        IsForOutsideMarker = 1 << 6,
    }
}
pub type ItemFlag = nsCSSFrameConstructor_ItemFlag;
pub type ItemFlags = nsCSSFrameConstructor_ItemFlag;

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::InsertionPoint (reduced to the
// parent frame and the container of the children)
#[derive(Clone, Copy)]
pub struct InsertionPoint {
    pub mParentFrame: Option<nsIFrame>,
    pub mContainer: ContentId,
}

/* A class representing a list of FrameConstructionItems.  Instances of this
   class are only created as AutoFrameConstructionItemList, or as a member
   of FrameConstructionItem. */
// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::FrameConstructionItemList
// ADAPTER: the items are owned by a Vec (Gecko: an arena-allocated LinkedList), and an iterator
// is an index (FCItemIterator); the list is passed next to the index where the C++ iterator carries
// a reference to it.
#[derive(Default)]
pub struct FrameConstructionItemList {
    pub mItems: Vec<FrameConstructionItem>,
    mInlineCount: u32,
    mBlockCount: u32,
    mItemCount: u32,
    // True if there is guaranteed to be a line boundary before the
    // frames created by these items
    mLineBoundaryAtStart: bool,
    // True if there is guaranteed to be a line boundary after the
    // frames created by these items
    mLineBoundaryAtEnd: bool,
    // True if the parent is guaranteed to have no shadow tree.
    mParentHasNoShadowDOM: bool,
    // How many items want each ParentType as their parent.
    mDesiredParentCounts: [u32; 10],
}

impl FrameConstructionItemList {
    // Gecko: FrameConstructionItemList::SetLineBoundaryAtStart
    pub fn SetLineBoundaryAtStart(&mut self, aBoundary: bool) {
        self.mLineBoundaryAtStart = aBoundary;
    }
    // Gecko: FrameConstructionItemList::SetLineBoundaryAtEnd
    pub fn SetLineBoundaryAtEnd(&mut self, aBoundary: bool) {
        self.mLineBoundaryAtEnd = aBoundary;
    }
    // Gecko: FrameConstructionItemList::SetParentHasNoShadowDOM
    pub fn SetParentHasNoShadowDOM(&mut self, aValue: bool) {
        self.mParentHasNoShadowDOM = aValue;
    }
    // Gecko: FrameConstructionItemList::HasLineBoundaryAtStart
    pub fn HasLineBoundaryAtStart(&self) -> bool {
        self.mLineBoundaryAtStart
    }
    // Gecko: FrameConstructionItemList::HasLineBoundaryAtEnd
    pub fn HasLineBoundaryAtEnd(&self) -> bool {
        self.mLineBoundaryAtEnd
    }
    // Gecko: FrameConstructionItemList::ParentHasNoShadowDOM
    pub fn ParentHasNoShadowDOM(&self) -> bool {
        self.mParentHasNoShadowDOM
    }
    // Gecko: FrameConstructionItemList::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        self.mItems.is_empty()
    }
    // Gecko: FrameConstructionItemList::AreAllItemsInline
    pub fn AreAllItemsInline(&self) -> bool {
        self.mInlineCount == self.mItemCount
    }
    // Gecko: FrameConstructionItemList::AreAllItemsBlock
    pub fn AreAllItemsBlock(&self) -> bool {
        self.mBlockCount == self.mItemCount
    }
    // Gecko: FrameConstructionItemList::AllWantParentType
    pub fn AllWantParentType(&self, aDesiredParentType: ParentType) -> bool {
        self.mDesiredParentCounts[aDesiredParentType as usize] == self.mItemCount
    }

    // aSuppressWhiteSpaceOptimizations is true if optimizations that
    // skip constructing whitespace frames for this item or items
    // around it cannot be performed.
    // Also, the return value is the index of the new item (Gecko: a pointer to it).
    // Gecko: FrameConstructionItemList::AppendItem
    pub fn AppendItem(&mut self, aFCData: &'static FrameConstructionData, aContent: ContentId, aComputedStyle: ServoArc<ComputedStyle>, aSuppressWhiteSpaceOptimizations: bool) -> usize {
        self.mItems.push(FrameConstructionItem::new(aFCData, aContent, aComputedStyle, aSuppressWhiteSpaceOptimizations));
        self.mItemCount += 1;
        let desiredParentType = self.mItems[self.mItems.len() - 1].DesiredParentType();
        self.mDesiredParentCounts[desiredParentType as usize] += 1;
        self.mItems.len() - 1
    }

    // Adjust our various counts for aItem being added or removed.  aDelta
    // should be either +1 or -1 depending on which is happening.
    // Gecko: FrameConstructionItemList::AdjustCountsForItem
    fn AdjustCountsForItem(&mut self, aItem: &FrameConstructionItem, aDelta: i32) {
        self.mItemCount = (self.mItemCount as i32 + aDelta) as u32;
        if aItem.mIsAllInline {
            self.mInlineCount = (self.mInlineCount as i32 + aDelta) as u32;
        }
        if aItem.mIsBlock {
            self.mBlockCount = (self.mBlockCount as i32 + aDelta) as u32;
        }
        let i = aItem.DesiredParentType() as usize;
        self.mDesiredParentCounts[i] = (self.mDesiredParentCounts[i] as i32 + aDelta) as u32;
    }

    // ---- FrameConstructionItemList::Iterator ----------------------------------------------------
    // ADAPTER: an iterator is an index into mItems (see FCItemIterator); one that is done is
    // mItems.len(). Removing or inserting items moves the indexes after them, so the methods that
    // do it (InsertItem, DeleteItemsTo) say where the iterators involved are afterwards.

    // Gecko: FrameConstructionItemList::Iterator::IsDone
    pub fn IsDone(&self, aIter: FCItemIterator) -> bool {
        aIter == self.mItems.len()
    }

    // Skip over all items that want the given parent type. Return whether
    // the iterator is done after doing that.  The iterator must not be done
    // when this is called.
    // Gecko: FrameConstructionItemList::Iterator::SkipItemsWantingParentType
    pub fn SkipItemsWantingParentType(&self, aIter: &mut FCItemIterator, aParentType: ParentType) -> bool {
        MOZ_ASSERT!(!self.IsDone(*aIter), "Shouldn't be done yet");
        while self.mItems[*aIter].DesiredParentType() == aParentType {
            *aIter += 1;
            if self.IsDone(*aIter) {
                return true;
            }
        }
        false
    }

    // Remove the item pointed to by aIter from this list and append it to aTargetList.
    // aIter now points to the next item of this list.
    // Gecko: FrameConstructionItemList::Iterator::AppendItemToList
    fn AppendItemToList(&mut self, aIter: &mut FCItemIterator, aTargetList: &mut FrameConstructionItemList) {
        MOZ_ASSERT!(!self.IsDone(*aIter), "should not be done");
        let item = self.mItems.remove(*aIter);
        self.AdjustCountsForItem(&item, -1);
        aTargetList.AdjustCountsForItem(&item, 1);
        aTargetList.mItems.push(item);
    }

    // As above, but moves all items from aIter until we get to aEnd; the item pointed to by aEnd
    // is not stolen.  After this method returns, aIter points to the item aEnd pointed to (as an
    // index: aIter does not move, the items before aEnd are gone), and aEnd is out of date: the
    // caller sets it to aIter.
    // Gecko: FrameConstructionItemList::Iterator::AppendItemsToList
    pub fn AppendItemsToList(&mut self, aIter: &mut FCItemIterator, aEnd: FCItemIterator, aTargetList: &mut FrameConstructionItemList) {
        // (the shortcut that moves a whole list into an empty target is only an optimization)
        let mut remaining = aEnd - *aIter;
        while remaining > 0 {
            self.AppendItemToList(aIter, aTargetList);
            remaining -= 1;
        }
    }

    // Insert aItem in this list right before the item aIter points to.  After the insertion,
    // aIter points to the item it pointed to (the one just after the newly-inserted item). aIter is
    // allowed to be done; in that case this call just appends the given item to the list.
    // Gecko: FrameConstructionItemList::Iterator::InsertItem
    pub fn InsertItem(&mut self, aIter: &mut FCItemIterator, aItem: FrameConstructionItem) {
        self.AdjustCountsForItem(&aItem, 1);
        self.mItems.insert(*aIter, aItem);
        *aIter += 1;
    }

    // Delete the items between aIter and aEnd, including the item aIter currently points to but
    // not including the item pointed to by aEnd.  When this returns, aIter points to the item
    // aEnd pointed to (the index is the same as before: the items before aEnd are gone), and aEnd
    // is out of date: the caller sets it to aIter. aIter must not equal aEnd when this method is
    // called.
    // Gecko: FrameConstructionItemList::Iterator::DeleteItemsTo
    pub fn DeleteItemsTo(&mut self, aIter: &mut FCItemIterator, aEnd: FCItemIterator) {
        MOZ_ASSERT!(*aIter != aEnd, "Shouldn't be at aEnd yet");
        let removed: Vec<FrameConstructionItem> = self.mItems.drain(*aIter..aEnd).collect();
        for item in &removed {
            self.AdjustCountsForItem(item, -1);
        }
    }

    // Gecko: FrameConstructionItemList::InlineItemAdded
    pub fn InlineItemAdded(&mut self) {
        self.mInlineCount += 1;
    }
    // Gecko: FrameConstructionItemList::BlockItemAdded
    pub fn BlockItemAdded(&mut self) {
        self.mBlockCount += 1;
    }
}

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::FCItemIterator
// ADAPTER: an index into the list (see FrameConstructionItemList). The Iterator methods that the
// ported code uses are on the list, taking the index.
pub type FCItemIterator = usize;

/* A struct representing an item for which frames might need to be
 * constructed.  This contains all the information needed to construct the
 * frame other than the parent frame and whatever would be stored in the
 * frame constructor state.  You probably want to use
 * AutoFrameConstructionItem instead of this struct. */
// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::FrameConstructionItem
pub struct FrameConstructionItem {
    // Child frame construction items.
    pub mChildItems: FrameConstructionItemList,

    // The FrameConstructionData to use.
    pub mFCData: &'static FrameConstructionData,
    // The nsIContent node to use when initializing the new frame.
    pub mContent: ContentId,
    // The style to use for creating the new frame.
    pub mComputedStyle: ServoArc<ComputedStyle>,
    // Whether optimizations to skip constructing textframes around
    // this content need to be suppressed.
    pub mSuppressWhiteSpaceOptimizations: bool,
    // Whether this is a text content item.
    pub mIsText: bool,
    // Whether this is a generated content container.
    // If it is, mContent is a strong pointer.
    pub mIsGeneratedContent: bool,
    // Whether construction from this item will create only frames that are
    // IsInlineOutside() in the principal child list.  This is not precise, but
    // conservative: if true the frames will really be inline, whereas if false
    // they might still all be inline.
    pub mIsAllInline: bool,
    // Whether construction from this item will create only frames that are
    // IsBlockOutside() in the principal child list.  This is not precise, but
    // conservative: if true the frames will really be blocks, whereas if false
    // they might still be blocks (and in particular, out-of-flows that didn't
    // find a containing block).
    pub mIsBlock: bool,
    // Whether this item is the rendered legend of a <fieldset>
    pub mIsRenderedLegend: bool,
}

impl FrameConstructionItem {
    // Gecko: layout/base/nsCSSFrameConstructor.cpp FrameConstructionItem::NeedsAnonFlexOrGridItem
    // Note: we implement -webkit-{inline-}box using nsFlexContainerFrame, but we
    // use different rules for what gets wrapped in an anonymous flex item.
    // NOT PORTED: the aIsLegacyWebKitBox branch (it needs -webkit-box, which is not in this Stylo
    // configuration): all inline-level content and abspos/fixedpos boxes would be wrapped.
    pub fn NeedsAnonFlexOrGridItem(&self, aIsLegacyWebKitBox: bool) -> bool {
        if (self.mFCData.mBits & FCDATA_IS_LINE_PARTICIPANT) != 0 {
            // This will be an inline non-replaced box.
            return true;
        }

        // if (aIsLegacyWebKitBox) { ... }: NOT PORTED, see above.

        false
    }

    // Gecko: FrameConstructionItem::FrameConstructionItem
    pub fn new(aFCData: &'static FrameConstructionData, aContent: ContentId, aComputedStyle: ServoArc<ComputedStyle>, aSuppressWhiteSpaceOptimizations: bool) -> FrameConstructionItem {
        FrameConstructionItem {
            mChildItems: FrameConstructionItemList::default(),
            mFCData: aFCData,
            mContent: aContent,
            mComputedStyle: aComputedStyle,
            mSuppressWhiteSpaceOptimizations: aSuppressWhiteSpaceOptimizations,
            mIsText: false,
            mIsGeneratedContent: false,
            mIsAllInline: false,
            mIsBlock: false,
            mIsRenderedLegend: false,
        }
    }

    // Gecko: FrameConstructionItem::DesiredParentType
    pub fn DesiredParentType(&self) -> ParentType {
        FCDATA_DESIRED_PARENT_TYPE(self.mFCData.mBits)
    }

    // Gecko: FrameConstructionItem::IsLineBoundary
    pub fn IsLineBoundary(&self) -> bool {
        self.mIsBlock || (self.mFCData.mBits & FCDATA_IS_LINE_BREAK) != 0
    }
}

//----------------------------------------------------------------------

// ADAPTER: a generated content node: the ::marker container, and the text node in it. Gecko makes
// these as DOM nodes (NS_NewXMLElement, CreateGenConTextNode); here they are entries in a table
// of the constructor, found by their ContentId (kGeneratedContentBase + index).
struct GeneratedContent {
    mParent: ContentId,
    mFirstChild: ContentId,
    mNextSibling: ContentId,
    // the text of a text node; None for the container
    mText: Option<String>,
    // the style the container was made with
    mStyle: Option<ServoArc<ComputedStyle>>,
    // the primary frame (nsIContent::SetPrimaryFrame)
    mPrimaryFrame: Option<nsIFrame>,
    // the id of the frame's content in the side store of synthetic contents (nsIContent), made by the
    // first MakeContent; 0 before
    mSyntheticContent: std::cell::Cell<u32>,
}

// ADAPTER: how an anonymous style is derived (see AnonymousStyle).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum AnonymousDisplay {
    Inline,
    Block,
    InlineBlock,
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::IsLeaf (virtual; the frame classes of replaced elements and
// form controls are leaves)
// ADAPTER: the property is set on the block frame that stands for such an element.
const kLeafFramePropertyId: u32 = 112;
const kInputButtonControlFramePropertyId: u32 = 113;

impl nsIFrame {
    pub fn LeafFrameProperty() -> FramePropertyDescriptor<bool> {
        FramePropertyDescriptor::new(kLeafFramePropertyId)
    }
    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsLeaf
    pub fn IsLeaf(&self) -> bool {
        self.GetProperty(nsIFrame::LeafFrameProperty()).unwrap_or(false)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsInputButtonControlFrame (a class flag in Gecko)
    // ADAPTER: the button-type <input>s are block frames here (see ConstructInputButtonControl); this marks them.
    pub fn InputButtonControlFrameProperty() -> FramePropertyDescriptor<bool> {
        FramePropertyDescriptor::new(kInputButtonControlFramePropertyId)
    }
    pub fn IsInputButtonControlFrame(&self) -> bool {
        self.GetProperty(nsIFrame::InputButtonControlFrameProperty()).unwrap_or(false)
    }
}

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor
// ADAPTER: not an nsFrameManager; and it holds the arena DOM and style data of the document
// (mStyleDom) that Gecko reaches through the PresShell, the Document and the ServoStyleSet. The
// pointer is valid for the life of the constructor (the caller keeps the StyleDom alive).
pub struct nsCSSFrameConstructor {
    mStyleDom: *const StyleDom,

    // Gecko: mRootElementFrame, mRootElementStyleFrame, mDocElementContainingBlock, mCanvasFrame
    mRootElementFrame: Option<nsIFrame>,
    mRootElementStyleFrame: Option<nsIFrame>,
    mDocElementContainingBlock: Option<nsIFrame>,
    mCanvasFrame: Option<nsIFrame>,
    // Gecko: nsFrameManager::mRootFrame (the viewport frame)
    mRootFrame: Option<nsIFrame>,

    mCurrentDepth: u16,

    // Gecko: mAlwaysCreateFramesForIgnorableWhitespace
    mAlwaysCreateFramesForIgnorableWhitespace: bool,

    // ADAPTER: generated content, the ::marker of each list item (originating content to the
    // container), and the anonymous styles made so far (keyed by the address of the parent style and
    // the display).
    mGenerated: Vec<GeneratedContent>,
    mMarkerOf: HashMap<ContentId, ContentId>,
    // ADAPTER: the counter and quote text of the whole document, made on the first generated content
    // that needs it (see nsCounterManager.rs); Gecko's nsCounterManager and nsQuoteList live in the
    // ContainStyleScopeManager.
    mCounterManager: Option<nsCounterManager>,
    // (each entry keeps the parent style it was made from, so that its address, the key, cannot be
    // reused by another style while the entry is there; ClearStyleCaches empties them at each flush)
    mAnonymousStyles: HashMap<(usize, AnonymousDisplay), (ServoArc<ComputedStyle>, ServoArc<ComputedStyle>)>,
    // ADAPTER: ServoStyleSet::ResolveInheritingAnonymousBoxStyle keeps the styles it made (keyed by the
    // address of the parent style and the pseudo type), so the cells of a big table share them.
    mAnonBoxStyles: HashMap<(usize, PseudoStyleType), (ServoArc<ComputedStyle>, ServoArc<ComputedStyle>)>,
    // ADAPTER: the ContentId of the content each frame was initialized with (nsIFrame::GetContent
    // gives an nsIContent here, see MakeContent), indexed by frame id; NONE where there is none.
    // CreateNeededPseudoContainers gives the wrappers it makes the content of their parent frame.
    mFrameContent: Vec<ContentId>,
    // ADAPTER: which primary frames TrackedPrimaryFrames lists: those of the elements that have an `id`
    // attribute (TrackPrimaryFramesOfElementsWithId; the debugging tools compare boxes by id), or of
    // every element (TrackPrimaryFramesOfAllElements; `layoutcount --boxes`). The primary frames
    // themselves are kept on the content (nsIContent::GetPrimaryFrame) for every node.
    mTrackPrimaryFrames: bool,
    mTrackAllPrimaryFrames: bool,

    // Gecko: mRemovingContent
    mRemovingContent: bool,
    // ADAPTER: the DOM removal being applied (see RemovedContent).
    mRemoval: Option<RemovedContent>,
    // ADAPTER: what InsertionKind::Async asked to rebuild (see the dynamic changes section).
    mPendingReconstructs: Vec<ContentId>,
    mDynamicChangeCounts: DynamicChangeCounts,
    // ADAPTER: the pseudo type of the anonymous box styles made since the caches were last cleared,
    // by the style's address (AllocFrame sets AnonBoxPseudoTypeProperty from it; the caches keep
    // the styles alive, so an address is not reused meanwhile).
    mAnonBoxPseudoOfStyle: HashMap<usize, PseudoStyleType>,
    // ADAPTER: the ::before and ::after container of each element that has one (Gecko keeps them
    // as properties of the element: nsGkAtoms::beforePseudoProperty, afterPseudoProperty).
    mBeforeOf: HashMap<ContentId, ContentId>,
    mAfterOf: HashMap<ContentId, ContentId>,
    // ADAPTER: the generated texts made from the counter manager (see RecalcQuotesAndCounters).
    mCounterUses: Vec<CounterUse>,
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp sTextData etc. (the `static constexpr
// FrameConstructionData` objects of the functions below)
static sTextData: FrameConstructionData = FrameConstructionData::new(Some(ToCreationFunc_NS_NewTextFrame), FCDATA_IS_LINE_PARTICIPANT);
static sBRData: FrameConstructionData = FrameConstructionData::new(Some(ToCreationFunc_NS_NewBRFrame), FCDATA_IS_LINE_PARTICIPANT | FCDATA_IS_LINE_BREAK);
static sSuppressData: FrameConstructionData = FrameConstructionData::new(None, FCDATA_SUPPRESS_FRAME);
static sInlineData: FrameConstructionData = FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructInline, FCDATA_IS_INLINE | FCDATA_IS_LINE_PARTICIPANT);
// (kCaptionCtorFlags of FindDisplayData)
const kCaptionCtorFlags: u32 = FCDATA_IS_TABLE_PART | FCDATA_DESIRED_PARENT_TYPE_TO_BITS(eTypeTable);
static sScrollableBlockData: [FrameConstructionData; 2] = [
    FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructScrollableBlock, 0),
    FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructScrollableBlock, kCaptionCtorFlags),
];
static sNonScrollableBlockData: [FrameConstructionData; 2] = [
    FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructNonScrollableBlock, 0),
    FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructNonScrollableBlock, kCaptionCtorFlags),
];
// (the `static constexpr FrameConstructionData data` objects of the table cases of FindDisplayData)
static sTableData: FrameConstructionData = FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructTable, 0);
static sTableRowGroupData: FrameConstructionData =
    FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructTableRowOrRowGroup, FCDATA_IS_TABLE_PART | FCDATA_DESIRED_PARENT_TYPE_TO_BITS(eTypeTable));
static sTableColData: FrameConstructionData =
    FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructTableCol, FCDATA_IS_TABLE_PART | FCDATA_DESIRED_PARENT_TYPE_TO_BITS(eTypeColGroup));
static sTableColGroupData: FrameConstructionData = FrameConstructionData::new(
    Some(ToCreationFunc_NS_NewTableColGroupFrame),
    FCDATA_IS_TABLE_PART | FCDATA_DISALLOW_OUT_OF_FLOW | FCDATA_SKIP_ABSPOS_PUSH | FCDATA_DESIRED_PARENT_TYPE_TO_BITS(eTypeTable),
);
static sTableRowData: FrameConstructionData =
    FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructTableRowOrRowGroup, FCDATA_IS_TABLE_PART | FCDATA_DESIRED_PARENT_TYPE_TO_BITS(eTypeRowGroup));
static sTableCellData: FrameConstructionData =
    FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructTableCell, FCDATA_IS_TABLE_PART | FCDATA_DESIRED_PARENT_TYPE_TO_BITS(eTypeRow));
// (the `static constexpr FrameConstructionData` objects of the Flex case of FindDisplayData)
static sFlexNonScrollableData: FrameConstructionData = FrameConstructionData::new(Some(ToCreationFunc_NS_NewFlexContainerFrame), 0);
static sFlexData: FrameConstructionData = FrameConstructionData::new(Some(ToCreationFunc_NS_NewFlexContainerFrame), FCDATA_MAY_NEED_SCROLLFRAME);
// (the `static constexpr FrameConstructionData` objects of the Grid case of FindDisplayData)
static sGridNonScrollableData: FrameConstructionData = FrameConstructionData::new(Some(ToCreationFunc_NS_NewGridContainerFrame), 0);
static sGridData: FrameConstructionData = FrameConstructionData::new(Some(ToCreationFunc_NS_NewGridContainerFrame), FCDATA_MAY_NEED_SCROLLFRAME);
// Gecko: layout/base/nsCSSFrameConstructor.cpp CreateNeededAnonFlexOrGridItems, `sBlockFCData`
static sAnonFlexOrGridItemBlockData: FrameConstructionData =
    FrameConstructionData::new(Some(ToCreationFunc_NS_NewBlockFrame), FCDATA_SKIP_FRAMESET | FCDATA_USE_CHILD_ITEMS | FCDATA_IS_WRAPPER_ANON_BOX);
// ADAPTER: the data of the frames that are not ported (see ConstructReplacedFrameStub)
static sReplacedStubData: FrameConstructionData = FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructReplacedFrameStub, FCDATA_ADAPTER_LEAF);
// ADAPTER: the SVG case of FindElementTagData: the outer <svg> element (SVGOuterSVGFrame.rs)
static sSVGOuterSVGData: FrameConstructionData = FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructSVGOuterSVG, FCDATA_ADAPTER_LEAF);

// ADAPTER: Gecko's nsAtom for a string known only at run time (the atom table); the strings are kept for the life of the
// thread, as the atoms of the table are.
fn InternAtom(aString: &str) -> nsAtom {
    thread_local! {
        static sAtoms: std::cell::RefCell<std::collections::HashSet<&'static str>> = std::cell::RefCell::new(std::collections::HashSet::new());
    }
    sAtoms.with(|atoms| {
        let mut atoms = atoms.borrow_mut();
        if let Some(a) = atoms.get(aString) {
            return *a;
        }
        let a: &'static str = Box::leak(aString.to_string().into_boxed_str());
        atoms.insert(a);
        a
    })
}

// ADAPTER: ToCreationFunc(NS_NewXFrame): the NS_New*Frame functions give a frame its class.
fn ToCreationFunc_NS_NewTextFrame(aFrame: nsIFrame) -> nsIFrame {
    // (NS_NewTextFrame also initializes the frame with its content; here nsCSSFrameConstructor::
    // InitFrame does that for every class)
    aFrame.SetType(LayoutFrameType::Text);
    aFrame
}
fn ToCreationFunc_NS_NewBRFrame(aFrame: nsIFrame) -> nsIFrame {
    NS_NewBRFrame(aFrame).0
}
fn ToCreationFunc_NS_NewWBRFrame(aFrame: nsIFrame) -> nsIFrame {
    NS_NewWBRFrame(aFrame).0
}
fn ToCreationFunc_NS_NewTableColGroupFrame(aFrame: nsIFrame) -> nsIFrame {
    NS_NewTableColGroupFrame(aFrame).0
}
fn ToCreationFunc_NS_NewFlexContainerFrame(aFrame: nsIFrame) -> nsIFrame {
    NS_NewFlexContainerFrame(aFrame).0
}
fn ToCreationFunc_NS_NewGridContainerFrame(aFrame: nsIFrame) -> nsIFrame {
    NS_NewGridContainerFrame(aFrame).0
}
fn ToCreationFunc_NS_NewBlockFrame(aFrame: nsIFrame) -> nsIFrame {
    NS_NewBlockFrame(aFrame).0
}

impl nsCSSFrameConstructor {
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::nsCSSFrameConstructor
    pub fn new(aStyleDom: &StyleDom) -> nsCSSFrameConstructor {
        // ADAPTER: the languages of the page, for the font lookup (see nsIContent::mLanguage).
        {
            let dom = &aStyleDom.tree.dom;
            let mut languages = std::collections::BTreeSet::new();
            let mut stack = vec![aStyleDom.tree.doc];
            while let Some(id) = stack.pop() {
                if dom.node(id).kind == Kind::Element {
                    if let Some(lang) = dom.attrs_of(id).iter().find(|a| dom.names.get(a.name) == "lang") {
                        let lang = dom.attr_value(lang).trim().to_ascii_lowercase();
                        if !lang.is_empty() {
                            languages.insert(lang);
                        }
                    }
                }
                stack.extend(dom.children(id));
                let root = dom.shadow_root(id);
                if root != NONE {
                    stack.push(root);
                }
            }
            mrt_text::SetKnownLanguages(languages.into_iter().collect());
        }
        // ADAPTER: nsTableFrame makes the synthetic column group and columns of a table itself and asks
        // the style set for their styles; this is the style set. The resolver stays installed after
        // the constructor is gone (the table frames use it when they reflow), and the StyleDom must
        // outlive the frames, as it must for the constructor.
        {
            let styleDom: &'static StyleDom = unsafe { &*(aStyleDom as *const StyleDom) };
            let cache: std::cell::RefCell<[Option<ServoArc<ComputedStyle>>; 2]> = std::cell::RefCell::new([None, None]);
            SetTableAnonBoxStyleResolver(Some(std::rc::Rc::new(move |_aTable: nsIFrame, aBox: TableAnonBox| {
                let i = match aBox {
                    TableAnonBox::ColumnGroup => 0,
                    TableAnonBox::Column => 1,
                };
                cache.borrow_mut()[i]
                    .get_or_insert_with(|| {
                        nsCSSFrameConstructor::MakeNonInheritingAnonBoxStyle(
                            styleDom,
                            if i == 0 { PseudoStyleType::MozTableColumnGroup } else { PseudoStyleType::MozTableColumn },
                        )
                    })
                    .clone()
            })));
        }
        // ADAPTER: the frames read their content from this DOM from now on (see nsIContent)
        nsIContent::SetDocument(&aStyleDom.tree.dom);
        nsCSSFrameConstructor {
            mStyleDom: aStyleDom as *const StyleDom,
            mRootElementFrame: None,
            mRootElementStyleFrame: None,
            mDocElementContainingBlock: None,
            mCanvasFrame: None,
            mRootFrame: None,
            mCurrentDepth: 0,
            mAlwaysCreateFramesForIgnorableWhitespace: false,
            mGenerated: Vec::new(),
            mMarkerOf: HashMap::new(),
            mCounterManager: None,
            mAnonymousStyles: HashMap::new(),
            mAnonBoxStyles: HashMap::new(),
            mFrameContent: Vec::new(),
            mTrackPrimaryFrames: false,
            mTrackAllPrimaryFrames: false,
            mRemovingContent: false,
            mRemoval: None,
            mPendingReconstructs: Vec::new(),
            mDynamicChangeCounts: DynamicChangeCounts::default(),
            mAnonBoxPseudoOfStyle: HashMap::new(),
            mBeforeOf: HashMap::new(),
            mAfterOf: HashMap::new(),
            mCounterUses: Vec::new(),
        }
    }

    // ADAPTER: remember the primary frame of every element that has an `id` attribute.
    pub fn TrackPrimaryFramesOfElementsWithId(&mut self) {
        self.mTrackPrimaryFrames = true;
    }

    // ADAPTER: remember the primary frame of every element.
    pub fn TrackPrimaryFramesOfAllElements(&mut self) {
        self.mTrackAllPrimaryFrames = true;
    }

    // ADAPTER: the frames remembered by TrackPrimaryFramesOfElementsWithId
    pub fn TrackedPrimaryFrames(&self) -> Vec<(NodeId, nsIFrame)> {
        if !self.mTrackPrimaryFrames && !self.mTrackAllPrimaryFrames {
            return Vec::new();
        }
        let dom = &self.sd().tree.dom;
        (1..dom.nodes.len() as NodeId)
            .filter(|&n| dom.node(n).kind == Kind::Element && (self.mTrackAllPrimaryFrames || self.HasAttr(n, "id")))
            .filter_map(|n| Some((n, nsIContent::FromNodeId(n).GetPrimaryFrame()?)))
            .collect()
    }

    // Gecko: dom/base/nsIContent.h nsIContent::GetPrimaryFrame
    pub fn GetPrimaryFrame(&self, aContent: ContentId) -> Option<nsIFrame> {
        if aContent == NONE {
            return None;
        }
        if aContent >= kGeneratedContentBase {
            return self.mGenerated[(aContent - kGeneratedContentBase) as usize].mPrimaryFrame;
        }
        nsIContent::FromNodeId(aContent).GetPrimaryFrame()
    }

    // Gecko: layout/base/nsFrameManager.h nsFrameManager::GetRootFrame
    pub fn GetRootFrame(&self) -> Option<nsIFrame> {
        self.mRootFrame
    }
    // Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::GetCanvasFrame
    pub fn GetCanvasFrame(&self) -> Option<nsIFrame> {
        self.mCanvasFrame
    }
    // Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::GetRootElementFrame
    pub fn GetRootElementFrame(&self) -> Option<nsIFrame> {
        self.mRootElementFrame
    }

    // ---- ADAPTER: the DOM, the styles and the frames of the arena -------------------------------

    fn sd(&self) -> &'static StyleDom {
        unsafe { &*self.mStyleDom }
    }

    // ADAPTER: the StyleDom the frames are made from.
    pub fn StyleDomOf(&self) -> &'static StyleDom {
        self.sd()
    }

    // ADAPTER: a new frame for `aStyle` (what `new (aPresShell) nsXFrame(aStyle, ...)` does).
    fn AllocFrame(&self, aStyle: ServoArc<ComputedStyle>) -> nsIFrame {
        let wm = WritingMode::new__ComputedStyle(&aStyle);
        let pseudoType = self.mAnonBoxPseudoOfStyle.get(&(aStyle.heap_ptr() as usize)).copied();
        let frame = WithFrameTree(|t| t.AddFrame(None, nsRect::new(0, 0, 0, 0), wm, Some(aStyle)));
        if let Some(t) = pseudoType {
            frame.SetProperty(nsIFrame::AnonBoxPseudoTypeProperty(), t);
        }
        frame
    }

    // ADAPTER: nsINode::IsText
    pub fn IsText(&self, aContent: ContentId) -> bool {
        if aContent >= kGeneratedContentBase {
            return self.mGenerated[(aContent - kGeneratedContentBase) as usize].mText.is_some();
        }
        self.sd().tree.dom.node(aContent).kind == Kind::Text
    }

    // ADAPTER: nsINode::IsElement
    pub fn IsElement(&self, aContent: ContentId) -> bool {
        if aContent >= kGeneratedContentBase {
            return self.mGenerated[(aContent - kGeneratedContentBase) as usize].mText.is_none();
        }
        self.sd().tree.dom.node(aContent).kind == Kind::Element
    }

    // ADAPTER: nsINode::IsComment (and IsProcessingInstruction, and the doctype: nodes that never
    // get frames)
    pub fn IsComment(&self, aContent: ContentId) -> bool {
        aContent < kGeneratedContentBase && matches!(self.sd().tree.dom.node(aContent).kind, Kind::Comment | Kind::Doctype)
    }

    // ADAPTER: FlattenedChildIterator (dom/base/ChildIterator.h): the children of aContent in the
    // flattened tree. A shadow host's are its shadow root's children, a slot's its assigned nodes (or
    // its own children when it has none); any other node's, generated content included, are its own.
    pub fn FlattenedChildIterator(&self, aContent: ContentId) -> FlattenedChildIterator {
        if aContent < kGeneratedContentBase {
            let dom = &self.sd().tree.dom;
            if dom.shadow_root(aContent) != NONE || (dom.is_slot(aContent) && dom.containing_shadow(aContent) != NONE) {
                return FlattenedChildIterator { mList: Some(dom.flattened_children(aContent).into_iter()), mNext: NONE };
            }
        }
        FlattenedChildIterator { mList: None, mNext: self.GetFirstChild(aContent) }
    }

    // ADAPTER: nsINode::GetFirstChild
    pub fn GetFirstChild(&self, aContent: ContentId) -> ContentId {
        if aContent >= kGeneratedContentBase {
            return self.mGenerated[(aContent - kGeneratedContentBase) as usize].mFirstChild;
        }
        self.sd().tree.dom.node(aContent).first_child
    }

    // ADAPTER: nsINode::GetNextSibling
    pub fn GetNextSibling(&self, aContent: ContentId) -> ContentId {
        if aContent >= kGeneratedContentBase {
            return self.mGenerated[(aContent - kGeneratedContentBase) as usize].mNextSibling;
        }
        self.sd().tree.dom.node(aContent).next_sibling
    }

    // ADAPTER: nsINode::GetPreviousSibling (generated content has one child at most)
    pub fn GetPreviousSibling(&self, aContent: ContentId) -> ContentId {
        if aContent >= kGeneratedContentBase {
            return NONE;
        }
        self.sd().tree.dom.node(aContent).prev_sibling
    }

    // ADAPTER: the text of a Text node
    pub fn TextOf(&self, aContent: ContentId) -> &str {
        if aContent >= kGeneratedContentBase {
            return self.mGenerated[(aContent - kGeneratedContentBase) as usize].mText.as_deref().unwrap_or("");
        }
        self.sd().tree.dom.text_of(aContent)
    }

    // Gecko: dom/base/CharacterData.h CharacterData::TextIsOnlyWhitespace
    // (whitespace is the HTML one: space, tab, line feed, form feed, carriage return)
    pub fn TextIsOnlyWhitespace(&self, aContent: ContentId) -> bool {
        self.TextOf(aContent).chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'))
    }

    // Gecko: dom/base/NodeInfo.h NodeInfo::NamespaceID (generated containers are in namespace 1)
    pub fn GetNameSpaceID(&self, aContent: ContentId) -> u8 {
        if aContent >= kGeneratedContentBase {
            return 1;
        }
        self.sd().tree.dom.node(aContent).ns
    }

    // Gecko: dom/base/NodeInfo.h NodeInfo::LocalName
    pub fn LocalNameOf(&self, aContent: ContentId) -> &str {
        if aContent >= kGeneratedContentBase {
            return "mozgeneratedcontentmarker";
        }
        self.sd().tree.dom.tag(aContent)
    }

    // Gecko: dom/base/nsIContent.h IsHTMLElement(nsAtom*)
    pub fn IsHTMLElement(&self, aContent: ContentId, aName: &str) -> bool {
        aContent < kGeneratedContentBase && self.IsElement(aContent) && self.GetNameSpaceID(aContent) == kNameSpaceID_XHTML && self.LocalNameOf(aContent) == aName
    }

    // Gecko: dom/base/Element.h Element::HasAttr
    pub fn HasAttr(&self, aContent: ContentId, aName: &str) -> bool {
        if aContent >= kGeneratedContentBase {
            return false;
        }
        let dom = &self.sd().tree.dom;
        dom.attrs_of(aContent).iter().any(|a| dom.names.get(a.name) == aName)
    }

    // Gecko: dom/base/Element.h Element::GetAttr (the value of an attribute of no namespace)
    pub fn GetAttr(&self, aContent: ContentId, aName: &str) -> Option<&str> {
        if aContent >= kGeneratedContentBase {
            return None;
        }
        let dom = &self.sd().tree.dom;
        dom.attrs_of(aContent).iter().find(|a| dom.names.get(a.name) == aName).map(|a| dom.attr_value(a))
    }

    // Gecko: dom/base/nsAttrValue.cpp nsAttrValue::ParseClampedNonNegativeInt (what the table cell and
    // column elements use for `colspan`, `rowspan` and `span`): HTML's rules for parsing a
    // non-negative integer; a missing, negative or malformed value is aDefault, and the result is
    // clamped to [aMin, aMax].
    pub fn ParseClampedNonNegativeInt(&self, aContent: ContentId, aName: &str, aDefault: i32, aMin: i32, aMax: i32) -> i32 {
        let value = match self.GetAttr(aContent, aName) {
            Some(v) => v,
            None => return aDefault,
        };
        let v = value.trim_start_matches([' ', '\t', '\n', '\x0c', '\r']);
        let v = v.strip_prefix('+').unwrap_or(v);
        let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return aDefault;
        }
        // an overflow clamps to the maximum
        let n = digits.parse::<i64>().unwrap_or(i64::MAX);
        n.clamp(aMin as i64, aMax as i64) as i32
    }

    // ADAPTER: the nsIContent a frame holds for aContent. A node of the arena DOM is its own content
    // (the frame reads the node from the DOM, see nsIContent), so every frame made for one node has
    // the same content, as in Gecko. Generated content has no DOM node: its first MakeContent makes
    // a synthetic content (the text, the language of the parent) and the later ones give it again.
    pub fn MakeContent(&self, aContent: ContentId) -> nsIContent {
        if aContent < kGeneratedContentBase {
            return nsIContent::FromNodeId(aContent);
        }
        let generated = &self.mGenerated[(aContent - kGeneratedContentBase) as usize];
        if let Some(content) = nsIContent::FromId(generated.mSyntheticContent.get()) {
            return content;
        }
        let content = match &generated.mText {
            Some(text) => nsIContent::TextNode(text),
            None => nsIContent::ForeignElement(self.GetNameSpaceID(aContent)),
        };
        let content = content.WithLanguage(self.LanguageOfContent(aContent));
        generated.mSyntheticContent.set(content.Id());
        content
    }

    // ADAPTER: the language of a node (nsIContent::mLanguage): a node of the DOM finds its own; generated
    // content has its parent's.
    fn LanguageOfContent(&self, aContent: ContentId) -> Option<nsAtom> {
        if aContent >= kGeneratedContentBase {
            let parent = self.mGenerated[(aContent - kGeneratedContentBase) as usize].mParent;
            return self.LanguageOfContent(parent);
        }
        nsIContent::FromNodeId(aContent).mLanguage()
    }

    // ADAPTER: a generated content node
    fn NewGeneratedContent(&mut self, aParent: ContentId, aText: Option<String>, aStyle: Option<ServoArc<ComputedStyle>>) -> ContentId {
        self.mGenerated.push(GeneratedContent { mParent: aParent, mFirstChild: NONE, mNextSibling: NONE, mText: aText, mStyle: aStyle, mPrimaryFrame: None, mSyntheticContent: std::cell::Cell::new(0) });
        kGeneratedContentBase + (self.mGenerated.len() - 1) as u32
    }

    // Gecko: dom/base/nsIContent.h nsIContent::SetPrimaryFrame
    fn SetPrimaryFrame(&mut self, aContent: ContentId, aFrame: nsIFrame) {
        if aContent >= kGeneratedContentBase {
            self.mGenerated[(aContent - kGeneratedContentBase) as usize].mPrimaryFrame = Some(aFrame);
        } else {
            nsIContent::FromNodeId(aContent).SetPrimaryFrame(Some(aFrame));
        }
    }

    // ADAPTER: the style of an anonymous box whose parent has style aParentStyle: its inherited
    // properties, the initial values for the rest, and the given display (ServoStyleSet::
    // ResolveInheritingAnonymousBoxStyle and ResolveStyleForText). One per parent style and display.
    fn AnonymousStyle(&mut self, aParentStyle: &ServoArc<ComputedStyle>, aDisplay: AnonymousDisplay) -> ServoArc<ComputedStyle> {
        let key = (aParentStyle.heap_ptr() as usize, aDisplay);
        if let Some(s) = self.mAnonymousStyles.get(&key) {
            return s.1.clone();
        }
        let display = match aDisplay {
            AnonymousDisplay::Inline => StyleDisplayValue::Inline,
            AnonymousDisplay::Block => StyleDisplayValue::Block,
            AnonymousDisplay::InlineBlock => StyleDisplayValue::InlineBlock,
        };
        let style = self.sd().anonymous_style(Some(&**aParentStyle), Some(display));
        self.mAnonymousStyles.insert(key, (aParentStyle.clone(), style.clone()));
        style
    }

    // ADAPTER: aStyle with `display` replaced (the frames that stand in for the ones not ported
    // are laid out with the display of the frame class that does the same work here). The result is
    // not cached: a sheet of such frames each get their own copy of the style.
    fn StyleWithDisplay(aStyle: &ServoArc<ComputedStyle>, aDisplay: StyleDisplayValue) -> ServoArc<ComputedStyle> {
        let mut style = aStyle.clone();
        ServoArc::make_mut(&mut style).mutate_box().set_display(aDisplay);
        style
    }

    // Gecko: layout/style/ServoStyleSet.cpp ServoStyleSet::ResolveInheritingAnonymousBoxStyle
    // ADAPTER: the table anonymous boxes (`::-moz-table`, `::-moz-table-wrapper`, `::-moz-cell-content`, ...)
    // are rules of ua.css in Gecko (`display: table; box-sizing: border-box`, and a list of `inherit`
    // for the wrapper); Stylo's servo configuration has no such pseudo-elements, so the style is made
    // from the anonymous style of the parent with the declarations of those rules applied by hand.
    pub fn ResolveInheritingAnonymousBoxStyle(&mut self, aPseudoType: PseudoStyleType, aParentStyle: &ServoArc<ComputedStyle>) -> ServoArc<ComputedStyle> {
        let key = (aParentStyle.heap_ptr() as usize, aPseudoType);
        if let Some(s) = self.mAnonBoxStyles.get(&key) {
            return s.1.clone();
        }
        let style = nsCSSFrameConstructor::MakeInheritingAnonBoxStyle(self.sd(), aPseudoType, aParentStyle);
        self.mAnonBoxStyles.insert(key, (aParentStyle.clone(), style.clone()));
        self.mAnonBoxPseudoOfStyle.insert(style.heap_ptr() as usize, aPseudoType);
        style
    }

    // ADAPTER: see ResolveInheritingAnonymousBoxStyle.
    fn MakeInheritingAnonBoxStyle(aStyleDom: &StyleDom, aPseudoType: PseudoStyleType, aParentStyle: &ServoArc<ComputedStyle>) -> ServoArc<ComputedStyle> {
        use mrt_style::style::computed_values::box_sizing::T as BoxSizing;
        let parentBox = aParentStyle.get_box();
        let display = match aPseudoType {
            // *|*::-moz-table { display: table; box-sizing: border-box; }
            PseudoStyleType::MozTable => StyleDisplayValue::Table,
            // *|*::-moz-inline-table { display: inline-table; box-sizing: border-box; }
            PseudoStyleType::MozInlineTable => StyleDisplayValue::InlineTable,
            // *|*::-moz-table-wrapper { display: inherit; ... }
            PseudoStyleType::MozTableWrapper => parentBox.display,
            PseudoStyleType::MozTableRowGroup => StyleDisplayValue::TableRowGroup,
            PseudoStyleType::MozTableRow => StyleDisplayValue::TableRow,
            // *|*::-moz-table-cell { display: table-cell; white-space: inherit; } (white-space inherits)
            PseudoStyleType::MozTableCell => StyleDisplayValue::TableCell,
            // *|*::-moz-cell-content { display: block; ... }
            PseudoStyleType::MozCellContent => StyleDisplayValue::Block,
            // *|*::-moz-anonymous-item { display: block; }
            PseudoStyleType::MozAnonymousItem => StyleDisplayValue::Block,
            _ => unreachable!("not an inheriting anonymous box"),
        };
        let mut style = aStyleDom.anonymous_style(Some(&**aParentStyle), Some(display));
        let s = ServoArc::make_mut(&mut style);
        match aPseudoType {
            PseudoStyleType::MozTable | PseudoStyleType::MozInlineTable => {
                s.mutate_position().box_sizing = BoxSizing::BorderBox;
            }
            PseudoStyleType::MozTableWrapper => {
                // The inherited properties here need to be safe to have on both the
                // table and the table wrapper, generally because code ignores them
                // for the table.
                // (the ones that matter to layout: display, margin, float, clear, position, the offsets,
                // z-index, vertical-align (alignment-baseline, baseline-shift, baseline-source), and the flex and grid item properties)
                let b = s.mutate_box();
                b.original_display = parentBox.original_display;
                b.position = parentBox.position;
                b.float = parentBox.float;
                b.clear = parentBox.clear;
                b.alignment_baseline = parentBox.alignment_baseline;
                b.baseline_shift = parentBox.baseline_shift.clone();
                b.baseline_source = parentBox.baseline_source;
                *s.mutate_margin() = aParentStyle.get_margin().clone();
                let parentPosition = aParentStyle.get_position();
                let p = s.mutate_position();
                p.top = parentPosition.top.clone();
                p.right = parentPosition.right.clone();
                p.bottom = parentPosition.bottom.clone();
                p.left = parentPosition.left.clone();
                p.z_index = parentPosition.z_index.clone();
                p.align_self = parentPosition.align_self.clone();
                p.justify_self = parentPosition.justify_self.clone();
                p.order = parentPosition.order.clone();
            }
            PseudoStyleType::MozCellContent => {
                // (unicode-bidi, text-overflow, overflow, resize, padding and box-decoration-break are
                // `inherit` here; the first two and the last two do not matter to layout)
                let b = s.mutate_box();
                b.overflow_x = parentBox.overflow_x;
                b.overflow_y = parentBox.overflow_y;
                *s.mutate_padding() = aParentStyle.get_padding().clone();
            }
            _ => {}
        }
        style
    }

    // Gecko: layout/style/ServoStyleSet.cpp ServoStyleSet::ResolveNonInheritingAnonymousBoxStyle
    // ADAPTER: only the two anonymous boxes of ua.css that are not given a parent style, the synthetic
    // column group and column (`display: table-column-group` and `table-column`, `visibility:
    // hidden`, the initial values for the rest).
    pub fn ResolveNonInheritingAnonymousBoxStyle(&mut self, aPseudoType: PseudoStyleType) -> ServoArc<ComputedStyle> {
        let style = nsCSSFrameConstructor::MakeNonInheritingAnonBoxStyle(self.sd(), aPseudoType);
        // (the style is not cached, so its address may be reused once it is freed: the entry is
        // overwritten by the next style made there, or cleared with the caches)
        self.mAnonBoxPseudoOfStyle.insert(style.heap_ptr() as usize, aPseudoType);
        style
    }

    // ADAPTER: see ResolveNonInheritingAnonymousBoxStyle.
    fn MakeNonInheritingAnonBoxStyle(aStyleDom: &StyleDom, aPseudoType: PseudoStyleType) -> ServoArc<ComputedStyle> {
        use mrt_style::style::computed_values::visibility::T as Visibility;
        let display = match aPseudoType {
            PseudoStyleType::MozTableColumnGroup => StyleDisplayValue::TableColumnGroup,
            PseudoStyleType::MozTableColumn => StyleDisplayValue::TableColumn,
            _ => unreachable!("not a non-inheriting anonymous box"),
        };
        let mut style = aStyleDom.anonymous_style(None, Some(display));
        ServoArc::make_mut(&mut style).mutate_inherited_box().visibility = Visibility::Hidden;
        style
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ResolveComputedStyle
    pub fn ResolveComputedStyle(&mut self, aContent: ContentId) -> Option<ServoArc<ComputedStyle>> {
        if self.IsElement(aContent) {
            if aContent >= kGeneratedContentBase {
                return self.mGenerated[(aContent - kGeneratedContentBase) as usize].mStyle.clone();
            }
            return self.sd().computed(aContent);
        }

        MOZ_ASSERT!(self.IsText(aContent), "shouldn't waste time creating ComputedStyles for comments and processing instructions");

        // Gecko: aContent->GetFlattenedTreeParentElement()
        let parent = if aContent >= kGeneratedContentBase {
            self.mGenerated[(aContent - kGeneratedContentBase) as usize].mParent
        } else {
            self.sd().tree.dom.flattened_parent(aContent)
        };
        MOZ_ASSERT!(parent != NONE, "Text out of the flattened tree?");
        let parentStyle = self.ResolveComputedStyle(parent)?;
        Some(self.AnonymousStyle(&parentStyle, AnonymousDisplay::Inline))
    }

    // ---- the root frames -------------------------------------------------------------------------

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructRootFrame
    pub fn ConstructRootFrame(&mut self) -> ViewportFrame {
        // --------- BUILD VIEWPORT -----------
        let viewportPseudoStyle = self.sd().anonymous_style(None, Some(StyleDisplayValue::Block));
        let viewportFrame = NS_NewViewportFrame(self.AllocFrame(viewportPseudoStyle));

        // XXXbz do we _have_ to pass a null content pointer to that frame?
        // Would it really kill us to pass in the root element or something?
        // What would that break?
        viewportFrame.Init(None, None, None);

        viewportFrame.AddStateBits(NS_FRAME_OWNS_ANON_BOXES);

        // mPresShell->SetNeedsWindowPropertiesSync(); NOT PORTED: there is no window.

        // Make it an absolute container for fixed-pos elements
        viewportFrame.AddStateBits(NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN);
        viewportFrame.MarkAsAbsoluteContainingBlock();

        self.mRootFrame = Some(viewportFrame.0);
        viewportFrame
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::SetUpDocElementContainingBlock
    pub fn SetUpDocElementContainingBlock(&mut self, aDocElement: ContentId) {
        MOZ_ASSERT!(aDocElement != NONE, "No element?");

        /*
          how the root frame hierarchy should look

        Galley presentation, with scrolling:

            ViewportFrame [fixed-cb]
              ScrollContainerFrame (if needed)
                nsCanvasFrame [abs-cb]
                  root element frame (nsBlockFrame, SVGOuterSVGFrame,
                                      nsTableWrapperFrame, nsPlaceholderFrame,
                                      nsFlexContainerFrame, nsGridContainerFrame)

          Print presentation, print-preview presentation and XUL: NOT PORTED.

        Meaning of nsCSSFrameConstructor fields:
          mRootElementFrame is "root element frame".  This is the primary frame for
            the root element.
          mDocElementContainingBlock is the parent of mRootElementFrame
            (i.e. nsCanvasFrame)
        */

        // --------- CREATE ROOT FRAME -------

        // Create the root frame. The document element's frame is a child of the
        // root frame.
        //
        // The root frame serves two purposes:
        // - reserves space for any margins needed for the document element's frame
        // - renders the document element's background. This ensures the background
        //   covers the entire canvas as specified by the CSS2 spec

        let viewportFrame = self.mRootFrame.unwrap();
        let viewportPseudoStyle = viewportFrame.Style();

        let canvasStyle = self.AnonymousStyle(&viewportPseudoStyle, AnonymousDisplay::Block);
        let rootCanvasFrame = NS_NewCanvasFrame(self.AllocFrame(canvasStyle));
        self.mCanvasFrame = Some(rootCanvasFrame.0);
        self.mDocElementContainingBlock = Some(rootCanvasFrame.0);

        // --------- IF SCROLLABLE WRAP IN SCROLLFRAME --------
        // NOT PORTED: scroll frames. The canvas is the child of the viewport and is reflowed with
        // an unconstrained block-size, which is what the scroll frame would hand it.
        let newFrame = rootCanvasFrame.0;
        let parentFrame = viewportFrame;

        // (rootCanvasFrame->Init(aDocElement, ...): the canvas gets no content here, see nsCanvasFrame.rs)
        self.InitFrame(newFrame, None, Some(parentFrame));

        if viewportFrame.HasAnyStateBits(NS_FRAME_FIRST_REFLOW) {
            SetInitialSingleChild(viewportFrame, newFrame);
        } else {
            nsContainerFrame::from_frame(viewportFrame).AppendFrames(FrameChildListID::Principal, nsFrameList::new(Some(newFrame), Some(newFrame)));
        }
    }

    // Construct the frames for the document element.  This can return null if the
    // document element is display:none, or if it's an SVG element that's not
    // <svg>, etc.
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructDocElementFrame
    pub fn ConstructDocElementFrame(&mut self, aDocElement: ContentId) -> Option<nsIFrame> {
        MOZ_ASSERT!(self.GetRootFrame().is_some(), "No viewport?  Someone forgot to call ConstructRootFrame!");
        MOZ_ASSERT!(self.mDocElementContainingBlock.is_none(), "Shouldn't have a doc element containing block here");

        // Ensure the document element is styled at this point.
        // (the caller resolved the styles) Make sure to call UpdateViewportScrollStylesOverride before
        // SetUpDocElementContainingBlock: NOT PORTED, there is no scroll frame.

        self.SetUpDocElementContainingBlock(aDocElement);

        // This has the side-effect of getting `mFrameTreeState` from our docshell.
        // NOT PORTED: frame state.

        NS_ASSERTION!(self.mDocElementContainingBlock.is_some(), "Should have parent by now");
        // (GetAbsoluteContainingBlock(mDocElementContainingBlock, FIXED_POS): the canvas is never a
        // fixed-pos containing block, so it is the viewport, the root frame)
        let mut state = nsFrameConstructorState::new(self.mRootFrame, None, None, self.mRootFrame);

        let computedStyle = self.ResolveComputedStyle(aDocElement)?;

        let display = nsStyleDisplay::new(computedStyle.clone());

        // --------- IF SCROLLABLE WRAP IN SCROLLFRAME --------
        // NOT PORTED: scroll frames.

        if display.mDisplay() == StyleDisplayValue::None {
            return None;
        }

        // This implements "The Principal Writing Mode".
        // https://drafts.csswg.org/css-writing-modes-3/#principal-flow
        //
        // If there's a <body> element in an HTML document, its writing-mode,
        // direction, and text-orientation override the root element's used value.
        // NOT PORTED: the root's own writing mode is used (horizontal pages).

        // Push the absolute containing block now so we can absolutely position the
        // root element
        let mut canvasCbSaveState = nsFrameConstructorSaveState::default();
        self.mCanvasFrame.unwrap().AddStateBits(NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN);

        state.PushAbsoluteContainingBlock(self.mCanvasFrame, self.mCanvasFrame, &mut canvasCbSaveState);

        // (mCanvasFrame != mDocElementContainingBlock and docElementCbSaveState: the doc element
        // containing block is always the canvas here, there is no print presentation.)

        // The rules from CSS 2.1, section 9.2.4, have already been applied
        // by the style system, so we can assume that display->mDisplay is
        // either NONE, BLOCK, or TABLE.

        // contentFrame is the primary frame for the root element. frameList contains
        // the children of the initial containing block.
        let mut frameList = nsFrameList::default();
        let mut absoluteSaveState = nsFrameConstructorSaveState::default();
        let mDocElementContainingBlock = self.mDocElementContainingBlock;

        // The SVG root, table and ruby cases: NOT PORTED. A table root is laid out as
        // a block. A flex or grid root is below.
        let (style, display) = if matches!(display.DisplayInside(), StyleDisplayInside::Flow | StyleDisplayInside::FlowRoot | StyleDisplayInside::Flex | StyleDisplayInside::Grid) {
            (computedStyle.clone(), display)
        } else {
            let style = nsCSSFrameConstructor::StyleWithDisplay(&computedStyle, StyleDisplayValue::Block);
            (style.clone(), nsStyleDisplay::new(style))
        };
        let mut contentFrame;
        let mut processChildren = false;
        if display.DisplayInside() == StyleDisplayInside::Flex || display.DisplayInside() == StyleDisplayInside::Grid {
            let func = if display.DisplayInside() == StyleDisplayInside::Grid { ToCreationFunc_NS_NewGridContainerFrame } else { ToCreationFunc_NS_NewFlexContainerFrame };
            contentFrame = func(self.AllocFrame(style.clone()));
            let geometricParent = state.GetGeometricParent(&display, mDocElementContainingBlock);
            self.InitAndRestoreFrame(&state, aDocElement, geometricParent, contentFrame);
            state.AddChild(self, contentFrame, &mut frameList, aDocElement, mDocElementContainingBlock, true, true, false, None);
            processChildren = true;

            contentFrame.AddStateBits(NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN);
            if contentFrame.IsAbsPosContainingBlock() {
                state.PushAbsoluteContainingBlock(Some(contentFrame), Some(contentFrame), &mut absoluteSaveState);
            }
        } else {
            contentFrame = NS_NewBlockFrame(self.AllocFrame(style.clone())).0;
            let geometricParent = state.GetGeometricParent(&display, mDocElementContainingBlock);
            let positioned = if contentFrame.IsAbsPosContainingBlock() { Some(contentFrame) } else { None };
            self.ConstructBlock(&mut state, aDocElement, geometricParent, mDocElementContainingBlock, &style, &mut contentFrame, &mut frameList, positioned);
        }

        MOZ_ASSERT!(frameList.FirstChild().is_some());
        MOZ_ASSERT!(if processChildren { self.mRootElementFrame.is_none() } else { self.mRootElementFrame == Some(contentFrame) }, "unexpected mRootElementFrame");
        if processChildren {
            self.mRootElementFrame = Some(contentFrame);
        }

        // Figure out which frame has the main style for the document element,
        // assigning it to mRootElementStyleFrame.
        // Backgrounds should be propagated from that frame to the viewport.
        // (GetParentComputedStyle: NOT PORTED, the root block is its own style frame)
        self.mRootElementStyleFrame = self.mRootElementFrame;

        if processChildren {
            // Still need to process the child content
            let mut childList = nsFrameList::default();

            NS_ASSERTION!(!contentFrame.IsBlockFrameOrSubclass(), "Only XUL frames should reach here");

            let mut floatSaveState = nsFrameConstructorSaveState::default();
            state.MaybePushFloatContainingBlock(contentFrame, &mut floatSaveState);

            self.ProcessChildren(&mut state, aDocElement, &style, contentFrame, true, &mut childList, false, None);

            // Set the initial child lists
            nsContainerFrame::from_frame(contentFrame).SetInitialChildList(FrameChildListID::Principal, childList);
            floatSaveState.Restore(&mut state); // ~nsFrameConstructorSaveState
        }

        let newFrame = frameList.FirstChild();
        // set the primary frame
        self.SetPrimaryFrame(aDocElement, contentFrame);
        nsContainerFrame::from_frame(mDocElementContainingBlock.unwrap()).SetInitialChildList(FrameChildListID::Principal, frameList);

        // (the destructors at the end of the C++ function, in reverse order of declaration)
        absoluteSaveState.Restore(&mut state); // ~nsFrameConstructorSaveState
        canvasCbSaveState.Restore(&mut state); // ~nsFrameConstructorSaveState
        state.ProcessFrameInsertionsForAllLists(); // ~nsFrameConstructorState

        // ConstructAnonymousContentForRoot(...): NOT PORTED, there is no anonymous content.

        newFrame
    }
}

// ---- placeholders, text, find-data ---------------------------------------------------------------

impl nsCSSFrameConstructor {
    // ADAPTER: nsIFrame::Init for every frame class: content and parent, then the class's own Init.
    // (nsIFrame::Init, nsContainerFrame::Init, ViewportFrame::Init, nsBlockFrame::Init,
    // nsTextFrame::Init)
    fn InitFrame(&self, aFrame: nsIFrame, aContent: Option<nsIContent>, aParent: Option<nsIFrame>) {
        // nsIFrame::Init
        aFrame.SetContent(aContent.clone());
        if let Some(p) = aParent {
            aFrame.SetParent(p);
        }
        // nsIFrame::DidSetComputedStyle: If the page contains markup that overrides text direction, and
        // does not contain any characters that would activate the Unicode
        // bidi algorithm, we need to call |SetBidiEnabled| on the pres
        // context before reflow starts.  See bug 115921.
        if aFrame.StyleVisibility().mDirection() == StyleDirection::Rtl {
            aFrame.PresContext().SetBidiEnabled();
        }
        match aFrame.Type() {
            LayoutFrameType::Block => nsBlockFrame::from_frame(aFrame).Init(aContent, aParent.unwrap_or(aFrame), None),
            LayoutFrameType::Text => nsTextFrame::from_frame(aFrame).Init(aContent, aParent.unwrap(), None),
            LayoutFrameType::Table => nsTableFrame::from_frame(aFrame).Init(aContent, aParent.unwrap(), None),
            LayoutFrameType::TableRowGroup => nsTableRowGroupFrame::from_frame(aFrame).Init(aContent, aParent.unwrap(), None),
            LayoutFrameType::TableRow => nsTableRowFrame::from_frame(aFrame).Init(aContent, aParent.unwrap(), None),
            LayoutFrameType::TableCell => nsTableCellFrame::from_frame(aFrame).Init(aContent, aParent.unwrap(), None),
            LayoutFrameType::TableCol => nsTableColFrame::from_frame(aFrame).Init(aContent, aParent.unwrap(), None),
            LayoutFrameType::TableColGroup => nsTableColGroupFrame::from_frame(aFrame).Init(aContent, aParent.unwrap(), None),
            LayoutFrameType::FlexContainer => nsFlexContainerFrame::from_frame(aFrame).Init(aContent, aParent.unwrap(), None),
            LayoutFrameType::GridContainer => nsGridContainerFrame::from_frame(aFrame).Init(aContent, aParent.unwrap(), None),
            // (the table wrapper has no Init of its own: nsContainerFrame::Init, which does nothing here)
            _ => {}
        }
    }

    // ADAPTER: see mFrameContent.
    fn RememberContentOfFrame(&mut self, aFrame: nsIFrame, aContent: ContentId) {
        let i = aFrame.0 as usize;
        if self.mFrameContent.len() <= i {
            self.mFrameContent.resize(i + 1, NONE);
        }
        self.mFrameContent[i] = aContent;
    }

    // ADAPTER: the ContentId that aFrame->GetContent() stands for (NONE: none).
    fn ContentOfFrame(&self, aFrame: nsIFrame) -> ContentId {
        self.mFrameContent.get(aFrame.0 as usize).copied().unwrap_or(NONE)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreatePlaceholderFrameFor
    pub fn CreatePlaceholderFrameFor(&mut self, aContent: ContentId, aFrame: nsIFrame, aParentFrame: Option<nsIFrame>, aPrevInFlow: Option<nsIFrame>, aTypeBit: nsFrameState) -> nsIFrame {
        // ResolveStyleForPlaceholder(): an anonymous box of the parent's style
        let parentStyle = aParentFrame.map(|p| p.Style()).unwrap_or_else(|| aFrame.Style());
        let placeholderStyle = self.AnonymousStyle(&parentStyle, AnonymousDisplay::Inline);

        // The placeholder frame gets a pseudo style.
        let placeholderFrame = NS_NewPlaceholderFrame(self.AllocFrame(placeholderStyle), aTypeBit);

        let content = self.MakeContent(aContent);
        self.InitFrame(placeholderFrame.0, Some(content), aParentFrame);

        // Associate the placeholder/out-of-flow with each other.
        placeholderFrame.SetOutOfFlowFrame(aFrame);
        // aFrame->SetProperty(nsIFrame::PlaceholderFrameProperty(), placeholderFrame);
        // (SetOutOfFlowFrame keeps the link in both directions)

        aFrame.AddStateBits(NS_FRAME_OUT_OF_FLOW);

        placeholderFrame.0
    }

    // aParentFrame might be null.  If it is, that means it was an inline frame.
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindTextData
    fn FindTextData(&self, aTextContent: ContentId, aParentFrame: Option<nsIFrame>) -> Option<&'static FrameConstructionData> {
        if let Some(p) = aParentFrame {
            // IsFrameForSVG(aParentFrame): the content of the frame is an SVG element
            if p.GetContent().map_or(false, |c| c.IsSVGElement()) {
                // (the SVG text frames are NOT PORTED)
                return None;
            }
        }

        Some(&sTextData)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructTextFrame
    fn ConstructTextFrame(&mut self, aData: &'static FrameConstructionData, aState: &mut nsFrameConstructorState, aContent: ContentId, aParentFrame: Option<nsIFrame>, aComputedStyle: &ServoArc<ComputedStyle>, aFrameList: &mut nsFrameList) {
        let creationFunc = match aData.mFunc {
            FrameConstructionData_Func::mCreationFunc(Some(f)) => f,
            _ => unreachable!("Must have frame construction data"),
        };

        let newFrame = creationFunc(self.AllocFrame(aComputedStyle.clone()));

        self.InitAndRestoreFrame(aState, aContent, aParentFrame, newFrame);

        // We never need to create a view for a text frame.

        // if (newFrame->IsGeneratedContentFrame()) {...} NOT PORTED: counters and quotes.

        // Add the newly constructed frame to the flow
        aFrameList.AppendFrame(None, newFrame);

        if !aState.mCreatingExtraFrames {
            self.SetPrimaryFrame(aContent, newFrame);
        }
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindDataByTag
    // ADAPTER: the tables are `FrameConstructionDataByTag` arrays below; a tag whose entry is a data
    // getter is resolved here.
    fn FindDataByTag(&self, aContent: ContentId, aStyle: &ServoArc<ComputedStyle>, aDataPtr: &'static [FrameConstructionDataByTag]) -> Option<&'static FrameConstructionData> {
        let tag = self.LocalNameOf(aContent);
        for curData in aDataPtr {
            if curData.mTag == tag {
                let data = &curData.mData;
                if (data.mBits & FCDATA_FUNC_IS_DATA_GETTER) != 0 {
                    if let FrameConstructionData_Func::mDataGetter(getter) = data.mFunc {
                        return getter(self, aContent, aStyle);
                    }
                }

                return Some(data);
            }
        }

        None
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindHTMLData
    fn FindHTMLData(&self, aContent: ContentId, aParentFrame: Option<nsIFrame>, aStyle: &ServoArc<ComputedStyle>) -> Option<&'static FrameConstructionData> {
        MOZ_ASSERT!(self.GetNameSpaceID(aContent) == kNameSpaceID_XHTML);

        // The pseudo-element cases (view transitions, select content, file content, reveal and
        // number spin box): NOT PORTED, there are no such pseudo-elements.

        // ADAPTER: the replaced elements and form controls, each of which has a frame class in Gecko
        // (NS_NewImageFrame, NS_NewInputButtonControlFrame, NS_NewSubDocumentFrame, ...) that is not
        // ported. They get a leaf block frame (ConstructReplacedFrameStub).
        // The entries not listed here (object, embed, fieldset, canvas, details, frameset) are
        // constructed by their display, see below.
        static sHTMLData: [FrameConstructionDataByTag; 13] = [
            // SIMPLE_TAG_CHAIN(img, nsCSSFrameConstructor::FindImgData)
            FrameConstructionDataByTag { mTag: "img", mData: FrameConstructionData::new__FrameConstructionDataGetter(nsCSSFrameConstructor::FindImgData, 0) },
            FrameConstructionDataByTag { mTag: "br", mData: FrameConstructionData::new(Some(ToCreationFunc_NS_NewBRFrame), FCDATA_IS_LINE_PARTICIPANT | FCDATA_IS_LINE_BREAK) },
            // SIMPLE_TAG_CREATE(wbr, NS_NewWBRFrame)
            FrameConstructionDataByTag { mTag: "wbr", mData: FrameConstructionData::new(Some(ToCreationFunc_NS_NewWBRFrame), 0) },
            // SIMPLE_TAG_CHAIN(button, nsCSSFrameConstructor::FindHTMLButtonData)
            FrameConstructionDataByTag { mTag: "button", mData: FrameConstructionData::new__FrameConstructionDataGetter(nsCSSFrameConstructor::FindHTMLButtonData, 0) },
            // SIMPLE_TAG_CHAIN(input, nsCSSFrameConstructor::FindInputData)
            FrameConstructionDataByTag { mTag: "input", mData: FrameConstructionData::new__FrameConstructionDataGetter(nsCSSFrameConstructor::FindInputData, 0) },
            // SIMPLE_TAG_CREATE(textarea, &nsCSSFrameConstructor::ConstructTextControl)
            FrameConstructionDataByTag { mTag: "textarea", mData: FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructTextControl, 0) },
            // SIMPLE_TAG_CHAIN(select, nsCSSFrameConstructor::FindSelectData)
            FrameConstructionDataByTag { mTag: "select", mData: FrameConstructionData::new__FrameConstructionDataGetter(nsCSSFrameConstructor::FindSelectData, 0) },
            // SIMPLE_TAG_CREATE(iframe, NS_NewSubDocumentFrame)
            FrameConstructionDataByTag { mTag: "iframe", mData: FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructSubDocument, FCDATA_ADAPTER_LEAF) },
            // SIMPLE_TAG_CREATE(video, NS_NewHTMLVideoFrame) and (audio, NS_NewHTMLVideoFrame)
            FrameConstructionDataByTag { mTag: "video", mData: FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructMediaElement, 0) },
            FrameConstructionDataByTag { mTag: "audio", mData: FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructMediaElement, 0) },
            FrameConstructionDataByTag { mTag: "progress", mData: FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructReplacedFrameStub, FCDATA_ADAPTER_LEAF) },
            FrameConstructionDataByTag { mTag: "meter", mData: FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructReplacedFrameStub, FCDATA_ADAPTER_LEAF) },
            // SIMPLE_TAG_CHAIN(details, nsCSSFrameConstructor::FindDetailsData)
            FrameConstructionDataByTag { mTag: "details", mData: FrameConstructionData::new__FrameConstructionDataGetter(nsCSSFrameConstructor::FindDetailsData, 0) },
        ];

        self.FindDataByTag(aContent, aStyle, &sHTMLData)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindImgData
    // ADAPTER: the frame of an image is the leaf block ConstructReplacedFrameStub makes (NS_NewImageFrame
    // is not ported). A broken <img> with alt text gets no image frame: it is an inline whose ::before
    // holds the alt text (see CreateGeneratedContentItem).
    fn FindImgData(&self, aContent: ContentId, aStyle: &ServoArc<ComputedStyle>) -> Option<&'static FrameConstructionData> {
        if nsImageFrame::ImageFrameTypeFor(self, aContent, aStyle) != nsImageFrame_ImageFrameType::ForElementRequest {
            // content: url gets handled by the generic code-path.
            return None;
        }

        // sImgData(NS_NewImageFrame)
        Some(&sReplacedStubData)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindDetailsData
    // NOT PORTED: the details frame; <details> is a block (by its display). What the closed state hides
    // is handled by ShouldCreateItemsForChild.
    fn FindDetailsData(&self, aContent: ContentId, aStyle: &ServoArc<ComputedStyle>) -> Option<&'static FrameConstructionData> {
        None
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindHTMLButtonData
    fn FindHTMLButtonData(&self, aContent: ContentId, aStyle: &ServoArc<ComputedStyle>) -> Option<&'static FrameConstructionData> {
        // Buttons force a (maybe inline) block unless their display is flex or grid.
        // TODO(emilio): It'd be good to remove this restriction more broadly.
        // There are some tests that expect block baselines on e.g. a `display: table`
        // button, but seems like it would be doable.
        let disp = nsStyleDisplay::new(aStyle.clone());
        let respectDisplay = {
            if disp.IsInlineFlow() {
                // For compat, `display: inline` and co need to create an inline-block.
                false
            } else {
                matches!(disp.DisplayInside(), StyleDisplayInside::Flex | StyleDisplayInside::Grid | StyleDisplayInside::FlowRoot)
            }
        };
        if respectDisplay {
            return None;
        }
        static sBlockData: [FrameConstructionData; 2] = [
            FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructNonScrollableBlock, 0),
            FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructScrollableBlock, 0),
        ];
        Some(&sBlockData[disp.IsScrollableOverflow() as usize])
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindInputData
    // The checkbox, radio and the text control types have their frames (reduced, see layout/forms), the
    // button types are blocks like <button>; NOT PORTED: the other input frame classes (the file, color,
    // range, date and time, and image types). A hidden input has display:none by the user-agent sheet and never gets here; the
    // others are leaf blocks.
    fn FindInputData(&self, aContent: ContentId, aStyle: &ServoArc<ComputedStyle>) -> Option<&'static FrameConstructionData> {
        // sInputData: SIMPLE_INT_CREATE(FormControlType::..., ...) by the type of the input
        static sCheckboxRadioData: FrameConstructionData = FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructCheckboxRadio, 0);
        static sTextControlData: FrameConstructionData = FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructTextControl, 0);
        // HTMLInputElement's `type`: ASCII case-insensitive, an unknown or missing one is `text`
        let ty = self.GetAttr(aContent, "type").map(|t| t.to_ascii_lowercase()).unwrap_or_default();
        match ty.as_str() {
            "checkbox" | "radio" => Some(&sCheckboxRadioData),
            // NS_NewInputButtonControlFrame: a block with the label as its text (see MozAltContentTextFor)
            "button" | "submit" | "reset" => {
                static sInputButtonData: FrameConstructionData = FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructInputButtonControl, 0);
                Some(&sInputButtonData)
            }
            "file" | "image" | "range" | "color" | "hidden" | "date" | "datetime-local" | "month" | "time" | "week" => Some(&sReplacedStubData),
            // text, email, tel, url, password, search, number (and the unknown types)
            _ => Some(&sTextControlData),
        }
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindSelectData
    // A select is a drop-down (nsComboboxControlFrame) unless it is `multiple` or its size is above 1 (then
    // an nsListControlFrame, a list box).
    fn FindSelectData(&self, aContent: ContentId, aStyle: &ServoArc<ComputedStyle>) -> Option<&'static FrameConstructionData> {
        static sComboboxData: FrameConstructionData = FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructComboboxControl, 0);
        static sListData: FrameConstructionData = FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructListControl, 0);
        let (size, multiple) = self.SelectSizeAndMultiple(aContent);
        Some(if !multiple && size <= 1 { &sComboboxData } else { &sListData })
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindDisplayData
    fn FindDisplayData(&self, aDisplay: &nsStyleDisplay, aContent: ContentId) -> Option<&'static FrameConstructionData> {
        // The style system ensures that floated and positioned frames are
        // block-level.
        NS_ASSERTION!(!(aDisplay.IsFloatingStyle() || aDisplay.IsAbsolutelyPositionedStyle()) || aDisplay.IsBlockOutsideStyle(), "Style system did not apply CSS2.1 section 9.7 fixups");

        // If this is "body", try propagating its scroll style to the viewport
        // Note that we need to do this even if the body is NOT scrollable;
        // it might have dynamically changed from scrollable to not scrollable,
        // and that might need to be propagated.
        // NOT PORTED: scroll frames (no viewport scroll style to propagate).
        let propagatedScrollToViewport = false;

        match aDisplay.DisplayInside() {
            StyleDisplayInside::Flow | StyleDisplayInside::FlowRoot => {
                if aDisplay.IsInlineFlow() {
                    return Some(&sInlineData);
                }

                // If the frame is a block-level frame and is scrollable, then wrap it in
                // a scroll frame.  Except we don't want to do that for paginated contexts
                // for frames that are block-outside and aren't frames for native
                // anonymous stuff.
                // XXX Ignore tables for the time being (except caption)
                let caption = aDisplay.DisplayOutside() == StyleDisplayOutside::TableCaption;
                let needScrollFrame = aDisplay.IsScrollableOverflow() && !propagatedScrollToViewport;
                if needScrollFrame {
                    let suppressScrollFrame = nsPresContext::Current().IsPaginated() && aDisplay.IsBlockOutsideStyle();
                    if !suppressScrollFrame {
                        return Some(&sScrollableBlockData[caption as usize]);
                    }
                }

                // Handle various non-scrollable blocks.
                Some(&sNonScrollableBlockData[caption as usize])
            }
            StyleDisplayInside::Table => Some(&sTableData),
            // NOTE: In the unlikely event that we add another table-part here that
            // has a desired-parent-type (& hence triggers table fixup), we'll need to
            // also update the flexbox chunk in ComputedStyle::ApplyStyleFixups().
            StyleDisplayInside::TableRowGroup => Some(&sTableRowGroupData),
            StyleDisplayInside::TableColumn => Some(&sTableColData),
            StyleDisplayInside::TableColumnGroup => Some(&sTableColGroupData),
            StyleDisplayInside::TableHeaderGroup => Some(&sTableRowGroupData),
            StyleDisplayInside::TableFooterGroup => Some(&sTableRowGroupData),
            StyleDisplayInside::TableRow => Some(&sTableRowData),
            StyleDisplayInside::TableCell => Some(&sTableCellData),
            // (StyleDisplayInside::WebkitBox: legacy -webkit-box is not in this Stylo configuration)
            StyleDisplayInside::Flex => {
                if propagatedScrollToViewport { Some(&sFlexNonScrollableData) } else { Some(&sFlexData) }
            }
            StyleDisplayInside::Grid => {
                if propagatedScrollToViewport { Some(&sGridNonScrollableData) } else { Some(&sGridData) }
            }
            // Ruby: not in this Stylo configuration.
            _ => {
                MOZ_ASSERT_UNREACHABLE!("unknown 'display' value");
                None
            }
        }
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindDataForContent
    fn FindDataForContent(&self, aContent: ContentId, aStyle: &ServoArc<ComputedStyle>, aParentFrame: Option<nsIFrame>, aFlags: ItemFlags) -> Option<&'static FrameConstructionData> {
        MOZ_ASSERT!(
            {
                let d = nsStyleDisplay::new(aStyle.clone()).mDisplay();
                d != StyleDisplayValue::None && d != StyleDisplayValue::Contents
            },
            "These two special display values should be handled earlier"
        );

        if self.IsText(aContent) {
            return self.FindTextData(aContent, aParentFrame);
        }

        self.FindElementData(aContent, aStyle, aParentFrame, aFlags)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindElementData
    fn FindElementData(&self, aContent: ContentId, aStyle: &ServoArc<ComputedStyle>, aParentFrame: Option<nsIFrame>, aFlags: ItemFlags) -> Option<&'static FrameConstructionData> {
        // Don't create frames for non-SVG element children of SVG elements.
        // (the outer <svg> is a leaf here, so its children never get here)

        if let Some(data) = self.FindElementTagData(aContent, aStyle, aParentFrame, aFlags) {
            return Some(data);
        }

        // Check for 'content: <image-url>' on the element (which makes us ignore
        // 'display' values other than 'none' or 'contents').
        // NOT PORTED: images.

        // shouldBlockify (a rendered legend or an outside ::marker): the marker's style is made
        // block-level when it is made (CreateGeneratedContentItem).

        let display = nsStyleDisplay::new(aStyle.clone());
        self.FindDisplayData(&display, aContent)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindElementTagData
    fn FindElementTagData(&self, aContent: ContentId, aStyle: &ServoArc<ComputedStyle>, aParentFrame: Option<nsIFrame>, aFlags: ItemFlags) -> Option<&'static FrameConstructionData> {
        match self.GetNameSpaceID(aContent) {
            kNameSpaceID_XHTML => self.FindHTMLData(aContent, aParentFrame, aStyle),
            // NOT PORTED: MathML frames (FindMathMLData): MathML elements are constructed by display.
            kNameSpaceID_MathML => None,
            // NOT PORTED: SVG frames (FindSVGData): an <svg> element is a leaf block, its
            // children get no frames (it is not processed).
            kNameSpaceID_SVG => Some(&sSVGOuterSVGData),
            _ => None,
        }
    }
}

// ---- items ----------------------------------------------------------------------------------------

impl nsCSSFrameConstructor {
    // Helper method for AddFrameConstructionItems etc.
    // Unsets the need-frame/restyle bits on aContent.
    // return true iff we should attempt to create frames for aContent.
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ShouldCreateItemsForChild
    fn ShouldCreateItemsForChild(&self, aState: &nsFrameConstructorState, aContent: ContentId, aParentFrame: Option<nsIFrame>) -> bool {
        // aContent->UnsetFlags(NODE_DESCENDANTS_NEED_FRAMES | NODE_NEEDS_FRAME);
        // NOT PORTED: lazy frame construction flags.

        // don't create a whitespace frame if aParent doesn't want it
        // (NeedFrameFor: only MathML frames ignore whitespace, NOT PORTED)

        // never create frames for comments or PIs
        if self.IsComment(aContent) {
            return false;
        }

        // ADAPTER: the content of a closed <details> (see IsHiddenByClosedDetails).
        if aParentFrame.is_some() && self.IsHiddenByClosedDetails(aContent) {
            return false;
        }

        true
    }

    // Gecko: dom/html/HTMLDetailsElement.cpp (the UA shadow tree: the first <summary> child is slotted
    // into the details' own summary slot, the other children into `::details-content`) and
    // layout/style/res/html.css (`details::details-content { content-visibility: hidden }`,
    // `details[open]::details-content { content-visibility: revert }`)
    // ADAPTER: there is no shadow tree, and mrt-style does not compute `::details-content`, so the
    // `content-visibility: hidden` of the closed state is applied here: a child of a closed <details>
    // that is not its first <summary> gets no frames (content-visibility: hidden skips the layout of the
    // contents; the frames Gecko keeps for them are not needed). The summary is an ordinary frame
    // (a list item with a disclosure marker, or whatever display the page gives it, e.g. flex).
    // NOT PORTED: the anonymous default summary ("Details") that Gecko makes for a <details> without a
    // <summary>; such an element shows nothing here while closed.
    fn IsHiddenByClosedDetails(&self, aContent: ContentId) -> bool {
        let parentContent = if aContent < kGeneratedContentBase { self.sd().tree.dom.node(aContent).parent } else { NONE };
        if parentContent == NONE || !self.IsHTMLElement(parentContent, "details") || self.HasAttr(parentContent, "open") {
            return false;
        }
        let mut first = self.GetFirstChild(parentContent);
        while first != NONE && !self.IsHTMLElement(first, "summary") {
            first = self.GetNextSibling(first);
        }
        first != aContent
    }

    // Add the frame construction items for the given aContent and aParentFrame
    // to the list.  This might add more than one item in some rare cases.
    // If aSuppressWhiteSpaceOptimizations is true, optimizations that
    // may suppress the construction of white-space-only text frames
    // must be skipped for these items and items around them.
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::AddFrameConstructionItems
    fn AddFrameConstructionItems(&mut self, aState: &mut nsFrameConstructorState, aContent: ContentId, aSuppressWhiteSpaceOptimizations: bool, aParentStyle: &ServoArc<ComputedStyle>, aInsertion: InsertionPoint, aItems: &mut FrameConstructionItemList, aFlags: ItemFlags) {
        let parentFrame = aInsertion.mParentFrame;
        if !self.ShouldCreateItemsForChild(aState, aContent, parentFrame) {
            return;
        }

        let computedStyle = match self.ResolveComputedStyle(aContent) {
            Some(s) => s,
            // ADAPTER: an element mrt-style did not style (it is not under the document element).
            None => return,
        };
        let flags = aFlags | ItemFlag::AllowPageBreak;
        // parentFrame->IsInSVGTextSubtree() etc.: NOT PORTED, SVG text.
        self.AddFrameConstructionItemsInternal(aState, aContent, parentFrame, aSuppressWhiteSpaceOptimizations, &computedStyle, flags, aItems);
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::AddFrameConstructionItemsInternal
    fn AddFrameConstructionItemsInternal(&mut self, aState: &mut nsFrameConstructorState, aContent: ContentId, aParentFrame: Option<nsIFrame>, aSuppressWhiteSpaceOptimizations: bool, aComputedStyle: &ServoArc<ComputedStyle>, aFlags: ItemFlags, aItems: &mut FrameConstructionItemList) {
        MOZ_ASSERT!(self.IsText(aContent) || self.IsElement(aContent), "Shouldn't get anything else here!");

        let withinSVGText = (aFlags & ItemFlag::IsWithinSVGText).to_bool();
        let isGeneratedContent = (aFlags & ItemFlag::IsGeneratedContent).to_bool();

        // 'display:none' elements never creates any frames at all.
        let display = nsStyleDisplay::new(aComputedStyle.clone());
        if display.mDisplay() == StyleDisplayValue::None {
            return;
        }

        if display.mDisplay() == StyleDisplayValue::Contents {
            // See the mDisplay fixup code in StyleAdjuster::adjust.

            // FIXME(bug 1588477): <svg:text>'s TextNodeCorrespondenceRecorder has
            // trouble with everything that looks like display: contents.
            if withinSVGText {
                return;
            }

            self.CreateGeneratedContentItem(aState, aParentFrame, aContent, aComputedStyle, PseudoStyleType::Before, aItems, ItemFlag::None);

            let insertion = InsertionPoint { mParentFrame: aParentFrame, mContainer: aContent };
            let mut iter = self.FlattenedChildIterator(aContent);
            let mut child = iter.GetNextChild(self);
            while child != NONE {
                self.AddFrameConstructionItems(aState, child, aSuppressWhiteSpaceOptimizations, aComputedStyle, insertion, aItems, aFlags);
                child = iter.GetNextChild(self);
            }
            aItems.SetParentHasNoShadowDOM(true);

            self.CreateGeneratedContentItem(aState, aParentFrame, aContent, aComputedStyle, PseudoStyleType::After, aItems, ItemFlag::None);
            return;
        }

        // ShouldSuppressFrameInListboxSelect, the fieldset legend: NOT PORTED.

        let data = match self.FindDataForContent(aContent, aComputedStyle, aParentFrame, aFlags) {
            Some(d) => d,
            None => return,
        };
        let bits = data.mBits;
        if (bits & FCDATA_SUPPRESS_FRAME) != 0 {
            return;
        }
        // Create our shadow tree lazily if needed: NOT PORTED (form controls).

        // canHavePageBreak: NOT PORTED, the page breaks of paginated documents.

        let idx = aItems.AppendItem(data, aContent, aComputedStyle.clone(), aSuppressWhiteSpaceOptimizations);
        aItems.mItems[idx].mIsText = !self.IsElement(aContent);
        aItems.mItems[idx].mIsGeneratedContent = isGeneratedContent;

        if (bits & FCDATA_IS_INLINE) != 0 {
            // To correctly set item->mIsAllInline we need to build up our child items
            // right now.
            self.BuildInlineChildItems(aState, &mut aItems.mItems[idx], (aFlags & ItemFlag::IsWithinSVGText).to_bool(), (aFlags & ItemFlag::AllowTextPathChild).to_bool());
            aItems.mItems[idx].mIsBlock = false;
        } else {
            // Compute a boolean isInline which is guaranteed to be false for blocks
            // (but may also be false for some inlines).
            let isInline =
                // Table-internal things are inline-outside if and only if they're kids
                // of inlines, since they'll trigger construction of inline-table
                // pseudos.
                ((bits & FCDATA_IS_TABLE_PART) != 0 &&
                 (aParentFrame.is_none() ||  // No aParentFrame means inline
                  aParentFrame.unwrap().StyleDisplay().IsInlineFlow())) ||
                // Things that are inline-outside but aren't inline frames are inline
                display.IsInlineOutsideStyle();

            // Set mIsAllInline conservatively.  It just might be that even an inline
            // that has mIsAllInline false doesn't need an {ib} split.  So this is just
            // an optimization to keep from doing too much work in cases when we can
            // show that mIsAllInline is true..
            aItems.mItems[idx].mIsAllInline = isInline ||
                // Figure out whether we're guaranteed this item will be out of flow.
                // This is not a precise test, since one of our ancestor inlines might
                // add an absolute containing block (if it's relatively positioned) when
                // there wasn't such a containing block before.  But it's conservative
                // in the sense that anything that will really end up as an in-flow
                // non-inline will test false here.  In other words, if this test is
                // true we're guaranteed to be inline; if it's false we don't know what
                // we'll end up as.
                //
                // If we make this test precise, we can remove some of the code dealing
                // with the imprecision in ConstructInline and adjust the comments on
                // mIsAllInline and mIsBlock in the header.
                ((bits & FCDATA_DISALLOW_OUT_OF_FLOW) == 0 && aState.GetGeometricParent(&display, None).is_some());

            // Set mIsBlock conservatively.  It's OK to set it false for some real
            // blocks, but not OK to set it true for things that aren't blocks.  Since
            // isOutOfFlow might be false even in cases when the frame will end up
            // out-of-flow, we can't use it here.  But we _can_ say that the frame will
            // for sure end up in-flow if it's not floated or absolutely positioned.
            aItems.mItems[idx].mIsBlock = !isInline && !display.IsAbsolutelyPositionedStyle() && !display.IsFloatingStyle() && (bits & FCDATA_IS_SVG_TEXT) == 0;
        }

        if aItems.mItems[idx].mIsAllInline {
            aItems.InlineItemAdded();
        } else if aItems.mItems[idx].mIsBlock {
            aItems.BlockItemAdded();
        }
    }

    /**
     * Return true if the frame construction item pointed to by aIter will
     * create a frame adjacent to a line boundary in the frame tree, and that
     * line boundary is induced by a content node adjacent to the frame's
     * content node in the content tree. The latter condition is necessary so
     * that ContentAppended/ContentInserted/ContentWillBeRemoved can easily find any
     * text nodes that were suppressed here.
     */
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::AtLineBoundary
    fn AtLineBoundary(&self, aItems: &FrameConstructionItemList, aIter: FCItemIterator) -> bool {
        let item = &aItems.mItems[aIter];
        if item.mSuppressWhiteSpaceOptimizations {
            return false;
        }

        if aIter == 0 {
            if aItems.HasLineBoundaryAtStart() && self.GetPreviousSibling(item.mContent) == NONE {
                return true;
            }
        } else {
            let prev = &aItems.mItems[aIter - 1];
            if prev.IsLineBoundary() && !prev.mSuppressWhiteSpaceOptimizations && self.GetPreviousSibling(item.mContent) == prev.mContent {
                return true;
            }
        }

        if aIter + 1 >= aItems.mItems.len() {
            if aItems.HasLineBoundaryAtEnd() && self.GetNextSibling(item.mContent) == NONE {
                return true;
            }
        } else {
            let next = &aItems.mItems[aIter + 1];
            if next.IsLineBoundary() && !next.mSuppressWhiteSpaceOptimizations && self.GetNextSibling(item.mContent) == next.mContent {
                return true;
            }
        }

        false
    }

    // Don't call this unless the frametree really depends on the answer!
    // Especially so for generated content, where we don't want to reframe
    // things.
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FrameConstructionItem::IsWhitespace
    fn ItemIsWhitespace(&self, aItem: &FrameConstructionItem) -> bool {
        if !aItem.mIsText {
            return false;
        }
        if aItem.mContent < kGeneratedContentBase {
            nsIContent::FromNodeId(aItem.mContent).SetFlags(NS_CREATE_FRAME_IF_NON_WHITESPACE | NS_REFRAME_IF_WHITESPACE);
        }
        self.TextIsOnlyWhitespace(aItem.mContent)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructFramesFromItem
    fn ConstructFramesFromItem(&mut self, aState: &mut nsFrameConstructorState, aItems: &mut FrameConstructionItemList, aIter: FCItemIterator, aParentFrame: Option<nsIFrame>, aFrameList: &mut nsFrameList) {
        let computedStyle = aItems.mItems[aIter].mComputedStyle.clone();
        if aItems.mItems[aIter].mIsText {
            // If this is collapsible whitespace next to a line boundary,
            // don't create a frame. item.IsWhitespace() also sets the
            // NS_CREATE_FRAME_IF_NON_WHITESPACE flag in the text node. (If we
            // end up creating a frame, nsTextFrame::Init will clear the flag.)
            // We don't do this for generated content, because some generated
            // text content is empty text nodes that are about to be initialized.
            // (We check mAdditionalStateBits because only the generated content
            // container's frame construction item is marked with
            // mIsGeneratedContent, and we might not have an aParentFrame.)
            // We don't do it for content that may have Shadow DOM siblings / insertion
            // points, because they make it difficult to correctly create the frame due
            // to dynamic changes.
            // We don't do it for SVG text, since we might need to position and
            // measure the white space glyphs due to x/y/dx/dy attributes.
            let item = &aItems.mItems[aIter];
            if self.AtLineBoundary(aItems, aIter)
                && !nsStyleTextAdapter::new(computedStyle.clone()).WhiteSpaceOrNewlineIsSignificant()
                && aItems.ParentHasNoShadowDOM()
                && (aState.mAdditionalStateBits & NS_FRAME_GENERATED_CONTENT) == 0
                && (item.mFCData.mBits & FCDATA_IS_LINE_PARTICIPANT) != 0
                && (item.mFCData.mBits & FCDATA_IS_SVG_TEXT) == 0
                && !self.mAlwaysCreateFramesForIgnorableWhitespace
                && self.ItemIsWhitespace(item)
            {
                return;
            }

            let data = item.mFCData;
            let content = item.mContent;
            self.ConstructTextFrame(data, aState, content, aParentFrame, &computedStyle, aFrameList);
            return;
        }

        let savedStateBits = aState.mAdditionalStateBits;
        if aItems.mItems[aIter].mIsGeneratedContent {
            // Ensure that frames created here are all tagged with
            // NS_FRAME_GENERATED_CONTENT.
            aState.mAdditionalStateBits |= NS_FRAME_GENERATED_CONTENT;
        }

        // XXXbz maybe just inline ConstructFrameFromItemInternal here or something?
        self.ConstructFrameFromItemInternal(&mut aItems.mItems[aIter], aState, aParentFrame, aFrameList);

        if aItems.mItems[aIter].mIsGeneratedContent {
            // Now that we've passed ownership of item.mContent to the frame, unset
            // our generated content flag so we don't release or unbind it ourselves.
            aItems.mItems[aIter].mIsGeneratedContent = false;
        }
        aState.mAdditionalStateBits = savedStateBits;
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreateNeededAnonFlexOrGridItems
    fn CreateNeededAnonFlexOrGridItems(&mut self, aState: &mut nsFrameConstructorState, aItems: &mut FrameConstructionItemList, aParentFrame: nsIFrame) {
        if aItems.mItems.is_empty() {
            return;
        }

        if !aParentFrame.IsFlexOrGridContainer() {
            return;
        }

        // NOT PORTED: -webkit-box (IsFlexContainerForLegacyWebKitBox is always false here).
        let isLegacyWebKitBox = false;
        let mut iter: FCItemIterator = 0;
        loop {
            'continue_: {
                // Advance iter past children that don't want to be wrapped
                if self.SkipItemsThatDontNeedAnonFlexOrGridItem(aItems, &mut iter, isLegacyWebKitBox) {
                    // Hit the end of the items without finding any remaining children that
                    // need to be wrapped. We're finished!
                    return;
                }

                // If our next potentially-wrappable child is whitespace, then see if
                // there's anything wrappable immediately after it. If not, we just drop
                // the whitespace and move on. (We're not supposed to create any anonymous
                // flex/grid items that _only_ contain whitespace).
                // (BUT if this is generated content, then we don't give whitespace nodes
                // any special treatment, because they're probably not really whitespace --
                // they're just temporarily empty, waiting for their generated text.)
                // XXXdholbert If this node's generated text will *actually end up being
                // entirely whitespace*, then we technically should still skip over it, per
                // the CSS grid & flexbox specs. I'm not bothering with that at this point,
                // since it's a pretty extreme edge case.
                if !aParentFrame.HasAnyStateBits(NS_FRAME_GENERATED_CONTENT) && self.ItemIsWhitespace(&aItems.mItems[iter]) {
                    let mut afterWhitespaceIter = iter;
                    let hitEnd = self.SkipWhitespace(aItems, &mut afterWhitespaceIter);
                    let nextChildNeedsAnonItem = !hitEnd && aItems.mItems[afterWhitespaceIter].NeedsAnonFlexOrGridItem(isLegacyWebKitBox);

                    if !nextChildNeedsAnonItem {
                        // There's nothing after the whitespace that we need to wrap, so we
                        // just drop this run of whitespace.
                        aItems.DeleteItemsTo(&mut iter, afterWhitespaceIter);
                        if hitEnd {
                            // Nothing left to do -- we're finished!
                            return;
                        }
                        // else, we have a next child and it does not want to be wrapped.  So,
                        // we jump back to the beginning of the loop to skip over that child
                        // (and anything else non-wrappable after it)
                        MOZ_ASSERT!(!aItems.IsDone(iter) && !aItems.mItems[iter].NeedsAnonFlexOrGridItem(isLegacyWebKitBox), "hitEnd and/or nextChildNeedsAnonItem lied");
                        break 'continue_;
                    }
                }

                // Now |iter| points to the first child that needs to be wrapped in an
                // anonymous flex/grid item. Now we see how many children after it also want
                // to be wrapped in an anonymous flex/grid item.
                let mut endIter = iter; // iterator to find the end of the group
                self.SkipItemsThatNeedAnonFlexOrGridItem(aItems, &mut endIter, isLegacyWebKitBox);

                NS_ASSERTION!(iter != endIter, "Should've had at least one wrappable child to seek past");

                // Now, we create the anonymous flex or grid item to contain the children
                // between |iter| and |endIter|.
                let parentContent = self.ContentOfFrame(aParentFrame);
                let wrapperStyle = self.ResolveInheritingAnonymousBoxStyle(PseudoStyleType::MozAnonymousItem, &aParentFrame.Style());

                // Use the content of our parent frame
                let mut newItem = FrameConstructionItem::new(&sAnonFlexOrGridItemBlockData, parentContent, wrapperStyle, true);

                newItem.mIsAllInline = nsStyleDisplay::new(newItem.mComputedStyle.clone()).IsInlineOutsideStyle();
                newItem.mIsBlock = !newItem.mIsAllInline;

                MOZ_ASSERT!(!newItem.mIsAllInline && newItem.mIsBlock, "expecting anonymous flex/grid items to be block-level (this will make a difference when we encounter 'align-items: baseline')");

                // Anonymous flex and grid items induce line boundaries around their
                // contents.
                newItem.mChildItems.SetLineBoundaryAtStart(true);
                newItem.mChildItems.SetLineBoundaryAtEnd(true);
                // The parent of the items in aItems is also the parent of the items
                // in mChildItems
                newItem.mChildItems.SetParentHasNoShadowDOM(aItems.ParentHasNoShadowDOM());

                // Eat up all items between |iter| and |endIter| and put them in our
                // wrapper. This advances |iter| to point to |endIter|.
                aItems.AppendItemsToList(&mut iter, endIter, &mut newItem.mChildItems);

                aItems.InsertItem(&mut iter, newItem);
            }
            if aItems.IsDone(iter) {
                break;
            }
        }
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp FrameConstructionItemList::Iterator::SkipItemsThatNeedAnonFlexOrGridItem
    // (on the constructor like SkipWhitespace; Gecko passes the nsFrameConstructorState for
    // NeedsAnonFlexOrGridItem's legacy-box check, which is not ported)
    fn SkipItemsThatNeedAnonFlexOrGridItem(&self, aItems: &FrameConstructionItemList, aIter: &mut FCItemIterator, aIsLegacyWebKitBox: bool) -> bool {
        MOZ_ASSERT!(!aItems.IsDone(*aIter), "Shouldn't be done yet");
        while aItems.mItems[*aIter].NeedsAnonFlexOrGridItem(aIsLegacyWebKitBox) {
            *aIter += 1;
            if aItems.IsDone(*aIter) {
                return true;
            }
        }
        false
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp FrameConstructionItemList::Iterator::SkipItemsThatDontNeedAnonFlexOrGridItem
    fn SkipItemsThatDontNeedAnonFlexOrGridItem(&self, aItems: &FrameConstructionItemList, aIter: &mut FCItemIterator, aIsLegacyWebKitBox: bool) -> bool {
        MOZ_ASSERT!(!aItems.IsDone(*aIter), "Shouldn't be done yet");
        while !aItems.mItems[*aIter].NeedsAnonFlexOrGridItem(aIsLegacyWebKitBox) {
            *aIter += 1;
            if aItems.IsDone(*aIter) {
                return true;
            }
        }
        false
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructFramesFromItemList
    fn ConstructFramesFromItemList(&mut self, aState: &mut nsFrameConstructorState, aItems: &mut FrameConstructionItemList, aParentFrame: nsIFrame, aParentIsWrapperAnonBox: bool, aFrameList: &mut nsFrameList) {
        // (the assertion on the float containing block candidate, and the rendered legend of a
        // fieldset: NOT PORTED)

        self.CreateNeededPseudoContainers(aState, aItems, aParentFrame);
        self.CreateNeededAnonFlexOrGridItems(aState, aItems, aParentFrame);
        // CreateNeededPseudoInternalRubyBoxes and CreateNeededPseudoSiblings: NOT PORTED (there is no ruby).

        let mut iter = 0;
        while iter < aItems.mItems.len() {
            MOZ_ASSERT!(!aItems.mItems[iter].mIsRenderedLegend, "Only one item can be the rendered legend, and it should've been handled above");
            self.ConstructFramesFromItem(aState, aItems, iter, Some(aParentFrame), aFrameList);
            iter += 1;
        }

        // VerifyGridFlexContainerChildren(aParentFrame, aFrameList);
        // Calculate and propagate page-name values for each frame in the frame list: NOT PORTED, pagination.

        if aParentIsWrapperAnonBox {
            // for (nsIFrame* f : aFrameList) { f->SetParentIsWrapperAnonBox(); } NOT PORTED: restyle.
        }
    }
}

// ---- constructing the frames ------------------------------------------------------------------------

impl nsCSSFrameConstructor {
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructFrameFromItemInternal
    fn ConstructFrameFromItemInternal(&mut self, aItem: &mut FrameConstructionItem, aState: &mut nsFrameConstructorState, aParentFrame: Option<nsIFrame>, aFrameList: &mut nsFrameList) {
        let data = aItem.mFCData;

        let bits = data.mBits;

        NS_ASSERTION!((bits & FCDATA_FUNC_IS_DATA_GETTER) == 0, "Should have dealt with this inside the data finder");

        // Some sets of bits are not compatible with each other
        // (CHECK_ONLY_ONE_BIT of the full constructor with the bits only the creation functions use:
        // debug-only assertions, NOT PORTED)
        MOZ_ASSERT!((bits & FCDATA_IS_WRAPPER_ANON_BOX) == 0 || (bits & FCDATA_USE_CHILD_ITEMS) != 0, "Wrapper anon boxes should always have FCDATA_USE_CHILD_ITEMS");

        // Don't create a subdocument frame for iframes if we're creating extra frames
        // (NOT PORTED: iframes)

        let content = aItem.mContent;
        let mut newFrame: nsIFrame;
        let primaryFrame: nsIFrame;
        let computedStyle = aItem.mComputedStyle.clone();
        let display = nsStyleDisplay::new(computedStyle.clone());
        if (bits & FCDATA_FUNC_IS_FULL_CTOR) != 0 {
            let fullConstructor = match data.mFunc {
                FrameConstructionData_Func::mFullConstructor(f) => f,
                _ => unreachable!(),
            };
            newFrame = fullConstructor(self, aState, aItem, aParentFrame, &display, aFrameList);
            primaryFrame = newFrame;
        } else {
            let creationFunc = match data.mFunc {
                FrameConstructionData_Func::mCreationFunc(Some(f)) => f,
                _ => unreachable!("Must have frame construction data"),
            };
            newFrame = creationFunc(self.AllocFrame(computedStyle.clone()));

            let allowOutOfFlow = (bits & FCDATA_DISALLOW_OUT_OF_FLOW) == 0;
            let geometricParent = if allowOutOfFlow { aState.GetGeometricParent(&display, aParentFrame) } else { aParentFrame };

            // In the non-scrollframe case, primaryFrame and newFrame are equal; in the
            // scrollframe case, newFrame is the scrolled frame while primaryFrame is
            // the scrollframe.
            // (FCDATA_MAY_NEED_SCROLLFRAME and BuildScrollContainerFrame: NOT PORTED)
            self.InitAndRestoreFrame(aState, content, geometricParent, newFrame);
            // ADAPTER: nsTableColGroupFrame::GetSpan reads `-x-span` of the style, which the `span`
            // attribute of the element maps to (HTMLTableColElement::MapAttributesIntoRule); a pseudo
            // column group (it has child items) has the initial span, 1.
            if newFrame.Type() == LayoutFrameType::TableColGroup && (bits & FCDATA_USE_CHILD_ITEMS) == 0 {
                nsTableColGroupFrame::from_frame(newFrame).SetSpan(self.ParseClampedNonNegativeInt(content, "span", 1, 1, MAX_COLSPAN));
            }
            primaryFrame = newFrame;

            // If we need to create a block formatting context to wrap our
            // kids, do it now.
            // (FCDATA_CREATE_BLOCK_WRAPPER_FOR_ALL_KIDS: NOT PORTED, used by the canvas and
            // fieldset frames)
            let maybeAbsoluteContainingBlockStyleFrame = primaryFrame;
            let maybeAbsoluteContainingBlock = newFrame;
            let possiblyLeafFrame = newFrame;

            aState.AddChild(self, primaryFrame, aFrameList, content, aParentFrame, allowOutOfFlow, allowOutOfFlow, false, None);

            let newFrameAsContainer = nsContainerFrame::do_QueryFrame(newFrame);
            if let Some(newFrameAsContainer) = newFrameAsContainer {
                // Process the child content if requested
                let mut childList = nsFrameList::default();
                let mut absoluteSaveState = nsFrameConstructorSaveState::default();

                if (bits & FCDATA_FORCE_NULL_ABSPOS_CONTAINER) != 0 {
                    aState.PushAbsoluteContainingBlock(None, None, &mut absoluteSaveState);
                } else if (bits & FCDATA_SKIP_ABSPOS_PUSH) == 0 {
                    maybeAbsoluteContainingBlock.AddStateBits(NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN);
                    if maybeAbsoluteContainingBlockStyleFrame.IsAbsPosContainingBlock() {
                        aState.PushAbsoluteContainingBlock(Some(maybeAbsoluteContainingBlock), Some(maybeAbsoluteContainingBlockStyleFrame), &mut absoluteSaveState);
                    }
                }

                let mut floatSaveState = nsFrameConstructorSaveState::default();
                aState.MaybePushFloatContainingBlock(newFrameAsContainer.0, &mut floatSaveState);

                if (bits & FCDATA_USE_CHILD_ITEMS) != 0 {
                    // At this point, we have not set up the auto value for this frame, and
                    // no caller will have set it so it is not redundant and therefor will
                    // not assert.
                    self.ConstructFramesFromItemList(aState, &mut aItem.mChildItems, newFrameAsContainer.0, (bits & FCDATA_IS_WRAPPER_ANON_BOX) != 0, &mut childList);
                } else {
                    // Process the child frames.
                    self.ProcessChildren(aState, content, &computedStyle, newFrameAsContainer.0, (bits & FCDATA_DISALLOW_GENERATED_CONTENT) == 0, &mut childList, (bits & FCDATA_ALLOW_BLOCK_STYLES) != 0, Some(possiblyLeafFrame));
                }

                // FCDATA_WRAP_KIDS_IN_BLOCKS: NOT PORTED, MathML.

                // Set the frame's initial child list. Note that MathML depends on this
                // being called even if childList is empty!
                newFrameAsContainer.SetInitialChildList(FrameChildListID::Principal, childList);

                floatSaveState.Restore(aState); // ~nsFrameConstructorSaveState
                absoluteSaveState.Restore(aState); // ~nsFrameConstructorSaveState
            }
        }

        NS_ASSERTION!(newFrame.IsLineParticipant() == ((bits & FCDATA_IS_LINE_PARTICIPANT) != 0) || (bits & FCDATA_FUNC_IS_FULL_CTOR) != 0, "Incorrectly set FCDATA_IS_LINE_PARTICIPANT bits");

        // Even if mCreatingExtraFrames is set, we may need to SetPrimaryFrame for
        // generated content that doesn't have one yet.  Note that we have to examine
        // the frame bit, because by this point mIsGeneratedContent has been cleared
        // on aItem.
        if !aState.mCreatingExtraFrames && (bits & FCDATA_SKIP_FRAMESET) == 0 {
            self.SetPrimaryFrame(content, primaryFrame);
        }
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructScrollableBlock
    // NOT PORTED: scroll frames (BuildScrollContainerFrame, nsCSSFrameConstructor.cpp lines
    // 4062-4130 and 4323-4363 at GECKO_REV). An element with overflow other than visible gets the plain
    // block frame, which establishes a block formatting context by its style.
    fn ConstructScrollableBlock(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        self.ConstructNonScrollableBlock(aState, aItem, aParentFrame, aDisplay, aFrameList)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructNonScrollableBlock
    fn ConstructNonScrollableBlock(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        let computedStyle = aItem.mComputedStyle.clone();
        let mut newFrame = NS_NewBlockFrame(self.AllocFrame(computedStyle.clone())).0;
        let geometricParent = aState.GetGeometricParent(aDisplay, aParentFrame);
        let positioned = if newFrame.IsAbsPosContainingBlock() { Some(newFrame) } else { None };
        self.ConstructBlock(aState, aItem.mContent, geometricParent, aParentFrame, &computedStyle, &mut newFrame, aFrameList, positioned);
        newFrame
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::InitAndRestoreFrame
    fn InitAndRestoreFrame(&mut self, aState: &nsFrameConstructorState, aContent: ContentId, aParentFrame: Option<nsIFrame>, aNewFrame: nsIFrame) {
        // Initialize the frame
        let content = self.MakeContent(aContent);
        self.InitFrame(aNewFrame, Some(content), aParentFrame);
        aNewFrame.AddStateBits(aState.mAdditionalStateBits);
        self.RememberContentOfFrame(aNewFrame, aContent);

        // if (aState.mFrameState) { RestoreFrameStateFor(...) }  NOT PORTED: frame state.
        // mContainStyleScopeManager.AddCounterChanges(aNewFrame): NOT PORTED, counters.
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructBlock
    fn ConstructBlock(&mut self, aState: &mut nsFrameConstructorState, aContent: ContentId, aParentFrame: Option<nsIFrame>, aContentParentFrame: Option<nsIFrame>, aComputedStyle: &ServoArc<ComputedStyle>, aNewFrame: &mut nsIFrame, aFrameList: &mut nsFrameList, aPositionedFrameForAbsPosContainer: Option<nsIFrame>) {
        // If a block frame is in a multi-column subtree, its children may need to
        // be chopped into runs of blocks containing column-spans and runs of
        // blocks containing no column-spans. Each run containing column-spans
        // will be wrapped by an anonymous block. See CreateColumnSpanSiblings() for
        // the implementation.
        //
        // If a block frame is a multi-column container, its children will need to
        // be processed as above. Moreover, it creates a ColumnSetWrapperFrame as
        // its outermost frame, and its children which have no
        // -moz-column-span-wrapper pseudo will be wrapped in ColumnSetFrames. See
        // FinishBuildingColumns() for the implementation.
        // NOT PORTED: columns (the long comment, BeginBuildingColumns, FinishBuildingColumns,
        // CreateColumnSpanSiblings, MayNeedToCreateColumnSpanSiblings: nsCSSFrameConstructor.cpp
        // lines 9983-10409 at GECKO_REV).

        let blockFrame = nsBlockFrame::do_QueryFrame(*aNewFrame).unwrap().0;
        MOZ_ASSERT!(blockFrame.IsBlockFrame(), "not a block frame?");

        // Create column hierarchy if necessary.
        // (needsColumn: NOT PORTED) No need to create column hierarchy. Initialize block frame.
        // blockFrame->SetComputedStyleWithoutNotification(aComputedStyle);  (the style is set at AllocFrame)
        self.InitAndRestoreFrame(aState, aContent, aParentFrame, blockFrame);

        aState.AddChild(self, *aNewFrame, aFrameList, aContent, if aContentParentFrame.is_some() { aContentParentFrame } else { aParentFrame }, true, true, false, None);
        if self.mRootElementFrame.is_none() {
            self.mRootElementFrame = Some(*aNewFrame);
        }

        // We should make the outer frame be the absolute containing block,
        // if one is required. We have to do this because absolute
        // positioning must be computed with respect to the CSS dimensions
        // of the element, which are the dimensions of the outer block. But
        // we can't really do that because only blocks can have absolute
        // children. So use the block and try to compensate with hacks
        // in nsBlockFrame::CalculateContainingBlockSizeForAbsolutes.
        let mut absoluteSaveState = nsFrameConstructorSaveState::default();
        (*aNewFrame).AddStateBits(NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN);
        if aPositionedFrameForAbsPosContainer.is_some() {
            aState.PushAbsoluteContainingBlock(Some(*aNewFrame), aPositionedFrameForAbsPosContainer, &mut absoluteSaveState);
        }

        let mut floatSaveState = nsFrameConstructorSaveState::default();
        aState.MaybePushFloatContainingBlock(blockFrame, &mut floatSaveState);

        // NS_FRAME_HAS_MULTI_COLUMN_ANCESTOR: NOT PORTED, columns.

        // Process the child content
        let mut childList = nsFrameList::default();
        self.ProcessChildren(aState, aContent, aComputedStyle, blockFrame, true, &mut childList, true, None);

        // if (!MayNeedToCreateColumnSpanSiblings(blockFrame, childList)) {...}: NOT PORTED, columns.
        nsBlockFrame::from_frame(blockFrame).SetInitialChildList(FrameChildListID::Principal, childList);

        floatSaveState.Restore(aState); // ~nsFrameConstructorSaveState
        absoluteSaveState.Restore(aState); // ~nsFrameConstructorSaveState
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructInline
    fn ConstructInline(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        // If an inline frame has non-inline kids, then we chop up the child list
        // into runs of blocks and runs of inlines, create anonymous block frames to
        // contain the runs of blocks, inline frames with our style for the runs of
        // inlines, and put all these frames, in order, into aFrameList.
        //
        // When there are column-span blocks in a run of blocks, instead of creating
        // an anonymous block to wrap them, we create multiple anonymous blocks,
        // wrapping runs of non-column-spans and runs of column-spans.
        //
        // We return the the first one.  The whole setup is called an {ib}
        // split; in what follows "frames in the split" refers to the anonymous blocks
        // and inlines that contain our children.
        //
        // {ib} splits maintain the following invariants:
        // 1) All frames in the split have the NS_FRAME_PART_OF_IBSPLIT bit
        //    set.
        //
        // 2) Each frame in the split has the nsIFrame::IBSplitSibling
        //    property pointing to the next frame in the split, except for the
        //    last one, which does not have it set.
        //
        // 3) Each frame in the split has the nsIFrame::IBSplitPrevSibling
        //    property pointing to the previous frame in the split, except for the
        //    first one, which does not have it set.
        //
        // 4) The first and last frame in the split are always inlines.
        //
        // 5) The frames wrapping runs of non-column-spans are linked together as
        //    continuations. The frames wrapping runs of column-spans are *not*
        //    linked with each other nor with other non-column-span wrappers.
        //    (columns: NOT PORTED)
        //
        // 6) The first and last frame in the chains of blocks are always wrapping
        //    non-column-spans. Both of them are created even if they're empty.
        //
        // An invariant that is NOT maintained is that the wrappers are actually
        // linked via GetNextSibling linkage.  A simple example is an inline
        // containing an inline that contains a block.  The three parts of the inner
        // inline end up with three different parents.
        //
        // For example, this HTML:
        // <span>
        //   <div>a</div>
        //   <span>
        //     b
        //     <div>c</div>
        //   </span>
        //   d
        //   <div>e</div>
        //   f
        //  </span>
        // Gives the following frame tree:
        //
        // Inline (outer span)
        // Block (anonymous, outer span)
        //   Block (div)
        //     Text("a")
        // Inline (outer span)
        //   Inline (inner span)
        //     Text("b")
        // Block (anonymous, outer span)
        //   Block (anonymous, inner span)
        //     Block (div)
        //       Text("c")
        // Inline (outer span)
        //   Inline (inner span)
        //   Text("d")
        // Block (anonymous, outer span)
        //   Block (div)
        //     Text("e")
        // Inline (outer span)
        //   Text("f")

        let content = aItem.mContent;
        let computedStyle = aItem.mComputedStyle.clone();

        let newFrame = NS_NewInlineFrame(self.AllocFrame(computedStyle.clone())).0;

        // Initialize the frame
        self.InitAndRestoreFrame(aState, content, aParentFrame, newFrame);

        // definition cannot be inside next block because the object's destructor is
        // significant. this is part of the fix for bug 42372

        let mut absoluteSaveState = nsFrameConstructorSaveState::default();

        let isAbsPosCB = newFrame.IsAbsPosContainingBlock();
        newFrame.AddStateBits(NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN);
        if isAbsPosCB {
            // Relatively positioned frames becomes a container for child
            // frames that are positioned
            aState.PushAbsoluteContainingBlock(Some(newFrame), Some(newFrame), &mut absoluteSaveState);
        }

        // NS_FRAME_HAS_MULTI_COLUMN_ANCESTOR: NOT PORTED, columns.

        // Process the child content
        let mut childList = nsFrameList::default();
        self.ConstructFramesFromItemList(aState, &mut aItem.mChildItems, newFrame, false, &mut childList);

        let mut firstBlock: Option<nsIFrame> = None;
        if !aItem.mIsAllInline {
            for f in &childList {
                if f.IsBlockOutside() {
                    firstBlock = Some(f);
                    break;
                }
            }
        }

        if aItem.mIsAllInline || firstBlock.is_none() {
            // This part is easy.  We either already know we have no non-inline kids,
            // or haven't found any when constructing actual frames (the latter can
            // happen only if out-of-flows that we thought had no containing block
            // acquired one when ancestor inlines and {ib} splits got
            // constructed).  Just put all the kids into the single inline frame and
            // bail.
            nsContainerFrame::from_frame(newFrame).SetInitialChildList(FrameChildListID::Principal, childList);
            aState.AddChild(self, newFrame, aFrameList, content, aParentFrame, true, true, false, None);
            absoluteSaveState.Restore(aState); // ~nsFrameConstructorSaveState
            return newFrame;
        }

        // This inline frame contains several types of children. Therefore this frame
        // has to be chopped into several pieces, as described above.

        // Grab the first inline's kids
        let firstInlineKids = childList.TakeFramesBefore(firstBlock);
        nsContainerFrame::from_frame(newFrame).SetInitialChildList(FrameChildListID::Principal, firstInlineKids);

        aFrameList.AppendFrame(None, newFrame);

        newFrame.AddStateBits(NS_FRAME_OWNS_ANON_BOXES);
        self.CreateIBSiblings(aState, newFrame, isAbsPosCB, &mut childList, aFrameList);

        absoluteSaveState.Restore(aState); // ~nsFrameConstructorSaveState

        newFrame
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreateIBSiblings
    fn CreateIBSiblings(&mut self, aState: &mut nsFrameConstructorState, aInitialInline: nsIFrame, aIsAbsPosCB: bool, aChildList: &mut nsFrameList, aSiblings: &mut nsFrameList) {
        let content = aInitialInline.GetContent();
        let computedStyle = aInitialInline.Style();
        let parentFrame = aInitialInline.GetParent();

        // Resolve the right style for our anonymous blocks.
        //
        // The distinction in styles is needed because of CSS 2.1, section
        // 9.2.1.1, which says:
        //
        //   When such an inline box is affected by relative positioning, any
        //   resulting translation also affects the block-level box contained
        //   in the inline box.
        let blockSC = self.AnonymousStyle(&computedStyle, AnonymousDisplay::Block);

        let mut lastNewInline = aInitialInline.FirstContinuation();
        loop {
            // On entry to this loop aChildList is not empty and the first frame in it
            // is block-level.
            MOZ_ASSERT!(aChildList.NotEmpty(), "Should have child items");
            MOZ_ASSERT!(aChildList.FirstChild().unwrap().IsBlockOutside(), "Must have list starting with block");

            // The initial run of blocks belongs to an anonymous block that we create
            // right now. The anonymous block will be the parent of these block
            // children of the inline.
            let blockFrame = NS_NewBlockFrame(self.AllocFrame(blockSC.clone())).0;
            blockFrame.SetIsBlockWrapper(true);
            self.InitFrame(blockFrame, content.clone(), parentFrame);
            blockFrame.AddStateBits(aState.mAdditionalStateBits);

            // Find the first non-block child which defines the end of our block kids
            // and the start of our next inline's kids
            let blockKids = aChildList.Split(|f| !f.IsBlockOutside());

            // (the multi-column case: NOT PORTED)
            MoveChildrenTo(aInitialInline, blockFrame, blockKids);

            SetFrameIsIBSplit(lastNewInline, Some(blockFrame));
            aSiblings.AppendFrame(None, blockFrame);

            // Now grab the initial inlines in aChildList and put them into an inline
            // frame.
            let inlineFrame = NS_NewInlineFrame(self.AllocFrame(computedStyle.clone())).0;
            self.InitFrame(inlineFrame, content.clone(), parentFrame);
            inlineFrame.AddStateBits(aState.mAdditionalStateBits);
            inlineFrame.AddStateBits(NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN);

            if aIsAbsPosCB {
                inlineFrame.MarkAsAbsoluteContainingBlock();
            }

            if aChildList.NotEmpty() {
                let inlineKids = aChildList.Split(|f| f.IsBlockOutside());
                MoveChildrenTo(aInitialInline, inlineFrame, inlineKids);
            }

            SetFrameIsIBSplit(blockFrame, Some(inlineFrame));
            aSiblings.AppendFrame(None, inlineFrame);
            lastNewInline = inlineFrame;

            if !aChildList.NotEmpty() {
                break;
            }
        }

        SetFrameIsIBSplit(lastNewInline, None);
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::BuildInlineChildItems
    fn BuildInlineChildItems(&mut self, aState: &mut nsFrameConstructorState, aParentItem: &mut FrameConstructionItem, aItemIsWithinSVGText: bool, aItemAllowsTextPathChild: bool) {
        let parentStyle = aParentItem.mComputedStyle.clone();
        let parentContent = aParentItem.mContent;

        if !aItemIsWithinSVGText {
            if nsStyleDisplay::new(parentStyle.clone()).IsListItem() {
                self.CreateGeneratedContentItem(aState, None, parentContent, &parentStyle, PseudoStyleType::Marker, &mut aParentItem.mChildItems, ItemFlag::None);
            }
            // Probe for generated content before
            self.CreateGeneratedContentItem(aState, None, parentContent, &parentStyle, PseudoStyleType::Before, &mut aParentItem.mChildItems, ItemFlag::None);
        }

        let mut flags = ItemFlag::None;
        if aItemIsWithinSVGText {
            flags |= ItemFlag::IsWithinSVGText;
        }
        // (AllowTextPathChild: SVG, NOT PORTED)

        let mut iter = self.FlattenedChildIterator(parentContent);
        let mut content = iter.GetNextChild(self);
        while content != NONE {
            self.AddFrameConstructionItems(aState, content, false, &parentStyle, InsertionPoint { mParentFrame: None, mContainer: parentContent }, &mut aParentItem.mChildItems, flags);
            content = iter.GetNextChild(self);
        }

        if !aItemIsWithinSVGText {
            // Probe for generated content after
            self.CreateGeneratedContentItem(aState, None, parentContent, &parentStyle, PseudoStyleType::After, &mut aParentItem.mChildItems, ItemFlag::None);
        }

        aParentItem.mIsAllInline = aParentItem.mChildItems.AreAllItemsInline();
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ProcessChildren
    fn ProcessChildren(&mut self, aState: &mut nsFrameConstructorState, aContent: ContentId, aComputedStyle: &ServoArc<ComputedStyle>, aFrame: nsIFrame, aCanHaveGeneratedContent: bool, aFrameList: &mut nsFrameList, aAllowBlockStyles: bool, aPossiblyLeafFrame: Option<nsIFrame>) {
        let kMaxDepth: u32 = 2 * MAX_REFLOW_DEPTH;
        let savedDepth = self.mCurrentDepth;
        if self.mCurrentDepth != u16::MAX {
            self.mCurrentDepth += 1;
        }

        let aPossiblyLeafFrame = aPossiblyLeafFrame.unwrap_or(aFrame);

        // XXXbz ideally, this would do all the pushing of various
        // containing blocks as needed, so callers don't have to do it...

        // Check that our parent frame is a block before allowing ::first-letter/line.
        // E.g. <button style="display:grid"> should not allow it.
        let allowFirstPseudos = aAllowBlockStyles && aFrame.IsBlockFrameOrSubclass();
        // ShouldHaveSpecialBlockStyle: NOT PORTED, ::first-letter and ::first-line.
        let haveFirstLetterStyle = false;
        let haveFirstLineStyle = false;

        let mut itemsToConstruct = FrameConstructionItemList::default();

        // If we have first-letter or first-line style then frames can get
        // moved around so don't set these flags.
        if allowFirstPseudos && !haveFirstLetterStyle && !haveFirstLineStyle {
            itemsToConstruct.SetLineBoundaryAtStart(true);
            itemsToConstruct.SetLineBoundaryAtEnd(true);
        }

        // Create any anonymous frames we need here.
        // GetAnonymousContent, AddFCItemsForAnonymousContent: NOT PORTED, no frame class creates
        // anonymous content.

        // Generated content should have the same style parent as normal kids.
        //
        // Note that we don't use this style for looking up things like special
        // block styles because in some cases involving table pseudo-frames it has
        // nothing to do with the parent frame's desired behavior.
        // (nsIFrame::CorrectStyleParentFrame skips the frames of pseudo styles; there are none here)
        let styleParentFrame = aFrame;
        let parentStyle = styleParentFrame.Style();
        // ::backdrop for the top layer: NOT PORTED

        let mut listItem: Option<nsIFrame> = None;
        let mut isOutsideMarker = false;
        if !aPossiblyLeafFrame.IsLeaf() {
            if aCanHaveGeneratedContent {
                if nsStyleDisplay::new(parentStyle.clone()).IsListItem() && nsBlockFrame::do_QueryFrame(aFrame).is_some() {
                    listItem = Some(aFrame);
                    isOutsideMarker = styleParentFrame.StyleList().mListStylePosition() == StyleListStylePosition::Outside;
                    let mut extraFlags = ItemFlag::None;
                    if isOutsideMarker {
                        extraFlags |= ItemFlag::IsForOutsideMarker;
                    }
                    self.CreateGeneratedContentItem(aState, Some(aFrame), aContent, &parentStyle, PseudoStyleType::Marker, &mut itemsToConstruct, extraFlags);
                }
                // <option> checkmark: NOT PORTED
                // Probe for generated content before
                self.CreateGeneratedContentItem(aState, Some(aFrame), aContent, &parentStyle, PseudoStyleType::Before, &mut itemsToConstruct, ItemFlag::None);
            }

            let addChildItems = (self.mCurrentDepth as u32) < kMaxDepth;
            if !addChildItems {
                NS_WARNING!("ProcessChildren max depth exceeded");
            }

            let insertion = InsertionPoint { mParentFrame: Some(aFrame), mContainer: aContent };
            let mut iter = self.FlattenedChildIterator(aContent);
            let mut child = iter.GetNextChild(self);
            while child != NONE {
                if addChildItems {
                    self.AddFrameConstructionItems(aState, child, false, &parentStyle, insertion, &mut itemsToConstruct, ItemFlag::None);
                }
                child = iter.GetNextChild(self);
            }
            itemsToConstruct.SetParentHasNoShadowDOM(true);

            if aCanHaveGeneratedContent {
                // Probe for generated content after
                self.CreateGeneratedContentItem(aState, Some(aFrame), aContent, &parentStyle, PseudoStyleType::After, &mut itemsToConstruct, ItemFlag::None);
            }
        } else {
            // ClearLazyBits(aContent->GetFirstChild(), nullptr); NOT PORTED
        }

        self.ConstructFramesFromItemList(aState, &mut itemsToConstruct, aFrame, /* aParentIsWrapperAnonBox = */ false, aFrameList);

        if let Some(listItem) = listItem {
            if let Some(markerFrame) = self.GetMarkerFrame(aContent) {
                if aFrameList.ContainsFrame(markerFrame) {
                    if isOutsideMarker {
                        // SetMarkerFrameForListItem will add childFrame to the marker list.
                        aFrameList.RemoveFrame(markerFrame);
                        // (the column set wrapper case: NOT PORTED)
                    }
                    nsBlockFrame::from_frame(listItem).SetMarkerFrameForListItem(markerFrame);
                    if isOutsideMarker {
                        // ADAPTER: an outside ::marker is an independent formatting context
                        // (EstablishesBFC asks the frame whether it is the outside marker of its
                        // parent, which is only true from here on, after nsBlockFrame::Init ran)
                        markerFrame.AddStateBits(NS_BLOCK_BFC);
                    }
                    MOZ_ASSERT!(nsBlockFrame::from_frame(listItem).HasOutsideMarker() == isOutsideMarker);
                }
            }
        }

        // WrapFramesInFirstLetterFrame, WrapFramesInFirstLineFrame: NOT PORTED.

        self.mCurrentDepth = savedDepth;
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetMarkerFrame
    // ADAPTER: the primary frame of the ::marker container made for aContent.
    fn GetMarkerFrame(&self, aContent: ContentId) -> Option<nsIFrame> {
        let marker = *self.mMarkerOf.get(&aContent)?;
        self.mGenerated[(marker - kGeneratedContentBase) as usize].mPrimaryFrame
    }

    /*
     * aParentFrame - the frame that should be the parent of the generated
     *   content.  This is the frame for the corresponding content node,
     *   which must not be a leaf frame.
     *
     * Any items created are added to aItems.
     *
     * We create an XML element (tag _moz_generated_content_before/after/marker)
     * representing the pseudoelement. We create a DOM node for each 'content'
     * item and make those nodes the children of the XML element. Then we create
     * a frame subtree for the XML element as if it were a regular child of
     * aParentFrame/aParentContent, giving the XML element the ::before, ::after
     * or ::marker style.
     */
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreateGeneratedContentItem
    // REDUCED: ::before, ::after and ::marker (::backdrop, ::checkmark, ::picker-icon: NOT PORTED).
    // The style of the pseudo-element is the one Stylo computed for it (StyleDom::pseudo_style: eagerly
    // for ::before and ::after, on the first request for ::marker); it exists only if `display` is not
    // none and, for ::before and ::after, `content` has items (ProbePseudoElementStyle).
    fn CreateGeneratedContentItem(&mut self, aState: &mut nsFrameConstructorState, aParentFrame: Option<nsIFrame>, aOriginatingElement: ContentId, aStyle: &ServoArc<ComputedStyle>, aPseudoElement: PseudoStyleType, aItems: &mut FrameConstructionItemList, aExtraFlags: ItemFlags) {
        use mrt_style::style::selector_parser::PseudoElement;
        let (pseudo, kind) = match aPseudoElement {
            PseudoStyleType::Before => (PseudoElement::Before, GenConKind::Before),
            PseudoStyleType::After => (PseudoElement::After, GenConKind::After),
            PseudoStyleType::Marker => (PseudoElement::Marker, GenConKind::Marker),
            // ::backdrop, ::checkmark, ::picker-icon: NOT PORTED
            _ => return,
        };
        if aOriginatingElement >= kGeneratedContentBase {
            return;
        }

        // HasUAWidget: NOT PORTED, the form controls are leaf frames and do not get here.

        // ADAPTER: html.css gives `img::before` the content `-moz-alt-content`, which Stylo's servo
        // configuration does not parse. The alt text of an <img> that got no image frame (FindImgData)
        // is made here: an inline ::before, as the rule makes it, holding the alt text (the MozAltContent
        // case of CreateGeneratedContent), with the rule's `unicode-bidi: isolate`.
        let altText = if aPseudoElement == PseudoStyleType::Before { self.MozAltContentTextFor(aOriginatingElement, aStyle) } else { None };
        let isAltContent = altText.is_some();

        // Probe for the existence of the pseudo-element.
        // |ProbePseudoElementStyle| checks the relevant properties for the pseudo.
        // It only returns a non-null value if the pseudo should exist.
        let mut pseudoStyle = match self.sd().pseudo_style(aOriginatingElement, pseudo) {
            Some(s) => s,
            None if isAltContent => self.sd().anonymous_style(Some(&**aStyle), None),
            None => return,
        };

        // ADAPTER: an outside ::marker is block-level (Gecko's style adjuster blockifies it; Stylo's
        // servo configuration does not).
        if aPseudoElement == PseudoStyleType::Marker && (aExtraFlags & ItemFlag::IsForOutsideMarker).to_bool() {
            let display = pseudoStyle.get_box().clone_display();
            if !nsStyleDisplay::new(pseudoStyle.clone()).IsBlockOutsideStyle() {
                pseudoStyle = nsCSSFrameConstructor::StyleWithDisplay(&pseudoStyle, display.equivalent_block_display(false));
            }
        }

        if isAltContent {
            ServoArc::make_mut(&mut pseudoStyle).mutate_text().set_unicode_bidi(StyleUnicodeBidi::Isolate);
        }

        // ADAPTER: the container is an entry of mGenerated, Gecko's _moz_generated_content_* element.
        let container = self.NewGeneratedContent(aOriginatingElement, None, Some(pseudoStyle.clone()));
        match aPseudoElement {
            PseudoStyleType::Marker => {
                self.mMarkerOf.insert(aOriginatingElement, container);
            }
            PseudoStyleType::Before => {
                self.mBeforeOf.insert(aOriginatingElement, container);
            }
            PseudoStyleType::After => {
                self.mAfterOf.insert(aOriginatingElement, container);
            }
            _ => {}
        }

        let items = NonAltContentItems(&pseudoStyle);
        if isAltContent {
            self.AppendGeneratedTextNode(container, altText.unwrap_or_default());
        }
        let mut index = 0usize;
        for item in items {
            self.CreateGeneratedContent(aState, aOriginatingElement, &pseudoStyle, item, kind, index, container);
            index += 1;
        }
        // If a ::marker has no 'content' then generate it from its 'list-style-*'.
        if index == 0 && aPseudoElement == PseudoStyleType::Marker {
            self.CreateGeneratedContentFromListStyle(aOriginatingElement, &pseudoStyle, container);
        }
        let _ = aStyle;

        let flags = ItemFlag::IsGeneratedContent | aExtraFlags;
        self.AddFrameConstructionItemsInternal(aState, container, aParentFrame, true, &pseudoStyle, flags, aItems);
    }

    // ADAPTER: the text of `content: -moz-alt-content` (the MozAltContent case of CreateGeneratedContent) for
    // the ::before of aElement, if it has any here: the "alt" attribute of an <img> that got no image frame
    // (FindImgData), and the label of an <input> button (its `value`, else the localized default, "Submit
    // Query" for a submit button and "Reset" for a reset button: dom/locales' forms.properties). Gecko
    // makes the label of an input button with anonymous content; this runtime makes it the same way as
    // the alt text, a text child of the button's block.
    fn MozAltContentTextFor(&self, aElement: ContentId, aStyle: &ServoArc<ComputedStyle>) -> Option<String> {
        if self.IsHTMLElement(aElement, "img") {
            if nsImageFrame::ImageFrameTypeFor(self, aElement, aStyle) == nsImageFrame_ImageFrameType::None {
                // Use the "alt" attribute
                return Some(self.GetAttr(aElement, "alt").unwrap_or("").to_string());
            }
            return None;
        }
        if self.IsHTMLElement(aElement, "input") {
            let ty = self.GetAttr(aElement, "type").map(|t| t.to_ascii_lowercase()).unwrap_or_default();
            let defaultLabel = match ty.as_str() {
                "submit" => "Submit Query",
                "reset" => "Reset",
                "button" => "",
                _ => return None,
            };
            // HTMLInputElement::GetDisplayFileName's neighbour for buttons: the `value` attribute, else the default
            return Some(self.GetAttr(aElement, "value").map(|v| v.to_string()).unwrap_or_else(|| defaultLabel.to_string()));
        }
        None
    }

    // ADAPTER: nsINode::AppendChildTo for a generated text node.
    fn AppendGeneratedText(&mut self, aContainer: ContentId, aText: String) {
        if aText.is_empty() {
            return;
        }
        self.AppendGeneratedTextNode(aContainer, aText);
    }

    // ADAPTER: AppendGeneratedText for a text that may be empty (the label of an input button, whose empty
    // text node still gets a text frame because the button has `white-space: pre`, which gives it a line).
    fn AppendGeneratedTextNode(&mut self, aContainer: ContentId, aText: String) {
        let child = self.NewGeneratedContent(aContainer, Some(aText), None);
        let c = (aContainer - kGeneratedContentBase) as usize;
        let first = self.mGenerated[c].mFirstChild;
        if first == NONE {
            self.mGenerated[c].mFirstChild = child;
            return;
        }
        let mut last = first;
        while self.mGenerated[(last - kGeneratedContentBase) as usize].mNextSibling != NONE {
            last = self.mGenerated[(last - kGeneratedContentBase) as usize].mNextSibling;
        }
        self.mGenerated[(last - kGeneratedContentBase) as usize].mNextSibling = child;
    }

    // ADAPTER: the counter and quote text of the document, made when first needed.
    fn CounterManager(&mut self) -> &nsCounterManager {
        let sd = self.sd();
        self.mCounterManager.get_or_insert_with(|| nsCounterManager::new(sd))
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreateGeneratedContent
    // The item at aContentIndex of `content` of the pseudo-element aKind of aOriginatingElement becomes
    // a text node child of aContainer. Strings and `attr()` are made here; `counter()`, `counters()` and the
    // quotes get the text nsCounterManager made for them in document order.
    // NOT PORTED: images (`url()` in `content`, GeneratedImageContent), and the nodes with an initializer
    // that nsCounterManager and nsQuoteList update when the document changes.
    fn CreateGeneratedContent(&mut self, aState: &mut nsFrameConstructorState, aOriginatingElement: ContentId, aPseudoStyle: &ServoArc<ComputedStyle>, aItem: &ContentItem<Image>, aKind: GenConKind, aContentIndex: usize, aContainer: ContentId) {
        let _ = aState;
        let text = match GetStaticItemText(self.sd(), aOriginatingElement, aItem) {
            Some(t) => t,
            None => {
                let _ = aPseudoStyle;
                let text = self.CounterManager().GetTexts(aOriginatingElement, aKind).and_then(|t| t.get(aContentIndex).cloned()).unwrap_or_default();
                self.mCounterUses.push(CounterUse { mOriginating: aOriginatingElement, mKind: aKind, mIndex: aContentIndex, mContainer: aContainer, mText: text.clone() });
                text
            }
        };
        self.AppendGeneratedText(aContainer, text);
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreateGeneratedContentFromListStyle
    // and CreateGeneratedContentFromListStyleType
    // The text of the marker is the prefix, the counter value and the suffix of the list style: "1. ",
    // "b. ", a bullet and a space. The bullets need no counter; the others get the text nsCounterManager
    // made from the `list-item` counter at the list item.
    // NOT PORTED: list-style-image (the image is left out, its text is the space after it), and
    // `list-style-type: <string>` (Stylo's servo configuration has no strings there).
    fn CreateGeneratedContentFromListStyle(&mut self, aOriginatingElement: ContentId, aPseudoStyle: &ServoArc<ComputedStyle>, aContainer: ContentId) {
        let styleType = CounterStyleName(&aPseudoStyle.get_list().list_style_type.0);
        let hasImage = !matches!(aPseudoStyle.get_list().list_style_image, Image::None);
        if !hasImage && (CounterStyle_IsBullet(&styleType) || styleType == "none") {
            // We don't need a use node inserted for these.
            let text = CounterStyle_GetMarkerText(&styleType, 0);
            self.AppendGeneratedText(aContainer, text);
            return;
        }
        let text = self.CounterManager().GetTexts(aOriginatingElement, GenConKind::Marker).and_then(|t| t.first().cloned()).unwrap_or_default();
        self.mCounterUses.push(CounterUse { mOriginating: aOriginatingElement, mKind: GenConKind::Marker, mIndex: 0, mContainer: aContainer, mText: text.clone() });
        self.AppendGeneratedText(aContainer, text);
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::RecalcQuotesAndCounters
    // ADAPTER: the counter and quote text is made for the whole document at once here (nsCounterManager,
    // not a translation), so after the document changed it is made again, and the element of each
    // generated text that came from it and changed is reframed (Gecko updates the text node of each
    // nsGenConNode whose text changed, which reflows its frame). Uses whose frames are gone are dropped.
    pub fn RecalcQuotesAndCounters(&mut self) {
        if self.mCounterManager.is_none() {
            return;
        }
        let manager = nsCounterManager::new(self.sd());
        let uses = std::mem::take(&mut self.mCounterUses);
        let mut kept = Vec::with_capacity(uses.len());
        let mut reframe = Vec::new();
        for u in uses {
            if self.mGenerated[(u.mContainer - kGeneratedContentBase) as usize].mPrimaryFrame.is_none() {
                continue;
            }
            let text = manager.GetTexts(u.mOriginating, u.mKind).and_then(|t| t.get(u.mIndex).cloned()).unwrap_or_default();
            if text != u.mText {
                reframe.push(u.mOriginating);
            } else {
                kept.push(u);
            }
        }
        self.mCounterUses = kept;
        self.mCounterManager = Some(manager);
        reframe.dedup();
        for content in reframe {
            self.RecreateFramesForContent(content, InsertionKind::Async);
        }
    }
}

// ADAPTER: a generated text made from the counter manager (see RecalcQuotesAndCounters).
struct CounterUse {
    mOriginating: ContentId,
    mKind: GenConKind,
    mIndex: usize,
    // the ::before, ::after or ::marker container the text is in
    mContainer: ContentId,
    mText: String,
}

// ---- dynamic changes: removals, reframes and insertions ------------------------------------------
//
// Gecko: layout/base/nsCSSFrameConstructor.cpp, the content notifications (ContentRangeInserted,
// ContentWillBeRemoved, CharacterDataChanged) and RecreateFramesForContent with the functions they
// use, translated in the order of the C++ file (DESIGN, "Incremental style and layout", step 3).
//
// ADAPTERS:
//   - The DOM changes on another thread than the frames (the runtime's main thread); the layout
//     thread gets each removal with what Gecko reads of the DOM when it happens (the old parent and
//     siblings, RemovedContent), and applies it before the flush's restyle, as Gecko destroys the
//     frames at the removal. Insertions are built in the flush, as Gecko builds lazily inserted
//     content in the style flush (NODE_NEEDS_FRAME).
//   - InsertionKind::Async (Gecko: a ReconstructFrame change hint posted to the RestyleManager, or
//     NODE_NEEDS_FRAME) queues the content in mPendingReconstructs; the layout thread rebuilds it
//     with RecreateFramesForContent(.., Sync) in the same flush (TakePendingReconstructs).
//   - The pseudo type of an anonymous box frame is a frame property (AnonBoxPseudoTypeProperty, set by
//     AllocFrame), as Stylo's servo configuration has no anonymous box pseudo-elements.
//
// NOT PORTED: ::first-letter and ::first-line (RemoveLetterFrames, RecoverLetterFrames,
// AppendFirstLineFrames, CheckForFirstLineInsertion), columns (MaybeRecreateForColumnSpan,
// GetMultiColumnContainingBlockFor), ruby, MathML, framesets, fieldsets, the viewport scroll styles
// override, frame state capture, accessibility notifications, the list-style-image fallback.

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::InsertionKind
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InsertionKind {
    Sync,
    Async,
}

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::RemovalKind
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemovalKind {
    Dom,
    ForReconstruction,
    ForDisplayNoneChange,
}

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::ContainingBlockType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContainingBlockType {
    ABS_POS,
    FIXED_POS,
}

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::SiblingDirection
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SiblingDirection {
    Forward,
    Backward,
}

// ADAPTER: what Gecko's ContentWillBeRemoved reads of the DOM when a node is removed, recorded by the
// runtime at the removal (the frames are on another thread): the parent the node had (NONE for none,
// and DOCUMENT stands for the document), and its siblings then.
#[derive(Clone, Copy, Debug)]
pub struct RemovedContent {
    pub mChild: ContentId,
    pub mContainer: ContentId,
    pub mPrevSibling: ContentId,
    pub mNextSibling: ContentId,
    // prevSibling->GetPreviousSibling() and nextSibling->GetNextSibling() were not null
    pub mPrevSiblingHasPrevSibling: bool,
    pub mNextSiblingHasNextSibling: bool,
}

// ADAPTER: counts of what the dynamic paths did, for the flush's report.
#[derive(Clone, Copy, Default, Debug)]
pub struct DynamicChangeCounts {
    pub mRemovals: usize,
    pub mReconstructs: usize,
    pub mInsertions: usize,
    // reframes of an ancestor that a change asked for (WipeContainingBlock, ReframeContainingBlock,
    // MaybeRecreateContainerForFrameRemoval, WipeInsertionParent)
    pub mContainerReframes: usize,
}

// ADAPTER: Gecko's FlattenedChildIterator with its position (Seek, GetPreviousChild, starting at the
// end); the children are listed when it is made.
#[derive(Clone)]
pub struct FlatChildIterator {
    mParent: ContentId,
    mChildren: Vec<ContentId>,
    // the current child: -1 before the first, mChildren.len() after the last
    mIndex: isize,
    mShadowDOMInvolved: bool,
}

impl FlatChildIterator {
    pub fn GetNextChild(&mut self) -> ContentId {
        let n = self.mChildren.len() as isize;
        if self.mIndex < n {
            self.mIndex += 1;
        }
        if self.mIndex < n { self.mChildren[self.mIndex as usize] } else { NONE }
    }
    pub fn GetPreviousChild(&mut self) -> ContentId {
        if self.mIndex >= 0 {
            self.mIndex -= 1;
        }
        if self.mIndex >= 0 { self.mChildren[self.mIndex as usize] } else { NONE }
    }
    // Looks for aChildToFind; if found, it is the current child (GetNextChild gives the one after it,
    // GetPreviousChild the one before).
    pub fn Seek(&mut self, aChildToFind: ContentId) -> bool {
        match self.mChildren.iter().position(|&c| c == aChildToFind) {
            Some(i) => {
                self.mIndex = i as isize;
                true
            }
            None => false,
        }
    }
    pub fn Parent(&self) -> ContentId {
        self.mParent
    }
    pub fn ShadowDOMInvolved(&self) -> bool {
        self.mShadowDOMInvolved
    }
}

// Gecko: the anonymous box pseudo type of a frame's style (see the ADAPTERS above).
const kAnonBoxPseudoTypePropertyId: u32 = 8100;

impl nsIFrame {
    pub fn AnonBoxPseudoTypeProperty() -> FramePropertyDescriptor<PseudoStyleType> {
        FramePropertyDescriptor::new(kAnonBoxPseudoTypePropertyId)
    }

    // Gecko: Style()->GetPseudoType()
    pub fn StylePseudoType(&self) -> PseudoStyleType {
        if let Some(t) = self.GetProperty(nsIFrame::AnonBoxPseudoTypeProperty()) {
            return t;
        }
        if self.IsBlockWrapper() {
            return PseudoStyleType::MozBlockInsideInlineWrapper;
        }
        self.Style().GetPseudoType()
    }

    // Gecko: Style()->IsPseudoOrAnonBox(). ADAPTER: the frames of anonymous boxes made from a
    // parent's style alone (text, placeholders, the viewport and the canvas) are told by their type
    // or by having no content.
    pub fn StyleIsPseudoOrAnonBox(&self) -> bool {
        self.StylePseudoType() != PseudoStyleType::NotPseudo || self.GetContent().is_none() || self.IsTextFrame() || self.IsPlaceholderFrame()
    }

    // Gecko: Style()->IsWrapperAnonBox() (PseudoStyle::IsWrapperAnonBox, the `wrapper = true` entries
    // of anon_boxes.toml that are ported)
    pub fn StyleIsWrapperAnonBox(&self) -> bool {
        matches!(
            self.StylePseudoType(),
            PseudoStyleType::MozInlineTable | PseudoStyleType::MozTable | PseudoStyleType::MozTableCell | PseudoStyleType::MozTableRowGroup | PseudoStyleType::MozTableRow | PseudoStyleType::MozAnonymousItem
        )
    }

    // Gecko: PseudoStyle::IsAnonBox(Style()->GetPseudoType())
    pub fn StyleIsAnonBox(&self) -> bool {
        !matches!(self.StylePseudoType(), PseudoStyleType::NotPseudo | PseudoStyleType::Before | PseudoStyleType::After | PseudoStyleType::Marker | PseudoStyleType::Other)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::PrincipalChildList
    pub fn PrincipalChildList(&self) -> &'static nsFrameList {
        self.GetChildList(FrameChildListID::Principal)
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetTailContinuation
    pub fn GetTailContinuation(&self) -> nsIFrame {
        let mut frame = *self;
        while frame.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) {
            frame = frame.GetPrevContinuation().expect("first continuation can't be overflow container");
        }
        while let Some(next) = frame.GetNextContinuation() {
            if next.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) {
                break;
            }
            frame = next;
        }
        frame
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetInFlowParent
    pub fn GetInFlowParent(&self) -> Option<nsIFrame> {
        if self.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) {
            return self.GetPlaceholderFrame()?.GetParent();
        }
        self.GetParent()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsViewportFrame
    pub fn IsViewportFrame(&self) -> bool {
        self.Type() == LayoutFrameType::Viewport
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsGeneratedContentFrame
    pub fn IsGeneratedContentFrame(&self) -> bool {
        self.HasAnyStateBits(NS_FRAME_GENERATED_CONTENT)
    }
}

impl nsLayoutUtils {
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::IsProperAncestorFrame (aCommonAncestor null)
    pub fn IsProperAncestorFrame(aAncestorFrame: nsIFrame, aFrame: nsIFrame) -> bool {
        let mut f = aFrame.GetParent();
        while let Some(p) = f {
            if p == aAncestorFrame {
                return true;
            }
            f = p.GetParent();
        }
        false
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetChildListNameFor
    pub fn GetChildListNameFor(aChildFrame: nsIFrame) -> FrameChildListID {
        MOZ_ASSERT!(!aChildFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW));
        if aChildFrame.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) {
            let pif = aChildFrame.GetPrevInFlow().unwrap();
            if pif.GetParent() == aChildFrame.GetParent() {
                return FrameChildListID::ExcessOverflowContainers;
            }
            return FrameChildListID::OverflowContainers;
        }
        FrameChildListID::Principal
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::LastContinuationWithChild
    pub fn LastContinuationWithChild(aFrame: nsIFrame) -> nsIFrame {
        let mut f = aFrame.LastContinuation();
        while f.PrincipalChildList().IsEmpty() {
            match f.GetPrevContinuation() {
                Some(p) => f = p,
                None => break,
            }
        }
        f
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetNextContinuationOrIBSplitSibling
    pub fn GetNextContinuationOrIBSplitSibling(aFrame: nsIFrame) -> Option<nsIFrame> {
        if let Some(result) = aFrame.GetNextContinuation() {
            return Some(result);
        }
        if aFrame.HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT) {
            // We only store the ib-split sibling annotation with the first frame in the
            // continuation chain.
            return aFrame.FirstContinuation().GetProperty(nsIFrame::IBSplitSibling());
        }
        None
    }
}

// Returns true if aFrame is an anonymous flex/grid item.
// Gecko: layout/base/nsCSSFrameConstructor.cpp IsAnonymousItem
fn IsAnonymousItem(aFrame: nsIFrame) -> bool {
    aFrame.StylePseudoType() == PseudoStyleType::MozAnonymousItem
}

/**
 * True if aFrame is an actual inline frame in the sense of non-replaced
 * display:inline CSS boxes.  In other words, it can be affected by {ib}
 * splitting and can contain first-letter frames.  Basically, this is either an
 * inline frame (positioned or otherwise) or an line frame (this last because
 * it can contain first-letter and because inserting blocks in the middle of it
 * needs to terminate it).
 */
// Gecko: layout/base/nsCSSFrameConstructor.cpp IsInlineFrame
fn IsInlineFrame(aFrame: nsIFrame) -> bool {
    aFrame.IsLineParticipant()
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp GetIBSplitSibling
fn GetIBSplitSibling(aFrame: nsIFrame) -> Option<nsIFrame> {
    MOZ_ASSERT!(IsFramePartOfIBSplit(aFrame), "Shouldn't call this");

    // We only store the "ib-split sibling" annotation with the first
    // frame in the continuation chain. Walk back to find that frame now.
    aFrame.FirstContinuation().GetProperty(nsIFrame::IBSplitSibling())
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp GetIBSplitPrevSibling
fn GetIBSplitPrevSibling(aFrame: nsIFrame) -> Option<nsIFrame> {
    MOZ_ASSERT!(IsFramePartOfIBSplit(aFrame), "Shouldn't call this");

    // We only store the ib-split sibling annotation with the first
    // frame in the continuation chain. Walk back to find that frame now.
    aFrame.FirstContinuation().GetProperty(nsIFrame::IBSplitPrevSibling())
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp GetLastIBSplitSibling
fn GetLastIBSplitSibling(aFrame: nsIFrame) -> nsIFrame {
    let mut frame = aFrame;
    loop {
        match GetIBSplitSibling(frame) {
            Some(next) => frame = next,
            None => return frame,
        }
    }
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp GetIBContainingBlockFor
fn GetIBContainingBlockFor(aFrame: nsIFrame) -> nsIFrame {
    MOZ_ASSERT!(IsFramePartOfIBSplit(aFrame), "GetIBContainingBlockFor() should only be called on known IB frames");

    // Get the first "normal" ancestor of the target frame.
    let mut aFrame = aFrame;
    let mut parentFrame;
    loop {
        parentFrame = aFrame.GetParent();

        let Some(p) = parentFrame else {
            NS_ASSERTION!(false, "no unsplit block frame in IB hierarchy");
            return aFrame;
        };

        // Note that we ignore non-ib-split frames which have a pseudo on their
        // ComputedStyle -- they're not the frames we're looking for!  In
        // particular, they may be hiding a real parent that _is_ in an ib-split.
        if !IsFramePartOfIBSplit(p) && !p.StyleIsPseudoOrAnonBox() {
            break;
        }

        aFrame = p;
    }

    // post-conditions
    let parentFrame = parentFrame.unwrap();
    NS_ASSERTION!(!IsFramePartOfIBSplit(parentFrame), "parentFrame is actually an ib-split frame");

    parentFrame
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp IsTablePseudo
fn IsTablePseudo(aFrame: nsIFrame) -> bool {
    let pseudoType = aFrame.StylePseudoType();
    if pseudoType == PseudoStyleType::NotPseudo {
        return false;
    }
    pseudoType == PseudoStyleType::MozTable
        || pseudoType == PseudoStyleType::MozInlineTable
        || pseudoType == PseudoStyleType::MozTableColumnGroup
        || pseudoType == PseudoStyleType::MozTableRowGroup
        || pseudoType == PseudoStyleType::MozTableRow
        || pseudoType == PseudoStyleType::MozTableCell
        || (pseudoType == PseudoStyleType::MozCellContent && aFrame.GetParent().unwrap().StylePseudoType() == PseudoStyleType::MozTableCell)
        || (pseudoType == PseudoStyleType::MozTableWrapper && nsTableWrapperFrame::from_frame(aFrame).InnerTableFrame().StyleIsPseudoOrAnonBox())
}

// Note that this is (subtly) different to ParentIsWrapperAnonBox, since
// ParentIsWrapperAnonBox is really just about restyles, but there are wrapper
// anon boxes that don't need to return true for that...
// FIXME(emilio): This should be less complicated, ParentIsWrapperAnonBox should
// probably be renamed to something else, and this should just use
// IsWrapperAnonBox or similar...
// Gecko: layout/base/nsCSSFrameConstructor.cpp IsWrapperPseudo
fn IsWrapperPseudo(aFrame: nsIFrame) -> bool {
    if !aFrame.StyleIsAnonBox() {
        return false;
    }
    aFrame.StyleIsWrapperAnonBox() || IsTablePseudo(aFrame)
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp IsInAnonymousTable
fn IsInAnonymousTable(aFrame: nsIFrame) -> bool {
    let mut f = Some(aFrame);
    while let Some(fr) = f {
        if !IsWrapperPseudo(fr) {
            return false;
        }
        if fr.IsTableWrapperFrame() {
            return true;
        }
        f = fr.GetParent();
    }
    MOZ_ASSERT_UNREACHABLE!("Expected to be called inside tables");
    false
}

/**
 * Finds the right parent frame to append content to aParentFrame.
 *
 * Cannot return or receive null.
 */
// Gecko: layout/base/nsCSSFrameConstructor.cpp ContinuationToAppendTo
fn ContinuationToAppendTo(aParentFrame: nsIFrame) -> nsIFrame {
    if IsFramePartOfIBSplit(aParentFrame) {
        // If the frame we are manipulating is a ib-split frame (that is, one that's
        // been created as a result of a block-in-inline situation) then we need to
        // append to the last ib-split sibling, not to the frame itself.
        //
        // Always make sure to look at the last continuation of the frame for the
        // {ib} case, even if that continuation is empty.
        //
        // We don't do this for the non-ib-split-frame case, since in the other
        // cases appending to the last nonempty continuation is fine and in fact not
        // doing that can confuse code that doesn't know to pull kids from
        // continuations other than its next one.
        return GetLastIBSplitSibling(aParentFrame).LastContinuation();
    }

    nsLayoutUtils::LastContinuationWithChild(aParentFrame)
}

/**
 * This function will get the next sibling for a frame insert operation given
 * the parent and previous sibling.  aPrevSibling may be null.
 */
// Gecko: layout/base/nsCSSFrameConstructor.cpp GetInsertNextSibling
fn GetInsertNextSibling(aParentFrame: nsIFrame, aPrevSibling: Option<nsIFrame>) -> Option<nsIFrame> {
    if let Some(p) = aPrevSibling {
        return p.GetNextSibling();
    }

    aParentFrame.PrincipalChildList().FirstChild()
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp ParentIsWrapperAnonBox
fn ParentIsWrapperAnonBox(aParent: nsIFrame) -> bool {
    let mut maybeAnonBox = aParent;
    if maybeAnonBox.StylePseudoType() == PseudoStyleType::MozCellContent {
        // The thing that would maybe be a wrapper anon box is the cell.
        maybeAnonBox = maybeAnonBox.GetParent().unwrap();
    }
    maybeAnonBox.StyleIsWrapperAnonBox()
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp IsWhitespaceFrame
fn IsWhitespaceFrame(aFrame: nsIFrame) -> bool {
    aFrame.IsTextFrame() && aFrame.GetContent().is_some_and(|c| c.TextIsOnlyWhitespace())
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp IsSyntheticColGroup
fn IsSyntheticColGroup(aFrame: nsIFrame) -> bool {
    aFrame.IsTableColGroupFrame() && nsTableColGroupFrame::from_frame(aFrame).IsSynthetic()
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp IsOnlyNonWhitespaceFrameInList
fn IsOnlyNonWhitespaceFrameInList(aFrameList: &nsFrameList, aFrame: nsIFrame, aIgnoreFrame: Option<nsIFrame>) -> bool {
    let mut aFrame = Some(aFrame);
    for f in aFrameList {
        if Some(f) == aIgnoreFrame {
            continue;
        }
        if Some(f) == aFrame {
            // If we have continuations, ignore them too.
            aFrame = aFrame.unwrap().GetNextContinuation();
        } else if !IsWhitespaceFrame(f) && !IsSyntheticColGroup(f) {
            // Synthetic colgroups get created unconditionally, so let's not consider
            // them as giving us a non-whitespace frame.
            return false;
        }
    }
    true
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp AllChildListsAreEffectivelyEmpty
fn AllChildListsAreEffectivelyEmpty(aFrame: nsIFrame) -> bool {
    let mut lists = Vec::new();
    aFrame.GetChildLists(&mut lists);
    for l in lists {
        if l.mList.IsEmpty() {
            continue;
        }
        // We have some existing frame, usually that would be considered as making
        // this list nonempty. But let's make an exception for the synthetic
        // colgroup that tables have, since that gets created unconditionally.
        if l.mID == FrameChildListID::Principal && aFrame.IsTableFrame() {
            if let Some(f) = l.mList.OnlyChild() {
                if IsSyntheticColGroup(f) {
                    continue;
                }
            }
        }
        return false;
    }
    true
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp SafeToInsertPseudoNeedingChildren
fn SafeToInsertPseudoNeedingChildren(aFrame: nsIFrame) -> bool {
    AllChildListsAreEffectivelyEmpty(aFrame)
}

// Returns true if aFrame is the only meaningful child of aParent (which is
// known to be a wrapper-pseudo). This lets us determine whether aParent can be
// removed, as a result of aFrame being removed.
// Gecko: layout/base/nsCSSFrameConstructor.cpp IsOnlyMeaningfulChildOfWrapperPseudo
fn IsOnlyMeaningfulChildOfWrapperPseudo(aFrame: nsIFrame, aParent: nsIFrame) -> bool {
    MOZ_ASSERT!(IsWrapperPseudo(aParent));
    // Handle a few special cases with tables and colgroups / captions.
    if aParent.IsTableFrame() {
        let wrapper = aParent.GetParent().unwrap();
        MOZ_ASSERT!(wrapper.IsTableWrapperFrame());
        MOZ_ASSERT!(!aFrame.IsTableCaption(), "Caption parent should be the wrapper");
        // We can't remove the table if there are any captions present (captions are
        // never anonymous themselves), because table wrapper always relies on
        // having a table frame.
        if wrapper.PrincipalChildList().OnlyChild().is_none() {
            return false;
        }
    }
    if aFrame.IsTableCaption() {
        MOZ_ASSERT!(aParent.IsTableWrapperFrame());
        let table = nsTableWrapperFrame::from_frame(aParent).InnerTableFrame().0;
        return IsOnlyNonWhitespaceFrameInList(aParent.PrincipalChildList(), aFrame, /* aIgnoreFrame = */ Some(table)) &&
            // This checks for both colgroups and the principal list of the table
            // frame.
            AllChildListsAreEffectivelyEmpty(table);
    }
    IsOnlyNonWhitespaceFrameInList(aParent.PrincipalChildList(), aFrame, None)
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp CanRemoveWrapperPseudoForChildRemoval
fn CanRemoveWrapperPseudoForChildRemoval(aFrame: nsIFrame, aParent: nsIFrame) -> bool {
    if !IsOnlyMeaningfulChildOfWrapperPseudo(aFrame, aParent) {
        return false;
    }
    // aParent->IsRubyBaseContainerFrame(): NOT PORTED, ruby.
    true
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp FindNextNonWhitespaceSibling
fn FindNextNonWhitespaceSibling(aFrame: nsIFrame) -> Option<nsIFrame> {
    let mut f = aFrame.GetNextSibling();
    while let Some(fr) = f {
        if !IsWhitespaceFrame(fr) {
            break;
        }
        f = fr.GetNextSibling();
    }
    f
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp FindPreviousNonWhitespaceSibling
fn FindPreviousNonWhitespaceSibling(aFrame: nsIFrame) -> Option<nsIFrame> {
    let mut f = aFrame.GetPrevSibling();
    while let Some(fr) = f {
        if !IsWhitespaceFrame(fr) {
            break;
        }
        f = fr.GetPrevSibling();
    }
    f
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp IsSafeToAppendToIBSplitInline
fn IsSafeToAppendToIBSplitInline(aParentFrame: nsIFrame, aNextSibling: Option<nsIFrame>) -> bool {
    MOZ_ASSERT!(IsInlineFrame(aParentFrame), "Must have an inline parent here");

    let mut aParentFrame = aParentFrame;
    let mut aNextSibling = aNextSibling;
    loop {
        NS_ASSERTION!(IsFramePartOfIBSplit(aParentFrame), "How is this not part of an ib-split?");
        if aNextSibling.is_some() || aParentFrame.GetNextContinuation().is_some() || GetIBSplitSibling(aParentFrame).is_some() {
            return false;
        }

        aNextSibling = aParentFrame.GetNextSibling();
        aParentFrame = aParentFrame.GetParent().unwrap();
        if !IsInlineFrame(aParentFrame) {
            break;
        }
    }

    true
}

impl nsCSSFrameConstructor {
    // ---- ADAPTER: the DOM of the dynamic paths -------------------------------------------------------

    // Gecko: Element::IsDisplayContents (the element's style says `display: contents`). Generated
    // content and text are never display: contents.
    fn IsDisplayContents(&self, aContent: ContentId) -> bool {
        if aContent == NONE || aContent >= kGeneratedContentBase || !self.IsElement(aContent) {
            return false;
        }
        self.sd().computed(aContent).is_some_and(|s| nsStyleDisplay::new(s).mDisplay() == StyleDisplayValue::Contents)
    }

    // Gecko: nsIContent::GetFlattenedTreeParent (NONE for the document element, whose parent is the
    // document, and for a node out of the flattened tree)
    fn GetFlattenedTreeParent(&self, aContent: ContentId) -> ContentId {
        if aContent >= kGeneratedContentBase {
            return self.mGenerated[(aContent - kGeneratedContentBase) as usize].mParent;
        }
        let parent = self.sd().tree.dom.flattened_parent(aContent);
        if parent == self.sd().tree.doc { NONE } else { parent }
    }

    // Gecko: nsIContent::GetParent (the document element has none: the document is not content).
    // While a DOM removal is applied, the removed node's parent is the one it had (RemovedContent).
    fn GetParentOf(&self, aContent: ContentId) -> ContentId {
        if let Some(r) = self.mRemoval {
            if r.mChild == aContent {
                return if r.mContainer == self.sd().tree.doc { NONE } else { r.mContainer };
            }
        }
        if aContent >= kGeneratedContentBase {
            return self.mGenerated[(aContent - kGeneratedContentBase) as usize].mParent;
        }
        let parent = self.sd().tree.dom.node(aContent).parent;
        if parent == self.sd().tree.doc { NONE } else { parent }
    }

    // Gecko: nsIContent::GetComposedDoc() != null (the node is connected)
    fn IsInComposedDoc(&self, aContent: ContentId) -> bool {
        aContent < kGeneratedContentBase && self.sd().tree.is_connected(aContent)
    }

    // Gecko: FlattenedChildIterator(aParent, aStartAtBeginning)
    pub fn FlatChildIterator(&self, aParent: ContentId, aStartAtBeginning: bool) -> FlatChildIterator {
        let mut children = Vec::new();
        let mut iter = self.FlattenedChildIterator(aParent);
        let shadowDOMInvolved = iter.mList.is_some();
        let mut c = iter.GetNextChild(self);
        while c != NONE {
            children.push(c);
            c = iter.GetNextChild(self);
        }
        let n = children.len() as isize;
        FlatChildIterator { mParent: aParent, mChildren: children, mIndex: if aStartAtBeginning { -1 } else { n }, mShadowDOMInvolved: shadowDOMInvolved }
    }

    // Gecko: StyleChildrenIterator: the ::before, the flattened children and the ::after of aContent
    // (::marker: NOT PORTED here, a display: contents element has none).
    fn StyleChildren(&self, aContent: ContentId) -> Vec<ContentId> {
        let mut out = Vec::new();
        if let Some(&b) = self.mBeforeOf.get(&aContent) {
            out.push(b);
        }
        let mut iter = self.FlatChildIterator(aContent, true);
        let mut c = iter.GetNextChild();
        while c != NONE {
            out.push(c);
            c = iter.GetNextChild();
        }
        if let Some(&a) = self.mAfterOf.get(&aContent) {
            out.push(a);
        }
        out
    }

    // Gecko: nsLayoutUtils::GetBeforeFrame, GetAfterFrame, GetMarkerFrame (the primary frame of the
    // pseudo-element of aContent)
    fn GetBeforeFrame(&self, aContent: ContentId) -> Option<nsIFrame> {
        self.GetPrimaryFrame(*self.mBeforeOf.get(&aContent)?)
    }
    fn GetAfterFrame(&self, aContent: ContentId) -> Option<nsIFrame> {
        self.GetPrimaryFrame(*self.mAfterOf.get(&aContent)?)
    }

    // ADAPTER: the content of a frame as a ContentId (see mFrameContent)
    fn ContentIdOf(&self, aFrame: nsIFrame) -> ContentId {
        self.ContentOfFrame(aFrame)
    }

    // ADAPTER: the reconstructions InsertionKind::Async asked for, in order (see the ADAPTERS above).
    pub fn TakePendingReconstructs(&mut self) -> Vec<ContentId> {
        std::mem::take(&mut self.mPendingReconstructs)
    }

    // ADAPTER: what the dynamic paths did since the last call.
    pub fn TakeDynamicChangeCounts(&mut self) -> DynamicChangeCounts {
        std::mem::take(&mut self.mDynamicChangeCounts)
    }

    // ADAPTER: frees the slots of the frames destroyed since the last call (see FrameTree::
    // mDestroyedFrames), after forgetting what this constructor knows of them: the content each was
    // made for, and the primary frame of generated content (Gecko's generated content nodes die with
    // their frames).
    pub fn ReleaseDestroyedFrames(&mut self) {
        let frames = WithFrameTree(|t| t.TakeDestroyedFrames());
        for &f in &frames {
            let content = self.ContentOfFrame(f);
            if content >= kGeneratedContentBase && content != NONE {
                let g = &mut self.mGenerated[(content - kGeneratedContentBase) as usize];
                if g.mPrimaryFrame == Some(f) {
                    g.mPrimaryFrame = None;
                }
            }
            if let Some(slot) = self.mFrameContent.get_mut(f.0 as usize) {
                *slot = NONE;
            }
        }
        crate::driver::ReleaseFrameSlots(&frames);
    }

    // ADAPTER: the caches keyed by a style's address are emptied at each flush: their keys pin the
    // styles they were made from (see AnonymousStyle), which would otherwise live as long as the page.
    pub fn ClearStyleCaches(&mut self) {
        self.mAnonymousStyles.clear();
        self.mAnonBoxStyles.clear();
        self.mAnonBoxPseudoOfStyle.clear();
    }

    // ---- the translation ------------------------------------------------------------------------------

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::GetAbsoluteContainingBlock
    pub fn GetAbsoluteContainingBlock(&self, aFrame: nsIFrame, aType: ContainingBlockType) -> Option<nsIFrame> {
        // Starting with aFrame, look for a frame that is absolutely positioned or
        // relatively positioned (and transformed, if aType is FIXED)
        let mut frame = Some(aFrame);
        while let Some(f) = frame {
            frame = f.GetParent();
            // if (frame->IsMathMLFrame()) { return nullptr; }  NOT PORTED: MathML.

            // Look for the ICB.
            if aType == ContainingBlockType::FIXED_POS && f.IsViewportFrame() {
                return Some(f);
            }

            // If the frame is positioned, we will probably return it as the containing
            // block (see the exceptions below).  Otherwise, we'll start looking at the
            // parent frame, unless we're dealing with a scrollframe.
            // Scrollframes are special since they're not positioned, but their
            // scrolledframe might be.  So, we need to check this special case to return
            // the correct containing block (the scrolledframe) in that case.
            // If we're looking for a fixed-pos containing block and the frame is
            // not transformed, skip it.
            if !f.IsAbsPosContainingBlock() {
                continue;
            }
            if aType == ContainingBlockType::FIXED_POS && !f.IsFixedPosContainingBlock() {
                continue;
            }
            // (the fieldset and scroll container cases: NOT PORTED, there are no such frames)
            // Only first continuations or first IB-split siblings can be containing
            // blocks.
            let absPosCBCandidate = nsLayoutUtils::FirstContinuationOrIBSplitSibling(f);
            // Is the frame really an absolute container?
            if !absPosCBCandidate.IsAbsoluteContainer() {
                continue;
            }

            // For tables, skip the inner frame and consider the table wrapper frame.
            if absPosCBCandidate.IsTableFrame() {
                continue;
            }
            // For table wrapper frames, we can just return absPosCBCandidate.
            return Some(absPosCBCandidate);
        }

        MOZ_ASSERT!(aType != ContainingBlockType::FIXED_POS, "no ICB in this frame tree?");

        // It is possible for the search for the containing block to fail, because
        // no absolute container can be found in the parent chain.  In those cases,
        // we fall back to the document element's containing block.
        self.mDocElementContainingBlock
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::AppendFramesToParent
    fn AppendFramesToParent(&mut self, aState: &mut nsFrameConstructorState, aParentFrame: nsIFrame, aFrameList: &mut nsFrameList, aPrevSibling: Option<nsIFrame>, aIsRecursiveCall: bool) {
        MOZ_ASSERT!(!IsFramePartOfIBSplit(aParentFrame) || GetIBSplitSibling(aParentFrame).map_or(true, |s| s.PrincipalChildList().FirstChild().is_none()), "aParentFrame has a ib-split sibling with kids?");
        MOZ_ASSERT!(aPrevSibling.map_or(true, |p| p.GetParent() == Some(aParentFrame)), "Parent and prevsibling don't match");

        let nextSibling = GetInsertNextSibling(aParentFrame, aPrevSibling);
        let _ = aIsRecursiveCall;

        // If we're inserting a list of frames at the end of the trailing inline
        // of an {ib} split, we may need to create additional {ib} siblings to parent
        // them.
        if nextSibling.is_none() && IsFramePartOfIBSplit(aParentFrame) {
            // When we get here, our frame list might start with a block.  If it does
            // so, and aParentFrame is an inline, and it and all its previous
            // continuations have no siblings, then put the initial blocks from the
            // frame list into the previous block of the {ib} split.  Note that we
            // didn't want to stop at the block part of the split when figuring out
            // initial parent, because that could screw up float parenting; it's easier
            // to do this little fixup here instead.
            if aFrameList.NotEmpty() && aFrameList.FirstChild().unwrap().IsBlockOutside() {
                // See whether our trailing inline is empty
                let firstContinuation = aParentFrame.FirstContinuation();
                if firstContinuation.PrincipalChildList().IsEmpty() {
                    // Our trailing inline is empty.  Collect our starting blocks from
                    // aFrameList, get the right parent frame for them, and put them in.
                    let blockKids = aFrameList.Split(|f| !f.IsBlockOutside());
                    NS_ASSERTION!(blockKids.NotEmpty(), "No blocks?");

                    let prevBlock = GetIBSplitPrevSibling(firstContinuation).unwrap();
                    let prevBlock = prevBlock.LastContinuation();
                    MoveChildrenTo(aParentFrame, prevBlock, blockKids);
                }
            }

            // We want to put some of the frames into this inline frame.
            let inlineKids = aFrameList.Split(|f| f.IsBlockOutside());

            if !inlineKids.IsEmpty() {
                nsFrameManager::AppendFrames(aParentFrame, FrameChildListID::Principal, inlineKids);
            }

            if !aFrameList.IsEmpty() {
                let mut ibSiblings = nsFrameList::default();
                let isAbsPosCB = aParentFrame.IsAbsPosContainingBlock();
                self.CreateIBSiblings(aState, aParentFrame, isAbsPosCB, aFrameList, &mut ibSiblings);

                // Make sure to trigger reflow of the inline that used to be our
                // last one and now isn't anymore, since its GetSkipSides() has
                // changed.
                PresShell_FrameNeedsReflow(aParentFrame, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);

                // Recurse so we create new ib siblings as needed for aParentFrame's
                // parent
                return self.AppendFramesToParent(aState, aParentFrame.GetParent().unwrap(), &mut ibSiblings, Some(aParentFrame), true);
            }
            return;
        }

        // (the ::-moz-column-content case: NOT PORTED, columns)

        // Insert the frames after our aPrevSibling
        nsFrameManager::InsertFrames(aParentFrame, FrameChildListID::Principal, aPrevSibling, std::mem::take(aFrameList));
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindSiblingInternal
    fn FindSiblingInternal(&self, aIter: &mut FlatChildIterator, aDirection: SiblingDirection) -> Option<nsIFrame> {
        let getInsideMarkerFrame = |this: &Self, aContent: ContentId| -> Option<nsIFrame> {
            let marker = this.GetMarkerFrame(aContent)?;
            let isInsideMarker = marker.GetInFlowParent().is_some_and(|p| p.StyleList().mListStylePosition() == StyleListStylePosition::Inside);
            if isInsideMarker { Some(marker) } else { None }
        };

        let getNearPseudo = |this: &Self, aContent: ContentId| -> Option<nsIFrame> {
            if aDirection == SiblingDirection::Forward {
                // (::backdrop: NOT PORTED)
                if let Some(marker) = getInsideMarkerFrame(this, aContent) {
                    return Some(marker);
                }
                return this.GetBeforeFrame(aContent);
            }
            this.GetAfterFrame(aContent)
        };

        let getFarPseudo = |this: &Self, aContent: ContentId| -> Option<nsIFrame> {
            if aDirection == SiblingDirection::Forward {
                // (::picker-icon: NOT PORTED)
                return this.GetAfterFrame(aContent);
            }
            if let Some(before) = this.GetBeforeFrame(aContent) {
                return Some(before);
            }
            // (::backdrop: NOT PORTED)
            getInsideMarkerFrame(this, aContent)
        };

        loop {
            let sibling = if aDirection == SiblingDirection::Forward { aIter.GetNextChild() } else { aIter.GetPreviousChild() };
            if sibling == NONE {
                break;
            }
            // NOTE(emilio): It's important to check GetPrimaryFrame() before
            // IsDisplayContents to get the correct insertion point when multiple
            // siblings go from display: non-none to display: contents.
            if let Some(primaryFrame) = self.GetPrimaryFrame(sibling) {
                // XXX the GetContent() == sibling check is needed due to bug 135040.
                // Remove it once that's fixed.
                // (the rendered legend check: NOT PORTED, fieldsets)
                if self.ContentIdOf(primaryFrame) == sibling {
                    return Some(primaryFrame);
                }
            }

            if self.IsDisplayContents(sibling) {
                if let Some(frame) = getNearPseudo(self, sibling) {
                    return Some(frame);
                }

                let startFromBeginning = aDirection == SiblingDirection::Forward;
                let mut iter = self.FlatChildIterator(sibling, startFromBeginning);
                if let Some(sibling) = self.FindSiblingInternal(&mut iter, aDirection) {
                    return Some(sibling);
                }
            }
        }

        getFarPseudo(self, aIter.Parent())
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::AdjustSiblingFrame
    fn AdjustSiblingFrame(&self, aSibling: Option<nsIFrame>, aDirection: SiblingDirection) -> Option<nsIFrame> {
        let mut aSibling = aSibling?;

        if aSibling.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) {
            aSibling = aSibling.GetPlaceholderFrame().expect("an out-of-flow without a placeholder");
        }

        MOZ_ASSERT!(aSibling.GetPrevContinuation().is_none(), "How?");
        if aDirection == SiblingDirection::Backward {
            // The frame may be a ib-split frame (a split inline frame that contains a
            // block).  Get the last part of that split.
            if IsFramePartOfIBSplit(aSibling) {
                aSibling = GetLastIBSplitSibling(aSibling);
            }

            // The frame may have a continuation. If so, we want the last
            // non-overflow-container continuation as our previous sibling.
            aSibling = aSibling.GetTailContinuation();
        }

        Some(aSibling)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindPreviousSibling
    fn FindPreviousSibling(&self, aIter: &FlatChildIterator) -> Option<nsIFrame> {
        self.AdjustSiblingFrame(self.FindSibling(aIter, SiblingDirection::Backward), SiblingDirection::Backward)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindNextSibling
    fn FindNextSibling(&self, aIter: &FlatChildIterator) -> Option<nsIFrame> {
        self.AdjustSiblingFrame(self.FindSibling(aIter, SiblingDirection::Forward), SiblingDirection::Forward)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindSibling
    fn FindSibling(&self, aIter: &FlatChildIterator, aDirection: SiblingDirection) -> Option<nsIFrame> {
        let mut siblingIter = aIter.clone();
        if let Some(sibling) = self.FindSiblingInternal(&mut siblingIter, aDirection) {
            return Some(sibling);
        }

        // Our siblings (if any) do not have a frame to guide us. The frame for the
        // target content should be inserted whereever a frame for the container would
        // be inserted. This is needed when inserting into display: contents nodes.
        let mut current = aIter.Parent();
        while self.IsDisplayContents(current) {
            let parent = self.GetFlattenedTreeParent(current);
            MOZ_ASSERT!(parent != NONE, "No display: contents on the root");

            let mut iter = self.FlatChildIterator(parent, true);
            iter.Seek(current);
            if let Some(sibling) = self.FindSiblingInternal(&mut iter, aDirection) {
                return Some(sibling);
            }

            current = parent;
        }

        None
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::GetInsertionPrevSibling
    fn GetInsertionPrevSibling(&self, aInsertion: &mut InsertionPoint, aChild: ContentId, aIsAppend: &mut bool) -> Option<nsIFrame> {
        MOZ_ASSERT!(aInsertion.mParentFrame.is_some(), "Must have parent frame to start with");

        *aIsAppend = false;

        // Find the frame that precedes the insertion point.
        let mut iter = self.FlatChildIterator(aInsertion.mContainer, true);
        // (the IsRootOfNativeAnonymousSubtree() case: NOT PORTED, there is no native anonymous content)
        iter.Seek(aChild);

        // Note that FindPreviousSibling is passed the iterator by value, so that
        // the later usage of the iterator starts from the same place.
        let mut prevSibling = self.FindPreviousSibling(&iter);

        // Now, find the geometric parent so that we can handle continuations
        // properly. Use the prev sibling if we have it; otherwise use the next
        // sibling.
        if let Some(prev) = prevSibling {
            aInsertion.mParentFrame = prev.GetParent().unwrap().GetContentInsertionFrame();

            if let Some(parent) = aInsertion.mParentFrame {
                if IsAnonymousItem(parent) {
                    // Special-case anonymous flex / grid items: Elements are blockified
                    // inside those containers, so the anonymous item is unlikely to be the
                    // right parent unless we're inserting a non-whitespace text-node.
                    //
                    // If we guess wrong, we catch this in WipeContainingBlock, but with a
                    // performance penalty (and sometimes correctness too, see bug 2014986).
                    if prev.GetNextSibling().is_none() && (!self.IsText(aChild) || self.TextIsOnlyWhitespace(aChild)) {
                        let tail = parent.GetTailContinuation();
                        prevSibling = Some(tail);
                        aInsertion.mParentFrame = tail.GetParent();
                    }
                }
            }

            let parent = aInsertion.mParentFrame.unwrap();
            *aIsAppend = GetInsertNextSibling(parent, prevSibling).is_none() && nsLayoutUtils::GetNextContinuationOrIBSplitSibling(parent).is_none() && !IsWrapperPseudo(parent);
        } else if let Some(nextSibling) = self.FindNextSibling(&iter) {
            // If there is no previous sibling, then find the frame that follows
            aInsertion.mParentFrame = nextSibling.GetParent().unwrap().GetContentInsertionFrame();
            if let Some(parent) = aInsertion.mParentFrame {
                if IsAnonymousItem(parent) {
                    // See the prevSibling special-case above.
                    if nextSibling.GetPrevSibling().is_none() && (!self.IsText(aChild) || self.TextIsOnlyWhitespace(aChild)) {
                        aInsertion.mParentFrame = parent.FirstContinuation().GetParent();
                    }
                }
            }
        } else {
            // No previous or next sibling, so treat this like an appended frame.
            *aIsAppend = true;

            let parent = ContinuationToAppendTo(aInsertion.mParentFrame.unwrap());
            aInsertion.mParentFrame = Some(parent);

            prevSibling = parent.PrincipalChildList().LastChild();
        }

        prevSibling
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::GetContentInsertionFrameFor
    fn GetContentInsertionFrameFor(&self, aContent: ContentId) -> Option<nsIFrame> {
        let mut aContent = aContent;
        let frame;
        loop {
            if let Some(f) = self.GetPrimaryFrame(aContent) {
                frame = f;
                break;
            }
            if !self.IsDisplayContents(aContent) {
                return None;
            }

            aContent = self.GetFlattenedTreeParent(aContent);
            if aContent == NONE {
                return None;
            }
        }

        // If the content of the frame is not the desired content then this is not
        // really a frame for the desired content.
        // XXX This check is needed due to bug 135040. Remove it once that's fixed.
        if self.ContentIdOf(frame) != aContent {
            return None;
        }

        let insertionFrame = frame.GetContentInsertionFrame();

        NS_ASSERTION!(insertionFrame.is_none() || insertionFrame == Some(frame) || !frame.IsLeaf(), "The insertion frame is the primary frame or the primary frame isn't a leaf");

        insertionFrame
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::AddTextItemIfNeeded
    fn AddTextItemIfNeeded(&mut self, aState: &mut nsFrameConstructorState, aParentStyle: &ServoArc<ComputedStyle>, aInsertion: InsertionPoint, aPossibleTextContent: ContentId, aItems: &mut FrameConstructionItemList) {
        MOZ_ASSERT!(aPossibleTextContent != NONE, "Must have node");
        if !self.IsText(aPossibleTextContent) || aPossibleTextContent >= kGeneratedContentBase || !nsIContent::FromNodeId(aPossibleTextContent).HasFlag(NS_CREATE_FRAME_IF_NON_WHITESPACE) || self.mPendingReconstructs.contains(&aPossibleTextContent) {
            // Not text, or not suppressed due to being all-whitespace (if it were being
            // suppressed, it would have the NS_CREATE_FRAME_IF_NON_WHITESPACE flag), or
            // going to be reframed anyway.
            return;
        }
        MOZ_ASSERT!(self.GetPrimaryFrame(aPossibleTextContent).is_none(), "Text node has a frame and NS_CREATE_FRAME_IF_NON_WHITESPACE");
        self.AddFrameConstructionItems(aState, aPossibleTextContent, false, aParentStyle, aInsertion, aItems, ItemFlag::None);
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ReframeTextIfNeeded
    fn ReframeTextIfNeeded(&mut self, aContent: ContentId) {
        if !self.IsText(aContent) || aContent >= kGeneratedContentBase || !nsIContent::FromNodeId(aContent).HasFlag(NS_CREATE_FRAME_IF_NON_WHITESPACE) || self.mPendingReconstructs.contains(&aContent) {
            // Not text, or not suppressed due to being all-whitespace (if it were being
            // suppressed, it would have the NS_CREATE_FRAME_IF_NON_WHITESPACE flag), or
            // going to be reframed anyway.
            return;
        }
        MOZ_ASSERT!(self.GetPrimaryFrame(aContent).is_none(), "Text node has a frame and NS_CREATE_FRAME_IF_NON_WHITESPACE");
        // ContentInserted(aContent, InsertionKind::Async);
        self.ContentRangeInserted(aContent, self.GetNextSibling(aContent), InsertionKind::Async);
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::IssueSingleInsertNofications
    fn IssueSingleInsertNofications(&mut self, aStartChild: ContentId, aEndChild: ContentId, aInsertionKind: InsertionKind) {
        let mut child = aStartChild;
        while child != aEndChild && child != NONE {
            let next = self.GetNextSibling(child);
            // Call ContentRangeInserted with this node.
            self.ContentRangeInserted(child, next, aInsertionKind);
            child = next;
        }
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::GetRangeInsertionPoint
    fn GetRangeInsertionPoint(&mut self, aStartChild: ContentId, aEndChild: ContentId, aInsertionKind: InsertionKind) -> InsertionPoint {
        MOZ_ASSERT!(aStartChild != NONE);
        MOZ_ASSERT!(self.GetParentOf(aStartChild) != NONE);

        // If the children of the container may be distributed to different insertion
        // points, insert them separately and bail out, letting ContentInserted handle
        // the mess.
        let parent = self.GetParentOf(aStartChild);
        if self.sd().tree.dom.shadow_root(parent) != NONE {
            self.IssueSingleInsertNofications(aStartChild, aEndChild, aInsertionKind);
            return InsertionPoint { mParentFrame: None, mContainer: NONE };
        }

        // Now the flattened tree parent of all the siblings is the same, just use the
        // same insertion point.
        self.GetInsertionPoint(aStartChild)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::GetInsertionPoint
    fn GetInsertionPoint(&self, aChild: ContentId) -> InsertionPoint {
        MOZ_ASSERT!(aChild != NONE);
        let insertionElement = self.GetFlattenedTreeParent(aChild);
        if insertionElement == NONE {
            // The element doesn't belong in the flattened tree, and thus we don't want
            // to render it.
            return InsertionPoint { mParentFrame: None, mContainer: NONE };
        }

        InsertionPoint { mParentFrame: self.GetContentInsertionFrameFor(insertionElement), mContainer: insertionElement }
    }

    // ContentRangeInserted handles creating frames for a range of nodes that
    // might not be at the end of their childlist. ContentRangeInserted isn't a real
    // content notification, but rather it handles regular ContentInserted calls
    // for a single node as well as the lazy construction of frames for a range of
    // nodes when called from CreateNeededFrames. For a range of nodes to be
    // suitable to have its frames constructed all at once they must meet the same
    // conditions that ContentAppended imposes (GetRangeInsertionPoint checks
    // these), plus more.
    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ContentRangeInserted
    pub fn ContentRangeInserted(&mut self, aStartChild: ContentId, aEndChild: ContentId, aInsertionKind: InsertionKind) {
        MOZ_ASSERT!(aStartChild != NONE, "must always pass a child");

        // If we have a null parent, then this must be the document element being
        // inserted, or some other child of the document in the DOM (might be a
        // processing instruction or comment).
        if self.GetParentOf(aStartChild) == NONE {
            let docElement = self.sd().tree.dom.children(self.sd().tree.doc).find(|&c| self.IsElement(c)).unwrap_or(NONE);
            let foundRoot = {
                let mut cur = aStartChild;
                let mut found = false;
                while cur != aEndChild && cur != NONE {
                    if cur == docElement {
                        found = true;
                        break;
                    }
                    cur = self.GetNextSibling(cur);
                }
                found
            };

            if !foundRoot {
                // Not the root element (could be e.g. a comment), just bail out
                return;
            }

            MOZ_ASSERT!(self.mRootElementFrame.is_none(), "root element frame already created");
            if aInsertionKind == InsertionKind::Async {
                // docElement->SetFlags(NODE_NEEDS_FRAME); LazilyStyleNewChildRange(docElement, nullptr);
                self.mPendingReconstructs.push(docElement);
                return;
            }

            // Create frames for the document element and its child elements
            self.mDynamicChangeCounts.mInsertions += 1;
            self.ConstructDocElementFrame(docElement);
            return;
        }

        let isSingleInsert = self.GetNextSibling(aStartChild) == aEndChild;
        let mut insertion = if isSingleInsert {
            // See if we have a Shadow DOM insertion point. If so, then that's our real
            // parent frame; if not, then the frame hasn't been built yet and we just
            // bail.
            self.GetInsertionPoint(aStartChild)
        } else {
            // Get our insertion point. If we need to issue single ContentInserteds
            // GetRangeInsertionPoint will take care of that for us.
            self.GetRangeInsertionPoint(aStartChild, aEndChild, aInsertionKind)
        };

        if insertion.mParentFrame.is_none() {
            // We're punting on frame construction because there's no container frame.
            // The Servo-backed style system handles this case like the lazy frame
            // construction case, except when we're already constructing frames, in
            // which case we shouldn't need to do anything else.
            return;
        }

        if aInsertionKind == InsertionKind::Async {
            // ConstructLazily(aStartChild, aEndChild); LazilyStyleNewChildRange(aStartChild, aEndChild);
            let mut child = aStartChild;
            while child != aEndChild && child != NONE {
                self.mPendingReconstructs.push(child);
                child = self.GetNextSibling(child);
            }
            return;
        }

        let mut isAppend = false;
        let mut prevSibling = self.GetInsertionPrevSibling(&mut insertion, aStartChild, &mut isAppend);

        let parentFrame = insertion.mParentFrame.unwrap();
        let frameType = parentFrame.Type();
        // MaybeRecreateForFrameset: NOT PORTED, framesets.

        // Don't construct kids of leaves
        if parentFrame.IsLeaf() {
            // Clear lazy bits so we don't try to construct again.
            // ClearLazyBits(aStartChild, aEndChild); NOT PORTED: lazy bits.
            return;
        }

        if self.WipeInsertionParent(parentFrame) {
            return;
        }

        let fixedCB = self.GetAbsoluteContainingBlock(parentFrame, ContainingBlockType::FIXED_POS);
        let absCB = self.GetAbsoluteContainingBlock(parentFrame, ContainingBlockType::ABS_POS);
        let floatCB = nsCSSFrameConstructor_GetFloatContainingBlock(Some(parentFrame));
        let mut state = nsFrameConstructorState::new(fixedCB, absCB, floatCB, self.mRootFrame);

        // Recover state for the containing block - we need to know if
        // it has :first-letter or :first-line style applied to it. The
        // reason we care is that the internal structure in these cases
        // is not the normal structure and requires custom updating
        // logic.
        let containingBlock = state.mFloatedList.mContainingBlock;
        // (haveFirstLetterStyle, haveFirstLineStyle: NOT PORTED, ::first-letter and ::first-line)
        let haveFirstLetterStyle = false;
        let haveFirstLineStyle = false;

        // (the list-style-image fallback: NOT PORTED, there are no images)

        let mut items = FrameConstructionItemList::default();
        let parentStyle = match self.ResolveComputedStyle(insertion.mContainer) {
            Some(s) => s,
            None => return,
        };
        let parentType = nsCSSFrameConstructor::GetParentType(parentFrame);
        let mut iter = self.FlatChildIterator(insertion.mContainer, true);
        let haveNoShadowDOM = !iter.ShadowDOMInvolved() || iter.GetNextChild() == NONE;
        let startPrev = self.GetPreviousSibling(aStartChild);
        if startPrev != NONE && parentType == eTypeBlock && haveNoShadowDOM {
            // If there's a text node in the normal content list just before the
            // new nodes, and it has no frame, make a frame construction item for
            // it, because it might need a frame now.  No need to do this if our
            // parent type is not block, though, since WipeContainingBlock
            // already handles that situation.
            self.AddTextItemIfNeeded(&mut state, &parentStyle, insertion, startPrev, &mut items);
        }

        // (suppressWhiteSpaceOptimizations: only for the roots of native anonymous subtrees, which
        // there are none of)
        let suppressWhiteSpaceOptimizations = false;
        let mut child = aStartChild;
        while child != aEndChild && child != NONE {
            self.AddFrameConstructionItems(&mut state, child, suppressWhiteSpaceOptimizations, &parentStyle, insertion, &mut items, ItemFlag::None);
            child = self.GetNextSibling(child);
        }

        if aEndChild != NONE && parentType == eTypeBlock && haveNoShadowDOM {
            // If there's a text node in the normal content list just after the
            // new nodes, and it has no frame, make a frame construction item for
            // it, because it might need a frame now.  No need to do this if our
            // parent type is not block, though, since WipeContainingBlock
            // already handles that situation.
            self.AddTextItemIfNeeded(&mut state, &parentStyle, insertion, aEndChild, &mut items);
        }

        // Perform special check for diddling around with the frames in a special
        // inline frame. If we're appending before :after content, then we're not
        // really appending, so let WipeContainingBlock know that.
        if self.WipeContainingBlock(&mut state, containingBlock, parentFrame, &mut items, isAppend, prevSibling) {
            state.ProcessFrameInsertionsForAllLists(); // ~nsFrameConstructorState
            return;
        }

        // If the parent is a block frame, and we're not in a special case
        // where frames can be moved around, determine if the list is for the
        // start or end of the block.
        if parentFrame.IsBlockFrameOrSubclass() && !haveFirstLetterStyle && !haveFirstLineStyle && !IsFramePartOfIBSplit(parentFrame) {
            items.SetLineBoundaryAtStart(prevSibling.map_or(true, |p| !p.IsInlineOutside() || p.IsBrFrame()));
            let nextSibling = GetInsertNextSibling(parentFrame, prevSibling);
            items.SetLineBoundaryAtEnd(nextSibling.map_or(true, |n| !n.IsInlineOutside() || n.IsBrFrame()));
        }
        // To suppress whitespace-only text frames, we have to verify that
        // our container's DOM child list matches its flattened tree child list.
        items.SetParentHasNoShadowDOM(haveNoShadowDOM);

        let mut floatSaveState = nsFrameConstructorSaveState::default();
        state.MaybePushFloatContainingBlock(parentFrame, &mut floatSaveState);

        // (IsPaginated: NOT PORTED, pagination)

        // If the container is a table and a caption will be appended, it needs to be
        // put in the table wrapper frame's additional child list.
        // We make no attempt here to set flags to indicate whether the list
        // will be at the start or end of a block. It doesn't seem worthwhile.
        let mut frameList = nsFrameList::default();
        let mut captionList = nsFrameList::default();
        self.ConstructFramesFromItemList(&mut state, &mut items, parentFrame, ParentIsWrapperAnonBox(parentFrame), &mut frameList);

        if frameList.NotEmpty() {
            // InvalidateCanvasIfNeeded: NOT PORTED, painting.
            if LayoutFrameType::Table == frameType || LayoutFrameType::TableWrapper == frameType {
                PullOutCaptionFrames(&mut frameList, &mut captionList);
                if prevSibling.is_some_and(|p| p.IsTableCaption()) {
                    // This can happen, but only if the table is empty (otherwise
                    // SafeToInsertPseudoNeedingChildren bails).
                    prevSibling = None;
                }
            }
        }

        // (AppendFirstLineFrames, CheckForFirstLineInsertion: NOT PORTED, ::first-line)

        // We might have captions; put them into the principal child list of the table
        // wrapper frame.
        if captionList.NotEmpty() {
            NS_ASSERTION!(LayoutFrameType::Table == frameType || LayoutFrameType::TableWrapper == frameType, "parent for caption is not table?");
            // We need to determine where to put the caption items; start with the
            // the parent frame that has already been determined and get the insertion
            // prevsibling of the first caption item.
            let outerTable = if parentFrame.IsTableFrame() { parentFrame.GetParent().unwrap() } else { parentFrame };

            // If the parent is not a table wrapper frame we will try to add frames
            // to a named child list that the parent does not honor and the frames
            // will get lost.
            MOZ_ASSERT!(outerTable.IsTableWrapperFrame(), "Pseudo frame construction failure; a caption can be only a child of a table wrapper frame");

            let captionContent = self.ContentIdOf(captionList.FirstChild().unwrap());
            let captionPrevSibling = self.FindCaptionPrevSibling(outerTable, captionContent);
            captionList.ApplySetParent(outerTable);
            if captionPrevSibling.GetNextSibling().is_none() {
                nsFrameManager::AppendFrames(outerTable, FrameChildListID::Principal, captionList);
            } else {
                nsFrameManager::InsertFrames(outerTable, FrameChildListID::Principal, Some(captionPrevSibling), captionList);
            }
        }

        // MaybeRecreateForColumnSpan: NOT PORTED, columns.

        if frameList.NotEmpty() {
            self.mDynamicChangeCounts.mInsertions += 1;
            // Notify the parent frame
            if isAppend {
                self.AppendFramesToParent(&mut state, parentFrame, &mut frameList, prevSibling, false);
            } else {
                nsFrameManager::InsertFrames(parentFrame, FrameChildListID::Principal, prevSibling, frameList);
            }
        }

        // (RecoverLetterFrames: NOT PORTED, ::first-letter)

        floatSaveState.Restore(&mut state); // ~nsFrameConstructorSaveState
        state.ProcessFrameInsertionsForAllLists(); // ~nsFrameConstructorState
    }

    // It's very likely that we don't have any existing captions (because we only
    // render one of them, see bug 144517). We also don't fragment table captions.
    // So, prefer just walking the existing captions and searching it, rather than
    // adding a special GetInsertionPrevSibling() version that skips everything but
    // table captions.
    // Gecko: layout/base/nsCSSFrameConstructor.cpp FindCaptionPrevSibling
    fn FindCaptionPrevSibling(&self, aTable: nsIFrame, aCaptionContent: ContentId) -> nsIFrame {
        let mut prevSibling = nsTableWrapperFrame::from_frame(aTable).InnerTableFrame().0;
        let mut caption = prevSibling.GetNextSibling();
        while let Some(c) = caption {
            // nsContentUtils::CompareTreePosition<TreeKind::Flat>(caption->GetContent(), aCaptionContent) >= 0
            if self.CompareFlatTreePosition(self.ContentIdOf(c), aCaptionContent) >= 0 {
                break;
            }
            prevSibling = c;
            caption = c.GetNextSibling();
        }
        prevSibling
    }

    // ADAPTER: nsContentUtils::CompareTreePosition<TreeKind::Flat> for two nodes of the arena: -1, 0
    // or 1 as aNode1 is before, the same as or after aNode2 in the flattened tree.
    fn CompareFlatTreePosition(&self, aNode1: ContentId, aNode2: ContentId) -> i32 {
        if aNode1 == aNode2 {
            return 0;
        }
        let chain = |mut n: ContentId| {
            let mut v = vec![n];
            loop {
                let p = self.GetFlattenedTreeParent(n);
                if p == NONE {
                    break;
                }
                v.push(p);
                n = p;
            }
            v.reverse();
            v
        };
        let (a, b) = (chain(aNode1), chain(aNode2));
        let mut i = 0;
        while i < a.len() && i < b.len() && a[i] == b[i] {
            i += 1;
        }
        if i == a.len() {
            return -1;
        }
        if i == b.len() {
            return 1;
        }
        if i == 0 {
            return 0;
        }
        let parent = a[i - 1];
        let mut iter = self.FlatChildIterator(parent, true);
        loop {
            let c = iter.GetNextChild();
            if c == NONE || c == a[i] {
                return -1;
            }
            if c == b[i] {
                return 1;
            }
        }
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ContentWillBeRemoved
    // ADAPTER: for a DOM removal (RemovalKind::Dom), the caller sets mRemoval to what the DOM was at
    // the removal (RemovedContent; see ContentRemoved).
    pub fn ContentWillBeRemoved(&mut self, aChild: ContentId, aKind: RemovalKind) -> bool {
        MOZ_ASSERT!(aChild != NONE);

        let wasRemovingContent = self.mRemovingContent;
        self.mRemovingContent = true;
        let result = self.ContentWillBeRemovedInternal(aChild, aKind);
        self.mRemovingContent = wasRemovingContent;
        result
    }

    fn ContentWillBeRemovedInternal(&mut self, aChild: ContentId, aKind: RemovalKind) -> bool {
        // We want to detect when the viewport override element stored in the
        // prescontext is in the subtree being removed.
        // (UpdateViewportScrollStylesOverride: NOT PORTED, there are no scroll frames)

        let mut childFrame = self.GetPrimaryFrame(aChild);
        if childFrame.is_some_and(|f| self.ContentIdOf(f) != aChild) {
            // XXXbz the GetContent() != aChild check is needed due to bug 135040.
            // Remove it once that's fixed.
            childFrame = None;
        }

        // If we're removing the root, then make sure to remove things starting at
        // the viewport's child instead of the primary frame (which might even be
        // null if the root was display:none, even though the frames above it got
        // created).  Detecting removal of a root is a little exciting; in particular,
        // having no parent is necessary but NOT sufficient.
        //
        // Due to how we process reframes, the content node might not even be in our
        // document by now.  So explicitly check whether the viewport's first kid's
        // content node is aChild.
        // ADAPTER: the canvas frame has no content here (see SetUpDocElementContainingBlock); the
        // root is removed when aChild's frame is the root element frame.
        let mut isRoot = false;
        if self.GetParentOf(aChild) == NONE && aChild < kGeneratedContentBase {
            if let Some(viewport) = self.GetRootFrame() {
                let firstChild = viewport.PrincipalChildList().FirstChild();
                if firstChild.is_some() && self.mRootElementFrame.is_some() && childFrame == self.mRootElementFrame {
                    isRoot = true;
                    childFrame = firstChild;
                    NS_ASSERTION!(childFrame.unwrap().GetNextSibling().is_none(), "How did that happen?");
                }
            }
        }

        // We need to be conservative about when to determine whether something has
        // display: contents or not because at this point our actual display may be
        // different.
        //
        // Consider the case of:
        //
        //   <div id="A" style="display: contents"><div id="B"></div></div>
        //
        // If we reconstruct A because its display changed to "none", we still need to
        // cleanup the frame on B, but A's display is now "none", so we can't poke at
        // the style of it.
        //
        // FIXME(emilio, bug 1450366): We can make this faster without adding much
        // complexity for the display: none -> other case, which right now
        // unnecessarily walks the content tree down.
        let CouldHaveBeenDisplayContents = |this: &Self, aContent: ContentId| -> bool { this.IsElement(aContent) && (aKind != RemovalKind::Dom || this.IsDisplayContents(aContent)) };

        let Some(mut childFrame) = childFrame else {
            if CouldHaveBeenDisplayContents(self, aChild) {
                // NOTE(emilio): We may iterate through ::before and ::after here and they
                // may be gone after the respective ContentWillBeRemoved call. Right now
                // StyleChildrenIterator handles that properly, so it's not an issue.
                // ADAPTER: the children are those of the DOM now (a removal of one of them since
                // came as its own removal, and has destroyed its frames).
                let removal = self.mRemoval.take();
                for c in self.StyleChildren(aChild) {
                    if self.GetPrimaryFrame(c).is_some() || CouldHaveBeenDisplayContents(self, c) {
                        let didReconstruct = self.ContentWillBeRemoved(c, aKind);
                        if didReconstruct {
                            self.mRemoval = removal;
                            return true;
                        }
                    }
                }
                self.mRemoval = removal;
            }
            return false;
        };

        // CaptureStateForFramesOf, InvalidateCanvasIfNeeded: NOT PORTED (frame state, painting).

        // See whether we need to remove more than just childFrame
        if self.MaybeRecreateContainerForFrameRemoval(childFrame) {
            return true;
        }

        // Get the childFrame's parent frame
        let mut parentFrame = childFrame.GetParent().unwrap();
        // (the frameset and MathML parents: NOT PORTED)

        // Examine the containing-block for the removed content and see if
        // :first-letter style applies.
        // (haveFLS: NOT PORTED, ::first-letter)

        // Notify the parent frame that it should delete the frame
        if childFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) {
            childFrame = childFrame.GetPlaceholderFrame().expect("Missing placeholder frame for out of flow.");
            parentFrame = childFrame.GetParent().unwrap();
        }

        // Take care of wrapper anonymous boxes that we might need to remove while
        // at it. Note that MaybeRecreateContainerForFrameRemoval takes care of
        // harder cases (merging sibling anonymous boxes etc).
        while IsWrapperPseudo(parentFrame) && CanRemoveWrapperPseudoForChildRemoval(childFrame, parentFrame) {
            childFrame = parentFrame;
            parentFrame = childFrame.GetParent().unwrap();
        }

        let canSkipWhitespaceFixup = (|| {
            if aKind == RemovalKind::ForReconstruction {
                // If we're just reconstructing frames for the element, then the
                // following ContentInserted notification on the element will
                // take care of fixing up any adjacent white-space text nodes.
                return true;
            }
            match parentFrame.Type() {
                LayoutFrameType::Table | LayoutFrameType::TableRow | LayoutFrameType::TableRowGroup | LayoutFrameType::TableCol | LayoutFrameType::TableColGroup | LayoutFrameType::TableWrapper => {
                    // Tables ignore all whitespace within their wrappers, so we can avoid
                    // reconstructing adjacent whitespace if the table is not anonymous.
                    if !IsInAnonymousTable(parentFrame) {
                        return true;
                    }
                    // If the table is anonymous and the child is at the edges, we might
                    // need to create whitespace at the edges of the table that wasn't
                    // there before, so we can only skip the check if childFrame has frames
                    // around. Captions are extra-special here, because they're siblings
                    // with the main table frame, so we can't count that as a relevant
                    // sibling for our purposes.
                    let prevSibling = childFrame.GetPrevSibling();
                    prevSibling.is_some_and(|p| !p.IsTableFrame()) && childFrame.GetNextSibling().is_some()
                }
                LayoutFrameType::GridContainer | LayoutFrameType::FlexContainer => {
                    // Flex and grid containers similarly skip whitespace and wrap
                    // non-whitespace in anonymous flex items, so any change to
                    // white-space that could matter would have triggered the
                    // reconstruction of the container itself due to merging anonymous
                    // grid / flex items.
                    true
                }
                _ => false,
            }
        })();
        let mut context = FrameDestroyContext::default();
        nsFrameManager::RemoveFrame(&mut context, nsLayoutUtils::GetChildListNameFor(childFrame), childFrame);
        self.mDynamicChangeCounts.mRemovals += 1;

        // NOTE(emilio): aChild could be dead here already if it is a ::before or
        // ::after pseudo-element (since in that case it was owned by childFrame,
        // which we just destroyed).

        if isRoot {
            self.mRootElementFrame = None;
            self.mRootElementStyleFrame = None;
            self.mDocElementContainingBlock = None;
            self.mCanvasFrame = None;
        }

        if !canSkipWhitespaceFixup {
            // Adjacent whitespace-only text nodes might have been suppressed if
            // this node does not have inline ends. Create frames for them now
            // if necessary.
            // Reframe any text node just before the node being removed, if there is
            // one, and if it's not the last child or the first child. If a whitespace
            // textframe was being suppressed and it's now the last child or first
            // child then it can stay suppressed since the parent must be a block
            // and hence it's adjacent to a block end.
            // If aOldNextSibling is null, then the text node before the node being
            // removed is the last node, and we don't need to worry about it.
            // ADAPTER: for a DOM removal, the siblings it had (RemovedContent).
            let (prevSibling, prevHasPrev, nextSibling, nextHasNext) = match self.mRemoval {
                Some(r) if r.mChild == aChild => (r.mPrevSibling, r.mPrevSiblingHasPrevSibling, r.mNextSibling, r.mNextSiblingHasNextSibling),
                _ => {
                    let prev = self.GetPreviousSibling(aChild);
                    let next = self.GetNextSibling(aChild);
                    (prev, prev != NONE && self.GetPreviousSibling(prev) != NONE, next, next != NONE && self.GetNextSibling(next) != NONE)
                }
            };
            if prevSibling != NONE && prevHasPrev {
                self.ReframeTextIfNeeded(prevSibling);
            }
            // Reframe any text node just after the node being removed, if there is
            // one, and if it's not the last child or the first child.
            if nextSibling != NONE && prevSibling != NONE && nextHasNext {
                self.ReframeTextIfNeeded(nextSibling);
            }
        }

        false
    }

    // ADAPTER: a DOM removal, applied on the layout thread with the DOM as it was then (RemovedContent):
    // Gecko's PresShell::ContentWillBeRemoved, which calls ContentWillBeRemoved(aChild, RemovalKind::Dom).
    pub fn ContentRemoved(&mut self, aRemoved: RemovedContent) {
        self.mRemoval = Some(aRemoved);
        self.ContentWillBeRemoved(aRemoved.mChild, RemovalKind::Dom);
        self.mRemoval = None;
        self.ReleaseDestroyedFrames();
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CharacterDataChanged
    // ADAPTER: the text frame's own CharacterDataChanged (nsTextFrame::CharacterDataChanged, which
    // marks the frames dirty and clears their text runs) is NOT PORTED; the text node's frames are
    // recreated instead, which gives the same frames.
    pub fn CharacterDataChanged(&mut self, aContent: ContentId) {
        let content = nsIContent::FromNodeId(aContent);
        content.RemoveFlowLengthProperty();
        if (content.HasFlag(NS_CREATE_FRAME_IF_NON_WHITESPACE) && !self.TextIsOnlyWhitespace(aContent)) || (content.HasFlag(NS_REFRAME_IF_WHITESPACE) && self.TextIsOnlyWhitespace(aContent)) {
            self.RecreateFramesForContent(aContent, InsertionKind::Async);
            return;
        }

        // It's possible the frame whose content changed isn't inserted into the
        // frame hierarchy yet, or that there is no frame that maps the content
        if self.GetPrimaryFrame(aContent).is_some() {
            // (NS_FRAME_IS_IN_SINGLE_CHAR_MI: NOT PORTED, MathML; the first-letter case: NOT PORTED)
            // Notify the first frame that maps the content. It will generate a reflow
            // command
            // frame->CharacterDataChanged(aInfo);  ADAPTER: see above.
            self.RecreateFramesForContent(aContent, InsertionKind::Sync);
        }
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::MaybeRecreateContainerForFrameRemoval
    fn MaybeRecreateContainerForFrameRemoval(&mut self, aFrame: nsIFrame) -> bool {
        MOZ_ASSERT!(aFrame.GetParent().is_some(), "Frame shouldn't be root");
        MOZ_ASSERT!(aFrame == aFrame.FirstContinuation(), "aFrame not the result of GetPrimaryFrame()?");

        let inFlowFrame = if aFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) { aFrame.GetPlaceholderFrame().unwrap() } else { aFrame };
        MOZ_ASSERT!(inFlowFrame == inFlowFrame.FirstContinuation(), "placeholder for primary frame has previous continuations?");
        let parent = inFlowFrame.GetParent().unwrap();

        // (NS_FRAME_HAS_MULTI_COLUMN_ANCESTOR: NOT PORTED, columns)

        if IsFramePartOfIBSplit(aFrame) {
            // The removal functions can't handle removal of an {ib} split directly; we
            // need to rebuild the containing block.
            self.mDynamicChangeCounts.mContainerReframes += 1;
            self.ReframeContainingBlock(aFrame);
            return true;
        }

        // (inFlowFrame->IsRenderedLegend(): NOT PORTED, fieldsets)

        // Might need to reconstruct things if this frame's nextSibling is a table
        // or ruby pseudo, since removal of this frame might mean that this pseudo
        // needs to get merged with the frame's prevSibling if that's also a table
        // or ruby pseudo.
        let nextSibling = FindNextNonWhitespaceSibling(inFlowFrame.LastContinuation());
        NS_ASSERTION!(!IsWrapperPseudo(inFlowFrame), "Shouldn't happen here (we start removals from primary frames)");
        // Effectively, for the ruby pseudo sibling case, only pseudo <ruby> frame
        // need to be checked here, since all other types of such frames will have
        // a ruby container parent, and be catched by "Check ruby containers" below.
        if nextSibling.is_some_and(IsWrapperPseudo) {
            let prevSibling = FindPreviousNonWhitespaceSibling(inFlowFrame);
            if prevSibling.is_some_and(IsWrapperPseudo) {
                // Good enough to recreate frames for aFrame's parent's content; even if
                // aFrame's parent is a pseudo, that'll be the right content node.
                // FIXME(emilio): Consider doing a more subtle check here like, only if
                // prevSibling and nextSibling share frame type. Or even consider just
                // moving the frames around and destroying nextSibling?
                self.mDynamicChangeCounts.mContainerReframes += 1;
                self.RecreateFramesForContent(self.ContentIdOf(parent), InsertionKind::Async);
                return true;
            }
        }

        // Check ruby containers: NOT PORTED, ruby.

        // Reconstruct if inflowFrame is parent's only child, and parent is, or has,
        // a non-fluid continuation, i.e. it was split by bidi resolution
        if inFlowFrame.GetPrevSibling().is_none()
            && inFlowFrame.GetNextSibling().is_none()
            && ((parent.GetPrevContinuation().is_some() && parent.GetPrevInFlow().is_none()) || (parent.GetNextContinuation().is_some() && parent.GetNextInFlow().is_none()))
        {
            self.mDynamicChangeCounts.mContainerReframes += 1;
            self.RecreateFramesForContent(self.ContentIdOf(parent), InsertionKind::Async);
            return true;
        }

        // We might still need to reconstruct things if the parent of inFlowFrame is
        // ib-split, since in that case the removal of aFrame might affect the
        // splitting of its parent.
        if !IsFramePartOfIBSplit(parent) {
            return false;
        }

        // If inFlowFrame is not the only in-flow child of |parent|, then removing
        // it will change nothing about the {ib} split.
        if Some(inFlowFrame) != parent.PrincipalChildList().FirstChild() || inFlowFrame.LastContinuation().GetNextSibling().is_some() {
            return false;
        }

        // If the parent is the first or last part of the {ib} split, then
        // removing one of its kids will have no effect on the splitting.
        // Get the first continuation up front so we don't have to do it twice.
        let parentFirstContinuation = parent.FirstContinuation();
        if GetIBSplitSibling(parentFirstContinuation).is_none() || GetIBSplitPrevSibling(parentFirstContinuation).is_none() {
            return false;
        }

        self.mDynamicChangeCounts.mContainerReframes += 1;
        self.ReframeContainingBlock(parent);
        true
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::RecreateFramesForContent
    pub fn RecreateFramesForContent(&mut self, aContent: ContentId, aInsertionKind: InsertionKind) {
        MOZ_ASSERT!(aContent != NONE);

        // If there is no document, we don't want to recreate frames for it.  (You
        // shouldn't generally be giving this method content without a document
        // anyway).
        // Rebuilding the frame tree can have bad effects, especially if it's the
        // frame tree for chrome (see bug 157322).
        // ADAPTER: generated content is reframed with the element it belongs to.
        let mut aContent = aContent;
        if aContent >= kGeneratedContentBase {
            aContent = self.mGenerated[(aContent - kGeneratedContentBase) as usize].mParent;
            return self.RecreateFramesForContent(aContent, InsertionKind::Async);
        }
        if !self.IsInComposedDoc(aContent) {
            return;
        }

        // (ShouldRecreateContainerForNativeAnonymousContentRoot, the MathML case: NOT PORTED)

        let frame = self.GetPrimaryFrame(aContent);

        if let Some(frame) = frame {
            let parent = frame.GetParent();
            let parentContent = parent.map_or(NONE, |p| self.ContentIdOf(p));
            // If the parent frame is a leaf then the subsequent insert will fail to
            // create a frame, so we need to recreate the parent content. This happens
            // with native anonymous content from the editor.
            if parent.is_some_and(|p| p.IsLeaf()) && parentContent != NONE && parentContent != aContent {
                return self.RecreateFramesForContent(parentContent, InsertionKind::Async);
            }
        }

        if let Some(frame) = frame {
            if self.MaybeRecreateContainerForFrameRemoval(frame) {
                return;
            }
        }

        MOZ_ASSERT!(self.GetParentOf(aContent) != NONE || self.sd().tree.dom.node(aContent).parent != NONE);
        let removalKind = if aInsertionKind == InsertionKind::Sync && self.IsElement(aContent) && self.sd().computed(aContent).map_or(true, |s| nsStyleDisplay::new(s).mDisplay() == StyleDisplayValue::None) {
            // If we know we're not going to have frames after reconstructing, it's
            // more efficient to do some of that work (a11y notifications, fixing-up
            // text nodes) earlier.
            RemovalKind::ForDisplayNoneChange
        } else {
            RemovalKind::ForReconstruction
        };
        let didReconstruct = self.ContentWillBeRemoved(aContent, removalKind);
        if didReconstruct || removalKind == RemovalKind::ForDisplayNoneChange {
            // If ContentWillBeRemoved triggered reconstruction, then we don't need to
            // do anything else because the frames will already have been built.
            return;
        }
        self.mDynamicChangeCounts.mReconstructs += 1;
        if aInsertionKind == InsertionKind::Async && self.IsElement(aContent) {
            // RestyleManager()->PostRestyleEvent(aContent->AsElement(), RestyleHint{0},
            //                                    nsChangeHint_ReconstructFrame);
            self.mPendingReconstructs.push(aContent);
            return;
        }
        // Now, recreate the frames associated with this content object.
        let next = self.GetNextSibling(aContent);
        self.ContentRangeInserted(aContent, next, aInsertionKind);
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::WipeInsertionParent
    fn WipeInsertionParent(&mut self, aFrame: nsIFrame) -> bool {
        let _ = aFrame;
        // (MathML, ruby and the multi-column container: NOT PORTED, none of those frames exist)
        false
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::WipeContainingBlock
    fn WipeContainingBlock(&mut self, aState: &mut nsFrameConstructorState, aContainingBlock: Option<nsIFrame>, aFrame: nsIFrame, aItems: &mut FrameConstructionItemList, aIsAppend: bool, aPrevSibling: Option<nsIFrame>) -> bool {
        if aItems.IsEmpty() {
            return false;
        }

        // Before we go and append the frames, we must check for several
        // special situations.

        // Situation #1 is when we insert content that becomes the canonical body
        // element, and its used WritingMode is different from the root element's
        // used WritingMode.
        // NOT PORTED: the principal writing mode is the root's own (see ConstructDocElementFrame).

        let mut nextSibling = GetInsertNextSibling(aFrame, aPrevSibling);

        // Situation #2 is a flex / grid  container frame into which we're inserting
        // new inline non-replaced children, adjacent to an existing anonymous flex or
        // grid item.
        if aFrame.IsFlexOrGridContainer() {
            // Check if we're adding to-be-wrapped content right *after* an existing
            // anonymous flex or grid item (which would need to absorb this content).
            let isLegacyWebKitBox = false;
            if aPrevSibling.is_some_and(IsAnonymousItem) && aItems.mItems[0].NeedsAnonFlexOrGridItem(isLegacyWebKitBox) {
                self.mDynamicChangeCounts.mContainerReframes += 1;
                self.RecreateFramesForContent(self.ContentIdOf(aFrame), InsertionKind::Async);
                return true;
            }

            // Check if we're adding to-be-wrapped content right *before* an existing
            // anonymous flex or grid item (which would need to absorb this content).
            if nextSibling.is_some_and(IsAnonymousItem) {
                // Jump to the last entry in the list
                if aItems.mItems.last().unwrap().NeedsAnonFlexOrGridItem(isLegacyWebKitBox) {
                    self.mDynamicChangeCounts.mContainerReframes += 1;
                    self.RecreateFramesForContent(self.ContentIdOf(aFrame), InsertionKind::Async);
                    return true;
                }
            }
        }

        // Situation #3 is an anonymous flex or grid item that's getting new children
        // who don't want to be wrapped.
        if IsAnonymousItem(aFrame) {
            // We need to push a null float containing block to be sure that
            // "NeedsAnonFlexOrGridItem" will know we're not honoring floats for this
            // inserted content. (In particular, this is necessary in order for
            // its "GetGeometricParent" call to return the correct result.)
            // We're not honoring floats on this content because it has the
            // _flex/grid container_ as its parent in the content tree.
            let mut floatSaveState = nsFrameConstructorSaveState::default();
            aState.PushFloatContainingBlock(None, &mut floatSaveState);

            let mut iter: FCItemIterator = 0;
            // Skip over things that _do_ need an anonymous flex item, because
            // they're perfectly happy to go here -- they won't cause a reframe.
            let containerFrame = aFrame.GetParent().unwrap();
            let isLegacyWebKitBox = false;
            let allNeedAnon = self.SkipItemsThatNeedAnonFlexOrGridItem(aItems, &mut iter, isLegacyWebKitBox);
            floatSaveState.Restore(aState); // ~nsFrameConstructorSaveState
            if !allNeedAnon {
                // We hit something that _doesn't_ need an anonymous flex item!
                // Rebuild the flex container to bust it out.
                self.mDynamicChangeCounts.mContainerReframes += 1;
                self.RecreateFramesForContent(self.ContentIdOf(containerFrame), InsertionKind::Async);
                return true;
            }

            // If we get here, then everything in |aItems| needs to be wrapped in
            // an anonymous flex or grid item.  That's where it's already going - good!
        }

        // Situation #4 is a case when table pseudo-frames don't work out right
        let parentType = nsCSSFrameConstructor::GetParentType(aFrame);
        // If all the kids want a parent of the type that aFrame is, then we're all
        // set to go.  Indeed, there won't be any table pseudo-frames created between
        // aFrame and the kids, so those won't need to be merged with any table
        // pseudo-frames that might already be kids of aFrame.  If aFrame itself is a
        // table pseudo-frame, then all the kids in this list would have wanted a
        // frame of that type wrapping them anyway, so putting them inside it is ok.
        if !aItems.AllWantParentType(parentType) {
            // Don't give up yet.  If parentType is not eTypeBlock and the parent is
            // not a generated content frame, then try filtering whitespace out of the
            // list.
            if parentType != eTypeBlock && !aFrame.IsGeneratedContentFrame() {
                // (the long comment of Gecko on leading and trailing whitespace: see
                // nsCSSFrameConstructor.cpp; the four cases below follow it)
                let mut iter: FCItemIterator = 0;
                let start = iter;
                loop {
                    if aItems.SkipItemsWantingParentType(&mut iter, parentType) {
                        break;
                    }

                    // iter points to an item that wants a different parent.  If it's not
                    // whitespace, we're done; no more point scanning the list.
                    if !self.ItemIsWhitespace(&aItems.mItems[iter]) {
                        break;
                    }

                    if iter == start {
                        // Leading whitespace.  How to handle this depends on our
                        // previous sibling and aFrame.  See the long comment above.
                        let mut prevSibling = aPrevSibling;
                        if prevSibling.is_none() {
                            // Try to find one after all
                            let mut parentPrevCont = aFrame.GetPrevContinuation();
                            while let Some(p) = parentPrevCont {
                                prevSibling = p.PrincipalChildList().LastChild();
                                if prevSibling.is_some() {
                                    break;
                                }
                                parentPrevCont = p.GetPrevContinuation();
                            }
                        }
                        if let Some(p) = prevSibling {
                            if IsTablePseudo(p) {
                                // need to reframe
                                break;
                            }
                        } else if IsTablePseudo(aFrame) {
                            // need to reframe
                            break;
                        }
                    }

                    let mut spaceEndIter = iter;
                    // Advance spaceEndIter past any whitespace
                    let trailingSpaces = self.SkipWhitespace(aItems, &mut spaceEndIter);

                    let okToDrop;
                    if trailingSpaces {
                        // Trailing whitespace.  How to handle this depeds on aIsAppend, our
                        // next sibling and aFrame.  See the long comment above.
                        let mut ok = aIsAppend && nextSibling.is_none();
                        if !ok {
                            if nextSibling.is_none() {
                                // Try to find one after all
                                let mut parentNextCont = aFrame.GetNextContinuation();
                                while let Some(p) = parentNextCont {
                                    nextSibling = p.PrincipalChildList().FirstChild();
                                    if nextSibling.is_some() {
                                        break;
                                    }
                                    parentNextCont = p.GetNextContinuation();
                                }
                            }

                            ok = nextSibling.is_some_and(|n| !IsTablePseudo(n)) || (nextSibling.is_none() && !IsTablePseudo(aFrame));
                        } else {
                            NS_ASSERTION!(!IsTablePseudo(aFrame), "How did that happen?");
                        }
                        okToDrop = ok;
                    } else {
                        okToDrop = aItems.mItems[spaceEndIter].DesiredParentType() == parentType;
                    }

                    if okToDrop {
                        aItems.DeleteItemsTo(&mut iter, spaceEndIter);
                    } else {
                        // We're done: we don't want to drop the whitespace, and it has the
                        // wrong parent type.
                        break;
                    }

                    // Now loop, since |iter| points to item right after the whitespace we
                    // removed.
                    if aItems.IsDone(iter) {
                        break;
                    }
                }
            }

            // We might be able to figure out some sort of optimizations here, but they
            // would have to depend on having a correct aPrevSibling and a correct next
            // sibling.  For example, we can probably avoid reframing if none of
            // aFrame, aPrevSibling, and next sibling are table pseudo-frames.  But it
            // doesn't seem worth it to worry about that for now, especially since we
            // in fact do not have a reliable aPrevSibling, nor any next sibling, in
            // this method.

            // aItems might have changed, so recheck the parent type thing.  In fact,
            // it might be empty, so recheck that too.
            if aItems.IsEmpty() {
                return false;
            }

            // If aFrame is empty, the insertion process will be able to take care of
            // creating any needed pseudo-parents.
            if !aItems.AllWantParentType(parentType) && !SafeToInsertPseudoNeedingChildren(aFrame) {
                // Reframing aFrame->GetContent() is good enough, since the content of
                // table pseudo-frames is the ancestor content.
                self.mDynamicChangeCounts.mContainerReframes += 1;
                self.RecreateFramesForContent(self.ContentIdOf(aFrame), InsertionKind::Async);
                return true;
            }
        }

        // Situation #5 is a frame in multicol subtree that's getting new children.
        // NOT PORTED: columns.

        // A <fieldset> may need to pick up a new rendered legend from aItems.
        // NOT PORTED: fieldsets.

        // Now we have several cases involving {ib} splits.  Put them all in a
        // do/while with breaks to take us to the "go and reconstruct" code.
        'reconstruct: {
            if IsInlineFrame(aFrame) {
                if aItems.AreAllItemsInline() {
                    // We can just put the kids in.
                    return false;
                }

                if !IsFramePartOfIBSplit(aFrame) {
                    // Need to go ahead and reconstruct.
                    break 'reconstruct;
                }

                // Now we're adding kids including some blocks to an inline part of an
                // {ib} split.  If we plan to call AppendFrames, and don't have a next
                // sibling for the new frames, and our parent is the last continuation of
                // the last part of the {ib} split, and the same is true of all our
                // ancestor inlines (they have no following continuations and they're the
                // last part of their {ib} splits and we'd be adding to the end for all
                // of them), then AppendFrames will handle things for us.  Bail out in
                // that case.
                if aIsAppend && IsSafeToAppendToIBSplitInline(aFrame, nextSibling) {
                    return false;
                }

                // Need to reconstruct.
                break 'reconstruct;
            }

            // Now we know we have a block parent.  If it's not part of an
            // ib-split, we're all set.
            if !IsFramePartOfIBSplit(aFrame) {
                return false;
            }

            // We're adding some kids to a block part of an {ib} split.  If all the
            // kids are blocks, we don't need to reconstruct.
            if aItems.AreAllItemsBlock() {
                return false;
            }

            // We might have some inline kids for this block.  Just fall out of the
            // loop and reconstruct.
        }

        // If we don't have a containing block, start with aFrame and look for one.
        let mut aContainingBlock = aContainingBlock.unwrap_or(aFrame);

        // To find the right block to reframe, just walk up the tree until we find a
        // frame that is:
        // 1)  Not part of an IB split
        // 2)  Not a pseudo-frame
        // 3)  Not an inline frame
        // We're guaranteed to find one, since ComputedStyle::ApplyStyleFixups
        // enforces that the root is display:none, display:table, or display:block.
        // Note that walking up "too far" is OK in terms of correctness, even if it
        // might be a little inefficient.  This is why we walk out of all
        // pseudo-frames -- telling which ones are or are not OK to walk out of is
        // too hard (and I suspect that we do in fact need to walk out of all of
        // them).
        while IsFramePartOfIBSplit(aContainingBlock) || aContainingBlock.IsInlineOutside() || aContainingBlock.StyleIsPseudoOrAnonBox() {
            aContainingBlock = aContainingBlock.GetParent().expect("Must have non-inline, non-ib-split, non-pseudo frame as root (or child of root, for a table root)!");
        }

        // Tell parent of the containing block to reformulate the
        // entire block. This is painful and definitely not optimal
        // but it will *always* get the right answer.

        let blockContent = self.ContentIdOf(aContainingBlock);
        self.mDynamicChangeCounts.mContainerReframes += 1;
        self.RecreateFramesForContent(blockContent, InsertionKind::Async);
        true
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ReframeContainingBlock
    fn ReframeContainingBlock(&mut self, aFrame: nsIFrame) {
        // (IsReflowLocked: the frame constructor never runs during reflow here)

        // Get the first "normal" ancestor of the target frame.
        let containingBlock = GetIBContainingBlockFor(aFrame);
        // From here we look for the containing block in case the target
        // frame is already a block (which can happen when an inline frame
        // wraps some of its content in an anonymous block; see
        // ConstructInline)

        // NOTE: We used to get the FloatContainingBlock here, but it was often
        // wrong. GetIBContainingBlock works much better and provides the correct
        // container in all cases so GetFloatContainingBlock(aFrame) has been
        // removed

        // And get the containingBlock's content
        let blockContent = self.ContentIdOf(containingBlock);
        if blockContent != NONE {
            self.RecreateFramesForContent(blockContent, InsertionKind::Async);
            return;
        }

        // If we get here, we're screwed!
        let root = self.sd().tree.dom.children(self.sd().tree.doc).find(|&c| self.IsElement(c)).unwrap_or(NONE);
        self.RecreateFramesForContent(root, InsertionKind::Async);
    }
}

// ---- the table section --------------------------------------------------------------------------------

// Gecko: layout/base/nsCSSFrameConstructor.cpp PullOutCaptionFrames
// Pull all the captions present in aItems out into aCaptions.
fn PullOutCaptionFrames(aList: &mut nsFrameList, aCaptions: &mut nsFrameList) {
    let mut child = aList.FirstChild();
    while let Some(c) = child {
        let nextSibling = c.GetNextSibling();
        if c.StyleDisplay().mDisplay() == StyleDisplayValue::TableCaption {
            aList.RemoveFrame(c);
            aCaptions.AppendFrame(None, c);
        }
        child = nextSibling;
    }
}

// Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::PseudoParentData
pub struct PseudoParentData {
    pub mFCData: FrameConstructionData,
    pub mPseudoType: PseudoStyleType,
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::sPseudoParentData
// (indexed by ParentType; the five ruby entries are NOT PORTED)
static sPseudoParentData: [PseudoParentData; 5] = [
    // Cell
    PseudoParentData {
        mFCData: FrameConstructionData::new__FrameFullConstructor(
            nsCSSFrameConstructor::ConstructTableCell,
            FCDATA_IS_TABLE_PART | FCDATA_SKIP_FRAMESET | FCDATA_USE_CHILD_ITEMS | FCDATA_IS_WRAPPER_ANON_BOX | FCDATA_DESIRED_PARENT_TYPE_TO_BITS(eTypeRow),
        ),
        mPseudoType: PseudoStyleType::MozTableCell,
    },
    // Row
    PseudoParentData {
        mFCData: FrameConstructionData::new__FrameFullConstructor(
            nsCSSFrameConstructor::ConstructTableRowOrRowGroup,
            FCDATA_IS_TABLE_PART | FCDATA_SKIP_FRAMESET | FCDATA_USE_CHILD_ITEMS | FCDATA_IS_WRAPPER_ANON_BOX | FCDATA_DESIRED_PARENT_TYPE_TO_BITS(eTypeRowGroup),
        ),
        mPseudoType: PseudoStyleType::MozTableRow,
    },
    // Row group
    PseudoParentData {
        mFCData: FrameConstructionData::new__FrameFullConstructor(
            nsCSSFrameConstructor::ConstructTableRowOrRowGroup,
            FCDATA_IS_TABLE_PART | FCDATA_SKIP_FRAMESET | FCDATA_USE_CHILD_ITEMS | FCDATA_IS_WRAPPER_ANON_BOX | FCDATA_DESIRED_PARENT_TYPE_TO_BITS(eTypeTable),
        ),
        mPseudoType: PseudoStyleType::MozTableRowGroup,
    },
    // Column group
    PseudoParentData {
        mFCData: FrameConstructionData::new(
            Some(ToCreationFunc_NS_NewTableColGroupFrame),
            FCDATA_IS_TABLE_PART
                | FCDATA_SKIP_FRAMESET
                | FCDATA_DISALLOW_OUT_OF_FLOW
                | FCDATA_USE_CHILD_ITEMS
                | FCDATA_SKIP_ABSPOS_PUSH
                // Not FCDATA_IS_WRAPPER_ANON_BOX, because we don't need to
                // restyle these: they have non-inheriting styles.
                | FCDATA_DESIRED_PARENT_TYPE_TO_BITS(eTypeTable),
        ),
        mPseudoType: PseudoStyleType::MozTableColumnGroup,
    },
    // Table
    PseudoParentData {
        mFCData: FrameConstructionData::new__FrameFullConstructor(nsCSSFrameConstructor::ConstructTable, FCDATA_SKIP_FRAMESET | FCDATA_USE_CHILD_ITEMS | FCDATA_IS_WRAPPER_ANON_BOX),
        mPseudoType: PseudoStyleType::MozTable,
    },
];

// ---- the frame classes that are not ported ------------------------------------------------------------

impl nsCSSFrameConstructor {
    // ADAPTER: the frames that stand in for a class that is not ported are block frames; this
    // makes one with the display that class would be laid out with (aDisplay replaces `display` in
    // the style) and constructs its children as a block does.
    fn ConstructAsBlockWithDisplay(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: StyleDisplayValue, aFrameList: &mut nsFrameList) -> nsIFrame {
        aItem.mComputedStyle = nsCSSFrameConstructor::StyleWithDisplay(&aItem.mComputedStyle, aDisplay);
        let display = nsStyleDisplay::new(aItem.mComputedStyle.clone());
        self.ConstructNonScrollableBlock(aState, aItem, aParentFrame, &display, aFrameList)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructTable
    // Construct the outer, inner table frames and the children frames for the
    // table.
    // XXX Page break frames for pseudo table frames are not constructed to avoid
    // the risk associated with revising the pseudo frame mechanism. The long term
    // solution of having frames handle page-break-before/after will solve the
    // problem.
    // (MathML tables, the pseudo-element case of a root table, and PushAbsoluteContainingBlock: NOT
    // PORTED)
    fn ConstructTable(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        MOZ_ASSERT!(aDisplay.mDisplay() == StyleDisplayValue::Table || aDisplay.mDisplay() == StyleDisplayValue::InlineTable, "Unexpected call");

        let content = aItem.mContent;
        let computedStyle = aItem.mComputedStyle.clone();
        // const bool isMathMLContent = content->IsMathMLElement(); NOT PORTED: MathML

        // create the pseudo SC for the table wrapper as a child of the inner SC
        let outerComputedStyle = self.ResolveInheritingAnonymousBoxStyle(PseudoStyleType::MozTableWrapper, &computedStyle);

        // Create the table wrapper frame which holds the caption and inner table
        // frame
        let newFrame = NS_NewTableWrapperFrame(self.AllocFrame(outerComputedStyle.clone())).0;

        let geometricParent = aState.GetGeometricParent(&nsStyleDisplay::new(outerComputedStyle.clone()), aParentFrame);

        // Init the table wrapper frame
        self.InitAndRestoreFrame(aState, content, geometricParent, newFrame);

        // Create the inner table frame
        let innerFrame = NS_NewTableFrame(self.AllocFrame(computedStyle.clone())).0;

        self.InitAndRestoreFrame(aState, content, Some(newFrame), innerFrame);
        innerFrame.AddStateBits(NS_FRAME_OWNS_ANON_BOXES);

        // Put the newly created frames into the right child list
        SetInitialSingleChild(newFrame, innerFrame);

        aState.AddChild(self, newFrame, aFrameList, content, aParentFrame, true, true, false, None);

        if self.mRootElementFrame.is_none() {
            self.mRootElementFrame = Some(newFrame);
        }

        let mut childList = nsFrameList::default();

        // Process children
        let mut absoluteSaveState = nsFrameConstructorSaveState::default();

        // Mark the table frame as an absolute container if needed
        newFrame.AddStateBits(NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN);
        if newFrame.IsAbsPosContainingBlock() {
            aState.PushAbsoluteContainingBlock(Some(newFrame), Some(newFrame), &mut absoluteSaveState);
        }

        let mut floatSaveState = nsFrameConstructorSaveState::default();
        aState.MaybePushFloatContainingBlock(innerFrame, &mut floatSaveState);

        if (aItem.mFCData.mBits & FCDATA_USE_CHILD_ITEMS) != 0 {
            self.ConstructFramesFromItemList(aState, &mut aItem.mChildItems, innerFrame, (aItem.mFCData.mBits & FCDATA_IS_WRAPPER_ANON_BOX) != 0, &mut childList);
        } else {
            self.ProcessChildren(aState, content, &computedStyle, innerFrame, true, &mut childList, false, None);
        }

        let mut captionList = nsFrameList::default();
        PullOutCaptionFrames(&mut childList, &mut captionList);

        // Set the inner table frame's principal child list.
        nsContainerFrame::from_frame(innerFrame).SetInitialChildList(FrameChildListID::Principal, childList);

        // Append caption frames to the table wrapper frame's principal child list.
        if captionList.NotEmpty() {
            captionList.ApplySetParent(newFrame);
            nsContainerFrame::from_frame(newFrame).AppendFrames(FrameChildListID::Principal, captionList);
        }

        floatSaveState.Restore(aState); // ~nsFrameConstructorSaveState
        absoluteSaveState.Restore(aState); // ~nsFrameConstructorSaveState

        newFrame
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp MakeTablePartAbsoluteContainingBlock
    // (a function of the file, here an associated function of the class)
    fn MakeTablePartAbsoluteContainingBlock(aState: &mut nsFrameConstructorState, aFrame: nsIFrame) {
        // If we're positioned, then we need to become an absolute containing block
        // for any absolutely positioned children.
        aFrame.AddStateBits(NS_FRAME_CAN_HAVE_ABSPOS_CHILDREN);
        // if (aFrame->IsAbsPosContainingBlock()) { aState.PushAbsoluteContainingBlock(...) }
        // NOT PORTED: positioned table parts. nsTableFrame does not reflow the absolutely positioned
        // children of its parts (nsTableFrame.cpp lines 1775-1832 at GECKO_REV, see nsTableFrame.rs),
        // so a positioned row, row group or cell is not made a containing block and its absolutely
        // positioned descendants go to the next containing block up (GAPS C3).
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructTableRowOrRowGroup
    fn ConstructTableRowOrRowGroup(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        MOZ_ASSERT!(
            aDisplay.mDisplay() == StyleDisplayValue::TableRow
                || aDisplay.mDisplay() == StyleDisplayValue::TableRowGroup
                || aDisplay.mDisplay() == StyleDisplayValue::TableFooterGroup
                || aDisplay.mDisplay() == StyleDisplayValue::TableHeaderGroup,
            "Not a row or row group"
        );
        let content = aItem.mContent;
        let computedStyle = aItem.mComputedStyle.clone();

        let newFrame: nsIFrame = if aDisplay.mDisplay() == StyleDisplayValue::TableRow {
            // (NS_NewMathMLmtrFrame: NOT PORTED)
            NS_NewTableRowFrame(self.AllocFrame(computedStyle.clone())).0
        } else {
            NS_NewTableRowGroupFrame(self.AllocFrame(computedStyle.clone())).0
        };

        self.InitAndRestoreFrame(aState, content, aParentFrame, newFrame);

        // nsFrameConstructorSaveState absoluteSaveState;
        nsCSSFrameConstructor::MakeTablePartAbsoluteContainingBlock(aState, newFrame);

        let mut floatSaveState = nsFrameConstructorSaveState::default();
        aState.MaybePushFloatContainingBlock(newFrame, &mut floatSaveState);

        let mut childList = nsFrameList::default();
        if (aItem.mFCData.mBits & FCDATA_USE_CHILD_ITEMS) != 0 {
            self.ConstructFramesFromItemList(aState, &mut aItem.mChildItems, newFrame, (aItem.mFCData.mBits & FCDATA_IS_WRAPPER_ANON_BOX) != 0, &mut childList);
        } else {
            self.ProcessChildren(aState, content, &computedStyle, newFrame, true, &mut childList, false, None);
        }

        nsContainerFrame::from_frame(newFrame).SetInitialChildList(FrameChildListID::Principal, childList);
        aFrameList.AppendFrame(None, newFrame);
        floatSaveState.Restore(aState); // ~nsFrameConstructorSaveState
        newFrame
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructTableCol
    fn ConstructTableCol(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aStyleDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        let content = aItem.mContent;
        let computedStyle = aItem.mComputedStyle.clone();

        let colFrame = NS_NewTableColFrame(self.AllocFrame(computedStyle.clone()));
        self.InitAndRestoreFrame(aState, content, aParentFrame, colFrame.0);
        // ADAPTER: Gecko's nsTableColFrame::GetSpan reads `-x-span` of the style, which the `span`
        // attribute of the element maps to (HTMLTableColElement::MapAttributesIntoRule).
        colFrame.SetSpan(self.ParseClampedNonNegativeInt(content, "span", 1, 1, MAX_COLSPAN));

        aFrameList.AppendFrame(None, colFrame.0);

        // construct additional col frames if the col frame has a span > 1
        let span = colFrame.GetSpan();
        for spanX in 1..span {
            let newCol = NS_NewTableColFrame(self.AllocFrame(computedStyle.clone()));
            self.InitAndRestoreFrame(aState, content, aParentFrame, newCol.0);
            aFrameList.LastChild().unwrap().SetNextContinuation(Some(newCol.0));
            newCol.0.SetPrevContinuation(aFrameList.LastChild());
            aFrameList.AppendFrame(None, newCol.0);
            newCol.SetColType(nsTableColType::eColAnonymousCol);
        }

        colFrame.0
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructTableCell
    // (MathML cells, scrollable cells (the cell content is never wrapped in a scroll frame) and the page
    // name tracker: NOT PORTED)
    fn ConstructTableCell(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        MOZ_ASSERT!(aDisplay.mDisplay() == StyleDisplayValue::TableCell, "Unexpected call");

        let content = aItem.mContent;
        let computedStyle = aItem.mComputedStyle.clone();

        let tableFrame = nsTableRowFrame::from_frame(aParentFrame.unwrap()).GetTableFrame();
        // Warning: If you change this and add a wrapper frame around table cell
        // frames, make sure Bug 368554 doesn't regress!
        // See IsInAutoWidthTableCellForQuirk() in nsImageFrame.cpp.
        let cellFrame = NS_NewTableCellFrame(self.AllocFrame(computedStyle.clone()), tableFrame);

        // Initialize the table cell frame
        self.InitAndRestoreFrame(aState, content, aParentFrame, cellFrame.0);
        cellFrame.0.AddStateBits(NS_FRAME_OWNS_ANON_BOXES);
        // ADAPTER: nsTableCellFrame::GetColSpan and GetRowSpan read the parsed `colspan` and `rowspan`
        // attributes of the element, except for a pseudo cell (which has no such attributes: its
        // content is its parent's). The frame is given the values (HTMLTableCellElement::ParseAttribute).
        if (aItem.mFCData.mBits & FCDATA_IS_WRAPPER_ANON_BOX) == 0 {
            cellFrame.SetSpans(self.ParseClampedNonNegativeInt(content, "rowspan", 1, 0, MAX_ROWSPAN), self.ParseClampedNonNegativeInt(content, "colspan", 1, 1, MAX_COLSPAN));
        }

        // Resolve pseudo style and initialize the body cell frame
        let innerPseudoStyle = self.ResolveInheritingAnonymousBoxStyle(PseudoStyleType::MozCellContent, &computedStyle);

        // Create a block frame that will format the cell's content
        let cellInnerFrame = NS_NewBlockFrame(self.AllocFrame(innerPseudoStyle)).0;
        // (the pseudo type MozCellContent is one of the block wrappers, see nsIFrame::IsBlockWrapper)
        cellInnerFrame.SetIsBlockWrapper(true);
        self.InitAndRestoreFrame(aState, content, Some(cellFrame.0), cellInnerFrame);

        // nsFrameConstructorSaveState absoluteSaveState;
        nsCSSFrameConstructor::MakeTablePartAbsoluteContainingBlock(aState, cellFrame.0);

        let mut floatSaveState = nsFrameConstructorSaveState::default();
        aState.MaybePushFloatContainingBlock(cellInnerFrame, &mut floatSaveState);

        let mut childList = nsFrameList::default();
        if (aItem.mFCData.mBits & FCDATA_USE_CHILD_ITEMS) != 0 {
            self.ConstructFramesFromItemList(aState, &mut aItem.mChildItems, cellInnerFrame, (aItem.mFCData.mBits & FCDATA_IS_WRAPPER_ANON_BOX) != 0, &mut childList);
        } else {
            // Process the child content
            self.ProcessChildren(aState, content, &computedStyle, cellInnerFrame, true, &mut childList, true, None);
        }

        nsContainerFrame::from_frame(cellInnerFrame).SetInitialChildList(FrameChildListID::Principal, childList);

        SetInitialSingleChild(cellFrame.0, cellInnerFrame);
        aFrameList.AppendFrame(None, cellFrame.0);
        floatSaveState.Restore(aState); // ~nsFrameConstructorSaveState
        cellFrame.0
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreateNeededPseudoContainers
    /**
     * This function walks through the items in aItems and
     * find items that cannot have aParentFrame as their parent.  We wrap
     * continuous runs of such items into a FrameConstructionItem for a frame that
     * gets them closer to their desired parents.  For example, a run of non-row
     * children of a row-group will get wrapped in a row.  When we later construct
     * the frame for this wrapper (in this case for the row), it'll be the correct
     * parent for the cells in the set of items we wrapped or we'll wrap cells
     * around everything else.  At the end of this method, aItems is guaranteed to
     * contain only items for frames that can be direct kids of aParentFrame.
     */
    fn CreateNeededPseudoContainers(&mut self, aState: &mut nsFrameConstructorState, aItems: &mut FrameConstructionItemList, aParentFrame: nsIFrame) {
        let ourParentType = nsCSSFrameConstructor::GetParentType(aParentFrame);
        if IsRubyParentType(ourParentType) || aItems.AllWantParentType(ourParentType) {
            // Nothing to do here
            return;
        }

        let mut iter: FCItemIterator = 0;
        // ADAPTER: Gecko's `continue` in this do-while loop goes to its condition; here it is
        // `break 'continue_` out of the block that holds the body, and the condition follows the block.
        loop {
            'continue_: {
                if aItems.SkipItemsWantingParentType(&mut iter, ourParentType) {
                    // Nothing else to do here; we're finished
                    return;
                }

                // Now we're pointing to the first child that wants a different parent
                // type.

                // Now try to figure out what kids we can group together.  We can generally
                // group everything that has a different desired parent type from us.  Two
                // exceptions to this:
                // 1) If our parent type is table, we can't group columns with anything
                //    else other than whitespace.
                // 2) Whitespace that lies between two things we can group which both want
                //    a non-block parent should be dropped, even if we can't group them
                //    with each other and even if the whitespace wants a parent of
                //    ourParentType.  Ends of the list count as things that don't want a
                //    block parent (so that for example we'll drop a whitespace-only list).

                let mut endIter = iter; /* iterator to find the end of the group */
                let mut groupingParentType = aItems.mItems[endIter].DesiredParentType();
                if aItems.AllWantParentType(groupingParentType) && groupingParentType != eTypeBlock {
                    // Just group them all and be done with it.  We need the check for
                    // eTypeBlock here to catch the "all the items are whitespace" case
                    // described above.
                    endIter = aItems.mItems.len(); // SetToEnd
                } else {
                    // Locate the end of the group.

                    // Keep track of the type the previous item wanted, in case we have to
                    // deal with whitespace.  Start it off with ourParentType, since that's
                    // the last thing |iter| would have skipped over.
                    let mut prevParentType = ourParentType;
                    loop {
                        // Walk an iterator past any whitespace that we might be able to drop
                        // from the list
                        let mut spaceEndIter = endIter;
                        if prevParentType != eTypeBlock && !aParentFrame.HasAnyStateBits(NS_FRAME_GENERATED_CONTENT) && self.ItemIsWhitespace(&aItems.mItems[spaceEndIter]) {
                            let trailingSpaces = self.SkipWhitespace(aItems, &mut spaceEndIter);

                            // We drop the whitespace in the following cases:
                            // 1) If these are not trailing spaces and the next item wants a table
                            //    or table-part parent
                            // 2) If these are trailing spaces and aParentFrame is a
                            //    tabular container according to rule 1.3 of CSS 2.1 Sec 17.2.1.
                            //    (Being a tabular container pretty much means ourParentType is
                            //    not eTypeBlock besides the eTypeColGroup case, which won't
                            //    reach here.)
                            if (!trailingSpaces && IsTableParentType(aItems.mItems[spaceEndIter].DesiredParentType())) || (trailingSpaces && ourParentType != eTypeBlock) {
                                let updateStart = iter == endIter;
                                aItems.DeleteItemsTo(&mut endIter, spaceEndIter);
                                // (the items before spaceEndIter are gone: it is endIter now)
                                spaceEndIter = endIter;
                                NS_ASSERTION!(trailingSpaces == aItems.IsDone(endIter), "These should match");

                                if updateStart {
                                    iter = endIter;
                                }

                                if trailingSpaces {
                                    break; /* Found group end */
                                }

                                if updateStart {
                                    // Update groupingParentType, since it might have been eTypeBlock
                                    // just because of the whitespace.
                                    groupingParentType = aItems.mItems[iter].DesiredParentType();
                                }
                            }
                        }

                        // Now endIter points to a non-whitespace item or a non-droppable
                        // whitespace item. In the latter case, if this is the end of the group
                        // we'll traverse this whitespace again.  But it'll all just be quick
                        // DesiredParentType() checks which will match ourParentType (that's
                        // what it means that this is the group end), so it's OK.
                        // However, when we are grouping a ruby parent, and endIter points to
                        // a non-droppable whitespace, if the next non-whitespace item also
                        // wants a ruby parent, the whitespace should also be included into
                        // the current ruby container.
                        prevParentType = aItems.mItems[endIter].DesiredParentType();
                        if prevParentType == ourParentType
                            && (endIter == spaceEndIter
                                || aItems.IsDone(spaceEndIter)
                                || !IsRubyParentType(groupingParentType)
                                || !IsRubyParentType(aItems.mItems[spaceEndIter].DesiredParentType()))
                        {
                            // End the group at endIter.
                            break;
                        }

                        if ourParentType == eTypeTable && (prevParentType == eTypeColGroup) != (groupingParentType == eTypeColGroup) {
                            // Either we started with columns and now found something else, or
                            // vice versa.  In any case, end the grouping.
                            break;
                        }

                        // If we have some whitespace that we were not able to drop and there is
                        // an item after the whitespace that is already properly parented, then
                        // make sure to include the spaces in our group but stop the group after
                        // that.
                        if spaceEndIter != endIter && !aItems.IsDone(spaceEndIter) && ourParentType == aItems.mItems[spaceEndIter].DesiredParentType() {
                            endIter = spaceEndIter;
                            break;
                        }

                        // Include the whitespace we didn't drop (if any) in the group.
                        endIter = spaceEndIter;
                        prevParentType = aItems.mItems[endIter].DesiredParentType();

                        endIter += 1; // endIter.Next()
                        if aItems.IsDone(endIter) {
                            break;
                        }
                    }
                }

                if iter == endIter {
                    // Nothing to wrap here; just skipped some whitespace
                    break 'continue_;
                }

                if ourParentType == eTypeColGroup {
                    // Suppress non-col items of colgroups.
                    aItems.DeleteItemsTo(&mut iter, endIter);
                    break 'continue_;
                }

                // Now group together all the items between iter and endIter.  The right
                // parent type to use depends on ourParentType.
                let wrapperType = match ourParentType {
                    // The parent type for a cell is eTypeBlock, since that's what a cell
                    // looks like to its kids.
                    eTypeRow => eTypeBlock,
                    eTypeRowGroup => eTypeRow,
                    // Either colgroup or rowgroup, depending on what we're grouping.
                    eTypeTable => {
                        if groupingParentType == eTypeColGroup {
                            eTypeColGroup
                        } else {
                            eTypeRowGroup
                        }
                    }
                    _ => {
                        NS_ASSERTION!(ourParentType == eTypeBlock, "Unrecognized parent type");
                        if IsRubyParentType(groupingParentType) {
                            // NOT PORTED: ruby; no item wants a ruby parent.
                            unimplemented!("NOT PORTED: ruby pseudo parents")
                        } else {
                            NS_ASSERTION!(IsTableParentType(groupingParentType), "groupingParentType should be either Ruby or table");
                            eTypeTable
                        }
                    }
                };

                let parentStyle = aParentFrame.Style();
                let parentContent = self.ContentOfFrame(aParentFrame);
                self.WrapItemsInPseudoParent(parentContent, &parentStyle, wrapperType, aItems, &mut iter, endIter);

                // Now |iter| points to the item that was the first one we didn't wrap;
                // loop and see whether we need to skip it or wrap it in something
                // different.
            }
            if aItems.IsDone(iter) {
                break;
            }
        }
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::WrapItemsInPseudoParent
    /**
     * This method wraps frame construction item from |aIter| to
     * |aEndIter|. After it returns, aIter points to the first item
     * after the wrapper.
     */
    fn WrapItemsInPseudoParent(&mut self, aParentContent: ContentId, aParentStyle: &ServoArc<ComputedStyle>, aWrapperType: ParentType, aItems: &mut FrameConstructionItemList, aIter: &mut FCItemIterator, aEndIter: FCItemIterator) {
        let pseudoData = &sPseudoParentData[aWrapperType as usize];
        let mut pseudoType = pseudoData.mPseudoType;
        let parentDisplay = nsStyleDisplay::new(aParentStyle.clone());
        let parentDisplayInside = parentDisplay.DisplayInside();

        // XXXmats should we use IsInlineInsideStyle() here instead? seems odd to
        // exclude RubyBaseContainer/RubyTextContainer...
        // (the parent display of ruby base and ruby text: NOT PORTED)
        if pseudoType == PseudoStyleType::MozTable && parentDisplay.IsInlineFlow() {
            pseudoType = PseudoStyleType::MozInlineTable;
        }
        let _ = parentDisplayInside;

        let wrapperStyle = if (pseudoData.mFCData.mBits & FCDATA_IS_WRAPPER_ANON_BOX) != 0 {
            self.ResolveInheritingAnonymousBoxStyle(pseudoType, aParentStyle)
        } else {
            self.ResolveNonInheritingAnonymousBoxStyle(pseudoType)
        };

        // Use the content of our parent frame
        let mut newItem = FrameConstructionItem::new(&pseudoData.mFCData, aParentContent, wrapperStyle, true);

        let disp = nsStyleDisplay::new(newItem.mComputedStyle.clone());
        // Here we're cheating a tad... technically, table-internal items should be
        // inline if aParentFrame is inline, but they'll get wrapped in an
        // inline-table in the end, so it'll all work out.  In any case, arguably
        // we don't need to maintain this state at this point... but it's better
        // to, I guess.
        newItem.mIsAllInline = disp.IsInlineOutsideStyle();

        // bool isRuby = disp->IsRubyDisplayType(); NOT PORTED: ruby
        // Table pseudo frames always induce line boundaries around their
        // contents.
        newItem.mChildItems.SetLineBoundaryAtStart(true);
        newItem.mChildItems.SetLineBoundaryAtEnd(true);
        // The parent of the items in aItems is also the parent of the items
        // in mChildItems
        newItem.mChildItems.SetParentHasNoShadowDOM(aItems.ParentHasNoShadowDOM());

        // Eat up all items between |aIter| and |aEndIter| and put them in our
        // wrapper Advances |aIter| to point to |aEndIter|.
        aItems.AppendItemsToList(aIter, aEndIter, &mut newItem.mChildItems);

        aItems.InsertItem(aIter, newItem);
    }

    // Gecko: layout/base/nsCSSFrameConstructor.h nsCSSFrameConstructor::GetParentType(nsIFrame*) and
    // layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::GetParentType(LayoutFrameType)
    fn GetParentType(aParentFrame: nsIFrame) -> ParentType {
        let aFrameType = aParentFrame.Type();
        if aFrameType == LayoutFrameType::Table {
            return eTypeTable;
        }
        if aFrameType == LayoutFrameType::TableRowGroup {
            return eTypeRowGroup;
        }
        if aFrameType == LayoutFrameType::TableRow {
            return eTypeRow;
        }
        if aFrameType == LayoutFrameType::TableColGroup {
            return eTypeColGroup;
        }
        // (RubyBaseContainer, RubyTextContainer and Ruby: NOT PORTED)

        eTypeBlock
    }

    // Skip over whitespace.  Return whether the iterator is done after doing
    // that.  The iterator must not be done, and must be pointing to a
    // whitespace item when this is called.
    // Gecko: FrameConstructionItemList::Iterator::SkipWhitespace
    // (on the constructor: IsWhitespace needs it)
    fn SkipWhitespace(&self, aItems: &FrameConstructionItemList, aIter: &mut FCItemIterator) -> bool {
        MOZ_ASSERT!(!aItems.IsDone(*aIter), "Shouldn't be done yet");
        MOZ_ASSERT!(self.ItemIsWhitespace(&aItems.mItems[*aIter]), "Not pointing to whitespace?");
        loop {
            *aIter += 1;
            if aItems.IsDone(*aIter) {
                return true;
            }
            if !self.ItemIsWhitespace(&aItems.mItems[*aIter]) {
                break;
            }
        }

        false
    }

    // ADAPTER: the frames of replaced elements and form controls (NS_NewImageFrame,
    // NS_NewInputButtonControlFrame, NS_NewSubDocumentFrame, NS_NewHTMLVideoFrame, ... and the
    // text control, select and progress frames, which Gecko also gives anonymous content): a
    // block frame with no children, an inline-block when the element is inline-level, so its size
    // is what its CSS says (the width and height attributes of an <img> reach it as presentational
    // hints).
    fn ConstructReplacedFrameStub(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        if aDisplay.IsInlineFlow() {
            aItem.mComputedStyle = nsCSSFrameConstructor::StyleWithDisplay(&aItem.mComputedStyle, StyleDisplayValue::InlineBlock);
        }
        let display = nsStyleDisplay::new(aItem.mComputedStyle.clone());
        let computedStyle = aItem.mComputedStyle.clone();
        let mut newFrame = NS_NewBlockFrame(self.AllocFrame(computedStyle.clone())).0;
        newFrame.SetProperty(nsIFrame::LeafFrameProperty(), true);
        let geometricParent = aState.GetGeometricParent(&display, aParentFrame);
        self.ConstructBlock(aState, aItem.mContent, geometricParent, aParentFrame, &computedStyle, &mut newFrame, aFrameList, None);
        newFrame
    }

    // Gecko: layout/svg/SVGOuterSVGFrame.cpp NS_NewSVGOuterSVGFrame, as FindSVGData uses it
    // ADAPTER: the leaf block of ConstructReplacedFrameStub, which also gets the `width`, `height` and
    // `viewBox` of the element as SVGOuterSVGData (SVGOuterSVGFrame.rs sizes the frame from it). The
    // descendants of the <svg> get no frames. NOT PORTED: SVGOuterSVGFrame and the SVG frames inside it.
    fn ConstructSVGOuterSVG(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        let fontSize = nsStyleFont::new(aItem.mComputedStyle.clone()).mFontSizeInCSSPixels();
        let data = SVGOuterSVGData::FromAttributes(self.GetAttr(aItem.mContent, "width"), self.GetAttr(aItem.mContent, "height"), self.GetAttr(aItem.mContent, "viewBox"), fontSize);
        let newFrame = self.ConstructReplacedFrameStub(aState, aItem, aParentFrame, aDisplay, aFrameList);
        newFrame.SetProperty(SVGOuterSVGDataProperty(), data);
        newFrame
    }
}

// ---- the form controls (layout/forms) -----------------------------------------------------------------

impl nsCSSFrameConstructor {
    // ADAPTER: the integer value of a positive-integer attribute (HTML's rules for parsing integers, as
    // nsAttrValue::ParsePositiveIntValue does); None if it is missing, malformed or not above 0.
    fn ParsePositiveIntAttr(&self, aContent: ContentId, aName: &str) -> Option<i32> {
        let n = self.ParseClampedNonNegativeInt(aContent, aName, 0, 0, i32::MAX);
        if n > 0 { Some(n) } else { None }
    }

    // ADAPTER: HTMLSelectElement::Size (the `size` attribute as an unsigned integer, 0 when missing or
    // malformed) and HTMLSelectElement::Multiple.
    fn SelectSizeAndMultiple(&self, aContent: ContentId) -> (i32, bool) {
        (self.ParseClampedNonNegativeInt(aContent, "size", 0, 0, i32::MAX), self.HasAttr(aContent, "multiple"))
    }

    // ADAPTER: the common part of the constructors below: a frame of the class that aCreate makes, with no
    // children (their anonymous content, and the options of a select, are NOT PORTED), as
    // ConstructReplacedFrameStub does for the classes that are not ported at all.
    fn ConstructFormControlFrame(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList, aCreate: fn(nsIFrame) -> nsIFrame, aData: FormControlData, aInlineBecomesInlineBlock: bool) -> nsIFrame {
        // A form control is atomic inline-level when its display is inline (Gecko's style adjuster does
        // this for them; Stylo's servo configuration does not).
        if aInlineBecomesInlineBlock && aDisplay.IsInlineFlow() {
            aItem.mComputedStyle = nsCSSFrameConstructor::StyleWithDisplay(&aItem.mComputedStyle, StyleDisplayValue::InlineBlock);
        }
        let display = nsStyleDisplay::new(aItem.mComputedStyle.clone());
        let computedStyle = aItem.mComputedStyle.clone();
        let newFrame = aCreate(self.AllocFrame(computedStyle));
        let geometricParent = aState.GetGeometricParent(&display, aParentFrame);
        self.InitAndRestoreFrame(aState, aItem.mContent, geometricParent, newFrame);
        newFrame.SetFormControlData(aData);
        aState.AddChild(self, newFrame, aFrameList, aItem.mContent, aParentFrame, true, true, false, None);
        newFrame
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindHTMLData
    // (SIMPLE_TAG_CREATE(video, NS_NewHTMLVideoFrame), SIMPLE_TAG_CREATE(audio, NS_NewHTMLVideoFrame))
    // REDUCED: no children (the controls, the poster and the caption are NOT PORTED).
    fn ConstructMediaElement(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        fn create(aFrame: nsIFrame) -> nsIFrame {
            NS_NewHTMLVideoFrame(aFrame).0
        }
        self.ConstructFormControlFrame(aState, aItem, aParentFrame, aDisplay, aFrameList, create, FormControlData::default(), false)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindHTMLData
    // (SIMPLE_TAG_CREATE(iframe, NS_NewSubDocumentFrame)): a leaf; the frame's document is laid out on its
    // own (nsSubDocumentFrame.rs).
    fn ConstructSubDocument(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        fn create(aFrame: nsIFrame) -> nsIFrame {
            NS_NewSubDocumentFrame(aFrame).0
        }
        self.ConstructFormControlFrame(aState, aItem, aParentFrame, aDisplay, aFrameList, create, FormControlData::default(), false)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::ConstructTextControl
    // REDUCED: no anonymous content (the editor, placeholder, button), the attributes the frame reads
    // (cols, rows) are given to it.
    fn ConstructTextControl(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        let content = aItem.mContent;
        let isTextArea = self.IsHTMLElement(content, "textarea");
        // HTMLInputElement::GetCols (`size`), HTMLTextAreaElement::GetCols and GetRows
        // (DEFAULT_ROWS_TEXTAREA 2; HTMLInputElement::GetRows is DEFAULT_ROWS, 1)
        let data = FormControlData {
            mCols: self.ParsePositiveIntAttr(content, if isTextArea { "cols" } else { "size" }),
            mRows: if isTextArea { self.ParsePositiveIntAttr(content, "rows").unwrap_or(2) } else { 1 },
            mIsTextArea: isTextArea,
            ..Default::default()
        };
        fn create(aFrame: nsIFrame) -> nsIFrame {
            NS_NewTextControlFrame(aFrame).0
        }
        self.ConstructFormControlFrame(aState, aItem, aParentFrame, aDisplay, aFrameList, create, data, true)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindInputData
    // (SIMPLE_INT_CREATE(FormControlType::InputCheckbox, ToCreationFunc(NS_NewCheckboxRadioFrame)))
    fn ConstructCheckboxRadio(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        fn create(aFrame: nsIFrame) -> nsIFrame {
            NS_NewCheckboxRadioFrame(aFrame).0
        }
        self.ConstructFormControlFrame(aState, aItem, aParentFrame, aDisplay, aFrameList, create, FormControlData::default(), true)
    }

    // ADAPTER: what the select frames read of the <option>s and <optgroup>s of aSelect: the rendered label
    // (HTMLOptionElement::GetRenderedLabel: the `label` attribute if not empty, else the text of the
    // option with the whitespace stripped and collapsed) and the padding of the option from its style.
    // An option with display:none has no frame in Gecko and is left out.
    fn CollectSelectOptions(&self, aSelect: ContentId) -> Vec<FormControlOption> {
        let mut options = Vec::new();
        let mut child = self.GetFirstChild(aSelect);
        while child != NONE {
            if self.IsHTMLElement(child, "option") {
                self.AppendSelectOption(child, &mut options);
            } else if self.IsHTMLElement(child, "optgroup") {
                if self.sd().computed(child).map_or(false, |s| nsStyleDisplay::new(s).mDisplay() != StyleDisplayValue::None) {
                    // the label of the group: its `label` attribute (the optgroup:before rule)
                    options.push(FormControlOption { mLabel: self.GetAttr(child, "label").unwrap_or("").to_string(), mIsOptGroupLabel: true, mPaddingISize: 0, mPaddingBSize: 0 });
                    let mut grandChild = self.GetFirstChild(child);
                    while grandChild != NONE {
                        if self.IsHTMLElement(grandChild, "option") {
                            self.AppendSelectOption(grandChild, &mut options);
                        }
                        grandChild = self.GetNextSibling(grandChild);
                    }
                }
            }
            child = self.GetNextSibling(child);
        }
        options
    }

    // ADAPTER: CollectSelectOptions for one <option>.
    fn AppendSelectOption(&self, aOption: ContentId, aOptions: &mut Vec<FormControlOption>) {
        let style = match self.sd().computed(aOption) {
            Some(s) => s,
            None => return,
        };
        if nsStyleDisplay::new(style.clone()).mDisplay() == StyleDisplayValue::None {
            return;
        }
        let label = match self.GetAttr(aOption, "label") {
            Some(l) if !l.is_empty() => l.to_string(),
            _ => {
                // the text of the option, the whitespace stripped and collapsed
                let mut text = String::new();
                let mut child = self.GetFirstChild(aOption);
                while child != NONE {
                    if self.IsText(child) {
                        text.push_str(self.TextOf(child));
                    }
                    child = self.GetNextSibling(child);
                }
                text.split(|c| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r')).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
            }
        };
        let mut padding = nsMargin::default();
        nsStylePadding::new(style).GetPadding(&mut padding);
        aOptions.push(FormControlOption { mLabel: label, mIsOptGroupLabel: false, mPaddingISize: padding.left + padding.right, mPaddingBSize: padding.top + padding.bottom });
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindInputData
    // (NS_NewInputButtonControlFrame, an nsBlockFrame subclass) REDUCED: the block frame <button> gets
    // (ConstructNonScrollableBlock), marked as an input button so that it has a line when its label is
    // empty (nsIFrame::ShouldHaveLineIfEmpty).
    fn ConstructInputButtonControl(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        let newFrame = self.ConstructNonScrollableBlock(aState, aItem, aParentFrame, aDisplay, aFrameList);
        newFrame.SetProperty(nsIFrame::InputButtonControlFrameProperty(), true);
        newFrame
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindSelectData
    // (NS_NewComboboxControlFrame)
    fn ConstructComboboxControl(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        let (size, multiple) = self.SelectSizeAndMultiple(aItem.mContent);
        let data = FormControlData { mRows: 1, mSelectSize: size, mSelectMultiple: multiple, mOptions: self.CollectSelectOptions(aItem.mContent), ..Default::default() };
        fn create(aFrame: nsIFrame) -> nsIFrame {
            NS_NewComboboxControlFrame(aFrame).0
        }
        self.ConstructFormControlFrame(aState, aItem, aParentFrame, aDisplay, aFrameList, create, data, true)
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::FindSelectData
    // (NS_NewListControlFrame)
    fn ConstructListControl(&mut self, aState: &mut nsFrameConstructorState, aItem: &mut FrameConstructionItem, aParentFrame: Option<nsIFrame>, aDisplay: &nsStyleDisplay, aFrameList: &mut nsFrameList) -> nsIFrame {
        let (size, multiple) = self.SelectSizeAndMultiple(aItem.mContent);
        let data = FormControlData { mRows: 1, mSelectSize: size, mSelectMultiple: multiple, mOptions: self.CollectSelectOptions(aItem.mContent), ..Default::default() };
        fn create(aFrame: nsIFrame) -> nsIFrame {
            NS_NewListControlFrame(aFrame).0
        }
        self.ConstructFormControlFrame(aState, aItem, aParentFrame, aDisplay, aFrameList, create, data, true)
    }
}

// Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreateContinuingFrame
// ADAPTER: a static function (Gecko's is a method reached through the PresShell). This makes the continuation of an inline, text,
// letter or line frame the way CreateContinuingFrame does for them: same style, content and
// writing mode, the new frame parented to aParentFrame (not linked into any child list: the
// caller does that) and hooked into the flow after aFrame (nsSplittableFrame::Init). The new
// frame is dirty and not yet reflowed, like every new frame (AddFrame).
// NOT PORTED: the other frame classes (blocks, tables, ...), the pseudo-element styles of
// ::first-line and ::first-letter continuations, and the state bits Init copies, except the one
// of an {ib} split.
pub fn nsCSSFrameConstructor_CreateContinuingFrame(aFrame: nsIFrame, aParentFrame: nsIFrame, aIsFluid: bool) -> nsIFrame {
    let nextContinuation = aFrame.GetNextContinuation();
    let nextInFlow = aFrame.GetNextInFlow();

    let rect = aFrame.GetRect();
    let wm = aFrame.GetWritingMode();
    let style = WithFrameTree(|t| t.mFrames[aFrame.0 as usize].mStyle.clone());
    let newFrame = WithFrameTree(|t| t.AddFrame(None, nsRect::new(rect.x, rect.y, 0, 0), wm, style));
    newFrame.SetType(aFrame.Type());
    newFrame.SetContent(aFrame.GetContent());
    newFrame.SetIsBlockWrapper(aFrame.IsBlockWrapper());
    newFrame.SetParent(aParentFrame);
    // nsIFrame::Init copies the bits that make the frame part of an {ib} split from the frame it continues
    newFrame.AddStateBits(aFrame.GetStateBits() & NS_FRAME_PART_OF_IBSPLIT);
    // Init of the frame class: it hooks the frame into the flow after aFrame (nsSplittableFrame::Init,
    // or for text nsContinuingTextFrame::Init), fluid or not; the non-fluid case is undone below
    if aFrame.Type() == LayoutFrameType::Text && aFrame.GetContent().map_or(false, |c| c.IsText()) {
        nsContinuingTextFrame::Init(nsTextFrame::from_frame(newFrame), aFrame.GetContent(), aParentFrame, aFrame);
    } else {
        aFrame.HookNextInFlow(newFrame);
    }

    if !aIsFluid {
        newFrame.SetPrevContinuation(Some(aFrame));
    }

    // Hook the new frame in front of the continuation aFrame had
    if let Some(nextInFlow) = nextInFlow {
        nextInFlow.SetPrevInFlow(Some(newFrame));
        newFrame.SetNextInFlow(Some(nextInFlow));
    } else if let Some(nextContinuation) = nextContinuation {
        nextContinuation.SetPrevContinuation(Some(newFrame));
        newFrame.SetNextContinuation(Some(nextContinuation));
    }

    newFrame
}

// Gecko's frame construction is tested by reftests and mochitests that need a whole browser; these
// tests are written from the documented behaviour (CSS 2.2 chapters 8, 9 and 10, HTML rendering): a
// snippet of HTML is parsed, styled with Firefox's user-agent sheets (mrt-style), its frames are
// constructed and reflowed (driver.rs), and the heights and frame counts are what the CSS says. The
// sizes are set in the snippets (line-height, height, width), so the numbers do not depend on a font.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::*;
    use mrt_style::mrt_dom::Dom;

    const PX: i32 = 60;

    // Parses, styles and lays `html` out at 1000 x 800 CSS px. The frame tree is this test thread's.
    fn layout(html: &str) -> (PageLayout, std::collections::BTreeMap<String, usize>) {
        let mut sd = StyleDom::new(Dom::parse(html), (1000.0, 800.0));
        sd.resolve();
        // the frames read their content from the DOM (see nsIContent)
        let sd = KeepDocumentAlive(sd);
        let result = LayoutDocument(&sd, 1000, 800);
        (result, FrameCountsByType())
    }

    // The document height in CSS px
    fn height(html: &str) -> f64 {
        layout(html).0.RootElementHeightInCSSPixels().unwrap()
    }

    const BODY: &str = "<!DOCTYPE html><html><head><style>body{margin:0}</style></head><body>";

    #[test]
    fn a_heading_and_a_paragraph_stack_their_heights() {
        // CSS 2.2 10.6.3: the height of a block is the distance from the top margin edge of its
        // first child to the bottom margin edge of the last. The heading is one 40px line, the
        // paragraph one 20px line, and the 10px between them is the paragraph's top margin.
        let html = format!("{BODY}<h1 style='margin:0;font-size:20px;line-height:40px'>Title</h1><p style='margin:10px 0 0;font-size:16px;line-height:20px'>Hello</p></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(70.0));
        // html, body, h1 and p are blocks that hold the two texts (the viewport and the canvas
        // have their own types)
        assert_eq!(counts["Block"], 4);
        assert_eq!(counts["Text"], 2);
        assert_eq!(counts["Viewport"], 1);
        assert_eq!(counts["Canvas"], 1);
    }

    #[test]
    fn the_margins_of_a_heading_and_a_paragraph_collapse() {
        // CSS 2.2 8.3.1: adjoining vertical margins of siblings collapse to the larger one: 20, not 30.
        let html = format!("{BODY}<h1 style='margin:0 0 20px;font-size:20px;line-height:40px'>T</h1><p style='margin:10px 0 0;line-height:20px'>x</p></body></html>");
        assert_eq!(height(&html), 40.0 + 20.0 + 20.0);
    }

    #[test]
    fn the_user_agent_style_of_a_heading_applies() {
        // html.css: h1 is display:block, 2em, margin-block 0.67em. With a 20px line-height, the
        // 32px font gives one 20px line, and 0.67em = 21.44px margins on both sides. The body has
        // no margin, so the margins of h1 collapse through it; the document element is a block
        // formatting context root, so they stay inside it: 21.44 + 20 + 21.44.
        let html = format!("{BODY}<h1 style='line-height:20px'>T</h1></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(counts["Block"], 3);
        let h = result.RootElementHeightInCSSPixels().unwrap();
        assert!((h - (20.0 + 2.0 * 21.44)).abs() < 0.5, "unexpected height {h}");
        let h1 = find_first(result.mRootElementFrame.unwrap(), "h1").unwrap();
        assert_eq!(h1.GetSize().height, 20 * PX);
        assert_eq!(h1.StyleFont().mFontSizeInCSSPixels(), 32.0);
    }

    #[test]
    fn display_none_makes_no_frames() {
        // CSS 2.2 9.2.4: display:none generates no boxes at all, so its subtree has no frames and
        // takes no space.
        let html = format!("{BODY}<div style='height:10px'></div><div style='display:none;height:50px'><p>gone</p></div><div style='height:5px'></div></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(15.0));
        assert_eq!(counts["Block"], 4, "html, body and two divs: {counts:?}");
        assert!(!counts.contains_key("Text"), "no text frame for the hidden paragraph: {counts:?}");
    }

    #[test]
    fn the_head_of_the_document_has_no_frames() {
        // html.css: head, title, style and script are display:none
        let html = format!("{BODY}<div style='height:10px'></div></body></html>");
        let (_, counts) = layout(&html);
        // viewport, canvas, html, body, div
        assert_eq!(counts["Block"], 3, "{counts:?}");
        assert_eq!(counts.values().sum::<usize>(), 5);
    }

    #[test]
    fn whitespace_only_text_next_to_blocks_gets_no_frame() {
        // CSS Text 3 4.1.1 and Gecko's frame construction: collapsible white space between block
        // boxes is not rendered, so its text node has no frame (it is at a line boundary).
        let html = format!("{BODY}<div>   \n  <p style='margin:0;height:10px'></p>  \n  <p style='margin:0;height:10px'></p>  \n </div></body></html>");
        let (result, counts) = layout(&html);
        assert!(!counts.contains_key("Text"), "{counts:?}");
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(20.0));
    }

    #[test]
    fn whitespace_between_inline_elements_keeps_its_frame() {
        // The space between two spans is rendered (it is not next to a line boundary): the text
        // frames are "a", " " and "b".
        let html = format!("{BODY}<p style='margin:0'><span>a</span> <span>b</span></p></body></html>");
        let (_, counts) = layout(&html);
        assert_eq!(counts["Text"], 3, "{counts:?}");
        assert_eq!(counts["Inline"], 2);
    }

    #[test]
    fn inline_boxes_wrap_into_lines_as_wide_as_the_container() {
        // CSS 2.2 9.4.2: a line box holds as many inline boxes as fit. Three 60px inline-blocks in
        // a 100px container make three lines of 20px (two do not fit: 120 > 100).
        let html = format!(
            "{BODY}<p style='margin:0;width:100px;font-size:16px;line-height:20px'>\
             <span style='display:inline-block;width:60px;height:20px'></span>\
             <span style='display:inline-block;width:60px;height:20px'></span>\
             <span style='display:inline-block;width:60px;height:20px'></span></p></body></html>"
        );
        let (result, counts) = layout(&html);
        assert_eq!(counts["Block"], 3 + 3, "html, body, p and three inline-blocks: {counts:?}");
        let h = result.RootElementHeightInCSSPixels().unwrap();
        // each line is at least the 20px inline-block, and the strut (20px line-height, baseline
        // at the bottom of the inline-block) may add a few px below it
        assert!(h >= 60.0 && h <= 60.0 + 3.0 * 8.0, "three lines, got {h}");
    }

    #[test]
    fn inline_blocks_that_fit_share_a_line() {
        let html = format!(
            "{BODY}<p style='margin:0;width:200px;font-size:16px;line-height:20px'>\
             <span style='display:inline-block;width:60px;height:20px'></span>\
             <span style='display:inline-block;width:60px;height:20px'></span></p></body></html>"
        );
        let h = height(&html);
        assert!(h >= 20.0 && h < 30.0, "one line, got {h}");
    }

    #[test]
    fn a_paragraph_of_text_wraps_in_a_narrow_container() {
        // The same text in a wide and in a narrow container: the narrow one has more lines, each
        // of the line-height (the text is real, so only the relation between the two is checked).
        let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
        let wide = height(&format!("{BODY}<p style='margin:0;width:900px;font:16px sans-serif;line-height:20px'>{text}</p></body></html>"));
        let narrow = height(&format!("{BODY}<p style='margin:0;width:100px;font:16px sans-serif;line-height:20px'>{text}</p></body></html>"));
        assert_eq!(wide, 20.0);
        assert!(narrow >= 80.0 && narrow % 20.0 == 0.0, "narrow paragraph height {narrow}");
        let (_, counts) = layout(&format!("{BODY}<p style='margin:0;width:100px;font:16px sans-serif;line-height:20px'>{text}</p></body></html>"));
        assert!(counts["TextContinuation"] >= 3, "wrapped text has continuing frames: {counts:?}");
    }

    #[test]
    fn list_items_have_an_outside_marker_and_the_height_of_their_line() {
        // CSS Lists 3: an outside ::marker is a box next to the first line of the list item; it
        // does not add height when it has the font and line-height of the list item.
        let html = format!("{BODY}<ul style='margin:0;padding:0 0 0 40px'><li style='line-height:20px'>a</li><li style='line-height:20px'>b</li><li style='line-height:20px'>c</li></ul></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(60.0));
        // html, body, ul, three li and three markers
        assert_eq!(counts["Block"], 3 + 3 + 3, "{counts:?}");
        // each list item's text plus the marker's text
        assert_eq!(counts["Text"], 6);
        // the markers are registered with their list items
        let rootFrame = result.mRootElementFrame.unwrap();
        let li = find_first(rootFrame, "li").expect("a list item frame");
        assert!(nsBlockFrame::from_frame(li).HasOutsideMarker());
        // and sit to the left of the list item's box
        let marker = nsBlockFrame::from_frame(li).GetOutsideMarker().unwrap();
        assert!(marker.GetRect().x < 0, "the outside marker is outside: {:?}", marker.GetRect());
    }

    #[test]
    fn list_style_none_has_no_marker_text() {
        let html = format!("{BODY}<ul style='margin:0;padding:0;list-style:none'><li style='line-height:20px'>a</li></ul></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(counts["Text"], 1, "{counts:?}");
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(20.0));
    }

    #[test]
    fn an_inside_marker_is_part_of_the_first_line() {
        let html = format!("{BODY}<ul style='margin:0;padding:0;list-style-position:inside'><li style='line-height:20px'>a</li></ul></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(20.0));
        assert_eq!(counts["Text"], 2, "{counts:?}");
        let li = find_first(result.mRootElementFrame.unwrap(), "li").unwrap();
        assert!(!nsBlockFrame::from_frame(li).HasOutsideMarker());
        assert!(nsBlockFrame::from_frame(li).HasMarker());
    }

    #[test]
    fn a_float_gets_a_placeholder_and_is_in_the_float_list_of_its_block() {
        // CSS 2.2 9.5: a float is taken out of the flow; the block formatting context root (the
        // document element) grows to contain it (10.6.7).
        let html = format!("{BODY}<div style='float:left;width:50px;height:30px'></div><div style='clear:both;height:10px'></div></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(counts["Placeholder"], 1, "{counts:?}");
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(40.0));
        let body = find_first(result.mRootElementFrame.unwrap(), "body").unwrap();
        let floats = body.GetChildList(FrameChildListID::Float);
        assert_eq!(floats.GetLength(), 1);
        let float = floats.FirstChild().unwrap();
        assert!(float.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW));
        assert_eq!(float.GetSize(), nsSize::new(50 * PX, 30 * PX));
        assert_eq!(float.GetParent(), Some(body));
        let placeholder = float.GetPlaceholderFrame().expect("the float's placeholder");
        assert_eq!(nsPlaceholderFrame::from_frame(placeholder).GetOutOfFlowFrame(), Some(float));
        assert!(placeholder.HasAnyStateBits(PLACEHOLDER_FOR_FLOAT));
    }

    #[test]
    fn text_wraps_beside_a_float() {
        // CSS 2.2 9.5: line boxes next to a float are shortened. A 100px container with a 60px
        // float: a 60px inline-block fits only below the float's 20px (40px left beside it).
        let html = format!(
            "{BODY}<div style='width:100px;font-size:16px;line-height:20px'><div style='float:left;width:60px;height:20px'></div>\
             <span style='display:inline-block;width:60px;height:20px'></span></div></body></html>"
        );
        let h = height(&html);
        assert!(h >= 40.0, "the inline-block moves below the float, got {h}");
    }

    #[test]
    fn a_block_in_an_inline_splits_the_inline() {
        // CSS 2.2 9.2.1.1: a block inside an inline breaks the inline in two, around an anonymous
        // block. Gecko's {ib} split: an inline, a block wrapper that holds the div, an inline.
        let html = format!("{BODY}<div style='margin:0;line-height:20px'><span id=s>a<div style='height:10px'></div>b</span></div></body></html>");
        let (result, counts) = layout(&html);
        // the two lines of text (20px each) and the div (10px)
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(50.0));
        // the span is two inlines
        assert_eq!(counts["Inline"], 2, "{counts:?}");
        let inlines = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::Inline);
        assert_eq!(inlines.len(), 2);
        assert!(inlines[0].HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT));
        assert!(inlines[1].HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT));
        assert!(inlines[0].FrameIsNonLastInIBSplit() && !inlines[0].FrameIsNonFirstInIBSplit());
        assert!(inlines[1].FrameIsNonFirstInIBSplit() && !inlines[1].FrameIsNonLastInIBSplit());
        // the block wrapper between them holds the div
        let wrapper = inlines[0].FirstContinuation().GetProperty(nsIFrame::IBSplitSibling()).unwrap();
        assert!(wrapper.IsBlockWrapper());
        assert_eq!(wrapper.GetProperty(nsIFrame::IBSplitSibling()), Some(inlines[1]));
    }

    // The tests of the table section. They are written from CSS 2.2 chapter 17 and the HTML rendering
    // section (tables), on the sizes of cells that are set in the snippets; Gecko's own tests for this
    // code are reftests. A cell is 30x10 where it does not say otherwise, with no padding and no spacing.
    const CELL: &str = "style='padding:0;width:30px;height:10px'";

    // The box of a frame relative to the document, in CSS px
    fn rect_in_doc(aFrame: nsIFrame) -> (f64, f64, f64, f64) {
        BoxInDocument(aFrame)
    }

    #[test]
    fn a_2x2_table_has_a_wrapper_a_table_a_row_group_rows_cells_and_synthetic_columns() {
        // The two rows of two 30x10 cells sit in one tbody (the parser makes it): 60x20.
        let html = format!("{BODY}<table style='border-spacing:0'><tr><td {CELL}></td><td {CELL}></td></tr><tr><td {CELL}></td><td {CELL}></td></tr></table></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(20.0));
        // html and body, and the block that formats the content of each of the 4 cells
        assert_eq!(counts["Block"], 2 + 4, "{counts:?}");
        for (name, n) in [("TableWrapper", 1), ("Table", 1), ("TableRowGroup", 1), ("TableRow", 2), ("TableCell", 4), ("TableColGroup", 1), ("TableCol", 2)] {
            assert_eq!(counts[name], n, "{name} in {counts:?}");
        }
        let root = result.mRootElementFrame.unwrap();
        let wrapper = find_first(root, "table").unwrap();
        assert_eq!(wrapper.Type(), LayoutFrameType::TableWrapper);
        assert_eq!(rect_in_doc(wrapper), (0.0, 0.0, 60.0, 20.0));
        let cells = find_all(root, LayoutFrameType::TableCell);
        assert_eq!(rect_in_doc(cells[1]), (30.0, 0.0, 30.0, 10.0));
        assert_eq!(rect_in_doc(cells[2]), (0.0, 10.0, 30.0, 10.0));
        // the inner table is the child of the wrapper, and a cell holds one block
        let table = find_all(root, LayoutFrameType::Table)[0];
        assert_eq!(table.GetParent(), Some(wrapper));
        let inner = cells[0].GetChildList(FrameChildListID::Principal).FirstChild().unwrap();
        assert_eq!(inner.Type(), LayoutFrameType::Block);
        assert!(inner.IsBlockWrapper());
    }

    #[test]
    fn a_cell_with_a_colspan_makes_the_columns_wide_enough_for_it() {
        // CSS 2.2 17.5.2.2: a cell that spans two columns is as wide as both. The 50px cell is wider
        // than the two 20px cells under it, so the two columns share the 10px that are missing.
        let html = format!("{BODY}<table style='border-spacing:0'><tr><td colspan=2 style='padding:0;width:50px;height:10px'></td></tr><tr><td style='padding:0;width:20px;height:10px'></td><td style='padding:0;width:20px;height:10px'></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        let cells = find_all(root, LayoutFrameType::TableCell);
        assert_eq!(cells.len(), 3);
        assert_eq!(rect_in_doc(cells[0]).2, 50.0);
        assert_eq!(rect_in_doc(cells[1]).2 + rect_in_doc(cells[2]).2, 50.0);
        assert_eq!(rect_in_doc(find_first(root, "table").unwrap()).2, 50.0);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(20.0));
    }

    #[test]
    fn a_colspan_of_zero_or_garbage_is_one() {
        // HTML: colspan is a positive integer of at most 1000; anything else is 1.
        let html = format!("{BODY}<table style='border-spacing:0'><tr><td colspan=0 {CELL}></td><td colspan=x {CELL}></td><td colspan=-3 {CELL}></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(rect_in_doc(find_first(root, "table").unwrap()).2, 90.0);
    }

    #[test]
    fn a_huge_colspan_is_clamped_to_1000_columns() {
        let html = format!("{BODY}<table style='border-spacing:0'><tr><td colspan=100000 {CELL}></td></tr></table></body></html>");
        let (result, counts) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(counts["TableCol"], 1000, "the cell is clamped to 1000 columns");
        assert_eq!(rect_in_doc(find_first(root, "table").unwrap()).2, 30.0);
    }

    #[test]
    fn a_cell_with_a_rowspan_is_as_high_as_the_rows_it_spans() {
        // CSS 2.2 17.5.3: the 30px cell spans the two 10px rows, so the rows share the missing 10px.
        let html = format!("{BODY}<table style='border-spacing:0'><tr><td rowspan=2 style='padding:0;width:20px;height:30px'></td><td style='padding:0;width:20px;height:10px'></td></tr><tr><td style='padding:0;width:20px;height:10px'></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        let cells = find_all(root, LayoutFrameType::TableCell);
        assert_eq!(rect_in_doc(cells[0]), (0.0, 0.0, 20.0, 30.0));
        assert_eq!(rect_in_doc(cells[1]).1, 0.0);
        // the second row's cell is in the second column of the table: the first is taken by the rowspan
        assert_eq!(rect_in_doc(cells[2]).0, 20.0);
        assert_eq!(rect_in_doc(find_first(root, "table").unwrap()), (0.0, 0.0, 40.0, 30.0));
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(30.0));
    }

    #[test]
    fn a_thead_is_first_and_a_tfoot_last_wherever_they_are_in_the_source() {
        // HTML and CSS 2.2 17.2: a table-header-group is the first row group and a table-footer-group
        // the last, whatever the order of the source.
        let html = format!("{BODY}<table style='border-spacing:0'><tfoot><tr><td {CELL}></td></tr></tfoot><tbody><tr><td {CELL}></td></tr></tbody><thead><tr><td {CELL}></td></tr></thead></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        let tfoot = find_first(root, "tfoot").unwrap();
        let tbody = find_first(root, "tbody").unwrap();
        let thead = find_first(root, "thead").unwrap();
        assert_eq!(rect_in_doc(thead).1, 0.0);
        assert_eq!(rect_in_doc(tbody).1, 10.0);
        assert_eq!(rect_in_doc(tfoot).1, 20.0);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(30.0));
    }

    #[test]
    fn the_cellspacing_and_cellpadding_attributes_space_and_pad_the_cells() {
        // HTML rendering, tables: cellspacing is the border-spacing of the table, cellpadding the padding
        // of its cells. One 20x10 cell with padding 3 is 26x16; the 4px spacing is around it: 34x24.
        let html = format!("{BODY}<table cellspacing=4 cellpadding=3><tr><td style='width:20px;height:10px'></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        let cell = find_all(root, LayoutFrameType::TableCell)[0];
        assert_eq!(rect_in_doc(cell), (4.0, 4.0, 26.0, 16.0));
        assert_eq!(rect_in_doc(find_first(root, "table").unwrap()), (0.0, 0.0, 34.0, 24.0));
    }

    #[test]
    fn cellpadding_zero_removes_the_padding_the_user_agent_sheet_gives_a_cell() {
        // html.css gives a cell 1px of padding; cellpadding=0 on the table wins over it.
        let html = format!("{BODY}<table cellspacing=0 cellpadding=0><tr><td style='width:20px;height:10px'></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let cell = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::TableCell)[0];
        assert_eq!(rect_in_doc(cell), (0.0, 0.0, 20.0, 10.0));
        // a style rule of the page wins over the attribute
        let html = format!("{BODY}<style>td{{padding:5px}}</style><table cellspacing=0 cellpadding=0><tr><td style='width:20px;height:10px'></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let cell = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::TableCell)[0];
        assert_eq!(rect_in_doc(cell), (0.0, 0.0, 30.0, 20.0));
    }

    #[test]
    fn the_border_attribute_gives_the_table_and_its_cells_a_border() {
        // html.css: table:-moz-table-border-nonzero has a 1px outset border and its cells a 1px inset one.
        // One 20x10 cell: 1 + 1 padding + 1 border around it: 24x14; border-spacing 2px and the table
        // border 1px: 30x20. border=0 has neither.
        let html = format!("{BODY}<table border=1><tr><td style='width:20px;height:10px'></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(rect_in_doc(find_all(root, LayoutFrameType::TableCell)[0]), (3.0, 3.0, 24.0, 14.0));
        assert_eq!(rect_in_doc(find_first(root, "table").unwrap()), (0.0, 0.0, 30.0, 20.0));
        let html = format!("{BODY}<table border=0><tr><td style='width:20px;height:10px'></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(rect_in_doc(find_first(root, "table").unwrap()), (0.0, 0.0, 26.0, 16.0));
    }

    #[test]
    fn a_table_of_width_100_percent_fills_its_container() {
        // CSS 2.2 17.5.2.2: a table with a width wider than its columns need gives the columns the rest.
        let html = format!("{BODY}<table width='100%' style='border-spacing:0'><tr><td style='padding:0;height:10px'></td><td style='padding:0;height:10px'></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(rect_in_doc(find_first(root, "table").unwrap()).2, 1000.0);
        let cells = find_all(root, LayoutFrameType::TableCell);
        assert_eq!(rect_in_doc(cells[0]).2 + rect_in_doc(cells[1]).2, 1000.0);
        // a width of 200 (pixels) and of 50%
        let html = format!("{BODY}<table width=200 style='border-spacing:0'><tr><td style='padding:0;height:10px'></td></tr></table><table width='50%' style='border-spacing:0'><tr><td style='padding:0;height:10px'></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        let wrappers = find_all(root, LayoutFrameType::TableWrapper);
        assert_eq!(rect_in_doc(wrappers[0]).2, 200.0);
        assert_eq!(rect_in_doc(wrappers[1]).2, 500.0);
    }

    #[test]
    fn a_td_width_attribute_is_the_width_of_its_column() {
        let html = format!("{BODY}<table style='border-spacing:0'><tr><td width=70 style='padding:0;height:10px'></td><td style='padding:0;width:30px'></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let cells = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::TableCell);
        assert_eq!(rect_in_doc(cells[0]).2, 70.0);
        assert_eq!(rect_in_doc(cells[1]).2, 30.0);
    }

    #[test]
    fn a_table_in_a_cell_is_laid_out_inside_it() {
        // The inner 30x10 table gives the outer cell its size.
        let html = format!(
            "{BODY}<table style='border-spacing:0'><tr><td style='padding:0'><table style='border-spacing:0'><tr><td {CELL}></td></tr></table></td></tr></table></body></html>"
        );
        let (result, counts) = layout(&html);
        assert_eq!(counts["TableWrapper"], 2);
        assert_eq!(counts["Table"], 2);
        let root = result.mRootElementFrame.unwrap();
        let wrappers = find_all(root, LayoutFrameType::TableWrapper);
        assert_eq!(rect_in_doc(wrappers[0]), (0.0, 0.0, 30.0, 10.0));
        assert_eq!(rect_in_doc(wrappers[1]), (0.0, 0.0, 30.0, 10.0));
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(10.0));
    }

    #[test]
    fn a_table_in_an_inline_block_sits_on_a_line() {
        // The inline-block is as big as its table; the line has no strut (line-height 0), so the line is
        // as high as the inline-block.
        let html = format!("{BODY}<div style='line-height:0'><div style='display:inline-block'><table style='border-spacing:0'><tr><td {CELL}></td></tr></table></div></div></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(rect_in_doc(find_first(root, "table").unwrap()), (0.0, 0.0, 30.0, 10.0));
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(10.0));
    }

    #[test]
    fn an_inline_table_sits_in_a_line_with_the_text_around_it() {
        // display: inline-table makes the wrapper inline-level: it is a child of the line, with the
        // text before it (here a 20px-tall line from the text and the table sitting on the baseline).
        let html = format!("{BODY}<div style='line-height:20px;font-size:10px'>a<table style='display:inline-table;border-spacing:0'><tr><td {CELL}></td></tr></table></div></body></html>");
        let (result, counts) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        let wrapper = find_first(root, "table").unwrap();
        assert!(wrapper.StyleDisplay().IsInlineOutsideStyle());
        assert_eq!(counts["Text"], 1);
        assert!(result.RootElementHeightInCSSPixels().unwrap() >= 10.0);
    }

    #[test]
    fn whitespace_between_the_parts_of_a_table_makes_no_frames() {
        // CSS 2.2 17.2.1: whitespace-only text between table parts is dropped. The table is the same
        // as the 2x2 one, spread over lines; there are no text frames.
        let html = format!(
            "{BODY}<table style='border-spacing:0'>\n  <tbody>\n  <tr>\n    <td {CELL}></td>\n    <td {CELL}></td>\n  </tr>\n  </tbody>\n</table></body></html>"
        );
        let (result, counts) = layout(&html);
        assert_eq!(counts.get("Text"), None, "{counts:?}");
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(10.0));
        // ... and in a cell, whitespace around a block goes too, but text stays
        let html = format!("{BODY}<table style='border-spacing:0'><tr><td style='padding:0'> <div style='height:10px;width:30px'></div> </td><td style='padding:0;line-height:10px'> x </td></tr></table></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(counts["Text"], 1, "{counts:?}");
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(10.0));
    }

    #[test]
    fn table_cells_without_a_row_get_an_anonymous_table_row_group_and_row() {
        // CSS 2.2 17.2.1: a run of cells with no table parent gets an anonymous table, row group and row
        // around it. (The parser inserts the tbody and tr of <td> in a <table>, so these are divs.)
        let html = format!("{BODY}<div style='display:table-cell;width:30px;height:10px'></div><div style='display:table-cell;width:30px;height:10px'></div></body></html>");
        let (result, counts) = layout(&html);
        for (name, n) in [("TableWrapper", 1), ("Table", 1), ("TableRowGroup", 1), ("TableRow", 1), ("TableCell", 2)] {
            assert_eq!(counts[name], n, "{name} in {counts:?}");
        }
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(rect_in_doc(find_first(root, "div").unwrap()).2, 30.0);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(10.0));
        // the anonymous table is as wide as its two cells
        let wrapper = find_all(root, LayoutFrameType::TableWrapper)[0];
        assert_eq!(rect_in_doc(wrapper).2, 60.0);
    }

    #[test]
    fn a_row_with_no_row_group_gets_an_anonymous_table_and_row_group() {
        // CSS 2.2 17.2.1: a table-row in a block gets an anonymous table and row group.
        let html = format!("{BODY}<div style='display:table-row'><div style='display:table-cell;width:30px;height:10px'></div></div></body></html>");
        let (result, counts) = layout(&html);
        for (name, n) in [("TableWrapper", 1), ("Table", 1), ("TableRowGroup", 1), ("TableRow", 1), ("TableCell", 1)] {
            assert_eq!(counts[name], n, "{name} in {counts:?}");
        }
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(10.0));
    }

    #[test]
    fn text_in_a_table_gets_an_anonymous_row_and_cell() {
        // CSS 2.2 17.2.1: text in a table gets an anonymous row (in a row group) and cell.
        let html = format!("{BODY}<div style='display:table;border-spacing:0;line-height:20px'>text</div></body></html>");
        let (result, counts) = layout(&html);
        for (name, n) in [("TableWrapper", 1), ("Table", 1), ("TableRowGroup", 1), ("TableRow", 1), ("TableCell", 1), ("Text", 1)] {
            assert_eq!(counts[name], n, "{name} in {counts:?}");
        }
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(20.0));
    }

    #[test]
    fn cells_inside_an_inline_get_an_anonymous_inline_table() {
        // CSS 2.2 17.2.1: the anonymous table around table parts in an inline is an inline-table.
        let html = format!("{BODY}<span style='line-height:10px'><span style='display:table-cell;width:30px;height:10px'></span></span></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(counts["TableWrapper"], 1, "{counts:?}");
        let wrapper = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::TableWrapper)[0];
        assert!(wrapper.StyleDisplay().IsInlineOutsideStyle());
    }

    #[test]
    fn a_caption_goes_above_the_table_and_caption_side_bottom_below_it() {
        // CSS 2.2 17.4: the caption is a block above the table box (inside the wrapper), or below it.
        let html = format!("{BODY}<table style='border-spacing:0'><caption style='padding:0;height:15px'></caption><tr><td {CELL}></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        let wrapper = find_first(root, "table").unwrap();
        let caption = find_first(root, "caption").unwrap();
        let table = find_all(root, LayoutFrameType::Table)[0];
        assert_eq!(caption.GetParent(), Some(wrapper));
        assert_eq!(rect_in_doc(caption), (0.0, 0.0, 30.0, 15.0));
        assert_eq!(rect_in_doc(table).1, 15.0);
        assert_eq!(rect_in_doc(wrapper), (0.0, 0.0, 30.0, 25.0));
        let html = format!("{BODY}<table style='border-spacing:0;caption-side:bottom'><caption style='padding:0;height:15px'></caption><tr><td {CELL}></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(rect_in_doc(find_first(root, "caption").unwrap()).1, 10.0);
        assert_eq!(rect_in_doc(find_all(root, LayoutFrameType::Table)[0]).1, 0.0);
    }

    #[test]
    fn a_col_gives_its_column_a_width_and_a_span_gives_columns() {
        // CSS 2.2 17.5.2.1: the width of a col is the width of its column. <col span=2> is two columns,
        // and a colgroup with a span and no col in it is that many columns.
        let html = format!(
            "{BODY}<table style='border-spacing:0;table-layout:fixed;width:100px'><colgroup><col style='width:40px'><col span=2 style='width:10px'></colgroup><tr><td style='padding:0;height:10px'></td><td style='padding:0'></td><td style='padding:0'></td></tr></table></body></html>"
        );
        let (result, counts) = layout(&html);
        // (the table is as wide as its width says; the three columns have 40, 10 and 10, the table the
        // remaining 40 spread over them)
        assert_eq!(counts["TableCol"], 3, "{counts:?}");
        assert_eq!(counts["TableColGroup"], 1);
        let cells = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::TableCell);
        let widths: Vec<f64> = cells.iter().map(|c| rect_in_doc(*c).2).collect();
        assert!((widths.iter().sum::<f64>() - 100.0).abs() < 0.1, "{widths:?}");
        assert!(widths[0] > widths[1]);
        assert!((widths[1] - widths[2]).abs() < 0.1);
    }

    #[test]
    fn a_colgroup_with_a_span_and_no_col_is_that_many_columns() {
        let html = format!("{BODY}<table style='border-spacing:0'><colgroup span=3></colgroup><tr><td {CELL}></td><td {CELL}></td><td {CELL}></td></tr></table></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(counts["TableCol"], 3, "{counts:?}");
        assert_eq!(counts["TableColGroup"], 1);
        assert_eq!(rect_in_doc(find_first(result.mRootElementFrame.unwrap(), "table").unwrap()).2, 90.0);
    }

    #[test]
    fn align_center_centers_a_table_and_valign_puts_the_content_of_a_cell_at_the_bottom() {
        // HTML rendering: <table align=center> has auto margins; valign=bottom on a cell is
        // vertical-align: bottom (CSS 2.2 17.5.3).
        let html = format!("{BODY}<table align=center style='border-spacing:0'><tr><td style='padding:0;width:100px;height:30px'></td><td valign=bottom style='padding:0;width:100px'><div style='height:10px'></div></td></tr></table></body></html>");
        let (result, _) = layout(&html);
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(rect_in_doc(find_first(root, "table").unwrap()), (400.0, 0.0, 200.0, 30.0));
        let cells = find_all(root, LayoutFrameType::TableCell);
        let inner = cells[1].GetChildList(FrameChildListID::Principal).FirstChild().unwrap();
        let div = inner.GetChildList(FrameChildListID::Principal).FirstChild().unwrap();
        assert_eq!(rect_in_doc(div).1, 20.0);
    }

    #[test]
    fn a_floating_table_leaves_a_placeholder() {
        // align=left on a table is float: left; the wrapper is the float and gets the placeholder.
        let html = format!("{BODY}<table align=left style='border-spacing:0'><tr><td {CELL}></td></tr></table><p style='margin:0;line-height:10px'>x</p></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(counts["Placeholder"], 1, "{counts:?}");
        let root = result.mRootElementFrame.unwrap();
        let wrapper = find_first(root, "table").unwrap();
        assert!(wrapper.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW));
        assert_eq!(rect_in_doc(wrapper), (0.0, 0.0, 30.0, 10.0));
    }

    #[test]
    fn a_replaced_element_is_a_leaf_block_of_the_size_of_its_attributes() {
        // NOT PORTED: images. The width and height attributes reach the frame as CSS.
        let html = format!("{BODY}<img style='display:block' width=30 height=20 src=x.png><p style='margin:0;line-height:10px'>x</p></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(30.0));
        let img = find_first(result.mRootElementFrame.unwrap(), "img").unwrap();
        assert!(img.IsLeaf());
        assert_eq!(img.GetSize(), nsSize::new(30 * PX, 20 * PX));
        assert_eq!(counts["Text"], 1);
    }

    #[test]
    fn a_broken_image_with_alt_text_is_an_inline_with_the_text() {
        // HTML rendering, nsImageFrame::ImageFrameTypeFor: the mini runtime loads no images, so every
        // <img> is broken, and a broken <img> with non-empty alt text gets no image frame: it is an
        // inline whose ::before holds the alt text (html.css: `img::before { content: -moz-alt-content }`).
        let html = format!("{BODY}<p style='margin:0;font:16px sans-serif;line-height:20px'>x<img alt='hello' width=300 height=200 src=x.png>y</p></body></html>");
        let (result, counts) = layout(&html);
        let img = find_first(result.mRootElementFrame.unwrap(), "img").unwrap();
        assert!(!img.IsLeaf());
        assert_eq!(img.Type(), LayoutFrameType::Inline);
        // x, the alt text, y
        assert_eq!(counts["Text"], 3, "{counts:?}");
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(20.0));
    }

    #[test]
    fn a_broken_image_without_alt_text_keeps_its_image_frame() {
        // ImageFrameTypeFor: no alt text (or an empty one), image frame sized from the attributes.
        for alt in ["", " alt=''"] {
            let html = format!("{BODY}<img style='display:block' width=30 height=20 src=x.png{alt}></body></html>");
            let (result, _) = layout(&html);
            let img = find_first(result.mRootElementFrame.unwrap(), "img").unwrap();
            assert!(img.IsLeaf(), "alt {alt:?}");
            assert_eq!(result.RootElementHeightInCSSPixels(), Some(20.0));
        }
    }

    #[test]
    fn in_quirks_mode_a_broken_image_with_alt_text_and_a_specified_size_keeps_its_image_frame() {
        // ImageFrameTypeFor: quirks mode and both width and height in the style (the attributes are
        // presentational hints, which count as style).
        let html = "<html><head><style>body{margin:0}</style></head><body><img style='display:block' alt='hello' width=30 height=20 src=x.png></body></html>";
        let (result, _) = layout(html);
        let img = find_first(result.mRootElementFrame.unwrap(), "img").unwrap();
        assert!(img.IsLeaf());
        assert_eq!(img.GetSize(), nsSize::new(30 * PX, 20 * PX));
    }

    #[test]
    fn a_wbr_is_a_zero_width_break_opportunity() {
        // HTML rendering: <wbr> allows a line break and has no size. Two 8-letter words that fit a
        // 100px line alone but not together break at the <wbr>: two lines of 30px; with no <wbr> they are one
        // word on one line. (A line-height under the font's height would let the zero-height box on the baseline
        // stretch the line, as an empty inline-block does.)
        let with = format!("{BODY}<div style='width:100px;font:16px sans-serif;line-height:30px'>aaaaaaaa<wbr>bbbbbbbb</div></body></html>");
        let (result, counts) = layout(&with);
        assert_eq!(counts["Wbr"], 1, "{counts:?}");
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(60.0));
        let wbr = find_first(result.mRootElementFrame.unwrap(), "wbr").unwrap();
        assert_eq!(wbr.GetSize().width, 0);
        let without = with.replace("<wbr>", "");
        assert_eq!(height(&without), 30.0);
    }

    #[test]
    fn a_block_with_scrollable_overflow_keeps_the_margins_of_its_children_inside() {
        // CSS 2.2 8.3.1 and 9.4.1: a block that establishes a block formatting context (overflow other than
        // visible and clip, display: flow-root) does not collapse its margins with those of its children, so
        // the 20px margins of the child are inside it, next to the block's own 30px margins. When the margins
        // collapse (a block that is no formatting context) the larger one, 30px, is all there is.
        // (The heights are of the root element, which is a margin root itself.)
        // Gecko gets this from the scroll frame's scrolled-content block.
        let child = "<p style='margin:20px 0;height:10px'></p>";
        let with = |style: &str| height(&format!("{BODY}<div style='margin:30px 0;{style}'>{child}</div></body></html>"));
        assert_eq!(with(""), 30.0 + 10.0 + 30.0, "overflow visible");
        assert_eq!(with("overflow:clip"), 30.0 + 10.0 + 30.0, "overflow clip");
        for style in ["overflow:hidden", "overflow:auto", "overflow:scroll", "overflow-x:hidden", "display:flow-root"] {
            assert_eq!(with(style), 30.0 + 20.0 + 10.0 + 20.0 + 30.0, "{style}");
        }
    }

    #[test]
    fn the_overflow_of_the_body_is_the_viewport_s_and_not_a_formatting_context() {
        // CSS Overflow 3 3.3: the overflow of the root element, or of the body when the root's is visible, is
        // propagated to the viewport; the body itself is not clipped and does not contain its children's margins
        // (the 20px margins collapse with the body's 30px ones).
        let body = |body_style: &str, html_style: &str| {
            height(&format!("<!DOCTYPE html><html style='{html_style}'><body style='margin:30px 0;{body_style}'><p style='margin:20px 0;height:10px'></p></body></html>"))
        };
        assert_eq!(body("overflow:hidden", ""), 30.0 + 10.0 + 30.0);
        // when the root's overflow is not visible it is the root's that is propagated, and the body's is its own
        assert_eq!(body("overflow:hidden", "overflow:auto"), 30.0 + 20.0 + 10.0 + 20.0 + 30.0);
    }

    #[test]
    fn a_line_break_ends_the_line() {
        // CSS 2.2 9.4.2 and HTML rendering: <br> ends the line. Three lines of 20px.
        let html = format!("{BODY}<p style='margin:0;font:16px sans-serif;line-height:20px'>a<br>b<br>c</p></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(counts["Br"], 2);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(60.0));
    }

    #[test]
    fn a_closed_details_shows_only_its_summary() {
        let html = format!("{BODY}<details><summary style='line-height:20px'>s</summary><p style='height:100px'>hidden</p></details></body></html>");
        let closed = height(&html);
        let open = height(&html.replace("<details>", "<details open>"));
        assert!(open >= closed + 100.0, "closed {closed}, open {open}");
    }

    // The box of the first frame of an `aName` element, in CSS px of the document: (x, y, w, h).
    fn box_of(aLayout: &PageLayout, aName: nsAtom) -> (f64, f64, f64, f64) {
        BoxInDocument(find_first(aLayout.mViewport, aName).expect("no frame for the element"))
    }

    // The tests below are written from CSS 2.2 section 10.3.7 and 10.6.4 and CSS Position 3, and
    // the numbers checked against Firefox 157 (bench/bridge-cases/abspos.html).

    #[test]
    fn an_absolutely_positioned_box_gets_a_frame_and_a_placeholder() {
        // The box is out of the flow: the document is as high as the in-flow div, and the box has
        // its frame in the canvas's absolute list with a placeholder where it was in the flow.
        let html = format!("{BODY}<div style='height:10px'></div><aside style='position:absolute;top:0;height:500px'></aside></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(10.0));
        assert_eq!(counts["Block"], 4, "html, body, the div and the aside: {counts:?}");
        assert_eq!(counts["Placeholder"], 1);
        let aside = find_first(result.mViewport, "aside").unwrap();
        assert_eq!(aside.GetParent(), Some(result.mCanvas));
        assert!(aside.GetPlaceholderFrame().is_some());
    }

    #[test]
    fn offsets_are_from_the_padding_edge_of_the_positioned_ancestor() {
        // CSS 2.2 10.1: the containing block is the padding box of the positioned ancestor.
        let html = format!("{BODY}<div style='position:relative;margin-left:30px;border:5px solid;padding:10px;height:100px'><aside style='position:absolute;left:7px;top:9px;width:20px;height:30px'></aside></div></body></html>");
        let (result, _) = layout(&html);
        assert_eq!(box_of(&result, "aside"), (30.0 + 5.0 + 7.0, 5.0 + 9.0, 20.0, 30.0));
    }

    #[test]
    fn auto_offsets_put_the_box_at_its_static_position() {
        // CSS 2.2 10.3.7 and 10.6.4: with both offsets of an axis auto, the box is where it would
        // have been in the flow: below the 10px div, at the start of the body.
        let html = format!("{BODY}<div style='height:10px'></div><aside style='position:absolute;width:5px;height:5px'></aside></body></html>");
        let (result, _) = layout(&html);
        assert_eq!(box_of(&result, "aside"), (0.0, 10.0, 5.0, 5.0));
    }

    #[test]
    fn an_auto_width_with_one_auto_offset_shrinks_to_fit() {
        // CSS 2.2 10.3.7: `right: 0` with `left` and `width` auto: shrink-to-fit, then placed
        // against the right padding edge of the 200px containing block.
        let html = format!("{BODY}<div style='position:relative;width:200px;height:50px'><aside style='position:absolute;right:0;top:0'><span style='display:inline-block;width:40px;height:10px'></span></aside></div></body></html>");
        let (result, _) = layout(&html);
        let (x, y, w, _) = box_of(&result, "aside");
        assert_eq!((x, y, w), (160.0, 0.0, 40.0));
    }

    #[test]
    fn an_auto_width_with_both_offsets_stretches() {
        // CSS Position 3 (abspos-auto-size): `left: 0; right: 0` stretches the box; a percentage
        // width resolves against the padding box (200 + 2 x 10).
        let html = format!("{BODY}<div style='position:relative;width:200px;padding:10px;height:50px'><aside style='position:absolute;left:0;right:0;top:0;height:5px'></aside><nav style='position:absolute;width:50%;top:0;height:5px'></nav></div></body></html>");
        let (result, _) = layout(&html);
        assert_eq!(box_of(&result, "aside").2, 220.0);
        assert_eq!(box_of(&result, "nav").2, 110.0);
    }

    #[test]
    fn auto_margins_center_a_box_between_its_offsets() {
        // CSS 2.2 10.3.7: with left, width and right fixed, the auto margins share what is left.
        let html = format!("{BODY}<div style='position:relative;width:300px;height:50px'><aside style='position:absolute;left:0;right:0;width:100px;margin:0 auto;top:0;height:5px'></aside></div></body></html>");
        let (result, _) = layout(&html);
        assert_eq!(box_of(&result, "aside").0, 100.0);
    }

    #[test]
    fn a_fixed_box_is_placed_against_the_viewport() {
        // CSS 2.2 10.1: the containing block of a fixed box is the viewport (800px high here),
        // even in a document much taller than it.
        let html = format!("{BODY}<div style='height:3000px'></div><aside style='position:fixed;bottom:0;left:0;height:20px;width:10px'></aside></body></html>");
        let (result, _) = layout(&html);
        assert_eq!(box_of(&result, "aside"), (0.0, 780.0, 10.0, 20.0));
        let aside = find_first(result.mViewport, "aside").unwrap();
        assert_eq!(aside.GetParent(), Some(result.mViewport));
    }

    #[test]
    fn a_transformed_ancestor_contains_fixed_boxes() {
        // CSS Transforms 1: a transformed element is the containing block of its fixed descendants.
        let html = format!("{BODY}<div style='height:40px'></div><section style='transform:translateX(0);height:100px'><aside style='position:fixed;bottom:0;left:0;height:20px;width:10px'></aside></section></body></html>");
        let (result, _) = layout(&html);
        assert_eq!(box_of(&result, "aside"), (0.0, 40.0 + 100.0 - 20.0, 10.0, 20.0));
    }

    #[test]
    fn a_relatively_positioned_inline_is_a_containing_block() {
        // CSS 2.2 10.1: an inline containing block starts at the padding edge of its first box.
        let html = format!("{BODY}<p style='margin:0;line-height:20px'>xx <span style='position:relative'>ab<em style='position:absolute;left:0;top:0;width:5px;height:5px'></em></span></p></body></html>");
        // (an <aside> would close the <p> in the HTML parser, as in Firefox)
        let (result, _) = layout(&html);
        let span = box_of(&result, "span");
        let em = box_of(&result, "em");
        assert_eq!((em.0, em.1), (span.0, span.1));
        let emFrame = find_first(result.mViewport, "em").unwrap();
        assert_eq!(emFrame.GetParent(), find_first(result.mViewport, "span"));
    }

    #[test]
    fn an_absolutely_positioned_box_adds_to_the_overflow_not_the_height() {
        let html = format!("{BODY}<div style='position:relative;height:10px'><aside style='position:absolute;top:1000px;height:50px;width:5px'></aside></div></body></html>");
        let (result, _) = layout(&html);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(10.0));
        let div = find_first(result.mViewport, "div").unwrap();
        assert_eq!(div.ScrollableOverflowRect().YMost(), 1050 * PX);
    }

    #[test]
    fn display_contents_flattens_the_element() {
        // CSS Display 3: display:contents generates no box for the element, its children take part
        // in the parent's formatting context.
        let html = format!("{BODY}<div style='display:contents'><p style='margin:0;height:10px'></p><p style='margin:0;height:10px'></p></div></body></html>");
        let (result, counts) = layout(&html);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(20.0));
        assert_eq!(counts["Block"], 4, "html, body and two p: {counts:?}");
    }

    #[test]
    fn the_canvas_rounds_the_document_height_up_to_whole_pixels() {
        // nsCanvasFrame::Reflow: RoundUpAppUnitsToCSSPixel
        let html = format!("{BODY}<div style='height:10.25px'></div></body></html>");
        let (result, _) = layout(&html);
        assert_eq!(result.DocumentHeightInCSSPixels(), 11.0);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(10.25));
    }

    #[test]
    fn the_whole_frame_tree_has_the_viewport_canvas_root_shape() {
        let (result, _) = layout(&format!("{BODY}<p>x</p></body></html>"));
        assert_eq!(result.mViewport.Type(), LayoutFrameType::Viewport);
        let canvas = nsContainerFrame::from_frame(result.mViewport).mFrames().FirstChild().unwrap();
        assert_eq!(canvas, result.mCanvas);
        assert_eq!(canvas.Type(), LayoutFrameType::Canvas);
        let root = nsContainerFrame::from_frame(canvas).mFrames().FirstChild().unwrap();
        assert_eq!(Some(root), result.mRootElementFrame);
        assert!(root.GetContent().unwrap().IsHTMLElement("html"));
        assert_eq!(result.mViewport.GetSize().width, 1000 * PX);
    }

    // The first frame in the tree below `aFrame` (document order of the principal lists) whose
    // content is the HTML element `aName`.
    fn find_first(aFrame: nsIFrame, aName: nsAtom) -> Option<nsIFrame> {
        if aFrame.GetContent().map_or(false, |c| c.IsHTMLElement(aName)) && aFrame.Type() != LayoutFrameType::Placeholder {
            return Some(aFrame);
        }
        let mut lists = Vec::new();
        aFrame.GetChildLists(&mut lists);
        for l in lists {
            let mut c = l.mList.FirstChild();
            while let Some(k) = c {
                if let Some(found) = find_first(k, aName) {
                    return Some(found);
                }
                c = k.GetNextSibling();
            }
        }
        None
    }

    // All the frames of `aType` below `aFrame`, in tree order
    fn find_all(aFrame: nsIFrame, aType: LayoutFrameType) -> Vec<nsIFrame> {
        let mut result = Vec::new();
        fn walk(f: nsIFrame, t: LayoutFrameType, out: &mut Vec<nsIFrame>) {
            if f.Type() == t {
                out.push(f);
            }
            let mut lists = Vec::new();
            f.GetChildLists(&mut lists);
            for l in lists {
                let mut c = l.mList.FirstChild();
                while let Some(k) = c {
                    walk(k, t, out);
                    c = k.GetNextSibling();
                }
            }
        }
        walk(aFrame, aType, &mut result);
        result
    }
}

// Tests of the frame tree that the constructor builds for flex containers (nothing is reflowed: these
// check what Gecko's frame constructor does, not flex layout, which nsFlexContainerFrame.rs tests).
// Written from the behaviour of nsCSSFrameConstructor::CreateNeededAnonFlexOrGridItems, there is no
// gtest for it.
#[cfg(test)]
mod flex_construction_tests {
    use super::*;
    use crate::driver::*;
    use mrt_style::mrt_dom::Dom;

    const BODY: &str = "<!DOCTYPE html><html><head><style>body{margin:0}</style></head><body>";

    // Parses, styles and constructs (no reflow) the frames of `html`.
    fn construct(html: &str) -> PageLayout {
        let mut sd = StyleDom::new(Dom::parse(html), (1000.0, 800.0));
        sd.resolve();
        // the frames read their content from the DOM (see nsIContent)
        let sd = KeepDocumentAlive(sd);
        ConstructFrames(&sd, 1000, 800)
    }

    fn children(aFrame: nsIFrame) -> Vec<nsIFrame> {
        let mut out = Vec::new();
        let mut c = nsContainerFrame::from_frame(aFrame).mFrames().FirstChild();
        while let Some(k) = c {
            out.push(k);
            c = k.GetNextSibling();
        }
        out
    }

    fn find_all(aFrame: nsIFrame, aType: LayoutFrameType) -> Vec<nsIFrame> {
        fn walk(f: nsIFrame, t: LayoutFrameType, out: &mut Vec<nsIFrame>) {
            if f.Type() == t {
                out.push(f);
            }
            let mut lists = Vec::new();
            f.GetChildLists(&mut lists);
            for l in lists {
                let mut c = l.mList.FirstChild();
                while let Some(k) = c {
                    walk(k, t, out);
                    c = k.GetNextSibling();
                }
            }
        }
        let mut result = Vec::new();
        walk(aFrame, t_of(aType), &mut result);
        result
    }
    fn t_of(t: LayoutFrameType) -> LayoutFrameType {
        t
    }

    // the local name of the element a frame is for
    fn name_of(aFrame: nsIFrame) -> Option<String> {
        aFrame.GetContent().and_then(|c| c.mHTMLLocalName())
    }

    fn types(aFrames: &[nsIFrame]) -> Vec<LayoutFrameType> {
        aFrames.iter().map(|f| f.Type()).collect()
    }

    #[test]
    fn inline_children_of_a_flex_container_are_blockified_items() {
        // CSS Flexbox 1 section 4: each in-flow child is a flex item and is blockified.
        let result = construct(&format!("{BODY}<div style='display:flex'><span>a</span><span>b</span></div></body></html>"));
        let flex = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer);
        assert_eq!(flex.len(), 1);
        let items = children(flex[0]);
        assert_eq!(types(&items), vec![LayoutFrameType::Block, LayoutFrameType::Block]);
        for item in &items {
            assert!(item.IsFlexItem());
            assert!(item.StyleDisplay().IsBlockOutsideStyle());
            assert_eq!(types(&children(*item)), vec![LayoutFrameType::Text]);
        }
    }

    #[test]
    fn text_in_a_flex_container_is_wrapped_in_an_anonymous_item() {
        let result = construct(&format!("{BODY}<div style='display:flex'>hello</div></body></html>"));
        let flex = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer)[0];
        let items = children(flex);
        assert_eq!(types(&items), vec![LayoutFrameType::Block]);
        assert_eq!(types(&children(items[0])), vec![LayoutFrameType::Text]);
        // the anonymous item is a block that does not generate its own content: it has the content of the container
        assert_eq!(name_of(items[0]), name_of(flex));
        assert!(items[0].StyleDisplay().IsBlockOutsideStyle());
    }

    #[test]
    fn whitespace_between_flex_items_is_dropped() {
        let result = construct(&format!("{BODY}<div style='display:flex'> <span>a</span> <span>b</span> </div></body></html>"));
        let flex = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer)[0];
        let items = children(flex);
        // two items, no anonymous item that holds only whitespace
        assert_eq!(items.len(), 2);
        assert_eq!(find_all(flex, LayoutFrameType::Text).len(), 2);
        for item in items {
            assert_eq!(types(&children(item)), vec![LayoutFrameType::Text]);
        }
    }

    #[test]
    fn whitespace_next_to_text_is_kept_inside_the_anonymous_item() {
        // "a" and " b" with an element between them: the text runs on each side of the element
        // are separate anonymous items, the element is a flex item of its own (CreateNeededAnonFlexOrGridItems
        // wraps only contiguous wrappable children).
        let result = construct(&format!("{BODY}<div style='display:flex'>a <span>x</span> b</div></body></html>"));
        let flex = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer)[0];
        let items = children(flex);
        assert_eq!(items.len(), 3);
        assert!(items.iter().all(|i| i.Type() == LayoutFrameType::Block));
        assert!(name_of(items[0]) == name_of(flex) && name_of(items[2]) == name_of(flex));
        assert!(name_of(items[1]) != name_of(flex));
    }

    #[test]
    fn a_contiguous_run_of_text_and_line_breaks_is_one_anonymous_item() {
        // Text frames and <br> are line participants: a run of them is wrapped together in ONE item.
        let result = construct(&format!("{BODY}<div style='display:flex'>x<br>y</div></body></html>"));
        let flex = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer)[0];
        let items = children(flex);
        assert_eq!(items.len(), 1);
        assert_eq!(types(&children(items[0])), vec![LayoutFrameType::Text, LayoutFrameType::Br, LayoutFrameType::Text]);
    }

    #[test]
    fn inline_flex_is_an_inline_level_flex_container_in_a_line() {
        let result = construct(&format!("{BODY}<div>a<span style='display:inline-flex'><i>x</i></span>b</div></body></html>"));
        let flex = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer);
        assert_eq!(flex.len(), 1);
        assert!(flex[0].StyleDisplay().IsInlineOutsideStyle());
        // an atomic inline: its parent is the block that holds the line, not an inline frame
        assert_eq!(flex[0].GetParent().unwrap().Type(), LayoutFrameType::Block);
        assert_eq!(types(&children(flex[0])), vec![LayoutFrameType::Block]);
    }

    #[test]
    fn a_float_in_a_flex_container_is_an_ordinary_item() {
        // Floats do not float in a flex container (ShouldSuppressFloatingOfDescendants): the child
        // is an item and there is no placeholder.
        let result = construct(&format!("{BODY}<div style='display:flex'><div style='float:left'>f</div></div></body></html>"));
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(find_all(root, LayoutFrameType::Placeholder).len(), 0);
        let flex = find_all(root, LayoutFrameType::FlexContainer)[0];
        let items = children(flex);
        assert_eq!(types(&items), vec![LayoutFrameType::Block]);
        assert!(!items[0].HasAnyStateBits(NS_FRAME_OUT_OF_FLOW));
    }

    #[test]
    fn a_flex_root_element_is_a_flex_container() {
        let result = construct("<!DOCTYPE html><html style='display:flex'><body>x</body></html>");
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(root.Type(), LayoutFrameType::FlexContainer);
        assert_eq!(types(&children(root)), vec![LayoutFrameType::Block]);
        assert!(root.GetContent().unwrap().IsHTMLElement("html"));
    }

    #[test]
    fn flex_containers_nest() {
        let result = construct(&format!("{BODY}<div style='display:flex'><div style='display:flex'><p>a</p><p>b</p></div></div></body></html>"));
        let flex = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer);
        assert_eq!(flex.len(), 2);
        assert_eq!(flex[1].GetParent(), Some(flex[0]));
        assert_eq!(types(&children(flex[1])), vec![LayoutFrameType::Block, LayoutFrameType::Block]);
    }
}

// Tests of the frame tree that the constructor builds for grid containers (nothing is reflowed: grid layout
// is tested in nsGridContainerFrame.rs). Written from the behaviour of CreateNeededAnonFlexOrGridItems and
// CSS Grid Layout 1 section 6 (grid items), there is no gtest for it.
#[cfg(test)]
mod grid_construction_tests {
    use super::*;
    use crate::driver::*;
    use mrt_style::mrt_dom::Dom;

    const BODY: &str = "<!DOCTYPE html><html><head><style>body{margin:0}</style></head><body>";

    fn construct(html: &str) -> PageLayout {
        let mut sd = StyleDom::new(Dom::parse(html), (1000.0, 800.0));
        sd.resolve();
        // the frames read their content from the DOM (see nsIContent)
        let sd = KeepDocumentAlive(sd);
        ConstructFrames(&sd, 1000, 800)
    }

    fn children(aFrame: nsIFrame) -> Vec<nsIFrame> {
        let mut out = Vec::new();
        let mut c = nsContainerFrame::from_frame(aFrame).mFrames().FirstChild();
        while let Some(k) = c {
            out.push(k);
            c = k.GetNextSibling();
        }
        out
    }

    fn find_all(aFrame: nsIFrame, aType: LayoutFrameType) -> Vec<nsIFrame> {
        fn walk(f: nsIFrame, t: LayoutFrameType, out: &mut Vec<nsIFrame>) {
            if f.Type() == t {
                out.push(f);
            }
            let mut lists = Vec::new();
            f.GetChildLists(&mut lists);
            for l in lists {
                let mut c = l.mList.FirstChild();
                while let Some(k) = c {
                    walk(k, t, out);
                    c = k.GetNextSibling();
                }
            }
        }
        let mut result = Vec::new();
        walk(aFrame, aType, &mut result);
        result
    }

    fn name_of(aFrame: nsIFrame) -> Option<String> {
        aFrame.GetContent().and_then(|c| c.mHTMLLocalName())
    }

    fn types(aFrames: &[nsIFrame]) -> Vec<LayoutFrameType> {
        aFrames.iter().map(|f| f.Type()).collect()
    }

    #[test]
    fn a_grid_container_gets_a_grid_container_frame() {
        let result = construct(&format!("{BODY}<div style='display:grid'><p>a</p></div></body></html>"));
        let grid = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer);
        assert_eq!(grid.len(), 1);
        assert!(grid[0].IsGridContainerFrame());
        assert!(!grid[0].IsBlockFrameOrSubclass());
    }

    #[test]
    fn inline_children_of_a_grid_container_are_blockified_items() {
        // CSS Grid 1 section 6: each in-flow child is a grid item and is blockified.
        let result = construct(&format!("{BODY}<div style='display:grid'><span>a</span><span>b</span></div></body></html>"));
        let grid = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer);
        assert_eq!(grid.len(), 1);
        let items = children(grid[0]);
        assert_eq!(types(&items), vec![LayoutFrameType::Block, LayoutFrameType::Block]);
        for item in &items {
            assert!(item.IsGridItem());
            assert!(item.StyleDisplay().IsBlockOutsideStyle());
            assert_eq!(types(&children(*item)), vec![LayoutFrameType::Text]);
        }
    }

    #[test]
    fn text_in_a_grid_container_is_wrapped_in_an_anonymous_item() {
        let result = construct(&format!("{BODY}<div style='display:grid'>hello</div></body></html>"));
        let grid = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer)[0];
        let items = children(grid);
        assert_eq!(types(&items), vec![LayoutFrameType::Block]);
        assert_eq!(types(&children(items[0])), vec![LayoutFrameType::Text]);
        assert_eq!(name_of(items[0]), name_of(grid));
        assert!(items[0].IsGridItem());
    }

    #[test]
    fn whitespace_between_grid_items_is_dropped() {
        let result = construct(&format!("{BODY}<div style='display:grid'> <span>a</span> <span>b</span> </div></body></html>"));
        let grid = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer)[0];
        let items = children(grid);
        assert_eq!(items.len(), 2);
        assert_eq!(find_all(grid, LayoutFrameType::Text).len(), 2);
    }

    #[test]
    fn inline_grid_is_an_inline_level_grid_container_in_a_line() {
        let result = construct(&format!("{BODY}<div>a<span style='display:inline-grid'><i>x</i></span>b</div></body></html>"));
        let grid = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer);
        assert_eq!(grid.len(), 1);
        assert!(grid[0].StyleDisplay().IsInlineOutsideStyle());
        assert_eq!(grid[0].GetParent().unwrap().Type(), LayoutFrameType::Block);
        assert_eq!(types(&children(grid[0])), vec![LayoutFrameType::Block]);
    }

    #[test]
    fn a_float_in_a_grid_container_is_an_ordinary_item() {
        // Floats do not float in a grid container (ShouldSuppressFloatingOfDescendants).
        let result = construct(&format!("{BODY}<div style='display:grid'><div style='float:left'>f</div></div></body></html>"));
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(find_all(root, LayoutFrameType::Placeholder).len(), 0);
        let grid = find_all(root, LayoutFrameType::GridContainer)[0];
        let items = children(grid);
        assert_eq!(types(&items), vec![LayoutFrameType::Block]);
        assert!(!items[0].HasAnyStateBits(NS_FRAME_OUT_OF_FLOW));
    }

    #[test]
    fn a_grid_root_element_is_a_grid_container() {
        let result = construct("<!DOCTYPE html><html style='display:grid'><body>x</body></html>");
        let root = result.mRootElementFrame.unwrap();
        assert_eq!(root.Type(), LayoutFrameType::GridContainer);
        assert_eq!(types(&children(root)), vec![LayoutFrameType::Block]);
    }

    #[test]
    fn grid_containers_nest() {
        let result = construct(&format!("{BODY}<div style='display:grid'><div style='display:grid'><p>a</p><p>b</p></div></div></body></html>"));
        let grid = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer);
        assert_eq!(grid.len(), 2);
        assert_eq!(grid[1].GetParent(), Some(grid[0]));
        assert_eq!(types(&children(grid[1])), vec![LayoutFrameType::Block, LayoutFrameType::Block]);
    }

    #[test]
    fn a_table_in_a_grid_container_is_an_item_through_its_wrapper() {
        let result = construct(&format!("{BODY}<div style='display:grid'><table><tr><td>x</td></tr></table></div></body></html>"));
        let grid = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer)[0];
        let items = children(grid);
        assert_eq!(types(&items), vec![LayoutFrameType::TableWrapper]);
        assert!(items[0].IsGridItem());
    }
}

// ADAPTER: FlattenedChildIterator's state (see nsCSSFrameConstructor::FlattenedChildIterator): a list
// for a shadow host or a slot, the sibling links for any other node.
pub struct FlattenedChildIterator {
    mList: Option<std::vec::IntoIter<ContentId>>,
    mNext: ContentId,
}

impl FlattenedChildIterator {
    pub fn GetNextChild(&mut self, aConstructor: &nsCSSFrameConstructor) -> ContentId {
        if let Some(list) = &mut self.mList {
            return list.next().unwrap_or(NONE);
        }
        let child = self.mNext;
        if child != NONE {
            self.mNext = aConstructor.GetNextSibling(child);
        }
        child
    }
}
