/* state used in reflow of block frames */
// Translated from layout/generic/BlockReflowState.h and BlockReflowState.cpp (GECKO_REV).
//
// Stage 2b, float placement and available-space computation for block reflow. NOT PORTED here:
// text-overflow / -webkit-line-clamp markers (TextOverflow), fragmentation (pushed floats,
// float continuations, column and page breaks), shape-outside, noisy-reflow debug printing
// (nsBlockFrame::gNoisyReflow and friends), LAYOUT logging.
//
// nsBlockFrame is nsBlockFrame.rs. nsLineBox, nsFrameList, nsLineLayout and nsReflowStatus are the real ones.
use crate::*;

// Gecko: layout/generic/BlockReflowState.h BlockReflowState::Flags
// Block reflow input flags.
// (C++ bit fields, `bool x : 1`, are bools here; the constructor makes them all false.)
#[derive(Clone, Copy, Default)]
pub struct BlockReflowState_Flags {
    // Set in the BlockReflowState constructor when reflowing a "block margin
    // root" frame (i.e. a frame with any of the NS_BLOCK_BFC flag set, for
    // which margins apply by default).
    //
    // The flag is also set when reflowing a frame whose computed BStart border
    // padding is non-zero.
    pub mIsBStartMarginRoot: bool,

    // Set in the BlockReflowState constructor when reflowing a "block margin
    // root" frame (i.e. a frame with any of the NS_BLOCK_BFC flag set, for
    // which margins apply by default).
    //
    // The flag is also set when reflowing a frame whose computed BEnd border
    // padding is non-zero.
    pub mIsBEndMarginRoot: bool,

    // Set if the BStart margin should be considered when placing a linebox that
    // contains a block frame. It may be set as a side-effect of calling
    // nsBlockFrame::ShouldApplyBStartMargin(); once set,
    // ShouldApplyBStartMargin() uses it as a fast-path way to return whether
    // the BStart margin should apply.
    //
    // If the flag hasn't been set in the block reflow state, then
    // ShouldApplyBStartMargin() will crawl the line list to see if a block
    // frame precedes the specified frame. If so, the BStart margin should be
    // applied, and the flag is set to cache the result. (If not, the BStart
    // margin will be applied as a result of the generational margin collapsing
    // logic in nsBlockReflowContext::ComputeCollapsedBStartMargin(). In this
    // case, the flag won't be set, so subsequent calls to
    // ShouldApplyBStartMargin() will continue crawl the line list.)
    //
    // This flag is also set in the BlockReflowState constructor if
    // mIsBStartMarginRoot is set; that is, the frame being reflowed is a margin
    // root by default.
    pub mShouldApplyBStartMargin: bool,

    // Set when mLineAdjacentToTop is valid.
    pub mHasLineAdjacentToTop: bool,

    // Set when the block has the equivalent of NS_BLOCK_*_BFC.
    pub mBlockNeedsFloatManager: bool,

    // Set when nsLineLayout::LineIsEmpty was true at the end of reflowing
    // the current line.
    pub mIsLineLayoutEmpty: bool,

    // Set when we need text-overflow or -webkit-line-clamp processing.
    pub mCanHaveOverflowMarkers: bool,
}

// Gecko: layout/generic/BlockReflowState.h BlockReflowState::PlaceFloatResult
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum BlockReflowState_PlaceFloatResult {
    Placed,
    ShouldPlaceBelowCurrentLine,
    ShouldPlaceInNextContinuation,
}

// Returns the first coordinate >= aBCoord that clears the
// floats indicated by aClearType and has enough inline size between floats
// (or no floats remaining) to accomodate aFloatAvoidingBlock.
// Gecko: layout/generic/BlockReflowState.h BlockReflowState::ClearFloatsResult
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum BlockReflowState_ClearFloatsResult {
    BCoordNoChange,
    BCoordAdvanced,
    FloatsPushedOrSplit,
}

// BlockReflowState contains additional reflow input information that the
// block frame uses along with ReflowInput. Like ReflowInput, this
// is read-only data that is passed down from a parent frame to its children.
// Gecko: layout/generic/BlockReflowState.h BlockReflowState
pub struct BlockReflowState<'a> {
    // This state is the "global" state computed once for the reflow of
    // the block.

    // The block frame that is using this object
    pub mBlock: nsBlockFrame,

    pub mPresContext: nsPresContext,

    pub mReflowInput: &'a ReflowInput<'a>,

    // The coordinates within the float manager where the block is being
    // placed <b>after</b> taking into account the blocks border and
    // padding. This, therefore, represents the inner "content area" (in
    // float manager coordinates) where child frames will be placed,
    // including child blocks and floats.
    pub mFloatManagerI: nscoord,
    pub mFloatManagerB: nscoord,

    // XXX get rid of this
    pub mReflowStatus: nsReflowStatus,

    // The float manager state as it was before the contents of this
    // block.  This is needed for positioning bullets, since we only want
    // to move the bullet to flow around floats that were before this
    // block, not floats inside of it.
    pub mFloatManagerStateBefore: SavedState,

    // The content area to reflow child frames within.  This is within
    // this frame's coordinate system and writing mode, which means
    // mContentArea.IStart == BorderPadding().IStart and
    // mContentArea.BStart == BorderPadding().BStart.
    // The block size may be NS_UNCONSTRAINEDSIZE, which indicates that there
    // is no page/column boundary below (the common case).
    // mContentArea.BEnd() should only be called after checking that
    // mContentArea.BSize is not NS_UNCONSTRAINEDSIZE; otherwise
    // coordinate overflow may occur.
    pub mContentArea: LogicalRect,

    // Amount of inset to apply during line-breaking, used by text-wrap:balance
    // to adjust line-breaks for more consistent lengths throughout the block.
    pub mInsetForBalance: nscoord,

    // Physical size. Use only for physical <-> logical coordinate conversion.
    //
    // Note: for vertical-rl writing-mode, if mContainerSize's width is
    // initialized to zero due to unconstrained block-size, lines will be
    // positioned (physically) incorrectly. We will fix them up at the end of
    // nsBlockFrame::Reflow() after we know the total block-size of the frame.
    pub mContainerSize: nsSize,

    // Track child overflow continuations.
    // NOT PORTED: fragmentation (the tracker is never created here).
    pub mOverflowTracker: *mut nsOverflowContinuationTracker,

    //----------------------------------------

    // This state is "running" state updated by the reflow of each line
    // in the block. This same state is "recovered" when a line is not
    // dirty and is passed over during incremental reflow.

    // The current line being reflowed
    // If it is mBlock->end_lines(), then it is invalid.
    pub mCurrentLine: nsLineList_iterator,

    // When mHasLineAdjacentToTop is set, this refers to a line
    // which we know is adjacent to the top of the block (in other words,
    // all lines before it are empty and do not have clearance. This line is
    // always before the current line.
    pub mLineAdjacentToTop: nsLineList_iterator,

    // The current block-direction coordinate in the block
    pub mBCoord: nscoord,

    // mBlock's computed logical border+padding with pre-reflow skip sides applied
    // (See the constructor and nsIFrame::PreReflowBlockLevelLogicalSkipSides).
    pub mBorderPadding: LogicalMargin,

    // The overflow areas of all floats placed so far
    pub mFloatOverflowAreas: OverflowAreas,

    // Previous child. This is used when pulling up a frame to update
    // the sibling list.
    pub mPrevChild: Option<nsIFrame>,

    // The previous child frames collapsed bottom margin value.
    pub mPrevBEndMargin: CollapsingMargin,

    // The current next-in-flow for the block. When lines are pulled
    // from a next-in-flow, this is used to know which next-in-flow to
    // pull from. When a next-in-flow is emptied of lines, we advance
    // this to the next next-in-flow.
    pub mNextInFlow: Option<nsBlockFrame>,

    //----------------------------------------

    // Temporary state, for line-reflow. This state is used during the reflow
    // of a given line, but doesn't have meaning before or after.

    // The list of floats that are "current-line" floats. These are
    // added to the line after the line has been reflowed, to keep the
    // list fiddling from being N^2.
    pub mCurrentLineFloats: Vec<nsIFrame>,

    // The list of floats which are "below current-line"
    // floats. These are reflowed/placed after the line is reflowed
    // and placed. Again, this is done to keep the list fiddling from
    // being N^2.
    pub mBelowCurrentLineFloats: Vec<nsIFrame>,

    // The list of floats that are waiting on a break opportunity in order to be
    // placed, since we're on a nowrap context.
    pub mNoWrapFloats: Vec<nsIFrame>,

    pub mMinLineHeight: nscoord,

    pub mLineNumber: i32,

    pub mFlags: BlockReflowState_Flags,

    // Cache the result of nsBlockFrame::FindTrailingClear() from mBlock's
    // prev-in-flows. See nsBlockFrame::ReflowPushedFloats().
    pub mTrailingClearFromPIF: UsedClear,

    // The amount of computed content block-size "consumed" by our previous
    // continuations.
    pub mConsumedBSize: nscoord,

    // The amount of block-axis alignment shift to assume during reflow.
    // Cached between reflows in the AlignContentShift property.
    // (This system optimizes reflow for not changing the shift.)
    pub mAlignContentShift: nscoord,

    // Cache the current line's BSize if nsBlockFrame::PlaceLine() fails to
    // place the line. When redoing the line, it will be used to query the
    // accurate float available space in AddFloat() and
    // nsBlockFrame::PlaceLine().
    pub mLineBSize: Option<nscoord>,
}

impl<'a> BlockReflowState<'a> {
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::BlockReflowState
    // (aInset defaults to 0 in C++.)
    pub fn new(
        aReflowInput: &'a ReflowInput<'a>,
        aPresContext: nsPresContext,
        aFrame: nsBlockFrame,
        aBStartMarginRoot: bool,
        aBEndMarginRoot: bool,
        aBlockNeedsFloatManager: bool,
        aConsumedBSize: nscoord,
        aEffectiveContentBoxBSize: nscoord,
        aInset: nscoord,
    ) -> BlockReflowState<'a> {
        let mReflowInput = aReflowInput;
        let mut mBorderPadding = mReflowInput.ComputedLogicalBorderPadding(mReflowInput.GetWritingMode());
        mBorderPadding.ApplySkipSides(aFrame.PreReflowBlockLevelLogicalSkipSides());
        let mut this = BlockReflowState {
            mBlock: aFrame,
            mPresContext: aPresContext,
            mReflowInput: mReflowInput,
            mFloatManagerI: 0,
            mFloatManagerB: 0,
            mReflowStatus: nsReflowStatus::new(),
            mFloatManagerStateBefore: SavedState::new(),
            mContentArea: LogicalRect::new__WritingMode(aReflowInput.GetWritingMode()),
            mInsetForBalance: aInset,
            mContainerSize: aReflowInput.ComputedSizeAsContainerIfConstrained(),
            mOverflowTracker: std::ptr::null_mut(),
            mCurrentLine: nsLineList_iterator::default(),
            mLineAdjacentToTop: nsLineList_iterator::default(),
            mBCoord: 0,
            mBorderPadding: mBorderPadding,
            mFloatOverflowAreas: OverflowAreas::default(),
            mPrevChild: None,
            mPrevBEndMargin: CollapsingMargin::default(),
            mNextInFlow: None,
            mCurrentLineFloats: Vec::new(),
            mBelowCurrentLineFloats: Vec::new(),
            mNoWrapFloats: Vec::new(),
            mMinLineHeight: aReflowInput.GetLineHeight(),
            mLineNumber: 0,
            mFlags: BlockReflowState_Flags::default(),
            mTrailingClearFromPIF: UsedClear::None,
            mConsumedBSize: aConsumedBSize,
            mAlignContentShift: aFrame.GetAlignContentShift(),
            mLineBSize: None,
        };
        NS_ASSERTION!(this.mConsumedBSize != NS_UNCONSTRAINEDSIZE, "The consumed block-size should be constrained!");

        let wm = aReflowInput.GetWritingMode();

