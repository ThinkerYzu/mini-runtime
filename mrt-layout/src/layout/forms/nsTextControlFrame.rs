/* rendering object for the text controls: <input type=text, email, tel, url, password, search, number>
   and <textarea> */
// Translated from layout/forms/nsTextControlFrame.h and nsTextControlFrame.cpp (GECKO_REV): the sizing
// (CalcFixedSize, IntrinsicISize, ComputeBaseline) and Reflow, as far as sizes go.
//
// The class is a ScrollContainerFrame in Gecko; `nsTextControlFrame` here is a handle to a frame of type
// TextInput (Deref to the frame handle). What is virtual in C++ is a `match` on the frame type:
// IntrinsicISize (nsIFrame.rs), ComputeAutoSize (nsIFrame.rs), Reflow (nsLineLayoutAdapter.rs).
//
// ADAPTER: the frame has no children (no anonymous editor, placeholder, preview or spin button): the
// ScrollContainerFrame::Reflow of Gecko, which reflows the editor inside the border and padding, is
// reduced to the box it ends with: the computed content size inside the border and padding.
// `field-sizing: content` (StyleFieldSizing::Content) is not in the servo configuration of Stylo, so
// the size is always the fixed one. The attributes GetCols and GetRows read are given by the frame
// constructor (FormControlData).
//
// NOT PORTED: the anonymous content and its reflow, the button (spin box, reveal, clear) and its
// minimum inline size added to a control with explicit cols, the letter-spacing added per column,
// the scrollbar sizes of a textarea (the harness has overlay scrollbars, which take no room: the
// desired scrollbar sizes are 0), the selection and editor, GetNaturalBaselineBOffset (the baseline
// is given to the line layout by Reflow), PeekOffset, state changes.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::Deref;

// Gecko: dom/html/TextControlElement.h TextControlElement::DEFAULT_COLS
const DEFAULT_COLS: i32 = 20;

// Gecko: layout/forms/nsTextControlFrame.h nsTextControlFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsTextControlFrame(pub nsIFrame);

impl Deref for nsTextControlFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/forms/nsTextControlFrame.cpp NS_NewTextControlFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (see NS_NewBlockFrame).
pub fn NS_NewTextControlFrame(aFrame: nsIFrame) -> nsTextControlFrame {
    aFrame.SetType(LayoutFrameType::TextInput);
    nsTextControlFrame(aFrame)
}

