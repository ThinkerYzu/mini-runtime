/* a presentation of a document, part of the document's view: the entry points that build
   the frame tree and reflow it */
// Translated from layout/base/PresShell.cpp (GECKO_REV), REDUCED to the reflow of the whole document,
// which is all a document that is laid out once needs. The type `PresShell` itself is in
// nsBlockFrame.rs (its FrameNeedsReflow); this file adds the methods, as Gecko's are in PresShell.cpp.
//
// NOT PORTED: everything else of the PresShell: incremental and interruptible reflow, the dirty
// roots and the reflow queue, the style flush (the caller resolves the styles with mrt-style
// first), selection, painting, scroll anchoring, accessibility, text perf metrics.
use crate::*;

impl PresShell {
    // Gecko: layout/base/PresShell.cpp PresShell::Initialize (the frame construction part): build
    // the viewport frame and the frames of the document element, under it.
    // ADAPTER: Gecko's PresShell::Initialize also sets up the visible area and the first reflow; the
    // frame constructor is passed in because the PresShell here is a unit type.
    pub fn Initialize(aFrameConstructor: &mut nsCSSFrameConstructor, aDocElement: ContentId) -> Option<nsIFrame> {
        let _rootFrame = aFrameConstructor.ConstructRootFrame();
        aFrameConstructor.ConstructDocElementFrame(aDocElement)
    }

    /**
     * Reflow the whole frame tree: the root frame (the viewport) with all the available inline-size
     * and an unconstrained block-size, the way a scroll frame reflows its scrolled frame.
     * Returns the size the root frame wants.
     */
    // Gecko: layout/base/PresShell.cpp PresShell::DoReflow (the isRoot case)
    pub fn DoReflow(aTarget: nsIFrame) -> ReflowOutput {
        // If the target frame is the root of the frame hierarchy, then
        // use all the available space.
        let wm = aTarget.GetWritingMode();
        let presContext = nsPresContext::Current();
        let size = LogicalSize::new__WritingMode_nsSize(wm, presContext.GetVisibleArea().Size());

        // Don't pass size directly to the reflow input, since a
        // constrained height implies page/column breaking.
        let reflowSize = LogicalSize::new(wm, size.ISize(wm), NS_UNCONSTRAINEDSIZE);
        let mut initFlags = ReflowInput_InitFlags::new();
        initFlags += ReflowInput_InitFlag::CallerWillInit;
        let mut reflowInput = ReflowInput::new__nsPresContext_nsIFrame_gfxContext_LogicalSize_InitFlags(&presContext, aTarget, gfxContext, reflowSize, initFlags);

        reflowInput.Init(&presContext, None, None, None);

        // fix the computed height
        NS_ASSERTION!(reflowInput.ComputedPhysicalMargin() == nsMargin::default(), "reflow input should not set margin for reflow roots");
        // (size.BSize(wm) is NS_UNCONSTRAINEDSIZE for the root: the visible area is replaced by the
        // size of the document, see below)
        NS_ASSERTION!(reflowInput.ComputedISize() == size.ISize(wm) - reflowInput.ComputedLogicalBorderPadding(wm).IStartEnd(wm), "reflow input computed incorrect inline size");

        let mut status = nsReflowStatus::new();
        let mut desiredSize = ReflowOutput::new__ReflowInput(&reflowInput);
        aTarget.Reflow(&presContext, &mut desiredSize, &reflowInput, &mut status);

        let boundsRelativeToTarget = nsRect::new(0, 0, desiredSize.Width(), desiredSize.Height());
        NS_ASSERTION!(status.IsEmpty(), "reflow roots should never split");

        aTarget.SetSize__nsSize(boundsRelativeToTarget.Size());
        aTarget.DidReflow(&presContext, None);
        desiredSize
    }
}