        if aBStartMarginRoot || 0 != this.mBorderPadding.BStart(wm) {
            this.mFlags.mIsBStartMarginRoot = true;
            this.mFlags.mShouldApplyBStartMargin = true;
        }
        if aBEndMarginRoot || 0 != this.mBorderPadding.BEnd(wm) {
            this.mFlags.mIsBEndMarginRoot = true;
        }
        if aBlockNeedsFloatManager {
            this.mFlags.mBlockNeedsFloatManager = true;
        }

        // mFlags.mCanHaveOverflowMarkers = css::TextOverflow::CanHaveOverflowMarkers(
        //     mBlock, css::TextOverflow::BeforeReflow::Yes);
        // NOT PORTED: text-overflow and -webkit-line-clamp markers (TextOverflow).
        this.mFlags.mCanHaveOverflowMarkers = false;

        MOZ_ASSERT!(!this.mReflowInput.mFloatManager.is_null(), "Float manager should be valid when creating BlockReflowState!");

        // Save the coordinate system origin for later.
        let (mut floatManagerI, mut floatManagerB) = (0, 0);
        this.FloatManager().GetTranslation(&mut floatManagerI, &mut floatManagerB);
        this.mFloatManagerI = floatManagerI;
        this.mFloatManagerB = floatManagerB;
        // never popped
        let mut stateBefore = SavedState::new();
        this.FloatManager().PushState(&mut stateBefore);
        this.mFloatManagerStateBefore = stateBefore;

        this.mNextInFlow = this.mBlock.GetNextInFlow().map(nsBlockFrame::from_frame);

        NS_WARNING_ASSERTION!(
            NS_UNCONSTRAINEDSIZE != aReflowInput.ComputedISize(),
            "have unconstrained width; this should only result from very large sizes, not attempts at intrinsic width calculation"
        );
        *this.mContentArea.ISize_mut(wm) = aReflowInput.ComputedISize();

        // Compute content area block-size. Unlike the inline-size, if we have a
        // specified style block-size, we ignore it since extra content is managed by
        // the "overflow" property. When we don't have a specified style block-size,
        // then we may end up limiting our block-size if the available block-size is
        // constrained (this situation occurs when we are paginated).
        let availableBSize = aReflowInput.AvailableBSize();
        if availableBSize != NS_UNCONSTRAINEDSIZE {
            // We are in a paginated situation. The block-end edge of the available
            // space to reflow the children is within our block-end border and padding.
            // If we're cloning our border and padding, and we're going to request
            // additional continuations because of our excessive content-box block-size,
            // then reserve some of our available space for our (cloned) block-end
            // border and padding.
            let reserveSpaceForBlockEndBP = this.mReflowInput.mStyleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone
                && (aEffectiveContentBoxBSize == NS_UNCONSTRAINEDSIZE || aEffectiveContentBoxBSize + this.mBorderPadding.BStartEnd(wm) > availableBSize);
            let bp = if reserveSpaceForBlockEndBP { this.mBorderPadding.BStartEnd(wm) } else { this.mBorderPadding.BStart(wm) };
            *this.mContentArea.BSize_mut(wm) = std::cmp::max(0, availableBSize - bp);
        } else {
            // When we are not in a paginated situation, then we always use a
            // unconstrained block-size.
            *this.mContentArea.BSize_mut(wm) = NS_UNCONSTRAINEDSIZE;
        }
        *this.mContentArea.IStart_mut(wm) = this.mBorderPadding.IStart(wm);
        this.mBCoord = this.mBorderPadding.BStart(wm);
        *this.mContentArea.BStart_mut(wm) = this.mBCoord;

        // Account for existing cached shift, we'll re-position in AlignContent() if
        // needed.
        if this.mAlignContentShift != 0 {
            this.mBCoord += this.mAlignContentShift;
            *this.mContentArea.BStart_mut(wm) += this.mAlignContentShift;

            if availableBSize != NS_UNCONSTRAINEDSIZE {
                *this.mContentArea.BSize_mut(wm) += this.mAlignContentShift;
            }
        }

