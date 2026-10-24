/* rendering object for CSS display:inline objects */
// Translated from layout/generic/nsInlineFrame.h and nsInlineFrame.cpp (GECKO_REV).
//
// nsInlineFrame and nsFirstLineFrame are handles (`nsInlineFrame(nsIFrame)`) that Deref to the base
// class's handle, like every class here (CONVENTIONS.md). What is virtual in C++ is a `match` on the
// frame type: `nsIFrame::Reflow` (nsLineLayoutAdapter.rs) calls `nsInlineFrame::Reflow` for the
// types Inline and Line, which picks the nsFirstLineFrame override for Line.
//
// NOT PORTED: painting (BuildDisplayList), the caret (GetCaretBaseline), selection movement
// (PeekOffsetCharacter), invalidation (InvalidateFrame*, SVG text), accessibility, attribute
// changes, tight bounds, ::first-line style repair (RestyleManager::ReparentComputedStyleForFirstLine,
// UpdateStyleOfOwnedAnonBoxesForIBSplit, nsFirstLineFrame::Init), {ib} splits, bidi, the abspos
// descendant marking. Marked where they would be.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

// Gecko: layout/generic/nsInlineFrame.h nsInlineFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsInlineFrame(pub nsIFrame);

impl Deref for nsInlineFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/generic/nsInlineFrame.h nsFirstLineFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsFirstLineFrame(pub nsInlineFrame);

impl Deref for nsFirstLineFrame {
    type Target = nsInlineFrame;
    fn deref(&self) -> &nsInlineFrame {
        &self.0
    }
}


impl nsInlineFrame {
    // static_cast<nsInlineFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsInlineFrame {
        nsInlineFrame(aFrame)
    }

    // do_QueryFrame<nsInlineFrame>(frame): the types Inline and Line (nsFirstLineFrame is a subclass).
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsInlineFrame> {
        if matches!(aFrame.Type(), LayoutFrameType::Inline | LayoutFrameType::Line) {
            Some(nsInlineFrame(aFrame))
        } else {
            None
        }
    }

    // Gecko: layout/generic/nsInlineFrame.h nsInlineFrame::mBaseline (initially NS_INTRINSIC_ISIZE_UNKNOWN)
    pub fn mBaseline(&self) -> *mut nscoord {
        // ADAPTER: nsLineLayout keeps this pointer for the whole line (BeginSpan), and new frames are made while
        // it is live (continuations): the frame data does not move (see FrameStore), so it stays valid.
        WithFrameTree(|t| &mut t.mFrames[(self.0).0 as usize].mBaseline as *mut nscoord)
    }
}

// Gecko: layout/generic/nsInlineFrame.cpp NS_NewInlineFrame
// ADAPTER: frames are made by whoever builds the frame tree (the frame constructor, a test) with
// FrameTree::AddFrame; this gives one the class.
pub fn NS_NewInlineFrame(aFrame: nsIFrame) -> nsInlineFrame {
    aFrame.SetType(LayoutFrameType::Inline);
    nsInlineFrame(aFrame)
}

// Gecko: layout/generic/nsInlineFrame.cpp NS_NewFirstLineFrame
pub fn NS_NewFirstLineFrame(aFrame: nsIFrame) -> nsFirstLineFrame {
    aFrame.SetType(LayoutFrameType::Line);
    nsFirstLineFrame(nsInlineFrame(aFrame))
}

// Gecko: layout/generic/nsInlineFrame.h nsInlineFrame::SetParentDuringReflow
// Used to defer setting the parent pointer of child frames pulled from a
// prev-in-flow's overflow list until they are reflowed. See
// InlineReflowInput::mSetParentDuringReflow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsInlineFrame_SetParentDuringReflow {
    No,
    Yes,
}

// Additional reflow input used during our reflow methods
// Gecko: layout/generic/nsInlineFrame.h nsInlineFrame::InlineReflowInput
pub struct nsInlineFrame_InlineReflowInput {
    pub mPrevFrame: Option<nsIFrame>,
    pub mNextInFlow: Option<nsInlineFrame>,
    pub mLineContainer: Option<nsIFrame>,
    pub mLineLayout: *mut nsLineLayout,

    // Yes if we should set each child frame's parent pointer when reflowing it,
    // having deferred that when we pulled the frames from our prev-in-flow's
    // overflow list. See nsInlineFrame::Reflow().
    pub mSetParentDuringReflow: nsInlineFrame_SetParentDuringReflow,
}

impl nsInlineFrame_InlineReflowInput {
    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::InlineReflowInput::InlineReflowInput
    pub fn new(aReflowInput: &ReflowInput, aSetParentDuringReflow: nsInlineFrame_SetParentDuringReflow) -> nsInlineFrame_InlineReflowInput {
        let lineLayout = aReflowInput.mLineLayout;
        nsInlineFrame_InlineReflowInput {
            mPrevFrame: None,
            mNextInFlow: aReflowInput.mFrame.GetNextInFlow().map(nsInlineFrame),
            mLineContainer: Some(unsafe { &*lineLayout }.LineContainerFrame()),
            mLineLayout: lineLayout,
            mSetParentDuringReflow: aSetParentDuringReflow,
        }
    }
}

impl nsInlineFrame {
    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::InvalidateFrame and InvalidateFrameWithRect
    // NOT PORTED: invalidation (display lists).

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::IsSelfEmpty
    /* virtual */
    pub fn IsSelfEmpty(&self) -> bool {
        // #if 0
        //   // I used to think inline frames worked this way, but it seems they
        //   // don't.  At least not in our codebase.
        //   if (GetPresContext()->CompatibilityMode() == eCompatibility_FullStandards) {
        //     return false;
        //   }
        // #endif
        let margin = self.StyleMargin();
        let border = self.StyleBorder();
        let padding = self.StylePadding();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.0, std::ptr::null_mut());
        // Block-start and -end ignored, since they shouldn't affect things, but this
        // doesn't really match with nsLineLayout.cpp's setting of
        // ZeroEffectiveSpanBox, anymore, so what should this really be?
        let wm = self.GetWritingMode();
        let mut haveStart: bool;
        let mut haveEnd: bool;

        let IsMarginZero = |aStyleMargin: &nsStyleMargin, aSide: Side, aParams: &AnchorPosResolutionParams| -> bool {
            let margin = aStyleMargin.GetMargin__Side_AnchorPosResolutionParams(aSide, aParams);
            if !margin.IsLengthPercentage() {
                return true;
            }
            let lp = margin.AsLengthPercentage();
            lp.Resolve(nscoord_MAX) == 0 && lp.Resolve(0) == 0
        };

        let HaveSide = |aSide: Side| -> bool {
            border.GetComputedBorder().Side(aSide) != 0 || !nsLayoutUtils::IsPaddingZero(&padding.mPadding().Get__Side(aSide)) || !IsMarginZero(&margin, aSide, &anchorResolutionParams)
        };
        // Initially set up haveStart and haveEnd in terms of visual (LTR/TTB)
        // coordinates; we'll exchange them later if bidi-RTL is in effect to
        // get logical start and end flags.
        if wm.IsVertical() {
            haveStart = HaveSide(eSideTop);
            haveEnd = HaveSide(eSideBottom);
        } else {
            haveStart = HaveSide(eSideLeft);
            haveEnd = HaveSide(eSideRight);
        }
        if haveStart || haveEnd {
            // We skip this block and return false for box-decoration-break:clone since
            // in that case all the continuations will have the border/padding/margin.
            if self.HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT) && self.StyleBorder().mBoxDecorationBreak() == StyleBoxDecorationBreak::Slice {
                // When direction=rtl, we need to consider logical rather than visual
                // start and end, so swap the flags.
                if wm.IsBidiRTL() {
                    std::mem::swap(&mut haveStart, &mut haveEnd);
                }
                // For ib-split frames, ignore things we know we'll skip in GetSkipSides.
                // XXXbz should we be doing this for non-ib-split frames too, in a more
                // general way?

                // Get the first continuation eagerly, as a performance optimization, to
                // avoid having to get it twice..
                let firstCont = self.FirstContinuation();
                return (!haveStart || firstCont.FrameIsNonFirstInIBSplit()) && (!haveEnd || firstCont.FrameIsNonLastInIBSplit());
            }
            return false;
        }
        true
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        if !self.IsSelfEmpty() {
            return false;
        }

        for kid in &*self.mFrames() {
            if !kid.IsEmpty() {
                return false;
            }
        }

        true
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::GetCaretBaseline
    // NOT PORTED: the caret.

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::PeekOffsetCharacter
    // NOT PORTED: selection movement.

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::Destroy
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        if let Some(overflowFrames) = self.GetOverflowFrames() {
            // Fixup the parent pointers for any child frames on the OverflowList.
            // nsIFrame::DestroyFrom depends on that to find the sticky scroll
            // container (an ancestor).
            overflowFrames.ApplySetParent(self.0);
        }
        self.nsContainerFrame_Destroy(aContext);
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::StealFrame
    pub fn StealFrame(&self, aChild: nsIFrame) {
        if self.MaybeStealOverflowContainerFrame(aChild) {
            return;
        }

        let mut parent = Some(self.0);
        while let Some(p) = parent {
            if p.mFrames().StartRemoveFrame(aChild) {
                return;
            }

            // We didn't find the child in our principal child list.
            // Maybe it's on the overflow list?
            if let Some(frameList) = p.GetOverflowFrames() {
                if frameList.ContinueRemoveFrame(aChild) {
                    if frameList.IsEmpty() {
                        p.DestroyOverflowList();
                    }
                    return;
                }
            }

            // Due to our "lazy reparenting" optimization 'aChild' might not actually
            // be on any of our child lists, but instead in one of our next-in-flows.
            parent = p.GetNextInFlow();
        }

        MOZ_ASSERT_UNREACHABLE!("nsInlineFrame::StealFrame: can't find aChild");
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::BuildDisplayList
    // NOT PORTED: painting.

    //////////////////////////////////////////////////////////////////////
    // Reflow methods

    /* virtual */
    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::AddInlineMinISize
    pub fn AddInlineMinISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlineMinISizeData) {
        self.DoInlineMinISize(aInput, aData);
    }

    /* virtual */
    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::AddInlinePrefISize
    pub fn AddInlinePrefISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlinePrefISizeData) {
        self.DoInlinePrefISize(aInput, aData);
    }

    /* virtual */
    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::ComputeSize
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
        // Inlines and text don't compute size before reflow.
        nsIFrame_SizeComputationResult { mLogicalSize: LogicalSize::new(aWM, NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE), mAspectRatioUsage: nsIFrame_AspectRatioUsage::None }
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::ComputeTightBounds
    // NOT PORTED: tight bounds (painting).
}

