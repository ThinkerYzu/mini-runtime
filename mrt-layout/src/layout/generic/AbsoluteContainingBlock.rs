/*
 * code for managing absolutely positioned children of a rendering
 * object that is a containing block for them
 */
// Translated from layout/generic/AbsoluteContainingBlock.h and AbsoluteContainingBlock.cpp (GECKO_REV).
//
// The containing block is owned by its frame through the AbsoluteContainingBlockProperty (nsIFrame.rs:
// GetAbsoluteContainingBlock, MarkAsAbsoluteContainingBlock), as in Gecko.
//
// NOT PORTED for the whole file:
//  - fragmentation (C2): the pushed absolute list, the unfragmented position, size and containing block
//    properties, mCumulativeContainingBlockBSize and abspos continuations. Every page is laid out
//    unfragmented, so `AllowFragmentation` never lets a kid split (the available block-size is
//    unconstrained) and the unfragmented properties are never set.
//  - anchor positioning (AnchorPosResolutionCache, position-area, position-try fallbacks and
//    position-try-order, position-visibility, scroll compensation): the style adapter has no anchor
//    references, so the fallback loop runs once with the base style.
//  - view transitions (the snapshot containing block), menupopups, interruptible reflow
//    (HasPendingInterrupt and CheckForInterrupt are false here), the debug-only noisy reflow printing.
use crate::nsLineLayoutAdapter::*;
use crate::*;

// Gecko: layout/generic/AbsoluteContainingBlock.h mozilla::AbsPosReflowFlag
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbsPosReflowFlag {
    // Allow the children in the absolute containing block to fragment. Without
    // this flag, the children will be monolithic.
    AllowFragmentation,
    CBWidthChanged,
    CBHeightChanged,
    IsGridContainerCB,
}

impl EnumSetValue for AbsPosReflowFlag {
    fn BitFor(self) -> u32 {
        1 << (self as u32)
    }
}

// Gecko: layout/generic/AbsoluteContainingBlock.h mozilla::AbsPosReflowFlags
pub type AbsPosReflowFlags = EnumSet<AbsPosReflowFlag>;

/**
 * This class contains the logic for being an absolute containing block.  This
 * class is used within viewport frames (for frames representing content with
 * fixed position) and blocks (for frames representing absolutely positioned
 * content), since each set of frames is absolutely positioned with respect to
 * its parent.
 *
 * There is no principal child list, just a named child list which contains
 * the absolutely positioned frames (FrameChildListID::Absolute).
 *
 * All functions include as the first argument the frame that is delegating
 * the request.
 */
// Gecko: layout/generic/AbsoluteContainingBlock.h mozilla::AbsoluteContainingBlock
#[derive(Default)]
pub struct AbsoluteContainingBlock {
    // Stores the abspos frames that have been placed in this containing block.
    pub mAbsoluteFrames: nsFrameList,
    // A temporary frame list used during reflow, storing abspos frames that need
    // to be reflowed by the delegating frame's next-in-flow after transferring
    // them to its own AbsoluteContainingBlock.
    // (Always empty here: fragmentation is NOT PORTED.)
    pub mPushedAbsoluteFrames: nsFrameList,
    // (mCumulativeContainingBlockBSize: NOT PORTED, fragmentation.)
}

// Gecko: layout/generic/AbsoluteContainingBlock.h AbsoluteContainingBlock::AnchorOffsetInfo
// (NOT PORTED: anchor positioning; the scroll offset is always zero and there is no position area.)
#[derive(Clone, Copy, Default, Debug)]
pub struct AbsoluteContainingBlock_AnchorOffsetInfo {
    pub mScrollOffset: nsPoint,
}

/**
 * Rects for abspos frames to position against. Differences are relevant
 * for containing blocks that scroll.
 *
 * See https://drafts.csswg.org/css-position-4/#scrollable-cb
 */
// Gecko: layout/generic/AbsoluteContainingBlock.h AbsoluteContainingBlock::ContainingBlockRects
#[derive(Clone, Copy, Default, Debug)]
pub struct AbsoluteContainingBlock_ContainingBlockRects {
    pub mLocal: nsRect,
    pub mScrollable: nsRect,
}

impl AbsoluteContainingBlock {
    // Gecko: layout/generic/AbsoluteContainingBlock.h AbsoluteContainingBlock::GetChildList
    pub fn GetChildList(&self) -> &nsFrameList {
        &self.mAbsoluteFrames
    }

    // Gecko: layout/generic/AbsoluteContainingBlock.h AbsoluteContainingBlock::GetPushedChildList
    pub fn GetPushedChildList(&self) -> &nsFrameList {
        &self.mPushedAbsoluteFrames
    }

    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::SetInitialChildList
    pub fn SetInitialChildList(&mut self, aDelegatingFrame: nsIFrame, aListID: FrameChildListID, aChildList: nsFrameList) {
        MOZ_ASSERT!(aListID == FrameChildListID::Absolute, "unexpected child list");
        #[cfg(debug_assertions)]
        for f in &aChildList {
            MOZ_ASSERT!(f.GetParent() == Some(aDelegatingFrame), "Unexpected parent");
        }
        // (nsIFrame::VerifyDirtyBitSet: NOT PORTED, debug-only.)
        self.mAbsoluteFrames = aChildList;
    }

    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::AppendFrames
    pub fn AppendFrames(&mut self, aDelegatingFrame: nsIFrame, aListID: FrameChildListID, aFrameList: nsFrameList) {
        MOZ_ASSERT!(aListID == FrameChildListID::Absolute, "unexpected child list");

        // Append the frames to our list of absolutely positioned frames
        self.mAbsoluteFrames.AppendFrames(None, aFrameList);

        // no damage to intrinsic widths, since absolutely positioned frames can't
        // change them
        PresShell_FrameNeedsReflow(aDelegatingFrame, IntrinsicDirty::None, NS_FRAME_HAS_DIRTY_CHILDREN);
    }

    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::InsertFrames
    pub fn InsertFrames(&mut self, aDelegatingFrame: nsIFrame, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aFrameList: nsFrameList) {
        MOZ_ASSERT!(aListID == FrameChildListID::Absolute, "unexpected child list");
        NS_ASSERTION!(aPrevFrame.map_or(true, |p| p.GetParent() == Some(aDelegatingFrame)), "inserting after sibling frame with different parent");

        self.mAbsoluteFrames.InsertFrames(None, aPrevFrame, aFrameList);

        // no damage to intrinsic widths, since absolutely positioned frames can't
        // change them
        PresShell_FrameNeedsReflow(aDelegatingFrame, IntrinsicDirty::None, NS_FRAME_HAS_DIRTY_CHILDREN);
    }

    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::RemoveFrame
    pub fn RemoveFrame(&mut self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        MOZ_ASSERT!(aListID == FrameChildListID::Absolute, "unexpected child list");

        let mut delFrames: Vec<nsIFrame> = Vec::new();
        let mut f = Some(aOldFrame);
        while let Some(frame) = f {
            delFrames.push(frame);
            f = frame.GetNextInFlow();
        }
        for delFrame in delFrames.into_iter().rev() {
            // delFrame->GetParent()->GetAbsoluteContainingBlock()->StealFrame(delFrame);
            // ADAPTER: the parent's containing block is this one unless an abspos continuation moved
            // to another fragment, which cannot happen (fragmentation is NOT PORTED).
            MOZ_ASSERT!(delFrame.GetParent().and_then(|p| p.GetAbsoluteContainingBlock()).map(|a| a as *mut AbsoluteContainingBlock) == Some(self as *mut AbsoluteContainingBlock));
            self.StealFrame(delFrame);
            nsContainerFrame::from_frame(delFrame).Destroy(aContext);
        }
    }