        this.mPrevChild = None;
        this.mCurrentLine = aFrame.LinesEnd();
        this
    }

    /**
     * Unshifts coords, restores availableBSize to reality.
     * (Constructor applies any cached shift before reflow
     *  so that frames are reflowed with cached shift)
     */
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::UndoAlignContentShift
    pub fn UndoAlignContentShift(&mut self) {
        if self.mAlignContentShift == 0 {
            return;
        }

        self.mBCoord -= self.mAlignContentShift;
        *self.mContentArea.BStart_mut(self.mReflowInput.GetWritingMode()) -= self.mAlignContentShift;

        if self.mReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE {
            *self.mContentArea.BSize_mut(self.mReflowInput.GetWritingMode()) -= self.mAlignContentShift;
        }
    }

    /**
     * Get the available reflow space (the area not occupied by floats)
     * for the current y coordinate. The available space is relative to
     * our coordinate system, which is the content box, with (0, 0) in the
     * upper left.
     *
     * The parameter aCBWM is the containing block's writing mode, which is
     * NOT necessarily the mode currently being used by the float manager.
     *
     * Returns whether there are floats present at the given block-direction
     * coordinate and within the inline size of the content rect.
     *
     * Note: some codepaths clamp this structure's inline-size to be >=0 "for
     * compatibility with nsSpaceManager". So if you encounter a nsFlowAreaRect
     * which appears to have an ISize of 0, you can't necessarily assume that a
     * 0-ISize float-avoiding block would actually fit; you need to check the
     * InitialISizeIsNegative flag to see whether that 0 is actually a clamped
     * negative value (in which case a 0-ISize float-avoiding block *should not*
     * be considered as fitting, because it would intersect some float).
     */
    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::GetFloatAvailableSpace(WritingMode)
    pub fn GetFloatAvailableSpace__WritingMode(&self, aCBWM: WritingMode) -> nsFlowAreaRect {
        self.GetFloatAvailableSpace__WritingMode_nscoord(aCBWM, self.mBCoord)
    }
    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::GetFloatAvailableSpaceForPlacingFloat
    pub fn GetFloatAvailableSpaceForPlacingFloat(&self, aCBWM: WritingMode, aBCoord: nscoord) -> nsFlowAreaRect {
        self.GetFloatAvailableSpaceWithState(aCBWM, aBCoord, ShapeType::Margin, None)
    }
    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::GetFloatAvailableSpace(WritingMode, nscoord)
    pub fn GetFloatAvailableSpace__WritingMode_nscoord(&self, aCBWM: WritingMode, aBCoord: nscoord) -> nsFlowAreaRect {
        self.GetFloatAvailableSpaceWithState(aCBWM, aBCoord, ShapeType::ShapeOutside, None)
    }

    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::GetFloatAvailableSpaceWithState
    pub fn GetFloatAvailableSpaceWithState(&self, aCBWM: WritingMode, aBCoord: nscoord, aShapeType: ShapeType, aState: Option<&SavedState>) -> nsFlowAreaRect {
        let wm = self.mReflowInput.GetWritingMode();
        #[cfg(debug_assertions)]
        {
            // Verify that the caller setup the coordinate system properly
            let (mut wI, mut wB) = (0, 0);
            self.FloatManager().GetTranslation(&mut wI, &mut wB);

            NS_ASSERTION!((wI == self.mFloatManagerI) && (wB == self.mFloatManagerB), "bad coord system");
        }

        let blockSize = if self.mContentArea.BSize(wm) == nscoord_MAX { nscoord_MAX } else { std::cmp::max(self.mContentArea.BEnd(wm) - aBCoord, 0) };
        let mut result = self.FloatManager().GetFlowArea(aCBWM, wm, aBCoord, blockSize, BandInfoType::BandFromPoint, aShapeType, self.mContentArea, aState, self.ContainerSize());
        // Keep the inline size >= 0 for compatibility with nsSpaceManager.
        if result.mRect.ISize(wm) < 0 {
            *result.mRect.ISize_mut(wm) = 0;
        }

        // NOT PORTED: noisy-reflow debug printing (#ifdef DEBUG, nsBlockFrame::gNoisyReflow).
        result
    }

    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::GetFloatAvailableSpaceForBSize
    pub fn GetFloatAvailableSpaceForBSize(&self, aCBWM: WritingMode, aBCoord: nscoord, aBSize: nscoord, aState: Option<&SavedState>) -> nsFlowAreaRect {
        let wm = self.mReflowInput.GetWritingMode();
        #[cfg(debug_assertions)]
        {
            // Verify that the caller setup the coordinate system properly
            let (mut wI, mut wB) = (0, 0);
            self.FloatManager().GetTranslation(&mut wI, &mut wB);

            NS_ASSERTION!((wI == self.mFloatManagerI) && (wB == self.mFloatManagerB), "bad coord system");
        }
        let mut result = self.FloatManager().GetFlowArea(aCBWM, wm, aBCoord, aBSize, BandInfoType::WidthWithinHeight, ShapeType::ShapeOutside, self.mContentArea, aState, self.ContainerSize());
        // Keep the width >= 0 for compatibility with nsSpaceManager.
        if result.mRect.ISize(wm) < 0 {
            *result.mRect.ISize_mut(wm) = 0;
        }

        // NOT PORTED: noisy-reflow debug printing (#ifdef DEBUG, nsBlockFrame::gNoisyReflow).
        result
    }

    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::FloatManager
    // (the C++ returns the shared nsFloatManager*, which ReflowInput::mFloatManager holds as a raw
    // pointer; the `&mut` is only valid while the owning nsAutoFloatManager lives.)
    pub fn FloatManager(&self) -> &mut nsFloatManager {
        MOZ_ASSERT!(!self.mReflowInput.mFloatManager.is_null(), "Float manager should be valid during the lifetime of BlockReflowState!");
        unsafe { &mut *self.mReflowInput.mFloatManager }
    }

    // Advances to the next band, i.e., the next horizontal stripe in
    // which there is a different set of floats.
    // Return false if it did not advance, which only happens for
    // constrained heights (and means that we should get pushed to the
    // next column/page).
    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::AdvanceToNextBand
    pub fn AdvanceToNextBand(&self, aFloatAvailableSpace: &LogicalRect, aBCoord: &mut nscoord) -> bool {
        let wm = self.mReflowInput.GetWritingMode();
        if aFloatAvailableSpace.BSize(wm) > 0 {
            // See if there's room in the next band.
            *aBCoord += aFloatAvailableSpace.BSize(wm);
        } else {
            if self.mReflowInput.AvailableHeight() != NS_UNCONSTRAINEDSIZE {
                // Stop trying to clear here; we'll just get pushed to the
                // next column or page and try again there.
                return false;
            }
            MOZ_ASSERT_UNREACHABLE!("avail space rect with zero height!");
            *aBCoord += 1;
        }
        true
    }

    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::FloatAvoidingBlockFitsInAvailSpace
    pub fn FloatAvoidingBlockFitsInAvailSpace(&self, aFloatAvoidingBlock: nsIFrame, aFloatAvailableSpace: &nsFlowAreaRect) -> bool {
        if !aFloatAvailableSpace.HasFloats() {
            // If there aren't any floats here, then we always fit.
            // We check this before calling ISizeToClearPastFloats, which is
            // somewhat expensive.
            return true;
        }

        // |aFloatAvailableSpace| was computed as having a negative size, which means
        // there are floats on both sides pushing inwards past each other, and
        // |aFloatAvoidingBlock| would necessarily intersect a float if we put it
        // here. So, it doesn't fit.
        if aFloatAvailableSpace.ISizeIsActuallyNegative() {
            return false;
        }

        let wm = self.mReflowInput.GetWritingMode();
        let replacedISize: nsBlockFrame_FloatAvoidingISizeToClear = nsBlockFrame::ISizeToClearPastFloats(self, aFloatAvailableSpace.mRect, aFloatAvoidingBlock);
        // The inline-start side of the replaced element should be offset by
        // the larger of the float intrusion or the replaced element's own
        // start margin.  The inline-end side is similar, except for Web
        // compatibility we ignore the margin.
        std::cmp::max(aFloatAvailableSpace.mRect.IStart(wm) - self.mContentArea.IStart(wm), replacedISize.marginIStart)
            + replacedISize.borderBoxISize
            + (self.mContentArea.IEnd(wm) - aFloatAvailableSpace.mRect.IEnd(wm))
            <= self.mContentArea.ISize(wm)
    }

    // True if the current block-direction coordinate, for placing the children
    // within the content area, is still adjacent with the block-start of the
    // content area.
    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::IsAdjacentWithBStart
    pub fn IsAdjacentWithBStart(&self) -> bool {
        self.mBCoord == self.ContentBStart()
    }

    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::BorderPadding
    pub fn BorderPadding(&self) -> &LogicalMargin {
        &self.mBorderPadding
    }

    /*
     * Reconstruct the vertical margin before the line |aLine| in order to
     * do an incremental reflow that begins with |aLine| without reflowing
     * the line before it.  |aLine| may point to the fencepost at the end of
     * the line list, and it is used this way since we (for now, anyway)
     * always need to recover margins at the end of a block.
     *
     * The reconstruction involves walking backward through the line list to
     * find any collapsed margins preceding the line that would have been in
     * the reflow input's |mPrevBEndMargin| when we reflowed that line in
     * a full reflow (under the rule in CSS2 that all adjacent vertical
     * margins of blocks collapse).
     */
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::ReconstructMarginBefore
    // (nsLineList::iterator `--aLine` is `operator_dec()`.)
    pub fn ReconstructMarginBefore(&mut self, mut aLine: nsLineList_iterator) {
        self.mPrevBEndMargin.Zero();
        let block = self.mBlock;

        let firstLine = block.LinesBegin();
        loop {
            aLine.operator_dec();
            if aLine.IsBlock() {
                self.mPrevBEndMargin = aLine.GetCarriedOutBEndMargin();
                break;
            }
            if !aLine.IsEmpty() {
                break;
            }
            if aLine == firstLine {
                // If the top margin was carried out (and thus already applied),
                // set it to zero.  Either way, we're done.
                if !self.mFlags.mIsBStartMarginRoot {
                    self.mPrevBEndMargin.Zero();
                }
                break;
            }
        }
    }

    // Caller must have called GetFloatAvailableSpace for the correct position
    // (which need not be the current mBCoord).
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::ComputeFloatAvoidingOffsets
    pub fn ComputeFloatAvoidingOffsets(&self, aFloatAvoidingBlock: nsIFrame, aFloatAvailableSpace: &LogicalRect, aIStartResult: &mut nscoord, aIEndResult: &mut nscoord) {
        let wm = self.mReflowInput.GetWritingMode();
        // The frame is clueless about the float manager and therefore we
        // only give it free space. An example is a table frame - the
        // tables do not flow around floats.
        // However, we can let its margins intersect floats.
        NS_ASSERTION!(aFloatAvailableSpace.IStart(wm) >= self.mContentArea.IStart(wm), "bad avail space rect inline-coord");
        NS_ASSERTION!(aFloatAvailableSpace.ISize(wm) == 0 || aFloatAvailableSpace.IEnd(wm) <= self.mContentArea.IEnd(wm), "bad avail space rect inline-size");

        let mut iStartOffset: nscoord;
        let mut iEndOffset: nscoord;
        if aFloatAvailableSpace.ISize(wm) == self.mContentArea.ISize(wm) {
            // We don't need to compute margins when there are no floats around.
            iStartOffset = 0;
            iEndOffset = 0;
        } else {
            let frameMargin: LogicalMargin = SizeComputationInput::new__nsIFrame_gfxContext_WritingMode_nscoord_Maybe_Maybe(
                aFloatAvoidingBlock,
                self.mReflowInput.mRenderingContext,
                wm,
                self.mContentArea.ISize(wm),
                None,
                None,
            )
            .ComputedLogicalMargin(wm);

            let iStartFloatIOffset = aFloatAvailableSpace.IStart(wm) - self.mContentArea.IStart(wm);
            iStartOffset = std::cmp::max(iStartFloatIOffset, frameMargin.IStart(wm)) - frameMargin.IStart(wm);
            iStartOffset = std::cmp::max(iStartOffset, 0); // in case of negative margin
            let iEndFloatIOffset = self.mContentArea.IEnd(wm) - aFloatAvailableSpace.IEnd(wm);
            iEndOffset = std::cmp::max(iEndFloatIOffset, frameMargin.IEnd(wm)) - frameMargin.IEnd(wm);
            iEndOffset = std::cmp::max(iEndOffset, 0); // in case of negative margin
        }
        *aIStartResult = iStartOffset;
        *aIEndResult = iEndOffset;
    }

    // Compute the amount of available space for reflowing a block frame at the
    // current block-direction coordinate mBCoord. Caller must have called
    // GetFloatAvailableSpace for the current mBCoord.
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::ComputeBlockAvailSpace
    pub fn ComputeBlockAvailSpace(&mut self, aFrame: nsIFrame, aFloatAvailableSpace: &nsFlowAreaRect, aBlockAvoidsFloats: bool) -> LogicalRect {
        // NOT PORTED: REALLY_NOISY_REFLOW printing.
        let wm = self.mReflowInput.GetWritingMode();
        let mut result = LogicalRect::new__WritingMode(wm);
        *result.BStart_mut(wm) = self.mBCoord;
        // Note: ContentBSize() and ContentBEnd() are not our content-box size and its
        // block-end edge. They really mean "the available block-size for children",
        // and "the block-end edge of the available space for children".
        *result.BSize_mut(wm) = if self.ContentBSize() == NS_UNCONSTRAINEDSIZE { NS_UNCONSTRAINEDSIZE } else { self.ContentBEnd() - self.mBCoord };
        // mBCoord might be greater than ContentBEnd() if the block's top margin
        // pushes it off the page/column. Negative available block-size can confuse
        // other code and is nonsense in principle.

        // XXX Do we really want this condition to be this restrictive (i.e.,
        // more restrictive than it used to be)?  The |else| here is allowed
        // by the CSS spec, but only out of desperation given implementations,
        // and the behavior it leads to is quite undesirable (it can cause
        // things to become extremely narrow when they'd fit quite well a
        // little bit lower).  Should the else be a quirk or something that
        // applies to a specific set of frame classes and no new ones?
        // If we did that, then for those frames where the condition below is
        // true but nsBlockFrame::BlockCanIntersectFloats is false,
        // nsBlockFrame::ISizeToClearPastFloats would need to use the
        // shrink-wrap formula, max(MinISize, min(avail width, PrefISize))
        // rather than just using MinISize.
        NS_ASSERTION!(nsBlockFrame::BlockCanIntersectFloats(aFrame) == !aBlockAvoidsFloats, "unexpected replaced width");
        if !aBlockAvoidsFloats {
            if aFloatAvailableSpace.HasFloats() {
                // Use the float-edge property to determine how the child block
                // will interact with the float.
                let borderStyle = aFrame.StyleBorder();
                match borderStyle.mFloatEdge() {
                    StyleFloatEdge::ContentBox => {
                        // content and only content does
                        // runaround of floats
                        // The child block will flow around the float. Therefore
                        // give it all of the available space.
                        *result.IStart_mut(wm) = self.mContentArea.IStart(wm);
                        *result.ISize_mut(wm) = self.mContentArea.ISize(wm);
                    }
                    StyleFloatEdge::MarginBox => {
                        // The child block's margins should be placed adjacent to,
                        // but not overlap the float.
                        *result.IStart_mut(wm) = aFloatAvailableSpace.mRect.IStart(wm);
                        *result.ISize_mut(wm) = aFloatAvailableSpace.mRect.ISize(wm);
                    }
                }
            } else {
                // Since there are no floats present the float-edge property
                // doesn't matter therefore give the block element all of the
                // available space since it will flow around the float itself.
                *result.IStart_mut(wm) = self.mContentArea.IStart(wm);
                *result.ISize_mut(wm) = self.mContentArea.ISize(wm);
            }
        } else {
            let (mut iStartOffset, mut iEndOffset) = (0, 0);
            self.ComputeFloatAvoidingOffsets(aFrame, &aFloatAvailableSpace.mRect, &mut iStartOffset, &mut iEndOffset);
            *result.IStart_mut(wm) = self.mContentArea.IStart(wm) + iStartOffset;
            *result.ISize_mut(wm) = self.mContentArea.ISize(wm) - iStartOffset - iEndOffset;
        }

        // NOT PORTED: REALLY_NOISY_REFLOW printing.
        result
    }

    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::ComputeAvailableSizeForFloat
    pub fn ComputeAvailableSizeForFloat(&self) -> LogicalSize {
        let wm = self.mReflowInput.GetWritingMode();
        let availBSize = if self.ContentBSize() == NS_UNCONSTRAINEDSIZE { NS_UNCONSTRAINEDSIZE } else { std::cmp::max(0, self.ContentBEnd() - self.mBCoord) };
        LogicalSize::new(wm, self.ContentISize(), availBSize)
    }

    /**
     * Append aFloatCont and its next-in-flows within the same block to mBlock's
     * pushed floats list. aFloatCont should not be on any child list when making
     * this call. Its next-in-flows will be removed from mBlock using StealFrame()
     * before being added to mBlock's pushed floats list. All appended frames will
     * be marked NS_FRAME_IS_PUSHED_OUT_OF_FLOW.
     */
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::AppendPushedFloatChain
    // NOT PORTED: fragmentation. Floats are only pushed past a column or page break (and
    // continued there), which mrt-layout does not do; the body is kept below.
    pub fn AppendPushedFloatChain(&mut self, aFloatCont: nsIFrame) {
        let mut aFloatCont = aFloatCont;
        let pushedFloats = self.mBlock.EnsurePushedFloats();
        loop {
            aFloatCont.AddStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW);
            pushedFloats.AppendFrame(Some(*self.mBlock), aFloatCont);
            match aFloatCont.GetNextInFlow() {
                Some(next) if next.GetParent() == Some(*self.mBlock) => aFloatCont = next,
                _ => break,
            }
            self.mBlock.StealFrame(aFloatCont);
        }
    }

    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::ContentIStart
    pub fn ContentIStart(&self) -> nscoord {
        self.mContentArea.IStart(self.mReflowInput.GetWritingMode())
    }
    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::ContentISize
    pub fn ContentISize(&self) -> nscoord {
        self.mContentArea.ISize(self.mReflowInput.GetWritingMode())
    }
    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::ContentIEnd
    pub fn ContentIEnd(&self) -> nscoord {
        self.mContentArea.IEnd(self.mReflowInput.GetWritingMode())
    }
    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::ContentBStart
    pub fn ContentBStart(&self) -> nscoord {
        self.mContentArea.BStart(self.mReflowInput.GetWritingMode())
    }
    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::ContentBSize
    pub fn ContentBSize(&self) -> nscoord {
        self.mContentArea.BSize(self.mReflowInput.GetWritingMode())
    }
    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::ContentBEnd
    pub fn ContentBEnd(&self) -> nscoord {
        NS_ASSERTION!(self.ContentBSize() != NS_UNCONSTRAINEDSIZE, "ContentBSize() is unconstrained, so ContentBEnd() may overflow.");
        self.mContentArea.BEnd(self.mReflowInput.GetWritingMode())
    }
    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::ContentSize
    pub fn ContentSize(&self, aWM: WritingMode) -> LogicalSize {
        let wm = self.mReflowInput.GetWritingMode();
        self.mContentArea.Size__WritingMode(wm).ConvertTo(aWM, wm)
    }

    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::ContainerSize
    pub fn ContainerSize(&self) -> nsSize {
        self.mContainerSize
    }

    /**
     * Restore information about floats into the float manager for an
     * incremental reflow, and simultaneously push the floats by
     * |aDeltaBCoord|, which is the amount |aLine| was pushed relative to its
     * parent.  The recovery of state is one of the things that makes
     * incremental reflow O(N^2) and this state should really be kept
     * around, attached to the frame tree.
     */
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::RecoverFloats
    fn RecoverFloats(&mut self, aLine: nsLineList_iterator, aDeltaBCoord: nscoord) {
        let wm = self.mReflowInput.GetWritingMode();
        if aLine.HasFloats() {
            // Place the floats into the float manager again. Also slide
            // them, just like the regular frames on the line.
            for &floatFrame in aLine.Floats() {
                if aDeltaBCoord != 0 {
                    floatFrame.MovePositionBy(nsPoint::new(0, aDeltaBCoord));
                }
                // NOT PORTED: noisy-reflow debug printing (#ifdef DEBUG, gNoisyFloatManager).
                self.FloatManager().AddFloat(floatFrame, nsFloatManager::GetRegionFor(wm, floatFrame, self.ContainerSize()), wm, self.ContainerSize());
            }
        } else if aLine.IsBlock() {
            nsBlockFrame::RecoverFloatsFor(aLine.mFirstChild.unwrap(), self.FloatManager(), wm, self.ContainerSize());
        }
    }

    /**
     * Everything done in this function is done O(N) times for each pass of
     * reflow so it is O(N*M) where M is the number of incremental reflow
     * passes.  That's bad.  Don't do stuff here.
     *
     * When this function is called, |aLine| has just been slid by |aDeltaBCoord|
     * and the purpose of RecoverStateFrom is to ensure that the
     * BlockReflowState is in the same state that it would have been in
     * had the line just been reflowed.
     *
     * Most of the state recovery that we have to do involves floats.
     */
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::RecoverStateFrom
    pub fn RecoverStateFrom(&mut self, aLine: nsLineList_iterator, aDeltaBCoord: nscoord) {
        // Make the line being recovered the current line
        self.mCurrentLine = aLine;

        // Place floats for this line into the float manager
        if aLine.HasFloats() || aLine.IsBlock() {
            self.RecoverFloats(aLine, aDeltaBCoord);

            // NOT PORTED: noisy-reflow debug printing (FloatManager()->List(stdout)).
        }
    }

    // Gecko: layout/generic/BlockReflowState.h BlockReflowState::AdvanceToNextLine
    pub fn AdvanceToNextLine(&mut self) {
        if self.mFlags.mIsLineLayoutEmpty {
            self.mFlags.mIsLineLayoutEmpty = false;
        } else {
            self.mLineNumber += 1;
        }
    }

    // This is called by the line layout's AddFloat method when a
    // place-holder frame is reflowed in a line. If the float is a
    // left-most child (it's x coordinate is at the line's left margin)
    // then the float is place immediately, otherwise the float
    // placement is deferred until the line has been reflowed.

    // XXXldb This behavior doesn't quite fit with CSS1 and CSS2 --
    // technically we're supposed let the current line flow around the
    // float as well unless it won't fit next to what we already have.
    // But nobody else implements it that way...
    //
    // @return true if AddFloat was able to place the float; false if the float
    // did not fit in available space.
    //
    // Note: if it returns false, then the float's position and size should be
    // considered stale/invalid (until the float is successfully placed).
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::AddFloat
    pub fn AddFloat(&mut self, aLineLayout: &mut nsLineLayout, aFloat: nsIFrame, aAvailableISize: nscoord) -> bool {
        // MOZ_ASSERT(aLineLayout, "must have line layout"): a reference cannot be null.
        MOZ_ASSERT!(self.mBlock.LinesEnd() != self.mCurrentLine, "null ptr");
        MOZ_ASSERT!(aFloat.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW), "aFloat must be an out-of-flow frame");

        MOZ_ASSERT!(aFloat.GetParent().is_some(), "float must have parent");
        MOZ_ASSERT!(aFloat.GetParent().unwrap().IsBlockFrameOrSubclass(), "float's parent must be block");
        if aFloat.HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW) || aFloat.GetParent() != Some(*self.mBlock) {
            MOZ_ASSERT!(
                aFloat.HasAnyStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW | NS_FRAME_FIRST_REFLOW),
                "float should be in this block unless it was marked as pushed out-of-flow, or just inserted"
            );
            // MOZ_ASSERT(aFloat->GetParent()->FirstContinuation() == mBlock->FirstContinuation());
            // NOT PORTED: continuations (FirstContinuation); no fragmentation.
            // If, in a previous reflow, the float was pushed entirely to
            // another column/page, we need to steal it back.  (We might just
            // push it again, though.)  Likewise, if that previous reflow
            // reflowed this block but not its next continuation, we might need
            // to steal it from our own float-continuations list.
            //
            // For more about pushed floats, see the comment above
            // nsBlockFrame::DrainPushedFloats.
            let floatParent = nsBlockFrame::from_frame(aFloat.GetParent().unwrap());
            floatParent.StealFrame(aFloat);

            aFloat.RemoveStateBits(NS_FRAME_IS_PUSHED_OUT_OF_FLOW);

            // Appending is fine, since if a float was pushed to the next
            // page/column, all later floats were also pushed.
            self.mBlock.EnsureFloats().AppendFrame(Some(*self.mBlock), aFloat);
        }

        // Because we are in the middle of reflowing a placeholder frame
        // within a line (and possibly nested in an inline frame or two
        // that's a child of our block) we need to restore the space
        // manager's translation to the space that the block resides in
        // before placing the float.
        let (mut oI, mut oB) = (0, 0);
        self.FloatManager().GetTranslation(&mut oI, &mut oB);
        let dI = oI - self.mFloatManagerI;
        let dB = oB - self.mFloatManagerB;
        self.FloatManager().Translate(-dI, -dB);

        let mut placed = false;

        // Now place the float immediately if possible. Otherwise stash it
        // away in mBelowCurrentLineFloats and place it later.
        // If one or more floats has already been pushed to the next line,
        // don't let this one go on the current line, since that would violate
        // float ordering.
        let mut shouldPlaceFloatBelowCurrentLine = false;
        if self.mBelowCurrentLineFloats.is_empty() {
            // If the current line is empty, we don't impose any inline-size constraint
            // from the line layout.
            let availableISizeInCurrentLine: Option<nscoord> = if aLineLayout.LineIsEmpty() { None } else { Some(aAvailableISize) };
            let result = self.FlowAndPlaceFloat(aFloat, availableISizeInCurrentLine);
            if result == BlockReflowState_PlaceFloatResult::Placed {
                placed = true;
                // Pass on updated available space to the current inline reflow engine
                let wm = self.mReflowInput.GetWritingMode();
                // If we have mLineBSize, we are reflowing the line again due to
                // LineReflowStatus::RedoMoreFloats. We should use mLineBSize to query the
                // correct available space.
                let floatAvailSpace = match self.mLineBSize {
                    None => self.GetFloatAvailableSpace__WritingMode_nscoord(wm, self.mBCoord),
                    Some(lineBSize) => self.GetFloatAvailableSpaceForBSize(wm, self.mBCoord, lineBSize, None),
                };
                let availSpace = LogicalRect::new(wm, floatAvailSpace.mRect.IStart(wm), self.mBCoord, floatAvailSpace.mRect.ISize(wm), floatAvailSpace.mRect.BSize(wm));
                aLineLayout.UpdateBand(wm, availSpace, aFloat);
                // Record this float in the current-line list
                self.mCurrentLineFloats.push(aFloat);
            } else if result == BlockReflowState_PlaceFloatResult::ShouldPlaceInNextContinuation {
                let mut line = *aLineLayout.GetLine().unwrap();
                line.SetHadFloatPushed();
            } else {
                MOZ_ASSERT!(result == BlockReflowState_PlaceFloatResult::ShouldPlaceBelowCurrentLine);
                shouldPlaceFloatBelowCurrentLine = true;
            }
        } else {
            shouldPlaceFloatBelowCurrentLine = true;
        }

        if shouldPlaceFloatBelowCurrentLine {
            // Always claim to be placed; we don't know whether we fit yet, so we
            // deal with this in PlaceBelowCurrentLineFloats
            placed = true;
            // This float will be placed after the line is done (it is a
            // below-current-line float).
            self.mBelowCurrentLineFloats.push(aFloat);
        }

        // Restore coordinate system
        self.FloatManager().Translate(dI, dB);

        placed
    }

    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::CanPlaceFloat
    fn CanPlaceFloat(&self, aFloatISize: nscoord, aFloatAvailableSpace: &nsFlowAreaRect) -> bool {
        // A float fits at a given block-dir position if there are no floats
        // at its inline-dir position (no matter what its inline size) or if
        // its inline size fits in the space remaining after prior floats have
        // been placed.
        // FIXME: We should allow overflow by up to half a pixel here (bug 21193).
        !aFloatAvailableSpace.HasFloats() || aFloatAvailableSpace.mRect.ISize(self.mReflowInput.GetWritingMode()) >= aFloatISize
    }

    // @param aAvailableISizeInCurrentLine the available inline-size of the
    //        current line if current line is not empty.
    // (aAvailableISizeInCurrentLine defaults to Nothing() in C++; the Rust caller passes None.)
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::FlowAndPlaceFloat
    pub fn FlowAndPlaceFloat(&mut self, aFloat: nsIFrame, aAvailableISizeInCurrentLine: Option<nscoord>) -> BlockReflowState_PlaceFloatResult {
        MOZ_ASSERT!(aFloat.GetParent() == Some(*self.mBlock), "Float frame has wrong parent");

        let wm = self.mReflowInput.GetWritingMode();
        // Save away the block-dir coordinate before placing the float. We will
        // restore mBCoord at the end after placing the float. This is
        // necessary because any adjustments to mBCoord during the float
        // placement are for the float only, not for any non-floating
        // content.
        // (AutoRestore<nscoord> restoreBCoord(mBCoord): Rust has no destructor that can reach
        // `self` while it is in use, so every `return` below restores it by hand through
        // `return_restoring!`.)
        let restoreBCoord = self.mBCoord;
        macro_rules! return_restoring {
            ($value:expr) => {{
                self.mBCoord = restoreBCoord;
                return $value;
            }};
        }

        // Whether the block-direction position available to place a float has been
        // pushed down due to the presence of other floats.
        // auto HasFloatPushedDown = [this, &restoreBCoord]() {
        //   return mBCoord != restoreBCoord.SavedValue();
        // };
        // (a closure over `self` would block every other use of it; an inline test where used.)

        // Grab the float's display information
        let floatDisplay = aFloat.StyleDisplay();

        // The float's old region, so we can propagate damage.
        let oldRegion = nsFloatManager::GetRegionFor(wm, aFloat, self.ContainerSize());

        let mut invalidationData: Option<ShapeInvalidationData> = aFloat.GetProperty(ShapeInvalidationDataProperty());

        // Enforce CSS2 9.5.1 rule [2], i.e., make sure that a float isn't
        // ``above'' another float that preceded it in the flow.
        self.mBCoord = std::cmp::max(self.FloatManager().LowestFloatBStart(), self.mBCoord);

        // See if the float should clear any preceding floats...
        // XXX We need to mark this float somehow so that it gets reflowed
        // when floats are inserted before it.
        if StyleClear::None != floatDisplay.mClear() {
            // XXXldb Does this handle vertical margins correctly?
            let (bCoord, result) = self.ClearFloats(self.mBCoord, floatDisplay.UsedClear(wm), None);
            if result == BlockReflowState_ClearFloatsResult::FloatsPushedOrSplit {
                self.PushFloatPastBreak(aFloat);
                return_restoring!(BlockReflowState_PlaceFloatResult::ShouldPlaceInNextContinuation);
            }
            self.mBCoord = bCoord;
        }

        let mut availSize = self.ComputeAvailableSizeForFloat();
        let floatWM = aFloat.GetWritingMode();
        let parentRI: &'a ReflowInput<'a> = self.mReflowInput;
        // Maybe<ReflowInput> floatRI(std::in_place, mPresContext, mReflowInput, aFloat, availSize.ConvertTo(floatWM, wm));
        let mut floatRI: Option<ReflowInput<'a>> = Some(Self::NewFloatReflowInput(&self.mPresContext, parentRI, aFloat, availSize.ConvertTo(floatWM, wm)));

        let mut floatMarginISize = FloatMarginISize(wm, floatRI.as_ref().unwrap());
        let mut floatMargin = floatRI.as_ref().unwrap().ComputedLogicalMargin(wm);
        let mut reflowStatus = nsReflowStatus::new();

        // If it's a floating first-letter, we need to reflow it before we
        // know how wide it is (since we don't compute which letters are part
        // of the first letter until reflow!).
        // We also need to do this early reflow if FloatMarginISize returned
        // an unconstrained inline-size, which can occur if the float had an
        // orthogonal writing mode and 'auto' block-size (in its mode).
        let earlyFloatReflow = aFloat.IsLetterFrame() || floatMarginISize == NS_UNCONSTRAINEDSIZE;
        if earlyFloatReflow {
            let block = self.mBlock;
            block.ReflowFloat(self, floatRI.as_mut().unwrap(), aFloat, &mut reflowStatus);
            floatMarginISize = aFloat.ISize(wm) + floatMargin.IStartEnd(wm);
            NS_ASSERTION!(
                reflowStatus.IsComplete(),
                "letter frames and orthogonal floats with auto block-size shouldn't break, and if they do now, then they're breaking at the wrong point"
            );
        }

        // Now we've computed the float's margin inline-size.
        if let Some(availableISizeInCurrentLine) = aAvailableISizeInCurrentLine {
            if floatMarginISize > availableISizeInCurrentLine {
                // The float cannot fit in the available inline-size of the current line.
                // Let's notify our caller to place it later.
                return_restoring!(BlockReflowState_PlaceFloatResult::ShouldPlaceBelowCurrentLine);
            }
        }

        // Find a place to place the float. The CSS2 spec doesn't want
        // floats overlapping each other or sticking out of the containing
        // block if possible (CSS2 spec section 9.5.1, see the rule list).
        let floatStyle = floatDisplay.UsedFloat(wm);
        MOZ_ASSERT!(UsedFloat::Left == floatStyle || UsedFloat::Right == floatStyle, "Invalid float type!");

        // Are we required to place at least part of the float because we're
        // at the top of the page (to avoid an infinite loop of pushing and
        // breaking).
        let mut mustPlaceFloat = self.mReflowInput.mFlags.mIsTopOfPage && self.IsAdjacentWithBStart();

        // Get the band of available space with respect to margin box.
        let mut floatAvailableSpace = self.GetFloatAvailableSpaceForPlacingFloat(wm, self.mBCoord);

        loop {
            if self.mReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE && floatAvailableSpace.mRect.BSize(wm) <= 0 && !mustPlaceFloat {
                // No space, nowhere to put anything.
                self.PushFloatPastBreak(aFloat);
                return_restoring!(BlockReflowState_PlaceFloatResult::ShouldPlaceInNextContinuation);
            }

            if self.CanPlaceFloat(floatMarginISize, &floatAvailableSpace) {
                // We found an appropriate place.
                break;
            }

            // Nope. try to advance to the next band.
            self.mBCoord += floatAvailableSpace.mRect.BSize(wm);
            floatAvailableSpace = self.GetFloatAvailableSpaceForPlacingFloat(wm, self.mBCoord);
            mustPlaceFloat = false;
        }

        // If the float is continued, it will get the same absolute x value as its
        // prev-in-flow

        // We don't worry about the geometry of the prev in flow, let the continuation
        // place and size itself as required.

        // Assign inline and block dir coordinates to the float. We don't use
        // LineLeft() and LineRight() here, because we would only have to
        // convert the result back into this block's writing mode.
        let mut floatPos = LogicalPoint::new(wm, 0, 0);
        let leftFloat = floatStyle == UsedFloat::Left;

        if leftFloat == wm.IsBidiLTR() {
            *floatPos.I_mut(wm) = floatAvailableSpace.mRect.IStart(wm);
        } else {
            *floatPos.I_mut(wm) = floatAvailableSpace.mRect.IEnd(wm) - floatMarginISize;
        }
        // CSS2 spec, 9.5.1 rule [4]: "A floating box's outer top may not
        // be higher than the top of its containing block."  (Since the
        // containing block is the content edge of the block box, this
        // means the margin edge of the float can't be higher than the
        // content edge of the block that contains it.)
        *floatPos.B_mut(wm) = std::cmp::max(self.mBCoord, self.ContentBStart());

        // Reflow the float after computing its vertical position so it knows
        // where to break.
        if !earlyFloatReflow {
            let oldAvailSize = availSize;
            availSize = self.ComputeAvailableSizeForFloat();
            if oldAvailSize != availSize {
                // floatRI.reset(); floatRI.emplace(mPresContext, mReflowInput, aFloat, availSize.ConvertTo(floatWM, wm));
                floatRI = Some(Self::NewFloatReflowInput(&self.mPresContext, parentRI, aFloat, availSize.ConvertTo(floatWM, wm)));
            }
            // Normally the mIsTopOfPage state is copied from the parent reflow input.
            // However, when reflowing a float, if we've placed other floats that force
            // this float being pushed down, we should unset the mIsTopOfPage bit.
            // (HasFloatPushedDown() is `mBCoord != restoreBCoord`.)
            if floatRI.as_ref().unwrap().mFlags.mIsTopOfPage && self.mBCoord != restoreBCoord {
                // HasFloatPushedDown() implies that we increased mBCoord, and we
                // should've turned off mustPlaceFloat when we did that.
                NS_ASSERTION!(!mustPlaceFloat, "mustPlaceFloat shouldn't be set if we're not at the top-of-page!");
                floatRI.as_mut().unwrap().mFlags.mIsTopOfPage = false;
            }
            let block = self.mBlock;
            block.ReflowFloat(self, floatRI.as_mut().unwrap(), aFloat, &mut reflowStatus);
        }
        if aFloat.GetPrevInFlow().is_some() {
            *floatMargin.BStart_mut(wm) = 0;
        }
        if reflowStatus.IsIncomplete() {
            *floatMargin.BEnd_mut(wm) = 0;
        }

        // If the float cannot fit (e.g. via fragmenting itself if applicable), or if
        // we're forced to break before it for CSS break-* reasons, then it needs to
        // be pushed in its entirety to the next column/page.
        //
        // Note we use the available block-size in floatRI rather than use
        // availSize.BSize() because nsBlockReflowContext::ReflowBlock() might adjust
        // floatRI's available size.
        let availBSize = floatRI.as_ref().unwrap().AvailableSize__WritingMode(floatWM).BSize(floatWM);
        let isTruncated = availBSize != NS_UNCONSTRAINEDSIZE && aFloat.BSize(floatWM) > availBSize;
        if (!floatRI.as_ref().unwrap().mFlags.mIsTopOfPage && isTruncated) || reflowStatus.IsInlineBreakBefore() {
            self.PushFloatPastBreak(aFloat);
            return_restoring!(BlockReflowState_PlaceFloatResult::ShouldPlaceInNextContinuation);
        }

        // We can't use aFloat->ShouldAvoidBreakInside(mReflowInput) here since
        // its mIsTopOfPage may be true even though the float isn't at the
        // top when floatPos.B(wm) > 0.
        if self.ContentBSize() != NS_UNCONSTRAINEDSIZE
            && !mustPlaceFloat
            && (!self.mReflowInput.mFlags.mIsTopOfPage || floatPos.B(wm) > 0)
            && StyleBreakWithin::Avoid == aFloat.StyleDisplay().mBreakInside()
            && (!reflowStatus.IsFullyComplete() || aFloat.BSize(wm) + floatMargin.BStartEnd(wm) > self.ContentBEnd() - floatPos.B(wm))
            && aFloat.GetPrevInFlow().is_none()
        {
            self.PushFloatPastBreak(aFloat);
            return_restoring!(BlockReflowState_PlaceFloatResult::ShouldPlaceInNextContinuation);
        }

        // Calculate the actual origin of the float frame's border rect
        // relative to the parent block; the margin must be added in
        // to get the border rect
        let mut origin = LogicalPoint::new(wm, floatMargin.IStart(wm) + floatPos.I(wm), floatMargin.BStart(wm) + floatPos.B(wm));

        // If float is relatively positioned, factor that in as well
        let floatOffsets: LogicalMargin = floatRI.as_ref().unwrap().ComputedLogicalOffsets(wm);
        ReflowInput::ApplyRelativePositioning__nsIFrame_WritingMode_LogicalMargin_LogicalPoint_nsSize(aFloat, wm, floatOffsets, &mut origin, self.ContainerSize());

        // Position the float.
        let moved = aFloat.GetLogicalPosition__WritingMode_nsSize(wm, self.ContainerSize()) != origin;
        if moved {
            aFloat.SetPosition__WritingMode_LogicalPoint_nsSize(wm, origin, self.ContainerSize());
        }

        // Update the float combined area state
        // XXX Floats should really just get invalidated here if necessary
        self.mFloatOverflowAreas.UnionWith(&aFloat.GetOverflowAreasRelativeToParent());

        // Place the float in the float manager
        // calculate region
        let mut region = nsFloatManager::CalculateRegionFor(wm, aFloat, floatMargin, self.ContainerSize());
        // if the float split, then take up all of the vertical height
        if reflowStatus.IsIncomplete() && (NS_UNCONSTRAINEDSIZE != self.ContentBSize()) {
            *region.BSize_mut(wm) = std::cmp::max(region.BSize(wm), self.ContentBSize() - floatPos.B(wm));
        }
        self.FloatManager().AddFloat(aFloat, region, wm, self.ContainerSize());

        // store region
        nsFloatManager::StoreRegionFor(wm, aFloat, region, self.ContainerSize());

        let invalidationDataNeeded = ShapeInvalidationData::IsNeeded(&floatDisplay);

        // If the float's dimensions or shape have changed, note the damage in the
        // float manager.
        if !region.IsEqualEdges(oldRegion)
            || invalidationData.is_some() != invalidationDataNeeded
            || (invalidationData.is_some() && !invalidationData.as_ref().unwrap().Matches(&floatDisplay))
        {
            // XXXwaterson conservative: we could probably get away with noting
            // less damage; e.g., if only height has changed, then only note the
            // area into which the float has grown or from which the float has
            // shrunk.
            let blockStart = std::cmp::min(region.BStart(wm), oldRegion.BStart(wm));
            let blockEnd = std::cmp::max(region.BEnd(wm), oldRegion.BEnd(wm));
            self.FloatManager().IncludeInDamage(blockStart, blockEnd);
        }

        if invalidationDataNeeded {
            if let Some(data) = invalidationData.as_mut() {
                data.Update(&floatDisplay);
                // (C++ updates the stored object through the pointer; here the property holds a copy.)
                aFloat.SetProperty(ShapeInvalidationDataProperty(), data.clone());
            } else {
                aFloat.SetProperty(ShapeInvalidationDataProperty(), ShapeInvalidationData::new(&floatDisplay));
            }
        } else if invalidationData.is_some() {
            invalidationData = None;
            aFloat.RemoveProperty(ShapeInvalidationDataProperty());
        }

        if !reflowStatus.IsFullyComplete() {
            // NOT PORTED: fragmentation (nsBlockFrame::SplitFloat creates the float's continuation).
            let block = self.mBlock;
            block.SplitFloat(self, aFloat, &reflowStatus);
        } else {
            MOZ_ASSERT!(aFloat.GetNextInFlow().is_none());
        }

        // NOT PORTED: noisy-reflow debug printing (#ifdef DEBUG, gNoisyFloatManager, gNoisyReflow).

        return_restoring!(BlockReflowState_PlaceFloatResult::Placed)
    }

    // ADAPTER: the C++ line `ReflowInput(mPresContext, mReflowInput, aFloat, availSize)` uses the
    // constructor's default arguments; Rust has none, so they are written out here once.
    fn NewFloatReflowInput(aPresContext: &nsPresContext, aParentRI: &'a ReflowInput<'a>, aFloat: nsIFrame, aAvailableSpace: LogicalSize) -> ReflowInput<'a> {
        ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            aPresContext,
            aParentRI,
            aFloat,
            aAvailableSpace,
            None,
            EnumSet::default(),
            StyleSizeOverrides::default(),
            ComputeSizeFlags::default(),
            std::ptr::null_mut(),
        )
    }

    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::PushFloatPastBreak
    fn PushFloatPastBreak(&mut self, aFloat: nsIFrame) {
        // This ensures that we:
        //  * don't try to place later but smaller floats (which CSS says
        //    must have their tops below the top of this float)
        //  * don't waste much time trying to reflow this float again until
        //    after the break
        let wm = self.mReflowInput.GetWritingMode();
        let floatStyle = aFloat.StyleDisplay().UsedFloat(wm);
        if floatStyle == UsedFloat::Left {
            self.FloatManager().SetPushedLeftFloatPastBreak();
        } else {
            MOZ_ASSERT!(floatStyle == UsedFloat::Right, "Unexpected float value!");
            self.FloatManager().SetPushedRightFloatPastBreak();
        }

        // Put the float on the pushed floats list, even though it
        // isn't actually a continuation.
        self.mBlock.StealFrame(aFloat);
        self.AppendPushedFloatChain(aFloat);
        self.mReflowStatus.SetOverflowIncomplete();
    }

    /**
     * Place below-current-line floats.
     */
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::PlaceBelowCurrentLineFloats
    pub fn PlaceBelowCurrentLineFloats(&mut self, aLine: &mut nsLineBox) {
        MOZ_ASSERT!(!self.mBelowCurrentLineFloats.is_empty());
        let mut floatsPlacedInLine: Vec<nsIFrame> = Vec::new();
        // (C++ iterates mBelowCurrentLineFloats while FlowAndPlaceFloat never touches it.)
        let belowCurrentLineFloats = self.mBelowCurrentLineFloats.clone();
        for f in belowCurrentLineFloats {
            // NOT PORTED: noisy-reflow debug printing (#ifdef DEBUG, gNoisyReflow).
            // Place the float
            let result = self.FlowAndPlaceFloat(f, None);
            MOZ_ASSERT!(result != BlockReflowState_PlaceFloatResult::ShouldPlaceBelowCurrentLine, "We are already dealing with below current line floats!");
            if result == BlockReflowState_PlaceFloatResult::Placed {
                floatsPlacedInLine.push(f);
            }
        }
        if floatsPlacedInLine.len() != self.mBelowCurrentLineFloats.len() {
            // We have some floats having ShouldPlaceInNextContinuation result.
            aLine.SetHadFloatPushed();
        }
        aLine.AppendFloats(floatsPlacedInLine);
        self.mBelowCurrentLineFloats.clear();
    }

    // Returns the first coordinate >= aBCoord that clears the
    // floats indicated by aClearType and has enough inline size between floats
    // (or no floats remaining) to accomodate aFloatAvoidingBlock.
    // (aFloatAvoidingBlock defaults to nullptr in C++.)
    // Gecko: layout/generic/BlockReflowState.cpp BlockReflowState::ClearFloats
    pub fn ClearFloats(&mut self, aBCoord: nscoord, aClearType: UsedClear, aFloatAvoidingBlock: Option<nsIFrame>) -> (nscoord, BlockReflowState_ClearFloatsResult) {
        // NOT PORTED: noisy-reflow debug printing (#ifdef DEBUG, gNoisyReflow).

        if !self.FloatManager().HasAnyFloats() {
            return (aBCoord, BlockReflowState_ClearFloatsResult::BCoordNoChange);
        }

        let mut newBCoord = aBCoord;

        if aClearType != UsedClear::None {
            newBCoord = self.FloatManager().ClearFloats(newBCoord, aClearType);

            if self.FloatManager().ClearContinues(aClearType) {
                return (newBCoord, BlockReflowState_ClearFloatsResult::FloatsPushedOrSplit);
            }
        }

        if let Some(aFloatAvoidingBlock) = aFloatAvoidingBlock {
            let cbWM = aFloatAvoidingBlock.GetContainingBlock__uint32_t(0).unwrap().GetWritingMode();
            loop {
                let floatAvailableSpace = self.GetFloatAvailableSpace__WritingMode_nscoord(cbWM, newBCoord);
                if self.FloatAvoidingBlockFitsInAvailSpace(aFloatAvoidingBlock, &floatAvailableSpace) {
                    break;
                }
                // See the analogous code for inlines in
                // nsBlockFrame::DoReflowInlineFrames
                if !self.AdvanceToNextBand(&floatAvailableSpace.mRect, &mut newBCoord) {
                    // Stop trying to clear here; we'll just get pushed to the
                    // next column or page and try again there.
                    break;
                }
            }
        }

        // NOT PORTED: noisy-reflow debug printing (#ifdef DEBUG, gNoisyReflow).

        let result = if newBCoord == aBCoord { BlockReflowState_ClearFloatsResult::BCoordNoChange } else { BlockReflowState_ClearFloatsResult::BCoordAdvanced };
        (newBCoord, result)
    }
}

