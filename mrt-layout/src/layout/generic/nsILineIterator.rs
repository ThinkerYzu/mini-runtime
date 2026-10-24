// Translated from layout/generic/nsILineIterator.h and nsILineIterator.cpp (GECKO_REV).
// (nsLineBox.rs needs the interface that nsLineIterator implements.)
use crate::*;

/**
 * Line iterator API.
 *
 * Lines are numbered from 0 to N, where 0 is the top line and N is
 * the bottom line.
 *
 * Obtain this interface from frames via nsIFrame::GetLineIterator.
 * This iterator belongs to the frame from which it was obtained, and should
 * not be deleted by the caller.
 * Note that any modification of the frame will invalidate the iterator!
 * Users must get a new iterator any time the target may have been touched.
 */
// Gecko: layout/generic/nsILineIterator.h nsILineIterator
pub trait nsILineIterator {
    /**
     * The number of lines in the block
     */
    fn GetNumLines(&self) -> i32;

    /**
     * Returns whether our lines are rtl.
     */
    fn IsLineIteratorFlowRTL(&mut self) -> bool;

    // Return miscellaneous information about a line.
    // (mozilla::Result<LineInfo, nsresult> is Result<nsILineIterator_LineInfo, nsresult>.)
    fn GetLine(&mut self, aLineNumber: i32) -> Result<nsILineIterator_LineInfo, nsresult>;

    /**
     * Given a frame that's a child of the block, find which line its on
     * and return that line index, as long as it's at least as big as
     * aStartLine.  Returns -1 if the frame cannot be found on lines
     * starting with aStartLine.
     */
    // (The default argument aStartLine = 0 is passed explicitly by callers.)
    fn FindLineContaining(&mut self, aFrame: nsIFrame, aStartLine: i32) -> i32;

    // Given a line number and a coordinate, find the frame on the line
    // that is nearest to aPos along the inline axis. (The block-axis coord
    // of aPos is irrelevant.)
    // The aPosIsBeforeFirstFrame and aPosIsAfterLastFrame flags are updated
    // appropriately.
    fn FindFrameAt(&mut self, aLineNumber: i32, aPos: nsPoint, aFrameFound: &mut Option<nsIFrame>, aPosIsBeforeFirstFrame: &mut bool, aPosIsAfterLastFrame: &mut bool) -> nsresult;

    // Check whether visual and logical order of frames within a line are
    // identical.
    //  If not, return the first and last visual frames
    fn CheckLineOrder(&mut self, aLine: i32, aIsReordered: &mut bool, aFirstVisual: &mut Option<nsIFrame>, aLastVisual: &mut Option<nsIFrame>) -> nsresult;
}

// Gecko: layout/generic/nsILineIterator.h nsILineIterator::LineInfo
#[derive(Clone, Copy, Debug)]
pub struct nsILineIterator_LineInfo {
    /** The first frame on the line. */
    pub mFirstFrameOnLine: Option<nsIFrame>,
    /** The numbers of frames on the line. */
    pub mNumFramesOnLine: i32,
    /**
     * The bounding box of the line (which is based on the in-flow position of
     * the frames on the line; if a frame was moved because of relative
     * positioning then its coordinates may be outside the line bounds)
     */
    pub mLineBounds: nsRect,
    /** Whether the line is wrapped at the end */
    pub mIsWrapped: bool,
}

impl Default for nsILineIterator_LineInfo {
    fn default() -> nsILineIterator_LineInfo {
        nsILineIterator_LineInfo { mFirstFrameOnLine: None, mNumFramesOnLine: 0, mLineBounds: nsRect::default(), mIsWrapped: false }
    }
}

impl nsILineIterator_LineInfo {
    /**
     * Return last frame of the line if there is no enough siblings of
     * mFirstFrameOnLine.
     * Otherwise, nullptr including in the unexpected error cases.
     */
    // Gecko: layout/generic/nsIFrame.cpp nsILineIterator::LineInfo::GetLastFrameOnLine
    // (the .cpp of this header does not define it; nsIFrame.cpp does)
    pub fn GetLastFrameOnLine(&self) -> Option<nsIFrame> {
        if self.mNumFramesOnLine == 0 {
            return None; // empty line, not illegal
        }
        MOZ_ASSERT!(self.mFirstFrameOnLine.is_some());
        let mut maybeLastFrame = self.mFirstFrameOnLine.unwrap();
        for i in 0..(self.mNumFramesOnLine - 1) {
            match maybeLastFrame.GetNextSibling() {
                Some(next) => maybeLastFrame = next,
                None => {
                    NS_WARNING!("GetLastFrameOnLine: not enough siblings");
                    return None;
                }
            }
        }
        Some(maybeLastFrame)
    }
}

// Gecko: layout/generic/nsILineIterator.h mozilla::LineFrameFinder
// Helper struct for FindFrameAt.
pub struct LineFrameFinder {
    pub mPos: LogicalPoint,
    pub mContainerSize: nsSize,
    pub mWM: WritingMode,
    pub mIsReversed: bool,

