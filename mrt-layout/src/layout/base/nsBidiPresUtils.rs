// Gecko: layout/base/nsBidiPresUtils.h and nsBidiPresUtils.cpp
//
// Bidi resolution of a block's paragraphs: the part that decides embedding levels and splits
// frames at level changes (Resolve, ResolveParagraph, TraverseFrames, ChildListMayRequireBidi,
// BidiParagraphData and the continuation helpers).
//
// NOT PORTED: the ruby variants of the repositioning (RepositionRubyFrame,
// RepositionRubyContentFrame), the caret and selection helpers (GetFrameToRightOf,
// GetFrameToLeftOf, CheckLineOrder, IsReversedDirectionFrame, ...), and the rendering helpers
// (FormatUnicodeText, ProcessText, ProcessSimpleRun, RenderText, MeasureTextWidth,
// StripBidiControlCharacters, CalculateBidiClass).
//
// ADAPTER (what differs from the C++):
//  - NS_BIDI_CONTROL_FRAME, the pointer 0xfffb1d1 that stands for "a virtual bidi control
//    character", is the frame id 0xfffb1d1 (no frame has such an id).
//  - mPresContext->BidiEngine() is one `Bidi` per nsPresContext in Gecko; the pres context here is a
//    small Copy value, so each BidiParagraphData has its own (it is used by one paragraph at a time
//    in Gecko too).
//  - AppendFrame and AdvanceAndAppendFrame take the line iterator as a parameter in C++, and the
//    only one ever passed is mCurrentTraverseLine, a member of the same object: Rust cannot borrow
//    both, so they use the member.
//  - Visual mode (mIsVisual) is never on here, so the form-control check that switches it off for
//    form controls is not translated.
//  - text-orientation: upright in vertical writing modes (the first test of GetBidiOverride) is not
//    translated: only horizontal text is laid out here.
use crate::*;
use crate::nsLineLayoutAdapter::nsStyleTextAdapter;
use mrt_style::style::properties::ComputedValues;
use mrt_style::style::servo_arc::Arc as ServoArc;
use std::collections::HashMap;

const kNextLine: u16 = 0x0085;
const kZWSP: u16 = 0x200B;
const kLineSeparator: u16 = 0x2028;
const kParagraphSeparator: u16 = 0x2029;
const kObjectSubstitute: u16 = 0xFFFC;
const kLRE: u16 = 0x202A;
const kRLE: u16 = 0x202B;
const kLRO: u16 = 0x202D;
const kRLO: u16 = 0x202E;
const kPDF: u16 = 0x202C;
const kLRI: u16 = 0x2066;
const kRLI: u16 = 0x2067;
const kFSI: u16 = 0x2068;
const kPDI: u16 = 0x2069;

const NS_BIDI_CONTROL_FRAME: nsIFrame = nsIFrame(0xfffb1d1);

// Gecko: layout/base/nsBidiPresUtils.cpp IsIsolateControl
fn IsIsolateControl(aChar: u16) -> bool {
    aChar == kLRI || aChar == kRLI || aChar == kFSI
}

// Given a ComputedStyle, return any bidi control character necessary to
// implement style properties that override directionality (i.e. if it has
// unicode-bidi:bidi-override, or text-orientation:upright in vertical
// writing mode) when applying the bidi algorithm.
//
// Returns 0 if no override control character is implied by this style.
// Gecko: layout/base/nsBidiPresUtils.cpp GetBidiOverride
fn GetBidiOverride(aComputedStyle: &ServoArc<ComputedValues>) -> u16 {
    // (the text-orientation: upright test: NOT PORTED, horizontal text only)
    let unicodeBidi = aComputedStyle.get_text().unicode_bidi;
    if unicodeBidi == StyleUnicodeBidi::BidiOverride || unicodeBidi == StyleUnicodeBidi::IsolateOverride {
        return if StyleDirection::Rtl == aComputedStyle.get_inherited_box().direction { kRLO } else { kLRO };
    }
    0
}

// Given a ComputedStyle, return any bidi control character necessary to
// implement style properties that affect bidi resolution (i.e. if it
// has unicode-bidiembed, isolate, or plaintext) when applying the bidi
// algorithm.
//
// Returns 0 if no control character is implied by the style.
//
// Note that GetBidiOverride and GetBidiControl need to be separate
// because in the case of unicode-bidi:isolate-override we need both
// FSI and LRO/RLO.
// Gecko: layout/base/nsBidiPresUtils.cpp GetBidiControl
fn GetBidiControl(aComputedStyle: &ServoArc<ComputedValues>) -> u16 {
    let direction = aComputedStyle.get_inherited_box().direction;
    match aComputedStyle.get_text().unicode_bidi {
        StyleUnicodeBidi::Embed => {
            if StyleDirection::Rtl == direction {
                kRLE
            } else {
                kLRE
            }
        }
        StyleUnicodeBidi::Isolate => {
            // <bdi> element already has its directionality set from content so
            // we never need to return kFSI.
            if StyleDirection::Rtl == direction {
                kRLI
            } else {
                kLRI
            }
        }
        StyleUnicodeBidi::IsolateOverride | StyleUnicodeBidi::Plaintext => kFSI,
        StyleUnicodeBidi::Normal | StyleUnicodeBidi::BidiOverride => 0,
    }
}

// Gecko: layout/base/nsBidiPresUtils.cpp AreContinuationsInOrder (DEBUG only)
fn AreContinuationsInOrder(aFrame1: nsIFrame, aFrame2: nsIFrame) -> bool {
    let mut f = Some(aFrame1);
    loop {
        f = f.and_then(|f| f.GetNextContinuation());
        if f.is_none() || f == Some(aFrame2) {
            break;
        }
    }
    f.is_some()
}

// Gecko: layout/base/nsBidiPresUtils.cpp BidiParagraphData::FrameInfo
#[derive(Clone, Copy)]
pub struct BidiParagraphData_FrameInfo {
    pub mFrame: Option<nsIFrame>,

    // The block containing mFrame (i.e., which continuation).
    pub mBlockContainer: Option<nsBlockFrame>,

    // true if mFrame is in mBlockContainer's overflow lines, false if
    // in primary lines
    pub mInOverflow: bool,
}

impl BidiParagraphData_FrameInfo {
    // Gecko: FrameInfo(nsIFrame* aFrame, nsBlockInFlowLineIterator& aLineIter)
    pub fn new__nsIFrame_nsBlockInFlowLineIterator(aFrame: nsIFrame, aLineIter: &nsBlockInFlowLineIterator) -> Self {
        BidiParagraphData_FrameInfo { mFrame: Some(aFrame), mBlockContainer: aLineIter.GetContainerOpt(), mInOverflow: aLineIter.GetInOverflowOpt() }
    }

    // Gecko: explicit FrameInfo(BidiControlFrameType aValue)
    pub fn new__BidiControlFrameType() -> Self {
        BidiParagraphData_FrameInfo { mFrame: Some(NS_BIDI_CONTROL_FRAME), mBlockContainer: None, mInOverflow: false }
    }

    // Gecko: FrameInfo()
    pub fn new() -> Self {
        BidiParagraphData_FrameInfo { mFrame: None, mBlockContainer: None, mInOverflow: false }
    }
}

/**
 * This class is designed to manage the process of mapping a frame to
 * the line that it's in, when we know that (a) the frames we ask it
 * about are always in the block's lines and (b) each successive frame
 * we ask it about is the same as or after (in depth-first search
 * order) the previous.
 *
 * Since we move through the lines at a different pace in Traverse and
 * ResolveParagraph, we use one of these for each.
 *
 * The state of the mapping is also different between TraverseFrames
 * and ResolveParagraph since since resolving can call functions
 * (EnsureBidiContinuation or SplitInlineAncestors) that can create
 * new frames and thus break lines.
 *
 * The TraverseFrames iterator is only used in some edge cases.
 */
// Gecko: layout/base/nsBidiPresUtils.cpp BidiParagraphData::FastLineIterator
pub struct BidiParagraphData_FastLineIterator {
    // These iterators *and* mPrevFrame track the line list that we're
    // iterating over.
    //
    // mPrevFrame, if non-null, should be either the frame we're currently
    // handling (in ResolveParagraph or TraverseFrames, depending on the
    // iterator) or a frame before it, and is also guaranteed to either be in
    // mCurrentLine or have been in mCurrentLine until recently.
    //
    // In case the splitting causes block frames to break lines, however, we
    // also track the first frame of the next line.  If that changes, it means
    // we've broken lines and we have to invalidate mPrevFrame.
    pub mLineIterator: nsBlockInFlowLineIterator,
    pub mPrevFrame: Option<nsIFrame>,
    pub mNextLineStart: Option<nsIFrame>,
}

impl BidiParagraphData_FastLineIterator {
    // Gecko: FastLineIterator()
    pub fn new() -> Self {
        BidiParagraphData_FastLineIterator { mLineIterator: nsBlockInFlowLineIterator::new(), mPrevFrame: None, mNextLineStart: None }
    }

    // Gecko: FastLineIterator::GetLine
    pub fn GetLine(&self) -> nsLineList_iterator {
        self.mLineIterator.GetLine()
    }

    // Gecko: FastLineIterator::IsFrameInCurrentLine
    pub fn IsFrameInCurrentLine(aLineIter: &nsBlockInFlowLineIterator, aPrevFrame: Option<nsIFrame>, aFrame: nsIFrame) -> bool {
        // SAFETY: the iterator points at a line of its list (GetLine of a valid iterator)
        let line = unsafe { &*aLineIter.GetLine().get() };
        MOZ_ASSERT!(aPrevFrame.is_none() || line.Contains(aPrevFrame.unwrap()), "aPrevFrame must be in aLineIter's current line");
        let endFrame = if aLineIter.IsLastLineInList() { None } else { unsafe { &*aLineIter.GetLine().next_().get() }.mFirstChild };
        let startFrame = if aPrevFrame.is_some() { aPrevFrame } else { line.mFirstChild };
        let mut frame = startFrame;
        while let Some(f) = frame {
            if Some(f) == endFrame {
                break;
            }
            if f == aFrame {
                return true;
            }
            frame = f.GetNextSibling();
        }
        false
    }

    // Gecko: FastLineIterator::FirstChildOfNextLine
    pub fn FirstChildOfNextLine(aIterator: &nsBlockInFlowLineIterator) -> Option<nsIFrame> {
        let line = aIterator.GetLine();
        let lineEnd = aIterator.End();
        MOZ_ASSERT!(line != lineEnd, "iterator should start off valid");
        let nextLine = line.next_();

        if nextLine != lineEnd {
            unsafe { &*nextLine.get() }.mFirstChild
        } else {
            None
        }
    }

    // Advance line iterator to the line containing aFrame, assuming
    // that aFrame is already in the line list our iterator is iterating
    // over.
    // Gecko: FastLineIterator::AdvanceToFrame
    pub fn AdvanceToFrame(&mut self, aFrame: nsIFrame) {
        if self.mPrevFrame.is_some() && Self::FirstChildOfNextLine(&self.mLineIterator) != self.mNextLineStart {
            // Something has caused a line to split.  We need to invalidate
            // mPrevFrame since it may now be in a *later* line, though it may
            // still be in this line, so we need to start searching for it from
            // the start of this line.
            self.mPrevFrame = None;
        }
        let mut child = aFrame;
        let mut parent = nsLayoutUtils::GetParentOrPlaceholderFor(child);
        while let Some(p) = parent {
            if p.IsBlockFrameOrSubclass() {
                break;
            }
            child = p;
            parent = nsLayoutUtils::GetParentOrPlaceholderFor(child);
        }
        MOZ_ASSERT!(parent.is_some(), "aFrame is not a descendent of a block frame");
        while !Self::IsFrameInCurrentLine(&self.mLineIterator, self.mPrevFrame, child) {
            let hasNext = self.mLineIterator.Next();
            MOZ_ASSERT!(hasNext, "Can't find frame in lines!");
            self.mPrevFrame = None;
        }
        self.mPrevFrame = Some(child);
        self.mNextLineStart = Self::FirstChildOfNextLine(&self.mLineIterator);
    }