// Return the inline-size that the float (including margins) will take up
// in the writing mode of the containing block. If this returns
// NS_UNCONSTRAINEDSIZE, we're dealing with an orthogonal block that
// has block-size:auto, and we'll need to actually reflow it to find out
// how much inline-size it will occupy in the containing block's mode.
// Gecko: layout/generic/BlockReflowState.cpp FloatMarginISize
fn FloatMarginISize(aCBWM: WritingMode, aFloatRI: &ReflowInput) -> nscoord {
    if aFloatRI.ComputedSize__WritingMode(aCBWM).ISize(aCBWM) == NS_UNCONSTRAINEDSIZE {
        return NS_UNCONSTRAINEDSIZE; // reflow is needed to get the true size
    }
    aFloatRI.ComputedSizeWithMarginBorderPadding(aCBWM).ISize(aCBWM)
}

// A frame property that stores the last shape source / margin / etc. if there's
// any shape, in order to invalidate the float area properly when it changes.
//
// TODO(emilio): This could really belong to GetRegionFor / StoreRegionFor, but
// when I tried it was a bit awkward because of the logical -> physical
// conversion that happens there.
//
// Maybe all this code could be refactored to make this cleaner, but keeping the
// two properties separated was slightly nicer.
// Gecko: layout/generic/BlockReflowState.cpp ShapeInvalidationData
// NOT PORTED: shape-outside (`shape-outside` is not in the servo configuration of Stylo, so
// IsNeeded() is always false and this struct is never stored; it is kept so that FlowAndPlaceFloat
// keeps its shape). mShapeMargin is the f32 that nsStyleDisplay::mShapeMargin() returns here
// (Gecko: LengthPercentage).
#[derive(Clone)]
struct ShapeInvalidationData {
    mShapeOutside: StyleShapeOutside,
    mShapeImageThreshold: f32,
    mShapeMargin: f32,
}