    /**
     * Return the pushed absolute frames. The caller is responsible for passing
     * the ownership of the frames to someone else, or destroying them.
     */
    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::StealPushedChildList
    pub fn StealPushedChildList(&mut self) -> nsFrameList {
        std::mem::take(&mut self.mPushedAbsoluteFrames)
    }

    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::DrainPushedChildList
    // NOT PORTED: fragmentation; the pushed list is always empty (AbsoluteContainingBlock.cpp lines
    // 150-172 at GECKO_REV).
    fn DrainPushedChildList(&mut self, aDelegatingFrame: nsIFrame) {
        MOZ_ASSERT!(self.mPushedAbsoluteFrames.IsEmpty(), "NOT PORTED: pushed absolute frames (fragmentation)");
    }

    /**
     * Prepare our absolute child list so that it is ready to reflow by moving all
     * the pushed absolute frames in aDelegatingFrame's prev-in-flow's absCB, and
     * some in our own pushed absolute child list, to our absolute child list.
     *
     * @return true if we have absolute frames after we return.
     */
    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::PrepareAbsoluteFrames
    // NOT PORTED: the frames pulled from the prev-in-flow's and the next-in-flows' containing blocks
    // (AbsoluteContainingBlock.cpp lines 176-210 and 219-239 at GECKO_REV): a delegating frame never
    // has continuations here (fragmentation, C2).
    pub fn PrepareAbsoluteFrames(&mut self, aDelegatingFrame: nsIFrame) -> bool {
        MOZ_ASSERT!(aDelegatingFrame.GetPrevInFlow().is_none() && aDelegatingFrame.GetNextInFlow().is_none(), "NOT PORTED: absolute containing blocks with continuations");

        // Similarly, for any children in our pushed child list that don't have a
        // prev-in-flow in our regular child list, we move those children back into
        // our child list.
        self.DrainPushedChildList(aDelegatingFrame);

        self.HasAbsoluteFrames()
    }

    /**
     * Return true if we have absolute frames.
     *
     * Note: During reflow, consider calling PrepareAbsoluteFrames() rather than
     * this method; it moves absolute frames from other lists to mAbsoluteFrames,
     * which may be needed to get the correct result.
     */
    // Gecko: layout/generic/AbsoluteContainingBlock.h AbsoluteContainingBlock::HasAbsoluteFrames
    pub fn HasAbsoluteFrames(&self) -> bool {
        self.mAbsoluteFrames.NotEmpty()
    }

    /**
     * Remove aFrame from one of our frame lists without destroying it.
     */
    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::StealFrame
    fn StealFrame(&mut self, aFrame: nsIFrame) {
        let frameRemoved = self.mAbsoluteFrames.StartRemoveFrame(aFrame) || self.mPushedAbsoluteFrames.ContinueRemoveFrame(aFrame);
        MOZ_ASSERT!(frameRemoved, "Failed to find aFrame from our child lists!");
    }

    // (SanityCheckChildListsBeforeReflow: NOT PORTED, debug-only and about continuations.)