    // Advance line iterator to the line containing aFrame, which may
    // require moving forward into overflow lines or into a later
    // continuation (or both).
    // Gecko: FastLineIterator::AdvanceToLinesAndFrame
    pub fn AdvanceToLinesAndFrame(&mut self, aFrameInfo: &BidiParagraphData_FrameInfo) {
        if self.mLineIterator.GetContainerOpt() != aFrameInfo.mBlockContainer || self.mLineIterator.GetInOverflowOpt() != aFrameInfo.mInOverflow {
            MOZ_ASSERT!(
                if self.mLineIterator.GetContainerOpt() == aFrameInfo.mBlockContainer {
                    !self.mLineIterator.GetInOverflowOpt() && aFrameInfo.mInOverflow
                } else {
                    self.mLineIterator.GetContainerOpt().is_none() || AreContinuationsInOrder(self.mLineIterator.GetContainerOpt().unwrap().0, aFrameInfo.mBlockContainer.unwrap().0)
                },
                "must move forwards"
            );
            let block = aFrameInfo.mBlockContainer.unwrap();
            let lines = if aFrameInfo.mInOverflow { unsafe { &mut *block.GetOverflowLines() }.mLines.begin_mut() } else { block.LinesBegin() };
            self.mLineIterator = nsBlockInFlowLineIterator::new__nsBlockFrame_LineIterator_bool(block, lines, aFrameInfo.mInOverflow);
            self.mPrevFrame = None;
        }
        self.AdvanceToFrame(aFrameInfo.mFrame.unwrap());
    }
}

// Gecko: layout/base/nsBidiPresUtils.cpp BidiParagraphData
pub struct BidiParagraphData {
    pub mBuffer: Vec<u16>,
    pub mEmbeddingStack: Vec<u16>,
    pub mLogicalFrames: Vec<BidiParagraphData_FrameInfo>,
    pub mContentToFrameIndex: HashMap<u32, i32>,
    // Cached presentation context for the frames we're processing.
    pub mPresContext: nsPresContext,
    // ADAPTER: mPresContext->BidiEngine()
    pub mBidiEngine: Bidi,
    pub mIsVisual: bool,
    pub mRequiresBidi: bool,
    pub mParaLevel: BidiEmbeddingLevel,
    pub mPrevContent: Option<nsIContent>,

    pub mCurrentTraverseLine: BidiParagraphData_FastLineIterator,
    pub mCurrentResolveLine: BidiParagraphData_FastLineIterator,
}

impl BidiParagraphData {
    // Gecko: BidiParagraphData::BidiParagraphData
    pub fn new(aBlockFrame: nsBlockFrame) -> BidiParagraphData {
        let mPresContext = aBlockFrame.PresContext();
        let mut this = BidiParagraphData {
            mBuffer: Vec::new(),
            mEmbeddingStack: Vec::new(),
            mLogicalFrames: Vec::new(),
            mContentToFrameIndex: HashMap::new(),
            mPresContext,
            mBidiEngine: Bidi::new(),
            mIsVisual: mPresContext.IsVisualMode(),
            mRequiresBidi: false,
            mParaLevel: nsBidiPresUtils::BidiLevelFromStyle(&aBlockFrame.Style()),
            mPrevContent: None,
            mCurrentTraverseLine: BidiParagraphData_FastLineIterator::new(),
            mCurrentResolveLine: BidiParagraphData_FastLineIterator::new(),
        };
        if this.mParaLevel.Value() > 0 {
            this.mRequiresBidi = true;
        }

        if this.mIsVisual {
            // NOT PORTED: visual mode is never on; the C++ drills up in content for a form control
            // and turns visual mode off for it (TODO(emilio) in the C++: IsInUAWidget).
        }
        this
    }

    // Gecko: BidiParagraphData::SetPara
    pub fn SetPara(&mut self) -> nsresult {
        if self.mBidiEngine.SetParagraph(&self.mBuffer, self.mParaLevel) != NS_OK {
            return NS_ERROR_FAILURE;
        }
        NS_OK
    }

    /**
     * mParaLevel can be BidiDirection::LTR as well as
     * BidiDirection::LTR or BidiDirection::RTL.
     * GetParagraphEmbeddingLevel() returns the actual (resolved) paragraph level
     * which is always either BidiDirection::LTR or
     * BidiDirection::RTL
     */
    // Gecko: BidiParagraphData::GetParagraphEmbeddingLevel
    pub fn GetParagraphEmbeddingLevel(&self) -> BidiEmbeddingLevel {
        let mut paraLevel = self.mParaLevel;
        if paraLevel == BidiEmbeddingLevel::DefaultLTR() || paraLevel == BidiEmbeddingLevel::DefaultRTL() {
            paraLevel = self.mBidiEngine.GetParagraphEmbeddingLevel();
        }
        paraLevel
    }

    // Gecko: BidiParagraphData::GetParagraphDirection
    pub fn GetParagraphDirection(&self) -> Bidi_ParagraphDirection {
        self.mBidiEngine.GetParagraphDirection()
    }

    // Gecko: BidiParagraphData::CountRuns
    pub fn CountRuns(&self, runCount: &mut i32) -> nsresult {
        match self.mBidiEngine.CountRuns() {
            Err(_) => NS_ERROR_FAILURE,
            Ok(n) => {
                *runCount = n;
                NS_OK
            }
        }
    }

    // Gecko: BidiParagraphData::GetLogicalRun
    pub fn GetLogicalRun(&self, aLogicalStart: i32, aLogicalLimit: &mut i32, aLevel: &mut BidiEmbeddingLevel) {
        self.mBidiEngine.GetLogicalRun(aLogicalStart, aLogicalLimit, aLevel);
        if self.mIsVisual {
            *aLevel = self.GetParagraphEmbeddingLevel();
        }
    }

    // Gecko: BidiParagraphData::ResetData
    pub fn ResetData(&mut self) {
        self.mLogicalFrames.clear();
        self.mContentToFrameIndex.clear();
        self.mBuffer.clear();
        self.mPrevContent = None;
        for i in 0..self.mEmbeddingStack.len() {
            self.mBuffer.push(self.mEmbeddingStack[i]);
            self.mLogicalFrames.push(BidiParagraphData_FrameInfo::new__BidiControlFrameType());
        }
    }

    // Gecko: BidiParagraphData::AppendFrame (the line iterator is mCurrentTraverseLine, see the top)
    pub fn AppendFrame(&mut self, aFrame: nsIFrame, aContent: Option<nsIContent>) {
        if let Some(content) = aContent {
            let index = self.FrameCount();
            self.mContentToFrameIndex.insert(content.Id(), index);
        }

        // We don't actually need to advance aLineIter to aFrame, since all we use
        // from it is the block and is-overflow state, which are correct already.
        self.mLogicalFrames.push(BidiParagraphData_FrameInfo::new__nsIFrame_nsBlockInFlowLineIterator(aFrame, &self.mCurrentTraverseLine.mLineIterator));
    }

    // Gecko: BidiParagraphData::AdvanceAndAppendFrame (the line iterator is mCurrentTraverseLine)
    pub fn AdvanceAndAppendFrame(&mut self, aFrame: &mut Option<nsIFrame>, aNextSibling: &mut Option<nsIFrame>) {
        let mut frame = *aFrame;
        let mut nextSibling = *aNextSibling;

        frame = frame.unwrap().GetNextContinuation();
        if let Some(f) = frame {
            self.AppendFrame(f, None);

            /*
             * If we have already overshot the saved next-sibling while
             * scanning the frame's continuations, advance it.
             */
            if Some(f) == nextSibling {
                nextSibling = f.GetNextSibling();
            }
        }

        *aFrame = frame;
        *aNextSibling = nextSibling;
    }

    // Gecko: BidiParagraphData::GetLastFrameForContent
    pub fn GetLastFrameForContent(&self, aContent: nsIContent) -> i32 {
        self.mContentToFrameIndex.get(&aContent.Id()).copied().unwrap_or(0)
    }

    // Gecko: BidiParagraphData::FrameCount
    pub fn FrameCount(&self) -> i32 {
        self.mLogicalFrames.len() as i32
    }

    // Gecko: BidiParagraphData::BufferLength
    pub fn BufferLength(&self) -> i32 {
        self.mBuffer.len() as i32
    }

    // Gecko: BidiParagraphData::FrameAt
    pub fn FrameAt(&self, aIndex: i32) -> Option<nsIFrame> {
        self.mLogicalFrames[aIndex as usize].mFrame
    }

    // Gecko: BidiParagraphData::FrameInfoAt
    pub fn FrameInfoAt(&self, aIndex: i32) -> BidiParagraphData_FrameInfo {
        self.mLogicalFrames[aIndex as usize]
    }

    // Gecko: BidiParagraphData::AppendUnichar
    pub fn AppendUnichar(&mut self, aCh: u16) {
        self.mBuffer.push(aCh);
    }

    // Gecko: BidiParagraphData::AppendString
    pub fn AppendString(&mut self, aString: &[u16]) {
        self.mBuffer.extend_from_slice(aString);
    }

    // Gecko: BidiParagraphData::AppendControlChar
    pub fn AppendControlChar(&mut self, aCh: u16) {
        self.mLogicalFrames.push(BidiParagraphData_FrameInfo::new__BidiControlFrameType());
        self.AppendUnichar(aCh);
    }

    // Gecko: BidiParagraphData::PushBidiControl
    pub fn PushBidiControl(&mut self, aCh: u16) {
        self.AppendControlChar(aCh);
        self.mEmbeddingStack.push(aCh);
    }

    // Gecko: BidiParagraphData::AppendPopChar
    pub fn AppendPopChar(&mut self, aCh: u16) {
        self.AppendControlChar(if IsIsolateControl(aCh) { kPDI } else { kPDF });
    }

    // Gecko: BidiParagraphData::PopBidiControl
    pub fn PopBidiControl(&mut self, aCh: u16) {
        MOZ_ASSERT!(!self.mEmbeddingStack.is_empty(), "embedding/override underflow");
        MOZ_ASSERT!(aCh == *self.mEmbeddingStack.last().unwrap());
        self.AppendPopChar(aCh);
        self.mEmbeddingStack.pop();
    }

    // Gecko: BidiParagraphData::ClearBidiControls
    pub fn ClearBidiControls(&mut self) {
        for i in (0..self.mEmbeddingStack.len()).rev() {
            let c = self.mEmbeddingStack[i];
            self.AppendPopChar(c);
        }
    }
}

// Gecko: layout/base/nsBidiPresUtils.h nsFrameContinuationState
#[derive(Clone, Copy, Default)]
pub struct nsFrameContinuationState {
    /**
     * The first visual frame in the continuation chain containing this frame, or
     * nullptr if this frame is the first visual frame in the chain.
     */
    pub mFirstVisualFrame: Option<nsIFrame>,