// Gecko: layout/generic/nsInlineFrame.cpp ReparentChildListStyle
// NOT PORTED: ::first-line style repair. The restyle manager reparents each child's ComputedStyle
// (RestyleManager::ReparentComputedStyleForFirstLine) and the descendants are marked dirty
// (nsLayoutUtils::MarkDescendantsDirty); frames only get here under a ::first-line frame, which
// the frame constructor does not build yet.
fn ReparentChildListStyle(aPresContext: &nsPresContext, aFrames: nsFrameList_Slice, aParentFrame: nsIFrame) {
    for f in aFrames {
        NS_ASSERTION!(f.GetParent() == Some(aParentFrame), "Bogus parentage");
        unimplemented!("NOT PORTED: ReparentComputedStyleForFirstLine (::first-line)");
    }
}

// Gecko: layout/generic/nsInlineFrame.cpp the call sites of ReparentComputedStyleForFirstLine +
// nsLayoutUtils::MarkDescendantsDirty
fn ReparentComputedStyleForFirstLineAndMarkDirty(aFrame: nsIFrame) {
    unimplemented!("NOT PORTED: ReparentComputedStyleForFirstLine (::first-line)")
}

impl nsInlineFrame {
    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::Reflow
    // (virtual; nsFirstLineFrame overrides it)
    pub fn Reflow(&self, aPresContext: &nsPresContext, aReflowOutput: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        if self.Type() == LayoutFrameType::Line {
            return nsFirstLineFrame(*self).Reflow(aPresContext, aReflowOutput, aReflowInput, aStatus);
        }
        self.nsInlineFrame_Reflow(aPresContext, aReflowOutput, aReflowInput, aStatus)
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::Reflow
    pub fn nsInlineFrame_Reflow(&self, aPresContext: &nsPresContext, aReflowOutput: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        // DO_GLOBAL_REFLOW_COUNT("nsInlineFrame");  NOT PORTED: reflow counts.
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        if aReflowInput.mLineLayout.is_null() {
            NS_ASSERTION!(false, "must have non-null aReflowInput.mLineLayout");
            return;
        }
        if self.IsFrameTreeTooDeep(aReflowInput, aReflowOutput, aStatus) {
            return;
        }
        let lineLayout = unsafe { &mut *aReflowInput.mLineLayout };

        let mut setParentDuringReflow = nsInlineFrame_SetParentDuringReflow::No;

        // Check for an overflow list with our prev-in-flow
        let prevInFlow = self.GetPrevInFlow();
        if let Some(prevInFlow) = prevInFlow {
            let mut prevOverflowFrames = AutoFrameListPtr::new(prevInFlow.StealOverflowFrames());
            if let Some(prevOverflow) = prevOverflowFrames.operator_arrow_mut() {
                // Check if we can defer setting the parent until reflow (optimization).
                // Only do it in simple cases where we're being reflowed for the
                // first time, nothing (e.g. bidi resolution) has already given
                // us children, and there's no next-in-flow, so all our frames
                // will be taken from prevOverflowFrames.
                if self.HasAnyStateBits(NS_FRAME_FIRST_REFLOW) && self.mFrames().IsEmpty() && self.GetNextInFlow().is_none() {
                    // If our child list is empty, just put the new frames into it.
                    // Note that we don't set the parent pointer for the new frames. Instead
                    // wait to do this until we actually reflow the frame. If the overflow
                    // list contains thousands of frames this is a big performance issue
                    // (see bug #5588)
                    self.mFrames().operator_assign_move(prevOverflow);
                    setParentDuringReflow = nsInlineFrame_SetParentDuringReflow::Yes;
                } else {
                    // Insert the new frames at the beginning of the child list
                    // and set their parent pointer
                    let newFrames = self.mFrames().InsertFrames(Some(self.0), None, std::mem::take(prevOverflow));
                    // If our prev in flow was under the first continuation of a first-line
                    // frame then we need to reparent the ComputedStyles to remove the
                    // special first-line styling. In the deferred case, we reparent
                    // the ComputedStyles when we set their parents in
                    // nsInlineFrame::ReflowFrames and nsInlineFrame::ReflowInlineFrame.
                    if lineLayout.GetInFirstLine() {
                        ReparentChildListStyle(aPresContext, newFrames, self.0);
                    }
                }
            }
        }

        // It's also possible that we have an overflow list for ourselves
        #[cfg(debug_assertions)]
        {
            if self.HasAnyStateBits(NS_FRAME_FIRST_REFLOW) {
                // If it's our initial reflow, then we should not have an overflow list.
                // However, add an assertion in case we get reflowed more than once with
                // the initial reflow reason
                let overflowFrames = self.GetOverflowFrames();
                NS_ASSERTION!(overflowFrames.as_ref().map_or(true, |l| l.IsEmpty()), "overflow list is not empty for initial reflow");
            }
        }
        if !self.HasAnyStateBits(NS_FRAME_FIRST_REFLOW) {
            self.DrainSelfOverflowListInternal(lineLayout.GetInFirstLine());
        }

        // Set our own reflow input (additional state above and beyond aReflowInput).
        let mut irs = nsInlineFrame_InlineReflowInput::new(aReflowInput, setParentDuringReflow);

        if self.mFrames().IsEmpty() {
            // Try to pull over one frame before starting so that we know
            // whether we have an anonymous block or not.
            let _ = self.PullOneFrame(aPresContext, &mut irs);
        }

        self.ReflowFrames(aPresContext, aReflowInput, &mut irs, aReflowOutput, aStatus);

        // if (!StaticPrefs::layout_abspos_fragment_aware_inline_cb_enabled()) {
        //   // This is the legacy, spec-incompatible behavior to reflow abspos children
        //   // using only the first inline fragment's rect.
        //   ReflowAbsoluteFrames(aPresContext, aReflowOutput, aReflowInput, aStatus);
        // } else {
        // (the pref is true in Firefox 157, the other branch is not taken)
        self.MarkBlockAncestorHavingAbsoluteDescendants(aReflowInput);
        // }

        // Note: the line layout code will properly compute our
        // overflow-rect state for us.
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::MarkBlockAncestorHavingAbsoluteDescendants
    pub fn MarkBlockAncestorHavingAbsoluteDescendants(&self, aReflowInput: &ReflowInput) {
        if !self.HasAbsolutelyPositionedChildren() {
            return;
        }

        // The line container is usually the block ancestor that will run
        // ReflowAbsoluteDescendantsInInlineFrame(). Ruby is the exception, whose
        // content is reflowed in a nested line layout with nsRubyTextContainerFrame
        // as the line container. In that case, keep finding the nearest block
        // ancestor.
        let lineContainer = unsafe { &*aReflowInput.mLineLayout }.LineContainerFrame();
        let mut block = nsBlockFrame::do_QueryFrame(lineContainer);
        if block.is_none() {
            block = nsLayoutUtils::FindNearestBlockAncestor(lineContainer);
        }
        MOZ_ASSERT!(block.is_some(), "An inline absolute containing block must have a block ancestor!");
        block.unwrap().AddStateBits(NS_BLOCK_HAS_INLINE_ABSPOS_DESCENDANT);
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::AttributeChanged
    // NOT PORTED: attribute changes (SVG text).

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::DrainSelfOverflowListInternal
    /**
     * Move any frames on our overflow list to the end of our principal list.
     * @param aInFirstLine whether we're in a first-line frame.
     * @return true if there were any overflow frames
     */
    pub fn DrainSelfOverflowListInternal(&self, aInFirstLine: bool) -> bool {
        let mut overflowFrames = AutoFrameListPtr::new(self.StealOverflowFrames());
        let Some(list) = overflowFrames.operator_arrow_mut() else {
            return false;
        };
        if list.IsEmpty() {
            return false;
        }

        // The frames on our own overflowlist may have been pushed by a previous
        // reflow that deferred setting their parent, so we need to ensure the
        // correct parent pointer.  This is sometimes skipped by Reflow.
        let firstChild = list.FirstChild();
        let mut f = firstChild;
        while let Some(fr) = f {
            fr.SetParent(self.0);
            if aInFirstLine {
                ReparentComputedStyleForFirstLineAndMarkDirty(fr);
            }
            f = fr.GetNextSibling();
        }
        self.mFrames().AppendFrames(None, std::mem::take(list));
        true
    }

    /* virtual */
    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::DrainSelfOverflowList
    // (virtual; nsFirstLineFrame overrides it)
    pub fn DrainSelfOverflowList(&self) -> bool {
        if self.Type() == LayoutFrameType::Line {
            return nsFirstLineFrame(*self).DrainSelfOverflowList();
        }
        let lineContainer = nsLayoutUtils::FindNearestBlockAncestor(self.0).map(|b| b.0);
        // Add the eInFirstLine flag if we have a ::first-line ancestor frame.
        // No need to look further than the nearest line container though.
        let mut inFirstLine = false;
        let mut p = self.GetParent();
        while p != lineContainer {
            let Some(pf) = p else { break };
            if pf.IsLineFrame() {
                inFirstLine = true;
                break;
            }
            p = pf.GetParent();
        }
        self.DrainSelfOverflowListInternal(inFirstLine)
    }

    /* virtual */
    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::CanContinueTextRun
    pub fn CanContinueTextRun(&self) -> bool {
        // We can continue a text run through an inline frame
        true
    }

    /* virtual */
    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::PullOverflowsFromPrevInFlow
    // (virtual; nsFirstLineFrame overrides it)
    pub fn PullOverflowsFromPrevInFlow(&self) {
        if self.Type() == LayoutFrameType::Line {
            return nsFirstLineFrame(*self).PullOverflowsFromPrevInFlow();
        }
        if let Some(prevInFlow) = self.GetPrevInFlow() {
            let mut prevOverflowFrames = AutoFrameListPtr::new(prevInFlow.StealOverflowFrames());
            if let Some(list) = prevOverflowFrames.operator_arrow_mut() {
                self.mFrames().InsertFrames(Some(self.0), None, std::mem::take(list));
            }
        }
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::ReflowFrames
    pub fn ReflowFrames(&self, aPresContext: &nsPresContext, aReflowInput: &ReflowInput, irs: &mut nsInlineFrame_InlineReflowInput, aReflowOutput: &mut ReflowOutput, aStatus: &mut nsReflowStatus) {
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        let lineLayout = unsafe { &mut *aReflowInput.mLineLayout };
        let inFirstLine = lineLayout.GetInFirstLine();
        let frameWM = aReflowInput.GetWritingMode();
        let lineWM = lineLayout.GetWritingMode();
        let framePadding = aReflowInput.ComputedLogicalBorderPadding(frameWM);
        let mut startEdge: nscoord = 0;
        let boxDecorationBreakClone = self.StyleBorder().mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone;
        // Don't offset by our start borderpadding if we have a prev continuation or
        // if we're in a part of an {ib} split other than the first one. For
        // box-decoration-break:clone we always offset our start since all
        // continuations have border/padding.
        if (self.GetPrevContinuation().is_none() && !self.FrameIsNonFirstInIBSplit()) || boxDecorationBreakClone {
            startEdge = framePadding.IStart(frameWM);
        }
        let mut availableISize = aReflowInput.AvailableISize();
        NS_ASSERTION!(availableISize != NS_UNCONSTRAINEDSIZE, "should no longer use available widths");
        // Subtract off inline axis border+padding from availableISize
        availableISize -= startEdge;
        availableISize -= framePadding.IEnd(frameWM);
        lineLayout.BeginSpan(self.0, aReflowInput, startEdge, startEdge + availableISize, self.mBaseline());

        // First reflow our principal children.
        let mut frame = self.mFrames().FirstChild();
        let mut done = false;
        while let Some(fr) = frame {
            // Set the child frame's parent pointer if we've deferred setting it until
            // now.
            if irs.mSetParentDuringReflow == nsInlineFrame_SetParentDuringReflow::Yes {
                let mut child = Some(fr);
                loop {
                    let Some(c) = child else { break };
                    c.SetParent(self.0);
                    if inFirstLine {
                        ReparentComputedStyleForFirstLineAndMarkDirty(c);
                    }
                    // We also need to do the same for |frame|'s next-in-flows that are in
                    // the sibling list. Otherwise, if we reflow |frame| and it's complete
                    // we'll crash when trying to delete its next-in-flow.
                    // This scenario doesn't happen often, but it can happen.
                    let mut nextSibling = c.GetNextSibling();
                    child = c.GetNextInFlow();
                    if let Some(ch) = child {
                        while nextSibling != Some(ch) && nextSibling.is_some() {
                            nextSibling = nextSibling.unwrap().GetNextSibling();
                        }
                        if nextSibling.is_none() {
                            child = None;
                        }
                    }
                    MOZ_ASSERT!(child.map_or(true, |ch| self.mFrames().ContainsFrame(ch)));
                    if child.is_none() {
                        break;
                    }
                }

                // Fix the parent pointer for ::first-letter child frame next-in-flows,
                // so nsFirstLetterFrame::Reflow can destroy them safely (bug 401042).
                let realFrame = nsPlaceholderFrame_GetRealFrameFor(fr);
                if realFrame.IsLetterFrame() {
                    let child = realFrame.PrincipalChildList_FirstChild();
                    if let Some(child) = child {
                        NS_ASSERTION!(child.IsTextFrame(), "unexpected frame type");
                        let mut nextInFlow = child.GetNextInFlow();
                        while let Some(nif) = nextInFlow {
                            NS_ASSERTION!(nif.IsTextFrame(), "unexpected frame type");
                            if self.mFrames().ContainsFrame(nif) {
                                nif.SetParent(self.0);
                                if inFirstLine {
                                    ReparentComputedStyleForFirstLineAndMarkDirty(nif);
                                }
                            } else {
                                #[cfg(debug_assertions)]
                                {
                                    // Once we find a next-in-flow that isn't ours none of the
                                    // remaining next-in-flows should be either.
                                    let mut n = Some(nif);
                                    while let Some(x) = n {
                                        NS_ASSERTION!(!self.mFrames().ContainsFrame(x), "unexpected letter frame flow");
                                        n = x.GetNextInFlow();
                                    }
                                }
                                break;
                            }
                            nextInFlow = nif.GetNextInFlow();
                        }
                    }
                }
            }
            MOZ_ASSERT!(fr.GetParent() == Some(self.0));

            if !done {
                let reflowingFirstLetter = lineLayout.GetFirstLetterStyleOK();
                self.ReflowInlineFrame(aPresContext, aReflowInput, irs, fr, aStatus);
                done = aStatus.IsInlineBreak() || (!reflowingFirstLetter && aStatus.IsIncomplete());
                if done {
                    if irs.mSetParentDuringReflow == nsInlineFrame_SetParentDuringReflow::No {
                        break;
                    }
                    // Keep reparenting the remaining siblings, but don't reflow them.
                    let pushedFrames = self.GetOverflowFrames();
                    if pushedFrames.map_or(false, |l| l.FirstChild() == Some(fr)) {
                        // Don't bother if |frame| was pushed to our overflow list.
                        break;
                    }
                } else {
                    irs.mPrevFrame = Some(fr);
                }
            }
            frame = fr.GetNextSibling();
        }

        // Attempt to pull frames from our next-in-flow until we can't
        if !done && self.GetNextInFlow().is_some() {
            loop {
                let reflowingFirstLetter = lineLayout.GetFirstLetterStyleOK();
                if frame.is_none() {
                    // Could be non-null if we pulled a first-letter frame and
                    // it created a continuation, since we don't push those.
                    frame = self.PullOneFrame(aPresContext, irs);
                }
                // #ifdef NOISY_PUSHING printf("%p pulled up %p\n", this, frame); #endif  (debug output: NOT PORTED)
                let Some(fr) = frame else {
                    break;
                };
                self.ReflowInlineFrame(aPresContext, aReflowInput, irs, fr, aStatus);
                if aStatus.IsInlineBreak() || (!reflowingFirstLetter && aStatus.IsIncomplete()) {
                    break;
                }
                irs.mPrevFrame = Some(fr);
                frame = fr.GetNextSibling();
            }
        }

        NS_ASSERTION!(!aStatus.IsComplete() || self.GetOverflowFrames().is_none(), "We can't be complete AND have overflow frames!");

        // If after reflowing our children they take up no area then make
        // sure that we don't either.
        //
        // Note: CSS demands that empty inline elements still affect the
        // line-height calculations. However, continuations of an inline
        // that are empty we force to empty so that things like collapsed
        // whitespace in an inline element don't affect the line-height.
        *aReflowOutput.ISize_mut(lineWM) = lineLayout.EndSpan(self.0);

        // Compute final width.

        // XXX Note that that the padding start and end are in the frame's
        //     writing mode, but the metrics' inline-size is in the line's
        //     writing mode. This makes sense if the line and frame are both
        //     vertical or both horizontal, but what should happen with
        //     orthogonal inlines?

        // Make sure to not include our start border and padding if we have a prev
        // continuation or if we're in a part of an {ib} split other than the first
        // one.  For box-decoration-break:clone we always include our start border
        // and padding since all continuations have them.
        if (self.GetPrevContinuation().is_none() && !self.FrameIsNonFirstInIBSplit()) || boxDecorationBreakClone {
            *aReflowOutput.ISize_mut(lineWM) += framePadding.IStart(frameWM);
        }

        /*
         * We want to only apply the end border and padding if we're the last
         * continuation and either not in an {ib} split or the last part of it.  To
         * be the last continuation we have to be complete (so that we won't get a
         * next-in-flow) and have no non-fluid continuations on our continuation
         * chain.  For box-decoration-break:clone we always apply the end border and
         * padding since all continuations have them.
         */
        if (aStatus.IsComplete() && self.LastInFlow().GetNextContinuation().is_none() && !self.FrameIsNonLastInIBSplit()) || boxDecorationBreakClone {
            *aReflowOutput.ISize_mut(lineWM) += framePadding.IEnd(frameWM);
        }

        nsLayoutUtils::SetBSizeFromFontMetrics(self.0, aReflowOutput, framePadding, lineWM, frameWM);

        // For now our overflow area is zero. The real value will be
        // computed in |nsLineLayout::RelativePositionFrames|.
        aReflowOutput.mOverflowAreas.Clear();

        // #ifdef NOISY_FINAL_SIZE ... #endif  (debug output: NOT PORTED)
    }

    // Returns whether there's any remaining frame to pull.
    // Returns whether there's any frame that PullOneFrame would pull from
    // aNextInFlow or any of aNextInFlow's next-in-flows.
    /* static */
    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::HasFramesToPull
    pub fn HasFramesToPull(aNextInFlow: Option<nsInlineFrame>) -> bool {
        let mut aNextInFlow = aNextInFlow;
        while let Some(nif) = aNextInFlow {
            if !nif.mFrames().IsEmpty() {
                return true;
            }
            if let Some(overflow) = nif.GetOverflowFrames() {
                if !overflow.IsEmpty() {
                    return true;
                }
            }
            aNextInFlow = nif.GetNextInFlow().map(nsInlineFrame);
        }
        false
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::ReflowInlineFrame
    pub fn ReflowInlineFrame(&self, aPresContext: &nsPresContext, aReflowInput: &ReflowInput, irs: &mut nsInlineFrame_InlineReflowInput, aFrame: nsIFrame, aStatus: &mut nsReflowStatus) {
        let lineLayout = unsafe { &mut *aReflowInput.mLineLayout };
        let reflowingFirstLetter = lineLayout.GetFirstLetterStyleOK();
        let mut pushedFrame = false;
        aStatus.Reset();
        lineLayout.ReflowFrame(aFrame, aStatus, None, &mut pushedFrame);

        if aStatus.IsInlineBreakBefore() {
            if Some(aFrame) != self.mFrames().FirstChild() {
                // Change break-before status into break-after since we have
                // already placed at least one child frame. This preserves the
                // break-type so that it can be propagated upward.
                let oldClearType = aStatus.FloatClearType();
                aStatus.Reset();
                aStatus.SetIncomplete();
                aStatus.SetInlineLineBreakAfter(oldClearType);
                self.PushFrames(aPresContext, aFrame, irs.mPrevFrame, irs);
            } else {
                // Preserve reflow status when breaking-before our first child
                // and propagate it upward without modification.
            }
            return;
        }

        // Create a next-in-flow if needed.
        if !aStatus.IsFullyComplete() {
            self.CreateNextInFlow(aFrame);
        }

        if aStatus.IsInlineBreakAfter() {
            let nextFrame = aFrame.GetNextSibling();
            if let Some(nextFrame) = nextFrame {
                aStatus.SetIncomplete();
                self.PushFrames(aPresContext, nextFrame, Some(aFrame), irs);
            } else {
                // We must return an incomplete status if there are more child
                // frames remaining in a next-in-flow that follows this frame.
                if nsInlineFrame::HasFramesToPull(self.GetNextInFlow().map(nsInlineFrame)) {
                    aStatus.SetIncomplete();
                }
            }
            return;
        }

        if !aStatus.IsFullyComplete() && !reflowingFirstLetter {
            if let Some(nextFrame) = aFrame.GetNextSibling() {
                self.PushFrames(aPresContext, nextFrame, Some(aFrame), irs);
            }
        }
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::PullOneFrame
    // (virtual; nsFirstLineFrame overrides it)
    pub fn PullOneFrame(&self, aPresContext: &nsPresContext, irs: &mut nsInlineFrame_InlineReflowInput) -> Option<nsIFrame> {
        if self.Type() == LayoutFrameType::Line {
            return nsFirstLineFrame(*self).PullOneFrame(aPresContext, irs);
        }
        self.nsInlineFrame_PullOneFrame(aPresContext, irs)
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::PullOneFrame
    pub fn nsInlineFrame_PullOneFrame(&self, aPresContext: &nsPresContext, irs: &mut nsInlineFrame_InlineReflowInput) -> Option<nsIFrame> {
        let mut frame: Option<nsIFrame> = None;
        let mut nextInFlow = irs.mNextInFlow;

        #[cfg(debug_assertions)]
        let willPull = nsInlineFrame::HasFramesToPull(nextInFlow);

        while let Some(nif) = nextInFlow {
            frame = nif.mFrames().FirstChild();
            if frame.is_none() {
                // The nextInFlow's principal list has no frames, try its overflow list.
                if let Some(overflowFrames) = nif.GetOverflowFrames() {
                    frame = overflowFrames.RemoveFirstChild();
                    if overflowFrames.IsEmpty() {
                        // We're stealing the only frame - delete the overflow list.
                        nif.DestroyOverflowList();
                    } else {
                        // We leave the remaining frames on the overflow list (rather than
                        // putting them on nextInFlow's principal list) so we don't have to
                        // set up the parent for them.
                    }
                    // ReparentFloatsForInlineChild needs it to be on a child list -
                    // we remove it again below.
                    let f = frame.unwrap();
                    nif.mFrames().operator_assign_move(&mut nsFrameList::new(Some(f), Some(f)));
                }
            }

            if let Some(f) = frame {
                // If our block has no next continuation, then any floats belonging to
                // the pulled frame must belong to our block already. This check ensures
                // we do no extra work in the common non-vertical-breaking case.
                if let Some(lc) = irs.mLineContainer {
                    if lc.GetNextContinuation().is_some() {
                        // The blockChildren.ContainsFrame check performed by
                        // ReparentFloatsForInlineChild will be fast because frame's ancestor
                        // will be the first child of its containing block.
                        nsContainerFrame::ReparentFloatsForInlineChild(lc, Some(f), false);
                    }
                }
                nif.mFrames().RemoveFirstChild();
                // nsFirstLineFrame::PullOneFrame calls ReparentComputedStyle.

                self.mFrames().InsertFrame(Some(self.0), irs.mPrevFrame, f);
                if !irs.mLineLayout.is_null() {
                    unsafe { &mut *irs.mLineLayout }.SetDirtyNextLine();
                }
                break;
            }
            nextInFlow = nif.GetNextInFlow().map(nsInlineFrame);
            irs.mNextInFlow = nextInFlow;
        }

        #[cfg(debug_assertions)]
        MOZ_ASSERT!(frame.is_some() == willPull);
        frame
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::PushFrames
    pub fn PushFrames(&self, aPresContext: &nsPresContext, aFromChild: nsIFrame, aPrevSibling: Option<nsIFrame>, aState: &mut nsInlineFrame_InlineReflowInput) {
        // #ifdef NOISY_PUSHING printf(...) #endif  (debug output: NOT PORTED)

        self.PushChildrenToOverflow(aFromChild, aPrevSibling.expect("pushing first child"));
        if !aState.mLineLayout.is_null() {
            unsafe { &mut *aState.mLineLayout }.SetDirtyNextLine();
        }
    }

    //////////////////////////////////////////////////////////////////////

    // Return true if the frame is first visual frame or first continuation
    // Gecko: layout/generic/nsInlineFrame.h nsInlineFrame::IsFirst
    pub fn IsFirst(&self) -> bool {
        // If the frame's bidi visual state is set, return is-first state
        // else return true if it's the first continuation.
        if self.HasAnyStateBits(NS_INLINE_FRAME_BIDI_VISUAL_STATE_IS_SET) {
            self.HasAnyStateBits(NS_INLINE_FRAME_BIDI_VISUAL_IS_FIRST)
        } else {
            self.GetPrevInFlow().is_none()
        }
    }

    // Return true if the frame is last visual frame or last continuation.
    // Gecko: layout/generic/nsInlineFrame.h nsInlineFrame::IsLast
    pub fn IsLast(&self) -> bool {
        // If the frame's bidi visual state is set, return is-last state
        // else return true if it's the last continuation.
        if self.HasAnyStateBits(NS_INLINE_FRAME_BIDI_VISUAL_STATE_IS_SET) {
            self.HasAnyStateBits(NS_INLINE_FRAME_BIDI_VISUAL_IS_LAST)
        } else {
            self.GetNextInFlow().is_none()
        }
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::GetLogicalSkipSides
    pub fn GetLogicalSkipSides(&self) -> LogicalSides {
        let mWritingMode = self.GetWritingMode();
        let mut skip = LogicalSides::new__WritingMode(mWritingMode);
        if self.StyleBorder().mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone {
            return skip;
        }

        if !self.IsFirst() {
            let prev = self.GetPrevContinuation();
            if self.HasAnyStateBits(NS_INLINE_FRAME_BIDI_VISUAL_STATE_IS_SET) || prev.map_or(false, |p| p.GetRect().height != 0 || p.GetRect().width != 0) {
                // Prev continuation is not empty therefore we don't render our start
                // border edge.
                skip += LogicalSide::IStart;
            } else {
                // If the prev continuation is empty, then go ahead and let our start
                // edge border render.
            }
        }
        if !self.IsLast() {
            let next = self.GetNextContinuation();
            if self.HasAnyStateBits(NS_INLINE_FRAME_BIDI_VISUAL_STATE_IS_SET) || next.map_or(false, |n| n.GetRect().height != 0 || n.GetRect().width != 0) {
                // Next continuation is not empty therefore we don't render our end
                // border edge.
                skip += LogicalSide::IEnd;
            } else {
                // If the next continuation is empty, then go ahead and let our end
                // edge border render.
            }
        }

        if self.HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT) {
            // All but the last part of an {ib} split should skip the "end" side (as
            // determined by this frame's direction) and all but the first part of such
            // a split should skip the "start" side.  But figuring out which part of
            // the split we are involves getting our first continuation, which might be
            // expensive.  So don't bother if we already have the relevant bits set.
            if skip != LogicalSides::new__WritingMode_EnumSet(mWritingMode, LogicalSides::IBoth) {
                // We're missing one of the skip bits, so check whether we need to set it.
                // Only get the first continuation once, as an optimization.
                let firstContinuation = self.FirstContinuation();
                if firstContinuation.FrameIsNonLastInIBSplit() {
                    skip += LogicalSide::IEnd;
                }
                if firstContinuation.FrameIsNonFirstInIBSplit() {
                    skip += LogicalSide::IStart;
                }
            }
        }

        skip
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::GetNaturalBaselineBOffset
    pub fn GetNaturalBaselineBOffset(&self, aWM: WritingMode, aBaselineGroup: BaselineSharingGroup, aExportContext: BaselineExportContext) -> Option<nscoord> {
        if aBaselineGroup == BaselineSharingGroup::Last {
            return None;
        }
        // TODO(dshin): Some functions seem to rely on this returning
        // NS_INTRINSIC_ISIZE_UNKNOWN. e.g. /css/css-pseudo/target-text-008.html
        Some(unsafe { *self.mBaseline() })
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::AccessibleType
    // NOT PORTED: accessibility.

    // Gecko: layout/generic/nsInlineFrame.cpp nsInlineFrame::UpdateStyleOfOwnedAnonBoxesForIBSplit
    // NOT PORTED: restyling of {ib} splits.
}

//////////////////////////////////////////////////////////////////////

// nsLineFrame implementation

// Gecko: layout/generic/nsInlineFrame.cpp nsFirstLineFrame::Init
// NOT PORTED: ::first-line style repair on continuations (ComputedStyle of the ::-moz-line-frame
// anonymous box, which this Stylo configuration has no pseudo type for).

// Gecko: layout/generic/nsInlineFrame.cpp nsFirstLineFrame::GetFrameName
// NOT PORTED: debug dumps.

impl nsFirstLineFrame {
    // Gecko: layout/generic/nsInlineFrame.cpp nsFirstLineFrame::PullOneFrame
    pub fn PullOneFrame(&self, aPresContext: &nsPresContext, irs: &mut nsInlineFrame_InlineReflowInput) -> Option<nsIFrame> {
        let frame = self.nsInlineFrame_PullOneFrame(aPresContext, irs);
        if let Some(f) = frame {
            if self.GetPrevInFlow().is_none() {
                // We are a first-line frame. Fixup the child frames
                // style-context that we just pulled.
                NS_ASSERTION!(f.GetParent() == Some(self.0.0), "Incorrect parent?");
                ReparentComputedStyleForFirstLineAndMarkDirty(f);
            }
        }
        frame
    }

    // Gecko: layout/generic/nsInlineFrame.cpp nsFirstLineFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aReflowOutput: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        if aReflowInput.mLineLayout.is_null() {
            return; // XXX does this happen? why?
        }

        // Check for an overflow list with our prev-in-flow
        if let Some(prevInFlow) = self.GetPrevInFlow() {
            let mut prevOverflowFrames = AutoFrameListPtr::new(prevInFlow.StealOverflowFrames());
            if let Some(list) = prevOverflowFrames.operator_arrow_mut() {
                // Reparent the new frames and their ComputedStyles.
                let newFrames = self.mFrames().InsertFrames(Some(self.0.0), None, std::mem::take(list));
                ReparentChildListStyle(aPresContext, newFrames, self.0.0);
            }
        }

        // It's also possible that we have an overflow list for ourselves.
        self.DrainSelfOverflowList();

        // Set our own reflow input (additional state above and beyond aReflowInput).
        let mut irs = nsInlineFrame_InlineReflowInput::new(aReflowInput, nsInlineFrame_SetParentDuringReflow::No);

        let wasEmpty = self.mFrames().IsEmpty();
        if wasEmpty {
            // Try to pull over one frame before starting so that we know
            // whether we have an anonymous block or not.
            self.PullOneFrame(aPresContext, &mut irs);
        }

        if self.GetPrevInFlow().is_none() {
            // XXX This is pretty sick, but what we do here is to pull-up, in
            // advance, all of the next-in-flows children. We re-resolve their
            // style while we are at at it so that when we reflow they have
            // the right style.
            //
            // All of this is so that text-runs reflow properly.
            irs.mPrevFrame = self.mFrames().LastChild();
            loop {
                let frame = self.PullOneFrame(aPresContext, &mut irs);
                let Some(frame) = frame else {
                    break;
                };
                irs.mPrevFrame = Some(frame);
            }
            irs.mPrevFrame = None;
        }

        let lineLayout = unsafe { &mut *aReflowInput.mLineLayout };
        NS_ASSERTION!(!lineLayout.GetInFirstLine(), "Nested first-line frames? BOGUS");
        lineLayout.SetInFirstLine(true);
        self.ReflowFrames(aPresContext, aReflowInput, &mut irs, aReflowOutput, aStatus);
        lineLayout.SetInFirstLine(false);

        // If we could be an abspos containing block, then this is where we would call
        // ReflowAbsoluteFrames. But we can't be, per bug 2036239 comment 1.
        // MOZ_ASSERT(!IsAbsoluteContainer(), ...);  NOT PORTED: absolute positioning.

        // Note: the line layout code will properly compute our overflow state for us
    }

    /* virtual */
    // Gecko: layout/generic/nsInlineFrame.cpp nsFirstLineFrame::PullOverflowsFromPrevInFlow
    pub fn PullOverflowsFromPrevInFlow(&self) {
        if let Some(prevInFlow) = self.GetPrevInFlow() {
            let mut prevOverflowFrames = AutoFrameListPtr::new(prevInFlow.StealOverflowFrames());
            if let Some(list) = prevOverflowFrames.operator_arrow_mut() {
                // Assume that our prev-in-flow has the same line container that we do.
                let newFrames = self.mFrames().InsertFrames(Some(self.0.0), None, std::mem::take(list));
                ReparentChildListStyle(&nsPresContext::Current(), newFrames, self.0.0);
            }
        }
    }

    /* virtual */
    // Gecko: layout/generic/nsInlineFrame.cpp nsFirstLineFrame::DrainSelfOverflowList
    pub fn DrainSelfOverflowList(&self) -> bool {
        let mut overflowFrames = AutoFrameListPtr::new(self.StealOverflowFrames());
        if let Some(list) = overflowFrames.operator_arrow_mut() {
            let result = !list.IsEmpty();
            let newFrames = self.mFrames().AppendFrames(None, std::mem::take(list));
            ReparentChildListStyle(&nsPresContext::Current(), newFrames, self.0.0);
            return result;
        }
        false
    }
}

// ---------------------------------------------------------------------------------------------
// ADAPTER: pieces of other Gecko files that nsInlineFrame.cpp calls and that have no translated
// home yet. Each says which file it stands for.
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsPlaceholderFrame.h nsPlaceholderFrame::GetRealFrameFor
// ADAPTER: the out-of-flow frame is the one given to SetPlaceholderFor (nsLineLayoutAdapter.rs).
pub fn nsPlaceholderFrame_GetRealFrameFor(aFrame: nsIFrame) -> nsIFrame {
    if aFrame.IsPlaceholderFrame() {
        return nsPlaceholderFrame::GetRealFrameForPlaceholder(aFrame);
    }
    aFrame
}

impl nsLayoutUtils {
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::FindNearestBlockAncestor
    pub fn FindNearestBlockAncestor(aFrame: nsIFrame) -> Option<nsBlockFrame> {
        let mut nextAncestor = aFrame.GetParent();
        while let Some(a) = nextAncestor {
            let block = nsBlockFrame::do_QueryFrame(a);
            if block.is_some() {
                return block;
            }
            nextAncestor = a.GetParent();
        }
        None
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::SetBSizeFromFontMetrics
    // NOT PORTED: the mixed vertical writing mode case that takes the larger of the vertical and
    // horizontal font metrics (nsLayoutUtils.cpp lines 8661-8671); the metrics are always present.
    pub fn SetBSizeFromFontMetrics(aFrame: nsIFrame, aMetrics: &mut ReflowOutput, aFramePadding: LogicalMargin, aLineWM: WritingMode, aFrameWM: WritingMode) {
        let fm = nsLayoutUtils::GetInflatedFontMetricsForFrame(aFrame);

        // Compute final height of the frame.
        //
        // Do things the standard css2 way -- though it's hard to find it
        // in the css2 spec! It's actually found in the css1 spec section
        // 4.4 (you will have to read between the lines to really see
        // it).
        //
        // The height of our box is the sum of our font size plus the top
        // and bottom border and padding. The height of children do not
        // affect our height.
        aMetrics.SetBlockStartAscent(if aLineWM.IsAlphabeticalBaseline() {
            if aLineWM.IsLineInverted() { fm.MaxDescent() } else { fm.MaxAscent() }
        } else {
            fm.MaxHeight() / 2
        });
        *aMetrics.BSize_mut(aLineWM) = fm.MaxHeight();
        aMetrics.SetBlockStartAscent(aMetrics.BlockStartAscent() + aFramePadding.BStart(aFrameWM));
        *aMetrics.BSize_mut(aLineWM) += aFramePadding.BStartEnd(aFrameWM);
    }
}

impl nsIFrame {
    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetLogicalSkipSides (virtual; the base returns no side)
    pub fn GetLogicalSkipSides(&self) -> LogicalSides {
        match self.Type() {
            // Inline, Line => nsInlineFrame::GetLogicalSkipSides
            LayoutFrameType::Inline | LayoutFrameType::Line => nsInlineFrame::from_frame(*self).GetLogicalSkipSides(),
            // Block => nsBlockFrame::GetLogicalSkipSides (nsBlockFrame.rs adds the arm)
            // TableRowGroup, TableRow, TableCell => their GetLogicalSkipSides
            LayoutFrameType::TableRowGroup => nsTableRowGroupFrame::from_frame(*self).GetLogicalSkipSides(),
            LayoutFrameType::TableRow => nsTableRowFrame::from_frame(*self).GetLogicalSkipSides(),
            LayoutFrameType::TableCell => nsTableCellFrame::from_frame(*self).GetLogicalSkipSides(),
            // NOT PORTED: the overrides of the text and other frames.
            _ => LogicalSides::new__WritingMode(self.GetWritingMode()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockReflowState_test_support::*;

    // Tests are written from the documented behaviour (CSS 2.2 8.6, 10.8 for inline boxes, and
    // the comments of nsInlineFrame.cpp: "Don't offset by our start borderpadding if we have a
    // prev continuation", "We want to only apply the end border and padding if we're the last
    // continuation"); Firefox has no gtest for this class.

    const AUTO: nscoord = NS_UNCONSTRAINEDSIZE;

    // 12px ascent, 3px descent
    fn SetTestFontMetrics() {
        SetFontMetricsStub(Some(nsFontMetricsStub { mMaxAscent: 12 * PX, mMaxDescent: 3 * PX, mXHeight: 6 * PX, mSubscriptOffset: 2 * PX, mSuperscriptOffset: 4 * PX }));
    }

    struct Fixture {
        f: Vec<nsIFrame>,
        fm: *mut nsFloatManager,
        root: ReflowInput<'static>,
    }

    // A block `root` (100px wide) with a display:inline `span` (style given) child, and text
    // children `a`, `b`, ... of the span with these widths (px): frames [root, span, a, b, ...].
    fn fixture(span_style: &str, text_widths: &[i32]) -> Fixture {
        fixture_of(span_style, text_widths, false)
    }

    // The same with inline-block children (atomic inlines: they cannot break inside, so one
    // that does not fit makes the line break before it).
    fn atomic_fixture(span_style: &str, widths: &[i32]) -> Fixture {
        fixture_of(span_style, widths, true)
    }

    fn fixture_of(span_style: &str, text_widths: &[i32], atomic: bool) -> Fixture {
        SetTestFontMetrics();
        let mut html = format!(r#"<div id=root style="margin:0;border:0;padding:0;width:100px"><span id=span style="display:inline;{span_style}">"#);
        let mut tree: Vec<(String, Option<usize>)> = vec![("root".to_string(), None), ("span".to_string(), Some(0))];
        for i in 0..text_widths.len() {
            html += &format!(r#"<div id=t{i} style="{}"></div>"#, if atomic { format!("display:inline-block;width:{}px;height:15px", text_widths[i]) } else { String::new() });
            tree.push((format!("t{i}"), Some(1)));
        }
        html += "</span></div>";
        let tree_ref: Vec<(&str, Option<usize>)> = tree.iter().map(|(n, p)| (n.as_str(), *p)).collect();
        let f = frames(&html, &tree_ref, 100);
        f[1].SetType(LayoutFrameType::Inline);
        for (i, w) in text_widths.iter().enumerate() {
            if atomic {
                f[2 + i].SetReflowStubSize(w * PX, 15 * PX, 12 * PX);
            } else {
                f[2 + i].SetType(LayoutFrameType::Text);
                f[2 + i].SetTextFrameStub(TextFrameStub { mISize: w * PX, mBSize: 15 * PX, mAscent: 12 * PX, ..Default::default() });
            }
        }
        let fm = Box::into_raw(Box::new(nsFloatManager::new(f[0].GetWritingMode())));
        let mut root = root_input(f[0], 100, AUTO);
        root.mFloatManager = fm;
        Fixture { f, fm, root }
    }

    impl Fixture {
        // A new line over `child` (the inline-level frame the line holds), 100px wide.
        fn line(&self, child: nsIFrame) -> nsLineLayout {
            let list = Box::leak(Box::new(nsLineList::new()));
            list.push_back(NS_NewLineBox__nsIFrame_bool(Some(child), false));
            let it = list.begin_mut();
            let mut ll = nsLineLayout::new(&nsPresContext::Current(), self.fm, &self.root, Some(&it), std::ptr::null_mut());
            ll.Init(std::ptr::null_mut::<BlockReflowState>(), 0, 0);
            ll.BeginLineReflow(0, 0, 100 * PX, AUTO, false, false, self.f[0].GetWritingMode(), nsSize::new(100 * PX, 0), 0);
            ll
        }
    }

    fn reflow(ll: &mut nsLineLayout, f: nsIFrame) -> (bool, nsReflowStatus) {
        let mut status = nsReflowStatus::new();
        let mut pushed = false;
        ll.ReflowFrame(f, &mut status, None, &mut pushed);
        (pushed, status)
    }

    fn children(p: nsIFrame) -> Vec<nsIFrame> {
        p.mFrames().begin().collect()
    }

    const EDGES: &str = "padding:0 5px;border-left:2px solid;border-right:3px solid";

    #[test]
    fn the_inline_is_as_wide_as_its_children_plus_border_and_padding() {
        let fx = fixture(EDGES, &[40, 20]);
        let mut ll = fx.line(fx.f[1]);
        let (pushed, status) = reflow(&mut ll, fx.f[1]);
        assert!(!pushed && status.IsFullyComplete());
        // 2 + 5 start edge, 40 + 20 text, 5 + 3 end edge
        assert_eq!(ll.GetCurrentICoord(), 75 * PX);
        ll.EndLineReflow();
    }

    #[test]
    fn the_children_start_after_the_start_edge() {
        let fx = fixture(EDGES, &[40, 20]);
        let mut ll = fx.line(fx.f[1]);
        reflow(&mut ll, fx.f[1]);
        ll.VerticalAlignLine();
        // (the children's x is relative to the line until the span is placed: 7px start edge)
        assert_eq!(fx.f[2].GetPosition().x, 7 * PX);
        assert_eq!(fx.f[3].GetPosition().x, 47 * PX);
        ll.EndLineReflow();
    }

    #[test]
    fn a_child_that_does_not_fit_goes_to_the_overflow_list_and_the_end_edge_is_left_out() {
        let fx = atomic_fixture(EDGES, &[60, 60]);
        let mut ll = fx.line(fx.f[1]);
        let (pushed, status) = reflow(&mut ll, fx.f[1]);
        assert!(!pushed);
        // incomplete: a next-in-flow is needed; the break comes after what was placed
        assert!(status.IsIncomplete());
        assert!(status.IsInlineBreakAfter());
        assert_eq!(children(fx.f[1]), vec![fx.f[2]]);
        let overflow: Vec<nsIFrame> = fx.f[1].GetOverflowFrames().unwrap().begin().collect();
        assert_eq!(overflow, vec![fx.f[3]]);
        // start edge 7 + first text 60; no end edge on a fragment that is not the last
        assert_eq!(ll.GetCurrentICoord(), 67 * PX);
        ll.EndLineReflow();
    }

    #[test]
    fn the_continuation_takes_the_overflow_and_only_the_last_fragment_has_the_end_edge() {
        let fx = atomic_fixture(EDGES, &[60, 60]);
        let mut ll = fx.line(fx.f[1]);
        reflow(&mut ll, fx.f[1]);
        ll.EndLineReflow();

        // what the block does for an incomplete child: a continuation after it
        let cont = fx.f[0].CreateNextInFlow_for_test(fx.f[1]);
        fx.f[0].SetType(LayoutFrameType::Block);
        let mut ll2 = fx.line(cont);
        let (pushed, status) = reflow(&mut ll2, cont);
        assert!(!pushed);
        assert!(status.IsFullyComplete());
        // the second text frame moved over, its parent is the continuation now
        assert_eq!(children(cont), vec![fx.f[3]]);
        assert_eq!(fx.f[3].GetParent(), Some(cont));
        assert!(fx.f[1].GetOverflowFrames().is_none());
        // no start edge (there is a previous fragment): 60 text + 5 + 3 end edge
        assert_eq!(ll2.GetCurrentICoord(), 68 * PX);
        ll2.EndLineReflow();
    }

    #[test]
    fn box_decoration_break_slice_skips_the_inner_sides_of_fragments() {
        let fx = fixture(EDGES, &[60, 60]);
        let cont = fx.f[0].CreateNextInFlow_for_test(fx.f[1]);
        // widths: both fragments non-empty
        fx.f[1].SetRect(nsRect::new(0, 0, 67 * PX, 15 * PX));
        cont.SetRect(nsRect::new(0, 0, 68 * PX, 15 * PX));
        let wm = fx.f[1].GetWritingMode();
        let first = nsInlineFrame::from_frame(fx.f[1]).GetLogicalSkipSides();
        assert!(!first.IStart() && first.IEnd());
        let last = nsInlineFrame::from_frame(cont).GetLogicalSkipSides();
        assert!(last.IStart() && !last.IEnd());
        // an empty previous fragment does not hide the start edge
        fx.f[1].SetRect(nsRect::new(0, 0, 0, 0));
        assert!(!nsInlineFrame::from_frame(cont).GetLogicalSkipSides().IStart());
        let _ = wm;
    }

    #[test]
    fn skip_sides_reduce_the_content_size() {
        // nsIFrame::SizeReducedBy uses GetLogicalSkipSides: the skipped sides' border+padding are not
        // taken off.
        let fx = fixture(EDGES, &[60, 60]);
        let cont = fx.f[0].CreateNextInFlow_for_test(fx.f[1]);
        fx.f[1].SetRect(nsRect::new(0, 0, 67 * PX, 15 * PX));
        cont.SetRect(nsRect::new(0, 0, 68 * PX, 15 * PX));
        let wm = cont.GetWritingMode();
        let bp = LogicalMargin::new(wm, 0, 8 * PX, 0, 7 * PX); // block-start, inline-end, block-end, inline-start
        let size = cont.SizeReducedBy(wm, bp);
        // inline-start is skipped, inline-end is not: 68 - 8
        assert_eq!(size.ISize(wm), 60 * PX);
    }

    #[test]
    fn is_self_empty_looks_at_border_padding_and_margin() {
        let none = fixture("", &[]);
        assert!(nsInlineFrame::from_frame(none.f[1]).IsSelfEmpty());
        assert!(none.f[1].IsEmpty());
        for style in ["padding-left:1px", "border-right:1px solid", "margin-right:2px", "padding-right:3px"] {
            let fx = fixture(style, &[]);
            assert!(!nsInlineFrame::from_frame(fx.f[1]).IsSelfEmpty(), "{style}");
            assert!(!fx.f[1].IsEmpty(), "{style}");
        }
        // block-direction edges are ignored
        for style in ["padding-top:4px", "margin-bottom:4px", "border-top:1px solid"] {
            let fx = fixture(style, &[]);
            assert!(nsInlineFrame::from_frame(fx.f[1]).IsSelfEmpty(), "{style}");
        }
    }

    #[test]
    fn is_empty_needs_all_children_empty() {
        let fx = fixture("", &[10]);
        assert!(!fx.f[2].IsEmpty()); // (text with no stub for emptiness is not empty)
        assert!(!fx.f[1].IsEmpty());
        fx.f[2].SetIsEmpty(true);
        assert!(fx.f[1].IsEmpty());
    }

    #[test]
    fn the_baseline_slot_is_stable_and_starts_unknown() {
        let fx = fixture("", &[]);
        let s = nsInlineFrame::from_frame(fx.f[1]);
        let p = s.mBaseline();
        // frames can be added while the pointer is live (continuations)
        let mut last = fx.f[1];
        for _ in 0..50 {
            last = fx.f[0].CreateNextInFlow_for_test(last);
        }
        assert_eq!(s.mBaseline(), p);
        assert_eq!(unsafe { *p }, NS_INTRINSIC_ISIZE_UNKNOWN);
        assert_eq!(s.GetNaturalBaselineBOffset(fx.f[1].GetWritingMode(), BaselineSharingGroup::First, BaselineExportContext::Other), Some(NS_INTRINSIC_ISIZE_UNKNOWN));
        assert_eq!(s.GetNaturalBaselineBOffset(fx.f[1].GetWritingMode(), BaselineSharingGroup::Last, BaselineExportContext::Other), None);
    }

    #[test]
    fn the_block_size_comes_from_the_font_and_the_block_edges() {
        let fx = fixture("padding:3px 0 4px;border-top:1px solid", &[]);
        let wm = fx.f[1].GetWritingMode();
        SetTestFontMetrics();
        let mut metrics = ReflowOutput::new__WritingMode(wm);
        let padding = LogicalMargin::new(wm, 4 * PX, 0, 4 * PX, 0);
        nsLayoutUtils::SetBSizeFromFontMetrics(fx.f[1], &mut metrics, padding, wm, wm);
        // "The height of our box is the sum of our font size plus the top and bottom border and
        // padding": 15 + 4 + 4; the ascent is the font ascent plus the block-start edge
        assert_eq!(metrics.BSize(wm), 23 * PX);
        assert_eq!(metrics.BlockStartAscent(), 16 * PX);
    }

    #[test]
    fn compute_size_of_an_inline_is_unconstrained() {
        let fx = fixture("", &[]);
        let wm = fx.f[1].GetWritingMode();
        let r = fx.f[1].ComputeSize(
            &SizeComputationInput::new__nsIFrame_gfxContext_AnchorPosResolutionCache(fx.f[1], Some(gfxContext), std::ptr::null_mut()),
            wm,
            LogicalSize::new(wm, 100 * PX, AUTO),
            100 * PX,
            LogicalSize::new__WritingMode(wm),
            LogicalSize::new__WritingMode(wm),
            &StyleSizeOverrides::default(),
            ComputeSizeFlags::default(),
        );
        assert_eq!(r.mLogicalSize.ISize(wm), AUTO);
        assert_eq!(r.mLogicalSize.BSize(wm), AUTO);
    }

    #[test]
    fn has_frames_to_pull_looks_through_the_next_in_flows_lists() {
        let fx = fixture("", &[10, 10]);
        let nif = fx.f[0].CreateNextInFlow_for_test(fx.f[1]);
        let nif2 = fx.f[0].CreateNextInFlow_for_test(nif);
        assert!(!nsInlineFrame::HasFramesToPull(Some(nsInlineFrame(nif))));
        // a frame in the second next-in-flow's overflow list counts
        nif2.SetOverflowFrames(nsFrameList::new(Some(fx.f[3]), Some(fx.f[3])));
        fx.f[2].SetNextSibling(None);
        assert!(nsInlineFrame::HasFramesToPull(Some(nsInlineFrame(nif))));
        assert!(!nsInlineFrame::HasFramesToPull(None));
    }

    #[test]
    fn pull_one_frame_takes_the_first_child_of_the_next_in_flow() {
        // An inline whose continuation has the children: the first frame moves to this frame.
        let fx = fixture("", &[10, 10]);
        let nif = fx.f[0].CreateNextInFlow_for_test(fx.f[1]);
        // move both texts into the continuation
        let s = nsInlineFrame::from_frame(fx.f[1]);
        let taken = fx.f[1].StealFramesAfter(None);
        for t in taken.begin() {
            t.SetParent(nif);
        }
        nif.mFrames().AppendFrames(Some(nif), taken);
        let ri = {
            let wm = fx.f[1].GetWritingMode();
            let _ = wm;
            nsInlineFrame_InlineReflowInput { mPrevFrame: None, mNextInFlow: Some(nsInlineFrame(nif)), mLineContainer: Some(fx.f[0]), mLineLayout: std::ptr::null_mut(), mSetParentDuringReflow: nsInlineFrame_SetParentDuringReflow::No }
        };
        let mut ri = ri;
        let got = s.PullOneFrame(&nsPresContext::Current(), &mut ri);
        assert_eq!(got, Some(fx.f[2]));
        assert_eq!(children(fx.f[1]), vec![fx.f[2]]);
        assert_eq!(fx.f[2].GetParent(), Some(fx.f[1]));
        assert_eq!(children(nif), vec![fx.f[3]]);
        // the next one goes after the previous frame
        ri.mPrevFrame = Some(fx.f[2]);
        assert_eq!(s.PullOneFrame(&nsPresContext::Current(), &mut ri), Some(fx.f[3]));
        assert_eq!(children(fx.f[1]), vec![fx.f[2], fx.f[3]]);
        assert_eq!(s.PullOneFrame(&nsPresContext::Current(), &mut ri), None);
    }

    #[test]
    fn push_frames_moves_the_tail_to_the_overflow_list() {
        let fx = fixture("", &[10, 10, 10]);
        let s = nsInlineFrame::from_frame(fx.f[1]);
        let mut ri = nsInlineFrame_InlineReflowInput { mPrevFrame: None, mNextInFlow: None, mLineContainer: None, mLineLayout: std::ptr::null_mut(), mSetParentDuringReflow: nsInlineFrame_SetParentDuringReflow::No };
        s.PushFrames(&nsPresContext::Current(), fx.f[3], Some(fx.f[2]), &mut ri);
        assert_eq!(children(fx.f[1]), vec![fx.f[2]]);
        assert_eq!(fx.f[1].GetOverflowFrames().unwrap().GetLength(), 2);
    }

    #[test]
    fn destroy_fixes_the_parents_of_the_overflow_frames_and_destroys_everything() {
        let fx = fixture("", &[10, 10]);
        let s = nsInlineFrame::from_frame(fx.f[1]);
        let mut ri = nsInlineFrame_InlineReflowInput { mPrevFrame: None, mNextInFlow: None, mLineContainer: None, mLineLayout: std::ptr::null_mut(), mSetParentDuringReflow: nsInlineFrame_SetParentDuringReflow::No };
        s.PushFrames(&nsPresContext::Current(), fx.f[3], Some(fx.f[2]), &mut ri);
        fx.f[3].SetParent(fx.f[0]); // a wrong parent, as lazy reparenting can leave
        let mut ctx = FrameDestroyContext::default();
        fx.f[1].Destroy(&mut ctx);
        assert_eq!(ctx.mDestroyedFrames.len(), 3);
        assert_eq!(*ctx.mDestroyedFrames.last().unwrap(), fx.f[1]);
    }

    #[test]
    fn steal_frame_finds_children_that_moved_to_a_next_in_flow() {
        // "Due to our lazy reparenting optimization aChild might not actually be on any of our
        // child lists, but instead in one of our next-in-flows."
        let fx = fixture("", &[10, 10]);
        let nif = fx.f[0].CreateNextInFlow_for_test(fx.f[1]);
        let taken = fx.f[1].StealFramesAfter(None);
        for t in taken.begin() {
            // (parent pointers still say the first fragment)
            let _ = t;
        }
        nif.mFrames().AppendFrames(None, taken);
        fx.f[1].StealFrame(fx.f[3]);
        assert_eq!(children(nif), vec![fx.f[2]]);
    }

    #[test]
    fn a_first_line_frame_lays_out_like_an_inline_and_resets_the_flag() {
        let fx = fixture("", &[30]);
        fx.f[1].SetType(LayoutFrameType::Line);
        let mut ll = fx.line(fx.f[1]);
        let (pushed, status) = reflow(&mut ll, fx.f[1]);
        assert!(!pushed && status.IsFullyComplete());
        assert_eq!(ll.GetCurrentICoord(), 30 * PX);
        assert!(!ll.GetInFirstLine());
        ll.EndLineReflow();
    }

    #[test]
    fn deferred_parent_pointers_are_set_when_the_frames_are_reflowed() {
        // The continuation takes its prev-in-flow's overflow list as its child list without
        // setting the parents (SetParentDuringReflow::Yes); ReflowFrames sets them.
        let fx = atomic_fixture("", &[60, 60, 50]);
        let mut ll = fx.line(fx.f[1]);
        reflow(&mut ll, fx.f[1]);
        ll.EndLineReflow();
        assert_eq!(fx.f[1].GetOverflowFrames().unwrap().GetLength(), 2);
        let cont = fx.f[0].CreateNextInFlow_for_test(fx.f[1]);
        let mut ll2 = fx.line(cont);
        let (_, status) = reflow(&mut ll2, cont);
        // the second child is placed (60), the third (50) does not fit next to it: pushed on again
        assert!(status.IsIncomplete());
        assert_eq!(fx.f[3].GetParent(), Some(cont));
        assert_eq!(fx.f[4].GetParent(), Some(cont));
        assert_eq!(children(cont), vec![fx.f[3]]);
        assert_eq!(cont.GetOverflowFrames().unwrap().begin().collect::<Vec<_>>(), vec![fx.f[4]]);
        ll2.EndLineReflow();
    }
}

#[cfg(test)]
impl nsIFrame {
    // test helper: CreateNextInFlow of `aChild` in this frame's parent. The container of the child
    // is its parent, which has to be a non-block, so the child's parent is typed Inline for the call
    // (and the root `self` keeps its type).
    fn CreateNextInFlow_for_test(&self, aChild: nsIFrame) -> nsIFrame {
        let parent = aChild.GetParent().unwrap();
        let t = parent.Type();
        parent.SetType(LayoutFrameType::Inline);
        let nif = parent.CreateNextInFlow(aChild).expect("a new continuation");
        parent.SetType(t);
        nif
    }
}