    /**
     * Called by the delegating frame after it has done its reflow first. This
     * function will reflow any absolutely positioned child frames that need to
     * be reflowed, e.g., because the absolutely positioned child frame has
     * 'auto' for an offset, or a percentage based width or height.
     *
     * @param aOverflowAreas, if non-null, is unioned with (in the local
     * coordinate space) the overflow areas of the absolutely positioned
     * children.
     * @param aReflowStatus This function merges in the statuses of the absolutely
     * positioned children's reflows.
     * @param aContainingBlock Rect representing the area where absolute
     * positioned children can be positioned. Generally, this is the padding rect
     * of `aDelegatingFrame` (Which would not have a valid mRect set during
     * reflow), offset against the `aDelegatingFrame`'s border rect.
     * @param aFlags zero or more AbsPosReflowFlags
     */
    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::Reflow
    pub fn Reflow(
        &mut self,
        aDelegatingFrame: nsIFrame,
        aPresContext: &nsPresContext,
        aReflowInput: &ReflowInput,
        aReflowStatus: &mut nsReflowStatus,
        aContainingBlock: nsRect,
        aFlags: AbsPosReflowFlags,
        mut aOverflowAreas: Option<&mut OverflowAreas>,
    ) {
        MOZ_ASSERT!(aReflowStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        let scrollableContainingBlock = ComputeScrollableContainingBlock(aDelegatingFrame, aContainingBlock, aOverflowAreas.as_deref());
        let passedContainingBlock = AbsoluteContainingBlock_ContainingBlockRects { mLocal: aContainingBlock, mScrollable: scrollableContainingBlock };

        // unfragmentedContainingBlockRects: the UnfragmentedContainingBlockProperty and the measuring
        // reflow are NOT PORTED (fragmentation); it is the passed rects.
        let unfragmentedContainingBlockRects = passedContainingBlock;

        // (prevInFlow's mCumulativeContainingBlockBSize: NOT PORTED, fragmentation.)

        // Assume all the kids may need a reflow when they are in a fragmented
        // context. We'll perform more targeted check below. For example, skip reflow
        // them when they are positioned in a later fragment.
        let reflowAll = aReflowInput.ShouldReflowAllKids() || aReflowInput.IsInFragmentedContext();
        let cbWidthChanged = aFlags.contains(AbsPosReflowFlag::CBWidthChanged);
        let cbHeightChanged = aFlags.contains(AbsPosReflowFlag::CBHeightChanged);
        let mut iter = self.mAbsoluteFrames.FirstChild();
        while let Some(kidFrame) = iter {
            // Advance the iterator first, so it's safe to move |kidFrame|.
            iter = kidFrame.GetNextSibling();
            // (kidFrame->HasAnchorPosReference() and the AnchorPosReferences property: NOT PORTED,
            // anchor positioning.)

            let kidNeedsReflow = reflowAll || kidFrame.IsSubtreeDirty() || self.FrameDependsOnContainer(kidFrame, cbWidthChanged, cbHeightChanged);
            if kidFrame.IsSubtreeDirty() {
                MaybeMarkAncestorsAsHavingDescendantDependentOnItsStaticPos(kidFrame, aDelegatingFrame);
            }
            if kidNeedsReflow {
                // (cbSize, border and cbBorderBoxSize are only used by the fragmentation code below.)

                // kidFrameNeedsPush: NOT PORTED, fragmentation (there is no unfragmented position).
                let mut kidStatus = nsReflowStatus::default();
                self.ReflowAbsoluteFrame(aDelegatingFrame, aPresContext, aReflowInput, &unfragmentedContainingBlockRects, aFlags, kidFrame, &mut kidStatus, aOverflowAreas.as_deref_mut(), None);

                // (mIsInFragmentainerMeasuringReflow: NOT PORTED, fragmentation.)
                MOZ_ASSERT!(!kidStatus.IsInlineBreakBefore(), "ShouldAvoidBreakInside should prevent this from happening");

                // The pushed and continued kids: NOT PORTED, fragmentation. Without an available
                // block-size a kid is always complete.
                let nextFrame = kidFrame.GetNextInFlow();
                if !kidStatus.IsFullyComplete() {
                    unimplemented!("NOT PORTED: abspos continuations (fragmentation)");
                } else if nextFrame.is_some() {
                    unimplemented!("NOT PORTED: abspos continuations (fragmentation)");
                }
            } else if let Some(o) = aOverflowAreas.as_deref_mut() {
                nsContainerFrame::from_frame(aDelegatingFrame).ConsiderChildOverflow(o, kidFrame, OverflowAreaUnionFlags::None);
            }

            // Make a CheckForInterrupt call, here, not just HasPendingInterrupt. ...
            // NOT PORTED: interruptible reflow.
        }

        // (newPushedAbsoluteFrames and mCumulativeContainingBlockBSize: NOT PORTED, fragmentation.)

        // Abspos frames can't cause their parent to be incomplete,
        // only overflow incomplete.
        if aReflowStatus.IsIncomplete() || self.mPushedAbsoluteFrames.NotEmpty() {
            aReflowStatus.SetOverflowIncomplete();
            aReflowStatus.SetNextInFlowNeedsReflow();
        }
    }

    /**
     * Returns true if the position of aFrame depends on the position of
     * its placeholder or if the position or size of aFrame depends on a
     * containing block dimension that changed.
     */
    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::FrameDependsOnContainer
    // ADAPTER: every reflow here is the first reflow of new frames, so every kid is dirty and
    // Reflow never asks; the answer is the conservative one.
    // NOT PORTED: the per-property checks (AbsoluteContainingBlock.cpp lines 854-982 at GECKO_REV).
    fn FrameDependsOnContainer(&self, aFrame: nsIFrame, aCBWidthChanged: bool, aCBHeightChanged: bool) -> bool {
        true
    }

    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::DestroyFrames
    pub fn DestroyFrames(&mut self, aContext: &mut FrameDestroyContext) {
        self.mAbsoluteFrames.DestroyFrames(aContext);
        self.mPushedAbsoluteFrames.DestroyFrames(aContext);
    }

    /**
     * Mark our size-dependent absolute frames with NS_FRAME_HAS_DIRTY_CHILDREN
     * so that we'll make sure to reflow them.
     */
    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::MarkSizeDependentFramesDirty
    pub fn MarkSizeDependentFramesDirty(&mut self) {
        self.DoMarkFramesDirty(false);
    }

    /**
     * Mark all our absolute frames with NS_FRAME_IS_DIRTY.
     */
    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::MarkAllFramesDirty
    pub fn MarkAllFramesDirty(&mut self) {
        self.DoMarkFramesDirty(true);
    }

    /**
     * Mark our absolute frames dirty.
     * @param aMarkAllDirty if true, all will be marked with NS_FRAME_IS_DIRTY.
     * Otherwise, the size-dependant ones will be marked with
     * NS_FRAME_HAS_DIRTY_CHILDREN.
     */
    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::DoMarkFramesDirty
    fn DoMarkFramesDirty(&mut self, aMarkAllDirty: bool) {
        for kidFrame in &self.mAbsoluteFrames {
            if aMarkAllDirty {
                kidFrame.MarkSubtreeDirty();
            } else if self.FrameDependsOnContainer(kidFrame, true, true) {
                // Add the weakest flags that will make sure we reflow this frame later
                kidFrame.AddStateBits(NS_FRAME_HAS_DIRTY_CHILDREN);
            }
        }
    }

    /**
     * After an abspos child's size is known, this method can be used to
     * resolve size-dependent values in the ComputedLogicalOffsets on its
     * reflow input.
     *
     * aCBSize, aKidSize, aMargin, aOffsets, are all expected in the absolute
     * containing block's writing-mode.
     *
     * aOffset is an outparam.
     */
    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::ResolveSizeDependentOffsets
    fn ResolveSizeDependentOffsets(&self, aKidReflowInput: &mut ReflowInput, aCBSize: LogicalSize, aKidSize: LogicalSize, aMargin: LogicalMargin, aAnchorOffsetInfo: &AbsoluteContainingBlock_AnchorOffsetInfo, aOffsets: &mut LogicalMargin) {
        let outerWM = aKidReflowInput.mParentReflowInput.unwrap().GetWritingMode();

        // Now that we know the child's size, we resolve any sentinel values in its
        // IStart/BStart offset coordinates that depend on that size.
        //  * NS_AUTOOFFSET indicates that the child's position in the given axis
        // is determined by its end-wards offset property, combined with its size and
        // available space. e.g.: "top: auto; height: auto; bottom: 50px"
        //  * m{I,B}OffsetsResolvedAfterSize indicate that the child is using its
        // static position in that axis, *and* its static position is determined by
        // the axis-appropriate css-align property (which may require the child's
        // size, e.g. to center it within the parent).
        if (NS_AUTOOFFSET == aOffsets.IStart(outerWM)) || (NS_AUTOOFFSET == aOffsets.BStart(outerWM)) || aKidReflowInput.mFlags.mIOffsetsNeedCSSAlign || aKidReflowInput.mFlags.mBOffsetsNeedCSSAlign {
            // placeholderContainer is used in each of the m{I,B}OffsetsNeedCSSAlign
            // clauses. We declare it at this scope so we can avoid having to look
            // it up twice (and only look it up if it's needed).
            let mut placeholderContainer: Option<nsIFrame> = None;

            if NS_AUTOOFFSET == aOffsets.IStart(outerWM) {
                NS_ASSERTION!(NS_AUTOOFFSET != aOffsets.IEnd(outerWM), "Can't solve for both start and end");
                *aOffsets.IStart_mut(outerWM) = aCBSize.ISize(outerWM) - aOffsets.IEnd(outerWM) - aMargin.IStartEnd(outerWM) - aKidSize.ISize(outerWM);
            } else if aKidReflowInput.mFlags.mIOffsetsNeedCSSAlign {
                placeholderContainer = GetPlaceholderContainer(aKidReflowInput.mFrame);
                let offset = OffsetToAlignedStaticPos(aKidReflowInput, aKidSize, aCBSize, placeholderContainer, outerWM, LogicalAxis::Inline, None, aAnchorOffsetInfo);
                // Shift IStart from its current position (at start corner of the
                // alignment container) by the returned offset.  And set IEnd to the
                // distance between the kid's end edge to containing block's end edge.
                *aOffsets.IStart_mut(outerWM) += offset;
                *aOffsets.IEnd_mut(outerWM) = aCBSize.ISize(outerWM) - (aOffsets.IStart(outerWM) + aKidSize.ISize(outerWM));
            }

            if NS_AUTOOFFSET == aOffsets.BStart(outerWM) {
                *aOffsets.BStart_mut(outerWM) = aCBSize.BSize(outerWM) - aOffsets.BEnd(outerWM) - aMargin.BStartEnd(outerWM) - aKidSize.BSize(outerWM);
            } else if aKidReflowInput.mFlags.mBOffsetsNeedCSSAlign {
                if placeholderContainer.is_none() {
                    placeholderContainer = GetPlaceholderContainer(aKidReflowInput.mFrame);
                }
                let offset = OffsetToAlignedStaticPos(aKidReflowInput, aKidSize, aCBSize, placeholderContainer, outerWM, LogicalAxis::Block, None, aAnchorOffsetInfo);
                // Shift BStart from its current position (at start corner of the
                // alignment container) by the returned offset.  And set BEnd to the
                // distance between the kid's end edge to containing block's end edge.
                *aOffsets.BStart_mut(outerWM) += offset;
                *aOffsets.BEnd_mut(outerWM) = aCBSize.BSize(outerWM) - (aOffsets.BStart(outerWM) + aKidSize.BSize(outerWM));
            }
            aKidReflowInput.SetComputedLogicalOffsets(outerWM, *aOffsets);
        }
    }

    /**
     * For frames that have intrinsic block sizes, since we want to use the
     * frame's actual instrinsic block-size, we don't compute margins in
     * InitAbsoluteConstraints because the block-size isn't computed yet. This
     * method computes the margins for them after layout.
     *
     * aCBSize, aKidSize, aMargin, aOffsets, are all expected in the absolute
     * containing block's writing-mode.
     *
     * aMargin and aOffsets are both outparams (though we only touch aOffsets if
     * the position is overconstrained)
     */
    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::ResolveAutoMarginsAfterLayout
    fn ResolveAutoMarginsAfterLayout(&self, aKidReflowInput: &mut ReflowInput, aCBSize: LogicalSize, aKidSize: LogicalSize, aMargin: &mut LogicalMargin, aOffsets: LogicalMargin) {
        let outerWM = aKidReflowInput.mParentReflowInput.unwrap().GetWritingMode();
        let styleMargin = aKidReflowInput.mStyleMargin.clone();
        let anchorResolutionParams = AnchorPosResolutionParams::From__SizeComputationInput_bool(&**aKidReflowInput, false);

        let mut ResolveMarginsInAxis = |aAxis: LogicalAxis, aMargin: &mut LogicalMargin| {
            let startSide = MakeLogicalSide(aAxis, LogicalEdge::Start);
            let endSide = MakeLogicalSide(aAxis, LogicalEdge::End);

            // No need to substract border sizes because aKidSize has it included
            // already. Also, if any offset is auto, the auto margin resolves to zero.
            // https://drafts.csswg.org/css-position-3/#abspos-margins
            let autoOffset = aOffsets.Side(startSide, outerWM) == NS_AUTOOFFSET || aOffsets.Side(endSide, outerWM) == NS_AUTOOFFSET;

            let availMarginSpace = if autoOffset {
                0
            } else {
                let stretchFitSize = std::cmp::max(0, aCBSize.Size(aAxis, outerWM) - aOffsets.StartEnd(aAxis, outerWM) - aMargin.StartEnd(aAxis, outerWM));
                stretchFitSize - aKidSize.Size(aAxis, outerWM)
            };

            let startSideMarginIsAuto = styleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(startSide, outerWM, &anchorResolutionParams).IsAuto();
            let endSideMarginIsAuto = styleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(endSide, outerWM, &anchorResolutionParams).IsAuto();

            if aAxis == LogicalAxis::Inline {
                ReflowInput::ComputeAbsPosInlineAutoMargin(availMarginSpace, outerWM, startSideMarginIsAuto, endSideMarginIsAuto, aMargin);
            } else {
                ReflowInput::ComputeAbsPosBlockAutoMargin(availMarginSpace, outerWM, startSideMarginIsAuto, endSideMarginIsAuto, aMargin);
            }
        };

        ResolveMarginsInAxis(LogicalAxis::Inline, aMargin);
        ResolveMarginsInAxis(LogicalAxis::Block, aMargin);
        aKidReflowInput.SetComputedLogicalMargin(outerWM, *aMargin);

        // InitOffsets should've created a UsedMarginProperty for us, if any margin is
        // auto.
        if aKidReflowInput.mFrame.HasProperty(nsIFrame::UsedMarginProperty()) {
            aKidReflowInput.mFrame.SetProperty(nsIFrame::UsedMarginProperty(), aMargin.GetPhysicalMargin(outerWM));
        }
    }

    // XXX Optimize the case where it's a resize reflow and the absolutely
    // positioned child has the exact same size and position and skip the
    // reflow...
    // Gecko: layout/generic/AbsoluteContainingBlock.cpp AbsoluteContainingBlock::ReflowAbsoluteFrame
    // NOT PORTED: the position-try fallback loop (position-try-fallbacks, position-try-order,
    // LastSuccessfulPositionFallback, AutoFallbackStyleSetter; anchor positioning), so the body runs
    // once with the base style; the fragmentation paths (unfragmented position and size, the
    // available block-size of a kid that may split; C2); menupopups; position-visibility.
    fn ReflowAbsoluteFrame(
        &mut self,
        aDelegatingFrame: nsIFrame,
        aPresContext: &nsPresContext,
        aReflowInput: &ReflowInput,
        aContainingBlockRects: &AbsoluteContainingBlock_ContainingBlockRects,
        aFlags: AbsPosReflowFlags,
        aKidFrame: nsIFrame,
        aStatus: &mut nsReflowStatus,
        aOverflowAreas: Option<&mut OverflowAreas>,
        aFragmentedContainingBlockRects: Option<&AbsoluteContainingBlock_ContainingBlockRects>,
    ) {
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        let outerWM = aReflowInput.GetWritingMode();
        let wm = aKidFrame.GetWritingMode();

        let isGrid = aFlags.contains(AbsPosReflowFlag::IsGridContainerCB);

        // (The fallbacks, the try order and the loop over them: NOT PORTED, see above.)
        let cb = ComputeContainingBlock(isGrid, aDelegatingFrame, aReflowInput, aContainingBlockRects, aKidFrame);
        let cbSize = LogicalSize::new__WritingMode_nsSize(outerWM, cb.mFinalRect.Size());

        let mut initFlags: ReflowInput_InitFlags = EnumSet::default();
        let staticPosIsCBOrigin = (|| {
            if aFlags.contains(AbsPosReflowFlag::IsGridContainerCB) {
                // When a grid container generates the abs.pos. CB for a *child* then
                // the static position is determined via CSS Box Alignment within the
                // abs.pos. CB (a grid area, i.e. a piece of the grid). In this
                // scenario, due to the multiple coordinate spaces in play, we use a
                // convenience flag to simply have the child's ReflowInput give it a
                // static position at its abs.pos. CB origin, and then we'll align &
                // offset it from there.
                let placeholder = aKidFrame.GetPlaceholderFrame();
                if let Some(placeholder) = placeholder {
                    if placeholder.GetParent() == Some(aDelegatingFrame) {
                        return true;
                    }
                }
            }
            // if (aKidFrame->IsMenuPopupFrame()): NOT PORTED, there are no popups.
            // TODO(emilio): Either reparent the top layer placeholder frames to the
            // viewport, or return true here for top layer frames more generally (not
            // only menupopups), see https://github.com/w3c/csswg-drafts/issues/8040.
            false
        })();

        if staticPosIsCBOrigin {
            initFlags += ReflowInput_InitFlag::StaticPosIsCBOrigin;
        }

        // kidFrameMaySplit: always false here, the available block-size is unconstrained (NOT PORTED:
        // fragmentation, C2).
        let kidFrameMaySplit = aReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE && aFlags.contains(AbsPosReflowFlag::AllowFragmentation) && !aDelegatingFrame.IsInlineFrame();
        if kidFrameMaySplit {
            unimplemented!("NOT PORTED: abspos frames that may split (fragmentation)");
        }

        // Get the border values
        let border = *aDelegatingFrame.GetLogicalUsedBorder(outerWM).ApplySkipSides(aDelegatingFrame.PreReflowBlockLevelLogicalSkipSides());

        let availBSize = NS_UNCONSTRAINEDSIZE;
        // (GetUnfragmentedSize: NOT PORTED, fragmentation; no size overrides.)
        let sizeOverrides = StyleSizeOverrides::default();
        let availSize = LogicalSize::new(outerWM, cbSize.ISize(outerWM), availBSize);
        let mut kidReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            aPresContext,
            aReflowInput,
            aKidFrame,
            availSize.ConvertTo(wm, outerWM),
            Some(cbSize.ConvertTo(wm, outerWM)),
            initFlags,
            sizeOverrides,
            ComputeSizeFlags::default(),
            std::ptr::null_mut(),
        );

        // (unfragmentedBSizeAsMinBSize: NOT PORTED, fragmentation.)

        // ReflowInput's constructor may change the available block-size to
        // unconstrained, e.g. in orthogonal reflow, so we retrieve it again and
        // account for kid's constraints in its own writing-mode if needed.
        // (kidPrevInFlow is always null here.)
        let mut kidAvailBSize = kidReflowInput.AvailableBSize();
        if kidAvailBSize != NS_UNCONSTRAINEDSIZE {
            kidAvailBSize -= kidReflowInput.ComputedLogicalMargin(wm).BStart(wm);
            let kidOffsetBStart = kidReflowInput.ComputedLogicalOffsets(wm).BStart(wm);
            if kidOffsetBStart != NS_AUTOOFFSET {
                kidAvailBSize -= kidOffsetBStart;
            }
            kidReflowInput.SetAvailableBSize(kidAvailBSize);
        }

        // Do the reflow
        let mut kidDesiredSize = ReflowOutput::new__ReflowInput(&kidReflowInput);
        aKidFrame.Reflow(aPresContext, &mut kidDesiredSize, &kidReflowInput, aStatus);

        // (IsMenuPopupFrame, and the unfragmented position or prev-in-flow branch: NOT PORTED.)
        let _ = (aFragmentedContainingBlockRects, border);
        {
            // Position the child relative to our padding edge.
            let kidSize = kidDesiredSize.Size(outerWM);

            let mut offsets = kidReflowInput.ComputedLogicalOffsets(outerWM);
            let mut margin = kidReflowInput.ComputedLogicalMargin(outerWM);

            // If we're doing CSS Box Alignment in either axis, that will apply the
            // margin for us in that axis (since the thing that's aligned is the
            // margin box).  So, we clear out the margin here to avoid applying it
            // twice.
            if kidReflowInput.mFlags.mIOffsetsNeedCSSAlign {
                *margin.IStart_mut(outerWM) = 0;
                *margin.IEnd_mut(outerWM) = 0;
            }
            if kidReflowInput.mFlags.mBOffsetsNeedCSSAlign {
                *margin.BStart_mut(outerWM) = 0;
                *margin.BEnd_mut(outerWM) = 0;
            }

            // If we're solving for start in either inline or block direction,
            // then compute it now that we know the dimensions.
            self.ResolveSizeDependentOffsets(&mut kidReflowInput, cbSize, kidSize, margin, &cb.GetAnchorOffsetInfo(), &mut offsets);

            self.ResolveAutoMarginsAfterLayout(&mut kidReflowInput, cbSize, kidSize, &mut margin, offsets);

            // If the inset is constrained as non-auto, we may have a child that does
            // not fill out the inset-reduced containing block. In this case, we need
            // to align the child by its margin box:
            // https://drafts.csswg.org/css-position-3/#abspos-layout
            let stylePos = aKidFrame.StylePosition();
            let anchorResolutionParams = AnchorPosOffsetResolutionParams::ExplicitCBFrameSize(AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aKidFrame, std::ptr::null_mut()), Some(cbSize));
            let iStartInsetAuto = stylePos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::IStart, outerWM, &anchorResolutionParams).IsAuto();
            let iEndInsetAuto = stylePos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::IEnd, outerWM, &anchorResolutionParams).IsAuto();
            let iInsetAuto = iStartInsetAuto || iEndInsetAuto;

            let bStartInsetAuto = stylePos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BStart, outerWM, &anchorResolutionParams).IsAuto();
            let bEndInsetAuto = stylePos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BEnd, outerWM, &anchorResolutionParams).IsAuto();
            let bInsetAuto = bStartInsetAuto || bEndInsetAuto;
            let kidMarginBox = LogicalSize::new(outerWM, margin.IStartEnd(outerWM) + kidSize.ISize(outerWM), margin.BStartEnd(outerWM) + kidSize.BSize(outerWM));
            let placeholderContainer = GetPlaceholderContainer(kidReflowInput.mFrame);

            // (insets: used only by the fallback fit check and anchor positioning, NOT PORTED.)
            if !iInsetAuto {
                MOZ_ASSERT!(!kidReflowInput.mFlags.mIOffsetsNeedCSSAlign, "Non-auto inline inset but requires CSS alignment for static position?");
                let alignOffset = OffsetToAlignedStaticPos(
                    &kidReflowInput,
                    kidMarginBox,
                    cbSize,
                    placeholderContainer,
                    outerWM,
                    LogicalAxis::Inline,
                    Some(NonAutoAlignParams { mCurrentStartInset: offsets.IStart(outerWM), mCurrentEndInset: offsets.IEnd(outerWM) }),
                    &cb.GetAnchorOffsetInfo(),
                );

                *offsets.IStart_mut(outerWM) += alignOffset;
                *offsets.IEnd_mut(outerWM) = cbSize.ISize(outerWM) - (offsets.IStart(outerWM) + kidMarginBox.ISize(outerWM));
            }
            if !bInsetAuto {
                MOZ_ASSERT!(!kidReflowInput.mFlags.mBOffsetsNeedCSSAlign, "Non-auto block inset but requires CSS alignment for static position?");
                let alignOffset = OffsetToAlignedStaticPos(
                    &kidReflowInput,
                    kidMarginBox,
                    cbSize,
                    placeholderContainer,
                    outerWM,
                    LogicalAxis::Block,
                    Some(NonAutoAlignParams { mCurrentStartInset: offsets.BStart(outerWM), mCurrentEndInset: offsets.BEnd(outerWM) }),
                    &cb.GetAnchorOffsetInfo(),
                );
                *offsets.BStart_mut(outerWM) += alignOffset;
                *offsets.BEnd_mut(outerWM) = cbSize.BSize(outerWM) - (offsets.BStart(outerWM) + kidMarginBox.BSize(outerWM));
            }

            let rect = LogicalRect::new__WritingMode_LogicalPoint_LogicalSize(outerWM, offsets.StartOffset(outerWM) + margin.StartOffset(outerWM), kidSize);
            let mut r = rect.GetPhysicalRect(outerWM, cbSize.GetPhysicalSize(outerWM));

            // So far, we've positioned against the padding edge of the containing
            // block, which is necessary for inset computation. However, the position
            // of a frame originates against the border box.
            r += cb.mFinalRect.TopLeft();

            // scrollShift: NOT PORTED, anchor positioning (always zero).
            aKidFrame.SetRect(r);
        }

        aKidFrame.DidReflow(aPresContext, Some(&kidReflowInput));

        // (firstTryRect, FitsInContainingBlock, the fallback bookkeeping and position-visibility:
        // NOT PORTED, see above.)

        if let Some(o) = aOverflowAreas {
            nsContainerFrame::from_frame(aDelegatingFrame).ConsiderChildOverflow(o, aKidFrame, OverflowAreaUnionFlags::ChildIsAbsPos);
        }
    }
}

