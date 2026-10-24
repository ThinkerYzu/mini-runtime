/* state and methods used while laying out a single line of a block frame */
// Translated from layout/generic/nsLineLayout.h and nsLineLayout.cpp (GECKO_REV).
//
// Notes on the translation (see CONVENTIONS.md for the general rules):
//  - PerFrameData and PerSpanData are linked through raw pointers, as in C++. They live in boxes
//    owned by `mFrameArena` and `mSpanArena` of the outermost line layout (C++: ArenaAllocator) and are recycled through
//    the free lists, so a pointer stays valid until the line layout is dropped.
//  - `mLineContainerRI` (`const ReflowInput&`) and PerSpanData::mReflowInput are raw pointers with
//    the lifetime erased: the caller keeps the ReflowInput alive for the whole line, as in C++.
//  - Bit fields are plain bools. The union of PerSpanData::mParent and mNextFreeSpan is two fields.
//  - What is not translated yet comes from nsLineLayoutAdapter.rs (see there).
//  - NOT PORTED for the whole file: the debug-only printing under NOISY_* and ABSURD_SIZE checks,
//    refcount logging (MOZ_COUNT_CTOR), bidi reordering (nsBidiPresUtils), ruby (RubyUtils,
//    annotations), ::first-letter specifics beyond the flags, text-combine, text-emphasis.
use crate::nsLineLayoutAdapter::*;
use crate::*;

// Gecko: layout/generic/nsLineLayout.cpp VALIGN_OTHER, VALIGN_TOP, VALIGN_BOTTOM, VALIGN_CENTER
pub const VALIGN_OTHER: u8 = 0;
pub const VALIGN_TOP: u8 = 1;
pub const VALIGN_BOTTOM: u8 = 2;
pub const VALIGN_CENTER: u8 = 3;

// Gecko: layout/generic/nsLineLayout.cpp BLOCKDIR_ALIGN_FRAMES_NO_MINIMUM, BLOCKDIR_ALIGN_FRAMES_NO_MAXIMUM
pub const BLOCKDIR_ALIGN_FRAMES_NO_MINIMUM: nscoord = nscoord_MAX;
pub const BLOCKDIR_ALIGN_FRAMES_NO_MAXIMUM: nscoord = nscoord_MIN;

// ADAPTER: the arena pointers are dereferenced through these (C++ writes `pfd->mX`).
#[inline]
fn PFD<'a>(aPFD: *mut nsLineLayout_PerFrameData) -> &'a mut nsLineLayout_PerFrameData {
    unsafe { &mut *aPFD }
}
#[inline]
fn PSD<'a>(aPSD: *mut nsLineLayout_PerSpanData) -> &'a mut nsLineLayout_PerSpanData {
    unsafe { &mut *aPSD }
}

// Per-frame data recorded by the line-layout reflow logic. This
// state is the state needed to post-process the line after reflow
// has completed (block-direction alignment, inline-direction alignment,
// justification and relative positioning).
// Gecko: layout/generic/nsLineLayout.h nsLineLayout::PerFrameData
pub struct nsLineLayout_PerFrameData {
    // link to next/prev frame in same span
    pub mNext: *mut nsLineLayout_PerFrameData,
    pub mPrev: *mut nsLineLayout_PerFrameData,

    // Link to the frame of next ruby annotation.  It is a linked list
    // through this pointer from ruby base to all its annotations.  It
    // could be nullptr if there is no more annotation.
    // If PFD_ISLINKEDTOBASE is set, the current PFD is one of the ruby
    // annotations in the base's list, otherwise it is the ruby base,
    // and its mNextAnnotation is the start of the linked list.
    pub mNextAnnotation: *mut nsLineLayout_PerFrameData,

    // pointer to child span data if this is an inline container frame
    pub mSpan: *mut nsLineLayout_PerSpanData,

    // The frame
    pub mFrame: nsIFrame,

    // From metrics
    pub mAscent: nscoord,
    // note that mBounds is a logical rect in the *line*'s writing mode.
    // When setting frame coordinates, we have to convert to the frame's
    //  writing mode
    pub mBounds: LogicalRect,
    pub mOverflowAreas: OverflowAreas,

    // From reflow-state
    pub mMargin: LogicalMargin,        // in *line* writing mode
    pub mBorderPadding: LogicalMargin, // in *line* writing mode
    pub mOffsets: LogicalMargin,       // in *frame* writing mode

    // state for text justification
    // Note that, although all frames would have correct inner
    // opportunities computed after ComputeFrameJustification, start
    // and end justifiable info are not reliable for non-text frames.
    pub mJustificationInfo: JustificationInfo,
    pub mJustificationAssignment: JustificationAssignment,

    // PerFrameData flags
    pub mIsRelativelyOrStickyPos: bool,
    pub mIsTextFrame: bool,
    pub mIsNonEmptyTextFrame: bool,
    pub mIsNonWhitespaceTextFrame: bool,
    pub mIsLetterFrame: bool,
    pub mRecomputeOverflow: bool,
    pub mIsMarker: bool,
    pub mSkipWhenTrimmingWhitespace: bool,
    pub mIsEmpty: bool,
    pub mIsPlaceholder: bool,
    pub mIsLinkedToBase: bool,

    // Other state we use
    pub mBlockDirAlign: u8,
    pub mWritingMode: WritingMode,
}