impl ShapeInvalidationData {
    // ShapeInvalidationData() = default;
    fn default() -> ShapeInvalidationData {
        ShapeInvalidationData { mShapeOutside: StyleShapeOutside::None, mShapeImageThreshold: 0.0, mShapeMargin: 0.0 }
    }

    // explicit ShapeInvalidationData(const nsStyleDisplay& aDisplay)
    fn new(aDisplay: &nsStyleDisplay) -> ShapeInvalidationData {
        let mut result = ShapeInvalidationData::default();
        result.Update(aDisplay);
        result
    }

    fn IsNeeded(aDisplay: &nsStyleDisplay) -> bool {
        aDisplay.mShapeOutside() != StyleShapeOutside::None
    }

    fn Update(&mut self, aDisplay: &nsStyleDisplay) {
        MOZ_ASSERT!(ShapeInvalidationData::IsNeeded(aDisplay));
        self.mShapeOutside = aDisplay.mShapeOutside();
        self.mShapeImageThreshold = aDisplay.mShapeImageThreshold();
        self.mShapeMargin = aDisplay.mShapeMargin();
    }

    fn Matches(&self, aDisplay: &nsStyleDisplay) -> bool {
        self.mShapeOutside == aDisplay.mShapeOutside() && self.mShapeImageThreshold == aDisplay.mShapeImageThreshold() && self.mShapeMargin == aDisplay.mShapeMargin()
    }
}

