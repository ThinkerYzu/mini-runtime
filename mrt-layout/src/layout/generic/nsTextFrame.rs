// Gecko: layout/generic/nsTextFrame.h, layout/generic/nsTextFrame.cpp
//
// nsTextFrame, nsContinuingTextFrame, nsTextFrame::PropertyProvider and BuildTextRunsScanner,
// translated in the order of nsTextFrame.cpp. See CONVENTIONS.md; the way this file differs from C++
// is in these places, each marked again where it happens:
//
//  - Frame classes. `nsTextFrame` is a handle (`nsTextFrame(nsIFrame)`) that Derefs to `nsIFrame`, like
//    nsInlineFrame. nsContinuingTextFrame is not a separate class: it is an `nsTextFrame` whose
//    `mIsContinuing` is set by `nsContinuingTextFrame::Init`; the members it overrides
//    (GetPrevContinuation, FirstContinuation, FirstInFlow, the AddInline*ISize no-ops) check that.
//    A frame's members (`mTextRun`, `mContentOffset`, ...) live in a thread-local table, reached
//    through accessors; `f->mContentOffset = x` is `f.set_mContentOffset(x)`.
//  - Continuations. As in Gecko, a frame has mPrevContinuation and mNextContinuation, and a link is an
//    in-flow when the later frame has NS_FRAME_IS_FLUID_CONTINUATION (nsIFrame.rs: SetNextInFlow,
//    SetNextContinuation, ...). Bidi resolution makes the non-fluid ones. The caches of the first
//    continuation (mFirstContinuation, mFirstInFlow) and the continuations array are not kept.
//  - Text runs. `RefPtr<gfxTextRun>` is `RefPtr_gfxTextRun`. A text run's user data (the list of
//    TextRunMappedFlow) is stored in the run (`gfxTextRun::mUserData`, an enum
//    `gfxTextRun_UserData`): the frame of a simple flow (flagged `IsSimpleFlow` as in Gecko) or a
//    TextRunUserData with an exact-size array of the flows.
//  - The text. `mContent->AsText()->DataBuffer()` is `nsTextFrame::CharacterDataBuffer()`, an
//    `Rc<CharacterDataBuffer>` (src/dom/base/CharacterDataBuffer.rs) made from the DOM node the frame's
//    nsIContent stands for (see nsIContent in nsIFrame.rs).
//  - The frame properties of the file (TabWidthProperty, HangableWhitespaceProperty,
//    TrimmableWhitespaceProperty, JustificationAssignmentProperty, UninflatedTextRunProperty,
//    FontSizeInflationProperty) are real frame properties, as in Gecko (src/layout/base/FrameProperties.rs).
//
// NOT PORTED (each stays in place as a stub with the Gecko code or a pointer to it): painting
// (BuildDisplayList, PaintText, DrawText, selection and decoration painting, PaintShadows), selection
// and the caret (GetSelectionDetails, PeekOffset*, GetPointFromOffset, CalcContentOffsetsFromFramePoint,
// GetChildFrameContainingOffset), find, ruby, text-combine-upright, SVG text, MathML text, font size inflation (always 1), hyphenation
// (soft hyphens still break through the line breaker's flags but there is no hyphen glyph measuring),
// text-overflow ellipsis, justification (ComputeJustification, SetupJustificationSpacing: the line gets
// no justification opportunities, so `text-align: justify` lays out as `start`), vertical text,
// `text-transform` in layout (identity: no transformed text runs; the rendered text applies the case
// transforms, see nsTextRunTransformations.rs), text-autospace,
// ::first-letter and ::first-line special cases (FindFirstLetterRange, SetFirstLetterLength,
// MaybeSplitFramesForFirstLetter), password masking, accessibility (the rendered text is ported, for
// innerText), glyph observers
// (SimpleTextRunUserData / ComplexTextRunUserData), the offset-to-frame cache, invalidation.
//
// The text-run layer (gfxTextRun, gfxFontGroup, nsFontMetrics) is another translation
// (gfxTextRun.rs, gfxFont.rs, nsFontMetrics.rs). A `RefPtr<gfxTextRun>` is `Rc<RefCell<gfxTextRun>>`;
// the trait RefPtr_gfxTextRun_Ext at the end gives it the members of gfxTextRun under their Gecko names.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

// ---------------------------------------------------------------------------------------------
// Frame state bits
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsFrameStateBits.h, group Text. The numbers are Gecko's own within the
// group (they overlap the Block group's numbers in nsFrameState.rs, which is harmless: no frame is
// both a text frame and a block). TEXT_OFFSETS_NEED_FIXING is defined in nsBlockFrame.rs.
pub const TEXT_FIRST_LETTER: nsFrameState = 1 << 20;
pub const TEXT_START_OF_LINE: nsFrameState = 1 << 21;
pub const TEXT_END_OF_LINE: nsFrameState = 1 << 22;
pub const TEXT_HYPHEN_BREAK: nsFrameState = 1 << 23;
pub const TEXT_TRIMMED_TRAILING_WHITESPACE: nsFrameState = 1 << 24;
pub const TEXT_JUSTIFICATION_ENABLED: nsFrameState = 1 << 25;
pub const TEXT_SELECTION_UNDERLINE_OVERFLOWED: nsFrameState = 1 << 26;
pub const TEXT_IS_ONLY_WHITESPACE: nsFrameState = 1 << 27;
pub const TEXT_ISNOT_ONLY_WHITESPACE: nsFrameState = 1 << 28;
pub const TEXT_IN_TEXTRUN_USER_DATA: nsFrameState = 1 << 29;
pub const TEXT_HAS_NONCOLLAPSED_CHARACTERS: nsFrameState = 1 << 31;
pub const TEXT_IS_IN_TOKEN_MATHML: nsFrameState = 1 << 32;
pub const TEXT_IN_UNINFLATED_TEXTRUN_USER_DATA: nsFrameState = 1 << 60;
pub const TEXT_HAS_FONT_INFLATION: nsFrameState = 1 << 61;
pub const TEXT_NO_RENDERED_GLYPHS: nsFrameState = 1 << 62;
pub const TEXT_IN_OFFSET_CACHE: nsFrameState = 1 << 63;

// Gecko: layout/generic/nsFrameStateBits.h, group Generic. This crate's own numbers (see nsFrameState.rs).
// Every continuation of a text frame is fluid here (see the top of the file), so the bit is set by
// nsContinuingTextFrame::Init and never cleared.
pub const NS_FRAME_IS_FLUID_CONTINUATION: nsFrameState = 1 << 13;
// Gecko: layout/generic/nsFrameStateBits.h NS_FRAME_IS_BIDI (Generic group: any leaf frame can get it,
// so its bit must not be one that a class of frames uses for something else; bit 17 was the first
// of the NS_INLINE_FRAME_BIDI_VISUAL bits, which moved to bits only the inline frames read).
pub const NS_FRAME_IS_BIDI: nsFrameState = 1 << 17;

// Gecko: layout/generic/nsTextFrame.cpp TEXT_REFLOW_FLAGS
pub const TEXT_REFLOW_FLAGS: nsFrameState = TEXT_FIRST_LETTER
    | TEXT_START_OF_LINE
    | TEXT_END_OF_LINE
    | TEXT_HYPHEN_BREAK
    | TEXT_TRIMMED_TRAILING_WHITESPACE
    | TEXT_JUSTIFICATION_ENABLED
    | TEXT_HAS_NONCOLLAPSED_CHARACTERS
    | TEXT_SELECTION_UNDERLINE_OVERFLOWED
    | TEXT_NO_RENDERED_GLYPHS;

// Gecko: layout/generic/nsTextFrame.cpp TEXT_WHITESPACE_FLAGS
pub const TEXT_WHITESPACE_FLAGS: nsFrameState = TEXT_IS_ONLY_WHITESPACE | TEXT_ISNOT_ONLY_WHITESPACE;

// ---------------------------------------------------------------------------------------------
// Tab widths
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsTextFrame.cpp TabWidth
#[derive(Clone, Copy, Debug)]
pub struct TabWidth {
    pub mOffset: u32, // DOM offset relative to the current frame's offset.
    pub mWidth: f32,  // extra space to be added at this position (in app units)
}

impl TabWidth {
    // Gecko: layout/generic/nsTextFrame.cpp TabWidth::TabWidth
    pub fn new(aOffset: u32, aWidth: u32) -> TabWidth {
        TabWidth { mOffset: aOffset, mWidth: aWidth as f32 }
    }
}

// Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::TabWidthStore
#[derive(Clone, Debug, Default)]
pub struct nsTextFrame_TabWidthStore {
    // Offset up to which tabs have been measured; positions beyond this have not
    // been calculated yet but may be appended if needed later.  It's a DOM
    // offset relative to the current frame's offset.
    pub mLimit: u32,

    // Need to recalc tab offsets if frame content offset differs from this.
    pub mValidForContentOffset: i32,

    // A TabWidth record for each tab character measured so far.
    pub mWidths: Vec<TabWidth>,
}

impl nsTextFrame_TabWidthStore {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::TabWidthStore::TabWidthStore
    pub fn new(aValidForContentOffset: i32) -> nsTextFrame_TabWidthStore {
        nsTextFrame_TabWidthStore { mLimit: 0, mValidForContentOffset: aValidForContentOffset, mWidths: Vec::new() }
    }

    // Apply tab widths to the aSpacing array, which corresponds to characters
    // beginning at aOffset and has length aLength. (Width records outside this
    // range will be ignored.)
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::TabWidthStore::ApplySpacing
    pub fn ApplySpacing(&self, aSpacing: &mut [gfxFont_Spacing], aOffset: u32, aLength: u32) {
        let mut i: usize = 0;
        let len = self.mWidths.len();

        // If aOffset is non-zero, do a binary search to find where to start
        // processing the tab widths, in case the list is really long. (See bug
        // 953247.)
        // We need to start from the first entry where mOffset >= aOffset.
        if aOffset > 0 {
            // mozilla::BinarySearch(TabwidthAdaptor(mWidths), 0, len, aOffset, &i): the first index
            // whose offset is not below aOffset (the found one, or the insertion point).
            i = self.mWidths.partition_point(|w| w.mOffset < aOffset);
        }

        let limit = aOffset + aLength;
        while i < len {
            let tw = &self.mWidths[i];
            if tw.mOffset >= limit {
                break;
            }
            aSpacing[(tw.mOffset - aOffset) as usize].mAfter += tw.mWidth as nscoord;
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Text run user data
// ---------------------------------------------------------------------------------------------

/*
 * Some general notes
 *
 * Text frames delegate work to gfxTextRun objects. The gfxTextRun object
 * transforms text to positioned glyphs. It can report the geometry of the
 * glyphs and paint them. Text frames configure gfxTextRuns by providing text,
 * spacing, language, and other information.
 *
 * A gfxTextRun can cover more than one DOM text node. This is necessary to
 * get kerning, ligatures and shaping for text that spans multiple text nodes
 * but is all the same font.
 *
 * The userdata for a gfxTextRun object can be:
 *
 *   - A nsTextFrame* in the case a text run maps to only one flow. In this
 *   case, the textrun's user data pointer is a pointer to mStartFrame for that
 *   flow, mDOMOffsetToBeforeTransformOffset is zero, and mContentLength is the
 *   length of the text node.
 *
 *   - A SimpleTextRunUserData in the case a text run maps to one flow, but we
 *   still have to keep a list of glyph observers.
 *
 *   - A ComplexTextRunUserData in the case a text run maps to multiple flows,
 *   but we need to keep a list of glyph observers.
 *
 *   - A TextRunUserData in the case a text run maps multiple flows, but it
 *   doesn't have any glyph observer for changes in SVG fonts.
 *
 * You can differentiate between the four different cases with the
 * IsSimpleFlow and MightHaveGlyphChanges flags.
 *
 * We go to considerable effort to make sure things work even if in-flow
 * siblings have different ComputedStyles (i.e., first-letter and first-line).
 *
 * Our convention is that unsigned integer character offsets are offsets into
 * the transformed string. Signed integer character offsets are offsets into
 * the DOM string.
 *
 * XXX currently we don't handle hyphenated breaks between text frames where the
 * hyphen occurs at the end of the first text frame, e.g.
 *   <b>Kit&shy;</b>ty
 */

// Rust: of the four user data cases of Gecko (SimpleTextRunUserData, TextRunUserData,
// ComplexTextRunUserData and the frame pointer of the simple case) two exist: no glyph observers
// (NOT PORTED), so SimpleTextRunUserData and ComplexTextRunUserData are not needed. The frame
// pointer is gfxTextRun_UserData::SimpleFlow, TextRunUserData is gfxTextRun_UserData::Flows.

/**
 * We use an array of these objects to record which text frames
 * are associated with the textrun. mStartFrame is the start of a list of
 * text frames. Some sequence of its continuations are covered by the textrun.
 * A content textnode can have at most one TextRunMappedFlow associated with it
 * for a given textrun.
 *
 * mDOMOffsetToBeforeTransformOffset is added to DOM offsets for those frames to
 * obtain the offset into the before-transformation text of the textrun. It can
 * be positive (when a text node starts in the middle of a text run) or negative
 * (when a text run starts in the middle of a text node). Of course it can also
 * be zero.
 */
// Gecko: layout/generic/nsTextFrame.cpp TextRunMappedFlow
#[derive(Clone, Copy, Debug)]
pub struct TextRunMappedFlow {
    pub mStartFrame: nsTextFrame,
    pub mDOMOffsetToBeforeTransformOffset: i32,
    // The text mapped starts at mStartFrame->GetContentOffset() and is this long
    pub mContentLength: u32,
}

// Gecko: layout/generic/nsTextFrame.cpp TextRunUserData
#[derive(Clone, Debug, Default)]
pub struct TextRunUserData {
    // ADAPTER: an exact-size array (Gecko: mMappedFlowCount and the flows allocated after the
    // struct), so there is no spare capacity.
    pub mMappedFlows: Box<[TextRunMappedFlow]>,
    pub mLastFlowIndex: u32,
}

// ADAPTER: the user data is stored in the text run itself (gfxTextRun::mUserData, an enum, see
// gfxTextRun_UserData), as in Gecko, instead of in a table keyed by the address of the run.

// Gecko: gfxTextRun::GetUserData() != nullptr
fn HasTextRunUserData(aTextRun: &RefPtr_gfxTextRun) -> bool {
    !matches!(aTextRun.borrow().GetUserData(), gfxTextRun_UserData::None)
}

// Gecko: layout/generic/nsTextFrame.cpp CreateUserData (and gfxTextRun::SetUserData)
fn SetTextRunUserData(aTextRun: &RefPtr_gfxTextRun, aUserData: gfxTextRun_UserData) {
    aTextRun.borrow_mut().SetUserData(aUserData);
}

// Gecko: layout/generic/nsTextFrame.cpp DestroyTextRunUserData
fn DestroyTextRunUserData(aTextRun: &RefPtr_gfxTextRun) {
    MOZ_ASSERT!(HasTextRunUserData(aTextRun));
    aTextRun.borrow_mut().SetUserData(gfxTextRun_UserData::None);
}

// Gecko: layout/generic/nsTextFrame.cpp GetMappedFlows (and the TextRunUserData cast)
// ADAPTER: a copy of the flows (a simple flow has none stored: its one flow is made up here, with
// the length of the text node as mContentLength, as the comment on the user data says).
fn GetMappedFlows(aTextRun: &RefPtr_gfxTextRun) -> Vec<TextRunMappedFlow> {
    match aTextRun.borrow().GetUserData() {
        gfxTextRun_UserData::None => {
            MOZ_ASSERT!(false, "UserData must exist.");
            Vec::new()
        }
        gfxTextRun_UserData::SimpleFlow(frame) => vec![TextRunMappedFlow {
            mStartFrame: *frame,
            mDOMOffsetToBeforeTransformOffset: 0,
            mContentLength: frame.CharacterDataBuffer().GetLength(),
        }],
        gfxTextRun_UserData::Flows(userData) => userData.mMappedFlows.to_vec(),
    }
}

// ADAPTER: the TextRunUserData cast of a multi-flow run, copied out of the run.
fn GetTextRunUserData(aTextRun: &RefPtr_gfxTextRun) -> TextRunUserData {
    match aTextRun.borrow().GetUserData() {
        gfxTextRun_UserData::Flows(userData) => userData.clone(),
        _ => panic!("not a multi-flow text run"),
    }
}

// Gecko: layout/generic/nsTextFrame.cpp GetFrameForSimpleFlow
/**
 * These are utility functions just for helping with the complexity related with
 * the text runs user data.
 */
fn GetFrameForSimpleFlow(aTextRun: &RefPtr_gfxTextRun) -> nsTextFrame {
    MOZ_ASSERT!(aTextRun.GetFlags2().0 & nsTextFrameUtils_Flags::IsSimpleFlow.0 != 0, "Not so simple flow?");
    match aTextRun.borrow().GetUserData() {
        gfxTextRun_UserData::SimpleFlow(frame) => *frame,
        _ => panic!("Not so simple flow?"),
    }
}

/**
 * Remove |aTextRun| from the frame continuation chain starting at
 * |aStartContinuation| if non-null, otherwise starting at |aFrame|.
 * Unmark |aFrame| as a text run owner if it's the frame we start at.
 * Return true if |aStartContinuation| is non-null and was found
 * in the next-continuation chain of |aFrame|.
 */
// Gecko: layout/generic/nsTextFrame.cpp ClearAllTextRunReferences
fn ClearAllTextRunReferences(aFrame: nsTextFrame, aTextRun: &RefPtr_gfxTextRun, aStartContinuation: Option<nsTextFrame>, aWhichTextRunState: nsFrameState) -> bool {
    let mut aFrame = Some(aFrame);
    MOZ_ASSERT!(aFrame.is_some(), "null frame");
    MOZ_ASSERT!(
        aStartContinuation.map_or(true, |s| {
            let infl = s.GetTextRun(nsTextFrame_TextRunType::eInflated);
            let uninfl = s.GetTextRun(nsTextFrame_TextRunType::eNotInflated);
            infl.as_ref().map_or(true, |r| Rc::ptr_eq(r, aTextRun)) || uninfl.as_ref().map_or(true, |r| Rc::ptr_eq(r, aTextRun))
        }),
        "wrong aStartContinuation for this text run"
    );

    if aStartContinuation.is_none() || aStartContinuation == aFrame {
        aFrame.unwrap().RemoveStateBits(aWhichTextRunState);
    } else {
        loop {
            NS_ASSERTION!(aFrame.unwrap().IsTextFrame(), "Bad frame");
            aFrame = aFrame.unwrap().GetNextContinuation().map(nsTextFrame::from_frame);
            if !(aFrame.is_some() && aFrame != aStartContinuation) {
                break;
            }
        }
    }
    let found = aStartContinuation == aFrame;
    while let Some(f) = aFrame {
        NS_ASSERTION!(f.IsTextFrame(), "Bad frame");
        if !f.RemoveTextRun(aTextRun) {
            break;
        }
        aFrame = f.GetNextContinuation().map(nsTextFrame::from_frame);
    }

    MOZ_ASSERT!(!found || aStartContinuation.is_some(), "how did we find null?");
    found
}

/**
 * Kill all references to |aTextRun| starting at |aStartContinuation|.
 * It could be referenced by any of its owners, and all their in-flows.
 * If |aStartContinuation| is null then process all userdata frames
 * and their continuations.
 * @note the caller is expected to take care of possibly destroying the
 * text run if all userdata frames were reset (userdata is deallocated
 * by this function though). The caller can detect this has occured by
 * checking |aTextRun->GetUserData() == nullptr|.
 */
// Gecko: layout/generic/nsTextFrame.cpp UnhookTextRunFromFrames
fn UnhookTextRunFromFrames(aTextRun: &RefPtr_gfxTextRun, aStartContinuation: Option<nsTextFrame>) {
    let mut aStartContinuation = aStartContinuation;
    if !HasTextRunUserData(aTextRun) {
        return;
    }

    if aTextRun.GetFlags2().0 & nsTextFrameUtils_Flags::IsSimpleFlow.0 != 0 {
        let userDataFrame = GetFrameForSimpleFlow(aTextRun);
        let whichTextRunState = if userDataFrame.GetTextRun(nsTextFrame_TextRunType::eInflated).map_or(false, |r| Rc::ptr_eq(&r, aTextRun)) { TEXT_IN_TEXTRUN_USER_DATA } else { TEXT_IN_UNINFLATED_TEXTRUN_USER_DATA };
        let found = ClearAllTextRunReferences(userDataFrame, aTextRun, aStartContinuation, whichTextRunState);
        NS_ASSERTION!(aStartContinuation.is_none() || found, "aStartContinuation wasn't found in simple flow text run");
        if !userDataFrame.HasAnyStateBits(whichTextRunState) {
            DestroyTextRunUserData(aTextRun);
        }
    } else {
        let userMappedFlows = GetMappedFlows(aTextRun);
        let mut destroyFromIndex: i32 = if aStartContinuation.is_some() { -1 } else { 0 };
        for i in 0..userMappedFlows.len() {
            let userDataFrame = userMappedFlows[i].mStartFrame;
            let whichTextRunState = if userDataFrame.GetTextRun(nsTextFrame_TextRunType::eInflated).map_or(false, |r| Rc::ptr_eq(&r, aTextRun)) { TEXT_IN_TEXTRUN_USER_DATA } else { TEXT_IN_UNINFLATED_TEXTRUN_USER_DATA };
            let found = ClearAllTextRunReferences(userDataFrame, aTextRun, aStartContinuation, whichTextRunState);
            if found {
                if userDataFrame.HasAnyStateBits(whichTextRunState) {
                    destroyFromIndex = i as i32 + 1;
                } else {
                    destroyFromIndex = i as i32;
                }
                aStartContinuation = None;
            }
        }
        NS_ASSERTION!(destroyFromIndex >= 0, "aStartContinuation wasn't found in multi flow text run");
        if destroyFromIndex == 0 {
            DestroyTextRunUserData(aTextRun);
        } else {
            if let gfxTextRun_UserData::Flows(userData) = aTextRun.borrow_mut().GetUserData_mut() {
                // (the array stays exact-size: rebuilt at the new length)
                let mut flows = std::mem::take(&mut userData.mMappedFlows).into_vec();
                flows.truncate(destroyFromIndex as usize);
                userData.mMappedFlows = flows.into_boxed_slice();
                if userData.mLastFlowIndex >= destroyFromIndex as u32 {
                    userData.mLastFlowIndex = destroyFromIndex as u32 - 1;
                }
            }
        }
    }
}

// Gecko: layout/generic/nsTextFrame.cpp InvalidateFrameDueToGlyphsChanged, GlyphObserver
// NOT PORTED: glyph observers (fonts whose glyphs change, SVG fonts): CreateObserversForAnimatedGlyphs
// and ClearObserversFromTextRun do nothing (nsTextFrame.cpp lines 438-448, 753-793, 1015-1096 at GECKO_REV).

// ---------------------------------------------------------------------------------------------
// Characters and whitespace
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetContentEnd
impl nsTextFrame {
    pub fn GetContentEnd(&self) -> i32 {
        let next = self.GetNextContinuation().map(nsTextFrame::from_frame);
        // In case of allocation failure when setting/modifying the text buffer,
        // it's possible our text might be missing. So we check the buffer length,
        // in addition to the offset of the next continuation (if any).
        let bufferLen = self.CharacterDataBuffer().GetLength() as i32;
        match next {
            Some(next) => std::cmp::min(bufferLen, next.GetContentOffset()),
            None => bufferLen,
        }
    }
}

// Gecko: layout/generic/nsTextFrame.cpp FlowLengthProperty
#[derive(Clone, Copy, Debug)]
pub struct FlowLengthProperty {
    pub mStartOffset: i32,
    // The offset of the next fixed continuation after mStartOffset, or
    // of the end of the text if there is none
    pub mEndFlowOffset: i32,
}

impl nsTextFrame {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetInFlowContentLength
    pub fn GetInFlowContentLength(&self) -> i32 {
        let mContent = self.GetContent().unwrap();
        if !self.HasAnyStateBits(NS_FRAME_IS_BIDI) {
            return mContent.TextLength() as i32 - self.GetContentOffset();
        }

        let mut flowLength = mContent.GetFlowLengthProperty();
        /**
         * This frame must start inside the cached flow. If the flow starts at
         * mContentOffset but this frame is empty, logically it might be before the
         * start of the cached flow.
         */
        if let Some(fl) = flowLength {
            if (fl.mStartOffset < self.GetContentOffset() || (fl.mStartOffset == self.GetContentOffset() && self.GetContentEnd() > self.GetContentOffset())) && fl.mEndFlowOffset > self.GetContentOffset() {
                NS_ASSERTION!(fl.mEndFlowOffset >= self.GetContentEnd(), "frame crosses fixed continuation boundary");
                return fl.mEndFlowOffset - self.GetContentOffset();
            }
        }

        let nextBidi = self.LastInFlow().GetNextContinuation().map(nsTextFrame);
        let endFlow = match nextBidi {
            Some(n) => n.GetContentOffset(),
            None => mContent.TextLength() as i32,
        };

        let fl = FlowLengthProperty { mStartOffset: self.GetContentOffset(), mEndFlowOffset: endFlow };
        flowLength = Some(fl);
        mContent.SetFlowLengthProperty(flowLength.unwrap());

        endFlow - self.GetContentOffset()
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::AdjustOffsetsForBidi
    pub fn AdjustOffsetsForBidi(&self, mut aStart: i32, mut aEnd: i32) {
        self.AddStateBits(NS_FRAME_IS_BIDI);
        let mContent = self.GetContent().unwrap();
        if mContent.HasFlowLengthProperty() {
            mContent.RemoveFlowLengthProperty();
        }

        /*
         * After Bidi resolution we may need to reassign text runs.
         * This is called during bidi resolution from the block container, so we
         * shouldn't be holding a local reference to a textrun anywhere.
         */
        self.ClearTextRuns();

        let prev = self.GetPrevContinuation().map(nsTextFrame);
        if let Some(prev) = prev {
            // the bidi resolver can be very evil when columns/pages are involved. Don't
            // let it violate our invariants.
            let prevOffset = prev.GetContentOffset();
            aStart = std::cmp::max(aStart, prevOffset);
            aEnd = std::cmp::max(aEnd, prevOffset);
            prev.ClearTextRuns();
        }

        self.set_mContentOffset(aStart);
        self.SetLength(aEnd - aStart, None, 0);
    }
}

// Smarter versions of dom::IsSpaceCharacter.
// Unicode is really annoying; sometimes a space character isn't whitespace ---
// when it combines with another character
// So we have several versions of IsSpace for use in different contexts.

// Gecko: layout/generic/nsTextFrame.cpp IsSpaceCombiningSequenceTail(const CharacterDataBuffer&, uint32_t)
fn IsSpaceCombiningSequenceTail__CharacterDataBuffer_uint32_t(aBuffer: &CharacterDataBuffer, aPos: u32) -> bool {
    NS_ASSERTION!(aPos <= aBuffer.GetLength(), "Bad offset");
    if !aBuffer.Is2b() {
        return false;
    }
    nsTextFrameUtils::IsSpaceCombiningSequenceTail__char16_t_ptr_int32_t(&aBuffer.Get2b()[aPos as usize..], (aBuffer.GetLength() - aPos) as i32)
}

// Check whether aPos is a space for CSS 'word-spacing' purposes
// Gecko: layout/generic/nsTextFrame.cpp IsCSSWordSpacingSpace
fn IsCSSWordSpacingSpace(aBuffer: &CharacterDataBuffer, aPos: u32, aFrame: nsTextFrame, aStyleText: &nsStyleTextAdapter) -> bool {
    NS_ASSERTION!(aPos < aBuffer.GetLength(), "No text for IsSpace!");

    let ch = aBuffer.CharAt(aPos);
    match ch as u32 {
        0x20 | CH_NBSP => !IsSpaceCombiningSequenceTail__CharacterDataBuffer_uint32_t(aBuffer, aPos + 1),
        0x0D | 0x09 => !aStyleText.WhiteSpaceIsSignificant(),
        0x0A => !aStyleText.NewlineIsSignificant(aFrame),
        _ => false,
    }
}

// Gecko: intl/components/src/UnicodeProperties.h intl::UnicodeProperties::IsCursiveScript
// ADAPTER (TEMP): UnicodeProperties.rs has no script lookup. The cursive scripts of Gecko's list
// (Arabic, Mongolian, Syriac, N'Ko, Phags-pa, Mandaic, Manichaean, Psalter Pahlavi, Hanifi Rohingya,
// Adlam, Old Uyghur, Sogdian), by their main blocks.
fn IsCursiveScript(aCh: u32) -> bool {
    matches!(aCh,
        0x0600..=0x06FF | 0x0700..=0x074F | 0x0750..=0x077F | 0x07C0..=0x07FF | 0x0840..=0x085F | 0x0860..=0x086F | 0x0870..=0x08FF |
        0x1800..=0x18AF | 0xA840..=0xA87F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF | 0x10AC0..=0x10AFF | 0x10B80..=0x10BAF | 0x10D00..=0x10D3F |
        0x10F30..=0x10F6F | 0x10F70..=0x10FAF | 0x1E900..=0x1E95F)
}

// Gecko: layout/generic/nsTextFrame.cpp kOghamSpaceMark
const kOghamSpaceMark: u16 = 0x1680;

// Check whether the string aChars/aLength starts with space that's
// trimmable according to CSS 'white-space:normal/nowrap'.
// Gecko: layout/generic/nsTextFrame.cpp IsTrimmableSpace(const char16_t*, uint32_t)
fn IsTrimmableSpace__char16_t_ptr_uint32_t(aChars: &[u16], aLength: u32) -> bool {
    NS_ASSERTION!(aLength > 0, "No text for IsSpace!");

    let ch = aChars[0];
    if ch == 0x20 || ch == kOghamSpaceMark {
        return !nsTextFrameUtils::IsSpaceCombiningSequenceTail__char16_t_ptr_int32_t(&aChars[1..], aLength as i32 - 1);
    }
    ch == 0x09 || ch == 0x0C || ch == 0x0A || ch == 0x0D
}

// Check whether the character aCh is trimmable according to CSS
// 'white-space:normal/nowrap'
// Gecko: layout/generic/nsTextFrame.cpp IsTrimmableSpace(char)
fn IsTrimmableSpace__char(aCh: u8) -> bool {
    aCh == b' ' || aCh == b'\t' || aCh == 0x0C || aCh == b'\n' || aCh == b'\r'
}

// Gecko: layout/generic/nsTextFrame.cpp IsTrimmableSpace(const CharacterDataBuffer&, uint32_t, const nsStyleText*, bool)
fn IsTrimmableSpace(aBuffer: &CharacterDataBuffer, aPos: u32, aStyleText: &nsStyleTextAdapter, aAllowHangingWS: bool) -> bool {
    NS_ASSERTION!(aPos < aBuffer.GetLength(), "No text for IsSpace!");

    match aBuffer.CharAt(aPos) {
        0x20 | kOghamSpaceMark => (!aStyleText.WhiteSpaceIsSignificant() || aAllowHangingWS) && !IsSpaceCombiningSequenceTail__CharacterDataBuffer_uint32_t(aBuffer, aPos + 1),
        0x0A => !aStyleText.NewlineIsSignificantStyle() && !aStyleText.WhiteSpaceCollapseIsPreserveSpaces(),
        0x09 | 0x0D | 0x0C => !aStyleText.WhiteSpaceIsSignificant() || aAllowHangingWS,
        _ => false,
    }
}

// Gecko: layout/generic/nsTextFrame.cpp IsSelectionInlineWhitespace, IsSelectionNewline
// NOT PORTED: selection (nsTextFrame.cpp lines 943-959 at GECKO_REV).

// Count the amount of trimmable whitespace (as per CSS
// 'white-space:normal/nowrap') in a character buffer. The first
// character is at offset aStartOffset; the maximum number of characters
// to check is aLength. aDirection is -1 or 1 depending on whether we should
// progress backwards or forwards.
// Gecko: layout/generic/nsTextFrame.cpp GetTrimmableWhitespaceCount
fn GetTrimmableWhitespaceCount(aBuffer: &CharacterDataBuffer, aStartOffset: i32, aLength: i32, aDirection: i32) -> u32 {
    if aLength == 0 {
        return 0;
    }

    let mut count: i32 = 0;
    if aBuffer.Is2b() {
        // Rust: `str` is the position in the buffer instead of a pointer.
        let mut str: i32 = aStartOffset;
        let mut bufferLen: i32 = aBuffer.GetLength() as i32 - aStartOffset;
        while count < aLength {
            if !IsTrimmableSpace__char16_t_ptr_uint32_t(&aBuffer.Get2b()[str as usize..], bufferLen as u32) {
                break;
            }
            str += aDirection;
            bufferLen -= aDirection;
            count += 1;
        }
    } else {
        let mut str: i32 = aStartOffset;
        while count < aLength {
            if !IsTrimmableSpace__char(aBuffer.Get1b()[str as usize]) {
                break;
            }
            str += aDirection;
            count += 1;
        }
    }
    count as u32
}

// Gecko: layout/generic/nsTextFrame.cpp IsAllWhitespace
fn IsAllWhitespace(aBuffer: &CharacterDataBuffer, aAllowNewline: bool) -> bool {
    if aBuffer.Is2b() {
        return false;
    }
    let len = aBuffer.GetLength() as i32;
    let str = aBuffer.Get1b();
    for i in 0..len {
        let ch = str[i as usize];
        if ch == b' ' || ch == b'\t' || ch == b'\r' || (ch == b'\n' && aAllowNewline) {
            continue;
        }
        return false;
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Style methods that nsTextFrame uses and nsStyleTextAdapter does not have yet
// ---------------------------------------------------------------------------------------------

// ADAPTER: nsStyleText members from layout/style/nsStyleStruct.h and nsStyleStructInlines.h, under
// Gecko's names, on the wrapper nsLineLayoutAdapter.rs made. Move them into it (or into the real
// nsStyleText) when convenient.
// NOT PORTED: `line-break`, `hyphens`, `tab-size`, `hyphenate-character` and `hyphenate-limit-chars`
// are not in the servo configuration of Stylo: the computed values are the initial ones (line-break
// auto, hyphens manual, tab-size 8).
pub trait nsStyleText_nsTextFrame {
    fn mWhiteSpaceCollapse(&self) -> mrt_style::style::computed_values::white_space_collapse::T;
    fn WhiteSpaceCollapseIsPreserveSpaces(&self) -> bool;
    fn NewlineIsSignificantStyle(&self) -> bool;
    fn NewlineIsSignificant(&self, aContextFrame: nsTextFrame) -> bool;
    fn WhiteSpaceOrNewlineIsSignificant(&self) -> bool;
    fn TabIsSignificant(&self) -> bool;
    fn WhiteSpaceCanHangOrVisuallyCollapse(&self) -> bool;
    fn WordCanWrapStyle(&self) -> bool;
    fn WordCanWrap(&self, aContextFrame: nsIFrame) -> bool;
    fn mHyphens(&self) -> StyleHyphens;
    fn mTextAlignIsJustify(&self) -> bool;
    fn mTextAlignLastIsJustify(&self) -> bool;
    fn mTextTransformIsNone(&self) -> bool;
    fn mLetterSpacingResolved(&self, aFontSize: nscoord) -> nscoord;
    fn mWordSpacingResolved(&self, aFontSize: nscoord) -> nscoord;
    fn mLetterSpacingIsDefinitelyZero(&self) -> bool;
    fn mWordSpacingIsDefinitelyZero(&self) -> bool;
}

impl nsStyleText_nsTextFrame for nsStyleTextAdapter {
    fn mWhiteSpaceCollapse(&self) -> mrt_style::style::computed_values::white_space_collapse::T {
        self.Style().get_inherited_text().white_space_collapse
    }

    // mWhiteSpaceCollapse == StyleWhiteSpaceCollapse::PreserveSpaces
    // NOT PORTED: `white-space-collapse: preserve-spaces` is only in the gecko configuration of Stylo.
    fn WhiteSpaceCollapseIsPreserveSpaces(&self) -> bool {
        false
    }

    // Gecko: nsStyleText::NewlineIsSignificantStyle
    fn NewlineIsSignificantStyle(&self) -> bool {
        use mrt_style::style::computed_values::white_space_collapse::T as C;
        let c = self.mWhiteSpaceCollapse();
        c == C::Preserve || c == C::PreserveBreaks || c == C::BreakSpaces
    }

    // Gecko: layout/style/nsStyleStructInlines.h nsStyleText::NewlineIsSignificant
    fn NewlineIsSignificant(&self, aContextFrame: nsTextFrame) -> bool {
        self.NewlineIsSignificantStyle() && !aContextFrame.ShouldSuppressLineBreak() && !aContextFrame.Style().IsTextCombined()
    }

    // Gecko: nsStyleText::WhiteSpaceOrNewlineIsSignificant
    fn WhiteSpaceOrNewlineIsSignificant(&self) -> bool {
        self.NewlineIsSignificantStyle() || self.WhiteSpaceIsSignificant()
    }

    // Gecko: nsStyleText::TabIsSignificant
    fn TabIsSignificant(&self) -> bool {
        use mrt_style::style::computed_values::white_space_collapse::T as C;
        let c = self.mWhiteSpaceCollapse();
        c == C::Preserve || c == C::BreakSpaces
    }

    // Gecko: nsStyleText::WhiteSpaceCanHangOrVisuallyCollapse
    fn WhiteSpaceCanHangOrVisuallyCollapse(&self) -> bool {
        use mrt_style::style::computed_values::white_space_collapse::T as C;
        // This was originally expressed in nsTextFrame in terms of:
        //   mWhiteSpace != StyleWhiteSpace::BreakSpaces &&
        //       WhiteSpaceCanWrapStyle() &&
        //       WhiteSpaceIsSignificant()
        // which simplifies to:
        self.WhiteSpaceCanWrapStyle() && self.mWhiteSpaceCollapse() != C::BreakSpaces
    }

    // Gecko: nsStyleText::WordCanWrapStyle (with EffectiveOverflowWrap; `word-break: break-word` only
    // exists in the gecko configuration of Stylo)
    fn WordCanWrapStyle(&self) -> bool {
        use mrt_style::style::values::specified::text::OverflowWrap;
        if !self.WhiteSpaceCanWrapStyle() {
            return false;
        }
        let owrap = self.Style().get_inherited_text().overflow_wrap;
        owrap == OverflowWrap::BreakWord || owrap == OverflowWrap::Anywhere
    }

    // Gecko: layout/style/nsStyleStructInlines.h nsStyleText::WordCanWrap
    fn WordCanWrap(&self, aContextFrame: nsIFrame) -> bool {
        self.WordCanWrapStyle() && !aContextFrame.IsInSVGTextSubtree()
    }

    fn mHyphens(&self) -> StyleHyphens {
        StyleHyphens::Manual
    }

    fn mTextAlignIsJustify(&self) -> bool {
        self.mTextAlign() == StyleTextAlign::Justify
    }

    fn mTextAlignLastIsJustify(&self) -> bool {
        use mrt_style::style::values::computed::text::TextAlignLast as L;
        self.Style().get_inherited_text().text_align_last == L::Justify
    }

    fn mTextTransformIsNone(&self) -> bool {
        self.Style().get_inherited_text().text_transform.is_none()
    }

    // mLetterSpacing.Resolve(fontSize): a percentage is of the font size.
    fn mLetterSpacingResolved(&self, aFontSize: nscoord) -> nscoord {
        StyleCSSPixelLength_ToAppUnits(self.Style().get_inherited_text().letter_spacing.0.resolve(ComputedLength::new(aFontSize as f32 / AppUnitsPerCSSPixel() as f32)))
    }

    fn mWordSpacingResolved(&self, aFontSize: nscoord) -> nscoord {
        StyleCSSPixelLength_ToAppUnits(self.Style().get_inherited_text().word_spacing.resolve(ComputedLength::new(aFontSize as f32 / AppUnitsPerCSSPixel() as f32)))
    }

    fn mLetterSpacingIsDefinitelyZero(&self) -> bool {
        mrt_style::style::Zero::is_zero(&self.Style().get_inherited_text().letter_spacing.0)
    }

    fn mWordSpacingIsDefinitelyZero(&self) -> bool {
        mrt_style::style::Zero::is_zero(&self.Style().get_inherited_text().word_spacing)
    }
}

impl nsStyleTextAdapter {
    // ADAPTER: the computed style the wrapper reads.
    pub fn Style(&self) -> mrt_style::style::servo_arc::Arc<mrt_style::style::properties::ComputedValues> {
        self.mStyle.clone()
    }
}

// Gecko: layout/style/ComputedStyle.h ComputedStyle::IsTextCombined
// NOT PORTED: text-combine-upright; never.
pub trait ComputedStyle_nsTextFrame {
    fn IsTextCombined(&self) -> bool;
}

impl ComputedStyle_nsTextFrame for mrt_style::style::servo_arc::Arc<mrt_style::style::properties::ComputedValues> {
    fn IsTextCombined(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------------------------
// BuildTextRunsScanner
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsTextFrame.h nsTextFrame::TextRunType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsTextFrame_TextRunType {
    // Anything in reflow (but not intrinsic width calculation) or
    // painting should use the inflated text run (i.e., with font size
    // inflation applied).
    eInflated,
    // Intrinsic width calculation should use the non-inflated text run.
    // When there is font size inflation, it will be different.
    eNotInflated,
}

// Like TextRunMappedFlow but with some differences. mStartFrame to mEndFrame
// (exclusive) are a sequence of in-flow frames (if mEndFrame is null, then
// continuations starting from mStartFrame are a sequence of in-flow frames).
// Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::MappedFlow
#[derive(Clone, Copy, Debug)]
pub struct BuildTextRunsScanner_MappedFlow {
    pub mStartFrame: nsTextFrame,
    pub mEndFrame: Option<nsTextFrame>,
    // When we consider breaking between elements, the nearest common
    // ancestor of the elements containing the characters is the one whose
    // CSS 'white-space' property governs. So this records the nearest common
    // ancestor of mStartFrame and the previous text frame, or null if there
    // was no previous text frame on this line.
    pub mAncestorControllingInitialBreak: Option<nsIFrame>,
}

impl BuildTextRunsScanner_MappedFlow {
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::MappedFlow::GetContentEnd
    pub fn GetContentEnd(&self) -> i32 {
        let bufferLen = self.mStartFrame.CharacterDataBuffer().GetLength() as i32;
        match self.mEndFrame {
            Some(e) => std::cmp::min(bufferLen, e.GetContentOffset()),
            None => bufferLen,
        }
    }
}

// Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::BreakSink
pub struct BuildTextRunsScanner_BreakSink {
    pub mTextRun: RefPtr_gfxTextRun,
    // DrawTarget* mDrawTarget: NOT PORTED, no drawing
    pub mOffsetIntoTextRun: u32,
}

impl BuildTextRunsScanner_BreakSink {
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::BreakSink::BreakSink
    pub fn new(aTextRun: RefPtr_gfxTextRun, aOffsetIntoTextRun: u32) -> BuildTextRunsScanner_BreakSink {
        BuildTextRunsScanner_BreakSink { mTextRun: aTextRun, mOffsetIntoTextRun: aOffsetIntoTextRun }
    }

    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::BreakSink::Finish
    // NOT PORTED: transformed text runs (nsTransformedTextRun::FinishSettingProperties).
    pub fn Finish(&self) {
        if self.mTextRun.GetFlags2().0 & nsTextFrameUtils_Flags::IsTransformed.0 != 0 {
            unimplemented!("NOT PORTED: text-transform (nsTransformedTextRun::FinishSettingProperties)")
        }
    }
}

impl nsILineBreakSink for BuildTextRunsScanner_BreakSink {
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::BreakSink::SetBreaks
    fn SetBreaks(&mut self, aOffset: u32, aLength: u32, aBreakBefore: &mut [u8]) {
        let range = gfxTextRun_Range::new(aOffset + self.mOffsetIntoTextRun, aOffset + self.mOffsetIntoTextRun + aLength);
        if self.mTextRun.SetPotentialLineBreaks(range, &aBreakBefore[..]) {
            // Be conservative and assume that some breaks have been set
            self.mTextRun.ClearFlagBits(nsTextFrameUtils_Flags::NoBreaks);
        }
    }

    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::BreakSink::SetCapitalization
    // ADAPTER: every run takes the marks (gfxTextRun::SetCapitalization), as the port makes no
    // transformed runs; Gecko's are transformed whenever text-transform is set.
    fn SetCapitalization(&mut self, aOffset: u32, aLength: u32, aCapitalize: &mut [bool]) {
        self.mTextRun.borrow_mut().SetCapitalization(aOffset + self.mOffsetIntoTextRun, aLength, aCapitalize);
    }
}

// The buffer BuildTextRunForFrames writes the transformed text to: the C++ void* of
// mMaxTextLength * (mDoubleByteText ? 2 : 1) bytes.
pub enum TextRunBuffer {
    Narrow(Vec<u8>),
    Wide(Vec<u16>),
}

/**
 * This class accumulates state as we scan a paragraph of text. It detects
 * textrun boundaries (changes from text to non-text, hard
 * line breaks, and font changes) and builds a gfxTextRun at each boundary.
 * It also detects linebreaker run boundaries (changes from text to non-text,
 * and hard line breaks) and at each boundary runs the linebreaker to compute
 * potential line breaks. It also records actual line breaks to store them in
 * the textruns.
 */
// Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner
pub struct BuildTextRunsScanner {
    mMappedFlows: Vec<BuildTextRunsScanner_MappedFlow>,
    mLineBreakBeforeFrames: Vec<nsTextFrame>,
    mBreakSinks: Vec<Box<BuildTextRunsScanner_BreakSink>>,
    mCreatedTextRuns: Vec<RefPtr_gfxTextRun>,
    mLineBreaker: nsLineBreaker,
    mCurrentFramesAllSameTextRun: Option<RefPtr_gfxTextRun>,
    // DrawTarget* mDrawTarget: NOT PORTED, no drawing
    mLineContainer: nsIFrame,
    mLastFrame: Option<nsTextFrame>,
    // The common ancestor of the current frame and the previous leaf frame
    // on the line, or null if there was no previous leaf frame.
    mCommonAncestorWithLastFrame: Option<nsIFrame>,
    // gfxMissingFontRecorder* mMissingFonts: NOT PORTED, no web fonts
    // mMaxTextLength is an upper bound on the size of the text in all mapped
    // frames The value UINT32_MAX represents overflow; text will be discarded
    mMaxTextLength: u32,
    mDoubleByteText: bool,
    mBidiEnabled: bool,
    mStartOfLine: bool,
    mSkipIncompleteTextRuns: bool,
    mCanStopOnThisLine: bool,
    mDoLineBreaking: bool,
    mWhichTextRun: nsTextFrame_TextRunType,
    mNextRunContextInfo: u8,
    mCurrentRunContextInfo: u8,
}

// Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::FindBoundaryState
pub struct BuildTextRunsScanner_FindBoundaryState<'a> {
    pub mStopAtFrame: Option<nsIFrame>,
    pub mFirstTextFrame: Option<nsTextFrame>,
    pub mLastTextFrame: Option<nsTextFrame>,
    pub mSeenTextRunBoundaryOnLaterLine: bool,
    pub mSeenTextRunBoundaryOnThisLine: bool,
    pub mSeenSpaceForLineBreakingOnThisLine: bool,
    pub mBuffer: &'a mut Vec<u16>,
}

// Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::FindBoundaryResult
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildTextRunsScanner_FindBoundaryResult {
    FB_CONTINUE,
    FB_STOPPED_AT_STOP_FRAME,
    FB_FOUND_VALID_TEXTRUN_BOUNDARY,
}
use BuildTextRunsScanner_FindBoundaryResult::*;

impl BuildTextRunsScanner {
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::BuildTextRunsScanner
    pub fn new(aPresContext: &nsPresContext, aLineContainer: nsIFrame, aWhichTextRun: nsTextFrame_TextRunType, aDoLineBreaking: bool) -> BuildTextRunsScanner {
        let mut this = BuildTextRunsScanner {
            mMappedFlows: Vec::new(),
            mLineBreakBeforeFrames: Vec::new(),
            mBreakSinks: Vec::new(),
            mCreatedTextRuns: Vec::new(),
            mLineBreaker: nsLineBreaker::default(),
            mCurrentFramesAllSameTextRun: None,
            mLineContainer: aLineContainer,
            mLastFrame: None,
            mCommonAncestorWithLastFrame: None,
            mMaxTextLength: 0,
            mDoubleByteText: false,
            mBidiEnabled: aPresContext.BidiEnabled(),
            mStartOfLine: true,
            mSkipIncompleteTextRuns: false,
            mCanStopOnThisLine: false,
            mDoLineBreaking: aDoLineBreaking,
            mWhichTextRun: aWhichTextRun,
            mNextRunContextInfo: nsTextFrameUtils::INCOMING_NONE,
            mCurrentRunContextInfo: nsTextFrameUtils::INCOMING_NONE,
        };
        this.ResetRunInfo();
        this
    }

    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::SetAtStartOfLine
    pub fn SetAtStartOfLine(&mut self) {
        self.mStartOfLine = true;
        self.mCanStopOnThisLine = false;
    }
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::SetSkipIncompleteTextRuns
    pub fn SetSkipIncompleteTextRuns(&mut self, aSkip: bool) {
        self.mSkipIncompleteTextRuns = aSkip;
    }
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::SetCommonAncestorWithLastFrame
    pub fn SetCommonAncestorWithLastFrame(&mut self, aFrame: Option<nsIFrame>) {
        self.mCommonAncestorWithLastFrame = aFrame;
    }
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::CanStopOnThisLine
    pub fn CanStopOnThisLine(&self) -> bool {
        self.mCanStopOnThisLine
    }
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::GetCommonAncestorWithLastFrame
    pub fn GetCommonAncestorWithLastFrame(&self) -> Option<nsIFrame> {
        self.mCommonAncestorWithLastFrame
    }
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::LiftCommonAncestorWithLastFrameToParent
    pub fn LiftCommonAncestorWithLastFrameToParent(&mut self, aFrame: Option<nsIFrame>) {
        if let Some(c) = self.mCommonAncestorWithLastFrame {
            if c.GetParent() == aFrame && aFrame.is_some() {
                self.mCommonAncestorWithLastFrame = aFrame;
            }
        }
    }
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::ResetRunInfo
    pub fn ResetRunInfo(&mut self) {
        self.mLastFrame = None;
        self.mMappedFlows.clear();
        self.mLineBreakBeforeFrames.clear();
        self.mMaxTextLength = 0;
        self.mDoubleByteText = false;
    }
}

impl Drop for BuildTextRunsScanner {
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::~BuildTextRunsScanner
    fn drop(&mut self) {
        // for (auto& run : mCreatedTextRuns) { CreateObserversForAnimatedGlyphs(run); }
        // NOT PORTED: glyph observers.
        if !std::thread::panicking() {
            NS_ASSERTION!(self.mBreakSinks.is_empty(), "Should have been cleared");
            NS_ASSERTION!(self.mLineBreakBeforeFrames.is_empty(), "Should have been cleared");
            NS_ASSERTION!(self.mMappedFlows.is_empty(), "Should have been cleared");
        }
    }
}

// Gecko: layout/generic/nsTextFrame.cpp FindLineContainer(const nsIFrame*) (and the non-const overload)
fn FindLineContainer(aFrame: Option<nsIFrame>) -> Option<nsIFrame> {
    let mut aFrame = aFrame;
    while let Some(f) = aFrame {
        if !(f.IsLineParticipant() || f.CanContinueTextRun()) {
            break;
        }
        aFrame = f.GetParent();
    }
    aFrame
}

// Gecko: layout/generic/nsTextFrame.cpp IsLineBreakingWhiteSpace
fn IsLineBreakingWhiteSpace(aChar: u16) -> bool {
    // 0x0A (\n) is not handled as white-space by the line breaker, since
    // we break before it, if it isn't transformed to a normal space.
    // (If we treat it as normal white-space then we'd only break after it.)
    // However, it does induce a line break or is converted to a regular
    // space, and either way it can be used to bound the region of text
    // that needs to be analyzed for line breaking.
    nsLineBreaker::IsSpace(aChar) || aChar == 0x0A
}

// Rust: the C++ takes a void* and a flag for the character size; this is generic over the character
// type like TransformText.
// Gecko: layout/generic/nsTextFrame.cpp TextContainsLineBreakerWhiteSpace
fn TextContainsLineBreakerWhiteSpace<CharT: TextFrameChar>(aText: &[CharT], aLength: u32) -> bool {
    for i in 0..aLength {
        if IsLineBreakingWhiteSpace(aText[i as usize].to_u32() as u16) {
            return true;
        }
    }
    false
}

// Gecko: layout/generic/nsTextFrame.cpp GetCSSWhitespaceToCompressionMode
fn GetCSSWhitespaceToCompressionMode(aFrame: nsTextFrame, aStyleText: &nsStyleTextAdapter) -> nsTextFrameUtils_CompressionMode {
    use mrt_style::style::computed_values::white_space_collapse::T as C;
    match aStyleText.mWhiteSpaceCollapse() {
        C::Collapse => nsTextFrameUtils_CompressionMode::COMPRESS_WHITESPACE_NEWLINE,
        C::PreserveBreaks => nsTextFrameUtils_CompressionMode::COMPRESS_WHITESPACE,
        C::Preserve | C::BreakSpaces => {
            if !aStyleText.NewlineIsSignificant(aFrame) {
                // If newline is set to be preserved, but then suppressed,
                // transform newline to space.
                return nsTextFrameUtils_CompressionMode::COMPRESS_NONE_TRANSFORM_TO_SPACE;
            }
            nsTextFrameUtils_CompressionMode::COMPRESS_NONE
        }
    }
}

// Gecko: layout/generic/nsTextFrame.cpp FrameTextTraversal
#[derive(Clone, Copy, Default)]
pub struct FrameTextTraversal {
    // These fields identify which frames should be recursively scanned
    // The first normal frame to scan (or null, if no such frame should be
    // scanned)
    pub mFrameToScan: Option<nsIFrame>,
    // The first overflow frame to scan (or null, if no such frame should be
    // scanned)
    pub mOverflowFrameToScan: Option<nsIFrame>,
    // Whether to scan the siblings of
    // mFrameToDescendInto/mOverflowFrameToDescendInto
    pub mScanSiblings: bool,

    // These identify the boundaries of the context required for
    // line breaking or textrun construction
    pub mLineBreakerCanCrossFrameBoundary: bool,
    pub mTextRunCanCrossFrameBoundary: bool,
}

impl FrameTextTraversal {
    // Gecko: layout/generic/nsTextFrame.cpp FrameTextTraversal::NextFrameToScan
    pub fn NextFrameToScan(&mut self) -> Option<nsIFrame> {
        let f;
        if let Some(scan) = self.mFrameToScan {
            f = Some(scan);
            self.mFrameToScan = if self.mScanSiblings { scan.GetNextSibling() } else { None };
        } else if let Some(scan) = self.mOverflowFrameToScan {
            f = Some(scan);
            self.mOverflowFrameToScan = if self.mScanSiblings { scan.GetNextSibling() } else { None };
        } else {
            f = None;
        }
        f
    }
}

// Gecko: layout/generic/nsTextFrame.cpp CanTextCrossFrameBoundary
fn CanTextCrossFrameBoundary(aFrame: nsIFrame) -> FrameTextTraversal {
    let mut result = FrameTextTraversal::default();

    let continuesTextRun = aFrame.CanContinueTextRun();
    if aFrame.IsPlaceholderFrame() {
        // placeholders are "invisible", so a text run should be able to span
        // across one. But don't descend into the out-of-flow.
        result.mLineBreakerCanCrossFrameBoundary = true;
        if continuesTextRun {
            // ... Except for first-letter floats, which are really in-flow
            // from the point of view of capitalization etc, so we'd better
            // descend into them. But we actually need to break the textrun for
            // first-letter floats since things look bad if, say, we try to make a
            // ligature across the float boundary.
            result.mFrameToScan = nsLayoutUtils::GetFloatFromPlaceholder(aFrame); // GetOutOfFlowFrame()
        } else {
            result.mTextRunCanCrossFrameBoundary = true;
        }
    } else if continuesTextRun {
        result.mFrameToScan = aFrame.PrincipalChildList_FirstChild();
        result.mOverflowFrameToScan = aFrame.GetChildList(FrameChildListID::Overflow).FirstChild();
        NS_WARNING_ASSERTION!(result.mOverflowFrameToScan.is_none(), "Scanning overflow inline frames is something we should avoid");
        result.mScanSiblings = true;
        result.mTextRunCanCrossFrameBoundary = true;
        result.mLineBreakerCanCrossFrameBoundary = true;
    } else {
        MOZ_ASSERT!(!aFrame.IsRubyTextContainerFrame(), "Shouldn't call this method for ruby text container");
    }
    result
}

impl BuildTextRunsScanner {
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::FindBoundaries
    pub fn FindBoundaries(&mut self, aFrame: nsIFrame, aState: &mut BuildTextRunsScanner_FindBoundaryState) -> BuildTextRunsScanner_FindBoundaryResult {
        let frameType = aFrame.Type();
        if frameType == LayoutFrameType::RubyTextContainer {
            // Don't stop a text run for ruby text container. We want ruby text
            // containers to be skipped, but continue the text run across them.
            return FB_CONTINUE;
        }

        let textFrame: Option<nsTextFrame> = if frameType == LayoutFrameType::Text { Some(nsTextFrame::from_frame(aFrame)) } else { None };
        if let Some(textFrame) = textFrame {
            if let Some(last) = aState.mLastTextFrame {
                if Some(textFrame.0) != last.GetNextInFlow() && !self.ContinueTextRunAcrossFrames(last, textFrame) {
                    aState.mSeenTextRunBoundaryOnThisLine = true;
                    if aState.mSeenSpaceForLineBreakingOnThisLine {
                        return FB_FOUND_VALID_TEXTRUN_BOUNDARY;
                    }
                }
            }
            if aState.mFirstTextFrame.is_none() {
                aState.mFirstTextFrame = Some(textFrame);
            }
            aState.mLastTextFrame = Some(textFrame);
        }

        if Some(aFrame) == aState.mStopAtFrame {
            return FB_STOPPED_AT_STOP_FRAME;
        }

        if let Some(textFrame) = textFrame {
            if aState.mSeenSpaceForLineBreakingOnThisLine {
                return FB_CONTINUE;
            }
            let characterDataBuffer = textFrame.CharacterDataBuffer();
            let start = textFrame.GetContentOffset() as u32;
            let mut length = textFrame.GetContentLength() as u32;
            let language: Option<nsAtom> = textFrame.StyleFont().mLanguage();
            let containsWhiteSpace;
            if characterDataBuffer.Is2b() {
                // It is possible that we may end up removing all whitespace in
                // a piece of text because of The White Space Processing Rules,
                // so we need to transform it before we can check existence of
                // such whitespaces.
                // aState->mBuffer.EnsureLengthAtLeast(length)
                if (aState.mBuffer.len() as u32) < length {
                    aState.mBuffer.resize(length as usize, 0);
                }
                let compression = GetCSSWhitespaceToCompressionMode(textFrame, &nsStyleTextAdapter::of(textFrame.0));
                let mut incomingFlags: u8 = 0;
                let mut skipChars = gfxSkipChars::new();
                let mut analysisFlags = nsTextFrameUtils_Flags::default();
                let written = nsTextFrameUtils::TransformText::<u16>(
                    &characterDataBuffer.Get2b()[start as usize..],
                    length,
                    &mut aState.mBuffer[..],
                    compression,
                    &mut incomingFlags,
                    &mut skipChars,
                    &mut analysisFlags,
                    language,
                );
                length = written as u32;
                containsWhiteSpace = TextContainsLineBreakerWhiteSpace::<u16>(&aState.mBuffer[..], length);
            } else {
                // If the text only contains ASCII characters, it is currently
                // impossible that TransformText would remove all whitespaces,
                // and thus the check below should return the same result for
                // transformed text and original text. So we don't need to try
                // transforming it here.
                containsWhiteSpace = TextContainsLineBreakerWhiteSpace::<u8>(&characterDataBuffer.Get1b()[start as usize..], length);
            }
            if containsWhiteSpace {
                aState.mSeenSpaceForLineBreakingOnThisLine = true;
                if aState.mSeenTextRunBoundaryOnLaterLine {
                    return FB_FOUND_VALID_TEXTRUN_BOUNDARY;
                }
            }
            return FB_CONTINUE;
        }

        let mut traversal = CanTextCrossFrameBoundary(aFrame);
        if !traversal.mTextRunCanCrossFrameBoundary {
            aState.mSeenTextRunBoundaryOnThisLine = true;
            if aState.mSeenSpaceForLineBreakingOnThisLine {
                return FB_FOUND_VALID_TEXTRUN_BOUNDARY;
            }
        }

        let mut f = traversal.NextFrameToScan();
        while let Some(fr) = f {
            let result = self.FindBoundaries(fr, aState);
            if result != FB_CONTINUE {
                return result;
            }
            f = traversal.NextFrameToScan();
        }

        if !traversal.mTextRunCanCrossFrameBoundary {
            aState.mSeenTextRunBoundaryOnThisLine = true;
            if aState.mSeenSpaceForLineBreakingOnThisLine {
                return FB_FOUND_VALID_TEXTRUN_BOUNDARY;
            }
        }

        FB_CONTINUE
    }
}

// build text runs for the 200 lines following aForFrame, and stop after that
// when we get a chance.
// Gecko: layout/generic/nsTextFrame.cpp NUM_LINES_TO_BUILD_TEXT_RUNS
const NUM_LINES_TO_BUILD_TEXT_RUNS: u32 = 200;

/**
 * General routine for building text runs. This is hairy because of the need
 * to build text runs that span content nodes.
 *
 * @param aContext The gfxContext we're using to construct this text run.
 * @param aForFrame The nsTextFrame for which we're building this text run.
 * @param aLineContainer the line container containing aForFrame; if null,
 *        we'll walk the ancestors to find it.  It's required to be non-null
 *        when aForFrameLine is non-null.
 * @param aForFrameLine the line containing aForFrame; if null, we'll figure
 *        out the line (slowly)
 * @param aWhichTextRun The type of text run we want to build. If font inflation
 *        is enabled, this will be eInflated, otherwise it's eNotInflated.
 */
// Gecko: layout/generic/nsTextFrame.cpp BuildTextRuns
// (the DrawTarget parameter is dropped: no drawing)
fn BuildTextRuns(aForFrame: nsTextFrame, aLineContainer: Option<nsIFrame>, aForFrameLine: Option<nsLineList_iterator>, aWhichTextRun: nsTextFrame_TextRunType) {
    let mut aLineContainer = aLineContainer;
    NS_ASSERTION!(aForFrameLine.is_none() || aLineContainer.is_some(), "line but no line container");

    let mut lineContainerChild: nsIFrame = aForFrame.0;
    if aLineContainer.is_none() {
        if aForFrame.IsFloatingFirstLetterChild() {
            lineContainerChild = aForFrame.GetParent().unwrap().GetPlaceholderFrame().unwrap();
        }
        aLineContainer = FindLineContainer(Some(lineContainerChild));
    } else {
        NS_ASSERTION!(aLineContainer == FindLineContainer(Some(aForFrame.0)) || (aLineContainer.unwrap().IsLetterFrame() && aLineContainer.unwrap().IsFloating()), "Wrong line container hint");
    }
    let aLineContainer = aLineContainer.unwrap();

    if aForFrame.HasAnyStateBits(TEXT_IS_IN_TOKEN_MATHML) {
        // NOT PORTED: MathML text (TEXT_IS_IN_TOKEN_MATHML, NS_FRAME_IS_IN_SINGLE_CHAR_MI are never set)
        aLineContainer.AddStateBits(TEXT_IS_IN_TOKEN_MATHML);
    }

    let presContext = aLineContainer.PresContext();
    let doLineBreaking = !aForFrame.IsInSVGTextSubtree();
    let mut scanner = BuildTextRunsScanner::new(&presContext, aLineContainer, aWhichTextRun, doLineBreaking);

    let block: Option<nsBlockFrame> = nsBlockFrame::do_QueryFrame(aLineContainer);

    let Some(block) = block else {
        let mut textRunContainer = aLineContainer;
        if aLineContainer.IsRubyTextContainerFrame() {
            // NOT PORTED: ruby
            unimplemented!("NOT PORTED: ruby text containers (BuildTextRuns)");
        } else {
            NS_ASSERTION!(aLineContainer.GetPrevInFlow().is_none() && aLineContainer.GetNextInFlow().is_none(), "Breakable non-block line containers other than ruby text container is not supported");
        }
        // Just loop through all the children of the linecontainer ... it's really
        // just one line
        scanner.SetAtStartOfLine();
        scanner.SetCommonAncestorWithLastFrame(None);
        let mut child = textRunContainer.PrincipalChildList_FirstChild();
        while let Some(c) = child {
            scanner.ScanFrame(c);
            child = c.GetNextSibling();
        }
        // Set mStartOfLine so FlushFrames knows its textrun ends a line
        scanner.SetAtStartOfLine();
        scanner.FlushFrames(true, false);
        return;
    };

    // Find the line containing 'lineContainerChild'.

    let mut isValid = true;
    let mut backIterator: nsBlockInFlowLineIterator;
    if let Some(forFrameLine) = aForFrameLine {
        backIterator = nsBlockInFlowLineIterator::new__nsBlockFrame_LineIterator(block, forFrameLine);
    } else {
        backIterator = nsBlockInFlowLineIterator::new__nsBlockFrame_nsIFrame_bool(block, lineContainerChild, &mut isValid);
        NS_ASSERTION!(isValid, "aForFrame not found in block, someone lied to us");
        NS_ASSERTION!(backIterator.GetContainer() == block, "Someone lied to us about the block");
    }
    let startLine = backIterator.GetLine();

    // Find a line where we can start building text runs. We choose the last line
    // where:
    // -- there is a textrun boundary between the start of the line and the
    // start of aForFrame
    // -- there is a space between the start of the line and the textrun boundary
    // (this is so we can be sure the line breaks will be set properly
    // on the textruns we construct).
    // The possibly-partial text runs up to and including the first space
    // are not reconstructed. We construct partial text runs for that text ---
    // for the sake of simplifying the code and feeding the linebreaker ---
    // but we discard them instead of assigning them to frames.
    // This is a little awkward because we traverse lines in the reverse direction
    // but we traverse the frames in each line in the forward direction.
    let mut forwardIterator = backIterator.clone();
    let mut stopAtFrame: Option<nsIFrame> = Some(lineContainerChild);
    let mut nextLineFirstTextFrame: Option<nsTextFrame> = None;
    let mut buffer: Vec<u16> = Vec::new();
    let mut seenTextRunBoundaryOnLaterLine = false;
    let mut mayBeginInTextRun = true;
    loop {
        forwardIterator = backIterator.clone();
        let line = backIterator.GetLine();
        if !backIterator.Prev() || backIterator.GetLine().IsBlock() {
            mayBeginInTextRun = false;
            break;
        }

        let mut state = BuildTextRunsScanner_FindBoundaryState {
            mStopAtFrame: stopAtFrame,
            mFirstTextFrame: None,
            mLastTextFrame: None,
            mSeenTextRunBoundaryOnLaterLine: seenTextRunBoundaryOnLaterLine,
            mSeenTextRunBoundaryOnThisLine: false,
            mSeenSpaceForLineBreakingOnThisLine: false,
            mBuffer: &mut buffer,
        };
        let mut foundBoundary = false;
        for child in line.ChildFrames() {
            let result = scanner.FindBoundaries(child, &mut state);
            if result == FB_FOUND_VALID_TEXTRUN_BOUNDARY {
                foundBoundary = true;
                break;
            } else if result == FB_STOPPED_AT_STOP_FRAME {
                break;
            }
        }
        if foundBoundary {
            break;
        }
        if stopAtFrame.is_none() && state.mLastTextFrame.is_some() && nextLineFirstTextFrame.is_some() && !scanner.ContinueTextRunAcrossFrames(state.mLastTextFrame.unwrap(), nextLineFirstTextFrame.unwrap()) {
            // Found a usable textrun boundary at the end of the line
            if state.mSeenSpaceForLineBreakingOnThisLine {
                break;
            }
            seenTextRunBoundaryOnLaterLine = true;
        } else if state.mSeenTextRunBoundaryOnThisLine {
            seenTextRunBoundaryOnLaterLine = true;
        }
        stopAtFrame = None;
        if state.mFirstTextFrame.is_some() {
            nextLineFirstTextFrame = state.mFirstTextFrame;
        }
    }
    scanner.SetSkipIncompleteTextRuns(mayBeginInTextRun);

    // Now iterate over all text frames starting from the current line.
    // First-in-flow text frames will be accumulated into textRunFrames as we go.
    // When a text run boundary is required we flush textRunFrames ((re)building
    // their gfxTextRuns as necessary).
    let mut seenStartLine = false;
    let mut linesAfterStartLine: u32 = 0;
    loop {
        let mut line = forwardIterator.GetLine();
        if line.IsBlock() {
            break;
        }
        line.SetInvalidateTextRuns(false);
        scanner.SetAtStartOfLine();
        scanner.SetCommonAncestorWithLastFrame(None);
        for child in line.ChildFrames() {
            scanner.ScanFrame(child);
        }
        if line == startLine {
            seenStartLine = true;
        }
        if seenStartLine {
            linesAfterStartLine += 1;
            if linesAfterStartLine >= NUM_LINES_TO_BUILD_TEXT_RUNS && scanner.CanStopOnThisLine() {
                // Don't flush frames; we may be in the middle of a textrun
                // that we can't end here. That's OK, we just won't build it.
                // Note that we must already have finished the textrun for aForFrame,
                // because we've seen the end of a textrun in a line after the line
                // containing aForFrame.
                scanner.FlushLineBreaks(None);
                // This flushes out mMappedFlows and mLineBreakBeforeFrames, which
                // silences assertions in the scanner destructor.
                scanner.ResetRunInfo();
                return;
            }
        }
        if !forwardIterator.Next() {
            break;
        }
    }

    // Set mStartOfLine so FlushFrames knows its textrun ends a line
    scanner.SetAtStartOfLine();
    scanner.FlushFrames(true, false);
}

impl BuildTextRunsScanner {
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::IsTextRunValidForMappedFlows
    pub fn IsTextRunValidForMappedFlows(&self, aTextRun: &RefPtr_gfxTextRun) -> bool {
        if aTextRun.GetFlags2().0 & nsTextFrameUtils_Flags::IsSimpleFlow.0 != 0 {
            return self.mMappedFlows.len() == 1 && self.mMappedFlows[0].mStartFrame == GetFrameForSimpleFlow(aTextRun) && self.mMappedFlows[0].mEndFrame.is_none();
        }

        let userMappedFlows = GetMappedFlows(aTextRun);
        if userMappedFlows.len() != self.mMappedFlows.len() {
            return false;
        }
        for i in 0..self.mMappedFlows.len() {
            if userMappedFlows[i].mStartFrame != self.mMappedFlows[i].mStartFrame || userMappedFlows[i].mContentLength as i32 != self.mMappedFlows[i].GetContentEnd() - self.mMappedFlows[i].mStartFrame.GetContentOffset() {
                return false;
            }
        }
        true
    }

    /**
     * This gets called when we need to make a text run for the current list of
     * frames.
     */
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::FlushFrames
    pub fn FlushFrames(&mut self, aFlushLineBreaks: bool, aSuppressTrailingBreak: bool) {
        let mut textRun: Option<RefPtr_gfxTextRun> = None;
        if !self.mMappedFlows.is_empty() {
            let same = self.mCurrentFramesAllSameTextRun.clone();
            if !self.mSkipIncompleteTextRuns
                && same.as_ref().map_or(false, |r| {
                    (r.GetFlags2().0 & nsTextFrameUtils_Flags::IncomingWhitespace.0 != 0) == (self.mCurrentRunContextInfo & nsTextFrameUtils::INCOMING_WHITESPACE != 0)
                        && (r.GetFlags().0 & ShapedTextFlags::TEXT_INCOMING_ARABICCHAR.0 != 0) == (self.mCurrentRunContextInfo & nsTextFrameUtils::INCOMING_ARABICCHAR != 0)
                        && self.IsTextRunValidForMappedFlows(r)
                })
            {
                // Optimization: We do not need to (re)build the textrun.
                let run = same.unwrap();
                textRun = Some(run.clone());

                if self.mDoLineBreaking {
                    // Feed this run's text into the linebreaker to provide context.
                    if !self.SetupLineBreakerContext(&run) {
                        return;
                    }
                }

                // Update mNextRunContextInfo appropriately
                self.mNextRunContextInfo = nsTextFrameUtils::INCOMING_NONE;
                if run.GetFlags2().0 & nsTextFrameUtils_Flags::TrailingWhitespace.0 != 0 {
                    self.mNextRunContextInfo |= nsTextFrameUtils::INCOMING_WHITESPACE;
                }
                if run.GetFlags().0 & ShapedTextFlags::TEXT_TRAILING_ARABICCHAR.0 != 0 {
                    self.mNextRunContextInfo |= nsTextFrameUtils::INCOMING_ARABICCHAR;
                }
            } else {
                // uint32_t bufferSize = mMaxTextLength * (mDoubleByteText ? 2 : 1); overflow check
                if self.mMaxTextLength == u32::MAX {
                    return;
                }
                let mut buffer = if self.mDoubleByteText { TextRunBuffer::Wide(vec![0u16; self.mMaxTextLength as usize]) } else { TextRunBuffer::Narrow(vec![0u8; self.mMaxTextLength as usize]) };
                textRun = self.BuildTextRunForFrames(&mut buffer);
                if let Some(run) = &textRun {
                    // if (gfxPlatform::GetPlatform()->OpenTypeSVGEnabled()): NOT PORTED (SVG glyphs);
                    // record the textrun for the call to CreateObserversForAnimatedGlyphs
                    self.mCreatedTextRuns.push(run.clone());
                }
            }
        }

        if aFlushLineBreaks {
            self.FlushLineBreaks(if aSuppressTrailingBreak { None } else { textRun.as_ref() });
        }

        self.mCanStopOnThisLine = true;
        self.ResetRunInfo();
    }

    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::FlushLineBreaks
    pub fn FlushLineBreaks(&mut self, aTrailingTextRun: Option<&RefPtr_gfxTextRun>) {
        // If the line-breaker is buffering a potentially-unfinished word,
        // preserve the state of being in-word so that we don't spuriously
        // capitalize the next letter.
        let inWord = self.mLineBreaker.InWord();
        let mut trailingLineBreak = false;
        let rv = self.mLineBreaker.Reset(&mut trailingLineBreak);
        self.mLineBreaker.SetWordContinuation(inWord);
        // textRun may be null for various reasons, including because we constructed
        // a partial textrun just to get the linebreaker and other state set up
        // to build the next textrun.
        if !NS_FAILED(rv) && trailingLineBreak {
            if let Some(run) = aTrailingTextRun {
                run.SetFlagBits(nsTextFrameUtils_Flags::HasTrailingBreak);
            }
        }

        for sink in &self.mBreakSinks {
            // TODO cause frames associated with the textrun to be reflowed, if they
            // aren't being reflowed already!
            sink.Finish();
        }
        self.mBreakSinks.clear();
    }

    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::AccumulateRunInfo
    pub fn AccumulateRunInfo(&mut self, aFrame: nsTextFrame) {
        if self.mMaxTextLength != u32::MAX {
            NS_ASSERTION!(self.mMaxTextLength < u32::MAX - aFrame.GetContentLength() as u32, "integer overflow");
            if self.mMaxTextLength >= u32::MAX - aFrame.GetContentLength() as u32 {
                self.mMaxTextLength = u32::MAX;
            } else {
                self.mMaxTextLength += aFrame.GetContentLength() as u32;
            }
        }
        self.mDoubleByteText |= aFrame.CharacterDataBuffer().Is2b();
        self.mLastFrame = Some(aFrame);
        self.mCommonAncestorWithLastFrame = aFrame.GetParent();

        let last = self.mMappedFlows.len() - 1;
        let mappedFlow = &mut self.mMappedFlows[last];
        NS_ASSERTION!(mappedFlow.mStartFrame == aFrame || mappedFlow.GetContentEnd() == aFrame.GetContentOffset(), "Overlapping or discontiguous frames => BAD");
        mappedFlow.mEndFrame = aFrame.GetNextContinuation().map(nsTextFrame::from_frame);
        let frameRun = aFrame.GetTextRun(self.mWhichTextRun);
        let same = match (&self.mCurrentFramesAllSameTextRun, &frameRun) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        };
        if !same {
            self.mCurrentFramesAllSameTextRun = None;
        }

        if self.mStartOfLine {
            self.mLineBreakBeforeFrames.push(aFrame);
            self.mStartOfLine = false;
        }

        // Default limits used by `hyphenate-limit-chars` for `auto` components, as
        // suggested by the CSS Text spec.
        // TODO: consider making these sensitive to the context, e.g. increasing the
        // values for long line lengths to reduce the tendency to hyphenate too much.
        let kDefaultHyphenateTotalWordLength: u32 = 5;
        let kDefaultHyphenatePreBreakLength: u32 = 2;
        let kDefaultHyphenatePostBreakLength: u32 = 2;

        // NOT PORTED: `hyphenate-limit-chars` is not in the servo configuration of Stylo; all three
        // components are `auto`.
        let pre = kDefaultHyphenatePreBreakLength;
        let post = kDefaultHyphenatePostBreakLength;
        let mut total = kDefaultHyphenateTotalWordLength;
        total = std::cmp::max(total, pre + post);
        self.mLineBreaker.SetHyphenateLimitChars(total, pre, post);
    }
}

// Gecko: layout/generic/nsTextFrame.cpp HasTerminalNewline
fn HasTerminalNewline(aFrame: nsTextFrame) -> bool {
    if aFrame.GetContentLength() == 0 {
        return false;
    }
    let characterDataBuffer = aFrame.CharacterDataBuffer();
    characterDataBuffer.CharAt(aFrame.GetContentEnd() as u32 - 1) == 0x0A
}

// Gecko: layout/generic/nsTextFrame.cpp GetFirstFontMetrics
fn GetFirstFontMetrics(aFontGroup: &Rc<gfxFontGroup>, aVerticalMetrics: bool) -> gfxFont_Metrics {
    // NOT PORTED: vertical metrics
    let font = aFontGroup.GetFirstValidFont(0, None, None);
    font.GetMetrics().clone()
}

// Gecko: layout/generic/nsTextFrame.cpp GetMinTabAdvanceAppUnits
fn GetMinTabAdvanceAppUnits(aTextRun: &RefPtr_gfxTextRun) -> gfxFloat {
    let chWidthAppUnits: gfxFloat = (GetFirstFontMetrics(&aTextRun.GetFontGroup(), aTextRun.IsVertical()).ZeroOrAveCharWidth() * aTextRun.GetAppUnitsPerDevUnit() as f64).round();
    0.5 * chWidthAppUnits
}

// Gecko: layout/generic/nsTextFrame.cpp GetSVGFontSizeScaleFactor
// NOT PORTED: SVG text; the factor is 1.

// Gecko: layout/generic/nsTextFrame.cpp ResolveLetterSpacing
fn ResolveLetterSpacing(aFrame: nsIFrame, aStyleText: &nsStyleTextAdapter) -> nscoord {
    // aFrame->IsInSVGTextSubtree(): NOT PORTED
    aStyleText.mLetterSpacingResolved(StyleCSSPixelLength_ToAppUnits(aFrame.StyleFont().mFont().size))
}

// This function converts non-coord values (e.g. percentages) to nscoord.
// Gecko: layout/generic/nsTextFrame.cpp ResolveWordSpacing
fn ResolveWordSpacing(aFrame: nsIFrame, aStyleText: &nsStyleTextAdapter) -> nscoord {
    // aFrame->IsInSVGTextSubtree(): NOT PORTED
    aStyleText.mWordSpacingResolved(StyleCSSPixelLength_ToAppUnits(aFrame.StyleFont().mFont().size))
}

impl nsTextFrame {
    // Return ShapedTextFlags::TEXT_ENABLE_SPACING if spacing is needed due to
    // letter-spacing, word-spacing, etc.
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetSpacingFlags
    pub fn GetSpacingFlags(&self) -> ShapedTextFlags {
        let styleText = nsStyleTextAdapter::of(self.0);

        // It's possible to have a calc() value that computes to zero but for which
        // IsDefinitelyZero() is false, in which case we'll return
        // TEXT_ENABLE_SPACING unnecessarily. That's ok because such cases are likely
        // to be rare, and avoiding TEXT_ENABLE_SPACING is just an optimization.
        // (|| TextAutospace::Enabled(...): NOT PORTED)
        let nonStandardSpacing = !styleText.mLetterSpacingIsDefinitelyZero() || !styleText.mWordSpacingIsDefinitelyZero();
        if nonStandardSpacing { ShapedTextFlags::TEXT_ENABLE_SPACING } else { ShapedTextFlags::default() }
    }
}

// Returns true if the frame has alignment-baseline and baseline-shift
// set to their initial values.
// Gecko: layout/generic/nsTextFrame.cpp HasDefaultVerticalAlignment
fn HasDefaultVerticalAlignment(aFrame: nsIFrame) -> bool {
    if aFrame.AlignmentBaseline() != StyleAlignmentBaseline::Baseline {
        return false;
    }

    match aFrame.BaselineShift() {
        StyleBaselineShift::Keyword(_) => false,
        StyleBaselineShift::Length(lp) => mrt_style::style::Zero::is_zero(&lp),
    }
}

impl BuildTextRunsScanner {
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::ContinueTextRunAcrossFrames
    pub fn ContinueTextRunAcrossFrames(&self, aFrame1: nsTextFrame, aFrame2: nsTextFrame) -> bool {
        // We don't need to check font size inflation, since
        // |FindLineContainer| above (via |nsIFrame::CanContinueTextRun|)
        // ensures that text runs never cross block boundaries.  This means
        // that the font size inflation on all text frames in the text run is
        // already guaranteed to be the same as each other (and for the line
        // container).
        if self.mBidiEnabled {
            let data1 = aFrame1.GetBidiData();
            let data2 = aFrame2.GetBidiData();
            if data1.embeddingLevel != data2.embeddingLevel || data2.precedingControl != kBidiLevelNone {
                return false;
            }
        }

        let sc1 = aFrame1.Style();
        let sc2 = aFrame2.Style();

        // Any difference in writing-mode/directionality inhibits shaping across
        // the boundary.
        let wm = WritingMode::new__ComputedStyle(&sc1);
        if wm != WritingMode::new__ComputedStyle(&sc2) {
            return false;
        }

        let textStyle1 = nsStyleTextAdapter::of(aFrame1.0);
        // If the first frame ends in a preformatted newline, then we end the textrun
        // here. This avoids creating giant textruns for an entire plain text file.
        // Note that we create a single text frame for a preformatted text node,
        // even if it has newlines in it, so typically we won't see trailing newlines
        // until after reflow has broken up the frame into one (or more) frames per
        // line. That's OK though.
        if textStyle1.NewlineIsSignificant(aFrame1) && HasTerminalNewline(aFrame1) {
            return false;
        }

        if !nsIContent::IsSameNode(&aFrame1.GetParent().unwrap().GetContent(), &aFrame2.GetParent().unwrap().GetContent()) {
            // Does aFrame, or any ancestor between it and aAncestor, have a property
            // that should inhibit cross-element-boundary shaping on aSide?
            let PreventCrossBoundaryShaping = |aFrame: nsIFrame, aAncestor: nsIFrame, aSide: Side| -> bool {
                let mut aFrame = aFrame;
                while aFrame != aAncestor {
                    let ctx = aFrame.Style();
                    // According to https://drafts.csswg.org/css-text/#boundary-shaping:
                    //
                    // Text shaping must be broken at inline box boundaries when any of
                    // the following are true for any box whose boundary separates the
                    // two typographic character units:
                    //
                    // 1. Any of margin/border/padding separating the two typographic
                    //    character units in the inline axis is non-zero.
                    // (the anchor resolution is NOT PORTED: no anchor positioning)
                    let margin = aFrame.StyleMargin().GetMargin__Side_AnchorPosResolutionParams(aSide, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aFrame, std::ptr::null_mut()));
                    if !margin.ConvertsToLength() || margin.AsLengthPercentage().ToLength() != 0 {
                        return true;
                    }
                    let padding = aFrame.StylePadding().mPadding().Get__Side(aSide).clone();
                    if !padding.ConvertsToLength() || padding.ToLength() != 0 {
                        return true;
                    }
                    if aFrame.StyleBorder().GetComputedBorder().Side(aSide) != 0 {
                        return true;
                    }

                    // 2. vertical-align is not baseline.
                    if !HasDefaultVerticalAlignment(aFrame) {
                        return true;
                    }

                    // 3. The boundary is a bidi isolation boundary.
                    let unicodeBidi = ctx.get_text().unicode_bidi;
                    if unicodeBidi == StyleUnicodeBidi::Isolate || unicodeBidi == StyleUnicodeBidi::IsolateOverride {
                        return true;
                    }

                    aFrame = aFrame.GetParent().unwrap();
                }
                false
            };

            let ancestor = nsLayoutUtils::FindNearestCommonAncestorFrameWithinBlock(aFrame1, aFrame2);

            let Some(ancestor) = ancestor else {
                // The two frames are within different blocks, e.g. due to block
                // fragmentation. In theory we shouldn't prevent cross-frame shaping
                // here, but it's an edge case where we should rarely decide to allow
                // cross-frame shaping, so we don't try harder here.
                return false;
            };

            // We inhibit cross-element-boundary shaping if we're in SVG content,
            // as there are too many things SVG might be doing (like applying per-
            // element positioning) that wouldn't make sense with shaping across
            // the boundary.
            if ancestor.IsInSVGTextSubtree() {
                return false;
            }

            // Map inline-end and inline-start to physical sides for checking presence
            // of non-zero margin/border/padding.
            let side1 = wm.PhysicalSide(LogicalSide::IEnd);
            let side2 = wm.PhysicalSide(LogicalSide::IStart);
            // If the frames have an embedding level that is opposite to the writing
            // mode, we need to swap which sides we're checking.
            let (side1, side2) = if aFrame1.GetEmbeddingLevel().IsRTL() == wm.IsBidiLTR() { (side2, side1) } else { (side1, side2) };

            if PreventCrossBoundaryShaping(aFrame1.0, ancestor, side1) || PreventCrossBoundaryShaping(aFrame2.0, ancestor, side2) {
                return false;
            }
        }

        if nsIContent::IsSameNode(&aFrame1.GetContent(), &aFrame2.GetContent()) && aFrame1.GetNextInFlow() != Some(aFrame2.0) {
            // aFrame2 must be a non-fluid continuation of aFrame1. This can happen
            // sometimes when the unicode-bidi property is used; the bidi resolver
            // breaks text into different frames even though the text has the same
            // direction. We can't allow these two frames to share the same textrun
            // because that would violate our invariant that two flows in the same
            // textrun have different content elements.
            return false;
        }

        if mrt_style::style::servo_arc::Arc::ptr_eq(&sc1, &sc2) {
            return true;
        }

        let textStyle2 = nsStyleTextAdapter::of(aFrame2.0);
        // textStyle1->mTextTransform != textStyle2->mTextTransform: both are identity here
        // (NOT PORTED: text-transform), so only the word-break and line-break checks remain.
        if sc1.get_inherited_text().text_transform != sc2.get_inherited_text().text_transform
            || sc1.get_inherited_text().word_break != sc2.get_inherited_text().word_break
        // || textStyle1->mLineBreak != textStyle2->mLineBreak: `line-break` is not in this Stylo
        {
            return false;
        }

        let fontStyle1 = aFrame1.StyleFont();
        let fontStyle2 = aFrame2.StyleFont();
        let letterSpacing1 = ResolveLetterSpacing(aFrame1.0, &textStyle1);
        let letterSpacing2 = ResolveLetterSpacing(aFrame2.0, &textStyle2);
        sc1.get_font() == sc2.get_font()
            && fontStyle1.mLanguage() == fontStyle2.mLanguage()
            && nsLayoutUtils::GetTextRunFlagsForStyle(&sc1, &fontStyle1, &textStyle1, letterSpacing1) == nsLayoutUtils::GetTextRunFlagsForStyle(&sc2, &fontStyle2, &textStyle2, letterSpacing2)
    }

    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::ScanFrame
    pub fn ScanFrame(&mut self, aFrame: nsIFrame) {
        let frameType = aFrame.Type();
        if frameType == LayoutFrameType::RubyTextContainer {
            // Don't include any ruby text container into the text run.
            return;
        }

        // First check if we can extend the current mapped frame block. This is
        // common.
        if !self.mMappedFlows.is_empty() {
            let mappedFlow = *self.mMappedFlows.last().unwrap();
            if mappedFlow.mEndFrame.map(|e| e.0) == Some(aFrame) && aFrame.HasAnyStateBits(NS_FRAME_IS_FLUID_CONTINUATION) {
                NS_ASSERTION!(frameType == LayoutFrameType::Text, "Flow-sibling of a text frame is not a text frame?");

                // Don't do this optimization if mLastFrame has a terminal newline...
                // it's quite likely preformatted and we might want to end the textrun
                // here. This is almost always true:
                let last = self.mLastFrame.unwrap();
                if mrt_style::style::servo_arc::Arc::ptr_eq(&last.Style(), &aFrame.Style()) && !HasTerminalNewline(last) {
                    self.AccumulateRunInfo(nsTextFrame::from_frame(aFrame));
                    return;
                }
            }
        }

        // Now see if we can add a new set of frames to the current textrun
        if frameType == LayoutFrameType::Text {
            let frame = nsTextFrame::from_frame(aFrame);

            if let Some(last) = self.mLastFrame {
                if !self.ContinueTextRunAcrossFrames(last, frame) {
                    self.FlushFrames(false, false);
                } else if nsIContent::IsSameNode(&last.GetContent(), &frame.GetContent()) {
                    self.AccumulateRunInfo(frame);
                    return;
                }
            }

            self.mMappedFlows.push(BuildTextRunsScanner_MappedFlow { mStartFrame: frame, mEndFrame: None, mAncestorControllingInitialBreak: self.mCommonAncestorWithLastFrame });

            self.AccumulateRunInfo(frame);
            if self.mMappedFlows.len() == 1 {
                self.mCurrentFramesAllSameTextRun = frame.GetTextRun(self.mWhichTextRun);
                self.mCurrentRunContextInfo = self.mNextRunContextInfo;
            }
            return;
        }

        // if (frameType == Placeholder && HasAnyStateBits(PLACEHOLDER_FOR_ABSPOS | PLACEHOLDER_FOR_FIXEDPOS))
        //   FlushFrames(true, false)  (bug 1418472)
        // NOT PORTED: absolute positioning; no placeholder is for an abspos frame.

        let mut traversal = CanTextCrossFrameBoundary(aFrame);
        let isBR = frameType == LayoutFrameType::Br;
        if !traversal.mLineBreakerCanCrossFrameBoundary {
            // BR frames are special. We do not need or want to record a break
            // opportunity before a BR frame.
            self.FlushFrames(true, isBR);
            self.mCommonAncestorWithLastFrame = Some(aFrame);
            self.mNextRunContextInfo &= !nsTextFrameUtils::INCOMING_WHITESPACE;
            self.mStartOfLine = false;
        } else if !traversal.mTextRunCanCrossFrameBoundary {
            self.FlushFrames(false, false);
        }

        let mut f = traversal.NextFrameToScan();
        while let Some(fr) = f {
            self.ScanFrame(fr);
            f = traversal.NextFrameToScan();
        }

        if !traversal.mLineBreakerCanCrossFrameBoundary {
            // Really if we're a BR frame this is unnecessary since descendInto will be
            // false. In fact this whole "if" statement should move into the
            // descendInto.
            self.FlushFrames(true, isBR);
            self.mCommonAncestorWithLastFrame = Some(aFrame);
            self.mNextRunContextInfo &= !nsTextFrameUtils::INCOMING_WHITESPACE;
        } else if !traversal.mTextRunCanCrossFrameBoundary {
            self.FlushFrames(false, false);
        }

        self.LiftCommonAncestorWithLastFrameToParent(aFrame.GetParent());
    }

    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::GetNextBreakBeforeFrame
    pub fn GetNextBreakBeforeFrame(&self, aIndex: &mut u32) -> Option<nsTextFrame> {
        let index = *aIndex;
        if index >= self.mLineBreakBeforeFrames.len() as u32 {
            return None;
        }
        *aIndex = index + 1;
        Some(self.mLineBreakBeforeFrames[index as usize])
    }
}

// Gecko: layout/generic/nsTextFrame.cpp GetFontGroupForFrame
// (the font group is the metrics' own; the metrics come from nsFontCache)
fn GetFontGroupForFrame(aFrame: nsIFrame, aFontSizeInflation: f32) -> Rc<gfxFontGroup> {
    let metrics = nsLayoutUtils::GetFontMetricsForFrame(aFrame, aFontSizeInflation);
    metrics.GetThebesFontGroup().clone()
}

impl nsTextFrame {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::InflatedFontMetrics
    pub fn InflatedFontMetrics(&self) -> nsFontMetrics {
        if let Some(fm) = self.with_data(|d| d.mFontMetrics.clone()) {
            return (*fm).clone();
        }
        let inflation = nsLayoutUtils::FontSizeInflationFor(self.0);
        let fm = nsLayoutUtils::GetFontMetricsForFrame(self.0, inflation);
        let shared = InternFontMetrics(&fm);
        self.with_data(|d| d.mFontMetrics = Some(shared));
        fm
    }
}

// Gecko: layout/generic/nsTextFrame.cpp GetInflatedFontGroupForFrame
fn GetInflatedFontGroupForFrame(aFrame: nsTextFrame) -> Rc<gfxFontGroup> {
    if let Some(textRun) = aFrame.GetTextRun(nsTextFrame_TextRunType::eInflated) {
        return textRun.GetFontGroup();
    }
    aFrame.InflatedFontMetrics().GetThebesFontGroup().clone()
}

// Gecko: layout/generic/nsTextFrame.cpp CreateReferenceDrawTarget, GetHyphenTextRun
// NOT PORTED: hyphenation. GetHyphenTextRun makes a text run of the hyphenate character to measure
// the hyphen (nsTextFrame.cpp lines 2320-2358 at GECKO_REV); the hyphen width is 0 here.

impl BuildTextRunsScanner {
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::BuildTextRunForFrames
    // Rust: the buffer is passed as an enum (see TextRunBuffer).
    pub fn BuildTextRunForFrames(&mut self, aTextBuffer: &mut TextRunBuffer) -> Option<RefPtr_gfxTextRun> {
        let mut skipChars = gfxSkipChars::new();

        let mut anyTextTransformStyle = false;
        let mut anyMathMLStyling = false;
        let mut anyTextEmphasis = false;
        let sstyScriptLevel: u8 = 0;
        let mathFlags: u32 = 0;
        let mut flags = ShapedTextFlags::default();
        let mut flags2 = nsTextFrameUtils_Flags::NoBreaks;

        if self.mCurrentRunContextInfo & nsTextFrameUtils::INCOMING_WHITESPACE != 0 {
            flags2 |= nsTextFrameUtils_Flags::IncomingWhitespace;
        }
        if self.mCurrentRunContextInfo & nsTextFrameUtils::INCOMING_ARABICCHAR != 0 {
            flags |= ShapedTextFlags::TEXT_INCOMING_ARABICCHAR;
        }

        let mut textBreakPoints: Vec<i32> = Vec::new();
        // If the situation is particularly simple (and common) we don't need to
        // allocate userData.
        let simpleFlow = self.mMappedFlows.len() == 1 && self.mMappedFlows[0].mEndFrame.is_none() && self.mMappedFlows[0].mStartFrame.GetContentOffset() == 0;
        let mut userMappedFlows: Vec<TextRunMappedFlow> = Vec::with_capacity(self.mMappedFlows.len());

        let mut currentTransformedTextOffset: u32 = 0;

        let mut nextBreakIndex: u32 = 0;
        let mut nextBreakBeforeFrame = self.GetNextBreakBeforeFrame(&mut nextBreakIndex);
        let isSVG = self.mLineContainer.IsInSVGTextSubtree();
        let lineContainerText = nsStyleTextAdapter::of(self.mLineContainer);
        let enabledJustification = lineContainerText.mTextAlignIsJustify() || lineContainerText.mTextAlignLastIsJustify();

        let mut textStyle: Option<nsStyleTextAdapter> = None;
        let mut fontStyle: Option<nsStyleFont> = None;
        let mut lastComputedStyle = None;
        for i in 0..self.mMappedFlows.len() {
            let mappedFlow = self.mMappedFlows[i];
            let f = mappedFlow.mStartFrame;

            lastComputedStyle = Some(f.Style());
            // Detect use of text-transform or font-variant anywhere in the run
            let ts = nsStyleTextAdapter::of(f.0);
            if !ts.mTextTransformIsNone() /* || mWebkitTextSecurity != None || IsTextCombined(): NOT PORTED */ {
                anyTextTransformStyle = true;
            }
            if ts.HasEffectiveTextEmphasis() {
                anyTextEmphasis = true;
            }
            flags |= f.GetSpacingFlags();
            let compression = GetCSSWhitespaceToCompressionMode(f, &ts);
            if (enabledJustification || f.ShouldSuppressLineBreak()) && !isSVG {
                flags |= ShapedTextFlags::TEXT_ENABLE_SPACING;
            }
            fontStyle = Some(f.StyleFont());
            // MathML (mMathVariant, NS_FRAME_IS_IN_SINGLE_CHAR_MI, TEXT_IS_IN_TOKEN_MATHML,
            // NS_FRAME_MATHML_SCRIPT_DESCENDANT, ssty script levels): NOT PORTED. anyMathMLStyling stays false.
            textStyle = Some(ts);

            // Figure out what content is included in this flow.
            let content = f.GetContent();
            let characterDataBuffer = f.CharacterDataBuffer();
            let contentStart = mappedFlow.mStartFrame.GetContentOffset();
            let contentEnd = mappedFlow.GetContentEnd();
            let contentLength = contentEnd - contentStart;
            let language: Option<nsAtom> = f.StyleFont().mLanguage();

            let newFlow = TextRunMappedFlow {
                mStartFrame: mappedFlow.mStartFrame,
                mDOMOffsetToBeforeTransformOffset: skipChars.GetOriginalCharCount() - mappedFlow.mStartFrame.GetContentOffset(),
                mContentLength: contentLength as u32,
            };
            userMappedFlows.push(newFlow);

            while let Some(nb) = nextBreakBeforeFrame {
                if !nsIContent::IsSameNode(&nb.GetContent(), &content) {
                    break;
                }
                textBreakPoints.push(nb.GetContentOffset() + newFlow.mDOMOffsetToBeforeTransformOffset);
                nextBreakBeforeFrame = self.GetNextBreakBeforeFrame(&mut nextBreakIndex);
            }

            let mut analysisFlags = nsTextFrameUtils_Flags::default();
            if characterDataBuffer.Is2b() {
                NS_ASSERTION!(self.mDoubleByteText, "Wrong buffer char size!");
                let TextRunBuffer::Wide(buf) = aTextBuffer else { unreachable!() };
                let bufStart = currentTransformedTextOffset as usize;
                let written = nsTextFrameUtils::TransformText::<u16>(
                    &characterDataBuffer.Get2b()[contentStart as usize..],
                    contentLength as u32,
                    &mut buf[bufStart..],
                    compression,
                    &mut self.mNextRunContextInfo,
                    &mut skipChars,
                    &mut analysisFlags,
                    language,
                );
                currentTransformedTextOffset = (bufStart + written) as u32;
            } else if self.mDoubleByteText {
                // Need to expand the text. First transform it into a temporary buffer,
                // then expand.
                let mut tempBuf: Vec<u8> = vec![0u8; contentLength as usize];
                let written = nsTextFrameUtils::TransformText::<u8>(
                    &characterDataBuffer.Get1b()[contentStart as usize..],
                    contentLength as u32,
                    &mut tempBuf[..],
                    compression,
                    &mut self.mNextRunContextInfo,
                    &mut skipChars,
                    &mut analysisFlags,
                    language,
                );
                let TextRunBuffer::Wide(buf) = aTextBuffer else { unreachable!() };
                let bufStart = currentTransformedTextOffset as usize;
                // ExpandBuffer(aDest, aSrc, aCount)
                for k in 0..written {
                    buf[bufStart + k] = tempBuf[k] as u16;
                }
                currentTransformedTextOffset = (bufStart + written) as u32;
            } else {
                let TextRunBuffer::Narrow(buf) = aTextBuffer else { unreachable!() };
                let bufStart = currentTransformedTextOffset as usize;
                let written = nsTextFrameUtils::TransformText::<u8>(
                    &characterDataBuffer.Get1b()[contentStart as usize..],
                    contentLength as u32,
                    &mut buf[bufStart..],
                    compression,
                    &mut self.mNextRunContextInfo,
                    &mut skipChars,
                    &mut analysisFlags,
                    language,
                );
                currentTransformedTextOffset = (bufStart + written) as u32;
            }
            flags2 |= analysisFlags;
        }

        if simpleFlow {
            flags2 |= nsTextFrameUtils_Flags::IsSimpleFlow;
        }

        let transformedLength = currentTransformedTextOffset;

        // Now build the textrun
        let firstFrame = self.mMappedFlows[0].mStartFrame;
        let fontInflation: f32;
        let fontGroup: Rc<gfxFontGroup>;
        if self.mWhichTextRun == nsTextFrame_TextRunType::eNotInflated {
            fontInflation = 1.0;
            fontGroup = GetFontGroupForFrame(firstFrame.0, fontInflation);
        } else {
            fontInflation = nsLayoutUtils::FontSizeInflationFor(firstFrame.0);
            fontGroup = GetInflatedFontGroupForFrame(firstFrame);
        }

        if flags2.0 & nsTextFrameUtils_Flags::HasTab.0 != 0 {
            flags |= ShapedTextFlags::TEXT_ENABLE_SPACING;
        }
        if flags2.0 & nsTextFrameUtils_Flags::HasShy.0 != 0 {
            flags |= ShapedTextFlags::TEXT_ENABLE_HYPHEN_BREAKS;
        }
        if self.mBidiEnabled && firstFrame.GetEmbeddingLevel().IsRTL() {
            flags |= ShapedTextFlags::TEXT_IS_RTL;
        }
        if self.mNextRunContextInfo & nsTextFrameUtils::INCOMING_WHITESPACE != 0 {
            flags2 |= nsTextFrameUtils_Flags::TrailingWhitespace;
        }
        if self.mNextRunContextInfo & nsTextFrameUtils::INCOMING_ARABICCHAR != 0 {
            flags |= ShapedTextFlags::TEXT_TRAILING_ARABICCHAR;
        }
        // ContinueTextRunAcrossFrames guarantees that it doesn't matter which
        // frame's style is used, so we use a mixture of the first frame and
        // last frame's style
        let textStyle = textStyle.unwrap();
        let fontStyle = fontStyle.unwrap();
        let lastComputedStyle = lastComputedStyle.unwrap();
        flags |= nsLayoutUtils::GetTextRunFlagsForStyle(&lastComputedStyle, &fontStyle, &textStyle, ResolveLetterSpacing(firstFrame.0, &textStyle));
        // XXX this is a bit of a hack. For performance reasons, if we're favouring
        // performance over quality, don't try to get accurate glyph extents.
        if flags.0 & ShapedTextFlags::TEXT_OPTIMIZE_SPEED.0 == 0 {
            flags |= ShapedTextFlags::TEXT_NEED_BOUNDING_BOX;
        }

        // Convert linebreak coordinates to transformed string offsets
        NS_ASSERTION!(nextBreakIndex == self.mLineBreakBeforeFrames.len() as u32, "Didn't find all the frames to break-before...");
        let mut textBreakPointsAfterTransform: Vec<u32> = Vec::new();
        {
            let mut iter = gfxSkipCharsIterator::new__gfxSkipChars_int32_t(&skipChars, 0);
            for i in 0..textBreakPoints.len() {
                nsTextFrameUtils::AppendLineBreakOffset(&mut textBreakPointsAfterTransform, iter.ConvertOriginalToSkipped(textBreakPoints[i]));
            }
        }
        if self.mStartOfLine {
            nsTextFrameUtils::AppendLineBreakOffset(&mut textBreakPointsAfterTransform, transformedLength);
        }

        // Setup factory chain
        // (needsToMaskPassword, nsCaseTransformTextRunFactory, MathMLTextRunFactory, and the
        // nsTransformedCharStyle array: NOT PORTED. `text-transform` is the identity.)
        let _ = (anyTextTransformStyle, anyMathMLStyling, mathFlags, sstyScriptLevel);

        // (the device pixel is the CSS pixel here: there is no zoom, see nsPresContext.rs)
        let appUnitsPerDevPixel = AppUnitsPerCSSPixel();
        let mut params = gfxTextRunFactory_Parameters {
            mDrawTarget: None,
            mSkipChars: Some(&mut skipChars),
            mInitialBreaks: Some(&textBreakPointsAfterTransform[..]),
            mInitialBreakCount: textBreakPointsAfterTransform.len() as u32,
            mAppUnitsPerDevUnit: appUnitsPerDevPixel,
        };

        let textRun: Option<RefPtr_gfxTextRun> = match aTextBuffer {
            TextRunBuffer::Wide(buf) => fontGroup.MakeTextRun__T_ptr_uint32_t_Parameters_ptr_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr::<u16>(&buf[..transformedLength as usize], transformedLength, &mut params, flags, flags2, None),
            TextRunBuffer::Narrow(buf) => {
                flags |= ShapedTextFlags::TEXT_IS_8BIT;
                fontGroup.MakeTextRun__T_ptr_uint32_t_Parameters_ptr_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr::<u8>(&buf[..transformedLength as usize], transformedLength, &mut params, flags, flags2, None)
            }
        };
        let Some(textRun) = textRun else {
            return None;
        };

        // The user data of the text run: the simple flow or the mapped flows.
        if simpleFlow {
            SetTextRunUserData(&textRun, gfxTextRun_UserData::SimpleFlow(userMappedFlows[0].mStartFrame));
        } else {
            SetTextRunUserData(&textRun, gfxTextRun_UserData::Flows(TextRunUserData { mMappedFlows: userMappedFlows.into_boxed_slice(), mLastFlowIndex: 0 }));
        }

        // We have to set these up after we've created the textrun, because
        // the breaks may be stored in the textrun during this very call.
        // This is a bit annoying because it requires another loop over the frames
        // making up the textrun, but I don't see a way to avoid this.
        // We have to do this if line-breaking is required OR if a text-transform
        // is in effect, because we depend on the line-breaker's scanner (via
        // BreakSink::Finish) to finish building transformed textruns.
        if self.mDoLineBreaking {
            self.SetupBreakSinksForTextRun(&textRun, aTextBuffer);
        }

        if anyTextEmphasis {
            self.SetupTextEmphasisForTextRun(&textRun, aTextBuffer);
        }

        if self.mSkipIncompleteTextRuns {
            self.mSkipIncompleteTextRuns = !match aTextBuffer {
                TextRunBuffer::Wide(buf) => TextContainsLineBreakerWhiteSpace::<u16>(&buf[..], transformedLength),
                TextRunBuffer::Narrow(buf) => TextContainsLineBreakerWhiteSpace::<u8>(&buf[..], transformedLength),
            };
            // Since we're doing to destroy the user data now, avoid a dangling
            // pointer. Strictly speaking we don't need to do this since it should
            // not be used (since this textrun will not be used and will be
            // itself deleted soon), but it's always better to not have dangling
            // pointers around.
            DestroyTextRunUserData(&textRun);
            return None;
        }

        // Actually wipe out the textruns associated with the mapped frames and
        // associate those frames with this text run.
        self.AssignTextRun(&textRun, fontInflation);
        Some(textRun)
    }

    // This is a cut-down version of BuildTextRunForFrames used to set up
    // context for the line-breaker, when the textrun has already been created.
    // So it does the same walk over the mMappedFlows, but doesn't actually
    // build a new textrun.
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::SetupLineBreakerContext
    pub fn SetupLineBreakerContext(&mut self, aTextRun: &RefPtr_gfxTextRun) -> bool {
        if self.mMaxTextLength == u32::MAX {
            return false;
        }
        let mut buffer = if self.mDoubleByteText { TextRunBuffer::Wide(vec![0u16; self.mMaxTextLength as usize]) } else { TextRunBuffer::Narrow(vec![0u8; self.mMaxTextLength as usize]) };

        let mut skipChars = gfxSkipChars::new();
        let language: Option<nsAtom> = self.mMappedFlows[0].mStartFrame.StyleFont().mLanguage();
        let mut textPtr: usize = 0;

        for i in 0..self.mMappedFlows.len() {
            let mappedFlow = self.mMappedFlows[i];
            let f = mappedFlow.mStartFrame;

            let textStyle = nsStyleTextAdapter::of(f.0);
            let compression = GetCSSWhitespaceToCompressionMode(f, &textStyle);

            // Figure out what content is included in this flow.
            let characterDataBuffer = f.CharacterDataBuffer();
            let contentStart = mappedFlow.mStartFrame.GetContentOffset();
            let contentEnd = mappedFlow.GetContentEnd();
            let contentLength = contentEnd - contentStart;

            let mut analysisFlags = nsTextFrameUtils_Flags::default();
            if characterDataBuffer.Is2b() {
                NS_ASSERTION!(self.mDoubleByteText, "Wrong buffer char size!");
                let TextRunBuffer::Wide(buf) = &mut buffer else { unreachable!() };
                let written = nsTextFrameUtils::TransformText::<u16>(&characterDataBuffer.Get2b()[contentStart as usize..], contentLength as u32, &mut buf[textPtr..], compression, &mut self.mNextRunContextInfo, &mut skipChars, &mut analysisFlags, language);
                textPtr += written;
            } else if self.mDoubleByteText {
                // Need to expand the text. First transform it into a temporary buffer,
                // then expand.
                let mut tempBuf: Vec<u8> = vec![0u8; contentLength as usize];
                let written = nsTextFrameUtils::TransformText::<u8>(&characterDataBuffer.Get1b()[contentStart as usize..], contentLength as u32, &mut tempBuf[..], compression, &mut self.mNextRunContextInfo, &mut skipChars, &mut analysisFlags, language);
                let TextRunBuffer::Wide(buf) = &mut buffer else { unreachable!() };
                for k in 0..written {
                    buf[textPtr + k] = tempBuf[k] as u16;
                }
                textPtr += written;
            } else {
                let TextRunBuffer::Narrow(buf) = &mut buffer else { unreachable!() };
                let written = nsTextFrameUtils::TransformText::<u8>(&characterDataBuffer.Get1b()[contentStart as usize..], contentLength as u32, &mut buf[textPtr..], compression, &mut self.mNextRunContextInfo, &mut skipChars, &mut analysisFlags, language);
                textPtr += written;
            }
        }

        // We have to set these up after we've created the textrun, because
        // the breaks may be stored in the textrun during this very call.
        // This is a bit annoying because it requires another loop over the frames
        // making up the textrun, but I don't see a way to avoid this.
        self.SetupBreakSinksForTextRun(aTextRun, &buffer);

        true
    }
}

// Gecko: layout/generic/nsTextFrame.cpp HasCompressedLeadingWhitespace
fn HasCompressedLeadingWhitespace(aFrame: nsTextFrame, aStyleText: &nsStyleTextAdapter, aContentEndOffset: i32, aIterator: &gfxSkipCharsIterator) -> bool {
    if !aIterator.IsOriginalCharSkipped(None) {
        return false;
    }

    let mut iter = *aIterator;
    let mut frameContentOffset = aFrame.GetContentOffset();
    let characterDataBuffer = aFrame.CharacterDataBuffer();
    while frameContentOffset < aContentEndOffset && iter.IsOriginalCharSkipped(None) {
        if IsTrimmableSpace(&characterDataBuffer, frameContentOffset as u32, aStyleText, false) {
            return true;
        }
        frameContentOffset += 1;
        iter.AdvanceOriginal(1);
    }
    false
}

impl BuildTextRunsScanner {
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::SetupBreakSinksForTextRun
    pub fn SetupBreakSinksForTextRun(&mut self, aTextRun: &RefPtr_gfxTextRun, aTextPtr: &TextRunBuffer) {
        // textruns have uniform language
        let styleFont = self.mMappedFlows[0].mStartFrame.StyleFont();
        // We should only use a language for hyphenation if it was specified
        // explicitly.
        let hyphenationLanguage: Option<nsAtom> = if styleFont.mExplicitLanguage() { styleFont.mLanguage() } else { None };
        // We keep this pointed at the skip-chars data for the current mappedFlow.
        // This lets us cheaply check whether the flow has compressed initial
        // whitespace...
        let skipChars = aTextRun.GetSkipChars();
        let mut iter = gfxSkipCharsIterator::new__gfxSkipChars_int32_t(skipChars, 0);

        for i in 0..self.mMappedFlows.len() {
            let mappedFlow = self.mMappedFlows[i];
            // The CSS word-break value may change within a word, so we reset it for
            // each MappedFlow. The line-breaker will flush its text if the property
            // actually changes.
            let styleText = nsStyleTextAdapter::of(mappedFlow.mStartFrame.0);
            let wordBreak = styleText.Style().get_inherited_text().word_break;
            match wordBreak {
                mrt_style::style::values::specified::text::WordBreak::BreakAll => self.mLineBreaker.SetWordBreak(WordBreakRule::BreakAll),
                mrt_style::style::values::specified::text::WordBreak::KeepAll => self.mLineBreaker.SetWordBreak(WordBreakRule::KeepAll),
                _ => self.mLineBreaker.SetWordBreak(WordBreakRule::Normal),
            }
            // switch (styleText->mLineBreak) { ... }: `line-break` is not in this Stylo; auto.
            self.mLineBreaker.SetStrictness(LineBreakRule::Auto);

            let offset = iter.GetSkippedOffset();
            let mut iterNext = iter;
            iterNext.AdvanceOriginal(mappedFlow.GetContentEnd() - mappedFlow.mStartFrame.GetContentOffset());

            self.mBreakSinks.push(Box::new(BuildTextRunsScanner_BreakSink::new(aTextRun.clone(), offset)));
            let breakSink: *mut BuildTextRunsScanner_BreakSink = &mut **self.mBreakSinks.last_mut().unwrap();

            let length = iterNext.GetSkippedOffset() - offset;
            let mut flags: u32 = 0;
            let initialBreakController = mappedFlow.mAncestorControllingInitialBreak.unwrap_or(self.mLineContainer);
            if !nsStyleTextAdapter::of(initialBreakController).WhiteSpaceCanWrap(initialBreakController) {
                flags |= nsLineBreaker::BREAK_SUPPRESS_INITIAL;
            }
            let startFrame = mappedFlow.mStartFrame;
            let textStyle = nsStyleTextAdapter::of(startFrame.0);
            if !textStyle.WhiteSpaceCanWrap(startFrame.0) {
                flags |= nsLineBreaker::BREAK_SUPPRESS_INSIDE;
            }
            if aTextRun.GetFlags2().0 & nsTextFrameUtils_Flags::NoBreaks.0 != 0 {
                flags |= nsLineBreaker::BREAK_SKIP_SETTING_NO_BREAKS;
            }
            if textStyle.Style().get_inherited_text().text_transform.case() == mrt_style::style::values::specified::text::TextTransformCase::Capitalize {
                flags |= nsLineBreaker::BREAK_NEED_CAPITALIZATION;
            }
            // if (textStyle->mHyphens == Auto && mLineBreak != Anywhere) flags |= BREAK_USE_AUTO_HYPHENATION:
            // NOT PORTED: hyphenation (`hyphens` is manual here)

            if HasCompressedLeadingWhitespace(startFrame, &textStyle, mappedFlow.GetContentEnd(), &iter) {
                self.mLineBreaker.AppendInvisibleWhitespace(flags);
            }

            if length > 0 {
                let sink: nsILineBreakSinkPtr = if self.mSkipIncompleteTextRuns { None } else { Some(breakSink as *mut dyn nsILineBreakSink) };
                match aTextPtr {
                    TextRunBuffer::Wide(text) => {
                        self.mLineBreaker.AppendText__nsAtom_ptr_char16_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(hyphenationLanguage, &text[offset as usize..], length, flags, sink);
                    }
                    TextRunBuffer::Narrow(text) => {
                        self.mLineBreaker.AppendText__nsAtom_ptr_uint8_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(hyphenationLanguage, &text[offset as usize..], length, flags, sink);
                    }
                }
            }

            iter = iterNext;
        }
    }

    // Gecko: layout/generic/nsTextFrame.cpp MayCharacterHaveEmphasisMark, BuildTextRunsScanner::SetupTextEmphasisForTextRun
    // NOT PORTED: text-emphasis (not in the servo configuration of Stylo; nsTextFrame.cpp lines
    // 2999-3073 at GECKO_REV).
    pub fn SetupTextEmphasisForTextRun(&mut self, aTextRun: &RefPtr_gfxTextRun, aTextPtr: &TextRunBuffer) {
        unimplemented!("NOT PORTED: text-emphasis")
    }
}

// Gecko: layout/generic/nsTextFrame.cpp FindFlowForContent
#[inline]
fn FindFlowForContent(aUserData: &TextRunUserData, aContent: &Option<nsIContent>) -> Option<TextRunMappedFlow> {
    // Find the flow that contains us
    let mut i: i32 = aUserData.mLastFlowIndex as i32;
    let mut delta: i32 = 1;
    let mut sign: i32 = 1;
    // Search starting at the current position and examine close-by
    // positions first, moving further and further away as we go.
    while i >= 0 && (i as u32) < aUserData.mMappedFlows.len() as u32 {
        let flow = &aUserData.mMappedFlows[i as usize];
        if nsIContent::IsSameNode(&flow.mStartFrame.GetContent(), aContent) {
            return Some(*flow);
        }

        i += delta;
        sign = -sign;
        delta = -delta + sign;
    }

    // We ran into an array edge.  Add |delta| to |i| once more to get
    // back to the side where we still need to search, then step in
    // the |sign| direction.
    i += delta;
    if sign > 0 {
        while i < aUserData.mMappedFlows.len() as i32 {
            let flow = &aUserData.mMappedFlows[i as usize];
            if nsIContent::IsSameNode(&flow.mStartFrame.GetContent(), aContent) {
                return Some(*flow);
            }
            i += 1;
        }
    } else {
        while i >= 0 {
            let flow = &aUserData.mMappedFlows[i as usize];
            if nsIContent::IsSameNode(&flow.mStartFrame.GetContent(), aContent) {
                return Some(*flow);
            }
            i -= 1;
        }
    }

    None
}

impl BuildTextRunsScanner {
    // Gecko: layout/generic/nsTextFrame.cpp BuildTextRunsScanner::AssignTextRun
    pub fn AssignTextRun(&mut self, aTextRun: &RefPtr_gfxTextRun, aInflation: f32) {
        for i in 0..self.mMappedFlows.len() {
            let mappedFlow = self.mMappedFlows[i];
            let startFrame = mappedFlow.mStartFrame;
            let endFrame = mappedFlow.mEndFrame;
            let mut f = Some(startFrame);
            while f != endFrame {
                let fr = f.unwrap();
                let oldTextRun = fr.GetTextRun(self.mWhichTextRun);
                if let Some(oldTextRun) = oldTextRun {
                    let firstFrame: nsTextFrame;
                    let mut startOffset: i32 = 0;
                    if oldTextRun.GetFlags2().0 & nsTextFrameUtils_Flags::IsSimpleFlow.0 != 0 {
                        firstFrame = GetFrameForSimpleFlow(&oldTextRun);
                    } else {
                        let userData = GetTextRunUserData(&oldTextRun);
                        let userMappedFlows = &userData.mMappedFlows;
                        firstFrame = userMappedFlows[0].mStartFrame;
                        if fr != firstFrame {
                            let flow = FindFlowForContent(&userData, &fr.GetContent());
                            if let Some(flow) = flow {
                                startOffset = flow.mDOMOffsetToBeforeTransformOffset;
                            } else {
                                NS_ERROR!("Can't find flow containing frame 'f'");
                            }
                        }
                    }

                    // Optimization: if |f| is the first frame in the flow then there are no
                    // prev-continuations that use |oldTextRun|.
                    let mut clearFrom: Option<nsTextFrame> = None;
                    if fr != firstFrame {
                        // If all the frames in the mapped flow starting at |f| (inclusive)
                        // are empty then we let the prev-continuations keep the old text run.
                        let mut iter = gfxSkipCharsIterator::new(oldTextRun.GetSkipChars(), startOffset, fr.GetContentOffset());
                        let textRunOffset = iter.ConvertOriginalToSkipped(fr.GetContentOffset());
                        clearFrom = if textRunOffset == oldTextRun.GetLength() { Some(fr) } else { None };
                    }
                    fr.ClearTextRun(clearFrom, self.mWhichTextRun);

                    #[cfg(debug_assertions)]
                    {
                        if !firstFrame.GetTextRun(self.mWhichTextRun).is_some() {
                            // oldTextRun was destroyed - assert that we don't reference it.
                            for j in 0..self.mBreakSinks.len() {
                                NS_ASSERTION!(!Rc::ptr_eq(&oldTextRun, &self.mBreakSinks[j].mTextRun), "destroyed text run is still in use");
                            }
                        }
                    }
                }
                fr.SetTextRun(aTextRun.clone(), self.mWhichTextRun, aInflation);
                f = fr.GetNextContinuation().map(nsTextFrame::from_frame);
            }
            // Set this bit now; we can't set it any earlier because
            // f->ClearTextRun() might clear it out.
            let whichTextRunState = if startFrame.GetTextRun(nsTextFrame_TextRunType::eInflated).map_or(false, |r| Rc::ptr_eq(&r, aTextRun)) { TEXT_IN_TEXTRUN_USER_DATA } else { TEXT_IN_UNINFLATED_TEXTRUN_USER_DATA };
            startFrame.AddStateBits(whichTextRunState);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// nsTextFrame
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsTextFrame.h nsTextFrame
// ADAPTER: a typed handle for `nsTextFrame*` (and `nsContinuingTextFrame*`, which is the same class
// with `mIsContinuing` set); see the top of the file.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct nsTextFrame(pub nsIFrame);

impl Deref for nsTextFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

impl DerefMut for nsTextFrame {
    fn deref_mut(&mut self) -> &mut nsIFrame {
        &mut self.0
    }
}

// Flags used to track whether certain properties are present.
// Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyFlags
// (the flags are the `Option`s of the frame's data below, so the enum is not needed)

// The members of nsTextFrame (nsTextFrame.h `protected:` section) and of nsContinuingTextFrame.
// The frame properties nsTextFrame.cpp declares (UninflatedTextRunProperty, FontSizeInflationProperty,
// HangableWhitespaceProperty, TrimmableWhitespaceProperty, TabWidthProperty,
// JustificationAssignmentProperty) are real frame properties, as in Gecko, below.
// Gecko: layout/generic/nsTextFrame.h nsTextFrame (the members)
#[derive(Clone, Default)]
pub struct nsTextFrame_Data {
    // mutable RefPtr<nsFontMetrics> mFontMetrics;
    // ADAPTER: shared with the other frames of the same font (InternFontMetrics), as the RefPtr in
    // Gecko points into nsFontCache.
    pub mFontMetrics: Option<Rc<nsFontMetrics>>,
    // RefPtr<gfxTextRun> mTextRun;
    pub mTextRun: Option<RefPtr_gfxTextRun>,
    // The key invariant here is that mContentOffset never decreases along
    // a next-continuation chain. And of course mContentOffset is always <= the
    // the text node's content length, and the mContentOffset for the first frame
    // is always 0. Furthermore the text mapped by a frame is determined by
    // GetContentOffset() and GetContentLength()/GetContentEnd(), which get
    // the length from the difference between this frame's offset and the next
    // frame's offset, or the text length if there is no next frame. This means
    // the frames always map the text node without overlapping or leaving any
    // gaps.
    pub mContentOffset: i32,
    // This does *not* indicate the length of text currently mapped by the frame;
    // instead it's a hint saying that this frame *wants* to map this much text
    // so if we create a new continuation, this is where that continuation should
    // start.
    pub mContentLengthHint: i32,
    pub mAscent: nscoord,
    pub mReflowRequestedForCharDataChange: bool,
    // nsContinuingTextFrame
    pub mIsContinuing: bool,
}

// ADAPTER: the members of all text frames of this thread, in a dense array: `mSlots` is indexed by the
// id of the frame and holds the index into `mData` plus one (0: the frame has no record yet). A
// record is made for a frame when it is first used. In Gecko the members are part of the frame
// object; here only text frames pay for them, and a record is 32 bytes.
#[derive(Default)]
pub struct nsTextFrame_Table {
    pub mSlots: Vec<u32>,
    pub mData: Vec<nsTextFrame_Data>,
}

thread_local! {
    static TEXT_FRAMES: RefCell<nsTextFrame_Table> = RefCell::new(nsTextFrame_Table::default());
    // ADAPTER: the font metrics the text frames hold, one for each distinct font (see InternFontMetrics).
    static TEXT_FONT_METRICS: RefCell<Vec<Rc<nsFontMetrics>>> = RefCell::new(Vec::new());
}

impl nsTextFrame {
    // ADAPTER: drops the text frames' table and the font metrics they held (stage 4; see
    // DestroyFrameTree in driver.rs).
    pub fn ResetTextFrameTable() {
        TEXT_FRAMES.with(|t| *t.borrow_mut() = nsTextFrame_Table::default());
        TEXT_FONT_METRICS.with(|m| *m.borrow_mut() = Vec::new());
    }

    // ADAPTER: forgets the members of these frames, whose slots are about to be reused (see
    // ReleaseFrameSlots in driver.rs): a new frame in the slot starts from a fresh record.
    pub fn ReleaseTextFrameData(aFrames: &[nsIFrame]) {
        TEXT_FRAMES.with(|t| {
            let mut t = t.borrow_mut();
            for f in aFrames {
                let id = f.0 as usize;
                if id < t.mSlots.len() && t.mSlots[id] != 0 {
                    let slot = t.mSlots[id] as usize - 1;
                    t.mData[slot] = nsTextFrame_Data::default();
                }
            }
        });
    }

    // ADAPTER: gives back the spare capacity of the text frames' table (see ShrinkFrameStorage in driver.rs).
    pub fn ShrinkTextFrameTable() {
        TEXT_FRAMES.with(|t| {
            let mut t = t.borrow_mut();
            // (the slots of the frames made after the last text frame are not needed)
            while t.mSlots.last() == Some(&0) {
                t.mSlots.pop();
            }
            t.mSlots.shrink_to_fit();
            t.mData.shrink_to_fit();
        });
    }
}

// ADAPTER: the metrics of aMetrics's font as the one `Rc` shared by every frame that has them.
fn InternFontMetrics(aMetrics: &nsFontMetrics) -> Rc<nsFontMetrics> {
    TEXT_FONT_METRICS.with(|t| {
        let mut t = t.borrow_mut();
        if let Some(found) = t.iter().find(|m| m.IsSameAs(aMetrics)) {
            return found.clone();
        }
        let shared = Rc::new(aMetrics.clone());
        t.push(shared.clone());
        shared
    })
}

// Gecko: layout/generic/nsTextFrame.cpp NS_DECLARE_FRAME_PROPERTY_RELEASABLE(UninflatedTextRunProperty, gfxTextRun)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(UninflatedTextRunProperty, RefPtr_gfxTextRun, 121);
// Gecko: layout/generic/nsTextFrame.cpp NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE(FontSizeInflationProperty, float)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(FontSizeInflationProperty, f32, 122);
// Gecko: layout/generic/nsTextFrame.cpp NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE(HangableWhitespaceProperty, nscoord)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(HangableWhitespaceProperty, nscoord, 123);
// Gecko: layout/generic/nsTextFrame.cpp NS_DECLARE_FRAME_PROPERTY_DELETABLE(TrimmableWhitespaceProperty, gfxTextRun::TrimmableWS)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(TrimmableWhitespaceProperty, gfxTextRun_TrimmableWS, 124);
// Gecko: layout/generic/nsTextFrame.cpp NS_DECLARE_FRAME_PROPERTY_DELETABLE(TabWidthProperty, nsTextFrame::TabWidthStore)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(TabWidthProperty, Rc<RefCell<nsTextFrame_TabWidthStore>>, 125);
// Gecko: layout/generic/nsTextFrame.cpp NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE(JustificationAssignmentProperty, JustificationAssignment)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(JustificationAssignmentProperty, JustificationAssignment, 126);

impl nsTextFrame {
    // static_cast<nsTextFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsTextFrame {
        nsTextFrame(aFrame)
    }

    // do_QueryFrame<nsTextFrame>(frame): the type Text.
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsTextFrame> {
        if aFrame.Type() == LayoutFrameType::Text {
            Some(nsTextFrame(aFrame))
        } else {
            None
        }
    }

    // ADAPTER: runs `f` on the members of this frame (see nsTextFrame_Data). `f` must not call
    // anything that reaches the table again.
    fn with_data<R>(&self, f: impl FnOnce(&mut nsTextFrame_Data) -> R) -> R {
        TEXT_FRAMES.with(|t| {
            let mut t = t.borrow_mut();
            let id = (self.0).0 as usize;
            if t.mSlots.len() <= id {
                t.mSlots.resize(id + 1, 0);
            }
            if t.mSlots[id] == 0 {
                t.mData.push(nsTextFrame_Data::default());
                t.mSlots[id] = t.mData.len() as u32;
            }
            let slot = t.mSlots[id] as usize - 1;
            f(&mut t.mData[slot])
        })
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::CharacterDataBuffer
    // Returns this text frame's content's text fragment.
    //
    // Assertions in Init() ensure we only ever get a Text node as content.
    pub fn CharacterDataBuffer(&self) -> Rc<CharacterDataBuffer> {
        self.GetContent().expect("a text frame without content").mCharacterDataBuffer().expect("a text frame whose content is not text")
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::GetContentOffset
    pub fn GetContentOffset(&self) -> i32 {
        self.with_data(|d| d.mContentOffset)
    }
    // `f->mContentOffset = x`
    pub fn set_mContentOffset(&self, aOffset: i32) {
        self.with_data(|d| d.mContentOffset = aOffset)
    }
    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::GetContentLength
    pub fn GetContentLength(&self) -> i32 {
        NS_ASSERTION!(self.GetContentEnd() - self.GetContentOffset() >= 0, "negative length");
        self.GetContentEnd() - self.GetContentOffset()
    }
    // This returns the length the frame thinks it *should* have after it was
    // last reflowed (0 if it hasn't been reflowed yet). This should be used only
    // when setting up the text offsets for a new continuation frame.
    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::GetContentLengthHint
    pub fn GetContentLengthHint(&self) -> i32 {
        self.with_data(|d| d.mContentLengthHint)
    }
    // `mAscent`
    pub fn mAscent(&self) -> nscoord {
        self.with_data(|d| d.mAscent)
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::IsInTextRunUserData
    pub fn IsInTextRunUserData(&self) -> bool {
        self.HasAnyStateBits(TEXT_IN_TEXTRUN_USER_DATA | TEXT_IN_UNINFLATED_TEXTRUN_USER_DATA)
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::GetTextRun
    pub fn GetTextRun(&self, aWhichTextRun: nsTextFrame_TextRunType) -> Option<RefPtr_gfxTextRun> {
        if aWhichTextRun == nsTextFrame_TextRunType::eInflated || !self.HasFontSizeInflation() {
            return self.with_data(|d| d.mTextRun.clone());
        }
        self.GetUninflatedTextRun()
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetUninflatedTextRun
    pub fn GetUninflatedTextRun(&self) -> Option<RefPtr_gfxTextRun> {
        self.GetProperty(UninflatedTextRunProperty())
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::SetTextRun
    pub fn SetTextRun(&self, aTextRun: RefPtr_gfxTextRun, aWhichTextRun: nsTextFrame_TextRunType, aInflation: f32) {
        // Our inflated text run is always stored in mTextRun.  In the cases
        // where our current inflation is not 1.0, however, we store two text
        // runs, and the uninflated one goes in a frame property.  We never
        // store a single text run in both.
        if aWhichTextRun == nsTextFrame_TextRunType::eInflated {
            if self.HasFontSizeInflation() && aInflation == 1.0 {
                // FIXME: Probably shouldn't do this within each SetTextRun
                // method, but it doesn't hurt.
                self.ClearTextRun(None, nsTextFrame_TextRunType::eNotInflated);
            }
            self.SetFontSizeInflation(aInflation);
        } else {
            MOZ_ASSERT!(aInflation == 1.0, "unexpected inflation");
            if self.HasFontSizeInflation() {
                self.SetProperty(UninflatedTextRunProperty(), aTextRun);
                return;
            }
            // fall through to setting mTextRun
        }

        self.with_data(|d| d.mTextRun = Some(aTextRun));

        // FIXME: Add assertions testing the relationship between
        // GetFontSizeInflation() and whether we have an uninflated text run
        // (but be aware that text runs can go away).
    }

    // Notify the frame that it should drop its pointer to a text run.
    // Returns whether the text run was removed (i.e., whether it was
    // associated with this frame, either as its inflated or non-inflated
    // text run.
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::RemoveTextRun
    pub fn RemoveTextRun(&self, aTextRun: &RefPtr_gfxTextRun) -> bool {
        let isTextRun = self.with_data(|d| d.mTextRun.as_ref().map_or(false, |r| Rc::ptr_eq(r, aTextRun)));
        if isTextRun {
            self.with_data(|d| {
                d.mTextRun = None;
                d.mFontMetrics = None;
            });
            return true;
        }
        if self.HasAnyStateBits(TEXT_HAS_FONT_INFLATION) && self.GetUninflatedTextRun().map_or(false, |r| Rc::ptr_eq(&r, aTextRun)) {
            self.RemoveProperty(UninflatedTextRunProperty());
            return true;
        }
        false
    }

    // Clears out |mTextRun| (or the uninflated text run, when aInflated
    // is nsTextFrame::eNotInflated and there is inflation) from all frames that
    // hold a reference to it, starting at |aStartContinuation|, or if it's
    // nullptr, starting at |this|.  Deletes the text run if all references
    // were cleared and it's not cached.
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ClearTextRun
    pub fn ClearTextRun(&self, aStartContinuation: Option<nsTextFrame>, aWhichTextRun: nsTextFrame_TextRunType) {
        let Some(textRun) = self.GetTextRun(aWhichTextRun) else {
            return;
        };

        if aWhichTextRun == nsTextFrame_TextRunType::eInflated {
            self.with_data(|d| d.mFontMetrics = None);
        }

        let checkmTextrun = self.with_data(|d| d.mTextRun.as_ref().map_or(false, |r| Rc::ptr_eq(r, &textRun)));
        UnhookTextRunFromFrames(&textRun, aStartContinuation);
        let _ = checkmTextrun;
        MOZ_ASSERT!(if checkmTextrun { self.with_data(|d| d.mTextRun.is_none()) } else { self.GetUninflatedTextRun().is_none() });
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::ClearTextRuns
    pub fn ClearTextRuns(&self) {
        self.ClearTextRun(None, nsTextFrame_TextRunType::eInflated);
        if self.HasFontSizeInflation() {
            self.ClearTextRun(None, nsTextFrame_TextRunType::eNotInflated);
        }
    }

    // Wipe out references to textrun(s) without deleting the textruns.
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::DisconnectTextRuns
    pub fn DisconnectTextRuns(&self) {
        MOZ_ASSERT!(!self.IsInTextRunUserData(), "Textrun mentions this frame in its user data so we can't just disconnect");
        self.with_data(|d| d.mTextRun = None);
        if self.HasAnyStateBits(TEXT_HAS_FONT_INFLATION) {
            self.RemoveProperty(UninflatedTextRunProperty());
        }
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::HasFontSizeInflation
    pub fn HasFontSizeInflation(&self) -> bool {
        self.HasAnyStateBits(TEXT_HAS_FONT_INFLATION)
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetFontSizeInflation
    pub fn GetFontSizeInflation(&self) -> f32 {
        if !self.HasFontSizeInflation() {
            return 1.0;
        }
        self.GetProperty(FontSizeInflationProperty()).unwrap_or(1.0)
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::SetFontSizeInflation
    pub fn SetFontSizeInflation(&self, aInflation: f32) {
        if aInflation == 1.0 {
            if self.HasFontSizeInflation() {
                self.RemoveStateBits(TEXT_HAS_FONT_INFLATION);
                self.RemoveProperty(FontSizeInflationProperty());
            }
            return;
        }

        self.AddStateBits(TEXT_HAS_FONT_INFLATION);
        self.SetProperty(FontSizeInflationProperty(), aInflation);
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::IsCurrentFontInflation
    pub fn IsCurrentFontInflation(&self, aInflation: f32) -> bool {
        (aInflation - self.GetFontSizeInflation()).abs() < 1e-6
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::SetHangableISize
    pub fn SetHangableISize(&self, aISize: nscoord) {
        MOZ_ASSERT!(aISize >= 0, "unexpected negative hangable advance");
        if aISize <= 0 {
            self.ClearHangableISize();
            return;
        }
        self.SetProperty(HangableWhitespaceProperty(), aISize);
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetHangableISize
    pub fn GetHangableISize(&self) -> nscoord {
        self.GetProperty(HangableWhitespaceProperty()).unwrap_or(0)
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ClearHangableISize
    pub fn ClearHangableISize(&self) {
        self.RemoveProperty(HangableWhitespaceProperty());
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::SetTrimmableWS
    pub fn SetTrimmableWS(&self, aTrimmableWS: gfxTextRun_TrimmableWS) {
        MOZ_ASSERT!(aTrimmableWS.mAdvance >= 0.0, "negative trimmable size");
        if aTrimmableWS.mAdvance <= 0.0 {
            self.ClearTrimmableWS();
            return;
        }
        self.SetProperty(TrimmableWhitespaceProperty(), aTrimmableWS);
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetTrimmableWS
    pub fn GetTrimmableWS(&self) -> gfxTextRun_TrimmableWS {
        self.GetProperty(TrimmableWhitespaceProperty()).unwrap_or_default()
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ClearTrimmableWS
    pub fn ClearTrimmableWS(&self) {
        self.RemoveProperty(TrimmableWhitespaceProperty());
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::AssignJustificationGaps
    pub fn AssignJustificationGaps(&self, aAssign: JustificationAssignment) {
        self.SetProperty(JustificationAssignmentProperty(), aAssign);
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetJustificationAssignment
    pub fn GetJustificationAssignment(&self) -> JustificationAssignment {
        self.GetProperty(JustificationAssignmentProperty()).unwrap_or_default()
    }
}

// Gecko: layout/generic/nsTextFrame.cpp NS_NewTextFrame
// ADAPTER: frames are made by whoever builds the frame tree (the frame constructor, a test) with
// FrameTree::AddFrame; this gives one the class. `aContent` is the text node it is for.
pub fn NS_NewTextFrame(aFrame: nsIFrame, aContent: nsIContent) -> nsTextFrame {
    aFrame.SetType(LayoutFrameType::Text);
    let frame = nsTextFrame(aFrame);
    frame.Init(Some(aContent), aFrame.GetParent().unwrap_or(aFrame), None);
    frame
}

// Gecko: layout/generic/nsTextFrame.cpp NS_NewContinuingTextFrame
// ADAPTER: see NS_NewTextFrame. The caller then calls nsContinuingTextFrame::Init.
pub fn NS_NewContinuingTextFrame(aFrame: nsIFrame) -> nsTextFrame {
    aFrame.SetType(LayoutFrameType::Text);
    let frame = nsTextFrame(aFrame);
    frame.with_data(|d| d.mIsContinuing = true);
    frame
}

impl nsTextFrame {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::Init
    pub fn Init(&self, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: Option<nsIFrame>) {
        NS_ASSERTION!(aPrevInFlow.is_none(), "Can't be a continuation!");
        MOZ_ASSERT!(aContent.as_ref().map_or(false, |c| c.IsText()), "Bogus content!");

        // Remove any NewlineOffsetProperty or InFlowContentLengthProperty since they
        // might be invalid if the content was modified while there was no frame
        // (NewlineOffsetProperty: NOT PORTED, it is not cached on the content)
        let content = aContent.unwrap();
        if content.HasFlowLengthProperty() {
            content.RemoveFlowLengthProperty();
        }

        // Since our content has a frame now, this flag is no longer needed.
        content.UnsetFlags(NS_CREATE_FRAME_IF_NON_WHITESPACE);

        // We're not a continuing frame.
        // mContentOffset = 0; not necessary since we get zeroed out at init
        // nsIFrame::Init(aContent, aParent, aPrevInFlow)
        self.0.SetContent(aContent);
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ClearFrameOffsetCache
    // NOT PORTED: the offset-to-frame cache (selection); TEXT_IN_OFFSET_CACHE is never set.

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::Destroy
    // (the virtual call is a `match` on the frame type in nsContainerFrame.rs's Destroy)
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        if self.with_data(|d| d.mIsContinuing) {
            return self.nsContinuingTextFrame_Destroy(aContext);
        }
        // ClearFrameOffsetCache(): NOT PORTED
        // We might want to clear NS_CREATE_FRAME_IF_NON_WHITESPACE or
        // NS_REFRAME_IF_WHITESPACE on mContent here, since our parent frame
        // type might be changing.  Not clear whether it's worth it.
        self.ClearTextRuns();
        if let Some(next) = self.GetNextContinuation() {
            next.SetPrevInFlow(None);
        }
        // Let the base class destroy the frame
        self.nsIFrame_Destroy(aContext);
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetContinuations
    // NOT PORTED: the cached continuations array (selection offset lookup).

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::FirstContinuation (nsContinuingTextFrame
    // caches it in mFirstContinuation; here it is found by walking back)
    pub fn FirstContinuation(&self) -> nsTextFrame {
        let mut first = *self;
        while let Some(prev) = first.GetPrevContinuation() {
            first = nsTextFrame(prev);
        }
        first
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::GetPrevContinuation (nsTextFrame: null;
    // nsContinuingTextFrame: mPrevContinuation)
    pub fn GetPrevContinuation(&self) -> Option<nsIFrame> {
        self.0.GetPrevContinuation()
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::LastInFlow
    pub fn LastInFlow(&self) -> nsTextFrame {
        let mut lastInFlow = *self;
        while let Some(next) = lastInFlow.GetNextInFlow() {
            lastInFlow = nsTextFrame(next);
        }
        lastInFlow
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::LastContinuation
    pub fn LastContinuation(&self) -> nsTextFrame {
        let mut lastContinuation = *self;
        while let Some(next) = lastContinuation.GetNextContinuation() {
            lastContinuation = nsTextFrame(next);
        }
        lastContinuation
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ShouldSuppressLineBreak
    pub fn ShouldSuppressLineBreak(&self) -> bool {
        // If the parent frame of the text frame is ruby content box, it must
        // suppress line break inside. This check is necessary, because when
        // a whitespace is only contained by pseudo ruby frames, its style
        // context won't have SuppressLineBreak bit set.
        // (mozilla::RubyUtils::IsRubyContentBox(GetParent()->Type()): NOT PORTED, ruby)
        self.Style().ShouldSuppressLineBreak()
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::InvalidateFrame, InvalidateFrameWithRect,
    // InvalidateSelectionState
    // NOT PORTED: invalidation (paint) and selection.

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::NotifyNativeAnonymousTextnodeChange,
    // CharacterDataChanged
    // NOT PORTED: DOM text changes (the DOM is built once); nsTextFrame.cpp lines 5080-5206 at GECKO_REV.
}

// Gecko: layout/generic/nsTextFrame.h nsContinuingTextFrame (same struct; see the top of the file)
pub struct nsContinuingTextFrame;

impl nsContinuingTextFrame {
    // Gecko: layout/generic/nsTextFrame.cpp nsContinuingTextFrame::Init
    pub fn Init(aFrame: nsTextFrame, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: nsIFrame) {
        // Hook the frame into the flow
        let prev = nsTextFrame(aPrevInFlow);
        let nextContinuation = prev.GetNextContinuation();
        aFrame.SetPrevInFlow(Some(aPrevInFlow));
        aPrevInFlow.SetNextInFlow(Some(aFrame.0));
        aFrame.with_data(|d| d.mIsContinuing = true);

        // NOTE: bypassing nsTextFrame::Init!!!
        aFrame.0.SetContent(aContent.clone());

        aFrame.set_mContentOffset(prev.GetContentOffset() + prev.GetContentLengthHint());
        NS_ASSERTION!(aFrame.GetContentOffset() < aContent.as_ref().unwrap().TextLength() as i32, "Creating ContinuingTextFrame, but there is no more content");
        if !mrt_style::style::servo_arc::Arc::ptr_eq(&prev.Style(), &aFrame.Style()) {
            // We're taking part of prev's text, and its style may be different
            // so clear its textrun which may no longer be valid (and don't set ours)
            prev.ClearTextRuns();
        } else {
            let inflation = prev.GetFontSizeInflation();
            aFrame.SetFontSizeInflation(inflation);
            let prevRun = prev.GetTextRun(nsTextFrame_TextRunType::eInflated);
            aFrame.with_data(|d| d.mTextRun = prevRun);
            if inflation != 1.0 {
                if let Some(uninflatedTextRun) = prev.GetTextRun(nsTextFrame_TextRunType::eNotInflated) {
                    aFrame.SetTextRun(uninflatedTextRun, nsTextFrame_TextRunType::eNotInflated, 1.0);
                }
            }
        }
        if aPrevInFlow.HasAnyStateBits(NS_FRAME_IS_BIDI) {
            let mut bidiData = aPrevInFlow.GetBidiData();
            bidiData.precedingControl = kBidiLevelNone;
            aFrame.SetProperty(nsIFrame::BidiDataProperty(), bidiData);

            if let Some(nextContinuation) = nextContinuation {
                aFrame.SetNextContinuation(Some(nextContinuation));
                nextContinuation.SetPrevContinuation(Some(aFrame.0));
                // Adjust next-continuations' content offset as needed.
                let mut nc = Some(nsTextFrame(nextContinuation));
                while let Some(n) = nc {
                    if !(n.GetContentOffset() < aFrame.GetContentOffset()) {
                        break;
                    }
                    let nextBidiData = n.GetBidiData();
                    NS_ASSERTION!(bidiData.embeddingLevel == nextBidiData.embeddingLevel && bidiData.baseLevel == nextBidiData.baseLevel, "stealing text from different type of BIDI continuation");
                    MOZ_ASSERT!(nextBidiData.precedingControl == kBidiLevelNone, "There shouldn't be any virtual bidi formatting character between continuations");
                    n.set_mContentOffset(aFrame.GetContentOffset());
                    nc = n.GetNextContinuation().map(nsTextFrame);
                }
            }
            aFrame.AddStateBits(NS_FRAME_IS_BIDI);
        } // prev frame is bidi
        let _ = aParent;
    }
}

impl nsTextFrame {
    // Gecko: layout/generic/nsTextFrame.cpp nsContinuingTextFrame::Destroy
    fn nsContinuingTextFrame_Destroy(&self, aContext: &mut FrameDestroyContext) {
        // ClearFrameOffsetCache(): NOT PORTED

        // The text associated with this frame will become associated with our
        // prev-continuation. If that means the text has changed style, then
        // we need to wipe out the text run for the text.
        // Note that mPrevContinuation can be null if we're destroying the whole
        // frame chain from the start to the end.
        // If this frame is mentioned in the userData for a textrun (say
        // because there's a direction change at the start of this frame), then
        // we have to clear the textrun because we're going away and the
        // textrun had better not keep a dangling reference to us.
        let prev = self.GetPrevContinuation().map(nsTextFrame);
        if self.IsInTextRunUserData() || prev.map_or(false, |p| !mrt_style::style::servo_arc::Arc::ptr_eq(&p.Style(), &self.Style())) {
            self.ClearTextRuns();
            // Clear the previous continuation's text run also, so that it can rebuild
            // the text run to include our text.
            if let Some(p) = prev {
                p.ClearTextRuns();
            }
        }
        nsIFrame::RemoveFromFlow(self.0);
        // Let the base class destroy the frame
        self.nsIFrame_Destroy(aContext);
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsContinuingTextFrame::FirstInFlow
    pub fn FirstInFlow(&self) -> nsIFrame {
        // Can't cast to |nsContinuingTextFrame*| because the first one isn't.
        let mut firstInFlow;
        let mut previous = self.0;
        loop {
            firstInFlow = previous;
            match firstInFlow.GetPrevInFlow() {
                Some(p) => previous = p,
                None => break,
            }
        }
        firstInFlow
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        self.0.IntrinsicISizeFromInline(aInput, aType)
    }
}

// ---------------------------------------------------------------------------------------------
// EnsureTextRun
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::EnsureTextRun
// Acquires the text run for this content, if necessary.
// @param aWhichTextRun indicates whether to get an inflated or non-inflated
// text run
// @param aLineContainer the block ancestor for this frame, or nullptr if
// unknown
// @param aFlowEndInTextRun if non-null, this returns the textrun offset of
// end of the text associated with this frame and its in-flow siblings
// @return a gfxSkipCharsIterator set up to map DOM offsets for this frame
// to offsets into the textrun; its initial offset is set to this frame's
// content offset
//
// (The DrawTarget parameter is dropped: there is no drawing.) Rust: the returned iterator
// borrows the skip chars of the text run, which the frame (or the caller) keeps alive; the text
// run's skip chars are never changed after it is made, so the borrow is extended to 'static
// (a text run is only dropped when its last frame lets go of it, which happens in reflow, where
// Gecko's iterators are invalid in the same way).
impl nsTextFrame {
    pub fn EnsureTextRun(&self, aWhichTextRun: nsTextFrame_TextRunType, aLineContainer: Option<nsIFrame>, aLine: Option<nsLineList_iterator>, aFlowEndInTextRun: Option<&mut u32>) -> gfxSkipCharsIterator<'static> {
        let mut textRun = self.GetTextRun(aWhichTextRun);
        if textRun.is_none() || aLine.map_or(false, |l| l.GetInvalidateTextRuns()) {
            BuildTextRuns(*self, aLineContainer, aLine, aWhichTextRun);
            textRun = self.GetTextRun(aWhichTextRun);
            if textRun.is_none() {
                // A text run was not constructed for this frame. This is bad. The caller
                // will check mTextRun.
                return gfxSkipCharsIterator::new__gfxSkipChars_int32_t(EmptySkipChars(), 0);
            }
            let tabWidths = self.GetProperty(TabWidthProperty());
            if let Some(tabWidths) = tabWidths {
                if tabWidths.borrow().mValidForContentOffset != self.GetContentOffset() {
                    self.RemoveProperty(TabWidthProperty());
                }
            }
        }
        let textRun = textRun.unwrap();
        let skipChars: &'static gfxSkipChars = unsafe { &*(textRun.GetSkipChars() as *const gfxSkipChars) };

        if textRun.GetFlags2().0 & nsTextFrameUtils_Flags::IsSimpleFlow.0 != 0 {
            if let Some(aFlowEndInTextRun) = aFlowEndInTextRun {
                *aFlowEndInTextRun = textRun.GetLength();
            }
            return gfxSkipCharsIterator::new(skipChars, 0, self.GetContentOffset());
        }

        let userData = GetTextRunUserData(&textRun);
        let flow = FindFlowForContent(&userData, &self.GetContent());
        if let Some(flow) = flow {
            // Since textruns can only contain one flow for a given content element,
            // this must be our flow.
            let flowIndex = userData.mMappedFlows.iter().position(|f| f.mStartFrame == flow.mStartFrame).unwrap();
            if let gfxTextRun_UserData::Flows(ud) = textRun.borrow_mut().GetUserData_mut() {
                ud.mLastFlowIndex = flowIndex as u32;
            }
            let iter = gfxSkipCharsIterator::new(skipChars, flow.mDOMOffsetToBeforeTransformOffset, self.GetContentOffset());
            if let Some(aFlowEndInTextRun) = aFlowEndInTextRun {
                if flowIndex + 1 < userData.mMappedFlows.len() {
                    let mut end = gfxSkipCharsIterator::new__gfxSkipChars_int32_t(skipChars, 0);
                    let next = &userData.mMappedFlows[flowIndex + 1];
                    *aFlowEndInTextRun = end.ConvertOriginalToSkipped(next.mStartFrame.GetContentOffset() + next.mDOMOffsetToBeforeTransformOffset);
                } else {
                    *aFlowEndInTextRun = textRun.GetLength();
                }
            }
            return iter;
        }

        NS_ERROR!("Can't find flow containing this frame???");
        gfxSkipCharsIterator::new__gfxSkipChars_int32_t(EmptySkipChars(), 0)
    }
}

// Gecko: gfxPlatform::EmptySkipChars
fn EmptySkipChars() -> &'static gfxSkipChars {
    thread_local! {
        static EMPTY: &'static gfxSkipChars = Box::leak(Box::new(gfxSkipChars::new()));
    }
    EMPTY.with(|e| *e)
}

// Gecko: layout/generic/nsTextFrame.cpp GetEndOfTrimmedText
fn GetEndOfTrimmedText(aBuffer: &CharacterDataBuffer, aStyleText: &nsStyleTextAdapter, aStart: u32, aEnd: u32, aIterator: &mut gfxSkipCharsIterator, aAllowHangingWS: bool) -> u32 {
    aIterator.SetSkippedOffset(aEnd);
    while aIterator.GetSkippedOffset() > aStart {
        aIterator.AdvanceSkipped(-1);
        if !IsTrimmableSpace(aBuffer, aIterator.GetOriginalOffset() as u32, aStyleText, aAllowHangingWS) {
            return aIterator.GetSkippedOffset() + 1;
        }
    }
    aStart
}

// Get the DOM content range mapped by this frame after excluding
// whitespace subject to start-of-line and end-of-line trimming.
// The textrun must have been created before calling this.
// Gecko: layout/generic/nsTextFrame.h nsTextFrame::TrimmedOffsets
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct nsTextFrame_TrimmedOffsets {
    pub mStart: i32,
    pub mLength: i32,
}

impl nsTextFrame_TrimmedOffsets {
    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::TrimmedOffsets::GetEnd
    pub fn GetEnd(&self) -> i32 {
        self.mStart + self.mLength
    }
}

// Gecko: layout/generic/nsTextFrame.h nsTextFrame::TrimmedOffsetFlags
MOZ_ENUM_CLASS_BITS! {
    pub struct nsTextFrame_TrimmedOffsetFlags: u8 {
        Default = 0,
        NotPostReflow = 1 << 0,
        NoTrimAfter = 1 << 1,
        NoTrimBefore = 1 << 2,
    }
}

impl nsTextFrame {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetTrimmedOffsets
    pub fn GetTrimmedOffsets(&self, aBuffer: &CharacterDataBuffer, aFlags: nsTextFrame_TrimmedOffsetFlags) -> nsTextFrame_TrimmedOffsets {
        NS_ASSERTION!(self.with_data(|d| d.mTextRun.is_some()), "Need textrun here");
        if !(aFlags & nsTextFrame_TrimmedOffsetFlags::NotPostReflow).to_bool() {
            // This should not be used during reflow. We need our TEXT_REFLOW_FLAGS
            // to be set correctly.  If our parent wasn't reflowed due to the frame
            // tree being too deep then the return value doesn't matter.
            NS_ASSERTION!(!self.HasAnyStateBits(NS_FRAME_FIRST_REFLOW) || self.GetParent().unwrap().HasAnyStateBits(NS_FRAME_TOO_DEEP_IN_FRAME_TREE), "Can only call this on frames that have been reflowed");
            NS_ASSERTION!(!self.HasAnyStateBits(NS_FRAME_IN_REFLOW), "Can only call this on frames that are not being reflowed");
        }

        let mut offsets = nsTextFrame_TrimmedOffsets { mStart: self.GetContentOffset(), mLength: self.GetContentLength() };
        let textStyle = nsStyleTextAdapter::of(self.0);
        // Note that pre-line newlines should still allow us to trim spaces
        // for display
        if textStyle.WhiteSpaceIsSignificant() {
            return offsets;
        }

        if !(aFlags & nsTextFrame_TrimmedOffsetFlags::NoTrimBefore).to_bool() && ((aFlags & nsTextFrame_TrimmedOffsetFlags::NotPostReflow).to_bool() || self.HasAnyStateBits(TEXT_START_OF_LINE)) {
            let whitespaceCount = GetTrimmableWhitespaceCount(aBuffer, offsets.mStart, offsets.mLength, 1) as i32;
            offsets.mStart += whitespaceCount;
            offsets.mLength -= whitespaceCount;
        }

        if !(aFlags & nsTextFrame_TrimmedOffsetFlags::NoTrimAfter).to_bool() && ((aFlags & nsTextFrame_TrimmedOffsetFlags::NotPostReflow).to_bool() || self.HasAnyStateBits(TEXT_END_OF_LINE)) {
            // This treats a trailing 'pre-line' newline as trimmable. That's fine,
            // it's actually what we want since we want whitespace before it to
            // be trimmed.
            let whitespaceCount = GetTrimmableWhitespaceCount(aBuffer, offsets.GetEnd() - 1, offsets.mLength, -1) as i32;
            offsets.mLength -= whitespaceCount;
        }
        offsets
    }
}

// Gecko: layout/generic/nsTextFrame.cpp IsJustifiableCharacter
// NOT PORTED: justification (nsTextFrame.cpp lines 3331-3407 at GECKO_REV).

impl nsTextFrame {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ClearMetrics
    pub fn ClearMetrics(&self, aMetrics: &mut ReflowOutput) {
        aMetrics.ClearSize();
        aMetrics.SetBlockStartAscent(0);
        self.with_data(|d| d.mAscent = 0);

        self.AddStateBits(TEXT_NO_RENDERED_GLYPHS);
    }
}

// Gecko: layout/generic/nsTextFrame.cpp FindChar
fn FindChar(characterDataBuffer: &CharacterDataBuffer, aOffset: i32, aLength: i32, ch: u16) -> i32 {
    if characterDataBuffer.Is2b() {
        let str = &characterDataBuffer.Get2b()[aOffset as usize..];
        for i in 0..aLength {
            if str[i as usize] == ch {
                return i + aOffset;
            }
        }
    } else if ch <= 0xFF {
        let str = &characterDataBuffer.Get1b()[aOffset as usize..];
        for i in 0..aLength {
            if str[i as usize] == ch as u8 {
                return i + aOffset;
            }
        }
    }
    -1
}

// Gecko: layout/generic/nsTextFrame.cpp IsChineseOrJapanese
// NOT PORTED: justification (its only user); the language of the text is not known here.

#[cfg(debug_assertions)]
// Gecko: layout/generic/nsTextFrame.cpp IsInBounds
fn IsInBounds(aStart: &gfxSkipCharsIterator, aContentLength: i32, aRange: gfxTextRun_Range) -> bool {
    if aStart.GetSkippedOffset() > aRange.start {
        return false;
    }
    if aContentLength == i32::MAX {
        return true;
    }
    let mut iter = *aStart;
    iter.AdvanceOriginal(aContentLength);
    iter.GetSkippedOffset() >= aRange.end
}

// ---------------------------------------------------------------------------------------------
// nsTextFrame::PropertyProvider
// ---------------------------------------------------------------------------------------------

/**
 * An implementation of gfxTextRun::PropertyProvider that computes spacing and
 * hyphenation based on CSS properties for a text frame.
 *
 * nsTextFrame normally creates a PropertyProvider as a temporary object on
 * on the stack, but this is not marked MOZ_STACK_CLASS because SVGTextFrame
 * wants to cache an instance across multiple calls using the same textframe.
 */
// Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider
// Rust: the members that C++ declares `mutable` (changed by the const methods) are Cells.
// The iterator `mStart` borrows the text run's skip chars, as EnsureTextRun's does.
pub struct nsTextFrame_PropertyProvider {
    mTextRun: RefPtr_gfxTextRun,
    mFontMetrics: RefCell<Option<nsFontMetrics>>,
    mTextStyle: nsStyleTextAdapter,
    mCharacterDataBuffer: Rc<CharacterDataBuffer>,
    mLineContainer: Option<nsIFrame>,
    mFrame: nsTextFrame,

    // Offset in original and transformed string
    mStart: gfxSkipCharsIterator<'static>,

    mTempIterator: gfxSkipCharsIterator<'static>,

    // Either null, or pointing to the frame's TabWidthProperty.
    mTabWidths: RefCell<Option<Rc<RefCell<nsTextFrame_TabWidthStore>>>>,

    // How far we've done tab-width calculation; this is ONLY valid when
    // mTabWidths is nullptr (otherwise rely on mTabWidths->mLimit instead).
    // It's a DOM offset relative to the current frame's offset.
    mTabWidthsAnalyzedLimit: Cell<u32>,

    // DOM string length, may be INT32_MAX
    mLength: i32,

    // space for each whitespace char
    mWordSpacing: nscoord,

    // space for each letter
    mLetterSpacing: nscoord,

    // If TextAutospace exists, inter-script spacing applies.
    // Maybe<mozilla::TextAutospace> mTextAutospace: NOT PORTED

    // min advance for <tab> char
    mMinTabAdvance: Cell<gfxFloat>,

    mHyphenWidth: Cell<gfxFloat>,
    mOffsetFromBlockOriginForTabs: Cell<gfxFloat>,

    // The values in mJustificationSpacings corresponds to unskipped
    // characters start from mJustificationArrayStart.
    mJustificationArrayStart: u32,
    mJustificationSpacings: Vec<gfxFont_Spacing>,

    mReflowing: bool,
    mWhichTextRun: nsTextFrame_TextRunType,
    mStartOfLineOffset: u32,
}

impl nsTextFrame_PropertyProvider {
    /**
     * Use this constructor for reflow, when we don't know what text is
     * really mapped by the frame and we have a lot of other data around.
     *
     * @param aLength can be INT32_MAX to indicate we cover all the text
     * associated with aFrame up to where its flow chain ends in the given
     * textrun. If INT32_MAX is passed, justification and hyphen-related methods
     * cannot be called, nor can GetOriginalLength().
     */
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::PropertyProvider(gfxTextRun*, const nsStyleText*, const CharacterDataBuffer&, nsTextFrame*, const gfxSkipCharsIterator&, int32_t, nsIFrame*, nscoord, TextRunType, bool)
    pub fn new__gfxTextRun_nsStyleText_CharacterDataBuffer_nsTextFrame_gfxSkipCharsIterator_int32_t_nsIFrame_nscoord_TextRunType_bool(
        aTextRun: RefPtr_gfxTextRun,
        aTextStyle: nsStyleTextAdapter,
        aBuffer: Rc<CharacterDataBuffer>,
        aFrame: nsTextFrame,
        aStart: gfxSkipCharsIterator<'static>,
        aLength: i32,
        aLineContainer: Option<nsIFrame>,
        aOffsetFromBlockOriginForTabs: nscoord,
        aWhichTextRun: nsTextFrame_TextRunType,
        aAtStartOfLine: bool,
    ) -> nsTextFrame_PropertyProvider {
        NS_ASSERTION!(aStart.IsInitialized(), "Start not initialized?");
        let mWordSpacing = ResolveWordSpacing(aFrame.0, &aTextStyle);
        let mLetterSpacing = ResolveLetterSpacing(aFrame.0, &aTextStyle);
        let mut this = nsTextFrame_PropertyProvider {
            mTextRun: aTextRun,
            mFontMetrics: RefCell::new(None),
            mTextStyle: aTextStyle,
            mCharacterDataBuffer: aBuffer,
            mLineContainer: aLineContainer,
            mFrame: aFrame,
            mStart: aStart,
            mTempIterator: aStart,
            mTabWidths: RefCell::new(None),
            mTabWidthsAnalyzedLimit: Cell::new(0),
            mLength: aLength,
            mWordSpacing,
            mLetterSpacing,
            mMinTabAdvance: Cell::new(-1.0),
            mHyphenWidth: Cell::new(-1.0),
            mOffsetFromBlockOriginForTabs: Cell::new(aOffsetFromBlockOriginForTabs as gfxFloat),
            mJustificationArrayStart: 0,
            mJustificationSpacings: Vec::new(),
            mReflowing: true,
            mWhichTextRun: aWhichTextRun,
            mStartOfLineOffset: u32::MAX,
        };
        if aAtStartOfLine {
            this.mStartOfLineOffset = this.mStart.GetSkippedOffset();
        }
        this.InitTextAutospace();
        this
    }

    /**
     * Use this constructor after the frame has been reflowed and we don't
     * have other data around. Gets everything from the frame. EnsureTextRun
     * *must* be called before this!!!
     */
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::PropertyProvider(nsTextFrame*, const gfxSkipCharsIterator&, TextRunType, nsFontMetrics*)
    pub fn new__nsTextFrame_gfxSkipCharsIterator_TextRunType_nsFontMetrics(aFrame: nsTextFrame, aStart: gfxSkipCharsIterator<'static>, aWhichTextRun: nsTextFrame_TextRunType, aFontMetrics: Option<nsFontMetrics>) -> nsTextFrame_PropertyProvider {
        let mTextRun = aFrame.GetTextRun(aWhichTextRun);
        NS_ASSERTION!(mTextRun.is_some(), "Textrun not initialized!");
        let mTextStyle = nsStyleTextAdapter::of(aFrame.0);
        let mWordSpacing = ResolveWordSpacing(aFrame.0, &mTextStyle);
        let mLetterSpacing = ResolveLetterSpacing(aFrame.0, &mTextStyle);
        let mut this = nsTextFrame_PropertyProvider {
            mTextRun: mTextRun.unwrap(),
            mFontMetrics: RefCell::new(aFontMetrics),
            mTextStyle,
            mCharacterDataBuffer: aFrame.CharacterDataBuffer(),
            mLineContainer: None,
            mFrame: aFrame,
            mStart: aStart,
            mTempIterator: aStart,
            mTabWidths: RefCell::new(None),
            mTabWidthsAnalyzedLimit: Cell::new(0),
            mLength: aFrame.GetContentLength(),
            mWordSpacing,
            mLetterSpacing,
            mMinTabAdvance: Cell::new(-1.0),
            mHyphenWidth: Cell::new(-1.0),
            mOffsetFromBlockOriginForTabs: Cell::new(0.0),
            mJustificationArrayStart: 0,
            mJustificationSpacings: Vec::new(),
            mReflowing: false,
            mWhichTextRun: aWhichTextRun,
            mStartOfLineOffset: u32::MAX,
        };
        this.InitTextAutospace();
        this
    }

    /**
     * As above, but assuming we want the inflated text run and associated
     * metrics.
     */
    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::PropertyProvider(nsTextFrame*, const gfxSkipCharsIterator&)
    pub fn new__nsTextFrame_gfxSkipCharsIterator(aFrame: nsTextFrame, aStart: gfxSkipCharsIterator<'static>) -> nsTextFrame_PropertyProvider {
        nsTextFrame_PropertyProvider::new__nsTextFrame_gfxSkipCharsIterator_TextRunType_nsFontMetrics(aFrame, aStart, nsTextFrame_TextRunType::eInflated, Some(aFrame.InflatedFontMetrics()))
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::GetShapedTextFlags
    // (nsLayoutUtils::GetTextRunOrientFlagsForStyle: NOT PORTED, horizontal text only)
    pub fn GetShapedTextFlags_impl(&self) -> ShapedTextFlags {
        ShapedTextFlags::default()
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::MinTabAdvance
    pub fn MinTabAdvance(&self) -> gfxFloat {
        if self.mMinTabAdvance.get() < 0.0 {
            self.mMinTabAdvance.set(GetMinTabAdvanceAppUnits(&self.mTextRun));
        }
        self.mMinTabAdvance.get()
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::GetFrame
    pub fn GetFrame(&self) -> nsTextFrame {
        self.mFrame
    }

    // This may not be equal to the frame offset/length in because we may have
    // adjusted for whitespace trimming according to the state bits set in the
    // frame (for the static provider)
    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::GetStart
    pub fn GetStart(&self) -> &gfxSkipCharsIterator<'static> {
        &self.mStart
    }

    // May return INT32_MAX if that was given to the constructor
    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::GetOriginalLength
    pub fn GetOriginalLength(&self) -> u32 {
        NS_ASSERTION!(self.mLength != i32::MAX, "Length not known");
        self.mLength as u32
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::GetCharacterDataBuffer
    pub fn GetCharacterDataBuffer(&self) -> &CharacterDataBuffer {
        &self.mCharacterDataBuffer
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::GetFontGroup
    pub fn GetFontGroup(&self) -> Rc<gfxFontGroup> {
        self.mTextRun.GetFontGroup()
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::GetFontMetrics
    pub fn GetFontMetrics(&self) -> nsFontMetrics {
        if self.mFontMetrics.borrow().is_none() {
            self.InitFontGroupAndFontMetrics();
        }
        self.mFontMetrics.borrow().clone().unwrap()
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::GetEndHint
    pub fn GetEndHint(&self) -> &gfxSkipCharsIterator<'static> {
        &self.mTempIterator
    }

    // Set a position that should be treated as start-of-line (for trimming
    // potential letter-spacing).
    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::SetStartOfLine
    pub fn SetStartOfLine(&mut self, aPosition: &gfxSkipCharsIterator) {
        self.mStartOfLineOffset = aPosition.GetSkippedOffset();
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::HasSpacing
    pub fn HasSpacing(&self) -> bool {
        self.mLetterSpacing != 0 || self.mWordSpacing != 0 /* || mTextAutospace */
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::LetterSpacing
    pub fn LetterSpacing_value(&self) -> nscoord {
        self.mLetterSpacing
    }

    /**
     * Compute the justification information in given DOM range, return
     * justification info and assignments if requested.
     */
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::ComputeJustification
    // NOT PORTED: justification (nsTextFrame.cpp lines 3725-3834 at GECKO_REV). No justification
    // opportunity is found: the info is empty.
    pub fn ComputeJustification(&mut self, aRange: gfxTextRun_Range, aAssignments: Option<&mut Vec<JustificationAssignment>>) -> JustificationInfo {
        let info = JustificationInfo::new();
        if let Some(a) = aAssignments {
            a.clear();
        }
        info
    }
}

// Gecko: layout/generic/nsTextFrame.cpp FindClusterStart
/**
 * Finds the offset of the first character of the cluster containing aPos
 */
fn FindClusterStart(aTextRun: &RefPtr_gfxTextRun, aOriginalStart: i32, aPos: &mut gfxSkipCharsIterator) {
    while aPos.GetOriginalOffset() > aOriginalStart {
        if aPos.IsOriginalCharSkipped(None) || aTextRun.IsClusterStart(aPos.GetSkippedOffset()) {
            break;
        }
        aPos.AdvanceOriginal(-1);
    }
}

// Gecko: layout/generic/nsTextFrame.cpp FindClusterEnd
/**
 * Finds the offset of the last character of the cluster containing aPos.
 * If aAllowSplitLigature is false, we also check for a ligature-group
 * start.
 */
fn FindClusterEnd(aTextRun: &RefPtr_gfxTextRun, aOriginalEnd: i32, aPos: &mut gfxSkipCharsIterator, aAllowSplitLigature: bool) {
    MOZ_ASSERT!(aPos.GetOriginalOffset() < aOriginalEnd, "character outside string");

    aPos.AdvanceOriginal(1);
    while aPos.GetOriginalOffset() < aOriginalEnd {
        if aPos.IsOriginalCharSkipped(None) || (aTextRun.IsClusterStart(aPos.GetSkippedOffset()) && (aAllowSplitLigature || aTextRun.IsLigatureGroupStart(aPos.GetSkippedOffset()))) {
            break;
        }
        aPos.AdvanceOriginal(1);
    }
    aPos.AdvanceOriginal(-1);
}

// Gecko: layout/generic/nsTextFrame.cpp GetFrameLineNum, FindFirstNewlinePosition,
// FindLastTabPositionBeforeNewline, NextPreservedWhiteSpaceOnLine,
// HasPreservedTabInFollowingSiblingOnLine
// NOT PORTED: justification (they find preserved tabs after the frame on the line so that justification
// can stand back; nsTextFrame.cpp lines 3583-3723 at GECKO_REV).

impl gfxTextRun_PropertyProvider for nsTextFrame_PropertyProvider {
    // aStart, aLength in transformed string offsets.
    // Returns false if no non-standard spacing was required.
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::GetSpacing
    fn GetSpacing(&self, aRange: gfxTextRun_Range, aSpacing: &mut [gfxFont_Spacing]) -> bool {
        self.GetSpacingInternal(aRange, aSpacing, self.mTextRun.GetFlags2().0 & nsTextFrameUtils_Flags::HasTab.0 == 0)
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::GetHyphenWidth
    fn GetHyphenWidth(&self) -> gfxFloat {
        if self.mHyphenWidth.get() < 0.0 {
            // const auto& hyphenateChar = mTextStyle->mHyphenateCharacter; if (hyphenateChar.IsAuto()) ...
            // NOT PORTED: `hyphenate-character` (not in this Stylo): always `auto`.
            self.mHyphenWidth.set(self.GetFontGroup().GetHyphenWidth(self));
        }
        self.mHyphenWidth.get() + self.mLetterSpacing as gfxFloat
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::GetDrawTarget
    // NOT PORTED: drawing; there is no reference draw target.
    fn GetDrawTarget(&self) -> Option<DrawTarget> {
        None
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::GetHyphenationBreaks
    fn GetHyphenationBreaks(&self, aRange: gfxTextRun_Range, aBreakBefore: &mut [gfxTextRun_HyphenType]) {
        #[cfg(debug_assertions)]
        MOZ_ASSERT!(IsInBounds(&self.mStart, self.mLength, aRange), "Range out of bounds");
        MOZ_ASSERT!(self.mLength != i32::MAX, "Can't call this with undefined length");

        if !self.mTextStyle.WhiteSpaceCanWrap(self.mFrame.0) || self.mTextStyle.mHyphens() == StyleHyphens::None {
            for b in aBreakBefore[..aRange.Length() as usize].iter_mut() {
                *b = gfxTextRun_HyphenType::None;
            }
            return;
        }

        // Iterate through the original-string character runs
        let mut run = nsSkipCharsRunIterator::new(&self.mStart, nsSkipCharsRunIterator_LengthMode::LENGTH_UNSKIPPED_ONLY, aRange.Length());
        run.SetSkippedOffset(aRange.start);
        // We need to visit skipped characters so that we can detect SHY
        run.SetVisitSkipped();

        let prevTrailingCharOffset = run.GetPos().GetOriginalOffset() - 1;
        let mut allowHyphenBreakBeforeNextChar = prevTrailingCharOffset >= self.mStart.GetOriginalOffset()
            && prevTrailingCharOffset < self.mStart.GetOriginalOffset() + self.mLength
            && self.mCharacterDataBuffer.CharAt(prevTrailingCharOffset as u32) as u32 == CH_SHY;

        while run.NextRun() {
            NS_ASSERTION!(run.GetRunLength() > 0, "Shouldn't return zero-length runs");
            if run.IsSkipped() {
                // Check if there's a soft hyphen which would let us hyphenate before
                // the next non-skipped character. Don't look at soft hyphens followed
                // by other skipped characters, we won't use them.
                allowHyphenBreakBeforeNextChar = self.mCharacterDataBuffer.CharAt((run.GetOriginalOffset() + run.GetRunLength() - 1) as u32) as u32 == CH_SHY;
            } else {
                let runOffsetInSubstring = (run.GetSkippedOffset() - aRange.start) as usize;
                for b in aBreakBefore[runOffsetInSubstring..runOffsetInSubstring + run.GetRunLength() as usize].iter_mut() {
                    *b = gfxTextRun_HyphenType::None;
                }
                // Don't allow hyphen breaks at the start of the line
                aBreakBefore[runOffsetInSubstring] = if allowHyphenBreakBeforeNextChar && (!self.mFrame.HasAnyStateBits(TEXT_START_OF_LINE) || run.GetSkippedOffset() > self.mStart.GetSkippedOffset()) { gfxTextRun_HyphenType::Soft } else { gfxTextRun_HyphenType::None };
                allowHyphenBreakBeforeNextChar = false;
            }
        }

        // if (mTextStyle->mHyphens == StyleHyphens::Auto) { ... IS_HYPHEN ... }: NOT PORTED,
        // `hyphens: auto` (hyphens is manual here)
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::GetHyphensOption
    fn GetHyphensOption(&self) -> StyleHyphens {
        self.mTextStyle.mHyphens()
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::GetShapedTextFlags
    fn GetShapedTextFlags(&self) -> ShapedTextFlags {
        self.GetShapedTextFlags_impl()
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::GetAppUnitsPerDevUnit
    fn GetAppUnitsPerDevUnit(&self) -> u32 {
        self.mTextRun.GetAppUnitsPerDevUnit() as u32
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::PropertyProvider::LetterSpacing
    fn LetterSpacing(&self) -> nscoord {
        self.mLetterSpacing
    }
}

// Gecko: layout/generic/nsTextFrame.cpp CanAddSpacingBefore
fn CanAddSpacingBefore(aTextRun: &RefPtr_gfxTextRun, aOffset: u32, aNewlineIsSignificant: bool) -> bool {
    let g = aTextRun.GetCharacterGlyphs();
    MOZ_ASSERT!(aOffset < aTextRun.GetLength());
    if aNewlineIsSignificant && g[aOffset as usize].CharIsNewline() {
        return false;
    }
    if aOffset == 0 {
        return true;
    }
    g[aOffset as usize].IsClusterStart() && g[aOffset as usize].IsLigatureGroupStart() && !g[aOffset as usize - 1].CharIsFormattingControl() && !g[aOffset as usize].CharIsTab()
}

// Gecko: layout/generic/nsTextFrame.cpp CanAddSpacingAfter
fn CanAddSpacingAfter(aTextRun: &RefPtr_gfxTextRun, aOffset: u32, aNewlineIsSignificant: bool) -> bool {
    let g = aTextRun.GetCharacterGlyphs();
    MOZ_ASSERT!(aOffset < aTextRun.GetLength());
    if aNewlineIsSignificant && g[aOffset as usize].CharIsNewline() {
        return false;
    }
    if aOffset + 1 >= aTextRun.GetLength() {
        return true;
    }
    g[aOffset as usize + 1].IsClusterStart() && g[aOffset as usize + 1].IsLigatureGroupStart() && !g[aOffset as usize].CharIsFormattingControl() && !g[aOffset as usize].CharIsTab()
}

// Gecko: layout/generic/nsTextFrame.cpp ComputeTabWidthAppUnits
fn ComputeTabWidthAppUnits(aFrame: nsIFrame) -> gfxFloat {
    // `tab-size` is not in the servo configuration of Stylo: the initial value, the number 8.
    // NOT PORTED: a `tab-size: <length>`.
    let spaces: gfxFloat = 8.0;
    MOZ_ASSERT!(spaces >= 0.0);

    let cb = aFrame.GetContainingBlock__uint32_t_nsStyleDisplay(0, &aFrame.StyleDisplay()).unwrap();
    let styleText = nsStyleTextAdapter::of(cb);

    // Round the space width when converting to appunits the same way textruns do.
    // We don't use GetFirstFontMetrics here because that may return a font that
    // does not actually have the <space> character, yet is considered the "first
    // available font" per CSS Fonts. Here, we want the font that would be used
    // to render <space>, even if that means looking further down the font-family
    // list.
    let fm = nsLayoutUtils::GetFontMetricsForFrame(cb, 1.0);
    // bool vertical = cb->GetWritingMode().IsCentralBaseline(): NOT PORTED, vertical metrics
    let font = fm.GetThebesFontGroup().GetFirstValidFont(' ' as u32, None, None);
    let metrics = font.GetMetrics();
    let spaceWidth: nscoord = NSToCoordRound__float((metrics.spaceWidth * AppUnitsPerCSSPixel() as f64) as f32); // AppUnitsPerDevPixel
    spaces * (spaceWidth as f64 + styleText.mLetterSpacingResolved(fm.EmHeight()) as f64 + styleText.mWordSpacingResolved(spaceWidth) as f64)
}

// Gecko: layout/generic/nsTextFrame.cpp LastNonMarkCharClass, LastNonMarkCharClassInFrame,
// GetPrecedingCharClassFromMappedFlows, GetPrecedingCharClassFromFrameTree, HasCJKGlyphRun
// NOT PORTED: text-autospace (nsTextFrame.cpp lines 3907-4086 at GECKO_REV).

impl nsTextFrame_PropertyProvider {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::GetSpacingInternal
    pub fn GetSpacingInternal(&self, aRange: gfxTextRun_Range, aSpacing: &mut [gfxFont_Spacing], aIgnoreTabs: bool) -> bool {
        #[cfg(debug_assertions)]
        MOZ_ASSERT!(IsInBounds(&self.mStart, self.mLength, aRange), "Range out of bounds");

        for s in aSpacing[..aRange.Length() as usize].iter_mut() {
            *s = gfxFont_Spacing::default();
        }

        if self.mFrame.Style().IsTextCombined() {
            return false;
        }

        // Track whether any non-standard spacing is actually present in this range.
        // If letter-spacing is non-zero this will always be true, but for the
        /// word-spacing and text-autospace cases it will depend on the actual text.
        let mut spacingPresent = self.mLetterSpacing != 0;

        // First, compute the word spacing, letter spacing, and text-autospace
        // spacing.
        if self.mWordSpacing != 0 || self.mLetterSpacing != 0 /* || mTextAutospace */ {
            let newlineIsSignificant = self.mTextStyle.NewlineIsSignificant(self.mFrame);
            // Which letter-spacing model are we using?
            //   0 - Gecko legacy model, spacing added to trailing side of letter
            //   1 - WebKit/Blink-compatible, spacing added to right-hand side
            //   2 - Symmetrical spacing, half added to each side
            // (StaticPrefs::layout_css_letter_spacing_model(): the default model, 0)
            let before: nscoord = 0;
            let after: nscoord = self.mLetterSpacing;

            // Find our offset into the original+transformed string
            let mut start = self.mStart;
            start.SetSkippedOffset(aRange.start);
            let mut atStart = self.mStartOfLineOffset == start.GetSkippedOffset() && !self.mFrame.IsInSVGTextSubtree();

            // Iterate over non-skipped characters
            let mut run = nsSkipCharsRunIterator::new(&start, nsSkipCharsRunIterator_LengthMode::LENGTH_UNSKIPPED_ONLY, aRange.Length());
            while run.NextRun() {
                let runOffsetInSubstring = run.GetSkippedOffset() - aRange.start;
                let mut iter = *run.GetPos();
                for i in 0..run.GetRunLength() {
                    if !atStart && before != 0 && CanAddSpacingBefore(&self.mTextRun, run.GetSkippedOffset() + i as u32, newlineIsSignificant) && !IsCursiveScript(self.currScalar(&mut iter, &run, i)) {
                        aSpacing[(runOffsetInSubstring + i as u32) as usize].mBefore += before;
                    }
                    if after != 0 && CanAddSpacingAfter(&self.mTextRun, run.GetSkippedOffset() + i as u32, newlineIsSignificant) {
                        // End of a cluster, not in a ligature: put letter-spacing after it,
                        // unless the base char of the cluster belonged to a cursive script.
                        iter.SetSkippedOffset(run.GetSkippedOffset() + i as u32);
                        FindClusterStart(&self.mTextRun, run.GetOriginalOffset(), &mut iter);
                        let baseChar = self.mCharacterDataBuffer.ScalarValueAt(iter.GetOriginalOffset() as u32);
                        if !IsCursiveScript(baseChar) {
                            aSpacing[(runOffsetInSubstring + i as u32) as usize].mAfter += after;
                        }
                    }
                    if self.mWordSpacing != 0 && IsCSSWordSpacingSpace(&self.mCharacterDataBuffer, (i + run.GetOriginalOffset()) as u32, self.mFrame, &self.mTextStyle) {
                        // It kinda sucks, but space characters can be part of clusters,
                        // and even still be whitespace (I think!)
                        iter.SetSkippedOffset(run.GetSkippedOffset() + i as u32);
                        FindClusterEnd(&self.mTextRun, run.GetOriginalOffset() + run.GetRunLength(), &mut iter, true);
                        let runOffset = iter.GetSkippedOffset() - aRange.start;
                        aSpacing[runOffset as usize].mAfter += self.mWordSpacing;
                        spacingPresent = true;
                    }
                    // text-autospace: NOT PORTED
                    atStart = false;
                }
            }
        }

        // Now add tab spacing, if there is any
        if !aIgnoreTabs {
            let tabWidth = ComputeTabWidthAppUnits(self.mFrame.0);
            if tabWidth > 0.0 {
                self.CalcTabWidths(aRange, tabWidth);
                if let Some(tabWidths) = self.mTabWidths.borrow().as_ref() {
                    tabWidths.borrow().ApplySpacing(aSpacing, aRange.start - self.mStart.GetSkippedOffset(), aRange.Length());
                    spacingPresent = true;
                }
            }
        }

        // Now add in justification spacing
        if self.mJustificationSpacings.len() > 0 {
            // If there is any spaces trimmed at the end, aStart + aLength may
            // be larger than the flags array. When that happens, we can simply
            // ignore those spaces.
            let arrayEnd = self.mJustificationArrayStart + self.mJustificationSpacings.len() as u32;
            let end = std::cmp::min(aRange.end, arrayEnd);
            MOZ_ASSERT!(aRange.start >= self.mJustificationArrayStart);
            for i in aRange.start..end {
                let spacing = &self.mJustificationSpacings[(i - self.mJustificationArrayStart) as usize];
                let offset = (i - aRange.start) as usize;
                aSpacing[offset].mBefore += spacing.mBefore;
                aSpacing[offset].mAfter += spacing.mAfter;
            }
            spacingPresent = true;
        }

        spacingPresent
    }

    // `currScalar` of the C++ lambda: the scalar value at the character i of the run.
    fn currScalar(&self, iter: &mut gfxSkipCharsIterator, run: &nsSkipCharsRunIterator, i: i32) -> u32 {
        iter.SetSkippedOffset(run.GetSkippedOffset() + i as u32);
        self.mCharacterDataBuffer.ScalarValueAt(iter.GetOriginalOffset() as u32)
    }
}

// aX and the result are in whole appunits.
// Gecko: layout/generic/nsTextFrame.cpp AdvanceToNextTab
fn AdvanceToNextTab(aX: gfxFloat, aTabWidth: gfxFloat, aMinAdvance: gfxFloat) -> gfxFloat {
    // Advance aX to the next multiple of aTabWidth. We must advance
    // by at least aMinAdvance.
    let nextPos = aX + aMinAdvance;
    if aTabWidth > 0.0 { (nextPos / aTabWidth).ceil() * aTabWidth } else { nextPos }
}

impl nsTextFrame_PropertyProvider {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::CalcTabWidths
    pub fn CalcTabWidths(&self, aRange: gfxTextRun_Range, aTabWidth: gfxFloat) {
        MOZ_ASSERT!(aTabWidth > 0.0);

        if self.mTabWidths.borrow().is_none() {
            if self.mReflowing && self.mLineContainer.is_none() {
                // Intrinsic width computation does its own tab processing. We
                // just don't do anything here.
                return;
            }
            if !self.mReflowing {
                *self.mTabWidths.borrow_mut() = self.mFrame.GetProperty(TabWidthProperty());
                return;
            }
        }

        let startOffset = self.mStart.GetSkippedOffset();
        MOZ_ASSERT!(aRange.start >= startOffset, "wrong start offset");
        MOZ_ASSERT!(aRange.end as i64 <= startOffset as i64 + self.mLength as i64, "beyond the end");
        let tabsEnd = match self.mTabWidths.borrow().as_ref() {
            Some(t) => t.borrow().mLimit,
            None => self.mTabWidthsAnalyzedLimit.get(),
        } + startOffset;
        if tabsEnd < aRange.end {
            NS_ASSERTION!(self.mReflowing, "We need precomputed tab widths, but don't have enough.");

            for i in tabsEnd..aRange.end {
                let mut spacing = [gfxFont_Spacing::default()];
                self.GetSpacingInternal(gfxTextRun_Range::new(i, i + 1), &mut spacing, true);
                self.mOffsetFromBlockOriginForTabs.set(self.mOffsetFromBlockOriginForTabs.get() + spacing[0].mBefore as f64);

                if !self.mTextRun.CharIsTab(i) {
                    if self.mTextRun.IsClusterStart(i) {
                        let mut clusterEnd = i + 1;
                        while clusterEnd < self.mTextRun.GetLength() && !self.mTextRun.IsClusterStart(clusterEnd) {
                            clusterEnd += 1;
                        }
                        self.mOffsetFromBlockOriginForTabs.set(self.mOffsetFromBlockOriginForTabs.get() + self.mTextRun.GetAdvanceWidth(gfxTextRun_Range::new(i, clusterEnd), None));
                    }
                } else {
                    if self.mTabWidths.borrow().is_none() {
                        let store = Rc::new(RefCell::new(nsTextFrame_TabWidthStore::new(self.mFrame.GetContentOffset())));
                        self.mFrame.SetProperty(TabWidthProperty(), store.clone());
                        *self.mTabWidths.borrow_mut() = Some(store);
                    }
                    let nextTab = AdvanceToNextTab(self.mOffsetFromBlockOriginForTabs.get(), aTabWidth, self.MinTabAdvance());
                    self.mTabWidths.borrow().as_ref().unwrap().borrow_mut().mWidths.push(TabWidth::new(i - startOffset, NSToIntRound__double(nextTab - self.mOffsetFromBlockOriginForTabs.get()) as u32));
                    self.mOffsetFromBlockOriginForTabs.set(nextTab);
                }

                self.mOffsetFromBlockOriginForTabs.set(self.mOffsetFromBlockOriginForTabs.get() + spacing[0].mAfter as f64);
            }

            if let Some(t) = self.mTabWidths.borrow().as_ref() {
                t.borrow_mut().mLimit = aRange.end - startOffset;
            }
        }

        if self.mTabWidths.borrow().is_none() {
            // Delete any stale property that may be left on the frame
            self.mFrame.RemoveProperty(TabWidthProperty());
            self.mTabWidthsAnalyzedLimit.set(std::cmp::max(self.mTabWidthsAnalyzedLimit.get(), aRange.end - startOffset));
        }
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::InitializeForDisplay
    // Call this after construction if you're not going to reflow the text
    pub fn InitializeForDisplay(&mut self, aTrimAfter: bool) {
        let trimmed = self.mFrame.GetTrimmedOffsets(&self.mCharacterDataBuffer, if aTrimAfter { nsTextFrame_TrimmedOffsetFlags::Default } else { nsTextFrame_TrimmedOffsetFlags::NoTrimAfter });
        self.mStart.SetOriginalOffset(trimmed.mStart);
        self.mLength = trimmed.mLength;
        if self.mFrame.HasAnyStateBits(TEXT_START_OF_LINE) {
            self.mStartOfLineOffset = self.mStart.GetSkippedOffset();
        }
        self.SetupJustificationSpacing(true);
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::InitializeForMeasure
    pub fn InitializeForMeasure(&mut self) {
        let trimmed = self.mFrame.GetTrimmedOffsets(&self.mCharacterDataBuffer, nsTextFrame_TrimmedOffsetFlags::NotPostReflow);
        self.mStart.SetOriginalOffset(trimmed.mStart);
        self.mLength = trimmed.mLength;
        if self.mFrame.HasAnyStateBits(TEXT_START_OF_LINE) {
            self.mStartOfLineOffset = self.mStart.GetSkippedOffset();
        }
        self.SetupJustificationSpacing(false);
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::SetupJustificationSpacing
    // NOT PORTED: justification (nsTextFrame.cpp lines 4502-4559 at GECKO_REV): the frame never has
    // TEXT_JUSTIFICATION_ENABLED with any gap, so nothing is assigned.
    fn SetupJustificationSpacing(&mut self, aPostReflow: bool) {
        MOZ_ASSERT!(self.mLength != i32::MAX, "Can't call this with undefined length");
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::InitFontGroupAndFontMetrics
    fn InitFontGroupAndFontMetrics(&self) {
        if self.mFontMetrics.borrow().is_none() {
            if self.mWhichTextRun == nsTextFrame_TextRunType::eInflated {
                *self.mFontMetrics.borrow_mut() = Some(self.mFrame.InflatedFontMetrics());
            } else {
                *self.mFontMetrics.borrow_mut() = Some(nsLayoutUtils::GetFontMetricsForFrame(self.mFrame.0, 1.0));
            }
        }
        // mFontGroup = mFontMetrics->GetThebesFontGroup(): GetFontGroup() asks the text run
    }

    // Initialize TextAutospace, if inter-script spacing applies.
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::PropertyProvider::InitTextAutospace
    // NOT PORTED: text-autospace.
    fn InitTextAutospace(&mut self) {}
}

// Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::BuildDisplayList, GetCursor
// NOT PORTED: painting and cursors.

// Gecko: layout/generic/nsTextFrame.cpp the painting section (PaintText, DrawText, selection,
// decorations, shadows, emphasis marks): nsTextFrame.cpp lines 5208-8205 at GECKO_REV.
// NOT PORTED: painting.

// Gecko: layout/generic/nsTextFrame.cpp the selection, caret and find section (IsEntirelyWhitespace
// .. HasVisibleText): nsTextFrame.cpp lines 8207-9420 at GECKO_REV.
// NOT PORTED: selection, caret, offsets from points.
impl nsTextFrame {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::IsEntirelyWhitespace
    /**
     * Check that the text in this frame is entirely whitespace. Importantly,
     * this function considers non-breaking spaces (0xa0) to be whitespace,
     * whereas nsTextFrame::IsEmpty does not. It also considers both one and
     * two-byte chars.
     */
    pub fn IsEntirelyWhitespace(&self) -> bool {
        let characterDataBuffer = self.CharacterDataBuffer();
        for index in 0..characterDataBuffer.GetLength() {
            let ch = characterDataBuffer.CharAt(index);
            if ch == 0x20 || ch == 0x0A || ch == 0x09 || ch == 0x0D || ch == 0xA0 {
                continue;
            }
            return false;
        }
        true
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetOffsets
    pub fn GetOffsets(&self) -> (i32, i32) {
        (self.GetContentOffset(), self.GetContentEnd())
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::HasVisibleText
    // NOT PORTED: used by the editor.
}

// Gecko: layout/generic/nsTextFrame.cpp FindFirstLetterRange .. FindStartAfterSkippingWhitespace's
// first-letter helpers (IsFirstLetterPrefixPunctuation, FindEndOfPrefixPunctuationRun, ...)
// NOT PORTED: ::first-letter (nsTextFrame.cpp lines 9423-9741 at GECKO_REV).

// Gecko: layout/generic/nsTextFrame.cpp FindStartAfterSkippingWhitespace
fn FindStartAfterSkippingWhitespace(aProvider: &nsTextFrame_PropertyProvider, aData: &mut nsIFrame_InlineIntrinsicISizeData, aTextStyle: &nsStyleTextAdapter, aIterator: &mut gfxSkipCharsIterator, aFlowEndInTextRun: u32) -> u32 {
    if aData.mSkipWhitespace {
        while aIterator.GetSkippedOffset() < aFlowEndInTextRun && IsTrimmableSpace(aProvider.GetCharacterDataBuffer(), aIterator.GetOriginalOffset() as u32, aTextStyle, false) {
            aIterator.AdvanceOriginal(1);
        }
    }
    aIterator.GetSkippedOffset()
}

impl nsTextFrame {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::MarkIntrinsicISizesDirty
    /* virtual */
    pub fn MarkIntrinsicISizesDirty(&self) {
        self.ClearTextRuns();
        // nsIFrame::MarkIntrinsicISizesDirty(): no caches on a leaf frame
    }

    // XXX this doesn't handle characters shaped by line endings. We need to
    // temporarily override the "current line ending" settings.
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::AddInlineMinISizeForFlow
    pub fn AddInlineMinISizeForFlow(&self, aRenderingContext: Option<gfxContext>, aData: &mut InlineMinISizeData, aTextRunType: nsTextFrame_TextRunType) {
        let mut flowEndInTextRun: u32 = 0;
        let mut iter = self.EnsureTextRun(aTextRunType, aData.LineContainer(), aData.mLine, Some(&mut flowEndInTextRun));
        let Some(textRun) = self.GetTextRun(aTextRunType) else {
            return;
        };

        // Pass null for the line container. This will disable tab spacing, but that's
        // OK since we can't really handle tabs for intrinsic sizing anyway.
        let textStyle = nsStyleTextAdapter::of(self.0);
        let characterDataBuffer = self.CharacterDataBuffer();

        // If we're hyphenating, the PropertyProvider needs the actual length;
        // otherwise we can just pass INT32_MAX to mean "all the text"
        let mut len: i32 = i32::MAX;
        let mut hyphenating = characterDataBuffer.GetLength() > 0 && (textStyle.mHyphens() == StyleHyphens::Auto || (textStyle.mHyphens() == StyleHyphens::Manual && textRun.GetFlags().0 & ShapedTextFlags::TEXT_ENABLE_HYPHEN_BREAKS.0 != 0));
        if hyphenating {
            let mut tmp = iter;
            len = std::cmp::min(self.GetContentOffset() + self.GetInFlowContentLength(), tmp.ConvertSkippedToOriginal(flowEndInTextRun)) - iter.GetOriginalOffset();
        }
        let mut provider = nsTextFrame_PropertyProvider::new__gfxTextRun_nsStyleText_CharacterDataBuffer_nsTextFrame_gfxSkipCharsIterator_int32_t_nsIFrame_nscoord_TextRunType_bool(
            textRun.clone(),
            textStyle.clone(),
            characterDataBuffer.clone(),
            *self,
            iter,
            len,
            None,
            0,
            aTextRunType,
            aData.mAtStartOfLine,
        );

        let collapseWhitespace = !textStyle.WhiteSpaceIsSignificant();
        let preformatNewlines = textStyle.NewlineIsSignificant(*self);
        let preformatTabs = textStyle.WhiteSpaceIsSignificant();
        let whitespaceCanHang = textStyle.WhiteSpaceCanHangOrVisuallyCollapse();
        let mut tabWidth: gfxFloat = -1.0;
        let start = FindStartAfterSkippingWhitespace(&provider, aData, &textStyle, &mut iter, flowEndInTextRun);

        // text-combine-upright frame is constantly 1em on inline-axis; but if it has
        // a following sibling with the same style, it contributes nothing.
        // (Style()->IsTextCombined(): NOT PORTED)

        // textStyle->EffectiveOverflowWrap() == Anywhere && WordCanWrap(this)
        if textStyle.Style().get_inherited_text().overflow_wrap == mrt_style::style::values::specified::text::OverflowWrap::Anywhere && textStyle.WordCanWrap(self.0) {
            aData.OptionallyBreak(0);
            aData.mCurrentLine += textRun.GetMinAdvanceWidth(gfxTextRun_Range::new(start, flowEndInTextRun), provider.LetterSpacing_value()) as nscoord;
            aData.mTrailingWhitespace = 0;
            aData.mAtStartOfLine = false;
            aData.OptionallyBreak(0);
            return;
        }

        let mut hyphBuffer: Vec<gfxTextRun_HyphenType> = Vec::new();
        if hyphenating {
            hyphBuffer.resize((flowEndInTextRun - start) as usize, gfxTextRun_HyphenType::None);
            provider.GetHyphenationBreaks(gfxTextRun_Range::new(start, flowEndInTextRun), &mut hyphBuffer[..]);
        }

        let glyphs = textRun.GetCharacterGlyphs();
        let limit = flowEndInTextRun as usize;
        let hasSpacing = provider.HasSpacing();
        let mut wordAdvance: nscoord = 0;
        let mut wordStart = start;
        // Rust: `g` is the index into glyphs, `g <= limit`.
        let mut g = start as usize;
        while g <= limit {
            if !hyphenating {
                while g < limit && glyphs[g].IsSimpleGlyphNoBreakBefore() {
                    wordAdvance += glyphs[g].GetSimpleAdvance() as nscoord;
                    g += 1;
                }
            }

            let mut preformattedNewline = false;
            let mut preformattedTab = false;
            if g < limit {
                // XXXldb Shouldn't we be including the newline as part of the
                // segment that it ends rather than part of the segment that it
                // starts?
                preformattedNewline = preformatNewlines && glyphs[g].CharIsNewline();
                preformattedTab = preformatTabs && glyphs[g].CharIsTab();
                if glyphs[g].CanBreakBefore() != gfxTextRun_CompressedGlyph::FLAG_BREAK_TYPE_NORMAL && !preformattedNewline && !preformattedTab && (!hyphenating || !gfxTextRun_IsOptionalHyphenBreak(hyphBuffer[g - start as usize])) {
                    // we can't break here (and it's not the end of the flow)
                    if glyphs[g].IsSimpleGlyph() {
                        wordAdvance += glyphs[g].GetSimpleAdvance() as nscoord;
                    } else if !hasSpacing {
                        for d in textRun.GetDetailedGlyphs(g as u32) {
                            wordAdvance += d.mAdvance as nscoord;
                        }
                    }
                    g += 1;
                    continue;
                }
            }

            let wordEnd = g as u32;
            if wordEnd > wordStart {
                let mut width: nscoord = if !hasSpacing && glyphs[wordStart as usize].IsLigatureGroupStart() && (wordEnd == flowEndInTextRun || glyphs[wordEnd as usize].IsLigatureGroupStart()) {
                    wordAdvance
                } else {
                    NSToCoordCeilClamped(textRun.GetAdvanceWidth(gfxTextRun_Range::new(wordStart, wordEnd), Some(&provider)))
                };
                width = std::cmp::max(0, width);
                aData.mCurrentLine = NSCoordSaturatingAdd(aData.mCurrentLine, width);
                aData.mAtStartOfLine = false;

                if collapseWhitespace || whitespaceCanHang {
                    let trimStart = GetEndOfTrimmedText(&characterDataBuffer, &textStyle, wordStart, wordEnd, &mut iter, whitespaceCanHang);
                    if trimStart == start {
                        // This is *all* trimmable whitespace, so whatever trailingWhitespace
                        // we saw previously is still trailing...
                        aData.mTrailingWhitespace += width;
                    } else {
                        // Some non-whitespace so the old trailingWhitespace is no longer
                        // trailing
                        let wsWidth = NSToCoordCeilClamped(textRun.GetAdvanceWidth(gfxTextRun_Range::new(trimStart, wordEnd), Some(&provider)));
                        aData.mTrailingWhitespace = std::cmp::max(0, wsWidth);
                    }
                } else {
                    aData.mTrailingWhitespace = 0;
                }
            }

            if g < limit {
                if glyphs[g].IsSimpleGlyph() {
                    wordAdvance = glyphs[g].GetSimpleAdvance() as nscoord;
                } else {
                    wordAdvance = 0;
                    if !preformattedNewline && !preformattedTab && !hasSpacing {
                        for d in textRun.GetDetailedGlyphs(g as u32) {
                            wordAdvance += d.mAdvance as nscoord;
                        }
                    }
                }
            }

            if preformattedTab {
                let mut spacing = [gfxFont_Spacing::default()];
                provider.GetSpacing(gfxTextRun_Range::new(wordEnd, wordEnd + 1), &mut spacing);
                aData.mCurrentLine += spacing[0].mBefore;
                if tabWidth < 0.0 {
                    tabWidth = ComputeTabWidthAppUnits(self.0);
                }
                let afterTab = AdvanceToNextTab(aData.mCurrentLine as gfxFloat, tabWidth, provider.MinTabAdvance());
                aData.mCurrentLine = (afterTab + spacing[0].mAfter as f64) as nscoord;
                wordStart = wordEnd + 1;
            } else if wordEnd < flowEndInTextRun || (wordEnd == textRun.GetLength() && textRun.GetFlags2().0 & nsTextFrameUtils_Flags::HasTrailingBreak.0 != 0) {
                if preformattedNewline {
                    aData.ForceBreak();
                } else if wordEnd < flowEndInTextRun && hyphenating && gfxTextRun_IsOptionalHyphenBreak(hyphBuffer[(wordEnd - start) as usize]) {
                    aData.OptionallyBreak(NSToCoordRound__float(provider.GetHyphenWidth() as f32));
                } else {
                    aData.OptionallyBreak(0);
                }
                if aData.mSkipWhitespace {
                    iter.SetSkippedOffset(wordEnd);
                    wordStart = FindStartAfterSkippingWhitespace(&provider, aData, &textStyle, &mut iter, flowEndInTextRun);
                } else {
                    wordStart = wordEnd;
                }
                provider.SetStartOfLine(&iter);
            }
            g += 1;
        }

        if start < flowEndInTextRun {
            // Check if we have collapsible whitespace at the end
            aData.mSkipWhitespace = IsTrimmableSpace(provider.GetCharacterDataBuffer(), iter.ConvertSkippedToOriginal(flowEndInTextRun - 1) as u32, &textStyle, false);
        }
        let _ = (&mut hyphenating, aRenderingContext);
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::MaybeSplitFramesForFirstLetter
    // NOT PORTED: ::first-letter (nsTextFrame.cpp lines 10054-10124 at GECKO_REV).

    // XXX Need to do something here to avoid incremental reflow bugs due to
    // first-line changing min-width
    /* virtual */
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::AddInlineMinISize
    pub fn AddInlineMinISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlineMinISizeData) {
        // nsContinuingTextFrame::AddInlineMinISize: Do nothing, since the first-in-flow accounts for everything.
        if self.with_data(|d| d.mIsContinuing) {
            return;
        }

        // Check if this textframe belongs to a first-letter frame that has not yet
        // been reflowed; if so, we need to deal with splitting off a continuation
        // before we can measure the advance correctly.
        // (IsUnreflowedLetterFrame(GetParent()) -> MaybeSplitFramesForFirstLetter(): NOT PORTED)

        let inflation = nsLayoutUtils::FontSizeInflationFor(self.0);
        let trtype = if inflation == 1.0 { nsTextFrame_TextRunType::eNotInflated } else { nsTextFrame_TextRunType::eInflated };

        if trtype == nsTextFrame_TextRunType::eInflated && !self.IsCurrentFontInflation(inflation) {
            // FIXME: Ideally, if we already have a text run, we'd move it to be
            // the uninflated text run.
            self.ClearTextRun(None, nsTextFrame_TextRunType::eInflated);
            self.with_data(|d| d.mFontMetrics = None);
        }

        let mut lastTextRun: Option<RefPtr_gfxTextRun> = None;
        // nsContinuingTextFrame does nothing for AddInlineMinISize; all text frames
        // in the flow are handled right here.
        let mut f = Some(*self);
        while let Some(fr) = f {
            // f->GetTextRun(nsTextFrame::eNotInflated) could be null if we
            // haven't set up textruns yet for f.  Except in OOM situations,
            // lastTextRun will only be null for the first text frame.
            let same = match (&fr.GetTextRun(trtype), &lastTextRun) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if fr == *self || !same {
                if let Some(container) = aData.LineContainer() {
                    let lc = fr.FindLineContainer();
                    if Some(container) != lc {
                        NS_ASSERTION!(fr != *self, "wrong InlineMinISizeData container for first continuation");
                        aData.mLine = None;
                        aData.SetLineContainer(lc);
                    }
                }

                // This will process all the text frames that share the same textrun as f.
                fr.AddInlineMinISizeForFlow(aInput.mContext, aData, trtype);
                lastTextRun = fr.GetTextRun(trtype);
            }
            f = fr.GetNextContinuation().map(nsTextFrame);
        }
    }

    // XXX this doesn't handle characters shaped by line endings. We need to
    // temporarily override the "current line ending" settings.
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::AddInlinePrefISizeForFlow
    pub fn AddInlinePrefISizeForFlow(&self, aRenderingContext: Option<gfxContext>, aData: &mut InlinePrefISizeData, aTextRunType: nsTextFrame_TextRunType) {
        // if (IsUnreflowedLetterFrame(GetParent())) MaybeSplitFramesForFirstLetter(): NOT PORTED

        let mut flowEndInTextRun: u32 = 0;
        let mut iter = self.EnsureTextRun(aTextRunType, aData.LineContainer(), aData.mLine, Some(&mut flowEndInTextRun));
        let Some(textRun) = self.GetTextRun(aTextRunType) else {
            return;
        };

        // Pass null for the line container. This will disable tab spacing, but that's
        // OK since we can't really handle tabs for intrinsic sizing anyway.

        let textStyle = nsStyleTextAdapter::of(self.0);
        let characterDataBuffer = self.CharacterDataBuffer();
        let mut provider = nsTextFrame_PropertyProvider::new__gfxTextRun_nsStyleText_CharacterDataBuffer_nsTextFrame_gfxSkipCharsIterator_int32_t_nsIFrame_nscoord_TextRunType_bool(
            textRun.clone(),
            textStyle.clone(),
            characterDataBuffer.clone(),
            *self,
            iter,
            i32::MAX,
            None,
            0,
            aTextRunType,
            aData.mLineIsEmpty,
        );

        // text-combine-upright frame is constantly 1em on inline-axis, or zero if
        // there is a following sibling with the same style.
        // (Style()->IsTextCombined(): NOT PORTED)

        let collapseWhitespace = !textStyle.WhiteSpaceIsSignificant();
        let preformatNewlines = textStyle.NewlineIsSignificant(*self);
        let preformatTabs = textStyle.TabIsSignificant();
        let mut tabWidth: gfxFloat = -1.0;
        let start = FindStartAfterSkippingWhitespace(&provider, aData, &textStyle, &mut iter, flowEndInTextRun);
        if aData.mLineIsEmpty {
            provider.SetStartOfLine(&iter);
        }

        // XXX Should we consider hyphenation here?
        // If newlines and tabs aren't preformatted, nothing to do inside
        // the loop so skip directly to the end
        let loopStart = if preformatNewlines || preformatTabs { start } else { flowEndInTextRun };
        let glyphs = textRun.GetCharacterGlyphs();
        let limit = flowEndInTextRun as usize;
        let canUseSimpleAdvance = loopStart == start && !provider.HasSpacing();
        let mut lineStart = start;
        let mut runAdvance: nscoord = 0;
        let mut g = loopStart as usize;
        while g <= limit {
            // Simple glyphs are never tabs or newlines, so batch-skip them to avoid
            // per-character CharIsTab/CharIsNewline calls.
            while g < limit && glyphs[g].IsSimpleGlyph() {
                runAdvance += glyphs[g].GetSimpleAdvance() as nscoord;
                g += 1;
            }

            // Now /g/ points to a non-simple glyph, or has reached /limit/.
            let mut preformattedNewline = false;
            let mut preformattedTab = false;
            if g < limit {
                // XXXldb Shouldn't we be including the newline as part of the
                // segment that it ends rather than part of the segment that it
                // starts?
                MOZ_ASSERT!(preformatNewlines || preformatTabs, "We can't be here unless newlines are hard breaks or there are tabs");
                MOZ_ASSERT!(!glyphs[g].IsSimpleGlyph(), "should have been skipped");
                preformattedNewline = preformatNewlines && glyphs[g].CharIsNewline();
                preformattedTab = preformatTabs && glyphs[g].CharIsTab();
                // Skip accumulating advance if there's custom spacing in the
                // PropertyProvider; we'll call textRun->GetAdvanceWidth instead.
                if canUseSimpleAdvance {
                    // We don't check g->ApplyLetterSpacingBetweenDetailedGlyphs() here
                    // because canUseSimpleAdvance depends on there being no spacing to
                    // consider.
                    for d in textRun.GetDetailedGlyphs(g as u32) {
                        runAdvance += d.mAdvance as nscoord;
                    }
                }
                if !preformattedNewline && !preformattedTab {
                    // we needn't break here (and it's not the end of the flow)
                    g += 1;
                    continue;
                }
            }

            let lineEnd = g as u32;
            if lineEnd > lineStart {
                // If there's no custom spacing, and we didn't immediately skip to
                // the end of the loop, and neither end of the range falls within a
                // ligature, we can use the advance that was accumulated above.
                let mut width: nscoord = if canUseSimpleAdvance && glyphs[lineStart as usize].IsLigatureGroupStart() && (lineEnd == flowEndInTextRun || glyphs[lineEnd as usize].IsLigatureGroupStart()) {
                    runAdvance
                } else {
                    NSToCoordCeilClamped(textRun.GetAdvanceWidth(gfxTextRun_Range::new(lineStart, lineEnd), Some(&provider)))
                };
                width = std::cmp::max(0, width);
                aData.mCurrentLine = NSCoordSaturatingAdd(aData.mCurrentLine, width);
                aData.mLineIsEmpty = false;

                if collapseWhitespace {
                    let trimStart = GetEndOfTrimmedText(&characterDataBuffer, &textStyle, lineStart, lineEnd, &mut iter, false);
                    if trimStart == start {
                        // This is *all* trimmable whitespace, so whatever trailingWhitespace
                        // we saw previously is still trailing...
                        aData.mTrailingWhitespace += width;
                    } else {
                        // Some non-whitespace so the old trailingWhitespace is no longer
                        // trailing
                        let wsWidth = NSToCoordCeilClamped(textRun.GetAdvanceWidth(gfxTextRun_Range::new(trimStart, lineEnd), Some(&provider)));
                        aData.mTrailingWhitespace = std::cmp::max(0, wsWidth);
                    }
                } else {
                    aData.mTrailingWhitespace = 0;
                }
            }
            runAdvance = 0;

            if preformattedTab {
                let mut spacing = [gfxFont_Spacing::default()];
                provider.GetSpacing(gfxTextRun_Range::new(lineEnd, lineEnd + 1), &mut spacing);
                aData.mCurrentLine += spacing[0].mBefore;
                if tabWidth < 0.0 {
                    tabWidth = ComputeTabWidthAppUnits(self.0);
                }
                let afterTab = AdvanceToNextTab(aData.mCurrentLine as gfxFloat, tabWidth, provider.MinTabAdvance());
                aData.mCurrentLine = (afterTab + spacing[0].mAfter as f64) as nscoord;
                aData.mLineIsEmpty = false;
                lineStart = lineEnd + 1;
            } else if preformattedNewline {
                aData.ForceBreak(UsedClear::Both);
                lineStart = lineEnd;
            }
            g += 1;
        }

        // Check if we have collapsible whitespace at the end
        if start < flowEndInTextRun {
            aData.mSkipWhitespace = IsTrimmableSpace(provider.GetCharacterDataBuffer(), iter.ConvertSkippedToOriginal(flowEndInTextRun - 1) as u32, &textStyle, false);
        }
        let _ = aRenderingContext;
    }

    // XXX Need to do something here to avoid incremental reflow bugs due to
    // first-line and first-letter changing pref-width
    /* virtual */
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::AddInlinePrefISize
    pub fn AddInlinePrefISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlinePrefISizeData) {
        // nsContinuingTextFrame::AddInlinePrefISize: Do nothing, since the first-in-flow accounts for everything.
        if self.with_data(|d| d.mIsContinuing) {
            return;
        }

        let inflation = nsLayoutUtils::FontSizeInflationFor(self.0);
        let trtype = if inflation == 1.0 { nsTextFrame_TextRunType::eNotInflated } else { nsTextFrame_TextRunType::eInflated };

        if trtype == nsTextFrame_TextRunType::eInflated && !self.IsCurrentFontInflation(inflation) {
            // FIXME: Ideally, if we already have a text run, we'd move it to be
            // the uninflated text run.
            self.ClearTextRun(None, nsTextFrame_TextRunType::eInflated);
            self.with_data(|d| d.mFontMetrics = None);
        }

        let mut lastTextRun: Option<RefPtr_gfxTextRun> = None;
        // nsContinuingTextFrame does nothing for AddInlineMinISize; all text frames
        // in the flow are handled right here.
        let mut f = Some(*self);
        while let Some(fr) = f {
            // f->GetTextRun(nsTextFrame::eNotInflated) could be null if we
            // haven't set up textruns yet for f.  Except in OOM situations,
            // lastTextRun will only be null for the first text frame.
            let same = match (&fr.GetTextRun(trtype), &lastTextRun) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if fr == *self || !same {
                if let Some(container) = aData.LineContainer() {
                    let lc = fr.FindLineContainer();
                    if Some(container) != lc {
                        NS_ASSERTION!(fr != *self, "wrong InlinePrefISizeData container for first continuation");
                        aData.mLine = None;
                        aData.SetLineContainer(lc);
                    }
                }

                // This will process all the text frames that share the same textrun as f.
                fr.AddInlinePrefISizeForFlow(aInput.mContext, aData, trtype);
                lastTextRun = fr.GetTextRun(trtype);
            }
            f = fr.GetNextContinuation().map(nsTextFrame);
        }
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ComputeSize
    // (the `match` on the frame type in nsIFrame::ComputeSize already answers for Text)

    // Gecko: layout/generic/nsTextFrame.cpp RoundOut, nsTextFrame::ComputeTightBounds, GetPrefWidthTightBounds
    // NOT PORTED: tight bounds (used by the first-letter and by the accessibility rects).
}

// Gecko: layout/generic/nsTextFrame.cpp RoundOut
fn RoundOut(aRect: &gfxRect) -> nsRect {
    let x = NSToCoordFloor__float(aRect.X() as f32);
    let y = NSToCoordFloor__float(aRect.Y() as f32);
    nsRect::new(x, y, NSToCoordCeil__float(aRect.XMost() as f32) - x, NSToCoordCeil__float(aRect.YMost() as f32) - y)
}

// Gecko: layout/generic/nsTextFrame.cpp HasSoftHyphenBefore
fn HasSoftHyphenBefore(aBuffer: &CharacterDataBuffer, aTextRun: &RefPtr_gfxTextRun, aStartOffset: i32, aIter: &gfxSkipCharsIterator) -> bool {
    if aIter.GetSkippedOffset() < aTextRun.GetLength() && aTextRun.CanHyphenateBefore(aIter.GetSkippedOffset()) {
        return true;
    }
    if aTextRun.GetFlags2().0 & nsTextFrameUtils_Flags::HasShy.0 == 0 {
        return false;
    }
    let mut iter = *aIter;
    while iter.GetOriginalOffset() > aStartOffset {
        iter.AdvanceOriginal(-1);
        if !iter.IsOriginalCharSkipped(None) {
            break;
        }
        if aBuffer.CharAt(iter.GetOriginalOffset() as u32) as u32 == CH_SHY {
            return true;
        }
    }
    false
}

/**
 * Removes all frames from aFrame up to (but not including) aFirstToNotRemove,
 * because their text has all been taken and reflowed by earlier frames.
 */
// Gecko: layout/generic/nsTextFrame.cpp RemoveEmptyInFlows
fn RemoveEmptyInFlows(aFrame: nsTextFrame, aFirstToNotRemove: nsTextFrame) {
    MOZ_ASSERT!(aFrame != aFirstToNotRemove, "This will go very badly");
    // We have to be careful here, because some RemoveFrame implementations
    // remove and destroy not only the passed-in frame but also all its following
    // in-flows (and sometimes all its following continuations in general).  So
    // we remove |f| and everything up to but not including firstToNotRemove from
    // the flow first, to make sure that only the things we want destroyed are
    // destroyed.

    // This sadly duplicates some of the logic from
    // nsSplittableFrame::RemoveFromFlow.  We can get away with not duplicating
    // all of it, because we know that the prev-continuation links of
    // firstToNotRemove and f are fluid, and non-null.
    NS_ASSERTION!(aFirstToNotRemove.GetPrevContinuation() == aFirstToNotRemove.GetPrevInFlow() && aFirstToNotRemove.GetPrevInFlow().is_some(), "aFirstToNotRemove should have a fluid prev continuation");
    NS_ASSERTION!(aFrame.GetPrevContinuation() == aFrame.GetPrevInFlow() && aFrame.GetPrevInFlow().is_some(), "aFrame should have a fluid prev continuation");

    let prevContinuation = nsTextFrame(aFrame.GetPrevContinuation().unwrap());
    let lastRemoved = nsTextFrame(aFirstToNotRemove.GetPrevContinuation().unwrap());

    let mut f = Some(aFrame);
    while f != Some(aFirstToNotRemove) {
        let fr = f.unwrap();
        // f is going to be destroyed soon, after it is unlinked from the
        // continuation chain. If its textrun is going to be destroyed we need to
        // do it now, before we unlink the frames to remove from the flow,
        // because Destroy calls ClearTextRuns() and that will start at the
        // first frame with the text run and walk the continuations.
        if fr.IsInTextRunUserData() {
            fr.ClearTextRuns();
        } else {
            fr.DisconnectTextRuns();
        }
        f = fr.GetNextContinuation().map(nsTextFrame);
    }

    prevContinuation.SetNextInFlow(Some(aFirstToNotRemove.0));
    aFirstToNotRemove.SetPrevInFlow(Some(prevContinuation.0));

    // **Note: it is important here that we clear the Next link from lastRemoved
    // BEFORE clearing the Prev link from aFrame, because SetPrevInFlow() will
    // follow the Next pointers, wiping out the cached mFirstContinuation field
    // from each following frame in the list. We need this to stop when it
    // reaches lastRemoved!
    lastRemoved.SetNextInFlow(None);
    aFrame.SetPrevInFlow(None);

    let parent = aFrame.GetParent().unwrap();
    let mut context = FrameDestroyContext::default();
    let parentBlock = nsBlockFrame::do_QueryFrame(parent);
    if let Some(parentBlock) = parentBlock {
        // Manually call DoRemoveFrame so we can tell it that we're
        // removing empty frames; this will keep it from blowing away
        // text runs.
        parentBlock.DoRemoveFrame(&mut context, aFrame.0, nsBlockFrame::FRAMES_ARE_EMPTY);
    } else {
        // Just remove it normally; use FrameChildListID::NoReflowPrincipal to avoid
        // posting new reflows.
        parent.RemoveFrame(&mut context, FrameChildListID::NoReflowPrincipal, aFrame.0);
    }
}

// Flags for aSetLengthFlags
// Gecko: layout/generic/nsTextFrame.h nsTextFrame::ALLOW_FRAME_CREATION_AND_DESTRUCTION
pub const nsTextFrame_ALLOW_FRAME_CREATION_AND_DESTRUCTION: u32 = 0x01;

impl nsTextFrame {
    // Update offsets to account for new length. This may clear mTextRun.
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::SetLength
    pub fn SetLength(&self, aLength: i32, aLineLayout: Option<&mut nsLineLayout>, aSetLengthFlags: u32) {
        let mut aLineLayout = aLineLayout;
        self.with_data(|d| d.mContentLengthHint = aLength);
        let end = self.GetContentOffset() + aLength;
        let mut f = self.GetNextInFlow().map(nsTextFrame);
        if f.is_none() {
            return;
        }

        // If our end offset is moving, then even if frames are not being pushed or
        // pulled, content is moving to or from the next line and the next line
        // must be reflowed.
        // If the next-continuation is dirty, then we should dirty the next line now
        // because we may have skipped doing it if we dirtied it in
        // CharacterDataChanged. This is ugly but teaching FrameNeedsReflow
        // and ChildIsDirty to handle a range of frames would be worse.
        if let Some(ll) = aLineLayout.as_deref_mut() {
            let fr = f.unwrap();
            if end != fr.GetContentOffset() || fr.HasAnyStateBits(NS_FRAME_IS_DIRTY) {
                ll.SetDirtyNextLine();
            }
        }

        if end < f.unwrap().GetContentOffset() {
            // Our frame is shrinking. Give the text to our next in flow.
            if let Some(ll) = aLineLayout.as_deref_mut() {
                if self.HasSignificantTerminalNewline() && !self.GetParent().unwrap().IsLetterFrame() && (aSetLengthFlags & nsTextFrame_ALLOW_FRAME_CREATION_AND_DESTRUCTION) != 0 {
                    // Whatever text we hand to our next-in-flow will end up in a frame all of
                    // its own, since it ends in a forced linebreak.  Might as well just put
                    // it in a separate frame now.  This is important to prevent text run
                    // churn; if we did not do that, then we'd likely end up rebuilding
                    // textruns for all our following continuations.
                    // We skip this optimization when the parent is a first-letter frame
                    // because it doesn't deal well with more than one child frame.
                    // We also skip this optimization if we were called during bidi
                    // resolution, so as not to create a new frame which doesn't appear in
                    // the bidi resolver's list of frames
                    let newFrame = nsCSSFrameConstructor_CreateContinuingFrame(self.0, self.GetParent().unwrap(), true);
                    let next = nsTextFrame(newFrame);
                    self.GetParent().unwrap().InsertFrames(FrameChildListID::NoReflowPrincipal, Some(self.0), ll.GetLine(), nsFrameList::new(Some(next.0), Some(next.0)));
                    f = Some(next);
                }
            }

            let fr = f.unwrap();
            fr.set_mContentOffset(end);
            let sameRun = match (fr.GetTextRun(nsTextFrame_TextRunType::eInflated), self.GetTextRun(nsTextFrame_TextRunType::eInflated)) {
                (Some(a), Some(b)) => Rc::ptr_eq(&a, &b),
                (None, None) => true,
                _ => false,
            };
            if !sameRun {
                self.ClearTextRuns();
                fr.ClearTextRuns();
            }
            return;
        }
        // Our frame is growing. Take text from our in-flow(s).
        // We can take text from frames in lines beyond just the next line.
        // We don't dirty those lines. That's OK, because when we reflow
        // our empty next-in-flow, it will take text from its next-in-flow and
        // dirty that line.

        // Note that in the process we may end up removing some frames from
        // the flow if they end up empty.
        let mut framesToRemove: Option<nsTextFrame> = None;
        while let Some(fr) = f {
            if fr.GetContentOffset() >= end {
                break;
            }
            fr.set_mContentOffset(end);
            let sameRun = match (fr.GetTextRun(nsTextFrame_TextRunType::eInflated), self.GetTextRun(nsTextFrame_TextRunType::eInflated)) {
                (Some(a), Some(b)) => Rc::ptr_eq(&a, &b),
                (None, None) => true,
                _ => false,
            };
            if !sameRun {
                self.ClearTextRuns();
                fr.ClearTextRuns();
            }
            let next = fr.GetNextInFlow().map(nsTextFrame);
            // Note: the "f->GetNextSibling() == next" check below is to restrict
            // this optimization to the case where they are on the same child list.
            // Otherwise we might remove the only child of a nsFirstLetterFrame
            // for example and it can't handle that.  See bug 597627 for details.
            if next.is_some() && next.unwrap().GetContentOffset() <= end && fr.GetNextSibling() == next.map(|n| n.0) && (aSetLengthFlags & nsTextFrame_ALLOW_FRAME_CREATION_AND_DESTRUCTION) != 0 {
                // |f| is now empty.  We may as well remove it, instead of copying all
                // the text from |next| into it instead; the latter leads to use
                // rebuilding textruns for all following continuations.
                // We skip this optimization if we were called during bidi resolution,
                // since the bidi resolver may try to handle the destroyed frame later
                // and crash
                if framesToRemove.is_none() {
                    // Remember that we have to remove this frame.
                    framesToRemove = Some(fr);
                }
            } else if let Some(ftr) = framesToRemove {
                RemoveEmptyInFlows(ftr, fr);
                framesToRemove = None;
            }
            f = next;
        }

        MOZ_ASSERT!(framesToRemove.is_none() || (f.is_some() && f.unwrap().GetContentOffset() == end), "How did we exit the loop if we null out framesToRemove if !next || next->mContentOffset > end ?");

        if let Some(ftr) = framesToRemove {
            // We are guaranteed that we exited the loop with f not null, per the
            // postcondition above
            RemoveEmptyInFlows(ftr, f.unwrap());
        }

        #[cfg(debug_assertions)]
        {
            let mut f = Some(*self);
            let mut iterations = 0;
            while let Some(fr) = f {
                if iterations >= 10 {
                    break;
                }
                fr.GetContentLength(); // Assert if negative length
                f = fr.GetNextContinuation().map(nsTextFrame);
                iterations += 1;
            }
            let mut f = Some(*self);
            let mut iterations = 0;
            while let Some(fr) = f {
                if iterations >= 10 {
                    break;
                }
                fr.GetContentLength(); // Assert if negative length
                f = fr.GetPrevContinuation().map(nsTextFrame);
                iterations += 1;
            }
        }
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::SetFirstLetterLength
    // NOT PORTED: ::first-letter.

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::IsFloatingFirstLetterChild
    pub fn IsFloatingFirstLetterChild(&self) -> bool {
        let frame = self.GetParent();
        frame.map_or(false, |f| f.IsFloating() && f.IsLetterFrame())
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::IsInitialLetterChild
    // NOT PORTED: `initial-letter` is not in the servo configuration of Stylo.
    pub fn IsInitialLetterChild(&self) -> bool {
        false
    }
}

// Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::NewlineProperty
// ADAPTER: the property is cached on the content in Gecko (nsGkAtoms::newline); the frames of one
// content are not many, so the cache is dropped and the search is repeated (NOT PORTED: the cache).
#[derive(Clone, Copy, Debug)]
pub struct nsTextFrame_NewlineProperty {
    pub mStartOffset: i32,
    // The offset of the first \n after mStartOffset, or -1 if there is none
    pub mNewlineOffset: i32,
}

impl nsTextFrame {
    // Return the content offset of the first preserved newline in this frame,
    // or return -1 if no preserved NL.
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetContentNewLineOffset
    pub fn GetContentNewLineOffset(&self, aOffset: i32, aCachedNewlineOffset: &mut Option<nsTextFrame_NewlineProperty>) -> i32 {
        let mut contentNewLineOffset: i32 = -1; // this will be -1 or a content offset
        if nsStyleTextAdapter::of(self.0).NewlineIsSignificant(*self) {
            // Pointer to the nsGkAtoms::newline set on this frame's element
            *aCachedNewlineOffset = None; // NOT PORTED: the cache
            contentNewLineOffset = FindChar(&self.CharacterDataBuffer(), aOffset, self.GetContent().unwrap().TextLength() as i32 - aOffset, 0x0A);
        }

        contentNewLineOffset
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aMetrics: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        // DO_GLOBAL_REFLOW_COUNT: NOT PORTED
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        // InvalidateSelectionState(): NOT PORTED

        // XXX If there's no line layout, we shouldn't even have created this
        // frame. This may happen if, for example, this is text inside a table
        // but not inside a cell. For now, just don't reflow.
        if aReflowInput.mLineLayout.is_null() {
            self.ClearMetrics(aMetrics);
            return;
        }

        self.ReflowText(unsafe { &mut *aReflowInput.mLineLayout }, aReflowInput.AvailableWidth(), aMetrics, aStatus);
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ReflowText
    // (the DrawTarget parameter is dropped: there is no drawing)
    pub fn ReflowText(&self, aLineLayout: &mut nsLineLayout, aAvailableWidth: nscoord, aMetrics: &mut ReflowOutput, aStatus: &mut nsReflowStatus) {
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        let presContext = self.PresContext();

        // (ReflowTextA11yNotifier: NOT PORTED, accessibility)

        /////////////////////////////////////////////////////////////////////
        // Set up flags and clear out state
        /////////////////////////////////////////////////////////////////////

        // Clear out the reflow input flags in mState. We also clear the whitespace
        // flags because this can change whether the frame maps whitespace-only text
        // or not. We also clear the flag that tracks whether we had a pending
        // reflow request from CharacterDataChanged (since we're reflowing now).
        self.RemoveStateBits(TEXT_REFLOW_FLAGS | TEXT_WHITESPACE_FLAGS);
        self.with_data(|d| d.mReflowRequestedForCharDataChange = false);
        // RemoveProperty(WebRenderTextBounds()): NOT PORTED

        // Discard cached continuations array that will be invalidated by the reflow.
        // (NOT PORTED: the cached continuations array)

        // Temporarily map all possible content while we construct our new textrun.
        // so that when doing reflow our styles prevail over any part of the
        // textrun we look at. Note that next-in-flows may be mapping the same
        // content; gfxTextRun construction logic will ensure that we take priority.
        let maxContentLength = self.GetInFlowContentLength();

        // InvalidateSelectionState(): NOT PORTED

        // We don't need to reflow if there is no content.
        if maxContentLength == 0 {
            self.ClearMetrics(aMetrics);
            return;
        }

        let textStyle = nsStyleTextAdapter::of(self.0);

        let atStartOfLine = aLineLayout.LineAtStart();
        if atStartOfLine {
            self.AddStateBits(TEXT_START_OF_LINE);
        }

        let mut flowEndInTextRun: u32 = 0;
        let lineContainer = aLineLayout.LineContainerFrame();
        let characterDataBuffer = self.CharacterDataBuffer();

        // DOM offsets of the text range we need to measure, after trimming
        // whitespace, restricting to first-letter, and restricting preformatted text
        // to nearest newline
        let mut length = maxContentLength;
        let mut offset = self.GetContentOffset();

        // Restrict preformatted text to the nearest newline
        let mut cachedNewlineOffset: Option<nsTextFrame_NewlineProperty> = None;
        let mut newLineOffset: i32 = -1; // this will be -1 or a content offset
        // This will just return -1 if newlines are not significant.
        let contentNewLineOffset = self.GetContentNewLineOffset(offset, &mut cachedNewlineOffset);
        if contentNewLineOffset < offset + length {
            // The new line offset could be outside this frame if the frame has been
            // split by bidi resolution. In that case we won't use it in this reflow
            // (newLineOffset will remain -1), but we will still cache it in mContent
            newLineOffset = contentNewLineOffset;
        }
        if newLineOffset >= 0 {
            length = newLineOffset + 1 - offset;
        }

        if (atStartOfLine && !textStyle.WhiteSpaceIsSignificant()) || self.HasAnyStateBits(TEXT_IS_IN_TOKEN_MATHML) {
            // Skip leading whitespace. Make sure we don't skip a 'pre-line'
            // newline if there is one.
            let skipLength = if newLineOffset >= 0 { length - 1 } else { length };
            let whitespaceCount = GetTrimmableWhitespaceCount(&characterDataBuffer, offset, skipLength, 1) as i32;
            if whitespaceCount != 0 {
                offset += whitespaceCount;
                length -= whitespaceCount;
                // Make sure this frame maps the trimmable whitespace.
                if offset > self.GetContentEnd() {
                    self.SetLength(offset - self.GetContentOffset(), Some(&mut *aLineLayout), nsTextFrame_ALLOW_FRAME_CREATION_AND_DESTRUCTION);
                }
            }
        }

        // If trimming whitespace left us with nothing to do, return early.
        if length == 0 {
            self.ClearMetrics(aMetrics);
            return;
        }

        let completedFirstLetter = false;
        // Layout dependent styles are a problem because we need to reconstruct
        // the gfxTextRun based on our layout.
        if aLineLayout.GetInFirstLetter() || aLineLayout.GetInFirstLine() {
            self.SetLength(maxContentLength, Some(&mut *aLineLayout), nsTextFrame_ALLOW_FRAME_CREATION_AND_DESTRUCTION);

            if aLineLayout.GetInFirstLetter() {
                // NOT PORTED: ::first-letter (FindFirstLetterRange; nsTextFrame.cpp lines 10909-10963
                // at GECKO_REV). The first-letter frame is never created, so GetInFirstLetter is false.
                unimplemented!("NOT PORTED: ::first-letter");
            }
        }

        let fontSizeInflation = nsLayoutUtils::FontSizeInflationFor(self.0);

        if !self.IsCurrentFontInflation(fontSizeInflation) {
            // FIXME: Ideally, if we already have a text run, we'd move it to be
            // the uninflated text run.
            self.ClearTextRun(None, nsTextFrame_TextRunType::eInflated);
            self.with_data(|d| d.mFontMetrics = None);
        }

        let mut iter = self.EnsureTextRun(nsTextFrame_TextRunType::eInflated, Some(lineContainer), aLineLayout.GetLine().copied(), Some(&mut flowEndInTextRun));

        NS_ASSERTION!(self.IsCurrentFontInflation(fontSizeInflation), "EnsureTextRun should have set font size inflation");

        if let Some(mTextRun) = self.GetTextRun(nsTextFrame_TextRunType::eInflated) {
            if iter.GetOriginalEnd() < offset + length {
                // The textrun does not map enough text for this frame. This can happen
                // when the textrun was ended in the middle of a text node because a
                // preformatted newline was encountered, and prev-in-flow frames have
                // consumed all the text of the textrun. We need a new textrun.
                self.ClearTextRuns();
                iter = self.EnsureTextRun(nsTextFrame_TextRunType::eInflated, Some(lineContainer), aLineLayout.GetLine().copied(), Some(&mut flowEndInTextRun));
            }
            let _ = mTextRun;
        }

        let Some(mTextRun) = self.GetTextRun(nsTextFrame_TextRunType::eInflated) else {
            self.ClearMetrics(aMetrics);
            return;
        };

        NS_ASSERTION!({ let mut it = iter; it.ConvertOriginalToSkipped(offset + length) <= mTextRun.GetLength() }, "Text run does not map enough text for our reflow");

        /////////////////////////////////////////////////////////////////////
        // See how much text should belong to this text frame, and measure it
        /////////////////////////////////////////////////////////////////////

        iter.SetOriginalOffset(offset);
        let xOffsetForTabs: nscoord = if mTextRun.GetFlags2().0 & nsTextFrameUtils_Flags::HasTab.0 != 0 { aLineLayout.GetCurrentFrameInlineDistanceFromBlock() - lineContainer.GetUsedBorderAndPadding().left } else { -1 };
        let provider = nsTextFrame_PropertyProvider::new__gfxTextRun_nsStyleText_CharacterDataBuffer_nsTextFrame_gfxSkipCharsIterator_int32_t_nsIFrame_nscoord_TextRunType_bool(
            mTextRun.clone(),
            textStyle.clone(),
            characterDataBuffer.clone(),
            *self,
            iter,
            length,
            Some(lineContainer),
            xOffsetForTabs,
            nsTextFrame_TextRunType::eInflated,
            self.HasAnyStateBits(TEXT_START_OF_LINE),
        );

        let transformedOffset = provider.GetStart().GetSkippedOffset();

        let mut boundingBoxType = gfxFont_BoundingBoxType::LOOSE_INK_EXTENTS;
        if self.IsFloatingFirstLetterChild() || self.IsInitialLetterChild() {
            // if (nsFirstLetterFrame* firstLetter = do_QueryFrame(GetParent())) { if (firstLetter->UseTightBounds()) ... }
            // NOT PORTED: ::first-letter
        }

        let mut limitLength = length;
        let mut forceBreak = aLineLayout.GetForcedBreakPosition(self.0);
        let mut forceBreakAfter = false;
        if forceBreak >= length {
            forceBreakAfter = forceBreak == length;
            // The break is not within the text considered for this textframe.
            forceBreak = -1;
        }
        if forceBreak >= 0 {
            limitLength = forceBreak;
        }
        // This is the heart of text reflow right here! We don't know where
        // to break, so we need to see how much text fits in the available width.
        let transformedLength: u32;
        if offset + limitLength >= characterDataBuffer.GetLength() as i32 {
            NS_ASSERTION!(offset + limitLength == characterDataBuffer.GetLength() as i32, "Content offset/length out of bounds");
            NS_ASSERTION!(flowEndInTextRun >= transformedOffset, "Negative flow length?");
            transformedLength = flowEndInTextRun - transformedOffset;
        } else {
            // we're not looking at all the content, so we need to compute the
            // length of the transformed substring we're looking at
            let mut iter = *provider.GetStart();
            iter.SetOriginalOffset(offset + limitLength);
            transformedLength = iter.GetSkippedOffset() - transformedOffset;
        }
        let mut textMetrics = gfxFont_RunMetrics::default();
        let mut transformedLastBreak: u32 = 0;
        let mut usedHyphenation = false;
        let mut trimmableWS = gfxTextRun_TrimmableWS::default();
        let availWidth: gfxFloat = aAvailableWidth as gfxFloat;
        // if (Style()->IsTextCombined()) availWidth = infinity: NOT PORTED
        let canTrimTrailingWhitespace = !textStyle.WhiteSpaceIsSignificant() || self.HasAnyStateBits(TEXT_IS_IN_TOKEN_MATHML);
        let isBreakSpaces = textStyle.WhiteSpaceCollapseIsBreakSpaces();
        // allow whitespace to overflow the container
        let whitespaceCanHang = textStyle.WhiteSpaceCanHangOrVisuallyCollapse();
        let mut breakPriority = aLineLayout.LastOptionalBreakPriority();
        let mut suppressBreak = gfxTextRun_SuppressBreak::eNoSuppressBreak;
        let shouldSuppressLineBreak = self.ShouldSuppressLineBreak();
        if shouldSuppressLineBreak {
            suppressBreak = gfxTextRun_SuppressBreak::eSuppressAllBreaks;
        } else if !aLineLayout.LineIsBreakable() {
            suppressBreak = gfxTextRun_SuppressBreak::eSuppressInitialBreak;
        }
        let transformedCharsFit = mTextRun.BreakAndMeasureText(
            transformedOffset,
            transformedLength,
            self.HasAnyStateBits(TEXT_START_OF_LINE),
            availWidth,
            &provider,
            suppressBreak,
            boundingBoxType,
            textStyle.WordCanWrap(self.0),
            textStyle.WhiteSpaceCanWrap(self.0),
            isBreakSpaces,
            // The following are output parameters:
            if canTrimTrailingWhitespace || whitespaceCanHang { Some(&mut trimmableWS) } else { None },
            &mut textMetrics,
            &mut usedHyphenation,
            &mut transformedLastBreak,
            // In/out
            &mut breakPriority,
        );
        if length == 0 && textMetrics.mAscent == 0.0 && textMetrics.mDescent == 0.0 {
            // If we're measuring a zero-length piece of text, update
            // the height manually.
            let fm = provider.GetFontMetrics();
            textMetrics.mAscent = fm.MaxAscent() as gfxFloat;
            textMetrics.mDescent = fm.MaxDescent() as gfxFloat;
        }
        if self.GetWritingMode().IsLineInverted() {
            std::mem::swap(&mut textMetrics.mAscent, &mut textMetrics.mDescent);
            textMetrics.mBoundingBox.y = -textMetrics.mBoundingBox.YMost();
        }
        // The "end" iterator points to the first character after the string mapped
        // by this frame. Basically, its original-string offset is offset+charsFit
        // after we've computed charsFit.
        let mut end = *provider.GetEndHint();
        end.SetSkippedOffset(transformedOffset + transformedCharsFit);
        let mut charsFit = end.GetOriginalOffset() - offset;
        if offset + charsFit == newLineOffset {
            // We broke before a trailing preformatted '\n'. The newline should
            // be assigned to this frame. Note that newLineOffset will be -1 if
            // there was no preformatted newline, so we wouldn't get here in that
            // case.
            charsFit += 1;
        }
        // That might have taken us beyond our assigned content range (because
        // we might have advanced over some skipped chars that extend outside
        // this frame), so get back in.
        let mut lastBreak: i32 = -1;
        if charsFit >= limitLength {
            charsFit = limitLength;
            if transformedLastBreak != u32::MAX {
                // lastBreak is needed.
                // This may set lastBreak greater than 'length', but that's OK
                lastBreak = end.ConvertSkippedToOriginal(transformedOffset + transformedLastBreak);
            }
            end.SetOriginalOffset(offset + charsFit);
            // If we were forced to fit, and the break position is after a soft hyphen,
            // note that this is a hyphenation break.
            if (forceBreak >= 0 || forceBreakAfter) && HasSoftHyphenBefore(&characterDataBuffer, &mTextRun, offset, &end) {
                usedHyphenation = true;
            }
        }
        if usedHyphenation {
            // Fix up metrics to include hyphen
            // AddHyphenToMetrics(this, mTextRun->IsRightToLeft(), &textMetrics, boundingBoxType, aDrawTarget)
            // NOT PORTED: hyphenation (the hyphen has no width)
            self.AddStateBits(TEXT_HYPHEN_BREAK | TEXT_HAS_NONCOLLAPSED_CHARACTERS);
        }
        if textMetrics.mBoundingBox.IsEmpty() {
            self.AddStateBits(TEXT_NO_RENDERED_GLYPHS);
        }

        let brokeText = forceBreak >= 0 || transformedCharsFit < transformedLength;
        if trimmableWS.mAdvance > 0.0 {
            if canTrimTrailingWhitespace {
                // Optimization: if we we can be sure this frame will be at end of line,
                // then trim the whitespace now.
                if brokeText || self.HasAnyStateBits(TEXT_IS_IN_TOKEN_MATHML) {
                    // We're definitely going to break so our trailing whitespace should
                    // definitely be trimmed. Record that we've already done it.
                    self.AddStateBits(TEXT_TRIMMED_TRAILING_WHITESPACE);
                    textMetrics.mAdvanceWidth -= trimmableWS.mAdvance;
                    trimmableWS.mAdvance = 0.0;
                }
                self.ClearHangableISize();
                self.ClearTrimmableWS();
            } else if whitespaceCanHang {
                // Figure out how much whitespace will hang if at end-of-line.
                let hang = f64::min(f64::max(0.0, textMetrics.mAdvanceWidth - availWidth), trimmableWS.mAdvance);
                self.SetHangableISize(NSToCoordRound__float((trimmableWS.mAdvance - hang) as f32));
                // nsLineLayout only needs the TrimmableWS property if justifying, so
                // check whether this is relevant.
                if textStyle.mTextAlignIsJustify() || textStyle.mTextAlignLastIsJustify() {
                    self.SetTrimmableWS(trimmableWS);
                }
                textMetrics.mAdvanceWidth -= hang;
                trimmableWS.mAdvance = 0.0;
            } else {
                MOZ_ASSERT_UNREACHABLE!("How did trimmableWS get set?!");
                self.ClearHangableISize();
                self.ClearTrimmableWS();
                trimmableWS.mAdvance = 0.0;
            }
        } else {
            // Remove any stale frame properties.
            self.ClearHangableISize();
            self.ClearTrimmableWS();
        }

        if !brokeText && lastBreak >= 0 {
            // Since everything fit and no break was forced,
            // record the last break opportunity
            NS_ASSERTION!(textMetrics.mAdvanceWidth - trimmableWS.mAdvance <= availWidth, "If the text doesn't fit, and we have a break opportunity, why didn't MeasureText use it?");
            MOZ_ASSERT!(lastBreak >= offset, "Strange break position");
            aLineLayout.NotifyOptionalBreakPosition(Some(self.0), lastBreak - offset, true, breakPriority);
        }

        let contentLength = offset + charsFit - self.GetContentOffset();

        /////////////////////////////////////////////////////////////////////
        // Compute output metrics
        /////////////////////////////////////////////////////////////////////

        // first-letter frames should use the tight bounding box metrics for
        // ascent/descent for good drop-cap effects
        if self.HasAnyStateBits(TEXT_FIRST_LETTER) {
            textMetrics.mAscent = f64::max(0.0, -textMetrics.mBoundingBox.Y());
            textMetrics.mDescent = f64::max(0.0, textMetrics.mBoundingBox.YMost());
        }

        // Setup metrics for caller
        // Disallow negative widths
        let wm = self.GetWritingMode();
        let mut finalSize = LogicalSize::new(wm, 0, 0);
        *finalSize.ISize_mut(wm) = NSToCoordCeilClamped(f64::max(0.0, textMetrics.mAdvanceWidth));

        let mut fontBaseline: nscoord;
        // Note(dshin): Baseline should tecnhically be halfway through the em box for
        // a central baseline. It is simply half of the text run block size so that it
        // can be easily calculated in `GetNaturalBaselineBOffset`.
        if transformedCharsFit == 0 && !usedHyphenation {
            aMetrics.SetBlockStartAscent(0);
            *finalSize.BSize_mut(wm) = 0;
            fontBaseline = 0;
        } else if boundingBoxType != gfxFont_BoundingBoxType::LOOSE_INK_EXTENTS {
            fontBaseline = NSToCoordCeil__float(textMetrics.mAscent as f32);
            let size = fontBaseline + NSToCoordCeil__float(textMetrics.mDescent as f32);
            // Use actual text metrics for floating first letter frame.
            aMetrics.SetBlockStartAscent(if wm.IsAlphabeticalBaseline() { fontBaseline } else { size / 2 });
            *finalSize.BSize_mut(wm) = size;
        } else {
            // Otherwise, ascent should contain the overline drawable area.
            // And also descent should contain the underline drawable area.
            // nsFontMetrics::GetMaxAscent/GetMaxDescent contains them.
            let fm = provider.GetFontMetrics();
            let fontAscent = if wm.IsLineInverted() { fm.MaxDescent() } else { fm.MaxAscent() };
            let fontDescent = if wm.IsLineInverted() { fm.MaxAscent() } else { fm.MaxDescent() };
            fontBaseline = std::cmp::max(NSToCoordCeil__float(textMetrics.mAscent as f32), fontAscent);
            let size = fontBaseline + std::cmp::max(NSToCoordCeil__float(textMetrics.mDescent as f32), fontDescent);
            aMetrics.SetBlockStartAscent(if wm.IsAlphabeticalBaseline() { fontBaseline } else { size / 2 });
            *finalSize.BSize_mut(wm) = size;
        }
        // if (Style()->IsTextCombined()) { ... }: NOT PORTED, text-combine-upright
        aMetrics.SetSize(wm, &finalSize);

        NS_ASSERTION!(aMetrics.BlockStartAscent() >= 0, "Negative ascent???");
        // The effective "descent" here will be negative in the case of a frame that
        // is participating in text-combine-upright, but is not the last frame within
        // the combined upright cell, because we zero out its block-size (see above).
        // So this creates an exception to the non-negative assertion here.
        #[cfg(debug_assertions)]
        {
            let descent = aMetrics.BSize(aMetrics.GetWritingMode()) - aMetrics.BlockStartAscent();
            NS_ASSERTION!(descent >= 0, "Unexpected negative descent???");
        }

        self.with_data(|d| d.mAscent = fontBaseline);

        // Handle text that runs outside its normal bounds.
        let mut boundingBox = RoundOut(&textMetrics.mBoundingBox);
        if mTextRun.IsVertical() {
            // Swap line-relative textMetrics dimensions to physical coordinates.
            // NOT PORTED: vertical text
            unimplemented!("NOT PORTED: vertical text")
        } else {
            boundingBox.y += self.mAscent();
        }
        aMetrics.SetOverflowAreasToDesiredBounds();
        let ink = *aMetrics.InkOverflow();
        aMetrics.InkOverflow_mut().UnionRect(ink, boundingBox);

        // When we have text decorations, we don't need to compute their overflow now
        // because we're guaranteed to do it later
        // (see nsLineLayout::RelativePositionFrames)
        // UnionAdditionalOverflow(presContext, aLineLayout.LineContainerFrame(), provider, &aMetrics.InkOverflow(), false, true)
        // NOT PORTED: text decorations and shadows (nsTextFrame.cpp lines 6006-6205 at GECKO_REV)
        let _ = presContext;

        /////////////////////////////////////////////////////////////////////
        // Clean up, update state
        /////////////////////////////////////////////////////////////////////

        // If all our characters are discarded or collapsed, then trimmable width
        // from the last textframe should be preserved. Otherwise the trimmable width
        // from this textframe overrides. (Currently in CSS trimmable width can be
        // at most one space so there's no way for trimmable width from a previous
        // frame to accumulate with trimmable width from this frame.)
        if transformedCharsFit > 0 {
            aLineLayout.SetTrimmableISize(NSToCoordFloor__float(trimmableWS.mAdvance as f32));
            self.AddStateBits(TEXT_HAS_NONCOLLAPSED_CHARACTERS);
        }
        let mut breakAfter = forceBreakAfter;
        if !shouldSuppressLineBreak {
            if charsFit > 0 && charsFit == length && textStyle.mHyphens() != StyleHyphens::None && HasSoftHyphenBefore(&characterDataBuffer, &mTextRun, offset, &end) {
                let fits = textMetrics.mAdvanceWidth + provider.GetHyphenWidth() <= availWidth;
                // Record a potential break after final soft hyphen
                aLineLayout.NotifyOptionalBreakPosition(Some(self.0), length, fits, gfxBreakPriority::eNormalBreak);
            }
            // length == 0 means either the text is empty or it's all collapsed away
            let emptyTextAtStartOfLine = atStartOfLine && length == 0;
            if !breakAfter && charsFit == length && !emptyTextAtStartOfLine && transformedOffset + transformedLength == mTextRun.GetLength() && (mTextRun.GetFlags2().0 & nsTextFrameUtils_Flags::HasTrailingBreak.0) != 0 {
                // We placed all the text in the textrun and we have a break opportunity
                // at the end of the textrun. We need to record it because the following
                // content may not care about nsLineBreaker.

                // Note that because we didn't break, we can be sure that (thanks to the
                // code up above) textMetrics.mAdvanceWidth includes the width of any
                // trailing whitespace. So we need to subtract trimmableWidth here
                // because if we did break at this point, that much width would be
                // trimmed.
                if textMetrics.mAdvanceWidth - trimmableWS.mAdvance > availWidth {
                    breakAfter = true;
                } else {
                    aLineLayout.NotifyOptionalBreakPosition(Some(self.0), length, true, gfxBreakPriority::eNormalBreak);
                }
            }
        }

        // Compute reflow status
        if contentLength != maxContentLength {
            aStatus.SetIncomplete();
        }

        if charsFit == 0 && length > 0 && !usedHyphenation {
            // Couldn't place any text
            aStatus.SetInlineLineBreakBeforeAndReset();
        } else if contentLength > 0 && self.GetContentOffset() + contentLength - 1 == newLineOffset {
            // Ends in \n
            aStatus.SetInlineLineBreakAfter(UsedClear::None);
            aLineLayout.SetLineEndsInBR(true);
        } else if breakAfter {
            aStatus.SetInlineLineBreakAfter(UsedClear::None);
        }
        if completedFirstLetter {
            aLineLayout.SetFirstLetterStyleOK(false);
            aStatus.SetFirstLetterComplete();
        }
        if brokeText && breakPriority == gfxBreakPriority::eWordWrapBreak {
            aLineLayout.SetUsedOverflowWrap();
        }

        // Updated the cached NewlineProperty, or delete it.
        // (NOT PORTED: the cache; GetContentNewLineOffset searches again)

        // Compute space and letter counts for justification, if required
        let lineContainerText = nsStyleTextAdapter::of(lineContainer);
        if (lineContainerText.mTextAlignIsJustify() || lineContainerText.mTextAlignLastIsJustify() || shouldSuppressLineBreak) && !lineContainer.IsInSVGTextSubtree() {
            self.AddStateBits(TEXT_JUSTIFICATION_ENABLED);
            // Range range(uint32_t(offset), uint32_t(offset + charsFit));
            // aLineLayout.SetJustificationInfo(provider.ComputeJustification(range));
            // NOT PORTED: justification; the line gets no justification opportunities.
        }

        self.SetLength(contentLength, Some(&mut *aLineLayout), nsTextFrame_ALLOW_FRAME_CREATION_AND_DESTRUCTION);

        // InvalidateFrame(): NOT PORTED
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::CanContinueTextRun
    /* virtual */
    pub fn CanContinueTextRun(&self) -> bool {
        // We can continue a text run through a text frame
        true
    }

    // Method that is called for a text frame that is logically
    // adjacent to the end of the line (i.e. followed only by empty text frames,
    // placeholders or inlines containing such).
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::TrimTrailingWhiteSpace
    // (the DrawTarget parameter is dropped: there is no drawing)
    pub fn TrimTrailingWhiteSpace(&self) -> nsTextFrame_TrimOutput {
        MOZ_ASSERT!(!self.HasAnyStateBits(NS_FRAME_IS_DIRTY | NS_FRAME_FIRST_REFLOW), "frame should have been reflowed");

        let mut result = nsTextFrame_TrimOutput { mChanged: false, mDeltaWidth: 0 };

        self.AddStateBits(TEXT_END_OF_LINE);

        if self.GetTextRun(nsTextFrame_TextRunType::eInflated).is_none() {
            // If reflow didn't create a textrun, there must have been no content once
            // leading whitespace was trimmed, so nothing more to do here.
            return result;
        }

        let contentLength = self.GetContentLength();
        if contentLength == 0 {
            return result;
        }

        let start = self.EnsureTextRun(nsTextFrame_TextRunType::eInflated, None, None, None);
        let Some(mTextRun) = self.GetTextRun(nsTextFrame_TextRunType::eInflated) else {
            // NS_ENSURE_TRUE(mTextRun, result)
            return result;
        };

        let trimmedStart = start.GetSkippedOffset();

        let characterDataBuffer = self.CharacterDataBuffer();
        let trimmed = self.GetTrimmedOffsets(&characterDataBuffer, nsTextFrame_TrimmedOffsetFlags::Default);
        let mut trimmedEndIter = start;
        let textStyle = nsStyleTextAdapter::of(self.0);
        let mut delta: gfxFloat = 0.0;
        let trimmedEnd = trimmedEndIter.ConvertOriginalToSkipped(trimmed.GetEnd());

        if !self.HasAnyStateBits(TEXT_TRIMMED_TRAILING_WHITESPACE) && trimmed.GetEnd() < self.GetContentEnd() {
            let mut end = trimmedEndIter;
            let endOffset = end.ConvertOriginalToSkipped(self.GetContentOffset() + contentLength);
            if trimmedEnd < endOffset {
                // We can't be dealing with tabs here ... they wouldn't be trimmed. So
                // it's OK to pass null for the line container.
                let provider = nsTextFrame_PropertyProvider::new__gfxTextRun_nsStyleText_CharacterDataBuffer_nsTextFrame_gfxSkipCharsIterator_int32_t_nsIFrame_nscoord_TextRunType_bool(
                    mTextRun.clone(),
                    textStyle.clone(),
                    characterDataBuffer.clone(),
                    *self,
                    start,
                    contentLength,
                    None,
                    0,
                    nsTextFrame_TextRunType::eInflated,
                    self.HasAnyStateBits(TEXT_START_OF_LINE),
                );
                delta = mTextRun.GetAdvanceWidth(gfxTextRun_Range::new(trimmedEnd, endOffset), Some(&provider));
                result.mChanged = true;
            }
        }

        let mut advanceDelta: gfxFloat = 0.0;
        mTextRun.SetLineBreaks(gfxTextRun_Range::new(trimmedStart, trimmedEnd), self.HasAnyStateBits(TEXT_START_OF_LINE), true, Some(&mut advanceDelta));
        if advanceDelta != 0.0 {
            result.mChanged = true;
        }

        // aDeltaWidth is *subtracted* from our width.
        // If advanceDelta is positive then setting the line break made us longer,
        // so aDeltaWidth could go negative.
        result.mDeltaWidth = NSToCoordFloor__float((delta - advanceDelta) as f32);
        // If aDeltaWidth goes negative, that means this frame might not actually fit
        // anymore!!! We need higher level line layout to recover somehow.
        // If it's because the frame has a soft hyphen that is now being displayed,
        // this should actually be OK, because our reflow recorded the break
        // opportunity that allowed the soft hyphen to be used, and we wouldn't
        // have recorded the opportunity unless the hyphen fit (or was the first
        // opportunity on the line).
        // Otherwise this can/ really only happen when we have glyphs with special
        // shapes at the end of lines, I think. Breaking inside a kerning pair won't
        // do it because that would mean we broke inside this textrun, and
        // BreakAndMeasureText should make sure the resulting shaped substring fits.
        // Maybe if we passed a maxTextLength? But that only happens at direction
        // changes (so we wouldn't kern across the boundary) or for first-letter
        // (which always fits because it starts the line!).
        NS_WARNING_ASSERTION!(result.mDeltaWidth >= 0, "Negative deltawidth, something odd is happening");

        result
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::RecomputeOverflow
    pub fn RecomputeOverflow(&self, aBlockFrame: nsIFrame, aIncludeShadows: bool) -> OverflowAreas {
        // RemoveProperty(WebRenderTextBounds()): NOT PORTED

        let bounds = nsRect::new(0, 0, self.GetSize().width, self.GetSize().height);
        let mut result = OverflowAreas::new(bounds, bounds);

        let iter = self.EnsureTextRun(nsTextFrame_TextRunType::eInflated, None, None, None);
        let Some(mTextRun) = self.GetTextRun(nsTextFrame_TextRunType::eInflated) else {
            return result;
        };

        let mut provider = nsTextFrame_PropertyProvider::new__nsTextFrame_gfxSkipCharsIterator(*self, iter);
        // Don't trim trailing space, in case we need to paint it as selected.
        provider.InitializeForDisplay(false);

        let mut textMetrics = mTextRun.MeasureText(ComputeTransformedRange(&provider), gfxFont_BoundingBoxType::LOOSE_INK_EXTENTS, Some(&provider));
        if self.GetWritingMode().IsLineInverted() {
            textMetrics.mBoundingBox.y = -textMetrics.mBoundingBox.YMost();
        }
        let mut boundingBox = RoundOut(&textMetrics.mBoundingBox);
        boundingBox.y += self.mAscent();
        let vis = *result.InkOverflow();
        result.InkOverflow_mut().UnionRect(vis, boundingBox);
        // UnionAdditionalOverflow(PresContext(), aBlockFrame, provider, &vis, true, aIncludeShadows);
        // NOT PORTED: text decorations and shadows
        let _ = (aBlockFrame, aIncludeShadows);
        result
    }
}

// Gecko: layout/generic/nsTextFrame.cpp ComputeTransformedRange
fn ComputeTransformedRange(aProvider: &nsTextFrame_PropertyProvider) -> gfxTextRun_Range {
    let mut iter = *aProvider.GetStart();
    let start = iter.GetSkippedOffset();
    iter.AdvanceOriginal(aProvider.GetOriginalLength() as i32);
    gfxTextRun_Range::new(start, iter.GetSkippedOffset())
}

// Gecko: layout/generic/nsTextFrame.cpp TransformChars
// NOT PORTED: password masking and -webkit-text-security (no password field's anonymous text).
// ADAPTER: the case transform takes the frame's text-transform (Gecko reads each character's style
// from the transformed text run, all of them this frame's here).
fn TransformChars(aFrame: nsTextFrame, aStyle: &nsStyleTextAdapter, aTextRun: &RefPtr_gfxTextRun, aSkippedOffset: u32, aBuffer: &CharacterDataBuffer, aBufferOffset: i32, aBufferLen: i32, aOut: &mut Vec<u16>) {
    let transform = aStyle.Style().get_inherited_text().text_transform.case();
    let mut fragString = Vec::new();
    // No text-transform, so we can copy directly to the output string; else use a temporary string
    // as source for the transform.
    let out = if transform == mrt_style::style::values::specified::text::TextTransformCase::None { &mut *aOut } else { &mut fragString };

    // Copy the text, with \n and \t replaced by <space> if appropriate.
    MOZ_ASSERT!(aBufferOffset >= 0);
    for i in 0..aBufferLen.max(0) as u32 {
        let mut ch = aBuffer.CharAt(aBufferOffset as u32 + i);
        if (ch == b'\n' as u16 && !aStyle.NewlineIsSignificant(aFrame)) || (ch == b'\t' as u16 && !aStyle.TabIsSignificant()) {
            ch = b' ' as u16;
        }
        out.push(ch);
    }

    if transform != mrt_style::style::values::specified::text::TextTransformCase::None {
        // Apply text-transform according to style in the transformed run.
        nsCaseTransformTextRunFactory_TransformString(&fragString, aOut, transform, Some((&aTextRun.borrow(), aSkippedOffset)));
    }
}

// Gecko: layout/generic/nsTextFrame.cpp LineStartsOrEndsAtHardLineBreak
fn LineStartsOrEndsAtHardLineBreak(aFrame: nsTextFrame, aLineContainer: nsBlockFrame, aStartsAtHardBreak: &mut bool, aEndsAtHardBreak: &mut bool) {
    let mut foundValidLine = false;
    let mut iter = nsBlockInFlowLineIterator::new__nsBlockFrame_nsIFrame_bool(aLineContainer, *aFrame, &mut foundValidLine);
    if !foundValidLine {
        NS_ERROR!("Invalid line!");
        *aStartsAtHardBreak = true;
        *aEndsAtHardBreak = true;
        return;
    }

    *aEndsAtHardBreak = !iter.GetLine().IsLineWrapped();
    if iter.Prev() {
        *aStartsAtHardBreak = !iter.GetLine().IsLineWrapped();
    } else {
        // Hit block boundary
        *aStartsAtHardBreak = true;
    }
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::RenderedText (the string as UTF-16)
#[derive(Default, Debug)]
pub struct nsIFrame_RenderedText {
    pub mString: Vec<u16>,
    pub mOffsetWithinNodeRenderedText: u32,
    pub mOffsetWithinNodeText: i32,
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::TextOffsetType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextOffsetType {
    // Passed-in start and end offsets are within the content text.
    OffsetsInContentText,
    // Passed-in start and end offsets are within the rendered text.
    OffsetsInRenderedText,
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::TrailingWhitespace
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrailingWhitespace {
    Trim,
    // Spaces preceding a caret at the end of a line should not be trimmed
    DontTrim,
}

// Gecko: layout/generic/nsTextFrame.h nsTextFrame::AppendRenderedTextState
struct AppendRenderedTextState {
    mStartOffset: u32,
    mEndOffset: u32,
    mOffsetType: TextOffsetType,
    mTrimTrailingWhitespace: TrailingWhitespace,
    mCharacterDataBuffer: Rc<CharacterDataBuffer>,
    mLineContainer: Option<nsBlockFrame>,
    mOffsetInRenderedString: u32,
    mHaveOffsets: bool,
}

impl nsTextFrame {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::AppendRenderedText
    fn AppendRenderedText(&self, aState: &mut AppendRenderedTextState, aResult: &mut nsIFrame_RenderedText) -> bool {
        if self.HasAnyStateBits(NS_FRAME_IS_DIRTY) {
            // We don't trust dirty frames, especially when computing rendered text.
            return false;
        }

        // Ensure the text run and grab the gfxSkipCharsIterator for it
        let mut iter = self.EnsureTextRun(nsTextFrame_TextRunType::eInflated, None, None, None);
        let Some(textRun) = self.GetTextRun(nsTextFrame_TextRunType::eInflated) else {
            return false;
        };
        let mut tmpIter = iter.clone();

        // Check if the frame starts/ends at a hard line break, to determine
        // whether whitespace should be trimmed.
        let mut startsAtHardBreak = false;
        let mut endsAtHardBreak = false;
        if !self.HasAnyStateBits(TEXT_START_OF_LINE | TEXT_END_OF_LINE) {
            startsAtHardBreak = false;
            endsAtHardBreak = false;
        } else if let Some(thisLc) = self.FindLineContainer().filter(|f| f.IsBlockFrameOrSubclass()).map(nsBlockFrame::from_frame) {
            if Some(thisLc) != aState.mLineContainer {
                // Setup line cursor when needed.
                aState.mLineContainer = Some(thisLc);
                thisLc.SetupLineCursorForQuery();
            }
            LineStartsOrEndsAtHardLineBreak(*self, thisLc, &mut startsAtHardBreak, &mut endsAtHardBreak);
        } else {
            // Weird situation where we have a line layout without a block.
            // No soft breaks occur in this situation.
            startsAtHardBreak = true;
            endsAtHardBreak = true;
        }

        // Whether we need to trim whitespaces after the text frame.
        // TrimmedOffsetFlags::Default will allow trimming; we set NoTrim* flags
        // in the cases where this should not occur.
        let mut trimFlags = nsTextFrame_TrimmedOffsetFlags::Default;
        if !self.IsAtEndOfLine() || aState.mTrimTrailingWhitespace != TrailingWhitespace::Trim || !endsAtHardBreak {
            trimFlags = trimFlags | nsTextFrame_TrimmedOffsetFlags::NoTrimAfter;
        }

        // Whether to trim whitespaces before the text frame.
        if !startsAtHardBreak {
            trimFlags = trimFlags | nsTextFrame_TrimmedOffsetFlags::NoTrimBefore;
        }

        let mut trimmedOffsets = self.GetTrimmedOffsets(&aState.mCharacterDataBuffer, trimFlags);
        use mrt_style::style::computed_values::white_space_collapse::T as C;
        let trimmedSignificantNewline = (trimmedOffsets.GetEnd() < self.GetContentEnd()
            || (aState.mTrimTrailingWhitespace == TrailingWhitespace::Trim && nsStyleTextAdapter::of(self.0).mWhiteSpaceCollapse() == C::PreserveBreaks))
            && self.HasSignificantTerminalNewline();
        let skippedToRenderedStringOffset = aState.mOffsetInRenderedString.wrapping_sub(tmpIter.ConvertOriginalToSkipped(trimmedOffsets.mStart));
        let nextOffsetInRenderedString = tmpIter
            .ConvertOriginalToSkipped(trimmedOffsets.GetEnd())
            .wrapping_add(if trimmedSignificantNewline { 1 } else { 0 })
            .wrapping_add(skippedToRenderedStringOffset);

        if aState.mOffsetType == TextOffsetType::OffsetsInRenderedText {
            if nextOffsetInRenderedString <= aState.mStartOffset {
                aState.mOffsetInRenderedString = nextOffsetInRenderedString;
                return true;
            }
            if !aState.mHaveOffsets {
                aResult.mOffsetWithinNodeText = tmpIter.ConvertSkippedToOriginal(aState.mStartOffset.wrapping_sub(skippedToRenderedStringOffset));
                aResult.mOffsetWithinNodeRenderedText = aState.mStartOffset;
                aState.mHaveOffsets = true;
            }
            if aState.mOffsetInRenderedString >= aState.mEndOffset {
                return false;
            }
        } else {
            if self.GetContentEnd() as u32 <= aState.mStartOffset {
                aState.mOffsetInRenderedString = nextOffsetInRenderedString;
                return true;
            }
            if !aState.mHaveOffsets {
                aResult.mOffsetWithinNodeText = aState.mStartOffset as i32;
                // Skip trimmed space when computed the rendered text offset.
                let clamped = std::cmp::max(aState.mStartOffset as i32, trimmedOffsets.mStart);
                aResult.mOffsetWithinNodeRenderedText = tmpIter.ConvertOriginalToSkipped(clamped).wrapping_add(skippedToRenderedStringOffset);
                MOZ_ASSERT!(aResult.mOffsetWithinNodeRenderedText >= aState.mOffsetInRenderedString && aResult.mOffsetWithinNodeRenderedText <= i32::MAX as u32, "Bad offset within rendered text");
                aState.mHaveOffsets = true;
            }
            if self.GetContentOffset() as u32 >= aState.mEndOffset {
                return false;
            }
        }

        let startOffset: i32;
        let endOffset: i32;
        if aState.mOffsetType == TextOffsetType::OffsetsInRenderedText {
            startOffset = tmpIter.ConvertSkippedToOriginal(aState.mStartOffset.wrapping_sub(skippedToRenderedStringOffset));
            endOffset = tmpIter.ConvertSkippedToOriginal(aState.mEndOffset.wrapping_sub(skippedToRenderedStringOffset));
        } else {
            startOffset = aState.mStartOffset as i32;
            endOffset = std::cmp::min(i32::MAX as u32, aState.mEndOffset) as i32;
        }

        // If startOffset and/or endOffset are inside of trimmedOffsets' range,
        // then clamp the edges of trimmedOffsets accordingly.
        let origTrimmedOffsetsEnd = trimmedOffsets.GetEnd();
        trimmedOffsets.mStart = std::cmp::max(trimmedOffsets.mStart as u32, startOffset as u32) as i32;
        trimmedOffsets.mLength = (std::cmp::min(origTrimmedOffsetsEnd as u32, endOffset as u32) as i32).wrapping_sub(trimmedOffsets.mStart);

        if trimmedOffsets.mLength > 0 {
            let textStyle = nsStyleTextAdapter::of(self.0);
            iter.SetOriginalOffset(trimmedOffsets.mStart);
            while iter.GetOriginalOffset() < trimmedOffsets.GetEnd() {
                let mut runLength = 0;
                let isSkipped = iter.IsOriginalCharSkipped(Some(&mut runLength));
                runLength = std::cmp::min(runLength, trimmedOffsets.GetEnd() - iter.GetOriginalOffset());
                if isSkipped {
                    MOZ_ASSERT!(runLength >= 0);
                    for i in 0..runLength.max(0) as u32 {
                        let ch = aState.mCharacterDataBuffer.CharAt(iter.GetOriginalOffset() as u32 + i);
                        if ch as u32 == CH_SHY {
                            // We should preserve soft hyphens. They can't be transformed.
                            aResult.mString.push(ch);
                        }
                    }
                } else {
                    TransformChars(*self, &textStyle, &textRun, iter.GetSkippedOffset(), &aState.mCharacterDataBuffer, iter.GetOriginalOffset(), runLength, &mut aResult.mString);
                }
                iter.AdvanceOriginal(runLength);
            }
        }

        if trimmedSignificantNewline && self.GetContentEnd() <= endOffset {
            // A significant newline was trimmed off (we must be
            // white-space:pre-line). Put it back.
            aResult.mString.push(b'\n' as u16);
        }
        aState.mOffsetInRenderedString = nextOffsetInRenderedString;

        true
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetRenderedText (the defaults: 0, UINT32_MAX,
    // OffsetsInContentText, TrailingWhitespace::Trim)
    pub fn GetRenderedText(&self, aStartOffset: u32, aEndOffset: u32, aOffsetType: TextOffsetType, aTrimTrailingWhitespace: TrailingWhitespace) -> nsIFrame_RenderedText {
        MOZ_ASSERT!(aStartOffset <= aEndOffset, "bogus offsets");
        MOZ_ASSERT!(
            self.GetPrevContinuation().is_none() || (aOffsetType == TextOffsetType::OffsetsInContentText && aStartOffset >= self.GetContentOffset() as u32 && aEndOffset <= self.GetContentEnd() as u32),
            "Must be called on first-in-flow, or content offsets must be given and be within this frame."
        );

        // The handling of offsets could be more efficient...
        let mut result = nsIFrame_RenderedText::default();
        let mut state = AppendRenderedTextState {
            mStartOffset: aStartOffset,
            mEndOffset: aEndOffset,
            mOffsetType: aOffsetType,
            mTrimTrailingWhitespace: aTrimTrailingWhitespace,
            mCharacterDataBuffer: self.CharacterDataBuffer(),
            mLineContainer: None,
            mOffsetInRenderedString: 0,
            mHaveOffsets: false,
        };

        let mut textFrame = Some(*self);
        while let Some(f) = textFrame {
            if !f.AppendRenderedText(&mut state, &mut result) {
                break;
            }
            textFrame = f.GetNextContinuation().map(nsTextFrame::from_frame);
        }

        if !state.mHaveOffsets {
            result.mOffsetWithinNodeText = state.mCharacterDataBuffer.GetLength() as i32;
            result.mOffsetWithinNodeRenderedText = state.mOffsetInRenderedString;
        }

        result
    }
}

impl nsTextFrame {
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        NS_ASSERTION!(!(self.HasAnyStateBits(TEXT_IS_ONLY_WHITESPACE) && self.HasAnyStateBits(TEXT_ISNOT_ONLY_WHITESPACE)), "Invalid state");

        // XXXldb Should this check compatibility mode as well???
        let textStyle = nsStyleTextAdapter::of(self.0);
        if textStyle.WhiteSpaceIsSignificant() {
            // When WhiteSpaceIsSignificant styles are in effect, we only treat the
            // frame as empty if its content really is entirely *empty* (not just
            // whitespace).
            return self.GetContentLength() == 0;
        }

        if self.HasAnyStateBits(TEXT_ISNOT_ONLY_WHITESPACE) {
            return false;
        }

        if self.HasAnyStateBits(TEXT_IS_ONLY_WHITESPACE) {
            return true;
        }

        let isEmpty = IsAllWhitespace(&self.CharacterDataBuffer(), !textStyle.WhiteSpaceCollapseIsPreserveBreaks());
        self.AddStateBits(if isEmpty { TEXT_IS_ONLY_WHITESPACE } else { TEXT_ISNOT_ONLY_WHITESPACE });
        isEmpty
    }

    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::IsSelfEmpty
    pub fn IsSelfEmpty(&self) -> bool {
        self.IsEmpty()
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ToCString, GetFrameName, List, ListTextRuns
    // NOT PORTED: frame dumps (DEBUG_FRAME_DUMP).

    /**
     * @return true if this text frame ends with a newline character.  It should
     * return false if it is not a text frame.
     */
    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::HasSignificantTerminalNewline
    pub fn HasSignificantTerminalNewline(&self) -> bool {
        HasTerminalNewline(*self) && nsStyleTextAdapter::of(self.0).NewlineIsSignificant(*self)
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::IsAtEndOfLine
    pub fn IsAtEndOfLine(&self) -> bool {
        self.HasAnyStateBits(TEXT_END_OF_LINE)
    }

    // Call this only after reflow the frame. Returns true if non-collapsed
    // characters are present.
    // Gecko: layout/generic/nsTextFrame.h nsTextFrame::HasNoncollapsedCharacters
    pub fn HasNoncollapsedCharacters(&self) -> bool {
        self.HasAnyStateBits(TEXT_HAS_NONCOLLAPSED_CHARACTERS)
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetNaturalBaselineBOffset
    pub fn GetNaturalBaselineBOffset(&self, aWM: WritingMode, aBaselineGroup: BaselineSharingGroup, _aExportContext: BaselineExportContext) -> Option<nscoord> {
        if aBaselineGroup == BaselineSharingGroup::Last {
            return None;
        }

        if !aWM.IsOrthogonalTo(&self.GetWritingMode()) {
            if aWM.IsCentralBaseline() {
                return Some(self.GetLogicalUsedBorderAndPadding(aWM).BStart(aWM) + self.ContentBSize(aWM) / 2);
            }
            return Some(self.mAscent());
        }

        // When the text frame has a writing mode orthogonal to the desired
        // writing mode, return a baseline coincides its parent frame.
        let parent = self.GetParent().unwrap();
        let position = self.GetNormalPosition();
        let parentAscent = parent.GetLogicalBaseline__WritingMode(aWM);
        if aWM.IsVerticalRL() {
            let parentDescent = parent.GetSize().width - parentAscent;
            let descent = parentDescent - position.x;
            return Some(self.GetSize().width - descent);
        }
        Some(parentAscent - if aWM.IsVertical() { position.x } else { position.y })
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::GetCaretBaseline
    // NOT PORTED: the caret.

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::HasAnyNoncollapsedCharacters
    pub fn HasAnyNoncollapsedCharacters(&self) -> bool {
        let mut iter = self.EnsureTextRun(nsTextFrame_TextRunType::eInflated, None, None, None);
        let offset = self.GetContentOffset();
        let offsetEnd = self.GetContentEnd();
        let skippedOffset = iter.ConvertOriginalToSkipped(offset) as i32;
        let skippedOffsetEnd = iter.ConvertOriginalToSkipped(offsetEnd) as i32;
        skippedOffset != skippedOffsetEnd
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ComputeCustomOverflow, ComputeCustomOverflowInternal
    pub fn ComputeCustomOverflow(&self, aOverflowAreas: &mut OverflowAreas) -> bool {
        self.ComputeCustomOverflowInternal(aOverflowAreas, true)
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ComputeCustomOverflowInternal
    pub fn ComputeCustomOverflowInternal(&self, aOverflowAreas: &mut OverflowAreas, aIncludeShadows: bool) -> bool {
        if self.HasAnyStateBits(NS_FRAME_FIRST_REFLOW) {
            return true;
        }

        let decorationsBlock: nsIFrame;
        if self.IsFloatingFirstLetterChild() {
            decorationsBlock = self.GetParent().unwrap();
        } else {
            let mut f = self.0;
            loop {
                if nsBlockFrame::do_QueryFrame(f).is_some() {
                    decorationsBlock = f;
                    break;
                }

                match f.GetParent() {
                    Some(p) => f = p,
                    None => {
                        NS_ERROR!("Couldn't find any block ancestor (for text decorations)");
                        return true;
                    }
                }
            }
        }

        *aOverflowAreas = self.RecomputeOverflow(decorationsBlock, aIncludeShadows);
        true
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::CountGraphemeClusters
    // NOT PORTED: text-combine-upright (its only user).

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::HasNonSuppressedText
    pub fn HasNonSuppressedText(&self) -> bool {
        if self.HasAnyStateBits(TEXT_ISNOT_ONLY_WHITESPACE |
            // If we haven't reflowed yet, or are currently doing so,
            // just return true because we can't be sure.
            NS_FRAME_FIRST_REFLOW | NS_FRAME_IN_REFLOW)
        {
            return true;
        }

        if self.GetTextRun(nsTextFrame_TextRunType::eInflated).is_none() {
            return false;
        }

        let offsets = self.GetTrimmedOffsets(&self.CharacterDataBuffer(), nsTextFrame_TrimmedOffsetFlags::NoTrimAfter);
        offsets.mLength != 0
    }

    // Gecko: layout/generic/nsTextFrame.cpp nsTextFrame::ComputeLineHeight
    pub fn ComputeLineHeight(&self) -> nscoord {
        // ReflowInput::CalcLineHeight(*Style(), PresContext(), GetContent(), NS_UNCONSTRAINEDSIZE, 1.0)
        // NOT PORTED: only the painting of decorations reads it.
        unimplemented!("NOT PORTED: nsTextFrame::ComputeLineHeight")
    }
}

impl nsStyleTextAdapter {
    // mWhiteSpaceCollapse == StyleWhiteSpaceCollapse::BreakSpaces
    pub fn WhiteSpaceCollapseIsBreakSpaces(&self) -> bool {
        use mrt_style::style::computed_values::white_space_collapse::T as C;
        self.mWhiteSpaceCollapse() == C::BreakSpaces
    }
    // mWhiteSpaceCollapse == StyleWhiteSpaceCollapse::PreserveBreaks
    pub fn WhiteSpaceCollapseIsPreserveBreaks(&self) -> bool {
        use mrt_style::style::computed_values::white_space_collapse::T as C;
        self.mWhiteSpaceCollapse() == C::PreserveBreaks
    }
}

// ---------------------------------------------------------------------------------------------
// The virtual calls into nsTextFrame (nsLineLayoutAdapter.rs, nsContainerFrame.rs and nsIFrame.rs
// dispatch on the frame type to these)
// ---------------------------------------------------------------------------------------------

// ADAPTER: nsLayoutUtils members that nsTextFrame.cpp calls and nsLayoutUtils.rs does not have yet.
pub trait nsLayoutUtils_nsTextFrame {
    fn FindNearestCommonAncestorFrameWithinBlock(aFrame1: nsTextFrame, aFrame2: nsTextFrame) -> Option<nsIFrame>;
    fn GetTextRunFlagsForStyle(aComputedStyle: &mrt_style::style::servo_arc::Arc<mrt_style::style::properties::ComputedValues>, aStyleFont: &nsStyleFont, aStyleText: &nsStyleTextAdapter, aLetterSpacing: nscoord) -> ShapedTextFlags;
}

impl nsLayoutUtils_nsTextFrame for nsLayoutUtils {
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::FindNearestCommonAncestorFrameWithinBlock
    fn FindNearestCommonAncestorFrameWithinBlock(aFrame1: nsTextFrame, aFrame2: nsTextFrame) -> Option<nsIFrame> {
        let mut f1: nsIFrame = aFrame1.0;
        let mut f2: nsIFrame = aFrame2.0;

        let mut n1 = 1;
        let mut n2 = 1;

        let mut f = f1.GetParent();
        loop {
            NS_ASSERTION!(f.is_some(), "All text frames should have a block ancestor");
            let Some(fr) = f else {
                return None;
            };
            if fr.IsBlockFrameOrSubclass() {
                break;
            }
            n1 += 1;
            f = fr.GetParent();
        }

        let mut f = f2.GetParent();
        loop {
            NS_ASSERTION!(f.is_some(), "All text frames should have a block ancestor");
            let Some(fr) = f else {
                return None;
            };
            if fr.IsBlockFrameOrSubclass() {
                break;
            }
            n2 += 1;
            f = fr.GetParent();
        }

        if n1 > n2 {
            std::mem::swap(&mut n1, &mut n2);
            std::mem::swap(&mut f1, &mut f2);
        }

        while n2 > n1 {
            f2 = f2.GetParent().unwrap();
            n2 -= 1;
        }

        let mut f1 = Some(f1);
        let mut f2 = Some(f2);
        while n2 >= 0 {
            if f1 == f2 {
                return f1;
            }
            f1 = f1.and_then(|f| f.GetParent());
            f2 = f2.and_then(|f| f.GetParent());
            n2 -= 1;
        }

        None
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetTextRunFlagsForStyle
    // NOT PORTED: text-justify: inter-character, -moz-control-character-visibility, and the
    // auto-quality minimum font size (a pref); `text-rendering: optimizeSpeed` is kept.
    fn GetTextRunFlagsForStyle(aComputedStyle: &mrt_style::style::servo_arc::Arc<mrt_style::style::properties::ComputedValues>, aStyleFont: &nsStyleFont, aStyleText: &nsStyleTextAdapter, aLetterSpacing: nscoord) -> ShapedTextFlags {
        use mrt_style::style::computed_values::text_rendering::T as R;
        let mut result = ShapedTextFlags::default();
        if aLetterSpacing != 0 {
            result |= ShapedTextFlags::TEXT_DISABLE_OPTIONAL_LIGATURES;
        }
        if aComputedStyle.get_inherited_text().text_rendering == R::Optimizespeed {
            result |= ShapedTextFlags::TEXT_OPTIMIZE_SPEED;
        }
        // | GetTextRunOrientFlagsForStyle(aComputedStyle): NOT PORTED, vertical text
        result
    }
}

// Gecko: layout/generic/nsTextFrame.cpp ReflowTextA11yNotifier, nsTextFrame::AccessibleType
// NOT PORTED: accessibility.

// ---------------------------------------------------------------------------------------------
// RefPtr<gfxTextRun>
// ---------------------------------------------------------------------------------------------

// ADAPTER: `RefPtr<gfxTextRun>` is `Rc<RefCell<gfxTextRun>>` (gfxTextRun.rs); this trait gives it the
// members of gfxTextRun under their Gecko names, so `textRun->GetFlags2()` is `textRun.GetFlags2()`
// and does the `borrow()` or `borrow_mut()` itself. The calls that change the text run
// (SetPotentialLineBreaks, SetLineBreaks, SetFlagBits, ClearFlagBits) must not be made while a
// `borrow()` is alive; none of the callers hold one.
pub trait RefPtr_gfxTextRun_Ext {
    fn GetLength(&self) -> u32;
    fn GetFlags(&self) -> ShapedTextFlags;
    fn GetFlags2(&self) -> nsTextFrameUtils_Flags;
    fn SetFlagBits(&self, aFlags: nsTextFrameUtils_Flags);
    fn ClearFlagBits(&self, aFlags: nsTextFrameUtils_Flags);
    // The skip chars never change after the text run is made, and the text run is on the heap
    // (an Rc), so the reference is valid for as long as the caller keeps its Rc.
    fn GetSkipChars(&self) -> &gfxSkipChars;
    fn GetFontGroup(&self) -> Rc<gfxFontGroup>;
    fn IsRightToLeft(&self) -> bool;
    fn IsVertical(&self) -> bool;
    fn GetAppUnitsPerDevUnit(&self) -> i32;
    fn IsClusterStart(&self, aPos: u32) -> bool;
    fn IsLigatureGroupStart(&self, aPos: u32) -> bool;
    fn CanBreakLineBefore(&self, aPos: u32) -> bool;
    fn CanHyphenateBefore(&self, aPos: u32) -> bool;
    fn CharIsSpace(&self, aPos: u32) -> bool;
    fn CharIsTab(&self, aPos: u32) -> bool;
    fn CharIsNewline(&self, aPos: u32) -> bool;
    fn GetCharacterGlyphs(&self) -> std::cell::Ref<'_, [gfxShapedText_CompressedGlyph]>;
    fn GetDetailedGlyphs(&self, aCharIndex: u32) -> Vec<gfxShapedText_DetailedGlyph>;
    fn SetPotentialLineBreaks(&self, aRange: gfxTextRun_Range, aBreakBefore: &[u8]) -> bool;
    fn SetLineBreaks(&self, aRange: gfxTextRun_Range, aLineBreakBefore: bool, aLineBreakAfter: bool, aAdvanceWidthDelta: Option<&mut gfxFloat>) -> bool;
    fn GetAdvanceWidth(&self, aRange: gfxTextRun_Range, aProvider: Option<&dyn gfxTextRun_PropertyProvider>) -> gfxFloat;
    fn GetMinAdvanceWidth(&self, aRange: gfxTextRun_Range, aLetterSpacing: nscoord) -> gfxFloat;
    fn MeasureText(&self, aRange: gfxTextRun_Range, aBoundingBoxType: gfxFont_BoundingBoxType, aProvider: Option<&dyn gfxTextRun_PropertyProvider>) -> gfxFont_RunMetrics;
    fn BreakAndMeasureText(
        &self,
        aStart: u32,
        aMaxLength: u32,
        aLineBreakBefore: bool,
        aWidth: gfxFloat,
        aProvider: &dyn gfxTextRun_PropertyProvider,
        aSuppressBreak: gfxTextRun_SuppressBreak,
        aBoundingBoxType: gfxFont_BoundingBoxType,
        aCanWordWrap: bool,
        aCanWhitespaceWrap: bool,
        aIsBreakSpaces: bool,
        aOutTrimmableWhitespace: Option<&mut gfxTextRun_TrimmableWS>,
        aOutMetrics: &mut gfxFont_RunMetrics,
        aOutUsedHyphenation: &mut bool,
        aOutLastBreak: &mut u32,
        aBreakPriority: &mut gfxBreakPriority,
    ) -> u32;
}

impl RefPtr_gfxTextRun_Ext for RefPtr_gfxTextRun {
    fn GetLength(&self) -> u32 {
        self.borrow().GetLength()
    }
    fn GetFlags(&self) -> ShapedTextFlags {
        self.borrow().GetFlags()
    }
    fn GetFlags2(&self) -> nsTextFrameUtils_Flags {
        self.borrow().GetFlags2()
    }
    fn SetFlagBits(&self, aFlags: nsTextFrameUtils_Flags) {
        self.borrow_mut().SetFlagBits(aFlags)
    }
    fn ClearFlagBits(&self, aFlags: nsTextFrameUtils_Flags) {
        self.borrow_mut().ClearFlagBits(aFlags)
    }
    fn GetSkipChars(&self) -> &gfxSkipChars {
        unsafe { &*(self.borrow().GetSkipChars() as *const gfxSkipChars) }
    }
    fn GetFontGroup(&self) -> Rc<gfxFontGroup> {
        self.borrow().GetFontGroup().clone()
    }
    fn IsRightToLeft(&self) -> bool {
        self.borrow().IsRightToLeft()
    }
    fn IsVertical(&self) -> bool {
        self.borrow().IsVertical()
    }
    fn GetAppUnitsPerDevUnit(&self) -> i32 {
        self.borrow().GetAppUnitsPerDevUnit()
    }
    fn IsClusterStart(&self, aPos: u32) -> bool {
        self.borrow().IsClusterStart(aPos)
    }
    fn IsLigatureGroupStart(&self, aPos: u32) -> bool {
        self.borrow().IsLigatureGroupStart(aPos)
    }
    fn CanBreakLineBefore(&self, aPos: u32) -> bool {
        self.borrow().CanBreakLineBefore(aPos)
    }
    fn CanHyphenateBefore(&self, aPos: u32) -> bool {
        self.borrow().CanHyphenateBefore(aPos)
    }
    fn CharIsSpace(&self, aPos: u32) -> bool {
        self.borrow().CharIsSpace(aPos)
    }
    fn CharIsTab(&self, aPos: u32) -> bool {
        self.borrow().CharIsTab(aPos)
    }
    fn CharIsNewline(&self, aPos: u32) -> bool {
        self.borrow().CharIsNewline(aPos)
    }
    fn GetCharacterGlyphs(&self) -> std::cell::Ref<'_, [gfxShapedText_CompressedGlyph]> {
        std::cell::Ref::map(self.borrow(), |r| r.GetCharacterGlyphs())
    }
    fn GetDetailedGlyphs(&self, aCharIndex: u32) -> Vec<gfxShapedText_DetailedGlyph> {
        let run = self.borrow();
        if run.GetCharacterGlyphs()[aCharIndex as usize].IsSimpleGlyph() || run.GetCharacterGlyphs()[aCharIndex as usize].GetGlyphCount() == 0 {
            return Vec::new();
        }
        run.GetDetailedGlyphs(aCharIndex).to_vec()
    }
    fn SetPotentialLineBreaks(&self, aRange: gfxTextRun_Range, aBreakBefore: &[u8]) -> bool {
        self.borrow_mut().SetPotentialLineBreaks(aRange, aBreakBefore)
    }
    fn SetLineBreaks(&self, aRange: gfxTextRun_Range, aLineBreakBefore: bool, aLineBreakAfter: bool, aAdvanceWidthDelta: Option<&mut gfxFloat>) -> bool {
        self.borrow_mut().SetLineBreaks(aRange, aLineBreakBefore, aLineBreakAfter, aAdvanceWidthDelta)
    }
    fn GetAdvanceWidth(&self, aRange: gfxTextRun_Range, aProvider: Option<&dyn gfxTextRun_PropertyProvider>) -> gfxFloat {
        self.borrow().GetAdvanceWidth__Range_PropertyProvider_ptr_Spacing_ptr(aRange, aProvider, None)
    }
    fn GetMinAdvanceWidth(&self, aRange: gfxTextRun_Range, aLetterSpacing: nscoord) -> gfxFloat {
        self.borrow().GetMinAdvanceWidth(aRange, aLetterSpacing)
    }
    fn MeasureText(&self, aRange: gfxTextRun_Range, aBoundingBoxType: gfxFont_BoundingBoxType, aProvider: Option<&dyn gfxTextRun_PropertyProvider>) -> gfxFont_RunMetrics {
        self.borrow().MeasureText__Range_BoundingBoxType_DrawTarget_ptr_PropertyProvider_ptr(aRange, aBoundingBoxType, None, aProvider)
    }
    fn BreakAndMeasureText(
        &self,
        aStart: u32,
        aMaxLength: u32,
        aLineBreakBefore: bool,
        aWidth: gfxFloat,
        aProvider: &dyn gfxTextRun_PropertyProvider,
        aSuppressBreak: gfxTextRun_SuppressBreak,
        aBoundingBoxType: gfxFont_BoundingBoxType,
        aCanWordWrap: bool,
        aCanWhitespaceWrap: bool,
        aIsBreakSpaces: bool,
        aOutTrimmableWhitespace: Option<&mut gfxTextRun_TrimmableWS>,
        aOutMetrics: &mut gfxFont_RunMetrics,
        aOutUsedHyphenation: &mut bool,
        aOutLastBreak: &mut u32,
        aBreakPriority: &mut gfxBreakPriority,
    ) -> u32 {
        self.borrow().BreakAndMeasureText(
            aStart,
            aMaxLength,
            aLineBreakBefore,
            aWidth,
            aProvider,
            aSuppressBreak,
            aBoundingBoxType,
            None,
            aCanWordWrap,
            aCanWhitespaceWrap,
            aIsBreakSpaces,
            aOutTrimmableWhitespace,
            aOutMetrics,
            aOutUsedHyphenation,
            aOutLastBreak,
            aBreakPriority,
        )
    }
}

// Tests. Firefox tests text layout with reftests and mochitests; there is no gtest for nsTextFrame, so
// these are written from the documented behaviour: CSS Text 3 (white space processing and collapsing,
// 4.1; line breaking at soft wrap opportunities, 5; the `white-space` values, 3.1; `word-spacing` and
// `letter-spacing`, 8) and CSS Sizing 3 (min-content is the widest unbreakable word, max-content the
// text on one line). The styles are Stylo's, the font is the system's `sans-serif` (through mrt-text),
// and the expected widths are measured with the same font metrics (`W`), so nothing depends on which
// font the machine has. A text node's frame has the style of a `display:inline` span inside the root,
// which inherits the text properties of the root.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockReflowState_test_support::*;

    const AUTO: nscoord = NS_UNCONSTRAINEDSIZE;

    #[derive(Clone, Copy)]
    enum K {
        // a block container (a `div`); its children are in lines
        Block,
        // an inline element (a `span`) with children
        Inline,
        // a text node, with the style of the element of that id
        Text(&'static str),
    }

    // Builds the frames for the elements of `spec` (id, parent index, kind): the types and contents,
    // then Init and SetInitialChildList for the blocks, as the frame constructor does.
    fn build(html: &str, spec: &[(&str, Option<usize>, K)], root_px: i32) -> Vec<nsIFrame> {
        let tree: Vec<(&str, Option<usize>)> = spec.iter().map(|s| (s.0, s.1)).collect();
        let f = frames(html, &tree, root_px);
        for (i, s) in spec.iter().enumerate() {
            match s.2 {
                K::Block => f[i].SetType(LayoutFrameType::Block),
                K::Inline => {
                    NS_NewInlineFrame(f[i]);
                }
                K::Text(text) => {
                    NS_NewTextFrame(f[i], nsIContent::TextNode(text));
                }
            }
        }
        for (i, s) in spec.iter().enumerate() {
            if matches!(s.2, K::Block) {
                nsBlockFrame::from_frame(f[i]).Init(None, s.1.map_or(f[i], |p| f[p]), None);
            }
        }
        for (i, s) in spec.iter().enumerate() {
            if matches!(s.2, K::Block) {
                let list = std::mem::take(&mut *f[i].mFrames());
                if list.NotEmpty() {
                    nsBlockFrame::from_frame(f[i]).SetInitialChildList(FrameChildListID::Principal, list);
                }
            }
        }
        f
    }

    // `<div id=root style="...; width:Wpx"><span id=t0></span></div>` with one text node `text` in the
    // span; frames [root, text].
    fn paragraph(style: &str, width_px: i32, text: &'static str) -> Vec<nsIFrame> {
        let html = format!(r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;font:20px sans-serif;width:{width_px}px;{style}"><span id=t0></span></div>"#);
        build(&html, &[("root", None, K::Block), ("t0", Some(0), K::Text(text))], width_px)
    }

    // Reflows the root (available inline-size from its style) and sets its size from the result.
    fn reflow(root: nsIFrame) -> ReflowOutput {
        let ri = root_input(root, root.GetRect().width / PX, AUTO);
        let wm = root.GetWritingMode();
        let mut metrics = ReflowOutput::new__ReflowInput(&ri);
        let mut status = nsReflowStatus::new();
        root.Reflow(&nsPresContext::Current(), &mut metrics, &ri, &mut status);
        assert!(status.IsFullyComplete());
        root.SetSize__WritingMode_LogicalSize(wm, metrics.Size(wm));
        metrics
    }

    // The width of `s` set in the font of `f`, in app units (a string measured as one text run).
    fn W(f: nsIFrame, s: &str) -> nscoord {
        let utf16: Vec<u16> = s.encode_utf16().collect();
        nsLayoutUtils::GetFontMetricsForFrame(f, 1.0).GetWidth__char16_t_ptr_uint32_t_DrawTarget_ptr(&utf16, utf16.len() as u32, None)
    }

    // The smallest whole number of px that is at least `w` app units, and the largest that is below it:
    // a container of the first fits a string of width `w`, one of the second does not.
    fn px_fit(w: nscoord) -> i32 {
        (w + PX - 1) / PX
    }
    fn px_below(w: nscoord) -> i32 {
        (w - 1) / PX
    }

    // The frames of the flow that starts at `first`.
    fn chain(first: nsIFrame) -> Vec<nsTextFrame> {
        let mut v = vec![nsTextFrame(first)];
        while let Some(n) = v.last().unwrap().GetNextInFlow() {
            v.push(nsTextFrame(n));
        }
        v
    }

    // The text a frame maps.
    fn text_of(f: nsTextFrame) -> String {
        let buffer = f.CharacterDataBuffer();
        let mut s = Vec::new();
        buffer.AppendTo__nsAString_uint32_t_uint32_t(&mut s, f.GetContentOffset() as u32, f.GetContentLength() as u32);
        String::from_utf16(&s).unwrap()
    }

    // The text of each frame of the flow, which is one line each in these tests.
    fn lines_of(first: nsIFrame) -> Vec<String> {
        chain(first).into_iter().map(text_of).collect()
    }

    // The greedy line breaking the CSS says for words separated by one space: a word goes on the line if
    // the line up to the word, without the space after it, fits in `max` app units. Each line keeps the
    // space that follows its last word, as the frames do.
    fn greedy(f: nsIFrame, words: &[&str], max: nscoord) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut cur = String::new();
        for (i, w) in words.iter().enumerate() {
            let candidate = if cur.is_empty() { w.to_string() } else { format!("{cur}{w}") };
            if cur.is_empty() || W(f, &candidate) <= max {
                cur = candidate;
            } else {
                lines.push(cur);
                cur = w.to_string();
            }
            if i + 1 < words.len() {
                cur.push(' ');
            }
        }
        lines.push(cur);
        lines
    }

    const WORDS: [&str; 5] = ["alpha", "beta", "gamma", "delta", "epsilon"];
    const TEXT: &str = "alpha beta gamma delta epsilon";

    #[test]
    fn a_paragraph_that_fits_is_one_line_in_one_frame() {
        let f = paragraph("", 800, TEXT);
        reflow(f[0]);
        assert_eq!(lines_of(f[1]), vec![TEXT.to_string()]);
        assert_eq!(f[1].GetSize().width, W(f[1], TEXT));
        assert_eq!(nsBlockFrame::from_frame(f[0]).Lines().count(), 1);
    }

    #[test]
    fn the_words_wrap_into_the_lines_the_container_width_allows() {
        // CSS Text 3 5.1: a soft wrap opportunity after each space; the line is filled with as many
        // words as fit.
        let probe = paragraph("", 800, TEXT);
        let one = probe[1];
        // wide enough for "alpha beta gamma" but not for "alpha beta gamma delta"
        let width = px_fit(W(one, "alpha beta gamma"));
        assert!(W(one, "alpha beta gamma delta") > width * PX);
        let f = paragraph("", width, TEXT);
        reflow(f[0]);
        assert_eq!(lines_of(f[1]), greedy(f[1], &WORDS, width * PX));
        assert_eq!(lines_of(f[1]), vec!["alpha beta gamma ".to_string(), "delta epsilon".to_string()]);
        assert_eq!(nsBlockFrame::from_frame(f[0]).Lines().count(), 2);
    }

    #[test]
    fn narrowing_the_container_makes_more_lines_and_continuing_frames() {
        let probe = paragraph("", 800, TEXT);
        // a container a pixel narrower than the longest word "epsilon": every word is on its own line
        let narrow = px_below(W(probe[1], "alpha beta"));
        assert!(narrow * PX >= W(probe[1], "epsilon"));
        let f = paragraph("", narrow, TEXT);
        reflow(f[0]);
        assert_eq!(lines_of(f[1]), vec!["alpha ", "beta ", "gamma ", "delta ", "epsilon"].into_iter().map(String::from).collect::<Vec<_>>());
        let frames = chain(f[1]);
        assert_eq!(frames.len(), 5);
        assert_eq!(nsBlockFrame::from_frame(f[0]).Lines().count(), 5);
        // the first frame is not a continuing frame, the others are, fluid continuations of the one before
        assert!(!frames[0].with_data(|d| d.mIsContinuing));
        for i in 1..frames.len() {
            assert!(frames[i].with_data(|d| d.mIsContinuing));
            assert_eq!(frames[i].GetPrevInFlow(), Some(frames[i - 1].0));
            assert!(frames[i].HasAnyStateBits(NS_FRAME_IS_FLUID_CONTINUATION));
            assert_eq!(frames[i].GetParent(), Some(f[0]));
            assert!(nsIContent::IsSameNode(&frames[i].GetContent(), &f[1].GetContent()));
            // the offsets run on without gaps or overlaps
            assert_eq!(frames[i].GetContentOffset(), frames[i - 1].GetContentOffset() + frames[i - 1].GetContentLength());
        }
        assert_eq!(frames.last().unwrap().GetContentEnd(), TEXT.len() as i32);
        // the lines stack: each line is a taller than the one before it starts
        let tops: Vec<nscoord> = nsBlockFrame::from_frame(f[0]).Lines().map(|l| l.BStart()).collect();
        assert!(tops.windows(2).all(|w| w[1] > w[0]));
    }

    #[test]
    fn every_line_is_as_wide_as_its_words_without_the_space_that_ends_it() {
        // CSS Text 3 4.1.3: a space at the end of a line is removed. The frame of a line that ends
        // in a soft wrap is as wide as the words, not the words and the space.
        let probe = paragraph("", 800, TEXT);
        let width = px_fit(W(probe[1], "alpha beta gamma"));
        let f = paragraph("", width, TEXT);
        reflow(f[0]);
        let frames = chain(f[1]);
        assert_eq!(frames[0].GetSize().width, W(f[1], "alpha beta gamma"));
        // the frame knows it was trimmed at the end of the line, and mapped the space
        assert!(frames[0].HasAnyStateBits(TEXT_END_OF_LINE) || frames[0].HasAnyStateBits(TEXT_TRIMMED_TRAILING_WHITESPACE));
        assert_eq!(text_of(frames[0]), "alpha beta gamma ");
        assert_eq!(frames[1].GetSize().width, W(f[1], "delta epsilon"));
        // ... and the second frame starts below the first, at the left edge
        assert_eq!(frames[1].GetPosition().x, 0);
        assert!(frames[1].GetPosition().y > frames[0].GetPosition().y);
    }

    #[test]
    fn a_trailing_space_at_the_end_of_the_last_line_is_trimmed_too() {
        let f = paragraph("", 800, "alpha beta ");
        reflow(f[0]);
        assert_eq!(lines_of(f[1]), vec!["alpha beta ".to_string()]);
        assert_eq!(f[1].GetSize().width, W(f[1], "alpha beta"));
        assert!(f[1].IsAtEndOfLine());
    }

    #[test]
    fn leading_white_space_is_removed_at_the_start_of_the_line() {
        let f = paragraph("", 800, "   alpha");
        reflow(f[0]);
        assert_eq!(f[1].GetSize().width, W(f[1], "alpha"));
        let trimmed = nsTextFrame(f[1]).GetTrimmedOffsets(&nsTextFrame(f[1]).CharacterDataBuffer(), nsTextFrame_TrimmedOffsetFlags::Default);
        assert_eq!(trimmed, nsTextFrame_TrimmedOffsets { mStart: 3, mLength: 5 });
    }

    #[test]
    fn white_space_normal_collapses_runs_of_spaces_and_newlines() {
        // CSS Text 3 4.1.1: a sequence of collapsible spaces and segment breaks is one space.
        let f = paragraph("", 800, "alpha   \n  beta");
        reflow(f[0]);
        assert_eq!(f[1].GetSize().width, W(f[1], "alpha beta"));
    }

    #[test]
    fn white_space_nowrap_does_not_wrap_even_when_the_text_overflows() {
        let probe = paragraph("", 800, TEXT);
        let narrow = px_below(W(probe[1], "alpha beta"));
        let f = paragraph("white-space:nowrap", narrow, TEXT);
        reflow(f[0]);
        assert_eq!(nsBlockFrame::from_frame(f[0]).Lines().count(), 1);
        assert_eq!(lines_of(f[1]), vec![TEXT.to_string()]);
        assert_eq!(f[1].GetSize().width, W(f[1], TEXT));
        assert!(f[1].GetSize().width > narrow * PX);
    }

    #[test]
    fn white_space_pre_keeps_spaces_and_breaks_lines_at_newlines_only() {
        // CSS Text 3 3.1: `pre` preserves spaces and breaks only at preserved segment breaks.
        let probe = paragraph("", 800, TEXT);
        let narrow = px_below(W(probe[1], "alpha beta"));
        let f = paragraph("white-space:pre", narrow, "ab\ncd  ef gh");
        reflow(f[0]);
        assert_eq!(lines_of(f[1]), vec!["ab\n".to_string(), "cd  ef gh".to_string()]);
        let frames = chain(f[1]);
        // the newline belongs to the first line and does not count in its width
        assert_eq!(frames[0].GetSize().width, W(f[1], "ab"));
        // both spaces of "cd  ef" count
        assert_eq!(frames[1].GetSize().width, W(f[1], "cd  ef gh"));
        assert!(frames[0].HasSignificantTerminalNewline());
        assert!(!frames[1].HasSignificantTerminalNewline());
    }

    #[test]
    fn white_space_pre_wrap_wraps_and_keeps_the_spaces() {
        let probe = paragraph("", 800, TEXT);
        let width = px_fit(W(probe[1], "alpha beta"));
        let f = paragraph("white-space:pre-wrap", width, "alpha beta gamma");
        reflow(f[0]);
        let lines = lines_of(f[1]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines.concat(), "alpha beta gamma");
        assert!(lines[0].starts_with("alpha beta"));
    }

    #[test]
    fn word_spacing_and_letter_spacing_widen_the_text() {
        // CSS Text 3 8.1 and 8.2: word-spacing adds to each word-separator, letter-spacing to each character.
        let plain = paragraph("", 800, "ab cd");
        reflow(plain[0]);
        let base = plain[1].GetSize().width;
        let word = paragraph("word-spacing:10px", 800, "ab cd");
        reflow(word[0]);
        assert_eq!(word[1].GetSize().width, base + 10 * PX);
        let letter = paragraph("letter-spacing:2px", 800, "ab cd");
        reflow(letter[0]);
        // five characters; Gecko puts the spacing after each of them
        assert_eq!(letter[1].GetSize().width, base + 5 * 2 * PX);
    }

    #[test]
    fn a_tab_in_preformatted_text_advances_to_the_next_tab_stop() {
        // CSS Text 3 7.2: tab-size 8 (spaces); a preserved tab ends at the next multiple of the tab width,
        // and at least half a `0` further.
        let f = paragraph("white-space:pre", 800, "a\tb");
        reflow(f[0]);
        let tabWidth = ComputeTabWidthAppUnits(f[1]);
        assert!(tabWidth > 0.0);
        let minAdvance = {
            let run = nsTextFrame(f[1]).GetTextRun(nsTextFrame_TextRunType::eInflated).unwrap();
            GetMinTabAdvanceAppUnits(&run)
        };
        let aW = W(f[1], "a") as f64;
        let stop = ((aW + minAdvance) / tabWidth).ceil() * tabWidth;
        let expected = stop + W(f[1], "b") as f64;
        assert!((f[1].GetSize().width as f64 - expected).abs() <= 1.0, "{} vs {}", f[1].GetSize().width, expected);
    }

    #[test]
    fn the_break_after_a_hyphen_is_taken() {
        // UAX #14: a break opportunity after a hyphen between letters.
        let probe = paragraph("", 800, "well-known");
        let width = px_fit(W(probe[1], "well-")) + 1;
        let f = paragraph("", width, "well-known");
        reflow(f[0]);
        assert_eq!(lines_of(f[1]), vec!["well-".to_string(), "known".to_string()]);
    }

    #[test]
    fn a_word_wider_than_the_container_stays_on_its_line() {
        // no break opportunity inside the word: the line overflows
        let f = paragraph("", 20, "extraordinarily short");
        reflow(f[0]);
        assert_eq!(lines_of(f[1]), vec!["extraordinarily ".to_string(), "short".to_string()]);
        assert!(chain(f[1])[0].GetSize().width > 20 * PX);
    }

    #[test]
    fn overflow_wrap_anywhere_breaks_inside_a_word() {
        let probe = paragraph("", 800, "extraordinarily");
        let width = px_fit(W(probe[1], "extraordina"));
        let f = paragraph("overflow-wrap:anywhere", width, "extraordinarily");
        reflow(f[0]);
        let lines = lines_of(f[1]);
        assert!(lines.len() >= 2, "{lines:?}");
        assert_eq!(lines.concat(), "extraordinarily");
        assert!(chain(f[1])[0].GetSize().width <= width * PX);
    }

    #[test]
    fn the_text_run_is_shared_by_the_whole_flow() {
        let probe = paragraph("", 800, TEXT);
        let narrow = px_below(W(probe[1], "alpha beta"));
        let f = paragraph("", narrow, TEXT);
        reflow(f[0]);
        let frames = chain(f[1]);
        let run = frames[0].GetTextRun(nsTextFrame_TextRunType::eInflated).unwrap();
        assert_eq!(run.GetLength(), TEXT.len() as u32);
        assert!(run.GetFlags2().0 & nsTextFrameUtils_Flags::IsSimpleFlow.0 != 0);
        for fr in &frames {
            assert!(Rc::ptr_eq(&fr.GetTextRun(nsTextFrame_TextRunType::eInflated).unwrap(), &run));
        }
        // the first frame is the one the run knows about
        assert!(frames[0].HasAnyStateBits(TEXT_IN_TEXTRUN_USER_DATA));
        assert!(!frames[1].HasAnyStateBits(TEXT_IN_TEXTRUN_USER_DATA));
    }

    #[test]
    fn min_content_is_the_widest_word_and_max_content_the_whole_text() {
        // CSS Sizing 3 5.1: min-content is the width with every soft wrap opportunity taken, max-content
        // the width with none taken.
        let f = paragraph("width:auto", 800, TEXT);
        let input = IntrinsicSizeInput::new(Some(gfxContext), None, None);
        let widest = WORDS.iter().map(|w| W(f[1], w)).max().unwrap();
        // the widths are measured to the pixel above, as the intrinsic sizes are rounded up
        let min = f[0].GetMinISize(&input);
        let pref = f[0].GetPrefISize(&input);
        assert!((min - widest).abs() <= 1, "min-content {min} vs widest word {widest}");
        assert!((pref - W(f[1], TEXT)).abs() <= 1, "max-content {pref} vs text {}", W(f[1], TEXT));
    }

    #[test]
    fn white_space_nowrap_makes_min_content_the_whole_text() {
        let f = paragraph("width:auto;white-space:nowrap", 800, TEXT);
        let input = IntrinsicSizeInput::new(Some(gfxContext), None, None);
        let (min, pref) = (f[0].GetMinISize(&input), f[0].GetPrefISize(&input));
        assert!((min - W(f[1], TEXT)).abs() <= 1);
        assert_eq!(min, pref);
    }

    #[test]
    fn white_space_pre_makes_both_intrinsic_sizes_the_widest_line() {
        let f = paragraph("width:auto;white-space:pre", 800, "ab\nalpha beta\nx");
        let input = IntrinsicSizeInput::new(Some(gfxContext), None, None);
        let widest = W(f[1], "alpha beta");
        let (min, pref) = (f[0].GetMinISize(&input), f[0].GetPrefISize(&input));
        assert!((min - widest).abs() <= 1, "{min} vs {widest}");
        assert!((pref - widest).abs() <= 1, "{pref} vs {widest}");
    }

    #[test]
    fn the_intrinsic_sizes_of_a_continuing_frame_are_zero() {
        // "Do nothing, since the first-in-flow accounts for everything."
        let probe = paragraph("", 800, TEXT);
        let narrow = px_below(W(probe[1], "alpha beta"));
        let f = paragraph("", narrow, TEXT);
        reflow(f[0]);
        let frames = chain(f[1]);
        let input = IntrinsicSizeInput::new(Some(gfxContext), None, None);
        assert_eq!(frames[1].IntrinsicISize(&input, IntrinsicISizeType::PrefISize), 0);
        assert!(frames[0].IntrinsicISize(&input, IntrinsicISizeType::PrefISize) > 0);
    }

    #[test]
    fn is_empty_tells_white_space_only_text_from_text() {
        // nsTextFrame::IsEmpty: collapsible white space only is empty; `pre` white space is not.
        let blank = paragraph("", 800, "  \n ");
        let word = paragraph("", 800, " a ");
        let pre = paragraph("white-space:pre", 800, "  ");
        assert!(nsTextFrame(blank[1]).IsEmpty());
        assert!(nsTextFrame(blank[1]).IsSelfEmpty());
        assert!(!nsTextFrame(word[1]).IsEmpty());
        assert!(!nsTextFrame(pre[1]).IsEmpty());
        assert!(nsTextFrame::from_frame(blank[1]).IsEntirelyWhitespace());
        // the virtual call on the handle gets to the same answer
        assert!(blank[1].IsEmpty());
        assert!(!word[1].IsEmpty());
    }

    #[test]
    fn a_white_space_only_frame_in_a_line_has_no_noncollapsed_characters() {
        let f = paragraph("", 800, "alpha");
        reflow(f[0]);
        assert!(f[1].HasNoncollapsedCharacters());
        let blank = paragraph("", 800, "   ");
        reflow(blank[0]);
        // the line is empty: the white space is at the start of the line and collapses away
        assert!(!blank[1].HasNoncollapsedCharacters());
        assert_eq!(blank[1].GetSize().width, 0);
    }

    #[test]
    fn the_baseline_is_the_font_ascent_and_the_frame_is_as_high_as_the_font() {
        let f = paragraph("", 800, "alpha");
        reflow(f[0]);
        let fm = nsLayoutUtils::GetFontMetricsForFrame(f[1], 1.0);
        let wm = f[1].GetWritingMode();
        assert_eq!(nsTextFrame(f[1]).mAscent(), fm.MaxAscent());
        assert_eq!(f[1].GetSize().height, fm.MaxAscent() + fm.MaxDescent());
        assert_eq!(nsTextFrame(f[1]).GetNaturalBaselineBOffset(wm, BaselineSharingGroup::First, BaselineExportContext::Other), Some(fm.MaxAscent()));
        assert_eq!(f[1].GetNaturalBaselineBOffset(wm, BaselineSharingGroup::First, BaselineExportContext::Other), Some(fm.MaxAscent()));
        assert_eq!(nsTextFrame(f[1]).GetNaturalBaselineBOffset(wm, BaselineSharingGroup::Last, BaselineExportContext::Other), None);
    }

    #[test]
    fn one_text_run_spans_the_text_nodes_of_inline_elements() {
        // <p>alpha <b>beta</b> gamma</p>: the three text nodes are one flow each, in one text run, and the
        // words wrap across the element boundaries as they would in one text node.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;font:20px sans-serif;width:800px">
            <span id=t0></span><span id=b style="display:inline"><span id=t1></span></span><span id=t2></span></div>"#;
        let spec = [("root", None, K::Block), ("t0", Some(0), K::Text("alpha ")), ("b", Some(0), K::Inline), ("t1", Some(2), K::Text("beta")), ("t2", Some(0), K::Text(" gamma"))];
        let f = build(html, &spec, 800);
        reflow(f[0]);
        let run0 = f[1].0;
        let r0 = nsTextFrame(f[1]).GetTextRun(nsTextFrame_TextRunType::eInflated).unwrap();
        let r1 = nsTextFrame(f[3]).GetTextRun(nsTextFrame_TextRunType::eInflated).unwrap();
        let r2 = nsTextFrame(f[4]).GetTextRun(nsTextFrame_TextRunType::eInflated).unwrap();
        assert!(Rc::ptr_eq(&r0, &r1) && Rc::ptr_eq(&r0, &r2));
        // "alpha " + "beta" + " gamma" with the collapsed space after "beta": 16 characters, one space at the join
        assert_eq!(r0.GetLength() as usize, "alpha beta gamma".len());
        assert!(r0.GetFlags2().0 & nsTextFrameUtils_Flags::IsSimpleFlow.0 == 0);
        assert_eq!(nsBlockFrame::from_frame(f[0]).Lines().count(), 1);
        let _ = run0;
    }

    #[test]
    fn words_wrap_between_text_nodes_of_inline_elements() {
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;font:20px sans-serif;width:__W__px">
            <span id=t0></span><span id=b style="display:inline"><span id=t1></span></span><span id=t2></span></div>"#;
        let spec = [("root", None, K::Block), ("t0", Some(0), K::Text("alpha ")), ("b", Some(0), K::Inline), ("t1", Some(2), K::Text("beta")), ("t2", Some(0), K::Text(" gamma"))];
        let probe = build(&html.replace("__W__", "800"), &spec, 800);
        // "alpha beta" fits, "alpha beta gamma" does not
        let width = px_fit(W(probe[1], "alpha beta"));
        assert!(W(probe[1], "alpha beta gamma") > width * PX);
        let f = build(&html.replace("__W__", &width.to_string()), &spec, width);
        reflow(f[0]);
        let lines: Vec<_> = nsBlockFrame::from_frame(f[0]).Lines().collect();
        assert_eq!(lines.len(), 2);
        // The break opportunity is after the space that starts the last text node, so that node is split:
        // its space stays on the first line (trimmed there) and "gamma" is in its continuing frame on the
        // second line.
        let t2 = chain(f[4]);
        assert_eq!(t2.len(), 2);
        assert_eq!(text_of(t2[0]), " ");
        assert_eq!(text_of(t2[1]), "gamma");
        assert_eq!(t2[1].GetPosition().x, 0);
        assert!(t2[1].GetPosition().y > f[1].GetPosition().y);
        assert_eq!(t2[1].GetSize().width, W(f[1], "gamma"));
        // the trailing space of the first line does not take any width
        assert_eq!(t2[0].GetSize().width, 0);
    }

    #[test]
    fn a_text_frame_without_text_content_still_works_through_its_stub() {
        // the tests of the line layout give a text frame canned sizes (TextFrameStub): the frame is not
        // the real one then
        let html = r#"<div id=root style="margin:0;border:0;padding:0;width:100px"><span id=t0></span></div>"#;
        let f = frames(html, &[("root", None), ("t0", Some(0))], 100);
        f[1].SetType(LayoutFrameType::Text);
        f[1].SetTextFrameStub(TextFrameStub { mISize: 30 * PX, mBSize: 10 * PX, mAscent: 8 * PX, ..Default::default() });
        assert!(!f[1].UsesRealTextFrame());
        assert!(f[1].HasNoncollapsedCharacters());
    }

    #[test]
    fn set_length_moves_text_to_the_next_frame_and_back() {
        // nsTextFrame::SetLength: a frame that shrinks gives its text to the next-in-flow, one that
        // grows takes it back.
        let probe = paragraph("", 800, TEXT);
        let narrow = px_below(W(probe[1], "alpha beta"));
        let f = paragraph("", narrow, TEXT);
        reflow(f[0]);
        let frames = chain(f[1]);
        let (second, third) = (frames[1], frames[2]);
        let end = second.GetContentEnd();
        frames[0].SetLength(2, None, 0);
        assert_eq!(frames[0].GetContentLength(), 2);
        assert_eq!(second.GetContentOffset(), 2);
        assert_eq!(second.GetContentEnd(), end);
        // growing again takes the text back from the next frame
        frames[0].SetLength(6, None, 0);
        assert_eq!(second.GetContentOffset(), 6);
        let _ = third;
    }

    #[test]
    fn tab_widths_are_cached_on_the_frame_and_dropped_when_the_offset_moves() {
        let f = paragraph("white-space:pre", 800, "a\tb");
        reflow(f[0]);
        let store = nsTextFrame(f[1]).GetProperty(TabWidthProperty());
        let store = store.expect("the reflow measured the tab");
        assert_eq!(store.borrow().mWidths.len(), 1);
        assert_eq!(store.borrow().mWidths[0].mOffset, 1);
        assert_eq!(store.borrow().mValidForContentOffset, 0);
    }

    #[test]
    fn the_overflow_covers_the_text() {
        let f = paragraph("", 800, "alpha");
        reflow(f[0]);
        let areas = nsTextFrame(f[1]).RecomputeOverflow(f[0], true);
        let size = f[1].GetSize();
        assert!(areas.InkOverflow().width >= size.width);
        assert!(areas.InkOverflow().height >= size.height);
    }

    #[test]
    fn a_character_data_buffer_has_one_byte_text_for_latin1_and_two_byte_text_otherwise() {
        let latin1 = CharacterDataBuffer::from_str("caf\u{e9}");
        assert!(!latin1.Is2b());
        assert_eq!(latin1.GetLength(), 4);
        assert_eq!(latin1.CharAt(3), 0xE9);
        let wide = CharacterDataBuffer::from_str("a\u{4e2d}\u{1F600}");
        assert!(wide.Is2b());
        assert_eq!(wide.GetLength(), 4);
        assert_eq!(wide.ScalarValueAt(2), 0x1F600);
        assert_eq!(wide.ScalarValueAt(3), 0);
    }

    #[test]
    fn east_asian_text_wraps_between_characters() {
        // UAX #14: ideographs break between each other
        let probe = paragraph("", 800, "\u{4e2d}\u{6587}\u{5b57}\u{7b26}\u{6e2c}\u{8a66}");
        let width = px_fit(W(probe[1], "\u{4e2d}\u{6587}\u{5b57}"));
        let f = paragraph("", width, "\u{4e2d}\u{6587}\u{5b57}\u{7b26}\u{6e2c}\u{8a66}");
        reflow(f[0]);
        let lines = lines_of(f[1]);
        assert!(lines.len() >= 2, "{lines:?}");
        assert_eq!(lines.concat(), "\u{4e2d}\u{6587}\u{5b57}\u{7b26}\u{6e2c}\u{8a66}");
    }

    #[test]
    fn widening_the_container_takes_the_text_back_into_fewer_frames() {
        // nsTextFrame::SetLength / RemoveEmptyInFlows: when the first frame grows on a later reflow, the
        // continuing frames that are left without text are removed.
        let probe = paragraph("", 800, TEXT);
        let narrow = px_below(W(probe[1], "alpha beta"));
        let f = paragraph("", narrow, TEXT);
        reflow(f[0]);
        assert_eq!(chain(f[1]).len(), 5);
        // a wide container, and everything dirty (a resize reflow)
        f[0].SetRect(nsRect::new(0, 0, 800 * PX, 0));
        for line in nsBlockFrame::from_frame(f[0]).Lines() {
            let mut line = line;
            line.MarkDirty();
        }
        f[0].AddStateBits(NS_FRAME_IS_DIRTY);
        for fr in chain(f[1]) {
            fr.AddStateBits(NS_FRAME_IS_DIRTY);
        }
        reflow(f[0]);
        assert_eq!(lines_of(f[1]), vec![TEXT.to_string()]);
        assert_eq!(nsBlockFrame::from_frame(f[0]).Lines().count(), 1);
        // the other frames are gone from the parent's child list
        assert_eq!(nsBlockFrame::from_frame(f[0]).Lines().map(|l| l.GetChildCount()).sum::<i32>(), 1);
    }

    #[test]
    fn an_inline_element_that_wraps_gets_continuing_text_frames_in_its_continuation() {
        // <b>beta gamma delta</b> wrapping: the inline gets a continuation for the second line, and the
        // text that moves there is in a continuing text frame whose parent is that continuation.
        let html = r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0;font:20px sans-serif;width:__W__px">
            <span id=t0></span><span id=b style="display:inline"><span id=t1></span></span></div>"#;
        let spec = [("root", None, K::Block), ("t0", Some(0), K::Text("alpha ")), ("b", Some(0), K::Inline), ("t1", Some(2), K::Text("beta gamma delta"))];
        let probe = build(&html.replace("__W__", "800"), &spec, 800);
        let width = px_fit(W(probe[1], "alpha beta gamma"));
        assert!(W(probe[1], "alpha beta gamma delta") > width * PX);
        let f = build(&html.replace("__W__", &width.to_string()), &spec, width);
        reflow(f[0]);
        assert_eq!(nsBlockFrame::from_frame(f[0]).Lines().count(), 2);
        let text = chain(f[3]);
        assert_eq!(text.iter().copied().map(text_of).collect::<Vec<_>>(), vec!["beta gamma ".to_string(), "delta".to_string()]);
        let inline_chain: Vec<nsIFrame> = {
            let mut v = vec![f[2]];
            while let Some(n) = v.last().unwrap().GetNextInFlow() {
                v.push(n);
            }
            v
        };
        assert_eq!(inline_chain.len(), 2);
        assert_eq!(text[0].GetParent(), Some(inline_chain[0]));
        assert_eq!(text[1].GetParent(), Some(inline_chain[1]));
    }

    #[test]
    fn text_align_justify_lays_out_like_start_without_justification() {
        // NOT PORTED: justification. The text is placed as for `start` and keeps its natural width.
        let probe = paragraph("", 800, TEXT);
        let width = px_fit(W(probe[1], "alpha beta gamma"));
        let f = paragraph("text-align:justify", width, TEXT);
        reflow(f[0]);
        assert_eq!(lines_of(f[1]), vec!["alpha beta gamma ".to_string(), "delta epsilon".to_string()]);
        assert_eq!(chain(f[1])[0].GetSize().width, W(f[1], "alpha beta gamma"));
        assert_eq!(chain(f[1])[0].GetPosition().x, 0);
    }

    #[test]
    fn the_second_reflow_of_a_clean_flow_leaves_the_frames_alone() {
        let probe = paragraph("", 800, TEXT);
        let width = px_fit(W(probe[1], "alpha beta gamma"));
        let f = paragraph("", width, TEXT);
        reflow(f[0]);
        let before: Vec<(i32, i32)> = chain(f[1]).iter().map(|t| (t.GetContentOffset(), t.GetContentLength())).collect();
        reflow(f[0]);
        let after: Vec<(i32, i32)> = chain(f[1]).iter().map(|t| (t.GetContentOffset(), t.GetContentLength())).collect();
        assert_eq!(before, after);
    }
}

// ---------------------------------------------------------------------------------------------
// Instrumentation (bench/textrun_profile.py, layoutprof --textruns): reads the text runs of all
// text frames; changes nothing.
// ---------------------------------------------------------------------------------------------

// One distinct text run, as the text frames hold it.
pub struct TextRunRecord {
    // gfxTextRun::MemStats
    pub mem: ([usize; 11], usize, usize, usize, (usize, usize, usize), usize, usize, u32),
    // text frames that hold this run as mTextRun / as mUninflatedTextRun
    pub frames: u32,
    pub frames_uninflated: u32,
    // Rc strong count (frames + scanner leftovers)
    pub strong: usize,
    // mapped flows of the user data: length and capacity of the Vec
    pub flows: usize,
    pub flows_cap: usize,
    // the text frame ids that start each mapped flow
    pub flow_starts: Vec<u32>,
    // (font address, word) of the whitespace-separated words of the mapped flows (untransformed text)
    pub words: Vec<(usize, String)>,
    // the text of each mapped flow (for comparing run boundaries with Firefox's)
    pub flow_texts: Vec<String>,
}

pub struct TextRunReport {
    pub runs: Vec<TextRunRecord>,
    pub text_frames: usize,
    pub continuing_frames: usize,
    pub frames_with_run: usize,
    pub userdata_len: usize,
    pub userdata_capacity: usize,
    pub size_of_userdata_entry: usize,
    pub size_of_mapped_flow: usize,
    pub size_of_text_frame_data: usize,
    pub text_frames_capacity: usize,
}

pub fn TextRunReportOfAllFrames() -> TextRunReport {
    let mut by_ptr: HashMap<usize, (RefPtr_gfxTextRun, u32, u32)> = HashMap::new();
    let (mut text_frames, mut continuing, mut with_run, mut cap) = (0, 0, 0, 0);
    TEXT_FRAMES.with(|t| {
        let t = t.borrow();
        cap = t.mData.capacity();
        for (id, slot) in t.mSlots.iter().enumerate() {
            if *slot == 0 {
                continue;
            }
            let d = &t.mData[*slot as usize - 1];
            let uninflated = nsIFrame(id as u32).GetProperty(UninflatedTextRunProperty());
            text_frames += 1;
            if d.mIsContinuing {
                continuing += 1;
            }
            if d.mTextRun.is_some() || uninflated.is_some() {
                with_run += 1;
            }
            if let Some(r) = &d.mTextRun {
                by_ptr.entry(Rc::as_ptr(r) as usize).or_insert_with(|| (r.clone(), 0, 0)).1 += 1;
            }
            if let Some(r) = &uninflated {
                by_ptr.entry(Rc::as_ptr(r) as usize).or_insert_with(|| (r.clone(), 0, 0)).2 += 1;
            }
        }
    });
    let mut runs = Vec::new();
    for (key, (run, frames, uninflated)) in by_ptr {
        let mem = run.borrow().MemStats();
        let font = mem.6;
        // (a simple flow stores no array: one flow, no capacity)
        let (flows, flows_cap, starts) = match run.borrow().GetUserData() {
            gfxTextRun_UserData::None => (0, 0, Vec::new()),
            gfxTextRun_UserData::SimpleFlow(f) => (1, 0, vec![(f.0).0]),
            gfxTextRun_UserData::Flows(u) => (u.mMappedFlows.len(), u.mMappedFlows.len(), u.mMappedFlows.iter().map(|f| (f.mStartFrame.0).0).collect()),
        };
        let mut words = Vec::new();
        let mut flow_texts = Vec::new();
        if HasTextRunUserData(&run) {
            for f in GetMappedFlows(&run) {
                let buf = f.mStartFrame.CharacterDataBuffer();
                let start = f.mStartFrame.GetContentOffset() as usize;
                let end = start + f.mContentLength as usize;
                let text: String = if buf.Is2b() { String::from_utf16_lossy(&buf.Get2b()[start..end]) } else { buf.Get1b()[start..end].iter().map(|&b| b as char).collect() };
                for w in text.split_whitespace() {
                    words.push((font, w.to_string()));
                }
                flow_texts.push(text);
            }
        }
        runs.push(TextRunRecord { mem, frames, frames_uninflated: uninflated, strong: Rc::strong_count(&run) - 1, flows, flows_cap, flow_starts: starts, words, flow_texts });
    }
    // (the user data lives in the runs: there is no table)
    let (ul, uc) = (0usize, 0usize);
    TextRunReport {
        runs,
        text_frames,
        continuing_frames: continuing,
        frames_with_run: with_run,
        userdata_len: ul,
        userdata_capacity: uc,
        size_of_userdata_entry: std::mem::size_of::<(usize, TextRunUserData)>(),
        size_of_mapped_flow: std::mem::size_of::<TextRunMappedFlow>(),
        size_of_text_frame_data: std::mem::size_of::<nsTextFrame_Data>(),
        text_frames_capacity: cap,
    }
}