// Gecko: layout/generic/AbsoluteContainingBlock.cpp MaybeMarkAncestorsAsHavingDescendantDependentOnItsStaticPos
// NOT PORTED: the DescendantMayDependOnItsStaticPosition bit is for restyling, which this runtime does
// not do (every flush builds new frames); AbsoluteContainingBlock.cpp lines 309-352 at GECKO_REV.
fn MaybeMarkAncestorsAsHavingDescendantDependentOnItsStaticPos(aFrame: nsIFrame, aContainingBlockFrame: nsIFrame) {
    MOZ_ASSERT!(aFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW));
}

// Gecko: layout/generic/AbsoluteContainingBlock.cpp ComputeScrollableContainingBlock
// NOT PORTED: scroll containers (the port has no ScrollContainerFrame, B39), so no delegating frame is
// `::-moz-scrolled-content` and the scrollable containing block is the local one.
fn ComputeScrollableContainingBlock(aDelegatingFrame: nsIFrame, aContainingBlock: nsRect, aOverflowAreas: Option<&OverflowAreas>) -> nsRect {
    aContainingBlock
}

// Gecko: layout/generic/AbsoluteContainingBlock.cpp ModifiedContainingBlock
// (mAnchorOffsetInfo: NOT PORTED, anchor positioning.)
struct ModifiedContainingBlock {
    // Unmodified scrollable or local containing block
    mMaybeScrollableRect: nsRect,
    // Containing block after all its modifications e.g. By grid/position-area.
    mFinalRect: nsRect,
}

