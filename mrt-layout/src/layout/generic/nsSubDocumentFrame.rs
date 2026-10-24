/* rendering object for replaced elements that contain a document, such as <iframe> */
// Translated from layout/generic/nsSubDocumentFrame.h and nsSubDocumentFrame.cpp (GECKO_REV): the
// sizing (GetIntrinsicSize, GetIntrinsicRatio, ComputeSize, IntrinsicISize), Reflow without the
// docshell, and the subdocument's size (GetDestRect, GetSubdocumentSize).
//
// `nsSubDocumentFrame` here is a handle to a frame of type SubDocument (Deref to the frame handle). What
// is virtual in C++ is a `match` on the frame type, at the same places as nsVideoFrame's: ComputeSize,
// ComputeAutoSize and GetIntrinsicRatio (nsIFrame.rs), GetIntrinsicSize (nsLineLayoutAdapter.rs),
// IntrinsicISize (nsIFrame.rs), Reflow (nsLineLayoutAdapter.rs).
//
// ADAPTER: the frame's document is laid out by its own layout thread (mrt-js host.rs, DESIGN "The
// frames' layout (S5)"), not through a docshell: Reflow sizes the frame, and the driver reads
// GetSubdocumentSize after the reflow and gives it to the frame's document as its viewport (what
// nsSubDocumentFrame::Reflow's SetPositionAndSize and ReflowFinished's UpdatePositionAndSize do).
//
// NOT PORTED: <object> and <embed> (their SVG documents' intrinsic size and ratio), <frame> in a
// frameset, contain-intrinsic-size (FinishIntrinsicSize), object-fit and object-position (the dest
// rect is the content box, as for the initial `fill`), painting, the reflow callback, the frame loader.
use crate::*;
use std::ops::Deref;

// Gecko: layout/base/LayoutConstants.h kFallbackIntrinsicWidthInPixels, kFallbackIntrinsicHeightInPixels
const kFallbackIntrinsicWidthInPixels: i32 = 300;
const kFallbackIntrinsicHeightInPixels: i32 = 150;
// Gecko: layout/base/LayoutConstants.h kFallbackIntrinsicSize
const kFallbackIntrinsicWidth: nscoord = kFallbackIntrinsicWidthInPixels * AppUnitsPerCSSPixel();
const kFallbackIntrinsicHeight: nscoord = kFallbackIntrinsicHeightInPixels * AppUnitsPerCSSPixel();

// Gecko: layout/generic/nsSubDocumentFrame.h nsSubDocumentFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsSubDocumentFrame(pub nsIFrame);

impl Deref for nsSubDocumentFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/generic/nsSubDocumentFrame.cpp NS_NewSubDocumentFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (see NS_NewBlockFrame).
pub fn NS_NewSubDocumentFrame(aFrame: nsIFrame) -> nsSubDocumentFrame {
    aFrame.SetType(LayoutFrameType::SubDocument);
    nsSubDocumentFrame(aFrame)
}