// Gecko: layout/generic/BlockReflowState.cpp ShapeInvalidationDataProperty
// (the id is this crate's own number for the property; 1 is FloatRegionProperty, 100-105 are in nsIFrame.rs)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(ShapeInvalidationDataProperty, ShapeInvalidationData, 200);

// NOT PORTED: refcount logging (none here), LAYOUT_WARN_IF_FALSE (is NS_WARNING_ASSERTION above).

// Tests: Firefox has no gtest for BlockReflowState, so these are written from the documented
// behaviour (CSS 2.2 9.5 floats, 9.5.2 clear, 10.3.3 block width) and the comments above, on real
// Stylo styles; see the test helpers below.
#[cfg(test)]
pub mod BlockReflowState_test_support {
    use crate::*;
    use mrt_style::mrt_dom::{Dom, Kind, NodeId};
    use mrt_style::style::properties::ComputedValues;
    use mrt_style::style::servo_arc::Arc as ServoArc;
    use mrt_style::StyleDom;

    pub const PX: i32 = 60;

    // Parses `html`, resolves styles, and returns the computed style of each element with an id.
    pub fn styles_of(html: &str, ids: &[&str]) -> Vec<ServoArc<ComputedValues>> {
        let mut sd = StyleDom::new(Dom::parse(html), (1489.0, 844.0));
        sd.resolve();
        ids.iter()
            .map(|want| {
                let dom = &sd.tree.dom;
                let node = (0..dom.nodes.len() as NodeId)
                    .find(|&n| dom.node(n).kind == Kind::Element && dom.attrs_of(n).iter().any(|a| dom.names.get(a.name) == "id" && dom.attr_value(a) == *want))
                    .unwrap_or_else(|| panic!("no element with id {want}"));
                sd.computed(node).expect("no computed style")
            })
            .collect()
    }

    // Frames for the elements with these ids, each the child of the frame at the index given with it
    // (None for the root); all of type Block, with an empty rect except the root (`root_width` px wide).
    pub fn frames(html: &str, tree: &[(&str, Option<usize>)], root_width: i32) -> Vec<nsIFrame> {
        let ids: Vec<&str> = tree.iter().map(|c| c.0).collect();
        let styles = styles_of(html, &ids);
        WithFrameTree(|t| {
            let mut result: Vec<nsIFrame> = Vec::new();
            for (i, style) in styles.into_iter().enumerate() {
                let wm = WritingMode::new__ComputedStyle(&style);
                let rect = if i == 0 { nsRect::new(0, 0, root_width * PX, 0) } else { nsRect::new(0, 0, 0, 0) };
                let parent = tree[i].1.map(|p| result[p]);
                let f = t.AddFrame(parent, rect, wm, Some(style));
                result.push(f);
            }
            result
        })
    }

    // A new block line (one child, block-outside) / inline line (one inline-outside child), as
    // NS_NewLineBox makes them. The caller owns the line (a Box::from_raw of it) unless it appends it
    // to a nsLineList, which then owns it.
    pub fn block_line(aChild: nsIFrame) -> *mut nsLineBox {
        NS_NewLineBox__nsIFrame_bool(Some(aChild), true)
    }
    pub fn inline_line(aChild: nsIFrame) -> *mut nsLineBox {
        NS_NewLineBox__nsIFrame_bool(Some(aChild), false)
    }

    // The reflow input of the root frame: available inline-size `avail_i` px, block-size `avail_b`
    // (NS_UNCONSTRAINEDSIZE for none).
    pub fn root_input<'a>(f: nsIFrame, avail_i: i32, avail_b: nscoord) -> ReflowInput<'a> {
        let wm = f.GetWritingMode();
        ReflowInput::new__nsPresContext_nsIFrame_gfxContext_LogicalSize_InitFlags(&nsPresContext::Current(), f, gfxContext, LogicalSize::new(wm, avail_i * PX, avail_b), EnumSet::default())
    }

