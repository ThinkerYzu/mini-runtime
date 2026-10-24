/*
 * rendering object that is the root of the frame tree, which contains
 * the document's scrollbars and contains fixed-positioned elements
 */
// Translated from layout/generic/ViewportFrame.h and ViewportFrame.cpp (GECKO_REV), Reflow only.
//
// The viewport frame is a container frame of type Viewport; `ViewportFrame` is a handle to it.
// Reflow is reached through the `match` on the frame type in nsLineLayoutAdapter.rs.
//
// NOT PORTED: the scroll container the viewport has in a window (the document is laid out
// unconstrained in the block direction; the fixed containing block is the visible area, see
// Reflow), the scrollbars, painting, the top layer, view transitions, IntrinsicISize,
// AppendDirectlyOwnedAnonBoxes, UpdateStyle.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::Deref;

// Gecko: layout/generic/ViewportFrame.h mozilla::ViewportFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewportFrame(pub nsIFrame);

impl Deref for ViewportFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/generic/ViewportFrame.cpp NS_NewViewportFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (see NS_NewBlockFrame).
pub fn NS_NewViewportFrame(aFrame: nsIFrame) -> ViewportFrame {
    aFrame.SetType(LayoutFrameType::Viewport);
    ViewportFrame(aFrame)
}

impl ViewportFrame {
    /**
     * Adjust aReflowInput to account for scrollbars and pres shell
     * GetVisualViewportSizeSet and
     * GetVisualViewportOffsetRelativeToLayoutViewport
     * @return the rect to use as containing block for fixed-pos children
     */
    // Gecko: layout/generic/ViewportFrame.cpp ViewportFrame::GetContainingBlockAdjustedForScrollbars
    // NOT PORTED: the scroll container (no scrollbars here, as in the Firefox compared with) and
    // AdjustViewportSizeForFixedPosition (the fixed viewport size of the PresShell is the visible
    // area here).
    pub fn GetContainingBlockAdjustedForScrollbars(&self, aReflowInput: &ReflowInput) -> nsRect {
        let wm = aReflowInput.GetWritingMode();

        let computedSize = aReflowInput.ComputedSize__();
        let origin = nsPoint::new(0, 0);

        nsRect::new__nsPoint_nsSize(origin, computedSize.GetPhysicalSize(wm))
    }

