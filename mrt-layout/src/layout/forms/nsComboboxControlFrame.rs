/* rendering object for the drop-down <select> (a <select> that is not `multiple` and has size 1 or less) */
// Translated from layout/forms/nsComboboxControlFrame.h and nsComboboxControlFrame.cpp (GECKO_REV): the
// sizing (DropDownButtonISize, GetLongestOptionISize, IntrinsicISize) and a reduced Reflow.
//
// The class is a ButtonControlFrame (an nsBlockFrame) in Gecko; `nsComboboxControlFrame` here is a handle to a
// frame of type ComboboxControl (Deref to the frame handle). What is virtual in C++ is a `match` on the frame
// type: IntrinsicISize and ComputeAutoSize (nsIFrame.rs), Reflow (nsLineLayoutAdapter.rs).
//
// ADAPTER: the frame has no children (no label frame holding the selected option's text, no drop-down
// button): the line the label frame lays out gives the frame a content block size of one line, which Reflow
// uses, and the button's width is the theme's, as DropDownButtonISize says. The options come from the frame
// constructor (FormControlData). The text of an option is measured with the frame's font metrics, as
// GetLongestOptionISize does with AppUnitWidthOfStringBidi.
//
// NOT PORTED: the children, the drop-down popup, the label's reflow with mDisplayISize, text-transform and
// -webkit-text-security of the labels (GetLongestOptionISize's TransformString), contain-intrinsic-size,
// field-sizing, events, GetNaturalBaselineBOffset (Reflow gives the line layout the baseline).
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::Deref;

// Gecko: widget/Theme.cpp kMinimumDropdownArrowButtonWidth (the theme's minimum width of
// StyleAppearance::MozMenulistArrowButton)
const kMinimumDropdownArrowButtonWidth: i32 = 18;

// Gecko: layout/forms/nsComboboxControlFrame.h nsComboboxControlFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsComboboxControlFrame(pub nsIFrame);

impl Deref for nsComboboxControlFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/forms/nsComboboxControlFrame.cpp NS_NewComboboxControlFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (see NS_NewBlockFrame).
pub fn NS_NewComboboxControlFrame(aFrame: nsIFrame) -> nsComboboxControlFrame {
    aFrame.SetType(LayoutFrameType::ComboboxControl);
    nsComboboxControlFrame(aFrame)
}

// ADAPTER: the width of a string in the font of aMetrics (nsLayoutUtils::AppUnitWidthOfStringBidi, without
// the bidi levels: the text is measured as one run).
pub fn AppUnitWidthOfString(aString: &str, aFontMetrics: &nsFontMetrics) -> nscoord {
    let utf16: Vec<u16> = aString.encode_utf16().collect();
    aFontMetrics.GetWidth__char16_t_ptr_uint32_t_DrawTarget_ptr(&utf16, utf16.len() as u32, None)
}

impl nsComboboxControlFrame {
    // static_cast<nsComboboxControlFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsComboboxControlFrame {
        nsComboboxControlFrame(aFrame)
    }

    // Gecko: layout/forms/nsComboboxControlFrame.cpp nsComboboxControlFrame::DropDownButtonISize
    pub fn DropDownButtonISize(&self) -> nscoord {
        // pc->Theme()->GetMinimumWidgetSize(pc, this, StyleAppearance::MozMenulistArrowButton).width
        // (in device pixels, which are CSS pixels here)
        kMinimumDropdownArrowButtonWidth * AppUnitsPerCSSPixel()
    }

    // Gecko: layout/forms/nsComboboxControlFrame.cpp nsComboboxControlFrame::GetLongestOptionISize
    pub fn GetLongestOptionISize(&self) -> nscoord {
        // Compute the width of each option's (potentially text-transformed) text,
        // and use the widest one as part of our intrinsic size.
        let mut maxOptionSize: nscoord = 0;
        let fm = nsLayoutUtils::GetInflatedFontMetricsForFrame(self.0);
        // (Select().Options(): the <option>s, not the <optgroup> labels)
        for option in self.FormControlData().mOptions.iter().filter(|o| !o.mIsOptGroupLabel) {
            maxOptionSize = std::cmp::max(maxOptionSize, AppUnitWidthOfString(&option.mLabel, &fm));
        }
        if maxOptionSize != 0 {
            // HACK: Add one app unit to workaround silly Netgear router styling, see
            // bug 1769580. In practice since this comes from font metrics is unlikely
            // to be perceivable.
            maxOptionSize += 1;
        }
        maxOptionSize
    }

    // Gecko: layout/forms/nsComboboxControlFrame.cpp nsComboboxControlFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        // contain-intrinsic-size, field-sizing: content: NOT PORTED
        let mut displayISize: nscoord = 0;
        displayISize += self.GetLongestOptionISize();

        // Add room for the dropmarker button (if there is one).
        displayISize += self.DropDownButtonISize();
        displayISize
    }

    // Gecko: layout/forms/nsComboboxControlFrame.cpp nsComboboxControlFrame::Reflow
    // ADAPTER: reduced to the border box of the computed content size; the block size of an auto-height
    // combobox is the one line of the label (ComboboxLabelFrame, an nsBlockFrame with a line-height of
    // the select's), the inline size is the computed one.
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");
        let wm = aReflowInput.GetWritingMode();

        let mut bSize = aReflowInput.ComputedBSize();
        if bSize == NS_UNCONSTRAINEDSIZE {
            bSize = aReflowInput.ApplyMinMaxBSize(aReflowInput.GetLineHeight(), 0);
        }
        let borderPadding = aReflowInput.ComputedLogicalBorderPadding(wm);
        let size = LogicalSize::new(wm, aReflowInput.ComputedISize() + borderPadding.IStartEnd(wm), bSize + borderPadding.BStartEnd(wm));
        aDesiredSize.SetSize(wm, &size);
        // The baseline of the label's line: the font's, centered in the line, below the border and padding.
        let lineHeight = aReflowInput.ApplyMinMaxBSize(aReflowInput.GetLineHeight(), 0);
        let fontMet = nsLayoutUtils::GetInflatedFontMetricsForFrame(self.0);
        aDesiredSize.SetBlockStartAscent(nsLayoutUtils::GetCenteredFontBaseline(&fontMet, lineHeight, wm.IsLineInverted()) + borderPadding.BStart(wm));
        aDesiredSize.SetOverflowAreasToDesiredBounds();
    }
}
