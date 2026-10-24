/* class that a parent frame uses to reflow a block frame */
// Translated from layout/generic/nsBlockReflowContext.h and nsBlockReflowContext.cpp (GECKO_REV).
//
// Margin collapsing (the generational block-start margin walk, the block-end margin and the
// empty-block rule), placement of a reflowed block and the container's float-manager translation.
// NOT PORTED: columns (the nsColumnSetFrame descent), fragmentation (deleting a complete child's
// next-in-flows through nsOverflowContinuationTracker), noisy block-direction margin printing
// (NOISY_BLOCK_DIR_MARGINS) and the absurd-size / uninitialized-metrics debug prints.
//
// nsBlockFrame is nsBlockFrame.rs; nsContainerFrame::FinishReflowChild is in nsContainerFrame.rs.
use crate::*;

/**
 * An encapsulation of the state and algorithm for reflowing block frames.
 */
// Gecko: layout/generic/nsBlockReflowContext.h nsBlockReflowContext
pub struct nsBlockReflowContext<'a> {
    // protected:
    mPresContext: nsPresContext,
    mOuterReflowInput: &'a ReflowInput<'a>,

    // `nsIFrame*`, null until ReflowBlock.
    mFrame: Option<nsIFrame>,
    mSpace: LogicalRect,

    mICoord: nscoord,
    mBCoord: nscoord,
    mContainerSize: nsSize,
    mWritingMode: WritingMode,
    mMetrics: ReflowOutput,
    mBStartMargin: CollapsingMargin,
}