    // static_cast<ViewportFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> ViewportFrame {
        ViewportFrame(aFrame)
    }

    // Gecko: layout/generic/ViewportFrame.cpp ViewportFrame::Init
    pub fn Init(&self, aContent: Option<nsIContent>, aParent: Option<nsIFrame>, aPrevInFlow: Option<nsIFrame>) {
        // nsContainerFrame::Init(aContent, aParent, aPrevInFlow);
        self.0.SetContent(aContent);
        if let Some(p) = aParent {
            self.0.SetParent(p);
        }
        // No need to call CreateView() here - the frame ctor will call SetView()
        // with the ViewManager's root view, so we'll assign it in SetViewInternal().

        // nsIFrame* parent = nsLayoutUtils::GetCrossDocParentFrameInProcess(this); ...
        // NOT PORTED: NS_FRAME_IN_POPUP (there are no popups and no parent document).
    }

    // Gecko: layout/generic/ViewportFrame.cpp ViewportFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        // Because |Reflow| sets ComputedBSize() on the child to our
        // ComputedBSize().
        self.AddStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE);

        // Set our size up front, since some parts of reflow depend on it
        // being already set.  Note that the computed height may be
        // unconstrained; that's ok.  Consumers should watch out for that.
        self.SetSize__nsSize(aReflowInput.ComputedPhysicalSize());

        // Reflow the main content first so that the placeholders of the
        // fixed-position frames will be in the right places on an initial
        // reflow.
        let mut kidBSize: nscoord = 0;
        let wm = aReflowInput.GetWritingMode();

        let mFrames = nsContainerFrame::from_frame(self.0).mFrames();
        if mFrames.NotEmpty() {
            // Deal with a non-incremental reflow or an incremental reflow
            // targeted at our one-and-only principal child frame.
            // (IsSubtreeDirty: NOT PORTED, every frame is dirty on the first reflow)
            if aReflowInput.ShouldReflowAllKids() || mFrames.FirstChild().unwrap().HasAnyStateBits(NS_FRAME_IS_DIRTY | NS_FRAME_HAS_DIRTY_CHILDREN) {
                // Reflow our one-and-only principal child frame
                let kidFrame = mFrames.FirstChild().unwrap();
                let mut kidDesiredSize = ReflowOutput::new__ReflowInput(aReflowInput);
                let kidWM = kidFrame.GetWritingMode();
                let availableSpace = aReflowInput.AvailableSize__WritingMode(kidWM);
                let mut kidReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                    aPresContext,
                    aReflowInput,
                    kidFrame,
                    availableSpace,
                    None,
                    EnumSet::default(),
                    StyleSizeOverrides::default(),
                    ComputeSizeFlags::default(),
                    std::ptr::null_mut(),
                );

                // Reflow the frame
                kidReflowInput.SetComputedBSize(aReflowInput.ComputedBSize(), ReflowInput_ResetResizeFlags::Yes);
                if aReflowInput.IsBResizeForWM(kidWM) {
                    kidReflowInput.SetBResize(true);
                }
                if aReflowInput.IsBResizeForPercentagesForWM(kidWM) {
                    kidReflowInput.SetBResizeForPercentages(true);
                }
                nsContainerFrame::from_frame(self.0).ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_nscoord_nscoord_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
                    kidFrame,
                    aPresContext,
                    &mut kidDesiredSize,
                    &kidReflowInput,
                    0,
                    0,
                    nsIFrame_ReflowChildFlags::Default,
                    aStatus,
                    None,
                );
                kidBSize = kidDesiredSize.BSize(wm);

                nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_nscoord_nscoord_ReflowChildFlags(
                    kidFrame,
                    aPresContext,
                    &kidDesiredSize,
                    Some(&kidReflowInput),
                    0,
                    0,
                    nsIFrame_ReflowChildFlags::Default,
                );
            } else {
                kidBSize = LogicalSize::new__WritingMode_nsSize(wm, mFrames.FirstChild().unwrap().GetSize()).BSize(wm);
            }
        }

        NS_ASSERTION!(aReflowInput.AvailableISize() != NS_UNCONSTRAINEDSIZE, "shouldn't happen anymore");

        // Return the max size as our desired size
        let maxSize = LogicalSize::new(
            wm,
            aReflowInput.AvailableISize(),
            // Being flowed initially at an unconstrained block size
            // means we should return our child's intrinsic size.
            if aReflowInput.ComputedBSize() != NS_UNCONSTRAINEDSIZE { aReflowInput.ComputedBSize() } else { kidBSize },
        );
        aDesiredSize.SetSize(wm, &maxSize);
        aDesiredSize.SetOverflowAreasToDesiredBounds();

        if self.HasAbsolutelyPositionedChildren() {
            // Make a copy of the reflow input and change the computed block size to
            // reflect the available space for the fixed items
            let mut reflowInput = aReflowInput.clone();

            if reflowInput.AvailableBSize() == NS_UNCONSTRAINEDSIZE {
                // We have an intrinsic-block-size document with abs-pos/fixed-pos
                // children. Set the available block-size and computed block-size to our
                // chosen block-size.
                // ADAPTER: Gecko's root reflow has the visible area's block-size (PresShell::DoReflow
                // fixes the computed height), so maxSize is the window's; the port reflows the
                // document unconstrained to measure it, so maxSize is the document's. The fixed
                // containing block is the window: the visible area's block-size.
                let visibleBSize = LogicalSize::new__WritingMode_nsSize(wm, aPresContext.GetVisibleArea().Size()).BSize(wm);
                reflowInput.SetAvailableBSize(visibleBSize);
                // Not having border/padding simplifies things
                NS_ASSERTION!(reflowInput.ComputedPhysicalBorderPadding() == nsMargin::default(), "Viewports can't have border/padding");
                reflowInput.SetComputedBSize(visibleBSize, ReflowInput_ResetResizeFlags::Yes);
            }

            // The containing block for children. We intentionally not take scrollbar
            // size and dynamic toolbar into account because
            // ::-moz-snapshot-containing-block should include those areas.
            //
            // We will take them into account in AbsoluteContainingBlock::Reflow(),
            // for kid frames other than ::-moz-snapshot-containing-block.
            let cb = nsRect::new__nsPoint_nsSize(nsPoint::default(), reflowInput.ComputedPhysicalSize());
            // XXX: To optimize the performance, set the flags only when the CB width or
            // height actually changes.
            let mut flags = AbsPosReflowFlags::default();
            flags += AbsPosReflowFlag::CBWidthChanged;
            flags += AbsPosReflowFlag::CBHeightChanged;
            let mut absposStatus = nsReflowStatus::default();
            self.GetAbsoluteContainingBlock().unwrap().Reflow(self.0, aPresContext, &reflowInput, &mut absposStatus, cb, flags, /* aOverflowAreas = */ None);
            aStatus.MergeCompletionStatusFrom(&absposStatus);
        }

        if mFrames.NotEmpty() {
            nsContainerFrame::from_frame(self.0).ConsiderChildOverflow(&mut aDesiredSize.mOverflowAreas, mFrames.FirstChild().unwrap(), OverflowAreaUnionFlags::None);
        }

        // If we were dirty then do a repaint
        // NOT PORTED: InvalidateFrame (painting).

        // Clipping is handled by the document container (e.g., nsSubDocumentFrame),
        // so we don't need to change our overflow areas.
        self.0.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(aDesiredSize, None);
    }
}
