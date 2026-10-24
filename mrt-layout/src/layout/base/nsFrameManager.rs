/* storage of the frame tree and information about it */
// Translated from layout/base/nsFrameManager.h and nsFrameManager.cpp (GECKO_REV), REDUCED to the
// three functions that add frames to and remove them from a parent: AppendFrames, InsertFrames and
// RemoveFrame. nsCSSFrameConstructor derives from nsFrameManager in Gecko; here they are associated
// functions of a unit type, so nsPlaceholderFrame::Destroy can call RemoveFrame without the constructor.
//
// NOT PORTED: the root frame (nsCSSFrameConstructor::GetRootFrame keeps it), frame state capture and
// restoration (CaptureFrameState, RestoreFrameState: no layout history), the memory reporter.
use crate::*;

// Gecko: layout/base/nsFrameManager.h nsFrameManager
pub struct nsFrameManager;

impl nsFrameManager {
    // Gecko: layout/base/nsFrameManager.cpp nsFrameManager::AppendFrames
    pub fn AppendFrames(aParentFrame: nsIFrame, aListID: FrameChildListID, aFrameList: nsFrameList) {
        if aParentFrame.IsAbsoluteContainer() && aListID == FrameChildListID::Absolute {
            aParentFrame.GetAbsoluteContainingBlock().unwrap().AppendFrames(aParentFrame, aListID, aFrameList);
        } else {
            aParentFrame.AppendFrames(aListID, aFrameList);
        }
    }

    // Gecko: layout/base/nsFrameManager.cpp nsFrameManager::InsertFrames
    pub fn InsertFrames(aParentFrame: nsIFrame, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aFrameList: nsFrameList) {
        MOZ_ASSERT!(
            aPrevFrame.map_or(true, |p| p.GetNextContinuation().map_or(true, |n| n.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) && !p.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER))),
            "aPrevFrame must be the last continuation in its chain!"
        );

        if aParentFrame.IsAbsoluteContainer() && aListID == FrameChildListID::Absolute {
            aParentFrame.GetAbsoluteContainingBlock().unwrap().InsertFrames(aParentFrame, aListID, aPrevFrame, aFrameList);
        } else {
            aParentFrame.InsertFrames(aListID, aPrevFrame, None, aFrameList);
        }
    }

    // Gecko: layout/base/nsFrameManager.cpp nsFrameManager::RemoveFrame
    pub fn RemoveFrame(aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        // In case the reflow doesn't invalidate anything since it just leaves
        // a gap where the old frame was, we invalidate it here.  (This is
        // reasonably likely to happen when removing a last child in a way
        // that doesn't change the size of the parent.)
        // This has to sure to invalidate the entire overflow rect; this
        // is important in the presence of absolute positioning
        // aOldFrame->InvalidateFrameForRemoval(); NOT PORTED: painting.

        NS_ASSERTION!(
            aOldFrame.GetPrevContinuation().is_none() ||
                // exception for
                // nsCSSFrameConstructor::RemoveFloatingFirstLetterFrames
                aOldFrame.IsTextFrame(),
            "Must remove first continuation."
        );
        NS_ASSERTION!(!(aOldFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) && aOldFrame.GetPlaceholderFrame().is_some()), "Must call RemoveFrame on placeholder for out-of-flows.");
        let parentFrame = aOldFrame.GetParent().unwrap();
        if parentFrame.IsAbsoluteContainer() && aListID == FrameChildListID::Absolute {
            parentFrame.GetAbsoluteContainingBlock().unwrap().RemoveFrame(aContext, aListID, aOldFrame);
        } else {
            parentFrame.RemoveFrame(aContext, aListID, aOldFrame);
        }
    }
}