    // The reflow input of a child, as a block parent passes it: the parent's content inline-size.
    pub fn child_input<'a>(parent: &'a ReflowInput<'a>, f: nsIFrame) -> ReflowInput<'a> {
        let wm = f.GetWritingMode();
        let avail = LogicalSize::new(wm, parent.ComputedISize(), NS_UNCONSTRAINEDSIZE);
        ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            &nsPresContext::Current(),
            parent,
            f,
            avail,
            None,
            EnumSet::default(),
            StyleSizeOverrides::default(),
            ComputeSizeFlags::default(),
            std::ptr::null_mut(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockReflowState_test_support::*;
    use crate::nsLineLayoutAdapter::TextFrameStub;

    const AUTO: nscoord = NS_UNCONSTRAINEDSIZE;

    // A 10 (PX au) wide block `root` with the given styles for children f1, f2, ...: each a
    // block child of root. Runs `body` with a BlockReflowState for root (a margin root with a
    // float manager) and the frames [root, f1, f2, ..., inline], where `inline` is one more child, a
    // display:inline frame that an inline line box can hold (nsLineBox asserts the kind of its child).
    fn with_state(root_style: &str, children: &[&str], avail_b: nscoord, body: impl FnOnce(&mut BlockReflowState, &[nsIFrame])) {
        let mut html = format!(r#"<div id=root style="margin:0;border:0;{root_style}">"#);
        let mut tree: Vec<(String, Option<usize>)> = vec![("root".to_string(), None)];
        for (i, style) in children.iter().enumerate() {
            html += &format!(r#"<div id=f{i} style="{style}"></div>"#);
            tree.push((format!("f{i}"), Some(0)));
        }
        html += r#"<div id=inl style="display:inline"></div>"#;
        tree.push(("inl".to_string(), Some(0)));
        html += "</div>";
        let tree_ref: Vec<(&str, Option<usize>)> = tree.iter().map(|(n, p)| (n.as_str(), *p)).collect();
        let f = frames(&html, &tree_ref, 10);
        for c in &f[1..f.len() - 1] {
            c.AddStateBits(NS_FRAME_OUT_OF_FLOW);
        }
        let wm = f[0].GetWritingMode();
        let mut fm = nsFloatManager::new(wm);
        let mut root = root_input(f[0], 10, avail_b);
        root.mFloatManager = &mut fm;
        let root = root;
        let mut state = BlockReflowState::new(&root, nsPresContext::Current(), nsBlockFrame::from_frame(f[0]), true, true, true, 0, AUTO, 0);
        body(&mut state, &f);
    }

    fn left_float(w: i32, h: i32) -> String {
        format!("float:left;margin:0;width:{w}px;height:{h}px")
    }
    fn right_float(w: i32, h: i32) -> String {
        format!("float:right;margin:0;width:{w}px;height:{h}px")
    }

    #[test]
    fn constructor_sets_content_area_and_margin_root_flags() {
        with_state("padding:3px 4px 5px 6px", &[], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            assert_eq!(s.mBCoord, 3 * PX);
            assert_eq!(s.ContentBStart(), 3 * PX);
            assert_eq!(s.ContentIStart(), 6 * PX);
            assert_eq!(s.ContentISize(), 10 * PX - 10 * PX);
            assert_eq!(s.ContentBSize(), AUTO);
            assert!(s.mFlags.mIsBStartMarginRoot && s.mFlags.mIsBEndMarginRoot);
            assert!(s.mFlags.mShouldApplyBStartMargin);
            assert!(s.mFlags.mBlockNeedsFloatManager);
            assert!(!s.mFlags.mCanHaveOverflowMarkers);
            assert_eq!(s.mBorderPadding.BStartEnd(wm), 8 * PX);
            assert!(s.IsAdjacentWithBStart());
        });
    }

    #[test]
    fn constructor_margin_root_follows_border_padding() {
        // Not a margin root by argument, but block-start padding makes it one (only that side).
        let html = r#"<div id=root style="margin:0;border:0;padding-top:2px"></div>"#;
        let f = frames(html, &[("root", None)], 10);
        let mut fm = nsFloatManager::new(f[0].GetWritingMode());
        let mut root = root_input(f[0], 10, AUTO);
        root.mFloatManager = &mut fm;
        let root = root;
        let s = BlockReflowState::new(&root, nsPresContext::Current(), nsBlockFrame::from_frame(f[0]), false, false, false, 0, AUTO, 0);
        assert!(s.mFlags.mIsBStartMarginRoot);
        assert!(!s.mFlags.mIsBEndMarginRoot);
        assert!(!s.mFlags.mBlockNeedsFloatManager);
    }

    #[test]
    fn constructor_paginated_content_block_size() {
        // A constrained available block-size minus the block-start border+padding is the
        // content area's block-size.
        with_state("padding-top:10px", &[], 100 * PX, |s, _| {
            assert_eq!(s.ContentBSize(), 90 * PX);
            assert_eq!(s.ContentBEnd(), 100 * PX);
        });
    }

    #[test]
    fn align_content_shift_is_applied_and_undone() {
        let html = r#"<div id=root style="margin:0;border:0;align-content:center"></div>"#;
        let f = frames(html, &[("root", None)], 10);
        let block = nsBlockFrame::from_frame(f[0]);
        block.SetProperty(AlignContentShift(), 5 * PX);
        let mut fm = nsFloatManager::new(f[0].GetWritingMode());
        let mut root = root_input(f[0], 10, 100 * PX);
        root.mFloatManager = &mut fm;
        let root = root;
        let mut s = BlockReflowState::new(&root, nsPresContext::Current(), block, true, true, true, 0, AUTO, 0);
        assert_eq!(s.mBCoord, 5 * PX);
        assert_eq!(s.ContentBStart(), 5 * PX);
        assert_eq!(s.ContentBSize(), 105 * PX);
        s.UndoAlignContentShift();
        assert_eq!(s.mBCoord, 0);
        assert_eq!(s.ContentBStart(), 0);
        assert_eq!(s.ContentBSize(), 100 * PX);
    }

    #[test]
    fn available_space_without_floats_is_the_content_area() {
        with_state("", &[], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            let r = s.GetFloatAvailableSpace__WritingMode(wm);
            assert!(!r.HasFloats());
            assert_eq!(r.mRect.IStart(wm), 0);
            assert_eq!(r.mRect.ISize(wm), 10 * PX);
        });
    }

    #[test]
    fn left_float_narrows_available_space() {
        let l = left_float(4, 5);
        with_state("", &[&l], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            assert_eq!(s.FlowAndPlaceFloat(f[1], None), BlockReflowState_PlaceFloatResult::Placed);
            assert_eq!(f[1].GetPosition(), nsPoint::new(0, 0));
            assert_eq!(f[1].GetSize(), nsSize::new(4 * PX, 5 * PX));
            let r = s.GetFloatAvailableSpace__WritingMode(wm);
            assert!(r.HasFloats());
            assert_eq!(r.mRect.IStart(wm), 4 * PX);
            assert_eq!(r.mRect.ISize(wm), 6 * PX);
            assert_eq!(r.mRect.BSize(wm), 5 * PX);
            // Below the float the whole width is free again.
            let below = s.GetFloatAvailableSpace__WritingMode_nscoord(wm, 5 * PX);
            assert!(!below.HasFloats());
            assert_eq!(below.mRect.ISize(wm), 10 * PX);
            assert_eq!(s.mBCoord, 0);
        });
    }

    #[test]
    fn floats_stack_left_and_right_and_share_a_line() {
        // The root is `10 * PX` au = 10 CSS px wide here; a left and a right float of 4px each
        // fit on the same line; a left one of 4px next to them does not (rule 5/7).
        let l = left_float(4, 5);
        let r = right_float(4, 3);
        let l2 = left_float(4, 2);
        with_state("", &[&l, &r, &l2], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            assert_eq!(s.FlowAndPlaceFloat(f[1], None), BlockReflowState_PlaceFloatResult::Placed);
            assert_eq!(s.FlowAndPlaceFloat(f[2], None), BlockReflowState_PlaceFloatResult::Placed);
            assert_eq!(f[1].GetPosition(), nsPoint::new(0, 0));
            assert_eq!(f[2].GetPosition(), nsPoint::new(6 * PX, 0));
            let space = s.GetFloatAvailableSpace__WritingMode(wm);
            assert_eq!(space.mRect.IStart(wm), 4 * PX);
            assert_eq!(space.mRect.ISize(wm), 2 * PX);
            // The third float (4px) does not fit in the remaining 2px: it goes down to the
            // bottom of the shorter float (3px), where only the left float (5px tall) remains.
            assert_eq!(s.FlowAndPlaceFloat(f[3], None), BlockReflowState_PlaceFloatResult::Placed);
            assert_eq!(f[3].GetPosition(), nsPoint::new(4 * PX, 3 * PX));
            // mBCoord is restored after placing a float (AutoRestore).
            assert_eq!(s.mBCoord, 0);
        });
    }

    #[test]
    fn float_that_cannot_fit_beside_another_goes_below_it() {
        // CSS 2.2 9.5.1 rule 7: a float that is too wide for the space next to an earlier float
        // is shifted down until it fits or there are no more floats.
        let a = left_float(7, 4);
        let b = left_float(7, 4);
        with_state("", &[&a, &b], AUTO, |s, f| {
            assert_eq!(s.FlowAndPlaceFloat(f[1], None), BlockReflowState_PlaceFloatResult::Placed);
            assert_eq!(s.FlowAndPlaceFloat(f[2], None), BlockReflowState_PlaceFloatResult::Placed);
            assert_eq!(f[2].GetPosition(), nsPoint::new(0, 4 * PX));
        });
    }

    #[test]
    fn float_is_not_higher_than_an_earlier_float() {
        // Rule 2: the top of a float may not be higher than the top of an earlier float.
        let a = left_float(7, 4);
        let b = left_float(7, 4);
        let c = right_float(2, 1);
        with_state("", &[&a, &b, &c], AUTO, |s, f| {
            s.FlowAndPlaceFloat(f[1], None);
            s.FlowAndPlaceFloat(f[2], None); // goes to y = 4px
            s.FlowAndPlaceFloat(f[3], None); // would fit at y = 0, but must not be above b
            assert_eq!(f[3].GetPosition().y, 4 * PX);
        });
    }

    #[test]
    fn float_with_clear_goes_below_the_cleared_floats() {
        let a = left_float(2, 4);
        let b = "float:left;clear:left;margin:0;width:2px;height:1px".to_string();
        with_state("", &[&a, &b], AUTO, |s, f| {
            s.FlowAndPlaceFloat(f[1], None);
            assert_eq!(s.FlowAndPlaceFloat(f[2], None), BlockReflowState_PlaceFloatResult::Placed);
            assert_eq!(f[2].GetPosition(), nsPoint::new(0, 4 * PX));
        });
    }

    #[test]
    fn float_wider_than_the_current_line_goes_below_it() {
        // With a non-empty current line the float must fit in the line's available inline-size,
        // otherwise the caller is told to place it after the line.
        let a = left_float(6, 2);
        with_state("", &[&a], AUTO, |s, f| {
            assert_eq!(s.FlowAndPlaceFloat(f[1], Some(5 * PX)), BlockReflowState_PlaceFloatResult::ShouldPlaceBelowCurrentLine);
            assert_eq!(s.FlowAndPlaceFloat(f[1], Some(6 * PX)), BlockReflowState_PlaceFloatResult::Placed);
        });
    }