impl nsSubDocumentFrame {
    // static_cast<nsSubDocumentFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsSubDocumentFrame {
        nsSubDocumentFrame(aFrame)
    }

    // Gecko: layout/generic/nsSubDocumentFrame.h nsSubDocumentFrame::IsInline
    // (mIsInline: the content is not a <frame> in a frameset)
    fn IsInline(&self) -> bool {
        self.GetContent().map_or(true, |c| !c.IsHTMLElement("frame"))
    }

    // Gecko: layout/generic/nsSubDocumentFrame.cpp nsSubDocumentFrame::GetIntrinsicSize
    pub fn GetIntrinsicSize(&self) -> IntrinsicSize {
        self.ComputeIntrinsicSize()
    }

    // Gecko: layout/generic/nsSubDocumentFrame.cpp nsSubDocumentFrame::ComputeIntrinsicSize
    fn ComputeIntrinsicSize(&self) -> IntrinsicSize {
        // containAxes (contain-intrinsic-size, FinishIntrinsicSize): NOT PORTED
        // nsIObjectLoadingContent's subdocument intrinsic size (<object>, <embed>): NOT PORTED

        if !self.IsInline() {
            return IntrinsicSize::default(); // <frame> elements have no useful intrinsic size.
        }

        // (XUL <iframe> and <browser>: none here)

        // We must be an HTML <iframe>. Return fallback size.
        IntrinsicSize { width: Some(kFallbackIntrinsicWidth), height: Some(kFallbackIntrinsicHeight) }
    }

    // Gecko: layout/generic/nsSubDocumentFrame.cpp nsSubDocumentFrame::GetIntrinsicRatio
    pub fn GetIntrinsicRatio(&self) -> AspectRatio {
        // 'contain:[inline-]size' replaced elements have no intrinsic ratio: NOT PORTED.
        // nsIObjectLoadingContent's subdocument intrinsic ratio: NOT PORTED.

        // NOTE(emilio): Even though we have an intrinsic size, we may not have an
        // intrinsic ratio. For example `<iframe style="width: 100px">` should not
        // shrink in the vertical axis to preserve the 300x150 ratio.
        // (nsAtomicContainerFrame::GetIntrinsicRatio is nsIFrame's: none)
        AspectRatio::default()
    }

    // Gecko: layout/generic/nsSubDocumentFrame.cpp nsSubDocumentFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        // (an SVG subdocument's percentage handling is not looked at, as in Gecko)
        self.GetIntrinsicSize().width.unwrap_or(0)
    }

    // Gecko: layout/generic/nsSubDocumentFrame.cpp nsSubDocumentFrame::ComputeSize
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
        nsIFrame_SizeComputationResult {
            mLogicalSize: self.ComputeSizeWithIntrinsicDimensions(aSizingInput.mRenderingContext, aWM, &self.GetIntrinsicSize(), self.GetAspectRatio(), aCBSize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
            mAspectRatioUsage: nsIFrame_AspectRatioUsage::None,
        }
    }

    // Gecko: layout/generic/nsSubDocumentFrame.cpp nsSubDocumentFrame::Reflow
    // ADAPTER: no docshell to size here (see the top of the file); GetSubdocumentSize gives the size it
    // would get.
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        // XUL <iframe> or <browser>, or HTML <iframe>, <object> or <embed>
        let wm = aReflowInput.GetWritingMode();
        aDesiredSize.SetSize(wm, &aReflowInput.ComputedSizeWithBorderPadding(wm));

        aDesiredSize.SetOverflowAreasToDesiredBounds();

        // FinishAndStoreOverflow, PostReflowCallback: NOT PORTED
    }

    // Gecko: layout/generic/nsSubDocumentFrame.cpp nsSubDocumentFrame::GetDestRect()
    // (a <frame> uses its border box, an <iframe> its content box; object-fit and object-position are
    // NOT PORTED, so the dest rect is that box, as for the initial `object-fit: fill`)
    pub fn GetDestRect(&self) -> nsRect {
        if !self.IsInline() {
            return nsRect::new(0, 0, self.GetSize().width, self.GetSize().height);
        }
        // GetContentRectRelativeToSelf
        let mut rect = self.GetPaddingRectRelativeToSelf();
        rect.Deflate__nsMargin(self.GetUsedPadding());
        rect
    }

    // Gecko: layout/generic/nsSubDocumentFrame.cpp nsSubDocumentFrame::Reflow's size of the docshell:
    // the dest rect in device pixels, rounded to inside (LayoutDeviceIntRect::FromAppUnitsToInside),
    // at one device pixel per CSS pixel.
    pub fn GetSubdocumentSize(&self) -> (i32, i32) {
        let destRect = self.GetDestRect();
        let perPixel = AppUnitsPerCSSPixel();
        let inside = |start: nscoord, size: nscoord| {
            let first = (start as f64 / perPixel as f64).ceil() as i32;
            let last = ((start + size) as f64 / perPixel as f64).floor() as i32;
            (last - first).max(0)
        };
        (inside(destRect.x, destRect.width), inside(destRect.y, destRect.height))
    }
}