    /**
     * The number of frames in the continuation chain containing this frame, if
     * this frame is the first visual frame of the chain, or 0 otherwise.
     */
    pub mFrameCount: u32,

    /**
     * TRUE if this frame is the first visual frame of its continuation chain on
     * this line and the chain has some frames on the previous lines.
     */
    pub mHasContOnPrevLines: bool,

    /**
     * TRUE if this frame is the first visual frame of its continuation chain on
     * this line and the chain has some frames on the next lines.
     */
    pub mHasContOnNextLines: bool,
}

/**
 * A map from a frame to its nsFrameContinuationState.
 */
// Gecko: layout/base/nsBidiPresUtils.h nsContinuationStates
// ADAPTER: Gecko gathers the states in an array of 32 and switches to a hash table above that, for
// speed; here it is a hash map.
pub struct nsContinuationStates {
    mTable: HashMap<u32, nsFrameContinuationState>,
}

impl nsContinuationStates {
    pub fn new() -> Self {
        nsContinuationStates { mTable: HashMap::new() }
    }

    pub fn Insert(&mut self, aFrame: nsIFrame) {
        self.mTable.entry(aFrame.0).or_default();
    }

    pub fn Has(&self, aFrame: nsIFrame) -> bool {
        self.mTable.contains_key(&aFrame.0)
    }

    pub fn Get(&mut self, aFrame: nsIFrame) -> &mut nsFrameContinuationState {
        self.mTable.get_mut(&aFrame.0).expect("frame is not in the continuation states")
    }
}

// Gecko: layout/base/nsBidiPresUtils.cpp BidiLineData
pub struct BidiLineData {
    mLogicalFrames: Vec<nsIFrame>,
    mVisualFrameIndex: Vec<i32>,
    mIndexMap: Vec<i32>,
    mLevels: Vec<BidiEmbeddingLevel>,
    mIsReordered: bool,
}

impl BidiLineData {
    // Gecko: BidiLineData::BidiLineData
    pub fn new(aFirstFrameOnLine: nsIFrame, mut aNumFramesOnLine: i32) -> BidiLineData {
        let mut this = BidiLineData { mLogicalFrames: Vec::new(), mVisualFrameIndex: Vec::new(), mIndexMap: Vec::new(), mLevels: Vec::new(), mIsReordered: false };
        // Initialize the logically-ordered array of frames using the top-level
        // frames of a single line
        let mut frame = Some(aFirstFrameOnLine);
        while let Some(f) = frame {
            if aNumFramesOnLine == 0 {
                break;
            }
            aNumFramesOnLine -= 1;
            let bidiData = nsBidiPresUtils::GetFrameBidiData(f);
            if bidiData.precedingControl != kBidiLevelNone {
                this.appendFrame(NS_BIDI_CONTROL_FRAME, bidiData.precedingControl);
            }
            this.appendFrame(f, bidiData.embeddingLevel);
            frame = f.GetNextSibling();
        }

        // Reorder the line
        Bidi::ReorderVisual(&this.mLevels, &mut this.mIndexMap);

        // Collect the frames in visual order, omitting virtual controls
        // and noting whether frames are reordered.
        for i in 0..this.mIndexMap.len() {
            let frame = this.mLogicalFrames[this.mIndexMap[i] as usize];
            if frame == NS_BIDI_CONTROL_FRAME {
                continue;
            }
            this.mVisualFrameIndex.push(this.mIndexMap[i]);
            if i as i32 != this.mIndexMap[i] {
                this.mIsReordered = true;
            }
        }
        this
    }

    // (the lambda appendFrame of the constructor)
    fn appendFrame(&mut self, frame: nsIFrame, level: BidiEmbeddingLevel) {
        self.mLogicalFrames.push(frame);
        self.mLevels.push(level);
        self.mIndexMap.push(0);
    }

    // Gecko: BidiLineData::LogicalFrameCount
    pub fn LogicalFrameCount(&self) -> u32 {
        self.mLogicalFrames.len() as u32
    }

    // Gecko: BidiLineData::VisualFrameCount
    pub fn VisualFrameCount(&self) -> u32 {
        self.mVisualFrameIndex.len() as u32
    }

    // Gecko: BidiLineData::LogicalFrameAt
    pub fn LogicalFrameAt(&self, aIndex: u32) -> nsIFrame {
        self.mLogicalFrames[aIndex as usize]
    }

    // Gecko: BidiLineData::VisualFrameAt
    pub fn VisualFrameAt(&self, aIndex: u32) -> nsIFrame {
        self.mLogicalFrames[self.mVisualFrameIndex[aIndex as usize] as usize]
    }

    // Gecko: BidiLineData::VisualFrameAndLevelAt
    pub fn VisualFrameAndLevelAt(&self, aIndex: u32) -> (nsIFrame, BidiEmbeddingLevel) {
        let index = self.mVisualFrameIndex[aIndex as usize] as usize;
        (self.mLogicalFrames[index], self.mLevels[index])
    }

    // Gecko: BidiLineData::IsReordered
    pub fn IsReordered(&self) -> bool {
        self.mIsReordered
    }

    // Gecko: BidiLineData::InitContinuationStates
    pub fn InitContinuationStates(&self, aContinuationStates: &mut nsContinuationStates) {
        for &frame in &self.mLogicalFrames {
            if frame != NS_BIDI_CONTROL_FRAME {
                nsBidiPresUtils::InitContinuationStates(frame, aContinuationStates);
            }
        }
    }
}

/* Some helper methods for Resolve() */

// Should this frame be split between text runs?
// Gecko: layout/base/nsBidiPresUtils.cpp IsBidiSplittable
fn IsBidiSplittable(aFrame: nsIFrame) -> bool {
    // Bidi inline containers should be split, unless they're line frames.
    let frameType = aFrame.Type();
    (aFrame.IsBidiInlineContainer() && frameType != LayoutFrameType::Line) || frameType == LayoutFrameType::Text
}

// Gecko: mozilla::RubyUtils::IsRubyBox
fn IsRubyBox(aFrameType: LayoutFrameType) -> bool {
    matches!(aFrameType, LayoutFrameType::Ruby | LayoutFrameType::RubyBase | LayoutFrameType::RubyBaseContainer | LayoutFrameType::RubyText | LayoutFrameType::RubyTextContainer)
}

// Should this frame be treated as a leaf (e.g. when building mLogicalFrames)?
// Gecko: layout/base/nsBidiPresUtils.cpp IsBidiLeaf
fn IsBidiLeaf(aFrame: nsIFrame) -> bool {
    let kid = aFrame.PrincipalChildList_FirstChild();
    if kid.is_some() {
        if aFrame.IsBidiInlineContainer() || IsRubyBox(aFrame.Type()) {
            return false;
        }
    }
    true
}

/**
 * Create non-fluid continuations for the ancestors of a given frame all the way
 * up the frame tree until we hit a non-splittable frame (a line or a block).
 *
 * @param aParent the first parent frame to be split
 * @param aFrame the child frames after this frame are reparented to the
 *        newly-created continuation of aParent.
 *        If aFrame is null, all the children of aParent are reparented.
 */
// Gecko: layout/base/nsBidiPresUtils.cpp SplitInlineAncestors
fn SplitInlineAncestors(aParent: nsContainerFrame, aLine: nsLineList_iterator, aFrame: Option<nsIFrame>) {
    let mut frame = aFrame;
    let mut parent = aParent;

    while IsBidiSplittable(parent.0) {
        let grandparent = nsContainerFrame(parent.GetParent().expect("Couldn't get parent's parent in nsBidiPresUtils::SplitInlineAncestors"));

        // Split the child list after |frame|, unless it is the last child.
        if frame.is_none() || frame.unwrap().GetNextSibling().is_some() {
            let newParent = nsContainerFrame(nsCSSFrameConstructor_CreateContinuingFrame(parent.0, grandparent.0, false));

            let tail = parent.StealFramesAfter(frame);

            // The parent's continuation adopts the siblings after the split.
            MOZ_ASSERT!(!newParent.IsBlockFrameOrSubclass(), "blocks should not be IsBidiSplittable");
            newParent.InsertFrames(FrameChildListID::NoReflowPrincipal, None, None, tail);

            // While passing &aLine to InsertFrames for a non-block isn't harmful
            // because it's a no-op, it doesn't really make sense.  However, the
            // MOZ_ASSERT() we need to guarantee that it's safe only works if the
            // parent is actually the block.
            let parentLine = if grandparent.IsBlockFrameOrSubclass() {
                MOZ_ASSERT!(unsafe { &*aLine.get() }.Contains(parent.0));
                Some(&aLine)
            } else {
                None
            };

            // The list name FrameChildListID::NoReflowPrincipal would indicate
            // we don't want reflow
            grandparent.InsertFrames(FrameChildListID::NoReflowPrincipal, Some(parent.0), parentLine, nsFrameList::new(Some(newParent.0), Some(newParent.0)));
        }

        frame = Some(parent.0);
        parent = grandparent;
    }
}

// Gecko: layout/base/nsBidiPresUtils.cpp MakeContinuationFluid
fn MakeContinuationFluid(aFrame: nsIFrame, aNext: nsIFrame) {
    NS_ASSERTION!(aFrame.GetNextInFlow().is_none() || aFrame.GetNextInFlow() == Some(aNext), "next-in-flow is not next continuation!");
    aFrame.SetNextInFlow(Some(aNext));

    NS_ASSERTION!(aNext.GetPrevInFlow().is_none() || aNext.GetPrevInFlow() == Some(aFrame), "prev-in-flow is not prev continuation!");
    aNext.SetPrevInFlow(Some(aFrame));
}

// Gecko: layout/base/nsBidiPresUtils.cpp MakeContinuationsNonFluidUpParentChain
fn MakeContinuationsNonFluidUpParentChain(aFrame: nsIFrame, aNext: Option<nsIFrame>) {
    let mut frame = Some(aFrame);
    let mut next = aNext;
    while let (Some(f), Some(n)) = (frame, next) {
        if !(n != f && Some(n) == f.GetNextInFlow() && IsBidiSplittable(f)) {
            break;
        }
        f.SetNextContinuation(Some(n));
        n.SetPrevContinuation(Some(f));
        frame = f.GetParent();
        next = n.GetParent();
    }
}

// If aFrame is the last child of its parent, convert bidi continuations to
// fluid continuations for all of its inline ancestors.
// If it isn't the last child, make sure that its continuation is fluid.
// Gecko: layout/base/nsBidiPresUtils.cpp JoinInlineAncestors
fn JoinInlineAncestors(aFrame: nsIFrame) {
    let mut frame = Some(aFrame);
    while let Some(f) = frame {
        if !IsBidiSplittable(f) {
            break;
        }
        if let Some(next) = f.GetNextContinuation() {
            MakeContinuationFluid(f, next);
        }
        // Join the parent only as long as we're its last child.
        if f.GetNextSibling().is_some() {
            break;
        }
        frame = f.GetParent();
    }
}

