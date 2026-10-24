/* class that manages rules for positioning floats */
// Translated from layout/generic/nsFloatManager.h and nsFloatManager.cpp (GECKO_REV).
use crate::*;
use std::collections::HashSet;

MOZ_ENUM_CLASS_BITS! {
    // Gecko: layout/generic/nsFloatManager.h nsFlowAreaRectFlags
    pub struct nsFlowAreaRectFlags: u32 {
        NoFlags = 0,
        HasFloats = 1 << 0,
        MayWiden = 1 << 1,
        ISizeIsActuallyNegative = 1 << 2,
    }
}

/**
 * The available space for content not occupied by floats is divided
 * into a sequence of rectangles in the block direction.  However, we
 * need to know not only the rectangle, but also whether it was reduced
 * (from the content rectangle) by floats that actually intruded into
 * the content rectangle. If it has been reduced by floats, then we also
 * track whether the flow area might widen as the floats narrow in the
 * block direction.
 */
// Gecko: layout/generic/nsFloatManager.h nsFlowAreaRect
#[derive(Clone, Copy)]
pub struct nsFlowAreaRect {
    pub mRect: LogicalRect,

    pub mAreaFlags: nsFlowAreaRectFlags,
}

impl nsFlowAreaRect {
    pub fn new(aWritingMode: WritingMode, aICoord: nscoord, aBCoord: nscoord, aISize: nscoord, aBSize: nscoord, aAreaFlags: nsFlowAreaRectFlags) -> nsFlowAreaRect {
        nsFlowAreaRect { mRect: LogicalRect::new(aWritingMode, aICoord, aBCoord, aISize, aBSize), mAreaFlags: aAreaFlags }
    }

    pub fn HasFloats(&self) -> bool {
        (self.mAreaFlags & nsFlowAreaRectFlags::HasFloats).to_bool()
    }
    pub fn MayWiden(&self) -> bool {
        (self.mAreaFlags & nsFlowAreaRectFlags::MayWiden).to_bool()
    }
    pub fn ISizeIsActuallyNegative(&self) -> bool {
        (self.mAreaFlags & nsFlowAreaRectFlags::ISizeIsActuallyNegative).to_bool()
    }
}

pub const NS_FLOAT_MANAGER_CACHE_SIZE: usize = 64;

/**
 * nsFloatManager is responsible for implementing CSS's rules for
 * positioning floats. An nsFloatManager object is created during reflow for
 * any block with NS_BLOCK_BFC. During reflow, the float manager for the nearest
 * such ancestor block is found in ReflowInput::mFloatManager.
 *
 * According to the line-relative mappings in CSS Writing Modes spec [1],
 * line-right and line-left are calculated with respect to the writing mode
 * of the containing block of the floats. All the writing modes passed to
 * nsFloatManager methods should be the containing block's writing mode.
 *
 * However, according to the abstract-to-physical mappings table [2], the
 * 'direction' property of the containing block doesn't affect the
 * interpretation of line-right and line-left. We actually implement this by
 * passing in the writing mode of the block formatting context (BFC), i.e.
 * the of BlockReflowState's writing mode.
 *
 * nsFloatManager uses a special logical coordinate space with inline
 * coordinates on the line-axis and block coordinates on the block-axis
 * based on the writing mode of the block formatting context. All the
 * physical types like nsRect, nsPoint, etc. use this coordinate space. See
 * FloatInfo::mRect for an example.
 *
 * [1] https://drafts.csswg.org/css-writing-modes/#line-mappings
 * [2] https://drafts.csswg.org/css-writing-modes/#logical-to-physical
 */
// Gecko: layout/generic/nsFloatManager.h nsFloatManager
pub struct nsFloatManager {
    #[cfg(debug_assertions)]
    // Store the writing mode from the block frame which establishes the block
    // formatting context (BFC) when the nsFloatManager is created.
    mWritingMode: WritingMode,

    // Translation from local to global coordinate space.
    mLineLeft: nscoord,
    mBlockStart: nscoord,
    // We use 11 here in order to fill up the jemalloc allocatoed chunk nicely,
    // see https://bugzilla.mozilla.org/show_bug.cgi?id=1362876#c6.
    // (AutoTArray<FloatInfo, 11> is a Vec here.)
    mFloats: Vec<FloatInfo>,
    mFloatDamage: nsIntervalSet,

    // Did we try to place a float that could not fit at all and had to be
    // pushed to the next page/column?  If so, we can't place any more
    // floats in this page/column because of the rule that the top of a
    // float cannot be above the top of an earlier float.  And we also
    // need to apply this information to 'clear', and thus need to
    // separate left and right floats.
    mPushedLeftFloatPastBreak: bool,
    mPushedRightFloatPastBreak: bool,

    // Did we split a float, with part of it needing to be pushed to the
    // next page/column.  This means that any 'clear' needs to continue to
    // the next page/column.
    mSplitLeftFloatAcrossBreak: bool,
    mSplitRightFloatAcrossBreak: bool,
    // NOT PORTED: sCachedFloatManagerCount, sCachedFloatManagers (a cache of freed
    // nsFloatManager objects; Rust allocates normally).
}

// Structure that stores the current state of a float manager for
// Save/Restore purposes.
// Gecko: layout/generic/nsFloatManager.h nsFloatManager::SavedState
#[derive(Clone, Copy)]
pub struct SavedState {
    mFloatInfoCount: u32,
    mLineLeft: nscoord,
    mBlockStart: nscoord,
    mPushedLeftFloatPastBreak: bool,
    mPushedRightFloatPastBreak: bool,
    mSplitLeftFloatAcrossBreak: bool,
    mSplitRightFloatAcrossBreak: bool,
}

impl SavedState {
    pub fn new() -> SavedState {
        SavedState {
            mFloatInfoCount: 0,
            mLineLeft: 0,
            mBlockStart: 0,
            mPushedLeftFloatPastBreak: false,
            mPushedRightFloatPastBreak: false,
            mSplitLeftFloatAcrossBreak: false,
            mSplitRightFloatAcrossBreak: false,
        }
    }
}

impl Default for SavedState {
    fn default() -> Self {
        SavedState::new()
    }
}

/**
 * Get information about the area available to content that flows
 * around floats.  Two different types of space can be requested:
 *   BandFromPoint: returns the band containing block-dir coordinate
 *     |aBCoord| (though actually with the top truncated to begin at
 *     aBCoord), but up to at most |aBSize| (which may be nscoord_MAX).
 *     This will return the tallest rectangle whose block start is
 *     |aBCoord| and in which there are no changes in what floats are
 *     on the sides of that rectangle, but will limit the block size
 *     of the rectangle to |aBSize|.  The inline start and end edges
 *     of the rectangle give the area available for line boxes in that
 *     space. The inline size of this resulting rectangle will not be
 *     negative.
 *   WidthWithinHeight: This returns a rectangle whose block start
 *     is aBCoord and whose block size is exactly aBSize.  Its inline
 *     start and end edges give the corresponding edges of the space
 *     that can be used for line boxes *throughout* that space.  (It
 *     is possible that more inline space could be used in part of the
 *     space if a float begins or ends in it.)  The inline size of the
 *     resulting rectangle can be negative.
 */
// Gecko: layout/generic/nsFloatManager.h nsFloatManager::BandInfoType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BandInfoType {
    BandFromPoint,
    WidthWithinHeight,
}

// Gecko: layout/generic/nsFloatManager.h nsFloatManager::ShapeType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeType {
    Margin,
    ShapeOutside,
}

