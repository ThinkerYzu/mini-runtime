/* rendering object for the HTML <video> and <audio> elements */
// Translated from layout/generic/nsVideoFrame.h and nsVideoFrame.cpp (GECKO_REV): the sizing
// (GetIntrinsicSize, GetIntrinsicRatio, ComputeSize, IntrinsicISize) and a reduced Reflow.
//
// nsVideoFrame is the frame class of both elements (nsAudioFrame is the same class with a different
// element); `nsVideoFrame` here is a handle to a frame of type HTMLVideo (Deref to the frame handle).
// What is virtual in C++ is a `match` on the frame type: ComputeSize, ComputeAutoSize and
// GetIntrinsicRatio (nsIFrame.rs), GetIntrinsicSize (nsLineLayoutAdapter.rs), IntrinsicISize
// (nsIFrame.rs), Reflow (nsLineLayoutAdapter.rs).
//
// ADAPTER: the mini runtime loads no media and no images, and builds no UA widget (the controls, the
// poster image, the caption), so:
//   - a <video> has no video size and no poster to show (the harness blocks images), which leaves the
//     fallback intrinsic size of 300 by 150 CSS px (kFallbackIntrinsicSize) and its 2:1 ratio, unless
//     the `aspect-ratio` style has a ratio;
//   - an <audio> has its controls (the UA sheet gives `audio:not([controls])` display:none, so every
//     audio that gets a frame has them) and its block size, when it is auto, is the height of the control
//     bar, which Gecko gets by reflowing the controls child: here kAudioControlsBSize, the 40px that
//     Firefox measures for it (bench/firefox_boxes: audio.mw-file-element 250 by 40, forms.html 300 by 40).
//     Its inline size is the intrinsic one, 300px (the fallback width), or the CSS width.
//
// NOT PORTED: the children (controls, caption, poster), the video size and `ShouldDisplayPoster`,
// contain-intrinsic-size (FinishIntrinsicSize), the object dest rect and fit (GetDestRect), painting,
// the reflow callback, accessibility.
use crate::*;
use std::ops::Deref;

// Gecko: layout/base/LayoutConstants.h kFallbackIntrinsicWidthInPixels, kFallbackIntrinsicHeightInPixels
const kFallbackIntrinsicWidthInPixels: i32 = 300;
const kFallbackIntrinsicHeightInPixels: i32 = 150;
// Gecko: layout/base/LayoutConstants.h kFallbackIntrinsicSize
const kFallbackIntrinsicWidth: nscoord = kFallbackIntrinsicWidthInPixels * AppUnitsPerCSSPixel();
const kFallbackIntrinsicHeight: nscoord = kFallbackIntrinsicHeightInPixels * AppUnitsPerCSSPixel();

// ADAPTER: the height of the audio control bar that the UA widget (toolkit/content/widgets/videocontrols)
// gives an <audio controls>, as Firefox measures it.
const kAudioControlsBSize: nscoord = 40 * AppUnitsPerCSSPixel();

// (layout/style/nsStyleStruct.h StyleAspectRatio::HasRatio; the drift tool cannot locate it at the base revision, so no marker)
fn HasRatio(aAspectRatio: &StyleAspectRatio) -> bool {
    matches!(aAspectRatio.ratio, mrt_style::style::values::generics::position::PreferredRatio::Ratio(_))
}

// Gecko: layout/generic/nsVideoFrame.h nsVideoFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsVideoFrame(pub nsIFrame);

impl Deref for nsVideoFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/generic/nsVideoFrame.cpp NS_NewHTMLVideoFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (see NS_NewBlockFrame).
pub fn NS_NewHTMLVideoFrame(aFrame: nsIFrame) -> nsVideoFrame {
    aFrame.SetType(LayoutFrameType::HTMLVideo);
    nsVideoFrame(aFrame)
}

