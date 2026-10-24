/* a frame that represents the canvas: it parents the root element's frame, and is the containing block of
   absolutely positioned boxes of the initial containing block */
// Translated from layout/generic/nsCanvasFrame.h and nsCanvasFrame.cpp (GECKO_REV), Reflow only.
//
// The canvas frame is a container frame of type Canvas; `nsCanvasFrame` is a handle to it.
// Reflow is reached through the `match` on the frame type in nsLineLayoutAdapter.rs.
//
// NOT PORTED: pagination (the previous canvas frame and its overflow frames, ReflowOverflowContainerChildren,
// the page-based block-size offset), the anonymous content (custom content container, caret, the
// accessible caret), the background painting, selection and event targets.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::Deref;

// Gecko: layout/generic/nsCanvasFrame.h nsCanvasFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsCanvasFrame(pub nsIFrame);

impl Deref for nsCanvasFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/generic/nsCanvasFrame.cpp NS_NewCanvasFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (see NS_NewBlockFrame).
pub fn NS_NewCanvasFrame(aFrame: nsIFrame) -> nsCanvasFrame {
    aFrame.SetType(LayoutFrameType::Canvas);
    nsCanvasFrame(aFrame)
}

impl nsCanvasFrame {
    // static_cast<nsCanvasFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsCanvasFrame {
        nsCanvasFrame(aFrame)
    }

    // Gecko: layout/generic/nsCanvasFrame.cpp nsCanvasFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        // auto* prevCanvasFrame = static_cast<nsCanvasFrame*>(GetPrevInFlow()); ...
        // NOT PORTED: pagination, there is never a previous canvas frame.
        let prevCanvasFrame: Option<nsCanvasFrame> = None;

        // Set our size up front, since some parts of reflow depend on it
        // being already set.  Note that the computed height may be
        // unconstrained; that's ok.  Consumers should watch out for that.
        self.SetSize__nsSize(aReflowInput.ComputedPhysicalSize());

        // Reflow our children.  Typically, we only have one child - the root
        // element's frame or a placeholder for that frame, if the root element
        // is abs-pos or fixed-pos.  Note that this child might be missing though
        // if that frame was Complete in one of our earlier continuations.  This
        // happens when we create additional pages purely to make room for painting
        // overflow (painted by BuildPreviousPageOverflow in nsPageFrame.cpp).
        // We may have additional children which are placeholders for continuations
        // of fixed-pos content, see nsCSSFrameConstructor::ReplicateFixedFrames.
        let wm = aReflowInput.GetWritingMode();
        aDesiredSize.SetSize(wm, &aReflowInput.ComputedSize__());
        if aReflowInput.ComputedBSize() == NS_UNCONSTRAINEDSIZE {
            // Set the block-size to zero for now in case we don't have any non-
            // placeholder children that would update the size in the loop below.
            *aDesiredSize.BSize_mut(wm) = 0;
        }
        aDesiredSize.SetOverflowAreasToDesiredBounds();
        let mFrames = nsContainerFrame::from_frame(self.0).mFrames();
        let mut kid = mFrames.FirstChild();
        while let Some(kidFrame) = kid {
            let nextKid = kidFrame.GetNextSibling();
            let mut kidDesiredSize = ReflowOutput::new__ReflowInput(aReflowInput);
            let kidDirty = kidFrame.HasAnyStateBits(NS_FRAME_IS_DIRTY);
            let kidWM = kidFrame.GetWritingMode();
            let mut availableSize = aReflowInput.AvailableSize__WritingMode(kidWM);
            let bOffset: nscoord = 0;
            // (the pagination block that adds up the block-sizes of the previous canvas frames and
            // reduces availableSize by bOffset: NOT PORTED)

            if availableSize.BSize(kidWM) > 0 {
                let mut kidReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                    aPresContext,
                    aReflowInput,
                    kidFrame,
                    availableSize,
                    None,
                    EnumSet::default(),
                    StyleSizeOverrides::default(),
                    ComputeSizeFlags::default(),
                    std::ptr::null_mut(),
                );

                if aReflowInput.IsBResizeForWM(kidReflowInput.GetWritingMode()) && kidFrame.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE) {
                    // Tell our kid it's being block-dir resized too.  Bit of a
                    // hack for framesets.
                    kidReflowInput.SetBResize(true);
                }

                let containerSize = aReflowInput.ComputedPhysicalSize();
                let margin = kidReflowInput.ComputedLogicalMargin(kidWM);
                let mut kidPt = LogicalPoint::new(kidWM, margin.IStart(kidWM), margin.BStart(kidWM));
                if kidWM.IsOrthogonalTo(&wm) {
                    *kidPt.I_mut(kidWM) += bOffset;
                } else {
                    *kidPt.B_mut(kidWM) += bOffset;
                }

                let mut kidStatus = nsReflowStatus::new();
                nsContainerFrame::from_frame(self.0).ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
                    kidFrame,
                    aPresContext,
                    &mut kidDesiredSize,
                    &kidReflowInput,
                    kidWM,
                    kidPt,
                    containerSize,
                    nsIFrame_ReflowChildFlags::Default,
                    &mut kidStatus,
                    None,
                );

                nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
                    kidFrame,
                    aPresContext,
                    &kidDesiredSize,
                    Some(&kidReflowInput),
                    kidWM,
                    kidPt,
                    containerSize,
                    nsIFrame_ReflowChildFlags::ApplyRelativePositioning,
                );

                if !kidStatus.IsFullyComplete() {
                    let mut nextFrame = kidFrame.GetNextInFlow();
                    NS_ASSERTION!(nextFrame.is_some() || kidStatus.NextInFlowNeedsReflow(), "If it's incomplete and has no nif yet, it must flag a nif reflow.");
                    if nextFrame.is_none() {
                        // NOT PORTED: pagination. The root element is never incomplete in a
                        // document laid out unconstrained in the block direction.
                        unimplemented!("NOT PORTED: pagination (nsCanvasFrame::SetOverflowFrames)");
                    }
                    if kidStatus.IsOverflowIncomplete() {
                        nextFrame.unwrap().AddStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER);
                    }
                }
                aStatus.MergeCompletionStatusFrom(&kidStatus);

                // If the child frame was just inserted, then we're responsible for making
                // sure it repaints
                // NOT PORTED: invalidation (painting), kidDirty is unused.

                // Return our desired size. Normally it's what we're told, but sometimes
                // we can be given an unconstrained block-size (when a window is
                // sizing-to-content), and we should compute our desired block-size. This
                // is done by PresShell::ResizeReflow, when given the BSizeLimit flag.
                //
                // We do this here rather than at the viewport frame, because the canvas
                // is what draws the background, so it can extend a little bit more than
                // the real content without visual glitches, realistically.
                if aReflowInput.ComputedBSize() == NS_UNCONSTRAINEDSIZE && !kidFrame.IsPlaceholderFrame() {
                    let mut finalSize = aReflowInput.ComputedSize__();
                    *finalSize.BSize_mut(wm) = nsPresContext::RoundUpAppUnitsToCSSPixel(kidFrame.GetLogicalSize__WritingMode(wm).BSize(wm) + kidReflowInput.ComputedLogicalMargin(wm).BStartEnd(wm));
                    aDesiredSize.SetSize(wm, &finalSize);
                    aDesiredSize.SetOverflowAreasToDesiredBounds();
                }
                aDesiredSize.mOverflowAreas.UnionWith(&(kidDesiredSize.mOverflowAreas + kidFrame.GetPosition()));
            } else if kidFrame.IsPlaceholderFrame() {
                // Placeholders always fit even if there's no available block-size left.
            } else {
                // This only occurs in paginated mode.  There is no available space on
                // this page due to reserving space for overflow from a previous page,
                // so we push our child to the next page.  Note that we can have some
                // placeholders for fixed pos. frames in mFrames too, so we need to be
                // careful to only push `kidFrame`.
                // NOT PORTED: pagination.
                unimplemented!("NOT PORTED: pagination (nsCanvasFrame::SetOverflowFrames)");
            }
            kid = nextKid;
        }

        // if (prevCanvasFrame) { ReflowOverflowContainerChildren(...) }  NOT PORTED: pagination.
        let _ = prevCanvasFrame;

        // FinishReflowWithAbsoluteFrames(aPresContext, aDesiredSize, aReflowInput, aStatus);
        // ADAPTER: in Gecko the root scroll frame gives the canvas the scrollport's block-size, so the
        // canvas, the initial containing block of absolutely positioned boxes, is as tall as the
        // window. Here the canvas is reflowed unconstrained and is as tall as the document, so its
        // absolutely positioned kids are reflowed against a containing block of the visible area's
        // block-size; the canvas keeps its own size.
        let canvasBSize = aDesiredSize.BSize(wm);
        if aReflowInput.ComputedBSize() == NS_UNCONSTRAINEDSIZE {
            *aDesiredSize.BSize_mut(wm) = LogicalSize::new__WritingMode_nsSize(wm, aPresContext.GetVisibleArea().Size()).BSize(wm);
        }
        nsContainerFrame::from_frame(self.0).ReflowAbsoluteFrames(aPresContext, aDesiredSize, aReflowInput, aStatus);
        *aDesiredSize.BSize_mut(wm) = canvasBSize;
        self.0.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(aDesiredSize, Some(&aReflowInput.mStyleDisplay));
    }
}

impl nsPresContext {
    // Gecko: layout/base/nsPresContext.h nsPresContext::RoundUpAppUnitsToCSSPixel
    pub fn RoundUpAppUnitsToCSSPixel(aAppUnits: nscoord) -> nscoord {
        // mozilla::RoundUpToMultiple(aAppUnits, mozilla::AppUnitsPerCSSPixel())
        let multiple = AppUnitsPerCSSPixel();
        (aAppUnits + multiple - 1).div_euclid(multiple) * multiple
    }
}
