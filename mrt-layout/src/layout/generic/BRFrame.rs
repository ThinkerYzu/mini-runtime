/* rendering object for HTML <br> elements */
// Translated from layout/generic/BRFrame.cpp (GECKO_REV). The class is private to that file in
// Gecko; `BRFrame` here is a handle to a frame of type Br (Deref to the frame handle).
//
// What is virtual in C++ is a `match` on the frame type: Reflow (nsLineLayoutAdapter.rs),
// AddInlineMinISize and AddInlinePrefISize (nsContainerFrame.rs).
//
// NOT PORTED: selection and caret (CalcContentOffsetsFromFramePoint, PeekOffset*), accessibility,
// GetNaturalBaselineBOffset (mAscent is not kept: no baseline export from a <br>), GetFrameName.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::Deref;

// Gecko: layout/generic/BRFrame.cpp mozilla::BRFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BRFrame(pub nsIFrame);

impl Deref for BRFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/generic/BRFrame.cpp NS_NewBRFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (see NS_NewBlockFrame).
pub fn NS_NewBRFrame(aFrame: nsIFrame) -> BRFrame {
    aFrame.SetType(LayoutFrameType::Br);
    BRFrame(aFrame)
}

impl BRFrame {
    // static_cast<BRFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> BRFrame {
        BRFrame(aFrame)
    }

    // Gecko: layout/generic/BRFrame.cpp BRFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aMetrics: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        let wm = aReflowInput.GetWritingMode();
        let mut finalSize = LogicalSize::new(wm, 0, 0);
        *finalSize.BSize_mut(wm) = 0; // BR frames with block size 0 are ignored in quirks
                                      // mode by nsLineLayout::VerticalAlignFrames .
                                      // However, it's not always 0.  See below.
        *finalSize.ISize_mut(wm) = 0;
        aMetrics.SetBlockStartAscent(0);

        // Only when the BR is operating in a line-layout situation will it
        // behave like a BR. Additionally, we suppress breaks from BR inside
        // of ruby frames. To determine if we're inside ruby, we have to rely
        // on the *parent's* ShouldSuppressLineBreak() method, instead of our
        // own, because we may have custom "display" value that makes our
        // ShouldSuppressLineBreak() return false.
        let ll = aReflowInput.mLineLayout;
        if !ll.is_null() && !self.GetParent().unwrap().Style().ShouldSuppressLineBreak() {
            let ll = unsafe { &mut *ll };
            // Note that the compatibility mode check excludes AlmostStandards
            // mode, since this is the inline box model.  See bug 161691.
            if ll.LineIsEmpty() || aPresContext.CompatibilityMode() == eCompatibility_FullStandards {
                // The line is logically empty; any whitespace is trimmed away.
                //
                // If this frame is going to terminate the line we know
                // that nothing else will go on the line. Therefore, in this
                // case, we provide some height for the BR frame so that it
                // creates some vertical whitespace.  It's necessary to use the
                // line-height rather than the font size because the
                // quirks-mode fix that doesn't apply the block's min
                // line-height makes this necessary to make BR cause a line
                // of the full line-height

                // We also do this in strict mode because BR should act like a
                // normal inline frame.  That line-height is used is important
                // here for cases where the line-height is less than 1.
                let fm = nsLayoutUtils::GetInflatedFontMetricsForFrame(self.GetParent().unwrap());
                // (the font metrics are never null here)
                let logicalHeight: nscoord = match aReflowInput.mParentReflowInput {
                    Some(parent) => parent.GetLineHeight(),
                    None => aReflowInput.GetLineHeight(),
                };
                *finalSize.BSize_mut(wm) = logicalHeight;
                aMetrics.SetBlockStartAscent(nsLayoutUtils::GetCenteredFontBaseline(&fm, logicalHeight, wm.IsLineInverted()));
            }

            // Return our reflow status
            aStatus.SetInlineLineBreakAfter(aReflowInput.mStyleDisplay.UsedClear(aReflowInput.GetCBWritingMode()));
            ll.SetLineEndsInBR(true);
        }

        aMetrics.SetSize(wm, &finalSize);
        aMetrics.SetOverflowAreasToDesiredBounds();

        // mAscent = aMetrics.BlockStartAscent(); NOT PORTED: baseline export.
    }

    // Gecko: layout/generic/BRFrame.cpp BRFrame::AddInlineMinISize
    pub fn AddInlineMinISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlineMinISizeData) {
        if !self.GetParent().unwrap().Style().ShouldSuppressLineBreak() {
            aData.ForceBreak();
        }
    }

    // Gecko: layout/generic/BRFrame.cpp BRFrame::AddInlinePrefISize
    pub fn AddInlinePrefISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlinePrefISizeData) {
        if !self.GetParent().unwrap().Style().ShouldSuppressLineBreak() {
            aData.ForceBreak(UsedClear::Both);
        }
    }
}