// Gecko: layout/base/nsBidiPresUtils.cpp CreateContinuation
// (returns aNewFrame, the out-parameter)
fn CreateContinuation(aFrame: nsIFrame, aLine: nsLineList_iterator, aIsFluid: bool) -> nsIFrame {
    let parent = nsContainerFrame(aFrame.GetParent().expect("Couldn't get frame parent in nsBidiPresUtils::CreateContinuation"));

    // While passing &aLine to InsertFrames for a non-block isn't harmful
    // because it's a no-op, it doesn't really make sense.  However, the
    // MOZ_ASSERT() we need to guarantee that it's safe only works if the
    // parent is actually the block.
    let parentLine = if parent.IsBlockFrameOrSubclass() {
        MOZ_ASSERT!(unsafe { &*aLine.get() }.Contains(aFrame));
        Some(&aLine)
    } else {
        None
    };

    // Have to special case floating first letter frames because the continuation
    // doesn't go in the first letter frame. The continuation goes with the rest
    // of the text that the first letter frame was made out of.
    // (parent->IsLetterFrame() && parent->IsFloating(): NOT PORTED, ::first-letter)

    let newFrame = nsCSSFrameConstructor_CreateContinuingFrame(aFrame, parent.0, aIsFluid);

    // The list name FrameChildListID::NoReflowPrincipal would indicate we don't
    // want reflow
    // XXXbz this needs higher-level framelist love
    parent.InsertFrames(FrameChildListID::NoReflowPrincipal, Some(aFrame), parentLine, nsFrameList::new(Some(newFrame), Some(newFrame)));

    if !aIsFluid {
        // Split inline ancestor frames
        SplitInlineAncestors(parent, aLine, Some(aFrame));
    }
    newFrame
}

// Gecko: nsIFrame::AdjustOffsetsForBidi (virtual, a no-op except for text frames)
fn AdjustOffsetsForBidi(aFrame: nsIFrame, aStart: i32, aEnd: i32) {
    if aFrame.IsTextFrame() {
        nsTextFrame::from_frame(aFrame).AdjustOffsetsForBidi(aStart, aEnd);
    }
}

// In ResolveParagraph, we previously used ReplaceChar(kSeparators, kSpace)
// to convert separators to spaces, but this hard-coded implementation is
// substantially faster than the general-purpose ReplaceChar function.
// This must be kept in sync with the definition of kSeparators.
// Gecko: layout/base/nsBidiPresUtils.cpp ReplaceSeparators
fn ReplaceSeparators(aText: &mut Vec<u16>, aStartIndex: usize) {
    const SeparatorToSpace: [u16; 32] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, b' ' as u16, b' ' as u16, b' ' as u16, 0x0c, b' ' as u16, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, b' ' as u16, b' ' as u16, b' ' as u16,
        b' ' as u16,
    ];
    for cp in aText[aStartIndex..].iter_mut() {
        if *cp < b' ' as u16 {
            *cp = SeparatorToSpace[*cp as usize];
        } else if *cp == kNextLine || *cp == kParagraphSeparator {
            *cp = b' ' as u16;
        }
    }
}

// Gecko: layout/base/nsBidiPresUtils.h nsBidiPresUtils
pub struct nsBidiPresUtils;