impl nsLineLayout_PerFrameData {
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::PerFrameData::Last
    pub fn Last(&mut self) -> *mut nsLineLayout_PerFrameData {
        let mut pfd: *mut nsLineLayout_PerFrameData = self;
        while !PFD(pfd).mNext.is_null() {
            pfd = PFD(pfd).mNext;
        }
        pfd
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::PerFrameData::IsStartJustifiable
    pub fn IsStartJustifiable(&self) -> bool {
        self.mJustificationInfo.mIsStartJustifiable
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::PerFrameData::IsEndJustifiable
    pub fn IsEndJustifiable(&self) -> bool {
        self.mJustificationInfo.mIsEndJustifiable
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::PerFrameData::ParticipatesInJustification
    pub fn ParticipatesInJustification(&self) -> bool {
        if self.mIsMarker || self.mIsEmpty || self.mSkipWhenTrimmingWhitespace {
            // Skip ::markers, empty frames, and placeholders
            return false;
        }
        if self.mIsTextFrame && !self.mIsNonWhitespaceTextFrame && self.mFrame.IsAtEndOfLine() {
            // Skip trimmed whitespaces
            return false;
        }
        true
    }
}

// In nsLineLayout, a "span" is a container inline frame, and a "frame" is one
// of its children.
//
// nsLineLayout::BeginLineReflow() creates the initial PerSpanData which is
// called the "root span". nsInlineFrame::ReflowFrames() creates a new
// PerSpanData when it calls nsLineLayout::BeginSpan(); at this time, the
// nsLineLayout object's mCurrentSpan is switched to the new span. The new
// span records the old mCurrentSpan as its parent. After reflowing the child
// inline frames, nsInlineFrame::ReflowFrames() calls nsLineLayout::EndSpan(),
// which pops the PerSpanData and re-sets mCurrentSpan.
// Gecko: layout/generic/nsLineLayout.h nsLineLayout::PerSpanData
pub struct nsLineLayout_PerSpanData {
    // (a union in C++: mParent while the span is in use, mNextFreeSpan on the free list)
    pub mParent: *mut nsLineLayout_PerSpanData,
    pub mNextFreeSpan: *mut nsLineLayout_PerSpanData,

    // The PerFrameData of the inline frame that "owns" the span, or null if
    // this is the root span. mFrame is initialized to the containing inline
    // frame's PerFrameData when a new PerSpanData is pushed in
    // nsLineLayout::BeginSpan().
    pub mFrame: *mut nsLineLayout_PerFrameData,

    // The first PerFrameData structure in the span.
    pub mFirstFrame: *mut nsLineLayout_PerFrameData,

    // The last PerFrameData structure in the span. PerFrameData structures are
    // added to the span as they are reflowed. mLastFrame may also be directly
    // manipulated if a line is split, or if frames are pushed from one line to
    // the next.
    pub mLastFrame: *mut nsLineLayout_PerFrameData,

    // (`const ReflowInput*`; the lifetime is erased, see the top of the file)
    pub mReflowInput: *const ReflowInput<'static>,
    pub mNoWrap: bool,
    pub mWritingMode: WritingMode,
    pub mContainsFloat: bool,
    pub mHasNonemptyContent: bool,

    pub mIStart: nscoord,
    pub mICoord: nscoord,
    pub mIEnd: nscoord,
    pub mInset: nscoord,

    pub mBStartLeading: nscoord,
    pub mBEndLeading: nscoord,
    pub mLogicalBSize: nscoord,
    pub mMinBCoord: nscoord,
    pub mMaxBCoord: nscoord,
    pub mBaseline: *mut nscoord,
}

impl nsLineLayout_PerSpanData {
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::PerSpanData::AppendFrame
    pub fn AppendFrame(&mut self, pfd: *mut nsLineLayout_PerFrameData) {
        if self.mLastFrame.is_null() {
            self.mFirstFrame = pfd;
        } else {
            PFD(self.mLastFrame).mNext = pfd;
            PFD(pfd).mPrev = self.mLastFrame;
        }
        self.mLastFrame = pfd;
    }
}

// Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::JustificationComputationState
pub struct nsLineLayout_JustificationComputationState {
    pub mFirstParticipant: *mut nsLineLayout_PerFrameData,
    pub mLastParticipant: *mut nsLineLayout_PerFrameData,
    // When we are going across a boundary of ruby base, i.e. entering
    // one, leaving one, or both, the following fields will be set to
    // the corresponding ruby base frame for handling ruby-align.
    pub mLastExitedRubyBase: *mut nsLineLayout_PerFrameData,
    pub mLastEnteredRubyBase: *mut nsLineLayout_PerFrameData,
}

impl nsLineLayout_JustificationComputationState {
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::JustificationComputationState::JustificationComputationState
    pub fn new() -> nsLineLayout_JustificationComputationState {
        nsLineLayout_JustificationComputationState {
            mFirstParticipant: std::ptr::null_mut(),
            mLastParticipant: std::ptr::null_mut(),
            mLastExitedRubyBase: std::ptr::null_mut(),
            mLastEnteredRubyBase: std::ptr::null_mut(),
        }
    }
}

// Gecko: layout/generic/nsLineLayout.h nsLineLayout
pub struct nsLineLayout {
    // This state is constant for a given block frame doing line layout
    mPresContext: nsPresContext,

    // A non-owning pointer, which points to the object owned by
    // nsAutoFloatManager::mNew.
    mFloatManager: *mut nsFloatManager,

    mStyleText: nsStyleTextAdapter, // for the block
    mLineContainerRI: *const ReflowInput<'static>,

    // The line layout for the base text.  It is usually nullptr.
    // It becomes not null when the current line layout is for ruby
    // annotations. When there is nested ruby inside annotation, it
    // forms a linked list from the inner annotation to the outermost
    // line layout. The outermost line layout, which has this member
    // being nullptr, is responsible for managing the life cycle of
    // per-frame data and per-span data, and handling floats.
    mBaseLineLayout: *mut nsLineLayout,

    mLastOptionalBreakFrame: Option<nsIFrame>,
    mForceBreakFrame: Option<nsIFrame>,

    // XXX Take care that nsRubyBaseContainer would give nullptr to this
    //     member. It should not be a problem currently, since the only
    //     code use it is handling float, which does not affect ruby.
    //     See comment in nsLineLayout::AddFloat
    mBlockRS: *mut BlockReflowState<'static>, /* XXX hack! */

    mLineBox: nsLineList_iterator,

    mFrameFreeList: *mut nsLineLayout_PerFrameData,
    mSpanFreeList: *mut nsLineLayout_PerSpanData,
    mRootSpan: *mut nsLineLayout_PerSpanData,
    mCurrentSpan: *mut nsLineLayout_PerSpanData,

    mLastOptionalBreakPriority: gfxBreakPriority,
    mLastOptionalBreakFrameOffset: i32,
    mForceBreakFrameOffset: i32,

    mMinLineBSize: nscoord,

    // The amount of text indent that we applied to this line, needed for
    // max-element-size calculation.
    mTextIndent: nscoord,

    // This state varies during the reflow of a line but is line
    // "global" state not span "local" state.
    mLineNumber: i32,
    mJustificationInfo: JustificationInfo,

    mTotalPlacedFrames: i32,

    mBStartEdge: nscoord,
    mMaxStartBoxBSize: nscoord,
    mMaxEndBoxBSize: nscoord,

    mInflationMinFontSize: nscoord,

    // Final computed line-bSize value after VerticalAlignFrames for
    // the block has been called.
    mFinalLineBSize: nscoord,

    // Amount of trimmable whitespace inline size for the trailing text
    // frame, if any
    mTrimmableISize: nscoord,

    // Physical size. Use only for physical <-> logical coordinate conversion.
    mContainerSize: nsSize,

    mFirstLetterStyleOK: bool,
    mIsTopOfPage: bool,
    mImpactedByFloats: bool,
    mLastFloatWasLetterFrame: bool,
    mLineIsEmpty: bool,
    mLineEndsInBR: bool,
    mNeedBackup: bool,
    mInFirstLine: bool,
    mGotLineBox: bool,
    mInFirstLetter: bool,
    mHasMarker: bool,
    mDirtyNextLine: bool,
    mLineAtStart: bool,
    mHasRuby: bool,
    mSuppressLineWrap: bool,
    mUsedOverflowWrap: bool,

    mSpanDepth: i32,
    #[cfg(debug_assertions)]
    mSpansAllocated: i32,
    #[cfg(debug_assertions)]
    mSpansFreed: i32,
    #[cfg(debug_assertions)]
    mFramesAllocated: i32,
    #[cfg(debug_assertions)]
    mFramesFreed: i32,

    /**
     * Per span and per frame data.
     */
    // (C++: mozilla::ArenaAllocator<1024, sizeof(void*)>; the boxes are what it hands out)
    mFrameArena: Vec<Box<nsLineLayout_PerFrameData>>,
    mSpanArena: Vec<Box<nsLineLayout_PerSpanData>>,
}

impl Drop for nsLineLayout {
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::~nsLineLayout
    // NOT PORTED: refcount logging (MOZ_COUNT_DTOR)
    fn drop(&mut self) {
        if !std::thread::panicking() {
            MOZ_ASSERT!(self.mRootSpan.is_null(), "bad line-layout user");
        }
    }
}

impl nsLineLayout {
    /**
     * @param aBaseLineLayout the nsLineLayout for ruby base,
     * nullptr if no separate base nsLineLayout is needed.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::nsLineLayout
    // NOT PORTED: refcount logging (MOZ_COUNT_CTOR)
    pub fn new(aPresContext: &nsPresContext, aFloatManager: *mut nsFloatManager, aLineContainerRI: &ReflowInput, aLine: Option<&nsLineList_iterator>, aBaseLineLayout: *mut nsLineLayout) -> nsLineLayout {
        let lineContainerRI: *const ReflowInput<'static> = unsafe { std::mem::transmute::<*const ReflowInput, *const ReflowInput<'static>>(aLineContainerRI as *const ReflowInput) };
        let containerFrame = aLineContainerRI.mFrame;
        let mut this = nsLineLayout {
            mPresContext: *aPresContext,
            mFloatManager: aFloatManager,
            mStyleText: nsStyleTextAdapter::of(containerFrame),
            mLineContainerRI: lineContainerRI,
            mBaseLineLayout: aBaseLineLayout,
            mLastOptionalBreakFrame: None,
            mForceBreakFrame: None,
            mBlockRS: std::ptr::null_mut(),
            mLineBox: nsLineList_iterator::default(),
            mFrameFreeList: std::ptr::null_mut(),
            mSpanFreeList: std::ptr::null_mut(),
            mRootSpan: std::ptr::null_mut(),
            mCurrentSpan: std::ptr::null_mut(),
            mLastOptionalBreakPriority: gfxBreakPriority::eNoBreak,
            mLastOptionalBreakFrameOffset: -1,
            mForceBreakFrameOffset: -1,
            mMinLineBSize: 0,
            mTextIndent: 0,
            mLineNumber: 0,
            mJustificationInfo: JustificationInfo::new(),
            mTotalPlacedFrames: 0,
            mBStartEdge: 0,
            mMaxStartBoxBSize: 0,
            mMaxEndBoxBSize: 0,
            mInflationMinFontSize: 0,
            mFinalLineBSize: 0,
            mTrimmableISize: 0,
            mContainerSize: nsSize::new(0, 0),
            mFirstLetterStyleOK: false,
            mIsTopOfPage: false,
            mImpactedByFloats: false,
            mLastFloatWasLetterFrame: false,
            mLineIsEmpty: false,
            mLineEndsInBR: false,
            mNeedBackup: false,
            mInFirstLine: false,
            mGotLineBox: false,
            mInFirstLetter: false,
            mHasMarker: false,
            mDirtyNextLine: false,
            mLineAtStart: false,
            mHasRuby: false,
            mSuppressLineWrap: containerFrame.IsInSVGTextSubtree(),
            mUsedOverflowWrap: false,
            mSpanDepth: 0,
            #[cfg(debug_assertions)]
            mSpansAllocated: 0,
            #[cfg(debug_assertions)]
            mSpansFreed: 0,
            #[cfg(debug_assertions)]
            mFramesAllocated: 0,
            #[cfg(debug_assertions)]
            mFramesFreed: 0,
            mFrameArena: Vec::new(),
            mSpanArena: Vec::new(),
        };
        NS_ASSERTION!(!aFloatManager.is_null() || this.LineContainerFrame().IsLetterFrame(), "float manager should be present");
        MOZ_ASSERT!(!aBaseLineLayout.is_null() == this.LineContainerFrame().IsRubyTextContainerFrame(), "Only ruby text container frames have a different base line layout");

        // Stash away some style data that we need
        // (`nsBlockFrame* blockFrame = do_QueryFrame(...)`: NOT PORTED, nsBlockFrame::StyleTextForLineLayout
        // is the style of the ::first-line frame when there is one; here always the container's.)
        this.mStyleText = nsStyleTextAdapter::of(this.LineContainerFrame());

        this.mInflationMinFontSize = nsLayoutUtils::InflationMinFontSizeFor(this.LineContainerFrame());

        if let Some(aLine) = aLine {
            this.mGotLineBox = true;
            this.mLineBox = *aLine;
        }
        this
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::Init
    // (the lifetime of the BlockReflowState is erased, see the top of the file)
    pub fn Init<'a>(&mut self, aState: *mut BlockReflowState<'a>, aMinLineBSize: nscoord, aLineNumber: i32) {
        self.mBlockRS = aState as *mut BlockReflowState<'static>;
        self.mMinLineBSize = aMinLineBSize;
        self.mLineNumber = aLineNumber;
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetLineNumber
    pub fn GetLineNumber(&self) -> i32 {
        self.mLineNumber
    }
}

// Gecko: layout/generic/nsLineLayout.cpp ShouldApplyTextIndent
fn ShouldApplyTextIndent(aLineContainer: nsIFrame) -> bool {
    if aLineContainer.IsRubyTextContainerFrame() {
        return false;
    }
    // (`nsBlockFrame* block = do_QueryFrame(aLineContainer); block && block->IsTextInput()`:
    // NOT PORTED, nsBlockFrame; text input frames are of their own type here.)
    if aLineContainer.Type() == LayoutFrameType::TextInput {
        // text-indent applies to the inner text input frames individually.
        return false;
    }
    true
}

impl nsLineLayout {
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::BeginLineReflow
    // (the default argument `aInset = 0` is written out at the call sites)
    pub fn BeginLineReflow(&mut self, aICoord: nscoord, aBCoord: nscoord, aISize: nscoord, aBSize: nscoord, aImpactedByFloats: bool, aIsTopOfPage: bool, aWritingMode: WritingMode, aContainerSize: nsSize, aInset: nscoord) {
        MOZ_ASSERT!(self.mRootSpan.is_null(), "bad linelayout user");
        NS_WARNING_ASSERTION!(aISize != NS_UNCONSTRAINEDSIZE, "have unconstrained width; this should only result from very large sizes, not attempts at intrinsic width calculation");
        // NOT PORTED: the DEBUG-only ABSURD_SIZE checks that print "bad caller" (needs
        // nsIFrame::IsAbsurdSizeAssertSuppressed and ListTag).
        #[cfg(debug_assertions)]
        {
            self.mSpansAllocated = 0;
            self.mSpansFreed = 0;
            self.mFramesAllocated = 0;
            self.mFramesFreed = 0;
        }

        self.mFirstLetterStyleOK = false;
        self.mIsTopOfPage = aIsTopOfPage;
        self.mImpactedByFloats = aImpactedByFloats;
        self.mTotalPlacedFrames = 0;
        if self.mBaseLineLayout.is_null() {
            self.mLineIsEmpty = true;
            self.mLineAtStart = true;
        } else {
            self.mLineIsEmpty = false;
            self.mLineAtStart = false;
        }
        self.mLineEndsInBR = false;
        self.mSpanDepth = 0;
        self.mMaxStartBoxBSize = 0;
        self.mMaxEndBoxBSize = 0;

        if self.mGotLineBox {
            self.mLineBox.ClearHasMarker();
        }

        let psd = self.NewPerSpanData();
        self.mRootSpan = psd;
        self.mCurrentSpan = psd;
        PSD(psd).mReflowInput = self.mLineContainerRI;
        PSD(psd).mIStart = aICoord;
        PSD(psd).mICoord = aICoord;
        PSD(psd).mIEnd = aICoord + aISize;
        // Set up inset to be used for text-wrap:balance implementation, but only if
        // the available size is greater than inset.
        PSD(psd).mInset = if aISize > aInset { aInset } else { 0 };
        self.mContainerSize = aContainerSize;

        self.mBStartEdge = aBCoord;

        PSD(psd).mNoWrap = !self.mStyleText.WhiteSpaceCanWrapStyle() || self.mSuppressLineWrap;
        PSD(psd).mWritingMode = aWritingMode;

        // Determine if this is the first line of the block (or first after a hard
        // line-break, if `each-line` is in effect).
        let containerFrame = self.LineContainerFrame();
        let textIndent = self.mStyleText.mTextIndent();
        if !mrt_style::style::Zero::is_zero(&textIndent.length) && ShouldApplyTextIndent(containerFrame) {
            let isFirstLineOrAfterHardBreak = (|| -> bool {
                if self.mLineNumber > 0 {
                    return textIndent.each_line && self.GetLine().is_some() && !self.GetLine().unwrap().prev().IsLineWrapped();
                }
                if let Some(prevBlock) = containerFrame.GetPrevInFlow() {
                    // NOT PORTED: nsBlockFrame (`prevBlock->Lines().empty() ||
                    // !prevBlock->LinesEnd().prev()->IsLineWrapped()`); the previous block is
                    // taken to end in a hard break.
                    return textIndent.each_line;
                }
                true
            })();

            // Resolve and apply the text-indent value if this line requires it.
            // The `hanging` option inverts which lines are to be indented.
            if isFirstLineOrAfterHardBreak != textIndent.hanging {
                let pctBasis: nscoord = self.LineContainerRI().ComputedISize();
                self.mTextIndent = textIndent.length.Resolve(pctBasis);
                PSD(psd).mICoord += self.mTextIndent;
            }
        }

        let pfd = self.NewPerFrameData(containerFrame);
        PFD(pfd).mAscent = 0;
        PFD(pfd).mSpan = psd;
        PSD(psd).mFrame = pfd;
        if containerFrame.IsRubyTextContainerFrame() {
            // Ruby text container won't be reflowed via ReflowFrame, hence the
            // relative positioning information should be recorded here.
            MOZ_ASSERT!(self.mBaseLineLayout != self as *mut nsLineLayout);
            PFD(pfd).mIsRelativelyOrStickyPos = self.LineContainerRI().mStyleDisplay.IsRelativelyOrStickyPositionedStyle();
            if PFD(pfd).mIsRelativelyOrStickyPos {
                MOZ_ASSERT!(self.LineContainerRI().GetWritingMode() == PFD(pfd).mWritingMode, "mLineContainerRI.frame == frame, hence they should have identical writing mode");
                PFD(pfd).mOffsets = self.LineContainerRI().ComputedLogicalOffsets(PFD(pfd).mWritingMode);
            }
        }
    }

    /**
     * Returns true if the line had to use an overflow-wrap break position.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::EndLineReflow
    pub fn EndLineReflow(&mut self) -> bool {
        #[cfg(debug_assertions)]
        NS_ASSERTION!(
            self.mBaseLineLayout.is_null() || (self.mSpansAllocated == 0 && self.mSpansFreed == 0 && self.mSpanFreeList.is_null() && self.mFramesAllocated == 0 && self.mFramesFreed == 0 && self.mFrameFreeList.is_null()),
            "Allocated frames or spans on non-base line layout?"
        );
        MOZ_ASSERT!(self.mRootSpan == self.mCurrentSpan);

        self.UnlinkFrame(PSD(self.mRootSpan).mFrame);
        self.mRootSpan = std::ptr::null_mut();
        self.mCurrentSpan = std::ptr::null_mut();

        #[cfg(debug_assertions)]
        {
            NS_ASSERTION!(self.mSpansAllocated == self.mSpansFreed, "leak");
            NS_ASSERTION!(self.mFramesAllocated == self.mFramesFreed, "leak");
        }

        // (the `#if 0` block that reports maxSpansAllocated is dead code in C++: left out)

        self.mUsedOverflowWrap
    }

    // XXX swtich to a single mAvailLineWidth that we adjust as each frame
    // on the line is placed. Each span can still have a per-span mICoord that
    // tracks where a child frame is going in its span; they don't need a
    // per-span mIStart?

    /**
     * Called when a float has been placed. This method updates the
     * inline frame and span data to account for any change in positions
     * due to available space for the line boxes changing.
     * @param aX/aY/aWidth/aHeight are the new available
     * space rectangle, relative to the containing block.
     * @param aFloatFrame the float frame that was placed.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::UpdateBand
    pub fn UpdateBand(&mut self, aWM: WritingMode, aNewAvailSpace: LogicalRect, aFloatFrame: nsIFrame) {
        let lineWM = PSD(self.mRootSpan).mWritingMode;
        // need to convert to our writing mode, because we might have a different
        // mode from the caller due to dir: auto
        let availSpace: LogicalRect = aNewAvailSpace.ConvertTo(lineWM, aWM, *self.ContainerSize());
        // NOT PORTED: the DEBUG-only ABSURD_SIZE checks on availSpace.

        // Compute the difference between last times width and the new width
        NS_WARNING_ASSERTION!(
            PSD(self.mRootSpan).mIEnd != NS_UNCONSTRAINEDSIZE && availSpace.ISize(lineWM) != NS_UNCONSTRAINEDSIZE,
            "have unconstrained inline size; this should only result from very large sizes, not attempts at intrinsic width calculation"
        );
        // The root span's mIStart moves to aICoord
        let deltaICoord: nscoord = availSpace.IStart(lineWM) - PSD(self.mRootSpan).mIStart;
        // The inline size of all spans changes by this much (the root span's
        // mIEnd moves to aICoord + aISize, its new inline size is aISize)
        let deltaISize: nscoord = availSpace.ISize(lineWM) - (PSD(self.mRootSpan).mIEnd - PSD(self.mRootSpan).mIStart);

        // Update the root span position
        PSD(self.mRootSpan).mIStart += deltaICoord;
        PSD(self.mRootSpan).mIEnd += deltaICoord;
        PSD(self.mRootSpan).mICoord += deltaICoord;

        // Now update the right edges of the open spans to account for any
        // change in available space width
        let mut psd = self.mCurrentSpan;
        while !psd.is_null() {
            PSD(psd).mIEnd += deltaISize;
            PSD(psd).mContainsFloat = true;
            psd = PSD(psd).mParent;
        }
        NS_ASSERTION!(
            PSD(self.mRootSpan).mContainsFloat && PSD(self.mRootSpan).mIStart == availSpace.IStart(lineWM) && PSD(self.mRootSpan).mIEnd == availSpace.IEnd(lineWM),
            "root span was updated incorrectly?"
        );

        // Update frame bounds
        // Note: Only adjust the outermost frames (the ones that are direct
        // children of the block), not the ones in the child spans. The reason
        // is simple: the frames in the spans have coordinates local to their
        // parent therefore they are moved when their parent span is moved.
        if deltaICoord != 0 {
            let mut pfd = PSD(self.mRootSpan).mFirstFrame;
            while !pfd.is_null() {
                *PFD(pfd).mBounds.IStart_mut(lineWM) += deltaICoord;
                pfd = PFD(pfd).mNext;
            }
        }

        self.mBStartEdge = availSpace.BStart(lineWM);
        self.mImpactedByFloats = true;

        self.mLastFloatWasLetterFrame = aFloatFrame.IsLetterFrame();
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::BeginSpan
    pub fn BeginSpan(&mut self, aFrame: nsIFrame, aSpanReflowInput: &ReflowInput, aIStart: nscoord, aIEnd: nscoord, aBaseline: *mut nscoord) {
        NS_ASSERTION!(aIEnd != NS_UNCONSTRAINEDSIZE, "should no longer be using unconstrained sizes");

        let psd = self.NewPerSpanData();
        // Link up span frame's pfd to point to its child span data
        let pfd = PSD(self.mCurrentSpan).mLastFrame;
        NS_ASSERTION!(PFD(pfd).mFrame == aFrame, "huh?");
        PFD(pfd).mSpan = psd;

        // Init new span
        PSD(psd).mFrame = pfd;
        PSD(psd).mParent = self.mCurrentSpan;
        PSD(psd).mReflowInput = unsafe { std::mem::transmute::<*const ReflowInput, *const ReflowInput<'static>>(aSpanReflowInput as *const ReflowInput) };
        PSD(psd).mIStart = aIStart;
        PSD(psd).mICoord = aIStart;
        PSD(psd).mIEnd = aIEnd;
        PSD(psd).mInset = 0; // inset applies only to the root span
        PSD(psd).mBaseline = aBaseline;

        let frame = aSpanReflowInput.mFrame;
        PSD(psd).mNoWrap = !nsStyleTextAdapter::of(frame).WhiteSpaceCanWrap(frame) || self.mSuppressLineWrap || frame.Style().ShouldSuppressLineBreak();
        PSD(psd).mWritingMode = aSpanReflowInput.GetWritingMode();

        // Switch to new span
        self.mCurrentSpan = psd;
        self.mSpanDepth += 1;
    }

    // Returns the width of the span
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::EndSpan
    pub fn EndSpan(&mut self, aFrame: nsIFrame) -> nscoord {
        NS_ASSERTION!(self.mSpanDepth > 0, "end-span without begin-span");
        let psd = self.mCurrentSpan;
        MOZ_ASSERT!(!PSD(psd).mParent.is_null(), "We never call this on the root");

        if PSD(psd).mNoWrap && !PSD(PSD(psd).mParent).mNoWrap {
            self.FlushNoWrapFloats();
        }

        let iSizeResult: nscoord = if !PSD(psd).mLastFrame.is_null() { PSD(psd).mICoord - PSD(psd).mIStart } else { 0 };

        self.mSpanDepth -= 1;
        PSD(self.mCurrentSpan).mReflowInput = std::ptr::null(); // no longer valid so null it out!
        self.mCurrentSpan = PSD(self.mCurrentSpan).mParent;
        iSizeResult
    }

    // This method attaches the last frame reflowed in this line layout
    // to that in the base line layout.
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::AttachLastFrameToBaseLineLayout
    pub fn AttachLastFrameToBaseLineLayout(&mut self) {
        let lastFrame = self.LastFrame();
        self.AttachFrameToBaseLineLayout(lastFrame);
    }

    // This method attaches the root frame of this line layout to the
    // last reflowed frame in the base line layout.
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::AttachRootFrameToBaseLineLayout
    pub fn AttachRootFrameToBaseLineLayout(&mut self) {
        let rootFrame = PSD(self.mRootSpan).mFrame;
        self.AttachFrameToBaseLineLayout(rootFrame);
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::AttachFrameToBaseLineLayout
    pub fn AttachFrameToBaseLineLayout(&mut self, aFrame: *mut nsLineLayout_PerFrameData) {
        MOZ_ASSERT!(!self.mBaseLineLayout.is_null(), "This method must not be called in a base line layout.");

        let baseFrame = unsafe { (*self.mBaseLineLayout).LastFrame() };
        MOZ_ASSERT!(!aFrame.is_null() && !baseFrame.is_null());
        MOZ_ASSERT!(!PFD(aFrame).mIsLinkedToBase, "The frame must not have been linked with the base");
        #[cfg(debug_assertions)]
        {
            let baseType = PFD(baseFrame).mFrame.Type();
            let annotationType = PFD(aFrame).mFrame.Type();
            MOZ_ASSERT!(
                (baseType == LayoutFrameType::RubyBaseContainer && annotationType == LayoutFrameType::RubyTextContainer) || (baseType == LayoutFrameType::RubyBase && annotationType == LayoutFrameType::RubyText)
            );
        }

        PFD(aFrame).mNextAnnotation = PFD(baseFrame).mNextAnnotation;
        PFD(baseFrame).mNextAnnotation = aFrame;
        PFD(aFrame).mIsLinkedToBase = true;
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::GetCurrentSpanCount
    pub fn GetCurrentSpanCount(&self) -> i32 {
        MOZ_ASSERT!(self.mCurrentSpan == self.mRootSpan, "bad linelayout user");
        let mut count: i32 = 0;
        let mut pfd = PSD(self.mRootSpan).mFirstFrame;
        while !pfd.is_null() {
            count += 1;
            pfd = PFD(pfd).mNext;
        }
        count
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::SplitLineTo
    pub fn SplitLineTo(&mut self, mut aNewCount: i32) {
        MOZ_ASSERT!(self.mCurrentSpan == self.mRootSpan, "bad linelayout user");

        let mut pfd = PSD(self.mRootSpan).mFirstFrame;
        while !pfd.is_null() {
            aNewCount -= 1;
            if aNewCount == 0 {
                // Truncate list at pfd (we keep pfd, but anything following is freed)
                let next = PFD(pfd).mNext;
                PFD(pfd).mNext = std::ptr::null_mut();
                PSD(self.mRootSpan).mLastFrame = pfd;

                // Now unlink all of the frames following pfd
                self.UnlinkFrame(next);
                break;
            }
            pfd = PFD(pfd).mNext;
        }
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::PushFrame
    pub fn PushFrame(&mut self, aFrame: nsIFrame) {
        let psd = self.mCurrentSpan;
        NS_ASSERTION!(PFD(PSD(psd).mLastFrame).mFrame == aFrame, "pushing non-last frame");

        // Take the last frame off of the span's frame list
        let pfd = PSD(psd).mLastFrame;
        if pfd == PSD(psd).mFirstFrame {
            // We are pushing away the only frame...empty the list
            PSD(psd).mFirstFrame = std::ptr::null_mut();
            PSD(psd).mLastFrame = std::ptr::null_mut();
        } else {
            let prevFrame = PFD(pfd).mPrev;
            PFD(prevFrame).mNext = std::ptr::null_mut();
            PSD(psd).mLastFrame = prevFrame;
        }

        // Now unlink the frame
        MOZ_ASSERT!(PFD(pfd).mNext.is_null());
        self.UnlinkFrame(pfd);
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::UnlinkFrame
    pub fn UnlinkFrame(&mut self, mut pfd: *mut nsLineLayout_PerFrameData) {
        while !pfd.is_null() {
            let next = PFD(pfd).mNext;
            if PFD(pfd).mIsLinkedToBase {
                // This frame is linked to a ruby base, and should not be freed
                // now. Just unlink it from the span. It will be freed when its
                // base frame gets unlinked.
                PFD(pfd).mNext = std::ptr::null_mut();
                PFD(pfd).mPrev = std::ptr::null_mut();
                pfd = next;
                continue;
            }

            // It is a ruby base frame. If there are any annotations
            // linked to this frame, free them first.
            let mut annotationPFD = PFD(pfd).mNextAnnotation;
            while !annotationPFD.is_null() {
                let nextAnnotation = PFD(annotationPFD).mNextAnnotation;
                MOZ_ASSERT!(PFD(annotationPFD).mNext.is_null() && PFD(annotationPFD).mPrev.is_null(), "PFD in annotations should have been unlinked.");
                self.FreeFrame(annotationPFD);
                annotationPFD = nextAnnotation;
            }

            self.FreeFrame(pfd);
            pfd = next;
        }
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::FreeFrame
    pub fn FreeFrame(&mut self, pfd: *mut nsLineLayout_PerFrameData) {
        if !PFD(pfd).mSpan.is_null() {
            self.FreeSpan(PFD(pfd).mSpan);
        }
        let outerLineLayout = self.GetOutermostLineLayout();
        let outer = unsafe { &mut *outerLineLayout };
        PFD(pfd).mNext = outer.mFrameFreeList;
        outer.mFrameFreeList = pfd;
        #[cfg(debug_assertions)]
        {
            outer.mFramesFreed += 1;
        }
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::FreeSpan
    pub fn FreeSpan(&mut self, psd: *mut nsLineLayout_PerSpanData) {
        // Unlink its frames
        self.UnlinkFrame(PSD(psd).mFirstFrame);

        let outerLineLayout = self.GetOutermostLineLayout();
        let outer = unsafe { &mut *outerLineLayout };
        // Now put the span on the free list since it's free too
        PSD(psd).mNextFreeSpan = outer.mSpanFreeList;
        outer.mSpanFreeList = psd;
        #[cfg(debug_assertions)]
        {
            outer.mSpansFreed += 1;
        }
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::IsZeroBSize
    pub fn IsZeroBSize(&self) -> bool {
        let mut pfd = PSD(self.mCurrentSpan).mFirstFrame;
        while !pfd.is_null() {
            if 0 != PFD(pfd).mBounds.BSize(PSD(self.mCurrentSpan).mWritingMode) {
                return false;
            }
            pfd = PFD(pfd).mNext;
        }
        true
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::NewPerFrameData
    pub fn NewPerFrameData(&mut self, aFrame: nsIFrame) -> *mut nsLineLayout_PerFrameData {
        let lineWM = PSD(self.mRootSpan).mWritingMode;
        let outerLineLayout = self.GetOutermostLineLayout();
        let outer = unsafe { &mut *outerLineLayout };
        // (all the members are written below, so a recycled one and a fresh one look the same;
        // C++ sets them one by one)
        let frameWM = aFrame.GetWritingMode();
        let fresh = nsLineLayout_PerFrameData {
            mNext: std::ptr::null_mut(),
            mPrev: std::ptr::null_mut(),
            mNextAnnotation: std::ptr::null_mut(),
            mSpan: std::ptr::null_mut(),
            mFrame: aFrame,
            mAscent: 0,
            mBounds: LogicalRect::new__WritingMode(lineWM),
            mOverflowAreas: OverflowAreas::default(),
            mMargin: LogicalMargin::new__WritingMode(lineWM),
            mBorderPadding: LogicalMargin::new__WritingMode(lineWM),
            mOffsets: LogicalMargin::new__WritingMode(frameWM),
            mJustificationInfo: JustificationInfo::new(),
            mJustificationAssignment: JustificationAssignment::new(),

            // all flags default to false
            mIsRelativelyOrStickyPos: false,
            mIsTextFrame: false,
            mIsNonEmptyTextFrame: false,
            mIsNonWhitespaceTextFrame: false,
            mIsLetterFrame: false,
            mRecomputeOverflow: false,
            mIsMarker: false,
            mSkipWhenTrimmingWhitespace: false,
            mIsEmpty: false,
            mIsPlaceholder: false,
            mIsLinkedToBase: false,

            #[cfg(debug_assertions)]
            mBlockDirAlign: 0xFF,
            #[cfg(not(debug_assertions))]
            mBlockDirAlign: 0,
            mWritingMode: frameWM,
        };
        let pfd: *mut nsLineLayout_PerFrameData;
        if outer.mFrameFreeList.is_null() {
            outer.mFrameArena.push(Box::new(fresh));
            pfd = &mut **outer.mFrameArena.last_mut().unwrap() as *mut nsLineLayout_PerFrameData;
        } else {
            pfd = outer.mFrameFreeList;
            outer.mFrameFreeList = PFD(pfd).mNext;
            unsafe {
                std::ptr::write(pfd, fresh);
            }
        }
        #[cfg(debug_assertions)]
        {
            outer.mFramesAllocated += 1;
        }
        pfd
    }
}

// Checks all four sides for percentage units.  This means it should
// only be used for things (margin, padding) where percentages on top
// and bottom depend on the *width* just like percentages on left and
// right.
// Gecko: layout/generic/nsLineLayout.cpp HasPercentageUnitSide
// (a template over StyleRect<T> in C++; the only use is the padding, so T is LengthPercentage)
fn HasPercentageUnitSide(aSides: &mrt_style::style::values::generics::rect::Rect<LengthPercentage>) -> bool {
    aSides.0.HasPercent() || aSides.1.HasPercent() || aSides.2.HasPercent() || aSides.3.HasPercent()
}

// Gecko: layout/generic/nsLineLayout.cpp HasPercentageUnitMargin
fn HasPercentageUnitMargin(aStyleMargin: &nsStyleMargin, aParams: &AnchorPosResolutionParams) -> bool {
    for side in AllPhysicalSides() {
        if aStyleMargin.GetMargin__Side_AnchorPosResolutionParams(side, aParams).HasPercent() {
            return true;
        }
    }
    false
}

// Gecko: layout/generic/nsLineLayout.cpp IsPercentageAware
fn IsPercentageAware(aFrame: nsIFrame, aWM: WritingMode) -> bool {
    let fType: LayoutFrameType = aFrame.Type();
    if fType == LayoutFrameType::Text {
        // None of these things can ever be true for text frames.
        return false;
    }

    // Some of these things don't apply to non-replaced inline frames
    // (that is, fType == LayoutFrameType::Inline), but we won't bother making
    // things unnecessarily complicated, since they'll probably be set
    // quite rarely.

    let margin = aFrame.StyleMargin();
    let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aFrame, std::ptr::null_mut());
    if HasPercentageUnitMargin(&margin, &anchorResolutionParams) {
        return true;
    }

    let padding = aFrame.StylePadding();
    if HasPercentageUnitSide(&padding.mPadding()) {
        return true;
    }

    // Note that borders can't be aware of percentages

    let pos = aFrame.StylePosition();
    let iSize = pos.ISize(aWM, &anchorResolutionParams);
    let anchorOffsetResolutionParams = AnchorPosOffsetResolutionParams::UseCBFrameSize(anchorResolutionParams);
    if (nsStylePosition::ISizeDependsOnContainer(&iSize) && !iSize.IsAuto())
        || nsStylePosition::MaxISizeDependsOnContainer(&pos.MaxISize(aWM, &anchorResolutionParams))
        || nsStylePosition::MinISizeDependsOnContainer(&pos.MinISize(aWM, &anchorResolutionParams))
        || pos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::IStart, aWM, &anchorOffsetResolutionParams).HasPercent()
        || pos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::IEnd, aWM, &anchorOffsetResolutionParams).HasPercent()
    {
        return true;
    }

    if iSize.IsAuto() {
        // We need to check for frames that shrink-wrap when they're auto
        // width.
        let disp = aFrame.StyleDisplay();
        if (disp.DisplayOutside() == StyleDisplayOutside::Inline && (disp.DisplayInside() == StyleDisplayInside::FlowRoot || disp.DisplayInside() == StyleDisplayInside::Table)) || fType == LayoutFrameType::FieldSet {
            return true;
        }

        // Per CSS 2.1, section 10.3.2:
        //   If 'height' and 'width' both have computed values of 'auto' and
        //   the element has an intrinsic ratio but no intrinsic height or
        //   width and the containing block's width does not itself depend
        //   on the replaced element's width, then the used value of 'width'
        //   is calculated from the constraint equation used for
        //   block-level, non-replaced elements in normal flow.
        let f = aFrame;
        if f.GetAspectRatio().operator_bool() &&
            // Some percents are treated like 'auto', so check != coord
            !pos.BSize(aWM, &anchorResolutionParams).ConvertsToLength()
        {
            let intrinsicSize = f.GetIntrinsicSize();
            if intrinsicSize.width.is_none() && intrinsicSize.height.is_none() {
                return true;
            }
        }
    }

    false
}

impl nsLineLayout {
    // Reflows the frame and returns the reflow status. aPushedFrame is true
    // if the frame is pushed to the next line because it doesn't fit.
    // (`ReflowOutput* aMetrics` may be null: `Option<&mut ReflowOutput>`)
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::ReflowFrame
    pub fn ReflowFrame(&mut self, aFrame: nsIFrame, aReflowStatus: &mut nsReflowStatus, aMetrics: Option<&mut ReflowOutput>, aPushedFrame: &mut bool) {
        // Initialize OUT parameter
        *aPushedFrame = false;

        let pfd = self.NewPerFrameData(aFrame);
        let psd = self.mCurrentSpan;
        PSD(psd).AppendFrame(pfd);

        if self.mCurrentSpan == self.mRootSpan {
            PFD(pfd).mFrame.RemoveProperty(nsIFrame::LineBaselineOffset());
        } else {
            #[cfg(debug_assertions)]
            {
                let hasLineOffset = PFD(pfd).mFrame.HasProperty(nsIFrame::LineBaselineOffset());
                NS_ASSERTION!(!hasLineOffset, "LineBaselineOffset was set but was not expected");
            }
        }

        self.mJustificationInfo = JustificationInfo::new();

        // Stash copies of some of the computed state away for later
        // (block-direction alignment, for example)
        let frameWM = PFD(pfd).mWritingMode;
        let lineWM = PSD(self.mRootSpan).mWritingMode;

        // NOTE: While the inline direction coordinate remains relative to the
        // parent span, the block direction coordinate is fixed at the top
        // edge for the line. During VerticalAlignFrames we will repair this
        // so that the block direction coordinate is properly set and relative
        // to the appropriate span.
        *PFD(pfd).mBounds.IStart_mut(lineWM) = PSD(psd).mICoord;
        *PFD(pfd).mBounds.BStart_mut(lineWM) = self.mBStartEdge;

        // We want to guarantee that we always make progress when
        // formatting. Therefore, if the object being placed on the line is
        // too big for the line, but it is the only thing on the line and is not
        // impacted by a float, then we go ahead and place it anyway. (If the line
        // is impacted by one or more floats, then it is safe to break because
        // we can move the line down below float(s).)
        //
        // Capture this state *before* we reflow the frame in case it clears
        // the state out. We need to know how to treat the current frame
        // when breaking.
        let notSafeToBreak: bool = self.LineIsEmpty() && !self.mImpactedByFloats;

        // Figure out whether we're talking about a textframe here
        let frameType: LayoutFrameType = aFrame.Type();
        let isText: bool = frameType == LayoutFrameType::Text;

        // Inline-ish and text-ish things don't compute their width;
        // everything else does.  We need to give them an available width that
        // reflects the space left on the line.
        NS_WARNING_ASSERTION!(PSD(psd).mIEnd != NS_UNCONSTRAINEDSIZE, "have unconstrained width; this should only result from very large sizes, not attempts at intrinsic width calculation");
        let availableSpaceOnLine: nscoord = PSD(psd).mIEnd - PSD(psd).mICoord - PSD(psd).mInset;

        // Setup reflow input for reflowing the frame
        let mut reflowInputHolder: Option<ReflowInput> = None;
        if !isText {
            // Compute the available size for the frame. This available width
            // includes room for the side margins.
            // For now, set the available block-size to unconstrained always.
            let mut availSize: LogicalSize = self.LineContainerRI().ComputedSize__WritingMode(frameWM);
            *availSize.BSize_mut(frameWM) = NS_UNCONSTRAINEDSIZE;
            let spanReflowInput: &ReflowInput = unsafe { &*PSD(psd).mReflowInput };
            reflowInputHolder = Some(ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                &self.mPresContext,
                spanReflowInput,
                aFrame,
                availSize,
                None,
                ReflowInput_InitFlags::default(),
                StyleSizeOverrides::default(),
                ComputeSizeFlags::default(),
                std::ptr::null_mut(),
            ));
            let reflowInput: &mut ReflowInput = reflowInputHolder.as_mut().unwrap();
            reflowInput.mLineLayout = self as *mut nsLineLayout;
            reflowInput.mFlags.mIsTopOfPage = self.mIsTopOfPage;
            if reflowInput.ComputedISize() == NS_UNCONSTRAINEDSIZE {
                reflowInput.SetAvailableISize(availableSpaceOnLine);
            }
            PFD(pfd).mMargin = reflowInput.ComputedLogicalMargin(lineWM);
            PFD(pfd).mBorderPadding = reflowInput.ComputedLogicalBorderPadding(lineWM);
            PFD(pfd).mIsRelativelyOrStickyPos = reflowInput.mStyleDisplay.IsRelativelyOrStickyPositionedStyle();
            if PFD(pfd).mIsRelativelyOrStickyPos {
                PFD(pfd).mOffsets = reflowInput.ComputedLogicalOffsets(frameWM);
            }

            // Calculate whether the the frame should have a start margin and
            // subtract the margin from the available width if necessary.
            // The margin will be applied to the starting inline coordinates of
            // the frame in CanPlaceFrame() after reflowing the frame.
            self.AllowForStartMargin(pfd, reflowInput);
        }
        // if isText(), no need to propagate NS_FRAME_IS_DIRTY from the parent,
        // because reflow doesn't look at the dirty bits on the frame being reflowed.

        // See if this frame depends on the inline-size of its containing block.
        // If so, disable resize reflow optimizations for the line.  (Note that,
        // to be conservative, we do this if we *try* to fit a frame on a
        // line, even if we don't succeed.)  (Note also that we can only make
        // this IsPercentageAware check *after* we've constructed our
        // ReflowInput, because that construction may be what forces aFrame
        // to lazily initialize its (possibly-percent-valued) intrinsic size.)
        if self.mGotLineBox && IsPercentageAware(aFrame, lineWM) {
            self.mLineBox.DisableResizeReflowOptimization();
        }

        // Note that we don't bother positioning the frame yet, because we're probably
        // going to end up moving it when we do the block-direction alignment.

        // Adjust float manager coordinate system for the frame.
        let mut reflowOutput = ReflowOutput::new__WritingMode(lineWM);
        #[cfg(debug_assertions)]
        {
            *reflowOutput.ISize_mut(lineWM) = 0xdeadbeef_u32 as nscoord;
            *reflowOutput.BSize_mut(lineWM) = 0xdeadbeef_u32 as nscoord;
        }
        let tI: nscoord = PFD(pfd).mBounds.LineLeft(lineWM, *self.ContainerSize());
        let tB: nscoord = PFD(pfd).mBounds.BStart(lineWM);
        unsafe { (*self.mFloatManager).Translate(tI, tB) };

        let mut savedOptionalBreakOffset: i32 = 0;
        let mut savedOptionalBreakPriority: gfxBreakPriority = gfxBreakPriority::eNoBreak;
        let savedOptionalBreakFrame: Option<nsIFrame> = self.GetLastOptionalBreakPosition(&mut savedOptionalBreakOffset, &mut savedOptionalBreakPriority);

        if !isText {
            aFrame.Reflow(&self.mPresContext, &mut reflowOutput, reflowInputHolder.as_ref().unwrap(), aReflowStatus);
        } else {
            // (the DrawTarget argument, `psd->mReflowInput->mRenderingContext->GetDrawTarget()`:
            // NOT PORTED, there is no drawing)
            aFrame.ReflowText(self, availableSpaceOnLine, &mut reflowOutput, aReflowStatus);
        }

        PFD(pfd).mJustificationInfo = self.mJustificationInfo;
        self.mJustificationInfo = JustificationInfo::new();

        // See if the frame is a placeholderFrame and if it is process
        // the float. At the same time, check if the frame has any non-collapsed-away
        // content.
        let mut placedFloat = false;
        let isEmpty: bool;
        if frameType == LayoutFrameType::None {
            isEmpty = PFD(pfd).mFrame.IsEmpty();
        } else if LayoutFrameType::Placeholder == frameType {
            isEmpty = true;
            PFD(pfd).mIsPlaceholder = true;
            PFD(pfd).mSkipWhenTrimmingWhitespace = true;
            let outOfFlowFrame = nsLayoutUtils::GetFloatFromPlaceholder(aFrame);
            if let Some(outOfFlowFrame) = outOfFlowFrame {
                if PSD(psd).mNoWrap &&
                    // We can always place floats in an empty line.
                    !self.LineIsEmpty() &&
                    // We always place floating letter frames. This kinda sucks. They'd
                    // usually fall into the LineIsEmpty() check anyway, except when
                    // there's something like a ::marker before or what not. We actually
                    // need to place them now, because they're pretty nasty and they
                    // create continuations that are in flow and not a kid of the
                    // previous continuation's parent. We don't want the deferred reflow
                    // of the letter frame to kill a continuation after we've stored it
                    // in the line layout data structures. See bug 1490281 to fix the
                    // underlying issue. When that's fixed this check should be removed.
                    !outOfFlowFrame.IsLetterFrame() &&
                    !unsafe { (*(*self.GetOutermostLineLayout()).mBlockRS).mFlags.mCanHaveOverflowMarkers }
                {
                    // We'll do this at the next break opportunity.
                    self.RecordNoWrapFloat(outOfFlowFrame);
                } else {
                    placedFloat = self.TryToPlaceFloat(outOfFlowFrame);
                }
            }
        } else if isText {
            // Note non-empty text-frames for inline frame compatibility hackery
            PFD(pfd).mIsTextFrame = true;
            let textFrame = PFD(pfd).mFrame;
            isEmpty = !textFrame.HasNoncollapsedCharacters();
            if !isEmpty {
                PFD(pfd).mIsNonEmptyTextFrame = true;
                PFD(pfd).mIsNonWhitespaceTextFrame = !textFrame.TextIsOnlyWhitespace();
            }
        } else if LayoutFrameType::Br == frameType {
            PFD(pfd).mSkipWhenTrimmingWhitespace = true;
            isEmpty = false;
        } else {
            if LayoutFrameType::Letter == frameType {
                PFD(pfd).mIsLetterFrame = true;
            }
            if !PFD(pfd).mSpan.is_null() {
                isEmpty = !PSD(PFD(pfd).mSpan).mHasNonemptyContent && PFD(pfd).mFrame.IsSelfEmpty();
            } else {
                isEmpty = PFD(pfd).mFrame.IsEmpty();
            }
        }
        PFD(pfd).mIsEmpty = isEmpty;

        unsafe { (*self.mFloatManager).Translate(-tI, -tB) };

        NS_ASSERTION!(reflowOutput.ISize(lineWM) >= 0, "bad inline size");
        NS_ASSERTION!(reflowOutput.BSize(lineWM) >= 0, "bad block size");
        if reflowOutput.ISize(lineWM) < 0 {
            *reflowOutput.ISize_mut(lineWM) = 0;
        }
        if reflowOutput.BSize(lineWM) < 0 {
            *reflowOutput.BSize_mut(lineWM) = 0;
        }

        // NOT PORTED: the DEBUG-only checks that print "didn't set w/h" and absurd metrics.

        // Unlike with non-inline reflow, the overflow area here does *not*
        // include the accumulation of the frame's bounds and its inline
        // descendants' bounds. Nor does it include the outline area; it's
        // just the union of the bounds of any absolute children. That is
        // added in later by nsLineLayout::ReflowInlineFrames.
        PFD(pfd).mOverflowAreas = reflowOutput.mOverflowAreas;

        *PFD(pfd).mBounds.ISize_mut(lineWM) = reflowOutput.ISize(lineWM);
        *PFD(pfd).mBounds.BSize_mut(lineWM) = reflowOutput.BSize(lineWM);

        // Size the frame, but |RelativePositionFrames| will size the view.
        aFrame.SetRect__WritingMode_LogicalRect_nsSize(lineWM, PFD(pfd).mBounds, self.ContainerSizeForSpan(psd));

        // Tell the frame that we're done reflowing it
        aFrame.DidReflow(&self.mPresContext, if isText { None } else { reflowInputHolder.as_ref() });

        if let Some(aMetrics) = aMetrics {
            *aMetrics = reflowOutput.clone();
        }

        if !aReflowStatus.IsInlineBreakBefore() {
            // If frame is complete and has a next-in-flow, we need to delete
            // them now. Do not do this when a break-before is signaled because
            // the frame is going to get reflowed again (and may end up wanting
            // a next-in-flow where it ends up).
            if aReflowStatus.IsComplete() {
                if let Some(kidNextInFlow) = aFrame.GetNextInFlow() {
                    // Remove all of the childs next-in-flows. Make sure that we ask
                    // the right parent to do the removal (it's possible that the
                    // parent is not this because we are executing pullup code)
                    let mut context = FrameDestroyContext::default();
                    kidNextInFlow.GetParent().unwrap().DeleteNextInFlowChild(&mut context, kidNextInFlow, true);
                }
            }

            // Check whether this frame breaks up text runs. All frames break up text
            // runs (hence return false here) except for text frames and inline
            // containers.
            let continuingTextRun: bool = aFrame.CanContinueTextRun();

            // Clear any residual mTrimmableISize if this isn't a text frame
            if !continuingTextRun && !PFD(pfd).mSkipWhenTrimmingWhitespace {
                self.mTrimmableISize = 0;
            }

            // See if we can place the frame. If we can't fit it, then we
            // return now.
            let mut optionalBreakAfterFits: bool = false;
            NS_ASSERTION!(isText || !reflowInputHolder.as_ref().unwrap().mStyleDisplay.IsFloating(reflowInputHolder.as_ref().unwrap().mFrame), "How'd we get a floated inline frame? The frame ctor should've dealt with this.");
            if self.CanPlaceFrame(pfd, notSafeToBreak, continuingTextRun, savedOptionalBreakFrame.is_some(), &mut reflowOutput, aReflowStatus, &mut optionalBreakAfterFits) {
                if !isEmpty {
                    PSD(psd).mHasNonemptyContent = true;
                    self.mLineIsEmpty = false;
                    if PFD(pfd).mSpan.is_null() {
                        // nonempty leaf content has been placed
                        self.mLineAtStart = false;
                    }
                    if LayoutFrameType::Ruby == frameType {
                        self.mHasRuby = true;
                        self.SyncAnnotationBounds(pfd);
                    }
                }

                // Place the frame, updating aBounds with the final size and
                // location.  Then apply the bottom+right margins (as
                // appropriate) to the frame.
                self.PlaceFrame(pfd, &mut reflowOutput);
                let span = PFD(pfd).mSpan;
                if !span.is_null() {
                    // The frame we just finished reflowing is an inline
                    // container.  It needs its child frames aligned in the block direction,
                    // so do most of it now.
                    self.VerticalAlignFrames(span);
                }

                if !continuingTextRun && !PSD(psd).mNoWrap {
                    if !self.LineIsEmpty() || placedFloat {
                        // record soft break opportunity after this content that can't be
                        // part of a text run. This is not a text frame so we know
                        // that offset INT32_MAX means "after the content".
                        if (!aFrame.IsPlaceholderFrame() || self.LineIsEmpty()) && self.NotifyOptionalBreakPosition(Some(aFrame), i32::MAX, optionalBreakAfterFits, gfxBreakPriority::eNormalBreak) {
                            // If this returns true then we are being told to actually break
                            // here.
                            aReflowStatus.SetInlineLineBreakAfter(UsedClear::None);
                        }
                    }
                }
            } else {
                self.PushFrame(aFrame);
                *aPushedFrame = true;
                // Undo any saved break positions that the frame might have told us about,
                // since we didn't end up placing it
                self.RestoreSavedBreakPosition(savedOptionalBreakFrame, savedOptionalBreakOffset, savedOptionalBreakPriority);
            }
        } else {
            self.PushFrame(aFrame);
            *aPushedFrame = true;
        }
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::AllowForStartMargin
    pub fn AllowForStartMargin(&mut self, pfd: *mut nsLineLayout_PerFrameData, aReflowInput: &mut ReflowInput) {
        NS_ASSERTION!(!aReflowInput.mStyleDisplay.IsFloating(aReflowInput.mFrame), "How'd we get a floated inline frame? The frame ctor should've dealt with this.");

        let lineWM = PSD(self.mRootSpan).mWritingMode;

        // Only apply start-margin on the first-in flow for inline frames,
        // and make sure to not apply it to any inline other than the first
        // in an ib split.  Note that the ib sibling (block-in-inline
        // sibling) annotations only live on the first continuation, but we
        // don't want to apply the start margin for later continuations
        // anyway.  For box-decoration-break:clone we apply the start-margin
        // on all continuations.
        if (PFD(pfd).mFrame.GetPrevContinuation().is_some() || PFD(pfd).mFrame.FrameIsNonFirstInIBSplit()) && aReflowInput.mStyleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Slice {
            // Zero this out so that when we compute the max-element-width of
            // the frame we will properly avoid adding in the starting margin.
            *PFD(pfd).mMargin.IStart_mut(lineWM) = 0;
        } else if NS_UNCONSTRAINEDSIZE == aReflowInput.ComputedISize() {
            NS_WARNING_ASSERTION!(
                NS_UNCONSTRAINEDSIZE != aReflowInput.AvailableISize(),
                "have unconstrained inline-size; this should only result from very large sizes, not attempts at intrinsic inline-size calculation"
            );
            // For inline-ish and text-ish things (which don't compute widths
            // in the reflow input), adjust available inline-size to account
            // for the start margin. The end margin will be accounted for when
            // we finish flowing the frame.
            let wm = aReflowInput.GetWritingMode();
            let newAvailableISize = aReflowInput.AvailableISize() - PFD(pfd).mMargin.ConvertTo(wm, lineWM).IStart(wm);
            aReflowInput.SetAvailableISize(newAvailableISize);
        }
    }

    /**
     * Returns the accumulated advance width of frames before the current frame
     * on the line, plus the line container's left border+padding.
     * This is always positive, the advance width is measured from
     * the right edge for RTL blocks and from the left edge for LTR blocks.
     * In other words, the current frame's distance from the line container's
     * start content edge is:
     * <code>GetCurrentFrameInlineDistanceFromBlock() -
     * lineContainer->GetUsedBorderAndPadding().left</code> Note the use of
     * <code>.left</code> for both LTR and RTL line containers.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::GetCurrentFrameInlineDistanceFromBlock
    pub fn GetCurrentFrameInlineDistanceFromBlock(&self) -> nscoord {
        let mut x: nscoord = 0;
        let mut psd = self.mCurrentSpan;
        while !psd.is_null() {
            x += PSD(psd).mICoord;
            psd = PSD(psd).mParent;
        }
        x
    }

    /**
     * This method syncs bounds of ruby annotations and ruby annotation
     * containers from their rect. It is necessary because:
     * Containers are not part of the line in their levels, which means
     * their bounds are not set properly before.
     * Ruby annotations' position may have been changed when reflowing
     * their containers.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::SyncAnnotationBounds
    // NOT PORTED: ruby (nsRubyFrame; mHasRuby is never set because no ruby frame is reflowed).
    // The C++ body is nsLineLayout.cpp lines 1134-1163 at GECKO_REV.
    pub fn SyncAnnotationBounds(&mut self, aRubyFrame: *mut nsLineLayout_PerFrameData) {
        unimplemented!("NOT PORTED: ruby annotations (SyncAnnotationBounds)");
    }

    /**
     * See if the frame can be placed now that we know it's desired size.
     * We can always place the frame if the line is empty. Note that we
     * know that the reflow-status is not a break-before because if it was
     * ReflowFrame above would have returned false, preventing this method
     * from being called. The logic in this method assumes that.
     *
     * Note that there is no check against the Y coordinate because we
     * assume that the caller will take care of that.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::CanPlaceFrame
    pub fn CanPlaceFrame(&mut self, pfd: *mut nsLineLayout_PerFrameData, aNotSafeToBreak: bool, aFrameCanContinueTextRun: bool, aCanRollBackBeforeFrame: bool, aMetrics: &mut ReflowOutput, aStatus: &mut nsReflowStatus, aOptionalBreakAfterFits: &mut bool) -> bool {
        MOZ_ASSERT!(!pfd.is_null(), "bad args, null pointers for frame data");

        *aOptionalBreakAfterFits = true;

        let lineWM = PSD(self.mRootSpan).mWritingMode;
        /*
         * We want to only apply the end margin if we're the last continuation and
         * either not in an {ib} split or the last inline in it.  In all other
         * cases we want to zero it out.  That means zeroing it out if any of these
         * conditions hold:
         * 1) The frame is not complete (in this case it will get a next-in-flow)
         * 2) The frame is complete but has a non-fluid continuation on its
         *    continuation chain.  Note that if it has a fluid continuation, that
         *    continuation will get destroyed later, so we don't want to drop the
         *    end-margin in that case.
         * 3) The frame is in an {ib} split and is not the last part.
         *
         * However, none of that applies if this is a letter frame (XXXbz why?)
         *
         * For box-decoration-break:clone we apply the end margin on all
         * continuations (that are not letter frames).
         */
        if (aStatus.IsIncomplete() || PFD(pfd).mFrame.LastInFlow().GetNextContinuation().is_some() || PFD(pfd).mFrame.FrameIsNonLastInIBSplit())
            && !PFD(pfd).mIsLetterFrame
            && PFD(pfd).mFrame.StyleBorder().mBoxDecorationBreak() == StyleBoxDecorationBreak::Slice
        {
            *PFD(pfd).mMargin.IEnd_mut(lineWM) = 0;
        }

        // Apply the start margin to the frame bounds.
        let startMargin: nscoord = PFD(pfd).mMargin.IStart(lineWM);
        let endMargin: nscoord = PFD(pfd).mMargin.IEnd(lineWM);

        *PFD(pfd).mBounds.IStart_mut(lineWM) += startMargin;

        let psd = self.mCurrentSpan;
        if PSD(psd).mNoWrap {
            // When wrapping is off, everything fits.
            return true;
        }

        // Set outside to true if the result of the reflow leads to the
        // frame sticking outside of our available area.
        let outside: bool = PFD(pfd).mBounds.IEnd(lineWM) - self.mTrimmableISize + endMargin > PSD(psd).mIEnd;
        if !outside {
            // If it fits, it fits
            return true;
        }
        *aOptionalBreakAfterFits = false;

        // When it doesn't fit, check for a few special conditions where we
        // allow it to fit anyway.
        if 0 == startMargin + PFD(pfd).mBounds.ISize(lineWM) + endMargin {
            // Empty frames always fit right where they are
            return true;
        }

        // another special case:  always place a BR
        if PFD(pfd).mFrame.IsBrFrame() {
            return true;
        }

        if aNotSafeToBreak {
            // There are no frames on the line that take up width and the line is
            // not impacted by floats, so we must allow the current frame to be
            // placed on the line
            return true;
        }

        // Special check for span frames
        if !PFD(pfd).mSpan.is_null() && PSD(PFD(pfd).mSpan).mContainsFloat {
            // If the span either directly or indirectly contains a float then
            // it fits. Why? It's kind of complicated, but here goes:
            //
            // 1. CanPlaceFrame is used for all frame placements on a line,
            // and in a span. This includes recursively placement of frames
            // inside of spans, and the span itself. Because the logic always
            // checks for room before proceeding (the code above here), the
            // only things on a line will be those things that "fit".
            //
            // 2. Before a float is placed on a line, the line has to be empty
            // (otherwise it's a "below current line" float and will be placed
            // after the line).
            //
            // Therefore, if the span directly or indirectly has a float
            // then it means that at the time of the placement of the float
            // the line was empty. Because of #1, only the frames that fit can
            // be added after that point, therefore we can assume that the
            // current span being placed has fit.
            //
            // So how do we get here and have a span that should already fit
            // and yet doesn't: Simple: span's that have the no-wrap attribute
            // set on them and contain a float and are placed where they
            // don't naturally fit.
            return true;
        }

        if aFrameCanContinueTextRun {
            // Let it fit, but we reserve the right to roll back.
            // Note that we usually won't get here because a text frame will break
            // itself to avoid exceeding the available width.
            // We'll only get here for text frames that couldn't break early enough.

            // We will want to try backup.
            self.mNeedBackup = true;
            return true;
        }

        aStatus.SetInlineLineBreakBeforeAndReset();
        false
    }

    /**
     * Place the frame. Update running counters.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::PlaceFrame
    pub fn PlaceFrame(&mut self, pfd: *mut nsLineLayout_PerFrameData, aMetrics: &mut ReflowOutput) {
        let lineWM = PSD(self.mRootSpan).mWritingMode;

        // If the frame's block direction does not match the line's, we can't use
        // its ascent; instead, treat it as a block with baseline at the block-end
        // edge (or block-begin in the case of an "inverted" line).
        if PFD(pfd).mWritingMode.GetBlockDir() != lineWM.GetBlockDir() {
            PFD(pfd).mAscent = if lineWM.IsAlphabeticalBaseline() {
                if lineWM.IsLineInverted() {
                    0
                } else {
                    aMetrics.BSize(lineWM)
                }
            } else {
                aMetrics.BSize(lineWM) / 2
            };
        } else {
            // For inline reflow participants, baseline may get assigned as the frame is
            // vertically aligned, which happens after this.
            let baselineSource = PFD(pfd).mFrame.BaselineSource();
            if baselineSource == StyleBaselineSource::Auto || PFD(pfd).mFrame.IsLineParticipant() {
                if aMetrics.BlockStartAscent() == ReflowOutput::ASK_FOR_BASELINE {
                    PFD(pfd).mAscent = PFD(pfd).mFrame.GetLogicalBaseline__WritingMode(lineWM);
                } else {
                    PFD(pfd).mAscent = aMetrics.BlockStartAscent();
                }
            } else {
                let sourceGroup = (|| {
                    match baselineSource {
                        StyleBaselineSource::First => return BaselineSharingGroup::First,
                        StyleBaselineSource::Last => return BaselineSharingGroup::Last,
                        StyleBaselineSource::Auto => {}
                    }
                    MOZ_ASSERT_UNREACHABLE!("Auto should be already handled?");
                    BaselineSharingGroup::First
                })();
                // We ignore line-layout specific layout quirks by setting
                // `BaselineExportContext::Other`.
                // Note(dshin): For a lot of frames, the export context does not make a
                // difference, and we may be wasting the value cached in
                // `BlockStartAscent`.
                PFD(pfd).mAscent = PFD(pfd).mFrame.GetLogicalBaseline__WritingMode_BaselineSharingGroup_BaselineExportContext(lineWM, sourceGroup, BaselineExportContext::Other);
            }
        }

        // Advance to next inline coordinate
        PSD(self.mCurrentSpan).mICoord = PFD(pfd).mBounds.IEnd(lineWM) + PFD(pfd).mMargin.IEnd(lineWM);

        // Count the number of non-placeholder frames on the line...
        if PFD(pfd).mFrame.IsPlaceholderFrame() {
            NS_ASSERTION!(
                PFD(pfd).mBounds.ISize(lineWM) == 0 && PFD(pfd).mBounds.BSize(lineWM) == 0,
                "placeholders should have 0 width/height (checking placeholders were never counted by the old code in this function)"
            );
        } else {
            self.mTotalPlacedFrames += 1;
        }
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::AddMarkerFrame
    pub fn AddMarkerFrame(&mut self, aFrame: nsIFrame, aMetrics: &ReflowOutput) {
        MOZ_ASSERT!(self.mCurrentSpan == self.mRootSpan, "bad linelayout user");
        MOZ_ASSERT!(self.mGotLineBox, "must have line box");

        // (`nsBlockFrame* blockFrame = do_QueryFrame(LineContainerFrame())`: NOT PORTED, nsBlockFrame)
        let blockFrame = self.LineContainerFrame();
        MOZ_ASSERT!(blockFrame.IsBlockFrameOrSubclass(), "must be for block");
        if !blockFrame.MarkerIsEmpty(aFrame) {
            self.mLineIsEmpty = false;
            self.mHasMarker = true;
            self.mLineBox.SetHasMarker();
        }

        let lineWM = PSD(self.mRootSpan).mWritingMode;
        let pfd = self.NewPerFrameData(aFrame);
        let psd = self.mRootSpan;

        MOZ_ASSERT!(!PSD(psd).mFirstFrame.is_null(), "adding marker to an empty line?");
        // Prepend the marker frame to the line.
        PFD(PSD(psd).mFirstFrame).mPrev = pfd;
        PFD(pfd).mNext = PSD(psd).mFirstFrame;
        PSD(psd).mFirstFrame = pfd;

        PFD(pfd).mIsMarker = true;
        if aMetrics.BlockStartAscent() == ReflowOutput::ASK_FOR_BASELINE {
            PFD(pfd).mAscent = aFrame.GetLogicalBaseline__WritingMode(lineWM);
        } else {
            PFD(pfd).mAscent = aMetrics.BlockStartAscent();
        }

        // Note: block-coord value will be updated during block-direction alignment
        PFD(pfd).mBounds = LogicalRect::new__WritingMode_nsRect_nsSize(lineWM, aFrame.GetRect(), *self.ContainerSize());
        PFD(pfd).mOverflowAreas = aMetrics.mOverflowAreas;
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::RemoveMarkerFrame
    pub fn RemoveMarkerFrame(&mut self, aFrame: nsIFrame) {
        let psd = self.mCurrentSpan;
        MOZ_ASSERT!(psd == self.mRootSpan, "::marker on non-root span?");
        MOZ_ASSERT!(PFD(PSD(psd).mFirstFrame).mFrame == aFrame, "::marker is not the first frame?");
        let pfd = PSD(psd).mFirstFrame;
        MOZ_ASSERT!(pfd != PSD(psd).mLastFrame, "::marker is the only frame?");
        PFD(PFD(pfd).mNext).mPrev = std::ptr::null_mut();
        PSD(psd).mFirstFrame = PFD(pfd).mNext;
        self.FreeFrame(pfd);
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::DumpPerSpanData
    // NOT PORTED: debug-only printing (nsIFrame::IndentBy, ListTag).
    #[cfg(debug_assertions)]
    pub fn DumpPerSpanData(&self, psd: *mut nsLineLayout_PerSpanData, aIndent: i32) {
        unimplemented!("NOT PORTED: DumpPerSpanData prints with printf and ListTag");
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::RecordNoWrapFloat
    pub fn RecordNoWrapFloat(&mut self, aFloat: nsIFrame) {
        unsafe { (*(*self.GetOutermostLineLayout()).mBlockRS).mNoWrapFloats.push(aFloat) };
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::FlushNoWrapFloats
    pub fn FlushNoWrapFloats(&mut self) {
        let noWrapFloats: Vec<nsIFrame> = unsafe { std::mem::take(&mut (*(*self.GetOutermostLineLayout()).mBlockRS).mNoWrapFloats) };
        for floatedFrame in noWrapFloats {
            self.TryToPlaceFloat(floatedFrame);
        }
        // noWrapFloats.Clear(): the list was taken, and TryToPlaceFloat does not add to it
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::TryToPlaceFloat
    pub fn TryToPlaceFloat(&mut self, aFloat: nsIFrame) -> bool {
        // Add mTrimmableISize to the available width since if the line ends here, the
        // width of the inline content will be reduced by mTrimmableISize.
        let availableISize: nscoord = PSD(self.mCurrentSpan).mIEnd - (PSD(self.mCurrentSpan).mICoord - self.mTrimmableISize);
        NS_ASSERTION!(!(aFloat.IsLetterFrame() && self.GetFirstLetterStyleOK()), "FirstLetterStyle set on line with floating first letter");
        let outer = self.GetOutermostLineLayout();
        unsafe { (*outer).AddFloat(aFloat, availableISize) }
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::NotifyOptionalBreakPosition
    pub fn NotifyOptionalBreakPosition(&mut self, aFrame: Option<nsIFrame>, aOffset: i32, aFits: bool, aPriority: gfxBreakPriority) -> bool {
        NS_ASSERTION!(!aFits || !self.mNeedBackup, "Shouldn't be updating the break position with a break that fits after we've already flagged an overrun");
        MOZ_ASSERT!(!self.mCurrentSpan.is_null(), "Should be doing line layout");
        if PSD(self.mCurrentSpan).mNoWrap {
            self.FlushNoWrapFloats();
        }

        // Remember the last break position that fits; if there was no break that fit,
        // just remember the first break
        if (aFits && aPriority >= self.mLastOptionalBreakPriority) || self.mLastOptionalBreakFrame.is_none() {
            self.mLastOptionalBreakFrame = aFrame;
            self.mLastOptionalBreakFrameOffset = aOffset;
            self.mLastOptionalBreakPriority = aPriority;
        }
        aFrame.is_some() && self.mForceBreakFrame == aFrame && self.mForceBreakFrameOffset == aOffset
    }
}

// Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::SetSpanForEmptyLine
impl nsLineLayout {
    pub fn SetSpanForEmptyLine(aPerSpanData: *mut nsLineLayout_PerSpanData, aWM: WritingMode, aContainerSize: nsSize, aBStartEdge: nscoord) {
        let mut pfd = PSD(aPerSpanData).mFirstFrame;
        while !pfd.is_null() {
            // Ideally, if the frame would collapse itself - but it depends on
            // knowing that the line is empty.
            if !PFD(pfd).mFrame.IsInlineFrame() && !PFD(pfd).mFrame.IsRubyFrame() && !PFD(pfd).mFrame.IsPlaceholderFrame() {
                pfd = PFD(pfd).mNext;
                continue;
            }
            // Collapse the physical size to 0.
            *PFD(pfd).mBounds.BStart_mut(aWM) = aBStartEdge;
            *PFD(pfd).mBounds.BSize_mut(aWM) = 0;
            // Initialize mBlockDirAlign (though it doesn't make much difference
            // because we don't align empty boxes).
            PFD(pfd).mBlockDirAlign = VALIGN_OTHER;
            PFD(pfd).mFrame.SetRect__WritingMode_LogicalRect_nsSize(aWM, PFD(pfd).mBounds, aContainerSize);
            if !PFD(pfd).mSpan.is_null() {
                // For child spans, the block-start edge is relative to that of parent,
                // which is zero (since it is empty). See NOTE in
                // `nsLineLayout::ReflowFrame`.
                nsLineLayout::SetSpanForEmptyLine(PFD(pfd).mSpan, aWM, aContainerSize, 0);
            }
            pfd = PFD(pfd).mNext;
        }
    }

    /**
     * Place frames in the block direction (CSS property vertical-align)
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::VerticalAlignLine
    pub fn VerticalAlignLine(&mut self) {
        // Partially place the children of the block frame. The baseline for
        // this operation is set to zero so that the y coordinates for all
        // of the placed children will be relative to there.
        let psd = self.mRootSpan;
        if self.mLineIsEmpty {
            // This line is empty, and should be consisting of only inline elements.
            // (inline-block elements would make the line non-empty).
            nsLineLayout::SetSpanForEmptyLine(psd, PSD(self.mRootSpan).mWritingMode, *self.ContainerSize(), self.mBStartEdge);

            self.mFinalLineBSize = 0;
            if self.mGotLineBox {
                let containerSize = *self.ContainerSize();
                self.mLineBox.SetBounds(PSD(psd).mWritingMode, PSD(psd).mIStart, self.mBStartEdge, PSD(psd).mICoord - PSD(psd).mIStart, 0, containerSize);

                self.mLineBox.SetLogicalAscent(0);
            }
            return;
        }
        self.VerticalAlignFrames(psd);

        // *** Note that comments here still use the anachronistic term
        // "line-height" when we really mean "size of the line in the block
        // direction", "vertical-align" when we really mean "alignment in
        // the block direction", and "top" and "bottom" when we really mean
        // "block start" and "block end". This is partly for brevity and
        // partly to retain the association with the CSS line-height and
        // vertical-align properties.
        //
        // Compute the line-height. The line-height will be the larger of:
        //
        // [1] maxBCoord - minBCoord (the distance between the first child's
        // block-start edge and the last child's block-end edge)
        //
        // [2] the maximum logical box block size (since not every frame may have
        // participated in #1; for example: "top" and "botttom" aligned frames)
        //
        // [3] the minimum line height ("line-height" property set on the
        // block frame)
        let mut lineBSize: nscoord = PSD(psd).mMaxBCoord - PSD(psd).mMinBCoord;

        // Now that the line-height is computed, we need to know where the
        // baseline is in the line. Position baseline so that mMinBCoord is just
        // inside the start of the line box.
        let mut baselineBCoord: nscoord = self.mBStartEdge - std::cmp::min(0, PSD(psd).mMinBCoord);

        // It's also possible that the line block-size isn't tall enough because
        // of "top" and "bottom" aligned elements that were not accounted for in
        // min/max BCoord.
        //
        // The CSS2 spec doesn't really say what happens when to the
        // baseline in this situations. What we do is if the largest start
        // aligned box block size is greater than the line block-size then we leave
        // the baseline alone. If the largest end aligned box is greater
        // than the line block-size then we slide the baseline forward by the extra
        // amount.
        //
        // Navigator 4 gives precedence to the first top/bottom aligned
        // object.  We just let block end aligned objects win.
        if lineBSize < self.mMaxEndBoxBSize {
            // When the line is shorter than the maximum block start aligned box
            let extra: nscoord = self.mMaxEndBoxBSize - lineBSize;
            baselineBCoord += extra;
            lineBSize = self.mMaxEndBoxBSize;
        }
        lineBSize = std::cmp::max(lineBSize, self.mMaxStartBoxBSize);

        // Now position all of the frames in the root span. We will also
        // recurse over the child spans and place any frames we find with
        // vertical-align: top/bottom/center.
        // XXX PERFORMANCE: set a bit per-span to avoid the extra work
        // (propagate it upward too)
        let lineWM = PSD(psd).mWritingMode;
        let mut pfd = PSD(psd).mFirstFrame;
        while !pfd.is_null() {
            if PFD(pfd).mBlockDirAlign == VALIGN_OTHER {
                *PFD(pfd).mBounds.BStart_mut(lineWM) += baselineBCoord;
                PFD(pfd).mFrame.SetRect__WritingMode_LogicalRect_nsSize(lineWM, PFD(pfd).mBounds, *self.ContainerSize());
            }
            pfd = PFD(pfd).mNext;
        }
        self.PlaceTopBottomCenterFrames(psd, -self.mBStartEdge, lineBSize);

        self.mFinalLineBSize = lineBSize;
        if self.mGotLineBox {
            // Fill in returned line-box and max-element-width data
            let containerSize = *self.ContainerSize();
            self.mLineBox.SetBounds(lineWM, PSD(psd).mIStart, self.mBStartEdge, PSD(psd).mICoord - PSD(psd).mIStart, lineBSize, containerSize);

            self.mLineBox.SetLogicalAscent(baselineBCoord - self.mBStartEdge);
        }
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::ComputeTopAlignFrameStart
    pub fn ComputeTopAlignFrameStart(&self, pfd: *const nsLineLayout_PerFrameData, aWM: WritingMode, aDistanceFromStart: nscoord, aLineBSize: nscoord) -> nscoord {
        let span = PFD(pfd as *mut nsLineLayout_PerFrameData).mSpan;
        if !span.is_null() {
            -aDistanceFromStart - PSD(span).mMinBCoord
        } else {
            -aDistanceFromStart + PFD(pfd as *mut nsLineLayout_PerFrameData).mMargin.BStart(aWM)
        }
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::ComputeBottomAlignFrameStart
    pub fn ComputeBottomAlignFrameStart(&self, pfd: *const nsLineLayout_PerFrameData, aWM: WritingMode, aDistanceFromStart: nscoord, aLineBSize: nscoord) -> nscoord {
        let span = PFD(pfd as *mut nsLineLayout_PerFrameData).mSpan;
        if !span.is_null() {
            // Compute bottom leading
            -aDistanceFromStart + aLineBSize - PSD(span).mMaxBCoord
        } else {
            let pfd = PFD(pfd as *mut nsLineLayout_PerFrameData);
            -aDistanceFromStart + aLineBSize - pfd.mMargin.BEnd(aWM) - pfd.mBounds.BSize(aWM)
        }
    }

    // Place frames with CSS property vertical-align: top/bottom/center.
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::PlaceTopBottomCenterFrames
    pub fn PlaceTopBottomCenterFrames(&mut self, psd: *mut nsLineLayout_PerSpanData, aDistanceFromStart: nscoord, aLineBSize: nscoord) {
        let mut pfd = PSD(psd).mFirstFrame;
        while !pfd.is_null() {
            let span = PFD(pfd).mSpan;
            #[cfg(debug_assertions)]
            NS_ASSERTION!(0xFF != PFD(pfd).mBlockDirAlign, "umr");
            let lineWM = PSD(self.mRootSpan).mWritingMode;
            let containerSize: nsSize = self.ContainerSizeForSpan(psd);
            match PFD(pfd).mBlockDirAlign {
                VALIGN_TOP => {
                    *PFD(pfd).mBounds.BStart_mut(lineWM) = self.ComputeTopAlignFrameStart(pfd, lineWM, aDistanceFromStart, aLineBSize);
                    PFD(pfd).mFrame.SetRect__WritingMode_LogicalRect_nsSize(lineWM, PFD(pfd).mBounds, containerSize);
                }
                VALIGN_BOTTOM => {
                    *PFD(pfd).mBounds.BStart_mut(lineWM) = self.ComputeBottomAlignFrameStart(pfd, lineWM, aDistanceFromStart, aLineBSize);
                    PFD(pfd).mFrame.SetRect__WritingMode_LogicalRect_nsSize(lineWM, PFD(pfd).mBounds, containerSize);
                }
                VALIGN_CENTER => {
                    let startTop: nscoord = self.ComputeTopAlignFrameStart(pfd, lineWM, aDistanceFromStart, aLineBSize);
                    let startBottom: nscoord = self.ComputeBottomAlignFrameStart(pfd, lineWM, aDistanceFromStart, aLineBSize);
                    *PFD(pfd).mBounds.BStart_mut(lineWM) = (startTop + startBottom) / 2;
                    PFD(pfd).mFrame.SetRect__WritingMode_LogicalRect_nsSize(lineWM, PFD(pfd).mBounds, containerSize);
                }
                _ => {}
            }
            if !span.is_null() {
                let fromStart: nscoord = aDistanceFromStart + PFD(pfd).mBounds.BStart(lineWM);
                self.PlaceTopBottomCenterFrames(span, fromStart, aLineBSize);
            }
            pfd = PFD(pfd).mNext;
        }
    }
}

// Gecko: layout/generic/nsLineLayout.cpp GetBSizeOfEmphasisMarks
fn GetBSizeOfEmphasisMarks(aSpanFrame: nsIFrame, aInflation: f32) -> nscoord {
    let fm: nsFontMetrics = nsLayoutUtils::GetFontMetricsOfEmphasisMarks(aSpanFrame, aInflation);
    // (`PresContext()->NormalizeRubyMetrics() ? (TrimmedAscent + TrimmedDescent) * RubyPositioningFactor : MaxHeight`:
    // NOT PORTED, ruby metrics; NormalizeRubyMetrics() is false)
    fm.MaxHeight()
}

impl nsLineLayout {
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::AdjustLeadings
    pub fn AdjustLeadings(&mut self, spanFrame: nsIFrame, psd: *mut nsLineLayout_PerSpanData, aStyleText: &nsStyleTextAdapter, aInflation: f32, aZeroEffectiveSpanBox: &mut bool) {
        MOZ_ASSERT!(spanFrame == PFD(PSD(psd).mFrame).mFrame);
        let mut requiredStartLeading: nscoord = 0;
        let mut requiredEndLeading: nscoord = 0;
        if spanFrame.IsRubyFrame() {
            // We may need to extend leadings here for ruby annotations as
            // required by section Line Spacing in the CSS Ruby spec.
            // See http://dev.w3.org/csswg/css-ruby/#line-height
            // NOT PORTED: ruby (`rubyFrame->GetBlockLeadings()`, RubyBlockLeadings).
            unimplemented!("NOT PORTED: ruby block leadings");
        }
        if aStyleText.HasEffectiveTextEmphasis() {
            let bsize: nscoord = GetBSizeOfEmphasisMarks(spanFrame, aInflation);
            let side: LogicalSide = aStyleText.TextEmphasisSide(PSD(self.mRootSpan).mWritingMode, spanFrame.StyleFont().mLanguage());
            if spanFrame.PresContext().NormalizeRubyMetrics() {
                // Add extra leading for emphasis marks only if their bsize exceeds the
                // space built in to the font (difference between its max ascent/descent
                // and the em-normalized metrics that are used to position the mark).
                // NOT PORTED: ruby-normalized metrics (TrimmedAscent, TrimmedDescent); the
                // condition is never true.
                unimplemented!("NOT PORTED: ruby-normalized font metrics");
            } else if side == LogicalSide::BStart {
                requiredStartLeading += bsize;
            } else {
                MOZ_ASSERT!(side == LogicalSide::BEnd, "emphasis marks must be in block axis");
                requiredEndLeading += bsize;
            }
        }

        let requiredLeading: nscoord = requiredStartLeading + requiredEndLeading;
        // If we do not require any additional leadings, don't touch anything
        // here even if it is greater than the original leading, because the
        // latter could be negative.
        if requiredLeading != 0 {
            let leading: nscoord = PSD(psd).mBStartLeading + PSD(psd).mBEndLeading;
            let deltaLeading: nscoord = requiredLeading - leading;
            if deltaLeading > 0 {
                // If the total leading is not wide enough for ruby annotations
                // and/or emphasis marks, extend the side which is not enough. If
                // both sides are not wide enough, replace the leadings with the
                // requested values.
                if requiredStartLeading < PSD(psd).mBStartLeading {
                    PSD(psd).mBEndLeading += deltaLeading;
                } else if requiredEndLeading < PSD(psd).mBEndLeading {
                    PSD(psd).mBStartLeading += deltaLeading;
                } else {
                    PSD(psd).mBStartLeading = requiredStartLeading;
                    PSD(psd).mBEndLeading = requiredEndLeading;
                }
                PSD(psd).mLogicalBSize += deltaLeading;
                // We have adjusted the leadings, it is no longer a zero
                // effective span box.
                *aZeroEffectiveSpanBox = false;
            }
        }
    }
}

// Gecko: layout/generic/nsLineLayout.cpp GetInflationForBlockDirAlignment
fn GetInflationForBlockDirAlignment(aFrame: nsIFrame, aInflationMinFontSize: nscoord) -> f32 {
    if aFrame.IsInSVGTextSubtree() {
        // NOT PORTED: SVG text (`SVGTextFrame::GetFontSizeScaleFactor`); the frames of SVG text
        // are not translated.
        unimplemented!("NOT PORTED: SVG text font size scale factor");
    }
    nsLayoutUtils::FontSizeInflationInner(aFrame, aInflationMinFontSize)
}

impl nsLineLayout {
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::ShouldApplyLineHeightInPreserveWhiteSpace
    fn ShouldApplyLineHeightInPreserveWhiteSpace(psd: *const nsLineLayout_PerSpanData) -> bool {
        let psd = PSD(psd as *mut nsLineLayout_PerSpanData);
        // (`Style()->IsAnonBox()`: a frame without content is an anonymous box here, see
        // nsIFrame::IsReplaced)
        if PFD(psd.mFrame).mFrame.GetContent().is_none() {
            // e.g. An empty `input[type=button]` should still be line-height sized.
            return true;
        }

        let mut pfd = psd.mFirstFrame;
        while !pfd.is_null() {
            if !PFD(pfd).mIsEmpty {
                return true;
            }
            pfd = PFD(pfd).mNext;
        }
        false
    }

    // Place frames in the block direction within a given span (CSS property
    // vertical-align) Note: this doesn't place frames with vertical-align:
    // top/bottom/center as those have to wait until the entire line box block
    // size is known. This is called after the span frame has finished being
    // reflowed so that we know its block size.
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::VerticalAlignFrames
    pub fn VerticalAlignFrames(&mut self, psd: *mut nsLineLayout_PerSpanData) {
        // Get parent frame info
        let spanFramePFD = PSD(psd).mFrame;
        let spanFrame = PFD(spanFramePFD).mFrame;

        // Get the parent frame's font for all of the frames in this span
        let inflation: f32 = GetInflationForBlockDirAlignment(spanFrame, self.mInflationMinFontSize);
        let fm: nsFontMetrics = nsLayoutUtils::GetFontMetricsForFrame(spanFrame, inflation);

        let preMode: bool = self.mStyleText.WhiteSpaceIsSignificant();

        // See if the span is an empty continuation. It's an empty continuation iff:
        // - it has a prev-in-flow
        // - it has no next in flow
        // - it's zero sized
        let lineWM = PSD(self.mRootSpan).mWritingMode;
        let emptyContinuation: bool = psd != self.mRootSpan && spanFrame.GetPrevInFlow().is_some() && spanFrame.GetNextInFlow().is_none() && PFD(spanFramePFD).mBounds.IsZeroSize();

        // Compute the span's zeroEffectiveSpanBox flag. What we are trying
        // to determine is how we should treat the span: should it act
        // "normally" according to css2 or should it effectively
        // "disappear".
        //
        // In general, if the document being processed is in full standards
        // mode then it should act normally (with one exception). The
        // exception case is when a span is continued and yet the span is
        // empty (e.g. compressed whitespace). For this kind of span we treat
        // it as if it were not there so that it doesn't impact the
        // line block-size.
        //
        // In almost standards mode or quirks mode, we should sometimes make
        // it disappear. The cases that matter are those where the span
        // contains no real text elements that would provide an ascent and
        // descent and height. However, if css style elements have been
        // applied to the span (border/padding/margin) so that it's clear the
        // document author is intending css2 behavior then we act as if strict
        // mode is set.
        //
        // This code works correctly for preMode, because a blank line
        // in PRE mode is encoded as a text node with a LF in it, since
        // text nodes with only whitespace are considered in preMode.
        //
        // Much of this logic is shared with the various implementations of
        // nsIFrame::IsEmpty since they need to duplicate the way it makes
        // some lines empty.  However, nsIFrame::IsEmpty can't be reused here
        // since this code sets zeroEffectiveSpanBox even when there are
        // non-empty children.
        let mut zeroEffectiveSpanBox: bool = false;
        // XXXldb If we really have empty continuations, then all these other
        // checks don't make sense for them.
        // XXXldb This should probably just use nsIFrame::IsSelfEmpty, assuming that
        // it agrees with this code.  (If it doesn't agree, it probably should.)
        if (emptyContinuation || self.mPresContext.CompatibilityMode() != eCompatibility_FullStandards) && ((psd == self.mRootSpan) || (PFD(spanFramePFD).mBorderPadding.IsAllZero() && PFD(spanFramePFD).mMargin.IsAllZero())) {
            // This code handles an issue with compatibility with non-css
            // conformant browsers. In particular, there are some cases
            // where the font-size and line-height for a span must be
            // ignored and instead the span must *act* as if it were zero
            // sized. In general, if the span contains any non-compressed
            // text then we don't use this logic.
            // However, this is not propagated outwards, since (in compatibility
            // mode) we don't want big line heights for things like
            // <p><font size="-1">Text</font></p>

            // We shouldn't include any whitespace that collapses, unless we're
            // preformatted (in which case it shouldn't, but the width=0 test is
            // perhaps incorrect).  This includes whitespace at the beginning of
            // a line and whitespace preceded (?) by other whitespace.
            // See bug 134580 and bug 155333.
            zeroEffectiveSpanBox = true;
            let mut pfd = PSD(psd).mFirstFrame;
            while !pfd.is_null() {
                if PFD(pfd).mIsTextFrame && (PFD(pfd).mIsNonWhitespaceTextFrame || preMode || PFD(pfd).mBounds.ISize(PSD(self.mRootSpan).mWritingMode) != 0) {
                    zeroEffectiveSpanBox = false;
                    break;
                }
                pfd = PFD(pfd).mNext;
            }
        }

        // Setup baselineBCoord, minBCoord, and maxBCoord
        let mut baselineBCoord: nscoord;
        let mut minBCoord: nscoord;
        let mut maxBCoord: nscoord;
        if psd == self.mRootSpan {
            // Use a zero baselineBCoord since we don't yet know where the baseline
            // will be (until we know how tall the line is; then we will
            // know). In addition, use extreme values for the minBCoord and maxBCoord
            // values so that only the child frames will impact their values
            // (since these are children of the block, there is no span box to
            // provide initial values).
            baselineBCoord = 0;
            minBCoord = BLOCKDIR_ALIGN_FRAMES_NO_MINIMUM;
            maxBCoord = BLOCKDIR_ALIGN_FRAMES_NO_MAXIMUM;
        } else {
            // Compute the logical block size for this span. The logical block size
            // is based on the "line-height" value, not the font-size. Also
            // compute the top leading.
            let inflation: f32 = GetInflationForBlockDirAlignment(spanFrame, self.mInflationMinFontSize);
            let mut logicalBSize: nscoord = ReflowInput::CalcLineHeight__ComputedStyle_nsPresContext_nsIContent_nscoord_float(&spanFrame.Style(), Some(&spanFrame.PresContext()), spanFrame.GetContent().as_ref(), self.LineContainerRI().ComputedHeight(), inflation);
            let contentBSize: nscoord = PFD(spanFramePFD).mBounds.BSize(lineWM) - PFD(spanFramePFD).mBorderPadding.BStartEnd(lineWM);

            // Special-case for a ::first-letter frame, set the line height to
            // the frame block size if the user has left line-height == normal
            if PFD(spanFramePFD).mIsLetterFrame && spanFrame.GetPrevInFlow().is_none() && spanFrame.StyleFont().mLineHeight().IsNormal() {
                logicalBSize = PFD(spanFramePFD).mBounds.BSize(lineWM);
            }

            let leading: nscoord = logicalBSize - contentBSize;
            PSD(psd).mBStartLeading = leading / 2;
            PSD(psd).mBEndLeading = leading - PSD(psd).mBStartLeading;
            PSD(psd).mLogicalBSize = logicalBSize;
            self.AdjustLeadings(spanFrame, psd, &nsStyleTextAdapter::of(spanFrame), inflation, &mut zeroEffectiveSpanBox);

            if zeroEffectiveSpanBox {
                // When the span-box is to be ignored, zero out the initial
                // values so that the span doesn't impact the final line
                // height. The contents of the span can impact the final line
                // height.

                // Note that things are readjusted for this span after its children
                // are reflowed
                minBCoord = BLOCKDIR_ALIGN_FRAMES_NO_MINIMUM;
                maxBCoord = BLOCKDIR_ALIGN_FRAMES_NO_MAXIMUM;
            } else {
                // The initial values for the min and max block coord values are in the
                // span's coordinate space, and cover the logical block size of the span.
                // If there are child frames in this span that stick out of this area
                // then the minBCoord and maxBCoord are updated by the amount of logical
                // blockSize that is outside this range.
                minBCoord = PFD(spanFramePFD).mBorderPadding.BStart(lineWM) - PSD(psd).mBStartLeading;
                maxBCoord = minBCoord + PSD(psd).mLogicalBSize;
            }

            // This is the distance from the top edge of the parents visual
            // box to the baseline. The span already computed this for us,
            // so just use it.
            baselineBCoord = PFD(spanFramePFD).mAscent;
            unsafe { *PSD(psd).mBaseline = baselineBCoord };
        }

        let mut maxStartBoxBSize: nscoord = 0;
        let mut maxEndBoxBSize: nscoord = 0;
        let mut pfd = PSD(psd).mFirstFrame;
        while !pfd.is_null() {
            let frame = PFD(pfd).mFrame;

            // Compute the logical block size of the frame
            let mut logicalBSize: nscoord;
            let frameSpan = PFD(pfd).mSpan;
            if !frameSpan.is_null() {
                // For span frames the logical-block-size and start-leading were
                // pre-computed when the span was reflowed.
                logicalBSize = PSD(frameSpan).mLogicalBSize;
            } else {
                // For other elements the logical block size is the same as the
                // frame's block size plus its margins.
                logicalBSize = PFD(pfd).mBounds.BSize(lineWM) + PFD(pfd).mMargin.BStartEnd(lineWM);
                if logicalBSize < 0 && self.mPresContext.CompatibilityMode() != eCompatibility_FullStandards {
                    PFD(pfd).mAscent -= logicalBSize;
                    logicalBSize = 0;
                }
            }

            // Get block-direction alignment values. For text frames, don't perform any
            // special baseline alignment; their inline container box is responsible for
            // performing baseline alignment.
            let mut alignmentBaseline: StyleAlignmentBaseline = StyleAlignmentBaseline::Baseline;
            if !PFD(pfd).mIsTextFrame {
                alignmentBaseline = frame.AlignmentBaseline();
            }

            let baselineShift: StyleBaselineShift = frame.BaselineShift();
            let mut baselineShiftEnum: Option<StyleBaselineShiftKeyword> = if baselineShift.IsKeyword() { Some(baselineShift.AsKeyword()) } else { None };

            // Apply writing mode transforms to the alignment properties
            if lineWM.IsVertical() {
                // For vertical writing mode where the dominant baseline is centered
                // (i.e. text-orientation is not sideways-*), we remap 'middle' to
                // 'middle-with-baseline' so that images align sensibly with the
                // center-baseline-aligned text.
                if alignmentBaseline == StyleAlignmentBaseline::Middle && !lineWM.IsSideways() {
                    alignmentBaseline = StyleAlignmentBaseline::MozMiddleWithBaseline;
                }

                // Swap the meanings of top and bottom when line is inverted
                // relative to block direction.
                if lineWM.IsLineInverted() {
                    match alignmentBaseline {
                        StyleAlignmentBaseline::TextTop => alignmentBaseline = StyleAlignmentBaseline::TextBottom,
                        StyleAlignmentBaseline::TextBottom => alignmentBaseline = StyleAlignmentBaseline::TextTop,
                        _ => {}
                    }

                    if let Some(kw) = baselineShiftEnum {
                        match kw {
                            StyleBaselineShiftKeyword::Top => baselineShiftEnum = Some(StyleBaselineShiftKeyword::Bottom),
                            StyleBaselineShiftKeyword::Bottom => baselineShiftEnum = Some(StyleBaselineShiftKeyword::Top),
                            _ => {}
                        }
                    }
                }
            }

            // (a lambda in C++: `fm` is its capture; here a nested fn that takes it as a parameter)
            fn GetFontBaseline(aFM: &nsFontMetrics, aAlignmentBaseline: StyleAlignmentBaseline) -> nscoord {
                match aAlignmentBaseline {
                    StyleAlignmentBaseline::Alphabetic => aFM.AlphabeticBaseline(),
                    StyleAlignmentBaseline::Central => aFM.CentralBaseline(),
                    StyleAlignmentBaseline::Ideographic => aFM.IdeographicUnderBaseline(),
                    StyleAlignmentBaseline::Mathematical => aFM.MathBaseline(),
                    StyleAlignmentBaseline::Hanging => aFM.HangingBaseline(),
                    _ => {
                        MOZ_ASSERT_UNREACHABLE!("Unexpected alignment baseline");
                        0
                    }
                }
            }

            // Adjust the bounds according to the given baseline.
            match alignmentBaseline {
                StyleAlignmentBaseline::Baseline => {
                    *PFD(pfd).mBounds.BStart_mut(lineWM) = baselineBCoord - PFD(pfd).mAscent;
                    PFD(pfd).mBlockDirAlign = VALIGN_OTHER;
                }

                StyleAlignmentBaseline::Alphabetic | StyleAlignmentBaseline::Central | StyleAlignmentBaseline::Ideographic | StyleAlignmentBaseline::Mathematical | StyleAlignmentBaseline::Hanging => {
                    let parentBaseline: nscoord = GetFontBaseline(&fm, alignmentBaseline) * lineWM.FlowRelativeToLineRelativeFactor();
                    *PFD(pfd).mBounds.BStart_mut(lineWM) = baselineBCoord - parentBaseline - PFD(pfd).mAscent;
                    // For child span frames, additional adjustment is required to align
                    // child's requested font baseline to the parent's font baseline.
                    // Non-span child frames already have the requested baseline's offset
                    // baked into their ascents, so no additional adjustment is required.
                    // See Baseline::SynthesizeBOffsetFromInnerBox.
                    if !frameSpan.is_null() {
                        let spanFm: nsFontMetrics = nsLayoutUtils::GetInflatedFontMetricsForFrame(frame);
                        let selfBaseline: nscoord = GetFontBaseline(&spanFm, alignmentBaseline) * lineWM.FlowRelativeToLineRelativeFactor();
                        *PFD(pfd).mBounds.BStart_mut(lineWM) += selfBaseline;
                    }
                    PFD(pfd).mBlockDirAlign = VALIGN_OTHER;
                }

                StyleAlignmentBaseline::Middle => {
                    // Align the midpoint of the frame with 1/2 the parents
                    // x-height above the baseline.
                    //
                    // TODO(bug 2030203): The css-inline-3 definition of `middle` doesn't
                    // match this existing CSS2 implementation. Per css-inline-3, this
                    // should align the x-middle baselines of the child and parent (like
                    // `alphabetic`/etc. above). But CSS2 aligned the midpoint of the child
                    // with the x-middle baseline of the parent, which can differ.
                    let parentXHeight: nscoord = lineWM.FlowRelativeToLineRelativeFactor() * fm.XHeight();
                    if !frameSpan.is_null() {
                        *PFD(pfd).mBounds.BStart_mut(lineWM) = baselineBCoord - (parentXHeight + PFD(pfd).mBounds.BSize(lineWM)) / 2;
                    } else {
                        *PFD(pfd).mBounds.BStart_mut(lineWM) = baselineBCoord - (parentXHeight + logicalBSize) / 2 + PFD(pfd).mMargin.BStart(lineWM);
                    }
                    PFD(pfd).mBlockDirAlign = VALIGN_OTHER;
                }

                StyleAlignmentBaseline::TextTop => {
                    // The top of the logical box is aligned with the top of
                    // the parent element's text.
                    // XXX For vertical text we will need a new API to get the logical
                    //     max-ascent here
                    let parentAscent: nscoord = if lineWM.IsLineInverted() { fm.MaxDescent() } else { fm.MaxAscent() };
                    if !frameSpan.is_null() {
                        *PFD(pfd).mBounds.BStart_mut(lineWM) = baselineBCoord - parentAscent - PFD(pfd).mBorderPadding.BStart(lineWM) + PSD(frameSpan).mBStartLeading;
                    } else {
                        *PFD(pfd).mBounds.BStart_mut(lineWM) = baselineBCoord - parentAscent + PFD(pfd).mMargin.BStart(lineWM);
                    }
                    PFD(pfd).mBlockDirAlign = VALIGN_OTHER;
                }

                StyleAlignmentBaseline::TextBottom => {
                    // The bottom of the logical box is aligned with the
                    // bottom of the parent elements text.
                    let parentDescent: nscoord = if lineWM.IsLineInverted() { fm.MaxAscent() } else { fm.MaxDescent() };
                    if !frameSpan.is_null() {
                        *PFD(pfd).mBounds.BStart_mut(lineWM) = baselineBCoord + parentDescent - PFD(pfd).mBounds.BSize(lineWM) + PFD(pfd).mBorderPadding.BEnd(lineWM) - PSD(frameSpan).mBEndLeading;
                    } else {
                        *PFD(pfd).mBounds.BStart_mut(lineWM) = baselineBCoord + parentDescent - PFD(pfd).mBounds.BSize(lineWM) - PFD(pfd).mMargin.BEnd(lineWM);
                    }
                    PFD(pfd).mBlockDirAlign = VALIGN_OTHER;
                }

                StyleAlignmentBaseline::MozMiddleWithBaseline => {
                    // Align the midpoint of the frame with the baseline of the parent.
                    if !frameSpan.is_null() {
                        *PFD(pfd).mBounds.BStart_mut(lineWM) = baselineBCoord - PFD(pfd).mBounds.BSize(lineWM) / 2;
                    } else {
                        *PFD(pfd).mBounds.BStart_mut(lineWM) = baselineBCoord - logicalBSize / 2 + PFD(pfd).mMargin.BStart(lineWM);
                    }
                    PFD(pfd).mBlockDirAlign = VALIGN_OTHER;
                }
            }

            // Shift from the baseline
            if let Some(kw) = baselineShiftEnum {
                match kw {
                    StyleBaselineShiftKeyword::Sub | StyleBaselineShiftKeyword::Super => {
                        *PFD(pfd).mBounds.BStart_mut(lineWM) += lineWM.FlowRelativeToLineRelativeFactor() * (if kw == StyleBaselineShiftKeyword::Sub { fm.SubscriptOffset() } else { -fm.SuperscriptOffset() });
                    }

                    StyleBaselineShiftKeyword::Top => {
                        PFD(pfd).mBlockDirAlign = VALIGN_TOP;
                        let mut subtreeBSize: nscoord = logicalBSize;
                        if !frameSpan.is_null() {
                            subtreeBSize = PSD(frameSpan).mMaxBCoord - PSD(frameSpan).mMinBCoord;
                            NS_ASSERTION!(subtreeBSize >= logicalBSize, "unexpected subtree block size");
                        }
                        if subtreeBSize > maxStartBoxBSize {
                            maxStartBoxBSize = subtreeBSize;
                        }
                    }

                    StyleBaselineShiftKeyword::Bottom => {
                        PFD(pfd).mBlockDirAlign = VALIGN_BOTTOM;
                        let mut subtreeBSize: nscoord = logicalBSize;
                        if !frameSpan.is_null() {
                            subtreeBSize = PSD(frameSpan).mMaxBCoord - PSD(frameSpan).mMinBCoord;
                            NS_ASSERTION!(subtreeBSize >= logicalBSize, "unexpected subtree block size");
                        }
                        if subtreeBSize > maxEndBoxBSize {
                            maxEndBoxBSize = subtreeBSize;
                        }
                    }

                    StyleBaselineShiftKeyword::Center => {
                        PFD(pfd).mBlockDirAlign = VALIGN_CENTER;
                        let mut subtreeBSize: nscoord = logicalBSize;
                        if !frameSpan.is_null() {
                            subtreeBSize = PSD(frameSpan).mMaxBCoord - PSD(frameSpan).mMinBCoord;
                            NS_ASSERTION!(subtreeBSize >= logicalBSize, "unexpected subtree block size");
                        }
                        if subtreeBSize > maxStartBoxBSize {
                            maxStartBoxBSize = subtreeBSize;
                        }
                        if subtreeBSize > maxEndBoxBSize {
                            maxEndBoxBSize = subtreeBSize;
                        }
                    }
                }
            } else {
                // We have either a coord, a percent, or a calc().
                // (C++ passes a lambda to Resolve that computes the percentage basis only
                // when there is a percentage; the same here.)
                let shiftLength: LengthPercentage = baselineShift.AsLength();
                let offset: nscoord = if shiftLength.HasPercent() {
                    // Percentages are like lengths, except treated as a percentage
                    // of the elements line block size value.
                    let inflation: f32 = GetInflationForBlockDirAlignment(frame, self.mInflationMinFontSize);
                    shiftLength.Resolve(ReflowInput::CalcLineHeight__ComputedStyle_nsPresContext_nsIContent_nscoord_float(&frame.Style(), Some(&frame.PresContext()), frame.GetContent().as_ref(), self.LineContainerRI().ComputedBSize(), inflation))
                } else {
                    shiftLength.Resolve(0)
                };

                // According to the CSS2 spec (10.8.1), a positive value
                // "raises" the box by the given distance while a negative value
                // "lowers" the box by the given distance (with zero being the
                // baseline). Since Y coordinates increase towards the bottom of
                // the screen we reverse the sign, unless the line orientation is
                // inverted relative to block direction.
                *PFD(pfd).mBounds.BStart_mut(lineWM) += -1 * offset * lineWM.FlowRelativeToLineRelativeFactor();
            }

            // Update minBCoord/maxBCoord for frames that we just placed. Do not factor
            // text into the equation.
            if PFD(pfd).mBlockDirAlign == VALIGN_OTHER {
                // Text frames do not contribute to the min/max Y values for the
                // line (instead their parent frame's font-size contributes).
                // XXXrbs -- relax this restriction because it causes text frames
                //           to jam together when 'font-size-adjust' is enabled
                //           and layout is using dynamic font heights (bug 20394)
                //        -- Note #1: With this code enabled and with the fact that we are
                //           not using Em[Ascent|Descent] as nsDimensions for text
                //           metrics in GFX mean that the discussion in bug 13072 cannot
                //           hold.
                //        -- Note #2: We still don't want empty-text frames to interfere.
                //           For example in quirks mode, avoiding empty text frames
                //           prevents "tall" lines around elements like <hr> since the
                //           rules of <hr> in quirks.css have pseudo text contents with LF
                //           in them.
                let canUpdate: bool;
                if PFD(pfd).mIsTextFrame {
                    // Only consider text frames if they're not empty and
                    // line-height=normal.
                    canUpdate = PFD(pfd).mIsNonWhitespaceTextFrame && frame.StyleFont().mLineHeight().IsNormal();
                } else {
                    canUpdate = !PFD(pfd).mIsPlaceholder;
                }

                if canUpdate {
                    let mut blockStart: nscoord;
                    let mut blockEnd: nscoord;
                    if !frameSpan.is_null() {
                        // For spans that were are now placing, use their position
                        // plus their already computed min-Y and max-Y values for
                        // computing blockStart and blockEnd.
                        blockStart = PFD(pfd).mBounds.BStart(lineWM) + PSD(frameSpan).mMinBCoord;
                        blockEnd = PFD(pfd).mBounds.BStart(lineWM) + PSD(frameSpan).mMaxBCoord;
                    } else {
                        blockStart = PFD(pfd).mBounds.BStart(lineWM) - PFD(pfd).mMargin.BStart(lineWM);
                        blockEnd = blockStart + logicalBSize;
                    }
                    if !preMode && self.mPresContext.CompatibilityMode() != eCompatibility_FullStandards && logicalBSize == 0 {
                        // Check if it's a BR frame that is not alone on its line (it
                        // is given a block size of zero to indicate this), and if so reset
                        // blockStart and blockEnd so that BR frames don't influence the line.
                        if frame.IsBrFrame() {
                            blockStart = BLOCKDIR_ALIGN_FRAMES_NO_MINIMUM;
                            blockEnd = BLOCKDIR_ALIGN_FRAMES_NO_MAXIMUM;
                        }
                    }
                    if blockStart < minBCoord {
                        minBCoord = blockStart;
                    }
                    if blockEnd > maxBCoord {
                        maxBCoord = blockEnd;
                    }
                }
                if psd != self.mRootSpan {
                    frame.SetRect__WritingMode_LogicalRect_nsSize(lineWM, PFD(pfd).mBounds, self.ContainerSizeForSpan(psd));
                }
            }
            pfd = PFD(pfd).mNext;
        }

        // Factor in the minimum line block-size when handling the root-span for
        // the block.
        if psd == self.mRootSpan {
            // We should factor in the block element's minimum line-height (as
            // defined in section 10.8.1 of the css2 spec) assuming that
            // zeroEffectiveSpanBox is not set on the root span.  This only happens
            // in some cases in quirks mode:
            //  (1) if the root span contains non-whitespace text directly (this
            //      is handled by zeroEffectiveSpanBox
            //  (2) if this line has a ::marker
            //  (3) if this is the last line of an LI, DT, or DD element
            //      (The last line before a block also counts, but not before a
            //      BR) (NN4/IE5 quirk)

            // (1) and (2) above
            let mut applyMinLH: bool = !zeroEffectiveSpanBox || self.mHasMarker;
            let isLastLine: bool = !self.mGotLineBox || (!self.mLineBox.IsLineWrapped() && !self.mLineEndsInBR);
            if !applyMinLH && isLastLine {
                let blockContent = PFD(PSD(self.mRootSpan).mFrame).mFrame.GetContent();
                if let Some(blockContent) = blockContent {
                    // (3) above, if the last line of LI, DT, or DD
                    if blockContent.IsAnyOfHTMLElements(&["li", "dt", "dd"] /* nsGkAtoms::li, dt, dd */) {
                        applyMinLH = true;
                    }
                }
            }
            if applyMinLH {
                if PSD(psd).mHasNonemptyContent || (preMode && nsLineLayout::ShouldApplyLineHeightInPreserveWhiteSpace(psd)) || self.mHasMarker {
                    let minimumLineBSize: nscoord = self.mMinLineBSize;
                    let mut blockStart: nscoord = -nsLayoutUtils::GetCenteredFontBaseline(&fm, minimumLineBSize, lineWM.IsLineInverted());
                    let mut blockEnd: nscoord = blockStart + minimumLineBSize;

                    if self.mStyleText.HasEffectiveTextEmphasis() {
                        // (`mPresContext->NormalizeRubyMetrics() ? RubyPositioningFactor * (TrimmedAscent + TrimmedDescent)
                        // : MaxHeight`: the ruby branch is NOT PORTED, NormalizeRubyMetrics() is false)
                        let fontMaxHeight: nscoord = fm.MaxHeight();
                        let emphasisHeight: nscoord = GetBSizeOfEmphasisMarks(spanFrame, inflation);
                        let mut delta: nscoord = fontMaxHeight + emphasisHeight - minimumLineBSize;
                        if delta > 0 {
                            if minimumLineBSize < fontMaxHeight {
                                // If the leadings are negative, fill them first.
                                let mut ascent: nscoord = fm.MaxAscent();
                                let mut descent: nscoord = fm.MaxDescent();
                                if lineWM.IsLineInverted() {
                                    std::mem::swap(&mut ascent, &mut descent);
                                }
                                blockStart = -ascent;
                                blockEnd = descent;
                                delta = emphasisHeight;
                            }
                            let side: LogicalSide = self.mStyleText.TextEmphasisSide(lineWM, spanFrame.StyleFont().mLanguage());
                            if side == LogicalSide::BStart {
                                blockStart -= delta;
                            } else {
                                blockEnd += delta;
                            }
                        }
                    }

                    minBCoord = std::cmp::min(blockStart, minBCoord);
                    maxBCoord = std::cmp::max(blockEnd, maxBCoord);
                } else {
                    // XXX issues:
                    // [1] BR's on empty lines stop working
                    // [2] May not honor css2's notion of handling empty elements
                    // [3] blank lines in a pre-section ("\n") (handled with preMode)

                    // XXX Are there other problems with this?
                    minBCoord = 0;
                    maxBCoord = 0;
                }
            }
        }

        if (minBCoord == BLOCKDIR_ALIGN_FRAMES_NO_MINIMUM) || (maxBCoord == BLOCKDIR_ALIGN_FRAMES_NO_MAXIMUM) {
            minBCoord = baselineBCoord;
            maxBCoord = baselineBCoord;
        }

        if psd != self.mRootSpan && zeroEffectiveSpanBox {
            let goodMinBCoord: nscoord = PFD(spanFramePFD).mBorderPadding.BStart(lineWM) - PSD(psd).mBStartLeading;
            let goodMaxBCoord: nscoord = goodMinBCoord + PSD(psd).mLogicalBSize;

            // For cases like the one in bug 714519 (text-decoration placement
            // or making nsLineLayout::IsZeroBSize() handle
            // vertical-align:top/bottom on a descendant of the line that's not
            // a child of it), we want to treat elements that are
            // vertical-align: top or bottom somewhat like children for the
            // purposes of this quirk.  To some extent, this is guessing, since
            // they might end up being aligned anywhere.  However, we'll guess
            // that they'll be placed aligned with the top or bottom of this
            // frame (as though this frame is the only thing in the line).
            // (Guessing isn't unreasonable, since all we're doing is reducing the
            // scope of a quirk and making the behavior more standards-like.)
            if maxStartBoxBSize > maxBCoord - minBCoord {
                // Distribute maxStartBoxBSize to ascent (baselineBCoord - minBCoord), and
                // then to descent (maxBCoord - baselineBCoord) by adjusting minBCoord or
                // maxBCoord, but not to exceed goodMinBCoord and goodMaxBCoord.
                let mut distribute: nscoord = maxStartBoxBSize - (maxBCoord - minBCoord);
                let ascentSpace: nscoord = std::cmp::max(minBCoord - goodMinBCoord, 0);
                if distribute > ascentSpace {
                    distribute -= ascentSpace;
                    minBCoord -= ascentSpace;
                    let descentSpace: nscoord = std::cmp::max(goodMaxBCoord - maxBCoord, 0);
                    maxBCoord += std::cmp::min(descentSpace, distribute);
                } else {
                    minBCoord -= distribute;
                }
            }
            if maxEndBoxBSize > maxBCoord - minBCoord {
                // Likewise, but preferring descent to ascent.
                let mut distribute: nscoord = maxEndBoxBSize - (maxBCoord - minBCoord);
                let descentSpace: nscoord = std::cmp::max(goodMaxBCoord - maxBCoord, 0);
                if distribute > descentSpace {
                    distribute -= descentSpace;
                    maxBCoord += descentSpace;
                    let ascentSpace: nscoord = std::cmp::max(minBCoord - goodMinBCoord, 0);
                    minBCoord -= std::cmp::min(ascentSpace, distribute);
                } else {
                    maxBCoord += distribute;
                }
            }

            if minBCoord > goodMinBCoord {
                let adjust: nscoord = minBCoord - goodMinBCoord; // positive

                // shrink the logical extents
                PSD(psd).mLogicalBSize -= adjust;
                PSD(psd).mBStartLeading -= adjust;
            }
            if maxBCoord < goodMaxBCoord {
                let adjust: nscoord = goodMaxBCoord - maxBCoord;
                PSD(psd).mLogicalBSize -= adjust;
                PSD(psd).mBEndLeading -= adjust;
            }
            if minBCoord > 0 {
                // shrink the content by moving its block start down.  This is tricky,
                // since the block start is the 0 for many coordinates, so what we do is
                // move everything else up.
                PFD(spanFramePFD).mAscent -= minBCoord; // move the baseline up
                *PFD(spanFramePFD).mBounds.BSize_mut(lineWM) -= minBCoord; // move the block end up
                PSD(psd).mBStartLeading += minBCoord;
                unsafe { *PSD(psd).mBaseline -= minBCoord };

                pfd = PSD(psd).mFirstFrame;
                while !pfd.is_null() {
                    *PFD(pfd).mBounds.BStart_mut(lineWM) -= minBCoord; // move all the children
                                                                       // back up
                    PFD(pfd).mFrame.SetRect__WritingMode_LogicalRect_nsSize(lineWM, PFD(pfd).mBounds, self.ContainerSizeForSpan(psd));
                    pfd = PFD(pfd).mNext;
                }
                maxBCoord -= minBCoord; // since minBCoord is in the frame's own
                                        // coordinate system
                minBCoord = 0;
            }
            if maxBCoord < PFD(spanFramePFD).mBounds.BSize(lineWM) {
                let adjust: nscoord = PFD(spanFramePFD).mBounds.BSize(lineWM) - maxBCoord;
                *PFD(spanFramePFD).mBounds.BSize_mut(lineWM) -= adjust; // move the bottom up
                PSD(psd).mBEndLeading += adjust;
            }
        }

        PSD(psd).mMinBCoord = minBCoord;
        PSD(psd).mMaxBCoord = maxBCoord;
        self.mMaxStartBoxBSize = std::cmp::max(self.mMaxStartBoxBSize, maxStartBoxBSize);
        self.mMaxEndBoxBSize = std::cmp::max(self.mMaxEndBoxBSize, maxEndBoxBSize);
    }
}