impl ModifiedContainingBlock {
    fn new(aRect: nsRect) -> Self {
        ModifiedContainingBlock { mMaybeScrollableRect: aRect, mFinalRect: aRect }
    }

    fn new__nsRect_nsRect(aMaybeScrollableRect: nsRect, aFinalRect: nsRect) -> Self {
        ModifiedContainingBlock { mMaybeScrollableRect: aMaybeScrollableRect, mFinalRect: aFinalRect }
    }

    fn GetAnchorOffsetInfo(&self) -> AbsoluteContainingBlock_AnchorOffsetInfo {
        AbsoluteContainingBlock_AnchorOffsetInfo::default()
    }
}

// Gecko: layout/generic/AbsoluteContainingBlock.cpp ComputeContainingBlock
// NOT PORTED: the anchor positioning paths (the default anchor, position-area), the snapshot
// containing block of view transitions.
fn ComputeContainingBlock(aIsGrid: bool, aDelegatingFrame: nsIFrame, aReflowInput: &ReflowInput, aContainingBlockRects: &AbsoluteContainingBlock_ContainingBlockRects, aKidFrame: nsIFrame) -> ModifiedContainingBlock {
    // The current containing block, with ongoing modifications.
    // Starts as a local containing block.
    let mut containingBlock = aContainingBlockRects.mLocal;

    if aDelegatingFrame.Type() == LayoutFrameType::Viewport {
        MOZ_ASSERT!(aContainingBlockRects.mScrollable == aContainingBlockRects.mLocal);
        containingBlock = ViewportFrame::from_frame(aDelegatingFrame).GetContainingBlockAdjustedForScrollbars(aReflowInput);
    }

    // https://drafts.csswg.org/css-position/#original-cb
    // Handle grid-based adjustment first...
    if aIsGrid {
        let border = aDelegatingFrame.GetUsedBorder();
        let borderShift = nsPoint::new(border.left, border.top);
        let preGridCB = containingBlock;
        // Shift in by border of the overall grid container.
        containingBlock = nsGridContainerFrame::GridItemCB(aKidFrame) + borderShift;
        return ModifiedContainingBlock::new__nsRect_nsRect(preGridCB, containingBlock);
    }
    ModifiedContainingBlock::new(containingBlock)
}