// NOT PORTED: class nsFloatManager::RoundedBoxShapeInfo, EllipseShapeInfo, PolygonShapeInfo,
// ImageShapeInfo (forward declarations in the header; definitions in nsFloatManager.cpp, see below).

// Gecko: layout/generic/nsFloatManager.h nsFloatManager::FloatInfo
pub struct FloatInfo {
    pub mFrame: nsIFrame,
    // The lowest block-ends of left/right floats up to and including
    // this one.
    pub mLeftBEnd: nscoord,
    pub mRightBEnd: nscoord,

    // NB! This is really a logical rect in a writing mode suitable for
    // placing floats, which is not necessarily the actual writing mode
    // either of the block which created the float manager or the block
    // that is calling the float manager. The inline coordinates are in
    // the line-relative axis of the float manager and its block
    // coordinates are in the float manager's block direction.
    pub mRect: nsRect,
    // Pointer to a concrete subclass of ShapeInfo or null, which means that
    // there is no shape-outside.
    pub mShapeInfo: Option<Box<dyn ShapeInfo>>,
}

impl FloatInfo {
    pub fn LineLeft__(&self) -> nscoord {
        self.mRect.x
    }
    pub fn LineRight__(&self) -> nscoord {
        self.mRect.XMost()
    }
    pub fn ISize(&self) -> nscoord {
        self.mRect.width
    }
    pub fn BStart__(&self) -> nscoord {
        self.mRect.y
    }
    pub fn BEnd__(&self) -> nscoord {
        self.mRect.YMost()
    }
    pub fn BSize(&self) -> nscoord {
        self.mRect.height
    }
    pub fn IsEmpty__(&self) -> bool {
        self.mRect.IsEmpty()
    }
}

// NOT PORTED: FloatInfo(FloatInfo&&) and ~FloatInfo() under NS_BUILD_REFCNT_LOGGING.

/**
 * A helper class to manage maintenance of the float manager during
 * nsBlockFrame::Reflow. It automatically restores the old float
 * manager in the reflow input when the object goes out of scope.
 */
// Gecko: layout/generic/nsFloatManager.h nsAutoFloatManager
// ADAPTER: `ReflowInput& mReflowInput` is a raw pointer. In C++ the helper and the reflow input are
// two names for objects that nsBlockFrame::Reflow keeps using side by side (the helper sits on the
// stack next to the `const ReflowInput&` that the caller cast the const away from); a Rust `&mut`
// would forbid using the reflow input while the helper lives. The reflow input must outlive the
// helper, which is how it is used in Gecko (both are locals of Reflow(), the reflow input declared first).
pub struct nsAutoFloatManager<'a> {
    mReflowInput: *mut ReflowInput<'a>,
    mNew: Option<Box<nsFloatManager>>,

    // A non-owning pointer, which points to the object owned by
    // nsAutoFloatManager::mNew.
    mOld: *mut nsFloatManager,
}