impl<'a> nsBlockReflowContext<'a> {
    // Gecko: layout/generic/nsBlockReflowContext.cpp nsBlockReflowContext::nsBlockReflowContext
    pub fn new(aPresContext: nsPresContext, aParentRI: &'a ReflowInput<'a>) -> nsBlockReflowContext<'a> {
        nsBlockReflowContext {
            mPresContext: aPresContext,
            mOuterReflowInput: aParentRI,
            mFrame: None,
            mSpace: LogicalRect::new__WritingMode(aParentRI.GetWritingMode()),
            mICoord: 0,
            mBCoord: 0,
            mContainerSize: nsSize::new(0, 0),
            mWritingMode: WritingMode::default(),
            mMetrics: ReflowOutput::new__ReflowInput(aParentRI),
            mBStartMargin: CollapsingMargin::default(),
        }
    }

    // Gecko: layout/generic/nsBlockReflowContext.h nsBlockReflowContext::GetCarriedOutBEndMargin
    pub fn GetCarriedOutBEndMargin(&mut self) -> &mut CollapsingMargin {
        &mut self.mMetrics.mCarriedOutBEndMargin
    }

    // Gecko: layout/generic/nsBlockReflowContext.h nsBlockReflowContext::GetMetrics
    pub fn GetMetrics(&self) -> &ReflowOutput {
        &self.mMetrics
    }

    /**
     * Computes the collapsed block-start margin (in the context's parent's
     * writing mode) for a block whose reflow input is in aRI.
     * The computed margin is added into aMargin, whose writing mode is the
     * parent's mode as found in mMetrics.GetWritingMode(); note this may not be
     * the block's own writing mode as found in aRI.
     * If aClearanceFrame is null then this is the first optimistic pass which
     * shall assume that no frames have clearance, and we clear the HasClearance
     * on all frames encountered.
     * If non-null, this is the second pass and the caller has decided
     * aClearanceFrame needs clearance (and we will therefore stop collapsing
     * there); also, this function is responsible for marking it with
     * SetHasClearance.
     * If in the optimistic pass any frame is encountered that might possibly
     * need clearance (i.e., if we really needed the optimism assumption) then
     * we set aMayNeedRetry to true.
     * We return true if we changed the clearance state of any line and marked it
     * dirty.
     */
    // (aBlockIsEmpty defaults to nullptr in C++: pass None.)
    // Gecko: layout/generic/nsBlockReflowContext.cpp nsBlockReflowContext::ComputeCollapsedBStartMargin
    pub fn ComputeCollapsedBStartMargin(&self, aRI: &ReflowInput, aMargin: &mut CollapsingMargin, aClearanceFrame: Option<nsIFrame>, aMayNeedRetry: &mut bool, mut aBlockIsEmpty: Option<&mut bool>) -> bool {
        let wm = aRI.GetWritingMode();
        let parentWM = self.mMetrics.GetWritingMode();

        // Include block-start element of frame's margin
        aMargin.Include__nscoord(aRI.ComputedLogicalMargin(parentWM).BStart(parentWM));

        // The inclusion of the block-end margin when empty is done by the caller
        // since it doesn't need to be done by the top-level (non-recursive)
        // caller.

        // NOT PORTED: NOISY_BLOCK_DIR_MARGINS printing.

        let mut dirtiedLine = false;
        let mut setBlockIsEmpty = false;

        // Calculate the frame's generational block-start-margin from its child
        // blocks. Note that if the frame has a non-zero block-start-border or
        // block-start-padding then this step is skipped because it will be a margin
        // root.  It is also skipped if the frame is a margin root for other
        // reasons.
        let frame: nsIFrame = DescendIntoBlockLevelFrame(aRI.mFrame);
        let prescontext = frame.PresContext();
        let mut block: Option<nsBlockFrame> = None;
        if 0 == aRI.ComputedLogicalBorderPadding(wm).BStart(wm) {
            block = nsBlockFrame::do_QueryFrame(frame);
            if let Some(b) = block {
                let (mut bStartMarginRoot, mut unused) = (false, false);
                b.IsMarginRoot(&mut bStartMarginRoot, &mut unused);
                if bStartMarginRoot {
                    block = None;
                }
            }
        }

        // iterate not just through the lines of 'block' but also its
        // overflow lines and the normal and overflow lines of its next in
        // flows. Note that this will traverse some frames more than once:
        // for example, if A contains B and A->nextinflow contains
        // B->nextinflow, we'll traverse B->nextinflow twice. But this is
        // OK because our traversal is idempotent.
        // (`goto done` is `break 'done`.)
        'done: {
            while let Some(blk) = block {
                for overflowLines in 0..=1 {
                    let mut line;
                    let line_end;
                    let mut anyLines = true;
                    if overflowLines != 0 {
                        let frames = blk.GetOverflowLines();
                        if frames.is_null() {
                            anyLines = false;
                            line = blk.LinesEnd();
                            line_end = blk.LinesEnd();
                        } else {
                            let frames = unsafe { &mut *frames };
                            line = frames.mLines.begin_mut();
                            line_end = frames.mLines.end_mut();
                        }
                    } else {
                        line = blk.LinesBegin();
                        line_end = blk.LinesEnd();
                    }
                    while anyLines && line != line_end {
                        if aClearanceFrame.is_none() && line.HasClearance() {
                            // If we don't have a clearance frame, then we're computing
                            // the collapsed margin in the first pass, assuming that all
                            // lines have no clearance. So clear their clearance flags.
                            line.ClearHasClearance();
                            line.MarkDirty();
                            dirtiedLine = true;
                        }

                        let mut isEmpty: bool = false;
                        if line.IsInline() {
                            isEmpty = line.IsEmpty();
                        } else {
                            let kid: nsIFrame = line.mFirstChild.unwrap();
                            if Some(kid) == aClearanceFrame {
                                line.SetHasClearance();
                                line.MarkDirty();
                                dirtiedLine = true;
                                if !setBlockIsEmpty {
                                    if let Some(p) = aBlockIsEmpty.as_mut() {
                                        setBlockIsEmpty = true;
                                        **p = false;
                                    }
                                }
                                break 'done;
                            }
                            // Here is where we recur. Now that we have determined that a
                            // generational collapse is required we need to compute the
                            // child blocks margin and so in so that we can look into
                            // it. For its margins to be computed we need to have a reflow
                            // input for it.

                            // We may have to construct an extra reflow input here if
                            // we drilled down through a block wrapper. At the moment
                            // we can only drill down one level so we only have to support
                            // one extra reflow input.
                            // (`new ReflowInput` / `delete` is an owned Option<Box<..>> here.)
                            let mut ownedOuterReflowInput: Option<Box<ReflowInput>> = None;
                            if frame != aRI.mFrame {
                                NS_ASSERTION!(frame.GetParent() == Some(aRI.mFrame), "Can only drill through one level of block wrapper");
                                let availSpace = aRI.ComputedSize__WritingMode(frame.GetWritingMode());
                                ownedOuterReflowInput = Some(Box::new(ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                                    &prescontext,
                                    aRI,
                                    frame,
                                    availSpace,
                                    None,
                                    EnumSet::default(),
                                    StyleSizeOverrides::default(),
                                    ComputeSizeFlags::default(),
                                    std::ptr::null_mut(),
                                )));
                            }
                            let outerReflowInput: &ReflowInput = match ownedOuterReflowInput.as_ref() {
                                Some(owned) => owned,
                                None => aRI,
                            };
                            {
                                let availSpace = outerReflowInput.ComputedSize__WritingMode(kid.GetWritingMode());
                                let innerReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                                    &prescontext,
                                    outerReflowInput,
                                    kid,
                                    availSpace,
                                    None,
                                    EnumSet::default(),
                                    StyleSizeOverrides::default(),
                                    ComputeSizeFlags::default(),
                                    std::ptr::null_mut(),
                                );
                                // Record that we're being optimistic by assuming the kid
                                // has no clearance
                                if kid.StyleDisplay().mClear() != StyleClear::None || !nsBlockFrame::BlockCanIntersectFloats(kid) {
                                    *aMayNeedRetry = true;
                                }
                                if self.ComputeCollapsedBStartMargin(&innerReflowInput, aMargin, aClearanceFrame, aMayNeedRetry, Some(&mut isEmpty)) {
                                    line.MarkDirty();
                                    dirtiedLine = true;
                                }
                                if isEmpty {
                                    let innerMargin = innerReflowInput.ComputedLogicalMargin(parentWM);
                                    aMargin.Include__nscoord(innerMargin.BEnd(parentWM));
                                }
                            }
                            // (if (outerReflowInput != &aRI) delete ...: ownedOuterReflowInput drops here.)
                        }
                        if !isEmpty {
                            if !setBlockIsEmpty {
                                if let Some(p) = aBlockIsEmpty.as_mut() {
                                    setBlockIsEmpty = true;
                                    **p = false;
                                }
                            }
                            break 'done;
                        }
                        line.operator_inc();
                    }
                    if !setBlockIsEmpty {
                        if let Some(p) = aBlockIsEmpty.as_mut() {
                            // The first time we reach here is when this is the first block
                            // and we have processed all its normal lines.
                            setBlockIsEmpty = true;
                            // All lines are empty, or we wouldn't be here!
                            **p = aRI.mFrame.IsSelfEmpty();
                        }
                    }
                }
                block = blk.GetNextInFlow().map(nsBlockFrame::from_frame);
            }
        }
        // done:

        if !setBlockIsEmpty {
            if let Some(p) = aBlockIsEmpty.as_mut() {
                **p = aRI.mFrame.IsEmpty();
            }
        }

        // NOT PORTED: NOISY_BLOCK_DIR_MARGINS printing.

        dirtiedLine
    }

    // Gecko: layout/generic/nsBlockReflowContext.cpp nsBlockReflowContext::ReflowBlock
    pub fn ReflowBlock(
        &mut self,
        aSpace: LogicalRect,
        aApplyBStartMargin: bool,
        aPrevMargin: &mut CollapsingMargin,
        aClearance: nscoord,
        aLine: Option<&nsLineBox>,
        aFrameRI: &mut ReflowInput,
        aFrameReflowStatus: &mut nsReflowStatus,
        aState: &BlockReflowState,
    ) {
        self.mFrame = Some(aFrameRI.mFrame);
        let mFrame = aFrameRI.mFrame;
        self.mWritingMode = aState.mReflowInput.GetWritingMode();
        self.mContainerSize = aState.ContainerSize();
        self.mSpace = aSpace;

        if !aState.IsAdjacentWithBStart() {
            aFrameRI.mFlags.mIsTopOfPage = false; // make sure this is cleared
        }

        if aApplyBStartMargin {
            self.mBStartMargin = *aPrevMargin;

            // NOT PORTED: NOISY_BLOCK_DIR_MARGINS printing.

            // Adjust the available size if it's constrained so that the
            // child frame doesn't think it can reflow into its margin area.
            if self.mWritingMode.IsOrthogonalTo(&mFrame.GetWritingMode()) {
                if NS_UNCONSTRAINEDSIZE != aFrameRI.AvailableISize() {
                    aFrameRI.SetAvailableISize(std::cmp::max(0, aFrameRI.AvailableISize() - self.mBStartMargin.Get() - aClearance));
                }
            } else {
                if NS_UNCONSTRAINEDSIZE != aFrameRI.AvailableBSize() {
                    aFrameRI.SetAvailableBSize(std::cmp::max(0, aFrameRI.AvailableBSize() - self.mBStartMargin.Get() - aClearance));
                }
            }
        } else {
            // nsBlockFrame::ReflowBlock might call us multiple times with
            // *different* values of aApplyBStartMargin.
            self.mBStartMargin.Zero();
        }

        let mut tI: nscoord = 0;
        let mut tB: nscoord = 0;
        // The values of x and y do not matter for floats, so don't bother
        // calculating them. Floats are guaranteed to have their own float
        // manager, so tI and tB don't matter.  mICoord and mBCoord don't
        // matter becacuse they are only used in PlaceBlock, which is not used
        // for floats.
        if let Some(aLine) = aLine {
            // Compute inline/block coordinate where reflow will begin. Use the
            // rules from 10.3.3 to determine what to apply. At this point in the
            // reflow auto inline-start/end margins will have a zero value.
            let usedMargin = aFrameRI.ComputedLogicalMargin(self.mWritingMode);
            self.mICoord = self.mSpace.IStart(self.mWritingMode) + usedMargin.IStart(self.mWritingMode);
            self.mBCoord = self.mSpace.BStart(self.mWritingMode) + self.mBStartMargin.Get() + aClearance;

            let space = LogicalRect::new(
                self.mWritingMode,
                self.mICoord,
                self.mBCoord,
                self.mSpace.ISize(self.mWritingMode) - usedMargin.IStartEnd(self.mWritingMode),
                self.mSpace.BSize(self.mWritingMode) - usedMargin.BStartEnd(self.mWritingMode),
            );
            tI = space.LineLeft(self.mWritingMode, self.mContainerSize);
            tB = self.mBCoord;

            if !mFrame.HasAnyStateBits(NS_BLOCK_BFC) {
                aFrameRI.mBlockDelta = self.mOuterReflowInput.mBlockDelta + self.mBCoord - aLine.BStart();
            }
        }

        #[cfg(debug_assertions)]
        {
            *self.mMetrics.ISize_mut(self.mWritingMode) = 0xdeadbeef_u32 as nscoord;
            *self.mMetrics.BSize_mut(self.mWritingMode) = 0xdeadbeef_u32 as nscoord;
        }

        // (mFloatManager is the shared raw pointer of ReflowInput.)
        unsafe { &mut *self.mOuterReflowInput.mFloatManager }.Translate(tI, tB);
        mFrame.Reflow(&self.mPresContext, &mut self.mMetrics, aFrameRI, aFrameReflowStatus);
        unsafe { &mut *self.mOuterReflowInput.mFloatManager }.Translate(-tI, -tB);

        // NOT PORTED: absurd-size and "didn't set i/b" debug printing (#ifdef DEBUG).

        if !mFrame.HasOverflowAreas() {
            self.mMetrics.SetOverflowAreasToDesiredBounds();
        }

        if !aFrameReflowStatus.IsInlineBreakBefore() && !aFrameRI.WillReflowAgainForClearance() && aFrameReflowStatus.IsFullyComplete() {
            // If mFrame is fully-complete and has a next-in-flow, we need to delete
            // them now. Do not do this when a break-before is signaled or when a
            // clearance frame is discovered in mFrame's subtree because mFrame is going
            // to get reflowed again (whether the frame is (in)complete is undefined in
            // that case anyway).
            if let Some(kidNextInFlow) = mFrame.GetNextInFlow() {
                // Remove all of the childs next-in-flows. Make sure that we ask
                // the right parent to do the removal (it's possible that the
                // parent is not this because we are executing pullup code).
                // Floats will eventually be removed via nsBlockFrame::RemoveFloat
                // which detaches the placeholder from the float.
                // NOT PORTED: fragmentation. nsOverflowContinuationTracker::AutoFinish fini(
                //     aState.mOverflowTracker, mFrame);
                // nsIFrame::DestroyContext context(mPresContext->PresShell());
                // kidNextInFlow->GetParent()->DeleteNextInFlowChild(context, kidNextInFlow, true);
                unimplemented!("NOT PORTED: fragmentation (DeleteNextInFlowChild of a complete block's next-in-flow)")
            }
        }
    }

    /**
     * Attempt to place the block frame within the available space.  If
     * it fits, apply inline-dir ("horizontal") positioning (CSS 10.3.3),
     * collapse margins (CSS2 8.3.1). Also apply relative positioning.
     */
    // Gecko: layout/generic/nsBlockReflowContext.cpp nsBlockReflowContext::PlaceBlock
    pub fn PlaceBlock(
        &mut self,
        aReflowInput: &ReflowInput,
        aForceFit: bool,
        aLine: &mut nsLineBox,
        aBEndMarginResult: &mut CollapsingMargin,
        aOverflowAreas: &mut OverflowAreas,
        aReflowStatus: &nsReflowStatus,
    ) -> bool {
        let mFrame = self.mFrame.unwrap();
        // Compute collapsed block-end margin value.
        let parentWM = self.mMetrics.GetWritingMode();

        // Don't apply the block-end margin if the block has a *later* sibling across
        // column-span split.
        if aReflowStatus.IsComplete() && !mFrame.HasColumnSpanSiblings() {
            *aBEndMarginResult = self.mMetrics.mCarriedOutBEndMargin;
            aBEndMarginResult.Include__nscoord(aReflowInput.ComputedLogicalMargin(parentWM).BEnd(parentWM));
        } else {
            // The used block-end-margin is set to zero before a break.
            aBEndMarginResult.Zero();
        }

        let mut backupContainingBlockAdvance: nscoord = 0;

        // Check whether the block's block-end margin collapses with its block-start
        // margin. See CSS 2.1 section 8.3.1; those rules seem to match
        // nsBlockFrame::IsEmpty(). Any such block must have zero block-size so
        // check that first. Note that a block can have clearance and still
        // have adjoining block-start/end margins, because the clearance goes
        // above the block-start margin.
        // Mark the frame as non-dirty; it has been reflowed (or we wouldn't
        // be here), and we don't want to assert in CachedIsEmpty()
        mFrame.RemoveStateBits(NS_FRAME_IS_DIRTY);
        let empty = 0 == self.mMetrics.BSize(parentWM) && aLine.CachedIsEmpty();
        if empty {
            // Collapse the block-end margin with the block-start margin that was
            // already applied.
            aBEndMarginResult.Include__CollapsingMargin(&self.mBStartMargin);

            // NOT PORTED: NOISY_BLOCK_DIR_MARGINS printing.
            // Section 8.3.1 of CSS 2.1 says that blocks with adjoining
            // "top/bottom" (i.e. block-start/end) margins whose top margin collapses
            // with their parent's top margin should have their top border-edge at the
            // top border-edge of their parent. We actually don't have to do
            // anything special to make this happen. In that situation,
            // nsBlockFrame::ShouldApplyBStartMargin will have returned false,
            // and mBStartMargin and aClearance will have been zero in
            // ReflowBlock.

            // If we did apply our block-start margin, but now we're collapsing it
            // into the block-end margin, we need to back up the containing
            // block's bCoord-advance by our block-start margin so that it doesn't get
            // counted twice. Note that here we're allowing the line's bounds
            // to become different from the block's position; we do this
            // because the containing block will place the next line at the
            // line's BEnd, and it must place the next line at a different
            // point from where this empty block will be.
            backupContainingBlockAdvance = self.mBStartMargin.Get();
        }

        // See if the frame fit. If it's the first frame or empty then it
        // always fits. If the block-size is unconstrained then it always fits,
        // even if there's some sort of integer overflow that makes bCoord +
        // mMetrics.BSize() appear to go beyond the available block size.
        let fragmentBStart = self.mBCoord - backupContainingBlockAdvance;
        if !empty && !aForceFit && self.mSpace.BSize(self.mWritingMode) != NS_UNCONSTRAINEDSIZE {
            let bSize = self.mMetrics.BSize(self.mWritingMode);
            let bEnd = fragmentBStart + bSize;
            if bEnd > self.mSpace.BEnd(self.mWritingMode) {
                // didn't fit, we must acquit.
                mFrame.DidReflow(&self.mPresContext, Some(aReflowInput));
                return false;
            }
            if bSize == 0 && aReflowStatus.IsIncomplete() && fragmentBStart == self.mSpace.BStart(self.mWritingMode) && !mFrame.HasAbsolutelyPositionedChildren() {
                // Reject a zero-height fragment of an incomplete block when it is the
                // first content in the fragmentainer. Per CSS Fragmentation spec, a
                // Class C break point exists only when there is a non-zero gap between
                // the content edge of a block container box and the outer edges of its
                // child content. If the fragment is at the very start of the available
                // space (no preceding siblings, no clearance gap, no margin gap), there
                // is no such gap, so we push the block to the next fragmentainer.
                // Exception: keep a zero-height fragment that serves as the containing
                // block origin for absolutely-positioned children.
                // https://www.w3.org/TR/css-break-3/#end-block
                mFrame.DidReflow(&self.mPresContext, Some(aReflowInput));
                return false;
            }
        }

        aLine.SetBounds(self.mWritingMode, self.mICoord, fragmentBStart, self.mMetrics.ISize(self.mWritingMode), self.mMetrics.BSize(self.mWritingMode), self.mContainerSize);

        // Now place the frame and complete the reflow process
        nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
            mFrame,
            &self.mPresContext,
            &self.mMetrics,
            Some(aReflowInput),
            self.mWritingMode,
            LogicalPoint::new(self.mWritingMode, self.mICoord, self.mBCoord),
            self.mContainerSize,
            nsIFrame_ReflowChildFlags::ApplyRelativePositioning,
        );

        *aOverflowAreas = self.mMetrics.mOverflowAreas + mFrame.GetPosition();

        true
    }
}