    mDone: bool,
    mFirstFrame: Option<nsIFrame>,
    mClosestFromStart: Option<nsIFrame>,
    mClosestFromEnd: Option<nsIFrame>,
}

impl LineFrameFinder {
    // Gecko: layout/generic/nsILineIterator.h LineFrameFinder::LineFrameFinder
    pub fn new(aPos: nsPoint, aContainerSize: nsSize, aWM: WritingMode, aIsReversed: bool) -> LineFrameFinder {
        LineFrameFinder {
            mPos: LogicalPoint::new__WritingMode_nsPoint_nsSize(aWM, aPos, aContainerSize),
            mContainerSize: aContainerSize,
            mWM: aWM,
            mIsReversed: aIsReversed,
            mDone: false,
            mFirstFrame: None,
            mClosestFromStart: None,
            mClosestFromEnd: None,
        }
    }

    // ADAPTER: nsIFrame::GetLogicalRect(aWM, aContainerSize) of nsIFrame.h, inlined (it is
    // `LogicalRect(aWritingMode, GetRect(), aContainerSize)`).
    fn GetLogicalRect(&self, aFrame: nsIFrame) -> LogicalRect {
        LogicalRect::new__WritingMode_nsRect_nsSize(self.mWM, aFrame.GetRect(), self.mContainerSize)
    }

    // Gecko: layout/generic/nsILineIterator.h LineFrameFinder::IsDone
    pub fn IsDone(&self) -> bool {
        self.mDone
    }

    // Gecko: layout/generic/nsILineIterator.cpp LineFrameFinder::Scan
    pub fn Scan(&mut self, aFrame: nsIFrame) {
        if self.mDone {
            return;
        }

        if self.mFirstFrame.is_none() {
            self.mFirstFrame = Some(aFrame);
        }

        let rect = self.GetLogicalRect(aFrame);
        if rect.ISize(self.mWM) == 0 {
            return;
        }
        // If pos.I() is inside this frame - this is it
        if rect.IStart(self.mWM) <= self.mPos.I(self.mWM) && rect.IEnd(self.mWM) > self.mPos.I(self.mWM) {
            self.mClosestFromStart = Some(aFrame);
            self.mClosestFromEnd = Some(aFrame);
            self.mDone = true;
            return;
        }
        if rect.IStart(self.mWM) < self.mPos.I(self.mWM) {
            if self.mClosestFromStart.is_none() || rect.IEnd(self.mWM) > self.GetLogicalRect(self.mClosestFromStart.unwrap()).IEnd(self.mWM) {
                self.mClosestFromStart = Some(aFrame);
            }
        } else if self.mClosestFromEnd.is_none() || rect.IStart(self.mWM) < self.GetLogicalRect(self.mClosestFromEnd.unwrap()).IStart(self.mWM) {
            self.mClosestFromEnd = Some(aFrame);
        }
    }

    // Gecko: layout/generic/nsILineIterator.cpp LineFrameFinder::Finish
    pub fn Finish(&mut self, aFrameFound: &mut Option<nsIFrame>, aPosIsBeforeFirstFrame: &mut bool, aPosIsAfterLastFrame: &mut bool) {
        if self.mClosestFromStart.is_none() && self.mClosestFromEnd.is_none() {
            // All frames were zero-width. Just take the first one.
            self.mClosestFromStart = self.mFirstFrame;
            self.mClosestFromEnd = self.mFirstFrame;
        }
        *aPosIsBeforeFirstFrame = if self.mIsReversed { self.mClosestFromEnd.is_none() } else { self.mClosestFromStart.is_none() };
        *aPosIsAfterLastFrame = if self.mIsReversed { self.mClosestFromStart.is_none() } else { self.mClosestFromEnd.is_none() };
        if self.mClosestFromStart == self.mClosestFromEnd {
            *aFrameFound = self.mClosestFromStart;
        } else if self.mClosestFromStart.is_none() {
            *aFrameFound = self.mClosestFromEnd;
        } else if self.mClosestFromEnd.is_none() {
            *aFrameFound = self.mClosestFromStart;
        } else {
            // we're between two frames
            let delta = self.GetLogicalRect(self.mClosestFromEnd.unwrap()).IStart(self.mWM) - self.GetLogicalRect(self.mClosestFromStart.unwrap()).IEnd(self.mWM);
            if self.mPos.I(self.mWM) < self.GetLogicalRect(self.mClosestFromStart.unwrap()).IEnd(self.mWM) + delta / 2 {
                *aFrameFound = self.mClosestFromStart;
            } else {
                *aFrameFound = self.mClosestFromEnd;
            }
        }
    }
}

/**
 * Helper intended to be used in a scope where we're using an nsILineIterator
 * and want to verify that no DOM mutations (which would invalidate the
 * iterator) occur while we're using it.
 */
// Gecko: layout/generic/nsILineIterator.h AutoAssertNoDomMutations
// NOT PORTED: the nsMutationGuard (there is no DOM mutation counter in this runtime); the
// type exists so that `let guard = AutoAssertNoDomMutations::default();` stays in translated code.
#[derive(Default)]
pub struct AutoAssertNoDomMutations {}