impl<'a> nsAutoFloatManager<'a> {
    // Gecko: layout/generic/nsFloatManager.h nsAutoFloatManager::nsAutoFloatManager
    pub fn new(aReflowInput: &mut ReflowInput<'a>) -> nsAutoFloatManager<'a> {
        nsAutoFloatManager { mReflowInput: aReflowInput as *mut ReflowInput<'a>, mNew: None, mOld: std::ptr::null_mut() }
    }

    /**
     * Create a new float manager for the specified frame. This will
     * `remember' the old float manager, and install the new float
     * manager in the reflow input.
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsAutoFloatManager::CreateFloatManager
    pub fn CreateFloatManager(&mut self, aPresContext: &nsPresContext) {
        MOZ_ASSERT!(self.mNew.is_none(), "Redundant call to CreateFloatManager!");

        // Create a new float manager and install it in the reflow
        // input. `Remember' the old float manager so we can restore it
        // later.
        // NOT PORTED: the PresShell argument (see nsFloatManager::new).
        let reflowInput = unsafe { &mut *self.mReflowInput };
        self.mNew = Some(Box::new(nsFloatManager::new(reflowInput.GetWritingMode())));

        // NOT PORTED: `#ifdef DEBUG` printing under nsBlockFrame::gNoisyFloatManager.

        // Set the float manager in the existing reflow input.
        self.mOld = reflowInput.mFloatManager;
        reflowInput.mFloatManager = &mut **self.mNew.as_mut().unwrap() as *mut nsFloatManager;
    }
}

// Gecko: layout/generic/nsFloatManager.cpp nsAutoFloatManager::~nsAutoFloatManager
impl<'a> Drop for nsAutoFloatManager<'a> {
    fn drop(&mut self) {
        // Restore the old float manager in the reflow input if necessary.
        if self.mNew.is_some() {
            // NOT PORTED: `#ifdef DEBUG` printing under nsBlockFrame::gNoisyFloatManager.

            unsafe { (*self.mReflowInput).mFloatManager = self.mOld };
        }
    }
}

// Gecko: layout/generic/nsFloatManager.cpp (nsFloatManager)
macro_rules! CHECK_BLOCK_AND_LINE_DIR {
    ($self:expr, $aWM:expr) => {
        #[cfg(debug_assertions)]
        NS_ASSERTION!(
            $aWM.GetBlockDir() == $self.mWritingMode.GetBlockDir() && $aWM.IsLineInverted() == $self.mWritingMode.IsLineInverted(),
            "incompatible writing modes"
        );
    };
}

impl nsFloatManager {
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::nsFloatManager
    // NOT PORTED: the PresShell argument (it only provides an arena for mFloatDamage).
    pub fn new(aWM: WritingMode) -> nsFloatManager {
        nsFloatManager {
            #[cfg(debug_assertions)]
            mWritingMode: aWM,
            mLineLeft: 0,
            mBlockStart: 0,
            mFloats: Vec::new(),
            mFloatDamage: nsIntervalSet::new(),
            mPushedLeftFloatPastBreak: false,
            mPushedRightFloatPastBreak: false,
            mSplitLeftFloatAcrossBreak: false,
            mSplitRightFloatAcrossBreak: false,
        }
    }

    // NOT PORTED: ~nsFloatManager, operator new, operator delete and Shutdown
    // (nsFloatManager.cpp lines 48-101): the cache of freed float managers.

    /**
     * Translate the current origin by the specified offsets. This
     * creates a new local coordinate space relative to the current
     * coordinate space.
     */
    // Gecko: layout/generic/nsFloatManager.h nsFloatManager::Translate
    pub fn Translate(&mut self, aLineLeft: nscoord, aBlockStart: nscoord) {
        self.mLineLeft += aLineLeft;
        self.mBlockStart += aBlockStart;
    }

    /**
     * Returns the current translation from local coordinate space to
     * world coordinate space. This represents the accumulated calls to
     * Translate().
     */
    // Gecko: layout/generic/nsFloatManager.h nsFloatManager::GetTranslation
    pub fn GetTranslation(&self, aLineLeft: &mut nscoord, aBlockStart: &mut nscoord) {
        *aLineLeft = self.mLineLeft;
        *aBlockStart = self.mBlockStart;
    }

    /**
     * Get information about the area available to content that flows
     * around floats.  Two different types of space can be requested
     * (BandInfoType, above).
     *
     * ShapeType can be used to request two different types of flow areas.
     * (This is the float area defined in CSS Shapes Module Level 1 §1.4):
     *    Margin: uses the float element's margin-box to request the flow area.
     *    ShapeOutside: uses the float element's shape-outside value to request
     *      the float area.
     *
     * @param aBCoord [in] block-dir coordinate for block start of available space
     *          desired, which are positioned relative to the current translation.
     * @param aBSize [in] see above
     * @param aContentArea [in] an nsRect representing the content area
     * @param aState [in] If null, use the current state, otherwise, do
     *                    computation based only on floats present in the given
     *                    saved state.
     * @return An nsFlowAreaRect whose:
     *           mRect is the resulting rectangle for line boxes.  It will not
     *             extend beyond aContentArea's inline bounds, but may be
     *             narrower when floats are present.
     *           mHasFloats is whether there are floats at the sides of the
     *             return value including those that do not reduce the line box
     *             inline size at all (because they are entirely in the margins)
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::GetFlowArea
    pub fn GetFlowArea(&self, aCBWM: WritingMode, aWM: WritingMode, aBCoord: nscoord, aBSize: nscoord, aBandInfoType: BandInfoType, aShapeType: ShapeType, aContentArea: LogicalRect, aState: Option<&SavedState>, aContainerSize: nsSize) -> nsFlowAreaRect {
        CHECK_BLOCK_AND_LINE_DIR!(self, aWM);
        NS_ASSERTION!(aBSize >= 0, "unexpected max block size");
        NS_ASSERTION!(aContentArea.ISize(aWM) >= 0, "unexpected content area inline size");

        let mut blockStart: nscoord = aBCoord + self.mBlockStart;
        if blockStart < nscoord_MIN {
            NS_WARNING!("bad value");
            blockStart = nscoord_MIN;
        }

        // Determine the last float that we should consider.
        let floatCount: u32;
        if let Some(aState) = aState {
            // Use the provided state.
            floatCount = aState.mFloatInfoCount;
            MOZ_ASSERT!(floatCount as usize <= self.mFloats.len(), "bad state");
        } else {
            // Use our current state.
            floatCount = self.mFloats.len() as u32;
        }

        // If there are no floats at all, or we're below the last one, return
        // quickly.
        if floatCount == 0 || (self.mFloats[(floatCount - 1) as usize].mLeftBEnd <= blockStart && self.mFloats[(floatCount - 1) as usize].mRightBEnd <= blockStart) {
            return nsFlowAreaRect::new(aWM, aContentArea.IStart(aWM), aBCoord, aContentArea.ISize(aWM), aBSize, nsFlowAreaRectFlags::NoFlags);
        }

        let mut blockEnd: nscoord;
        if aBSize == nscoord_MAX {
            // This warning (and the two below) are possible to hit on pages
            // with really large objects.
            NS_WARNING_ASSERTION!(aBandInfoType == BandInfoType::BandFromPoint, "bad height");
            blockEnd = nscoord_MAX;
        } else {
            blockEnd = blockStart + aBSize;
            if blockEnd < blockStart || blockEnd > nscoord_MAX {
                NS_WARNING!("bad value");
                blockEnd = nscoord_MAX;
            }
        }
        let mut lineLeft: nscoord = self.mLineLeft + aContentArea.LineLeft(aWM, aContainerSize);
        let mut lineRight: nscoord = self.mLineLeft + aContentArea.LineRight(aWM, aContainerSize);
        if lineRight < lineLeft {
            NS_WARNING!("bad value");
            lineRight = lineLeft;
        }

        // Walk backwards through the floats until we either hit the front of
        // the list or we're above |blockStart|.
        let mut haveFloats = false;
        let mut mayWiden = false;
        let mut i = floatCount;
        while i > 0 {
            let fi: &FloatInfo = &self.mFloats[(i - 1) as usize];
            if fi.mLeftBEnd <= blockStart && fi.mRightBEnd <= blockStart {
                // There aren't any more floats that could intersect this band.
                break;
            }
            if fi.IsEmpty__ShapeType(aShapeType) {
                // Ignore empty float areas.
                // https://drafts.csswg.org/css-shapes/#relation-to-box-model-and-float-behavior
                i -= 1;
                continue;
            }

            let floatBStart: nscoord = fi.BStart__ShapeType(aShapeType);
            let floatBEnd: nscoord = fi.BEnd__ShapeType(aShapeType);
            if blockStart < floatBStart && aBandInfoType == BandInfoType::BandFromPoint {
                // This float is below our band.  Shrink our band's height if needed.
                if floatBStart < blockEnd {
                    blockEnd = floatBStart;
                }
            }
            // If blockStart == blockEnd (which happens only with WidthWithinHeight),
            // we include floats that begin at our 0-height vertical area.  We
            // need to do this to satisfy the invariant that a
            // WidthWithinHeight call is at least as narrow on both sides as a
            // BandFromPoint call beginning at its blockStart.
            else if blockStart < floatBEnd && (floatBStart < blockEnd || (floatBStart == blockEnd && blockStart == blockEnd)) {
                // This float is in our band.

                // Shrink our band's width if needed.
                let floatStyle: UsedFloat = fi.mFrame.StyleDisplay().UsedFloat(aCBWM);

                // When aBandInfoType is BandFromPoint, we're only intended to
                // consider a point along the y axis rather than a band.
                let bandBlockEnd: nscoord = if aBandInfoType == BandInfoType::BandFromPoint { blockStart } else { blockEnd };
                if floatStyle == UsedFloat::Left {
                    // A left float
                    let lineRightEdge: nscoord = fi.LineRight__ShapeType_nscoord_nscoord(aShapeType, blockStart, bandBlockEnd);
                    if lineRightEdge > lineLeft {
                        lineLeft = lineRightEdge;
                        // Only set haveFloats to true if the float is inside our
                        // containing block.  This matches the spec for what some
                        // callers want and disagrees for other callers, so we should
                        // probably provide better information at some point.
                        haveFloats = true;

                        // Our area may widen in the block direction if this float may
                        // narrow in the block direction.
                        mayWiden = mayWiden || fi.MayNarrowInBlockDirection(aShapeType);
                    }
                } else {
                    // A right float
                    let lineLeftEdge: nscoord = fi.LineLeft__ShapeType_nscoord_nscoord(aShapeType, blockStart, bandBlockEnd);
                    if lineLeftEdge < lineRight {
                        lineRight = lineLeftEdge;
                        // See above.
                        haveFloats = true;
                        mayWiden = mayWiden || fi.MayNarrowInBlockDirection(aShapeType);
                    }
                }

                // Shrink our band's height if needed.
                if floatBEnd < blockEnd && aBandInfoType == BandInfoType::BandFromPoint {
                    blockEnd = floatBEnd;
                }
            }
            i -= 1;
        }

        let blockSize: nscoord = if blockEnd == nscoord_MAX { nscoord_MAX } else { blockEnd - blockStart };
        // convert back from LineLeft/Right to IStart
        let inlineStart: nscoord = if aWM.IsBidiLTR() { lineLeft - self.mLineLeft } else { self.mLineLeft - lineRight + LogicalSize::new__WritingMode_nsSize(aWM, aContainerSize).ISize(aWM) };

        let mut flags: nsFlowAreaRectFlags = (if haveFloats { nsFlowAreaRectFlags::HasFloats } else { nsFlowAreaRectFlags::NoFlags }) | (if mayWiden { nsFlowAreaRectFlags::MayWiden } else { nsFlowAreaRectFlags::NoFlags });
        // Some callers clamp the inline size of nsFlowAreaRect to be nonnegative
        // "for compatibility with nsSpaceManager". So, we set a flag here to record
        // the fact that the ISize is actually negative, so that downstream code can
        // realize that there's no place here where we could put a float-avoiding
        // block (even one with ISize of 0).
        if lineRight - lineLeft < 0 {
            flags |= nsFlowAreaRectFlags::ISizeIsActuallyNegative;
        }

        nsFlowAreaRect::new(aWM, inlineStart, blockStart - self.mBlockStart, lineRight - lineLeft, blockSize, flags)
    }

    /**
     * Add a float that comes after all floats previously added.  Its
     * block start must be even with or below the top of all previous
     * floats.
     *
     * aMarginRect is relative to the current translation.  The caller
     * must ensure aMarginRect.height >= 0 and aMarginRect.width >= 0.
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::AddFloat
    pub fn AddFloat(&mut self, aFloatFrame: nsIFrame, aMarginRect: LogicalRect, aWM: WritingMode, aContainerSize: nsSize) {
        CHECK_BLOCK_AND_LINE_DIR!(self, aWM);
        NS_ASSERTION!(aMarginRect.ISize(aWM) >= 0, "negative inline size!");
        NS_ASSERTION!(aMarginRect.BSize(aWM) >= 0, "negative block size!");

        let mut info = FloatInfo::new(aFloatFrame, self.mLineLeft, self.mBlockStart, aMarginRect, aWM, aContainerSize);

        // Set mLeftBEnd and mRightBEnd.
        if self.HasAnyFloats() {
            let tail: &FloatInfo = &self.mFloats[self.mFloats.len() - 1];
            info.mLeftBEnd = tail.mLeftBEnd;
            info.mRightBEnd = tail.mRightBEnd;
        } else {
            info.mLeftBEnd = nscoord_MIN;
            info.mRightBEnd = nscoord_MIN;
        }
        let cbWM: WritingMode = aFloatFrame.GetParent().unwrap().GetWritingMode();
        let floatStyle: UsedFloat = aFloatFrame.StyleDisplay().UsedFloat(cbWM);
        MOZ_ASSERT!(floatStyle == UsedFloat::Left || floatStyle == UsedFloat::Right, "Unexpected float style!");
        let thisBEnd: nscoord = info.BEnd__();
        let sideBEnd: &mut nscoord = if floatStyle == UsedFloat::Left { &mut info.mLeftBEnd } else { &mut info.mRightBEnd };
        if thisBEnd > *sideBEnd {
            *sideBEnd = thisBEnd;
        }

        self.mFloats.push(info);
    }

    /**
     * Notify that we tried to place a float that could not fit at all and
     * had to be pushed to the next page/column?  (If so, we can't place
     * any more floats in this page/column because of the rule that the
     * top of a float cannot be above the top of an earlier float.  It
     * also means that any clear needs to continue to the next column.)
     */
    // Gecko: layout/generic/nsFloatManager.h nsFloatManager::SetPushedLeftFloatPastBreak
    pub fn SetPushedLeftFloatPastBreak(&mut self) {
        self.mPushedLeftFloatPastBreak = true;
    }
    // Gecko: layout/generic/nsFloatManager.h nsFloatManager::SetPushedRightFloatPastBreak
    pub fn SetPushedRightFloatPastBreak(&mut self) {
        self.mPushedRightFloatPastBreak = true;
    }

    /**
     * Notify that we split a float, with part of it needing to be pushed
     * to the next page/column.  (This means that any 'clear' needs to
     * continue to the next page/column.)
     */
    // Gecko: layout/generic/nsFloatManager.h nsFloatManager::SetSplitLeftFloatAcrossBreak
    pub fn SetSplitLeftFloatAcrossBreak(&mut self) {
        self.mSplitLeftFloatAcrossBreak = true;
    }
    // Gecko: layout/generic/nsFloatManager.h nsFloatManager::SetSplitRightFloatAcrossBreak
    pub fn SetSplitRightFloatAcrossBreak(&mut self) {
        self.mSplitRightFloatAcrossBreak = true;
    }

    /**
     * Remove the regions associated with this floating frame and its
     * next-sibling list.  Some of the frames may never have been added;
     * we just skip those. This is not fully general; it only works as
     * long as the N frames to be removed are the last N frames to have
     * been added; if there's a frame in the middle of them that should
     * not be removed, YOU LOSE.
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::RemoveTrailingRegions
    pub fn RemoveTrailingRegions(&mut self, aFrameList: Option<nsIFrame>) -> nsresult {
        if aFrameList.is_none() {
            return NS_OK;
        }
        // This could be a good bit simpler if we could guarantee that the
        // floats given were at the end of our list, so we could just search
        // for the head of aFrameList.  (But we can't;
        // layout/reftests/bugs/421710-1.html crashes.)
        let mut frameSet: HashSet<nsIFrame> = HashSet::with_capacity(1);

        let mut f = aFrameList;
        while let Some(frame) = f {
            frameSet.insert(frame);
            f = frame.GetNextSibling();
        }

        let mut newLength: usize = self.mFloats.len();
        while newLength > 0 {
            if !frameSet.contains(&self.mFloats[newLength - 1].mFrame) {
                break;
            }
            newLength -= 1;
        }
        self.mFloats.truncate(newLength);

        #[cfg(debug_assertions)]
        {
            for i in 0..self.mFloats.len() {
                NS_ASSERTION!(!frameSet.contains(&self.mFloats[i].mFrame), "Frame region deletion was requested but we couldn't delete it");
            }
        }

        NS_OK
    }

    // Gecko: layout/generic/nsFloatManager.h nsFloatManager::HasAnyFloats
    pub fn HasAnyFloats(&self) -> bool {
        !self.mFloats.is_empty()
    }

    /**
     * Methods for dealing with the propagation of float damage during
     * reflow.
     */
    // Gecko: layout/generic/nsFloatManager.h nsFloatManager::HasFloatDamage
    pub fn HasFloatDamage(&self) -> bool {
        !self.mFloatDamage.IsEmpty()
    }

    // Gecko: layout/generic/nsFloatManager.h nsFloatManager::IncludeInDamage
    pub fn IncludeInDamage(&mut self, aIntervalBegin: nscoord, aIntervalEnd: nscoord) {
        self.mFloatDamage.IncludeInterval(aIntervalBegin + self.mBlockStart, aIntervalEnd + self.mBlockStart);
    }

    // Gecko: layout/generic/nsFloatManager.h nsFloatManager::IntersectsDamage
    pub fn IntersectsDamage(&self, aIntervalBegin: nscoord, aIntervalEnd: nscoord) -> bool {
        self.mFloatDamage.Intersects(aIntervalBegin + self.mBlockStart, aIntervalEnd + self.mBlockStart)
    }

    /**
     * Saves the current state of the float manager into aState.
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::PushState
    pub fn PushState(&self, aState: &mut SavedState) {
        // This is a cheap push implementation, which
        // only saves the (x,y) and last frame in the mFrameInfoMap
        // which is enough info to get us back to where we should be
        // when pop is called.
        //
        // This push/pop mechanism is used to undo any
        // floats that were added during the unconstrained reflow
        // in nsBlockReflowContext::DoReflowBlock(). (See bug 96736)
        //
        // It should also be noted that the state for mFloatDamage is
        // intentionally not saved or restored in PushState() and PopState(),
        // since that could lead to bugs where damage is missed/dropped when
        // we move from position A to B (during the intermediate incremental
        // reflow mentioned above) and then from B to C during the subsequent
        // reflow. In the typical case A and C will be the same, but not always.
        // Allowing mFloatDamage to accumulate the damage incurred during both
        // reflows ensures that nothing gets missed.
        aState.mLineLeft = self.mLineLeft;
        aState.mBlockStart = self.mBlockStart;
        aState.mPushedLeftFloatPastBreak = self.mPushedLeftFloatPastBreak;
        aState.mPushedRightFloatPastBreak = self.mPushedRightFloatPastBreak;
        aState.mSplitLeftFloatAcrossBreak = self.mSplitLeftFloatAcrossBreak;
        aState.mSplitRightFloatAcrossBreak = self.mSplitRightFloatAcrossBreak;
        aState.mFloatInfoCount = self.mFloats.len() as u32;
    }

    /**
     * Restores the float manager to the saved state.
     *
     * These states must be managed using stack discipline. PopState can only
     * be used after PushState has been used to save the state, and it can only
     * be used once --- although it can be omitted; saved states can be ignored.
     * States must be popped in the reverse order they were pushed.  A
     * call to PopState invalidates any saved states Pushed after the
     * state passed to PopState was pushed.
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::PopState
    pub fn PopState(&mut self, aState: &SavedState) {
        self.mLineLeft = aState.mLineLeft;
        self.mBlockStart = aState.mBlockStart;
        self.mPushedLeftFloatPastBreak = aState.mPushedLeftFloatPastBreak;
        self.mPushedRightFloatPastBreak = aState.mPushedRightFloatPastBreak;
        self.mSplitLeftFloatAcrossBreak = aState.mSplitLeftFloatAcrossBreak;
        self.mSplitRightFloatAcrossBreak = aState.mSplitRightFloatAcrossBreak;

        NS_ASSERTION!(aState.mFloatInfoCount as usize <= self.mFloats.len(), "somebody misused PushState/PopState");
        self.mFloats.truncate(aState.mFloatInfoCount as usize);
    }

    /**
     * Get the block start of the last float placed into the float
     * manager, to enforce the rule that a float can't be above an earlier
     * float. Returns the minimum nscoord value if there are no floats.
     *
     * The result is relative to the current translation.
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::LowestFloatBStart
    pub fn LowestFloatBStart(&self) -> nscoord {
        if self.mPushedLeftFloatPastBreak || self.mPushedRightFloatPastBreak {
            return nscoord_MAX;
        }
        if !self.HasAnyFloats() {
            return nscoord_MIN;
        }
        self.mFloats[self.mFloats.len() - 1].BStart__() - self.mBlockStart
    }

    /**
     * Return the coordinate of the lowest float matching aClearType in
     * this float manager. Returns aBCoord if there are no matching
     * floats.
     *
     * Both aBCoord and the result are relative to the current translation.
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::ClearFloats
    pub fn ClearFloats(&self, aBCoord: nscoord, aClearType: UsedClear) -> nscoord {
        if !self.HasAnyFloats() {
            return aBCoord;
        }

        let mut blockEnd: nscoord = aBCoord + self.mBlockStart;

        let tail: &FloatInfo = &self.mFloats[self.mFloats.len() - 1];
        match aClearType {
            UsedClear::Both => {
                blockEnd = std::cmp::max(blockEnd, tail.mLeftBEnd);
                blockEnd = std::cmp::max(blockEnd, tail.mRightBEnd);
            }
            UsedClear::Left => {
                blockEnd = std::cmp::max(blockEnd, tail.mLeftBEnd);
            }
            UsedClear::Right => {
                blockEnd = std::cmp::max(blockEnd, tail.mRightBEnd);
            }
            UsedClear::None => {
                // Do nothing
            }
        }

        blockEnd -= self.mBlockStart;

        blockEnd
    }

    /**
     * Checks if clear would pass into the floats' BFC's next-in-flow,
     * i.e. whether floats affecting this clear have continuations.
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::ClearContinues
    pub fn ClearContinues(&self, aClearType: UsedClear) -> bool {
        ((self.mPushedLeftFloatPastBreak || self.mSplitLeftFloatAcrossBreak) && (aClearType == UsedClear::Both || aClearType == UsedClear::Left)) || ((self.mPushedRightFloatPastBreak || self.mSplitRightFloatAcrossBreak) && (aClearType == UsedClear::Both || aClearType == UsedClear::Right))
    }

    // Gecko: layout/generic/nsFloatManager.h nsFloatManager::AssertStateMatches
    pub fn AssertStateMatches(&self, aState: &SavedState) {
        NS_ASSERTION!(
            aState.mLineLeft == self.mLineLeft
                && aState.mBlockStart == self.mBlockStart
                && aState.mPushedLeftFloatPastBreak == self.mPushedLeftFloatPastBreak
                && aState.mPushedRightFloatPastBreak == self.mPushedRightFloatPastBreak
                && aState.mSplitLeftFloatAcrossBreak == self.mSplitLeftFloatAcrossBreak
                && aState.mSplitRightFloatAcrossBreak == self.mSplitRightFloatAcrossBreak
                && aState.mFloatInfoCount as usize == self.mFloats.len(),
            "float manager state should match saved state"
        );
    }

    /**
     * Dump the state of the float manager out to a file.
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::List
    // (DEBUG_FRAME_DUMP in C++)
    #[cfg(debug_assertions)]
    pub fn List(&self) -> nsresult {
        if !self.HasAnyFloats() {
            return NS_OK;
        }

        for i in 0..self.mFloats.len() {
            let fi: &FloatInfo = &self.mFloats[i];
            eprintln!("Float {}: frame={:?} rect={{{},{},{},{}}} BEnd={{l:{}, r:{}}}", i, fi.mFrame, fi.LineLeft__(), fi.BStart__(), fi.ISize(), fi.BSize(), fi.mLeftBEnd, fi.mRightBEnd);
        }
        NS_OK
    }

    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::CalculateRegionFor
    /**
     * Calculate the float region for this frame using aMargin and the
     * frame's mRect. The region includes the margins around the float,
     * but doesn't include the relative offsets.
     * Note that if the frame is or has a continuation, aMargin's top
     * and/or bottom must be zeroed by the caller.
     */
    pub fn CalculateRegionFor(aWM: WritingMode, aFloat: nsIFrame, aMargin: LogicalMargin, aContainerSize: nsSize) -> LogicalRect {
        // We consider relatively positioned frames at their original position.
        let mut region: LogicalRect = LogicalRect::new__WritingMode_nsRect_nsSize(aWM, nsRect::new__nsPoint_nsSize(aFloat.GetNormalPosition(), aFloat.GetSize()), aContainerSize);

        // Float region includes its margin
        region.Inflate__WritingMode_LogicalMargin(aWM, aMargin);

        // Don't store rectangles with negative margin-box width or height in
        // the float manager; it can't deal with them.
        if region.ISize(aWM) < 0 {
            // Preserve the right margin-edge for left floats and the left
            // margin-edge for right floats
            let display: nsStyleDisplay = aFloat.StyleDisplay();
            let cbWM: WritingMode = aFloat.GetParent().unwrap().GetWritingMode();
            let floatStyle: UsedFloat = display.UsedFloat(cbWM);
            if (UsedFloat::Left == floatStyle) == aWM.IsBidiLTR() {
                *region.IStart_mut(aWM) = region.IEnd(aWM);
            }
            *region.ISize_mut(aWM) = 0;
        }
        if region.BSize(aWM) < 0 {
            *region.BSize_mut(aWM) = 0;
        }
        region
    }
}

// Gecko: layout/generic/nsFloatManager.cpp FloatRegionProperty
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(FloatRegionProperty, nsMargin, 1);

impl nsFloatManager {
    /**
     * Get float region stored on the frame. (Defaults to mRect if it's
     * not there.) The float region is the area impacted by this float;
     * the coordinates are relative to the containing block frame.
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::GetRegionFor
    pub fn GetRegionFor(aWM: WritingMode, aFloat: nsIFrame, aContainerSize: nsSize) -> LogicalRect {
        let mut region: LogicalRect = LogicalRect::new__WritingMode_nsRect_nsSize(aWM, aFloat.GetRect(), aContainerSize);
        if let Some(storedRegion) = aFloat.GetProperty(FloatRegionProperty()) {
            region.Inflate__WritingMode_LogicalMargin(aWM, LogicalMargin::new__WritingMode_nsMargin(aWM, storedRegion));
        }
        region
    }

    /**
     * Store the float region on the frame. The region is stored
     * as a delta against the mRect, so repositioning the frame will
     * also reposition the float region.
     */
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::StoreRegionFor
    pub fn StoreRegionFor(aWM: WritingMode, aFloat: nsIFrame, aRegion: LogicalRect, aContainerSize: nsSize) {
        let region: nsRect = aRegion.GetPhysicalRect(aWM, aContainerSize);
        let rect: nsRect = aFloat.GetRect();
        if region.IsEqualEdges(rect) {
            aFloat.RemoveProperty(FloatRegionProperty());
        } else {
            // ADAPTER: Gecko gets a pointer to the stored margin and assigns through it.
            aFloat.SetProperty(FloatRegionProperty(), region - rect);
        }
    }
}

/////////////////////////////////////////////////////////////////////////////
// ShapeInfo is an abstract class for implementing all the shapes in CSS
// Shapes Module. A subclass needs to override all the methods to adjust
// the flow area with respect to its shape.
//
// Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::ShapeInfo
pub trait ShapeInfo {
    fn LineLeft(&self, aBStart: nscoord, aBEnd: nscoord) -> nscoord;
    fn LineRight(&self, aBStart: nscoord, aBEnd: nscoord) -> nscoord;
    fn BStart(&self) -> nscoord;
    fn BEnd(&self) -> nscoord;
    fn IsEmpty(&self) -> bool;

    // Does this shape possibly get inline narrower in the BStart() to BEnd()
    // span when proceeding in the block direction? This is false for unrounded
    // rectangles that span all the way to BEnd(), but could be true for other
    // shapes. Note that we don't care if the BEnd() falls short of the margin
    // rect -- the ShapeInfo can only affect float behavior in the span between
    // BStart() and BEnd().
    fn MayNarrowInBlockDirection(&self) -> bool;

    // Translate the current origin by the specified offsets.
    fn Translate(&mut self, aLineLeft: nscoord, aBlockStart: nscoord);
}

impl dyn ShapeInfo {
    // Convert the LogicalRect to the special logical coordinate space used
    // in float manager.
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::ShapeInfo::ConvertToFloatLogical
    pub fn ConvertToFloatLogical__LogicalRect_WritingMode_nsSize(aRect: LogicalRect, aWM: WritingMode, aContainerSize: nsSize) -> nsRect {
        nsRect::new(aRect.LineLeft(aWM, aContainerSize), aRect.BStart(aWM), aRect.ISize(aWM), aRect.BSize(aWM))
    }
}

// NOT PORTED: the rest of class nsFloatManager::ShapeInfo (nsFloatManager.cpp lines 508-655 and
// 2442-2970: ComputeShapeBoxRect, CreateShapeBox, CreateBasicShape, CreateInset,
// CreateCircleOrEllipse, CreatePolygon, CreateImageShape, ComputeEllipseLineInterceptDiff,
// XInterceptAtY, the point and corner-radii forms of ConvertToFloatLogical,
// MinIntervalIndexContainingY, LineEdge, the distance-field constants and CalcUsedShapeMargin5X) and
// the four subclasses (EllipseShapeInfo lines 657-1026, RoundedBoxShapeInfo 1027-1214,
// PolygonShapeInfo 1215-1732, ImageShapeInfo 1733-2255). They implement `shape-outside`, which is
// not in the servo configuration of Stylo (nsStyleDisplay::mShapeOutside() is always None), so no
// FloatInfo ever gets a ShapeInfo. Translate them when `shape-outside` is wanted.

/////////////////////////////////////////////////////////////////////////////
// FloatInfo

impl FloatInfo {
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::FloatInfo::FloatInfo
    pub fn new(aFrame: nsIFrame, aLineLeft: nscoord, aBlockStart: nscoord, aMarginRect: LogicalRect, aWM: WritingMode, aContainerSize: nsSize) -> FloatInfo {
        let mut this = FloatInfo {
            mFrame: aFrame,
            mLeftBEnd: nscoord_MIN,
            mRightBEnd: nscoord_MIN,
            mRect: <dyn ShapeInfo>::ConvertToFloatLogical__LogicalRect_WritingMode_nsSize(aMarginRect, aWM, aContainerSize) + nsPoint::new(aLineLeft, aBlockStart),
            mShapeInfo: None,
        };
        // using ShapeOutsideType = StyleShapeOutside::Tag;

        if this.IsEmpty__() {
            // Per spec, a float area defined by a shape is clipped to the float’s
            // margin box. Therefore, no need to create a shape info if the float's
            // margin box is empty, since a float area can only be smaller than the
            // margin box.

            // https://drafts.csswg.org/css-shapes/#relation-to-box-model-and-float-behavior
            return this;
        }

        let styleDisplay: nsStyleDisplay = this.mFrame.StyleDisplay();
        let shapeOutside = styleDisplay.mShapeOutside();

        // nscoord shapeMargin = shapeOutside.IsNone() ? 0 : nsLayoutUtils::ResolveToLength<true>(...)
        // NOT PORTED: shapeMargin (only used by the shape cases below).

        match shapeOutside {
            StyleShapeOutside::None => {
                // No need to create shape info.
                return this;
            }

            StyleShapeOutside::Image => {
                // NOT PORTED: shape-outside: <image> (ShapeInfo::CreateImageShape)
                unimplemented!("NOT PORTED: shape-outside")
            }

            StyleShapeOutside::Box => {
                // NOT PORTED: shape-outside: <shape-box> (ShapeInfo::ComputeShapeBoxRect, CreateShapeBox)
                unimplemented!("NOT PORTED: shape-outside")
            }

            StyleShapeOutside::Shape => {
                // NOT PORTED: shape-outside: <basic-shape> (ShapeInfo::CreateBasicShape)
                unimplemented!("NOT PORTED: shape-outside")
            }
        }

        // MOZ_ASSERT(mShapeInfo, "All shape-outside values except none should have mShapeInfo!");
        // Translate the shape to the same origin as nsFloatManager.
        // mShapeInfo->Translate(aLineLeft, aBlockStart);
        // (unreachable here: every case above returns or is not ported)
    }

    // aBStart and aBEnd are the starting and ending coordinate of a band.
    // LineLeft() and LineRight() return the innermost line-left extent and
    // line-right extent within the given band, respectively.
    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::FloatInfo::LineLeft
    pub fn LineLeft__ShapeType_nscoord_nscoord(&self, aShapeType: ShapeType, aBStart: nscoord, aBEnd: nscoord) -> nscoord {
        if aShapeType == ShapeType::Margin {
            return self.LineLeft__();
        }

        MOZ_ASSERT!(aShapeType == ShapeType::ShapeOutside);
        let Some(shapeInfo) = self.mShapeInfo.as_ref() else {
            return self.LineLeft__();
        };
        // Clip the flow area to the margin-box because
        // https://drafts.csswg.org/css-shapes-1/#relation-to-box-model-and-float-behavior
        // says "When a shape is used to define a float area, the shape is clipped
        // to the float’s margin box."
        std::cmp::max(self.LineLeft__(), shapeInfo.LineLeft(aBStart, aBEnd))
    }

    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::FloatInfo::LineRight
    pub fn LineRight__ShapeType_nscoord_nscoord(&self, aShapeType: ShapeType, aBStart: nscoord, aBEnd: nscoord) -> nscoord {
        if aShapeType == ShapeType::Margin {
            return self.LineRight__();
        }

        MOZ_ASSERT!(aShapeType == ShapeType::ShapeOutside);
        let Some(shapeInfo) = self.mShapeInfo.as_ref() else {
            return self.LineRight__();
        };
        // Clip the flow area to the margin-box. See LineLeft().
        std::cmp::min(self.LineRight__(), shapeInfo.LineRight(aBStart, aBEnd))
    }

    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::FloatInfo::BStart
    pub fn BStart__ShapeType(&self, aShapeType: ShapeType) -> nscoord {
        if aShapeType == ShapeType::Margin {
            return self.BStart__();
        }

        MOZ_ASSERT!(aShapeType == ShapeType::ShapeOutside);
        let Some(shapeInfo) = self.mShapeInfo.as_ref() else {
            return self.BStart__();
        };
        // Clip the flow area to the margin-box. See LineLeft().
        std::cmp::max(self.BStart__(), shapeInfo.BStart())
    }

    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::FloatInfo::BEnd
    pub fn BEnd__ShapeType(&self, aShapeType: ShapeType) -> nscoord {
        if aShapeType == ShapeType::Margin {
            return self.BEnd__();
        }

        MOZ_ASSERT!(aShapeType == ShapeType::ShapeOutside);
        let Some(shapeInfo) = self.mShapeInfo.as_ref() else {
            return self.BEnd__();
        };
        // Clip the flow area to the margin-box. See LineLeft().
        std::cmp::min(self.BEnd__(), shapeInfo.BEnd())
    }

    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::FloatInfo::IsEmpty
    pub fn IsEmpty__ShapeType(&self, aShapeType: ShapeType) -> bool {
        if aShapeType == ShapeType::Margin {
            return self.IsEmpty__();
        }

        MOZ_ASSERT!(aShapeType == ShapeType::ShapeOutside);
        let Some(shapeInfo) = self.mShapeInfo.as_ref() else {
            return self.IsEmpty__();
        };
        shapeInfo.IsEmpty()
    }

    // Gecko: layout/generic/nsFloatManager.cpp nsFloatManager::FloatInfo::MayNarrowInBlockDirection
    pub fn MayNarrowInBlockDirection(&self, aShapeType: ShapeType) -> bool {
        // This function mirrors the cases of the three argument versions of
        // LineLeft() and LineRight(). This function returns true if and only if
        // either of those functions could possibly return "narrower" values with
        // increasing aBStart values. "Narrower" means closer to the far end of
        // the float shape.
        if aShapeType == ShapeType::Margin {
            return false;
        }

        MOZ_ASSERT!(aShapeType == ShapeType::ShapeOutside);
        let Some(shapeInfo) = self.mShapeInfo.as_ref() else {
            return false;
        };

        shapeInfo.MayNarrowInBlockDirection()
    }
}

// Tests: layout/generic has no gtest for nsFloatManager (Firefox tests it through reftests), so
// these are written from the documented behaviour of GetFlowArea, AddFloat, ClearFloats and the
// saved state. Frames get their styles from real HTML run through Stylo (mrt-style).
#[cfg(test)]
mod tests {
    use super::*;
    use mrt_style::mrt_dom::{Dom, Kind, NodeId};
    use mrt_style::style::properties::ComputedValues;
    use mrt_style::style::servo_arc::Arc as ServoArc;
    use mrt_style::StyleDom;

    // Parses `html`, resolves styles, and returns the computed style of each element with an id.
    fn styles_of(html: &str, ids: &[&str]) -> Vec<ServoArc<ComputedValues>> {
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

    const CONTAINER: nsSize = nsSize { width: 400, height: 1000 };

    // A container frame (width 400) with one child frame per rect, in the style of the same-index id.
    fn frames(html: &str, ids: &[&str], rects: &[nsRect]) -> Vec<nsIFrame> {
        let wm = WritingMode::default();
        let mut s = styles_of(html, &["box"].iter().chain(ids.iter()).copied().collect::<Vec<_>>());
        let box_style = s.remove(0);
        WithFrameTree(|t| {
            let parent = t.AddFrame(None, nsRect::new(0, 0, 400, 1000), wm, Some(box_style));
            s.into_iter().zip(rects).map(|(st, r)| t.AddFrame(Some(parent), *r, wm, Some(st))).collect()
        })
    }

    fn area(fm: &nsFloatManager, b: nscoord, bsize: nscoord) -> nsFlowAreaRect {
        let wm = WritingMode::default();
        fm.GetFlowArea(wm, wm, b, bsize, BandInfoType::BandFromPoint, ShapeType::Margin, LogicalRect::new(wm, 0, 0, 400, nscoord_MAX), None, CONTAINER)
    }

    fn add(fm: &mut nsFloatManager, f: nsIFrame, r: nsRect) {
        let wm = WritingMode::default();
        fm.AddFloat(f, LogicalRect::new__WritingMode_nsRect_nsSize(wm, r, CONTAINER), wm, CONTAINER);
    }

    const HTML: &str = r#"<div id=box><div id=l1 style="float:left"></div><div id=r1 style="float:right"></div><div id=l2 style="float:left"></div></div>"#;

    #[test]
    fn no_floats_gives_the_whole_content_area() {
        let fm = nsFloatManager::new(WritingMode::default());
        let a = area(&fm, 0, nscoord_MAX);
        assert_eq!((a.mRect.IStart(WritingMode::default()), a.mRect.ISize(WritingMode::default())), (0, 400));
        assert!(!a.HasFloats());
        assert_eq!(fm.ClearFloats(7, UsedClear::Both), 7);
        assert_eq!(fm.LowestFloatBStart(), nscoord_MIN);
    }

    #[test]
    fn a_left_float_narrows_its_band_and_ends_it() {
        let wm = WritingMode::default();
        let r = [nsRect::new(0, 0, 100, 50)];
        let f = frames(HTML, &["l1"], &r);
        let mut fm = nsFloatManager::new(wm);
        add(&mut fm, f[0], r[0]);

        let a = area(&fm, 0, nscoord_MAX);
        assert_eq!((a.mRect.IStart(wm), a.mRect.ISize(wm), a.mRect.BSize(wm)), (100, 300, 50));
        assert!(a.HasFloats() && !a.MayWiden());

        let below = area(&fm, 50, nscoord_MAX);
        assert_eq!((below.mRect.IStart(wm), below.mRect.ISize(wm)), (0, 400));
        assert!(!below.HasFloats());
    }

    #[test]
    fn left_and_right_floats_make_bands() {
        let wm = WritingMode::default();
        let r = [nsRect::new(0, 0, 100, 50), nsRect::new(320, 0, 80, 30)];
        let f = frames(HTML, &["l1", "r1"], &r);
        let mut fm = nsFloatManager::new(wm);
        add(&mut fm, f[0], r[0]);
        add(&mut fm, f[1], r[1]);

        let first = area(&fm, 0, nscoord_MAX);
        assert_eq!((first.mRect.IStart(wm), first.mRect.ISize(wm), first.mRect.BSize(wm)), (100, 220, 30));
        let second = area(&fm, 30, nscoord_MAX);
        assert_eq!((second.mRect.IStart(wm), second.mRect.ISize(wm), second.mRect.BSize(wm)), (100, 300, 20));

        // The two floats' lowest edges, for `clear`.
        assert_eq!(fm.ClearFloats(0, UsedClear::Left), 50);
        assert_eq!(fm.ClearFloats(0, UsedClear::Right), 30);
        assert_eq!(fm.ClearFloats(0, UsedClear::Both), 50);
        assert_eq!(fm.ClearFloats(0, UsedClear::None), 0);
        assert_eq!(fm.ClearFloats(80, UsedClear::Both), 80);
    }

    #[test]
    fn width_within_height_takes_the_narrowest_over_the_span() {
        let wm = WritingMode::default();
        let r = [nsRect::new(0, 0, 100, 50), nsRect::new(320, 20, 80, 30)];
        let f = frames(HTML, &["l1", "r1"], &r);
        let mut fm = nsFloatManager::new(wm);
        add(&mut fm, f[0], r[0]);
        add(&mut fm, f[1], r[1]);
        let a = fm.GetFlowArea(wm, wm, 0, 40, BandInfoType::WidthWithinHeight, ShapeType::Margin, LogicalRect::new(wm, 0, 0, 400, nscoord_MAX), None, CONTAINER);
        // Over 0..40 the left float always intrudes by 100 and the right one from 20 on by 80.
        assert_eq!((a.mRect.IStart(wm), a.mRect.ISize(wm), a.mRect.BSize(wm)), (100, 220, 40));
    }

    #[test]
    fn push_and_pop_state_undo_added_floats() {
        let wm = WritingMode::default();
        let r = [nsRect::new(0, 0, 100, 50), nsRect::new(320, 0, 80, 30)];
        let f = frames(HTML, &["l1", "r1"], &r);
        let mut fm = nsFloatManager::new(wm);
        add(&mut fm, f[0], r[0]);
        let mut saved = SavedState::new();
        fm.PushState(&mut saved);
        add(&mut fm, f[1], r[1]);
        assert!(area(&fm, 0, nscoord_MAX).mRect.ISize(wm) == 220);
        // computing against the saved state ignores the later float
        let old = fm.GetFlowArea(wm, wm, 0, nscoord_MAX, BandInfoType::BandFromPoint, ShapeType::Margin, LogicalRect::new(wm, 0, 0, 400, nscoord_MAX), Some(&saved), CONTAINER);
        assert_eq!(old.mRect.ISize(wm), 300);
        fm.PopState(&saved);
        assert_eq!(area(&fm, 0, nscoord_MAX).mRect.ISize(wm), 300);
        fm.AssertStateMatches(&saved);
    }

    #[test]
    fn translate_shifts_the_coordinate_space() {
        let wm = WritingMode::default();
        let r = [nsRect::new(0, 0, 100, 50)];
        let f = frames(HTML, &["l1"], &r);
        let mut fm = nsFloatManager::new(wm);
        fm.Translate(0, 10);
        add(&mut fm, f[0], r[0]); // the float's block start is 10 in the manager's world
        let (mut ll, mut bs) = (0, 0);
        fm.GetTranslation(&mut ll, &mut bs);
        assert_eq!((ll, bs), (0, 10));
        assert_eq!(fm.LowestFloatBStart(), 0); // relative to the current translation
        assert_eq!(area(&fm, 0, nscoord_MAX).mRect.IStart(wm), 100);
        fm.Translate(0, 60); // now 60 below the float's start: beyond its end
        assert!(!area(&fm, 0, nscoord_MAX).HasFloats());
    }

    #[test]
    fn remove_trailing_regions_drops_the_listed_last_floats() {
        let wm = WritingMode::default();
        let r = [nsRect::new(0, 0, 100, 50), nsRect::new(0, 50, 60, 10), nsRect::new(0, 60, 60, 10)];
        let f = frames(HTML, &["l1", "r1", "l2"], &r);
        let mut fm = nsFloatManager::new(wm);
        for i in 0..3 {
            add(&mut fm, f[i], r[i]);
        }
        // f[1] and f[2] are siblings in this tree, so the list starting at f[1] is [f1, f2]
        assert_eq!(fm.RemoveTrailingRegions(Some(f[1])), NS_OK);
        assert_eq!(fm.LowestFloatBStart(), 0);
        assert!(fm.HasAnyFloats());
    }

    #[test]
    fn float_region_is_stored_as_a_delta_on_the_frame() {
        let wm = WritingMode::default();
        let r = [nsRect::new(10, 20, 100, 50)];
        let f = frames(HTML, &["l1"], &r);
        let plain = nsFloatManager::GetRegionFor(wm, f[0], CONTAINER);
        assert_eq!(plain.GetPhysicalRect(wm, CONTAINER), r[0]);
        let region = LogicalRect::new__WritingMode_nsRect_nsSize(wm, nsRect::new(5, 15, 120, 70), CONTAINER);
        nsFloatManager::StoreRegionFor(wm, f[0], region, CONTAINER);
        assert_eq!(nsFloatManager::GetRegionFor(wm, f[0], CONTAINER).GetPhysicalRect(wm, CONTAINER), nsRect::new(5, 15, 120, 70));
        // storing the frame's own rect removes the delta
        nsFloatManager::StoreRegionFor(wm, f[0], plain, CONTAINER);
        assert_eq!(f[0].GetProperty(FloatRegionProperty()), None);
    }

    #[test]
    fn writing_mode_is_read_from_the_computed_style() {
        let s = styles_of(r#"<div id=box><p id=a style="direction:rtl"></p><p id=b></p></div>"#, &["a", "b"]);
        let a = WritingMode::new__ComputedStyle(&s[0]);
        let b = WritingMode::new__ComputedStyle(&s[1]);
        assert!(!a.IsBidiLTR());
        assert!(b.IsBidiLTR());
        assert!(!a.IsVertical() && !b.IsVertical());
    }

    #[test]
    fn right_to_left_flips_inline_start_not_line_left() {
        // In an rtl block a float:left stays on the line-left side: GetFlowArea works in the
        // line-relative axis, so the band is narrowed on the left whatever the direction.
        let rtl = WritingMode::new__ComputedStyle(&styles_of(r#"<div id=box><p id=a style="direction:rtl"></p></div>"#, &["a"])[0]);
        let r = [nsRect::new(0, 0, 100, 50)];
        let f = frames(HTML, &["l1"], &r);
        let mut fm = nsFloatManager::new(rtl);
        fm.AddFloat(f[0], LogicalRect::new__WritingMode_nsRect_nsSize(rtl, r[0], CONTAINER), rtl, CONTAINER);
        let a = fm.GetFlowArea(rtl, rtl, 0, nscoord_MAX, BandInfoType::BandFromPoint, ShapeType::Margin, LogicalRect::new(rtl, 0, 0, 400, nscoord_MAX), None, CONTAINER);
        // physical: the float occupies x 0..100, so the free line-left..right span is 100..400,
        // which in rtl is inline-start 0 (at the right edge) and inline size 300.
        assert_eq!(a.mRect.ISize(rtl), 300);
        assert_eq!(a.mRect.GetPhysicalRect(rtl, CONTAINER).x, 100);
    }
}