// Gecko: layout/generic/nsLineLayout.cpp SlideSpanFrameRect
fn SlideSpanFrameRect(aFrame: nsIFrame, aDeltaWidth: nscoord) {
    // This should not use nsIFrame::MovePositionBy because it happens
    // prior to relative positioning.  In particular, because
    // nsBlockFrame::PlaceLine calls aLineLayout.TrimTrailingWhiteSpace()
    // prior to calling aLineLayout.RelativePositionFrames().
    let mut p: nsPoint = aFrame.GetPosition();
    p.x -= aDeltaWidth;
    aFrame.SetPosition__nsPoint(p);
}

impl nsLineLayout {
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::TrimTrailingWhiteSpaceIn
    pub fn TrimTrailingWhiteSpaceIn(&mut self, psd: *mut nsLineLayout_PerSpanData, aDeltaISize: &mut nscoord) -> bool {
        let mut pfd = PSD(psd).mFirstFrame;
        if pfd.is_null() {
            *aDeltaISize = 0;
            return false;
        }
        pfd = PFD(pfd).Last();
        while !pfd.is_null() {
            let childSpan = PFD(pfd).mSpan;
            let lineWM = PSD(self.mRootSpan).mWritingMode;
            if !childSpan.is_null() {
                // Maybe the child span has the trailing white-space in it?
                if self.TrimTrailingWhiteSpaceIn(childSpan, aDeltaISize) {
                    let deltaISize: nscoord = *aDeltaISize;
                    if deltaISize != 0 {
                        // Adjust the child spans frame size
                        *PFD(pfd).mBounds.ISize_mut(lineWM) -= deltaISize;
                        if psd != self.mRootSpan {
                            // When the child span is not a direct child of the block
                            // we need to update the child spans frame rectangle
                            // because it most likely will not be done again. Spans
                            // that are direct children of the block will be updated
                            // later, however, because the VerticalAlignFrames method
                            // will be run after this method.
                            let containerSize: nsSize = self.ContainerSizeForSpan(childSpan);
                            let f = PFD(pfd).mFrame;
                            let mut r = f.GetLogicalRect__WritingMode_nsSize(lineWM, containerSize);
                            *r.ISize_mut(lineWM) -= deltaISize;
                            f.SetRect__WritingMode_LogicalRect_nsSize(lineWM, r, containerSize);
                        }

                        // Adjust the inline end edge of the span that contains the child span
                        PSD(psd).mICoord -= deltaISize;

                        // Slide any frames that follow the child span over by the
                        // correct amount. The only thing that can follow the child
                        // span is empty stuff, so we are just making things
                        // sensible (keeping the combined area honest).
                        while !PFD(pfd).mNext.is_null() {
                            pfd = PFD(pfd).mNext;
                            *PFD(pfd).mBounds.IStart_mut(lineWM) -= deltaISize;
                            if psd != self.mRootSpan {
                                // When the child span is not a direct child of the block
                                // we need to update the child span's frame rectangle
                                // because it most likely will not be done again. Spans
                                // that are direct children of the block will be updated
                                // later, however, because the VerticalAlignFrames method
                                // will be run after this method.
                                SlideSpanFrameRect(PFD(pfd).mFrame, deltaISize);
                            }
                        }
                    }
                    return true;
                }
            } else if !PFD(pfd).mIsTextFrame && !PFD(pfd).mSkipWhenTrimmingWhitespace {
                // If we hit a frame on the end that's not text and not a placeholder,
                // then there is no trailing whitespace to trim. Stop the search.
                *aDeltaISize = 0;
                return true;
            } else if PFD(pfd).mIsTextFrame {
                // Call TrimTrailingWhiteSpace even on empty textframes because they
                // might have a soft hyphen which should now appear, changing the frame's
                // width
                // (the DrawTarget argument: NOT PORTED, there is no drawing)
                let trimOutput: nsTextFrame_TrimOutput = PFD(pfd).mFrame.TrimTrailingWhiteSpace();

                if trimOutput.mChanged {
                    PFD(pfd).mRecomputeOverflow = true;
                }

                // Delta width not being zero means that
                // there is trimmed space in the frame.
                if trimOutput.mDeltaWidth != 0 {
                    *PFD(pfd).mBounds.ISize_mut(lineWM) -= trimOutput.mDeltaWidth;

                    // If any trailing space is trimmed, the justification opportunity
                    // generated by the space should be removed as well.
                    PFD(pfd).mJustificationInfo.CancelOpportunityForTrimmedSpace();

                    // See if the text frame has already been placed in its parent
                    if psd != self.mRootSpan {
                        // The frame was already placed during psd's
                        // reflow. Update the frames rectangle now.
                        PFD(pfd).mFrame.SetRect__WritingMode_LogicalRect_nsSize(lineWM, PFD(pfd).mBounds, self.ContainerSizeForSpan(psd));
                    }

                    // Adjust containing span's right edge
                    PSD(psd).mICoord -= trimOutput.mDeltaWidth;

                    // Slide any frames that follow the text frame over by the
                    // right amount. The only thing that can follow the text
                    // frame is empty stuff, so we are just making things
                    // sensible (keeping the combined area honest).
                    while !PFD(pfd).mNext.is_null() {
                        pfd = PFD(pfd).mNext;
                        *PFD(pfd).mBounds.IStart_mut(lineWM) -= trimOutput.mDeltaWidth;
                        if psd != self.mRootSpan {
                            // When the child span is not a direct child of the block
                            // we need to update the child spans frame rectangle
                            // because it most likely will not be done again. Spans
                            // that are direct children of the block will be updated
                            // later, however, because the VerticalAlignFrames method
                            // will be run after this method.
                            SlideSpanFrameRect(PFD(pfd).mFrame, trimOutput.mDeltaWidth);
                        }
                    }
                }

                if PFD(pfd).mIsNonEmptyTextFrame || trimOutput.mChanged {
                    // Pass up to caller so they can shrink their span
                    *aDeltaISize = trimOutput.mDeltaWidth;
                    return true;
                }
            }
            pfd = PFD(pfd).mPrev;
        }

        *aDeltaISize = 0;
        false
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::TrimTrailingWhiteSpace
    pub fn TrimTrailingWhiteSpace(&mut self) -> bool {
        let mut deltaISize: nscoord = 0;
        let rootSpan = self.mRootSpan;
        self.TrimTrailingWhiteSpaceIn(rootSpan, &mut deltaISize);
        0 != deltaISize
    }
}

// Gecko: layout/generic/nsLineLayout.cpp IsRubyAlignSpaceAround
fn IsRubyAlignSpaceAround(aRubyBase: nsIFrame) -> bool {
    nsStyleTextAdapter::of(aRubyBase).IsRubyAlignSpaceAround()
}

impl nsLineLayout {
    /**
     * Assign justification gaps for justification
     * opportunities across two frames.
     */
    /* static */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::AssignInterframeJustificationGaps
    pub fn AssignInterframeJustificationGaps(aFrame: *mut nsLineLayout_PerFrameData, aState: &mut nsLineLayout_JustificationComputationState) -> i32 {
        let prev = aState.mLastParticipant;
        MOZ_ASSERT!(!prev.is_null());

        // (the C++ references `assign` and `prevAssign` are written as the fields below, because
        // `aFrame` and `prev` can be the same object in the ruby branch only through pointers)
        if !aState.mLastExitedRubyBase.is_null() || !aState.mLastEnteredRubyBase.is_null() {
            let exitedRubyBase = aState.mLastExitedRubyBase;
            if exitedRubyBase.is_null() || IsRubyAlignSpaceAround(PFD(exitedRubyBase).mFrame) {
                PFD(prev).mJustificationAssignment.mGapsAtEnd = 1;
            } else {
                PFD(exitedRubyBase).mJustificationAssignment.mGapsAtEnd = 1;
            }

            let enteredRubyBase = aState.mLastEnteredRubyBase;
            if enteredRubyBase.is_null() || IsRubyAlignSpaceAround(PFD(enteredRubyBase).mFrame) {
                PFD(aFrame).mJustificationAssignment.mGapsAtStart = 1;
            } else {
                PFD(enteredRubyBase).mJustificationAssignment.mGapsAtStart = 1;
            }

            // We are no longer going across a ruby base boundary.
            aState.mLastExitedRubyBase = std::ptr::null_mut();
            aState.mLastEnteredRubyBase = std::ptr::null_mut();
            return 1;
        }

        let info = PFD(aFrame).mJustificationInfo;
        let prevInfo = PFD(prev).mJustificationInfo;
        if !info.mIsStartJustifiable && !prevInfo.mIsEndJustifiable {
            return 0;
        }

        if !info.mIsStartJustifiable {
            PFD(prev).mJustificationAssignment.mGapsAtEnd = 2;
            PFD(aFrame).mJustificationAssignment.mGapsAtStart = 0;
        } else if !prevInfo.mIsEndJustifiable {
            PFD(prev).mJustificationAssignment.mGapsAtEnd = 0;
            PFD(aFrame).mJustificationAssignment.mGapsAtStart = 2;
        } else {
            PFD(prev).mJustificationAssignment.mGapsAtEnd = 1;
            PFD(aFrame).mJustificationAssignment.mGapsAtStart = 1;
        }
        1
    }

