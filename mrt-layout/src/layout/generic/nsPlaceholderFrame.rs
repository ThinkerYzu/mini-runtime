/*
 * rendering object for the point that anchors out-of-flow rendering
 * objects such as floats and absolutely positioned elements
 */
// Translated from layout/generic/nsPlaceholderFrame.h and nsPlaceholderFrame.cpp (GECKO_REV).
//
// A placeholder is a leaf frame of type Placeholder; `nsPlaceholderFrame` is a handle to it that
// Derefs to the frame handle, like every class here (CONVENTIONS.md). The link to the
// out-of-flow frame (`mOutOfFlowFrame`) is kept by nsLineLayoutAdapter.rs (SetPlaceholderFor,
// both directions; OutOfFlowFor and nsIFrame::GetPlaceholderFrame read it).
//
// What is virtual in C++ is a `match` on the frame type in the adapter: Reflow
// (nsLineLayoutAdapter.rs), AddInlineMinISize / AddInlinePrefISize (nsContainerFrame.rs, bodies
// in nsLineLayoutAdapter.rs), IsEmpty and IsSelfEmpty (nsIFrameLists.rs, nsLineLayoutAdapter.rs).
//
// NOT PORTED: BuildDisplayList, List and GetFrameName,
// accessibility, RenumberFrameAndDescendants, the style-parent queries for out-of-flows
// (GetParentComputedStyleForOutOfFlow, GetLayoutParentStyleForOutOfFlow), PLACEHOLDER_FOR_TOPLAYER
// (there is no top layer).
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::Deref;

// Gecko: layout/generic/nsPlaceholderFrame.h nsPlaceholderFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsPlaceholderFrame(pub nsIFrame);

impl Deref for nsPlaceholderFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/generic/nsPlaceholderFrame.h PLACEHOLDER_TYPE_MASK
pub const PLACEHOLDER_TYPE_MASK: nsFrameState = PLACEHOLDER_FOR_FLOAT | PLACEHOLDER_FOR_ABSPOS | PLACEHOLDER_FOR_FIXEDPOS | PLACEHOLDER_FOR_TOPLAYER;

/**
 * Create a new placeholder frame.  aTypeBit must be one of the
 * PLACEHOLDER_FOR_* constants above.
 */
// Gecko: layout/generic/nsPlaceholderFrame.cpp NS_NewPlaceholderFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (see NS_NewBlockFrame).
pub fn NS_NewPlaceholderFrame(aFrame: nsIFrame, aTypeBits: nsFrameState) -> nsPlaceholderFrame {
    aFrame.SetType(LayoutFrameType::Placeholder);
    // Gecko: nsPlaceholderFrame::nsPlaceholderFrame
    MOZ_ASSERT!(
        aTypeBits == PLACEHOLDER_FOR_FLOAT
            || aTypeBits == PLACEHOLDER_FOR_ABSPOS
            || aTypeBits == PLACEHOLDER_FOR_FIXEDPOS
            || aTypeBits == (PLACEHOLDER_FOR_TOPLAYER | PLACEHOLDER_FOR_ABSPOS)
            || aTypeBits == (PLACEHOLDER_FOR_TOPLAYER | PLACEHOLDER_FOR_FIXEDPOS),
        "Unexpected type bit"
    );
    aFrame.AddStateBits(aTypeBits);
    nsPlaceholderFrame(aFrame)
}

// Gecko: layout/generic/nsPlaceholderFrame.cpp ChildListIDForOutOfFlow
fn ChildListIDForOutOfFlow(aPlaceholderState: nsFrameState, aChild: nsIFrame) -> FrameChildListID {
    let _ = aChild;
    if (aPlaceholderState & PLACEHOLDER_FOR_FLOAT) != 0 {
        return FrameChildListID::Float;
    }
    MOZ_ASSERT!((aPlaceholderState & (PLACEHOLDER_FOR_FIXEDPOS | PLACEHOLDER_FOR_ABSPOS)) != 0);
    FrameChildListID::Absolute
}