// Gecko: layout/generic/nsBlockReflowContext.cpp DescendIntoBlockLevelFrame
fn DescendIntoBlockLevelFrame(aFrame: nsIFrame) -> nsIFrame {
    let type_ = aFrame.Type();
    if type_ == LayoutFrameType::ColumnSet {
        // NOT PORTED: columns.
        //   static_cast<nsColumnSetFrame*>(aFrame)->DrainOverflowColumns();
        //   nsIFrame* child = aFrame->PrincipalChildList().FirstChild();
        //   if (child) { return DescendIntoBlockLevelFrame(child); }
        unimplemented!("NOT PORTED: columns (nsColumnSetFrame::DrainOverflowColumns)")
    }
    aFrame
}

// (The TEMP nsContainerFrame::FinishReflowChild and nsIFrame_ReflowChildFlags that were here are
// in nsContainerFrame.rs now.)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockReflowState_test_support::*;

    // Tests are written from the documented behaviour (CSS 2.2 8.3.1 margin collapsing, 10.3.3
    // block width, and the comments in nsBlockReflowContext.cpp); Firefox has no gtest for this.

    const AUTO: nscoord = NS_UNCONSTRAINEDSIZE;

    // root > outer > inner, all blocks. `outer_style` and `inner_style` are the inline styles.
    fn nested(outer_style: &str, inner_style: &str) -> Vec<nsIFrame> {
        let html = format!(r#"<div id=root style="margin:0;padding:0;border:0"><div id=outer style="{outer_style}"><div id=inner style="{inner_style}"></div></div></div>"#);
        frames(&html, &[("root", None), ("outer", Some(0)), ("inner", Some(1))], 10)
    }

    // The collapsed block-start margin of `outer` with `inner` as its only child, wired with one
    // block line (a line pushed on `mLines()` of the block).
    fn collapsed_margin(f: &[nsIFrame], inner_is_empty_line: bool) -> (nscoord, bool, bool) {
        let root = root_input(f[0], 10, AUTO);
        let outer = child_input(&root, f[1]);
        let block = nsBlockFrame::from_frame(f[1]);
        let wm = f[1].GetWritingMode();
        f[2].SetIsEmpty(inner_is_empty_line);
        // `outer` and `inner` have no border or padding: nsBlockFrame::IsSelfEmpty would say true
        // (for inner, if it is empty).
        f[1].SetSelfEmpty(inner_is_empty_line);
        f[2].SetSelfEmpty(inner_is_empty_line);
        block.mLines().push_back(block_line(f[2]));
        let ctx = nsBlockReflowContext::new(nsPresContext::Current(), &root);
        let mut margin = CollapsingMargin::default();
        let mut mayNeedRetry = false;
        let mut isEmpty = false;
        let dirtied = ctx.ComputeCollapsedBStartMargin(&outer, &mut margin, None, &mut mayNeedRetry, Some(&mut isEmpty));
        (margin.Get(), mayNeedRetry, dirtied)
    }

    #[test]
    fn collapsed_margin_takes_the_larger_of_parent_and_first_child() {
        // CSS 2.2 8.3.1: the top margin of a block and its first in-flow child collapse (to the
        // larger one) when there is no border or padding between them.
        let f = nested("margin-top:10px", "margin-top:30px;height:5px");
        let (m, retry, dirtied) = collapsed_margin(&f, false);
        assert_eq!(m, 30 * PX);
        assert!(!retry);
        assert!(!dirtied);
    }

    #[test]
    fn collapsed_margin_stops_at_parent_padding() {
        // With padding on the parent the child's margin does not collapse through it.
        let f = nested("margin-top:10px;padding-top:1px", "margin-top:30px;height:5px");
        let (m, _, _) = collapsed_margin(&f, false);
        assert_eq!(m, 10 * PX);
    }

    #[test]
    fn collapsed_margin_adds_negative_and_positive() {
        // The result is the largest positive plus the most negative margin.
        let f = nested("margin-top:10px", "margin-top:-4px;height:5px");
        let (m, _, _) = collapsed_margin(&f, false);
        assert_eq!(m, 10 * PX - 4 * PX);
    }

    #[test]
    fn empty_child_line_lets_the_margin_reach_the_next_line() {
        // An empty child block adds its block-end margin to the collapse (the caller does that
        // for the top-level frame; the recursion does it for the child).
        let f = nested("margin-top:0", "margin-top:5px;margin-bottom:20px");
        let (m, _, _) = collapsed_margin(&f, true);
        assert_eq!(m, 20 * PX);
    }

    #[test]
    fn clearing_child_marks_retry() {
        // In the optimistic pass a child with `clear` may need clearance: *aMayNeedRetry is set.
        let f = nested("margin-top:0", "clear:both;height:5px");
        let (_, retry, _) = collapsed_margin(&f, false);
        assert!(retry);
    }

    #[test]
    fn clearance_frame_gets_its_line_marked() {
        // The second pass: the line of the clearance frame is marked HasClearance and dirty,
        // and the walk stops there.
        let f = nested("margin-top:0", "margin-top:30px;height:5px");
        let root = root_input(f[0], 10, AUTO);
        let outer = child_input(&root, f[1]);
        let block = nsBlockFrame::from_frame(f[1]);
        block.mLines().push_back(block_line(f[2]));
        let ctx = nsBlockReflowContext::new(nsPresContext::Current(), &root);
        let mut margin = CollapsingMargin::default();
        let mut retry = false;
        let dirtied = ctx.ComputeCollapsedBStartMargin(&outer, &mut margin, Some(f[2]), &mut retry, None);
        assert!(dirtied);
        assert!(block.LinesBegin().HasClearance());
        assert!(block.LinesBegin().IsDirty());
        assert_eq!(margin.Get(), 0);
    }

    #[test]
    fn first_pass_clears_old_clearance() {
        // aClearanceFrame null: lines that had clearance lose it and become dirty.
        let f = nested("margin-top:0", "height:5px");
        let root = root_input(f[0], 10, AUTO);
        let outer = child_input(&root, f[1]);
        let block = nsBlockFrame::from_frame(f[1]);
        let line = block_line(f[2]);
        unsafe { (*line).SetHasClearance() };
        block.mLines().push_back(line);
        let ctx = nsBlockReflowContext::new(nsPresContext::Current(), &root);
        let mut margin = CollapsingMargin::default();
        let mut retry = false;
        assert!(ctx.ComputeCollapsedBStartMargin(&outer, &mut margin, None, &mut retry, None));
        assert!(!block.LinesBegin().HasClearance());
        assert!(block.LinesBegin().IsDirty());
    }

    // A reflow of `f[1]` (a child block of `f[0]`) through ReflowBlock and PlaceBlock, in a parent
    // with `avail_b` available block-size.
    fn reflow_and_place(f: &[nsIFrame], space_b: nscoord, applyMargin: bool, force: bool) -> (bool, nscoord, CollapsingMargin, nsLineBox, ReflowOutput) {
        let mut fm = nsFloatManager::new(f[0].GetWritingMode());
        let mut root = root_input(f[0], 10, space_b);
        root.mFloatManager = &mut fm;
        let root = root;
        let wm = f[0].GetWritingMode();
        let state = BlockReflowState::new(&root, nsPresContext::Current(), nsBlockFrame::from_frame(f[0]), true, true, true, 0, NS_UNCONSTRAINEDSIZE, 0);
        let mut ri = child_input(&root, f[1]);
        let mut ctx = nsBlockReflowContext::new(nsPresContext::Current(), &root);
        let mut line = unsafe { *Box::from_raw(block_line(f[1])) };
        let mut prevMargin = CollapsingMargin::default();
        prevMargin.Include__nscoord(7 * PX);
        let space = LogicalRect::new(wm, 0, 0, 10 * PX, space_b);
        let mut status = nsReflowStatus::new();
        ctx.ReflowBlock(space, applyMargin, &mut prevMargin, 0, Some(&line), &mut ri, &mut status, &state);
        let mut endMargin = CollapsingMargin::default();
        let mut overflow = OverflowAreas::default();
        let fit = ctx.PlaceBlock(&ri, force, &mut line, &mut endMargin, &mut overflow, &status);
        (fit, f[1].GetPosition().y, endMargin, line, ctx.GetMetrics().clone())
    }

    fn two_blocks(child_style: &str) -> Vec<nsIFrame> {
        let html = format!(r#"<div id=root style="margin:0;padding:0;border:0"><div id=c style="{child_style}"></div></div>"#);
        frames(&html, &[("root", None), ("c", Some(0))], 10)
    }

    #[test]
    fn place_block_applies_the_previous_margin_and_own_margins() {
        // The block lands below the collapsed margin passed in (7px) and its inline-start margin
        // moves it inline-wise (10.3.3); its block-end margin goes to aBEndMarginResult.
        let f = two_blocks("margin:0 0 9px 3px;height:20px;width:100px");
        let (fit, y, endMargin, line, metrics) = reflow_and_place(&f, AUTO, true, false);
        assert!(fit);
        assert_eq!(y, 7 * PX);
        assert_eq!(f[1].GetPosition().x, 3 * PX);
        assert_eq!(endMargin.Get(), 9 * PX);
        assert_eq!(line.GetBounds().BStart(f[0].GetWritingMode()), 7 * PX);
        assert_eq!(metrics.BSize(f[0].GetWritingMode()), 20 * PX);
    }

    #[test]
    fn place_block_without_start_margin() {
        // aApplyBStartMargin false: the previous margin is not applied (ReflowBlock zeroes it).
        let f = two_blocks("margin:0;height:20px");
        let (fit, y, _, _, _) = reflow_and_place(&f, AUTO, false, false);
        assert!(fit);
        assert_eq!(y, 0);
    }

    #[test]
    fn place_block_rejects_a_block_that_does_not_fit() {
        // A non-empty block that ends past the available block-size does not fit, unless forced.
        let f = two_blocks("margin:0;height:50px");
        let (fit, _, _, _, _) = reflow_and_place(&f, 40 * PX, false, false);
        assert!(!fit);
        let f = two_blocks("margin:0;height:50px");
        let (fit, _, _, _, _) = reflow_and_place(&f, 40 * PX, false, true);
        assert!(fit);
    }

    #[test]
    fn empty_block_collapses_its_margins_through() {
        // CSS 2.2 8.3.1: a zero-height block's own start and end margins collapse together; the
        // line's cached emptiness says it is empty. The start margin was applied (7px previous,
        // 4px own: collapsed 7px) so it is included in the end margin result too.
        let f = two_blocks("margin:4px 0 11px 0;height:0");
        let html_wm = f[0].GetWritingMode();
        let mut fm = nsFloatManager::new(html_wm);
        let mut root = root_input(f[0], 10, AUTO);
        root.mFloatManager = &mut fm;
        let root = root;
        let state = BlockReflowState::new(&root, nsPresContext::Current(), nsBlockFrame::from_frame(f[0]), true, true, true, 0, NS_UNCONSTRAINEDSIZE, 0);
        let mut ri = child_input(&root, f[1]);
        let mut ctx = nsBlockReflowContext::new(nsPresContext::Current(), &root);
        let mut line = unsafe { *Box::from_raw(block_line(f[1])) };
        f[1].SetIsEmpty(true);
        let mut prevMargin = CollapsingMargin::default();
        prevMargin.Include__nscoord(7 * PX);
        let mut status = nsReflowStatus::new();
        ctx.ReflowBlock(LogicalRect::new(html_wm, 0, 0, 10 * PX, AUTO), true, &mut prevMargin, 0, Some(&line), &mut ri, &mut status, &state);
        let mut endMargin = CollapsingMargin::default();
        let mut overflow = OverflowAreas::default();
        assert!(ctx.PlaceBlock(&ri, false, &mut line, &mut endMargin, &mut overflow, &status));
        // end margin 11px collapsed with the applied start margin 7px = 11px
        assert_eq!(endMargin.Get(), 11 * PX);
        // the line starts the applied start margin higher, so the next line is not pushed twice
        assert_eq!(line.GetBounds().BStart(html_wm), 0);
    }
}