// Given an out-of-flow frame, this method returns the parent frame of its
// placeholder frame or null if it doesn't have a placeholder for some reason.
// Gecko: layout/generic/AbsoluteContainingBlock.cpp GetPlaceholderContainer
fn GetPlaceholderContainer(aPositionedFrame: nsIFrame) -> Option<nsIFrame> {
    let placeholder = aPositionedFrame.GetPlaceholderFrame();
    placeholder.and_then(|p| p.GetParent())
}

// Gecko: layout/generic/AbsoluteContainingBlock.cpp NonAutoAlignParams
#[derive(Clone, Copy)]
struct NonAutoAlignParams {
    mCurrentStartInset: nscoord,
    mCurrentEndInset: nscoord,
}

/**
 * This function returns the offset of an abs/fixed-pos child's static
 * position, with respect to the "start" corner of its alignment container,
 * according to CSS Box Alignment.  This function only operates in a single
 * axis at a time -- callers can choose which axis via the |aAbsPosCBAxis|
 * parameter. This is called under two scenarios:
 * 1. We're statically positioning this absolutely positioned box, meaning
 *    that the offsets are auto and will change depending on the alignment
 *    of the box.
 * 2. The offsets are non-auto, but the element may not fill the inset-reduced
 *    containing block, so its margin box needs to be aligned in that axis.
 *    This is the step 4 of [1]. Should also be noted that, unlike static
 *    positioning, where we may confine the alignment area for flex/grid
 *    parent containers, we explicitly align to the inset-reduced absolute
 *    container size.
 *
 * [1]: https://drafts.csswg.org/css-position-3/#abspos-layout
 *
 * @param aKidReflowInput The ReflowInput for the to-be-aligned abspos child.
 * @param aKidSizeInAbsPosCBWM The child frame's size (after it's been given
 *                             the opportunity to reflow), in terms of
 *                             aAbsPosCBWM.
 * @param aAbsPosCBSize The abspos CB size, in terms of aAbsPosCBWM.
 * @param aPlaceholderContainer The parent of the child frame's corresponding
 *                              placeholder frame, cast to a nsContainerFrame.
 *                              (This will help us choose which alignment enum
 *                              we should use for the child.)
 * @param aAbsPosCBWM The child frame's containing block's WritingMode.
 * @param aAbsPosCBAxis The axis (of the containing block) that we should
 *                      be doing this computation for.
 * @param aNonAutoAlignParams Parameters, if specified, indicating that we're
 *                            handling scenario 2.
 */