impl nsBidiPresUtils {
    /*
     * Overview of the implementation of Resolve():
     *
     *  Walk through the descendants of aBlockFrame and build:
     *   * mLogicalFrames: an nsTArray of nsIFrame* pointers in logical order
     *   * mBuffer: an nsString containing a representation of
     *     the content of the frames.
     *     In the case of text frames, this is the actual text context of the
     *     frames, but some other elements are represented in a symbolic form which
     *     will make the Unicode Bidi Algorithm give the correct results.
     *     Bidi isolates, embeddings, and overrides set by CSS, <bdi>, or <bdo>
     *     elements are represented by the corresponding Unicode control characters.
     *     <br> elements are represented by U+2028 LINE SEPARATOR
     *     Other inline elements are represented by U+FFFC OBJECT REPLACEMENT
     *     CHARACTER
     *
     *  Then pass mBuffer to the Bidi engine for resolving of embedding levels
     *  by nsBidi::SetPara() and division into directional runs by
     *  nsBidi::CountRuns().
     *
     *  Finally, walk these runs in logical order using nsBidi::GetLogicalRun() and
     *  correlate them with the frames indexed in mLogicalFrames, setting the
     *  baseLevel and embeddingLevel properties according to the results returned
     *  by the Bidi engine.
     *
     *  The rendering layer requires each text frame to contain text in only one
     *  direction, so we may need to call EnsureBidiContinuation() to split frames.
     *  We may also need to call RemoveBidiContinuation() to convert frames created
     *  by EnsureBidiContinuation() in previous reflows into fluid continuations.
     */
    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::Resolve
    pub fn Resolve(aBlockFrame: nsBlockFrame) -> nsresult {
        let mut bpd = BidiParagraphData::new(aBlockFrame);

        // Handle bidi-override being set on the block itself before calling
        // TraverseFrames.
        // No need to call GetBidiControl as well, because isolate and embed
        // values of unicode-bidi property are redundant on block elements.
        // unicode-bidi:plaintext on a block element is handled by block frame
        // via using nsIFrame::GetWritingMode(nsIFrame*).
        let ch = GetBidiOverride(&aBlockFrame.Style());
        if ch != 0 {
            bpd.PushBidiControl(ch);
            bpd.mRequiresBidi = true;
        } else {
            // If there are no unicode-bidi properties and no RTL characters in the
            // block's content, then it is pure LTR and we can skip the rest of bidi
            // resolution.
            let mut currContent: Option<nsIContent> = None;
            let mut block = Some(aBlockFrame);
            while let Some(b) = block {
                b.RemoveStateBits(NS_BLOCK_NEEDS_BIDI_RESOLUTION);
                if !bpd.mRequiresBidi && Self::ChildListMayRequireBidi(b.mFrames().FirstChild(), &mut currContent) {
                    bpd.mRequiresBidi = true;
                }
                if !bpd.mRequiresBidi {
                    let overflowLines = b.GetOverflowLines();
                    if !overflowLines.is_null() {
                        if Self::ChildListMayRequireBidi(unsafe { &*overflowLines }.mFrames.FirstChild(), &mut currContent) {
                            bpd.mRequiresBidi = true;
                        }
                    }
                }
                block = b.GetNextContinuation().map(nsBlockFrame::from_frame);
            }
            if !bpd.mRequiresBidi {
                return NS_OK;
            }
        }

        let mut block = Some(aBlockFrame);
        while let Some(b) = block {
            b.RemoveStateBits(NS_BLOCK_NEEDS_BIDI_RESOLUTION);
            bpd.mCurrentTraverseLine.mLineIterator = nsBlockInFlowLineIterator::new__nsBlockFrame_LineIterator(b, b.LinesBegin());
            bpd.mCurrentTraverseLine.mPrevFrame = None;
            Self::TraverseFrames(b.mFrames().FirstChild(), &mut bpd);
            let overflowLines = b.GetOverflowLines();
            if !overflowLines.is_null() {
                let overflowLines = unsafe { &mut *overflowLines };
                bpd.mCurrentTraverseLine.mLineIterator = nsBlockInFlowLineIterator::new__nsBlockFrame_LineIterator_bool(b, overflowLines.mLines.begin_mut(), true);
                bpd.mCurrentTraverseLine.mPrevFrame = None;
                Self::TraverseFrames(overflowLines.mFrames.FirstChild(), &mut bpd);
            }
            block = b.GetNextContinuation().map(nsBlockFrame::from_frame);
        }

        if ch != 0 {
            bpd.PopBidiControl(ch);
        }

        Self::ResolveParagraph(&mut bpd)
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::ResolveParagraph
    pub fn ResolveParagraph(aBpd: &mut BidiParagraphData) -> nsresult {
        if aBpd.BufferLength() < 1 {
            return NS_OK;
        }

        ReplaceSeparators(&mut aBpd.mBuffer, 0);

        let mut runCount: i32 = 0;

        let mut rv = aBpd.SetPara();
        if rv != NS_OK {
            return rv;
        }

        let mut embeddingLevel = aBpd.GetParagraphEmbeddingLevel();

        rv = aBpd.CountRuns(&mut runCount);
        if rv != NS_OK {
            return rv;
        }

        let mut runLength: i32 = 0; // the length of the current run of text
        let mut logicalLimit: i32 = 0; // the end of the current run + 1
        let mut numRun: i32 = -1;
        let mut fragmentLength: i32 = 0; // the length of the current text frame
        let mut frameIndex: i32 = -1; // index to the frames in mLogicalFrames
        let frameCount = aBpd.FrameCount();
        let mut contentOffset: i32 = 0; // offset of current frame in its content node
        let mut isTextFrame = false;
        let mut frame: Option<nsIFrame> = None;
        let mut frameInfo = BidiParagraphData_FrameInfo::new();
        let mut content: Option<nsIContent> = None;
        let mut contentTextLength: i32 = 0;

        if runCount == 1 && frameCount == 1 && aBpd.GetParagraphDirection() == Bidi_ParagraphDirection::LTR && aBpd.GetParagraphEmbeddingLevel().Value() == 0 {
            // We have a single left-to-right frame in a left-to-right paragraph,
            // without bidi isolation from the surrounding text.
            // Make sure that the embedding level and base level frame properties aren't
            // set (because if they are this frame used to have some other direction,
            // so we can't do this optimization), and we're done.
            let frame = aBpd.FrameAt(0).unwrap();
            if frame != NS_BIDI_CONTROL_FRAME {
                let bidiData = frame.GetBidiData();
                if bidiData.embeddingLevel.Value() == 0 && bidiData.baseLevel.Value() == 0 {
                    frame.AddStateBits(NS_FRAME_IS_BIDI);
                    return NS_OK;
                }
            }
        }

        let mut lastRealFrame = BidiParagraphData_FrameInfo::new();
        let mut lastEmbeddingLevel = kBidiLevelNone;
        let mut precedingControl = kBidiLevelNone;

        // (a lambda in C++; it uses and changes the locals above)
        macro_rules! storeBidiDataToFrame {
            () => {{
                let mut bidiData = FrameBidiData::default();
                bidiData.embeddingLevel = embeddingLevel;
                bidiData.baseLevel = aBpd.GetParagraphEmbeddingLevel();
                // If a control character doesn't have a lower embedding level than
                // both the preceding and the following frame, it isn't something
                // needed for getting the correct result. This optimization should
                // remove almost all of embeds and overrides, and some of isolates.
                if precedingControl >= embeddingLevel || precedingControl >= lastEmbeddingLevel {
                    bidiData.precedingControl = kBidiLevelNone;
                } else {
                    bidiData.precedingControl = precedingControl;
                }
                precedingControl = kBidiLevelNone;
                lastEmbeddingLevel = embeddingLevel;
                frame.unwrap().SetProperty(nsIFrame::BidiDataProperty(), bidiData);
            }};
        }

        loop {
            if fragmentLength <= 0 {
                // Get the next frame from mLogicalFrames
                frameIndex += 1;
                if frameIndex >= frameCount {
                    break;
                }
                frameInfo = aBpd.FrameInfoAt(frameIndex);
                frame = frameInfo.mFrame;
                if frame == Some(NS_BIDI_CONTROL_FRAME) || !frame.unwrap().IsTextFrame() {
                    /*
                     * Any non-text frame corresponds to a single character in the text
                     * buffer (a bidi control character, LINE SEPARATOR, or OBJECT
                     * SUBSTITUTE)
                     */
                    isTextFrame = false;
                    fragmentLength = 1;
                } else {
                    aBpd.mCurrentResolveLine.AdvanceToLinesAndFrame(&frameInfo);
                    content = frame.unwrap().GetContent();
                    if content.is_none() {
                        rv = NS_OK;
                        break;
                    }
                    contentTextLength = content.unwrap().TextLength() as i32;
                    let (start, end) = nsTextFrame::from_frame(frame.unwrap()).GetOffsets();
                    NS_ASSERTION!(!(contentTextLength < end - start), "Frame offsets don't fit in content");
                    fragmentLength = std::cmp::min(contentTextLength, end - start);
                    contentOffset = start;
                    isTextFrame = true;
                }
            } // if (fragmentLength <= 0)

            if runLength <= 0 {
                // Get the next run of text from the Bidi engine
                numRun += 1;
                if numRun >= runCount {
                    // We've run out of runs of text; but don't forget to store bidi data
                    // to the frame before breaking out of the loop (bug 1426042).
                    if frame != Some(NS_BIDI_CONTROL_FRAME) {
                        storeBidiDataToFrame!();
                        if isTextFrame {
                            AdjustOffsetsForBidi(frame.unwrap(), contentOffset, contentOffset + fragmentLength);
                        }
                    }
                    break;
                }
                let lineOffset = logicalLimit;
                aBpd.GetLogicalRun(lineOffset, &mut logicalLimit, &mut embeddingLevel);
                runLength = logicalLimit - lineOffset;
            } // if (runLength <= 0)

            if frame == Some(NS_BIDI_CONTROL_FRAME) {
                // In theory, we only need to do this for isolates. However, it is
                // easier to do this for all here because we do not maintain the
                // index to get corresponding character from buffer. Since we do
                // have proper embedding level for all those characters, including
                // them wouldn't affect the final result.
                precedingControl = std::cmp::min(precedingControl, embeddingLevel);
            } else {
                storeBidiDataToFrame!();
                if isTextFrame {
                    if contentTextLength == 0 {
                        // Set the base level and embedding level of the current run even
                        // on an empty frame. Otherwise frame reordering will not be correct.
                        AdjustOffsetsForBidi(frame.unwrap(), 0, 0);
                        // Nothing more to do for an empty frame, except update
                        // lastRealFrame like we do below.
                        lastRealFrame = frameInfo;
                        continue;
                    }
                    let mut currentLine = aBpd.mCurrentResolveLine.GetLine();
                    if (runLength > 0) && (runLength < fragmentLength) {
                        /*
                         * The text in this frame continues beyond the end of this directional
                         * run. Create a non-fluid continuation frame for the next directional
                         * run.
                         */
                        unsafe { &mut *currentLine.get_mut() }.MarkDirty();
                        let runEnd = contentOffset + runLength;
                        // (nsBidiPresUtils::EnsureBidiContinuation)
                        let nextBidi = Self::EnsureBidiContinuation(frame.unwrap(), currentLine, contentOffset, runEnd);
                        AdjustOffsetsForBidi(nextBidi, runEnd, contentOffset + fragmentLength);
                        frame = Some(nextBidi);
                        frameInfo.mFrame = frame;
                        contentOffset = runEnd;

                        aBpd.mCurrentResolveLine.AdvanceToFrame(frame.unwrap());
                    }
                    // if (runLength < fragmentLength)
                    else {
                        if contentOffset + fragmentLength == contentTextLength {
                            /*
                             * We have finished all the text in this content node. Convert any
                             * further non-fluid continuations to fluid continuations and
                             * advance frameIndex to the last frame in the content node
                             */
                            let newIndex = aBpd.GetLastFrameForContent(content.unwrap());
                            if newIndex > frameIndex {
                                unsafe { &mut *currentLine.get_mut() }.MarkDirty();
                                Self::RemoveBidiContinuation(aBpd, frame.unwrap(), frameIndex, newIndex);
                                frameIndex = newIndex;
                                frameInfo = aBpd.FrameInfoAt(frameIndex);
                                frame = frameInfo.mFrame;
                            }
                        } else if fragmentLength > 0 && runLength > fragmentLength {
                            /*
                             * There is more text that belongs to this directional run in the
                             * next text frame: make sure it is a fluid continuation of the
                             * current frame. Do not advance frameIndex, because the next frame
                             * may contain multi-directional text and need to be split
                             */
                            let mut newIndex = frameIndex;
                            loop {
                                newIndex += 1;
                                if !(newIndex < frameCount && aBpd.FrameAt(newIndex) == Some(NS_BIDI_CONTROL_FRAME)) {
                                    break;
                                }
                            }
                            if newIndex < frameCount {
                                unsafe { &mut *currentLine.get_mut() }.MarkDirty();
                                Self::RemoveBidiContinuation(aBpd, frame.unwrap(), frameIndex, newIndex);
                            }
                        } else if runLength == fragmentLength {
                            /*
                             * If the directional run ends at the end of the frame, make sure
                             * that any continuation is non-fluid, and do the same up the
                             * parent chain
                             */
                            let next = frame.unwrap().GetNextInFlow();
                            if next.is_some() {
                                unsafe { &mut *currentLine.get_mut() }.MarkDirty();
                                MakeContinuationsNonFluidUpParentChain(frame.unwrap(), next);
                            }
                        }
                        AdjustOffsetsForBidi(frame.unwrap(), contentOffset, contentOffset + fragmentLength);
                    }
                } // isTextFrame
            } // not bidi control frame
            let temp = runLength;
            runLength -= fragmentLength;
            fragmentLength -= temp;

            // Record last real frame so that we can do splitting properly even
            // if a run ends after a virtual bidi control frame.
            if frame != Some(NS_BIDI_CONTROL_FRAME) {
                lastRealFrame = frameInfo;
            }
            if lastRealFrame.mFrame.is_some() && fragmentLength <= 0 {
                // If the frame is at the end of a run, and this is not the end of our
                // paragraph, split all ancestor inlines that need splitting.
                // To determine whether we're at the end of the run, we check that we've
                // finished processing the current run, and that the current frame
                // doesn't have a fluid continuation (it could have a fluid continuation
                // of zero length, so testing runLength alone is not sufficient).
                if runLength <= 0 && lastRealFrame.mFrame.unwrap().GetNextInFlow().is_none() {
                    if numRun + 1 < runCount {
                        let mut child = lastRealFrame.mFrame.unwrap();
                        let mut parent = child.GetParent();
                        // As long as we're on the last sibling, the parent doesn't have to
                        // be split.
                        // However, if the parent has a fluid continuation, we do have to make
                        // it non-fluid. This can happen e.g. when we have a first-letter
                        // frame and the end of the first-letter coincides with the end of a
                        // directional run.
                        while let Some(p) = parent {
                            if !(IsBidiSplittable(p) && child.GetNextSibling().is_none()) {
                                break;
                            }
                            let next = p.GetNextInFlow();
                            if let Some(next) = next {
                                p.SetNextContinuation(Some(next));
                                next.SetPrevContinuation(Some(p));
                            }
                            child = p;
                            parent = child.GetParent();
                        }
                        if let Some(p) = parent {
                            if IsBidiSplittable(p) {
                                aBpd.mCurrentResolveLine.AdvanceToLinesAndFrame(&lastRealFrame);
                                SplitInlineAncestors(nsContainerFrame(p), aBpd.mCurrentResolveLine.GetLine(), Some(child));

                                aBpd.mCurrentResolveLine.AdvanceToLinesAndFrame(&lastRealFrame);
                            }
                        }
                    }
                } else if frame != Some(NS_BIDI_CONTROL_FRAME) {
                    // We're not at an end of a run. If |frame| is the last child of its
                    // parent, and its ancestors happen to have bidi continuations, convert
                    // them into fluid continuations.
                    JoinInlineAncestors(frame.unwrap());
                }
            }
        } // for

        rv
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::TraverseFrames
    pub fn TraverseFrames(aCurrentFrame: Option<nsIFrame>, aBpd: &mut BidiParagraphData) {
        let Some(aCurrentFrame) = aCurrentFrame else {
            return;
        };

        let initialLineContainer = aBpd.mCurrentTraverseLine.mLineIterator.GetContainerOpt();

        let mut childFrame = Some(aCurrentFrame);
        loop {
            let cf = childFrame.unwrap();
            /*
             * It's important to get the next sibling and next continuation *before*
             * handling the frame: If we encounter a forced paragraph break and call
             * ResolveParagraph within this loop, doing GetNextSibling and
             * GetNextContinuation after that could return a bidi continuation that had
             * just been split from the original childFrame and we would process it
             * twice.
             */
            let mut nextSibling = cf.GetNextSibling();

            // If the real frame for a placeholder is a first letter frame, we need to
            // drill down into it and include its contents in Bidi resolution.
            // If not, we just use the placeholder.
            let mut frame = cf;
            if cf.IsPlaceholderFrame() {
                let realFrame = nsPlaceholderFrame::GetRealFrameForPlaceholder(cf);
                if realFrame.IsLetterFrame() {
                    frame = realFrame;
                }
            }

            let DifferentBidiValues = |aSC1: &ServoArc<ComputedValues>, aFrame2: nsIFrame| -> bool {
                let sc2 = aFrame2.Style();
                GetBidiControl(aSC1) != GetBidiControl(&sc2) || GetBidiOverride(aSC1) != GetBidiOverride(&sc2)
            };

            let sc = frame.Style();
            let nextContinuation = frame.GetNextContinuation();
            let prevContinuation = frame.GetPrevContinuation();
            let isLastFrame = nextContinuation.map_or(true, |n| DifferentBidiValues(&sc, n));
            let isFirstFrame = prevContinuation.map_or(true, |p| DifferentBidiValues(&sc, p));

            let mut controlChar: u16 = 0;
            let mut overrideChar: u16 = 0;
            let frameType = frame.Type();
            if frame.IsBidiInlineContainer() || IsRubyBox(frameType) {
                if !frame.HasAnyStateBits(NS_FRAME_FIRST_REFLOW) {
                    nsContainerFrame(frame).DrainSelfOverflowList();
                }

                controlChar = GetBidiControl(&sc);
                overrideChar = GetBidiOverride(&sc);

                // Add dummy frame pointers representing bidi control codes before
                // the first frames of elements specifying override, isolation, or
                // plaintext.
                if isFirstFrame {
                    if controlChar != 0 {
                        aBpd.PushBidiControl(controlChar);
                    }
                    if overrideChar != 0 {
                        aBpd.PushBidiControl(overrideChar);
                    }
                }
            }

            if IsBidiLeaf(frame) {
                /* Bidi leaf frame: add the frame to the mLogicalFrames array,
                 * and add its index to the mContentToFrameIndex hashtable. This
                 * will be used in RemoveBidiContinuation() to identify the last
                 * frame in the array with a given content.
                 */
                let content = frame.GetContent();
                aBpd.AppendFrame(frame, content);

                // Append the content of the frame to the paragraph buffer
                if LayoutFrameType::Text == frameType {
                    if !nsIContent::IsSameNode(&content, &aBpd.mPrevContent) {
                        aBpd.mPrevContent = content;
                        if !nsStyleTextAdapter::of(frame).NewlineIsSignificant(nsTextFrame::from_frame(frame)) {
                            content.unwrap().mCharacterDataBuffer().unwrap().AppendTo__nsAString(&mut aBpd.mBuffer);
                        } else {
                            /*
                             * For preformatted text we have to do bidi resolution on each line
                             * separately.
                             */
                            let mut text: Vec<u16> = Vec::new();
                            content.unwrap().mCharacterDataBuffer().unwrap().AppendTo__nsAString(&mut text);
                            let mut frame = Some(frame);
                            loop {
                                let mut next: Option<nsIFrame> = None;

                                let (mut start, mut end) = nsTextFrame::from_frame(frame.unwrap()).GetOffsets();
                                let endLine = text[start as usize..].iter().position(|&c| c == b'\n' as u16).map(|p| (p as i32) + start);
                                let Some(mut endLine) = endLine else {
                                    /*
                                     * If there is no newline in the text content, just save the
                                     * text from this frame and its continuations, and do bidi
                                     * resolution later
                                     */
                                    aBpd.AppendString(&text[start as usize..]);
                                    while frame.is_some() && nextSibling.is_some() {
                                        aBpd.AdvanceAndAppendFrame(&mut frame, &mut nextSibling);
                                    }
                                    break;
                                };

                                /*
                                 * If there is a newline in the frame, break the frame after the
                                 * newline, do bidi resolution and repeat until the last sibling
                                 */
                                endLine += 1;

                                /*
                                 * If the frame ends before the new line, save the text and move
                                 * into the next continuation
                                 */
                                aBpd.AppendString(&text[start as usize..(std::cmp::min(end, endLine)) as usize]);
                                while end < endLine && nextSibling.is_some() {
                                    aBpd.AdvanceAndAppendFrame(&mut frame, &mut nextSibling);
                                    NS_ASSERTION!(frame.is_some(), "Premature end of continuation chain");
                                    (start, end) = nsTextFrame::from_frame(frame.unwrap()).GetOffsets();
                                    aBpd.AppendString(&text[start as usize..(std::cmp::min(end, endLine)) as usize]);
                                }

                                if end < endLine {
                                    aBpd.mPrevContent = None;
                                    break;
                                }

                                let mut createdContinuation = false;
                                if (endLine as usize) < text.len() {
                                    /*
                                     * Timing is everything here: if the frame already has a bidi
                                     * continuation, we need to make the continuation fluid *before*
                                     * resetting the length of the current frame. Otherwise
                                     * nsTextFrame::SetLength won't set the continuation frame's
                                     * text offsets correctly.
                                     *
                                     * On the other hand, if the frame doesn't have a continuation,
                                     * we need to create one *after* resetting the length, or
                                     * CreateContinuingFrame will complain that there is no more
                                     * content for the continuation.
                                     */
                                    next = frame.unwrap().GetNextInFlow();
                                    if next.is_none() {
                                        // If the frame already has a bidi continuation, make it fluid
                                        next = frame.unwrap().GetNextContinuation();
                                        if let Some(n) = next {
                                            MakeContinuationFluid(frame.unwrap(), n);
                                            JoinInlineAncestors(frame.unwrap());
                                        }
                                    }

                                    let textFrame = nsTextFrame::from_frame(frame.unwrap());
                                    textFrame.SetLength(endLine - start, None, 0);

                                    // If it weren't for CreateContinuation needing this to
                                    // be current, we could restructure the marking dirty
                                    // below to use mCurrentResolveLine and eliminate
                                    // mCurrentTraverseLine entirely.
                                    aBpd.mCurrentTraverseLine.AdvanceToFrame(frame.unwrap());

                                    if next.is_none() {
                                        // If the frame has no next in flow, create one.
                                        next = Some(CreateContinuation(frame.unwrap(), aBpd.mCurrentTraverseLine.GetLine(), true));
                                        createdContinuation = true;
                                    }
                                    // Mark the line before the newline as dirty.
                                    unsafe { &mut *aBpd.mCurrentTraverseLine.GetLine().get_mut() }.MarkDirty();
                                }
                                Self::ResolveParagraphWithinBlock(aBpd);

                                if nextSibling.is_none() && !createdContinuation {
                                    break;
                                }
                                if let Some(n) = next {
                                    frame = Some(n);
                                    aBpd.AppendFrame(n, None);
                                    // Mark the line after the newline as dirty.
                                    aBpd.mCurrentTraverseLine.AdvanceToFrame(n);
                                    unsafe { &mut *aBpd.mCurrentTraverseLine.GetLine().get_mut() }.MarkDirty();
                                }

                                /*
                                 * If we have already overshot the saved next-sibling while
                                 * scanning the frame's continuations, advance it.
                                 */
                                if frame.is_some() && frame == nextSibling {
                                    nextSibling = frame.unwrap().GetNextSibling();
                                }

                                if next.is_none() {
                                    break;
                                }
                            }
                        }
                    }
                } else if LayoutFrameType::Br == frameType {
                    // break frame -- append line separator
                    aBpd.AppendUnichar(kLineSeparator);
                    Self::ResolveParagraphWithinBlock(aBpd);
                } else {
                    // other frame type -- see the Unicode Bidi Algorithm:
                    // "...inline objects (such as graphics) are treated as if they are ...
                    // U+FFFC"
                    // <wbr>, however, is treated as U+200B ZERO WIDTH SPACE. See
                    // http://dev.w3.org/html5/spec/Overview.html#phrasing-content-1.
                    // Empty inline frames are also treated as kZWSP, to avoid unexpected
                    // bidi reordering of the surrounding content.
                    let isWbr = content.map_or(false, |c| c.with_HTMLLocalName(|n| n == Some("wbr")));
                    aBpd.AppendUnichar(if isWbr || (frame.IsInlineFrame() && nsInlineFrame::from_frame(frame).IsEmpty()) { kZWSP } else { kObjectSubstitute });
                    if !frame.IsInlineOutside() {
                        // if it is not inline, end the paragraph
                        Self::ResolveParagraphWithinBlock(aBpd);
                    }
                }
            } else {
                // For a non-leaf frame, recurse into TraverseFrames
                let kid = frame.PrincipalChildList_FirstChild();
                if kid.is_some() {
                    Self::TraverseFrames(kid, aBpd);
                }
            }

            // If the element is attributed by dir, indicate direction pop (add PDF
            // frame)
            if isLastFrame {
                // Add a dummy frame pointer representing a bidi control code after the
                // last frame of an element specifying embedding or override
                if overrideChar != 0 {
                    aBpd.PopBidiControl(overrideChar);
                }
                if controlChar != 0 {
                    aBpd.PopBidiControl(controlChar);
                }
            }
            childFrame = nextSibling;
            if childFrame.is_none() {
                break;
            }
        }

        MOZ_ASSERT!(initialLineContainer == aBpd.mCurrentTraverseLine.mLineIterator.GetContainerOpt());
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::ChildListMayRequireBidi
    pub fn ChildListMayRequireBidi(aFirstChild: Option<nsIFrame>, aCurrContent: &mut Option<nsIContent>) -> bool {
        MOZ_ASSERT!(aFirstChild.is_none() || aFirstChild.unwrap().GetPrevSibling().is_none(), "Expecting to traverse from the start of a child list");

        let mut childFrame = aFirstChild;
        while let Some(cf) = childFrame {
            let mut frame = cf;

            // If the real frame for a placeholder is a first-letter frame, we need to
            // consider its contents for potential Bidi resolution.
            if cf.IsPlaceholderFrame() {
                let realFrame = nsPlaceholderFrame::GetRealFrameForPlaceholder(cf);
                if realFrame.IsLetterFrame() {
                    frame = realFrame;
                }
            }

            // If unicode-bidi properties are present, we should do bidi resolution.
            let sc = frame.Style();
            if GetBidiControl(&sc) != 0 || GetBidiOverride(&sc) != 0 {
                return true;
            }

            if IsBidiLeaf(frame) {
                if frame.IsTextFrame() {
                    // If the frame already has a BidiDataProperty, we know we need to
                    // perform bidi resolution (even if no bidi content is NOW present --
                    // we might need to remove the property set by a previous reflow, if
                    // content has changed; see bug 1366623).
                    if frame.HasProperty(nsIFrame::BidiDataProperty()) {
                        return true;
                    }

                    // Check whether the text frame has any RTL characters; if so, bidi
                    // resolution will be needed.
                    let content = frame.GetContent();
                    if !nsIContent::IsSameNode(&content, aCurrContent) {
                        *aCurrContent = content;
                        let txt = content.unwrap().mCharacterDataBuffer().unwrap();
                        if txt.Is2b() && HasRTLChars(txt.Get2b()) {
                            return true;
                        }
                    }
                }
            } else if Self::ChildListMayRequireBidi(frame.PrincipalChildList_FirstChild(), aCurrContent) {
                return true;
            }

            childFrame = cf.GetNextSibling();
        }

        false
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::ResolveParagraphWithinBlock
    pub fn ResolveParagraphWithinBlock(aBpd: &mut BidiParagraphData) {
        aBpd.ClearBidiControls();
        Self::ResolveParagraph(aBpd);
        aBpd.ResetData();
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::ReorderFrames
    pub fn ReorderFrames(mut aFirstFrameOnLine: nsIFrame, mut aNumFramesOnLine: i32, aLineWM: WritingMode, aContainerSize: nsSize, mut aStart: nscoord) -> nscoord {
        let mut containerSize = aContainerSize;

        // If this line consists of a line frame, reorder the line frame's children.
        if aFirstFrameOnLine.IsLineFrame() {
            // The line frame is positioned at the start-edge, so use its size
            // as the container size.
            containerSize = aFirstFrameOnLine.GetSize();

            let Some(first) = aFirstFrameOnLine.PrincipalChildList_FirstChild() else {
                return 0;
            };
            aFirstFrameOnLine = first;
            // All children of the line frame are on the first line. Setting
            // aNumFramesOnLine to -1 makes InitLogicalArrayFromLine look at all of
            // them.
            aNumFramesOnLine = -1;
            // As the line frame itself has been adjusted at its inline-start position
            // by the caller, we do not want to apply this to its children.
            aStart = 0;
        }

        // No need to bidi-reorder the line if there's only a single frame.
        if aNumFramesOnLine == 1 {
            let bidiData = nsBidiPresUtils::GetFrameBidiData(aFirstFrameOnLine);
            let mut continuationStates = nsContinuationStates::new();
            Self::InitContinuationStates(aFirstFrameOnLine, &mut continuationStates);
            return aStart + Self::RepositionFrame(Some(aFirstFrameOnLine), bidiData.embeddingLevel.IsLTR(), aStart, &mut continuationStates, aLineWM, false, containerSize);
        }

        let bld = BidiLineData::new(aFirstFrameOnLine, aNumFramesOnLine);
        Self::RepositionInlineFrames(&bld, aLineWM, containerSize, aStart)
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::IsFirstOrLast
    pub fn IsFirstOrLast(aFrame: nsIFrame, aContinuationStates: &mut nsContinuationStates, aSpanDirMatchesLineDir: bool, aIsFirst: &mut bool, aIsLast: &mut bool) {
        /*
         * Since we lay out frames in the line's direction, visiting a frame with
         * 'mFirstVisualFrame == nullptr', means it's the first appearance of one
         * of its continuation chain frames on the line.
         * To determine if it's the last visual frame of its continuation chain on
         * the line or not, we count the number of frames of the chain on the line,
         * and then reduce it when we lay out a frame of the chain. If this value
         * becomes 1 it means that it's the last visual frame of its continuation
         * chain on this line.
         */

        let firstInLineOrder: bool;
        let lastInLineOrder: bool;
        let firstFrameId: u32;

        if aContinuationStates.Get(aFrame).mFirstVisualFrame.is_none() {
            // aFrame is the first visual frame of its continuation chain
            let mut count: u32 = 1;

            /**
             * Traverse continuation chain of aFrame in both backward and forward
             * directions while the frames are on this line. Count the frames and
             * set their mFirstVisualFrame to aFrame.
             */
            // Traverse continuation chain backward
            let mut frame = aFrame.GetPrevContinuation();
            while let Some(f) = frame {
                if !aContinuationStates.Has(f) {
                    break;
                }
                count += 1;
                aContinuationStates.Get(f).mFirstVisualFrame = Some(aFrame);
                frame = f.GetPrevContinuation();
            }
            let hasContOnPrevLines = frame.is_some();

            // Traverse continuation chain forward
            let mut frame = aFrame.GetNextContinuation();
            while let Some(f) = frame {
                if !aContinuationStates.Has(f) {
                    break;
                }
                count += 1;
                aContinuationStates.Get(f).mFirstVisualFrame = Some(aFrame);
                frame = f.GetNextContinuation();
            }
            let hasContOnNextLines = frame.is_some();

            let frameState = aContinuationStates.Get(aFrame);
            frameState.mFrameCount = count;
            frameState.mFirstVisualFrame = Some(aFrame);
            frameState.mHasContOnPrevLines = hasContOnPrevLines;
            frameState.mHasContOnNextLines = hasContOnNextLines;

            firstInLineOrder = true;
            firstFrameId = aFrame.0;
        } else {
            // aFrame is not the first visual frame of its continuation chain
            firstInLineOrder = false;
            firstFrameId = aContinuationStates.Get(aFrame).mFirstVisualFrame.unwrap().0;
        }

        let firstFrameState = *aContinuationStates.Get(nsIFrame(firstFrameId));
        lastInLineOrder = firstFrameState.mFrameCount == 1;

        if aSpanDirMatchesLineDir {
            *aIsFirst = firstInLineOrder;
            *aIsLast = lastInLineOrder;
        } else {
            *aIsFirst = lastInLineOrder;
            *aIsLast = firstInLineOrder;
        }

        if aContinuationStates.Get(aFrame).mHasContOnPrevLines {
            *aIsFirst = false;
        }
        if firstFrameState.mHasContOnNextLines {
            *aIsLast = false;
        }

        if (*aIsFirst || *aIsLast) && aFrame.HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT) {
            // For ib splits, don't treat anything except the last part as
            // endmost or anything except the first part as startmost.
            // As an optimization, only get the first continuation once.
            let firstContinuation = aFrame.FirstContinuation();
            if firstContinuation.FrameIsNonLastInIBSplit() {
                // We are not endmost
                *aIsLast = false;
            }
            if firstContinuation.FrameIsNonFirstInIBSplit() {
                // We are not startmost
                *aIsFirst = false;
            }
        }

        // Reduce number of remaining frames of the continuation chain on the line.
        aContinuationStates.Get(nsIFrame(firstFrameId)).mFrameCount -= 1;

        if (aFrame.IsInlineFrame() || aFrame.IsLineFrame()) {
            aFrame.AddStateBits(NS_INLINE_FRAME_BIDI_VISUAL_STATE_IS_SET);

            if *aIsFirst {
                aFrame.AddStateBits(NS_INLINE_FRAME_BIDI_VISUAL_IS_FIRST);
            } else {
                aFrame.RemoveStateBits(NS_INLINE_FRAME_BIDI_VISUAL_IS_FIRST);
            }

            if *aIsLast {
                aFrame.AddStateBits(NS_INLINE_FRAME_BIDI_VISUAL_IS_LAST);
            } else {
                aFrame.RemoveStateBits(NS_INLINE_FRAME_BIDI_VISUAL_IS_LAST);
            }
        }
    }

    /*
     * Position aFrame and its descendants to their visual places. Also if aFrame
     * is not leaf, resize it to embrace its children.
     *
     * @param aFrame               The frame which itself and its children are
     *                             going to be repositioned
     * @param aIsEvenLevel         TRUE means the embedding level of this frame
     *                             is even (LTR)
     * @param aStartOrEnd          The distance to the start or end of aFrame
     *                             without considering its inline margin. If the
     *                             container reverses the order of its children,
     *                             it is the distance to the end, otherwise, it is
     *                             the distance to the start.
     * @param aContinuationStates  A map from nsIFrame* to nsFrameContinuationState
     * @param aContainerWM         The writing mode of the container frame
     * @param aContainerReverseDir Whether the container reverses the order
     *                             of its children
     * @param aContainerSize       The size of the container
     * @return                     The inline size of aFrame
     */
    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::RepositionFrame
    pub fn RepositionFrame(aFrame: Option<nsIFrame>, aIsEvenLevel: bool, aStartOrEnd: nscoord, aContinuationStates: &mut nsContinuationStates, aContainerWM: WritingMode, aContainerReverseDir: bool, aContainerSize: nsSize) -> nscoord {
        let lineSize = if aContainerWM.IsVertical() { aContainerSize.height } else { aContainerSize.width };
        NS_ASSERTION!(lineSize != NS_UNCONSTRAINEDSIZE, "Unconstrained inline line size in bidi frame reordering");
        let Some(aFrame) = aFrame else {
            return 0;
        };

        let mut isFirst = false;
        let mut isLast = false;
        let frameWM = aFrame.GetWritingMode();
        Self::IsFirstOrLast(aFrame, aContinuationStates, aContainerWM.IsBidiLTR() == frameWM.IsBidiLTR(), &mut isFirst, &mut isLast);

        // We only need the margin if the frame is first or last in its own
        // writing mode, but we're traversing the frames in the order of the
        // container's writing mode. To get the right values, we set start and
        // end margins on a logical margin in the frame's writing mode, and
        // then convert the margin to the container's writing mode to set the
        // coordinates.

        // This method is called from nsBlockFrame::PlaceLine via the call to
        // bidiUtils->ReorderFrames, so this is guaranteed to be after the inlines
        // have been reflowed, which is required for GetUsedMargin/Border/Padding
        let mut frameISize = aFrame.ISize(frameWM);
        let mut frameMargin = aFrame.GetLogicalUsedMargin(frameWM);
        let mut borderPadding = aFrame.GetLogicalUsedBorderAndPadding(frameWM);
        // Since the visual order of frame could be different from the continuation
        // order, we need to remove any inline border/padding [that is already applied
        // based on continuation order] and then add it back based on the visual order
        // (i.e. isFirst/isLast) to get the correct isize for the current frame.
        // We don't need to do that for 'box-decoration-break:clone' because then all
        // continuations have border/padding/margin applied.
        if aFrame.StyleBorder().mBoxDecorationBreak() == StyleBoxDecorationBreak::Slice {
            // First remove the border/padding that was applied based on logical order.
            if aFrame.GetPrevContinuation().is_none() {
                frameISize -= borderPadding.IStart(frameWM);
            }
            if aFrame.GetNextContinuation().is_none() {
                frameISize -= borderPadding.IEnd(frameWM);
            }
            // Set margin/border/padding based on visual order.
            if !isFirst {
                *frameMargin.IStart_mut(frameWM) = 0;
                *borderPadding.IStart_mut(frameWM) = 0;
            }
            if !isLast {
                *frameMargin.IEnd_mut(frameWM) = 0;
                *borderPadding.IEnd_mut(frameWM) = 0;
            }
            // Add the border/padding which is now based on visual order.
            frameISize += borderPadding.IStartEnd(frameWM);
        }

        let mut icoord: nscoord = 0;
        if IsBidiLeaf(aFrame) {
            icoord += if frameWM.IsOrthogonalTo(&aContainerWM) { aFrame.BSize(frameWM) } else { frameISize };
        } else if IsRubyBox(aFrame.Type()) {
            // NOT PORTED: ruby (nsBidiPresUtils::RepositionRubyFrame)
            unimplemented!("NOT PORTED: ruby (nsBidiPresUtils::RepositionRubyFrame)");
        } else {
            let reverseDir = aIsEvenLevel != frameWM.IsBidiLTR();
            icoord += if reverseDir { borderPadding.IEnd(frameWM) } else { borderPadding.IStart(frameWM) };
            let logicalSize = LogicalSize::new(frameWM, frameISize, aFrame.BSize(frameWM));
            let frameSize = logicalSize.GetPhysicalSize(frameWM);
            // Reposition the child frames
            let mut f = aFrame.PrincipalChildList_FirstChild();
            while let Some(fr) = f {
                icoord += Self::RepositionFrame(Some(fr), aIsEvenLevel, icoord, aContinuationStates, frameWM, reverseDir, frameSize);
                f = fr.GetNextSibling();
            }
            icoord += if reverseDir { borderPadding.IStart(frameWM) } else { borderPadding.IEnd(frameWM) };
        }

        // In the following variables, if aContainerReverseDir is true, i.e.
        // the container is positioning its children in reverse of its logical
        // direction, the "StartOrEnd" refers to the distance from the frame
        // to the inline end edge of the container, elsewise, it refers to the
        // distance to the inline start edge.
        let margin = frameMargin.ConvertTo(aContainerWM, frameWM);
        let marginStartOrEnd = if aContainerReverseDir { margin.IEnd(aContainerWM) } else { margin.IStart(aContainerWM) };
        let frameStartOrEnd = aStartOrEnd + marginStartOrEnd;

        let mut rect = aFrame.GetLogicalRect__WritingMode_nsSize(aContainerWM, aContainerSize);
        *rect.ISize_mut(aContainerWM) = icoord;
        *rect.IStart_mut(aContainerWM) = if aContainerReverseDir { lineSize - frameStartOrEnd - icoord } else { frameStartOrEnd };
        aFrame.SetRect__WritingMode_LogicalRect_nsSize(aContainerWM, rect, aContainerSize);

        icoord + margin.IStartEnd(aContainerWM)
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::InitContinuationStates
    pub fn InitContinuationStates(aFrame: nsIFrame, aContinuationStates: &mut nsContinuationStates) {
        aContinuationStates.Insert(aFrame);
        if !IsBidiLeaf(aFrame) {
            // Continue for child frames
            let mut frame = aFrame.PrincipalChildList_FirstChild();
            while let Some(f) = frame {
                Self::InitContinuationStates(f, aContinuationStates);
                frame = f.GetNextSibling();
            }
        }
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::RepositionInlineFrames
    pub fn RepositionInlineFrames(aBld: &BidiLineData, aLineWM: WritingMode, aContainerSize: nsSize, mut aStart: nscoord) -> nscoord {
        let mut continuationStates = nsContinuationStates::new();
        aBld.InitContinuationStates(&mut continuationStates);

        if aLineWM.IsBidiLTR() {
            for index in 0..aBld.VisualFrameCount() {
                let (frame, level) = aBld.VisualFrameAndLevelAt(index);
                aStart += Self::RepositionFrame(Some(frame), level.IsLTR(), aStart, &mut continuationStates, aLineWM, false, aContainerSize);
            }
        } else {
            for index in (0..aBld.VisualFrameCount()).rev() {
                let (frame, level) = aBld.VisualFrameAndLevelAt(index);
                aStart += Self::RepositionFrame(Some(frame), level.IsLTR(), aStart, &mut continuationStates, aLineWM, false, aContainerSize);
            }
        }

        aStart
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::GetFirstLeaf
    pub fn GetFirstLeaf(aFrame: nsIFrame) -> nsIFrame {
        let mut firstLeaf = aFrame;
        while !IsBidiLeaf(firstLeaf) {
            let firstChild = firstLeaf.PrincipalChildList_FirstChild().unwrap();
            // nsPlaceholderFrame::GetRealFrameFor
            let realFrame = if firstChild.IsPlaceholderFrame() { nsPlaceholderFrame::GetRealFrameForPlaceholder(firstChild) } else { firstChild };
            firstLeaf = if realFrame.IsLetterFrame() { realFrame } else { firstChild };
        }
        firstLeaf
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::GetFrameBidiData
    pub fn GetFrameBidiData(aFrame: nsIFrame) -> FrameBidiData {
        Self::GetFirstLeaf(aFrame).GetBidiData()
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::GetFrameEmbeddingLevel
    pub fn GetFrameEmbeddingLevel(aFrame: nsIFrame) -> BidiEmbeddingLevel {
        Self::GetFirstLeaf(aFrame).GetEmbeddingLevel()
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::GetFrameBaseLevel
    pub fn GetFrameBaseLevel(aFrame: nsIFrame) -> BidiEmbeddingLevel {
        let mut firstLeaf = aFrame;
        while !IsBidiLeaf(firstLeaf) {
            firstLeaf = firstLeaf.PrincipalChildList_FirstChild().unwrap();
        }
        firstLeaf.GetBaseLevel()
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::EnsureBidiContinuation
    // (returns aNewFrame, the out-parameter)
    pub fn EnsureBidiContinuation(aFrame: nsIFrame, aLine: nsLineList_iterator, aStart: i32, aEnd: i32) -> nsIFrame {
        AdjustOffsetsForBidi(aFrame, aStart, aEnd);
        CreateContinuation(aFrame, aLine, false)
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::RemoveBidiContinuation
    pub fn RemoveBidiContinuation(aBpd: &BidiParagraphData, aFrame: nsIFrame, aFirstIndex: i32, aLastIndex: i32) {
        // If we're only processing one frame, and it is already a fluid continuation
        // (next-in-flow), there's nothing to do.
        if aLastIndex == aFirstIndex + 1 && aFrame.GetNextInFlow() == aFrame.GetNextContinuation() {
            return;
        }
        let mut bidiData = aFrame.GetBidiData();
        bidiData.precedingControl = kBidiLevelNone;
        for index in (aFirstIndex + 1)..=aLastIndex {
            let mut frame = aBpd.FrameAt(index);
            if frame != Some(NS_BIDI_CONTROL_FRAME) {
                // Make the frame and its continuation ancestors fluid,
                // so they can be reused or deleted by normal reflow code
                frame.unwrap().SetProperty(nsIFrame::BidiDataProperty(), bidiData);
                frame.unwrap().AddStateBits(NS_FRAME_IS_BIDI);
                while let Some(f) = frame {
                    if !IsBidiSplittable(f) {
                        break;
                    }
                    let prev = f.GetPrevContinuation();
                    if let Some(prev) = prev {
                        MakeContinuationFluid(prev, f);
                        frame = f.GetParent();
                    } else {
                        break;
                    }
                }
            }
        }

        // Make sure that the last continuation we made fluid does not itself have a
        // fluid continuation (this can happen when re-resolving after dynamic changes
        // to content)
        let lastFrame = aBpd.FrameAt(aLastIndex).unwrap();
        MakeContinuationsNonFluidUpParentChain(lastFrame, lastFrame.GetNextInFlow());
    }

    // Gecko: layout/base/nsBidiPresUtils.cpp nsBidiPresUtils::BidiLevelFromStyle
    pub fn BidiLevelFromStyle(aComputedStyle: &ServoArc<ComputedValues>) -> BidiEmbeddingLevel {
        if aComputedStyle.get_text().unicode_bidi == StyleUnicodeBidi::Plaintext {
            return BidiEmbeddingLevel::DefaultLTR();
        }

        if aComputedStyle.get_inherited_box().direction == StyleDirection::Rtl {
            return BidiEmbeddingLevel::RTL();
        }

        BidiEmbeddingLevel::LTR()
    }
}

// Firefox tests bidi with reftests and mochitests that need a whole browser; these tests are written
// from the documented behaviour (the Unicode Bidirectional Algorithm, CSS Writing Modes 3 chapter 2,
// and what nsBidiPresUtils::Resolve says in its comments): a snippet of HTML is parsed, styled with
// Firefox's user-agent sheets, laid out (driver.rs), and the embedding levels, the frame splits, the
// text runs and the visual positions are what the algorithm gives.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::*;
    use mrt_style::mrt_dom::Dom;
    use mrt_style::StyleDom;

    const PX: i32 = 60;

    // Parses, styles and lays `html` out at 1000 x 800 CSS px. The frame tree is this test thread's.
    fn layout(html: &str) -> PageLayout {
        let mut sd = StyleDom::new(Dom::parse(html), (1000.0, 800.0));
        sd.resolve();
        let sd = KeepDocumentAlive(sd);
        LayoutDocument(&sd, 1000, 800)
    }

    // The text frames of the tree in creation order: (frame, text of its range).
    fn text_frames() -> Vec<(nsTextFrame, String)> {
        let n = WithFrameTree(|t| t.mFrames.len());
        let mut v = Vec::new();
        for id in 0..n as u32 {
            let f = nsIFrame(id);
            if f.Type() == LayoutFrameType::Text {
                let tf = nsTextFrame(f);
                let buf = tf.CharacterDataBuffer();
                let units: Vec<u16> = if buf.Is2b() { buf.Get2b().to_vec() } else { buf.Get1b().iter().map(|&b| b as u16).collect() };
                let (start, end) = tf.GetOffsets();
                v.push((tf, String::from_utf16_lossy(&units[start as usize..end as usize])));
            }
        }
        v
    }

    const HE: &str = "\u{5e9}\u{5dc}\u{5d5}\u{5dd}"; // a Hebrew word of four letters

    #[test]
    fn a_page_without_rtl_text_is_not_bidi() {
        // nsBidiPresUtils::Resolve is only run when the document has RTL characters or an RTL direction
        // (Document::GetBidiEnabled), so no frame gets bidi data.
        layout("<!DOCTYPE html><html><body><p>abc def</p></body></html>");
        assert!(!nsPresContext::Current().BidiEnabled());
        for (f, _) in text_frames() {
            assert!(!f.HasAnyStateBits(NS_FRAME_IS_BIDI));
            assert!(!f.HasProperty(nsIFrame::BidiDataProperty()));
        }
    }

    #[test]
    fn an_rtl_word_in_ltr_text_splits_the_frame_in_three() {
        // UAX #9: "abc " and " def" are level 0 and the Hebrew word level 1 (the spaces next to a
        // strong LTR character are LTR). Resolve cuts the text frame at each level change with a
        // non-fluid continuation, and each piece has its own text run.
        layout(&format!("<!DOCTYPE html><html><body><p>abc {HE} def</p></body></html>"));
        assert!(nsPresContext::Current().BidiEnabled());
        let frames = text_frames();
        let texts: Vec<&str> = frames.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(texts, vec!["abc ", HE, " def"]);
        let levels: Vec<u8> = frames.iter().map(|(f, _)| f.GetEmbeddingLevel().Value()).collect();
        assert_eq!(levels, vec![0, 1, 0]);
        // continuations, but not in-flows: the splits are not fluid
        assert_eq!(frames[0].0.GetNextContinuation(), Some(frames[1].0 .0));
        assert_eq!(frames[0].0.GetNextInFlow(), None);
        assert_eq!(frames[1].0.GetNextContinuation(), Some(frames[2].0 .0));
        assert_eq!(frames[1].0.GetNextInFlow(), None);
        assert!(frames.iter().all(|(f, _)| f.HasAnyStateBits(NS_FRAME_IS_BIDI)));
        assert_eq!(TextRunReportOfAllFrames().runs.len(), 3);
    }

    #[test]
    fn the_flow_length_stops_at_a_non_fluid_continuation() {
        // nsTextFrame::GetInFlowContentLength: with bidi it is the length up to the next
        // non-fluid continuation (4 for "abc "), not the rest of the text node (12).
        layout(&format!("<!DOCTYPE html><html><body><p>abc {HE} def</p></body></html>"));
        let frames = text_frames();
        assert_eq!(frames[0].0.GetInFlowContentLength(), 4);
        assert_eq!(frames[1].0.GetInFlowContentLength(), 4);
        assert_eq!(frames[2].0.GetInFlowContentLength(), 4);
    }

    #[test]
    fn latin_text_in_an_rtl_paragraph_is_level_two() {
        // UAX #9 rule I1/I2: LTR letters in an RTL paragraph go up two levels. direction:rtl turns
        // bidi on by itself (nsIFrame::DidSetComputedStyle calls SetBidiEnabled).
        layout("<!DOCTYPE html><html><body><p style='direction:rtl'>abc</p></body></html>");
        assert!(nsPresContext::Current().BidiEnabled());
        let frames = text_frames();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].0.GetBaseLevel().Value(), 1);
        assert_eq!(frames[0].0.GetEmbeddingLevel().Value(), 2);
    }

    #[test]
    fn a_bdi_element_ends_the_text_run_around_it() {
        // html.css: bdi { unicode-bidi: isolate }. PreventCrossBoundaryShaping case 3: the boundary of
        // an isolate ends the run, bidi text or not.
        layout("<!DOCTYPE html><html><body><p>abc <bdi>def</bdi> ghi</p></body></html>");
        assert_eq!(text_frames().len(), 3);
        assert_eq!(TextRunReportOfAllFrames().runs.len(), 3);
    }

    #[test]
    fn the_first_box_of_an_rtl_line_is_at_the_right_edge() {
        // Visual order (nsBidiPresUtils::ReorderFrames): in a direction:rtl block the first box in
        // logical order is placed at the line's end, so the 100px box is at x = 300 - 100 and the
        // 50px one next to it on its left.
        layout("<!DOCTYPE html><html><body style='margin:0'><div style='direction:rtl;width:300px'><span style='display:inline-block;width:100px;height:10px'></span><span style='display:inline-block;width:50px;height:10px'></span></div></body></html>");
        let n = WithFrameTree(|t| t.mFrames.len());
        let mut boxes = Vec::new();
        for id in 0..n as u32 {
            let f = nsIFrame(id);
            if f.Type() == LayoutFrameType::Block && f.GetSize().height == 10 * PX {
                boxes.push((f.GetSize().width, f.GetRect().x));
            }
        }
        assert_eq!(boxes, vec![(100 * PX, 200 * PX), (50 * PX, 150 * PX)]);
    }

    #[test]
    fn an_element_with_a_dir_attribute_is_an_isolate() {
        // html.css: :-moz-has-dir-attr { unicode-bidi: isolate } (the state is set for any dir
        // attribute, valid or not), so the run ends around the span as it does around a bdi.
        layout("<!DOCTYPE html><html><body><p>abc <span dir='ltr'>def</span> ghi</p></body></html>");
        assert_eq!(TextRunReportOfAllFrames().runs.len(), 3);
    }

    #[test]
    fn a_bdi_without_dir_is_like_auto_but_a_plain_span_is_not() {
        layout("<!DOCTYPE html><html><body><p>abc <span>def</span> ghi</p></body></html>");
        assert_eq!(TextRunReportOfAllFrames().runs.len(), 1);
    }

    #[test]
    fn a_link_is_underlined_by_the_user_agent_sheet() {
        // ua.css: :any-link { text-decoration: underline }. The rule only matches when the element's
        // state says it is a link (Stylo files the rule in a bucket it reads for that state).
        use crate::nsLineLayoutAdapter::ComputedStyle_LineLayoutAdapter;
        layout("<!DOCTYPE html><html><body><a href='#'>x</a> <a>y</a></body></html>");
        let decorated: Vec<(String, bool)> = text_frames().iter().map(|(f, t)| (t.clone(), f.Style().HasTextDecorationLines())).collect();
        assert_eq!(decorated, vec![("x".to_string(), true), (" ".to_string(), false), ("y".to_string(), false)]);
    }
}
