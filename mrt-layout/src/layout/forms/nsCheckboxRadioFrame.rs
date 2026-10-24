/* rendering object for HTML <input type="checkbox"> and <input type="radio"> */
// Translated from layout/forms/nsCheckboxRadioFrame.h and nsCheckboxRadioFrame.cpp (GECKO_REV): the sizing
// (DefaultSize, IntrinsicISize, ComputeAutoSize, GetNaturalBaselineBOffset) and Reflow.
//
// The class is an nsAtomicContainerFrame in Gecko; `nsCheckboxRadioFrame` here is a handle to a frame of type
// CheckboxRadio (Deref to the frame handle). What is virtual in C++ is a `match` on the frame type:
// IntrinsicISize (nsIFrame.rs), ComputeAutoSize (nsIFrame.rs), Reflow (nsLineLayoutAdapter.rs),
// GetNaturalBaselineBOffset (nsBlockFrame.rs).
//
// ADAPTER: the native theme. `StyleDisplay()->HasNativeAppearance()` is always true here: the servo
// configuration of Stylo has no `appearance` property, so an author's `appearance: none` is not seen.
// The theme's checkbox size (Theme::GetCheckboxRadioPrefSize, kCheckboxRadioSize in widget/Theme.cpp:
// 14px) and its border width (kCheckboxRadioBorderWidth, 1px) are the constants of the non-native
// theme, which Gecko uses for checkboxes and radios on every platform; Firefox measures 14 by 14 here.
//
// NOT PORTED: painting, events, the placeholder children reflow (the frame has no children),
// font size inflation (it is off), the zoom (EffectiveZoom is 1).
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::Deref;

// Gecko: widget/Theme.cpp kCheckboxRadioSize
const kCheckboxRadioSize: i32 = 14;
// Gecko: widget/Theme.cpp kCheckboxRadioBorderWidth
const kCheckboxRadioBorderWidth: i32 = 1;

// Gecko: layout/forms/nsCheckboxRadioFrame.h nsCheckboxRadioFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsCheckboxRadioFrame(pub nsIFrame);

impl Deref for nsCheckboxRadioFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/forms/nsCheckboxRadioFrame.cpp NS_NewCheckboxRadioFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (see NS_NewBlockFrame).
pub fn NS_NewCheckboxRadioFrame(aFrame: nsIFrame) -> nsCheckboxRadioFrame {
    aFrame.SetType(LayoutFrameType::CheckboxRadio);
    nsCheckboxRadioFrame(aFrame)
}

impl nsCheckboxRadioFrame {
    // static_cast<nsCheckboxRadioFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsCheckboxRadioFrame {
        nsCheckboxRadioFrame(aFrame)
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::HasNativeAppearance
    // ADAPTER: always true, see the top of this file.
    fn HasNativeAppearance(&self) -> bool {
        true
    }

    // Gecko: layout/forms/nsCheckboxRadioFrame.cpp nsCheckboxRadioFrame::DefaultSize
    pub fn DefaultSize(&self) -> nscoord {
        // PresContext()->Theme()->GetCheckboxRadioPrefSize() : CSSCoord(13.0f)
        let size = if self.HasNativeAppearance() { kCheckboxRadioSize } else { 13 };
        // CSSPixel::ToAppUnits(Style()->EffectiveZoom().Zoom(size))
        size * AppUnitsPerCSSPixel()
    }

    // Gecko: layout/forms/nsCheckboxRadioFrame.cpp nsCheckboxRadioFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        if self.HasNativeAppearance() { self.DefaultSize() } else { 0 }
    }

    // Gecko: layout/forms/nsCheckboxRadioFrame.cpp nsCheckboxRadioFrame::ComputeAutoSize
    pub fn ComputeAutoSize(
        &self,
        aSizingInput: &SizeComputationInput,
        aWM: WritingMode,
        aCBSize: LogicalSize,
        aAvailableISize: nscoord,
        aMargin: LogicalSize,
        aBorderPadding: LogicalSize,
        aSizeOverrides: &StyleSizeOverrides,
        aFlags: ComputeSizeFlags,
    ) -> LogicalSize {
        let size = LogicalSize::new(aWM, 0, 0);
        if !self.HasNativeAppearance() {
            return size;
        }
        // nsAtomicContainerFrame::ComputeAutoSize is nsContainerFrame's
        self.nsContainerFrame_ComputeAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags)
    }

    // Gecko: layout/forms/nsCheckboxRadioFrame.cpp nsCheckboxRadioFrame::GetNaturalBaselineBOffset
    pub fn GetNaturalBaselineBOffset(&self, aWM: WritingMode, aBaselineGroup: BaselineSharingGroup, aExportContext: BaselineExportContext) -> Option<nscoord> {
        // NS_ASSERTION(!IsSubtreeDirty(), "frame must not be dirty");

        if aBaselineGroup == BaselineSharingGroup::Last {
            return None;
        }

        if self.StyleDisplay().IsBlockOutsideStyle() {
            return None;
        }

        // For appearance:none we use a standard CSS baseline, i.e. synthesized from
        // our margin-box.
        if !self.HasNativeAppearance() {
            return None;
        }

        if aWM.IsCentralBaseline() {
            return Some(self.BSize(aWM) / 2);
        }

        // This is for compatibility with Chrome, Safari and Edge (Dec 2016).
        // Treat radio buttons and checkboxes as having an intrinsic baseline
        // at the block-end of the control (use the block-end content edge rather
        // than the margin edge).
        // For "inverted" lines (typically in writing-mode:vertical-lr), use the
        // block-start end instead.
        // See kCheckboxRadioBorderWidth in Theme.cpp
        let bp = kCheckboxRadioBorderWidth * AppUnitsPerCSSPixel();
        Some(if aWM.IsLineInverted() { std::cmp::min(bp, self.BSize(aWM)) } else { std::cmp::max(0, self.BSize(aWM) - bp) })
    }

    // Gecko: layout/forms/nsCheckboxRadioFrame.cpp nsCheckboxRadioFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        let wm = aReflowInput.GetWritingMode();
        MOZ_ASSERT!(aReflowInput.ComputedLogicalBorderPadding(wm).IsAllZero());

        let contentBoxSize = aReflowInput.ComputedSizeWithBSizeFallback(|| self.DefaultSize());
        aDesiredSize.SetSize(wm, &contentBoxSize);
        // font size inflation: NOT PORTED (it is off)

        // The children (placeholders of out-of-flow content): NOT PORTED, the frame has none.

        aDesiredSize.SetOverflowAreasToDesiredBounds();
        // FinishAndStoreOverflow(&aDesiredSize): NOT PORTED (overflow is not stored for this frame).
    }
}