    #[test]
    fn float_overflow_areas_and_damage_are_recorded() {
        let a = left_float(3, 2);
        with_state("", &[&a], AUTO, |s, f| {
            s.FlowAndPlaceFloat(f[1], None);
            assert_eq!(*s.mFloatOverflowAreas.InkOverflow(), nsRect::new(0, 0, 3 * PX, 2 * PX));
            // The float is new, so its region differs from the (empty) old one: damage.
            assert!(s.FloatManager().HasFloatDamage());
        });
    }

    #[test]
    fn clear_floats_advances_past_floats() {
        let a = left_float(2, 4);
        let b = right_float(2, 6);
        with_state("", &[&a, &b], AUTO, |s, f| {
            // No floats yet: unchanged.
            assert_eq!(s.ClearFloats(0, UsedClear::Both, None), (0, BlockReflowState_ClearFloatsResult::BCoordNoChange));
            s.FlowAndPlaceFloat(f[1], None);
            s.FlowAndPlaceFloat(f[2], None);
            assert_eq!(s.ClearFloats(0, UsedClear::Left, None), (4 * PX, BlockReflowState_ClearFloatsResult::BCoordAdvanced));
            assert_eq!(s.ClearFloats(0, UsedClear::Right, None), (6 * PX, BlockReflowState_ClearFloatsResult::BCoordAdvanced));
            assert_eq!(s.ClearFloats(0, UsedClear::Both, None), (6 * PX, BlockReflowState_ClearFloatsResult::BCoordAdvanced));
            assert_eq!(s.ClearFloats(7 * PX, UsedClear::Both, None), (7 * PX, BlockReflowState_ClearFloatsResult::BCoordNoChange));
            assert_eq!(s.ClearFloats(0, UsedClear::None, None), (0, BlockReflowState_ClearFloatsResult::BCoordNoChange));
        });
    }

    #[test]
    fn advance_to_next_band_moves_by_the_band_size() {
        with_state("", &[], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            let band = LogicalRect::new(wm, 0, 0, 10 * PX, 7 * PX);
            let mut b = 3 * PX;
            assert!(s.AdvanceToNextBand(&band, &mut b));
            assert_eq!(b, 10 * PX);
        });
        // With a constrained available block-size an empty band means "pushed to the next page".
        with_state("", &[], 50 * PX, |s, f| {
            let wm = f[0].GetWritingMode();
            let band = LogicalRect::new(wm, 0, 0, 10 * PX, 0);
            let mut b = 3 * PX;
            assert!(!s.AdvanceToNextBand(&band, &mut b));
            assert_eq!(b, 3 * PX);
        });
    }

    #[test]
    fn available_space_for_float_is_the_rest_of_the_content_area() {
        with_state("padding-top:10px", &[], 100 * PX, |s, f| {
            let wm = f[0].GetWritingMode();
            assert_eq!(s.ComputeAvailableSizeForFloat(), LogicalSize::new(wm, s.ContentISize(), 90 * PX));
        });
        with_state("", &[], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            assert_eq!(s.ComputeAvailableSizeForFloat(), LogicalSize::new(wm, 10 * PX, AUTO));
        });
    }

    #[test]
    fn block_that_flows_around_floats_gets_the_whole_content_area() {
        // A block that does not establish a BFC lets its content flow around the float itself:
        // it gets the full content inline-size (float-edge: content-box).
        let a = left_float(4, 4);
        let child = "margin:0".to_string();
        with_state("", &[&a, &child], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            s.FlowAndPlaceFloat(f[1], None);
            f[2].RemoveStateBits(NS_FRAME_OUT_OF_FLOW);
            let space = s.GetFloatAvailableSpace__WritingMode(wm);
            let r = s.ComputeBlockAvailSpace(f[2], &space, false);
            assert_eq!(r.IStart(wm), 0);
            assert_eq!(r.ISize(wm), 10 * PX);
            assert_eq!(r.BStart(wm), 0);
            assert_eq!(r.BSize(wm), AUTO);
        });
    }

    #[test]
    fn float_avoiding_offsets() {
        // A block that avoids floats is offset by the float intrusion, less its own margin.
        let a = left_float(4, 4);
        let child = "margin-left:1px".to_string();
        with_state("", &[&a, &child], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            s.FlowAndPlaceFloat(f[1], None);
            let space = s.GetFloatAvailableSpace__WritingMode(wm);
            let (mut i_start, mut i_end) = (0, 0);
            s.ComputeFloatAvoidingOffsets(f[2], &space.mRect, &mut i_start, &mut i_end);
            assert_eq!(i_start, 4 * PX - 1 * PX);
            assert_eq!(i_end, 0);
        });
        // No floats around: no offsets (and the margins are not even computed).
        with_state("", &[&"margin-left:1px".to_string()], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            let space = s.GetFloatAvailableSpace__WritingMode(wm);
            let (mut i_start, mut i_end) = (9, 9);
            s.ComputeFloatAvoidingOffsets(f[1], &space.mRect, &mut i_start, &mut i_end);
            assert_eq!((i_start, i_end), (0, 0));
        });
    }

    // A real line layout over a one-line list of the block: `ll` has begun a line as wide as the
    // block, at the block-start of the content. (Run `body`, then end the line.)
    fn with_line_layout(s: &mut BlockReflowState, f: &[nsIFrame], body: impl FnOnce(&mut BlockReflowState, &mut nsLineLayout)) {
        let wm = f[0].GetWritingMode();
        let block = s.mBlock;
        block.mLines().push_back(inline_line(*f.last().unwrap()));
        s.mCurrentLine = block.LinesBegin();
        let it = block.LinesBegin();
        let fm: *mut nsFloatManager = s.FloatManager();
        let mut ll = nsLineLayout::new(&nsPresContext::Current(), fm, s.mReflowInput, Some(&it), std::ptr::null_mut());
        ll.Init(s as *mut BlockReflowState, 0, 0);
        ll.BeginLineReflow(0, s.mBCoord, 10 * PX, AUTO, false, false, wm, nsSize::new(10 * PX, 0), 0);
        body(s, &mut ll);
        ll.EndLineReflow();
    }

    #[test]
    fn add_float_places_immediately_on_an_empty_line() {
        let a = left_float(3, 2);
        with_state("", &[&a], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            with_line_layout(s, f, |s, ll| {
                assert!(ll.LineIsEmpty());
                assert!(s.AddFloat(ll, f[1], 10 * PX));
                assert_eq!(f[1].GetPosition(), nsPoint::new(0, 0));
                assert_eq!(s.mCurrentLineFloats, vec![f[1]]);
                assert!(s.mBelowCurrentLineFloats.is_empty());
                // The line layout was told about the new band next to the float: the line now
                // starts after the 3px float, and is impacted by floats.
                assert_eq!(ll.GetCurrentICoord(), 3 * PX);
                assert!(ll.LineIsBreakable());
                // The coordinate system is restored.
                let (mut i, mut b) = (1, 1);
                s.FloatManager().GetTranslation(&mut i, &mut b);
                assert_eq!((i, b), (s.mFloatManagerI, s.mFloatManagerB));
            });
        });
    }

    #[test]
    fn add_float_defers_a_float_that_does_not_fit_the_nonempty_line() {
        let a = left_float(6, 2);
        let b = left_float(1, 1);
        with_state("", &[&a, &b, &"".to_string()], AUTO, |s, f| {
            // f[3] is a text frame, 1px wide, so that the line is not empty
            f[3].RemoveStateBits(NS_FRAME_OUT_OF_FLOW);
            f[3].SetType(LayoutFrameType::Text);
            f[3].SetTextFrameStub(TextFrameStub { mISize: PX, mBSize: PX, mAscent: PX, ..Default::default() });
            with_line_layout(s, f, |s, ll| {
                let mut status = nsReflowStatus::new();
                let mut pushed = false;
                ll.ReflowFrame(f[3], &mut status, None, &mut pushed);
                assert!(!ll.LineIsEmpty());
                // Only 5px left on the line: the 6px float waits, and so does every later float.
                assert!(s.AddFloat(ll, f[1], 5 * PX));
                assert_eq!(s.mBelowCurrentLineFloats, vec![f[1]]);
                assert!(s.mCurrentLineFloats.is_empty());
                assert!(s.AddFloat(ll, f[2], 5 * PX));
                assert_eq!(s.mBelowCurrentLineFloats, vec![f[1], f[2]]);
                assert!(!s.FloatManager().HasAnyFloats());
            });
        });
    }

    #[test]
    fn place_below_current_line_floats_places_them_on_the_line() {
        let a = left_float(6, 2);
        let b = left_float(6, 2);
        with_state("", &[&a, &b], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            s.mBelowCurrentLineFloats = vec![f[1], f[2]];
            let mut line = unsafe { Box::from_raw(inline_line(*f.last().unwrap())) };
            s.PlaceBelowCurrentLineFloats(&mut line);
            assert!(s.mBelowCurrentLineFloats.is_empty());
            assert_eq!(*line.Floats(), vec![f[1], f[2]]);
            assert!(!line.HadFloatPushed());
            assert_eq!(f[2].GetPosition(), nsPoint::new(0, 2 * PX));
        });
    }

    #[test]
    fn reconstruct_margin_before_finds_the_previous_block_margin() {
        with_state("", &[&"margin:0".to_string()], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            let block = s.mBlock;
            // lines: a block line that carried out 9px, then an empty inline line
            let line = block_line(f[1]);
            let mut carried = CollapsingMargin::default();
            carried.Include__nscoord(9 * PX);
            unsafe { (*line).SetCarriedOutBEndMargin(carried) };
            block.mLines().push_back(line);
            block.mLines().push_back(inline_line(*f.last().unwrap()));
            f.last().unwrap().SetIsEmpty(true);
            let end = block.LinesEnd();
            s.ReconstructMarginBefore(end);
            assert_eq!(s.mPrevBEndMargin.Get(), 9 * PX);
            // Starting at the second line, the block line right before it is found at once.
            s.mPrevBEndMargin.Zero();
            s.ReconstructMarginBefore(block.LinesBegin().next_());
            assert_eq!(s.mPrevBEndMargin.Get(), 9 * PX);
        });
    }

    #[test]
    fn recover_state_makes_the_line_current_and_replays_its_floats() {
        let a = left_float(3, 2);
        with_state("", &[&a], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            let block = s.mBlock;
            let line = inline_line(*f.last().unwrap());
            unsafe { (*line).AppendFloats(vec![f[1]]) };
            // Give the float a region as FlowAndPlaceFloat would have stored.
            s.FlowAndPlaceFloat(f[1], None);
            block.mLines().push_back(line);
            // A fresh float manager state: replay into it, sliding the float down by 5px.
            let before = f[1].GetPosition();
            s.FloatManager().PopState(&SavedState::new());
            s.RecoverStateFrom(block.LinesBegin(), 5 * PX);
            assert!(s.mCurrentLine == block.LinesBegin());
            assert_eq!(f[1].GetPosition(), nsPoint::new(before.x, before.y + 5 * PX));
            assert!(s.FloatManager().HasAnyFloats());
        });
    }

    #[test]
    fn advance_to_next_line_counts_only_nonempty_lines() {
        with_state("", &[], AUTO, |s, _| {
            s.AdvanceToNextLine();
            assert_eq!(s.mLineNumber, 1);
            s.mFlags.mIsLineLayoutEmpty = true;
            s.AdvanceToNextLine();
            assert_eq!(s.mLineNumber, 1);
            assert!(!s.mFlags.mIsLineLayoutEmpty);
        });
    }

    #[test]
    fn float_available_space_for_bsize_looks_at_the_given_band() {
        let a = left_float(3, 2);
        with_state("", &[&a], AUTO, |s, f| {
            let wm = f[0].GetWritingMode();
            s.FlowAndPlaceFloat(f[1], None);
            // A line 5px tall starting at 0 is cut by the 2px float: the width within the height
            // is what is free throughout it.
            let r = s.GetFloatAvailableSpaceForBSize(wm, 0, 5 * PX, None);
            assert!(r.HasFloats());
            assert_eq!(r.mRect.IStart(wm), 3 * PX);
            assert_eq!(r.mRect.ISize(wm), 7 * PX);
        });
    }
}
