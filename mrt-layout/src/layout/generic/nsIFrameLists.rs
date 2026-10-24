// ADAPTER (not a translation of a whole file): the parts of layout/generic/nsIFrame.h that
// nsFrameList.rs and nsLineBox.rs use and that nsIFrame.rs does not have yet: the previous-sibling
// link, sibling and parent setters, frame destruction, and the emptiness queries of line boxes.
// See CONVENTIONS.md, "Frames and styles". Move a method into nsIFrame.rs if you prefer; the
// names are Gecko's.
use crate::*;

// Gecko: layout/base/FrameDestroyContext.h mozilla::FrameDestroyContext (reduced).
// ADAPTER: Gecko's context holds the PresShell and the list of frames whose destruction is
// deferred; here it only remembers which frames were destroyed, in order, so tests (and
// callers) can see them.
#[derive(Default, Debug)]
pub struct FrameDestroyContext {
    pub mDestroyedFrames: Vec<nsIFrame>,
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::DestroyContext
pub type nsIFrame_DestroyContext = FrameDestroyContext;

// ADAPTER: Gecko's nsIFrame::IsEmpty() and CachedIsEmpty() are virtual (nsBlockFrame, nsInlineFrame,
// nsTextFrame, ... override them; nsIFrame's answer is "not empty"). The overrides are not
// translated yet, so a frame answers through this property when it has it: whoever builds the
// frame (the frame constructor, a test) sets it to what the override would return.
const kEmptyFramePropertyId: u32 = 900;

impl nsIFrame {
    // ADAPTER: the descriptor of the stand-in property described above.
    pub fn EmptyFrameProperty() -> FramePropertyDescriptor<bool> {
        FramePropertyDescriptor::new(kEmptyFramePropertyId)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetPrevSibling
    pub fn GetPrevSibling(&self) -> Option<nsIFrame> {
        WithFrameTree(|t| t.mFrames[self.0 as usize].mPrevSibling)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::SetNextSibling
    // (the only function that touches mPrevSibling, as in Gecko)
    pub fn SetNextSibling(&self, aNextSibling: Option<nsIFrame>) {
        NS_ASSERTION!(Some(*self) != aNextSibling, "Creating a circular frame list, this is very bad.");
        WithFrameTree(|t| {
            let mNextSibling = t.mFrames[self.0 as usize].mNextSibling;
            if let Some(next) = mNextSibling {
                if t.mFrames[next.0 as usize].mPrevSibling == Some(*self) {
                    t.mFrames[next.0 as usize].mPrevSibling = None;
                }
            }
            t.mFrames[self.0 as usize].mNextSibling = aNextSibling;
            if let Some(next) = aNextSibling {
                t.mFrames[next.0 as usize].mPrevSibling = Some(*self);
            }
        })
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::SetParent
    // ADAPTER: only the parent pointer. Gecko's version also maintains the frame state bits that
    // depend on the parent (NS_FRAME_IN_POPUP, the has-child-with-view bit, ...) and reparents
    // the view; none of that exists here.
    pub fn SetParent(&self, aParent: nsIFrame) {
        WithFrameTree(|t| t.mFrames[self.0 as usize].mParent = Some(aParent))
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::Destroy
    // ADAPTER: frames live in the arena of the frame tree for the life of the page, so
    // "destroying" one only detaches it from its parent and records it in the context.
    // This is the base class's Destroy; the virtual call `Destroy` is in nsContainerFrame.rs (a
    // `match` on the frame type: container frames destroy their child lists first).
    pub fn nsIFrame_Destroy(&self, aContext: &mut FrameDestroyContext) {
        if self.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) {
            if let Some(placeholder) = self.GetPlaceholderFrame() {
                // placeholder->SetOutOfFlowFrame(nullptr);
                crate::nsLineLayoutAdapter::ClearPlaceholderFor(placeholder);
            }
        }

        // Gecko (near the end): RemoveAllProperties(), which runs the destructors of deletable values
        self.RemoveAllProperties();
        // Gecko (earlier in the function): if (mContent && mContent->GetPrimaryFrame() == this) {
        // mContent->SetPrimaryFrame(nullptr); }
        if let Some(content) = self.GetContent() {
            if content.GetPrimaryFrame() == Some(*self) {
                content.SetPrimaryFrame(None);
            }
        }
        WithFrameTree(|t| {
            t.mFrames[self.0 as usize].mParent = None;
            // (presShell->FreeFrame(id, this): the slot is freed once the change is done, see
            // FrameTree::mDestroyedFrames)
            t.NoteDestroyed(*self);
        });
        aContext.mDestroyedFrames.push(*self);
    }

    // Gecko: layout/generic/nsIFrameInlines.h nsIFrame::IsBlockOutside
    // (calls nsStyleDisplay::IsBlockOutside(this), which is below)
    pub fn IsBlockOutside(&self) -> bool {
        self.StyleDisplay().IsBlockOutside(*self)
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::IsEmpty
    // ADAPTER: see kEmptyFramePropertyId.
    // (When nsBlockFrame.rs lands, a Block frame answers with nsBlockFrame::IsEmpty here, a `match`
    // on the frame type; test frames are all of type Block, so that cannot be done yet.)
    pub fn IsEmpty(&self) -> bool {
        if let Some(isEmpty) = self.GetProperty(nsIFrame::EmptyFrameProperty()) {
            return isEmpty;
        }
        match self.Type() {
            // Inline, Line => nsInlineFrame::IsEmpty
            LayoutFrameType::Inline | LayoutFrameType::Line => nsInlineFrame::from_frame(*self).IsEmpty(),
            // Block => nsBlockFrame::IsEmpty
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).IsEmpty(),
            // Text => nsTextFrame::IsEmpty
            LayoutFrameType::Text if self.UsesRealTextFrame() => nsTextFrame::from_frame(*self).IsEmpty(),
            // Placeholder => nsPlaceholderFrame::IsEmpty
            LayoutFrameType::Placeholder => nsPlaceholderFrame::from_frame(*self).IsEmpty(),
            _ => false,
        }
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::CachedIsEmpty
    // ADAPTER: see kEmptyFramePropertyId.
    pub fn CachedIsEmpty(&self) -> bool {
        if self.GetProperty(nsIFrame::EmptyFrameProperty()).is_none() && self.Type() == LayoutFrameType::Block {
            // Block => nsBlockFrame::CachedIsEmpty
            return nsBlockFrame::from_frame(*self).CachedIsEmpty();
        }
        self.IsEmpty()
    }

    // ADAPTER: sets the stand-in property of IsEmpty().
    pub fn SetIsEmpty(&self, aIsEmpty: bool) {
        self.SetProperty(nsIFrame::EmptyFrameProperty(), aIsEmpty);
    }
}

impl nsStyleDisplay {
    // Gecko: layout/style/nsStyleStructInlines.h nsStyleDisplay::IsBlockOutside
    pub fn IsBlockOutside(&self, aContextFrame: nsIFrame) -> bool {
        // NS_ASSERTION(aContextFrame->StyleDisplay() == this, "unexpected aContextFrame");
        // NOT PORTED: the assertion (the wrapper is a value, so there is no identity to compare).
        if aContextFrame.IsInSVGTextSubtree() {
            return aContextFrame.IsBlockFrame();
        }
        self.IsBlockOutsideStyle()
    }
}

impl nsIFrame {
    // Gecko: layout/generic/nsIFrame.h CLASS_FLAG_METHOD(IsBlockFrame, Block) (the frame class Block
    // only; nsBlockFrame's subclasses have their own types)
    pub fn IsBlockFrame(&self) -> bool {
        self.Type() == LayoutFrameType::Block
    }
}