impl nsVideoFrame {
    // static_cast<nsVideoFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsVideoFrame {
        nsVideoFrame(aFrame)
    }

    // Gecko: layout/generic/nsVideoFrame.h nsVideoFrame::HasVideoElement
    // ADAPTER: the frame constructor says which element the frame is for (FormControlData.mIsTextArea is
    // not used by video; the element name is the answer).
    pub fn HasVideoElement(&self) -> bool {
        self.GetContent().map_or(false, |c| c.IsHTMLElement("video"))
    }

    // Gecko: layout/generic/nsVideoFrame.cpp nsVideoFrame::ComputeSize
    pub fn ComputeSize(
        &self,
        aSizingInput: &SizeComputationInput,
        aWM: WritingMode,
        aCBSize: LogicalSize,
        aAvailableISize: nscoord,
        aMargin: LogicalSize,
        aBorderPadding: LogicalSize,
        aSizeOverrides: &StyleSizeOverrides,
        aFlags: ComputeSizeFlags,
    ) -> nsIFrame_SizeComputationResult {
        if !self.HasVideoElement() {
            // nsContainerFrame::ComputeSize is nsIFrame::ComputeSize
            return self.nsIFrame_ComputeSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags);
        }

        nsIFrame_SizeComputationResult {
            mLogicalSize: self.ComputeSizeWithIntrinsicDimensions(aSizingInput.mRenderingContext, aWM, &self.GetIntrinsicSize(), self.GetAspectRatio(), aCBSize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            mAspectRatioUsage: nsIFrame_AspectRatioUsage::None,
        }
    }

    // Gecko: layout/generic/nsVideoFrame.cpp nsVideoFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        // <audio> / <video> has the same min / pref ISize.
        self.GetIntrinsicSize().width.unwrap_or(0)
    }

    // Gecko: layout/generic/nsVideoFrame.cpp nsVideoFrame::GetIntrinsicRatio(bool)
    pub fn GetIntrinsicRatio(&self) -> AspectRatio {
        if !self.HasVideoElement() {
            // Audio elements have no intrinsic ratio.
            return AspectRatio::default();
        }

        // 'contain:[inline-]size' replaced elements have no intrinsic ratio: NOT PORTED.

        // element->GetVideoSize(), ShouldDisplayPoster(): NOT PORTED (no media, no images).

        if HasRatio(&self.StylePosition().mAspectRatio()) {
            return AspectRatio::default();
        }

        AspectRatio::FromSize__float_float_UseBoxSizing(kFallbackIntrinsicWidthInPixels as f32, kFallbackIntrinsicHeightInPixels as f32, UseBoxSizing::No)
    }

    // Gecko: layout/generic/nsVideoFrame.cpp nsVideoFrame::GetIntrinsicSize(bool)
    pub fn GetIntrinsicSize(&self) -> IntrinsicSize {
        // containAxes (contain-intrinsic-size, FinishIntrinsicSize): NOT PORTED
        if !self.HasVideoElement() {
            // An audio element with no "controls" attribute, distinguished by the last
            // and only child being the control, falls back to no intrinsic size.
            // (ADAPTER: that one is display:none and has no frame; this one has its controls.)
            return IntrinsicSize { width: Some(kFallbackIntrinsicWidth), height: Some(kFallbackIntrinsicHeight) };
        }

        // element->GetVideoSize(), ShouldDisplayPoster(): NOT PORTED (no media, no images).

        if HasRatio(&self.StylePosition().mAspectRatio()) {
            return IntrinsicSize::default();
        }

        IntrinsicSize { width: Some(kFallbackIntrinsicWidth), height: Some(kFallbackIntrinsicHeight) }
    }

    // Gecko: layout/generic/nsVideoFrame.cpp nsVideoFrame::Reflow
    // ADAPTER: no children are reflowed (see the top of the file); the block size of an <audio> that
    // shrink-wraps in the block axis is the control bar's.
    pub fn Reflow(&self, aPresContext: &nsPresContext, aMetrics: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        let myWM = aReflowInput.GetWritingMode();
        let mut contentBoxBSize = aReflowInput.ComputedBSize();
        let logicalBP = aReflowInput.ComputedLogicalBorderPadding(myWM);
        let borderBoxISize = aReflowInput.ComputedISize() + logicalBP.IStartEnd(myWM);
        let isBSizeShrinkWrapping = contentBoxBSize == NS_UNCONSTRAINEDSIZE;

        let mut borderBoxBSize: nscoord = 0;
        if !isBSizeShrinkWrapping {
            borderBoxBSize = contentBoxBSize + logicalBP.BStartEnd(myWM);
        }

        // Reflow the child frames: the controls give an <audio> its block size.
        if isBSizeShrinkWrapping && !self.HasVideoElement() {
            // Resolve our own BSize based on the controls' size in the same axis. Unless we're
            // size-contained, in which case we have to behave as if we have an intrinsic size of 0.
            if self.GetContainSizeAxes().mBContained {
                contentBoxBSize = 0;
            } else {
                contentBoxBSize = kAudioControlsBSize;
            }
        }

        if isBSizeShrinkWrapping {
            if contentBoxBSize == NS_UNCONSTRAINEDSIZE {
                // We didn't get a BSize from our intrinsic size/ratio, nor did we
                // get one from our controls. Just use BSize of 0.
                contentBoxBSize = 0;
            }
            contentBoxBSize = aReflowInput.ApplyMinMaxBSize(contentBoxBSize, 0);
            borderBoxBSize = contentBoxBSize + logicalBP.BStartEnd(myWM);
        }

        let logicalDesiredSize = LogicalSize::new(myWM, borderBoxISize, borderBoxBSize);
        aMetrics.SetSize(myWM, &logicalDesiredSize);

        aMetrics.SetOverflowAreasToDesiredBounds();
        // GetDestRect, FinishAndStoreOverflow: NOT PORTED
    }
}
