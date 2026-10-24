/* rendering object for HTML <wbr> elements */
// Translated from layout/generic/WBRFrame.cpp (GECKO_REV). The class is private to that file in
// Gecko; `WBRFrame` here is a handle to a frame of type Wbr (Deref to the frame handle).
//
// The class has no Reflow of its own: nsIFrame's default is used (the frame reports its computed size,
// 0 by 0, see nsIFrame::Reflow in nsLineLayoutAdapter.rs). It is not a line participant, so the line
// layout treats it as an atomic inline-level box, with an optional break after it
// (nsLineLayout::ReflowFrame).
//
// NOT PORTED: PeekOffsetNoAmount, PeekOffsetCharacter, PeekOffsetWord (selection and caret movement).
use crate::*;
use std::ops::Deref;

// Gecko: layout/generic/WBRFrame.cpp mozilla::WBRFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WBRFrame(pub nsIFrame);

impl Deref for WBRFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/generic/WBRFrame.cpp NS_NewWBRFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (see NS_NewBlockFrame).
pub fn NS_NewWBRFrame(aFrame: nsIFrame) -> WBRFrame {
    aFrame.SetType(LayoutFrameType::Wbr);
    WBRFrame(aFrame)
}