    /**
     * Compute the justification info of the given span, and store the
     * number of inner opportunities into the frame's justification info.
     * It returns the number of non-inner opportunities it detects.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::ComputeFrameJustification
    pub fn ComputeFrameJustification(&mut self, aPSD: *mut nsLineLayout_PerSpanData, aState: &mut nsLineLayout_JustificationComputationState) -> i32 {
        NS_ASSERTION!(!aPSD.is_null(), "null arg");
        NS_ASSERTION!(aState.mLastParticipant.is_null() || PFD(aState.mLastParticipant).mSpan.is_null(), "Last participant shall always be a leaf frame");
        let mut firstChild: bool = true;
        MOZ_ASSERT!(PFD(PSD(aPSD).mFrame).mJustificationInfo.mInnerOpportunities == 0, "Justification info should not have been set yet.");
        // (`int32_t& innerOpportunities = aPSD->mFrame->mJustificationInfo.mInnerOpportunities`:
        // a local that is written back at the end, because a child span's call may read it)
        let mut innerOpportunities: i32 = 0;
        let mut outerOpportunities: i32 = 0;

        let mut pfd = PSD(aPSD).mFirstFrame;
        while !pfd.is_null() {
            if !PFD(pfd).ParticipatesInJustification() {
                pfd = PFD(pfd).mNext;
                continue;
            }

            let isRubyBase: bool = PFD(pfd).mFrame.IsRubyBaseFrame();
            let outerRubyBase = aState.mLastEnteredRubyBase;
            if isRubyBase {
                aState.mLastEnteredRubyBase = pfd;
            }

            let mut extraOpportunities: i32 = 0;
            if !PFD(pfd).mSpan.is_null() {
                let span = PFD(pfd).mSpan;
                extraOpportunities = self.ComputeFrameJustification(span, aState);
                innerOpportunities += PFD(pfd).mJustificationInfo.mInnerOpportunities;
            } else {
                if PFD(pfd).mIsTextFrame {
                    innerOpportunities += PFD(pfd).mJustificationInfo.mInnerOpportunities;
                }

                if aState.mLastParticipant.is_null() {
                    aState.mFirstParticipant = pfd;
                    // It is not an empty ruby base, but we are not assigning gaps
                    // to the content for now. Clear the last entered ruby base so
                    // that we can correctly set the last exited ruby base.
                    aState.mLastEnteredRubyBase = std::ptr::null_mut();
                } else {
                    extraOpportunities = nsLineLayout::AssignInterframeJustificationGaps(pfd, aState);
                }

                aState.mLastParticipant = pfd;
            }

            if isRubyBase {
                if aState.mLastEnteredRubyBase == pfd {
                    // There is no justification participant inside this ruby base.
                    // Ignore this ruby base completely and restore the outer ruby
                    // base here.
                    aState.mLastEnteredRubyBase = outerRubyBase;
                } else {
                    aState.mLastExitedRubyBase = pfd;
                }
            }

            if firstChild {
                outerOpportunities = extraOpportunities;
                firstChild = false;
            } else {
                innerOpportunities += extraOpportunities;
            }
            pfd = PFD(pfd).mNext;
        }

        PFD(PSD(aPSD).mFrame).mJustificationInfo.mInnerOpportunities = innerOpportunities;
        outerOpportunities
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::AdvanceAnnotationInlineBounds
    // NOT PORTED: ruby annotations (RubyUtils::GetReservedISize, nsRubyTextFrame::IsCollapsed).
    // The C++ body is nsLineLayout.cpp lines 2932-2971 at GECKO_REV.
    pub fn AdvanceAnnotationInlineBounds(&mut self, aPFD: *mut nsLineLayout_PerFrameData, aContainerSize: nsSize, aDeltaICoord: nscoord, aDeltaISize: nscoord) {
        unimplemented!("NOT PORTED: ruby annotations (AdvanceAnnotationInlineBounds)");
    }

    /**
     * This function applies the changes of icoord and isize caused by
     * justification to annotations of the given frame.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::ApplyLineJustificationToAnnotations
    pub fn ApplyLineJustificationToAnnotations(&mut self, aPFD: *mut nsLineLayout_PerFrameData, aDeltaICoord: nscoord, aDeltaISize: nscoord) {
        let mut pfd = PFD(aPFD).mNextAnnotation;
        while !pfd.is_null() {
            let containerSize: nsSize = PFD(pfd).mFrame.GetParent().unwrap().GetSize();
            self.AdvanceAnnotationInlineBounds(pfd, containerSize, aDeltaICoord, aDeltaISize);

            // There are two cases where an annotation frame has siblings which
            // do not attached to a ruby base-level frame:
            // 1. there's an intra-annotation whitespace which has no intra-base
            //    white-space to pair with;
            // 2. there are not enough ruby bases to be paired with annotations.
            // In these cases, their size should not be affected, but we still
            // need to move them so that they won't overlap other frames.
            let mut sibling = PFD(pfd).mNext;
            while !sibling.is_null() && !PFD(sibling).mIsLinkedToBase {
                self.AdvanceAnnotationInlineBounds(sibling, containerSize, aDeltaICoord + aDeltaISize, 0);
                sibling = PFD(sibling).mNext;
            }
            pfd = PFD(pfd).mNextAnnotation;
        }
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::ApplyFrameJustification
    pub fn ApplyFrameJustification(&mut self, aPSD: *mut nsLineLayout_PerSpanData, aState: &mut JustificationApplicationState) -> nscoord {
        NS_ASSERTION!(!aPSD.is_null(), "null arg");

        let mut deltaICoord: nscoord = 0;
        let mut pfd = PSD(aPSD).mFirstFrame;
        while !pfd.is_null() {
            let mut dw: nscoord = 0;
            let lineWM = PSD(self.mRootSpan).mWritingMode;
            let assign: JustificationAssignment = PFD(pfd).mJustificationAssignment;
            let isInlineText: bool = PFD(pfd).mIsTextFrame && !PFD(pfd).mWritingMode.IsOrthogonalTo(&lineWM);

            // Don't apply justification if the frame doesn't participate. Same
            // as the condition used in ComputeFrameJustification. Note that,
            // we still need to move the frame based on deltaICoord even if the
            // frame itself doesn't expand.
            if PFD(pfd).ParticipatesInJustification() {
                if isInlineText {
                    if aState.IsJustifiable() {
                        // Set corresponding justification gaps here, so that the
                        // text frame knows how it should add gaps at its sides.
                        let info = PFD(pfd).mJustificationInfo;
                        let textFrame = PFD(pfd).mFrame;
                        textFrame.AssignJustificationGaps(assign);
                        dw = aState.Consume(JustificationUtils::CountGaps(&info, &assign));
                    }

                    if dw != 0 {
                        PFD(pfd).mRecomputeOverflow = true;
                    }
                } else if !PFD(pfd).mSpan.is_null() {
                    dw = self.ApplyFrameJustification(PFD(pfd).mSpan, aState);
                }
            } else {
                MOZ_ASSERT!(assign.TotalGaps() == 0, "Non-participants shouldn't have assigned gaps");
            }

            *PFD(pfd).mBounds.ISize_mut(lineWM) += dw;
            let mut gapsAtEnd: nscoord = 0;
            if !isInlineText && assign.TotalGaps() != 0 {
                // It is possible that we assign gaps to non-text frame or an
                // orthogonal text frame. Apply the gaps as margin for them.
                deltaICoord += aState.Consume(assign.mGapsAtStart as i32);
                gapsAtEnd = aState.Consume(assign.mGapsAtEnd as i32);
                dw += gapsAtEnd;
            }
            *PFD(pfd).mBounds.IStart_mut(lineWM) += deltaICoord;

            // The gaps added to the end of the frame should also be
            // excluded from the isize added to the annotation.
            self.ApplyLineJustificationToAnnotations(pfd, deltaICoord, dw - gapsAtEnd);
            deltaICoord += dw;
            PFD(pfd).mFrame.SetRect__WritingMode_LogicalRect_nsSize(lineWM, PFD(pfd).mBounds, self.ContainerSizeForSpan(aPSD));
            pfd = PFD(pfd).mNext;
        }
        deltaICoord
    }
}

// Gecko: layout/generic/nsLineLayout.cpp FindNearestRubyBaseAncestor
// NOT PORTED: ruby (nsRubyBaseFrame); only called when the line has ruby.
fn FindNearestRubyBaseAncestor(aFrame: nsIFrame) -> Option<nsIFrame> {
    MOZ_ASSERT!(aFrame.Style().ShouldSuppressLineBreak());
    let mut frame = Some(aFrame);
    while let Some(f) = frame {
        if f.IsRubyBaseFrame() {
            break;
        }
        frame = f.GetParent();
    }
    // XXX It is possible that no ruby base ancestor is found because of
    // some edge cases like form control or canvas inside ruby text.
    // See bug 1138092 comment 4.
    NS_WARNING_ASSERTION!(frame.is_some(), "no ruby base ancestor?");
    frame
}

impl nsLineLayout {
    /**
     * This method expands the given frame by the given reserved isize.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::ExpandRubyBox
    // NOT PORTED: ruby. The C++ body is nsLineLayout.cpp lines 3075-3117 at GECKO_REV.
    pub fn ExpandRubyBox(&mut self, aFrame: *mut nsLineLayout_PerFrameData, aReservedISize: nscoord, aContainerSize: nsSize) {
        unimplemented!("NOT PORTED: ruby (ExpandRubyBox)");
    }

    /**
     * This method expands the given frame by the reserved inline size.
     * It also expands its annotations if they are expandable and have
     * reserved isize larger than zero.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::ExpandRubyBoxWithAnnotations
    // NOT PORTED: ruby. The C++ body is nsLineLayout.cpp lines 3124-3178 at GECKO_REV.
    pub fn ExpandRubyBoxWithAnnotations(&mut self, aFrame: *mut nsLineLayout_PerFrameData, aContainerSize: nsSize) {
        unimplemented!("NOT PORTED: ruby (ExpandRubyBoxWithAnnotations)");
    }

    /**
     * This method looks for all expandable ruby box in the given span, and
     * calls ExpandRubyBox to expand them in depth-first preorder.
     */
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::ExpandInlineRubyBoxes
    // NOT PORTED: ruby (RubyUtils::IsExpandableRubyBox). Only called when mHasRuby is set.
    // The C++ body is nsLineLayout.cpp lines 3184-3194 at GECKO_REV.
    pub fn ExpandInlineRubyBoxes(&mut self, aSpan: *mut nsLineLayout_PerSpanData) {
        unimplemented!("NOT PORTED: ruby (ExpandInlineRubyBoxes)");
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::GetHangFrom
    pub fn GetHangFrom(&self, aSpan: *const nsLineLayout_PerSpanData, aLineIsRTL: bool) -> nscoord {
        let mut pfd = PSD(aSpan as *mut nsLineLayout_PerSpanData).mLastFrame;
        let mut result: nscoord = 0;
        while !pfd.is_null() {
            let childSpan = PFD(pfd).mSpan;
            if !childSpan.is_null() {
                return self.GetHangFrom(childSpan, aLineIsRTL);
            }
            if PFD(pfd).mIsTextFrame {
                let lastText = PFD(pfd).mFrame;
                result = lastText.GetHangableISize();
                if result != 0 {
                    // If the hangable space will be at the start edge of the line, due to
                    // its bidi direction being against the line direction, we flag this by
                    // negating the advance.
                    // (`lastText->EnsureTextRun(eInflated); GetTextRun(eInflated)`: NOT PORTED,
                    // text runs; a null text run is skipped, as in C++)
                    if lastText.TextRunIsRightToLeft() != aLineIsRTL {
                        result = -result;
                    }
                }
                return result;
            }
            if !PFD(pfd).mSkipWhenTrimmingWhitespace {
                // If we hit a frame on the end that's not text and not a placeholder or
                // <br>, then there is no trailing whitespace to hang. Stop the search.
                return result;
            }
            // Scan back for a preceding frame whose whitespace we can hang.
            pfd = PFD(pfd).mPrev;
        }
        result
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::GetTrimFrom
    pub fn GetTrimFrom(&self, aSpan: *const nsLineLayout_PerSpanData, aLineIsRTL: bool) -> gfxTextRun_TrimmableWS {
        let mut pfd = PSD(aSpan as *mut nsLineLayout_PerSpanData).mLastFrame;
        while !pfd.is_null() {
            let childSpan = PFD(pfd).mSpan;
            if !childSpan.is_null() {
                return self.GetTrimFrom(childSpan, aLineIsRTL);
            }
            if PFD(pfd).mIsTextFrame {
                let lastText = PFD(pfd).mFrame;
                let mut result = lastText.GetTrimmableWS();
                if result.mAdvance != 0.0 {
                    // (text run lookup: NOT PORTED, see GetHangFrom)
                    if lastText.TextRunIsRightToLeft() != aLineIsRTL {
                        result.mAdvance = -result.mAdvance;
                    }
                }
                return result;
            }
            if !PFD(pfd).mSkipWhenTrimmingWhitespace {
                // If we hit a frame on the end that's not text and not a placeholder or
                // <br>, then there is no trailing whitespace to trim. Stop the search.
                return gfxTextRun_TrimmableWS::default();
            }
            // Scan back for a preceding frame whose whitespace we can trim.
            pfd = PFD(pfd).mPrev;
        }
        gfxTextRun_TrimmableWS::default()
    }

    // Align inline frames within the line according to the CSS text-align
    // property.
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::TextAlignLine
    pub fn TextAlignLine(&mut self, aLine: &mut nsLineBox, aIsLastLine: bool) {
        /**
         * NOTE: aIsLastLine ain't necessarily so: it is correctly set by caller
         * only in cases where the last line needs special handling.
         */
        let psd = self.mRootSpan;
        let lineWM = PSD(psd).mWritingMode;
        NS_WARNING_ASSERTION!(PSD(psd).mIEnd != NS_UNCONSTRAINEDSIZE, "have unconstrained width; this should only result from very large sizes, not attempts at intrinsic width calculation");
        let availISize: nscoord = PSD(psd).mIEnd - PSD(psd).mIStart;
        let mut remainingISize: nscoord = availISize - aLine.ISize();

        let mut dx: nscoord = 0;
        let textAlign: StyleTextAlign = if aIsLastLine { self.mStyleText.TextAlignForLastLine() } else { self.mStyleText.mTextAlign() };

        // Check if there's trailing whitespace we need to "hang" at line-wrap.
        let mut hang: nscoord = 0;
        let mut trimCount: u32 = 0;
        if aLine.IsLineWrapped() {
            if textAlign == StyleTextAlign::Justify {
                let trim = self.GetTrimFrom(self.mRootSpan, lineWM.IsBidiRTL());
                hang = NSToCoordRound__double(trim.mAdvance);
                trimCount = trim.mCount;
            } else {
                hang = self.GetHangFrom(self.mRootSpan, lineWM.IsBidiRTL());
            }
        }

        let isSVG: bool = self.LineContainerFrame().IsInSVGTextSubtree();
        let doTextAlign: bool = remainingISize > 0 || hang != 0;

        let mut additionalGaps: i32 = 0;
        if !isSVG && (self.mHasRuby || (doTextAlign && textAlign == StyleTextAlign::Justify)) {
            let mut computeState = nsLineLayout_JustificationComputationState::new();
            self.ComputeFrameJustification(psd, &mut computeState);
            if self.mHasRuby && !computeState.mFirstParticipant.is_null() {
                let firstFrame = computeState.mFirstParticipant;
                if PFD(firstFrame).mFrame.Style().ShouldSuppressLineBreak() {
                    MOZ_ASSERT!(PFD(firstFrame).mJustificationAssignment.mGapsAtStart == 0);
                    let rubyBase = FindNearestRubyBaseAncestor(PFD(firstFrame).mFrame);
                    if let Some(rubyBase) = rubyBase {
                        if IsRubyAlignSpaceAround(rubyBase) {
                            PFD(firstFrame).mJustificationAssignment.mGapsAtStart = 1;
                            additionalGaps += 1;
                        }
                    }
                }
                let lastFrame = computeState.mLastParticipant;
                if PFD(lastFrame).mFrame.Style().ShouldSuppressLineBreak() {
                    MOZ_ASSERT!(PFD(lastFrame).mJustificationAssignment.mGapsAtEnd == 0);
                    let rubyBase = FindNearestRubyBaseAncestor(PFD(lastFrame).mFrame);
                    if let Some(rubyBase) = rubyBase {
                        if IsRubyAlignSpaceAround(rubyBase) {
                            PFD(lastFrame).mJustificationAssignment.mGapsAtEnd = 1;
                            additionalGaps += 1;
                        }
                    }
                }
            }
        }

        if !isSVG && doTextAlign {
            // (the C++ `switch` falls through from Justify to Start when the line cannot be
            // justified; here `justified` says the Justify arm finished)
            let mut handled = false;
            if textAlign == StyleTextAlign::Justify {
                let opportunities: i32 = PFD(PSD(psd).mFrame).mJustificationInfo.mInnerOpportunities - (if hang != 0 { trimCount as i32 } else { 0 });
                if opportunities > 0 {
                    let gaps: i32 = opportunities * 2 + additionalGaps;
                    remainingISize += hang.abs();
                    let mut applyState = JustificationApplicationState::new(gaps, remainingISize);

                    // Apply the justification, and make sure to update our linebox
                    // width to account for it.
                    let applied = self.ApplyFrameJustification(psd, &mut applyState);
                    let containerSizeForSpan = self.ContainerSizeForSpan(psd);
                    aLine.ExpandBy(applied, containerSizeForSpan);

                    // If the trimmable trailing whitespace that we want to hang had
                    // reverse-inline directionality, adjust line position to account for
                    // it being at the inline-start side.
                    // On top of the original "hang" amount, justification will have
                    // modified its width, so we include that adjustment here.
                    if hang < 0 {
                        dx = hang - (trimCount as i32) * remainingISize / opportunities;
                    }

                    // Gaps that belong to trimmed whitespace were not included in the
                    // applyState count, so we need to add them here for the assert.
                    #[cfg(debug_assertions)]
                    {
                        let trimmedGaps: i32 = if hang != 0 { (trimCount * 2) as i32 } else { 0 };
                        MOZ_ASSERT!(applyState.mGaps.mHandled == applyState.mGaps.mCount + trimmedGaps, "Unprocessed justification gaps");
                        // Similarly, account for the adjustment applied to the trimmed
                        // whitespace, which is in addition to the adjustment that applies
                        // within the actual width of the line.
                        let trimmedAdjustment: i32 = (trimCount as i32) * remainingISize / opportunities;
                        NS_ASSERTION!(applyState.mWidth.mConsumed == applyState.mWidth.mAvailable + trimmedAdjustment, "Unprocessed justification width");
                    }
                    handled = true;
                }
                // Fall through to the default case if we could not justify to fill
                // the space.
            }
            if !handled {
                match textAlign {
                    // (Justify arrives here only by the fall-through above)
                    StyleTextAlign::Justify | StyleTextAlign::Start => {
                        // Default alignment is to start edge so do nothing, except to apply
                        // any "reverse-hang" amount resulting from reversed-direction trailing
                        // space.
                        // Char is for tables so treat as start if we find it in block layout.
                        if hang < 0 {
                            dx = hang;
                        }
                    }

                    StyleTextAlign::Left | StyleTextAlign::MozLeft => {
                        if lineWM.IsBidiRTL() {
                            dx = remainingISize + (if hang > 0 { hang } else { 0 });
                        } else if hang < 0 {
                            dx = hang;
                        }
                    }

                    StyleTextAlign::Right | StyleTextAlign::MozRight => {
                        if lineWM.IsBidiLTR() {
                            dx = remainingISize + (if hang > 0 { hang } else { 0 });
                        } else if hang < 0 {
                            dx = hang;
                        }
                    }

                    StyleTextAlign::End => {
                        dx = remainingISize + (if hang > 0 { hang } else { 0 });
                    }

                    StyleTextAlign::Center | StyleTextAlign::MozCenter => {
                        dx = (remainingISize + hang) / 2;
                    }
                }
            }
        }

        if self.mHasRuby {
            let rootSpan = self.mRootSpan;
            self.ExpandInlineRubyBoxes(rootSpan);
        }

        let mut startFrame = PSD(psd).mFirstFrame;
        MOZ_ASSERT!(!startFrame.is_null(), "empty line?");
        if PFD(startFrame).mIsMarker {
            // ::marker shouldn't participate in bidi reordering nor text alignment.
            startFrame = PFD(startFrame).mNext;
            MOZ_ASSERT!(!startFrame.is_null(), "no frame after ::marker?");
            MOZ_ASSERT!(!PFD(startFrame).mIsMarker, "multiple ::markers?");
        }

        let bidi: bool = self.mPresContext.BidiEnabled() && (!self.mPresContext.IsVisualMode() || lineWM.IsBidiRTL());
        if bidi {
            nsBidiPresUtils::ReorderFrames(PFD(startFrame).mFrame, aLine.GetChildCount(), lineWM, self.mContainerSize, unsafe { (*psd).mIStart } + self.mTextIndent + dx);
        }

        if dx != 0 {
            // For the bidi case, if startFrame is a ::first-line frame, the mIStart and
            // mTextIndent offsets will already have been applied to its position, but
            // we still need to apply the text-align adjustment |dx| to its position.
            let needToAdjustFrames: bool = !bidi || PFD(startFrame).mFrame.IsLineFrame();
            MOZ_ASSERT!(!PFD(startFrame).mFrame.IsLineFrame() || PFD(startFrame).mNext.is_null());
            if needToAdjustFrames {
                let mut pfd = startFrame;
                while !pfd.is_null() {
                    *PFD(pfd).mBounds.IStart_mut(lineWM) += dx;
                    PFD(pfd).mFrame.SetRect__WritingMode_LogicalRect_nsSize(lineWM, PFD(pfd).mBounds, self.ContainerSizeForSpan(psd));
                    pfd = PFD(pfd).mNext;
                }
            }
            aLine.IndentBy(dx, self.mContainerSize);
        }
    }

    // This method applies any relative positioning to the given frame.
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::ApplyRelativePositioning
    pub fn ApplyRelativePositioning(&mut self, aPFD: *mut nsLineLayout_PerFrameData) {
        if !PFD(aPFD).mIsRelativelyOrStickyPos {
            return;
        }

        let frame = PFD(aPFD).mFrame;
        let frameWM = PFD(aPFD).mWritingMode;
        let mut origin: LogicalPoint = frame.GetLogicalPosition__nsSize(*self.ContainerSize());
        // right and bottom are handled by
        // ReflowInput::ComputeRelativeOffsets
        ReflowInput::ApplyRelativePositioning__nsIFrame_WritingMode_LogicalMargin_LogicalPoint_nsSize(frame, frameWM, PFD(aPFD).mOffsets, &mut origin, *self.ContainerSize());
        frame.SetPosition__WritingMode_LogicalPoint_nsSize(frameWM, origin, *self.ContainerSize());
    }

    // This method do relative positioning for ruby annotations.
    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::RelativePositionAnnotations
    // NOT PORTED: ruby. The C++ body is nsLineLayout.cpp lines 3467-3482 at GECKO_REV.
    pub fn RelativePositionAnnotations(&mut self, aRubyPSD: *mut nsLineLayout_PerSpanData, aOverflowAreas: &mut OverflowAreas) {
        unimplemented!("NOT PORTED: ruby (RelativePositionAnnotations)");
    }

    /**
     * Handle all the relative positioning in the line, compute the
     * combined area (== overflow area) for the line, and handle view
     * sizing/positioning and the setting of the overflow rect.
     */
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::RelativePositionFrames (the inline overload)
    pub fn RelativePositionFrames__OverflowAreas(&mut self, aOverflowAreas: &mut OverflowAreas) {
        let rootSpan = self.mRootSpan;
        self.RelativePositionFrames__PerSpanData_OverflowAreas(rootSpan, aOverflowAreas);
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::RelativePositionFrames(PerSpanData*, OverflowAreas&)
    pub fn RelativePositionFrames__PerSpanData_OverflowAreas(&mut self, psd: *mut nsLineLayout_PerSpanData, aOverflowAreas: &mut OverflowAreas) {
        let mut overflowAreas = OverflowAreas::default();
        let wm = PSD(psd).mWritingMode;
        if psd != self.mRootSpan {
            // The span's overflow areas come in three parts:
            // -- this frame's width and height
            // -- pfd->mOverflowAreas, which is the area of a ::marker or the union
            // of a relatively positioned frame's absolute children
            // -- the bounds of all inline descendants
            // The former two parts are computed right here, we gather the descendants
            // below.
            // At this point psd->mFrame->mBounds might be out of date since
            // bidi reordering can move and resize the frames. So use the frame's
            // rect instead of mBounds.
            let size = PFD(PSD(psd).mFrame).mFrame.GetSize();
            let adjustedBounds = nsRect::new(0, 0, size.width, size.height);

            let scrollable = *PFD(PSD(psd).mFrame).mOverflowAreas.ScrollableOverflow();
            overflowAreas.ScrollableOverflow_mut().UnionRect(scrollable, adjustedBounds);
            let ink = *PFD(PSD(psd).mFrame).mOverflowAreas.InkOverflow();
            overflowAreas.InkOverflow_mut().UnionRect(ink, adjustedBounds);
        } else {
            // Note(dshin, bug 1940938): `mICoord` can be negative due to a negative
            // `text-indent` value (i.e. "Outdenting"). Ensure that we have a valid
            // overflow rect for that case.
            let iStart = std::cmp::min(PSD(psd).mIStart, PSD(psd).mICoord);
            let iSize = (PSD(psd).mICoord - PSD(psd).mIStart).abs();
            let rect = LogicalRect::new(wm, iStart, self.mBStartEdge, iSize, self.mFinalLineBSize);
            // The minimum combined area for the frames that are direct
            // children of the block starts at the upper left corner of the
            // line and is sized to match the size of the line's bounding box
            // (the same size as the values returned from VerticalAlignFrames)
            *overflowAreas.InkOverflow_mut() = rect.GetPhysicalRect(wm, *self.ContainerSize());
            *overflowAreas.ScrollableOverflow_mut() = *overflowAreas.InkOverflow();
        }

        let mut pfd = PSD(psd).mFirstFrame;
        while !pfd.is_null() {
            let frame = PFD(pfd).mFrame;

            // Adjust the origin of the frame
            self.ApplyRelativePositioning(pfd);

            // Note: the combined area of a child is in its coordinate
            // system. We adjust the childs combined area into our coordinate
            // system before computing the aggregated value by adding in
            // <b>x</b> and <b>y</b> which were computed above.
            let mut r = OverflowAreas::default();
            if !PFD(pfd).mSpan.is_null() {
                // Compute a new combined area for the child span before
                // aggregating it into our combined area.
                self.RelativePositionFrames__PerSpanData_OverflowAreas(PFD(pfd).mSpan, &mut r);
            } else {
                r = PFD(pfd).mOverflowAreas;
                if PFD(pfd).mIsTextFrame {
                    // We need to recompute overflow areas in four cases:
                    // (1) When PFD_RECOMPUTEOVERFLOW is set due to trimming
                    // (2) When there are text decorations, since we can't recompute the
                    //     overflow area until Reflow and VerticalAlignLine have finished
                    // (3) When there are text emphasis marks, since the marks may be
                    //     put further away if the text is inside ruby.
                    // (4) When there are text strokes
                    let frameStyleText = nsStyleTextAdapter::of(frame);
                    if PFD(pfd).mRecomputeOverflow || frame.Style().HasTextDecorationLines() || frameStyleText.HasEffectiveTextEmphasis() || frameStyleText.HasWebkitTextStroke() {
                        r = frame.RecomputeOverflow(self.LineContainerFrame());
                    }
                    frame.FinishAndStoreOverflow(&mut r, frame.GetSize());
                }
            }

            overflowAreas.UnionWith(&(r + frame.GetPosition()));
            pfd = PFD(pfd).mNext;
        }

        // Also compute relative position in the annotations.
        if PFD(PSD(psd).mFrame).mFrame.IsRubyFrame() {
            self.RelativePositionAnnotations(psd, &mut overflowAreas);
        }

        // If we just computed a spans combined area, we need to update its
        // overflow rect...
        if psd != self.mRootSpan {
            let spanPFD = PSD(psd).mFrame;
            let frame = PFD(spanPFD).mFrame;
            frame.FinishAndStoreOverflow(&mut overflowAreas, frame.GetSize());
        }
        *aOverflowAreas = overflowAreas;
    }

    // Support methods for word-wrapping during line reflow

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::SetJustificationInfo
    pub fn SetJustificationInfo(&mut self, aInfo: &JustificationInfo) {
        self.mJustificationInfo = *aInfo;
    }

    /**
     * @return true if so far during reflow no non-empty content has been
     * placed in the line (according to nsIFrame::IsEmpty())
     */
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::LineIsEmpty
    pub fn LineIsEmpty(&self) -> bool {
        self.mLineIsEmpty
    }

    /**
     * @return true if so far during reflow no non-empty leaf content
     * (non-collapsed whitespace, replaced element, inline-block, etc) has been
     * placed in the line
     */
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::LineAtStart
    pub fn LineAtStart(&self) -> bool {
        self.mLineAtStart
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::LineIsBreakable
    pub fn LineIsBreakable(&self) -> bool {
        // XXX mTotalPlacedFrames should go away and we should just use
        // mLineIsEmpty here instead
        self.mTotalPlacedFrames != 0 || self.mImpactedByFloats
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetLineEndsInBR
    pub fn GetLineEndsInBR(&self) -> bool {
        self.mLineEndsInBR
    }
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::SetLineEndsInBR
    pub fn SetLineEndsInBR(&mut self, aOn: bool) {
        self.mLineEndsInBR = aOn;
    }

    //----------------------------------------
    // Inform the line-layout about the presence of a floating frame
    // XXX get rid of this: use get-frame-type?
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::AddFloat
    pub fn AddFloat(&mut self, aFloat: nsIFrame, aAvailableISize: nscoord) -> bool {
        // When reflowing ruby text frames, no block reflow state is
        // provided to the line layout. However, floats should never be
        // associated with ruby text containers, hence this method should
        // not be called in that case.
        MOZ_ASSERT!(!self.mBlockRS.is_null(), "Should not call this method if there is no block reflow state available");
        let blockRS = self.mBlockRS;
        unsafe { (*blockRS).AddFloat(self, aFloat, aAvailableISize) }
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::SetTrimmableISize
    pub fn SetTrimmableISize(&mut self, aTrimmableISize: nscoord) {
        self.mTrimmableISize = aTrimmableISize;
    }

    //----------------------------------------

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetFirstLetterStyleOK
    pub fn GetFirstLetterStyleOK(&self) -> bool {
        self.mFirstLetterStyleOK
    }
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::SetFirstLetterStyleOK
    pub fn SetFirstLetterStyleOK(&mut self, aSetting: bool) {
        self.mFirstLetterStyleOK = aSetting;
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetInFirstLetter
    pub fn GetInFirstLetter(&self) -> bool {
        self.mInFirstLetter
    }
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::SetInFirstLetter
    pub fn SetInFirstLetter(&mut self, aSetting: bool) {
        self.mInFirstLetter = aSetting;
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetInFirstLine
    pub fn GetInFirstLine(&self) -> bool {
        self.mInFirstLine
    }
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::SetInFirstLine
    pub fn SetInFirstLine(&mut self, aSetting: bool) {
        self.mInFirstLine = aSetting;
    }

    // Calling this during block reflow ensures that the next line of inlines
    // will be marked dirty, if there is one.
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::SetDirtyNextLine
    pub fn SetDirtyNextLine(&mut self) {
        self.mDirtyNextLine = true;
    }
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetDirtyNextLine
    pub fn GetDirtyNextLine(&self) -> bool {
        self.mDirtyNextLine
    }

    //----------------------------------------

    /**
     * Like NotifyOptionalBreakPosition, but here it's OK for mNeedBackup
     * to be set, because the caller is merely pruning some saved break
     * position(s) that are actually not feasible.
     */
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::RestoreSavedBreakPosition
    pub fn RestoreSavedBreakPosition(&mut self, aFrame: Option<nsIFrame>, aOffset: i32, aPriority: gfxBreakPriority) {
        self.mLastOptionalBreakFrame = aFrame;
        self.mLastOptionalBreakFrameOffset = aOffset;
        self.mLastOptionalBreakPriority = aPriority;
    }
    /**
     * Signal that no backing up will be required after all.
     */
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::ClearOptionalBreakPosition
    pub fn ClearOptionalBreakPosition(&mut self) {
        self.mNeedBackup = false;
        self.mLastOptionalBreakFrame = None;
        self.mLastOptionalBreakFrameOffset = -1;
        self.mLastOptionalBreakPriority = gfxBreakPriority::eNoBreak;
    }
    // Retrieve last set optional break position. When this returns null, no
    // optional break has been recorded (which means that the line can't break
    // yet).
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetLastOptionalBreakPosition
    pub fn GetLastOptionalBreakPosition(&self, aOffset: &mut i32, aPriority: &mut gfxBreakPriority) -> Option<nsIFrame> {
        *aOffset = self.mLastOptionalBreakFrameOffset;
        *aPriority = self.mLastOptionalBreakPriority;
        self.mLastOptionalBreakFrame
    }
    // Whether any optional break position has been recorded.
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::HasOptionalBreakPosition
    pub fn HasOptionalBreakPosition(&self) -> bool {
        self.mLastOptionalBreakFrame.is_some()
    }
    // Get the priority of the last optional break position recorded.
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::LastOptionalBreakPriority
    pub fn LastOptionalBreakPriority(&self) -> gfxBreakPriority {
        self.mLastOptionalBreakPriority
    }

    /**
     * Check whether frames overflowed the available width and CanPlaceFrame
     * requested backing up to a saved break position.
     */
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::NeedsBackup
    pub fn NeedsBackup(&self) -> bool {
        self.mNeedBackup
    }

    // Line layout may place too much content on a line, overflowing its available
    // width. When that happens, if SetLastOptionalBreakPosition has been
    // used to record an optional break that wasn't taken, we can reflow the line
    // again and force the break to happen at that point (i.e., backtracking
    // to the last choice point).

    // Record that we want to break at the given content+offset (which
    // should have been previously returned by GetLastOptionalBreakPosition
    // from another nsLineLayout).
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::ForceBreakAtPosition
    pub fn ForceBreakAtPosition(&mut self, aFrame: nsIFrame, aOffset: i32) {
        self.mForceBreakFrame = Some(aFrame);
        self.mForceBreakFrameOffset = aOffset;
    }
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::HaveForcedBreakPosition
    pub fn HaveForcedBreakPosition(&self) -> bool {
        self.mForceBreakFrame.is_some()
    }
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetForcedBreakPosition
    pub fn GetForcedBreakPosition(&self, aFrame: nsIFrame) -> i32 {
        if self.mForceBreakFrame == Some(aFrame) {
            self.mForceBreakFrameOffset
        } else {
            -1
        }
    }

    /**
     * This can't be null. It usually returns a block frame but may return
     * some other kind of frame when inline frames are reflowed in a non-block
     * context (e.g. MathML or floating first-letter).
     */
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::LineContainerFrame
    pub fn LineContainerFrame(&self) -> nsIFrame {
        self.LineContainerRI().mFrame
    }
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::LineContainerRI
    pub fn LineContainerRI(&self) -> &ReflowInput<'static> {
        unsafe { &*self.mLineContainerRI }
    }
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetLine (const)
    pub fn GetLine(&self) -> Option<&nsLineList_iterator> {
        if self.mGotLineBox {
            Some(&self.mLineBox)
        } else {
            None
        }
    }
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetLine (non-const)
    pub fn GetLine_mut(&mut self) -> Option<&mut nsLineList_iterator> {
        if self.mGotLineBox {
            Some(&mut self.mLineBox)
        } else {
            None
        }
    }

    /**
     * Move the inline position where the next frame will be reflowed forward by
     * aAmount.
     */
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::AdvanceICoord
    pub fn AdvanceICoord(&mut self, aAmount: nscoord) {
        PSD(self.mCurrentSpan).mICoord += aAmount;
    }
    /**
     * Returns the writing mode for the root span.
     */
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetWritingMode
    pub fn GetWritingMode(&self) -> WritingMode {
        PSD(self.mRootSpan).mWritingMode
    }
    /**
     * Returns the inline position where the next frame will be reflowed.
     */
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetCurrentICoord
    pub fn GetCurrentICoord(&self) -> nscoord {
        PSD(self.mCurrentSpan).mICoord
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::SetSuppressLineWrap
    pub fn SetSuppressLineWrap(&mut self, aEnabled: bool) {
        self.mSuppressLineWrap = aEnabled;
    }

    /**
     * Record that the line had to resort to an overflow-wrap break.
     */
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::SetUsedOverflowWrap
    pub fn SetUsedOverflowWrap(&mut self) {
        self.mUsedOverflowWrap = true;
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::GetOutermostLineLayout
    pub fn GetOutermostLineLayout(&mut self) -> *mut nsLineLayout {
        let mut lineLayout: *mut nsLineLayout = self;
        while !unsafe { (*lineLayout).mBaseLineLayout.is_null() } {
            lineLayout = unsafe { (*lineLayout).mBaseLineLayout };
        }
        lineLayout
    }

    // The container size to use when converting between logical and
    // physical coordinates for frames in this span. For the root span
    // this is the size of the block cached in mContainerSize; for
    // child spans it's the size of the root span.
    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::ContainerSizeForSpan
    pub fn ContainerSizeForSpan(&self, aPSD: *mut nsLineLayout_PerSpanData) -> nsSize {
        if aPSD == self.mRootSpan {
            self.mContainerSize
        } else {
            PFD(PSD(aPSD).mFrame).mBounds.Size__WritingMode(PSD(self.mRootSpan).mWritingMode).GetPhysicalSize(PSD(self.mRootSpan).mWritingMode)
        }
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::ContainerSize
    pub fn ContainerSize(&self) -> &nsSize {
        &self.mContainerSize
    }

    // Gecko: layout/generic/nsLineLayout.cpp nsLineLayout::NewPerSpanData
    pub fn NewPerSpanData(&mut self) -> *mut nsLineLayout_PerSpanData {
        let outerLineLayout = self.GetOutermostLineLayout();
        let outer = unsafe { &mut *outerLineLayout };
        let psd: *mut nsLineLayout_PerSpanData;
        let fresh = nsLineLayout_PerSpanData {
            mParent: std::ptr::null_mut(),
            mNextFreeSpan: std::ptr::null_mut(),
            mFrame: std::ptr::null_mut(),
            mFirstFrame: std::ptr::null_mut(),
            mLastFrame: std::ptr::null_mut(),
            mReflowInput: std::ptr::null(),
            mNoWrap: false,
            mWritingMode: WritingMode::default(),
            mContainsFloat: false,
            mHasNonemptyContent: false,
            mIStart: 0,
            mICoord: 0,
            mIEnd: 0,
            mInset: 0,
            mBStartLeading: 0,
            mBEndLeading: 0,
            mLogicalBSize: 0,
            mMinBCoord: 0,
            mMaxBCoord: 0,
            mBaseline: std::ptr::null_mut(),
        };
        if outer.mSpanFreeList.is_null() {
            outer.mSpanArena.push(Box::new(fresh));
            psd = &mut **outer.mSpanArena.last_mut().unwrap() as *mut nsLineLayout_PerSpanData;
        } else {
            psd = outer.mSpanFreeList;
            outer.mSpanFreeList = PSD(psd).mNextFreeSpan;
            unsafe {
                std::ptr::write(psd, fresh);
            }
        }

        #[cfg(debug_assertions)]
        {
            outer.mSpansAllocated += 1;
        }
        psd
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::LastFrame
    pub fn LastFrame(&self) -> *mut nsLineLayout_PerFrameData {
        PSD(self.mCurrentSpan).mLastFrame
    }

    // Gecko: layout/generic/nsLineLayout.h nsLineLayout::InBlockContext
    pub fn InBlockContext(&self) -> bool {
        self.mSpanDepth == 0
    }
}

// Firefox has no gtest for nsLineLayout (it is covered by reftests, which need a whole page), so
// these are written from the documented behaviour: the comments above, CSS 2.2 10.8 (line
// height, vertical-align), CSS Text 3 (text-align, text-indent, justification) and the C++ code
// they exercise. Frames are made from real Stylo styles; text and inline-block reflow report the
// sizes set with SetTextFrameStub and SetReflowStubSize (nsLineLayoutAdapter.rs).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockReflowState_test_support::{frames, root_input, PX};

    const NO_BSIZE: nscoord = NS_UNCONSTRAINEDSIZE;

    // 12px ascent, 3px descent, x-height 6px; sub/superscript offsets 2px and 4px
    fn SetTestFontMetrics() {
        SetFontMetricsStub(Some(nsFontMetricsStub { mMaxAscent: 12 * PX, mMaxDescent: 3 * PX, mXHeight: 6 * PX, mSubscriptOffset: 2 * PX, mSuperscriptOffset: 4 * PX }));
    }

    // Builds a block `root` (100px wide) with the given child frames (ids `a`, `b`, ... in the
    // html, of the given frame types) and a line layout over a one-line list, then runs `body`.
    // The line layout is ended (EndLineReflow) after `body` if the body began a line.
    fn with_line(root_style: &str, children: &[(&str, LayoutFrameType)], lineNumber: i32, body: impl FnOnce(&mut nsLineLayout, &[nsIFrame], *mut nsLineBox, &ReflowInput)) {
        SetTestFontMetrics();
        let mut html = format!(r#"<div id=root style="margin:0;border:0;padding:0;width:100px;{root_style}">"#);
        let mut tree: Vec<(String, Option<usize>)> = vec![("root".to_string(), None)];
        for (id, _) in children {
            // (the child styles are in the id: `name|style`)
            let (name, style) = id.split_once('|').unwrap_or((id, ""));
            html += &format!(r#"<div id={name} style="{style}"></div>"#);
            tree.push((name.to_string(), Some(0)));
        }
        // (one more inline frame, only so that the line box has a child frame: nsLineBox's
        // constructor asserts that an inline line has one of the right kind)
        html += r#"<div id=linechild style="display:inline"></div>"#;
        tree.push(("linechild".to_string(), Some(0)));
        html += "</div>";
        let tree_ref: Vec<(&str, Option<usize>)> = tree.iter().map(|(n, p)| (n.as_str(), *p)).collect();
        let f = frames(&html, &tree_ref, 100);
        for (i, (_, t)) in children.iter().enumerate() {
            f[i + 1].SetType(*t);
        }
        let wm = f[0].GetWritingMode();
        let mut fm = nsFloatManager::new(wm);
        let mut root = root_input(f[0], 100, NO_BSIZE);
        root.mFloatManager = &mut fm;
        let mut list = nsLineList::new();
        let line = NS_NewLineBox__nsIFrame_bool(Some(f[children.len() + 1]), false);
        list.push_back(line);
        let it = list.begin_mut();
        let mut ll = nsLineLayout::new(&nsPresContext::Current(), &mut fm, &root, Some(&it), std::ptr::null_mut());
        ll.Init(std::ptr::null_mut::<BlockReflowState>(), 0, lineNumber);
        body(&mut ll, &f, line, &root);
        if !ll.mRootSpan.is_null() {
            ll.EndLineReflow();
        }
    }

    fn begin(ll: &mut nsLineLayout, f: &[nsIFrame], avail: i32) {
        let wm = f[0].GetWritingMode();
        ll.BeginLineReflow(0, 0, avail * PX, NO_BSIZE, false, false, wm, nsSize::new(100 * PX, 0), 0);
    }

    fn text(f: nsIFrame, aISize: i32, aBSize: i32, aAscent: i32) {
        f.SetTextFrameStub(TextFrameStub { mISize: aISize * PX, mBSize: aBSize * PX, mAscent: aAscent * PX, ..Default::default() });
    }

    // Reflows `f` on the line; returns whether it was pushed to the next line, and the status.
    fn reflow(ll: &mut nsLineLayout, f: nsIFrame) -> (bool, nsReflowStatus) {
        let mut status = nsReflowStatus::new();
        let mut pushed = false;
        ll.ReflowFrame(f, &mut status, None, &mut pushed);
        (pushed, status)
    }

    #[test]
    fn text_frames_are_placed_side_by_side() {
        with_line("", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            assert!(ll.LineIsEmpty() && ll.LineAtStart() && !ll.LineIsBreakable());
            text(f[1], 40, 12, 10);
            text(f[2], 30, 12, 10);
            let (pushed, _) = reflow(ll, f[1]);
            assert!(!pushed);
            assert_eq!(f[1].GetRect(), nsRect::new(0, 0, 40 * PX, 12 * PX));
            assert!(!ll.LineIsEmpty() && !ll.LineAtStart() && ll.LineIsBreakable());
            reflow(ll, f[2]);
            assert_eq!(f[2].GetRect(), nsRect::new(40 * PX, 0, 30 * PX, 12 * PX));
            assert_eq!(ll.GetCurrentICoord(), 70 * PX);
            assert_eq!(ll.GetCurrentSpanCount(), 2);
        });
    }

    #[test]
    fn a_frame_that_does_not_fit_is_pushed_to_the_next_line() {
        with_line("", &[("a", LayoutFrameType::Text), ("b|display:inline-block;width:60px;height:10px", LayoutFrameType::Block)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            text(f[1], 60, 12, 10);
            f[2].SetReflowStubSize(60 * PX, 10 * PX, 10 * PX);
            assert!(!reflow(ll, f[1]).0);
            let (pushed, status) = reflow(ll, f[2]);
            // CanPlaceFrame: it sticks out of the line, the line has content, it cannot continue a text run.
            assert!(pushed);
            assert!(status.IsInlineBreakBefore());
            assert_eq!(ll.GetCurrentSpanCount(), 1);
            assert_eq!(ll.GetCurrentICoord(), 60 * PX);
        });
    }

    #[test]
    fn the_first_frame_is_placed_even_if_it_is_too_wide() {
        // "we always make progress": nothing else on the line and no float next to it.
        with_line("", &[("a|display:inline-block;width:150px;height:10px", LayoutFrameType::Block)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            f[1].SetReflowStubSize(150 * PX, 10 * PX, 10 * PX);
            let (pushed, status) = reflow(ll, f[1]);
            assert!(!pushed);
            assert!(!status.IsInlineBreakBefore());
            assert_eq!(ll.GetCurrentICoord(), 150 * PX);
        });
    }

    #[test]
    fn an_overflowing_text_frame_asks_for_backup() {
        with_line("", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            text(f[1], 60, 12, 10);
            text(f[2], 60, 12, 10);
            reflow(ll, f[1]);
            assert!(!ll.NeedsBackup());
            // A text frame could not break early enough: it is placed, and the line asks to be
            // reflowed with a forced break at the last optional break position.
            let (pushed, _) = reflow(ll, f[2]);
            assert!(!pushed);
            assert!(ll.NeedsBackup());
            ll.ClearOptionalBreakPosition();
            assert!(!ll.NeedsBackup());
        });
    }

    #[test]
    fn nowrap_lines_take_everything() {
        with_line("white-space:nowrap", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            text(f[1], 80, 12, 10);
            text(f[2], 80, 12, 10);
            assert!(!reflow(ll, f[1]).0);
            let (pushed, status) = reflow(ll, f[2]);
            assert!(!pushed && !status.IsInlineBreakBefore());
            assert!(!ll.NeedsBackup());
            assert_eq!(ll.GetCurrentICoord(), 160 * PX);
        });
    }

    #[test]
    fn margins_move_and_widen_the_frame() {
        with_line("", &[("a|display:inline-block;width:30px;height:10px;margin:0 7px 0 5px", LayoutFrameType::Block)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            f[1].SetReflowStubSize(30 * PX, 10 * PX, 10 * PX);
            reflow(ll, f[1]);
            // the start margin is applied in CanPlaceFrame, the end margin advances the coordinate
            // (the frame's rect is set again when the line is aligned in the block direction)
            ll.VerticalAlignLine();
            assert_eq!(f[1].GetRect().x, 5 * PX);
            assert_eq!(f[1].GetRect().width, 30 * PX);
            assert_eq!(ll.GetCurrentICoord(), (5 + 30 + 7) * PX);
        });
    }

    #[test]
    fn text_indent_applies_to_the_first_line_only() {
        with_line("text-indent:20px", &[], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            assert_eq!(ll.GetCurrentICoord(), 20 * PX);
            ll.EndLineReflow();
        });
        with_line("text-indent:20px", &[], 3, |ll, f, _, _| {
            begin(ll, f, 100);
            assert_eq!(ll.GetCurrentICoord(), 0);
            ll.EndLineReflow();
        });
        // (`hanging` and `each-line` are not tested: Stylo of this configuration does not parse
        // the text-indent keywords, they are behind a pref, so the style never has them.)
    }

    #[test]
    fn the_baseline_aligns_text_of_different_sizes() {
        with_line("", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text)], 0, |ll, f, line, _| {
            begin(ll, f, 100);
            text(f[1], 40, 12, 10);
            text(f[2], 30, 14, 8);
            reflow(ll, f[1]);
            reflow(ll, f[2]);
            ll.VerticalAlignLine();
            // Relative to the baseline the first frame spans -10px..2px, the second -8px..6px.
            // The line is 16px tall, with its baseline 10px below the top.
            assert_eq!(f[1].GetRect(), nsRect::new(0, 0, 40 * PX, 12 * PX));
            assert_eq!(f[2].GetRect(), nsRect::new(40 * PX, 2 * PX, 30 * PX, 14 * PX));
            let line = unsafe { &*line };
            assert_eq!(line.BSize(), 16 * PX);
            assert_eq!(line.GetLogicalAscent(), 10 * PX);
            assert_eq!(line.BStart(), 0);
            assert_eq!(ll.mFinalLineBSize, 16 * PX);
        });
    }

    #[test]
    fn an_empty_line_has_no_height() {
        with_line("", &[], 0, |ll, f, line, _| {
            begin(ll, f, 100);
            ll.VerticalAlignLine();
            assert_eq!(unsafe { &*line }.BSize(), 0);
            assert_eq!(ll.mFinalLineBSize, 0);
        });
    }

    #[test]
    fn vertical_align_top_stretches_the_line() {
        with_line("", &[("a", LayoutFrameType::Text), ("b|display:inline-block;vertical-align:top;width:20px;height:30px", LayoutFrameType::Block)], 0, |ll, f, line, _| {
            begin(ll, f, 100);
            text(f[1], 40, 12, 10);
            f[2].SetReflowStubSize(20 * PX, 30 * PX, 30 * PX);
            reflow(ll, f[1]);
            reflow(ll, f[2]);
            ll.VerticalAlignLine();
            // The text alone would make a 12px line; the top-aligned box makes it 30px, and sits at its top.
            assert_eq!(unsafe { &*line }.BSize(), 30 * PX);
            assert_eq!(f[2].GetRect().y, 0);
            assert_eq!(f[1].GetRect().y, 0);
        });
    }

    #[test]
    fn vertical_align_bottom_sits_on_the_line_bottom() {
        with_line("", &[("a", LayoutFrameType::Text), ("b|display:inline-block;vertical-align:bottom;width:20px;height:30px", LayoutFrameType::Block)], 0, |ll, f, line, _| {
            begin(ll, f, 100);
            text(f[1], 40, 12, 10);
            f[2].SetReflowStubSize(20 * PX, 30 * PX, 30 * PX);
            reflow(ll, f[1]);
            reflow(ll, f[2]);
            ll.VerticalAlignLine();
            // A bottom-aligned box taller than the text: the line is as tall as the box, and the
            // baseline slides down by the extra amount so the text still sits on it.
            assert_eq!(unsafe { &*line }.BSize(), 30 * PX);
            assert_eq!(f[2].GetRect().y, 0);
            assert_eq!(f[1].GetRect().y, 18 * PX);
        });
    }

    // Two text frames of 40px and 30px on a 100px line, aligned and ready for TextAlignLine.
    fn aligned_line(ll: &mut nsLineLayout, f: &[nsIFrame], info: JustificationInfo) {
        begin(ll, f, 100);
        f[1].SetTextFrameStub(TextFrameStub { mISize: 40 * PX, mBSize: 12 * PX, mAscent: 10 * PX, mJustificationInfo: info, ..Default::default() });
        f[2].SetTextFrameStub(TextFrameStub { mISize: 30 * PX, mBSize: 12 * PX, mAscent: 10 * PX, mJustificationInfo: info, ..Default::default() });
        reflow(ll, f[1]);
        reflow(ll, f[2]);
        ll.VerticalAlignLine();
    }

    fn xs(f: &[nsIFrame]) -> (nscoord, nscoord) {
        (f[1].GetRect().x, f[2].GetRect().x)
    }

    #[test]
    fn text_align_start_center_end_and_right() {
        for (style, expected) in [("text-align:start", (0, 40)), ("text-align:center", (15, 55)), ("text-align:end", (30, 70)), ("text-align:right", (30, 70)), ("text-align:left", (0, 40))] {
            with_line(style, &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text)], 0, |ll, f, line, _| {
                aligned_line(ll, f, JustificationInfo::new());
                ll.TextAlignLine(unsafe { &mut *line }, false);
                assert_eq!(xs(f), (expected.0 * PX, expected.1 * PX), "{style}");
                // the line box moved with the frames
                assert_eq!(unsafe { &*line }.IStart(), expected.0 * PX, "{style}");
            });
        }
    }

    #[test]
    fn text_align_last_applies_to_the_last_line() {
        with_line("text-align:left;text-align-last:right", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text)], 0, |ll, f, line, _| {
            aligned_line(ll, f, JustificationInfo::new());
            ll.TextAlignLine(unsafe { &mut *line }, false);
            assert_eq!(xs(f), (0, 40 * PX));
            ll.TextAlignLine(unsafe { &mut *line }, true);
            assert_eq!(xs(f), (30 * PX, 70 * PX));
        });
    }

    #[test]
    fn a_line_that_is_full_is_not_aligned() {
        with_line("text-align:right", &[("a", LayoutFrameType::Text)], 0, |ll, f, line, _| {
            begin(ll, f, 100);
            text(f[1], 100, 12, 10);
            reflow(ll, f[1]);
            ll.VerticalAlignLine();
            ll.TextAlignLine(unsafe { &mut *line }, false);
            assert_eq!(f[1].GetRect().x, 0);
        });
    }

    #[test]
    fn text_align_justify_spreads_the_free_space_over_the_gaps() {
        let info = JustificationInfo { mInnerOpportunities: 0, mIsStartJustifiable: true, mIsEndJustifiable: true };
        with_line("text-align:justify", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text)], 0, |ll, f, line, _| {
            aligned_line(ll, f, info);
            ll.TextAlignLine(unsafe { &mut *line }, false);
            // One opportunity between the frames: two gaps, 15px each of the 30px that were free.
            assert_eq!(f[1].GetRect(), nsRect::new(0, 0, 55 * PX, 12 * PX));
            assert_eq!(f[2].GetRect(), nsRect::new(55 * PX, 0, 45 * PX, 12 * PX));
            assert_eq!(f[1].TextFrameStub().mAssignedGaps, Some(JustificationAssignment { mGapsAtStart: 0, mGapsAtEnd: 1 }));
            assert_eq!(f[2].TextFrameStub().mAssignedGaps, Some(JustificationAssignment { mGapsAtStart: 1, mGapsAtEnd: 0 }));
            assert_eq!(unsafe { &*line }.ISize(), 100 * PX);
        });
    }

