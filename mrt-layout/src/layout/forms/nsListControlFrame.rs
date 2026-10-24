/* rendering object for the list box <select> (a <select> that is `multiple` or has a size above 1) */
// Translated from layout/forms/nsListControlFrame.h and nsListControlFrame.cpp (GECKO_REV): the sizing
// (CalcBSizeOfARow, CalcFallbackRowBSize, IntrinsicISize, CalcIntrinsicBSize) and a reduced Reflow.
//
// The class is a ScrollContainerFrame in Gecko; `nsListControlFrame` here is a handle to a frame of type
// ListControl (Deref to the frame handle). What is virtual in C++ is a `match` on the frame type:
// IntrinsicISize and ComputeAutoSize (nsIFrame.rs), Reflow (nsLineLayoutAdapter.rs).
//
// ADAPTER: the frame has no children (no scrolled frame with an option frame per <option>): the options
// come from the frame constructor (FormControlData) with their padding, and a row is the tallest option,
// which is a line of line-height `normal` (the UA sheet forces it for the options of a list box) plus the
// padding of the option. The inline size of the scrolled frame is the widest option: its text, measured
// with the frame's font metrics, plus the option's padding. An <optgroup> label is measured in the regular
// font (Gecko uses bold italic: NOT PORTED). The scrollbar sizes are 0 (the harness has overlay scrollbars).
//
// NOT PORTED: the children and the second reflow pass (mMightNeedSecondPass: the rows are known up front
// here), contain-size, field-sizing, selection, events.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::Deref;

// Gecko: layout/forms/nsListControlFrame.h nsListControlFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsListControlFrame(pub nsIFrame);

impl Deref for nsListControlFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/forms/nsListControlFrame.cpp NS_NewListControlFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (see NS_NewBlockFrame).
pub fn NS_NewListControlFrame(aFrame: nsIFrame) -> nsListControlFrame {
    aFrame.SetType(LayoutFrameType::ListControl);
    nsListControlFrame(aFrame)
}

impl nsListControlFrame {
    // static_cast<nsListControlFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsListControlFrame {
        nsListControlFrame(aFrame)
    }

    // Gecko: layout/forms/nsListControlFrame.cpp nsListControlFrame::CalcFallbackRowBSize
    pub fn CalcFallbackRowBSize(&self, aFontSizeInflation: f32) -> nscoord {
        let fontMet = nsLayoutUtils::GetFontMetricsForFrame(self.0, aFontSizeInflation);
        fontMet.MaxHeight()
    }

    // Gecko: layout/forms/nsListControlFrame.cpp nsListControlFrame::CalcBSizeOfARow
    pub fn CalcBSizeOfARow(&self) -> nscoord {
        // Calculate the block size in our writing mode of a single row in the
        // listbox or dropdown list by using the tallest thing in the subtree,
        // since there may be option groups in addition to option elements,
        // either of which may be visible or invisible, may use different
        // fonts, etc.
        // ADAPTER: GetMaxRowBSize over the option frames, from the options the constructor read; the
        // line of an option is `normal` (the UA rule `select:-moz-select-list-box option { line-height:
        // normal !important }`) in the font of the select.
        let mut rowBSize: nscoord = 0;
        let mut found = false;
        let styleFont = nsStyleFont::new(self.Style());
        let lineHeight = ReflowInput::CalcLineHeight__StyleLineHeight_nsStyleFont_nsPresContext_bool_nsIContent_nscoord_float(
            &StyleLineHeight::Normal,
            &styleFont,
            Some(&self.PresContext()),
            false,
            self.GetContent().as_ref(),
            NS_UNCONSTRAINEDSIZE,
            nsLayoutUtils::FontSizeInflationFor(self.0),
        );
        for option in self.FormControlData().mOptions.iter() {
            // an <optgroup> label has no padding of its own
            let childBSize = lineHeight + option.mPaddingBSize;
            // XXX bug 1499176: skip empty <optgroup> labels (zero bsize) for now
            if !option.mIsOptGroupLabel || childBSize > 0 {
                found = true;
                rowBSize = std::cmp::max(childBSize, rowBSize);
            }
        }
        if !found {
            // We don't have any <option>s or <optgroup> labels with a frame.
            let inflation = nsLayoutUtils::FontSizeInflationFor(self.0);
            rowBSize = self.CalcFallbackRowBSize(inflation);
        }
        rowBSize
    }

    // Gecko: layout/forms/nsListControlFrame.cpp nsListControlFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        // Always add scrollbar inline sizes to the intrinsic isize of the
        // scrolled content. Combobox frames depend on this happening in the
        // dropdown, and standalone listboxes are overflow:scroll so they need
        // it too.
        // ADAPTER: GetScrolledFrame()->IntrinsicISize(): the widest option block; the scrollbar is 0 wide.
        let fm = nsLayoutUtils::GetInflatedFontMetricsForFrame(self.0);
        let mut result: nscoord = 0;
        for option in self.FormControlData().mOptions.iter() {
            result = std::cmp::max(result, AppUnitWidthOfString(&option.mLabel, &fm) + option.mPaddingISize);
        }
        result
    }

    // Gecko: layout/forms/nsListControlFrame.cpp nsListControlFrame::CalcIntrinsicBSize
    pub fn CalcIntrinsicBSize(&self, aBSizeOfARow: nscoord, aNumberOfOptions: i32) -> nscoord {
        // (field-sizing: content: NOT PORTED)
        // mNumDisplayRows = Select().Size();
        let mut numDisplayRows = self.FormControlData().mSelectSize;
        if numDisplayRows < 1 {
            numDisplayRows = 4;
        }
        numDisplayRows * aBSizeOfARow
    }

    // Gecko: layout/forms/nsListControlFrame.cpp nsListControlFrame::Reflow
    // ADAPTER: reduced to the border box of the computed content size, which is the rows of an
    // auto-height list box.
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");
        NS_WARNING_ASSERTION!(aReflowInput.ComputedISize() != NS_UNCONSTRAINEDSIZE, "Must have a computed inline size");
        self.MarkInReflow();
        let wm = aReflowInput.GetWritingMode();

        let mut bSize = aReflowInput.ComputedBSize();
        if bSize == NS_UNCONSTRAINEDSIZE {
            let length = self.FormControlData().mOptions.len() as i32;
            bSize = self.CalcIntrinsicBSize(self.CalcBSizeOfARow(), length);
            bSize = aReflowInput.ApplyMinMaxBSize(bSize, 0);
        }
        let borderPadding = aReflowInput.ComputedLogicalBorderPadding(wm);
        let size = LogicalSize::new(wm, aReflowInput.ComputedISize() + borderPadding.IStartEnd(wm), bSize + borderPadding.BStartEnd(wm));
        aDesiredSize.SetSize(wm, &size);
        aDesiredSize.SetOverflowAreasToDesiredBounds();
    }
}