// Gecko: layout/generic/AbsoluteContainingBlock.cpp OffsetToAlignedStaticPos
// NOT PORTED: the anchor-center paths (anchor positioning).
fn OffsetToAlignedStaticPos(
    aKidReflowInput: &ReflowInput,
    aKidSizeInAbsPosCBWM: LogicalSize,
    aAbsPosCBSize: LogicalSize,
    aPlaceholderContainer: Option<nsIFrame>,
    aAbsPosCBWM: WritingMode,
    aAbsPosCBAxis: LogicalAxis,
    aNonAutoAlignParams: Option<NonAutoAlignParams>,
    aAnchorOffsetInfo: &AbsoluteContainingBlock_AnchorOffsetInfo,
) -> nscoord {
    let Some(aPlaceholderContainer) = aPlaceholderContainer else {
        // (The placeholder container should be the thing that kicks this whole
        // process off, by setting PLACEHOLDER_STATICPOS_NEEDS_CSSALIGN.  So it
        // should exist... but bail gracefully if it doesn't.)
        NS_WARNING!("Missing placeholder-container when computing a CSS Box Alignment static position");
        return 0;
    };

    // (Most of this function is simply preparing args that we'll pass to
    // AlignJustifySelf at the end.)

    // NOTE: Our alignment container is aPlaceholderContainer's content-box
    // (or an area within it, if aPlaceholderContainer is a grid). So, we'll
    // perform most of our arithmetic/alignment in aPlaceholderContainer's
    // WritingMode. For brevity, we use the abbreviation "pc" for "placeholder
    // container" in variables below.
    let pcWM = aPlaceholderContainer.GetWritingMode();
    let absPosCBSizeInPCWM = aAbsPosCBSize.ConvertTo(pcWM, aAbsPosCBWM);

    // Find what axis aAbsPosCBAxis corresponds to, in placeholder's parent's
    // writing-mode.
    let pcAxis = aAbsPosCBWM.ConvertAxisTo(aAbsPosCBAxis, pcWM);
    let alignAreaSize = (|| {
        if aNonAutoAlignParams.is_none() {
            let placeholderContainerIsContainingBlock = Some(aPlaceholderContainer) == aKidReflowInput.mCBReflowInput.map(|r| r.mFrame);

            let parentType = aPlaceholderContainer.Type();
            let mut alignAreaSize = LogicalSize::new__WritingMode(pcWM);
            if parentType == LayoutFrameType::FlexContainer {
                // We store the frame rect in FinishAndStoreOverflow, which runs _after_
                // reflowing the absolute frames, so handle the special case of the
                // frame being the actual containing block here, by getting the size
                // from aAbsPosCBSize.
                //
                // The alignment container is the flex container's content box.
                if placeholderContainerIsContainingBlock {
                    alignAreaSize = aAbsPosCBSize.ConvertTo(pcWM, aAbsPosCBWM);
                    // aAbsPosCBSize is the padding-box, so substract the padding to get
                    // the content box.
                    alignAreaSize -= aPlaceholderContainer.GetLogicalUsedPadding(pcWM).Size(pcWM);
                } else {
                    alignAreaSize = aPlaceholderContainer.GetLogicalSize__WritingMode(pcWM);
                    let pcBorderPadding = aPlaceholderContainer.GetLogicalUsedBorderAndPadding(pcWM);
                    alignAreaSize -= pcBorderPadding.Size(pcWM);
                }
                return alignAreaSize;
            }
            if parentType == LayoutFrameType::GridContainer {
                // This abspos elem's parent is a grid container. Per CSS Grid 10.1
                // & 10.2:
                //  - If the grid container *also* generates the abspos containing block
                //  (a
                // grid area) for this abspos child, we use that abspos containing block
                // as the alignment container, too. (And its size is aAbsPosCBSize.)
                //  - Otherwise, we use the grid's padding box as the alignment
                //  container.
                // https://drafts.csswg.org/css-grid/#static-position
                if placeholderContainerIsContainingBlock {
                    // The alignment container is the grid area that we're using as the
                    // absolute containing block.
                    alignAreaSize = aAbsPosCBSize.ConvertTo(pcWM, aAbsPosCBWM);
                } else {
                    // The alignment container is a the grid container's content box
                    // (which we can get by subtracting away its border & padding from
                    // frame's size):
                    alignAreaSize = aPlaceholderContainer.GetLogicalSize__WritingMode(pcWM);
                    let pcBorderPadding = aPlaceholderContainer.GetLogicalUsedBorderAndPadding(pcWM);
                    alignAreaSize -= pcBorderPadding.Size(pcWM);
                }
                return alignAreaSize;
            }
        }
        // Either we're in scenario 1 but within a non-flex/grid parent, or in
        // scenario 2.
        aAbsPosCBSize.ConvertTo(pcWM, aAbsPosCBWM)
    })();

    let existingOffset = match aNonAutoAlignParams {
        Some(p) => p.mCurrentStartInset + p.mCurrentEndInset,
        None => 0,
    };
    let alignAreaSizeInAxis = (if pcAxis == LogicalAxis::Inline { alignAreaSize.ISize(pcWM) } else { alignAreaSize.BSize(pcWM) }) - existingOffset;

    let mut flags: AlignJustifyFlags = EnumSet::default();
    flags += CSSAlignUtils_AlignJustifyFlag::IgnoreAutoMargins;
    // Given that scenario 2 ignores the parent container type, special handling
    // of absolutely-positioned child is also ignored.
    let mut alignConst = if aNonAutoAlignParams.is_some() {
        nsContainerFrame::from_frame(aPlaceholderContainer).CSSAlignmentForAbsPosChildWithinContainingBlock(&**aKidReflowInput, pcAxis, absPosCBSizeInPCWM)
    } else {
        nsContainerFrame::from_frame(aPlaceholderContainer).CSSAlignmentForAbsPosChild(aKidReflowInput, pcAxis)
    };
    // If the safe bit in alignConst is set, set the safe flag in |flags|.
    let safetyBits = alignConst & (StyleAlignFlags::SAFE | StyleAlignFlags::UNSAFE);
    alignConst &= !StyleAlignFlags::FLAG_BITS;
    if safetyBits.contains(StyleAlignFlags::SAFE) {
        flags += CSSAlignUtils_AlignJustifyFlag::OverflowSafe;
    }

    // Find out if placeholder-container & the OOF child have the same start-sides
    // in the placeholder-container's pcAxis.
    let kidWM = aKidReflowInput.GetWritingMode();
    if pcWM.ParallelAxisStartsOnSameSide(pcAxis, &kidWM) {
        flags += CSSAlignUtils_AlignJustifyFlag::SameSide;
    }

    if aNonAutoAlignParams.is_some() {
        flags += CSSAlignUtils_AlignJustifyFlag::AligningMarginBox;
    }

    // (baselineAdjust is unused. CSSAlignmentForAbsPosChild() should've
    // converted 'baseline'/'last baseline' enums to their fallback values.)
    let baselineAdjust: nscoord = 0;

    // AlignJustifySelf operates in the kid's writing mode, so we need to
    // represent the child's size and the desired axis in that writing mode:
    let kidSizeInOwnWM = aKidSizeInAbsPosCBWM.ConvertTo(kidWM, aAbsPosCBWM);
    let kidAxis = aAbsPosCBWM.ConvertAxisTo(aAbsPosCBAxis, kidWM);

    // Build an Inset Modified anchor info from the anchor which can be used to
    // align to the anchor-center, if AlignJustifySelf is AnchorCenter.
    // NOT PORTED: anchor positioning (no anchor-center alignment info).
    let anchorAlignInfo: Option<CSSAlignUtils_AnchorAlignInfo> = None;

    let mut offset = CSSAlignUtils::AlignJustifySelf_WithAnchor(&alignConst, kidAxis, flags, baselineAdjust, alignAreaSizeInAxis, aKidReflowInput, &kidSizeInOwnWM, &anchorAlignInfo);

    // Safe alignment clamping for anchor-center: NOT PORTED, anchor positioning.

    let rawAlignConst = if pcAxis == LogicalAxis::Inline { aKidReflowInput.mStylePosition.mJustifySelf().0 } else { aKidReflowInput.mStylePosition.mAlignSelf().0 };
    if let Some(params) = aNonAutoAlignParams {
        if safetyBits.is_empty() && rawAlignConst != StyleAlignFlags::AUTO {
            // No `safe` or `unsafe` specified - "in-between" behaviour for relevant
            // alignment values: https://drafts.csswg.org/css-position-3/#abspos-layout
            // Skip if the raw self alignment for this element is `auto` to preserve
            // legacy behaviour, except in the case where the resolved value is
            // anchor-center (where "legacy behavior" is not a concern).
            // Follows https://drafts.csswg.org/css-align-3/#auto-safety-position
            let cbSize = aAbsPosCBSize.Size(aAbsPosCBAxis, aAbsPosCBWM);
            // IMCB stands for "Inset-Modified Containing Block."
            let imcbStart = params.mCurrentStartInset;
            let imcbEnd = cbSize - params.mCurrentEndInset;
            // Need to pull the offset into the "current view," unless it already did.
            let scrollOffset = if aAbsPosCBWM.PhysicalAxis(aAbsPosCBAxis) == PhysicalAxis::Horizontal { aAnchorOffsetInfo.mScrollOffset.x } else { aAnchorOffsetInfo.mScrollOffset.y };
            let kidSize = aKidSizeInAbsPosCBWM.Size(aAbsPosCBAxis, aAbsPosCBWM);
            let kidStart = params.mCurrentStartInset + offset - scrollOffset;
            let kidEnd = kidStart + kidSize;
            // "[...] the overflow limit rect is the bounding rectangle of the alignment
            // subject’s inset-modified containing block and its original containing
            // block."
            let overflowLimitRectStart = std::cmp::min(0, imcbStart);
            let overflowLimitRectEnd = std::cmp::max(cbSize, imcbEnd);

            if kidStart >= imcbStart && kidEnd <= imcbEnd {
                // 1. We fit inside the IMCB, no action needed.
            } else if kidSize <= overflowLimitRectEnd - overflowLimitRectStart {
                // 2. We overflowed IMCB, try to cover IMCB completely, if it's not.
                if kidStart <= imcbStart && kidEnd >= imcbEnd {
                    // IMCB already covered, ensure that we aren't escaping the limit rect.
                    if kidStart < overflowLimitRectStart {
                        offset += overflowLimitRectStart - kidStart;
                    } else if kidEnd > overflowLimitRectEnd {
                        offset -= kidEnd - overflowLimitRectEnd;
                    }
                } else if kidEnd < imcbEnd && kidStart < imcbStart {
                    // Space to end, overflowing on start - nudge to end.
                    offset += std::cmp::min(imcbStart - kidStart, imcbEnd - kidEnd);
                } else if kidStart > imcbStart && kidEnd > imcbEnd {
                    // Space to start, overflowing on end - nudge to start.
                    offset -= std::cmp::min(kidEnd - imcbEnd, kidStart - imcbStart);
                }
            } else {
                // 3. We'll overflow the limit rect. Start align the subject int overflow
                // limit rect.
                offset = -params.mCurrentStartInset + overflowLimitRectStart;
            }
        }
    }

    // "offset" is in terms of the CSS Box Alignment container (i.e. it's in
    // terms of pcWM). But our return value needs to in terms of the containing
    // block's writing mode, which might have the opposite directionality in the
    // given axis. In that case, we just need to negate "offset" when returning,
    // to make it have the right effect as an offset for coordinates in the
    // containing block's writing mode.
    if !pcWM.ParallelAxisStartsOnSameSide(pcAxis, &aAbsPosCBWM) {
        return -offset;
    }
    offset
}