    #[test]
    fn text_align_justify_without_opportunities_falls_back_to_start() {
        with_line("text-align:justify", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text)], 0, |ll, f, line, _| {
            aligned_line(ll, f, JustificationInfo::new());
            ll.TextAlignLine(unsafe { &mut *line }, false);
            assert_eq!(xs(f), (0, 40 * PX));
        });
    }

    #[test]
    fn justification_counts_inner_opportunities_of_text() {
        let info = JustificationInfo { mInnerOpportunities: 2, mIsStartJustifiable: true, mIsEndJustifiable: true };
        with_line("text-align:justify", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text)], 0, |ll, f, line, _| {
            aligned_line(ll, f, info);
            ll.TextAlignLine(unsafe { &mut *line }, false);
            // 2 + 2 inner opportunities and one between the frames: 5 opportunities, 10 gaps of 3px.
            // The first frame has 4 gaps (2 inner x 2) and one gap at its end: 5 gaps = 15px.
            assert_eq!(f[1].GetRect().width, (40 + 15) * PX);
            assert_eq!(f[2].GetRect().x, (40 + 15) * PX);
            assert_eq!(f[2].GetRect().width, (30 + 15) * PX);
        });
    }

    #[test]
    fn trailing_white_space_is_trimmed() {
        with_line("", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            text(f[1], 40, 12, 10);
            f[2].SetTextFrameStub(TextFrameStub {
                mISize: 30 * PX,
                mBSize: 12 * PX,
                mAscent: 10 * PX,
                mTrimOutput: nsTextFrame_TrimOutput { mChanged: true, mDeltaWidth: 5 * PX },
                mJustificationInfo: JustificationInfo { mInnerOpportunities: 1, mIsStartJustifiable: true, mIsEndJustifiable: true },
                ..Default::default()
            });
            reflow(ll, f[1]);
            reflow(ll, f[2]);
            assert!(ll.TrimTrailingWhiteSpace());
            assert_eq!(ll.GetCurrentICoord(), 65 * PX);
            let last = PFD(PSD(ll.mRootSpan).mLastFrame);
            assert_eq!(last.mBounds.ISize(ll.GetWritingMode()), 25 * PX);
            assert!(last.mRecomputeOverflow);
            // the space that was trimmed no longer gives an expansion opportunity
            assert_eq!(last.mJustificationInfo.mInnerOpportunities, 0);
            // nothing left to trim
            assert!(!ll.TrimTrailingWhiteSpace());
        });
    }

    #[test]
    fn a_trailing_non_text_frame_stops_the_trim() {
        with_line("", &[("a", LayoutFrameType::Text), ("b|display:inline-block;width:10px;height:10px", LayoutFrameType::Block)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            f[1].SetTextFrameStub(TextFrameStub {
                mISize: 40 * PX,
                mBSize: 12 * PX,
                mAscent: 10 * PX,
                mTrimOutput: nsTextFrame_TrimOutput { mChanged: true, mDeltaWidth: 5 * PX },
                ..Default::default()
            });
            f[2].SetReflowStubSize(10 * PX, 10 * PX, 10 * PX);
            reflow(ll, f[1]);
            reflow(ll, f[2]);
            // an atomic inline follows the text: there is no trailing white space
            assert!(!ll.TrimTrailingWhiteSpace());
            assert_eq!(ll.GetCurrentICoord(), 50 * PX);
        });
    }

    #[test]
    fn update_band_moves_the_line_and_its_frames() {
        with_line("", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Block)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            let wm = f[0].GetWritingMode();
            text(f[1], 40, 12, 10);
            reflow(ll, f[1]);
            assert!(!ll.mImpactedByFloats);
            // a float took the first 30px of the line
            ll.UpdateBand(wm, LogicalRect::new(wm, 30 * PX, 0, 70 * PX, 20 * PX), f[2]);
            assert!(ll.mImpactedByFloats);
            assert_eq!(PSD(ll.mRootSpan).mIStart, 30 * PX);
            assert_eq!(PSD(ll.mRootSpan).mIEnd, 100 * PX);
            assert_eq!(ll.GetCurrentICoord(), 70 * PX);
            assert_eq!(PFD(PSD(ll.mRootSpan).mFirstFrame).mBounds.IStart(wm), 30 * PX);
            assert!(PSD(ll.mRootSpan).mContainsFloat);
        });
    }

    #[test]
    fn optional_break_positions_and_forced_breaks() {
        with_line("", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            assert!(!ll.HasOptionalBreakPosition());
            assert!(!ll.NotifyOptionalBreakPosition(Some(f[1]), 3, true, gfxBreakPriority::eNormalBreak));
            assert!(ll.HasOptionalBreakPosition());
            let (mut offset, mut priority) = (0, gfxBreakPriority::eNoBreak);
            assert_eq!(ll.GetLastOptionalBreakPosition(&mut offset, &mut priority), Some(f[1]));
            assert_eq!((offset, priority), (3, gfxBreakPriority::eNormalBreak));
            // a later break that fits replaces it if its priority is at least as high...
            ll.NotifyOptionalBreakPosition(Some(f[2]), 5, true, gfxBreakPriority::eWordWrapBreak);
            assert_eq!(ll.GetLastOptionalBreakPosition(&mut offset, &mut priority), Some(f[1]));
            ll.NotifyOptionalBreakPosition(Some(f[2]), 5, true, gfxBreakPriority::eNormalBreak);
            assert_eq!(ll.GetLastOptionalBreakPosition(&mut offset, &mut priority), Some(f[2]));
            // ...and a forced break position makes the notification say "break here"
            assert!(!ll.HaveForcedBreakPosition());
            ll.ForceBreakAtPosition(f[2], 5);
            assert!(ll.HaveForcedBreakPosition());
            assert_eq!(ll.GetForcedBreakPosition(f[2]), 5);
            assert_eq!(ll.GetForcedBreakPosition(f[1]), -1);
            assert!(ll.NotifyOptionalBreakPosition(Some(f[2]), 5, true, gfxBreakPriority::eNormalBreak));
            ll.ClearOptionalBreakPosition();
            assert!(!ll.HasOptionalBreakPosition());
        });
    }

    #[test]
    fn split_line_to_keeps_the_first_frames() {
        with_line("", &[("a", LayoutFrameType::Text), ("b", LayoutFrameType::Text), ("c", LayoutFrameType::Text)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            for i in 1..=3 {
                text(f[i], 10, 12, 10);
                reflow(ll, f[i]);
            }
            assert_eq!(ll.GetCurrentSpanCount(), 3);
            ll.SplitLineTo(2);
            assert_eq!(ll.GetCurrentSpanCount(), 2);
            assert_eq!(PFD(ll.LastFrame()).mFrame, f[2]);
        });
    }

    #[test]
    fn a_placeholder_without_float_takes_no_room() {
        with_line("", &[("a", LayoutFrameType::Placeholder)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            f[1].SetReflowStubSize(0, 0, 0);
            let (pushed, _) = reflow(ll, f[1]);
            assert!(!pushed);
            assert!(ll.LineIsEmpty());
            assert!(!ll.LineIsBreakable());
            let pfd = PFD(ll.LastFrame());
            assert!(pfd.mIsPlaceholder && pfd.mIsEmpty && pfd.mSkipWhenTrimmingWhitespace);
        });
    }

    #[test]
    fn a_percentage_width_disables_the_resize_optimization() {
        with_line("", &[("a|display:inline-block;width:50%;height:10px", LayoutFrameType::Block), ("b|display:inline-block;width:10px;height:10px", LayoutFrameType::Block)], 0, |ll, f, line, _| {
            begin(ll, f, 100);
            f[1].SetReflowStubSize(50 * PX, 10 * PX, 10 * PX);
            f[2].SetReflowStubSize(10 * PX, 10 * PX, 10 * PX);
            reflow(ll, f[2]);
            assert!(!unsafe { &*line }.ResizeReflowOptimizationDisabled());
            reflow(ll, f[1]);
            assert!(unsafe { &*line }.ResizeReflowOptimizationDisabled());
        });
    }

    #[test]
    fn relative_positioning_moves_the_frame_and_extends_the_overflow() {
        with_line("", &[("a|display:inline-block;position:relative;left:10px;width:20px;height:10px", LayoutFrameType::Block)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            f[1].SetReflowStubSize(20 * PX, 10 * PX, 10 * PX);
            reflow(ll, f[1]);
            ll.VerticalAlignLine();
            assert_eq!(f[1].GetRect().x, 0);
            let mut overflow = OverflowAreas::default();
            ll.RelativePositionFrames__OverflowAreas(&mut overflow);
            assert_eq!(f[1].GetRect().x, 10 * PX);
            // the line box (20px wide) united with the frame in its new place
            assert_eq!(overflow.InkOverflow().x, 0);
            assert_eq!(overflow.InkOverflow().XMost(), 30 * PX);
            assert_eq!(overflow.ScrollableOverflow().XMost(), 30 * PX);
        });
    }

    #[test]
    fn text_overflow_is_stored_on_the_frame() {
        with_line("", &[("a", LayoutFrameType::Text)], 0, |ll, f, _, _| {
            begin(ll, f, 100);
            text(f[1], 40, 12, 10);
            reflow(ll, f[1]);
            ll.VerticalAlignLine();
            let mut overflow = OverflowAreas::default();
            ll.RelativePositionFrames__OverflowAreas(&mut overflow);
            assert_eq!(overflow.InkOverflow().width, 40 * PX);
            assert_eq!(f[1].GetOverflowAreas().InkOverflow().width, 40 * PX);
        });
    }

    // An inline container: the Reflow of nsInlineFrame calls BeginSpan, ReflowFrame for each
    // child and EndSpan on the line layout it finds in its reflow input.
    #[test]
    fn a_span_lays_out_its_children_relative_to_itself() {
        with_line("", &[("s|display:inline", LayoutFrameType::Inline), ("t1", LayoutFrameType::Text), ("t2", LayoutFrameType::Text)], 0, |ll, f, line, _| {
            begin(ll, f, 100);
            let (s, t1, t2) = (f[1], f[2], f[3]);
            text(t1, 40, 12, 10);
            text(t2, 30, 12, 10);
            // (the frame data of the span lives for the whole line, like nsInlineFrame::mBaseline)
            let baseline: *mut nscoord = Box::leak(Box::new(0));
            let baseline_addr = baseline as usize;
            s.SetReflowStub(FrameReflowStub::Func(std::rc::Rc::new(move |frame, _pc, metrics, ri, _status| {
                let ll = unsafe { &mut *ri.mLineLayout };
                ll.BeginSpan(frame, ri, 0, 100 * PX, baseline_addr as *mut nscoord);
                let (mut status, mut pushed) = (nsReflowStatus::new(), false);
                ll.ReflowFrame(t1, &mut status, None, &mut pushed);
                ll.ReflowFrame(t2, &mut status, None, &mut pushed);
                let wm = metrics.GetWritingMode();
                *metrics.ISize_mut(wm) = ll.EndSpan(frame);
                *metrics.BSize_mut(wm) = 15 * PX;
                metrics.SetBlockStartAscent(12 * PX);
            })));
            let (pushed, _) = reflow(ll, s);
            assert!(!pushed);
            assert_eq!(ll.GetCurrentSpanCount(), 1);
            assert_eq!(ll.GetCurrentICoord(), 70 * PX);
            // children are positioned inside the span
            assert_eq!(t1.GetRect().x, 0);
            assert_eq!(t2.GetRect().x, 40 * PX);
            assert_eq!(s.GetRect().width, 70 * PX);
            // the span's text sits on the baseline the span reported: ascent 12px, so child tops are at 12 - 10
            assert_eq!(unsafe { *baseline }, 12 * PX);
            ll.VerticalAlignLine();
            let line = unsafe { &*line };
            assert!(line.BSize() >= 15 * PX);
            // a span with only text is not empty
            let pfd = PFD(PSD(ll.mRootSpan).mFirstFrame);
            assert!(!pfd.mIsEmpty);
            assert!(!pfd.mSpan.is_null());
        });
    }

    #[test]
    fn an_empty_span_does_not_make_the_line_non_empty() {
        with_line("", &[("s|display:inline", LayoutFrameType::Inline)], 0, |ll, f, line, _| {
            begin(ll, f, 100);
            let s = f[1];
            s.SetSelfEmpty(true);
            let baseline: *mut nscoord = Box::leak(Box::new(0));
            let baseline_addr = baseline as usize;
            s.SetReflowStub(FrameReflowStub::Func(std::rc::Rc::new(move |frame, _pc, metrics, ri, _status| {
                let ll = unsafe { &mut *ri.mLineLayout };
                ll.BeginSpan(frame, ri, 0, 100 * PX, baseline_addr as *mut nscoord);
                let wm = metrics.GetWritingMode();
                *metrics.ISize_mut(wm) = ll.EndSpan(frame);
                *metrics.BSize_mut(wm) = 0;
                metrics.SetBlockStartAscent(0);
            })));
            reflow(ll, s);
            assert!(ll.LineIsEmpty());
            ll.VerticalAlignLine();
            // an empty line: the inline frame is collapsed to zero block-size
            assert_eq!(unsafe { &*line }.BSize(), 0);
            assert_eq!(s.GetRect().height, 0);
        });
    }
}