impl nsTextControlFrame {
    // static_cast<nsTextControlFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsTextControlFrame {
        nsTextControlFrame(aFrame)
    }

    // Gecko: layout/forms/nsTextControlFrame.h nsTextControlFrame::IsSingleLineTextControl
    // (DEFINE_TEXTCTRL_CONST_FORWARDER: the element's answer, from the data the constructor read)
    pub fn IsSingleLineTextControl(&self) -> bool {
        !self.FormControlData().mIsTextArea
    }

    // Gecko: layout/forms/nsTextControlFrame.h nsTextControlFrame::GetCols
    pub fn GetCols(&self) -> Option<i32> {
        self.FormControlData().mCols
    }

    // Gecko: layout/forms/nsTextControlFrame.h nsTextControlFrame::GetRows
    pub fn GetRows(&self) -> i32 {
        self.FormControlData().mRows
    }

    // Gecko: layout/forms/nsTextControlFrame.cpp nsTextControlFrame::CalcFixedSize
    pub fn CalcFixedSize(&self, aWM: WritingMode) -> LogicalSize {
        // MOZ_ASSERT(StyleUIReset()->mFieldSizing == StyleFieldSizing::Fixed);

        let mut intrinsicSize = LogicalSize::new(aWM, 0, 0);
        let inflation = nsLayoutUtils::FontSizeInflationFor(self.0);
        let fontMet = nsLayoutUtils::GetFontMetricsForFrame(self.0, inflation);
        let lineHeight = ReflowInput::CalcLineHeight__ComputedStyle_nsPresContext_nsIContent_nscoord_float(&self.Style(), Some(&self.PresContext()), self.GetContent().as_ref(), NS_UNCONSTRAINEDSIZE, inflation);
        // Use the larger of the font's "average" char width or the width of the
        // zero glyph (if present) as the basis for resolving the size attribute.
        let charWidth: nscoord = std::cmp::max(fontMet.ZeroOrAveCharWidth(), fontMet.AveCharWidth());
        let charMaxAdvance: nscoord = fontMet.MaxAdvance();

        // Initialize based on the width in characters.
        let maybeCols = self.GetCols();
        let cols: i32 = maybeCols.unwrap_or(DEFAULT_COLS);
        *intrinsicSize.ISize_mut(aWM) = cols * charWidth;

        // If we do not have what appears to be a fixed-width font, add a "slop"
        // amount based on the max advance of the font (clamped to twice charWidth,
        // because some fonts have a few extremely-wide outliers that would result
        // in excessive width here; e.g. the triple-emdash ligature in SFNS Text),
        // minus 4px. This helps avoid input fields becoming unusably narrow with
        // small size values.
        if charMaxAdvance - charWidth > AppUnitsPerCSSPixel() {
            let mut internalPadding: nscoord = std::cmp::max(0, std::cmp::min(charMaxAdvance, charWidth * 2) - nsPresContext::CSSPixelsToAppUnits__int32_t(4));
            internalPadding = RoundToMultiple(internalPadding, AppUnitsPerCSSPixel());
            *intrinsicSize.ISize_mut(aWM) += internalPadding;
        }

        // Increment width with cols * letter-spacing.
        // NOT PORTED: letter-spacing.

        // Set the height equal to total number of rows (times the height of each
        // line, of course)
        *intrinsicSize.BSize_mut(aWM) = lineHeight * self.GetRows();

        // Add in the size of the scrollbars for textarea
        // NOT PORTED: the scrollbars (overlay, 0 wide).

        // Add the inline size of the button if our char size is explicit, so as to
        // make sure to make enough space for it.
        // NOT PORTED: the button (the frame has no anonymous content).

        intrinsicSize
    }

    // Gecko: layout/forms/nsTextControlFrame.cpp nsTextControlFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        // if (StyleUIReset()->mFieldSizing == StyleFieldSizing::Content): NOT PORTED
        // Our min inline size is just our preferred inline-size if we have auto
        // inline size.
        let wm = self.GetWritingMode();
        self.CalcFixedSize(wm).ISize(wm)
    }

    // Gecko: layout/forms/nsTextControlFrame.cpp nsTextControlFrame::ComputeBaseline
    pub fn ComputeBaseline(aFrame: nsIFrame, aReflowInput: &ReflowInput, aForSingleLineControl: bool) -> Option<nscoord> {
        // If we're layout-contained, we have no baseline.
        if aReflowInput.mStyleDisplay.IsContainLayout() {
            return None;
        }
        let wm = aReflowInput.GetWritingMode();

        let mut lineHeight = aReflowInput.ComputedBSize();
        if !aForSingleLineControl || lineHeight == NS_UNCONSTRAINEDSIZE {
            lineHeight = aReflowInput.ApplyMinMaxBSize(aReflowInput.GetLineHeight(), 0);
        }
        let fontMet = nsLayoutUtils::GetInflatedFontMetricsForFrame(aFrame);
        Some(nsLayoutUtils::GetCenteredFontBaseline(&fontMet, lineHeight, wm.IsLineInverted()) + aReflowInput.ComputedLogicalBorderPadding(wm).BStart(wm))
    }

    // Gecko: layout/forms/nsTextControlFrame.cpp nsTextControlFrame::Reflow
    // ADAPTER: ScrollContainerFrame::Reflow is reduced to the border box of the computed content size.
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");
        {
            // Calculate the baseline and store it in mFirstBaseline.
            // TODO(emilio): Is this still needed?
            let baseline = nsTextControlFrame::ComputeBaseline(self.0, aReflowInput, self.IsSingleLineTextControl());
            if let Some(baseline) = baseline {
                aDesiredSize.SetBlockStartAscent(baseline);
            }
        }

        // FIXME(emilio): This is rather hacky, but matches how nsBlockFrame tweaks
        // avail bsize. Maybe do a copy instead, or plumb stuff further down but...
        let wm = aReflowInput.GetWritingMode();
        let mut bSize = aReflowInput.ComputedBSize();
        if bSize == NS_UNCONSTRAINEDSIZE {
            bSize = aReflowInput.ApplyMinMaxBSize(self.CalcFixedSize(wm).BSize(wm), 0);
        }
        // ScrollContainerFrame::Reflow, reduced: the border box around the content box.
        let borderPadding = aReflowInput.ComputedLogicalBorderPadding(wm);
        let size = LogicalSize::new(wm, aReflowInput.ComputedISize() + borderPadding.IStartEnd(wm), bSize + borderPadding.BStartEnd(wm));
        aDesiredSize.SetSize(wm, &size);
        aDesiredSize.SetOverflowAreasToDesiredBounds();
    }
}
