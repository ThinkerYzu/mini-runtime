// Translated from layout/generic/nsBlockFrame.h and nsBlockFrame.cpp (GECKO_REV), stage 2b.
//
// Rust shape (see CONVENTIONS.md):
//  - `nsBlockFrame` is a `Copy` handle over the frame (a block is an nsIFrame of type Block) that
//    derefs to nsIFrame, as the C++ class derives from nsContainerFrame, nsSplittableFrame and
//    nsIFrame. The members of the C++ class (`mLines`, `mCachedIntrinsics`) live in one
//    heap block per frame kept behind a frame property (see nsBlockFrame_Members); the list
//    properties (floats, pushed floats, overflow lines, ...) are frame properties holding raw pointers,
//    as in Gecko. `mLines()` and `mCachedIntrinsics()` give the members as `&'static mut` (the reference does
//    not outlive the frame; a caller must not hold one across a call that reaches the same list).
//  - `nsLineList::iterator` is `nsLineList_iterator` (a raw pointer wrapper, Copy). Range-for over
//    `Lines()` is a lazy Rust iterator (`nsLineList_range`) that steps to the next line before the
//    body runs, so the body may erase the line it is on.
//  - Out parameters (`bool*`, `nscoord&`) are `&mut`. `BlockReflowState&` is `&mut BlockReflowState`.
//  - Virtual methods that Gecko dispatches on the frame class are a `match` on the frame type in the
//    adapter layer (nsIFrameLists.rs, nsLineLayoutAdapter.rs); this file has the nsBlockFrame
//    override under its own name.
//
// NOT PORTED (the whole file): fragmentation (columns, pages, overflow containers, continuations of
// floats, pushing and pulling of lines between continuations is kept only where it is also needed
// without fragmentation), bidi, ruby, ::first-letter and ::first-line frame creation, text-overflow,
// scrolling-related code, selection / accessibility / printing, display lists, debug dumps,
// absolutely positioned child reflow (call sites are stubbed), -webkit-line-clamp, text-wrap:balance
// retries, -moz-box ordinals. Each is marked "NOT PORTED" where it occurs. Refcount logging
// (MOZ_COUNT_CTOR) is not ported.
//
// TEMP stubs: everything that Gecko takes from nsContainerFrame, nsSplittableFrame, nsInlineFrame
// and nsIFrame that is not translated yet is at the end of the file under "TEMP stubs", each marked.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::{Deref, DerefMut};

// Gecko: layout/generic/nsBlockFrame.h LineReflowStatus
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineReflowStatus {
    // The line was completely reflowed and fit in available width, and we should
    // try to pull up content from the next line if possible.
    OK,
    // The line was completely reflowed and fit in available width, but we should
    // not try to pull up content from the next line.
    Stop,
    // We need to reflow the line again at its current vertical position. The
    // new reflow should not try to pull up any frames from the next line.
    RedoNoPull,
    // We need to reflow the line again using the floats from its height
    // this reflow, since its height made it hit floats that were not
    // adjacent to its top.
    RedoMoreFloats,
    // We need to reflow the line again at a lower vertical postion where there
    // may be more horizontal space due to different float configuration.
    RedoNextBand,
    // The line did not fit in the available vertical space. Try pushing it to
    // the next page or column if it's not the first line on the current
    // page/column.
    Truncated,
}

/**
 * Some invariants:
 * -- The overflow out-of-flows list contains the out-of-
 * flow frames whose placeholders are in the overflow list.
 * -- A given piece of content has at most one placeholder
 * frame in a block's normal child list.
 * -- While a block is being reflowed, and from then until
 * its next-in-flow is reflowed it may have a
 * PushedFloatProperty frame property that points to
 * an nsFrameList. This list contains continuations for
 * floats whose prev-in-flow is in the block's regular float
 * list and first-in-flows of floats that did not fit, but
 * whose placeholders are in the block or one of its
 * prev-in-flows.
 * -- In all these frame lists, if there are two frames for
 * the same content appearing in the list, then the frames
 * appear with the prev-in-flow before the next-in-flow.
 * -- While reflowing a block, its overflow line list
 * will usually be empty but in some cases will have lines
 * (while we reflow the block at its shrink-wrap width).
 * In this case any new overflowing content must be
 * prepended to the overflow lines.
 */

/*
 * Base class for block and inline frames.
 * The block frame has an additional child list, FrameChildListID::Absolute,
 * which contains the absolutely positioned frames.
 */
// Gecko: layout/generic/nsBlockFrame.h nsBlockFrame
// ADAPTER: a handle (see the notes at the top of the file). `nsBlockFrame*` that may be null is
// `Option<nsBlockFrame>`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsBlockFrame(pub nsIFrame);

impl Deref for nsBlockFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::FrameLines
pub struct nsBlockFrame_FrameLines {
    pub mLines: nsLineList,
    pub mFrames: nsFrameList,
}

impl Default for nsBlockFrame_FrameLines {
    fn default() -> nsBlockFrame_FrameLines {
        nsBlockFrame_FrameLines { mLines: nsLineList::new(), mFrames: nsFrameList::default() }
    }
}

// Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::FloatAvoidingISizeToClear
/**
 * Returns the inline size that needs to be cleared past floats for
 * blocks that avoid (i.e. cannot intersect) floats.  aState must already
 * have GetFloatAvailableSpace called on it for the block-dir position that
 * we care about (which need not be its current mBCoord)
 */
#[derive(Clone, Copy, Debug)]
pub struct nsBlockFrame_FloatAvoidingISizeToClear {
    // Note that we care about the inline-start margin but can ignore
    // the inline-end margin.
    pub marginIStart: nscoord,
    pub borderBoxISize: nscoord,
}

// ADAPTER: the members of the C++ class nsBlockFrame that are plain data (`mFrames`, the principal
// child list of nsContainerFrame, is `nsIFrame::mFrames()` in nsContainerFrame.rs). Gecko keeps them in the frame object; here they are in one heap block per frame,
// reached through a frame property that holds the pointer (the block lives as long as the frame:
// Destroy() frees it).
pub struct nsBlockFrame_Members {
    // Gecko: nsBlockFrame::mLines
    pub mLines: nsLineList,
    // Gecko: nsBlockFrame::mCachedIntrinsics
    pub mCachedIntrinsics: IntrinsicISizesCache,
}

// Gecko: layout/generic/nsBlockFrame.cpp (the frame properties declared near the top)
// (ids: this crate's own numbers, the range 6000..6099 belongs to nsBlockFrame.)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(MembersProperty, usize, 6000);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(OverflowLinesProperty, *mut nsBlockFrame_FrameLines, 6001);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(OverflowOutOfFlowsProperty, *mut nsFrameList, 6002);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(FloatsProperty, *mut nsFrameList, 6003);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(PushedFloatsProperty, *mut nsFrameList, 6004);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(OutsideMarkerProperty, *mut nsFrameList, 6005);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(InsideMarkerProperty, nsIFrame, 6006);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(LineCursorPropertyDisplay, *mut nsLineBox, 6007);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(LineCursorPropertyQuery, *mut nsLineBox, 6008);
// (public: the tests of BlockReflowState.rs set the shift. NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE in C++.)
pub fn AlignContentShift() -> FramePropertyDescriptor<nscoord> {
    FramePropertyDescriptor::new(6009)
}
// (FirstLetterProperty is nsContainerFrame's: nsContainerFrame::FirstLetterProperty() in nsContainerFrame.rs.)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(LineIteratorProperty, usize, 6011);

// Gecko: layout/generic/nsBlockFrame.cpp MIN_LINES_NEEDING_CURSOR
const MIN_LINES_NEEDING_CURSOR: i32 = 20;

// ADAPTER: `for (auto& line : block->Lines())`. Steps to the next line before yielding the current
// one, so the loop body may erase it. `.rev()` is C++'s `Reversed(Lines())`.
pub struct nsLineList_range {
    mBegin: nsLineList_iterator,
    mEnd: nsLineList_iterator,
}

impl Iterator for nsLineList_range {
    type Item = nsLineList_iterator;
    fn next(&mut self) -> Option<nsLineList_iterator> {
        if self.mBegin == self.mEnd {
            return None;
        }
        let current = self.mBegin;
        self.mBegin = self.mBegin.next_();
        Some(current)
    }
}

impl DoubleEndedIterator for nsLineList_range {
    fn next_back(&mut self) -> Option<nsLineList_iterator> {
        if self.mBegin == self.mEnd {
            return None;
        }
        self.mEnd = self.mEnd.prev();
        Some(self.mEnd)
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp ClearFloatsResult, ShapeType aliases
type ClearFloatsResult = BlockReflowState_ClearFloatsResult;

// Gecko: layout/generic/nsBlockFrame.cpp MarkAllInlineLinesDirty
fn MarkAllInlineLinesDirty(aBlock: nsBlockFrame) {
    for mut line in aBlock.Lines() {
        if line.IsInline() {
            line.MarkDirty();
        }
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp MarkAllDescendantLinesDirty
fn MarkAllDescendantLinesDirty(aBlock: nsBlockFrame) {
    for mut line in aBlock.Lines() {
        if line.IsBlock() {
            let bf = nsBlockFrame::do_QueryFrame(line.mFirstChild.unwrap());
            if let Some(bf) = bf {
                MarkAllDescendantLinesDirty(bf);
            }
        }
        line.MarkDirty();
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp MarkSameFloatManagerLinesDirty
fn MarkSameFloatManagerLinesDirty(aBlock: nsBlockFrame) {
    let mut blockWithFloatMgr = aBlock;
    while !blockWithFloatMgr.HasAnyStateBits(NS_BLOCK_BFC) {
        let bf = blockWithFloatMgr.GetParent().and_then(nsBlockFrame::do_QueryFrame);
        let Some(bf) = bf else {
            break;
        };
        blockWithFloatMgr = bf;
    }

    // Mark every line at and below the line where the float was
    // dirty, and mark their lines dirty too. We could probably do
    // something more efficient --- e.g., just dirty the lines that intersect
    // the float vertically.
    MarkAllDescendantLinesDirty(blockWithFloatMgr);
}

/**
 * Returns true if aFrame is a block that has one or more float children.
 */
// Gecko: layout/generic/nsBlockFrame.cpp BlockHasAnyFloats
fn BlockHasAnyFloats(aFrame: nsIFrame) -> bool {
    let Some(block) = nsBlockFrame::do_QueryFrame(aFrame) else {
        return false;
    };
    if block.GetChildList(FrameChildListID::Float).FirstChild().is_some() {
        return true;
    }

    for line in block.Lines() {
        if line.IsBlock() && BlockHasAnyFloats(line.mFirstChild.unwrap()) {
            return true;
        }
    }
    false
}

// Gecko: layout/generic/nsBlockFrame.cpp FrameHasVisibleInlineText
// Determines whether the given frame is visible text or has visible text that
// participate in the same line. Frames that are not line participants do not
// have their children checked.
// NOT PORTED: used by the display-list code (text-overflow / high-contrast backplates), nsBlockFrame.cpp
// lines 136-165 at GECKO_REV.

// Gecko: layout/generic/nsBlockFrame.cpp GetFrameTextArea, GetLineTextArea, GetBackplateColor
// NOT PORTED: display-list code (nsDisplayListBuilder, nsCSSRendering), nsBlockFrame.cpp lines 167-265
// at GECKO_REV.

// Gecko: layout/generic/nsBlockFrame.cpp GetNormalMarginRect
fn GetNormalMarginRect(aFrame: nsIFrame, aIncludePositiveMargins: bool) -> nsRect {
    let mut m = aFrame.GetUsedMargin();
    m.ApplySkipSides(aFrame.GetSkipSides());
    if !aIncludePositiveMargins {
        m.EnsureAtMost(nsMargin::default());
    }
    let mut rect = aFrame.GetRectRelativeToSelf();
    rect.Inflate__nsMargin(m);
    rect + aFrame.GetNormalPosition()
}

// Gecko: layout/generic/nsBlockFrame.cpp (#ifdef DEBUG) nsBlockFrame::gNoisy* flags,
// nsBlockFrame::InitDebugFlags, ShowDebugFlags, LineReflowStatus ToString, nsBlockFrame::IndentBy
// NOT PORTED: debug-only noisy-reflow flags and printing (nsBlockFrame.cpp lines 278-368 at GECKO_REV);
// the debug flags are all false, so every `if (gNoisyReflow)` block is left out (marked at its place).

// Gecko: layout/generic/nsBlockFrame.cpp RecordReflowStatus
// NOT PORTED: REFLOW_STATUS_COVERAGE is never defined.

// Gecko: layout/generic/nsBlockFrame.cpp NS_NewBlockFrame
// ADAPTER: a block frame is created by the frame tree (FrameTree::AddFrame; the type defaults to
// Block), so there is nothing to allocate: this wraps an existing frame.
pub fn NS_NewBlockFrame(aFrame: nsIFrame) -> nsBlockFrame {
    nsBlockFrame(aFrame)
}

impl nsBlockFrame {
    // ADAPTER: static_cast<nsBlockFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsBlockFrame {
        nsBlockFrame(aFrame)
    }

    // ADAPTER: do_QueryFrame<nsBlockFrame>(frame): the frame is of type Block.
    // (NS_QUERYFRAME_HEAD(nsBlockFrame): NS_QUERYFRAME_ENTRY(nsBlockFrame) and the entries of
    // the base classes.) The column-set wrapper is a subclass in Gecko but has its own type here.
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsBlockFrame> {
        if aFrame.Type() == LayoutFrameType::Block {
            Some(nsBlockFrame(aFrame))
        } else {
            None
        }
    }

    // ADAPTER: this frame's data members (see nsBlockFrame_Members). Created on first use.
    fn Members(&self) -> &'static mut nsBlockFrame_Members {
        let p = match self.0.GetProperty(MembersProperty()) {
            Some(p) => p,
            None => {
                let p = Box::into_raw(Box::new(nsBlockFrame_Members {
                    mLines: nsLineList::new(),
                    mCachedIntrinsics: IntrinsicISizesCache::default(),
                })) as usize;
                self.0.SetOwnedPointerProperty::<nsBlockFrame_Members>(MembersProperty(), p);
                p
            }
        };
        unsafe { &mut *(p as *mut nsBlockFrame_Members) }
    }

    // ADAPTER: nsBlockFrame::mLines
    pub fn mLines(&self) -> &'static mut nsLineList {
        &mut self.Members().mLines
    }

    // ADAPTER: nsBlockFrame::mCachedIntrinsics
    pub fn mCachedIntrinsics(&self) -> &'static mut IntrinsicISizesCache {
        &mut self.Members().mCachedIntrinsics
    }
}

// Gecko: layout/generic/nsBlockFrame.h nsBlockFrame (inline members, in declaration order)
impl nsBlockFrame {
    // typedef nsLineList::iterator LineIterator; ... (the typedefs are the nsLineList_* aliases)

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::LinesBegin
    pub fn LinesBegin(&self) -> nsLineList_iterator {
        self.mLines().begin_mut()
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::LinesEnd
    pub fn LinesEnd(&self) -> nsLineList_iterator {
        self.mLines().end_mut()
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::LinesRBegin
    pub fn LinesRBegin(&self) -> nsLineList_reverse_iterator {
        self.mLines().rbegin_mut()
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::LinesREnd
    pub fn LinesREnd(&self) -> nsLineList_reverse_iterator {
        self.mLines().rend_mut()
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::LinesBeginFrom
    pub fn LinesBeginFrom(&self, aList: *mut nsLineBox) -> nsLineList_iterator {
        self.mLines().begin_mut__nsLineBox(aList)
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::LinesRBeginFrom
    pub fn LinesRBeginFrom(&self, aList: *mut nsLineBox) -> nsLineList_reverse_iterator {
        self.mLines().rbegin_mut__nsLineBox(aList)
    }

    // Methods declared to be used in 'range-based-for-loop'
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::Lines
    // (ADAPTER: returns the lazy range of nsLineList_range, not the list; the list is `mLines()`.)
    pub fn Lines(&self) -> nsLineList_range {
        nsLineList_range { mBegin: self.LinesBegin(), mEnd: self.LinesEnd() }
    }

    // Line cursor methods to speed up line searching in which one query result
    // is expected to be close to the next in general. This is mainly for
    // searching line(s) containing a point. It is also used as a cache for local
    // computation. The basic idea for the former is that we set the cursor
    // property if the lines' overflowArea.InkOverflow().ys and
    // overflowArea.InkOverflow().yMosts are non-decreasing
    // (considering only non-empty overflowArea.InkOverflow()s; empty
    // overflowArea.InkOverflow()s never participate in event handling
    // or painting), and the block has sufficient number of lines. The
    // cursor property points to a "recently used" line. If we get a
    // series of requests that work on lines
    // "near" the cursor, then we can find those nearby lines quickly by
    // starting our search at the cursor.

    // We have two independent line cursors, one used for display-list building
    // and the other for a11y or other frame queries. Either or both may be
    // present at any given time. When we reflow or otherwise munge the lines,
    // both cursors will be cleared.
    // The display cursor is only created and used if the lines satisfy the non-
    // decreasing y-coordinate condition (see SetupLineCursorForDisplay comment
    // below), whereas the query cursor may be created for any block. The two
    // are separated so creating a cursor for a11y queries (eg GetRenderedText)
    // does not risk confusing the display-list building code.

    // Clear out line cursors because we're disturbing the lines (i.e., Reflow)
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::ClearLineCursors
    pub fn ClearLineCursors(&self) {
        if self.MaybeHasLineCursor() {
            self.ClearLineCursorForDisplay();
            self.ClearLineCursorForQuery();
            self.RemoveStateBits(NS_BLOCK_HAS_LINE_CURSOR);
        }
        self.ClearLineIterator();
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::ClearLineCursorForDisplay
    pub fn ClearLineCursorForDisplay(&self) {
        self.RemoveProperty(LineCursorPropertyDisplay());
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::ClearLineCursorForQuery
    pub fn ClearLineCursorForQuery(&self) {
        self.RemoveProperty(LineCursorPropertyQuery());
    }

    // Clear just the line-iterator property; this is used if we need to get a
    // LineIterator temporarily during reflow, when using a persisted iterator
    // would be invalid. So we clear the stored property immediately after use.
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::ClearLineIterator
    pub fn ClearLineIterator(&self) {
        // NOT PORTED: GetLineIterator() (selection / caret / bidi), so the property is never set.
        self.RemoveProperty(LineIteratorProperty());
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::MaybeHasLineCursor
    // Note that the NS_BLOCK_HAS_LINE_CURSOR flag does not necessarily mean the
    // cursor is present, as it covers both the "display" and "query" cursors,
    // but may remain set if they have been separately deleted. In such a case,
    // the Get* accessors will be slightly more expensive, but will still safely
    // return null if the cursor is absent.
    pub fn MaybeHasLineCursor(&self) -> bool {
        self.HasAnyStateBits(NS_BLOCK_HAS_LINE_CURSOR)
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::GetLineCursorForDisplay
    pub fn GetLineCursorForDisplay(&self) -> *mut nsLineBox {
        if self.MaybeHasLineCursor() {
            self.GetProperty(LineCursorPropertyDisplay()).unwrap_or(std::ptr::null_mut())
        } else {
            std::ptr::null_mut()
        }
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::GetLineCursorForQuery
    pub fn GetLineCursorForQuery(&self) -> *mut nsLineBox {
        if self.MaybeHasLineCursor() {
            self.GetProperty(LineCursorPropertyQuery()).unwrap_or(std::ptr::null_mut())
        } else {
            std::ptr::null_mut()
        }
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::SetLineCursorForDisplay
    pub fn SetLineCursorForDisplay(&self, aLine: *mut nsLineBox) {
        MOZ_ASSERT!(!aLine.is_null(), "must have a line");
        MOZ_ASSERT!(!self.mLines().empty(), "aLine isn't my line");
        self.SetProperty(LineCursorPropertyDisplay(), aLine);
        self.AddStateBits(NS_BLOCK_HAS_LINE_CURSOR);
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::NewLineBox (two overloads)
    pub fn NewLineBox__nsIFrame_bool(&self, aFrame: nsIFrame, aIsBlock: bool) -> *mut nsLineBox {
        NS_NewLineBox__nsIFrame_bool(Some(aFrame), aIsBlock)
    }
    pub fn NewLineBox__nsLineBox_nsIFrame_int32_t(&self, aFromLine: *mut nsLineBox, aFrame: nsIFrame, aCount: i32) -> *mut nsLineBox {
        NS_NewLineBox__nsLineBox_nsIFrame_int32_t(aFromLine, Some(aFrame), aCount)
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::FreeLineBox
    pub fn FreeLineBox(&self, aLine: *mut nsLineBox) {
        if aLine == self.GetLineCursorForDisplay() {
            self.ClearLineCursorForDisplay();
        }
        if aLine == self.GetLineCursorForQuery() {
            self.ClearLineCursorForQuery();
        }
        unsafe { (*aLine).Destroy() };
    }

    // Return true if this frame has a ::marker frame.
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::HasMarker
    pub fn HasMarker(&self) -> bool {
        self.HasAnyStateBits(NS_BLOCK_HAS_MARKER)
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::IsButtonControlFrame
    // NOT PORTED: form controls (IsInputButtonControlFrame, IsColorControlFrame,
    // IsComboboxControlFrame): this runtime has no such frames, so none is a button control.
    pub fn IsButtonControlFrame(&self) -> bool {
        false
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::IsTextInput
    // NOT PORTED: text controls (:-moz-scrolled-content under a text input frame).
    pub fn IsTextInput(&self) -> bool {
        false
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::IsSingleLineTextInput
    pub fn IsSingleLineTextInput(&self) -> bool {
        self.IsTextInput() && self.GetContent().map_or(false, |c| c.IsHTMLElement("input"))
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::IsButtonLike
    pub fn IsButtonLike(&self) -> bool {
        if self.GetContent().map_or(false, |c| c.IsAnyOfHTMLElements(&[nsGkAtoms::button])) {
            // NOTE(emilio): We need the IsAnonBox check to deal with things like the
            // :-moz-anonymous-item of a <button> with display: grid. We don't want
            // that to e.g. center its contents. But we do want the scrolled-content
            // box of a button to do it.
            // NOT PORTED: pseudo types (the servo configuration has no anonymous box pseudo
            // types): a button's frame is never an anon box here, so it is button-like.
            return true;
        }
        self.IsButtonControlFrame()
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::IsButtonOrTextInput
    pub fn IsButtonOrTextInput(&self) -> bool {
        self.IsButtonLike() || self.IsTextInput()
    }

    /** Returns the effective align-content of this frame */
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::EffectiveAlignContent
    pub fn EffectiveAlignContent(&self) -> StyleAlignFlags {
        if self.IsButtonLike() {
            return StyleAlignFlags::CENTER;
        }
        if self.IsSingleLineTextInput() {
            return StyleAlignFlags::CENTER | StyleAlignFlags::UNSAFE;
        }
        self.Style().get_position().align_content.primary()
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::IsContentAligned
    pub fn IsContentAligned(&self) -> bool {
        self.EffectiveAlignContent() != StyleAlignFlags::NORMAL
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::GetAlignContentShift
    pub fn GetAlignContentShift(&self) -> nscoord {
        if self.IsContentAligned() {
            self.GetProperty(AlignContentShift()).unwrap_or(0)
        } else {
            0
        }
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::HasLineClampEllipsis
    pub fn HasLineClampEllipsis(&self) -> bool {
        self.HasAnyStateBits(NS_BLOCK_HAS_LINE_CLAMP_ELLIPSIS)
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::HasLineClampEllipsisDescendant
    pub fn HasLineClampEllipsisDescendant(&self) -> bool {
        self.HasAnyStateBits(NS_BLOCK_HAS_LINE_CLAMP_ELLIPSIS_DESCENDANT)
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::SetHasLineClampEllipsis
    pub fn SetHasLineClampEllipsis(&self, aValue: bool) {
        self.AddOrRemoveStateBits(NS_BLOCK_HAS_LINE_CLAMP_ELLIPSIS, aValue);
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::SetHasLineClampEllipsisDescendant
    pub fn SetHasLineClampEllipsisDescendant(&self, aValue: bool) {
        self.AddOrRemoveStateBits(NS_BLOCK_HAS_LINE_CLAMP_ELLIPSIS_DESCENDANT, aValue);
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::IsInLineClampContext
    pub fn IsInLineClampContext(&self) -> bool {
        self.GetLineClampRoot().is_some()
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::HasOverflowLines
    pub fn HasOverflowLines(&self) -> bool {
        self.HasAnyStateBits(NS_BLOCK_HAS_OVERFLOW_LINES)
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::CollectFloats
    pub fn CollectFloats(&self, aFrame: nsIFrame, aList: &mut nsFrameList, aCollectFromSiblings: bool) {
        if self.MaybeHasFloats() {
            self.DoCollectFloats(aFrame, aList, aCollectFromSiblings);
        }
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::GetMarker
    // Return the ::marker frame or nullptr if we don't have one.
    pub fn GetMarker(&self) -> Option<nsIFrame> {
        let outside = self.GetOutsideMarker();
        if outside.is_some() {
            outside
        } else {
            self.GetInsideMarker()
        }
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp (the code from NS_NewBlockFrame to the end of the line
// clamp helpers, in order)
impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::~nsBlockFrame (= default)
    // NOT PORTED: nothing to do here: the members that own data are released by Destroy().

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::AddSizeOfExcludingThisForTree
    // NOT PORTED: memory reporting (nsBlockFrame.cpp lines 424-440 at GECKO_REV).

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::Destroy
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        self.ClearLineCursors();
        self.DestroyAbsoluteFrames(aContext);
        let presContext = self.PresContext();
        // mozilla::PresShell* presShell = presContext->PresShell();
        // NOT PORTED: the PresShell (arena allocation); frames and lists are heap objects here.
        if self.HasFloats() {
            self.SafelyDestroyFrameListProp(aContext, FloatsProperty());
            self.RemoveStateBits(NS_BLOCK_HAS_FLOATS);
        }
        nsLineBox::DeleteLineList(presContext, self.mLines(), &mut *self.mFrames(), aContext);

        if self.HasPushedFloats() {
            self.SafelyDestroyFrameListProp(aContext, PushedFloatsProperty());
            self.RemoveStateBits(NS_BLOCK_HAS_PUSHED_FLOATS);
        }

        // destroy overflow lines now
        let overflowLines = self.RemoveOverflowLines();
        if !overflowLines.is_null() {
            let mut overflowLines = unsafe { Box::from_raw(overflowLines) };
            nsLineBox::DeleteLineList(presContext, &mut overflowLines.mLines, &mut overflowLines.mFrames, aContext);
            drop(overflowLines);
        }

        if self.HasAnyStateBits(NS_BLOCK_HAS_OVERFLOW_OUT_OF_FLOWS) {
            self.SafelyDestroyFrameListProp(aContext, OverflowOutOfFlowsProperty());
            self.RemoveStateBits(NS_BLOCK_HAS_OVERFLOW_OUT_OF_FLOWS);
        }

        if self.HasMarker() {
            self.SafelyDestroyFrameListProp(aContext, OutsideMarkerProperty());
            self.RemoveStateBits(NS_BLOCK_HAS_MARKER);
        }

        self.nsContainerFrame_Destroy(aContext);

        // ADAPTER: free the member block (see nsBlockFrame_Members); the lists in it are empty now.
        if let Some(p) = self.GetProperty(MembersProperty()) {
            drop(unsafe { Box::from_raw(p as *mut nsBlockFrame_Members) });
            self.RemoveProperty(MembersProperty());
        }
    }

    /* virtual */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetLineIterator
    // NOT PORTED: the line iterator is used by selection, the caret and bidi (nsILineIterator is
    // translated in nsILineIterator.rs / nsLineBox.rs, but nothing calls it here).
    pub fn GetLineIterator(&self) -> ! {
        unimplemented!("NOT PORTED: nsBlockFrame::GetLineIterator (selection / caret / bidi)")
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::List
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetFrameName
    // NOT PORTED: frame dumps (DEBUG_FRAME_DUMP), nsBlockFrame.cpp lines 496-541 at GECKO_REV.

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::InvalidateFrame
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::InvalidateFrameWithRect
    // NOT PORTED: invalidation (paint); nothing is painted (nsBlockFrame.cpp lines 543-565 at GECKO_REV).

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::SynthesizeFallbackBaseline
    pub fn SynthesizeFallbackBaseline(&self, aWM: WritingMode, aBaselineGroup: BaselineSharingGroup) -> nscoord {
        if self.IsButtonOrTextInput() && self.StyleDisplay().IsInlineOutsideStyle() {
            return Baseline::SynthesizeBOffsetFromContentBox(self.0, aWM, aBaselineGroup);
        }
        Baseline::SynthesizeBOffsetFromMarginBox(self.0, aWM, aBaselineGroup)
    }

    // template <typename LineIteratorType> GetBaselineBOffset(aStart, aEnd, ...)
    // The two instantiations of the C++ template are the forward and the reverse iterators; the
    // Rust function takes any iterator of nsLineList_iterator (`nsLineList_range` or its `.rev()`),
    // and the direction check of the MOZ_ASSERT below is on aBaselineGroup.
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetBaselineBOffset
    fn GetBaselineBOffset<LineIteratorType: Iterator<Item = nsLineList_iterator>>(
        &self,
        aLines: LineIteratorType,
        aWM: WritingMode,
        aBaselineGroup: BaselineSharingGroup,
        aExportContext: BaselineExportContext,
    ) -> Option<nscoord> {
        for line in aLines {
            if !line.IsBlock() {
                // XXX Is this the right test?  We have some bogus empty lines
                // floating around, but IsEmpty is perhaps too weak.
                if line.BSize() != 0 || !line.IsEmpty() {
                    let ascent = line.BStart() + line.GetLogicalAscent();
                    if aBaselineGroup == BaselineSharingGroup::Last {
                        return Some(self.BSize(aWM) - ascent);
                    }
                    return Some(ascent);
                }
                continue;
            }
            let kid = line.mFirstChild.unwrap();
            if aWM.IsOrthogonalTo(&kid.GetWritingMode()) {
                continue;
            }
            if aExportContext == BaselineExportContext::LineLayout && kid.IsTableWrapperFrame() {
                // `<table>` in inline-block context does not export any baseline.
                continue;
            }
            let kidBaselineGroup = if aExportContext == BaselineExportContext::LineLayout { kid.GetDefaultBaselineSharingGroup() } else { aBaselineGroup };
            let kidBaseline = kid.GetNaturalBaselineBOffset(aWM, kidBaselineGroup, aExportContext);
            let Some(kidBaseline) = kidBaseline else {
                continue;
            };
            let mut result = kidBaseline;
            if kidBaselineGroup == BaselineSharingGroup::Last {
                result = kid.BSize(aWM) - result;
            }
            // Ignore relative positioning for baseline calculations.
            let sz = line.mContainerSize;
            result += kid.GetLogicalNormalPosition(aWM, sz).B(aWM);
            if aBaselineGroup == BaselineSharingGroup::Last {
                return Some(self.BSize(aWM) - result);
            }
            return Some(result);
        }
        None
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetNaturalBaselineBOffset
    pub fn GetNaturalBaselineBOffset(&self, aWM: WritingMode, aBaselineGroup: BaselineSharingGroup, aExportContext: BaselineExportContext) -> Option<nscoord> {
        if self.StyleDisplay().IsContainLayout() {
            return None;
        }

        let mut offset: Option<nscoord> = if aBaselineGroup == BaselineSharingGroup::First {
            self.GetBaselineBOffset(self.Lines(), aWM, aBaselineGroup, aExportContext)
        } else {
            self.GetBaselineBOffset(self.Lines().rev(), aWM, aBaselineGroup, aExportContext)
        };
        if offset.is_none() && self.IsButtonOrTextInput() {
            for line in self.Lines().rev() {
                if line.IsEmpty() {
                    continue;
                }
                // Buttons use the end of the content as a baseline if we haven't found
                // one yet.
                let bEnd = line.BEnd();
                offset = Some(if aBaselineGroup == BaselineSharingGroup::Last { self.BSize(aWM) - bEnd } else { bEnd });
                break;
            }
        }
        offset
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetCaretBaseline
    // NOT PORTED: the caret (editing / selection).
    pub fn GetCaretBaseline(&self) -> nscoord {
        let wm = self.GetWritingMode();
        if !self.mLines().empty() {
            let line = self.LinesBegin();
            if !line.IsEmpty() {
                if line.IsBlock() {
                    return self.GetLogicalUsedBorderAndPadding(wm).BStart(wm) + line.mFirstChild.unwrap().GetCaretBaseline();
                }
                return line.BStart() + line.GetLogicalAscent();
            }
        }
        self.GetFontMetricsDerivedCaretBaseline(self.ContentBSize(wm))
    }

    /////////////////////////////////////////////////////////////////////////////
    // Child frame enumeration

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetChildList
    pub fn GetChildList(&self, aListID: FrameChildListID) -> &'static nsFrameList {
        match aListID {
            FrameChildListID::Principal => self.mFrames().as_static(),
            FrameChildListID::Overflow => {
                let overflowLines = self.GetOverflowLines();
                if !overflowLines.is_null() {
                    unsafe { &(*overflowLines).mFrames }
                } else {
                    nsFrameList::EmptyList()
                }
            }
            FrameChildListID::OverflowOutOfFlow => {
                let list = self.GetOverflowOutOfFlows();
                if !list.is_null() { unsafe { &*list } } else { nsFrameList::EmptyList() }
            }
            FrameChildListID::Float => {
                let list = self.GetFloats();
                if !list.is_null() { unsafe { &*list } } else { nsFrameList::EmptyList() }
            }
            FrameChildListID::PushedFloats => {
                let list = self.GetPushedFloats();
                if !list.is_null() { unsafe { &*list } } else { nsFrameList::EmptyList() }
            }
            FrameChildListID::Marker => {
                let list = self.GetOutsideMarkerList();
                if !list.is_null() { unsafe { &*list } } else { nsFrameList::EmptyList() }
            }
            _ => self.nsContainerFrame_GetChildList(aListID),
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetChildLists
    pub fn GetChildLists(&self, aLists: &mut Vec<FrameChildList>) {
        self.nsContainerFrame_GetChildLists(aLists);
        let overflowLines = self.GetOverflowLines();
        if !overflowLines.is_null() {
            unsafe { &(*overflowLines).mFrames }.AppendIfNonempty(aLists, FrameChildListID::Overflow);
        }
        let list = self.GetOverflowOutOfFlows();
        if !list.is_null() {
            unsafe { &*list }.AppendIfNonempty(aLists, FrameChildListID::OverflowOutOfFlow);
        }
        let list = self.GetOutsideMarkerList();
        if !list.is_null() {
            unsafe { &*list }.AppendIfNonempty(aLists, FrameChildListID::Marker);
        }
        let list = self.GetFloats();
        if !list.is_null() {
            unsafe { &*list }.AppendIfNonempty(aLists, FrameChildListID::Float);
        }
        let list = self.GetPushedFloats();
        if !list.is_null() {
            unsafe { &*list }.AppendIfNonempty(aLists, FrameChildListID::PushedFloats);
        }
    }

    /* virtual */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::IsFloatContainingBlock
    pub fn IsFloatContainingBlock(&self) -> bool {
        true
    }
}

/**
 * Remove the first line from aFromLines and adjust the associated frame list
 * aFromFrames accordingly.  The removed line is assigned to *aOutLine and
 * a frame list with its frames is assigned to *aOutFrames, i.e. the frames
 * that were extracted from the head of aFromFrames.
 * aFromLines must contain at least one line, the line may be empty.
 * @return true if aFromLines becomes empty
 */
// Gecko: layout/generic/nsBlockFrame.cpp RemoveFirstLine
fn RemoveFirstLine(aFromLines: &mut nsLineList, aFromFrames: &mut nsFrameList, aOutLine: &mut *mut nsLineBox, aOutFrames: &mut nsFrameList) -> bool {
    let mut removedLine = aFromLines.begin_mut();
    *aOutLine = removedLine.get_mut();
    let next = aFromLines.erase(removedLine);
    let isLastLine = next == aFromLines.end_mut();
    let firstFrameInNextLine = if isLastLine { None } else { next.mFirstChild };
    *aOutFrames = aFromFrames.TakeFramesBefore(firstFrameInNextLine);
    isLastLine
}

//////////////////////////////////////////////////////////////////////
// Reflow methods

impl nsBlockFrame {
    /* virtual */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::MarkIntrinsicISizesDirty
    pub fn MarkIntrinsicISizesDirty(&self) {
        let dirtyBlock = nsBlockFrame::from_frame(self.FirstContinuation());
        dirtyBlock.mCachedIntrinsics().Clear();
        if !self.HasAnyStateBits(NS_BLOCK_NEEDS_BIDI_RESOLUTION) {
            let mut frame = Some(dirtyBlock.0);
            while let Some(f) = frame {
                f.AddStateBits(NS_BLOCK_NEEDS_BIDI_RESOLUTION);
                frame = f.GetNextContinuation();
            }
        }

        self.nsContainerFrame_MarkIntrinsicISizesDirty();
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::CheckIntrinsicCacheAgainstShrinkWrapState
    fn CheckIntrinsicCacheAgainstShrinkWrapState(&self) {
        let presContext = self.PresContext();
        if !nsLayoutUtils::FontSizeInflationEnabled(&presContext) {
            return;
        }
        let inflationEnabled = !presContext.mInflationDisabledForShrinkWrap();
        if inflationEnabled != self.HasAnyStateBits(NS_BLOCK_INTRINSICS_INFLATED) {
            self.mCachedIntrinsics().Clear();
            self.AddOrRemoveStateBits(NS_BLOCK_INTRINSICS_INFLATED, inflationEnabled);
        }
    }

    // Whether this line is indented by the text-indent amount.
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::TextIndentAppliesTo
    fn TextIndentAppliesTo(&self, aLine: &nsLineList_iterator) -> bool {
        let textIndent = nsStyleTextAdapter::of(self.0).mTextIndent();

        let isFirstLineOrAfterHardBreak = (|| {
            if *aLine != self.LinesBegin() {
                // If not the first line of the block, but 'each-line' is in effect,
                // check if the previous line was not wrapped.
                return textIndent.each_line && !aLine.prev().IsLineWrapped();
            }
            if let Some(prevBlock) = self.GetPrevInFlow().and_then(nsBlockFrame::do_QueryFrame) {
                // There's a prev-in-flow, so this only counts as a first-line if
                // 'each-line' and the prev-in-flow's last line was not wrapped.
                return textIndent.each_line && (prevBlock.mLines().empty() || !prevBlock.LinesEnd().prev().IsLineWrapped());
            }
            true
        })();

        // The 'hanging' option inverts which lines are/aren't indented.
        isFirstLineOrAfterHardBreak != textIndent.hanging
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        let firstCont = self.FirstContinuation();
        if firstCont != self.0 {
            return firstCont.IntrinsicISize(aInput, aType);
        }

        self.CheckIntrinsicCacheAgainstShrinkWrapState();

        // mCachedIntrinsics.GetOrSet(*this, aType, aInput, [&] { ... })
        // (the closure runs MinISize/PrefISize, which reach mCachedIntrinsics of other blocks only,
        // never this cache's borrow, so the cache is moved out while it computes.)
        let mut cache = std::mem::take(self.mCachedIntrinsics());
        let result = cache.GetOrSet(self.0, aType, aInput, || if aType == IntrinsicISizeType::MinISize { self.MinISize(aInput) } else { self.PrefISize(aInput) });
        *self.mCachedIntrinsics() = cache;
        result
    }

    /* virtual */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::MinISize
    fn MinISize(&self, aInput: &IntrinsicSizeInput) -> nscoord {
        if let Some(containISize) = self.ContainIntrinsicISize() {
            return containISize;
        }

        // #ifdef DEBUG if (gNoisyIntrinsic) {...}  NOT PORTED: debug printing.

        let mut curFrame = Some(*self);
        while let Some(cf) = curFrame {
            cf.LazyMarkLinesDirty();
            curFrame = cf.GetNextContinuation().map(nsBlockFrame::from_frame);
        }

        if self.HasAnyStateBits(NS_BLOCK_NEEDS_BIDI_RESOLUTION) && self.PresContext().BidiEnabled() {
            self.ResolveBidi();
        }

        let whiteSpaceCanWrap = nsStyleTextAdapter::of(self.0).WhiteSpaceCanWrapStyle();
        let mut data = InlineMinISizeData::default();
        let mut curFrame = Some(*self);
        while let Some(cf) = curFrame {
            let mut line = cf.LinesBegin();
            let line_end = cf.LinesEnd();
            while line != line_end {
                // #ifdef DEBUG if (gNoisyIntrinsic) {...}  NOT PORTED: debug printing.
                if line.IsBlock() {
                    data.ForceBreak();
                    let kid = line.mFirstChild.unwrap();
                    let kidInput = IntrinsicSizeInput::new__IntrinsicSizeInput_WritingMode_WritingMode(aInput, kid.GetWritingMode(), self.GetWritingMode());
                    data.mCurrentLine = nsLayoutUtils::IntrinsicForContainer(kidInput.mContext, kid, IntrinsicISizeType::MinISize, kidInput.mPercentageBasisForChildren, 0, &StyleSizeOverrides::default());
                    data.ForceBreak();
                } else {
                    if cf.GetPrevContinuation().is_none() && self.TextIndentAppliesTo(&line) {
                        data.mCurrentLine += nsStyleTextAdapter::of(self.0).mTextIndent().length.Resolve(0);
                    }
                    data.mLine = Some(line);
                    data.SetLineContainer(Some(cf.0));
                    for kid in line.ChildFrames() {
                        let kid = kid;
                        let kidInput = IntrinsicSizeInput::new__IntrinsicSizeInput_WritingMode_WritingMode(aInput, kid.GetWritingMode(), self.GetWritingMode());
                        kid.AddInlineMinISize(&kidInput, &mut data);
                        if whiteSpaceCanWrap && data.mTrailingWhitespace != 0 {
                            data.OptionallyBreak(0);
                        }
                    }
                }
                // #ifdef DEBUG if (gNoisyIntrinsic) {...}  NOT PORTED: debug printing.
                line.operator_inc();
            }
            curFrame = cf.GetNextContinuation().map(nsBlockFrame::from_frame);
        }
        data.ForceBreak();
        data.mPrevLines
    }

    /* virtual */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::PrefISize
    fn PrefISize(&self, aInput: &IntrinsicSizeInput) -> nscoord {
        if let Some(containISize) = self.ContainIntrinsicISize() {
            return containISize;
        }

        // #ifdef DEBUG if (gNoisyIntrinsic) {...}  NOT PORTED: debug printing.

        let mut curFrame = Some(*self);
        while let Some(cf) = curFrame {
            cf.LazyMarkLinesDirty();
            curFrame = cf.GetNextContinuation().map(nsBlockFrame::from_frame);
        }

        if self.HasAnyStateBits(NS_BLOCK_NEEDS_BIDI_RESOLUTION) && self.PresContext().BidiEnabled() {
            self.ResolveBidi();
        }
        let mut data = InlinePrefISizeData::default();
        let mut curFrame = Some(*self);
        while let Some(cf) = curFrame {
            let mut line = cf.LinesBegin();
            let line_end = cf.LinesEnd();
            while line != line_end {
                // #ifdef DEBUG if (gNoisyIntrinsic) {...}  NOT PORTED: debug printing.
                if line.IsBlock() {
                    let kid = line.mFirstChild.unwrap();
                    let clearType: UsedClear;
                    if !data.mLineIsEmpty || nsBlockFrame::BlockCanIntersectFloats(kid) {
                        clearType = UsedClear::Both;
                    } else {
                        clearType = kid.StyleDisplay().UsedClear(self.GetWritingMode());
                    }
                    data.ForceBreak(clearType);
                    let kidInput = IntrinsicSizeInput::new__IntrinsicSizeInput_WritingMode_WritingMode(aInput, kid.GetWritingMode(), self.GetWritingMode());
                    data.mCurrentLine = nsLayoutUtils::IntrinsicForContainer(kidInput.mContext, kid, IntrinsicISizeType::PrefISize, kidInput.mPercentageBasisForChildren, 0, &StyleSizeOverrides::default());
                    data.ForceBreak(UsedClear::Both);
                } else {
                    if cf.GetPrevContinuation().is_none() && self.TextIndentAppliesTo(&line) {
                        let indent = nsStyleTextAdapter::of(self.0).mTextIndent().length.Resolve(0);
                        data.mCurrentLine += indent;
                        // XXXmats should the test below be indent > 0?
                        if indent != 0 {
                            data.mLineIsEmpty = false;
                        }
                    }
                    data.mLine = Some(line);
                    data.SetLineContainer(Some(cf.0));
                    for kid in line.ChildFrames() {
                        let kidInput = IntrinsicSizeInput::new__IntrinsicSizeInput_WritingMode_WritingMode(aInput, kid.GetWritingMode(), self.GetWritingMode());
                        kid.AddInlinePrefISize(&kidInput, &mut data);
                    }
                }
                // #ifdef DEBUG if (gNoisyIntrinsic) {...}  NOT PORTED: debug printing.
                line.operator_inc();
            }
            curFrame = cf.GetNextContinuation().map(nsBlockFrame::from_frame);
        }
        data.ForceBreak(UsedClear::Both);
        data.mPrevLines
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ComputeTightBounds
    // NOT PORTED: tight bounds (painting / SVG text / getBoundingClientRect-like queries).

    /* virtual */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetPrefWidthTightBounds
    // NOT PORTED: only MathML token elements use it (nsBlockFrame.cpp lines 986-1039 at GECKO_REV).
    pub fn GetPrefWidthTightBounds(&self, aRenderingContext: Option<gfxContext>, aX: &mut nscoord, aXMost: &mut nscoord) -> nsresult {
        unimplemented!("NOT PORTED: nsBlockFrame::GetPrefWidthTightBounds (MathML)")
    }
}

/**
 * Return whether aNewAvailableSpace is smaller *on either side*
 * (inline-start or inline-end) than aOldAvailableSpace, so that we know
 * if we need to redo layout on an line, replaced block, or block
 * formatting context, because its height (which we used to compute
 * aNewAvailableSpace) caused it to intersect additional floats.
 */
// Gecko: layout/generic/nsBlockFrame.cpp AvailableSpaceShrunk
fn AvailableSpaceShrunk(aWM: WritingMode, aOldAvailableSpace: &LogicalRect, aNewAvailableSpace: &LogicalRect, aCanGrow: bool /* debug-only */) -> bool {
    if aNewAvailableSpace.ISize(aWM) == 0 {
        // Positions are not significant if the inline size is zero.
        return aOldAvailableSpace.ISize(aWM) != 0;
    }
    if aCanGrow {
        NS_ASSERTION!(
            aNewAvailableSpace.IStart(aWM) <= aOldAvailableSpace.IStart(aWM) || aNewAvailableSpace.IEnd(aWM) <= aOldAvailableSpace.IEnd(aWM),
            "available space should not shrink on the start side and grow on the end side"
        );
        NS_ASSERTION!(
            aNewAvailableSpace.IStart(aWM) >= aOldAvailableSpace.IStart(aWM) || aNewAvailableSpace.IEnd(aWM) >= aOldAvailableSpace.IEnd(aWM),
            "available space should not grow on the start side and shrink on the end side"
        );
    } else {
        NS_ASSERTION!(
            aOldAvailableSpace.IStart(aWM) <= aNewAvailableSpace.IStart(aWM) && aOldAvailableSpace.IEnd(aWM) >= aNewAvailableSpace.IEnd(aWM),
            "available space should never grow"
        );
    }
    // Have we shrunk on either side?
    aNewAvailableSpace.IStart(aWM) > aOldAvailableSpace.IStart(aWM) || aNewAvailableSpace.IEnd(aWM) < aOldAvailableSpace.IEnd(aWM)
}

/**
 * Returns aFrame if it is an in-flow, non-BFC block frame, and null otherwise.
 *
 * This is used to determine whether to recurse into aFrame when applying
 * -webkit-line-clamp.
 */
// Gecko: layout/generic/nsBlockFrame.cpp GetAsLineClampDescendant (two overloads, one const)
fn GetAsLineClampDescendant(aFrame: nsIFrame) -> Option<nsBlockFrame> {
    let block = nsBlockFrame::do_QueryFrame(aFrame);
    match block {
        Some(block) if !block.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW | NS_BLOCK_BFC) => Some(block),
        _ => None,
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp IsLineClampRoot
// NOT PORTED: -webkit-line-clamp (nsBlockFrame.cpp lines 1097-1131 at GECKO_REV). Nothing is a
// line-clamp root.
fn IsLineClampRoot(aFrame: nsBlockFrame) -> bool {
    false
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetLineClampRoot
    // NOT PORTED: -webkit-line-clamp (the body is the walk up through GetAsLineClampDescendant to an
    // IsLineClampRoot block, nsBlockFrame.cpp lines 1133-1148 at GECKO_REV; no block is a root).
    pub fn GetLineClampRoot(&self) -> Option<nsBlockFrame> {
        if IsLineClampRoot(*self) {
            return Some(*self);
        }
        let mut cur = Some(*self);
        while let Some(c) = cur {
            if GetAsLineClampDescendant(c.0).is_none() {
                break;
            }
            cur = c.GetParent().and_then(nsBlockFrame::do_QueryFrame);
            let Some(c) = cur else {
                break;
            };
            if IsLineClampRoot(c) {
                return Some(c);
            }
        }
        None
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::MaybeHasFloats
    pub fn MaybeHasFloats(&self) -> bool {
        if self.HasFloats() {
            return true;
        }
        if self.HasPushedFloats() {
            return true;
        }
        // For the OverflowOutOfFlowsProperty I think we do enforce that, but it's
        // a mix of out-of-flow frames, so that's why the method name has "Maybe".
        self.HasAnyStateBits(NS_BLOCK_HAS_OVERFLOW_OUT_OF_FLOWS)
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp LineClampLineIterator, ClearLineClampEllipsis (static)
// NOT PORTED: -webkit-line-clamp (nsBlockFrame.cpp lines 1162-1276 at GECKO_REV). GetLineClampRoot is
// always null, so these have no callers that do anything.
fn ClearLineClampEllipsis_static(aFrame: nsBlockFrame) -> bool {
    false
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ClearLineClampEllipsis
    pub fn ClearLineClampEllipsis(&self) {
        ClearLineClampEllipsis_static(*self);
    }
}

// Compute an inline absolute containing block's rect in aInlineFrame's
// coordinate space (relative to its border-box origin) per
// https://drafts.csswg.org/css-position-3/#absolute-cb
// Gecko: layout/generic/nsBlockFrame.cpp ComputeInlineAbsoluteCBRect
fn ComputeInlineAbsoluteCBRect(aInlineFrame: nsIFrame) -> nsRect {
    MOZ_ASSERT!(aInlineFrame.IsAbsoluteContainer(), "Why computing the rect if it is not an absolute container?");

    let cbwm = aInlineFrame.GetWritingMode();

    // The LogicalRect API requires a container size to compute logical
    // coordinates whose "start" side is on the far physical side of the container
    // (RTL or vertical-rl writing modes). For our usage here, we don't need a
    // real container size, because that only influences the position of the
    // logical rect (which is an intermediate variable in this function). The
    // container size ultimately gets canceled out when we convert the logical
    // rect back to a physical rect at the end of this function, so we use a
    // zero-initialized dummy size.
    let dummyContainerSize = nsSize::default();

    // A helper to get aFrame's border-box rect relative to aInlineFrame, in
    // aInlineFrame's writing mode.
    let BorderBoxRectRelativeToInlineFrame = |aFrame: nsIFrame| {
        let physicalRect = aFrame.GetRectRelativeToSelf() + aFrame.GetOffsetTo(aInlineFrame);
        LogicalRect::new__WritingMode_nsRect_nsSize(cbwm, physicalRect, dummyContainerSize)
    };

    let firstCont = nsLayoutUtils::FirstContinuationOrIBSplitSibling(aInlineFrame);
    let lastCont = nsLayoutUtils::LastContinuationOrIBSplitSibling(aInlineFrame);
    let firstContRect = BorderBoxRectRelativeToInlineFrame(firstCont);
    let lastContRect = BorderBoxRectRelativeToInlineFrame(lastCont);

    // Start edges from the first continuation, end edges from the last
    // continuation.
    let iStart = firstContRect.IStart(cbwm);
    let bStart = firstContRect.BStart(cbwm);
    let iEnd = lastContRect.IEnd(cbwm);
    let bEnd = lastContRect.BEnd(cbwm);
    let mut cbRect = LogicalRect::new(cbwm, iStart, bStart, iEnd - iStart, bEnd - bStart);

    // The CSS Position 3 spec says to form the cb rect from content edges of
    // continuations [1], but other browsers and the unfragmented scenario defined
    // in CSS 2.2 section 10.1.4 [2] use the padding edge. Therefore, we deflate
    // only the border for interop.
    // https://github.com/w3c/csswg-drafts/issues/13952
    //
    // [1] https://drafts.csswg.org/css-position-3/#absolute-cb
    // [2] https://www.w3.org/TR/CSS22/visudet.html#containing-block-details
    let firstBorder = firstCont.GetLogicalUsedBorder(cbwm);
    let lastBorder = lastCont.GetLogicalUsedBorder(cbwm);
    let cbBorder = LogicalMargin::new(cbwm, firstBorder.BStart(cbwm), lastBorder.IEnd(cbwm), lastBorder.BEnd(cbwm), firstBorder.IStart(cbwm));
    cbRect.Deflate__WritingMode_LogicalMargin(cbwm, cbBorder);

    cbRect.GetPhysicalRect(cbwm, dummyContainerSize)
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ReflowAbsoluteDescendantsInInlineFrame
    pub fn ReflowAbsoluteDescendantsInInlineFrame(&self, aPresContext: &nsPresContext, aReflowInput: &ReflowInput, aReflowOutput: &mut ReflowOutput, aStatus: &mut nsReflowStatus) {
        let mut foundAbspos = false;
        for mut line in self.Lines() {
            if line.IsBlock() {
                // The block frame in this line is responsible for reflowing its abspos
                // descendants.
                continue;
            }

            // Traverse the kids on this inline line, and translate each kid's overflow
            // areas into this block's coordinate space and accumulate.
            let mut lineAbsposOverflow = OverflowAreas::default();
            for kid in line.ChildFrames() {
                if let Some(kidOverflow) = self.WalkInlineDescendantsToReflowAbsoluteFrames(kid, aPresContext, aReflowInput, aStatus) {
                    foundAbspos = true;
                    lineAbsposOverflow.UnionWithAbsoluteOverflowAreas(&(kidOverflow + kid.GetPosition()));
                }
            }

            if lineAbsposOverflow != OverflowAreas::default() {
                // Update the line's overflow areas to include abspos descendants'
                // overflow areas. This is required for painting code to generate their
                // display items.
                let mut lineOverflow = line.GetOverflowAreas();
                lineOverflow.UnionWithAbsoluteOverflowAreas(&lineAbsposOverflow);
                line.SetOverflowAreas(&lineOverflow);

                // Update this block frame's output overflow areas, too.
                aReflowOutput.mOverflowAreas.UnionWithAbsoluteOverflowAreas(&lineAbsposOverflow);
            }
        }

        if !foundAbspos {
            // Remove NS_BLOCK_HAS_INLINE_ABSPOS_DESCENDANT bit if we didn't find any
            // abspos descendants after a full walk over all our inline lines.
            self.RemoveStateBits(NS_BLOCK_HAS_INLINE_ABSPOS_DESCENDANT);
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::WalkInlineDescendantsToReflowAbsoluteFrames
    pub fn WalkInlineDescendantsToReflowAbsoluteFrames(&self, aFrame: nsIFrame, aPresContext: &nsPresContext, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) -> Option<OverflowAreas> {
        if aFrame.IsBlockFrameOrSubclass() {
            // Block frames (e.g. 'inline-block') will walk their own inline
            // descendants.
            return None;
        }

        // absposOverflow accumulates overflow areas from the visited abspos
        // descendants, in aFrame's coordinate space.
        let mut absposOverflow = OverflowAreas::default();
        let mut foundAbspos = false;

        // Traverse aFrame's kids first. Each kid will give us back the overflow areas
        // from its own abspos descendants. We translate those to aFrame's coordinate
        // space, and accumulate.
        let mut kid = aFrame.PrincipalChildList_FirstChild();
        while let Some(k) = kid {
            if let Some(absposOverflowFromKid) = self.WalkInlineDescendantsToReflowAbsoluteFrames(k, aPresContext, aReflowInput, aStatus) {
                foundAbspos = true;
                absposOverflow.UnionWithAbsoluteOverflowAreas(&(absposOverflowFromKid + k.GetPosition()));
            }
            kid = k.GetNextSibling();
        }

        // (nsInlineFrame* inlineFrame = do_QueryFrame(aFrame): nsInlineFrame and its subclasses)
        if matches!(aFrame.Type(), LayoutFrameType::Inline | LayoutFrameType::Line) {
            // If aFrame is an inline frame (or a subclass of inline frame), reflow its
            // abspos kids, and accumulate their overflow areas.
            if let Some(absposOverflowFromInlineFrame) = self.ReflowAbsoluteFramesInInlineFrame(aFrame, aPresContext, aReflowInput, aStatus) {
                foundAbspos = true;
                absposOverflow.UnionWithAbsoluteOverflowAreas(&absposOverflowFromInlineFrame);
            }
        }

        if !foundAbspos {
            return None;
        }

        if absposOverflow != OverflowAreas::default() {
            // Update aFrame's overflow areas via FinishAndStoreOverflow() so that
            // painting related properties are set correctly, e.g. setting
            // PreEffectsBBoxProperty() in ComputeEffectsRect().
            let mut frameOverflow = aFrame.GetOverflowAreas();
            frameOverflow.UnionWithAbsoluteOverflowAreas(&absposOverflow);
            aFrame.FinishAndStoreOverflow(&mut frameOverflow, aFrame.GetSize());
        }

        Some(absposOverflow)
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ReflowAbsoluteFramesInInlineFrame
    pub fn ReflowAbsoluteFramesInInlineFrame(&self, aInlineFrame: nsIFrame, aPresContext: &nsPresContext, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) -> Option<OverflowAreas> {
        let absCB = aInlineFrame.GetAbsoluteContainingBlock()?;
        if !absCB.PrepareAbsoluteFrames(aInlineFrame) {
            return None;
        }

        // TODO(Bug 2038072): After we support splitting abspos frames in multicol or
        // printing, a later inline continuation might have abspos kids.
        MOZ_ASSERT!(aInlineFrame.GetPrevInFlow().is_none(), "Only the first inline continuation can have absolute kids!");

        let cbRect = ComputeInlineAbsoluteCBRect(aInlineFrame);
        let cbwm = aInlineFrame.GetWritingMode();

        // TODO(Bug 2038072): Pass an actual available block-size to split the abspos
        // frames correctly in multicol or printing.
        let availSize = LogicalSize::new(cbwm, aInlineFrame.ISize(cbwm), NS_UNCONSTRAINEDSIZE);
        let inlineRI = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            aPresContext,
            aReflowInput,
            aInlineFrame,
            availSize,
            None,
            EnumSet::default(),
            StyleSizeOverrides::default(),
            ComputeSizeFlags::default(),
            std::ptr::null_mut(),
        );

        // TODO(Bug 2038072): Add AbsPosReflowFlag::AllowFragmentation flag.
        let mut flags = AbsPosReflowFlags::default();
        flags += AbsPosReflowFlag::CBWidthChanged;
        flags += AbsPosReflowFlag::CBHeightChanged;

        // absoluteOverflow is in aInlineFrame's coordinate space.
        let mut absposOverflow = OverflowAreas::default();
        let mut absposStatus = nsReflowStatus::default();
        absCB.Reflow(aInlineFrame, aPresContext, &inlineRI, &mut absposStatus, cbRect, flags, Some(&mut absposOverflow));
        aStatus.MergeCompletionStatusFrom(&absposStatus);
        Some(absposOverflow)
    }
}

// Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::TrialReflowState
/**
 * For text-wrap:balance, we iteratively try reflowing with adjusted inline
 * size to find the "best" result (the tightest size that can be applied
 * without increasing the total line count of the block).
 * This record is used to manage the state of these "trial reflows", and
 * return results from the final trial.
 */
pub struct nsBlockFrame_TrialReflowState {
    // Values pre-computed at start of Reflow(), constant across trials.
    pub mConsumedBSize: nscoord,
    pub mEffectiveContentBoxBSize: nscoord,
    pub mNeedFloatManager: bool,
    // [out] Whether reflowing resulted in use of an overflow-wrap break.
    pub mUsedOverflowWrap: bool,
    // Settings for the current trial.
    pub mBalancing: bool,
    pub mInset: nscoord,
    // Results computed during the trial reflow. Values from the final trial
    // will be used by the remainder of Reflow().
    pub mOcBounds: OverflowAreas,
    pub mFcBounds: OverflowAreas,
    pub mBlockEndEdgeOfChildren: nscoord,
    pub mContainerWidth: nscoord,
}

impl nsBlockFrame_TrialReflowState {
    // Initialize for the initial trial reflow, with zero inset.
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::TrialReflowState::TrialReflowState
    pub fn new(aConsumedBSize: nscoord, aEffectiveContentBoxBSize: nscoord, aNeedFloatManager: bool) -> nsBlockFrame_TrialReflowState {
        nsBlockFrame_TrialReflowState {
            mConsumedBSize: aConsumedBSize,
            mEffectiveContentBoxBSize: aEffectiveContentBoxBSize,
            mNeedFloatManager: aNeedFloatManager,
            mUsedOverflowWrap: false,
            mBalancing: false,
            mInset: 0,
            mOcBounds: OverflowAreas::default(),
            mFcBounds: OverflowAreas::default(),
            mBlockEndEdgeOfChildren: 0,
            mContainerWidth: 0,
        }
    }

    // Adjust the inset amount, and reset state for a new trial.
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::TrialReflowState::ResetForBalance
    pub fn ResetForBalance(&mut self, aInsetDelta: nscoord) {
        // Tells the reflow-lines loop we must consider all lines "dirty" (as we
        // are modifying the effective inline-size to be used).
        self.mBalancing = true;
        // Adjust inset to apply.
        self.mInset += aInsetDelta;
        // Re-initialize state that the reflow loop will compute.
        self.mOcBounds.Clear();
        self.mFcBounds.Clear();
        self.mBlockEndEdgeOfChildren = 0;
        self.mContainerWidth = 0;
        self.mUsedOverflowWrap = false;
    }
}

// ADAPTER: mozilla::MakeScopeExit (mfmt/ScopeExit.h): runs the closure when dropped.
struct ScopeExit<F: FnMut()>(F);
impl<F: FnMut()> Drop for ScopeExit<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::Reflow
    #[allow(invalid_reference_casting)]
    pub fn Reflow(&self, aPresContext: &nsPresContext, aMetrics: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        // ADAPTER: Gecko's `const ReflowInput&` is const_cast to a mutable one in a few places below
        // (SetAvailableBSize, the float manager of nsAutoFloatManager).
        let aReflowInputMut: *mut ReflowInput = aReflowInput as *const ReflowInput as *mut ReflowInput;

        // NOT PORTED: content-visibility (hidden by an in-flow parent).
        if self.IsHiddenByContentVisibilityOfInFlowParentForLayout() && self.GetNextContinuation().is_none() {
            self.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(aMetrics, Some(&aReflowInput.mStyleDisplay));
            return;
        }

        self.MarkInReflow();
        // DO_GLOBAL_REFLOW_COUNT("nsBlockFrame");  NOT PORTED: reflow counting.
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        // #ifdef DEBUG if (gNoisyReflow) {...}  NOT PORTED: debug printing and timing.

        // ColumnSetWrapper's children depend on ColumnSetWrapper's block-size or
        // max-block-size because both affect the children's available block-size.
        if self.IsColumnSetWrapperFrame() {
            self.AddStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE);
        }

        let restoreReflowInputAvailBSize: std::cell::Cell<Option<nscoord>> = std::cell::Cell::new(None);
        let restoreRef = &restoreReflowInputAvailBSize;
        let _MaybeRestore = ScopeExit(move || {
            if let Some(b) = restoreRef.get() {
                unsafe { (*aReflowInputMut).SetAvailableBSize(b) };
            }
        });

        let wm = aReflowInput.GetWritingMode();
        let consumedBSize = self.CalcAndCacheConsumedBSize();
        let effectiveContentBoxBSize = self.GetEffectiveComputedBSize(aReflowInput, consumedBSize);
        // If we have non-auto block size, we're clipping our kids and we fit,
        // make sure our kids fit too.
        if aReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE
            && aReflowInput.ComputedBSize() != NS_UNCONSTRAINEDSIZE
            && self.ShouldApplyOverflowClipping(&aReflowInput.mStyleDisplay).contains(wm.PhysicalAxis(LogicalAxis::Block))
        {
            let mut blockDirExtras = aReflowInput.ComputedLogicalBorderPadding(wm);
            if self.GetLogicalSkipSides().BStart() {
                *blockDirExtras.BStart_mut(wm) = 0;
            } else {
                // Block-end margin never causes us to create continuations, so we
                // don't need to worry about whether it fits in its entirety.
                *blockDirExtras.BStart_mut(wm) += aReflowInput.ComputedLogicalMargin(wm).BStart(wm);
            }

            if effectiveContentBoxBSize + blockDirExtras.BStartEnd(wm) <= aReflowInput.AvailableBSize() {
                restoreReflowInputAvailBSize.set(Some(aReflowInput.AvailableBSize()));
                unsafe { (*aReflowInputMut).SetAvailableBSize(NS_UNCONSTRAINEDSIZE) };
            }
        }

        if self.IsFrameTreeTooDeep(aReflowInput, aMetrics, aStatus) {
            return;
        }

        // OK, some lines may be reflowed. Blow away any saved line cursor
        // because we may invalidate the nondecreasing
        // overflowArea.InkOverflow().y/yMost invariant, and we may even
        // delete the line with the line cursor.
        self.ClearLineCursors();

        // See comment below about oldSize. Use *only* for the
        // abs-pos-containing-block-size-change optimization!
        let oldSize = self.GetSize();

        // Should we create a float manager?
        let mut autoFloatManager = nsAutoFloatManager::new(unsafe { &mut *aReflowInputMut });

        // XXXldb If we start storing the float manager in the frame rather
        // than keeping it around only during reflow then we should create it
        // only when there are actually floats to manage.  Otherwise things
        // like tables will gain significant bloat.
        //
        // See https://bugzilla.mozilla.org/show_bug.cgi?id=1931286:
        // if we're a reflow root and no float manager is provided by the caller
        // in aReflowInput, we'd normally expect the block to be a BFC and so
        // BlockNeedsFloatManager will return true. But sometimes the block may
        // have lost its BFC-ness since it was recorded as a dirty reflow root
        // but before the reflow actually happens. Creating a float manager here
        // avoids crashing, but may not be entirely correct in such a case.
        let needFloatManager = aReflowInput.mFloatManager.is_null() || nsBlockFrame::BlockNeedsFloatManager(self.0);
        if needFloatManager {
            autoFloatManager.CreateFloatManager(aPresContext);
        }

        if self.HasAnyStateBits(NS_BLOCK_NEEDS_BIDI_RESOLUTION) && self.PresContext().BidiEnabled() {
            nsBlockFrame::from_frame(self.FirstContinuation()).ResolveBidi();
        }

        // Whether to apply text-wrap: balance behavior.
        // NOT PORTED: text-wrap: balance (the retry loop below runs one trial; the original
        // `bool tryBalance = StyleText()->mTextWrapStyle == StyleTextWrapStyle::Balance &&
        // !GetPrevContinuation();` and the BalanceTarget struct, the countLinesUpTo and
        // getClampPosition lambdas and the balance branches of the loop are nsBlockFrame.cpp lines
        // 1586-1760 at GECKO_REV).
        let tryBalance = false;
        let _ = tryBalance;

        // text-wrap: balance loop, executed only once if balancing is not required.
        let mut reflowStatus: nsReflowStatus;
        let mut trialState = nsBlockFrame_TrialReflowState::new(consumedBSize, effectiveContentBoxBSize, needFloatManager);
        loop {
            // Save the initial floatManager state for repeated trial reflows.
            // We'll restore (and re-save) the initial state each time we repeat the
            // reflow.
            let mut floatManagerState = SavedState::new();
            unsafe { &mut *aReflowInput.mFloatManager }.PushState(&mut floatManagerState);

            *aMetrics = ReflowOutput::new__WritingMode(aMetrics.GetWritingMode());
            reflowStatus = self.TrialReflow(aPresContext, aMetrics, aReflowInput, &mut trialState);

            // NOT PORTED: text-wrap: balance iteration (see above).

            // If we reach here, no balancing was required, so just exit; we don't
            // reset (pop) the floatManager state because this is the reflow we're
            // going to keep. So the saved state is just dropped.
            break;
        } // End of text-wrap: balance retry loop

        // If the block direction is right-to-left, we need to update the bounds of
        // lines that were placed relative to mContainerSize during reflow, as
        // we typically do not know the true container size until we've reflowed all
        // its children. So we use a dummy mContainerSize during reflow (see
        // BlockReflowState's constructor) and then fix up the positions of the
        // lines here, once the final block size is known.
        //
        // Note that writing-mode:vertical-rl is the only case where the block
        // logical direction progresses in a negative physical direction, and
        // therefore block-dir coordinate conversion depends on knowing the width
        // of the coordinate space in order to translate between the logical and
        // physical origins.
        if aReflowInput.GetWritingMode().IsVerticalRL() {
            let containerSize = aMetrics.PhysicalSize();
            let deltaX = containerSize.width - trialState.mContainerWidth;
            if deltaX != 0 {
                // We compute our lines and markers' overflow areas later in
                // ComputeOverflowAreas(), so we don't need to adjust their overflow areas
                // here.
                let physicalDelta = nsPoint::new(deltaX, 0);
                for line in self.Lines() {
                    self.UpdateLineContainerSize(line.get() as *mut nsLineBox, containerSize);
                }
                trialState.mFcBounds.Clear();
                let floats = self.GetFloats();
                if !floats.is_null() {
                    for f in unsafe { &*floats } {
                        f.MovePositionBy(physicalDelta);
                        self.ConsiderChildOverflow__OverflowAreas_nsIFrame(&mut trialState.mFcBounds, f);
                    }
                }
                let markerList = self.GetOutsideMarkerList();
                if !markerList.is_null() {
                    for f in unsafe { &*markerList } {
                        f.MovePositionBy(physicalDelta);
                    }
                }
                // NOT PORTED: overflow containers (fragmentation):
                // if (nsFrameList* overflowContainers = GetOverflowContainers()) {...}
            }
        }

        aMetrics.SetOverflowAreasToDesiredBounds();
        self.ComputeOverflowAreas(&mut aMetrics.mOverflowAreas, &aReflowInput.mStyleDisplay);
        // Factor overflow container child bounds into the overflow area
        aMetrics.mOverflowAreas.UnionWith(&trialState.mOcBounds);
        // Factor pushed float child bounds into the overflow area
        aMetrics.mOverflowAreas.UnionWith(&trialState.mFcBounds);

        // Reflow absolute descendants of inline absolute containing blocks after all
        // the lines are reflowed and placed.
        //
        // NS_BLOCK_HAS_INLINE_ABSPOS_DESCENDANT would have been set on us, if
        // appropriate, when we reflowed our inline lines, by
        // nsInlineFrame::MarkBlockAncestorHavingAbsoluteDescendants() (called from
        // the Reflow() of an inline/ruby descendant that has abspos children), so it
        // is up to date here. At the end of ReflowAbsoluteDescendantsInInlineFrame(),
        // the bit will be cleared when no abspos descendant is found (we do not
        // proactively traverse this block's subtree to clear the bit when an abspos
        // frame is removed).
        // (StaticPrefs::layout_abspos_fragment_aware_inline_cb_enabled() is true in Firefox 157;
        // HasPendingInterrupt is false here.)
        if self.HasAnyStateBits(NS_BLOCK_HAS_INLINE_ABSPOS_DESCENDANT) && !aReflowInput.WillReflowAgainForClearance() {
            self.ReflowAbsoluteDescendantsInInlineFrame(aPresContext, aReflowInput, aMetrics, &mut reflowStatus);
        }

        // Let the absolutely positioned container reflow any absolutely positioned
        // child frames that need to be reflowed, e.g., elements with a percentage
        // based width/height
        // We want to do this under either of two conditions:
        //  1. If we didn't do the incremental reflow above.
        //  2. If our size changed.
        // Even though it's the padding edge that's the containing block, we
        // can use our rect (the border edge) since if the border style
        // changed, the reflow would have been targeted at us so we'd satisfy
        // condition 1.
        // XXX checking oldSize is bogus, there are various reasons we might have
        // reflowed but our size might not have been changed to what we
        // asked for (e.g., we ended up being pushed to a new page)
        // When WillReflowAgainForClearance is true, we will reflow again without
        // resetting the size. Because of this, we must not reflow our abs-pos
        // children in that situation --- what we think is our "new size" will not be
        // our real new size. This also happens to be more efficient.
        let absoluteContainer = self.GetAbsoluteContainingBlock();
        if let Some(absoluteContainer) = absoluteContainer {
            if absoluteContainer.PrepareAbsoluteFrames(self.0) {
                // (haveInterrupt: HasPendingInterrupt is false here, interruptible reflow is NOT PORTED.)
                if aReflowInput.WillReflowAgainForClearance() {
                    // Make sure that when we reflow again we'll actually reflow all the abs
                    // pos frames that might conceivably depend on our size (or all of them,
                    // if we're dirty right now and interrupted; in that case we also need
                    // to mark them all with NS_FRAME_IS_DIRTY).  Sadly, we can't do much
                    // better than that, because we don't really know what our size will be,
                    // and it might in fact not change on the followup reflow!
                    absoluteContainer.MarkSizeDependentFramesDirty();
                } else {
                    // Mark frames that depend on changes we just made to this frame as dirty:
                    // Now we can assume that the padding edge hasn't moved.
                    // We need to reflow the absolutes if one of them depends on
                    // its placeholder position, or the containing block size in a
                    // direction in which the containing block size might have
                    // changed.

                    // XXX "width" and "height" in this block will become ISize and BSize
                    // when AbsoluteContainingBlock is logicalized
                    let cbWidthChanged = aMetrics.Width() != oldSize.width;
                    // (!GetContent()->GetParent(): the root element, whose parent is the document)
                    let isRoot = self.IsRootElementStyle();
                    // If isRoot and we have auto height, then we are the initial
                    // containing block and the containing block height is the
                    // viewport height, which can't change during incremental
                    // reflow.
                    let cbHeightChanged = !(isRoot && NS_UNCONSTRAINEDSIZE == aReflowInput.ComputedHeight()) && aMetrics.Height() != oldSize.height;

                    let mut flags = AbsPosReflowFlags::default();
                    flags += AbsPosReflowFlag::AllowFragmentation;
                    if cbWidthChanged {
                        flags += AbsPosReflowFlag::CBWidthChanged;
                    }
                    if cbHeightChanged {
                        flags += AbsPosReflowFlag::CBHeightChanged;
                    }
                    // Setup the line cursor here to optimize line searching for
                    // calculating hypothetical position of absolutely-positioned
                    // frames.
                    self.SetupLineCursorForQuery();

                    let mut cbRect = LogicalRect::new__WritingMode_LogicalPoint_LogicalSize(wm, LogicalPoint::new__WritingMode(wm), aMetrics.Size(wm));
                    cbRect.Deflate__WritingMode_LogicalMargin(wm, *aReflowInput.ComputedLogicalBorder(wm).ApplySkipSides(self.PreReflowBlockLevelLogicalSkipSides()));
                    let mut absposStatus = nsReflowStatus::default();
                    let containerSize = aMetrics.PhysicalSize();
                    absoluteContainer.Reflow(self.0, aPresContext, aReflowInput, &mut absposStatus, cbRect.GetPhysicalRect(wm, containerSize), flags, Some(&mut aMetrics.mOverflowAreas));
                    reflowStatus.MergeCompletionStatusFrom(&absposStatus);
                }
            }
        }

        self.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(aMetrics, Some(&aReflowInput.mStyleDisplay));

        *aStatus = reflowStatus;

        // #ifdef DEBUG
        //   nsLayoutUtils::AssertNoDuplicateContinuations(this, GetChildList(FrameChildListID::Float));
        //   if (gNoisyReflow) {...}  if (gLameReflowMetrics) {...}
        // NOT PORTED: debug-only checks and printing.
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::TrialReflow
    pub fn TrialReflow(&self, aPresContext: &nsPresContext, aMetrics: &mut ReflowOutput, aReflowInput: &ReflowInput, aTrialState: &mut nsBlockFrame_TrialReflowState) -> nsReflowStatus {
        // #ifdef DEBUG nsLayoutUtils::AssertNoDuplicateContinuations(...)  NOT PORTED: debug-only check.

        // ALWAYS drain overflow. We never want to leave the previnflow's
        // overflow lines hanging around; block reflow depends on the
        // overflow line lists being cleared out between reflow passes.
        self.DrainOverflowLines();

        // Clear any existing -webkit-line-clamp ellipsis if we're reflowing the
        // line-clamp root.
        if IsLineClampRoot(*self) {
            self.ClearLineClampEllipsis();
        }

        let (mut blockStartMarginRoot, mut blockEndMarginRoot) = (false, false);
        self.IsMarginRoot(&mut blockStartMarginRoot, &mut blockEndMarginRoot);

        let mut state = BlockReflowState::new(
            aReflowInput,
            *aPresContext,
            *self,
            blockStartMarginRoot,
            blockEndMarginRoot,
            aTrialState.mNeedFloatManager,
            aTrialState.mConsumedBSize,
            aTrialState.mEffectiveContentBoxBSize,
            aTrialState.mInset,
        );

        // Handle paginated overflow (see nsContainerFrame.h)
        let ocStatus = nsReflowStatus::new();
        if self.GetPrevInFlow().is_some() {
            // NOT PORTED: fragmentation:
            // ReflowOverflowContainerChildren(aPresContext, aReflowInput, aTrialState.mOcBounds,
            //     ReflowChildFlags::Default, ocStatus, DefaultChildFrameMerge, Some(state.ContainerSize()));
        }

        // Now that we're done cleaning up our overflow container lists, we can
        // give |state| its nsOverflowContinuationTracker.
        // NOT PORTED: fragmentation: nsOverflowContinuationTracker tracker(this, false);
        // state.mOverflowTracker = &tracker;

        // Drain & handle pushed floats
        self.DrainPushedFloats();
        self.ReflowPushedFloats(&mut state, &mut aTrialState.mFcBounds);

        // If we're not dirty (which means we'll mark everything dirty later)
        // and our inline-size has changed, mark the lines dirty that we need to
        // mark dirty for a resize reflow.
        if !self.HasAnyStateBits(NS_FRAME_IS_DIRTY) && aReflowInput.IsIResize() {
            self.PrepareResizeReflow(&mut state);
        }

        // The same for percentage text-indent, except conditioned on the
        // parent resizing.
        if !self.HasAnyStateBits(NS_FRAME_IS_DIRTY)
            && aReflowInput.mCBReflowInput.map_or(false, |cb| cb.IsIResize())
            && nsStyleTextAdapter::of(self.0).mTextIndent().length.HasPercent()
            && !self.mLines().empty()
        {
            let mut front = self.LinesBegin();
            front.MarkDirty();
        }

        // For text-wrap:balance trials, we need to reflow all the inline lines even
        // if they're not all "dirty", but we don't need to reflow any block lines.
        if aTrialState.mBalancing {
            MarkAllInlineLinesDirty(*self);
        } else {
            self.LazyMarkLinesDirty();
        }

        // Now reflow...
        aTrialState.mUsedOverflowWrap = self.ReflowDirtyLines(&mut state);

        // If we have a next-in-flow, and that next-in-flow has pushed floats from
        // this frame from a previous iteration of reflow, then we should not return
        // a status with IsFullyComplete() equals to true, since we actually have
        // overflow, it's just already been handled.

        // NOTE: This really shouldn't happen, since we _should_ pull back our floats
        // and reflow them, but just in case it does, this is a safety precaution so
        // we don't end up with a placeholder pointing to frames that have already
        // been deleted as part of removing our next-in-flow.
        if state.mReflowStatus.IsFullyComplete() {
            let mut nif = self.GetNextInFlow().map(nsBlockFrame::from_frame);
            while let Some(n) = nif {
                if n.HasPushedFloatsFromPrevContinuation() {
                    if n.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) {
                        state.mReflowStatus.SetOverflowIncomplete();
                    } else {
                        state.mReflowStatus.SetIncomplete();
                    }
                    break;
                }

                nif = n.GetNextInFlow().map(nsBlockFrame::from_frame);
            }
        }

        state.mReflowStatus.MergeCompletionStatusFrom(&ocStatus);

        // If we end in a BR with clear and affected floats continue,
        // we need to continue, too.
        if NS_UNCONSTRAINEDSIZE != aReflowInput.AvailableBSize() && state.mReflowStatus.IsComplete() && state.FloatManager().ClearContinues(self.FindTrailingClear()) {
            state.mReflowStatus.SetIncomplete();
        }

        if !state.mReflowStatus.IsFullyComplete() {
            if self.HasOverflowLines() || self.HasPushedFloats() {
                state.mReflowStatus.SetNextInFlowNeedsReflow();
            }
        }

        // Place the ::marker's frame if it is placed next to a block child.
        //
        // According to the CSS2 spec, section 12.6.1, the ::marker's box
        // participates in the height calculation of the list-item box's
        // first line box.
        //
        // There are exactly two places a ::marker can be placed: near the
        // first or second line. It's only placed on the second line in a
        // rare case: an empty first line followed by a second line that
        // contains a block (example: <LI>\n<P>... ). This is where
        // the second case can happen.
        let outsideMarker = self.GetOutsideMarker();
        if let Some(outsideMarker) = outsideMarker {
            if !self.mLines().empty() && (self.LinesBegin().IsBlock() || (0 == self.LinesBegin().BSize() && self.LinesBegin() != self.LinesEnd().prev() && self.LinesBegin().next_().IsBlock())) {
                // Reflow the ::marker's frame.
                let mut reflowOutput = ReflowOutput::new__ReflowInput(aReflowInput);
                // XXX Use the entire line when we fix bug 25888.
                let mut position = nsLayoutUtils_LinePosition::default();
                let wm = aReflowInput.GetWritingMode();
                let havePosition = nsLayoutUtils::GetFirstLinePosition(wm, self.0, &mut position);
                let lineBStart = if havePosition { position.mBStart } else { aReflowInput.ComputedLogicalBorderPadding(wm).BStart(wm) };
                self.ReflowOutsideMarker(outsideMarker, &mut state, &mut reflowOutput, lineBStart);
                NS_ASSERTION!(!self.MarkerIsEmpty(outsideMarker) || reflowOutput.BSize(wm) == 0, "empty ::marker frame took up space");

                if havePosition && !self.MarkerIsEmpty(outsideMarker) {
                    // We have some lines to align the ::marker with.

                    // Doing the alignment using the baseline will also cater for
                    // ::markers that are placed next to a child block (bug 92896)

                    // Tall ::markers won't look particularly nice here...
                    let mut bbox = outsideMarker.GetLogicalRect__WritingMode_nsSize(wm, reflowOutput.PhysicalSize());
                    let baselineGroup = BaselineSharingGroup::First;
                    let mut result: Option<nscoord> = None;
                    if !wm.IsOrthogonalTo(&outsideMarker.GetWritingMode()) {
                        result = outsideMarker.GetNaturalBaselineBOffset(wm, baselineGroup, BaselineExportContext::LineLayout);
                    }
                    let markerBaseline = result.unwrap_or_else(|| bbox.BSize(wm) + outsideMarker.GetLogicalUsedMargin(wm).BEnd(wm));
                    *bbox.BStart_mut(wm) = position.mBaseline - markerBaseline;
                    outsideMarker.SetRect__WritingMode_LogicalRect_nsSize(wm, bbox, reflowOutput.PhysicalSize());
                }
                // Otherwise just leave the ::marker where it is, up against our
                // block-start padding.
            }
        }

        self.CheckFloats(&mut state);

        // Compute our final size (for this trial layout)
        aTrialState.mBlockEndEdgeOfChildren = self.ComputeFinalSize(aReflowInput, &mut state, aMetrics);
        aTrialState.mContainerWidth = state.ContainerSize().width;

        // Align content
        self.AlignContent(&mut state, aMetrics, aTrialState.mBlockEndEdgeOfChildren);

        state.mReflowStatus
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::CheckForCollapsedBEndMarginFromClearanceLine
    pub fn CheckForCollapsedBEndMarginFromClearanceLine(&self) -> bool {
        for mut line in self.Lines().rev() {
            if 0 != line.BSize() || !line.CachedIsEmpty() {
                return false;
            }
            if line.HasClearance() {
                return true;
            }
        }
        false
    }

    // Gecko: layout/generic/nsBlockFrame.cpp FindLineClampTarget
    // NOT PORTED: -webkit-line-clamp (nsBlockFrame.cpp lines 2173-2224 at GECKO_REV).

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ApplyLineClamp
    // NOT PORTED: -webkit-line-clamp (the body, nsBlockFrame.cpp lines 2226-2264 at GECKO_REV, starts
    // with `auto* root = GetLineClampRoot(); if (!root) return aContentBlockEndEdge;`, and there is
    // never a root).
    pub fn ApplyLineClamp(&self, aContentBlockEndEdge: nscoord) -> nscoord {
        let root = self.GetLineClampRoot();
        if root.is_none() {
            return aContentBlockEndEdge;
        }
        unimplemented!("NOT PORTED: -webkit-line-clamp (nsBlockFrame::ApplyLineClamp)")
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ComputeFinalSize
    // Returns block-end edge of children.
    pub fn ComputeFinalSize(&self, aReflowInput: &ReflowInput, aState: &mut BlockReflowState, aMetrics: &mut ReflowOutput) -> nscoord {
        let wm = aState.mReflowInput.GetWritingMode();
        let borderPadding = *aState.BorderPadding();
        // #ifdef NOISY_FINAL_SIZE ... NOT PORTED: debug printing.

        // Compute final inline size
        let mut finalSize = LogicalSize::new__WritingMode(wm);
        *finalSize.ISize_mut(wm) = NSCoordSaturatingAdd(NSCoordSaturatingAdd(borderPadding.IStart(wm), aReflowInput.ComputedISize()), borderPadding.IEnd(wm));

        // Return block-end margin information
        // rbs says he hit this assertion occasionally (see bug 86947), so
        // just set the margin to zero and we'll figure out why later
        // NS_ASSERTION(aMetrics.mCarriedOutBEndMargin.IsZero(),
        //             "someone else set the margin");
        let mut nonCarriedOutBDirMargin: nscoord = 0;
        if !aState.mFlags.mIsBEndMarginRoot {
            // Apply rule from CSS 2.1 section 8.3.1. If we have some empty
            // line with clearance and a non-zero block-start margin and all
            // subsequent lines are empty, then we do not allow our children's
            // carried out block-end margin to be carried out of us and collapse
            // with our own block-end margin.
            if self.CheckForCollapsedBEndMarginFromClearanceLine() {
                // Convert the children's carried out margin to something that
                // we will include in our height
                nonCarriedOutBDirMargin = aState.mPrevBEndMargin.Get();
                aState.mPrevBEndMargin.Zero();
            }
            aMetrics.mCarriedOutBEndMargin = aState.mPrevBEndMargin;
        } else {
            aMetrics.mCarriedOutBEndMargin.Zero();
        }

        let mut blockEndEdgeOfChildren: nscoord = aState.mBCoord + nonCarriedOutBDirMargin;
        // Shrink wrap our height around our contents.
        if aState.mFlags.mIsBEndMarginRoot || NS_UNCONSTRAINEDSIZE != aReflowInput.ComputedBSize() {
            // When we are a block-end-margin root make sure that our last
            // child's block-end margin is fully applied. We also do this when
            // we have a computed height, since in that case the carried out
            // margin is not going to be applied anywhere, so we should note it
            // here to be included in the overflow area.
            // Apply the margin only if there's space for it.
            if blockEndEdgeOfChildren < aState.mReflowInput.AvailableBSize() {
                // Truncate block-end margin if it doesn't fit to our available BSize.
                blockEndEdgeOfChildren = std::cmp::min(blockEndEdgeOfChildren + aState.mPrevBEndMargin.Get(), aState.mReflowInput.AvailableBSize());
            }
        }
        if aState.mFlags.mBlockNeedsFloatManager {
            // Include the float manager's state to properly account for the
            // block-end margin of any floated elements; e.g., inside a table cell.
            //
            // Note: The block coordinate returned by ClearFloats is always greater than
            // or equal to blockEndEdgeOfChildren.
            blockEndEdgeOfChildren = aState.ClearFloats(blockEndEdgeOfChildren, UsedClear::Both, None).0;
        }

        // undo cached alignment shift for sizing purposes
        // (we used shifted positions because the float manager uses them)
        blockEndEdgeOfChildren -= aState.mAlignContentShift;
        aState.UndoAlignContentShift();

        if NS_UNCONSTRAINEDSIZE != aReflowInput.ComputedBSize() {
            // Note: We don't use blockEndEdgeOfChildren because it includes the
            // previous margin.
            let contentBSizeWithBStartBP: nscoord = aState.mBCoord + nonCarriedOutBDirMargin;

            // We don't care about ApplyLineClamp's return value (the line-clamped
            // content BSize) in this explicit-BSize codepath, but we do still need to
            // call ApplyLineClamp for ellipsis markers to be placed as-needed.
            self.ApplyLineClamp(contentBSizeWithBStartBP);

            *finalSize.BSize_mut(wm) = self.ComputeFinalBSize(aState, contentBSizeWithBStartBP);

            // If the content block-size is larger than the effective computed
            // block-size, we extend the block-size to contain all the content.
            // https://drafts.csswg.org/css-sizing-4/#aspect-ratio-minimum
            if aReflowInput.ShouldApplyAutomaticMinimumOnBlockAxis() {
                // Note: finalSize.BSize(wm) is the border-box size, so we compare it with
                // the content's block-size plus our border and padding..
                *finalSize.BSize_mut(wm) = std::cmp::max(finalSize.BSize(wm), contentBSizeWithBStartBP + borderPadding.BEnd(wm));

                // The size should be capped by its maximum block size.
                if aReflowInput.ComputedMaxBSize() != NS_UNCONSTRAINEDSIZE {
                    *finalSize.BSize_mut(wm) = std::cmp::min(finalSize.BSize(wm), aReflowInput.ComputedMaxBSize() + borderPadding.BStartEnd(wm));
                }
            }

            // Don't carry out a block-end margin when our BSize is fixed.
            //
            // Note: this also includes the case that aReflowInput.ComputedBSize() is
            // calculated from aspect-ratio. i.e. Don't carry out block margin-end if it
            // is replaced by the block size from aspect-ratio and inline size.
            aMetrics.mCarriedOutBEndMargin.Zero();
        } else if let Some(containBSize) = self.ContainIntrinsicBSize(if self.IsComboboxControlFrame() { aReflowInput.GetLineHeight() } else { 0 }) {
            // If we're size-containing in block axis and we don't have a specified
            // block size, then our final size should actually be computed from only
            // our border, padding and contain-intrinsic-block-size, ignoring the
            // actual contents. Hence this case is a simplified version of the case
            // below.
            let contentBSize: nscoord = containBSize;
            let mut autoBSize: nscoord = aReflowInput.ApplyMinMaxBSize(contentBSize, aState.mConsumedBSize);
            aMetrics.mCarriedOutBEndMargin.Zero();
            autoBSize += borderPadding.BStartEnd(wm);
            *finalSize.BSize_mut(wm) = autoBSize;
        } else if aState.mReflowStatus.IsInlineBreakBefore() {
            // Our parent is expected to push this frame to the next page/column so
            // what size we set here doesn't really matter.
            *finalSize.BSize_mut(wm) = aReflowInput.AvailableBSize();
        } else if aState.mReflowStatus.IsComplete() {
            let lineClampedContentBlockEndEdge: nscoord = self.ApplyLineClamp(blockEndEdgeOfChildren);

            let bpBStart: nscoord = borderPadding.BStart(wm);
            let contentBSize: nscoord = blockEndEdgeOfChildren - bpBStart;
            let lineClampedContentBSize: nscoord = lineClampedContentBlockEndEdge - bpBStart;

            let autoBSize: nscoord = aReflowInput.ApplyMinMaxBSize(lineClampedContentBSize, aState.mConsumedBSize);
            if autoBSize != contentBSize {
                // Our min-block-size, max-block-size, or -webkit-line-clamp value made
                // our bsize change.  Don't carry out our kids' block-end margins.
                aMetrics.mCarriedOutBEndMargin.Zero();
            }
            let mut bSize: nscoord = autoBSize + borderPadding.BStartEnd(wm);
            if autoBSize > contentBSize && bSize > aReflowInput.AvailableBSize() && aReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE {
                // Applying `min-size` made us overflow our available size.
                // Clamp it and report that we're Incomplete, or BreakBefore if we have
                // 'break-inside: avoid' that is applicable.
                bSize = aReflowInput.AvailableBSize();
                if self.ShouldAvoidBreakInside(aReflowInput) {
                    aState.mReflowStatus.SetInlineLineBreakBeforeAndReset();
                } else {
                    aState.mReflowStatus.SetIncomplete();
                }
            }
            *finalSize.BSize_mut(wm) = bSize;
        } else {
            NS_ASSERTION!(aReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE, "Shouldn't be incomplete if availableBSize is UNCONSTRAINED.");
            let mut bSize: nscoord = std::cmp::max(aState.mBCoord, aReflowInput.AvailableBSize());
            if aReflowInput.AvailableBSize() == NS_UNCONSTRAINEDSIZE {
                // This should never happen, but it does. See bug 414255
                bSize = aState.mBCoord;
            }
            let maxBSize: nscoord = aReflowInput.ComputedMaxBSize();
            if maxBSize != NS_UNCONSTRAINEDSIZE && aState.mConsumedBSize + bSize - borderPadding.BStart(wm) > maxBSize {
                // Compute this fragment's block-size, with the max-block-size
                // constraint taken into consideration.
                let clampedBSizeWithoutEndBP: nscoord = std::cmp::max(0, maxBSize - aState.mConsumedBSize) + borderPadding.BStart(wm);
                let clampedBSize: nscoord = clampedBSizeWithoutEndBP + borderPadding.BEnd(wm);
                if clampedBSize <= aReflowInput.AvailableBSize() {
                    // We actually fit after applying `max-size` so we should be
                    // Overflow-Incomplete instead.
                    bSize = clampedBSize;
                    aState.mReflowStatus.SetOverflowIncomplete();
                } else {
                    // We cannot fit after applying `max-size` with our block-end BP, so
                    // we should draw it in our next continuation.
                    bSize = clampedBSizeWithoutEndBP;
                }
            }
            *finalSize.BSize_mut(wm) = bSize;
        }

        if self.IsTrueOverflowContainer() {
            if aState.mReflowStatus.IsIncomplete() {
                // Overflow containers can only be overflow complete.
                // Note that auto height overflow containers have no normal children
                NS_ASSERTION!(finalSize.BSize(wm) == 0, "overflow containers must be zero-block-size");
                aState.mReflowStatus.SetOverflowIncomplete();
            }
        } else if aReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE && !aState.mReflowStatus.IsInlineBreakBefore() && aState.mReflowStatus.IsComplete() {
            // Currently only used for grid items, but could be used in other contexts.
            // The FragStretchBSizeProperty is our expected non-fragmented block-size
            // we should stretch to (for align-self:stretch etc).  In some fragmentation
            // cases though, the last fragment (this frame since we're complete), needs
            // to have extra size applied because earlier fragments consumed too much of
            // our computed size due to overflowing their containing block.  (E.g. this
            // ensures we fill the last row when a multi-row grid item is fragmented).
            if let Some(bSize) = self.GetProperty(nsIFrame::FragStretchBSizeProperty()) {
                *finalSize.BSize_mut(wm) = std::cmp::max(bSize, finalSize.BSize(wm));
            }
        }

        // Clamp the content size to fit within the margin-box clamp size, if any.
        if aReflowInput.mComputeSizeFlags.contains(ComputeSizeFlag::BClampMarginBoxMinSize) && aState.mReflowStatus.IsComplete() {
            if let Some(cbSize) = self.GetProperty(nsIFrame::BClampMarginBoxMinSizeProperty()) {
                let marginBoxBSize = finalSize.BSize(wm) + aReflowInput.ComputedLogicalMargin(wm).BStartEnd(wm);
                let overflow = marginBoxBSize - cbSize;
                if overflow > 0 {
                    let contentBSize = finalSize.BSize(wm) - borderPadding.BStartEnd(wm);
                    let newContentBSize = std::cmp::max(0, contentBSize - overflow);
                    // XXXmats deal with percentages better somehow?
                    *finalSize.BSize_mut(wm) -= contentBSize - newContentBSize;
                }
            }
        }

        // Screen out negative block sizes --- can happen due to integer overflows :-(
        *finalSize.BSize_mut(wm) = std::cmp::max(0, finalSize.BSize(wm));
        aMetrics.SetSize(wm, &finalSize);

        blockEndEdgeOfChildren
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::AlignContent
    pub fn AlignContent(&self, aState: &mut BlockReflowState, aMetrics: &mut ReflowOutput, aBEndEdgeOfChildren: nscoord) {
        let originalAlignment: StyleAlignFlags = self.EffectiveAlignContent();
        let alignment: StyleAlignFlags = originalAlignment & !StyleAlignFlags::FLAG_BITS;

        // Short circuit
        let isCentered = alignment == StyleAlignFlags::CENTER || alignment == StyleAlignFlags::SPACE_AROUND || alignment == StyleAlignFlags::SPACE_EVENLY;
        let isEndAlign = alignment == StyleAlignFlags::END || alignment == StyleAlignFlags::FLEX_END || alignment == StyleAlignFlags::LAST_BASELINE;
        if !isEndAlign && !isCentered && aState.mAlignContentShift == 0 {
            // desired shift = 0, no cached shift to undo
            return;
        }

        // NOTE: ComputeFinalSize already called aState.UndoAlignContentShift(),
        //       so metrics no longer include cached shift.
        // NOTE: Content is currently positioned at cached shift
        // NOTE: Content has been fragmented against 0-shift assumption.

        // Calculate shift
        let mut shift: nscoord = 0;
        let wm = aState.mReflowInput.GetWritingMode();
        if (isCentered || isEndAlign) && !self.mLines().empty() && aState.mReflowStatus.IsFullyComplete() && self.GetPrevInFlow().is_none() {
            let availB: nscoord = aState.mReflowInput.AvailableBSize();
            let endB: nscoord = aMetrics.Size(wm).BSize(wm) - aState.BorderPadding().BEnd(wm);
            shift = std::cmp::min(availB, endB) - aBEndEdgeOfChildren;

            // note: these measures all include start BP, so it subtracts out
            if !originalAlignment.contains(StyleAlignFlags::UNSAFE) {
                shift = std::cmp::max(0, shift);
            }
            if isCentered {
                shift = shift / 2;
            }
        }
        // else: zero shift if start-aligned or if fragmented

        let delta: nscoord = shift - aState.mAlignContentShift;
        if delta != 0 {
            // Shift children
            let translation = LogicalPoint::new(wm, 0, delta);
            for line in self.Lines() {
                self.SlideLine(aState, line.get() as *mut nsLineBox, delta);
            }
            for kid in self.GetChildList(FrameChildListID::Float) {
                kid.MovePositionBy__WritingMode_LogicalPoint(wm, translation);
            }
            let outsideMarker = self.GetOutsideMarker();
            if let Some(outsideMarker) = outsideMarker {
                if !self.mLines().empty() {
                    outsideMarker.MovePositionBy__WritingMode_LogicalPoint(wm, translation);
                }
            }
        }

        if shift != 0 {
            // Cache shift
            self.SetProperty(AlignContentShift(), shift);
        } else {
            self.RemoveProperty(AlignContentShift());
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ComputeOverflowAreas
    // Helper method for Reflow(). Computes the overflow areas created by our
    // children, and includes them into aOverflowAreas.
    pub fn ComputeOverflowAreas(&self, aOverflowAreas: &mut OverflowAreas, aDisplay: &nsStyleDisplay) {
        // XXX_perf: This can be done incrementally.  It is currently one of
        // the things that makes incremental reflow O(N^2).
        let mut overflowClipAxes = self.ShouldApplyOverflowClipping(aDisplay);
        let overflowClipMargin = self.OverflowClipMargin(overflowClipAxes, /* aAllowNegative = */ false);
        if overflowClipAxes == kPhysicalAxesBoth && overflowClipMargin == nsMargin::default() {
            return;
        }

        // We rely here on our caller having called SetOverflowAreasToDesiredBounds().
        let frameBounds: nsRect = *aOverflowAreas.ScrollableOverflow();

        let wm = self.GetWritingMode();
        let borderPadding = self.GetLogicalUsedBorderAndPadding(wm).GetPhysicalMargin(wm);
        // Compute content-box by subtracting borderPadding off of frame rect.
        // This gives us a reasonable starting-rect for the child-rect-unioning
        // below, which we can then inflate by our padding (without needing to
        // worry about having double-counted our padding or anything).
        let mut frameContentBounds = frameBounds;
        frameContentBounds.Deflate__nsMargin(borderPadding);
        // Margin rects (Zero-area rects included) of in-flow children (And floats,
        // as discussed later) are unioned (starting with the scroller's own
        // content-box), then inflated by the scroll container's padding...
        let mut inFlowChildBounds = frameContentBounds;
        // ... While scrollable overflow rects contributed from further descendants
        // (Regardless of if they're in-flow or out-of-flow) are unioned separately
        // and their union does not get inflated by the scroll container's padding.
        let mut inFlowScrollableOverflow = frameContentBounds;

        for line in self.Lines() {
            *aOverflowAreas.InkOverflow_mut() = aOverflowAreas.InkOverflow().Union(line.InkOverflowRect());
            if aDisplay.IsContainLayout() {
                // If we have layout containment, we should only consider our child's
                // ink overflow, leaving the scrollable regions of the parent
                // unaffected.
                // Note: Any overflow must be treated as ink overflow (As per
                // https://drafts.csswg.org/css-contain/#containment-layout part 3).
                // However, by unioning the children's ink overflow, we've already
                // incorporated its scrollable overflow, since scrollable overflow
                // is a subset of ink overflow.
                continue;
            }

            if line.IsInline() {
                // This is the maximum contribution for inline line-participating frames -
                // See `GetLineFrameInFlowBounds`.
                inFlowChildBounds = inFlowChildBounds.UnionEdges(line.GetPhysicalBounds());
            }
            let lineInFlowChildBounds = line.GetInFlowChildBounds();
            if let Some(b) = lineInFlowChildBounds {
                inFlowChildBounds = inFlowChildBounds.UnionEdges(b);
            }
            inFlowScrollableOverflow = inFlowScrollableOverflow.Union(line.ScrollableOverflowRect());
        }

        // if (Style()->GetPseudoType() == PseudoStyleType::MozScrolledContent) {...}
        // NOT PORTED: scrolling (padding inflation of the scrolled content's overflow, and the
        // single-line text input case); the servo configuration has no ::-moz-scrolled-content.
        let _ = inFlowChildBounds;

        // Note: we're using UnionAllWith so as to maintain the invariant of
        // ink overflow being a superset of scrollable overflow.
        aOverflowAreas.UnionAllWith(inFlowScrollableOverflow);

        // Factor an outside ::marker in; normally the ::marker will be factored
        // into the line-box's overflow areas. However, if the line is a block
        // line then it won't; if there are no lines, it won't. So just
        // factor it in anyway (it can't hurt if it was already done).
        // XXXldb Can we just fix GetOverflowArea instead?
        if let Some(outsideMarker) = self.GetOutsideMarker() {
            aOverflowAreas.UnionAllWith(outsideMarker.GetRect());
        }

        if !overflowClipAxes.isEmpty() {
            aOverflowAreas.ApplyClipping(frameBounds, overflowClipAxes, overflowClipMargin);
        }

        // #ifdef NOISY_OVERFLOW_AREAS ... NOT PORTED: debug printing.
    }

    // Text control padding handling is kinda disgusting.
    // Gecko: layout/generic/nsBlockFrame.cpp RestrictPaddingInflationInInline
    // ADAPTER: there is no ::-moz-scrolled-content (no scroll container frames), so the frame is
    // always an overflow: visible one whose scroll{Width,Height} is asked for: false.
    fn RestrictPaddingInflationInInline(&self) -> bool {
        // This can only happen when computing scrollable overflow for overflow:
        // visible frames (for scroll{Width,Height}).
        false
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ComputePaddingInflatedScrollableOverflow
    pub fn ComputePaddingInflatedScrollableOverflow(&self, aInFlowChildBounds: nsRect) -> nsRect {
        let mut result = aInFlowChildBounds;
        let wm = self.GetWritingMode();
        let mut padding = self.GetLogicalUsedPadding(wm);
        if self.RestrictPaddingInflationInInline() {
            *padding.IStart_mut(wm) = 0;
            *padding.IEnd_mut(wm) = 0;
        }
        result.Inflate__nsMargin(padding.GetPhysicalMargin(wm));
        result
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetLineFrameInFlowBounds
    pub fn GetLineFrameInFlowBounds(&self, aLine: &nsLineBox, aLineChildFrame: nsIFrame, aConsiderPositiveMargins: bool) -> Option<nsRect> {
        MOZ_ASSERT!(aLineChildFrame.GetParent() == Some(self.0), "Line's frame doesn't belong to this block frame?");
        // Line participants are considered in-flow for content within the line
        // bounds, which should be accounted for from the line bounds. This is
        // consistent with e.g. inline element's `margin-bottom` not affecting the
        // placement of the next line.
        if aLineChildFrame.IsPlaceholderFrame() || aLineChildFrame.IsLineParticipant() {
            return None;
        }
        if aLine.IsInline() {
            return Some(GetNormalMarginRect(aLineChildFrame, aConsiderPositiveMargins));
        }
        let wm = self.GetWritingMode();
        let mut rect = aLineChildFrame.GetRectRelativeToSelf();
        // Special handling is required for boxes of zero block size, which carry
        // out margin collapsing with themselves. We end up "rewinding" the line
        // position after carrying out the block start margin. This is not reflected
        // in the zero-sized frame's own frame-position.
        let linePoint = aLine.GetPhysicalBounds().TopLeft();
        let normalPosition =
            if aLineChildFrame.GetLogicalSize__WritingMode(wm).BSize(wm) == 0 { linePoint } else { aLineChildFrame.GetNormalPosition() };
        // Ensure we use the margin we actually carried out.
        let mut margin;
        if aConsiderPositiveMargins {
            let mut logicalMargin = aLineChildFrame.GetLogicalUsedMargin(wm);
            *logicalMargin.BEnd_mut(wm) = aLine.GetCarriedOutBEndMargin().Get();
            margin = logicalMargin.GetPhysicalMargin(wm);
            margin.ApplySkipSides(aLineChildFrame.GetSkipSides());
        } else {
            margin = aLineChildFrame.GetUsedMargin();
            margin.ApplySkipSides(aLineChildFrame.GetSkipSides());
            margin.EnsureAtMost(nsMargin::default());
        }
        rect.Inflate__nsMargin(margin);
        Some(rect + normalPosition)
    }
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::UnionChildOverflow
    pub fn UnionChildOverflow(&self, aOverflowAreas: &mut OverflowAreas, mut aAsIfScrolled: bool) {
        // We need to update the overflow areas of lines manually, as they
        // get cached and re-used otherwise. Lines aren't exposed as normal
        // frame children, so calling UnionChildOverflow alone will end up
        // using the old cached values.
        let wm = self.GetWritingMode();

        // ButtonControlFrame elements don't support scrolling, and some like
        // comboboxes intentionally ignore padding to place their inner elements like
        // the button, so to preserve behavior of stuff like the scroll{Width,Height}
        // APIs, forcefully ignore aAsIfScrolled there.
        aAsIfScrolled = aAsIfScrolled && !self.IsButtonControlFrame();

        // Overflow area computed here should agree with one computed in
        // `ComputeOverflowAreas` (see bug 1800939 and bug 1800719). So the
        // documentation in that function applies here as well.
        // ADAPTER: there is no ::-moz-scrolled-content (no scroll container frames), so `isScrolled` is
        // only true as-if-scrolled (scroll{Width,Height}).
        let isScrolled = aAsIfScrolled;
        // Note that we don't add line in-flow margins if we're not a BFC (which can
        // happen only for overflow: visible), so that we don't incorrectly account
        // for margins that otherwise collapse through, see bug 1936156. Note that
        // ::-moz-scrolled-content is always a BFC (see `AnonymousBoxIsBFC`).
        let considerPositiveMarginsForInFlowChildBounds = isScrolled && self.HasAnyStateBits(NS_BLOCK_BFC);

        // Relying on aOverflowAreas having been set to frame border rect (if
        // aAsIfScrolled is false), or padding rect (if true).
        let mut frameContentBounds = *aOverflowAreas.ScrollableOverflow();
        frameContentBounds.Deflate__nsMargin((if aAsIfScrolled { self.GetLogicalUsedPadding(wm) } else { self.GetLogicalUsedBorderAndPadding(wm) }).GetPhysicalMargin(wm));
        // We need to take in-flow children's margin rect into account, and inflate
        // it by the padding.
        let mut inFlowChildBounds = frameContentBounds;
        let mut inFlowScrollableOverflow = frameContentBounds;

        let inkOverflowOnly = !aAsIfScrolled && self.StyleDisplay().IsContainLayout();

        for mut line in self.Lines() {
            let bounds: nsRect = line.GetPhysicalBounds();
            let mut lineAreas = OverflowAreas::new(bounds, bounds);

            let flags = if aAsIfScrolled { OverflowAreaUnionFlags::AsIfScrolled } else { OverflowAreaUnionFlags::None };
            for lineFrame in line.ChildFrames() {
                // Ensure this is called for each frame in the line
                self.ConsiderChildOverflow__OverflowAreas_nsIFrame_OverflowAreaUnionFlags(&mut lineAreas, lineFrame, flags);

                if inkOverflowOnly || !isScrolled {
                    continue;
                }

                if let Some(lineFrameBounds) = self.GetLineFrameInFlowBounds(&line, lineFrame, considerPositiveMarginsForInFlowChildBounds) {
                    inFlowChildBounds = inFlowChildBounds.UnionEdges(lineFrameBounds);
                }
            }

            // Consider the overflow areas of the floats attached to the line as well
            if line.HasFloats() {
                for &f in line.Floats() {
                    self.ConsiderChildOverflow__OverflowAreas_nsIFrame_OverflowAreaUnionFlags(&mut lineAreas, f, flags);
                    if inkOverflowOnly || !isScrolled {
                        continue;
                    }
                    let rect = GetNormalMarginRect(f, considerPositiveMarginsForInFlowChildBounds);
                    inFlowChildBounds = inFlowChildBounds.UnionEdges(rect);
                }
            }

            if !aAsIfScrolled {
                line.SetOverflowAreas(&lineAreas);
            }
            *aOverflowAreas.InkOverflow_mut() = aOverflowAreas.InkOverflow().Union(*lineAreas.InkOverflow());
            if !inkOverflowOnly {
                inFlowScrollableOverflow = inFlowScrollableOverflow.Union(*lineAreas.ScrollableOverflow());
            }
        }

        if isScrolled {
            let paddingInflatedOverflow = self.ComputePaddingInflatedScrollableOverflow(inFlowChildBounds);
            aOverflowAreas.UnionAllWith(paddingInflatedOverflow);
        }
        aOverflowAreas.UnionAllWith(inFlowScrollableOverflow);

        // Union with child frames, skipping the principal and float lists
        // since we already handled those using the line boxes.
        nsLayoutUtils::UnionChildOverflow(self.0, aOverflowAreas, &[FrameChildListID::Principal, FrameChildListID::Float]);
    }

    // ADAPTER: the scrollable overflow that Gecko's ComputeOverflowAreas gives this block as the
    // ::-moz-scrolled-content of a scroll container (its "Padding inflation only applies to scrolled
    // containers" branch), for scroll{Width,Height} of a block whose overflow is not visible: the port
    // has no scroll container frames, so the block stands for its scrolled content, whose frame is
    // the block's padding box. Unlike UnionChildOverflow as if scrolled, the bounds of inline lines
    // are in-flow bounds, inflated by the padding. Each block-level line's in-flow bounds are its
    // child's (GetLineFrameInFlowBounds), as reflow records them for scrolled content
    // (nsLineBox::SetInFlowChildBounds); scrolled content is a BFC, so positive margins count.
    pub fn ScrolledContentOverflow(&self) -> nsRect {
        let wm = self.GetWritingMode();
        let paddingRect = self.GetPaddingRectRelativeToSelf();
        let mut frameContentBounds = paddingRect;
        frameContentBounds.Deflate__nsMargin(self.GetLogicalUsedPadding(wm).GetPhysicalMargin(wm));
        let mut inFlowChildBounds = frameContentBounds;
        let mut inFlowScrollableOverflow = frameContentBounds;
        for line in self.Lines() {
            if line.IsInline() {
                // This is the maximum contribution for inline line-participating frames -
                // See `GetLineFrameInFlowBounds`.
                inFlowChildBounds = inFlowChildBounds.UnionEdges(line.GetPhysicalBounds());
            }
            for lineFrame in line.ChildFrames() {
                if let Some(b) = self.GetLineFrameInFlowBounds(&line, lineFrame, true) {
                    inFlowChildBounds = inFlowChildBounds.UnionEdges(b);
                }
            }
            if line.HasFloats() {
                for &f in line.Floats() {
                    inFlowChildBounds = inFlowChildBounds.UnionEdges(GetNormalMarginRect(f, true));
                }
            }
            inFlowScrollableOverflow = inFlowScrollableOverflow.Union(line.ScrollableOverflowRect());
        }
        let mut areas = OverflowAreas::new(paddingRect, paddingRect);
        areas.UnionAllWith(self.ComputePaddingInflatedScrollableOverflow(inFlowChildBounds));
        areas.UnionAllWith(inFlowScrollableOverflow);
        // the other child lists (absolutely positioned frames), as UnionChildOverflow adds them
        nsLayoutUtils::UnionChildOverflow(self.0, &mut areas, &[FrameChildListID::Principal, FrameChildListID::Float]);
        *areas.ScrollableOverflow()
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ComputeCustomOverflow
    pub fn ComputeCustomOverflow(&self, aOverflowAreas: &mut OverflowAreas) -> bool {
        // Line cursor invariants depend on the overflow areas of the lines, so
        // we must clear the line cursor since those areas may have changed.
        self.ClearLineCursors();
        self.nsContainerFrame_ComputeCustomOverflow(aOverflowAreas)
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::LazyMarkLinesDirty
    pub fn LazyMarkLinesDirty(&self) {
        if self.HasAnyStateBits(NS_BLOCK_LOOK_FOR_DIRTY_FRAMES) {
            let mut line = self.LinesBegin();
            let line_end = self.LinesEnd();
            while line != line_end {
                for lineFrame in line.ChildFrames() {
                    if lineFrame.IsSubtreeDirty() {
                        // NOTE:  MarkLineDirty does more than just marking the line dirty.
                        self.MarkLineDirty(line, self.mLines());
                        break;
                    }
                }
                line.operator_inc();
            }
            self.RemoveStateBits(NS_BLOCK_LOOK_FOR_DIRTY_FRAMES);
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::MarkLineDirty
    pub fn MarkLineDirty(&self, mut aLine: nsLineList_iterator, aLineList: &nsLineList) {
        // Mark aLine dirty
        aLine.MarkDirty();
        aLine.SetInvalidateTextRuns(true);
        // #ifdef DEBUG if (gNoisyReflow) {...}  NOT PORTED: debug printing.

        // Mark previous line dirty if it's an inline line so that it can
        // maybe pullup something from the line just affected.
        // XXX We don't need to do this if aPrevLine ends in a break-after...
        if aLine.get() != aLineList.front() && aLine.IsInline() && aLine.prev().IsInline() {
            let mut prev = aLine.prev();
            prev.MarkDirty();
            prev.SetInvalidateTextRuns(true);
            // #ifdef DEBUG if (gNoisyReflow) {...}  NOT PORTED: debug printing.
        }
    }
}

/**
 * Test whether lines are certain to be aligned left so that we can make
 * resizing optimizations
 */
// Gecko: layout/generic/nsBlockFrame.cpp IsAlignedLeft
#[inline]
fn IsAlignedLeft(aAlignment: StyleTextAlign, aDirection: StyleDirection, aUnicodeBidi: StyleUnicodeBidi, aFrame: nsIFrame) -> bool {
    aFrame.IsInSVGTextSubtree()
        || StyleTextAlign::Left == aAlignment
        || (((StyleTextAlign::Start == aAlignment && StyleDirection::Ltr == aDirection) || (StyleTextAlign::End == aAlignment && StyleDirection::Rtl == aDirection)) && aUnicodeBidi != StyleUnicodeBidi::Plaintext)
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::PrepareResizeReflow
    pub fn PrepareResizeReflow(&self, aState: &mut BlockReflowState) {
        // See if we can try and avoid marking all the lines as dirty
        // FIXME(emilio): This should be writing-mode aware, I guess.
        let tryAndSkipLines =
            // The left content-edge must be a constant distance from the left
            // border-edge.
            !self.StylePadding().mPadding_Get(eSideLeft).HasPercent();

        // #ifdef DEBUG if (gDisableResizeOpt) {...} if (gNoisyReflow) {...}  NOT PORTED: debug flags.

        if tryAndSkipLines {
            let wm = aState.mReflowInput.GetWritingMode();
            let newAvailISize: nscoord = aState.mReflowInput.ComputedLogicalBorderPadding(wm).IStart(wm) + aState.mReflowInput.ComputedISize();

            // #ifdef DEBUG if (gNoisyReflow) {...}  NOT PORTED: debug printing.

            let mut line = self.LinesBegin();
            let line_end = self.LinesEnd();
            while line != line_end {
                // We let child blocks make their own decisions the same
                // way we are here.
                let isLastLine = line.get() == self.mLines().back() && self.GetNextInFlow().is_none();
                if line.IsBlock()
                    || line.HasFloats()
                    || (!isLastLine && !line.HasForcedLineBreakAfter())
                    || (isLastLine || !line.IsLineWrapped())
                    || line.ResizeReflowOptimizationDisabled()
                    || line.IsImpactedByFloat()
                    || (line.IEnd() > newAvailISize)
                {
                    line.MarkDirty();
                }

                // #ifdef REALLY_NOISY_REFLOW ... #ifdef DEBUG if (gNoisyReflow && !line->IsDirty()) {...}
                // NOT PORTED: debug printing.
                line.operator_inc();
            }
        } else {
            // Mark everything dirty
            for mut line in self.Lines() {
                line.MarkDirty();
            }
        }
    }

    //----------------------------------------

    /**
     * Propagate reflow "damage" from from earlier lines to the current
     * line.  The reflow damage comes from the following sources:
     *  1. The regions of float damage remembered during reflow.
     *  2. The combination of nonzero |aDeltaBCoord| and any impact by a
     *     float, either the previous reflow or now.
     *
     * When entering this function, |aLine| is still at its old position and
     * |aDeltaBCoord| indicates how much it will later be slid (assuming it
     * doesn't get marked dirty and reflowed entirely).
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::PropagateFloatDamage
    pub fn PropagateFloatDamage(&self, aState: &mut BlockReflowState, aLine: *mut nsLineBox, aDeltaBCoord: nscoord) {
        let aLine = unsafe { &mut *aLine };
        let floatManager = aState.FloatManager();
        NS_ASSERTION!(
            aState.mReflowInput.mParentReflowInput.map_or(false, |p| std::ptr::eq(p.mFloatManager, floatManager)) || aState.mReflowInput.mBlockDelta == 0,
            "Bad block delta passed in"
        );

        // Check to see if there are any floats; if there aren't, there can't
        // be any float damage
        if !floatManager.HasAnyFloats() {
            return;
        }

        // Check the damage region recorded in the float damage.
        if floatManager.HasFloatDamage() {
            // Need to check mBounds *and* mCombinedArea to find intersections
            // with aLine's floats
            let lineBCoordBefore: nscoord = aLine.BStart() + aDeltaBCoord;
            let lineBCoordAfter: nscoord = lineBCoordBefore + aLine.BSize();
            // Scrollable overflow should be sufficient for things that affect
            // layout.
            let wm = aState.mReflowInput.GetWritingMode();
            let containerSize = aState.ContainerSize();
            let overflow: LogicalRect = aLine.GetOverflowArea__OverflowType_WritingMode_nsSize(OverflowType::Scrollable, wm, containerSize);
            let lineBCoordCombinedBefore: nscoord = overflow.BStart(wm) + aDeltaBCoord;
            let lineBCoordCombinedAfter: nscoord = lineBCoordCombinedBefore + overflow.BSize(wm);

            let isDirty = floatManager.IntersectsDamage(lineBCoordBefore, lineBCoordAfter) || floatManager.IntersectsDamage(lineBCoordCombinedBefore, lineBCoordCombinedAfter);
            if isDirty {
                aLine.MarkDirty();
                return;
            }
        }

        // Check if the line is moving relative to the float manager
        if aDeltaBCoord + aState.mReflowInput.mBlockDelta != 0 {
            if aLine.IsBlock() {
                // Unconditionally reflow sliding blocks; we only really need to reflow
                // if there's a float impacting this block, but the current float manager
                // makes it difficult to check that.  Therefore, we let the child block
                // decide what it needs to reflow.
                aLine.MarkDirty();
            } else {
                let wasImpactedByFloat = aLine.IsImpactedByFloat();
                let floatAvailableSpace: nsFlowAreaRect = aState.GetFloatAvailableSpaceForBSize(aState.mReflowInput.GetWritingMode(), aLine.BStart() + aDeltaBCoord, aLine.BSize(), None);

                // #ifdef REALLY_NOISY_REFLOW ... NOT PORTED: debug printing.

                // Mark the line dirty if it was or is affected by a float
                // We actually only really need to reflow if the amount of impact
                // changes, but that's not straightforward to check
                if wasImpactedByFloat || floatAvailableSpace.HasFloats() {
                    aLine.MarkDirty();
                }
            }
        }
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp LineHasClear
fn LineHasClear(aLine: &nsLineBox) -> bool {
    if aLine.IsBlock() {
        aLine.HasFloatClearTypeBefore() || aLine.mFirstChild.unwrap().HasAnyStateBits(NS_BLOCK_HAS_CLEAR_CHILDREN) || !nsBlockFrame::BlockCanIntersectFloats(aLine.mFirstChild.unwrap())
    } else {
        aLine.HasFloatClearTypeAfter()
    }
}

impl nsBlockFrame {
    /**
     * Reparent a whole list of floats from aOldParent to this block.  The
     * floats might be taken from aOldParent's overflow list. They will be
     * removed from the list. They end up appended to our floats list.
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ReparentFloats
    pub fn ReparentFloats(&self, aFirstFrame: nsIFrame, aOldParent: nsBlockFrame, aReparentSiblings: bool) {
        let mut list = nsFrameList::default();
        aOldParent.CollectFloats(aFirstFrame, &mut list, aReparentSiblings);
        if list.NotEmpty() {
            for f in &list {
                MOZ_ASSERT!(!f.HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW), "CollectFloats should've removed that bit");
                nsContainerFrame::ReparentFrame(f, aOldParent.0, self.0);
            }
            self.EnsureFloats().AppendFrames(None, list);
        }
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp DumpLine
// NOT PORTED: debug printing (#ifdef DEBUG, nsBlockFrame::gNoisyReflow); a no-op.
fn DumpLine(aState: &BlockReflowState, aLine: *mut nsLineBox, aDeltaBCoord: nscoord, aDeltaIndent: i32) {}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::LinesAreEmpty
    pub fn LinesAreEmpty(&self) -> bool {
        for line in self.Lines() {
            if !line.IsEmpty() {
                return false;
            }
        }
        true
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ReflowDirtyLines
    // Returns whether an overflow-wrap break was used anywhere.
    pub fn ReflowDirtyLines(&self, aState: &mut BlockReflowState) -> bool {
        let mut keepGoing = true;
        let mut foundAnyClears = aState.mTrailingClearFromPIF != UsedClear::None;
        let mut willReflowAgain = false;
        let mut usedOverflowWrap = false;

        // #ifdef DEBUG if (gNoisyReflow) {...}  NOT PORTED: debug printing.

        let selfDirty = self.HasAnyStateBits(NS_FRAME_IS_DIRTY) || (aState.mReflowInput.IsBResize() && self.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE));

        // Reflow our last line if our availableBSize has increased
        // so that we (and our last child) pull up content as necessary
        if aState.mReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE && self.GetNextInFlow().is_some() && aState.mReflowInput.AvailableBSize() > self.GetLogicalSize__().BSize(aState.mReflowInput.GetWritingMode()) {
            let mut lastLine = self.LinesEnd();
            if lastLine != self.LinesBegin() {
                lastLine.operator_dec();
                lastLine.MarkDirty();
            }
        }
        // the amount by which we will slide the current line if it is not
        // dirty
        let mut deltaBCoord: nscoord = 0;

        // whether we did NOT reflow the previous line and thus we need to
        // recompute the carried out margin before the line if we want to
        // reflow it or if its previous margin is dirty
        let mut needToRecoverState = false;
        // Float continuations were reflowed in ReflowPushedFloats
        let mut reflowedFloat = self.HasFloats() && unsafe { &*self.GetFloats() }.FirstChild().unwrap().HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW);
        let mut lastLineMovedUp = false;
        // We save up information about BR-clearance here
        let mut inlineFloatClearType: UsedClear = aState.mTrailingClearFromPIF;

        let mut line = self.LinesBegin();
        let line_end = self.LinesEnd();

        // Determine if children of this frame could have breaks between them for
        // page names.
        //
        // We need to check for paginated layout, the named-page pref, and if the
        // available block-size is constrained.
        //
        // Note that we need to check for paginated layout as named-pages are only
        // used during paginated reflow. We need to additionally check for
        // unconstrained block-size to avoid introducing fragmentation breaks during
        // "measuring" reflows within an overall paginated reflow, and to avoid
        // fragmentation in monolithic containers like 'inline-block'.
        //
        // Because we can only break for named pages using Class A breakpoints, we
        // also need to check that the block flow direction of the containing frame
        // of these items (which is this block) is parallel to that of this page.
        // See: https://www.w3.org/TR/css-break-3/#btw-blocks
        // NOT PORTED: pagination (named pages): `canBreakForPageNames` is always false
        // (nsBlockFrame.cpp lines 3196-3226 at GECKO_REV).
        let presCtx = aState.mPresContext;
        let canBreakForPageNames = false;

        // Reflow the lines that are already ours
        while line != line_end {
            DumpLine(aState, line.get() as *mut nsLineBox, deltaBCoord, 0);
            // #ifdef DEBUG AutoNoisyIndenter indent2(gNoisyReflow);  NOT PORTED: debug printing.

            if selfDirty {
                line.MarkDirty();
            }

            // This really sucks, but we have to look inside any blocks that have clear
            // elements inside them.
            // XXX what can we do smarter here?
            if !line.IsDirty() && line.IsBlock() && line.mFirstChild.unwrap().HasAnyStateBits(NS_BLOCK_HAS_CLEAR_CHILDREN) && aState.FloatManager().HasAnyFloats() {
                line.MarkDirty();
            }

            let mut floatAvoidingBlock: Option<nsIFrame> = None;
            if line.IsBlock() && !nsBlockFrame::BlockCanIntersectFloats(line.mFirstChild.unwrap()) {
                floatAvoidingBlock = line.mFirstChild;
            }

            // We have to reflow the line if it's a block whose clearance
            // might have changed, so detect that.
            if !line.IsDirty() && (line.HasFloatClearTypeBefore() || floatAvoidingBlock.is_some()) {
                let mut curBCoord: nscoord = aState.mBCoord;
                // See where we would be after applying any clearance due to
                // BRs.
                if inlineFloatClearType != UsedClear::None {
                    curBCoord = aState.ClearFloats(curBCoord, inlineFloatClearType, None).0;
                }

                let (newBCoord, result) = aState.ClearFloats(curBCoord, line.FloatClearTypeBefore(), floatAvoidingBlock);

                if line.HasClearance() {
                    // Reflow the line if it might not have clearance anymore.
                    if result == ClearFloatsResult::BCoordNoChange
                        // aState.mBCoord is the clearance point which should be the
                        // block-start border-edge of the block frame. If sliding the
                        // block by deltaBCoord isn't going to put it in the predicted
                        // position, then we'd better reflow the line.
                        || newBCoord != line.BStart() + deltaBCoord
                    {
                        line.MarkDirty();
                    }
                } else {
                    // Reflow the line if the line might have clearance now.
                    if result != ClearFloatsResult::BCoordNoChange {
                        line.MarkDirty();
                    }
                }
            }

            // We might have to reflow a line that is after a clearing BR.
            if inlineFloatClearType != UsedClear::None {
                aState.mBCoord = aState.ClearFloats(aState.mBCoord, inlineFloatClearType, None).0;
                if aState.mBCoord != line.BStart() + deltaBCoord {
                    // SlideLine is not going to put the line where the clearance
                    // put it. Reflow the line to be sure.
                    line.MarkDirty();
                }
                inlineFloatClearType = UsedClear::None;
            }

            let previousMarginWasDirty = line.IsPreviousMarginDirty();
            if previousMarginWasDirty {
                // If the previous margin is dirty, reflow the current line
                line.MarkDirty();
                line.ClearPreviousMarginDirty();
            } else if aState.ContentBSize() != NS_UNCONSTRAINEDSIZE {
                let scrollableOverflowBEnd: nscoord = LogicalRect::new__WritingMode_nsRect_nsSize(line.mWritingMode, line.ScrollableOverflowRect(), line.mContainerSize).BEnd(line.mWritingMode);
                if scrollableOverflowBEnd + deltaBCoord > aState.ContentBEnd() {
                    // Lines that aren't dirty but get slid past our available block-size
                    // constraint must be reflowed.
                    line.MarkDirty();
                }
            }

            if !line.IsDirty() {
                let isPaginated =
                    // Last column can be reflowed unconstrained during column balancing.
                    // Hence the additional NS_FRAME_HAS_MULTI_COLUMN_ANCESTOR bit check
                    // as a fail-safe fallback.
                    aState.mReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE ||
                    // NOT PORTED: columns (NS_FRAME_HAS_MULTI_COLUMN_ANCESTOR)
                    // Table can also be reflowed unconstrained during printing.
                    aState.mPresContext.IsPaginated();
                if isPaginated {
                    // We are in a paginated context, i.e. in columns or pages.
                    let mayContainFloats = line.IsBlock() || line.HasFloats() || line.HadFloatPushed();
                    if mayContainFloats {
                        // The following if-else conditions check whether this line -- which
                        // might have floats in its subtree, or has floats as direct children,
                        // or had floats pushed -- needs to be reflowed.
                        if deltaBCoord != 0 || aState.mReflowInput.IsBResize() {
                            // The distance to the block-end edge might have changed. Reflow the
                            // line both because the breakpoints within its floats may have
                            // changed and because we might have to push/pull the floats in
                            // their entirety.
                            line.MarkDirty();
                        } else if self.HasPushedFloats() {
                            // We had pushed floats which haven't been drained by our
                            // next-in-flow, which means our parent is currently reflowing us
                            // again due to clearance without creating a next-in-flow for us.
                            // Reflow the line to redo the floats split logic to correctly set
                            // our reflow status.
                            line.MarkDirty();
                        } else if aState.mReflowInput.mFlags.mMustReflowPlaceholders {
                            // Reflow the line (that may containing a float's placeholder frame)
                            // if our parent tells us to do so.
                            line.MarkDirty();
                        } else if aState.mReflowInput.mFlags.mMovedBlockFragments {
                            // Our parent's line containing us moved to a different fragment.
                            // Reflow the line because the decision about whether the float fits
                            // may be different in a different fragment.
                            line.MarkDirty();
                        }
                    }
                }
            }

            if !line.IsDirty() {
                // See if there's any reflow damage that requires that we mark the
                // line dirty.
                self.PropagateFloatDamage(aState, line.get() as *mut nsLineBox, deltaBCoord);
            }

            // If the container size has changed, reset mContainerSize. If the
            // line's writing mode is not ltr, or if the line is not left-aligned, also
            // mark the line dirty.
            if aState.ContainerSize() != line.mContainerSize {
                line.mContainerSize = aState.ContainerSize();

                let isLastLine = line.get() == self.mLines().back() && self.GetNextInFlow().is_none();
                let align = if isLastLine { nsStyleTextAdapter::of(self.0).TextAlignForLastLine() } else { nsStyleTextAdapter::of(self.0).mTextAlign() };
                if line.mWritingMode.IsVertical() || line.mWritingMode.IsBidiRTL() || !IsAlignedLeft(align, self.Style().get_inherited_box().direction, self.Style().get_text().unicode_bidi, self.0) {
                    line.MarkDirty();
                }
            }

            // Check for a page break caused by CSS named pages.
            //
            // We should break for named pages when two frames meet at a class A
            // breakpoint, where the first frame has a different end page value to the
            // second frame's start page value. canBreakForPageNames is true iff
            // children of this frame can form class A breakpoints, and that we are not
            // in a measurement reflow or in a monolithic container such as
            // 'inline-block'.
            //
            // We specifically do not want to cause a page-break for named pages when
            // we are at the top of a page. This would otherwise happen when the
            // previous sibling is an nsPageBreakFrame, or all previous siblings on the
            // current page are zero-height. The latter may not be per-spec, but is
            // compatible with Chrome's implementation of named pages.
            // NOT PORTED: pagination (named pages), nsBlockFrame.cpp lines 3382-3413 at GECKO_REV.
            let shouldBreakForPageName = false;
            let _ = canBreakForPageNames;

            if needToRecoverState && line.IsDirty() {
                // We need to reconstruct the block-end margin only if we didn't
                // reflow the previous line and we do need to reflow (or repair
                // the block-start position of) the next line.
                aState.ReconstructMarginBefore(line);
            }

            let reflowedPrevLine = !needToRecoverState;
            if needToRecoverState {
                needToRecoverState = false;

                // Update aState.mPrevChild as if we had reflowed all of the frames in
                // this line.
                if line.IsDirty() {
                    NS_ASSERTION!(line.mFirstChild.unwrap().GetPrevSibling() == line.prev().LastChild(), "unexpected line frames");
                    aState.mPrevChild = line.mFirstChild.unwrap().GetPrevSibling();
                }
            }

            // Now repair the line and update |aState.mBCoord| by calling
            // |ReflowLine| or |SlideLine|.
            // If we're going to reflow everything again, then no need to reflow
            // the dirty line ... unless the line has floats, in which case we'd
            // better reflow it now to refresh its float cache, which may contain
            // dangling frame pointers! Ugh! This reflow of the line may be
            // incorrect because we skipped reflowing previous lines (e.g., floats
            // may be placed incorrectly), but that's OK because we'll mark the
            // line dirty below under "if (aState.mReflowInput.mDiscoveredClearance..."
            if line.IsDirty() && (line.HasFloats() || !willReflowAgain) {
                lastLineMovedUp = true;

                let maybeReflowingForFirstTime = line.IStart() == 0 && line.BStart() == 0 && line.ISize() == 0 && line.BSize() == 0;

                // Compute the dirty lines "before" BEnd, after factoring in
                // the running deltaBCoord value - the running value is implicit in
                // aState.mBCoord.
                let oldB: nscoord = line.BStart();
                let oldBMost: nscoord = line.BEnd();

                NS_ASSERTION!(!willReflowAgain || !line.IsBlock(), "Don't reflow blocks while willReflowAgain is true, reflow of block abs-pos children depends on this");

                if shouldBreakForPageName {
                    // Immediately fragment for page-name. It is possible we could break
                    // out of the loop right here, but this should make it more similar to
                    // what happens when reflow causes fragmentation.
                    // Set the page name, so that PushTruncatedLine does not need to
                    // recalculate the new page name.
                    // NOT PORTED: pagination (SetNextPageContentFramePageName, PushTruncatedLine with
                    // ComputeNewPageNameIfNeeded::No).
                    unreachable!("NOT PORTED: pagination (named pages)");
                } else {
                    // Reflow the dirty line. If it's an incremental reflow, then force
                    // it to invalidate the dirty area if necessary
                    usedOverflowWrap |= self.ReflowLine(aState, line, &mut keepGoing);
                }

                if aState.mReflowInput.WillReflowAgainForClearance() {
                    line.MarkDirty();
                    willReflowAgain = true;
                    // Note that once we've entered this state, every line that gets here
                    // (e.g. because it has floats) gets marked dirty and reflowed again.
                    // in the next pass. This is important, see above.
                }

                if line.HasFloats() {
                    reflowedFloat = true;
                }

                if !keepGoing {
                    DumpLine(aState, line.get() as *mut nsLineBox, deltaBCoord, -1);
                    if 0 == line.GetChildCount() {
                        self.DeleteLine(aState, line, line_end);
                    }
                    break;
                }

                // Test to see whether the margin that should be carried out
                // to the next line (NL) might have changed. In ReflowBlockFrame
                // we call nextLine->MarkPreviousMarginDirty if the block's
                // actual carried-out block-end margin changed. So here we only
                // need to worry about the following effects:
                // 1) the line was just created, and it might now be blocking
                // a carried-out block-end margin from previous lines that
                // used to reach NL from reaching NL
                // 2) the line used to be empty, and is now not empty,
                // thus blocking a carried-out block-end margin from previous lines
                // that used to reach NL from reaching NL
                // 3) the line wasn't empty, but now is, so a carried-out
                // block-end margin from previous lines that didn't used to reach NL
                // now does
                // 4) the line might have changed in a way that affects NL's
                // ShouldApplyBStartMargin decision. The three things that matter
                // are the line's emptiness, its adjacency to the block-start edge of the
                // block, and whether it has clearance (the latter only matters if the
                // block was and is adjacent to the block-start and empty).
                //
                // If the line is empty now, we can't reliably tell if the line was empty
                // before, so we just assume it was and do
                // nextLine->MarkPreviousMarginDirty. This means the checks in 4) are
                // redundant; if the line is empty now we don't need to check 4), but if
                // the line is not empty now and we're sure it wasn't empty before, any
                // adjacency and clearance changes are irrelevant to the result of
                // nextLine->ShouldApplyBStartMargin.
                if line.next_() != self.LinesEnd() {
                    let maybeWasEmpty = oldB == line.next_().BStart();
                    let isEmpty = {
                        let mut l = line;
                        l.CachedIsEmpty()
                    };
                    if maybeReflowingForFirstTime /*1*/ || (isEmpty || maybeWasEmpty)
                    /*2/3/4*/
                    {
                        let mut next = line.next_();
                        next.MarkPreviousMarginDirty();
                        // since it's marked dirty, nobody will care about |deltaBCoord|
                    }
                }

                // If the line was just reflowed for the first time, then its
                // old mBounds cannot be trusted so this deltaBCoord computation is
                // bogus. But that's OK because we just did
                // MarkPreviousMarginDirty on the next line which will force it
                // to be reflowed, so this computation of deltaBCoord will not be
                // used.
                deltaBCoord = line.BEnd() - oldBMost;

                // Now do an interrupt check. We want to do this only in the case when we
                // actually reflow the line, so that if we get back in here we'll get
                // further on the reflow before interrupting.
                // NOT PORTED: reflow interrupts (aState.mPresContext->CheckForInterrupt(this)).
            } else {
                // NOT PORTED: fragmentation: aState.mOverflowTracker->Skip(line->mFirstChild, aState.mReflowStatus);
                // Nop except for blocks (we don't create overflow container
                // continuations for any inlines atm), so only checking mFirstChild
                // is enough

                lastLineMovedUp = deltaBCoord < 0;

                if deltaBCoord != 0 {
                    self.SlideLine(aState, line.get() as *mut nsLineBox, deltaBCoord);
                }

                NS_ASSERTION!(!line.IsDirty() || !line.HasFloats(), "Possibly stale float cache here!");
                if willReflowAgain && line.IsBlock() {
                    // If we're going to reflow everything again, and this line is a block,
                    // then there is no need to recover float state. The line may contain
                    // other lines with floats, but in that case RecoverStateFrom would only
                    // add floats to the float manager. We don't need to do that because
                    // everything's going to get reflowed again "for real". Calling
                    // RecoverStateFrom in this situation could be lethal because the
                    // block's descendant lines may have float caches containing dangling
                    // frame pointers. Ugh!
                    // If this line is inline, then we need to recover its state now
                    // to make sure that we don't forget to move its floats by deltaBCoord.
                } else {
                    // XXX EVIL O(N^2) EVIL
                    aState.RecoverStateFrom(line, deltaBCoord);
                }

                // Keep mBCoord up to date in case we're propagating reflow damage
                // and also because our final height may depend on it. If the
                // line is inlines, then only update mBCoord if the line is not
                // empty, because that's what PlaceLine does. (Empty blocks may
                // want to update mBCoord, e.g. if they have clearance.)
                if line.IsBlock() || !{
                    let mut l = line;
                    l.CachedIsEmpty()
                } {
                    aState.mBCoord = line.BEnd();
                }

                needToRecoverState = true;

                if reflowedPrevLine && !line.IsBlock() && aState.mPresContext.HasPendingInterrupt() {
                    // Need to make sure to pull overflows from any prev-in-flows
                    let mut inlineKid = line.mFirstChild;
                    while let Some(k) = inlineKid {
                        k.PullOverflowsFromPrevInFlow();
                        inlineKid = k.PrincipalChildList_FirstChild();
                    }
                }
            }

            // Record if we need to clear floats before reflowing the next
            // line. Note that inlineFloatClearType will be handled and
            // cleared before the next line is processed, so there is no
            // need to combine break types here.
            if line.HasFloatClearTypeAfter() {
                inlineFloatClearType = line.FloatClearTypeAfter();
            }

            if LineHasClear(&line) {
                foundAnyClears = true;
            }

            DumpLine(aState, line.get() as *mut nsLineBox, deltaBCoord, -1);

            if aState.mPresContext.HasPendingInterrupt() {
                willReflowAgain = true;
                // Another option here might be to leave |line| clean if
                // !HasPendingInterrupt() before the CheckForInterrupt() call, since in
                // that case the line really did reflow as it should have.  Not sure
                // whether that would be safe, so doing this for now instead.  Also not
                // sure whether we really want to mark all lines dirty after an
                // interrupt, but until we get better at propagating float damage we
                // really do need to do it this way; see comments inside MarkLineDirty.
                self.MarkLineDirtyForInterrupt(line.get() as *mut nsLineBox);
            }

            // (the increments of the for statement)
            line.operator_inc();
            aState.AdvanceToNextLine();
        }

        // Handle BR-clearance from the last line of the block
        if inlineFloatClearType != UsedClear::None {
            aState.mBCoord = aState.ClearFloats(aState.mBCoord, inlineFloatClearType, None).0;
        }

        if needToRecoverState {
            // Is this expensive?
            aState.ReconstructMarginBefore(line);

            // Update aState.mPrevChild as if we had reflowed all of the frames in
            // the last line.
            NS_ASSERTION!(line == line_end || line.mFirstChild.unwrap().GetPrevSibling() == line.prev().LastChild(), "unexpected line frames");
            aState.mPrevChild = if line == line_end { self.mFrames().LastChild() } else { line.mFirstChild.unwrap().GetPrevSibling() };
        }

        // We can skip trying to pull up the next line if our height is constrained
        // (so we can report being incomplete) and there is no next in flow or we
        // were told not to or we know it will be futile, i.e.,
        // -- the next in flow is not changing
        // -- and we cannot have added more space for its first line to be
        // pulled up into,
        // -- it's an incremental reflow of a descendant
        // -- and we didn't reflow any floats (so the available space
        // didn't change)
        // -- my chain of next-in-flows either has no first line, or its first
        // line isn't dirty.
        let heightConstrained = aState.mReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE;
        let mut skipPull = willReflowAgain && heightConstrained;
        if !skipPull && heightConstrained && aState.mNextInFlow.is_some() && (aState.mReflowInput.mFlags.mNextInFlowUntouched && !lastLineMovedUp && !self.HasAnyStateBits(NS_FRAME_IS_DIRTY) && !reflowedFloat) {
            // We'll place lineIter at the last line of this block, so that
            // nsBlockInFlowLineIterator::Next() will take us to the first
            // line of my next-in-flow-chain.  (But first, check that I
            // have any lines -- if I don't, just bail out of this
            // optimization.)
            let mut lineIter = self.LinesEnd();
            if lineIter != self.LinesBegin() {
                lineIter.operator_dec(); // I have lines; step back from dummy iterator to last line.
                let mut bifLineIter = nsBlockInFlowLineIterator::new__nsBlockFrame_LineIterator(*self, lineIter);

                // Check for next-in-flow-chain's first line.
                // (First, see if there is such a line, and second, see if it's clean)
                if !bifLineIter.Next() || !bifLineIter.GetLine().IsDirty() {
                    skipPull = true;
                }
            }
        }

        if skipPull && aState.mNextInFlow.is_some() {
            NS_ASSERTION!(heightConstrained, "Height should be constrained here\n");
            if aState.mNextInFlow.unwrap().IsTrueOverflowContainer() {
                aState.mReflowStatus.SetOverflowIncomplete();
            } else {
                aState.mReflowStatus.SetIncomplete();
            }
        }

        if !skipPull && aState.mNextInFlow.is_some() {
            // Pull data from a next-in-flow if there's still room for more
            // content here.
            while keepGoing && aState.mNextInFlow.is_some() {
                // Grab first line from our next-in-flow
                let nextInFlow = aState.mNextInFlow.unwrap();
                let mut pulledLine: *mut nsLineBox = std::ptr::null_mut();
                let mut pulledFrames = nsFrameList::default();
                if !nextInFlow.mLines().empty() {
                    RemoveFirstLine(nextInFlow.mLines(), &mut *nextInFlow.mFrames(), &mut pulledLine, &mut pulledFrames);
                    self.ClearLineCursors();
                } else {
                    // Grab an overflow line if there are any
                    let overflowLines = nextInFlow.GetOverflowLines();
                    if overflowLines.is_null() {
                        aState.mNextInFlow = nextInFlow.GetNextInFlow().map(nsBlockFrame::from_frame);
                        continue;
                    }
                    let overflowLines = unsafe { &mut *overflowLines };
                    let last = RemoveFirstLine(&mut overflowLines.mLines, &mut overflowLines.mFrames, &mut pulledLine, &mut pulledFrames);
                    if last {
                        nextInFlow.DestroyOverflowLines();
                    }
                }

                if pulledFrames.IsEmpty() {
                    // The line is empty. Try the next one.
                    NS_ASSERTION!(unsafe { &*pulledLine }.GetChildCount() == 0 && unsafe { &*pulledLine }.mFirstChild.is_none(), "bad empty line");
                    nextInFlow.FreeLineBox(pulledLine);
                    continue;
                }

                if nextInFlow.MaybeHasLineCursor() {
                    if pulledLine == nextInFlow.GetLineCursorForDisplay() {
                        nextInFlow.ClearLineCursorForDisplay();
                    }
                    if pulledLine == nextInFlow.GetLineCursorForQuery() {
                        nextInFlow.ClearLineCursorForQuery();
                    }
                }
                nsContainerFrame::ReparentFrames(&pulledFrames, nextInFlow.0, self.0);
                unsafe { &mut *pulledLine }.SetMovedFragments();

                NS_ASSERTION!(pulledFrames.LastChild() == unsafe { &*pulledLine }.LastChild(), "Unexpected last frame");
                NS_ASSERTION!(aState.mPrevChild.is_some() || self.mLines().empty(), "should have a prevchild here");
                NS_ASSERTION!(aState.mPrevChild == self.mFrames().LastChild(), "Incorrect aState.mPrevChild before inserting line at end");

                // Shift pulledLine's frames into our mFrames list.
                self.mFrames().AppendFrames(None, pulledFrames);

                // Add line to our line list, and set its last child as our new prev-child
                line = self.mLines().before_insert(self.LinesEnd(), pulledLine);
                aState.mPrevChild = self.mFrames().LastChild();

                // Reparent floats whose placeholders are in the line.
                self.ReparentFloats(unsafe { &*pulledLine }.mFirstChild.unwrap(), nextInFlow, true);

                DumpLine(aState, pulledLine, deltaBCoord, 0);
                // #ifdef DEBUG AutoNoisyIndenter indent2(gNoisyReflow);  NOT PORTED: debug printing.

                if aState.mPresContext.HasPendingInterrupt() {
                    self.MarkLineDirtyForInterrupt(line.get() as *mut nsLineBox);
                } else {
                    // Now reflow it and any lines that it makes during it's reflow
                    // (we have to loop here because reflowing the line may cause a new
                    // line to be created; see SplitLine's callers for examples of
                    // when this happens).
                    while line != self.LinesEnd() {
                        usedOverflowWrap |= self.ReflowLine(aState, line, &mut keepGoing);

                        if aState.mReflowInput.WillReflowAgainForClearance() {
                            line.MarkDirty();
                            keepGoing = false;
                            // If we are going to be reflowed again by our ancestor due to a
                            // clearance frame being discovered, reset the reflow completion
                            // status. The current status does not matter to our parent frame
                            // since we will reflow again anyway. This also prevents the false
                            // assumption that we are incomplete when reflowing under an
                            // unconstrained available block-size.
                            aState.mReflowStatus.Reset();
                            break;
                        }

                        DumpLine(aState, line.get() as *mut nsLineBox, deltaBCoord, -1);
                        if !keepGoing {
                            if 0 == line.GetChildCount() {
                                self.DeleteLine(aState, line, line_end);
                            }
                            break;
                        }

                        if LineHasClear(&line) {
                            foundAnyClears = true;
                        }

                        // NOT PORTED: reflow interrupts:
                        // if (aState.mPresContext->CheckForInterrupt(this)) {
                        //   MarkLineDirtyForInterrupt(line); break; }

                        // If this is an inline frame then its time to stop
                        line.operator_inc();
                        aState.AdvanceToNextLine();
                    }
                }
            }

            if aState.mReflowStatus.IsIncomplete() {
                aState.mReflowStatus.SetNextInFlowNeedsReflow();
            } // XXXfr shouldn't set this flag when nextinflow has no lines
        }

        // Handle an odd-ball case: a list-item with no lines
        let outsideMarker = self.GetOutsideMarker();
        if let Some(outsideMarker) = outsideMarker {
            if self.mLines().empty() {
                let mut metrics = ReflowOutput::new__ReflowInput(aState.mReflowInput);
                let wm = aState.mReflowInput.GetWritingMode();
                self.ReflowOutsideMarker(outsideMarker, aState, &mut metrics, aState.mReflowInput.ComputedPhysicalBorderPadding().top);
                NS_ASSERTION!(!self.MarkerIsEmpty(outsideMarker) || metrics.BSize(wm) == 0, "empty ::marker frame took up space");

                if !self.MarkerIsEmpty(outsideMarker) {
                    // There are no lines so we have to fake up some y motion so that
                    // we end up with *some* height.
                    // (Note: if we're layout-contained, we have to be sure to leave our
                    // ReflowOutput's BlockStartAscent() (i.e. the baseline) untouched,
                    // because layout-contained frames have no baseline.)
                    if !aState.mReflowInput.mStyleDisplay.IsContainLayout() && metrics.BlockStartAscent() == ReflowOutput::ASK_FOR_BASELINE {
                        let mut ascent: nscoord = 0;
                        let wm = aState.mReflowInput.GetWritingMode();
                        if nsLayoutUtils::GetFirstLineBaseline(wm, outsideMarker, &mut ascent) {
                            metrics.SetBlockStartAscent(ascent);
                        } else {
                            metrics.SetBlockStartAscent(metrics.BSize(wm));
                        }
                    }

                    let fm = nsLayoutUtils::GetInflatedFontMetricsForFrame(self.0);

                    let minAscent: nscoord = nsLayoutUtils::GetCenteredFontBaseline(&fm, aState.mMinLineHeight, wm.IsLineInverted());
                    let minDescent: nscoord = aState.mMinLineHeight - minAscent;

                    aState.mBCoord += std::cmp::max(minAscent, metrics.BlockStartAscent()) + std::cmp::max(minDescent, metrics.BSize(wm) - metrics.BlockStartAscent());

                    let offset: nscoord = minAscent - metrics.BlockStartAscent();
                    if offset > 0 {
                        outsideMarker.SetRect(outsideMarker.GetRect() + nsPoint::new(0, offset));
                    }
                }
            }
        }

        if self.LinesAreEmpty() && self.ShouldHaveLineIfEmpty() {
            aState.mBCoord += aState.mMinLineHeight;
        }

        if foundAnyClears {
            self.AddStateBits(NS_BLOCK_HAS_CLEAR_CHILDREN);
        } else {
            self.RemoveStateBits(NS_BLOCK_HAS_CLEAR_CHILDREN);
        }

        // #ifdef DEBUG VerifyLines(true); VerifyOverflowSituation(); if (gNoisyReflow) {...}
        // NOT PORTED: debug-only verification and printing.
        let _ = presCtx;

        usedOverflowWrap
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::MarkLineDirtyForInterrupt
    pub fn MarkLineDirtyForInterrupt(&self, aLine: *mut nsLineBox) {
        let aLine = unsafe { &mut *aLine };
        aLine.MarkDirty();

        // Just checking NS_FRAME_IS_DIRTY is ok, because we've already
        // marked the lines that need to be marked dirty based on our
        // vertical resize stuff.  So we'll definitely reflow all those kids;
        // the only question is how they should behave.
        if self.HasAnyStateBits(NS_FRAME_IS_DIRTY) {
            // Mark all our child frames dirty so we make sure to reflow them
            // later.
            for f in aLine.ChildFrames() {
                f.MarkSubtreeDirty();
            }
            // And mark all the floats whose reflows we might be skipping dirty too.
            if aLine.HasFloats() {
                for &f in aLine.Floats() {
                    f.MarkSubtreeDirty();
                }
            }
        } else {
            // Dirty all the descendant lines of block kids to handle float damage,
            // since our nsFloatManager will go away by the next time we're reflowing.
            // XXXbz Can we do something more like what PropagateFloatDamage does?
            // Would need to sort out the exact business with mBlockDelta for that....
            // This marks way too much dirty.  If we ever make this better, revisit
            // which lines we mark dirty in the interrupt case in ReflowDirtyLines.
            let bf = nsBlockFrame::do_QueryFrame(aLine.mFirstChild.unwrap());
            if let Some(bf) = bf {
                MarkAllDescendantLinesDirty(bf);
            }
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DeleteLine
    pub fn DeleteLine(&self, aState: &mut BlockReflowState, mut aLine: nsLineList_iterator, aLineEnd: nsLineList_iterator) {
        MOZ_ASSERT!(0 == aLine.GetChildCount(), "can't delete !empty line");
        if 0 == aLine.GetChildCount() {
            NS_ASSERTION!(aState.mCurrentLine == aLine, "using function more generally than designed, but perhaps OK now");
            let line: *mut nsLineBox = aLine.get_mut();
            aLine = self.mLines().erase(aLine);
            self.FreeLineBox(line);
            self.ClearLineCursors();
            // Mark the previous margin of the next line dirty since we need to
            // recompute its top position.
            if aLine != aLineEnd {
                aLine.MarkPreviousMarginDirty();
            }
        }
    }

    /**
     * Reflow a line. The line will either contain a single block frame
     * or contain 1 or more inline frames. aKeepReflowGoing indicates
     * whether or not the caller should continue to reflow more lines.
     * Returns true if the reflow used an overflow-wrap breakpoint.
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ReflowLine
    pub fn ReflowLine(&self, aState: &mut BlockReflowState, mut aLine: nsLineList_iterator, aKeepReflowGoing: &mut bool) -> bool {
        MOZ_ASSERT!(aLine.GetChildCount() != 0, "reflowing empty line");

        // Setup the line-layout for the new line
        aState.mCurrentLine = aLine;
        aLine.ClearDirty();
        aLine.InvalidateCachedIsEmpty();
        aLine.ClearHadFloatPushed();

        // If this line contains a single block that is hidden by `content-visibility`
        // don't reflow the line. If this line contains inlines and the first one is
        // hidden by `content-visibility`, all of them are, so avoid reflow in that
        // case as well.
        // For frames that own anonymous children, even the first child is hidden by
        // `content-visibility`, there could be some anonymous children need reflow,
        // so we don't skip reflow this line.
        let firstChild = aLine.mFirstChild.unwrap();
        // NOT PORTED: content-visibility (IsHiddenByContentVisibilityOfInFlowParentForLayout is false),
        // so the early return `return false;` does not happen.
        if firstChild.IsHiddenByContentVisibilityOfInFlowParentForLayout() && !self.HasAnyStateBits(NS_FRAME_OWNS_ANON_BOXES) {
            return false;
        }

        // Now that we know what kind of line we have, reflow it
        let mut usedOverflowWrap = false;
        if aLine.IsBlock() {
            self.ReflowBlockFrame(aState, aLine, aKeepReflowGoing);
        } else {
            aLine.SetLineWrapped(false);
            usedOverflowWrap = self.ReflowInlineFrames(aState, aLine, aKeepReflowGoing);

            // Store the line's float edges for overflow marker analysis if needed.
            aLine.ClearFloatEdges();
            if aState.mFlags.mCanHaveOverflowMarkers {
                let wm = aLine.mWritingMode;
                let r: nsFlowAreaRect = aState.GetFloatAvailableSpaceForBSize(wm, aLine.BStart(), aLine.BSize(), None);
                if r.HasFloats() {
                    let so: LogicalRect = aLine.GetOverflowArea__OverflowType_WritingMode_nsSize(OverflowType::Scrollable, wm, aLine.mContainerSize);
                    let s: nscoord = r.mRect.IStart(wm);
                    let e: nscoord = r.mRect.IEnd(wm);
                    if so.IEnd(wm) > e || so.IStart(wm) < s {
                        // This line is overlapping a float - store the edges marking the area
                        // between the floats for text-overflow analysis.
                        aLine.SetFloatEdges(s, e);
                    }
                }
            }
        }

        aLine.ClearMovedFragments();

        usedOverflowWrap
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::PullFrame
    pub fn PullFrame(&self, aState: &mut BlockReflowState, aLine: nsLineList_iterator) -> Option<nsIFrame> {
        // First check our remaining lines.
        if self.LinesEnd() != aLine.next_() {
            return self.PullFrameFrom(aLine.get() as *mut nsLineBox, *self, aLine.next_());
        }

        NS_ASSERTION!(self.GetOverflowLines().is_null(), "Our overflow lines should have been removed at the start of reflow");

        // Try each next-in-flow.
        let mut nextInFlow = aState.mNextInFlow;
        while let Some(nif) = nextInFlow {
            if nif.mLines().empty() {
                nif.DrainSelfOverflowList();
            }
            if !nif.mLines().empty() {
                return self.PullFrameFrom(aLine.get() as *mut nsLineBox, nif, nif.mLines().begin_mut());
            }
            nextInFlow = nif.GetNextInFlow().map(nsBlockFrame::from_frame);
            aState.mNextInFlow = nextInFlow;
        }

        None
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::PullFrameFrom
    pub fn PullFrameFrom(&self, aLine: *mut nsLineBox, aFromContainer: nsBlockFrame, mut aFromLine: nsLineList_iterator) -> Option<nsIFrame> {
        let fromLine: *mut nsLineBox = aFromLine.get_mut();
        MOZ_ASSERT!(!fromLine.is_null(), "bad line to pull from");
        let fromLine = unsafe { &mut *fromLine };
        let aLine = unsafe { &mut *aLine };
        MOZ_ASSERT!(fromLine.GetChildCount() != 0, "empty line");
        MOZ_ASSERT!(aLine.GetChildCount() != 0, "empty line");
        MOZ_ASSERT!(!self.HasProperty(LineIteratorProperty()), "Shouldn't have line iterators mid-reflow");

        NS_ASSERTION!(fromLine.IsBlock() == fromLine.mFirstChild.unwrap().IsBlockOutside(), "Disagreement about whether it's a block or not");

        if fromLine.IsBlock() {
            // If our line is not empty and the child in aFromLine is a block
            // then we cannot pull up the frame into this line. In this case
            // we stop pulling.
            return None;
        }
        // Take frame from fromLine
        let frame = fromLine.mFirstChild.unwrap();
        let newFirstChild = frame.GetNextSibling();

        if aFromContainer != *self {
            // The frame is being pulled from a next-in-flow; therefore we need to add
            // it to our sibling list.
            MOZ_ASSERT!(aLine as *mut nsLineBox as *const nsLineBox == self.mLines().back());
            MOZ_ASSERT!(aFromLine == aFromContainer.mLines().begin_mut(), "should only pull from first line");
            aFromContainer.mFrames().RemoveFrame(frame);

            // When pushing and pulling frames we need to check for whether any
            // views need to be reparented.
            nsContainerFrame::ReparentFrame(frame, aFromContainer.0, self.0);
            self.mFrames().AppendFrame(None, frame);

            // The frame might have (or contain) floats that need to be brought
            // over too. (pass 'false' since there are no siblings to check)
            self.ReparentFloats(frame, aFromContainer, false);
        } else {
            MOZ_ASSERT!(aLine as *mut nsLineBox == aFromLine.prev().get() as *mut nsLineBox);
        }

        aLine.NoteFrameAdded(frame);
        fromLine.NoteFrameRemoved(frame);

        if fromLine.GetChildCount() > 0 {
            // Mark line dirty now that we pulled a child
            fromLine.MarkDirty();
            fromLine.mFirstChild = newFirstChild;
        } else {
            // Free up the fromLine now that it's empty.
            // Its bounds might need to be redrawn, though.
            if aFromLine.next_() != aFromContainer.mLines().end_mut() {
                let mut next = aFromLine.next_();
                next.MarkPreviousMarginDirty();
            }
            aFromContainer.mLines().erase(aFromLine);
            // aFromLine is now invalid
            aFromContainer.FreeLineBox(fromLine as *mut nsLineBox);
        }

        // #ifdef DEBUG VerifyLines(true); VerifyOverflowSituation();  NOT PORTED: debug-only verification.

        Some(frame)
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::SlideLine
    pub fn SlideLine(&self, aState: &mut BlockReflowState, aLine: *mut nsLineBox, aDeltaBCoord: nscoord) {
        MOZ_ASSERT!(aDeltaBCoord != 0, "why slide a line nowhere?");

        // Adjust line state
        unsafe { &mut *aLine }.SlideBy(aDeltaBCoord, aState.ContainerSize());

        // Adjust the frames in the line
        self.MoveChildFramesOfLine(aLine, aDeltaBCoord);
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::UpdateLineContainerSize
    pub fn UpdateLineContainerSize(&self, aLine: *mut nsLineBox, aNewContainerSize: nsSize) {
        let aLine = unsafe { &mut *aLine };
        if aNewContainerSize == aLine.mContainerSize {
            return;
        }

        // Adjust line state
        let sizeDelta: nsSize = aLine.UpdateContainerSize(aNewContainerSize);

        // Changing container width only matters if writing mode is vertical-rl
        if self.GetWritingMode().IsVerticalRL() {
            self.MoveChildFramesOfLine(aLine as *mut nsLineBox, sizeDelta.width);
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::MoveChildFramesOfLine
    pub fn MoveChildFramesOfLine(&self, aLine: *mut nsLineBox, aDeltaBCoord: nscoord) {
        let aLine = unsafe { &*aLine };
        // Adjust the frames in the line
        let Some(kid) = aLine.mFirstChild else {
            return;
        };

        let wm = self.GetWritingMode();
        let translation = LogicalPoint::new(wm, 0, aDeltaBCoord);

        if aLine.IsBlock() {
            if aDeltaBCoord != 0 {
                kid.MovePositionBy__WritingMode_LogicalPoint(wm, translation);
            }
        } else {
            // Adjust the block-dir coordinate of the frames in the line.
            // Note: we need to re-position views even if aDeltaBCoord is 0, because
            // one of our parent frames may have moved and so the view's position
            // relative to its parent may have changed.
            if aDeltaBCoord != 0 {
                for f in aLine.ChildFrames() {
                    f.MovePositionBy__WritingMode_LogicalPoint(wm, translation);
                }
            }
        }
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp IsNonAutoNonZeroBSize
#[inline]
fn IsNonAutoNonZeroBSize(aCoord: &StyleSize) -> bool {
    // The "extremum length" values (see ExtremumLength) that return true from
    // 'BehavesLikeInitialValueOnBlockAxis()' were originally aimed at
    // inline-size (or width, as it was before logicalization). For now, let them
    // return false here, so we treat them like 'auto' pending a real
    // implementation. (See bug 1126420.)
    if aCoord.BehavesLikeInitialValueOnBlockAxis() {
        return false;
    }
    if aCoord.BehavesLikeStretchOnBlockAxis() {
        // We return true for "stretch" because it's essentially equivalent to
        // "100%" for the purposes of this function (and this function returns true
        // for nonzero percentage values, in the final return statement below).
        return true;
    }
    MOZ_ASSERT!(aCoord.IsLengthPercentage());
    // If we evaluate the length/percent/calc at a percentage basis of
    // both nscoord_MAX and 0, and it's zero both ways, then it's a zero
    // length, percent, or combination thereof.  Test > 0 so we clamp
    // negative calc() results to 0.
    aCoord.AsLengthPercentage().Resolve(nscoord_MAX) > 0 || aCoord.AsLengthPercentage().Resolve(0) > 0
}

impl nsBlockFrame {
    /* virtual */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::IsSelfEmpty
    pub fn IsSelfEmpty(&self) -> bool {
        if self.IsHiddenByContentVisibilityOfInFlowParentForLayout() {
            return true;
        }

        // Blocks which are margin-roots (including inline-blocks) cannot be treated
        // as empty for margin-collapsing and other purposes. They're more like
        // replaced elements.
        if self.HasAnyStateBits(NS_BLOCK_BFC) {
            return false;
        }

        let wm = self.GetWritingMode();
        let position = self.StylePosition();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.0, std::ptr::null_mut());
        let bSize = position.BSize(wm, &anchorResolutionParams);

        if IsNonAutoNonZeroBSize(&position.MinBSize(wm, &anchorResolutionParams)) || IsNonAutoNonZeroBSize(&bSize) {
            return false;
        }

        // FIXME: Bug 1646100 - Take intrinsic size into account.
        // FIXME: Handle the case that both inline and block sizes are auto.
        // https://github.com/w3c/csswg-drafts/issues/5060.
        // Note: block-size could be zero or auto/intrinsic keywords here.
        if bSize.BehavesLikeInitialValueOnBlockAxis() && position.mAspectRatio().HasFiniteRatio() {
            return false;
        }

        let border = self.StyleBorder();
        let padding = self.StylePadding();

        if border.GetComputedBorderWidth(wm.PhysicalSide(LogicalSide::BStart)) != 0
            || border.GetComputedBorderWidth(wm.PhysicalSide(LogicalSide::BEnd)) != 0
            || !nsLayoutUtils::IsPaddingZero(&padding.mPadding_GetBStart(wm))
            || !nsLayoutUtils::IsPaddingZero(&padding.mPadding_GetBEnd(wm))
        {
            return false;
        }

        let outsideMarker = self.GetOutsideMarker();
        if let Some(outsideMarker) = outsideMarker {
            if !self.MarkerIsEmpty(outsideMarker) {
                return false;
            }
        }

        true
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::CachedIsEmpty
    pub fn CachedIsEmpty(&self) -> bool {
        if !self.IsSelfEmpty() {
            return false;
        }
        for mut line in self.Lines() {
            if !line.CachedIsEmpty() {
                return false;
            }
        }
        true
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        if !self.IsSelfEmpty() {
            return false;
        }
        self.LinesAreEmpty()
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ShouldApplyBStartMargin
    pub fn ShouldApplyBStartMargin(&self, aState: &mut BlockReflowState, aLine: *mut nsLineBox) -> bool {
        let aLine = unsafe { &*aLine };
        if aLine.mFirstChild.unwrap().IsPageBreakFrame() {
            // A page break frame consumes margins adjacent to it.
            // https://drafts.csswg.org/css-break/#break-margins
            return false;
        }

        if aState.mFlags.mShouldApplyBStartMargin {
            // Apply short-circuit check to avoid searching the line list
            return true;
        }

        if !aState.IsAdjacentWithBStart() {
            // If we aren't at the start block-coordinate then something of non-zero
            // height must have been placed. Therefore the childs block-start margin
            // applies.
            aState.mFlags.mShouldApplyBStartMargin = true;
            return true;
        }

        // Determine if this line is "essentially" the first line
        let mut line = self.LinesBegin();
        if aState.mFlags.mHasLineAdjacentToTop {
            line = aState.mLineAdjacentToTop;
        }
        while line.get() != aLine as *const nsLineBox {
            if !line.CachedIsEmpty() || line.HasClearance() {
                // A line which precedes aLine is non-empty, or has clearance,
                // so therefore the block-start margin applies.
                aState.mFlags.mShouldApplyBStartMargin = true;
                return true;
            }
            // No need to apply the block-start margin if the line has floats.  We
            // should collapse anyway (bug 44419)
            line.operator_inc();
            aState.mFlags.mHasLineAdjacentToTop = true;
            aState.mLineAdjacentToTop = line;
        }

        // The line being reflowed is "essentially" the first line in the
        // block. Therefore its block-start margin will be collapsed by the
        // generational collapsing logic with its parent (us).
        false
    }
}

// ADAPTER: the C++ calls `ReflowInput childReflowInput(aState.mPresContext, aState.mReflowInput, frame,
// availSize, cbSize)`, i.e. the constructor with its default arguments (no init flags, no size
// overrides, no compute-size flags, no anchor-pos cache).
fn NewChildReflowInput<'a>(aPresContext: nsPresContext, aParentReflowInput: &'a ReflowInput<'a>, aFrame: nsIFrame, aAvailableSpace: LogicalSize, aContainingBlockSize: Option<LogicalSize>) -> ReflowInput<'a> {
    ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
        &aPresContext,
        aParentReflowInput,
        aFrame,
        aAvailableSpace,
        aContainingBlockSize,
        EnumSet::default(),
        StyleSizeOverrides::default(),
        ComputeSizeFlags::default(),
        std::ptr::null_mut(),
    )
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ReflowBlockFrame
    pub fn ReflowBlockFrame(&self, aState: &mut BlockReflowState, mut aLine: nsLineList_iterator, aKeepReflowGoing: &mut bool) {
        MOZ_ASSERT!(*aKeepReflowGoing, "bad caller");

        let Some(frame) = aLine.mFirstChild else {
            NS_ASSERTION!(false, "program error - unexpected empty line");
            return;
        };

        // If the previous frame was a page-break-frame, then preemptively push this
        // frame to the next page.
        // This is primarily important for the placeholders for abspos frames, which
        // measure as zero height and then would be placed on this page.
        if aState.ContentBSize() != NS_UNCONSTRAINEDSIZE {
            let prev = frame.GetPrevSibling();
            if prev.map_or(false, |p| p.IsPageBreakFrame()) {
                self.PushTruncatedLine(aState, aLine, aKeepReflowGoing, nsBlockFrame_ComputeNewPageNameIfNeeded::Yes);
                return;
            }
        }

        // Prepare the block reflow engine
        // (aState.mReflowInput is `&'a ReflowInput<'a>`, a Copy of the reference.)
        let stateReflowInput: &ReflowInput = aState.mReflowInput;
        let mut brc = nsBlockReflowContext::new(aState.mPresContext, stateReflowInput);

        let cbWM = frame.GetContainingBlock__uint32_t(0).unwrap().GetWritingMode();
        let mut clearType: UsedClear = frame.StyleDisplay().UsedClear(cbWM);
        if aState.mTrailingClearFromPIF != UsedClear::None {
            clearType = nsLayoutUtils::CombineClearType(clearType, aState.mTrailingClearFromPIF);
            aState.mTrailingClearFromPIF = UsedClear::None;
        }

        // Clear past floats before the block if the clear style is not none
        aLine.ClearForcedLineBreak();
        if clearType != UsedClear::None {
            aLine.SetFloatClearTypeBefore(clearType);
        }

        // See if we should apply the block-start margin. If the block frame being
        // reflowed is a continuation, then we don't apply its block-start margin
        // because it's not significant. Otherwise, dig deeper.
        let mut applyBStartMargin = frame.GetPrevContinuation().is_none() && self.ShouldApplyBStartMargin(aState, aLine.get() as *mut nsLineBox);
        if applyBStartMargin {
            // The HasClearance setting is only valid if ShouldApplyBStartMargin
            // returned false (in which case the block-start margin-root set our
            // clearance flag). Otherwise clear it now. We'll set it later on
            // ourselves if necessary.
            aLine.ClearHasClearance();
        }
        let mut treatWithClearance = aLine.HasClearance();

        let mut mightClearFloats = clearType != UsedClear::None;
        let mut floatAvoidingBlock: Option<nsIFrame> = None;
        if !nsBlockFrame::BlockCanIntersectFloats(frame) {
            mightClearFloats = true;
            floatAvoidingBlock = Some(frame);
        }

        // If our block-start margin was counted as part of some parent's block-start
        // margin collapse, and we are being speculatively reflowed assuming this
        // frame DID NOT need clearance, then we need to check that
        // assumption.
        if !treatWithClearance && !applyBStartMargin && mightClearFloats && !aState.mReflowInput.mDiscoveredClearance.is_null() {
            let curBCoord: nscoord = aState.mBCoord + aState.mPrevBEndMargin.Get();
            let (clearBCoord, result) = aState.ClearFloats(curBCoord, clearType, floatAvoidingBlock);
            if result != ClearFloatsResult::BCoordNoChange {
                let _ = clearBCoord;

                // Only record the first frame that requires clearance
                if unsafe { (*aState.mReflowInput.mDiscoveredClearance).is_none() } {
                    unsafe { *aState.mReflowInput.mDiscoveredClearance = Some(frame) };
                }
                aState.mPrevChild = Some(frame);
                // Exactly what we do now is flexible since we'll definitely be
                // reflowed.
                return;
            }
        }
        if treatWithClearance {
            applyBStartMargin = true;
        }

        let mut clearanceFrame: Option<nsIFrame> = None;
        let startingBCoord: nscoord = aState.mBCoord;
        let incomingMargin: CollapsingMargin = aState.mPrevBEndMargin;
        let mut clearance: nscoord;
        loop {
            clearance = 0;
            let mut bStartMargin: nscoord = 0;
            let mut mayNeedRetry = false;
            let mut clearedFloats = false;
            let mut clearedPushedOrSplitFloat = false;
            if applyBStartMargin {
                // Precompute the blocks block-start margin value so that we can get the
                // correct available space (there might be a float that's
                // already been placed below the aState.mPrevBEndMargin

                // Setup a reflowInput to get the style computed block-start margin
                // value. We'll use a reason of `resize' so that we don't fudge
                // any incremental reflow input.

                // The availSpace here is irrelevant to our needs - all we want
                // out if this setup is the block-start margin value which doesn't depend
                // on the childs available space.
                // XXX building a complete ReflowInput just to get the block-start
                // margin seems like a waste. And we do this for almost every block!
                let wm = frame.GetWritingMode();
                let availSpace: LogicalSize = aState.ContentSize(wm);
                let reflowInput = NewChildReflowInput(aState.mPresContext, stateReflowInput, frame, availSpace, None);

                if treatWithClearance {
                    aState.mBCoord += aState.mPrevBEndMargin.Get();
                    aState.mPrevBEndMargin.Zero();
                }

                // Now compute the collapsed margin-block-start value into
                // aState.mPrevBEndMargin, assuming that all child margins
                // collapse down to clearanceFrame.
                brc.ComputeCollapsedBStartMargin(&reflowInput, &mut aState.mPrevBEndMargin, clearanceFrame, &mut mayNeedRetry, None);

                // XXX optimization; we could check the collapsing children to see if they
                // are sure to require clearance, and so avoid retrying them

                if clearanceFrame.is_some() {
                    // Don't allow retries on the second pass. The clearance decisions for
                    // the blocks whose block-start margins collapse with ours are now
                    // fixed.
                    mayNeedRetry = false;
                }

                if !treatWithClearance && clearanceFrame.is_none() && mightClearFloats {
                    // We don't know if we need clearance and this is the first,
                    // optimistic pass.  So determine whether *this block* needs
                    // clearance. Note that we do not allow the decision for whether
                    // this block has clearance to change on the second pass; that
                    // decision is only allowed to be made under the optimistic
                    // first pass.
                    let curBCoord: nscoord = aState.mBCoord + aState.mPrevBEndMargin.Get();
                    let (clearBCoord, result) = aState.ClearFloats(curBCoord, clearType, floatAvoidingBlock);
                    if result != ClearFloatsResult::BCoordNoChange {
                        let _ = clearBCoord;

                        // Looks like we need clearance and we didn't know about it already.
                        // So recompute collapsed margin
                        treatWithClearance = true;
                        // Remember this decision, needed for incremental reflow
                        aLine.SetHasClearance();

                        // Apply incoming margins
                        aState.mBCoord += aState.mPrevBEndMargin.Get();
                        aState.mPrevBEndMargin.Zero();

                        // Compute the collapsed margin again, ignoring the incoming margin
                        // this time
                        mayNeedRetry = false;
                        brc.ComputeCollapsedBStartMargin(&reflowInput, &mut aState.mPrevBEndMargin, clearanceFrame, &mut mayNeedRetry, None);
                    }
                }

                // Temporarily advance the running block-direction value so that the
                // GetFloatAvailableSpace method will return the right available space.
                // This undone as soon as the horizontal margins are computed.
                bStartMargin = aState.mPrevBEndMargin.Get();

                if treatWithClearance {
                    let currentBCoord: nscoord = aState.mBCoord;
                    // advance mBCoord to the clear position.
                    let (clearBCoord, result) = aState.ClearFloats(aState.mBCoord, clearType, floatAvoidingBlock);
                    aState.mBCoord = clearBCoord;

                    clearedFloats = result != ClearFloatsResult::BCoordNoChange;
                    clearedPushedOrSplitFloat = result == ClearFloatsResult::FloatsPushedOrSplit;

                    // Compute clearance. It's the amount we need to add to the block-start
                    // border-edge of the frame, after applying collapsed margins
                    // from the frame and its children, to get it to line up with
                    // the block-end of the floats. The former is
                    // currentBCoord + bStartMargin, the latter is the current
                    // aState.mBCoord.
                    // Note that negative clearance is possible
                    clearance = aState.mBCoord - (currentBCoord + bStartMargin);

                    // Add clearance to our block-start margin while we compute available
                    // space for the frame
                    bStartMargin += clearance;

                    // Note that aState.mBCoord should stay where it is: at the block-start
                    // border-edge of the frame
                } else {
                    // Advance aState.mBCoord to the block-start border-edge of the frame.
                    aState.mBCoord += bStartMargin;
                }
            }

            aLine.SetLineIsImpactedByFloat(false);

            // Here aState.mBCoord is the block-start border-edge of the block.
            // Compute the available space for the block
            let mut floatAvailableSpace: nsFlowAreaRect = aState.GetFloatAvailableSpace__WritingMode(cbWM);
            let wm = aState.mReflowInput.GetWritingMode();
            let mut availSpace: LogicalRect = aState.ComputeBlockAvailSpace(frame, &floatAvailableSpace, floatAvoidingBlock.is_some());

            // The check for
            //   (!aState.mReflowInput.mFlags.mIsTopOfPage || clearedFloats)
            // is to some degree out of paranoia:  if we reliably eat up block-start
            // margins at the top of the page as we ought to, it wouldn't be
            // needed.
            if (!aState.mReflowInput.mFlags.mIsTopOfPage || clearedFloats) && (availSpace.BSize(wm) < 0 || clearedPushedOrSplitFloat) {
                // We know already that this child block won't fit on this
                // page/column due to the block-start margin or the clearance.  So we
                // need to get out of here now.  (If we don't, most blocks will handle
                // things fine, and report break-before, but zero-height blocks
                // won't, and will thus make their parent overly-large and force
                // *it* to be pushed in its entirety.)
                aState.mBCoord = startingBCoord;
                aState.mPrevBEndMargin = incomingMargin;
                if self.ShouldAvoidBreakInside(aState.mReflowInput) {
                    self.SetBreakBeforeStatusBeforeLine(aState, aLine, aKeepReflowGoing);
                } else {
                    self.PushTruncatedLine(aState, aLine, aKeepReflowGoing, nsBlockFrame_ComputeNewPageNameIfNeeded::Yes);
                }
                return;
            }

            // Now put the block-dir coordinate back to the start of the
            // block-start-margin + clearance.
            aState.mBCoord -= bStartMargin;
            *availSpace.BStart_mut(wm) -= bStartMargin;
            if NS_UNCONSTRAINEDSIZE != availSpace.BSize(wm) {
                *availSpace.BSize_mut(wm) += bStartMargin;
            }

            // Construct the reflow input for the block.
            let mut childReflowInput: Option<ReflowInput>;
            let mut cbSize: Option<LogicalSize> = None;
            let mut availSize: LogicalSize = availSpace.Size(wm);
            let mut columnSetWrapperHasNoBSizeLeft = false;
            // NOT PORTED: columns (the :-moz-column-content branch computing cbSize for the
            // multicol containing block, and the ColumnSetWrapper branch limiting availSize;
            // nsBlockFrame.cpp lines 4535-4585 at GECKO_REV).
            let _ = (&mut cbSize, &mut availSize, &mut columnSetWrapperHasNoBSizeLeft);

            childReflowInput = Some(NewChildReflowInput(aState.mPresContext, stateReflowInput, frame, availSize.ConvertTo(frame.GetWritingMode(), wm), cbSize));

            childReflowInput.as_mut().unwrap().mFlags.mColumnSetWrapperHasNoBSizeLeft = columnSetWrapperHasNoBSizeLeft;

            if aLine.MovedFragments() {
                // We only need to set this the first reflow, since if we reflow
                // again (and replace childReflowInput) we'll be reflowing it
                // again in the same fragment as the previous time.
                childReflowInput.as_mut().unwrap().mFlags.mMovedBlockFragments = true;
            }

            let mut floatManagerState = SavedState::new();
            let mut frameReflowStatus = nsReflowStatus::new();
            loop {
                if floatAvailableSpace.HasFloats() {
                    // Set if floatAvailableSpace.HasFloats() is true for any
                    // iteration of the loop.
                    aLine.SetLineIsImpactedByFloat(true);
                }

                // We might need to store into mDiscoveredClearance later if it's
                // currently null; we want to overwrite any writes that
                // brc.ReflowBlock() below does, so we need to remember now
                // whether it's empty.
                let shouldStoreClearance = !aState.mReflowInput.mDiscoveredClearance.is_null() && unsafe { (*aState.mReflowInput.mDiscoveredClearance).is_none() };

                // Reflow the block into the available space
                if mayNeedRetry || floatAvoidingBlock.is_some() {
                    aState.FloatManager().PushState(&mut floatManagerState);
                }

                if mayNeedRetry {
                    childReflowInput.as_mut().unwrap().mDiscoveredClearance = &mut clearanceFrame as *mut Option<nsIFrame>;
                } else if !applyBStartMargin {
                    childReflowInput.as_mut().unwrap().mDiscoveredClearance = aState.mReflowInput.mDiscoveredClearance;
                }

                frameReflowStatus.Reset();
                let mut prevBEndMargin = aState.mPrevBEndMargin;
                brc.ReflowBlock(availSpace, applyBStartMargin, &mut prevBEndMargin, clearance, Some(&*aLine), childReflowInput.as_mut().unwrap(), &mut frameReflowStatus, aState);
                aState.mPrevBEndMargin = prevBEndMargin;

                if frameReflowStatus.IsInlineBreakBefore() {
                    // No need to retry this loop if there is a break opportunity before the
                    // child block.
                    break;
                }

                // Now the block has a height.  Using that height, get the
                // available space again and call ComputeBlockAvailSpace again.
                // If ComputeBlockAvailSpace gives a different result, we need to
                // reflow again.
                let Some(floatAvoidingBlockFrame) = floatAvoidingBlock else {
                    break;
                };

                let oldFloatAvailableSpaceRect: LogicalRect = floatAvailableSpace.mRect;
                floatAvailableSpace = aState.GetFloatAvailableSpaceForBSize(cbWM, aState.mBCoord + bStartMargin, brc.GetMetrics().BSize(wm), Some(&floatManagerState));
                NS_ASSERTION!(floatAvailableSpace.mRect.BStart(wm) == oldFloatAvailableSpaceRect.BStart(wm), "yikes");
                // Restore the height to the position of the next band.
                *floatAvailableSpace.mRect.BSize_mut(wm) = oldFloatAvailableSpaceRect.BSize(wm);
                // Determine whether the available space shrunk on either side,
                // because (the first time round) we now know the block's height,
                // and it may intersect additional floats, or (on later
                // iterations) because narrowing the width relative to the
                // previous time may cause the block to become taller.  Note that
                // since we're reflowing the block, narrowing the width might also
                // make it shorter, so we must pass aCanGrow as true.
                if !AvailableSpaceShrunk(wm, &oldFloatAvailableSpaceRect, &floatAvailableSpace.mRect, true) {
                    // The size and position we chose before are fine (i.e., they
                    // don't cause intersecting with floats that requires a change
                    // in size or position), so we're done.
                    break;
                }

                let mut advanced = false;
                if !aState.FloatAvoidingBlockFitsInAvailSpace(floatAvoidingBlockFrame, &floatAvailableSpace) {
                    // Advance to the next band.
                    let mut newBCoord: nscoord = aState.mBCoord;
                    if aState.AdvanceToNextBand(&floatAvailableSpace.mRect, &mut newBCoord) {
                        advanced = true;
                    }
                    // ClearFloats might be able to advance us further once we're there.
                    aState.mBCoord = aState.ClearFloats(newBCoord, UsedClear::None, floatAvoidingBlock).0;

                    // Start over with a new available space rect at the new height.
                    floatAvailableSpace = aState.GetFloatAvailableSpaceWithState(cbWM, aState.mBCoord, ShapeType::ShapeOutside, Some(&floatManagerState));
                }

                let oldAvailSpace: LogicalRect = availSpace;
                availSpace = aState.ComputeBlockAvailSpace(frame, &floatAvailableSpace, floatAvoidingBlock.is_some());

                if !advanced && availSpace.IsEqualEdges(oldAvailSpace) {
                    break;
                }

                // We need another reflow.
                aState.FloatManager().PopState(&mut floatManagerState);

                if !treatWithClearance && !applyBStartMargin && !aState.mReflowInput.mDiscoveredClearance.is_null() {
                    // We set shouldStoreClearance above to record only the first
                    // frame that requires clearance.
                    if shouldStoreClearance {
                        unsafe { *aState.mReflowInput.mDiscoveredClearance = Some(frame) };
                    }
                    aState.mPrevChild = Some(frame);
                    // Exactly what we do now is flexible since we'll definitely be
                    // reflowed.
                    return;
                }

                if advanced {
                    // We're pushing down the border-box, so we don't apply margin anymore.
                    // This should never cause us to move up since the call to
                    // GetFloatAvailableSpaceForBSize above included the margin.
                    applyBStartMargin = false;
                    bStartMargin = 0;
                    treatWithClearance = true; // avoid hitting test above
                    clearance = 0;
                }

                childReflowInput = None;
                childReflowInput = Some(NewChildReflowInput(aState.mPresContext, stateReflowInput, frame, availSpace.Size(wm).ConvertTo(frame.GetWritingMode(), wm), None));
            }

            if mayNeedRetry && clearanceFrame.is_some() {
                // Found a clearance frame, so we need to reflow |frame| a second time.
                // Restore the states and start over again.
                aState.FloatManager().PopState(&mut floatManagerState);
                aState.mBCoord = startingBCoord;
                aState.mPrevBEndMargin = incomingMargin;
                continue;
            }

            aState.mPrevChild = Some(frame);

            let childReflowInput = childReflowInput.as_ref().unwrap();
            if childReflowInput.WillReflowAgainForClearance() {
                return;
            }

            // #if defined(REFLOW_STATUS_COVERAGE) RecordReflowStatus(true, frameReflowStatus);  NOT PORTED.

            if frameReflowStatus.IsInlineBreakBefore() {
                // None of the child block fits.
                if self.ShouldAvoidBreakInside(aState.mReflowInput) {
                    self.SetBreakBeforeStatusBeforeLine(aState, aLine, aKeepReflowGoing);
                } else {
                    self.PushTruncatedLine(aState, aLine, aKeepReflowGoing, nsBlockFrame_ComputeNewPageNameIfNeeded::Yes);
                }
            } else {
                // Note: line-break-after a block is a nop

                // Try to place the child block.
                // Don't force the block to fit if we have positive clearance, because
                // pushing it to the next page would give it more room.
                // Don't force the block to fit if it's impacted by a float. If it is,
                // then pushing it to the next page would give it more room. Note that
                // isImpacted doesn't include impact from the block's own floats.
                let forceFit = aState.IsAdjacentWithBStart() && clearance <= 0 && !floatAvailableSpace.HasFloats();
                let mut collapsedBEndMargin = CollapsingMargin::default();
                let mut overflowAreas = OverflowAreas::default();
                *aKeepReflowGoing = brc.PlaceBlock(childReflowInput, forceFit, &mut *aLine, &mut collapsedBEndMargin, &mut overflowAreas, &frameReflowStatus);
                if !frameReflowStatus.IsFullyComplete() && self.ShouldAvoidBreakInside(aState.mReflowInput) {
                    *aKeepReflowGoing = false;
                    aLine.MarkDirty();
                }

                if aLine.SetCarriedOutBEndMargin(collapsedBEndMargin) {
                    let mut nextLine = aLine;
                    nextLine.operator_inc();
                    if nextLine != self.LinesEnd() {
                        nextLine.MarkPreviousMarginDirty();
                    }
                }

                // NOT PORTED: scrolling (the :-moz-scrolled-content in-flow bounds of the line,
                // GetLineFrameInFlowBounds and SetInFlowChildBounds).

                aLine.SetOverflowAreas(&overflowAreas);
                if *aKeepReflowGoing {
                    // Some of the child block fit

                    // Advance to new Y position
                    let newBCoord: nscoord = aLine.BEnd();
                    aState.mBCoord = newBCoord;

                    // Continue the block frame now if it didn't completely fit in
                    // the available space.
                    if !frameReflowStatus.IsFullyComplete() {
                        // NOT PORTED: fragmentation (continuations of the child block, overflow
                        // containers, pushing the rest of the lines; nsBlockFrame.cpp lines 4804-4900
                        // at GECKO_REV). Without fragmentation a block child is always fully complete.
                        let madeContinuation = self.CreateContinuationFor(aState, std::ptr::null_mut(), frame);
                        let _ = madeContinuation;
                        unimplemented!("NOT PORTED: fragmentation (block child reported an incomplete status)");
                    } else {
                        // frame is fully complete
                        // #ifdef NOISY_BLOCK_DIR_MARGINS ... NOT PORTED: debug printing.
                        aState.mPrevBEndMargin = collapsedBEndMargin;
                    }
                    // #ifdef NOISY_BLOCK_DIR_MARGINS ... NOT PORTED: debug printing.
                } else {
                    if !frameReflowStatus.IsFullyComplete() {
                        // The frame reported an incomplete status, but then it also didn't
                        // fit.  This means we need to reflow it again so that it can
                        // (again) report the incomplete status.
                        frame.AddStateBits(NS_FRAME_HAS_DIRTY_CHILDREN);
                    }

                    if (aLine.get() == self.mLines().front() && self.GetPrevInFlow().is_none()) || self.ShouldAvoidBreakInside(aState.mReflowInput) {
                        // If it's our very first line *or* we're not at the top of the page
                        // and we have page-break-inside:avoid, then we need to be pushed to
                        // our parent's next-in-flow.
                        self.SetBreakBeforeStatusBeforeLine(aState, aLine, aKeepReflowGoing);
                    } else {
                        // Push the line that didn't fit and any lines that follow it
                        // to our next-in-flow.
                        self.PushTruncatedLine(aState, aLine, aKeepReflowGoing, nsBlockFrame_ComputeNewPageNameIfNeeded::Yes);
                    }
                }
            }
            break; // out of the reflow retry loop
        }

        // #ifdef DEBUG VerifyLines(true);  NOT PORTED: debug-only verification.
    }

    // Returns true if an overflow-wrap break was used.
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ReflowInlineFrames
    pub fn ReflowInlineFrames(&self, aState: &mut BlockReflowState, mut aLine: nsLineList_iterator, aKeepReflowGoing: &mut bool) -> bool {
        *aKeepReflowGoing = true;
        let mut usedOverflowWrap = false;

        aLine.SetLineIsImpactedByFloat(false);

        // Setup initial coordinate system for reflowing the inline frames
        // into. Apply a previous block frame's block-end margin first.
        if self.ShouldApplyBStartMargin(aState, aLine.get() as *mut nsLineBox) {
            aState.mBCoord += aState.mPrevBEndMargin.Get();
        }
        let mut floatAvailableSpace: nsFlowAreaRect = aState.GetFloatAvailableSpace__WritingMode(self.GetWritingMode());

        let mut lineReflowStatus = LineReflowStatus::OK;
        loop {
            let mut availableSpaceBSize: nscoord = 0;
            aState.mLineBSize = None;
            loop {
                let mut allowPullUp = true;
                let mut forceBreakInFrame: Option<nsIFrame> = None;
                let mut forceBreakOffset: i32 = -1;
                let mut forceBreakPriority = gfxBreakPriority::eNoBreak;
                loop {
                    let mut floatManagerState = SavedState::new();
                    aState.FloatManager().PushState(&mut floatManagerState);

                    // Once upon a time we allocated the first 30 nsLineLayout objects
                    // on the stack, and then we switched to the heap.  At that time
                    // these objects were large (1100 bytes on a 32 bit system).
                    // Then the nsLineLayout object was shrunk to 156 bytes by
                    // removing some internal buffers.  Given that it is so much
                    // smaller, the complexity of 2 different ways of allocating
                    // no longer makes sense.  Now we always allocate on the stack.
                    {
                        // (ADAPTER: nsLineLayout keeps raw pointers to the block reflow state, as
                        // in C++, where it holds a BlockReflowState*.)
                        let statePtr: *mut BlockReflowState = aState as *mut BlockReflowState;
                        let stateReflowInput: &ReflowInput = aState.mReflowInput;
                        let mut lineLayout = nsLineLayout::new(&aState.mPresContext, aState.FloatManager() as *mut nsFloatManager, stateReflowInput, Some(&aLine), std::ptr::null_mut());
                        lineLayout.Init(statePtr, aState.mMinLineHeight, aState.mLineNumber);
                        if let Some(f) = forceBreakInFrame {
                            lineLayout.ForceBreakAtPosition(f, forceBreakOffset);
                        }
                        self.DoReflowInlineFrames(aState, &mut lineLayout, aLine, &mut floatAvailableSpace, &mut availableSpaceBSize, &floatManagerState, aKeepReflowGoing, &mut lineReflowStatus, allowPullUp);
                        usedOverflowWrap = lineLayout.EndLineReflow();

                        if LineReflowStatus::RedoNoPull == lineReflowStatus || LineReflowStatus::RedoMoreFloats == lineReflowStatus || LineReflowStatus::RedoNextBand == lineReflowStatus {
                            if lineLayout.NeedsBackup() {
                                NS_ASSERTION!(forceBreakInFrame.is_none(), "Backing up twice; this should never be necessary");
                                // If there is no saved break position, then this will set
                                // set forceBreakInFrame to null and we won't back up, which is
                                // correct.
                                forceBreakInFrame = lineLayout.GetLastOptionalBreakPosition(&mut forceBreakOffset, &mut forceBreakPriority);
                            } else {
                                forceBreakInFrame = None;
                            }
                            // restore the float manager state
                            aState.FloatManager().PopState(&mut floatManagerState);
                            // Clear out float lists
                            aState.mCurrentLineFloats.clear();
                            aState.mBelowCurrentLineFloats.clear();
                            aState.mNoWrapFloats.clear();
                        }
                    }

                    // Don't allow pullup on a subsequent LineReflowStatus::RedoNoPull pass
                    allowPullUp = false;
                    if !(LineReflowStatus::RedoNoPull == lineReflowStatus) {
                        break;
                    }
                }
                if !(LineReflowStatus::RedoMoreFloats == lineReflowStatus) {
                    break;
                }
            }
            if !(LineReflowStatus::RedoNextBand == lineReflowStatus) {
                break;
            }
        }

        usedOverflowWrap
    }
}

// Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::ComputeNewPageNameIfNeeded
/**
 * Indicates if we need to compute a page name for the next page when pushing
 * a truncated line.
 *
 * Using a value of No saves work when a new page name has already been set
 * with nsCSSFrameConstructor::SetNextPageContentFramePageName.
 */
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsBlockFrame_ComputeNewPageNameIfNeeded {
    Yes,
    No,
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::SetBreakBeforeStatusBeforeLine
    pub fn SetBreakBeforeStatusBeforeLine(&self, aState: &mut BlockReflowState, mut aLine: nsLineList_iterator, aKeepReflowGoing: &mut bool) {
        aState.mReflowStatus.SetInlineLineBreakBeforeAndReset();
        // Reflow the line again when we reflow at our new position.
        aLine.MarkDirty();
        *aKeepReflowGoing = false;
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::PushTruncatedLine
    pub fn PushTruncatedLine(&self, aState: &mut BlockReflowState, aLine: nsLineList_iterator, aKeepReflowGoing: &mut bool, aComputeNewPageName: nsBlockFrame_ComputeNewPageNameIfNeeded) {
        self.PushLines(aState, aLine.prev());
        *aKeepReflowGoing = false;

        if aComputeNewPageName == nsBlockFrame_ComputeNewPageNameIfNeeded::Yes {
            // mCanHaveClassABreakpoints can only be true during paginated reflow, and
            // we expect this function to only be called when the available bsize is
            // constrained.
            // NOT PORTED: pagination (named pages; MaybeSetNextPageContentFramePageName).
        }
        aState.mReflowStatus.SetIncomplete();
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DoReflowInlineFrames
    pub fn DoReflowInlineFrames(
        &self,
        aState: &mut BlockReflowState,
        aLineLayout: &mut nsLineLayout,
        mut aLine: nsLineList_iterator,
        aFloatAvailableSpace: &mut nsFlowAreaRect,
        aAvailableSpaceBSize: &mut nscoord,
        aFloatStateBeforeLine: &SavedState,
        aKeepReflowGoing: &mut bool,
        aLineReflowStatus: &mut LineReflowStatus,
        aAllowPullUp: bool,
    ) {
        // Forget all of the floats on the line
        aLine.ClearFloats();
        aState.mFloatOverflowAreas.Clear();

        // We need to set this flag on the line if any of our reflow passes
        // are impacted by floats.
        if aFloatAvailableSpace.HasFloats() {
            aLine.SetLineIsImpactedByFloat(true);
        }
        // #ifdef REALLY_NOISY_REFLOW ... NOT PORTED: debug printing.

        let outerWM = aState.mReflowInput.GetWritingMode();
        let lineWM = self.WritingModeForLine(outerWM, aLine.mFirstChild.unwrap());
        let lineRect: LogicalRect = aFloatAvailableSpace.mRect.ConvertTo(lineWM, outerWM, aState.ContainerSize());

        let iStart: nscoord = lineRect.IStart(lineWM);
        let availISize: nscoord = lineRect.ISize(lineWM);
        let availBSize: nscoord;
        if aState.mReflowInput.AvailableBSize() == NS_UNCONSTRAINEDSIZE {
            availBSize = NS_UNCONSTRAINEDSIZE;
        } else {
            /* XXX get the height right! */
            availBSize = lineRect.BSize(lineWM);
        }

        // Make sure to enable resize optimization before we call BeginLineReflow
        // because it might get disabled there
        aLine.EnableResizeReflowOptimization();

        aLineLayout.BeginLineReflow(iStart, aState.mBCoord, availISize, availBSize, aFloatAvailableSpace.HasFloats(), false /*XXX isTopOfPage*/, lineWM, aState.mContainerSize, aState.mInsetForBalance);

        aState.mFlags.mIsLineLayoutEmpty = false;

        // XXX Unfortunately we need to know this before reflowing the first
        // inline frame in the line. FIX ME.
        if 0 == aLineLayout.GetLineNumber() && self.HasAllStateBits(NS_BLOCK_HAS_FIRST_LETTER_CHILD | NS_BLOCK_HAS_FIRST_LETTER_STYLE) {
            aLineLayout.SetFirstLetterStyleOK(true);
        }
        NS_ASSERTION!(!(self.HasAnyStateBits(NS_BLOCK_HAS_FIRST_LETTER_CHILD) && self.GetPrevContinuation().is_some()), "first letter child bit should only be on first continuation");

        // Reflow the frames that are already on the line first
        let mut lineReflowStatus = LineReflowStatus::OK;
        let mut i: i32;
        let mut frame: Option<nsIFrame> = aLine.mFirstChild;

        if aFloatAvailableSpace.HasFloats() {
            // There is a soft break opportunity at the start of the line, because
            // we can always move this line down below float(s).
            if aLineLayout.NotifyOptionalBreakPosition(frame, 0, true, gfxBreakPriority::eNormalBreak) {
                lineReflowStatus = LineReflowStatus::RedoNextBand;
            }
        }

        // need to repeatedly call GetChildCount here, because the child
        // count can change during the loop!
        i = 0;
        while LineReflowStatus::OK == lineReflowStatus && i < aLine.GetChildCount() {
            self.SetLineCursorForDisplay(aLine.get() as *mut nsLineBox);
            self.ReflowInlineFrame(aState, aLineLayout, aLine, frame.unwrap(), &mut lineReflowStatus);
            if LineReflowStatus::OK != lineReflowStatus {
                // It is possible that one or more of next lines are empty
                // (because of DeleteNextInFlowChild). If so, delete them now
                // in case we are finished.
                aLine.operator_inc();
                while (aLine != self.LinesEnd()) && (0 == aLine.GetChildCount()) {
                    // XXX Is this still necessary now that DeleteNextInFlowChild
                    // uses DoRemoveFrame?
                    let toremove: *mut nsLineBox = aLine.get_mut();
                    aLine = self.mLines().erase(aLine);
                    NS_ASSERTION!(unsafe { &*toremove }.mFirstChild.is_none(), "bad empty line");
                    self.FreeLineBox(toremove);
                    self.ClearLineCursors();
                }
                aLine.operator_dec();

                NS_ASSERTION!(lineReflowStatus != LineReflowStatus::Truncated, "ReflowInlineFrame should never determine that a line needs to go to the next page/column");
            }
            // (the increments of the for statement)
            i += 1;
            frame = frame.unwrap().GetNextSibling();
        }

        // Don't pull up new frames into lines with continuation placeholders
        if aAllowPullUp {
            // Pull frames and reflow them until we can't
            while LineReflowStatus::OK == lineReflowStatus {
                frame = self.PullFrame(aState, aLine);
                if frame.is_none() {
                    break;
                }

                while LineReflowStatus::OK == lineReflowStatus {
                    let oldCount = aLine.GetChildCount();
                    self.SetLineCursorForDisplay(aLine.get() as *mut nsLineBox);
                    self.ReflowInlineFrame(aState, aLineLayout, aLine, frame.unwrap(), &mut lineReflowStatus);
                    if aLine.GetChildCount() != oldCount {
                        // We just created a continuation for aFrame AND its going
                        // to end up on this line (e.g. :first-letter
                        // situation). Therefore we have to loop here before trying
                        // to pull another frame.
                        frame = frame.unwrap().GetNextSibling();
                    } else {
                        break;
                    }
                }
            }
        }
        self.ClearLineCursors();

        aState.mFlags.mIsLineLayoutEmpty = aLineLayout.LineIsEmpty();

        // We only need to backup if the line isn't going to be reflowed again anyway
        let mut needsBackup = aLineLayout.NeedsBackup() && (lineReflowStatus == LineReflowStatus::Stop || lineReflowStatus == LineReflowStatus::OK);
        if needsBackup && aLineLayout.HaveForcedBreakPosition() {
            NS_WARNING!("We shouldn't be backing up more than once! Someone must have set a break opportunity beyond the available width, even though there were better break opportunities before it");
            needsBackup = false;
        }
        if needsBackup {
            // We need to try backing up to before a text run
            // XXX It's possible, in fact not unusual, for the break opportunity to
            // already be the end of the line. We should detect that and optimize to not
            // re-do the line.
            if aLineLayout.HasOptionalBreakPosition() {
                // We can back up!
                lineReflowStatus = LineReflowStatus::RedoNoPull;
            }
        } else {
            // In case we reflow this line again, remember that we don't
            // need to force any breaking
            aLineLayout.ClearOptionalBreakPosition();
        }

        if LineReflowStatus::RedoNextBand == lineReflowStatus {
            // This happens only when we have a line that is impacted by
            // floats and the first element in the line doesn't fit with
            // the floats.
            //
            // If there's block space available, we either try to reflow the line
            // past the current band (if it's non-zero and the band definitely won't
            // widen around a shape-outside), otherwise we try one pixel down. If
            // there's no block space available, we push the line to the next
            // page/column.
            NS_ASSERTION!(NS_UNCONSTRAINEDSIZE != aFloatAvailableSpace.mRect.BSize(outerWM), "unconstrained block size on totally empty line");

            // See the analogous code for blocks in BlockReflowState::ClearFloats.
            let bandBSize: nscoord = aFloatAvailableSpace.mRect.BSize(outerWM);
            if bandBSize > 0 || NS_UNCONSTRAINEDSIZE == aState.mReflowInput.AvailableBSize() {
                NS_ASSERTION!(bandBSize == 0 || aFloatAvailableSpace.HasFloats(), "redo line on totally empty line with non-empty band...");
                // We should never hit this case if we've placed floats on the
                // line; if we have, then the GetFloatAvailableSpace call is wrong
                // and needs to happen after the caller pops the float manager
                // state.
                aState.FloatManager().AssertStateMatches(aFloatStateBeforeLine);

                if !aFloatAvailableSpace.MayWiden() && bandBSize > 0 {
                    // Move it down far enough to clear the current band.
                    aState.mBCoord += bandBSize;
                } else {
                    // Move it down by one dev pixel.
                    aState.mBCoord += aState.mPresContext.DevPixelsToAppUnits(1);
                }

                *aFloatAvailableSpace = aState.GetFloatAvailableSpace__WritingMode(self.GetWritingMode());
            } else {
                // There's nowhere to retry placing the line, so we want to push
                // it to the next page/column where its contents can fit not
                // next to a float.
                lineReflowStatus = LineReflowStatus::Truncated;
                self.PushTruncatedLine(aState, aLine, aKeepReflowGoing, nsBlockFrame_ComputeNewPageNameIfNeeded::Yes);
            }

            // XXX: a small optimization can be done here when paginating:
            // if the new Y coordinate is past the end of the block then
            // push the line and return now instead of later on after we are
            // past the float.
        } else if LineReflowStatus::Truncated != lineReflowStatus && LineReflowStatus::RedoNoPull != lineReflowStatus {
            // If we are propagating out a break-before status then there is
            // no point in placing the line.
            if !aState.mReflowStatus.IsInlineBreakBefore() {
                if !self.PlaceLine(aState, aLineLayout, aLine, aFloatStateBeforeLine, aFloatAvailableSpace, aAvailableSpaceBSize, aKeepReflowGoing) {
                    lineReflowStatus = LineReflowStatus::RedoMoreFloats;
                    // PlaceLine already called GetFloatAvailableSpaceForBSize or its
                    // variant for us.
                }
            }
        }
        // #ifdef DEBUG if (gNoisyReflow) {...}  NOT PORTED: debug printing.

        if aLineLayout.GetDirtyNextLine() {
            // aLine may have been pushed to the overflow lines.
            let overflowLines = self.GetOverflowLines();
            // We can't just compare iterators front() to aLine here, since they may be
            // in different lists.
            let pushedToOverflowLines = !overflowLines.is_null() && unsafe { &*overflowLines }.mLines.front() == aLine.get();
            if pushedToOverflowLines {
                // aLine is stale, it's associated with the main line list but it should
                // be associated with the overflow line list now
                aLine = unsafe { &mut *overflowLines }.mLines.begin_mut();
            }
            let mut iter = nsBlockInFlowLineIterator::new__nsBlockFrame_LineIterator_bool(*self, aLine, pushedToOverflowLines);
            if iter.Next() && iter.GetLine().IsInline() {
                let mut l = iter.GetLine();
                l.MarkDirty();
                if iter.GetContainer() != *self {
                    aState.mReflowStatus.SetNextInFlowNeedsReflow();
                }
            }
        }

        *aLineReflowStatus = lineReflowStatus;
    }

    /**
     * Reflow an inline frame. The reflow status is mapped from the frames
     * reflow status to the lines reflow status (not to our reflow status).
     * The line reflow status is simple: true means keep placing frames
     * on the line; false means don't (the line is done). If the line
     * has some sort of breaking affect then aLine's break-type will be set
     * to something other than UsedClear::None.
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ReflowInlineFrame
    pub fn ReflowInlineFrame(&self, aState: &mut BlockReflowState, aLineLayout: &mut nsLineLayout, mut aLine: nsLineList_iterator, aFrame: nsIFrame, aLineReflowStatus: &mut LineReflowStatus) {
        *aLineReflowStatus = LineReflowStatus::OK;

        // #ifdef NOISY_FIRST_LETTER ... NOT PORTED: debug printing.

        if aFrame.IsPlaceholderFrame() {
            // auto ph = static_cast<nsPlaceholderFrame*>(aFrame); ph->ForgetLineIsEmptySoFar();
            // NOT PORTED: placeholder frames cache "line is empty so far" for their hypothetical
            // position (only needed by absolute positioning).
        }

        // Reflow the inline frame
        let mut frameReflowStatus = nsReflowStatus::new();
        let mut pushedFrame = false;
        aLineLayout.ReflowFrame(aFrame, &mut frameReflowStatus, None, &mut pushedFrame);

        if frameReflowStatus.NextInFlowNeedsReflow() {
            aLineLayout.SetDirtyNextLine();
        }

        // #ifdef REALLY_NOISY_REFLOW ... #if defined(REFLOW_STATUS_COVERAGE) ...  NOT PORTED: debug-only.

        // Send post-reflow notification
        aState.mPrevChild = Some(aFrame);

        /* XXX
        This is where we need to add logic to handle some odd behavior.
        For one thing, we should usually place at least one thing next
        to a left float, even when that float takes up all the width on a line.
        see bug 22496
        */

        // Process the child frames reflow status. There are 5 cases:
        // complete, not-complete, break-before, break-after-complete,
        // break-after-not-complete. There are two situations: we are a
        // block or we are an inline. This makes a total of 10 cases
        // (fortunately, there is some overlap).
        aLine.ClearForcedLineBreak();
        if frameReflowStatus.IsInlineBreak() || aState.mTrailingClearFromPIF != UsedClear::None {
            // Always abort the line reflow (because a line break is the
            // minimal amount of break we do).
            *aLineReflowStatus = LineReflowStatus::Stop;

            // XXX what should aLine's break-type be set to in all these cases?
            if frameReflowStatus.IsInlineBreakBefore() {
                // Break-before cases.
                if Some(aFrame) == aLine.mFirstChild {
                    // If we break before the first frame on the line then we must
                    // be trying to place content where there's no room (e.g. on a
                    // line with wide floats). Inform the caller to reflow the
                    // line after skipping past a float.
                    *aLineReflowStatus = LineReflowStatus::RedoNextBand;
                } else {
                    // It's not the first child on this line so go ahead and split
                    // the line. We will see the frame again on the next-line.
                    self.SplitLine(aState, aLineLayout, aLine, Some(aFrame), aLineReflowStatus);

                    // If we're splitting the line because the frame didn't fit and it
                    // was pushed, then mark the line as having word wrapped. We need to
                    // know that if we're shrink wrapping our width
                    if pushedFrame {
                        aLine.SetLineWrapped(true);
                    }
                }
            } else {
                MOZ_ASSERT!(frameReflowStatus.IsInlineBreakAfter() || aState.mTrailingClearFromPIF != UsedClear::None, "We should've handled inline break-before in the if-branch!");

                // If a float split and its prev-in-flow was followed by a <BR>, then
                // combine the <BR>'s float clear type with the inline's float clear type
                // (the inline will be the very next frame after the split float).
                let mut clearType: UsedClear = frameReflowStatus.FloatClearType();
                if aState.mTrailingClearFromPIF != UsedClear::None {
                    clearType = nsLayoutUtils::CombineClearType(clearType, aState.mTrailingClearFromPIF);
                    aState.mTrailingClearFromPIF = UsedClear::None;
                }
                // Break-after cases
                if clearType != UsedClear::None || aLineLayout.GetLineEndsInBR() {
                    aLine.SetForcedLineBreakAfter(clearType);
                }
                if frameReflowStatus.IsComplete() {
                    // Split line, but after the frame just reflowed
                    self.SplitLine(aState, aLineLayout, aLine, aFrame.GetNextSibling(), aLineReflowStatus);

                    if frameReflowStatus.IsInlineBreakAfter() && !aLineLayout.GetLineEndsInBR() {
                        aLineLayout.SetDirtyNextLine();
                    }
                }
            }
        }

        if !frameReflowStatus.IsFullyComplete() {
            // Create a continuation for the incomplete frame. Note that the
            // frame may already have a continuation.
            self.CreateContinuationFor(aState, aLine.get() as *mut nsLineBox, aFrame);

            // Remember that the line has wrapped
            if !aLineLayout.GetLineEndsInBR() {
                aLine.SetLineWrapped(true);
            }

            // If we just ended a first-letter frame or reflowed a placeholder then
            // don't split the line and don't stop the line reflow...
            // But if we are going to stop anyways we'd better split the line.
            if (!frameReflowStatus.FirstLetterComplete() && !aFrame.IsPlaceholderFrame()) || *aLineReflowStatus == LineReflowStatus::Stop {
                // Split line after the current frame
                *aLineReflowStatus = LineReflowStatus::Stop;
                self.SplitLine(aState, aLineLayout, aLine, aFrame.GetNextSibling(), aLineReflowStatus);
            }
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::CreateContinuationFor
    pub fn CreateContinuationFor(&self, aState: &mut BlockReflowState, aLine: *mut nsLineBox, aFrame: nsIFrame) -> bool {
        let mut newFrame: Option<nsIFrame> = None;

        if aFrame.GetNextInFlow().is_none() {
            // NOT PORTED: fragmentation (the frame constructor creates the continuing frame).
            newFrame = Some(self.CreateContinuingFrame(aFrame));

            self.mFrames().InsertFrame(None, Some(aFrame), newFrame.unwrap());

            if !aLine.is_null() {
                unsafe { &mut *aLine }.NoteFrameAdded(newFrame.unwrap());
            }
        }
        // #ifdef DEBUG VerifyLines(false);  NOT PORTED: debug-only verification.
        newFrame.is_some()
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::SplitFloat
    // NOT PORTED: fragmentation (float continuations, nsBlockFrame.cpp lines 5468-5508 at GECKO_REV).
    pub fn SplitFloat(&self, aState: &mut BlockReflowState, aFloat: nsIFrame, aFloatStatus: &nsReflowStatus) {
        MOZ_ASSERT!(!aFloatStatus.IsFullyComplete(), "why split the frame if it's fully complete?");
        unimplemented!("NOT PORTED: fragmentation (nsBlockFrame::SplitFloat)")
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp CheckPlaceholderInLine
fn CheckPlaceholderInLine(aBlock: nsIFrame, aLine: &nsLineBox, aFloat: Option<nsIFrame>) -> bool {
    let Some(aFloat) = aFloat else {
        return true;
    };
    NS_ASSERTION!(aFloat.GetPrevContinuation().is_none(), "float in a line should never be a continuation");
    NS_ASSERTION!(!aFloat.HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW), "float in a line should never be a pushed float");
    let ph = aFloat.FirstInFlow().GetPlaceholderFrame();
    let mut f = ph;
    while let Some(fr) = f {
        if fr.GetParent() == Some(aBlock) {
            return aLine.Contains(fr);
        }
        f = fr.GetParent();
    }
    NS_ASSERTION!(false, "aBlock is not an ancestor of aFrame!");
    true
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::SplitLine
    pub fn SplitLine(&self, aState: &mut BlockReflowState, aLineLayout: &mut nsLineLayout, aLine: nsLineList_iterator, aFrame: Option<nsIFrame>, aLineReflowStatus: &mut LineReflowStatus) {
        MOZ_ASSERT!(aLine.IsInline(), "illegal SplitLine on block line");

        let pushCount: i32 = aLine.GetChildCount() - aLineLayout.GetCurrentSpanCount();
        MOZ_ASSERT!(pushCount >= 0, "bad push count");

        // #ifdef DEBUG if (gNoisyReflow) {...}  NOT PORTED: debug printing.

        if 0 != pushCount {
            MOZ_ASSERT!(aLine.GetChildCount() > pushCount, "bad push");
            MOZ_ASSERT!(aFrame.is_some(), "whoops");
            #[cfg(debug_assertions)]
            {
                let mut f = aFrame;
                let mut count = pushCount;
                while f.is_some() && count > 0 {
                    f = f.unwrap().GetNextSibling();
                    count -= 1;
                }
                NS_ASSERTION!(count == 0, "Not enough frames to push");
            }

            // Put frames being split out into their own line
            let newLine: *mut nsLineBox = self.NewLineBox__nsLineBox_nsIFrame_int32_t(aLine.get() as *mut nsLineBox, aFrame.unwrap(), pushCount);
            self.mLines().after_insert(aLine, newLine);
            // #ifdef DEBUG if (gReallyNoisyReflow) { newLine->List(...) }  NOT PORTED: debug printing.

            // Let line layout know that some frames are no longer part of its
            // state.
            aLineLayout.SplitLineTo(aLine.GetChildCount());

            // If floats have been placed whose placeholders have been pushed to the new
            // line, we need to reflow the old line again. We don't want to look at the
            // frames in the new line, because as a large paragraph is laid out the
            // we'd get O(N^2) performance. So instead we just check that the last
            // float and the last below-current-line float are still in aLine.
            if !CheckPlaceholderInLine(self.0, &aLine, if aLine.HasFloats() { aLine.Floats().last().copied() } else { None })
                || !CheckPlaceholderInLine(self.0, &aLine, aState.mBelowCurrentLineFloats.last().copied())
            {
                *aLineReflowStatus = LineReflowStatus::RedoNoPull;
            }

            // #ifdef DEBUG VerifyLines(true);  NOT PORTED: debug-only verification.
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::IsLastLine
    pub fn IsLastLine(&self, aState: &mut BlockReflowState, mut aLine: nsLineList_iterator) -> bool {
        loop {
            aLine.operator_inc();
            if !(aLine != self.LinesEnd()) {
                break;
            }
            // There is another line
            if 0 != aLine.GetChildCount() {
                // If the next line is a block line then this line is the last in a
                // group of inline lines.
                return aLine.IsBlock();
            }
            // The next line is empty, try the next one
        }

        // Try our next-in-flows lines to answer the question
        let mut nextInFlow = self.GetNextInFlow().map(nsBlockFrame::from_frame);
        while let Some(nif) = nextInFlow {
            for line in nif.Lines() {
                if 0 != line.GetChildCount() {
                    return line.IsBlock();
                }
            }
            nextInFlow = nif.GetNextInFlow().map(nsBlockFrame::from_frame);
        }

        // This is the last line - so don't allow justification
        true
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::PlaceLine
    // Return false if it needs another reflow because of reduced space
    // between floats that are next to it (but not next to its top), and
    // return true otherwise.
    pub fn PlaceLine(
        &self,
        aState: &mut BlockReflowState,
        aLineLayout: &mut nsLineLayout,
        mut aLine: nsLineList_iterator,
        aFloatStateBeforeLine: &SavedState,
        aFlowArea: &mut nsFlowAreaRect,       // in-out
        aAvailableSpaceBSize: &mut nscoord, // in-out
        aKeepReflowGoing: &mut bool,
    ) -> bool {
        // Try to position the floats in a nowrap context.
        aLineLayout.FlushNoWrapFloats();

        // Trim extra white-space from the line before placing the frames
        aLineLayout.TrimTrailingWhiteSpace();

        // Vertically align the frames on this line.
        //
        // According to the CSS2 spec, section 12.6.1, the "marker" box
        // participates in the height calculation of the list-item box's
        // first line box.
        //
        // There are exactly two places a ::marker can be placed: near the
        // first or second line. It's only placed on the second line in a
        // rare case: when the first line is empty.
        let wm = aState.mReflowInput.GetWritingMode();
        let mut addedMarker = false;
        let outsideMarker = self.GetOutsideMarker();
        if let Some(outsideMarker) = outsideMarker {
            if (aLine.get() == self.mLines().front() && (!aLineLayout.IsZeroBSize() || (aLine.get() == self.mLines().back())))
                || (self.mLines().front() != self.mLines().back() && 0 == unsafe { &*self.mLines().front() }.BSize() && aLine == self.mLines().begin_mut().next_())
            {
                let mut metrics = ReflowOutput::new__ReflowInput(aState.mReflowInput);
                self.ReflowOutsideMarker(outsideMarker, aState, &mut metrics, aState.mBCoord);
                NS_ASSERTION!(!self.MarkerIsEmpty(outsideMarker) || metrics.BSize(wm) == 0, "empty ::marker frame took up space");
                aLineLayout.AddMarkerFrame(outsideMarker, &metrics);
                addedMarker = true;
            }
        }
        aLineLayout.VerticalAlignLine();

        // We want to consider the floats in the current line when determining
        // whether the float available space is shrunk. If mLineBSize doesn't
        // exist, we are in the first pass trying to place the line. Calling
        // GetFloatAvailableSpace() like we did in BlockReflowState::AddFloat()
        // for UpdateBand().

        // floatAvailableSpaceWithOldLineBSize is the float available space with
        // the old BSize, but including the floats that were added in this line.
        let floatAvailableSpaceWithOldLineBSize: LogicalRect = if aState.mLineBSize.is_none() {
            aState.GetFloatAvailableSpace__WritingMode_nscoord(wm, aLine.BStart()).mRect
        } else {
            aState.GetFloatAvailableSpaceForBSize(wm, aLine.BStart(), aState.mLineBSize.unwrap(), None).mRect
        };

        // As we redo for floats, we can't reduce the amount of BSize we're
        // checking.
        *aAvailableSpaceBSize = std::cmp::max(*aAvailableSpaceBSize, aLine.BSize());
        let floatAvailableSpaceWithLineBSize: LogicalRect = aState.GetFloatAvailableSpaceForBSize(wm, aLine.BStart(), *aAvailableSpaceBSize, None).mRect;

        // If the available space between the floats is smaller now that we
        // know the BSize, return false (and cause another pass with
        // LineReflowStatus::RedoMoreFloats).  We ensure aAvailableSpaceBSize
        // never decreases, which means that we can't reduce the set of floats
        // we intersect, which means that the available space cannot grow.
        if AvailableSpaceShrunk(wm, &floatAvailableSpaceWithOldLineBSize, &floatAvailableSpaceWithLineBSize, false) {
            // Prepare data for redoing the line.
            aState.mLineBSize = Some(aLine.BSize());

            // Since we want to redo the line, we update aFlowArea by using the
            // aFloatStateBeforeLine, which is the float manager's state before the
            // line is placed.
            let oldFloatAvailableSpace: LogicalRect = aFlowArea.mRect;
            *aFlowArea = aState.GetFloatAvailableSpaceForBSize(wm, aLine.BStart(), *aAvailableSpaceBSize, Some(aFloatStateBeforeLine));

            NS_ASSERTION!(aFlowArea.mRect.BStart(wm) == oldFloatAvailableSpace.BStart(wm), "yikes");
            // Restore the BSize to the position of the next band.
            *aFlowArea.mRect.BSize_mut(wm) = oldFloatAvailableSpace.BSize(wm);

            // Enforce both IStart() and IEnd() never move outwards to prevent
            // infinite grow-shrink loops.
            let iStartDiff: nscoord = aFlowArea.mRect.IStart(wm) - oldFloatAvailableSpace.IStart(wm);
            let iEndDiff: nscoord = aFlowArea.mRect.IEnd(wm) - oldFloatAvailableSpace.IEnd(wm);
            if iStartDiff < 0 {
                *aFlowArea.mRect.IStart_mut(wm) -= iStartDiff;
                *aFlowArea.mRect.ISize_mut(wm) += iStartDiff;
            }
            if iEndDiff > 0 {
                *aFlowArea.mRect.ISize_mut(wm) -= iEndDiff;
            }

            return false;
        }

        // #ifdef DEBUG if (!GetParent()->IsAbsurdSizeAssertSuppressed()) {...}
        // NOT PORTED: debug-only absurd-size check.

        // Only block frames horizontally align their children because
        // inline frames "shrink-wrap" around their children (therefore
        // there is no extra horizontal space).
        let styleText = nsStyleTextAdapter::of(self.0);

        /**
         * We don't care checking for IsLastLine properly if we don't care (if it
         * can't change the used text-align value for the line).
         *
         * In other words, isLastLine really means isLastLineAndWeCare.
         */
        let isLastLine = !self.IsInSVGTextSubtree() && styleText.TextAlignForLastLine() != styleText.mTextAlign() && (aLineLayout.GetLineEndsInBR() || self.IsLastLine(aState, aLine));

        aLineLayout.TextAlignLine(&mut *aLine, isLastLine);

        // From here on, pfd->mBounds rectangles are incorrect because bidi
        // might have moved frames around!
        let mut overflowAreas = OverflowAreas::default();
        aLineLayout.RelativePositionFrames__OverflowAreas(&mut overflowAreas);
        // NOT PORTED: scrolling (the :-moz-scrolled-content in-flow bounds of the line).
        aLine.SetOverflowAreas(&overflowAreas);
        if addedMarker {
            aLineLayout.RemoveMarkerFrame(self.GetOutsideMarker().unwrap());
        }

        // Inline lines do not have margins themselves; however they are
        // impacted by prior block margins. If this line ends up having some
        // height then we zero out the previous block-end margin value that was
        // already applied to the line's starting Y coordinate. Otherwise we
        // leave it be so that the previous blocks block-end margin can be
        // collapsed with a block that follows.
        let newBCoord: nscoord;

        if !aLine.CachedIsEmpty() {
            // This line has some height. Therefore the application of the
            // previous-bottom-margin should stick.
            aState.mPrevBEndMargin.Zero();
            newBCoord = aLine.BEnd();
        } else {
            // Don't let the previous-bottom-margin value affect the newBCoord
            // coordinate (it was applied in ReflowInlineFrames speculatively)
            // since the line is empty.
            // We already called |ShouldApplyBStartMargin|, and if we applied it
            // then mShouldApplyBStartMargin is set.
            let dy: nscoord = if aState.mFlags.mShouldApplyBStartMargin { -aState.mPrevBEndMargin.Get() } else { 0 };
            newBCoord = aState.mBCoord + dy;
        }

        if !aState.mReflowStatus.IsFullyComplete() && self.ShouldAvoidBreakInside(aState.mReflowInput) {
            aLine.AppendFloats(std::mem::take(&mut aState.mCurrentLineFloats));
            self.SetBreakBeforeStatusBeforeLine(aState, aLine, aKeepReflowGoing);
            return true;
        }

        // See if the line fit (our first line always does).
        if self.mLines().front() != aLine.get() && aState.ContentBSize() != NS_UNCONSTRAINEDSIZE && newBCoord > aState.ContentBEnd() {
            NS_ASSERTION!(aState.mCurrentLine == aLine, "oops");
            if self.ShouldAvoidBreakInside(aState.mReflowInput) {
                // All our content doesn't fit, start on the next page.
                self.SetBreakBeforeStatusBeforeLine(aState, aLine, aKeepReflowGoing);
            } else {
                // Push aLine and all of its children and anything else that
                // follows to our next-in-flow.
                self.PushTruncatedLine(aState, aLine, aKeepReflowGoing, nsBlockFrame_ComputeNewPageNameIfNeeded::Yes);
            }
            return true;
        }

        // Note that any early return before this update of aState.mBCoord
        // must either (a) return false or (b) set aKeepReflowGoing to false.
        // Otherwise we'll keep reflowing later lines at an incorrect
        // position, and we might not come back and clean up the damage later.
        aState.mBCoord = newBCoord;

        // Add the already placed current-line floats to the line
        aLine.AppendFloats(std::mem::take(&mut aState.mCurrentLineFloats));

        // Any below current line floats to place?
        if !aState.mBelowCurrentLineFloats.is_empty() {
            // Reflow the below-current-line floats, which places on the line's
            // float list.
            aState.PlaceBelowCurrentLineFloats(&mut *aLine);
        }

        // When a line has floats, factor them into the overflow areas computations.
        if aLine.HasFloats() {
            // Union the float overflow areas (stored in aState) and the value computed
            // by the line layout code.
            let mut lineOverflowAreas: OverflowAreas = aState.mFloatOverflowAreas;
            lineOverflowAreas.UnionWith(&aLine.GetOverflowAreas());
            aLine.SetOverflowAreas(&lineOverflowAreas);
            // NOT PORTED: scrolling (the in-flow bounds of floats for :-moz-scrolled-content).

            // #ifdef NOISY_OVERFLOW_AREAS ... NOT PORTED: debug printing.
        }

        // Apply break-after clearing if necessary
        // This must stay in sync with |ReflowDirtyLines|.
        if aLine.HasFloatClearTypeAfter() {
            aState.mBCoord = aState.ClearFloats(aState.mBCoord, aLine.FloatClearTypeAfter(), None).0;
        }
        true
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::PushLines
    pub fn PushLines(&self, aState: &mut BlockReflowState, aLineBefore: nsLineList_iterator) {
        // NOTE: aLineBefore is always a normal line, not an overflow line.
        // The following expression will assert otherwise.
        #[cfg(debug_assertions)]
        let _check = aLineBefore == self.mLines().begin_mut();

        let overBegin: nsLineList_iterator = aLineBefore.next_();

        // PushTruncatedPlaceholderLine sometimes pushes the first line.  Ugh.
        let firstLine = overBegin == self.LinesBegin();

        if overBegin != self.LinesEnd() {
            // Remove floats in the lines from floats list.
            let mut floats = nsFrameList::default();
            self.CollectFloats(overBegin.mFirstChild.unwrap(), &mut floats, true);

            if floats.NotEmpty() {
                #[cfg(debug_assertions)]
                for f in &floats {
                    MOZ_ASSERT!(!f.HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW), "CollectFloats should've removed that bit");
                }
                // Push the floats onto the front of the overflow out-of-flows list
                let mut oofs = nsBlockFrame_nsAutoOOFFrameList::new(*self);
                oofs.mList.InsertFrames(None, None, floats);
            }

            // overflow lines can already exist in some cases, in particular,
            // when shrinkwrapping and we discover that the shrinkwrap causes
            // the height of some child block to grow which creates additional
            // overflowing content. In such cases we must prepend the new
            // overflow to the existing overflow.
            let mut overflowLines = self.RemoveOverflowLines();
            if overflowLines.is_null() {
                // XXXldb use presshell arena!
                overflowLines = Box::into_raw(Box::new(nsBlockFrame_FrameLines::default()));
            }
            if !overflowLines.is_null() {
                let overflowLinesRef = unsafe { &mut *overflowLines };
                let lineBeforeLastFrame: Option<nsIFrame>;
                if firstLine {
                    lineBeforeLastFrame = None; // removes all frames
                } else {
                    let f = overBegin.mFirstChild;
                    lineBeforeLastFrame = match f {
                        Some(f) => f.GetPrevSibling(),
                        None => self.mFrames().LastChild(),
                    };
                    NS_ASSERTION!(f.is_none() || lineBeforeLastFrame == aLineBefore.LastChild(), "unexpected line frames");
                }
                let pushedFrames = self.mFrames().TakeFramesAfter(lineBeforeLastFrame);
                overflowLinesRef.mFrames.InsertFrames(None, None, pushedFrames);

                let begin = overflowLinesRef.mLines.begin_mut();
                overflowLinesRef.mLines.splice__iterator_nsLineList_iterator_iterator(begin, self.mLines(), overBegin, self.LinesEnd());
                NS_ASSERTION!(!overflowLinesRef.mLines.empty(), "should not be empty");
                // this takes ownership but it won't delete it immediately so we
                // can keep using it.
                self.SetOverflowLines(overflowLines);

                // Mark all the overflow lines dirty so that they get reflowed when
                // they are pulled up by our next-in-flow.

                let cursorForDisplay = self.GetLineCursorForDisplay();
                let cursorForQuery = self.GetLineCursorForQuery();

                // XXXldb Can this get called O(N) times making the whole thing O(N^2)?
                let mut line = overflowLinesRef.mLines.begin_mut();
                let line_end = overflowLinesRef.mLines.end_mut();
                while line != line_end {
                    if line.get() as *mut nsLineBox == cursorForDisplay || line.get() as *mut nsLineBox == cursorForQuery {
                        self.ClearLineCursors();
                    }
                    line.MarkDirty();
                    line.MarkPreviousMarginDirty();
                    line.SetMovedFragments();
                    line.SetBoundsEmpty();
                    if line.HasFloats() {
                        line.ClearFloats();
                    }
                    line.operator_inc();
                }
            }
        }

        // #ifdef DEBUG VerifyOverflowSituation();  NOT PORTED: debug-only verification.
    }

    // The overflowLines property is stored as a pointer to a line list,
    // which must be deleted.  However, the following functions all maintain
    // the invariant that the property is never set if the list is empty.

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DrainOverflowLines
    pub fn DrainOverflowLines(&self) -> bool {
        // #ifdef DEBUG VerifyOverflowSituation();  NOT PORTED: debug-only verification.

        // Steal the prev-in-flow's overflow lines and prepend them.
        let mut didFindOverflow = false;
        let prevBlock = self.GetPrevInFlow().map(nsBlockFrame::from_frame);
        if let Some(prevBlock) = prevBlock {
            prevBlock.ClearLineCursors();
            let overflowLines = prevBlock.RemoveOverflowLines();
            if !overflowLines.is_null() {
                let mut overflowLines = unsafe { Box::from_raw(overflowLines) };
                // Make all the frames on the overflow line list mine.
                nsContainerFrame::ReparentFrames(&overflowLines.mFrames, prevBlock.0, self.0);

                // Collect overflow containers from our OverflowContainers list that are
                // continuations from the frames we picked up from our prev-in-flow, then
                // prepend those to ExcessOverflowContainers to ensure the continuations
                // are ordered.
                // NOT PORTED: fragmentation (overflow containers).

                // Make the overflow out-of-flow frames mine too.
                let mut oofs = nsBlockFrame_nsAutoOOFFrameList::new(prevBlock);
                if oofs.mList.NotEmpty() {
                    // In case we own any next-in-flows of any of the drained frames, then
                    // move those to the PushedFloat list.
                    let mut pushedFloats = nsFrameList::default();
                    for f in &oofs.mList {
                        let mut nif = f.GetNextInFlow();
                        while let Some(n) = nif {
                            if n.GetParent() != Some(self.0) {
                                break;
                            }
                            MOZ_ASSERT!(n.HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW));
                            self.RemoveFloat(n);
                            pushedFloats.AppendFrame(None, n);
                            nif = n.GetNextInFlow();
                        }
                    }
                    nsContainerFrame::ReparentFrames(&oofs.mList, prevBlock.0, self.0);
                    self.EnsureFloats().InsertFrames(None, None, std::mem::take(&mut oofs.mList));
                    if !pushedFloats.IsEmpty() {
                        let pf = self.EnsurePushedFloats();
                        pf.InsertFrames(None, None, pushedFloats);
                    }
                }
                drop(oofs);

                if !self.mLines().empty() {
                    // Remember to recompute the margins on the first line. This will
                    // also recompute the correct deltaBCoord if necessary.
                    let mut front = self.LinesBegin();
                    front.MarkPreviousMarginDirty();
                }
                // The overflow lines have already been marked dirty and their previous
                // margins marked dirty also.

                // Prepend the overflow frames/lines to our principal list.
                self.mFrames().InsertFrames(None, None, std::mem::take(&mut overflowLines.mFrames));
                let begin = self.mLines().begin_mut();
                self.mLines().splice__iterator_nsLineList(begin, &mut overflowLines.mLines);
                NS_ASSERTION!(overflowLines.mLines.empty(), "splice should empty list");
                drop(overflowLines);
                didFindOverflow = true;
            }
        }

        // Now append our own overflow lines.
        self.DrainSelfOverflowList() || didFindOverflow
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DrainSelfOverflowList
    pub fn DrainSelfOverflowList(&self) -> bool {
        let ourOverflowLines = self.RemoveOverflowLines();
        if ourOverflowLines.is_null() {
            return false;
        }
        let mut ourOverflowLines = unsafe { Box::from_raw(ourOverflowLines) };

        // No need to reparent frames in our own overflow lines/oofs, because they're
        // already ours. But we should put overflow floats back in our floats list.
        // (explicit scope to remove the OOF list before VerifyOverflowSituation)
        {
            let mut oofs = nsBlockFrame_nsAutoOOFFrameList::new(*self);
            if oofs.mList.NotEmpty() {
                #[cfg(debug_assertions)]
                for f in &oofs.mList {
                    MOZ_ASSERT!(!f.HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW), "CollectFloats should've removed that bit");
                }
                // The overflow floats go after our regular floats.
                self.EnsureFloats().AppendFrames(None, std::mem::take(&mut oofs.mList));
            }
        }
        if !ourOverflowLines.mLines.empty() {
            self.mFrames().AppendFrames(None, std::mem::take(&mut ourOverflowLines.mFrames));
            let end = self.mLines().end_mut();
            self.mLines().splice__iterator_nsLineList(end, &mut ourOverflowLines.mLines);
        }

        // #ifdef DEBUG VerifyOverflowSituation();  NOT PORTED: debug-only verification.
        true
    }

    /**
     * Pushed floats are floats whose placeholders are in a previous
     * continuation.  They might themselves be next-continuations of a float
     * that partially fit in an earlier continuation, or they might be the
     * first continuation of a float that couldn't be placed at all.
     *
     * Pushed floats live permanently at the beginning of a block's float
     * list, where they must live *before* any floats whose placeholders are
     * in that block.
     *
     * Temporarily, during reflow, they also live on the pushed floats list,
     * which only holds them between (a) when one continuation pushes them to
     * its pushed floats list because they don't fit and (b) when the next
     * continuation pulls them onto the beginning of its float list.
     *
     * DrainPushedFloats sets up pushed floats the way we need them at the
     * start of reflow; they are then reflowed by ReflowPushedFloats (which
     * might push some of them on).  Floats with placeholders in this block
     * are reflowed by (BlockReflowState/nsLineLayout)::AddFloat, which
     * also maintains these invariants.
     *
     * DrainSelfPushedFloats moves any pushed floats from this block's own
     * pushed floats list back into floats list.  DrainPushedFloats additionally
     * moves frames from its prev-in-flow's pushed floats list into floats list.
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DrainSelfPushedFloats
    pub fn DrainSelfPushedFloats(&self) {
        // If we're getting reflowed multiple times without our
        // next-continuation being reflowed, we might need to pull back floats
        // that we just put in the list to be pushed to our next-in-flow.
        // We don't want to pull back any next-in-flows of floats on our own
        // float list, and we only need to pull back first-in-flows whose
        // placeholders were in earlier blocks (since first-in-flows whose
        // placeholders are in this block will get pulled appropriately by
        // AddFloat, and will then be more likely to be in the correct order).
        let ourPushedFloats = self.GetPushedFloats();
        if !ourPushedFloats.is_null() {
            let ourPushedFloats = unsafe { &mut *ourPushedFloats };
            let mut floats = self.GetFloats();

            // When we pull back floats, we want to put them with the pushed
            // floats, which must live at the start of our float list, but we
            // want them at the end of those pushed floats.
            // FIXME: This isn't quite right!  What if they're all pushed floats?
            let mut insertionPrevSibling: Option<nsIFrame> = None; /* beginning of list */
            let mut f = if floats.is_null() { None } else { unsafe { &*floats }.FirstChild() };
            while let Some(fr) = f {
                if !fr.HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW) {
                    break;
                }
                insertionPrevSibling = Some(fr);
                f = fr.GetNextSibling();
            }

            let mut f = ourPushedFloats.LastChild();
            while let Some(fr) = f {
                let prevSibling = fr.GetPrevSibling();

                let placeholder = fr.GetPlaceholderFrame();
                let floatOriginalParent = self.GetFloatContainingBlock(placeholder.unwrap());
                if floatOriginalParent != Some(self.0) {
                    // This is a first continuation that was pushed from one of our
                    // previous continuations.  Take it out of the pushed floats
                    // list and put it in our floats list, before any of our
                    // floats, but after other pushed floats.
                    ourPushedFloats.RemoveFrame(fr);
                    if floats.is_null() {
                        floats = self.EnsureFloats() as *mut nsFrameList;
                    }
                    unsafe { &mut *floats }.InsertFrame(None, insertionPrevSibling, fr);
                }

                f = prevSibling;
            }

            if ourPushedFloats.IsEmpty() {
                let stolen = self.StealPushedFloats();
                unsafe { &*stolen }.Delete();
            }
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DrainPushedFloats
    pub fn DrainPushedFloats(&self) {
        self.DrainSelfPushedFloats();

        // After our prev-in-flow has completed reflow, it may have a pushed
        // floats list, containing floats that we need to own.  Take these.
        let prevBlock = self.GetPrevInFlow().map(nsBlockFrame::from_frame);
        if let Some(prevBlock) = prevBlock {
            let stolen = prevBlock.StealPushedFloats();
            let mut list = AutoFrameListPtr::new(if stolen.is_null() { None } else { Some(unsafe { Box::from_raw(stolen) }) });
            if let Some(l) = list.operator_arrow_mut() {
                if l.NotEmpty() {
                    self.EnsureFloats().InsertFrames(Some(self.0), None, std::mem::take(l));
                }
            }
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetOverflowLines
    pub fn GetOverflowLines(&self) -> *mut nsBlockFrame_FrameLines {
        if !self.HasOverflowLines() {
            return std::ptr::null_mut();
        }
        let prop = self.GetProperty(OverflowLinesProperty()).unwrap_or(std::ptr::null_mut());
        #[cfg(debug_assertions)]
        {
            let p = unsafe { prop.as_ref() };
            NS_ASSERTION!(
                p.map_or(false, |p| !p.mLines.empty()) && (if unsafe { &*p.unwrap().mLines.front() }.GetChildCount() == 0 { p.unwrap().mFrames.IsEmpty() } else { unsafe { &*p.unwrap().mLines.front() }.mFirstChild == p.unwrap().mFrames.FirstChild() }),
                "value should always be stored and non-empty when state set"
            );
        }
        prop
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::RemoveOverflowLines
    pub fn RemoveOverflowLines(&self) -> *mut nsBlockFrame_FrameLines {
        if !self.HasOverflowLines() {
            return std::ptr::null_mut();
        }
        // TakeProperty
        let prop = self.GetProperty(OverflowLinesProperty()).unwrap_or(std::ptr::null_mut());
        self.RemoveProperty(OverflowLinesProperty());
        #[cfg(debug_assertions)]
        {
            let p = unsafe { prop.as_ref() };
            NS_ASSERTION!(
                p.map_or(false, |p| !p.mLines.empty()) && (if unsafe { &*p.unwrap().mLines.front() }.GetChildCount() == 0 { p.unwrap().mFrames.IsEmpty() } else { unsafe { &*p.unwrap().mLines.front() }.mFirstChild == p.unwrap().mFrames.FirstChild() }),
                "value should always be stored and non-empty when state set"
            );
        }
        self.RemoveStateBits(NS_BLOCK_HAS_OVERFLOW_LINES);
        prop
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DestroyOverflowLines
    pub fn DestroyOverflowLines(&self) {
        NS_ASSERTION!(self.HasOverflowLines(), "huh?");
        let prop = self.GetProperty(OverflowLinesProperty()).unwrap_or(std::ptr::null_mut());
        self.RemoveProperty(OverflowLinesProperty());
        NS_ASSERTION!(!prop.is_null() && unsafe { &*prop }.mLines.empty(), "value should always be stored but empty when destroying");
        self.RemoveStateBits(NS_BLOCK_HAS_OVERFLOW_LINES);
        if !prop.is_null() {
            drop(unsafe { Box::from_raw(prop) });
        }
    }

    // This takes ownership of aOverflowLines.
    // XXX We should allocate overflowLines from presShell arena!
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::SetOverflowLines
    pub fn SetOverflowLines(&self, aOverflowLines: *mut nsBlockFrame_FrameLines) {
        NS_ASSERTION!(!aOverflowLines.is_null(), "null lines");
        let l = unsafe { &*aOverflowLines };
        NS_ASSERTION!(!l.mLines.empty(), "empty lines");
        NS_ASSERTION!(unsafe { &*l.mLines.front() }.mFirstChild == l.mFrames.FirstChild(), "invalid overflow lines / frames");
        NS_ASSERTION!(!self.HasAnyStateBits(NS_BLOCK_HAS_OVERFLOW_LINES), "Overwriting existing overflow lines");

        // Verify that we won't overwrite an existing overflow list
        NS_ASSERTION!(self.GetProperty(OverflowLinesProperty()).is_none(), "existing overflow list");
        self.SetProperty(OverflowLinesProperty(), aOverflowLines);
        self.AddStateBits(NS_BLOCK_HAS_OVERFLOW_LINES);
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetOverflowOutOfFlows
    pub fn GetOverflowOutOfFlows(&self) -> *mut nsFrameList {
        if !self.HasAnyStateBits(NS_BLOCK_HAS_OVERFLOW_OUT_OF_FLOWS) {
            return std::ptr::null_mut();
        }
        let result = self.GetProperty(OverflowOutOfFlowsProperty()).unwrap_or(std::ptr::null_mut());
        NS_ASSERTION!(!result.is_null(), "value should always be non-empty when state set");
        result
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::SetOverflowOutOfFlows
    // This takes ownership of the frames in aList.
    pub fn SetOverflowOutOfFlows(&self, mut aList: nsFrameList, aPropValue: *mut nsFrameList) {
        MOZ_ASSERT!(self.HasAnyStateBits(NS_BLOCK_HAS_OVERFLOW_OUT_OF_FLOWS) == !aPropValue.is_null(), "state does not match value");

        if aList.IsEmpty() {
            if !self.HasAnyStateBits(NS_BLOCK_HAS_OVERFLOW_OUT_OF_FLOWS) {
                return;
            }
            let list = self.GetProperty(OverflowOutOfFlowsProperty()).unwrap_or(std::ptr::null_mut());
            self.RemoveProperty(OverflowOutOfFlowsProperty());
            NS_ASSERTION!(aPropValue == list, "prop value mismatch");
            let list = unsafe { &mut *list };
            list.Clear();
            list.Delete();
            self.RemoveStateBits(NS_BLOCK_HAS_OVERFLOW_OUT_OF_FLOWS);
        } else if self.HasAnyStateBits(NS_BLOCK_HAS_OVERFLOW_OUT_OF_FLOWS) {
            NS_ASSERTION!(aPropValue == self.GetProperty(OverflowOutOfFlowsProperty()).unwrap_or(std::ptr::null_mut()), "prop value mismatch");
            unsafe { *aPropValue = std::mem::take(&mut aList) };
        } else {
            self.SetProperty(OverflowOutOfFlowsProperty(), Box::into_raw(Box::new(aList)));
            self.AddStateBits(NS_BLOCK_HAS_OVERFLOW_OUT_OF_FLOWS);
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetInsideMarker
    pub fn GetInsideMarker(&self) -> Option<nsIFrame> {
        if !self.HasMarker() {
            return None;
        }
        if let Some(frame) = self.GetProperty(InsideMarkerProperty()) {
            return Some(frame);
        }
        None
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetOutsideMarker
    pub fn GetOutsideMarker(&self) -> Option<nsIFrame> {
        let list = self.GetOutsideMarkerList();
        if list.is_null() { None } else { unsafe { &*list }.FirstChild() }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetOutsideMarkerList
    pub fn GetOutsideMarkerList(&self) -> *mut nsFrameList {
        if !self.HasMarker() {
            return std::ptr::null_mut();
        }
        if let Some(list) = self.GetProperty(OutsideMarkerProperty()) {
            MOZ_ASSERT!(unsafe { &*list }.GetLength() == 1, "bogus outside ::marker list");
            return list;
        }
        std::ptr::null_mut()
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::HasFloats
    pub fn HasFloats(&self) -> bool {
        let isStateBitSet = self.HasAnyStateBits(NS_BLOCK_HAS_FLOATS);
        MOZ_ASSERT!(isStateBitSet == self.HasProperty(FloatsProperty()), "State bit should accurately reflect presence/absence of the property!");
        isStateBitSet
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetFloats
    pub fn GetFloats(&self) -> *mut nsFrameList {
        if !self.HasFloats() {
            return std::ptr::null_mut();
        }
        let list = self.GetProperty(FloatsProperty()).unwrap_or(std::ptr::null_mut());
        MOZ_ASSERT!(!list.is_null(), "List should always be valid when the property is set!");
        MOZ_ASSERT!(unsafe { &*list }.NotEmpty(), "Someone forgot to delete the list when it is empty!");
        list
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::EnsureFloats
    // (MOZ_NONNULL_RETURN: a reference.)
    pub fn EnsureFloats(&self) -> &'static mut nsFrameList {
        let list = self.GetFloats();
        if !list.is_null() {
            return unsafe { &mut *list };
        }
        let list = Box::into_raw(Box::new(nsFrameList::default()));
        self.SetProperty(FloatsProperty(), list);
        self.AddStateBits(NS_BLOCK_HAS_FLOATS);
        unsafe { &mut *list }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::StealFloats
    pub fn StealFloats(&self) -> *mut nsFrameList {
        if !self.HasFloats() {
            return std::ptr::null_mut();
        }
        let list = self.GetProperty(FloatsProperty()).unwrap_or(std::ptr::null_mut());
        self.RemoveProperty(FloatsProperty());
        self.RemoveStateBits(NS_BLOCK_HAS_FLOATS);
        MOZ_ASSERT!(!list.is_null(), "List should always be valid when the property is set!");
        list
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::HasPushedFloats
    pub fn HasPushedFloats(&self) -> bool {
        let isStateBitSet = self.HasAnyStateBits(NS_BLOCK_HAS_PUSHED_FLOATS);
        MOZ_ASSERT!(isStateBitSet == self.HasProperty(PushedFloatsProperty()), "State bit should accurately reflect presence/absence of the property!");
        isStateBitSet
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetPushedFloats
    pub fn GetPushedFloats(&self) -> *mut nsFrameList {
        if !self.HasPushedFloats() {
            return std::ptr::null_mut();
        }
        let list = self.GetProperty(PushedFloatsProperty()).unwrap_or(std::ptr::null_mut());
        MOZ_ASSERT!(!list.is_null(), "List should always be valid when the property is set!");
        MOZ_ASSERT!(unsafe { &*list }.NotEmpty(), "Someone forgot to delete the list when it is empty!");
        list
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::EnsurePushedFloats
    pub fn EnsurePushedFloats(&self) -> &'static mut nsFrameList {
        let result = self.GetPushedFloats();
        if !result.is_null() {
            return unsafe { &mut *result };
        }

        let result = Box::into_raw(Box::new(nsFrameList::default()));
        self.SetProperty(PushedFloatsProperty(), result);
        self.AddStateBits(NS_BLOCK_HAS_PUSHED_FLOATS);

        unsafe { &mut *result }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::StealPushedFloats
    pub fn StealPushedFloats(&self) -> *mut nsFrameList {
        if !self.HasPushedFloats() {
            return std::ptr::null_mut();
        }
        let list = self.GetProperty(PushedFloatsProperty()).unwrap_or(std::ptr::null_mut());
        self.RemoveProperty(PushedFloatsProperty());
        self.RemoveStateBits(NS_BLOCK_HAS_PUSHED_FLOATS);
        MOZ_ASSERT!(!list.is_null(), "List should always be valid when the property is set!");
        list
    }
}

// Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::nsAutoOOFFrameList
/**
 * This class is useful for efficiently modifying the out of flow
 * overflow list. It gives the client direct writable access to
 * the frame list temporarily but ensures that property is only
 * written back if absolutely necessary.
 */
pub struct nsBlockFrame_nsAutoOOFFrameList {
    pub mList: nsFrameList,
    mPropValue: *mut nsFrameList,
    mBlock: nsBlockFrame,
}

impl nsBlockFrame_nsAutoOOFFrameList {
    // Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::nsAutoOOFFrameList::nsAutoOOFFrameList
    pub fn new(aBlock: nsBlockFrame) -> nsBlockFrame_nsAutoOOFFrameList {
        let mPropValue = aBlock.GetOverflowOutOfFlows();
        let mut mList = nsFrameList::default();
        if !mPropValue.is_null() {
            mList = std::mem::take(unsafe { &mut *mPropValue });
        }
        nsBlockFrame_nsAutoOOFFrameList { mList, mPropValue, mBlock: aBlock }
    }
}

// Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::nsAutoOOFFrameList::~nsAutoOOFFrameList
impl Drop for nsBlockFrame_nsAutoOOFFrameList {
    fn drop(&mut self) {
        self.mBlock.SetOverflowOutOfFlows(std::mem::take(&mut self.mList), self.mPropValue);
    }
}

//////////////////////////////////////////////////////////////////////
// Frame list manipulation routines

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::AppendFrames
    pub fn AppendFrames(&self, aListID: FrameChildListID, aFrameList: nsFrameList) {
        if aFrameList.IsEmpty() {
            return;
        }
        if aListID != FrameChildListID::Principal {
            if FrameChildListID::Float == aListID {
                self.DrainSelfPushedFloats(); // ensure the last frame is in floats list.
                self.EnsureFloats().AppendFrames(None, aFrameList);
                return;
            }
            MOZ_ASSERT!(FrameChildListID::NoReflowPrincipal == aListID, "unexpected child list");
        }

        // Find the proper last-child for where the append should go
        let lastKid = self.mFrames().LastChild();
        NS_ASSERTION!(
            (if self.mLines().empty() { None } else { unsafe { &*self.mLines().back() }.LastChild() }) == lastKid,
            "out-of-sync mLines / mFrames"
        );

        // #ifdef NOISY_REFLOW_REASON ... NOT PORTED: debug printing.

        if self.IsInSVGTextSubtree() {
            // NOT PORTED: SVG text (the parent is never an SVGTextFrame here).
            MOZ_ASSERT!(self.GetParent().unwrap().IsSVGTextFrame(), "unexpected block frame in SVG text");
        }

        self.AddFrames(aFrameList, lastKid, None);
        if aListID != FrameChildListID::NoReflowPrincipal {
            self.PresShell().FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN); // XXX sufficient?
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::InsertFrames
    pub fn InsertFrames(&self, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aPrevFrameLine: Option<&nsLineList_iterator>, aFrameList: nsFrameList) {
        NS_ASSERTION!(aPrevFrame.is_none() || aPrevFrame.unwrap().GetParent() == Some(self.0), "inserting after sibling frame with different parent");

        if aListID != FrameChildListID::Principal {
            if FrameChildListID::Float == aListID {
                self.DrainSelfPushedFloats(); // ensure aPrevFrame is in floats list.
                self.EnsureFloats().InsertFrames(Some(self.0), aPrevFrame, aFrameList);
                return;
            }
            MOZ_ASSERT!(FrameChildListID::NoReflowPrincipal == aListID, "unexpected child list");
        }

        // #ifdef NOISY_REFLOW_REASON ... NOT PORTED: debug printing.

        self.AddFrames(aFrameList, aPrevFrame, aPrevFrameLine);
        if aListID != FrameChildListID::NoReflowPrincipal {
            self.PresShell().FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN); // XXX sufficient?
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::RemoveFrame
    pub fn RemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        // #ifdef NOISY_REFLOW_REASON ... NOT PORTED: debug printing.

        if aListID == FrameChildListID::Principal {
            let hasFloats = BlockHasAnyFloats(aOldFrame);
            self.DoRemoveFrame(aContext, aOldFrame, nsBlockFrame::REMOVE_FIXED_CONTINUATIONS);
            if hasFloats {
                MarkSameFloatManagerLinesDirty(*self);
            }
        } else if FrameChildListID::Float == aListID {
            // Make sure to mark affected lines dirty for the float frame
            // we are removing; this way is a bit messy, but so is the rest of the code.
            // See bug 390762.
            NS_ASSERTION!(aOldFrame.GetPrevContinuation().is_none(), "RemoveFrame should not be called on pushed floats.");
            let mut f = Some(aOldFrame);
            while let Some(fr) = f {
                if fr.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) {
                    break;
                }
                MarkSameFloatManagerLinesDirty(nsBlockFrame::from_frame(fr.GetParent().unwrap()));
                f = fr.GetNextContinuation();
            }
            nsBlockFrame::DoRemoveFloats(aContext, aOldFrame);
        } else if FrameChildListID::NoReflowPrincipal == aListID {
            // Skip the call to |FrameNeedsReflow| below by returning now.
            self.DoRemoveFrame(aContext, aOldFrame, nsBlockFrame::REMOVE_FIXED_CONTINUATIONS);
            return;
        } else {
            MOZ_CRASH!("unexpected child list");
        }

        self.PresShell().FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN); // XXX sufficient?
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp ShouldPutNextSiblingOnNewLine
fn ShouldPutNextSiblingOnNewLine(aLastFrame: nsIFrame) -> bool {
    let type_ = aLastFrame.Type();
    if type_ == LayoutFrameType::Br {
        return true;
    }
    // XXX the TEXT_OFFSETS_NEED_FIXING check is a wallpaper for bug 822910.
    if type_ == LayoutFrameType::Text && !aLastFrame.HasAnyStateBits(TEXT_OFFSETS_NEED_FIXING) {
        return aLastFrame.HasSignificantTerminalNewline();
    }
    false
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::AddFrames
    pub fn AddFrames(&self, aFrameList: nsFrameList, mut aPrevSibling: Option<nsIFrame>, aPrevSiblingLine: Option<&nsLineList_iterator>) {
        // Clear our line cursor, since our lines may change.
        self.ClearLineCursors();

        if aFrameList.IsEmpty() {
            return;
        }

        // Attempt to find the line that contains the previous sibling
        let mut lineList: *mut nsLineList = self.mLines() as *mut nsLineList;
        let mut frames: *mut nsFrameList = self.mFrames().as_ptr();
        let mut prevSibLine: nsLineList_iterator;
        let mut prevSiblingIndex: i32;
        if let Some(aPrevSiblingLine) = aPrevSiblingLine {
            MOZ_ASSERT!(aPrevSibling.is_some());
            prevSibLine = *aPrevSiblingLine;
            let overflowLines = self.GetOverflowLines();
            // MOZ_ASSERT(prevSibLine.IsInSameList(mLines.begin()) || (overflowLines && prevSibLine.IsInSameList(overflowLines->mLines.begin())), "must be one of our line lists");
            if !overflowLines.is_null() {
                // We need to find out which list it's actually in.  Assume that
                // *if* we have overflow lines, that our primary lines aren't
                // huge, but our overflow lines might be.
                let mut line = self.mLines().begin_mut();
                let lineEnd = self.mLines().end_mut();
                while line != lineEnd {
                    if line == prevSibLine {
                        break;
                    }
                    line.operator_inc();
                }
                if line == lineEnd {
                    // By elimination, the line must be in our overflow lines.
                    lineList = unsafe { &mut (*overflowLines).mLines } as *mut nsLineList;
                    frames = unsafe { &mut (*overflowLines).mFrames } as *mut nsFrameList;
                }
            }

            let nextLine = prevSibLine.next_();
            let lastFrameInLine = if nextLine == unsafe { &mut *lineList }.end_mut() { unsafe { &*frames }.LastChild() } else { nextLine.mFirstChild.unwrap().GetPrevSibling() };
            prevSiblingIndex = prevSibLine.RLIndexOf(aPrevSibling.unwrap(), lastFrameInLine);
            MOZ_ASSERT!(prevSiblingIndex >= 0, "aPrevSibling must be in aPrevSiblingLine");
        } else {
            prevSibLine = unsafe { &mut *lineList }.end_mut();
            prevSiblingIndex = -1;
            if let Some(prevSib) = aPrevSibling {
                // XXX_perf This is technically O(N^2) in some cases, but by using
                // RFind instead of Find, we make it O(N) in the most common case,
                // which is appending content.

                // Find the line that contains the previous sibling
                if !nsLineBox::RFindLineContaining(prevSib, &unsafe { &mut *lineList }.begin_mut(), &mut prevSibLine, self.mFrames().LastChild(), &mut prevSiblingIndex) {
                    // Not in mLines - try overflow lines.
                    let overflowLines = self.GetOverflowLines();
                    let mut found = false;
                    if !overflowLines.is_null() {
                        let ol = unsafe { &mut *overflowLines };
                        prevSibLine = ol.mLines.end_mut();
                        prevSiblingIndex = -1;
                        found = nsLineBox::RFindLineContaining(prevSib, &ol.mLines.begin_mut(), &mut prevSibLine, ol.mFrames.LastChild(), &mut prevSiblingIndex);
                    }
                    if found {
                        lineList = unsafe { &mut (*overflowLines).mLines } as *mut nsLineList;
                        frames = unsafe { &mut (*overflowLines).mFrames } as *mut nsFrameList;
                    } else {
                        // Note: defensive code! RFindLineContaining must not return
                        // false in this case, so if it does...
                        MOZ_ASSERT_UNREACHABLE!("prev sibling not in line list");
                        aPrevSibling = None;
                        prevSibLine = unsafe { &mut *lineList }.end_mut();
                    }
                }
            }
        }
        let lineList: &mut nsLineList = unsafe { &mut *lineList };
        let frames: &mut nsFrameList = unsafe { &mut *frames };

        // Find the frame following aPrevSibling so that we can join up the
        // two lists of frames.
        if let Some(prevSib) = aPrevSibling {
            // Split line containing aPrevSibling in two if the insertion
            // point is somewhere in the middle of the line.
            let rem: i32 = prevSibLine.GetChildCount() - prevSiblingIndex - 1;
            if rem != 0 {
                // Split the line in two where the frame(s) are being inserted.
                let line: *mut nsLineBox = self.NewLineBox__nsLineBox_nsIFrame_int32_t(prevSibLine.get() as *mut nsLineBox, prevSib.GetNextSibling().unwrap(), rem);
                lineList.after_insert(prevSibLine, line);
                // Mark prevSibLine dirty and as needing textrun invalidation, since
                // we may be breaking up text in the line. Its previous line may also
                // need to be invalidated because it may be able to pull some text up.
                self.MarkLineDirty(prevSibLine, lineList);
                // The new line will also need its textruns recomputed because of the
                // frame changes.
                unsafe { &mut *line }.MarkDirty();
                unsafe { &mut *line }.SetInvalidateTextRuns(true);
            }
        } else if !lineList.empty() {
            unsafe { &mut *lineList.front_mut() }.MarkDirty();
            unsafe { &mut *lineList.front_mut() }.SetInvalidateTextRuns(true);
        }
        let newFrames = frames.InsertFrames(None, aPrevSibling, aFrameList);

        // Walk through the new frames being added and update the line data
        // structures to fit.
        for newFrame in newFrames {
            NS_ASSERTION!(aPrevSibling.is_none() || aPrevSibling.unwrap().GetNextSibling() == Some(newFrame), "Unexpected aPrevSibling");
            NS_ASSERTION!(!newFrame.IsPlaceholderFrame() || (!newFrame.IsAbsolutelyPositioned(None) && !newFrame.IsFloating()), "Placeholders should not float or be positioned");

            let isBlock = newFrame.IsBlockOutside();

            // If the frame is a block frame, or if there is no previous line or if the
            // previous line is a block line we need to make a new line.  We also make
            // a new line, as an optimization, in the two cases we know we'll need it:
            // if the previous line ended with a <br>, or if it has significant
            // whitespace and ended in a newline.
            if isBlock || prevSibLine == lineList.end_mut() || prevSibLine.IsBlock() || (aPrevSibling.is_some() && ShouldPutNextSiblingOnNewLine(aPrevSibling.unwrap())) {
                // Create a new line for the frame and add its line to the line
                // list.
                let line: *mut nsLineBox = self.NewLineBox__nsIFrame_bool(newFrame, isBlock);
                if prevSibLine != lineList.end_mut() {
                    // Append new line after prevSibLine
                    lineList.after_insert(prevSibLine, line);
                    prevSibLine.operator_inc();
                } else {
                    // New line is going before the other lines
                    lineList.push_front(line);
                    prevSibLine = lineList.begin_mut();
                }
            } else {
                prevSibLine.NoteFrameAdded(newFrame);
                // We're adding inline content to prevSibLine, so we need to mark it
                // dirty, ensure its textruns are recomputed, and possibly do the same
                // to its previous line since that line may be able to pull content up.
                self.MarkLineDirty(prevSibLine, lineList);
            }

            aPrevSibling = Some(newFrame);
        }

        // #ifdef DEBUG MOZ_ASSERT(aFrameList.IsEmpty()); VerifyLines(true);  NOT PORTED: debug-only verification
        // (aFrameList was moved into InsertFrames).
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetRubyContentPseudoFrame
    // NOT PORTED: ruby (display: block ruby). There is never a ruby content pseudo frame.
    pub fn GetRubyContentPseudoFrame(&self) -> Option<nsIFrame> {
        let firstChild = self.mFrames().FirstChild();
        if let Some(firstChild) = firstChild {
            if firstChild.IsRubyFrame() {
                // && firstChild->Style()->GetPseudoType() == PseudoStyleType::MozBlockRubyContent
                // NOT PORTED: pseudo types (no ruby frames are ever created).
                return None;
            }
        }
        None
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetContentInsertionFrame
    pub fn GetContentInsertionFrame(&self) -> nsIFrame {
        // 'display:block ruby' use the inner (Ruby) frame for insertions.
        if let Some(rubyContentPseudoFrame) = self.GetRubyContentPseudoFrame() {
            return rubyContentPseudoFrame;
        }
        self.0
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::AppendDirectlyOwnedAnonBoxes
    // NOT PORTED: restyle of anonymous boxes (stylo post-traversal).

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::RemoveFloatFromFloatCache
    pub fn RemoveFloatFromFloatCache(&self, aFloat: nsIFrame) {
        // Find which line contains the float, so we can update
        // the float cache.
        for mut line in self.Lines() {
            if line.IsInline() && line.RemoveFloat(aFloat) {
                break;
            }
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::RemoveFloat
    pub fn RemoveFloat(&self, aFloat: nsIFrame) {
        // Floats live in floats list, pushed floats list, or overflow out-of-flow
        // list.
        MOZ_ASSERT!(
            self.GetChildList(FrameChildListID::Float).ContainsFrame(aFloat) || self.GetChildList(FrameChildListID::PushedFloats).ContainsFrame(aFloat) || self.GetChildList(FrameChildListID::OverflowOutOfFlow).ContainsFrame(aFloat),
            "aFloat is not our child or on an unexpected frame list"
        );

        let mut didStartRemovingFloat = false;
        let floats = self.GetFloats();
        if !floats.is_null() {
            let floats = unsafe { &mut *floats };
            didStartRemovingFloat = true;
            if floats.StartRemoveFrame(aFloat) {
                if floats.IsEmpty() {
                    unsafe { &*self.StealFloats() }.Delete();
                }
                return;
            }
        }

        let pushedFloats = self.GetPushedFloats();
        if !pushedFloats.is_null() {
            let pushedFloats = unsafe { &mut *pushedFloats };
            let found: bool;
            if didStartRemovingFloat {
                found = pushedFloats.ContinueRemoveFrame(aFloat);
            } else {
                didStartRemovingFloat = true;
                found = pushedFloats.StartRemoveFrame(aFloat);
            }
            if found {
                if pushedFloats.IsEmpty() {
                    unsafe { &*self.StealPushedFloats() }.Delete();
                }
                return;
            }
        }

        {
            let mut oofs = nsBlockFrame_nsAutoOOFFrameList::new(*self);
            if if didStartRemovingFloat { oofs.mList.ContinueRemoveFrame(aFloat) } else { oofs.mList.StartRemoveFrame(aFloat) } {
                return;
            }
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DoRemoveFloats
    pub fn DoRemoveFloats(aContext: &mut FrameDestroyContext, aFrame: nsIFrame) {
        // Note: nsFloatingFirstLetterFrame might lose its floating style after
        // nsBlockFrame::UpdateFirstLetterStyle().
        MOZ_ASSERT!(aFrame.IsFloating() || aFrame.Type() == LayoutFrameType::Letter, "DoRemoveFloats() can only remove float elements!");

        // The containing block is always the parent of aFrame.
        let block = nsBlockFrame::from_frame(aFrame.GetParent().unwrap());

        // First remove aFrame's next-in-flows.
        if let Some(nif) = aFrame.GetNextInFlow() {
            nif.GetParent().unwrap().DeleteNextInFlowChild(aContext, nif, false);
        }
        // Now remove aFrame from its child list and Destroy it.
        block.RemoveFloatFromFloatCache(aFrame);
        block.RemoveFloat(aFrame);
        aFrame.Destroy(aContext);
    }

    /**
     * This helps us iterate over the list of all normal + overflow lines
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::TryAllLines
    pub fn TryAllLines(&self, aIterator: &mut nsLineList_iterator, aStartIterator: &mut nsLineList_iterator, aEndIterator: &mut nsLineList_iterator, aInOverflowLines: &mut bool, aOverflowLines: &mut *mut nsBlockFrame_FrameLines) {
        if *aIterator == *aEndIterator {
            if !*aInOverflowLines {
                // Try the overflow lines
                *aInOverflowLines = true;
                let lines = self.GetOverflowLines();
                if !lines.is_null() {
                    *aStartIterator = unsafe { &mut *lines }.mLines.begin_mut();
                    *aIterator = *aStartIterator;
                    *aEndIterator = unsafe { &mut *lines }.mLines.end_mut();
                    *aOverflowLines = lines;
                }
            }
        }
    }
}

// Gecko: layout/generic/nsBlockFrame.h nsBlockInFlowLineIterator
/**
 * Iterates over all lines in the prev-in-flows/next-in-flows of this block.
 */
#[derive(Clone)]
pub struct nsBlockInFlowLineIterator {
    mFrame: Option<nsBlockFrame>,
    mLine: nsLineList_iterator,
    mLineList: *mut nsLineList, // the line list mLine is in
}

impl nsBlockInFlowLineIterator {
    // Allow to be uninitialized (and then assigned from another object).
    // Gecko: layout/generic/nsBlockFrame.h nsBlockInFlowLineIterator::nsBlockInFlowLineIterator()
    pub fn new() -> nsBlockInFlowLineIterator {
        nsBlockInFlowLineIterator { mFrame: None, mLine: nsLineList_iterator::default(), mLineList: std::ptr::null_mut() }
    }

    /**
     * Set up the iterator to point to aLine which must be a normal line
     * in aFrame (not an overflow line).
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockInFlowLineIterator::nsBlockInFlowLineIterator(nsBlockFrame*, LineIterator)
    pub fn new__nsBlockFrame_LineIterator(aFrame: nsBlockFrame, aLine: nsLineList_iterator) -> nsBlockInFlowLineIterator {
        let this = nsBlockInFlowLineIterator { mFrame: Some(aFrame), mLine: aLine, mLineList: aFrame.mLines() as *mut nsLineList };
        // This will assert if aLine isn't in mLines of aFrame:
        let _check = aLine == aFrame.LinesBegin();
        this
    }

    // XXX nsBlockFrame uses this internally in one place.  Try to remove it.
    // XXX uhm, and nsBidiPresUtils::Resolve too.
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockInFlowLineIterator::nsBlockInFlowLineIterator(nsBlockFrame*, LineIterator, bool)
    pub fn new__nsBlockFrame_LineIterator_bool(aFrame: nsBlockFrame, aLine: nsLineList_iterator, aInOverflow: bool) -> nsBlockInFlowLineIterator {
        nsBlockInFlowLineIterator {
            mFrame: Some(aFrame),
            mLine: aLine,
            mLineList: if aInOverflow { &mut unsafe { &mut *aFrame.GetOverflowLines() }.mLines as *mut nsLineList } else { aFrame.mLines() as *mut nsLineList },
        }
    }

    /**
     * Set up the iterator to point to the first line found starting from
     * aFrame. Sets aFoundValidLine to false if there is no such line.
     * After aFoundValidLine has returned false, don't call any methods on this
     * object again.
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockInFlowLineIterator::nsBlockInFlowLineIterator(nsBlockFrame*, bool*)
    pub fn new__nsBlockFrame_bool(aFrame: nsBlockFrame, aFoundValidLine: &mut bool) -> nsBlockInFlowLineIterator {
        let mut this = nsBlockInFlowLineIterator { mFrame: Some(aFrame), mLine: nsLineList_iterator::default(), mLineList: aFrame.mLines() as *mut nsLineList };
        this.mLine = aFrame.LinesBegin();
        *aFoundValidLine = this.FindValidLine();
        this
    }

    /**
     * Set up the iterator to point to the line that contains aFindFrame (either
     * directly or indirectly).  If aFrame is out of flow, or contained in an
     * out-of-flow, finds the line containing the out-of-flow's placeholder. If
     * the frame is not found, sets aFoundValidLine to false. After
     * aFoundValidLine has returned false, don't call any methods on this
     * object again.
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockInFlowLineIterator::nsBlockInFlowLineIterator(nsBlockFrame*, nsIFrame*, bool*)
    pub fn new__nsBlockFrame_nsIFrame_bool(aFrame: nsBlockFrame, aFindFrame: nsIFrame, aFoundValidLine: &mut bool) -> nsBlockInFlowLineIterator {
        let mut this = nsBlockInFlowLineIterator { mFrame: Some(aFrame), mLine: nsLineList_iterator::default(), mLineList: aFrame.mLines() as *mut nsLineList };
        *aFoundValidLine = false;

        let Some(child) = FindChildContaining(aFrame, aFindFrame) else {
            return this;
        };

        let line_end = aFrame.LinesEnd();
        this.mLine = aFrame.LinesBegin();
        if this.mLine != line_end && this.mLine.next_() == line_end && !aFrame.HasOverflowLines() {
            // The block has a single line - that must be it!
            *aFoundValidLine = true;
            return this;
        }

        // Try to use the cursor if it exists, otherwise fall back to the first line
        let cursor = aFrame.GetLineCursorForQuery();
        if !cursor.is_null() {
            this.mLine = line_end;
            // Perform a simultaneous forward and reverse search starting from the
            // line cursor.
            let mut line = aFrame.LinesBeginFrom(cursor);
            let mut rline = aFrame.LinesRBeginFrom(cursor);
            let rline_end = aFrame.LinesREnd();
            // rline is positioned on the line containing 'cursor', so it's not
            // rline_end. So we can safely increment it (i.e. move it to one line
            // earlier) to start searching there.
            rline.operator_inc();
            while line != line_end || rline != rline_end {
                if line != line_end {
                    if line.Contains(child) {
                        this.mLine = line;
                        break;
                    }
                    line.operator_inc();
                }
                if rline != rline_end {
                    if rline.Contains(child) {
                        this.mLine.SetPosition(rline.get() as *mut nsLineBox);
                        break;
                    }
                    rline.operator_inc();
                }
            }
            if this.mLine != line_end {
                *aFoundValidLine = true;
                if this.mLine.get() as *mut nsLineBox != cursor {
                    aFrame.SetProperty(LineCursorPropertyQuery(), this.mLine.get() as *mut nsLineBox);
                }
                return this;
            }
        } else {
            this.mLine = aFrame.LinesBegin();
            while this.mLine != line_end {
                if this.mLine.Contains(child) {
                    *aFoundValidLine = true;
                    return this;
                }
                this.mLine.operator_inc();
            }
        }
        // Didn't find the line
        MOZ_ASSERT!(this.mLine == line_end, "mLine should be line_end at this point");

        // If we reach here, it means that we have not been able to find the
        // desired frame in our in-flow lines.  So we should start looking at
        // our overflow lines. In order to do that, we set mLine to the end
        // iterator so that FindValidLine starts to look at overflow lines,
        // if any.

        if !this.FindValidLine() {
            return this;
        }

        loop {
            if this.mLine.Contains(child) {
                *aFoundValidLine = true;
                return this;
            }
            if !this.Next() {
                break;
            }
        }
        this
    }

    // Gecko: layout/generic/nsBlockFrame.h nsBlockInFlowLineIterator::GetLine
    pub fn GetLine(&self) -> nsLineList_iterator {
        self.mLine
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockInFlowLineIterator::GetContainer
    pub fn GetContainer(&self) -> nsBlockFrame {
        self.mFrame.unwrap()
    }
    // ADAPTER: GetContainer for an iterator that has no container yet (Gecko: nullptr)
    pub fn GetContainerOpt(&self) -> Option<nsBlockFrame> {
        self.mFrame
    }
    // ADAPTER: GetInOverflow for an iterator that has no container yet (false)
    pub fn GetInOverflowOpt(&self) -> bool {
        self.mFrame.is_some() && self.GetInOverflow()
    }
    // Gecko: layout/generic/nsBlockFrame.h nsBlockInFlowLineIterator::GetInOverflow
    pub fn GetInOverflow(&self) -> bool {
        self.mLineList != self.mFrame.unwrap().mLines() as *mut nsLineList
    }

    /**
     * Returns the current line list we're iterating, null means
     * we're iterating |mLines| of the container.
     */
    // Gecko: layout/generic/nsBlockFrame.h nsBlockInFlowLineIterator::GetLineList
    pub fn GetLineList(&self) -> *mut nsLineList {
        self.mLineList
    }

    /**
     * Returns the end-iterator of whatever line list we're in.
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockInFlowLineIterator::End
    pub fn End(&self) -> nsLineList_iterator {
        unsafe { &mut *self.mLineList }.end_mut()
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockInFlowLineIterator::IsLastLineInList
    pub fn IsLastLineInList(&self) -> bool {
        let end = self.End();
        self.mLine != end && self.mLine.next_() == end
    }

    /**
     * Returns false if there are no more lines. After this has returned false,
     * don't call any methods on this object again.
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockInFlowLineIterator::Next
    pub fn Next(&mut self) -> bool {
        self.mLine.operator_inc();
        self.FindValidLine()
    }

    /**
     * Returns false if there are no more lines. After this has returned false,
     * don't call any methods on this object again.
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockInFlowLineIterator::Prev
    pub fn Prev(&mut self) -> bool {
        let begin = unsafe { &mut *self.mLineList }.begin_mut();
        if self.mLine != begin {
            self.mLine.operator_dec();
            return true;
        }
        let mut currentlyInOverflowLines = self.GetInOverflow();
        loop {
            if currentlyInOverflowLines {
                self.mLineList = self.mFrame.unwrap().mLines() as *mut nsLineList;
                self.mLine = unsafe { &mut *self.mLineList }.end_mut();
                if self.mLine != unsafe { &mut *self.mLineList }.begin_mut() {
                    self.mLine.operator_dec();
                    return true;
                }
            } else {
                self.mFrame = self.mFrame.unwrap().GetPrevInFlow().map(nsBlockFrame::from_frame);
                let Some(frame) = self.mFrame else {
                    return false;
                };
                let overflowLines = frame.GetOverflowLines();
                if !overflowLines.is_null() {
                    self.mLineList = unsafe { &mut (*overflowLines).mLines } as *mut nsLineList;
                    self.mLine = unsafe { &mut *self.mLineList }.end_mut();
                    NS_ASSERTION!(self.mLine != unsafe { &mut *self.mLineList }.begin_mut(), "empty overflow line list?");
                    self.mLine.operator_dec();
                    return true;
                }
            }
            currentlyInOverflowLines = !currentlyInOverflowLines;
        }
    }

    /**
     * Moves iterator to next valid line reachable from the current block.
     * Returns false if there are no valid lines.
     */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockInFlowLineIterator::FindValidLine
    fn FindValidLine(&mut self) -> bool {
        let end = unsafe { &mut *self.mLineList }.end_mut();
        if self.mLine != end {
            return true;
        }
        let mut currentlyInOverflowLines = self.GetInOverflow();
        loop {
            if currentlyInOverflowLines {
                self.mFrame = self.mFrame.unwrap().GetNextInFlow().map(nsBlockFrame::from_frame);
                let Some(frame) = self.mFrame else {
                    return false;
                };
                self.mLineList = frame.mLines() as *mut nsLineList;
                self.mLine = unsafe { &mut *self.mLineList }.begin_mut();
                if self.mLine != unsafe { &mut *self.mLineList }.end_mut() {
                    return true;
                }
            } else {
                let overflowLines = self.mFrame.unwrap().GetOverflowLines();
                if !overflowLines.is_null() {
                    self.mLineList = unsafe { &mut (*overflowLines).mLines } as *mut nsLineList;
                    self.mLine = unsafe { &mut *self.mLineList }.begin_mut();
                    NS_ASSERTION!(self.mLine != unsafe { &mut *self.mLineList }.end_mut(), "empty overflow line list?");
                    return true;
                }
            }
            currentlyInOverflowLines = !currentlyInOverflowLines;
        }
    }
}

// Gecko: layout/generic/nsBlockFrame.cpp AnonymousBoxIsBFC
// NOT PORTED: pseudo types. The servo configuration of Stylo has no anonymous box pseudo types, so the
// frame constructor tells the frame (mIsBlockWrapper is the one bit kept; the BFC anonymous boxes
// (:-moz-fieldset-content, :-moz-column-content, :-moz-cell-content, :-moz-scrolled-content,
// :-moz-anonymous-item) are not created by this runtime).
fn AnonymousBoxIsBFC(aFrame: nsIFrame) -> bool {
    false
}

// Gecko: layout/generic/nsBlockFrame.cpp StyleEstablishesBFC
fn StyleEstablishesBFC(aFrame: nsIFrame) -> bool {
    // paint/layout containment boxes and multi-column containers establish an
    // independent formatting context.
    // https://drafts.csswg.org/css-contain/#containment-paint
    // https://drafts.csswg.org/css-contain/#containment-layout
    // https://github.com/w3c/csswg-drafts/issues/10544
    // https://drafts.csswg.org/css-align/#distribution-block
    // https://drafts.csswg.org/css-multicol/#columns
    let disp = aFrame.StyleDisplay();
    disp.IsContainPaint()
        || disp.IsContainLayout()
        || disp.mContainerType_SizeOrInlineSize()
        || disp.DisplayInside() == StyleDisplayInside::FlowRoot
        || disp.IsAbsolutelyPositionedStyle()
        || disp.IsFloatingStyle()
        || aFrame.IsRootElementStyle()
        || AnonymousBoxIsBFC(aFrame)
}

// ADAPTER: is aFrame a block whose `overflow` is not visible or clip, so that Gecko would have wrapped it in
// a ScrollContainerFrame (nsCSSFrameConstructor::ConstructScrollableBlock) and made its content a BFC? The
// root element and the body whose overflow is propagated to the viewport (the style of the viewport takes it,
// the frames of html and body are not scrolled) are not.
fn IsScrolledContentStandIn(aFrame: nsIFrame) -> bool {
    if !aFrame.StyleDisplay().IsScrollableOverflow() || aFrame.IsRootElementStyle() {
        return false;
    }
    if aFrame.GetContent().map_or(false, |c| c.IsHTMLElement(nsGkAtoms::body)) {
        if let Some(parent) = aFrame.GetParent() {
            if parent.IsRootElementStyle() && !parent.StyleDisplay().IsScrollableOverflow() {
                return false;
            }
        }
    }
    true
}

// Gecko: layout/generic/nsBlockFrame.cpp EstablishesBFC
fn EstablishesBFC(aFrame: nsBlockFrame) -> bool {
    // aFrame->HasAnyClassFlag(LayoutFrameClassFlags::BlockFormattingContext)
    // NOT PORTED: class flags (the subclasses that always establish a BFC: table cells, scroll
    // frames, ...: none is a nsBlockFrame here).
    // ADAPTER: there are no scroll frames (ConstructScrollableBlock builds the plain block), so a block with
    // scrollable overflow is the BFC that the scroll frame's scrolled-content block (AnonymousBoxIsBFC of
    // `::-moz-scrolled-content`) would be: its children's margins do not leave it and it does not overlap floats.
    if IsScrolledContentStandIn(aFrame.0) {
        return true;
    }

    if let Some(parent) = aFrame.GetParent() {
        if parent.IsFieldSetFrame() {
            // A rendered legend always establishes a new formatting context, and so
            // does the fieldset content frame, so we can just return true here.
            // https://html.spec.whatwg.org/#rendered-legend
            return true;
        }

        let wm = aFrame.GetWritingMode();
        let parentWM = parent.GetWritingMode();
        if wm.GetBlockDir() != parentWM.GetBlockDir() || wm.IsVerticalSideways() != parentWM.IsVerticalSideways() {
            // If a box has a different writing-mode value than its containing block
            // [...] if the box is a block container, then it establishes a new block
            // formatting context.
            // https://drafts.csswg.org/css-writing-modes/#block-flow
            return true;
        }
    }

    if aFrame.IsColumnSpan() {
        return true;
    }

    if aFrame.IsContentAligned() {
        return true;
    }

    // if (aFrame->IsSuppressedScrollableBlockForPrint()) return true;
    // NOT PORTED: printing.

    // const auto* style = aFrame->Style();
    // if (style->GetPseudoType() == PseudoStyleType::Marker) {...}
    // NOT PORTED: pseudo types (the outside ::marker frame is flagged by the frame constructor
    // through SetMarkerFrameForListItem; whether an outside ::marker is a BFC is then decided by
    // IsOutsideMarker, a TEMP stand-in).
    if aFrame.IsOutsideMarkerFrame() {
        // An outside ::marker needs to be an independent formatting context
        // to avoid being influenced by the float manager etc.
        return true;
    }
    StyleEstablishesBFC(aFrame.0)
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DidSetComputedStyle
    pub fn DidSetComputedStyle(&self, aOldStyle: Option<()>) {
        self.nsContainerFrame_DidSetComputedStyle(aOldStyle);
        // if (IsInSVGTextSubtree() && ...non-scaling stroke...)
        // NOT PORTED: SVG text.
        if aOldStyle.is_none() {
            return;
        }

        let isBFC = EstablishesBFC(*self);
        if self.HasAnyStateBits(NS_BLOCK_BFC) != isBFC {
            if self.MaybeHasFloats() {
                // If the frame contains floats, this update may change their float
                // manager. Be safe by dirtying all descendant lines of the nearest
                // ancestor's float manager.
                self.RemoveStateBits(NS_BLOCK_BFC);
                MarkSameFloatManagerLinesDirty(*self);
            }
            self.AddOrRemoveStateBits(NS_BLOCK_BFC, isBFC);
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::UpdateFirstLetterStyle
    // NOT PORTED: ::first-letter frames and restyling (Stylo post-traversal).
}

// Gecko: layout/generic/nsBlockFrame.cpp FindChildContaining
fn FindChildContaining(aFrame: nsBlockFrame, mut aFindFrame: nsIFrame) -> Option<nsIFrame> {
    let mut child: Option<nsIFrame>;
    loop {
        let mut block: Option<nsIFrame> = Some(aFrame.0);
        loop {
            child = nsLayoutUtils::FindChildContainingDescendant(block.unwrap(), aFindFrame);
            if child.is_some() {
                break;
            }
            block = block.unwrap().GetNextContinuation();
            if block.is_none() {
                break;
            }
        }
        let Some(c) = child else {
            return None;
        };
        if !c.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) {
            break;
        }
        aFindFrame = c.GetPlaceholderFrame().unwrap();
    }

    child
}

impl nsBlockFrame {
    // This function removes aDeletedFrame and all its continuations.  It
    // is optimized for deleting a whole series of frames. The easy
    // implementation would invoke itself recursively on
    // aDeletedFrame->GetNextContinuation, then locate the line containing
    // aDeletedFrame and remove aDeletedFrame from that line. But here we
    // start by locating aDeletedFrame and then scanning from that point
    // on looking for continuations.
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DoRemoveFrame
    pub fn DoRemoveFrame(&self, aContext: &mut FrameDestroyContext, aDeletedFrame: nsIFrame, mut aFlags: u32) {
        let mut aDeletedFrame: Option<nsIFrame> = Some(aDeletedFrame);
        MOZ_ASSERT!(!aDeletedFrame.unwrap().HasAnyStateBits(NS_FRAME_OUT_OF_FLOW | NS_FRAME_IS_OVERFLOW_CONTAINER), "DoRemoveFrame() does not support removing out-of-flow frames or overflow containers!");

        // Find the line that contains deletedFrame. Start from the line cursor
        // (if available) and search to the end of the normal line list, then
        // from the start to the line cursor, and last the overflow lines.
        let mut line_start = self.mLines().begin_mut();
        let mut line_end = self.mLines().end_mut();
        let mut line = line_start;

        // We use the line cursor to attempt to optimize removal, but must ensure
        // it is cleared if lines change such that it may become invalid.
        let mut found = false;
        let cursor = self.GetLineCursorForDisplay();
        if !cursor.is_null() {
            line.SetPosition(cursor);
            while line != line_end {
                if line.Contains(aDeletedFrame.unwrap()) {
                    found = true;
                    break;
                }
                line.operator_inc();
            }
            if !found {
                // Setup for a shorter TryAllLines normal line search to avoid searching
                // the [cursor .. line_end] range again.
                line = line_start;
                line_end.SetPosition(cursor);
            }
        }

        let mut overflowLines: *mut nsBlockFrame_FrameLines = std::ptr::null_mut();
        let mut searchingOverflowList = false;
        if !found {
            // Make sure we look in the overflow lines even if the normal line
            // list is empty.
            self.TryAllLines(&mut line, &mut line_start, &mut line_end, &mut searchingOverflowList, &mut overflowLines);
            while line != line_end {
                if line.Contains(aDeletedFrame.unwrap()) {
                    break;
                }
                line.operator_inc();
                self.TryAllLines(&mut line, &mut line_start, &mut line_end, &mut searchingOverflowList, &mut overflowLines);
            }
            if !searchingOverflowList && (self.GetStateBits() & NS_BLOCK_HAS_LINE_CURSOR) != 0 {
                // Restore line_end since we shortened the search to the cursor.
                line_end = self.mLines().end_mut();
                // Clear our line cursors, since our normal line list may change.
                self.ClearLineCursors();
            }
        }

        if line == line_end {
            NS_ERROR!("can't find deleted frame in lines");
            return;
        }

        if (aFlags & nsBlockFrame::FRAMES_ARE_EMPTY) == 0 {
            if line != line_start {
                let mut prev = line.prev();
                prev.MarkDirty();
                prev.SetInvalidateTextRuns(true);
            } else if searchingOverflowList && !self.mLines().empty() {
                let back = unsafe { &mut *self.mLines().back_mut() };
                back.MarkDirty();
                back.SetInvalidateTextRuns(true);
            }
        }

        while line != line_end && aDeletedFrame.is_some() {
            let mut deleted = aDeletedFrame.unwrap();
            MOZ_ASSERT!(Some(self.0) == deleted.GetParent(), "messed up delete code");
            MOZ_ASSERT!(line.Contains(deleted), "frame not in line");

            if (aFlags & nsBlockFrame::FRAMES_ARE_EMPTY) == 0 {
                line.MarkDirty();
                line.SetInvalidateTextRuns(true);
            }

            // If the frame being deleted is the last one on the line then
            // optimize away the line->Contains(next-in-flow) call below.
            let mut isLastFrameOnLine = 1 == line.GetChildCount();
            if !isLastFrameOnLine {
                let next = line.next_();
                let lastFrame = if next != line_end {
                    next.mFirstChild.unwrap().GetPrevSibling()
                } else if searchingOverflowList {
                    unsafe { &*overflowLines }.mFrames.LastChild()
                } else {
                    self.mFrames().LastChild()
                };
                NS_ASSERTION!(next == line_end || lastFrame == line.LastChild(), "unexpected line frames");
                isLastFrameOnLine = lastFrame == Some(deleted);
            }

            // Remove aDeletedFrame from the line
            if line.mFirstChild == Some(deleted) {
                // We should be setting this to null if aDeletedFrame
                // is the only frame on the line. HOWEVER in that case
                // we will be removing the line anyway, see below.
                line.mFirstChild = deleted.GetNextSibling();
            }

            // Hmm, this won't do anything if we're removing a frame in the first
            // overflow line... Hopefully doesn't matter
            line.operator_dec();
            if line != line_end && !line.IsBlock() {
                // Since we just removed a frame that follows some inline
                // frames, we need to reflow the previous line.
                line.MarkDirty();
            }
            line.operator_inc();

            // Take aDeletedFrame out of the sibling list. Note that
            // prevSibling will only be nullptr when we are deleting the very
            // first frame in the main or overflow list.
            if searchingOverflowList {
                unsafe { &mut *overflowLines }.mFrames.RemoveFrame(deleted);
            } else {
                self.mFrames().RemoveFrame(deleted);
            }

            // Update the child count of the line to be accurate
            line.NoteFrameRemoved(deleted);

            // Destroy frame; capture its next continuation first in case we need
            // to destroy that too.
            let mut deletedNextContinuation: Option<nsIFrame> = if (aFlags & nsBlockFrame::REMOVE_FIXED_CONTINUATIONS) != 0 { deleted.GetNextContinuation() } else { deleted.GetNextInFlow() };
            // #ifdef NOISY_REMOVE_FRAME ... NOT PORTED: debug printing.

            // If next-in-flow is an overflow container, must remove it first.
            // FIXME: Can we do this unconditionally?
            if let Some(dnc) = deletedNextContinuation {
                if dnc.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) {
                    dnc.GetParent().unwrap().DeleteNextInFlowChild(aContext, dnc, false);
                    deletedNextContinuation = None;
                }
            }

            deleted.Destroy(aContext);
            aDeletedFrame = deletedNextContinuation;

            let mut haveAdvancedToNextLine = false;
            // If line is empty, remove it now.
            if 0 == line.GetChildCount() {
                // #ifdef NOISY_REMOVE_FRAME ... NOT PORTED: debug printing.
                let cur: *mut nsLineBox = line.get_mut();
                if !searchingOverflowList {
                    line = self.mLines().erase(line);
                    self.ClearLineCursors();
                    // Invalidate the space taken up by the line.
                    // XXX We need to do this if we're removing a frame as a result of
                    // a call to RemoveFrame(), but we may not need to do this in all
                    // cases...
                    // #ifdef NOISY_BLOCK_INVALIDATE ... NOT PORTED: debug printing.
                } else {
                    let ol = unsafe { &mut *overflowLines };
                    line = ol.mLines.erase(line);
                    if ol.mLines.empty() {
                        self.DestroyOverflowLines();
                        overflowLines = std::ptr::null_mut();
                        // We just invalidated our iterators.  Since we were in
                        // the overflow lines list, which is now empty, set them
                        // so we're at the end of the regular line list.
                        line_start = self.mLines().begin_mut();
                        line_end = self.mLines().end_mut();
                        line = line_end;
                    }
                }
                self.FreeLineBox(cur);

                // If we're removing a line, ReflowDirtyLines isn't going to
                // know that it needs to slide lines unless something is marked
                // dirty.  So mark the previous margin of the next line dirty if
                // there is one.
                if line != line_end {
                    line.MarkPreviousMarginDirty();
                }
                haveAdvancedToNextLine = true;
            } else {
                // Make the line that just lost a frame dirty, and advance to
                // the next line.
                if deletedNextContinuation.is_none() || isLastFrameOnLine || !line.Contains(deletedNextContinuation.unwrap()) {
                    line.MarkDirty();
                    line.operator_inc();
                    haveAdvancedToNextLine = true;
                }
            }

            if let Some(dnc) = deletedNextContinuation {
                // See if we should keep looking in the current flow's line list.
                if dnc.GetParent() != Some(self.0) {
                    // The deceased frames continuation is not a child of the
                    // current block. So break out of the loop so that we advance
                    // to the next parent.
                    //
                    // If we have a continuation in a different block then all bets are
                    // off regarding whether we are deleting frames without actual content,
                    // so don't propagate FRAMES_ARE_EMPTY any further.
                    aFlags &= !nsBlockFrame::FRAMES_ARE_EMPTY;
                    break;
                }

                // If we advanced to the next line then check if we should switch to the
                // overflow line list.
                if haveAdvancedToNextLine {
                    if line != line_end && !searchingOverflowList && !line.Contains(dnc) {
                        // We have advanced to the next *normal* line but the next-in-flow
                        // is not there - force a switch to the overflow line list.
                        line = line_end;
                    }

                    self.TryAllLines(&mut line, &mut line_start, &mut line_end, &mut searchingOverflowList, &mut overflowLines);
                    // #ifdef NOISY_REMOVE_FRAME ... NOT PORTED: debug printing.
                }
            }
        }

        if (aFlags & nsBlockFrame::FRAMES_ARE_EMPTY) == 0 && line.next_() != line_end {
            let mut next = line.next_();
            next.MarkDirty();
            next.SetInvalidateTextRuns(true);
        }

        // #ifdef DEBUG VerifyLines(true); VerifyOverflowSituation();  NOT PORTED: debug-only verification.

        // Advance to next flow block if the frame has more continuations.
        let Some(aDeletedFrame) = aDeletedFrame else {
            return;
        };
        let nextBlock = nsBlockFrame::do_QueryFrame(aDeletedFrame.GetParent().unwrap());
        NS_ASSERTION!(nextBlock.is_some(), "Our child's continuation's parent is not a block?");
        let flags: u32 = aFlags & nsBlockFrame::REMOVE_FIXED_CONTINUATIONS;
        nextBlock.unwrap().DoRemoveFrame(aContext, aDeletedFrame, flags);
    }
}

// Gecko: layout/generic/nsBlockFrame.h nsBlockFrame::{REMOVE_FIXED_CONTINUATIONS, FRAMES_ARE_EMPTY}
impl nsBlockFrame {
    pub const REMOVE_FIXED_CONTINUATIONS: u32 = 0x02;
    pub const FRAMES_ARE_EMPTY: u32 = 0x04;
}

// Gecko: layout/generic/nsBlockFrame.cpp FindBlockLineFor
fn FindBlockLineFor(aChild: nsIFrame, aBegin: nsLineList_iterator, aEnd: nsLineList_iterator, aResult: &mut nsLineList_iterator) -> bool {
    MOZ_ASSERT!(aChild.IsBlockOutside());
    let mut line = aBegin;
    while line != aEnd {
        MOZ_ASSERT!(line.GetChildCount() > 0);
        if line.IsBlock() && line.mFirstChild == Some(aChild) {
            MOZ_ASSERT!(line.GetChildCount() == 1);
            *aResult = line;
            return true;
        }
        line.operator_inc();
    }
    false
}

// Gecko: layout/generic/nsBlockFrame.cpp FindInlineLineFor
fn FindInlineLineFor(aChild: nsIFrame, aFrameList: &nsFrameList, aBegin: nsLineList_iterator, aEnd: nsLineList_iterator, aResult: &mut nsLineList_iterator) -> bool {
    MOZ_ASSERT!(!aChild.IsBlockOutside());
    let mut line = aBegin;
    while line != aEnd {
        MOZ_ASSERT!(line.GetChildCount() > 0);
        if !line.IsBlock() {
            // Optimize by comparing the line's last child first.
            let next = line.next_();
            if Some(aChild) == (if next == aEnd { aFrameList.LastChild() } else { next.mFirstChild.unwrap().GetPrevSibling() }) || line.Contains(aChild) {
                *aResult = line;
                return true;
            }
        }
        line.operator_inc();
    }
    false
}

// Gecko: layout/generic/nsBlockFrame.cpp FindLineFor
fn FindLineFor(aChild: nsIFrame, aFrameList: &nsFrameList, aBegin: nsLineList_iterator, aEnd: nsLineList_iterator, aResult: &mut nsLineList_iterator) -> bool {
    if aChild.IsBlockOutside() { FindBlockLineFor(aChild, aBegin, aEnd, aResult) } else { FindInlineLineFor(aChild, aFrameList, aBegin, aEnd, aResult) }
}

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::StealFrame
    pub fn StealFrame(&self, aChild: nsIFrame) {
        MOZ_ASSERT!(aChild.GetParent() == Some(self.0));

        if aChild.IsFloating() {
            self.RemoveFloat(aChild);
            return;
        }

        if self.MaybeStealOverflowContainerFrame(aChild) {
            return;
        }

        MOZ_ASSERT!(!aChild.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW));

        let mut line = nsLineList_iterator::default();
        if FindLineFor(aChild, &*self.mFrames(), self.mLines().begin_mut(), self.mLines().end_mut(), &mut line) {
            self.RemoveFrameFromLine(aChild, line, &mut *self.mFrames(), self.mLines());
        } else {
            let overflowLines = unsafe { &mut *self.GetOverflowLines() };
            let found = FindLineFor(aChild, &overflowLines.mFrames, overflowLines.mLines.begin_mut(), overflowLines.mLines.end_mut(), &mut line);
            MOZ_ASSERT!(found, "Why can't we find aChild in our overflow lines?");
            self.RemoveFrameFromLine(aChild, line, &mut overflowLines.mFrames, &mut overflowLines.mLines);
            if overflowLines.mLines.empty() {
                self.DestroyOverflowLines();
            }
        }
    }

    // Helper method for StealFrame.
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::RemoveFrameFromLine
    pub fn RemoveFrameFromLine(&self, aChild: nsIFrame, mut aLine: nsLineList_iterator, aFrameList: &mut nsFrameList, aLineList: &mut nsLineList) {
        aFrameList.RemoveFrame(aChild);
        if Some(aChild) == aLine.mFirstChild {
            aLine.mFirstChild = aChild.GetNextSibling();
        }
        aLine.NoteFrameRemoved(aChild);
        if aLine.GetChildCount() > 0 {
            aLine.MarkDirty();
        } else {
            // The line became empty - destroy it.
            let lineBox: *mut nsLineBox = aLine.get_mut();
            aLine = aLineList.erase(aLine);
            if aLine != aLineList.end_mut() {
                aLine.MarkPreviousMarginDirty();
            }
            self.FreeLineBox(lineBox);
            self.ClearLineCursors();
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DeleteNextInFlowChild
    pub fn DeleteNextInFlowChild(&self, aContext: &mut FrameDestroyContext, aNextInFlow: nsIFrame, aDeletingEmptyFrames: bool) {
        MOZ_ASSERT!(aNextInFlow.GetPrevInFlow().is_some(), "bad next-in-flow");

        if aNextInFlow.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW | NS_FRAME_IS_OVERFLOW_CONTAINER) {
            self.nsContainerFrame_DeleteNextInFlowChild(aContext, aNextInFlow, aDeletingEmptyFrames);
        } else {
            // #ifdef DEBUG if (aDeletingEmptyFrames) { nsLayoutUtils::AssertTreeOnlyEmptyNextInFlows(aNextInFlow); }
            // NOT PORTED: debug-only check.
            self.DoRemoveFrame(aContext, aNextInFlow, if aDeletingEmptyFrames { nsBlockFrame::FRAMES_ARE_EMPTY } else { 0 });
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::StyleTextForLineLayout
    pub fn StyleTextForLineLayout(&self) -> nsStyleTextAdapter {
        // Return the pointer to an unmodified style text
        nsStyleTextAdapter::of(self.0)
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ReflowFloat
    // @param aReflowStatus an incomplete status indicates the float should be
    //        split but only if the available block-size is constrained.
    pub fn ReflowFloat(&self, aState: &mut BlockReflowState, aFloatRI: &mut ReflowInput, aFloat: nsIFrame, aReflowStatus: &mut nsReflowStatus) {
        MOZ_ASSERT!(aReflowStatus.IsEmpty(), "Caller should pass a fresh reflow status!");
        MOZ_ASSERT!(aFloat.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW), "aFloat must be an out-of-flow frame");

        let wm = aState.mReflowInput.GetWritingMode();

        // Setup a block reflow context to reflow the float.
        let stateReflowInput: &ReflowInput = aState.mReflowInput;
        let mut brc = nsBlockReflowContext::new(aState.mPresContext, stateReflowInput);

        let mut clearanceFrame: Option<nsIFrame> = None;
        loop {
            let mut margin = CollapsingMargin::default();
            let mut mayNeedRetry = false;
            aFloatRI.mDiscoveredClearance = std::ptr::null_mut();
            // Only first in flow gets a block-start margin.
            if aFloat.GetPrevInFlow().is_none() {
                brc.ComputeCollapsedBStartMargin(aFloatRI, &mut margin, clearanceFrame, &mut mayNeedRetry, None);

                if mayNeedRetry && clearanceFrame.is_none() {
                    aFloatRI.mDiscoveredClearance = &mut clearanceFrame as *mut Option<nsIFrame>;
                    // We don't need to push the float manager state because the the block
                    // has its own float manager that will be destroyed and recreated
                }
            }

            // When reflowing a float, aSpace argument doesn't matter because we pass
            // nullptr to aLine and we don't call nsBlockReflowContext::PlaceBlock()
            // later.
            brc.ReflowBlock(LogicalRect::new__WritingMode(wm), true, &mut margin, 0, None, aFloatRI, aReflowStatus, aState);
            if !clearanceFrame.is_some() {
                break;
            }
        }

        if aFloat.IsLetterFrame() {
            // We never split floating first letters; an incomplete status for such
            // frames simply means that there is more content to be reflowed on the
            // line.
            if aReflowStatus.IsIncomplete() {
                aReflowStatus.Reset();
            }
        }

        NS_ASSERTION!(aReflowStatus.IsFullyComplete() || aFloatRI.AvailableBSize() != NS_UNCONSTRAINEDSIZE, "The status can only be incomplete or overflow-incomplete if the available block-size is constrained!");

        if aReflowStatus.NextInFlowNeedsReflow() {
            aState.mReflowStatus.SetNextInFlowNeedsReflow();
        }

        let metrics: &ReflowOutput = brc.GetMetrics();

        // Set the rect, make sure the view is properly sized and positioned,
        // and tell the frame we're done reflowing it
        // XXXldb This seems like the wrong place to be doing this -- shouldn't
        // we be doing this in BlockReflowState::FlowAndPlaceFloat after
        // we've positioned the float, and shouldn't we be doing the equivalent
        // of |PlaceFrameView| here?
        let metricsWM = metrics.GetWritingMode();
        aFloat.SetSize__WritingMode_LogicalSize(metricsWM, metrics.Size(metricsWM));
        aFloat.DidReflow(&aState.mPresContext, Some(aFloatRI));
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::FindTrailingClear
    pub fn FindTrailingClear(&self) -> UsedClear {
        let mut b = Some(*self);
        while let Some(blk) = b {
            let endLine = blk.LinesRBegin();
            if endLine != blk.LinesREnd() {
                return endLine.FloatClearTypeAfter();
            }
            b = blk.GetPrevInFlow().map(nsBlockFrame::from_frame);
        }
        UsedClear::None
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ReflowPushedFloats
    pub fn ReflowPushedFloats(&self, aState: &mut BlockReflowState, aOverflowAreas: &mut OverflowAreas) {
        // Pushed floats live at the start of our float list; see comment
        // above nsBlockFrame::DrainPushedFloats.
        let mut floats = self.GetFloats();
        let mut f: Option<nsIFrame> = if floats.is_null() { None } else { unsafe { &*floats }.FirstChild() };
        let mut prev: Option<nsIFrame> = None;
        while f.is_some() && f.unwrap().HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW) {
            let fr = f.unwrap();
            MOZ_ASSERT!(prev == fr.GetPrevSibling());
            // When we push a first-continuation float in a non-initial reflow,
            // it's possible that we end up with two continuations with the same
            // parent.  This happens if, on the previous reflow of the block or
            // a previous reflow of the line containing the block, the float was
            // split between continuations A and B of the parent, but on the
            // current reflow, none of the float can fit in A.
            //
            // When this happens, we might even have the two continuations
            // out-of-order due to the management of the pushed floats.  In
            // particular, if the float's placeholder was in a pushed line that
            // we reflowed before it was pushed, and we split the float during
            // that reflow, we might have the continuation of the float before
            // the float itself.  (In the general case, however, it's correct
            // for floats in the pushed floats list to come before floats
            // anchored in pushed lines; however, in this case it's wrong.  We
            // should probably find a way to fix it somehow, since it leads to
            // incorrect layout in some cases.)
            //
            // When we have these out-of-order continuations, we might hit the
            // next-continuation before the previous-continuation.  When that
            // happens, just push it.  When we reflow the next continuation,
            // we'll either pull all of its content back and destroy it (by
            // calling DeleteNextInFlowChild), or nsBlockFrame::SplitFloat will
            // pull it out of its current position and push it again (and
            // potentially repeat this cycle for the next continuation, although
            // hopefully then they'll be in the right order).
            //
            // We should also need this code for the in-order case if the first
            // continuation of a float gets moved across more than one
            // continuation of the containing block.  In this case we'd manage
            // to push the second continuation without this check, but not the
            // third and later.
            let prevContinuation = fr.GetPrevContinuation();
            if prevContinuation.map_or(false, |pc| pc.GetParent() == fr.GetParent()) {
                unsafe { &mut *floats }.RemoveFrame(fr);
                if unsafe { &*floats }.IsEmpty() {
                    unsafe { &*self.StealFloats() }.Delete();
                    floats = std::ptr::null_mut();
                }
                aState.AppendPushedFloatChain(fr);
                if floats.is_null() {
                    // The floats list becomes empty after removing |f|. Bail out.
                    f = None;
                    prev = None;
                    break;
                }
                // Even if we think |floats| is valid, AppendPushedFloatChain() can also
                // push |f|'s next-in-flows in our floats list to our pushed floats list.
                // If all the floats in the floats list are pushed, the floats list will
                // be deleted, and |floats| will be stale and poisoned. Therefore, we need
                // to get the floats list again to check its validity.
                floats = self.GetFloats();
                if floats.is_null() {
                    f = None;
                    prev = None;
                    break;
                }
                f = if prev.is_none() { unsafe { &*floats }.FirstChild() } else { prev.unwrap().GetNextSibling() };
                continue;
            }

            // Always call FlowAndPlaceFloat; we might need to place this float if it
            // didn't belong to this block the last time it was reflowed.  Note that if
            // the float doesn't get placed, we don't consider its overflow areas.
            // (Not-getting-placed means it didn't fit and we pushed it instead of
            // placing it, and its position could be stale.)
            if aState.FlowAndPlaceFloat(fr, None) == BlockReflowState_PlaceFloatResult::Placed {
                self.ConsiderChildOverflow__OverflowAreas_nsIFrame(aOverflowAreas, fr);
            }

            // If f is the only child in the floats list, pushing it to the pushed
            // floats list in FlowAndPlaceFloat() can result in the floats list being
            // deleted. Get the floats list again.
            floats = self.GetFloats();
            if floats.is_null() {
                f = None;
                prev = None;
                break;
            }

            let mut next = if prev.is_none() { unsafe { &*floats }.FirstChild() } else { prev.unwrap().GetNextSibling() };
            if next == Some(fr) {
                // We didn't push |f| so its next-sibling is next.
                next = fr.GetNextSibling();
                prev = Some(fr);
            } // else: we did push |f| so |prev|'s new next-sibling is next.
            f = next;
        }
        let _ = (f, prev);

        // If there are pushed or split floats, then we may need to continue BR
        // clearance
        let (bCoord, result) = aState.ClearFloats(0, UsedClear::Both, None);
        if result != ClearFloatsResult::BCoordNoChange {
            let _ = bCoord;
            if let Some(prevBlock) = self.GetPrevInFlow().map(nsBlockFrame::from_frame) {
                aState.mTrailingClearFromPIF = prevBlock.FindTrailingClear();
            }
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::RecoverFloats
    pub fn RecoverFloats(&self, aFloatManager: &mut nsFloatManager, aWM: WritingMode, aContainerSize: nsSize) {
        // Recover our own floats
        let mut stop: Option<nsIFrame> = None; // Stop before we reach pushed floats that
                                               // belong to our next-in-flow
        let floats = self.GetFloats();
        let mut f: Option<nsIFrame> = if floats.is_null() { None } else { unsafe { &*floats }.FirstChild() };
        while let Some(fr) = f {
            if Some(fr) == stop {
                break;
            }
            let region: LogicalRect = nsFloatManager::GetRegionFor(aWM, fr, aContainerSize);
            aFloatManager.AddFloat(fr, region, aWM, aContainerSize);
            if stop.is_none() && fr.GetNextInFlow().is_some() {
                stop = fr.GetNextInFlow();
            }
            f = fr.GetNextSibling();
        }

        // Recurse into our overflow container children
        let mut oc = self.GetChildList(FrameChildListID::OverflowContainers).FirstChild();
        while let Some(o) = oc {
            nsBlockFrame::RecoverFloatsFor(o, aFloatManager, aWM, aContainerSize);
            oc = o.GetNextSibling();
        }

        // Recurse into our normal children
        for line in self.Lines() {
            if line.IsBlock() {
                nsBlockFrame::RecoverFloatsFor(line.mFirstChild.unwrap(), aFloatManager, aWM, aContainerSize);
            }
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::RecoverFloatsFor
    pub fn RecoverFloatsFor(aFrame: nsIFrame, aFloatManager: &mut nsFloatManager, aWM: WritingMode, aContainerSize: nsSize) {
        // Only blocks have floats
        let block = nsBlockFrame::do_QueryFrame(aFrame);
        // Don't recover any state inside a block that has its own float manager
        // (we don't currently have any blocks like this, though, thanks to our
        // use of extra frames for 'overflow')
        if let Some(block) = block {
            if !nsBlockFrame::BlockNeedsFloatManager(block.0) {
                // If the element is relatively positioned, then adjust x and y
                // accordingly so that we consider relatively positioned frames
                // at their original position.

                let rect: LogicalRect = block.GetLogicalNormalRect(aWM, aContainerSize);
                let lineLeft: nscoord = rect.LineLeft(aWM, aContainerSize);
                let blockStart: nscoord = rect.BStart(aWM);
                aFloatManager.Translate(lineLeft, blockStart);
                block.RecoverFloats(aFloatManager, aWM, aContainerSize);
                aFloatManager.Translate(-lineLeft, -blockStart);
            }
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::HasPushedFloatsFromPrevContinuation
    pub fn HasPushedFloatsFromPrevContinuation(&self) -> bool {
        let floats = self.GetFloats();
        if !floats.is_null() {
            let floats = unsafe { &*floats };
            // If we have pushed floats, then they should be at the beginning of our
            // float list.
            if floats.FirstChild().unwrap().HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW) {
                return true;
            }
            #[cfg(debug_assertions)]
            {
                // Double-check the above assertion that pushed floats should be at the
                // beginning of our floats list.
                for f in floats {
                    NS_ASSERTION!(!f.HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW), "pushed floats must be at the beginning of the float list");
                }
            }
        }

        // We may have a pending push of pushed floats, too.
        self.HasPushedFloats()
    }
}

//////////////////////////////////////////////////////////////////////
// Painting, event handling

// Gecko: layout/generic/nsBlockFrame.cpp ComputeInkOverflowArea, DebugOutputDrawLine, DisplayLine,
// nsBlockFrame::BuildDisplayList
// NOT PORTED: painting and display lists (nsBlockFrame.cpp lines 7830-8340 at GECKO_REV).

impl nsBlockFrame {
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::AccessibleType
    // NOT PORTED: accessibility.

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::SetupLineCursorForDisplay
    pub fn SetupLineCursorForDisplay(&self) {
        if self.mLines().empty() || self.HasProperty(LineCursorPropertyDisplay()) {
            return;
        }

        self.SetProperty(LineCursorPropertyDisplay(), self.mLines().front_mut());
        self.AddStateBits(NS_BLOCK_HAS_LINE_CURSOR);
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::SetupLineCursorForQuery
    pub fn SetupLineCursorForQuery(&self) {
        if self.mLines().empty() || self.HasProperty(LineCursorPropertyQuery()) {
            return;
        }

        self.SetProperty(LineCursorPropertyQuery(), self.mLines().front_mut());
        self.AddStateBits(NS_BLOCK_HAS_LINE_CURSOR);
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetFirstLineContaining
    pub fn GetFirstLineContaining(&self, y: nscoord) -> *mut nsLineBox {
        // Although this looks like a "querying" method, it is used by the
        // display-list building code, so uses the Display cursor.
        let property = self.GetLineCursorForDisplay();
        if property.is_null() {
            return std::ptr::null_mut();
        }
        let mut cursor = self.mLines().begin_mut__nsLineBox(property);
        let mut cursorArea: nsRect = cursor.InkOverflowRect();

        while (cursorArea.IsEmpty() || cursorArea.YMost() > y) && cursor.get() != self.mLines().front() {
            cursor = cursor.prev();
            cursorArea = cursor.InkOverflowRect();
        }
        while (cursorArea.IsEmpty() || cursorArea.YMost() <= y) && cursor.get() != self.mLines().back() {
            cursor = cursor.next_();
            cursorArea = cursor.InkOverflowRect();
        }

        if cursor.get() as *mut nsLineBox != property {
            self.SetProperty(LineCursorPropertyDisplay(), cursor.get() as *mut nsLineBox);
        }

        cursor.get() as *mut nsLineBox
    }

    /* virtual */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ChildIsDirty
    pub fn ChildIsDirty(&self, aChild: nsIFrame) {
        // See if the child is absolutely positioned
        if aChild.IsAbsolutelyPositioned(None) {
            // do nothing
        } else if Some(aChild) == self.GetOutsideMarker() {
            // The ::marker lives in the first line, unless the first line has
            // height 0 and there is a second line, in which case it lives
            // in the second line.
            let mut markerLine = self.LinesBegin();
            if markerLine != self.LinesEnd() && markerLine.BSize() == 0 && markerLine.get() != self.mLines().back() {
                markerLine = markerLine.next_();
            }

            if markerLine != self.LinesEnd() {
                self.MarkLineDirty(markerLine, self.mLines());
            }
            // otherwise we have an empty line list, and ReflowDirtyLines
            // will handle reflowing the ::marker.
        } else {
            // Note that we should go through our children to mark lines dirty
            // before the next reflow.  Doing it now could make things O(N^2)
            // since finding the right line is O(N).
            // We don't need to worry about marking lines on the overflow list
            // as dirty; we're guaranteed to reflow them if we take them off the
            // overflow list.
            // However, we might have gotten a float, in which case we need to
            // reflow the line containing its placeholder.  So find the
            // ancestor-or-self of the placeholder that's a child of the block,
            // and mark it as NS_FRAME_HAS_DIRTY_CHILDREN too, so that we mark
            // its line dirty when we handle NS_BLOCK_LOOK_FOR_DIRTY_FRAMES.
            // We need to take some care to handle the case where a float is in
            // a different continuation than its placeholder, including marking
            // an extra block with NS_BLOCK_LOOK_FOR_DIRTY_FRAMES.
            if !aChild.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) {
                self.AddStateBits(NS_BLOCK_LOOK_FOR_DIRTY_FRAMES);
            } else {
                NS_ASSERTION!(aChild.IsFloating(), "should be a float");
                let thisFC = self.FirstContinuation();
                let placeholderPath = aChild.GetPlaceholderFrame();
                // SVG code sometimes sends FrameNeedsReflow notifications during
                // frame destruction, leading to null placeholders, but we're safe
                // ignoring those.
                if let Some(mut placeholderPath) = placeholderPath {
                    loop {
                        let parent = placeholderPath.GetParent().unwrap();
                        if parent.GetContent_ptr_eq(self.GetContent()) && parent.FirstContinuation() == thisFC {
                            parent.AddStateBits(NS_BLOCK_LOOK_FOR_DIRTY_FRAMES);
                            break;
                        }
                        placeholderPath = parent;
                    }
                    placeholderPath.AddStateBits(NS_FRAME_HAS_DIRTY_CHILDREN);
                }
            }
        }

        self.nsContainerFrame_ChildIsDirty(aChild);
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::Init
    // ADAPTER: the frame is created by FrameTree::AddFrame (it has its content, parent and style);
    // Init() does the part that is the block's own.
    pub fn Init(&self, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: Option<nsIFrame>) {
        // These are all the block specific frame bits, they are copied from
        // the prev-in-flow to a newly created next-in-flow, except for the
        // NS_BLOCK_FLAGS_NON_INHERITED_MASK bits below.
        const NS_BLOCK_FLAGS_MASK: nsFrameState = NS_BLOCK_BFC | NS_BLOCK_HAS_FIRST_LETTER_STYLE | NS_BLOCK_HAS_FIRST_LETTER_CHILD | NS_BLOCK_HAS_MARKER | NS_BLOCK_HAS_INLINE_ABSPOS_DESCENDANT;

        // This is the subset of NS_BLOCK_FLAGS_MASK that is NOT inherited
        // by default.  They should only be set on the first-in-flow.
        const NS_BLOCK_FLAGS_NON_INHERITED_MASK: nsFrameState = NS_BLOCK_HAS_FIRST_LETTER_CHILD | NS_BLOCK_HAS_MARKER;

        if let Some(prev) = aPrevInFlow {
            // Copy over the inherited block frame bits from the prev-in-flow.
            self.RemoveStateBits(NS_BLOCK_FLAGS_MASK);
            self.AddStateBits(prev.GetStateBits() & (NS_BLOCK_FLAGS_MASK & !NS_BLOCK_FLAGS_NON_INHERITED_MASK));
        }

        self.nsContainerFrame_Init(aContent, aParent, aPrevInFlow);

        if aPrevInFlow.is_none() || aPrevInFlow.unwrap().HasAnyStateBits(NS_BLOCK_NEEDS_BIDI_RESOLUTION) {
            self.AddStateBits(NS_BLOCK_NEEDS_BIDI_RESOLUTION);
        }

        if EstablishesBFC(*self) {
            self.AddStateBits(NS_BLOCK_BFC);
        }

        if self.HasAnyStateBits(NS_FRAME_FONT_INFLATION_CONTAINER) && self.HasAnyStateBits(NS_BLOCK_BFC) {
            self.AddStateBits(NS_FRAME_FONT_INFLATION_FLOW_ROOT);
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::SetInitialChildList
    pub fn SetInitialChildList(&self, aListID: FrameChildListID, aChildList: nsFrameList) {
        if FrameChildListID::Float == aListID {
            let floats = self.EnsureFloats();
            *floats = aChildList;
        } else if FrameChildListID::Principal == aListID {
            // #ifdef DEBUG: the first-letter style bit check
            // NOT PORTED: ::first-letter (pseudo types).

            self.AddFrames(aChildList, None, None);
        } else {
            self.nsContainerFrame_SetInitialChildList(aListID, aChildList);
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::SetMarkerFrameForListItem
    pub fn SetMarkerFrameForListItem(&self, aMarkerFrame: nsIFrame) {
        MOZ_ASSERT!(!self.HasMarker(), "How can we have a ::marker frame already?");

        if self.StyleList().mListStylePosition() == StyleListStylePosition::Inside {
            self.SetProperty(InsideMarkerProperty(), aMarkerFrame);
        } else {
            self.SetProperty(OutsideMarkerProperty(), Box::into_raw(Box::new(nsFrameList::new(Some(aMarkerFrame), Some(aMarkerFrame)))));
        }
        self.AddStateBits(NS_BLOCK_HAS_MARKER);
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::MarkerIsEmpty
    pub fn MarkerIsEmpty(&self, aMarker: nsIFrame) -> bool {
        MOZ_ASSERT!(self.StyleDisplay().IsListItem() && Some(aMarker) == self.GetOutsideMarker(), "should only care about an outside ::marker");
        // ADAPTER: the style-based body of Gecko lives in nsIFrame::MarkerIsEmpty (nsLineLayoutAdapter.rs),
        // which nsLineLayout calls on the line container frame.
        nsIFrame::MarkerIsEmpty(&self.0, aMarker)
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::HasOutsideMarker
    pub fn HasOutsideMarker(&self) -> bool {
        self.HasMarker() && self.HasProperty(OutsideMarkerProperty())
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ReflowOutsideMarker
    pub fn ReflowOutsideMarker(&self, aMarkerFrame: nsIFrame, aState: &mut BlockReflowState, aMetrics: &mut ReflowOutput, aLineTop: nscoord) {
        let ri: &ReflowInput = aState.mReflowInput;

        let markerWM = aMarkerFrame.GetWritingMode();
        let mut availSize = LogicalSize::new__WritingMode(markerWM);
        // Make up an inline-size since it doesn't really matter (XXX).
        *availSize.ISize_mut(markerWM) = aState.ContentISize();
        *availSize.BSize_mut(markerWM) = NS_UNCONSTRAINEDSIZE;

        let reflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            &aState.mPresContext,
            ri,
            aMarkerFrame,
            availSize,
            None,
            EnumSet::default(),
            StyleSizeOverrides::default(),
            { let mut f = ComputeSizeFlags::default(); f += ComputeSizeFlag::ShrinkWrap; f },
            std::ptr::null_mut(),
        );
        let mut status = nsReflowStatus::new();
        aMarkerFrame.Reflow(&aState.mPresContext, aMetrics, &reflowInput, &mut status);

        // Get the float available space using our saved state from before we
        // started reflowing the block, so that we ignore any floats inside
        // the block.
        // FIXME: aLineTop isn't actually set correctly by some callers, since
        // they reposition the line.
        let floatAvailSpace: LogicalRect = aState.GetFloatAvailableSpaceWithState(ri.GetWritingMode(), aLineTop, ShapeType::ShapeOutside, Some(&aState.mFloatManagerStateBefore)).mRect;
        // FIXME (bug 25888): need to check the entire region that the first
        // line overlaps, not just the top pixel.

        // Place the ::marker now.  We want to place the ::marker relative to the
        // border-box of the associated block (using the right/left margin of
        // the ::marker frame as separation).  However, if a line box would be
        // displaced by floats that are *outside* the associated block, we
        // want to displace it by the same amount.  That is, we act as though
        // the edge of the floats is the content-edge of the block, and place
        // the ::marker at a position offset from there by the block's padding,
        // the block's border, and the ::marker frame's margin.

        // IStart from floatAvailSpace gives us the content/float start edge
        // in the current writing mode. Then we subtract out the start
        // border/padding and the ::marker's width and margin to offset the position.
        let wm = ri.GetWritingMode();
        // Get the ::marker's margin, converted to our writing mode so that we can
        // combine it with other logical values here.
        let markerMargin: LogicalMargin = reflowInput.ComputedLogicalMargin(wm);
        let iStart: nscoord = floatAvailSpace.IStart(wm) - ri.ComputedLogicalBorderPadding(wm).IStart(wm) - markerMargin.IEnd(wm) - aMetrics.ISize(wm);

        // Approximate the ::marker's position; vertical alignment will provide
        // the final vertical location. We pass our writing-mode here, because
        // it may be different from the ::marker frame's mode.
        let bStart: nscoord = floatAvailSpace.BStart(wm);
        aMarkerFrame.SetRect__WritingMode_LogicalRect_nsSize(wm, LogicalRect::new(wm, iStart, bStart, aMetrics.ISize(wm), aMetrics.BSize(wm)), aState.ContainerSize());
        aMarkerFrame.DidReflow(&aState.mPresContext, Some(aState.mReflowInput));
    }

    // This is used to scan frames for any float placeholders, add their
    // floats to the list represented by aList, and remove the
    // floats from whatever list they might be in. We don't search descendants
    // that are float containing blocks.  Floats that or not children of 'this'
    // are ignored (they are not added to aList).
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::DoCollectFloats
    pub fn DoCollectFloats(&self, aFrame: nsIFrame, aList: &mut nsFrameList, aCollectSiblings: bool) {
        let mut aFrame = Some(aFrame);
        while let Some(fr) = aFrame {
            // Don't descend into float containing blocks.
            if !fr.IsFloatContainingBlock() {
                let mut outOfFlowFrame: Option<nsIFrame> = if fr.IsPlaceholderFrame() { nsLayoutUtils::GetFloatFromPlaceholder(fr) } else { None };
                while let Some(oof) = outOfFlowFrame {
                    if oof.GetParent() != Some(self.0) {
                        break;
                    }
                    self.RemoveFloat(oof);
                    // Remove the NS_FRAME_IS_PUSHED_OUT_OF_FLOW bit, in case
                    // |outOfFlowFrame| came from the PushedFloats list.
                    oof.RemoveStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW);
                    aList.AppendFrame(None, oof);
                    outOfFlowFrame = oof.GetNextInFlow();
                    // FIXME: By not pulling floats whose parent is one of our
                    // later siblings, are we risking the pushed floats getting
                    // out-of-order?
                    // XXXmats nsInlineFrame's lazy reparenting depends on NOT doing that.
                }

                if let Some(first) = fr.PrincipalChildList_FirstChild() {
                    self.DoCollectFloats(first, aList, true);
                }
                if let Some(first) = fr.GetChildList(FrameChildListID::Overflow).FirstChild() {
                    self.DoCollectFloats(first, aList, true);
                }
            }
            if !aCollectSiblings {
                break;
            }
            aFrame = fr.GetNextSibling();
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::CheckFloats
    pub fn CheckFloats(&self, aState: &mut BlockReflowState) {
        #[cfg(debug_assertions)]
        {
            // If any line is still dirty, that must mean we're going to reflow this
            // block again soon (e.g. because we bailed out after noticing that
            // clearance was imposed), so don't worry if the floats are out of sync.
            let mut anyLineDirty = false;

            // Check that the float list is what we would have built
            let mut lineFloats: Vec<nsIFrame> = Vec::new();
            for line in self.Lines() {
                if line.HasFloats() {
                    lineFloats.extend_from_slice(line.Floats());
                }
                if line.IsDirty() {
                    anyLineDirty = true;
                }
            }

            let mut storedFloats: Vec<nsIFrame> = Vec::new();
            let mut equal = true;
            let mut hasHiddenFloats = false;
            let mut i: usize = 0;
            for f in self.GetChildList(FrameChildListID::Float) {
                if f.HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW) {
                    continue;
                }
                // There are chances that the float children won't be added to lines,
                // because in nsBlockFrame::ReflowLine, it skips reflow line if the first
                // child of the line is IsHiddenByContentVisibilityOfInFlowParentForLayout.
                // There are also chances that the floats in line are out of date, for
                // instance, lines could reflow if
                // PresShell::IsForcingLayoutForHiddenContent, and after forcingLayout is
                // off, the reflow of lines could be skipped, but the floats are still in
                // there. Here we can't know whether the floats hidden by c-v are included
                // in the lines or not. So we use hasHiddenFloats to skip the float length
                // checking.
                if !hasHiddenFloats && f.IsHiddenByContentVisibilityOfInFlowParentForLayout() {
                    hasHiddenFloats = true;
                }
                storedFloats.push(f);
                if i < lineFloats.len() && lineFloats[i] != f {
                    equal = false;
                }
                i += 1;
            }

            if (!equal || lineFloats.len() != storedFloats.len()) && !anyLineDirty && !hasHiddenFloats {
                NS_ERROR!("nsBlockFrame::CheckFloats: Explicit float list is out of sync with float cache");
            }
        }

        let oofs = self.GetOverflowOutOfFlows();
        if !oofs.is_null() && unsafe { &*oofs }.NotEmpty() {
            // Floats that were pushed should be removed from our float
            // manager.  Otherwise the float manager's YMost or XMost might
            // be larger than necessary, causing this block to get an
            // incorrect desired height (or width).  Some of these floats
            // may not actually have been added to the float manager because
            // they weren't reflowed before being pushed; that's OK,
            // RemoveRegions will ignore them. It is safe to do this here
            // because we know from here on the float manager will only be
            // used for its XMost and YMost, not to place new floats and
            // lines.
            aState.FloatManager().RemoveTrailingRegions(unsafe { &*oofs }.FirstChild());
        }
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::IsMarginRoot
    pub fn IsMarginRoot(&self, aBStartMarginRoot: &mut bool, aBEndMarginRoot: &mut bool) {
        let parent = self.GetParent();
        if !self.HasAnyStateBits(NS_BLOCK_BFC) {
            if parent.is_none() || parent.unwrap().IsFloatContainingBlock() {
                *aBStartMarginRoot = false;
                *aBEndMarginRoot = false;
                return;
            }
        }

        if parent.map_or(false, |p| p.IsColumnSetFrame()) {
            // The first column is a start margin root and the last column is an end
            // margin root.  (If the column-set is split by a column-span:all box then
            // the first and last column in each column-set fragment are margin roots.)
            *aBStartMarginRoot = self.GetPrevInFlow().is_none();
            *aBEndMarginRoot = self.GetNextInFlow().is_none();
            return;
        }

        *aBStartMarginRoot = true;
        *aBEndMarginRoot = true;
    }

    /* static */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::BlockNeedsFloatManager
    pub fn BlockNeedsFloatManager(aBlock: nsIFrame) -> bool {
        NS_ASSERTION!(aBlock.IsBlockFrameOrSubclass(), "aBlock must be a block");

        let parent = aBlock.GetParent();
        aBlock.HasAnyStateBits(NS_BLOCK_BFC) || parent.map_or(false, |p| !p.IsFloatContainingBlock())
    }

    /* static */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::BlockCanIntersectFloats
    pub fn BlockCanIntersectFloats(aFrame: nsIFrame) -> bool {
        // NS_BLOCK_BFC is block specific bit, check first as an optimization, it's
        // okay because we also check that it is a block frame.
        !aFrame.HasAnyStateBits(NS_BLOCK_BFC) && !aFrame.IsReplaced() && aFrame.IsBlockFrameOrSubclass()
    }

    // Note that this width can vary based on the vertical position.
    // However, the cases where it varies are the cases where the width fits
    // in the available space given, which means that variation shouldn't
    // matter.
    /* static */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ISizeToClearPastFloats
    pub fn ISizeToClearPastFloats(aState: &BlockReflowState, aFloatAvailableSpace: LogicalRect, aFloatAvoidingBlock: nsIFrame) -> nsBlockFrame_FloatAvoidingISizeToClear {
        let (mut inlineStartOffset, mut inlineEndOffset): (nscoord, nscoord) = (0, 0);
        let wm = aState.mReflowInput.GetWritingMode();

        let mut result = nsBlockFrame_FloatAvoidingISizeToClear { marginIStart: 0, borderBoxISize: 0 };
        aState.ComputeFloatAvoidingOffsets(aFloatAvoidingBlock, &aFloatAvailableSpace, &mut inlineStartOffset, &mut inlineEndOffset);
        let availISize: nscoord = aState.mContentArea.ISize(wm) - inlineStartOffset - inlineEndOffset;

        // We actually don't want the min width here; see bug 427782; we only
        // want to displace if the width won't compute to a value small enough
        // to fit.
        // All we really need here is the result of ComputeSize, and we
        // could *almost* get that from an SizeComputationInput, except for the
        // last argument.
        let frWM = aFloatAvoidingBlock.GetWritingMode();
        let availSpace: LogicalSize = LogicalSize::new(wm, availISize, NS_UNCONSTRAINEDSIZE).ConvertTo(frWM, wm);
        let reflowInput = NewChildReflowInput(aState.mPresContext, aState.mReflowInput, aFloatAvoidingBlock, availSpace, None);
        result.borderBoxISize = reflowInput.ComputedSizeWithBorderPadding(wm).ISize(wm);

        // Use the margins from sizingInput rather than reflowInput so that
        // they aren't reduced by ignoring margins in overconstrained cases.
        let sizingInput = SizeComputationInput::new__nsIFrame_gfxContext_WritingMode_nscoord_Maybe_Maybe(aFloatAvoidingBlock, aState.mReflowInput.mRenderingContext, wm, aState.mContentArea.ISize(wm), None, None);
        let computedMargin: LogicalMargin = sizingInput.ComputedLogicalMargin(wm);

        let marginISize: nscoord = computedMargin.IStartEnd(wm);
        let iSize = reflowInput.mStylePosition.ISize(wm, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(reflowInput.mFrame, std::ptr::null_mut()));
        if marginISize < 0 && (iSize.IsAuto() || iSize.BehavesLikeStretchOnInlineAxis()) {
            // If we get here, floatAvoidingBlock has a negative amount of inline-axis
            // margin and an 'auto' (or ~equivalently, -moz-available) inline
            // size. Under these circumstances, we use the margin to establish a
            // (positive) minimum size for the border-box, in order to satisfy the
            // equation in CSS2 10.3.3.  That equation essentially simplifies to the
            // following:
            //
            //   iSize of margins + iSize of borderBox = iSize of containingBlock
            //
            // ...where "iSize of borderBox" is the sum of floatAvoidingBlock's
            // inline-axis components of border, padding, and {width,height}.
            //
            // Right now, in the above equation, "iSize of margins" is the only term
            // that we know for sure. (And we also know that it's negative, since we
            // got here.) The other terms are as-yet unresolved, since the frame has an
            // 'auto' iSize, and since we aren't yet sure if we'll clear this frame
            // beyond floats or place it alongside them.
            //
            // However: we *do* know that the equation's "iSize of containingBlock"
            // term *must* be non-negative, since boxes' widths and heights generally
            // can't be negative in CSS.  To satisfy that requirement, we can then
            // infer that the equation's "iSize of borderBox" term *must* be large
            // enough to cancel out the (known-to-be-negative) "iSize of margins"
            // term. Therefore, marginISize value (negated to make it positive)
            // establishes a lower-bound for how much inline-axis space our border-box
            // will really require in order to fit alongside any floats.
            //
            // XXXdholbert This explanation is admittedly a bit hand-wavy and may not
            // precisely match what any particular spec requires. It's the best
            // reasoning I could come up with to explain engines' behavior.  Also, our
            // behavior with -moz-available doesn't seem particularly correct here, per
            // bug 1767217, though that's probably due to a bug elsewhere in our float
            // handling code...
            result.borderBoxISize = std::cmp::max(result.borderBoxISize, -marginISize);
        }

        result.marginIStart = computedMargin.IStart(wm);
        result
    }

    /* static */
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetNearestAncestorBlock
    pub fn GetNearestAncestorBlock(aCandidate: nsIFrame) -> Option<nsBlockFrame> {
        let mut aCandidate = Some(aCandidate);
        while let Some(c) = aCandidate {
            if let Some(block) = nsBlockFrame::do_QueryFrame(c) {
                // yay, candidate is a block!
                return Some(block);
            }
            // Not a block. Check its parent next.
            aCandidate = c.GetParent();
        }
        MOZ_ASSERT_UNREACHABLE!("Fell off frame tree looking for ancestor block!");
        None
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ComputeFinalBSize
    /**
     * Compute the final block size of this frame.
     *
     * @param aState BlockReflowState passed from parent during reflow.
     *        Note: aState.mReflowStatus is mostly an "input" parameter. When this
     *        method is called, it should represent what our status would be as if
     *        we were shrinkwrapping our children's block-size. This method will
     *        then adjust it before returning if our status is different in light
     *        of our actual final block-size and current page/column's available
     *        block-size.
     * @param aBEndEdgeOfChildren The distance between this frame's block-start
     *        border-edge and the block-end edge of our last child's border-box.
     *        This is effectively our block-start border-padding plus the
     *        block-size of our children, precomputed outside of this function.
     * @return our final block-size with respect to aReflowInput's writing-mode.
     */
    pub fn ComputeFinalBSize(&self, aState: &mut BlockReflowState, aBEndEdgeOfChildren: nscoord) -> nscoord {
        let wm = aState.mReflowInput.GetWritingMode();

        let effectiveContentBoxBSize: nscoord = self.GetEffectiveComputedBSize(aState.mReflowInput, aState.mConsumedBSize);
        let blockStartBP: nscoord = aState.BorderPadding().BStart(wm);
        let blockEndBP: nscoord = aState.BorderPadding().BEnd(wm);

        NS_ASSERTION!(
            !self.IsTrueOverflowContainer() || (effectiveContentBoxBSize == 0 && blockStartBP == 0 && blockEndBP == 0),
            "An overflow container's effective content-box block-size, block-start BP, and block-end BP should all be zero!"
        );

        let effectiveContentBoxBSizeWithBStartBP: nscoord = NSCoordSaturatingAdd(blockStartBP, effectiveContentBoxBSize);
        let effectiveBorderBoxBSize: nscoord = NSCoordSaturatingAdd(effectiveContentBoxBSizeWithBStartBP, blockEndBP);

        if self.HasColumnSpanSiblings() {
            MOZ_ASSERT!(self.LastInFlow().GetNextContinuation().is_some(), "Frame constructor should've created column-span siblings!");

            // If a block is split by any column-spans, we calculate the final
            // block-size by shrinkwrapping our children's block-size for all the
            // fragments except for those after the final column-span, but we should
            // take no more than our effective border-box block-size. If there's any
            // leftover block-size, our next continuations will take up rest.
            //
            // We don't need to adjust aBri.mReflowStatus because our children's status
            // is the same as ours.
            return std::cmp::min(effectiveBorderBoxBSize, aBEndEdgeOfChildren);
        }

        let availBSize: nscoord = aState.mReflowInput.AvailableBSize();
        if availBSize == NS_UNCONSTRAINEDSIZE {
            return effectiveBorderBoxBSize;
        }

        // Save our children's reflow status.
        let isChildStatusComplete = aState.mReflowStatus.IsComplete();
        if isChildStatusComplete && effectiveContentBoxBSize > 0 && effectiveBorderBoxBSize > availBSize && self.ShouldAvoidBreakInside(aState.mReflowInput) {
            aState.mReflowStatus.SetInlineLineBreakBeforeAndReset();
            return effectiveBorderBoxBSize;
        }

        let isBDBClone = aState.mReflowInput.mStyleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone;

        // The maximum value our content-box block-size can take within the given
        // available block-size.
        let maxContentBoxBSize: nscoord = aState.ContentBSize();

        // The block-end edge of our content-box (relative to this frame's origin) if
        // we consumed the maximum block-size available to us (maxContentBoxBSize).
        let maxContentBoxBEnd: nscoord = aState.ContentBEnd();

        // These variables are uninitialized intentionally so that the compiler can
        // check they are assigned in every if-else branch below.
        let finalContentBoxBSizeWithBStartBP: nscoord;
        let isOurStatusComplete: bool;

        if effectiveBorderBoxBSize <= availBSize {
            // Our effective border-box block-size can fit in the available block-size,
            // so we are complete.
            finalContentBoxBSizeWithBStartBP = effectiveContentBoxBSizeWithBStartBP;
            isOurStatusComplete = true;
        } else if effectiveContentBoxBSizeWithBStartBP <= maxContentBoxBEnd {
            // Note: The following assertion should generally hold because, for
            // box-decoration-break:clone, this "else if" branch is mathematically
            // equivalent to the initial "if".
            NS_ASSERTION!(!isBDBClone, "This else-if branch is handling a situation that's specific to box-decoration-break:slice, i.e. a case when we can skip our block-end border and padding!");

            // Our effective content-box block-size plus the block-start border and
            // padding can fit in the available block-size, but it cannot fit after
            // adding the block-end border and padding. Thus, we need a continuation
            // (unless we already weren't asking for any block-size, in which case we
            // stay complete to avoid looping forever).
            finalContentBoxBSizeWithBStartBP = effectiveContentBoxBSizeWithBStartBP;
            isOurStatusComplete = effectiveContentBoxBSize == 0;
        } else {
            // We aren't going to be able to fit our content-box in the space available
            // to it, which means we'll probably call ourselves incomplete to request a
            // continuation. But before making that decision, we check for certain
            // conditions which would force us to overflow beyond the available space --
            // these might result in us actually being complete if we're forced to
            // overflow far enough.
            if aState.mReflowInput.mFlags.mIsTopOfPage && isBDBClone && maxContentBoxBSize <= 0 && aBEndEdgeOfChildren == blockStartBP {
                // In this rare case, we are at the top of page/column, we have
                // box-decoration-break:clone and zero available block-size for our
                // content-box (e.g. our own block-start border and padding already exceed
                // the available block-size), and we didn't lay out any child to consume
                // our content-box block-size. To ensure we make progress (avoid looping
                // forever), use 1px as our content-box block-size regardless of our
                // effective content-box block-size, in the spirit of
                // https://drafts.csswg.org/css-break/#breaking-rules.
                finalContentBoxBSizeWithBStartBP = blockStartBP + AppUnitsPerCSSPixel();
                isOurStatusComplete = effectiveContentBoxBSize <= AppUnitsPerCSSPixel();
            } else if aBEndEdgeOfChildren > maxContentBoxBEnd {
                // We have a unbreakable child whose block-end edge exceeds the available
                // block-size for children.
                if aBEndEdgeOfChildren >= effectiveContentBoxBSizeWithBStartBP {
                    // The unbreakable child's block-end edge forces us to consume all of
                    // our effective content-box block-size.
                    finalContentBoxBSizeWithBStartBP = effectiveContentBoxBSizeWithBStartBP;

                    // Even though we've consumed all of our effective content-box
                    // block-size, we may still need to report an incomplete status in order
                    // to get another continuation, which will be responsible for laying out
                    // & drawing our block-end border & padding. But if we have no such
                    // border & padding, or if we're forced to apply that border & padding
                    // on this frame due to box-decoration-break:clone, then we don't need
                    // to bother with that additional continuation.
                    isOurStatusComplete = isBDBClone || blockEndBP == 0;
                } else {
                    // The unbreakable child's block-end edge doesn't force us to consume
                    // all of our effective content-box block-size.
                    finalContentBoxBSizeWithBStartBP = aBEndEdgeOfChildren;
                    isOurStatusComplete = false;
                }
            } else {
                // The children's block-end edge can fit in the content-box space that we
                // have available for it. Consume all the space that is available so that
                // our inline-start/inline-end borders extend all the way to the block-end
                // edge of column/page.
                finalContentBoxBSizeWithBStartBP = maxContentBoxBEnd;
                isOurStatusComplete = false;
            }
        }

        let mut finalBorderBoxBSize: nscoord = finalContentBoxBSizeWithBStartBP;
        if isOurStatusComplete {
            finalBorderBoxBSize = NSCoordSaturatingAdd(finalBorderBoxBSize, blockEndBP);
            if isChildStatusComplete {
                // We want to use children's reflow status as ours, which can be overflow
                // incomplete. Suppress the urge to call aBri.mReflowStatus.Reset() here.
            } else {
                aState.mReflowStatus.SetOverflowIncomplete();
            }
        } else {
            NS_ASSERTION!(!self.IsTrueOverflowContainer(), "An overflow container should always be complete because of its zero border-box block-size!");
            if isBDBClone {
                finalBorderBoxBSize = NSCoordSaturatingAdd(finalBorderBoxBSize, blockEndBP);
            }
            aState.mReflowStatus.SetIncomplete();
            if self.GetNextInFlow().is_none() {
                aState.mReflowStatus.SetNextInFlowNeedsReflow();
            }
        }

        finalBorderBoxBSize
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::ResolveBidi
    pub fn ResolveBidi(&self) -> nsresult {
        NS_ASSERTION!(self.GetPrevInFlow().is_none(), "ResolveBidi called on non-first continuation");
        MOZ_ASSERT!(self.PresContext().BidiEnabled());
        nsBidiPresUtils::Resolve(*self)
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::UpdatePseudoElementStyles
    // NOT PORTED: restyle of ::first-letter and ::first-line (Stylo post-traversal).

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetFirstLetter
    pub fn GetFirstLetter(&self) -> Option<nsIFrame> {
        if !self.HasAnyStateBits(NS_BLOCK_HAS_FIRST_LETTER_STYLE) {
            // Certainly no first-letter frame.
            return None;
        }

        self.GetProperty(nsContainerFrame::FirstLetterProperty())
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetFirstLineFrame
    pub fn GetFirstLineFrame(&self) -> Option<nsIFrame> {
        let maybeFirstLine = self.mFrames().FirstChild();
        if let Some(m) = maybeFirstLine {
            if m.IsLineFrame() {
                return Some(m);
            }
        }

        None
    }

    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::VerifyLines
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::VerifyOverflowSituation
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetDepth
    // Gecko: layout/generic/nsBlockFrame.cpp nsBlockFrame::GetFirstLetterStyle
    // NOT PORTED: debug-only verification (#ifdef DEBUG, active only with gVerifyLines), nsBlockFrame.cpp
    // lines 8980-9144 at GECKO_REV.
}

//////////////////////////////////////////////////////////////////////////////////////////
// TEMP stubs
//
// Everything below is NOT part of nsBlockFrame.h/.cpp. It stands in for members of nsIFrame,
// nsContainerFrame, nsSplittableFrame, nsLayoutUtils, Baseline, nsPresContext and the style structs
// that nsBlockFrame calls and that no other translated file has yet. Each item names the Gecko
// member it stands for. When the real member lands in its own file, delete the item here (a
// duplicate definition fails to compile, which is how a clash shows up).
//////////////////////////////////////////////////////////////////////////////////////////

// Gecko: generated by cbindgen from Stylo (no header): mozilla::StyleAlignFlags, StyleDirection,
// StyleUnicodeBidi
pub type StyleAlignFlags = mrt_style::style::values::specified::align::AlignFlags;
pub type StyleDirection = mrt_style::style::computed_values::direction::T;
pub type StyleUnicodeBidi = mrt_style::style::computed_values::unicode_bidi::T;

// Gecko: layout/generic/nsIFrameInlines.h and nsIFrame.h members (see the notes above)
impl nsIFrame {
    // Gecko: layout/generic/nsSplittableFrame.h nsSplittableFrame::PreReflowBlockLevelLogicalSkipSides
    // NOT PORTED: fragmentation (box-decoration-break slicing): no side is skipped.
    pub fn PreReflowBlockLevelLogicalSkipSides(&self) -> LogicalSides {
        LogicalSides::new__WritingMode(self.GetWritingMode())
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetSkipSides
    // NOT PORTED: fragmentation: no side is skipped.
    pub fn GetSkipSides(&self) -> Sides {
        Sides::default()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetRectRelativeToSelf
    pub fn GetRectRelativeToSelf(&self) -> nsRect {
        nsRect::new(0, 0, self.GetSize().width, self.GetSize().height)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalNormalPosition
    pub fn GetLogicalNormalPosition(&self, aWritingMode: WritingMode, aContainerSize: nsSize) -> LogicalPoint {
        // Subtract the size of this frame from the container size to get
        // the correct position in rtl frames where the origin is on the
        // right instead of the left
        LogicalPoint::new__WritingMode_nsPoint_nsSize(aWritingMode, self.GetNormalPosition(), aContainerSize - self.GetSize())
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalNormalRect
    pub fn GetLogicalNormalRect(&self, aWritingMode: WritingMode, aContainerSize: nsSize) -> LogicalRect {
        let r = nsRect::new(self.GetNormalPosition().x, self.GetNormalPosition().y, self.GetSize().width, self.GetSize().height);
        LogicalRect::new__WritingMode_nsRect_nsSize(aWritingMode, r, aContainerSize)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::MovePositionBy(WritingMode, const LogicalPoint&)
    pub fn MovePositionBy__WritingMode_LogicalPoint(&self, aWritingMode: WritingMode, aTranslation: LogicalPoint) {
        // The LogicalPoint is relative to this frame's container size (Gecko: GetSize()).
        let p = aTranslation.GetPhysicalPoint(aWritingMode, nsSize::new(0, 0));
        // The container size cancels out for a translation whose origin is (0, 0); only the
        // vertical-rl flip of the block axis matters, and GetPhysicalPoint handles that for a
        // zero-size container by returning the translation as a physical offset.
        self.MovePositionBy(p);
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::ContentBSize
    pub fn ContentBSize(&self, aWM: WritingMode) -> nscoord {
        let bp = self.GetLogicalUsedBorderAndPadding(aWM);
        std::cmp::max(0, self.BSize(aWM) - bp.BStartEnd(aWM))
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::GetFontMetricsDerivedCaretBaseline
    pub fn GetFontMetricsDerivedCaretBaseline(&self, aBSize: nscoord) -> nscoord {
        let inflation = nsLayoutUtils::FontSizeInflationFor(*self);
        let fm = nsLayoutUtils::GetFontMetricsForFrame(*self, inflation);
        let wm = self.GetWritingMode();
        let lineHeight = ReflowInput::CalcLineHeight__ComputedStyle_nsPresContext_nsIContent_nscoord_float(&self.Style(), Some(&self.PresContext()), self.GetContent().as_ref(), aBSize, inflation);
        nsLayoutUtils::GetCenteredFontBaseline(&fm, lineHeight, wm.IsLineInverted()) + self.GetLogicalUsedBorderAndPadding(wm).BStart(wm)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetCaretBaseline
    // NOT PORTED: the caret.
    pub fn GetCaretBaseline(&self) -> nscoord {
        match self.Type() {
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).GetCaretBaseline(),
            _ => self.GetFontMetricsDerivedCaretBaseline(self.ContentBSize(self.GetWritingMode())),
        }
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetNaturalBaselineBOffset (virtual)
    pub fn GetNaturalBaselineBOffset(&self, aWM: WritingMode, aBaselineGroup: BaselineSharingGroup, aExportContext: BaselineExportContext) -> Option<nscoord> {
        match self.Type() {
            // Block => nsBlockFrame::GetNaturalBaselineBOffset
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).GetNaturalBaselineBOffset(aWM, aBaselineGroup, aExportContext),
            // Text => nsTextFrame::GetNaturalBaselineBOffset
            LayoutFrameType::Text if self.UsesRealTextFrame() => nsTextFrame::from_frame(*self).GetNaturalBaselineBOffset(aWM, aBaselineGroup, aExportContext),
            // TableWrapper => nsTableWrapperFrame::GetNaturalBaselineBOffset
            LayoutFrameType::TableWrapper => nsTableWrapperFrame::from_frame(*self).GetNaturalBaselineBOffset(aWM, aBaselineGroup, aExportContext),
            // CheckboxRadio => nsCheckboxRadioFrame::GetNaturalBaselineBOffset
            LayoutFrameType::CheckboxRadio => nsCheckboxRadioFrame::from_frame(*self).GetNaturalBaselineBOffset(aWM, aBaselineGroup, aExportContext),
            // FlexContainer => nsFlexContainerFrame::GetNaturalBaselineBOffset
            LayoutFrameType::FlexContainer => nsFlexContainerFrame::from_frame(*self).GetNaturalBaselineBOffset(aWM, aBaselineGroup, aExportContext),
            // GridContainer => nsGridContainerFrame::GetNaturalBaselineBOffset
            LayoutFrameType::GridContainer => nsGridContainerFrame::from_frame(*self).GetNaturalBaselineBOffset(aWM, aBaselineGroup, aExportContext),
            // NOT PORTED: the other overrides (br, tables, ...); the base class has
            // no natural baseline.
            _ => None,
        }
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetDefaultBaselineSharingGroup (virtual)
    pub fn GetDefaultBaselineSharingGroup(&self) -> BaselineSharingGroup {
        match self.Type() {
            // Block => nsBlockFrame::GetDefaultBaselineSharingGroup
            LayoutFrameType::Block => BaselineSharingGroup::Last,
            _ => BaselineSharingGroup::First,
        }
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::ShouldHaveLineIfEmpty
    // NOT PORTED: editing hosts (the body is `IsInputButtonControlFrame() || IsEditingHost(this)` for a
    // frame without a pseudo type, nsIFrame.cpp lines 4974-4984 at GECKO_REV), and the pseudo type check
    // (the frames that are input buttons have none).
    pub fn ShouldHaveLineIfEmpty(&self) -> bool {
        self.IsInputButtonControlFrame()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsSubtreeDirty
    pub fn IsSubtreeDirty(&self) -> bool {
        self.HasAnyStateBits(NS_FRAME_IS_DIRTY | NS_FRAME_HAS_DIRTY_CHILDREN)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::PullOverflowsFromPrevInFlow
    // NOT PORTED: fragmentation.
    pub fn PullOverflowsFromPrevInFlow(&self) {}

    // Gecko: layout/generic/nsIFrame.h nsIFrame::HasAllStateBits
    pub fn HasAllStateBits(&self, aBitMask: nsFrameState) -> bool {
        (self.GetStateBits() & aBitMask) == aBitMask
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::AddOrRemoveStateBits
    pub fn AddOrRemoveStateBits(&self, aBits: nsFrameState, aVal: bool) {
        if aVal {
            self.AddStateBits(aBits);
        } else {
            self.RemoveStateBits(aBits);
        }
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::PresShell
    pub fn PresShell(&self) -> PresShell {
        PresShell
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsHiddenByContentVisibilityOfInFlowParentForLayout
    // NOT PORTED: content-visibility.
    pub fn IsHiddenByContentVisibilityOfInFlowParentForLayout(&self) -> bool {
        false
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsColumnSetWrapperFrame, IsColumnSetFrame,
    // IsFieldSetFrame, IsComboboxControlFrame, IsSVGTextFrame, IsPageBreakFrame
    pub fn IsColumnSetWrapperFrame(&self) -> bool {
        self.Type() == LayoutFrameType::ColumnSetWrapper
    }
    pub fn IsColumnSetFrame(&self) -> bool {
        self.Type() == LayoutFrameType::ColumnSet
    }
    pub fn IsFieldSetFrame(&self) -> bool {
        self.Type() == LayoutFrameType::FieldSet
    }
    pub fn IsComboboxControlFrame(&self) -> bool {
        self.Type() == LayoutFrameType::ComboboxControl
    }
    pub fn IsSVGTextFrame(&self) -> bool {
        self.Type() == LayoutFrameType::SVGText
    }
    pub fn IsPageBreakFrame(&self) -> bool {
        self.Type() == LayoutFrameType::PageBreak
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsFloating (nsIFrameInlines.h)
    pub fn IsFloating(&self) -> bool {
        self.StyleDisplay().IsFloating(*self)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsFloatContainingBlock (virtual)
    pub fn IsFloatContainingBlock(&self) -> bool {
        match self.Type() {
            // Block => nsBlockFrame::IsFloatContainingBlock
            LayoutFrameType::Block => true,
            // NOT PORTED: the other overrides (nsInlineFrame is false, table cells, ... are true).
            LayoutFrameType::Inline | LayoutFrameType::Line => false,
            _ => false,
        }
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsRootElementStyle (Style()->IsRootElementStyle())
    pub fn IsRootElementStyle(&self) -> bool {
        self.GetContent().map_or(false, |c| c.IsHTMLElement(nsGkAtoms::html))
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsColumnSpan
    // NOT PORTED: columns.
    pub fn IsColumnSpan(&self) -> bool {
        false
    }

    // ADAPTER: Style()->GetPseudoType() == PseudoStyleType::Marker for the frame of an outside
    // ::marker. The frame constructor registers it with SetMarkerFrameForListItem on its parent
    // block, so the question is "is this frame the outside marker of its parent".
    pub fn IsOutsideMarkerFrame(&self) -> bool {
        self.GetParent().and_then(nsBlockFrame::do_QueryFrame).map_or(false, |p| p.GetOutsideMarker() == Some(*self))
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::HasSignificantTerminalNewline
    // NOT PORTED: text frames (the text of a text frame is not known here); a text frame never
    // ends in a significant newline.
    pub fn HasSignificantTerminalNewline(&self) -> bool {
        // Text => nsTextFrame::HasSignificantTerminalNewline
        if self.Type() == LayoutFrameType::Text && self.UsesRealTextFrame() {
            return nsTextFrame::from_frame(*self).HasSignificantTerminalNewline();
        }
        false
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetPlaceholderFrame
    // ADAPTER: the reverse of the placeholder-to-out-of-flow link kept by nsLineLayoutAdapter.rs.
    pub fn GetPlaceholderFrame(&self) -> Option<nsIFrame> {
        nsLineLayoutAdapter::PlaceholderFor(*self)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetContent() == other->GetContent()
    // (the content is a handle of the DOM node now, so this is the pointer comparison; it was a
    // comparison of HTML local names while a frame held a copy of its content)
    pub fn GetContent_ptr_eq(&self, aOther: Option<nsIContent>) -> bool {
        self.GetContent() == aOther
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::GetFloatContainingBlock
    pub fn GetFloatContainingBlock(&self, aFrame: nsIFrame) -> Option<nsIFrame> {
        let mut parent = aFrame.GetParent();
        while let Some(p) = parent {
            if p.IsFloatContainingBlock() {
                return Some(p);
            }
            parent = p.GetParent();
        }
        None
    }

    // Gecko: layout/base/nsCSSFrameConstructor.cpp nsCSSFrameConstructor::CreateContinuingFrame
    // NOT PORTED: fragmentation.
    pub fn CreateContinuingFrame(&self, aFrame: nsIFrame) -> nsIFrame {
        nsCSSFrameConstructor_CreateContinuingFrame(aFrame, *self, true)
    }

    // Gecko: layout/generic/nsSplittableFrame.cpp nsSplittableFrame::CalcAndCacheConsumedBSize
    // NOT PORTED: fragmentation: nothing was consumed by earlier continuations.
    pub fn CalcAndCacheConsumedBSize(&self) -> nscoord {
        let prev = self.GetPrevContinuation();
        if prev.is_none() {
            return 0;
        }
        unimplemented!("NOT PORTED: fragmentation (nsSplittableFrame::CalcAndCacheConsumedBSize)")
    }

    // Gecko: layout/generic/nsSplittableFrame.cpp nsSplittableFrame::GetEffectiveComputedBSize
    pub fn GetEffectiveComputedBSize(&self, aReflowInput: &ReflowInput, aConsumedBSize: nscoord) -> nscoord {
        let mut bSize = aReflowInput.ComputedBSize();
        if bSize == NS_UNCONSTRAINEDSIZE {
            return NS_UNCONSTRAINEDSIZE;
        }

        bSize -= aConsumedBSize;

        // nsFieldSetFrame's inner frames are special since some of their content-box
        // BSize may be consumed by positioning it below the legend.  So we always
        // report zero for true overflow containers here.
        // XXXmats: hmm, can we fix this so that the sizes actually adds up instead?
        // NOT PORTED: fieldsets.

        // We may have stretched the frame beyond its computed height. Oh well.
        std::cmp::max(0, bSize)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::ShouldApplyOverflowClipping
    // NOT PORTED: scrolling / overflow clipping: no axis is clipped.
    pub fn ShouldApplyOverflowClipping(&self, aDisplay: &nsStyleDisplay) -> PhysicalAxes {
        PhysicalAxes::new()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::OverflowClipMargin
    // NOT PORTED: overflow-clip-margin.
    pub fn OverflowClipMargin(&self, aClipAxes: PhysicalAxes, aAllowNegative: bool) -> nsMargin {
        nsMargin::default()
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::ContainIntrinsicISize and ContainIntrinsicBSize
    // NOT PORTED: contain-intrinsic-size values (`contain-intrinsic-size: <length>` on a
    // size-contained box); a size-contained axis has an intrinsic size of 0.
    pub fn ContainIntrinsicISize(&self) -> Option<nscoord> {
        let axes = self.GetContainSizeAxes();
        let wm = self.GetWritingMode();
        let contained = if wm.IsVertical() { axes.mBContained } else { axes.mIContained };
        if contained { Some(0) } else { None }
    }
    pub fn ContainIntrinsicBSize(&self, aFallback: nscoord) -> Option<nscoord> {
        let axes = self.GetContainSizeAxes();
        let wm = self.GetWritingMode();
        let contained = if wm.IsVertical() { axes.mIContained } else { axes.mBContained };
        if contained { Some(aFallback) } else { None }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::{Init, MarkIntrinsicISizesDirty,
    // ComputeCustomOverflow, DidSetComputedStyle} called by name from
    // nsBlockFrame's overrides
    pub fn nsContainerFrame_Init(&self, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: Option<nsIFrame>) {}
    pub fn nsContainerFrame_MarkIntrinsicISizesDirty(&self) {}
    pub fn nsContainerFrame_ComputeCustomOverflow(&self, aOverflowAreas: &mut OverflowAreas) -> bool {
        true
    }
    pub fn nsContainerFrame_DidSetComputedStyle(&self, aOldStyle: Option<()>) {}
}

// Gecko: layout/base/PresShell.h mozilla::PresShell (only FrameNeedsReflow)
pub struct PresShell;

impl PresShell {
    // Gecko: layout/base/PresShell.cpp PresShell::FrameNeedsReflow
    pub fn FrameNeedsReflow(&self, aFrame: nsIFrame, aIntrinsicDirty: IntrinsicDirty, aBitToAdd: nsFrameState) {
        PresShell_FrameNeedsReflow(aFrame, aIntrinsicDirty, aBitToAdd);
    }
}

// Gecko: layout/generic/nsIFrame.h TEXT_OFFSETS_NEED_FIXING, nsFrameStateBits.h
// NS_FRAME_FONT_INFLATION_CONTAINER
pub const TEXT_OFFSETS_NEED_FIXING: nsFrameState = 1 << 50;
pub const NS_FRAME_FONT_INFLATION_CONTAINER: nsFrameState = 1 << 51;

// Gecko: layout/base/nsPresContext.h members
pub trait nsPresContext_nsBlockFrameTemp {
    fn HasPendingInterrupt(&self) -> bool;
    fn DevPixelsToAppUnits(&self, aPixels: i32) -> nscoord;
    fn mInflationDisabledForShrinkWrap(&self) -> bool;
}

impl nsPresContext_nsBlockFrameTemp for nsPresContext {
    // NOT PORTED: reflow interrupts.
    fn HasPendingInterrupt(&self) -> bool {
        false
    }
    // 1 device pixel is 1 CSS pixel here (no device scale).
    fn DevPixelsToAppUnits(&self, aPixels: i32) -> nscoord {
        aPixels * AppUnitsPerCSSPixel()
    }
    // NOT PORTED: font size inflation.
    fn mInflationDisabledForShrinkWrap(&self) -> bool {
        false
    }
}

// Gecko: layout/style/nsStyleStruct.h nsStylePadding::mPadding.Get(side) and GetBStart/GetBEnd
// (StyleRect<LengthPercentage> accessors), nsStyleBorder::GetComputedBorderWidth
impl nsStylePadding {
    pub fn mPadding_Get(&self, aSide: Side) -> LengthPercentage {
        self.mPadding().Get__Side(aSide).clone()
    }
    pub fn mPadding_GetBStart(&self, aWM: WritingMode) -> LengthPercentage {
        self.mPadding().Get__Side(aWM.PhysicalSide(LogicalSide::BStart)).clone()
    }
    pub fn mPadding_GetBEnd(&self, aWM: WritingMode) -> LengthPercentage {
        self.mPadding().Get__Side(aWM.PhysicalSide(LogicalSide::BEnd)).clone()
    }
}

impl nsStyleBorder {
    pub fn GetComputedBorderWidth(&self, aSide: Side) -> nscoord {
        self.GetComputedBorder().Side(aSide)
    }
}

// Gecko: layout/base/Baseline.h mozilla::Baseline (SynthesizeBOffsetFromContentBox and
// SynthesizeBOffsetFromMarginBox; layout/base/Baseline.cpp)
pub struct Baseline;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Baseline_BoxType {
    Margin,
    Border,
    Padding,
    Content,
}

impl Baseline {
    // Gecko: layout/base/Baseline.cpp ComputeBStartOffset
    fn ComputeBStartOffset(aWM: WritingMode, aGroup: BaselineSharingGroup, aBSize: nscoord, aMargin: &LogicalMargin) -> nscoord {
        if aGroup == BaselineSharingGroup::First { -aMargin.BStart(aWM) } else { aBSize + aMargin.BStart(aWM) }
    }

    // Gecko: layout/base/Baseline.cpp ComputeBEndOffset
    fn ComputeBEndOffset(aWM: WritingMode, aGroup: BaselineSharingGroup, aBSize: nscoord, aMargin: &LogicalMargin) -> nscoord {
        if aGroup == BaselineSharingGroup::First { aBSize + aMargin.BEnd(aWM) } else { -aMargin.BEnd(aWM) }
    }

    // Gecko: layout/base/Baseline.cpp SynthesizeBOffsetFromInnerBox
    fn SynthesizeBOffsetFromInnerBox(aType: Baseline_BoxType, aFrame: nsIFrame, aWM: WritingMode, aGroup: BaselineSharingGroup) -> nscoord {
        let wm = aFrame.GetWritingMode();
        MOZ_ASSERT!(aType == Baseline_BoxType::Border || !aWM.IsOrthogonalTo(&wm));

        let bp: LogicalMargin = match aType {
            // Bug 2034085 - This is not currently applying the logical skip sides
            // to the margin box, unlike the other boxes below. This may be
            // resulting in incorrect offset calculations if the box is fragmented.
            Baseline_BoxType::Margin => aFrame.GetLogicalUsedMargin(aWM),
            Baseline_BoxType::Border => LogicalMargin::new__WritingMode(aWM),
            Baseline_BoxType::Padding => {
                let mut b = aFrame.GetLogicalUsedBorder(wm);
                let b = b.ApplySkipSides(aFrame.GetLogicalSkipSides()).ConvertTo(aWM, wm);
                LogicalMargin::new__WritingMode(aWM) - b
            }
            Baseline_BoxType::Content => {
                let mut b = aFrame.GetLogicalUsedBorderAndPadding(wm);
                let b = b.ApplySkipSides(aFrame.GetLogicalSkipSides()).ConvertTo(aWM, wm);
                LogicalMargin::new__WritingMode(aWM) - b
            }
        };

        let mut baseline = aFrame.AlignmentBaseline();
        if baseline == StyleAlignmentBaseline::Baseline {
            baseline = if aWM.IsCentralBaseline() { StyleAlignmentBaseline::Central } else { StyleAlignmentBaseline::Alphabetic };
        }

        let mut group = aGroup;
        if aWM.IsLineInverted() {
            group = if aGroup == BaselineSharingGroup::First { BaselineSharingGroup::Last } else { BaselineSharingGroup::First };
        }

        // Synthesize the inline baseline for an atomic inline from its margin box.
        // See: https://www.w3.org/TR/css-inline-3/#baseline-synthesis-box
        match baseline {
            StyleAlignmentBaseline::Central | StyleAlignmentBaseline::Mathematical | StyleAlignmentBaseline::Middle => {
                let boxBSize: nscoord = aFrame.BSize(aWM) + bp.BStartEnd(aWM);
                let halfBoxBSize: nscoord = (boxBSize / 2) + (boxBSize % 2);
                if aWM.IsLineInverted() { -bp.BEnd(aWM) + halfBoxBSize } else { -bp.BStart(aWM) + halfBoxBSize }
            }
            StyleAlignmentBaseline::Hanging | StyleAlignmentBaseline::TextTop => Baseline::ComputeBStartOffset(aWM, group, aFrame.BSize(aWM), &bp),
            // Baseline (handled above), Alphabetic, Ideographic, TextBottom and the rest.
            _ => Baseline::ComputeBEndOffset(aWM, group, aFrame.BSize(aWM), &bp),
        }
    }

    // Gecko: layout/base/Baseline.cpp Baseline::SynthesizeBOffsetFromContentBox
    pub fn SynthesizeBOffsetFromContentBox(aFrame: nsIFrame, aWM: WritingMode, aGroup: BaselineSharingGroup) -> nscoord {
        Baseline::SynthesizeBOffsetFromInnerBox(Baseline_BoxType::Content, aFrame, aWM, aGroup)
    }

    // Gecko: layout/base/Baseline.cpp Baseline::SynthesizeBOffsetFromPaddingBox
    pub fn SynthesizeBOffsetFromPaddingBox(aFrame: nsIFrame, aWM: WritingMode, aGroup: BaselineSharingGroup) -> nscoord {
        Baseline::SynthesizeBOffsetFromInnerBox(Baseline_BoxType::Padding, aFrame, aWM, aGroup)
    }

    // Gecko: layout/base/Baseline.cpp Baseline::SynthesizeBOffsetFromBorderBox
    pub fn SynthesizeBOffsetFromBorderBox(aFrame: nsIFrame, aWM: WritingMode, aGroup: BaselineSharingGroup) -> nscoord {
        Baseline::SynthesizeBOffsetFromInnerBox(Baseline_BoxType::Border, aFrame, aWM, aGroup)
    }

    // Gecko: layout/base/Baseline.cpp Baseline::SynthesizeBOffsetFromMarginBox
    pub fn SynthesizeBOffsetFromMarginBox(aFrame: nsIFrame, aWM: WritingMode, aGroup: BaselineSharingGroup) -> nscoord {
        Baseline::SynthesizeBOffsetFromInnerBox(Baseline_BoxType::Margin, aFrame, aWM, aGroup)
    }
}

// Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::LinePosition
#[derive(Clone, Copy, Debug)]
pub struct nsLayoutUtils_LinePosition {
    pub mBStart: nscoord,
    pub mBaseline: nscoord,
    pub mBEnd: nscoord,
}

impl Default for nsLayoutUtils_LinePosition {
    fn default() -> Self {
        nsLayoutUtils_LinePosition { mBStart: nscoord_MAX, mBaseline: nscoord_MAX, mBEnd: nscoord_MAX }
    }
}

impl std::ops::Add<nscoord> for nsLayoutUtils_LinePosition {
    type Output = nsLayoutUtils_LinePosition;
    fn add(self, aOffset: nscoord) -> nsLayoutUtils_LinePosition {
        nsLayoutUtils_LinePosition { mBStart: self.mBStart + aOffset, mBaseline: self.mBaseline + aOffset, mBEnd: self.mBEnd + aOffset }
    }
}

// Gecko: layout/base/nsLayoutUtils.cpp members used by nsBlockFrame.cpp
impl nsLayoutUtils {
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetFirstLineBaseline
    pub fn GetFirstLineBaseline(aWritingMode: WritingMode, aFrame: nsIFrame, aResult: &mut nscoord) -> bool {
        let mut position = nsLayoutUtils_LinePosition::default();
        if !nsLayoutUtils::GetFirstLinePosition(aWritingMode, aFrame, &mut position) {
            return false;
        }
        *aResult = position.mBaseline;
        true
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetFirstLinePosition
    // NOT PORTED: tables, flex and grid containers, scroll frames, fieldsets and column sets
    // (nsLayoutUtils.cpp lines 5663-5747 at GECKO_REV); a frame that is not a block has no first line.
    pub fn GetFirstLinePosition(aWM: WritingMode, aFrame: nsIFrame, aResult: &mut nsLayoutUtils_LinePosition) -> bool {
        if aFrame.StyleDisplay().IsContainLayout() {
            return false;
        }
        let block = nsBlockFrame::do_QueryFrame(aFrame);
        let Some(block) = block else {
            // For the first-line baseline we also have to check for a table, and if
            // so, use the baseline of its first row.
            let fType = aFrame.Type();
            if fType == LayoutFrameType::TableWrapper || fType == LayoutFrameType::FlexContainer || fType == LayoutFrameType::GridContainer {
                if (fType == LayoutFrameType::FlexContainer && aFrame.HasAnyStateBits(NS_STATE_FLEX_SYNTHESIZE_BASELINE)) || (fType == LayoutFrameType::GridContainer && aFrame.HasAnyStateBits(NS_STATE_GRID_SYNTHESIZE_BASELINE)) || (fType == LayoutFrameType::TableWrapper && nsTableWrapperFrame::from_frame(aFrame).GetRowCount() == 0) {
                    // empty grid/flex/table container
                    aResult.mBStart = 0;
                    aResult.mBaseline = Baseline::SynthesizeBOffsetFromBorderBox(aFrame, aWM, BaselineSharingGroup::First);
                    aResult.mBEnd = aFrame.BSize(aWM);
                    return true;
                }
                if fType == LayoutFrameType::TableWrapper && aFrame.GetWritingMode().IsOrthogonalTo(&aWM) {
                    // For tables, the upcoming GetLogicalBaseline call would determine the
                    // table's baseline from its first row that has a baseline. However:
                    // this doesn't make sense for an orthogonal writing mode, so in that
                    // case we report no baseline instead. The table wrapper and its rows
                    // should flow the same way, so we can bail out early, but this logic
                    // wouldn't be correct to transplant into other places in the codebase
                    // (Depending on how bug 1786633 is resolved).
                    return false;
                }
                aResult.mBStart = 0;
                aResult.mBaseline = aFrame.GetLogicalBaseline__WritingMode(aWM);
                // This is what we want for the list bullet caller; not sure if
                // other future callers will want the same.
                aResult.mBEnd = aFrame.BSize(aWM);
                return true;
            }
            // NOT PORTED: scroll frames, fieldsets and column sets (nsLayoutUtils.cpp lines 5693-5742).

            // No baseline.
            return false;
        };

        for line in block.Lines() {
            if line.IsBlock() {
                let kid = line.mFirstChild.unwrap();
                let mut kidPosition = nsLayoutUtils_LinePosition::default();
                if nsLayoutUtils::GetFirstLinePosition(aWM, kid, &mut kidPosition) {
                    // XXX Not sure if this is the correct value to use for container
                    //    width here. It will only be used in vertical-rl layout,
                    //    which we don't have full support and testing for yet.
                    let containerSize = line.mContainerSize;
                    *aResult = kidPosition + kid.GetLogicalNormalPosition(aWM, containerSize).B(aWM);
                    return true;
                }
            } else {
                // XXX Is this the right test?  We have some bogus empty lines
                // floating around, but IsEmpty is perhaps too weak.
                if 0 != line.BSize() || !line.IsEmpty() {
                    let bStart: nscoord = line.BStart();
                    aResult.mBStart = bStart;
                    aResult.mBaseline = bStart + line.GetLogicalAscent();
                    aResult.mBEnd = bStart + line.BSize();
                    return true;
                }
            }
        }
        false
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetLastLineBaseline
    // NOT PORTED: scroll frames and column sets (nsLayoutUtils.cpp lines 5792-5819).
    pub fn GetLastLineBaseline(aWM: WritingMode, aFrame: nsIFrame, aResult: &mut nscoord) -> bool {
        if aFrame.StyleDisplay().IsContainLayout() {
            return false;
        }

        let block = nsBlockFrame::do_QueryFrame(aFrame);
        let Some(block) = block else {
            // No baseline.
            return false;
        };

        let mut line = block.LinesRBegin();
        let line_end = block.LinesREnd();
        while line != line_end {
            if line.IsBlock() {
                let kid = line.mFirstChild.unwrap();
                let mut kidBaseline: nscoord = 0;
                let containerSize = line.mContainerSize;
                if nsLayoutUtils::GetLastLineBaseline(aWM, kid, &mut kidBaseline) {
                    // Ignore relative positioning for baseline calculations
                    *aResult = kidBaseline + kid.GetLogicalNormalPosition(aWM, containerSize).B(aWM);
                    return true;
                }
                if kid.IsScrollContainerFrame() {
                    // Defer to nsIFrame::GetLogicalBaseline (which synthesizes a baseline
                    // from the margin-box).
                    kidBaseline = kid.GetLogicalBaseline__WritingMode(aWM);
                    *aResult = kidBaseline + kid.GetLogicalNormalPosition(aWM, containerSize).B(aWM);
                    return true;
                }
            } else {
                // XXX Is this the right test?  We have some bogus empty lines
                // floating around, but IsEmpty is perhaps too weak.
                if line.BSize() != 0 || !line.IsEmpty() {
                    *aResult = line.BStart() + line.GetLogicalAscent();
                    return true;
                }
            }
            line.operator_inc();
        }
        false
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::CombineClearType
    pub fn CombineClearType(aOrigClearType: UsedClear, aNewClearType: UsedClear) -> UsedClear {
        let mut clearType = aOrigClearType;
        match clearType {
            UsedClear::Left => {
                if UsedClear::Right == aNewClearType || UsedClear::Both == aNewClearType {
                    clearType = UsedClear::Both;
                }
            }
            UsedClear::Right => {
                if UsedClear::Left == aNewClearType || UsedClear::Both == aNewClearType {
                    clearType = UsedClear::Both;
                }
            }
            UsedClear::None => {
                if UsedClear::Left == aNewClearType || UsedClear::Right == aNewClearType || UsedClear::Both == aNewClearType {
                    clearType = aNewClearType;
                }
            }
            UsedClear::Both => {
                // Do nothing.
            }
        }
        clearType
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::FindChildContainingDescendant
    pub fn FindChildContainingDescendant(aParent: nsIFrame, aDescendantFrame: nsIFrame) -> Option<nsIFrame> {
        let mut result = Some(aDescendantFrame);

        while let Some(r) = result {
            let parent = r.GetParent();
            if parent == Some(aParent) {
                break;
            }

            // The frame is not an immediate child of aParent so walk up another level
            result = parent;
        }

        result
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::UnionChildOverflow
    pub fn UnionChildOverflow(aFrame: nsIFrame, aOverflowAreas: &mut OverflowAreas, aSkipChildLists: &[FrameChildListID]) {
        let mut lists: Vec<FrameChildList> = Vec::new();
        aFrame.GetChildLists(&mut lists);
        for l in &lists {
            if aSkipChildLists.contains(&l.mID) {
                continue;
            }
            for child in &l.mList {
                aOverflowAreas.UnionWith(&child.GetOverflowAreasRelativeToParent());
            }
        }
    }
}

impl nsIFrame {
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::WritingModeForLine
    pub fn WritingModeForLine(&self, aSelfWM: WritingMode, aSubFrame: nsIFrame) -> WritingMode {
        MOZ_ASSERT!(aSelfWM == self.GetWritingMode());
        let mut writingMode = aSelfWM;

        if self.Style().get_text().unicode_bidi == StyleUnicodeBidi::Plaintext {
            let frameLevel = nsBidiPresUtils::GetFrameBaseLevel(aSubFrame);
            writingMode.SetDirectionFromBidiLevel(frameLevel);
        }

        writingMode
    }
}

// Gecko: layout/generic/nsContainerFrame.h ConsiderChildOverflow (two overloads: the second has the
// flags argument). nsContainerFrame.rs has the one with flags as `ConsiderChildOverflow`.
impl nsIFrame {
    pub fn ConsiderChildOverflow__OverflowAreas_nsIFrame(&self, aOverflowAreas: &mut OverflowAreas, aChildFrame: nsIFrame) {
        self.ConsiderChildOverflow(aOverflowAreas, aChildFrame, OverflowAreaUnionFlags::None);
    }
    pub fn ConsiderChildOverflow__OverflowAreas_nsIFrame_OverflowAreaUnionFlags(&self, aOverflowAreas: &mut OverflowAreas, aChildFrame: nsIFrame, aFlags: OverflowAreaUnionFlags) {
        self.ConsiderChildOverflow(aOverflowAreas, aChildFrame, aFlags);
    }
}

// Gecko (servo/ports/geckolib/cbindgen.toml, GenericAspectRatio): StyleAspectRatio::HasFiniteRatio is
// `static_cast<bool>(ToLayoutRatio())`: a ratio that is not degenerate (neither term zero).
pub trait StyleAspectRatio_nsBlockFrameTemp {
    fn HasFiniteRatio(&self) -> bool;
}

impl StyleAspectRatio_nsBlockFrameTemp for StyleAspectRatio {
    fn HasFiniteRatio(&self) -> bool {
        use mrt_style::style::values::generics::position::PreferredRatio;
        match &self.ratio {
            PreferredRatio::Ratio(r) => r.0.0 > 0.0 && r.1.0 > 0.0 && r.0.0.is_finite() && r.1.0.is_finite(),
            PreferredRatio::None => false,
        }
    }
}

// Tests. Firefox has no gtest for nsBlockFrame (its layout is tested with reftests), so these are
// written from the documented behaviour (CSS 2.2 8.3.1 margin collapsing, 9.4.1 block formatting
// contexts, 9.5 floats, 9.5.2 clear, 10.6.3 auto heights, 12.5 list markers, 10.7 min-height) on real
// Stylo styles. A "text" frame is a TextFrameStub: a word of a fixed width and height.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockReflowState_test_support::*;
    use crate::nsLineLayoutAdapter::TextFrameStub;

    const AUTO: nscoord = NS_UNCONSTRAINEDSIZE;

    // What a frame of the test tree is.
    #[derive(Clone, Copy)]
    enum K {
        // a block-level box (its children are in lines)
        Block,
        // a float: out-of-flow, in the parent's float list (its placeholder is the next entry)
        Float,
        // the placeholder of the float at this index
        Placeholder(usize),
        // a word: width in px, height in px, break opportunity before it
        Text(i32, i32, bool),
        // the outside ::marker of the parent list item: width and height in px
        Marker(i32, i32),
    }

    // Builds the frames for the elements with the ids of `spec` (each the child of the entry whose
    // index is given) and does what the frame constructor does: types, floats and their placeholders,
    // the marker, Init (the BFC bit) and SetInitialChildList for every block. All elements are in `html`.
    fn build(html: &str, spec: &[(&str, Option<usize>, K)]) -> Vec<nsIFrame> {
        let tree: Vec<(&str, Option<usize>)> = spec.iter().map(|s| (s.0, s.1)).collect();
        let f = frames(html, &tree, 10);
        for (i, s) in spec.iter().enumerate() {
            match s.2 {
                K::Block | K::Float | K::Marker(..) => f[i].SetType(LayoutFrameType::Block),
                K::Placeholder(_) => f[i].SetType(LayoutFrameType::Placeholder),
                K::Text(w, h, brk) => {
                    f[i].SetType(LayoutFrameType::Text);
                    f[i].SetTextFrameStub(TextFrameStub { mISize: w * PX, mBSize: h * PX, mAscent: h * PX * 4 / 5, mBreakBefore: brk, ..Default::default() });
                }
            }
        }
        // out-of-flow and marker frames leave the principal list of their parent
        for (i, s) in spec.iter().enumerate() {
            let parent = s.1.map(|p| f[p]);
            match s.2 {
                K::Float => {
                    let parent = parent.unwrap();
                    parent.mFrames().RemoveFrame(f[i]);
                    f[i].AddStateBits(NS_FRAME_OUT_OF_FLOW);
                    nsBlockFrame::from_frame(parent).EnsureFloats().AppendFrame(Some(parent), f[i]);
                }
                K::Placeholder(target) => f[i].SetPlaceholderFor(f[target]),
                K::Marker(w, h) => {
                    let parent = parent.unwrap();
                    parent.mFrames().RemoveFrame(f[i]);
                    f[i].SetReflowStubSize(w * PX, h * PX, h * PX);
                    f[i].SetIsEmpty(false);
                    nsBlockFrame::from_frame(parent).SetMarkerFrameForListItem(f[i]);
                }
                _ => {}
            }
        }
        // Init, then SetInitialChildList (the order of the frame constructor)
        for (i, s) in spec.iter().enumerate() {
            if matches!(s.2, K::Block | K::Float | K::Marker(..)) {
                let block = nsBlockFrame::from_frame(f[i]);
                block.Init(None, s.1.map_or(f[i], |p| f[p]), None);
            }
        }
        for (i, s) in spec.iter().enumerate() {
            if matches!(s.2, K::Block | K::Float) {
                let list = std::mem::take(&mut *f[i].mFrames());
                if list.NotEmpty() {
                    nsBlockFrame::from_frame(f[i]).SetInitialChildList(FrameChildListID::Principal, list);
                }
            }
        }
        f
    }

    // Reflows the root (available inline-size `avail` px, unconstrained block-size) and sets its size
    // from the result, as its parent would.
    fn reflow(root: nsIFrame, avail: i32) -> ReflowOutput {
        let ri = root_input(root, avail, AUTO);
        let wm = root.GetWritingMode();
        let mut metrics = ReflowOutput::new__ReflowInput(&ri);
        let mut status = nsReflowStatus::new();
        root.Reflow(&nsPresContext::Current(), &mut metrics, &ri, &mut status);
        assert!(status.IsFullyComplete());
        root.SetSize__WritingMode_LogicalSize(wm, metrics.Size(wm));
        metrics
    }

    // The lines of a block as (block-start, block-size, is-block) in px.
    fn lines(block: nsIFrame) -> Vec<(i32, i32, bool)> {
        nsBlockFrame::from_frame(block).Lines().map(|l| (l.BStart() / PX, l.BSize() / PX, l.IsBlock())).collect()
    }

    #[test]
    fn stacked_blocks_add_their_heights() {
        // CSS 2.2 10.6.3: the height of a block with block children is the distance from the top
        // margin-edge of the first child to the bottom margin-edge of the last child.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=a style="margin:0;height:10px"></div><div id=b style="margin:0;height:20px"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block), ("b", Some(0), K::Block)]);
        let m = reflow(f[0], 10);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 30 * PX);
        assert_eq!(f[1].GetPosition(), nsPoint::new(0, 0));
        assert_eq!(f[1].GetSize(), nsSize::new(10 * PX, 10 * PX));
        assert_eq!(f[2].GetPosition(), nsPoint::new(0, 10 * PX));
        assert_eq!(f[2].GetSize(), nsSize::new(10 * PX, 20 * PX));
        assert_eq!(lines(f[0]), vec![(0, 10, true), (10, 20, true)]);
    }

    #[test]
    fn sibling_margins_collapse_to_the_larger_one() {
        // CSS 2.2 8.3.1: the adjoining vertical margins of in-flow siblings collapse to the maximum.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=a style="margin:0 0 20px 0;height:10px"></div><div id=b style="margin:30px 0 0 0;height:10px"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block), ("b", Some(0), K::Block)]);
        let m = reflow(f[0], 10);
        assert_eq!(f[1].GetPosition().y, 0);
        assert_eq!(f[2].GetPosition().y, 10 * PX + 30 * PX);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 50 * PX);
    }

    #[test]
    fn parent_and_first_child_margins_collapse() {
        // CSS 2.2 8.3.1: the top margin of a block collapses with the top margin of its first
        // child when there is no border, padding or clearance between them: the child's 30px wins
        // over the parent's 10px, and the parent's top edge moves down by the collapsed margin.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=p style="margin:10px 0 0 0;padding:0;border:0"><div id=c style="margin:30px 0 0 0;height:10px"></div></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("p", Some(0), K::Block), ("c", Some(1), K::Block)]);
        let m = reflow(f[0], 10);
        assert_eq!(f[1].GetPosition().y, 30 * PX);
        assert_eq!(f[2].GetPosition().y, 0);
        assert_eq!(f[1].GetSize().height, 10 * PX);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 40 * PX);
    }

    #[test]
    fn padding_stops_parent_child_margin_collapsing() {
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=p style="margin:10px 0 0 0;padding:1px 0 0 0;border:0"><div id=c style="margin:30px 0 0 0;height:10px"></div></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("p", Some(0), K::Block), ("c", Some(1), K::Block)]);
        let m = reflow(f[0], 10);
        assert_eq!(f[1].GetPosition().y, 10 * PX);
        assert_eq!(f[2].GetPosition().y, 1 * PX + 30 * PX);
        assert_eq!(m.BSize(f[0].GetWritingMode()), (10 + 1 + 30 + 10) * PX);
    }

    #[test]
    fn empty_block_margins_collapse_through_it() {
        // CSS 2.2 8.3.1: the top and bottom margins of an empty block collapse together and with
        // the adjoining margins: the gap between a and b is the largest of 5, 7 and 3.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=a style="margin:0;height:10px"></div>
            <div id=e style="margin:5px 0 7px 0;padding:0;border:0"></div>
            <div id=b style="margin:3px 0 0 0;height:10px"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block), ("e", Some(0), K::Block), ("b", Some(0), K::Block)]);
        let m = reflow(f[0], 10);
        assert_eq!(f[2].GetSize().height, 0);
        assert!(nsBlockFrame::from_frame(f[2]).IsEmpty());
        assert_eq!(f[3].GetPosition().y, 10 * PX + 7 * PX);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 27 * PX);
    }

    #[test]
    fn an_empty_block_has_no_height() {
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0"></div>"#;
        let f = build(html, &[("root", None, K::Block)]);
        let m = reflow(f[0], 10);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 0);
        assert_eq!(m.ISize(f[0].GetWritingMode()), 10 * PX);
        assert!(nsBlockFrame::from_frame(f[0]).mLines().empty());
    }

    #[test]
    fn auto_height_includes_padding_and_border() {
        // CSS 2.2 10.6.3 and 8.1: the border box is the content plus padding and border.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:3px 0 4px 0;border:1px solid">
            <div id=a style="margin:0;height:10px"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block)]);
        let m = reflow(f[0], 10);
        assert_eq!(m.BSize(f[0].GetWritingMode()), (1 + 3 + 10 + 4 + 1) * PX);
        assert_eq!(f[1].GetPosition(), nsPoint::new(1 * PX, (1 + 3) * PX));
        assert_eq!(f[1].GetSize().width, 8 * PX);
    }

    #[test]
    fn min_height_raises_an_auto_height_and_a_specified_height_wins() {
        // CSS 2.2 10.7: min-height is applied to the auto height. (The box is a child of the root:
        // a reflow root's own min and max sizes are ignored, as in Gecko.)
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=box style="margin:0;padding:0;border:0;min-height:50px"><div id=a style="margin:0;height:10px"></div></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("box", Some(0), K::Block), ("a", Some(1), K::Block)]);
        let m = reflow(f[0], 10);
        assert_eq!(f[1].GetSize().height, 50 * PX);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 50 * PX);

        // a specified height is not affected by the content
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=box style="margin:0;padding:0;border:0;height:5px"><div id=a style="margin:0;height:10px"></div></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("box", Some(0), K::Block), ("a", Some(1), K::Block)]);
        let m = reflow(f[0], 10);
        assert_eq!(f[1].GetSize().height, 5 * PX);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 5 * PX);
        // the child sticks out of the box: it is overflow, not size
        assert_eq!(f[2].GetSize().height, 10 * PX);

        // max-height clamps an auto height
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=box style="margin:0;padding:0;border:0;max-height:4px"><div id=a style="margin:0;height:10px"></div></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("box", Some(0), K::Block), ("a", Some(1), K::Block)]);
        reflow(f[0], 10);
        assert_eq!(f[1].GetSize().height, 4 * PX);
    }

    #[test]
    fn a_paragraph_of_words_wraps_into_lines() {
        // CSS 2.2 9.4.2: inline boxes are laid out in line boxes; when a line cannot hold the next
        // word it breaks. Four 4px words in a 10px block: two words (8px) per line; each line is
        // as high as the line-height (20px).
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:20px">
            <span id=w0></span><span id=w1></span><span id=w2></span><span id=w3></span></div>"#;
        let spec = &[("root", None, K::Block), ("w0", Some(0), K::Text(4, 16, false)), ("w1", Some(0), K::Text(4, 16, true)), ("w2", Some(0), K::Text(4, 16, true)), ("w3", Some(0), K::Text(4, 16, true))];
        let f = build(html, spec);
        let m = reflow(f[0], 10);
        assert_eq!(lines(f[0]), vec![(0, 20, false), (20, 20, false)]);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 40 * PX);
        assert_eq!(f[1].GetPosition().x, 0);
        assert_eq!(f[2].GetPosition().x, 4 * PX);
        // the second line starts with w2
        assert_eq!(f[3].GetPosition().x, 0);
        assert_eq!(f[4].GetPosition().x, 4 * PX);
        let block = nsBlockFrame::from_frame(f[0]);
        let first = block.LinesBegin();
        assert_eq!(first.GetChildCount(), 2);
        assert_eq!(first.mFirstChild, Some(f[1]));
        assert_eq!(block.LinesBegin().next_().mFirstChild, Some(f[3]));
    }

    #[test]
    fn a_short_paragraph_stays_on_one_line() {
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:20px">
            <span id=w0></span><span id=w1></span></div>"#;
        let spec = &[("root", None, K::Block), ("w0", Some(0), K::Text(4, 16, false)), ("w1", Some(0), K::Text(4, 16, true))];
        let f = build(html, spec);
        let m = reflow(f[0], 10);
        assert_eq!(lines(f[0]), vec![(0, 20, false)]);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 20 * PX);
    }

    #[test]
    fn a_left_float_makes_the_text_flow_beside_it() {
        // CSS 2.2 9.5: a left float is taken out of the flow and line boxes next to it are shortened.
        // The block is 20px wide; four 5px words would fit on one line, but the 10px float leaves
        // 10px next to it, so two words fit per line. The float is 15px high, so both lines (0-10
        // and 10-20) are next to it at their start. The words start at the float's right edge.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:10px">
            <div id=fl style="float:left;width:10px;height:15px;margin:0"></div><span id=ph></span>
            <span id=w0></span><span id=w1></span><span id=w2></span><span id=w3></span></div>"#;
        let spec = &[
            ("root", None, K::Block),
            ("fl", Some(0), K::Float),
            ("ph", Some(0), K::Placeholder(1)),
            ("w0", Some(0), K::Text(5, 8, false)),
            ("w1", Some(0), K::Text(5, 8, true)),
            ("w2", Some(0), K::Text(5, 8, true)),
            ("w3", Some(0), K::Text(5, 8, true)),
        ];
        let f = build(html, spec);
        let m = reflow(f[0], 20);
        assert_eq!(f[1].GetPosition(), nsPoint::new(0, 0));
        assert_eq!(f[1].GetSize(), nsSize::new(10 * PX, 15 * PX));
        assert_eq!(lines(f[0]), vec![(0, 10, false), (10, 10, false)]);
        assert_eq!(f[3].GetPosition().x, 10 * PX);
        assert_eq!(f[4].GetPosition().x, 15 * PX);
        assert_eq!(f[5].GetPosition().x, 10 * PX);
        assert_eq!(f[6].GetPosition().x, 15 * PX);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 20 * PX);
    }

    #[test]
    fn without_the_float_the_same_words_fit_on_one_line() {
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:10px">
            <span id=w0></span><span id=w1></span><span id=w2></span><span id=w3></span></div>"#;
        let spec = &[("root", None, K::Block), ("w0", Some(0), K::Text(5, 8, false)), ("w1", Some(0), K::Text(5, 8, true)), ("w2", Some(0), K::Text(5, 8, true)), ("w3", Some(0), K::Text(5, 8, true))];
        let f = build(html, spec);
        let m = reflow(f[0], 20);
        assert_eq!(lines(f[0]), vec![(0, 10, false)]);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 10 * PX);
    }

    #[test]
    fn a_right_float_sits_at_the_right_edge() {
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:10px">
            <div id=fl style="float:right;width:6px;height:6px;margin:0"></div><span id=ph></span><span id=w0></span></div>"#;
        let spec = &[("root", None, K::Block), ("fl", Some(0), K::Float), ("ph", Some(0), K::Placeholder(1)), ("w0", Some(0), K::Text(4, 8, false))];
        let f = build(html, spec);
        reflow(f[0], 20);
        assert_eq!(f[1].GetPosition(), nsPoint::new(14 * PX, 0));
        assert_eq!(f[3].GetPosition().x, 0);
    }

    #[test]
    fn clear_moves_a_block_below_the_float() {
        // CSS 2.2 9.5.2: clear:left puts the top border edge of the block below the bottom margin
        // edge of earlier left floats (clearance).
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=fl style="float:left;width:10px;height:30px;margin:0"></div><span id=ph></span>
            <div id=b style="clear:left;margin:0;height:10px"></div></div>"#;
        let spec = &[("root", None, K::Block), ("fl", Some(0), K::Float), ("ph", Some(0), K::Placeholder(1)), ("b", Some(0), K::Block)];
        let f = build(html, spec);
        let m = reflow(f[0], 20);
        assert_eq!(f[1].GetPosition(), nsPoint::new(0, 0));
        assert_eq!(f[3].GetPosition().y, 30 * PX);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 40 * PX);
    }

    #[test]
    fn a_block_without_clear_flows_beside_a_float_with_its_content() {
        // A float does not move a following block; the block's line boxes are shortened instead,
        // and the block itself starts at the top of the container.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=fl style="float:left;width:10px;height:30px;margin:0"></div><span id=ph></span>
            <div id=b style="margin:0;height:10px"></div></div>"#;
        let spec = &[("root", None, K::Block), ("fl", Some(0), K::Float), ("ph", Some(0), K::Placeholder(1)), ("b", Some(0), K::Block)];
        let f = build(html, spec);
        let m = reflow(f[0], 20);
        assert_eq!(f[3].GetPosition().y, 0);
        // the float extends past the end of the in-flow content: a BFC root contains its floats
        assert_eq!(m.BSize(f[0].GetWritingMode()), 30 * PX);
    }

    #[test]
    fn a_new_formatting_context_does_not_overlap_a_float() {
        // CSS 2.2 9.5: the border box of a block that establishes a new BFC must not overlap a
        // float in the same BFC. In a 20px container with a 10px wide, 30px high left float:
        //  - an auto-width flow-root block shrinks to the 10px that are left and sits at x=10;
        //  - one that is 8px wide fits beside the float, at x=10;
        //  - one that is 15px wide does not fit, so it is moved below the float (y=30).
        for (width, expect_x, expect_y, expect_w) in [("auto", 10, 0, 10), ("8px", 10, 0, 8), ("15px", 0, 30, 15)] {
            let html = format!(
                r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
                <div id=fl style="float:left;width:10px;height:30px;margin:0"></div><span id=ph></span>
                <div id=b style="display:flow-root;margin:0;padding:0;border:0;width:{width};height:5px"></div></div>"#
            );
            let spec = &[("root", None, K::Block), ("fl", Some(0), K::Float), ("ph", Some(0), K::Placeholder(1)), ("b", Some(0), K::Block)];
            let f = build(&html, spec);
            reflow(f[0], 20);
            assert_eq!(f[3].GetPosition(), nsPoint::new(expect_x * PX, expect_y * PX), "width: {width}");
            assert_eq!(f[3].GetSize().width, expect_w * PX, "width: {width}");
        }
    }

    #[test]
    fn a_float_inside_a_block_is_contained_by_its_formatting_context_root() {
        // The root is a BFC, so it grows to contain the float; a plain block (not a BFC) does not.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=p style="margin:0;padding:0;border:0"><div id=fl style="float:left;width:10px;height:30px;margin:0"></div><span id=ph></span></div></div>"#;
        let spec = &[("root", None, K::Block), ("p", Some(0), K::Block), ("fl", Some(1), K::Float), ("ph", Some(1), K::Placeholder(2))];
        let f = build(html, spec);
        let m = reflow(f[0], 20);
        // p has only a float: its own height is 0 (the float sticks out), the BFC root contains it
        assert_eq!(f[1].GetSize().height, 0);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 30 * PX);
        assert_eq!(f[2].GetPosition(), nsPoint::new(0, 0));
    }

    #[test]
    fn an_outside_marker_sits_left_of_the_first_line() {
        // CSS Lists 3: an outside ::marker is placed outside the list item's border box, next to the
        // first line box; it does not change the item's height or its content's position.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:20px">
            <div id=li style="display:list-item;margin:0;padding:0;border:0"><span id=m></span><span id=w0></span></div></div>"#;
        let spec = &[("root", None, K::Block), ("li", Some(0), K::Block), ("m", Some(1), K::Marker(8, 8)), ("w0", Some(1), K::Text(4, 16, false))];
        let f = build(html, spec);
        let m = reflow(f[0], 10);
        let li = nsBlockFrame::from_frame(f[1]);
        assert!(li.HasMarker());
        assert_eq!(li.GetOutsideMarker(), Some(f[2]));
        assert!(li.HasOutsideMarker());
        assert_eq!(m.BSize(f[0].GetWritingMode()), 20 * PX);
        assert_eq!(f[1].GetSize().height, 20 * PX);
        // outside: to the left of the item's content box, its right edge at the content edge
        assert_eq!(f[2].GetPosition().x, -8 * PX);
        assert_eq!(f[2].GetSize(), nsSize::new(8 * PX, 8 * PX));
        assert_eq!(f[3].GetPosition().x, 0);
    }

    #[test]
    fn an_item_with_a_marker_but_no_lines_still_gets_a_height() {
        // ReflowDirtyLines: "a list-item with no lines" gets the marker's height as its own.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:20px">
            <div id=li style="display:list-item;margin:0;padding:0;border:0"><span id=m></span></div></div>"#;
        let spec = &[("root", None, K::Block), ("li", Some(0), K::Block), ("m", Some(1), K::Marker(8, 8))];
        let f = build(html, spec);
        reflow(f[0], 10);
        assert!(f[1].GetSize().height > 0);
    }

    #[test]
    fn block_flags_and_is_empty() {
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=a style="margin:0;height:10px"></div><div id=b style="margin:0;padding:0;border:0"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block), ("b", Some(0), K::Block)]);
        // flow-root and the root establish BFCs, plain blocks do not
        assert!(f[0].HasAnyStateBits(NS_BLOCK_BFC));
        assert!(!f[1].HasAnyStateBits(NS_BLOCK_BFC));
        reflow(f[0], 10);
        // a block with a height is not empty, an empty one is (before and after reflow)
        assert!(!nsBlockFrame::from_frame(f[1]).IsEmpty());
        assert!(nsBlockFrame::from_frame(f[2]).IsEmpty());
        assert!(nsBlockFrame::from_frame(f[2]).CachedIsEmpty());
        assert!(!nsBlockFrame::from_frame(f[0]).IsEmpty());
        // a BFC root is never self-empty (it is more like a replaced element)
        assert!(!nsBlockFrame::from_frame(f[0]).IsSelfEmpty());
    }

    #[test]
    fn a_second_reflow_keeps_the_result() {
        // Clean lines are slid (not reflowed) and the result is the same.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=a style="margin:0 0 5px 0;height:10px"></div><div id=b style="margin:0;height:20px"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block), ("b", Some(0), K::Block)]);
        let first = reflow(f[0], 10).BSize(f[0].GetWritingMode());
        let second = reflow(f[0], 10).BSize(f[0].GetWritingMode());
        assert_eq!(first, 35 * PX);
        assert_eq!(second, first);
        assert_eq!(f[2].GetPosition().y, 15 * PX);
    }

    #[test]
    fn adding_a_frame_marks_the_lines_and_the_ancestors_dirty() {
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=a style="margin:0;height:10px"></div><div id=b style="margin:0;height:20px"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block), ("b", Some(0), K::Block)]);
        reflow(f[0], 10);
        // a new block child, appended after b
        let st = f[2].Style();
        let wm = f[2].GetWritingMode();
        let c = WithFrameTree(|t| t.AddFrame(None, nsRect::new(0, 0, 0, 0), wm, Some(st)));
        c.AddStateBits(NS_FRAME_IS_DIRTY);
        c.SetParent(f[0]);
        let root = nsBlockFrame::from_frame(f[0]);
        // (AddFrame with no parent leaves the frame unlinked; AppendFrames links it)
        let mut list = nsFrameList::default();
        list.AppendFrame(None, c);
        root.AppendFrames(FrameChildListID::Principal, list);
        assert_eq!(root.mLines().size(), 3);
        assert_eq!(root.mFrames().LastChild(), Some(c));
        assert!(root.HasAnyStateBits(NS_FRAME_HAS_DIRTY_CHILDREN));
        assert!(root.LinesBegin().next_().next_().IsDirty());
    }

    #[test]
    fn removing_a_frame_removes_its_line() {
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=a style="margin:0;height:10px"></div><div id=b style="margin:0;height:20px"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block), ("b", Some(0), K::Block)]);
        reflow(f[0], 10);
        let root = nsBlockFrame::from_frame(f[0]);
        let mut ctx = FrameDestroyContext::default();
        root.RemoveFrame(&mut ctx, FrameChildListID::Principal, f[1]);
        assert_eq!(ctx.mDestroyedFrames, vec![f[1]]);
        assert_eq!(root.mLines().size(), 1);
        assert_eq!(root.mFrames().GetLength(), 1);
        // and the next reflow moves b to the top
        let m = reflow(f[0], 10);
        assert_eq!(f[2].GetPosition().y, 0);
        assert_eq!(m.BSize(f[0].GetWritingMode()), 20 * PX);
    }

    #[test]
    fn steal_frame_takes_a_frame_out_of_its_line() {
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=a style="margin:0;height:10px"></div><div id=b style="margin:0;height:20px"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block), ("b", Some(0), K::Block)]);
        let root = nsBlockFrame::from_frame(f[0]);
        root.StealFrame(f[1]);
        assert_eq!(root.mFrames().FirstChild(), Some(f[2]));
        assert_eq!(root.mLines().size(), 1);
        assert_eq!(root.LinesBegin().mFirstChild, Some(f[2]));
    }

    #[test]
    fn child_lists_report_floats_and_the_marker() {
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=fl style="float:left;width:10px;height:5px;margin:0"></div><span id=ph></span></div>"#;
        let f = build(html, &[("root", None, K::Block), ("fl", Some(0), K::Float), ("ph", Some(0), K::Placeholder(1))]);
        let root = nsBlockFrame::from_frame(f[0]);
        assert!(root.HasFloats());
        assert!(root.MaybeHasFloats());
        assert_eq!(root.GetChildList(FrameChildListID::Float).FirstChild(), Some(f[1]));
        assert_eq!(root.GetChildList(FrameChildListID::Principal).FirstChild(), Some(f[2]));
        let mut lists = Vec::new();
        f[0].GetChildLists(&mut lists);
        let ids: Vec<FrameChildListID> = lists.iter().map(|l| l.mID).collect();
        assert_eq!(ids, vec![FrameChildListID::Principal, FrameChildListID::Float]);
    }

    #[test]
    fn block_can_intersect_floats_unless_it_is_a_bfc_or_replaced() {
        let html = r#"<div id=root style="display:flow-root"><div id=a></div><div id=b style="overflow:hidden;display:flow-root"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block), ("b", Some(0), K::Block)]);
        assert!(nsBlockFrame::BlockCanIntersectFloats(f[1]));
        assert!(!nsBlockFrame::BlockCanIntersectFloats(f[2]));
        assert!(!nsBlockFrame::BlockCanIntersectFloats(f[0]));
        assert!(nsBlockFrame::BlockNeedsFloatManager(f[0]));
        assert!(!nsBlockFrame::BlockNeedsFloatManager(f[1]));
        let (mut s, mut e) = (true, true);
        nsBlockFrame::from_frame(f[1]).IsMarginRoot(&mut s, &mut e);
        assert!(!s && !e);
        nsBlockFrame::from_frame(f[2]).IsMarginRoot(&mut s, &mut e);
        assert!(s && e);
    }

    // ---------------------------------------------------------------------------------------
    // Intrinsic inline-sizes (nsLayoutUtils::IntrinsicForAxis / IntrinsicForContainer,
    // nsBlockFrame::MinISize / PrefISize) and the shrink-to-fit sizes that need them. Gecko has no
    // gtest for these; the tests are written from CSS 2.2 10.3.5 (floats) and 10.3.9 (inline-block):
    // the used width is min(max(preferred minimum width, available width), preferred width), and
    // from CSS Sizing 3 (min-content and max-content contributions).
    // ---------------------------------------------------------------------------------------

    fn intrinsic_input() -> IntrinsicSizeInput {
        IntrinsicSizeInput::new(Some(gfxContext), None, None)
    }

    // The (min-content, max-content) inline-size of the content box of `f`, in px.
    fn min_pref(f: nsIFrame) -> (i32, i32) {
        let input = intrinsic_input();
        (f.GetMinISize(&input) / PX, f.GetPrefISize(&input) / PX)
    }

    // The (min, pref) contribution of `f` to the inline-size of its parent: the content size with
    // the width, min-width and max-width properties applied and padding, border and margin added.
    fn contributions(f: nsIFrame) -> (i32, i32) {
        let c = |t| nsLayoutUtils::IntrinsicForContainer(Some(gfxContext), f, t, None, 0, &StyleSizeOverrides::default()) / PX;
        (c(IntrinsicISizeType::MinISize), c(IntrinsicISizeType::PrefISize))
    }

    // A flow-root `root` with a float `fl` (style given) that has the `kids` (words and blocks).
    fn float_with(float_style: &str, kids: &[(&str, K)], kid_markup: &str) -> Vec<nsIFrame> {
        let html = format!(
            r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:10px">
            <div id=fl style="float:left;{float_style}">{kid_markup}</div><span id=ph></span></div>"#
        );
        let mut spec: Vec<(&str, Option<usize>, K)> = vec![("root", None, K::Block), ("fl", Some(0), K::Float), ("ph", Some(0), K::Placeholder(1))];
        for (id, k) in kids {
            spec.push((id, Some(1), *k));
        }
        build(&html, &spec)
    }

    const THREE_WORDS: &str = r#"<span id=w0></span><span id=w1></span><span id=w2></span>"#;

    fn three_words() -> [(&'static str, K); 3] {
        [("w0", K::Text(30, 8, false)), ("w1", K::Text(40, 8, true)), ("w2", K::Text(20, 8, true))]
    }

    #[test]
    fn the_intrinsic_sizes_of_a_paragraph_are_the_widest_word_and_the_whole_line() {
        // CSS Sizing 3: min-content is the longest unbreakable run (40px), max-content is all the
        // words on one line (30 + 40 + 20 = 90px).
        let f = float_with("margin:0;", &three_words(), THREE_WORDS);
        assert_eq!(min_pref(f[1]), (40, 90));
    }

    #[test]
    fn an_empty_block_has_zero_intrinsic_sizes() {
        let f = float_with("margin:0;", &[], "");
        assert_eq!(min_pref(f[1]), (0, 0));
        assert_eq!(contributions(f[1]), (0, 0));
    }

    #[test]
    fn a_float_with_auto_width_shrinks_to_fit_its_preferred_width() {
        // CSS 2.2 10.3.5: the used width is min(max(preferred minimum width, available width),
        // preferred width). Available 200: the preferred width 90 fits.
        let f = float_with("margin:0;height:10px", &three_words(), THREE_WORDS);
        reflow(f[0], 200);
        assert_eq!(f[1].GetSize().width, 90 * PX);
        // the words are on one line
        assert_eq!(lines(f[1]), vec![(0, 10, false)]);
    }

    #[test]
    fn a_float_with_auto_width_uses_the_available_width_between_min_and_pref() {
        // Available 60: more than the minimum (40), less than the preferred width (90): the float is
        // 60 wide and the words wrap (30 + 40 do not fit on one 60px line, 40 + 20 do).
        let f = float_with("margin:0;", &three_words(), THREE_WORDS);
        reflow(f[0], 60);
        assert_eq!(f[1].GetSize().width, 60 * PX);
        assert_eq!(lines(f[1]).len(), 2);
    }

    #[test]
    fn a_float_with_auto_width_is_not_narrower_than_its_min_content() {
        // Available 30 is less than the widest word (40): the float takes 40.
        let f = float_with("margin:0;", &three_words(), THREE_WORDS);
        reflow(f[0], 30);
        assert_eq!(f[1].GetSize().width, 40 * PX);
    }

    #[test]
    fn a_float_with_block_children_of_explicit_widths_is_as_wide_as_the_widest() {
        // Block children contribute their width (plus their own padding, border and margin).
        let kids = [("a", K::Block), ("b", K::Block)];
        let markup = r#"<div id=a style="margin:0;padding:0;border:0;width:60px;height:5px"></div><div id=b style="margin:0;padding:0;border:0;width:80px;height:5px"></div>"#;
        let f = float_with("margin:0;", &kids, markup);
        assert_eq!(min_pref(f[1]), (80, 80));
        reflow(f[0], 200);
        assert_eq!(f[1].GetSize().width, 80 * PX);
        // both children are block-level boxes inside, each at its own width
        assert_eq!(f[3].GetSize().width, 60 * PX);
        assert_eq!(f[4].GetSize().width, 80 * PX);
        // Narrower than the children: the minimum wins.
        let f = float_with("margin:0;", &kids, markup);
        reflow(f[0], 50);
        assert_eq!(f[1].GetSize().width, 80 * PX);
    }

    #[test]
    fn padding_border_and_margin_of_nested_blocks_are_added_to_the_contributions() {
        // The child's width:50px is its content width, so it contributes 50 + padding 2*4 +
        // border 2*1 + margin 2*3 = 66px. The float's own padding, border and margin are not in its
        // content size, but they are in its contribution to the root: 66 + 2*5 + 2*2 + 2*7 = 94px.
        let kids = [("a", K::Block)];
        let markup = r#"<div id=a style="margin:0 3px;padding:0 4px;border:1px solid;width:50px;height:5px;box-sizing:content-box"></div>"#;
        let f = float_with("margin:0 7px;padding:0 5px;border:2px solid;box-sizing:content-box", &kids, markup);
        assert_eq!(contributions(f[3]), (66, 66));
        assert_eq!(min_pref(f[1]), (66, 66));
        assert_eq!(contributions(f[1]), (94, 94));
        // The float, shrink-wrapped, is 66px of content in a 94px margin box.
        reflow(f[0], 200);
        let wm = f[1].GetWritingMode();
        assert_eq!(f[1].GetSize().width, (66 + 10 + 4) * PX);
        assert_eq!(f[1].ContentSize(wm).ISize(wm), 66 * PX);
    }

    #[test]
    fn nested_blocks_with_words_add_their_padding_to_min_and_pref() {
        // A block with padding 5px on both sides around the words: 40 + 10 = 50 min, 90 + 10 pref.
        let html = format!(
            r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:10px">
            <div id=fl style="float:left;margin:0"><div id=in style="margin:0;padding:0 5px;border:0">{THREE_WORDS}</div></div><span id=ph></span></div>"#
        );
        let spec = vec![
            ("root", None, K::Block),
            ("fl", Some(0), K::Float),
            ("ph", Some(0), K::Placeholder(1)),
            ("in", Some(1), K::Block),
            ("w0", Some(3), K::Text(30, 8, false)),
            ("w1", Some(3), K::Text(40, 8, true)),
            ("w2", Some(3), K::Text(20, 8, true)),
        ];
        let f = build(&html, &spec);
        assert_eq!(contributions(f[3]), (50, 100));
        assert_eq!(min_pref(f[1]), (50, 100));
        reflow(f[0], 500);
        assert_eq!(f[1].GetSize().width, 100 * PX);
    }

    #[test]
    fn width_applies_to_the_contribution_and_box_sizing_changes_what_it_includes() {
        // 'width' replaces the content size: with content-box the padding and border come on top
        // (100 + 2*10 + 2*2 = 124), with border-box they are inside (100). Margins are always added.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=a style="margin:0 6px;padding:0 10px;border:2px solid;width:100px;box-sizing:content-box"></div>
            <div id=b style="margin:0 6px;padding:0 10px;border:2px solid;width:100px;box-sizing:border-box"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block), ("b", Some(0), K::Block)]);
        assert_eq!(contributions(f[1]), (124 + 12, 124 + 12));
        assert_eq!(contributions(f[2]), (100 + 12, 100 + 12));
        // a border-box width that is smaller than padding + border is floored at padding + border
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0">
            <div id=a style="margin:0;padding:0 10px;border:2px solid;width:5px;box-sizing:border-box"></div></div>"#;
        let f = build(html, &[("root", None, K::Block), ("a", Some(0), K::Block)]);
        assert_eq!(contributions(f[1]), (24, 24));
    }

    #[test]
    fn min_width_and_max_width_clamp_the_contribution() {
        // Content: min 40, pref 90 (three words).
        let kids = three_words();
        let case = |style: &str| -> (i32, i32) {
            let f = float_with(&format!("margin:0;{style}"), &kids, THREE_WORDS);
            contributions(f[1])
        };
        assert_eq!(case(""), (40, 90));
        // min-width raises both
        assert_eq!(case("min-width:120px"), (120, 120));
        assert_eq!(case("min-width:60px"), (60, 90));
        // max-width lowers both
        assert_eq!(case("max-width:30px"), (30, 30));
        assert_eq!(case("max-width:70px"), (40, 70));
        // min-width wins over max-width (CSS 2.2 10.4)
        assert_eq!(case("min-width:80px;max-width:30px"), (80, 80));
        // a definite width is clamped as well
        assert_eq!(case("width:200px;max-width:50px"), (50, 50));
        assert_eq!(case("width:10px;min-width:50px"), (50, 50));
        // with border-box the limits include padding and border
        assert_eq!(case("box-sizing:border-box;padding:0 5px;border:0;max-width:30px"), (30, 30));
        assert_eq!(case("padding:0 5px;max-width:30px"), (40, 40));
    }

    #[test]
    fn a_percentage_width_is_treated_as_auto_for_a_block_and_as_zero_for_the_min_of_a_replaced_box() {
        // CSS Sizing 3 5.2.1: the min-content contribution of a replaced box with a percentage
        // width is zero (the percentage is cyclic); a non-replaced block ignores the percentage
        // and contributes its content, as if its width were auto.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:10px">
            <div id=b style="margin:0;padding:0 5px;border:0;width:50%"><span id=w0></span><span id=w1></span></div>
            <div id=img style="margin:0;padding:0 5px;border:0;width:50%"></div></div>"#;
        let spec = &[("root", None, K::Block), ("b", Some(0), K::Block), ("w0", Some(1), K::Text(30, 8, false)), ("w1", Some(1), K::Text(40, 8, true)), ("img", Some(0), K::Block)];
        let f = build(html, spec);
        // the block: 10 padding + content (min 40, pref 70)
        assert_eq!(contributions(f[1]), (50, 80));
        // the image: replaced, intrinsic width 100 (given as stand-in), its min-contribution is just
        // the padding, TEMP: the type and the sizes are stand-ins for nsImageFrame
        f[4].SetType(LayoutFrameType::Image);
        f[4].SetIntrinsicISizes(100 * PX, 100 * PX);
        assert_eq!(contributions(f[4]), (10, 110));
    }

    #[test]
    fn the_aspect_ratio_gives_the_inline_size_from_a_definite_height() {
        // aspect-ratio:2/1 with height:50px: the inline-size is 100px, even when the content is empty.
        let f = float_with("margin:0;height:50px;aspect-ratio:2/1", &[], "");
        assert_eq!(contributions(f[1]), (100, 100));
        reflow(f[0], 500);
        assert_eq!(f[1].GetSize(), nsSize::new(100 * PX, 50 * PX));
        // max-height limits it through the ratio
        let f = float_with("margin:0;height:50px;max-height:20px;aspect-ratio:2/1", &[], "");
        assert_eq!(contributions(f[1]), (40, 40));
    }

    #[test]
    fn two_floats_in_a_row_each_shrink_to_their_own_content() {
        // Each float is as wide as its own preferred width; the second sits right of the first.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:10px">
            <div id=f1 style="float:left;margin:0"><span id=a></span></div><span id=p1></span>
            <div id=f2 style="float:left;margin:0"><span id=b></span></div><span id=p2></span></div>"#;
        let spec = &[
            ("root", None, K::Block),
            ("f1", Some(0), K::Float),
            ("p1", Some(0), K::Placeholder(1)),
            ("f2", Some(0), K::Float),
            ("p2", Some(0), K::Placeholder(3)),
            ("a", Some(1), K::Text(25, 8, false)),
            ("b", Some(3), K::Text(35, 8, false)),
        ];
        let f = build(html, spec);
        reflow(f[0], 200);
        assert_eq!(f[1].GetSize().width, 25 * PX);
        assert_eq!(f[3].GetSize().width, 35 * PX);
        assert_eq!(f[1].GetPosition().x, 0);
        assert_eq!(f[3].GetPosition().x, 25 * PX);
    }

    #[test]
    fn an_inline_block_with_auto_width_shrinks_to_fit() {
        // CSS 2.2 10.3.9: an inline-block with width:auto uses the shrink-to-fit width, like a float.
        // The inline-block is a child of the line, between two words.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:10px">
            <span id=pre></span><div id=ib style="display:inline-block;margin:0;padding:0 3px;border:0"><span id=w0></span><span id=w1></span></div><span id=post></span></div>"#;
        let spec = &[
            ("root", None, K::Block),
            ("pre", Some(0), K::Text(10, 8, false)),
            ("ib", Some(0), K::Block),
            ("post", Some(0), K::Text(10, 8, true)),
            ("w0", Some(2), K::Text(30, 8, false)),
            ("w1", Some(2), K::Text(40, 8, true)),
        ];
        let f = build(html, spec);
        // the box: 70 of words + 6 of padding
        assert_eq!(contributions(f[2]), (46, 76));
        // wide enough for everything on one line
        reflow(f[0], 500);
        assert_eq!(f[2].GetSize().width, 76 * PX);
        assert_eq!(f[2].GetPosition().x, 10 * PX);
        assert_eq!(f[3].GetPosition().x, 86 * PX);
        // CSS 2.2 10.3.9: the available width is the containing block's width (60), not what is
        // left on the line: min(max(40, 60 - 6), 70) = 54 of content, 60 with the padding. It does
        // not fit next to the first word (10), so it goes to the next line, where it is 60 wide,
        // and the last word goes to the line after it.
        let f = build(html, spec);
        reflow(f[0], 60);
        assert_eq!(f[2].GetSize().width, 60 * PX);
        assert_eq!(f[2].GetPosition(), nsPoint::new(0, 10 * PX));
        assert_eq!(f[3].GetPosition().x, 0);
        assert_eq!(lines(f[0]).len(), 3);
        // The two words of the box (30 and 40) do not fit in 54, so they are on two lines.
        assert_eq!(lines(f[2]).len(), 2);
        assert_eq!(f[2].GetSize().height, 20 * PX);
        // Available 30 is less than the minimum 40 + 6 of the box: it takes the minimum.
        let f = build(html, spec);
        reflow(f[0], 30);
        assert_eq!(f[2].GetSize().width, 46 * PX);
    }

    #[test]
    fn the_contribution_of_an_inline_block_counts_in_the_min_and_max_of_its_container() {
        // The container's min-content is the widest of the parts (an inline-block is unbreakable
        // from the outside, but its contribution is its min-content, 46 + margins 4); its
        // max-content is the sum of the parts on the line: 10 + 80 + 10.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;line-height:10px">
            <span id=pre></span><div id=ib style="display:inline-block;margin:0 2px;padding:0 3px;border:0"><span id=w0></span><span id=w1></span></div><span id=post></span></div>"#;
        let spec = &[
            ("root", None, K::Block),
            ("pre", Some(0), K::Text(10, 8, false)),
            ("ib", Some(0), K::Block),
            ("post", Some(0), K::Text(10, 8, false)),
            ("w0", Some(2), K::Text(30, 8, false)),
            ("w1", Some(2), K::Text(40, 8, true)),
        ];
        let f = build(html, &spec[..]);
        assert_eq!(min_pref(f[0]), (50, 10 + 80 + 10));
    }
}