impl nsPlaceholderFrame {
    // static_cast<nsPlaceholderFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsPlaceholderFrame {
        nsPlaceholderFrame(aFrame)
    }

    // Get/Set the associated out of flow frame
    // Gecko: layout/generic/nsPlaceholderFrame.h nsPlaceholderFrame::GetOutOfFlowFrame
    pub fn GetOutOfFlowFrame(&self) -> Option<nsIFrame> {
        nsLineLayoutAdapter::OutOfFlowFor(self.0)
    }

    // Gecko: layout/generic/nsPlaceholderFrame.h nsPlaceholderFrame::SetOutOfFlowFrame
    // (ADAPTER: only the non-null case; a placeholder never gives its frame up)
    pub fn SetOutOfFlowFrame(&self, aFrame: nsIFrame) {
        NS_ASSERTION!(aFrame.GetPrevContinuation().is_none(), "OOF must be first continuation");
        self.0.SetPlaceholderFor(aFrame);
    }

    // Gecko: layout/generic/nsPlaceholderFrame.cpp nsPlaceholderFrame::Destroy
    // (the virtual call is a `match` on the frame type in nsContainerFrame.rs's Destroy)
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        if let Some(oof) = self.GetOutOfFlowFrame() {
            // mOutOfFlowFrame = nullptr;
            // oof->RemoveProperty(nsIFrame::PlaceholderFrameProperty());
            // (both links are kept by nsLineLayoutAdapter.rs)
            nsLineLayoutAdapter::ClearPlaceholderFor(self.0);

            // Destroy the out of flow now.
            let listId = ChildListIDForOutOfFlow(self.GetStateBits(), oof);
            nsFrameManager::RemoveFrame(aContext, listId, oof);
        }

        self.0.nsIFrame_Destroy(aContext);
    }

    // Gecko: layout/generic/nsPlaceholderFrame.cpp nsPlaceholderFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        // #ifdef DEBUG: the check that the out-of-flow frame is not reflowed before its placeholder.
        // NOT PORTED: debug-only check.

        self.MarkInReflow();
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");
        aDesiredSize.ClearSize();
    }

    // Gecko: layout/generic/nsPlaceholderFrame.h nsPlaceholderFrame::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        true
    }

    // Gecko: layout/generic/nsPlaceholderFrame.h nsPlaceholderFrame::IsSelfEmpty
    pub fn IsSelfEmpty(&self) -> bool {
        true
    }

    // Gecko: layout/generic/nsPlaceholderFrame.cpp nsPlaceholderFrame::CanContinueTextRun
    pub fn CanContinueTextRun(&self) -> bool {
        match self.GetOutOfFlowFrame() {
            None => false,
            Some(oof) => oof.CanContinueTextRun(),
        }
    }

    // Gecko: layout/generic/nsPlaceholderFrame.h nsPlaceholderFrame::SetLineIsEmptySoFar
    pub fn SetLineIsEmptySoFar(&self, aValue: bool) {
        self.AddOrRemoveStateBits(PLACEHOLDER_LINE_IS_EMPTY_SO_FAR, aValue);
        self.AddStateBits(PLACEHOLDER_HAVE_LINE_IS_EMPTY_SO_FAR);
    }

    // Gecko: layout/generic/nsPlaceholderFrame.h nsPlaceholderFrame::GetLineIsEmptySoFar
    pub fn GetLineIsEmptySoFar(&self, aResult: &mut bool) -> bool {
        let haveValue = self.HasAnyStateBits(PLACEHOLDER_HAVE_LINE_IS_EMPTY_SO_FAR);
        if haveValue {
            *aResult = self.HasAnyStateBits(PLACEHOLDER_LINE_IS_EMPTY_SO_FAR);
        }
        haveValue
    }

    // Gecko: layout/generic/nsPlaceholderFrame.h nsPlaceholderFrame::ForgetLineIsEmptySoFar
    pub fn ForgetLineIsEmptySoFar(&self) {
        self.RemoveStateBits(PLACEHOLDER_HAVE_LINE_IS_EMPTY_SO_FAR);
    }

    /**
     * @return the out-of-flow for aFrame if aFrame is a placeholder; otherwise
     * aFrame
     */
    // Gecko: layout/generic/nsPlaceholderFrame.h nsPlaceholderFrame::GetRealFrameFor
    pub fn GetRealFrameFor(aFrame: nsIFrame) -> nsIFrame {
        if aFrame.IsPlaceholderFrame() {
            return nsPlaceholderFrame::GetRealFrameForPlaceholder(aFrame);
        }
        aFrame
    }

    /**
     * @return the out-of-flow for aFrame, which is known to be a placeholder
     */
    // Gecko: layout/generic/nsPlaceholderFrame.h nsPlaceholderFrame::GetRealFrameForPlaceholder
    pub fn GetRealFrameForPlaceholder(aFrame: nsIFrame) -> nsIFrame {
        MOZ_ASSERT!(aFrame.IsPlaceholderFrame(), "Must have placeholder frame as input");
        let outOfFlow = nsPlaceholderFrame(aFrame).GetOutOfFlowFrame();
        NS_ASSERTION!(outOfFlow.is_some(), "Null out-of-flow for placeholder?");
        outOfFlow.unwrap()
    }
}
