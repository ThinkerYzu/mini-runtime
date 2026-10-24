// Translated from layout/generic/nsFrameList.h and nsFrameList.cpp (GECKO_REV), with the
// nsFrameList members that nsIFrame.h defines inline (StartRemoveFrame, ContinueRemoveFrame, the
// traversal classes of the iterators).
//
// Notes on the Rust shape (see CONVENTIONS.md):
//  - `nsIFrame*` is `Option<nsIFrame>`, `nsContainerFrame*` is `Option<nsIFrame>` (the parent
//    frame handle) where it may be null and `nsIFrame` where it may not.
//  - `nsFrameList&&` parameters take the list by value (the caller writes
//    `std::mem::take(&mut list)` where C++ writes `std::move(list)`); "clears out aFrameList" is
//    then automatic. `nsFrameList` is move-only like in C++; `Clone()` is the explicit copy.
//  - The nested classes are top-level: `nsFrameList_Iterator<T>`, `nsFrameList_Slice`,
//    `nsFrameList_ForwardFrameTraversal`, `nsFrameList_BackwardFrameTraversal`. The iterators also
//    implement `std::iter::Iterator` (so `for f in list.begin()` and `for f in &list` work as the
//    C++ range-for does); `operator_star`, `operator_inc` and `operator_dec` are `*it`, `++it`,
//    `--it`.
//  - NOT PORTED (whole file): the PresShell argument of `operator new` and `Delete` (frame lists
//    are heap allocated with Box here), the DEBUG_FRAME_DUMP code (List, ChildListName's
//    callers), and DEBUG_FRAME_LIST (VerifyList), which Gecko leaves off.
use crate::*;

// NOT PORTED: includes (stdio.h, mozilla/EnumSet.h, FunctionTypeTraits.h, nsDebug.h, nsTArray.h).

// class nsContainerFrame; class nsIContent; class nsIFrame; class nsPresContext;
// namespace mozilla { struct FrameDestroyContext; class PresShell; class FrameChildList;

// Gecko: layout/generic/nsFrameList.h mozilla::FrameChildListID
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum FrameChildListID {
    // The individual concrete child lists.
    Principal,
    Absolute,
    PushedAbsolute,
    Overflow,
    OverflowContainers,
    ExcessOverflowContainers,
    OverflowOutOfFlow,
    Float,
    Marker,
    PushedFloats,
    // A special alias for FrameChildListID::Principal that suppress the reflow
    // request that is normally done when manipulating child lists.
    NoReflowPrincipal,
}

impl EnumSetValue for FrameChildListID {
    fn BitFor(self) -> u32 {
        1 << (self as u32)
    }
}

// Uncomment this to enable expensive frame-list integrity checking
// #define DEBUG_FRAME_LIST

// Gecko: layout/generic/nsIFrame.h nsFrameList::ForwardFrameTraversal and BackwardFrameTraversal
// (declared in nsFrameList.h, defined inline in nsIFrame.h). In C++ they are two classes with the
// same two static functions; the trait stands for "a class with static Next and Prev".
pub trait nsFrameList_FrameTraversal {
    fn Next(aFrame: nsIFrame) -> Option<nsIFrame>;
    fn Prev(aFrame: nsIFrame) -> Option<nsIFrame>;
}

// Next()/Prev() need to know about nsIFrame. To make them inline, their
// implementations are in nsIFrame.h.
// Gecko: layout/generic/nsFrameList.h nsFrameList::ForwardFrameTraversal
pub struct nsFrameList_ForwardFrameTraversal;
// Gecko: layout/generic/nsFrameList.h nsFrameList::BackwardFrameTraversal
pub struct nsFrameList_BackwardFrameTraversal;

// Operators of nsFrameList::Iterator
// ---------------------------------------------------

impl nsFrameList_FrameTraversal for nsFrameList_ForwardFrameTraversal {
    // Gecko: layout/generic/nsIFrame.h nsFrameList::ForwardFrameTraversal::Next
    fn Next(aFrame: nsIFrame) -> Option<nsIFrame> {
        aFrame.GetNextSibling()
    }
    // Gecko: layout/generic/nsIFrame.h nsFrameList::ForwardFrameTraversal::Prev
    fn Prev(aFrame: nsIFrame) -> Option<nsIFrame> {
        aFrame.GetPrevSibling()
    }
}

impl nsFrameList_FrameTraversal for nsFrameList_BackwardFrameTraversal {
    // Gecko: layout/generic/nsIFrame.h nsFrameList::BackwardFrameTraversal::Next
    fn Next(aFrame: nsIFrame) -> Option<nsIFrame> {
        aFrame.GetPrevSibling()
    }
    // Gecko: layout/generic/nsIFrame.h nsFrameList::BackwardFrameTraversal::Prev
    fn Prev(aFrame: nsIFrame) -> Option<nsIFrame> {
        aFrame.GetNextSibling()
    }
}

/**
 * A class for managing a list of frames.
 */
// Gecko: layout/generic/nsFrameList.h nsFrameList
// (the copy constructor and copy assignment are deleted: no Clone/Copy, use Clone() below)
#[derive(Debug, PartialEq, Eq)]
pub struct nsFrameList {
    // protected:
    mFirstChild: Option<nsIFrame>,
    mLastChild: Option<nsIFrame>,
}

// Gecko: layout/generic/nsFrameList.h nsFrameList::iterator, const_iterator, reverse_iterator,
// const_reverse_iterator (the const and non-const ones are the same type)
pub type nsFrameList_iterator = nsFrameList_Iterator<nsFrameList_ForwardFrameTraversal>;
pub type nsFrameList_const_iterator = nsFrameList_Iterator<nsFrameList_ForwardFrameTraversal>;
pub type nsFrameList_reverse_iterator = nsFrameList_Iterator<nsFrameList_BackwardFrameTraversal>;
pub type nsFrameList_const_reverse_iterator = nsFrameList_Iterator<nsFrameList_BackwardFrameTraversal>;

// private:
// nsFrameList::sEmptyList (layout/generic/nsFrameList.cpp line 18: a static data member, which
// has no body for tools/gecko-drift.py to compare, so it carries no Gecko: marker)
static sEmptyList: nsFrameList = nsFrameList { mFirstChild: None, mLastChild: None };

// constexpr nsFrameList() : mFirstChild(nullptr), mLastChild(nullptr) {}
impl Default for nsFrameList {
    fn default() -> nsFrameList {
        nsFrameList { mFirstChild: None, mLastChild: None }
    }
}

impl nsFrameList {
    // Gecko: layout/generic/nsFrameList.h nsFrameList::nsFrameList(nsIFrame*, nsIFrame*)
    pub fn new(aFirstFrame: Option<nsIFrame>, aLastFrame: Option<nsIFrame>) -> nsFrameList {
        let list = nsFrameList { mFirstChild: aFirstFrame, mLastChild: aLastFrame };
        list.VerifyList();
        list
    }

    // nsFrameList is a move-only class by default. Use Clone() if you really want
    // a copy of this list.
    // Gecko: layout/generic/nsFrameList.h nsFrameList::Clone
    pub fn Clone(&self) -> nsFrameList {
        nsFrameList::new(self.mFirstChild, self.mLastChild)
    }

    /**
     * Transfer frames in aOther to this list. aOther becomes empty after this
     * operation.
     */
    // nsFrameList(nsFrameList&& aOther) is a Rust move (`let list = other;`) when the source is
    // not used again, and `std::mem::take(&mut other)` when it stays alive and must be emptied.
    // Gecko: layout/generic/nsFrameList.h nsFrameList::operator=(nsFrameList&&)
    // (`*self = std::mem::take(aOther)` would drop the old value silently; this keeps Gecko's assertion)
    pub fn operator_assign_move(&mut self, aOther: &mut nsFrameList) {
        if !std::ptr::eq(self, aOther) {
            MOZ_ASSERT!(self.IsEmpty(), "Assigning to a non-empty list will lose frames!");
            self.mFirstChild = aOther.FirstChild();
            self.mLastChild = aOther.LastChild();
            aOther.Clear();
        }
    }

    /**
     * Infallibly allocate a nsFrameList from the shell arena.
     */
    // void* operator new(size_t sz, mozilla::PresShell* aPresShell);
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::operator new
    // NOT PORTED: arena allocation. `Box::new(nsFrameList::default())` is the replacement.

    /**
     * Deallocate this list that was allocated from the shell arena.
     * The list is required to be empty.
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::Delete
    // NOT PORTED: the PresShell argument and FreeByObjectID; dropping the Box frees the list.
    pub fn Delete(&self) {
        MOZ_ASSERT!(!std::ptr::eq(self, nsFrameList::EmptyList()), "Shouldn't Delete() this list");
        NS_ASSERTION!(self.IsEmpty(), "Shouldn't Delete() a non-empty list");
    }

    /**
     * For each frame in this list: remove it from the list then call
     * Destroy() on it with the passed context as an argument.
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::DestroyFrames
    pub fn DestroyFrames(&mut self, aContext: &mut FrameDestroyContext) {
        while let Some(frame) = self.RemoveLastChild() {
            frame.Destroy(aContext);
        }
        MOZ_ASSERT!(self.mFirstChild.is_none() && self.mLastChild.is_none(), "We should've destroyed all frames!");
    }

    // Gecko: layout/generic/nsFrameList.h nsFrameList::Clear
    pub fn Clear(&mut self) {
        self.mFirstChild = None;
        self.mLastChild = None;
    }

    /**
     * Append aFrameList to this list.  If aParent is not null,
     * reparents the newly added frames.  Clears out aFrameList and
     * returns a list slice represening the newly-appended frames.
     */
    // Gecko: layout/generic/nsFrameList.h nsFrameList::AppendFrames
    pub fn AppendFrames(&mut self, aParent: Option<nsIFrame>, aFrameList: nsFrameList) -> nsFrameList_Slice {
        let last = self.LastChild();
        self.InsertFrames(aParent, last, aFrameList)
    }

    /**
     * Append aFrame to this list.  If aParent is not null,
     * reparents the newly added frame.
     */
    // Gecko: layout/generic/nsFrameList.h nsFrameList::AppendFrame
    pub fn AppendFrame(&mut self, aParent: Option<nsIFrame>, aFrame: nsIFrame) {
        self.AppendFrames(aParent, nsFrameList::new(Some(aFrame), Some(aFrame)));
    }

    /**
     * Take aFrame out of the frame list. This also disconnects aFrame
     * from the sibling list. The frame must be non-null and present on
     * this list.
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::RemoveFrame
    pub fn RemoveFrame(&mut self, aFrame: nsIFrame) {
        // MOZ_ASSERT(aFrame, "null ptr");  (the type of aFrame says so)
        // #ifdef DEBUG_FRAME_LIST
        //   // ContainsFrame is O(N)
        //   MOZ_ASSERT(ContainsFrame(aFrame), "wrong list");
        // #endif
        // NOT PORTED: the DEBUG_FRAME_LIST assertion above (Gecko leaves it off).

        let nextFrame = aFrame.GetNextSibling();
        if Some(aFrame) == self.mFirstChild {
            self.mFirstChild = nextFrame;
            aFrame.SetNextSibling(None);
            if nextFrame.is_none() {
                self.mLastChild = None;
            }
        } else {
            let prevSibling = aFrame.GetPrevSibling();
            NS_ASSERTION!(prevSibling.is_some() && prevSibling.unwrap().GetNextSibling() == Some(aFrame), "Broken frame linkage");
            let prevSibling = prevSibling.unwrap();
            prevSibling.SetNextSibling(nextFrame);
            aFrame.SetNextSibling(None);
            if nextFrame.is_none() {
                self.mLastChild = Some(prevSibling);
            }
        }
    }

    /**
     * Take all the frames before aFrame out of the frame list; aFrame and all the
     * frames after it stay in this list. If aFrame is nullptr, remove the entire
     * frame list.
     * @param aFrame a frame in this frame list, or nullptr.
     * @return the removed frames, if any.
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::TakeFramesBefore
    #[must_use]
    pub fn TakeFramesBefore(&mut self, aFrame: Option<nsIFrame>) -> nsFrameList {
        let Some(aFrame) = aFrame else {
            // We handed over the whole list.
            return std::mem::take(self);
        };

        MOZ_ASSERT!(self.ContainsFrame(aFrame), "aFrame is not on this list!");

        if Some(aFrame) == self.mFirstChild {
            // aFrame is our first child. Nothing to extract.
            return nsFrameList::default();
        }

        // Extract all previous siblings of aFrame as a new list.
        let prev = aFrame.GetPrevSibling();
        let newFirstChild = self.mFirstChild;
        let newLastChild = prev;

        prev.unwrap().SetNextSibling(None);
        self.mFirstChild = Some(aFrame);

        nsFrameList::new(newFirstChild, newLastChild)
    }

    /**
     * Take all the frames after aFrame out of the frame list; aFrame and all the
     * frames before it stay in this list. If aFrame is nullptr, removes the
     * entire list.
     * @param aFrame a frame in this list, or nullptr.
     * @return the removed frames, if any.
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::TakeFramesAfter
    #[must_use]
    pub fn TakeFramesAfter(&mut self, aFrame: Option<nsIFrame>) -> nsFrameList {
        let Some(aFrame) = aFrame else {
            return std::mem::take(self);
        };

        MOZ_ASSERT!(self.ContainsFrame(aFrame), "aFrame is not on this list!");

        let newFirstChild = aFrame.GetNextSibling();
        if newFirstChild.is_none() {
            return nsFrameList::default();
        }

        let newLastChild = self.mLastChild;
        self.mLastChild = Some(aFrame);
        self.mLastChild.unwrap().SetNextSibling(None);
        nsFrameList::new(newFirstChild, newLastChild)
    }

    /**
     * Take the first (or last) child (if any) out of the frame list.
     * @return the first (or last) child, or nullptr if the list is empty
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::RemoveFirstChild
    pub fn RemoveFirstChild(&mut self) -> Option<nsIFrame> {
        if let Some(firstChild) = self.mFirstChild {
            self.RemoveFrame(firstChild);
            return Some(firstChild);
        }
        None
    }

    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::RemoveLastChild
    pub fn RemoveLastChild(&mut self) -> Option<nsIFrame> {
        if let Some(lastChild) = self.mLastChild {
            self.RemoveFrame(lastChild);
            return Some(lastChild);
        }
        None
    }

    /**
     * The following two functions are intended to be used in concert for
     * removing a frame from its frame list when the set of possible frame
     * lists is known in advance, but the exact frame list is unknown.
     * aFrame must be non-null.
     * Example use:
     *   bool removed = frameList1.StartRemoveFrame(aFrame) ||
     *                  frameList2.ContinueRemoveFrame(aFrame) ||
     *                  frameList3.ContinueRemoveFrame(aFrame);
     *   MOZ_ASSERT(removed);
     *
     * @note One of the frame lists MUST contain aFrame, if it's on some other
     *       frame list then the example above will likely lead to crashes.
     * This function is O(1).
     * @return true iff aFrame was removed from /some/ list, not necessarily
     *         this one.  If it was removed from a different list then it is
     *         guaranteed that that list is still non-empty.
     * (this method is implemented in nsIFrame.h to be able to inline)
     */
    // Gecko: layout/generic/nsIFrame.h nsFrameList::StartRemoveFrame
    pub fn StartRemoveFrame(&mut self, aFrame: nsIFrame) -> bool {
        if aFrame.GetPrevSibling().is_some() && aFrame.GetNextSibling().is_some() {
            nsFrameList::UnhookFrameFromSiblings(aFrame);
            return true;
        }
        self.ContinueRemoveFrame(aFrame)
    }

    /**
     * Precondition: StartRemoveFrame MUST be called before this.
     * This function is O(1).
     * @see StartRemoveFrame
     * @return true iff aFrame was removed from this list
     * (this method is implemented in nsIFrame.h to be able to inline)
     */
    // Gecko: layout/generic/nsIFrame.h nsFrameList::ContinueRemoveFrame
    pub fn ContinueRemoveFrame(&mut self, aFrame: nsIFrame) -> bool {
        MOZ_ASSERT!(aFrame.GetPrevSibling().is_none() || aFrame.GetNextSibling().is_none(), "Forgot to call StartRemoveFrame?");
        if Some(aFrame) == self.mLastChild {
            MOZ_ASSERT!(aFrame.GetNextSibling().is_none(), "broken frame list");
            let prevSibling = aFrame.GetPrevSibling();
            let Some(prevSibling) = prevSibling else {
                MOZ_ASSERT!(Some(aFrame) == self.mFirstChild, "broken frame list");
                self.mFirstChild = None;
                self.mLastChild = None;
                return true;
            };
            MOZ_ASSERT!(prevSibling.GetNextSibling() == Some(aFrame), "Broken frame linkage");
            prevSibling.SetNextSibling(None);
            self.mLastChild = Some(prevSibling);
            return true;
        }
        if Some(aFrame) == self.mFirstChild {
            MOZ_ASSERT!(aFrame.GetPrevSibling().is_none(), "broken frame list");
            self.mFirstChild = aFrame.GetNextSibling();
            aFrame.SetNextSibling(None);
            MOZ_ASSERT!(self.mFirstChild.is_some(), "broken frame list");
            return true;
        }
        false
    }

    /**
     * Take a frame out of the frame list and then destroy it.
     * The frame must be non-null and present on this list.
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::DestroyFrame
    pub fn DestroyFrame(&mut self, aContext: &mut FrameDestroyContext, aFrame: nsIFrame) {
        // MOZ_ASSERT(aFrame, "null ptr");  (the type of aFrame says so)
        self.RemoveFrame(aFrame);
        aFrame.Destroy(aContext);
    }

    /**
     * Insert aFrame right after aPrevSibling, or prepend it to this
     * list if aPrevSibling is null. If aParent is not null, also
     * reparents newly-added frame. Note that this method always
     * sets the frame's nextSibling pointer.
     */
    // Gecko: layout/generic/nsFrameList.h nsFrameList::InsertFrame
    pub fn InsertFrame(&mut self, aParent: Option<nsIFrame>, aPrevSibling: Option<nsIFrame>, aFrame: nsIFrame) {
        self.InsertFrames(aParent, aPrevSibling, nsFrameList::new(Some(aFrame), Some(aFrame)));
    }

    /**
     * Inserts aFrameList into this list after aPrevSibling (at the beginning if
     * aPrevSibling is null).  If aParent is not null, reparents the newly added
     * frames.  Clears out aFrameList and returns a list slice representing the
     * newly-inserted frames.
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::InsertFrames
    pub fn InsertFrames(&mut self, aParent: Option<nsIFrame>, aPrevSibling: Option<nsIFrame>, mut aFrameList: nsFrameList) -> nsFrameList_Slice {
        MOZ_ASSERT!(aFrameList.NotEmpty(), "Unexpected empty list");

        if let Some(aParent) = aParent {
            aFrameList.ApplySetParent(aParent);
        }

        NS_ASSERTION!(
            self.IsEmpty() || self.FirstChild().unwrap().GetParent() == aFrameList.FirstChild().unwrap().GetParent(),
            "frame to add has different parent"
        );
        NS_ASSERTION!(
            aPrevSibling.is_none() || aPrevSibling.unwrap().GetParent() == aFrameList.FirstChild().unwrap().GetParent(),
            "prev sibling has different parent"
        );
        // #ifdef DEBUG_FRAME_LIST
        //   // ContainsFrame is O(N)
        //   NS_ASSERTION(!aPrevSibling || ContainsFrame(aPrevSibling), "prev sibling is not on this list");
        // #endif
        // NOT PORTED: the DEBUG_FRAME_LIST assertion above (Gecko leaves it off).

        let firstNewFrame = aFrameList.FirstChild().unwrap();
        let nextSibling;
        if let Some(aPrevSibling) = aPrevSibling {
            nextSibling = aPrevSibling.GetNextSibling();
            aPrevSibling.SetNextSibling(Some(firstNewFrame));
        } else {
            nextSibling = self.mFirstChild;
            self.mFirstChild = Some(firstNewFrame);
        }

        let lastNewFrame = aFrameList.LastChild().unwrap();
        lastNewFrame.SetNextSibling(nextSibling);
        if nextSibling.is_none() {
            self.mLastChild = Some(lastNewFrame);
        }

        self.VerifyList();

        aFrameList.Clear();
        nsFrameList_Slice::new(Some(firstNewFrame), nextSibling)
    }

    /**
     * Split this list just before the first frame that matches aPredicate,
     * and return a nsFrameList containing all the frames before it. The
     * matched frame and all frames after it stay in this list. If no matched
     * frame exists, all the frames are drained into the returned list, and
     * this list ends up empty.
     *
     * aPredicate should be of this function signature: bool(nsIFrame*).
     */
    // Gecko: layout/generic/nsFrameList.h nsFrameList::Split
    // (the static_assert on the predicate's type is the Rust bound `FnMut(nsIFrame) -> bool`)
    pub fn Split<Predicate: FnMut(nsIFrame) -> bool>(&mut self, mut aPredicate: Predicate) -> nsFrameList {
        for f in self.begin() {
            if aPredicate(f) {
                return self.TakeFramesBefore(Some(f));
            }
        }
        std::mem::take(self)
    }

    // Gecko: layout/generic/nsFrameList.h nsFrameList::FirstChild
    pub fn FirstChild(&self) -> Option<nsIFrame> {
        self.mFirstChild
    }

    // Gecko: layout/generic/nsFrameList.h nsFrameList::LastChild
    pub fn LastChild(&self) -> Option<nsIFrame> {
        self.mLastChild
    }

    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::FrameAt
    pub fn FrameAt(&self, mut aIndex: i32) -> Option<nsIFrame> {
        MOZ_ASSERT!(aIndex >= 0, "invalid arg");
        if aIndex < 0 {
            return None;
        }
        let mut frame = self.mFirstChild;
        while {
            let more = aIndex > 0;
            aIndex -= 1;
            more
        } && frame.is_some()
        {
            frame = frame.unwrap().GetNextSibling();
        }
        frame
    }

    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::IndexOf
    pub fn IndexOf(&self, aFrame: nsIFrame) -> i32 {
        let mut count = 0;
        let mut f = self.mFirstChild;
        while let Some(frame) = f {
            if frame == aFrame {
                return count;
            }
            count += 1;
            f = frame.GetNextSibling();
        }
        -1
    }

    // Gecko: layout/generic/nsFrameList.h nsFrameList::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        self.mFirstChild.is_none()
    }

    // Gecko: layout/generic/nsFrameList.h nsFrameList::NotEmpty
    pub fn NotEmpty(&self) -> bool {
        self.mFirstChild.is_some()
    }

    /**
     * Return true if aFrame is on this list.
     * @note this method has O(n) time complexity over the length of the list
     * XXXmats: ideally, we should make this function #ifdef DEBUG
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::ContainsFrame
    pub fn ContainsFrame(&self, aFrame: nsIFrame) -> bool {
        // MOZ_ASSERT(aFrame, "null ptr");  (the type of aFrame says so)
        let mut frame = self.mFirstChild;
        while let Some(f) = frame {
            if f == aFrame {
                return true;
            }
            frame = f.GetNextSibling();
        }
        false
    }

    /**
     * Get the number of frames in this list. Note that currently the
     * implementation has O(n) time complexity. Do not call it repeatedly in hot
     * code.
     * XXXmats: ideally, we should make this function #ifdef DEBUG
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::GetLength
    pub fn GetLength(&self) -> i32 {
        let mut count = 0;
        let mut frame = self.mFirstChild;
        while let Some(f) = frame {
            count += 1;
            frame = f.GetNextSibling();
        }
        count
    }

    /**
     * If this frame list has only one frame, return that frame.
     * Otherwise, return null.
     */
    // Gecko: layout/generic/nsFrameList.h nsFrameList::OnlyChild
    pub fn OnlyChild(&self) -> Option<nsIFrame> {
        if self.FirstChild() == self.LastChild() {
            return self.FirstChild();
        }
        None
    }

    /**
     * Call SetParent(aParent) for each frame in this list.
     * @param aParent the new parent frame, must be non-null
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::ApplySetParent
    pub fn ApplySetParent(&self, aParent: nsIFrame) {
        // NS_ASSERTION(aParent, "null ptr");  (the type of aParent says so)
        let mut f = self.FirstChild();
        while let Some(frame) = f {
            frame.SetParent(aParent);
            f = frame.GetNextSibling();
        }
    }

    /**
     * If this frame list is non-empty then append it to aLists as the
     * aListID child list.
     */
    // Gecko: layout/generic/nsFrameList.h nsFrameList::AppendIfNonempty
    pub fn AppendIfNonempty(&self, aLists: &mut Vec<FrameChildList>, aListID: FrameChildListID) {
        if self.NotEmpty() {
            aLists.push(FrameChildList::new(self, aListID));
        }
    }

    /**
     * Return the frame before this frame in visual order (after Bidi reordering).
     * If aFrame is null, return the last frame in visual order.
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::GetPrevVisualFor
    pub fn GetPrevVisualFor(&self, aFrame: Option<nsIFrame>) -> Option<nsIFrame> {
        // NOT PORTED: the body (nsFrameList.cpp lines 248-324 at GECKO_REV). It depends on
        // nsBidiPresUtils (ParagraphDirection, GetFrameToLeftOf, GetFrameToRightOf,
        // IsFrameInParagraphDirection) and on nsIFrame::GetLineIterator(), none of which exist
        // here; bidi reordering is not part of the stage 2b scope.
        unimplemented!("NOT PORTED: nsBidiPresUtils, nsIFrame::GetLineIterator")
    }

    /**
     * Return the frame after this frame in visual order (after Bidi reordering).
     * If aFrame is null, return the first frame in visual order.
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::GetNextVisualFor
    pub fn GetNextVisualFor(&self, aFrame: Option<nsIFrame>) -> Option<nsIFrame> {
        // NOT PORTED: the body (nsFrameList.cpp lines 326-403 at GECKO_REV); same reason as
        // GetPrevVisualFor.
        unimplemented!("NOT PORTED: nsBidiPresUtils, nsIFrame::GetLineIterator")
    }

    // #ifdef DEBUG_FRAME_DUMP
    //   void List(FILE* out) const;
    // #endif
    // NOT PORTED: frame dumps (nsFrameList.cpp lines 239-246 at GECKO_REV; needs nsIFrame::List).

    // Gecko: layout/generic/nsFrameList.h nsFrameList::EmptyList
    pub fn EmptyList() -> &'static nsFrameList {
        &sEmptyList
    }

    // Gecko: layout/generic/nsFrameList.h nsFrameList::begin, cbegin, end, cend, rbegin, crbegin,
    // rend, crend
    pub fn begin(&self) -> nsFrameList_iterator {
        nsFrameList_Iterator::new(self.mFirstChild)
    }
    pub fn cbegin(&self) -> nsFrameList_const_iterator {
        self.begin()
    }
    pub fn end(&self) -> nsFrameList_iterator {
        nsFrameList_Iterator::new(None)
    }
    pub fn cend(&self) -> nsFrameList_const_iterator {
        self.end()
    }
    pub fn rbegin(&self) -> nsFrameList_reverse_iterator {
        nsFrameList_Iterator::new(self.mLastChild)
    }
    pub fn crbegin(&self) -> nsFrameList_const_reverse_iterator {
        self.rbegin()
    }
    pub fn rend(&self) -> nsFrameList_reverse_iterator {
        nsFrameList_Iterator::new(None)
    }
    pub fn crend(&self) -> nsFrameList_const_reverse_iterator {
        self.rend()
    }

    // #ifdef DEBUG_FRAME_LIST
    //   void VerifyList() const;
    // #else
    //   void VerifyList() const {}
    // #endif
    // Gecko: layout/generic/nsFrameList.h nsFrameList::VerifyList
    // NOT PORTED: the DEBUG_FRAME_LIST version (nsFrameList.cpp lines 405-446 at GECKO_REV); it is
    // an expensive check that Gecko leaves off.
    fn VerifyList(&self) {}

    /**
     * Disconnect aFrame from its siblings.  This must only be called if aFrame
     * is NOT the first or last sibling, because otherwise its nsFrameList will
     * have a stale mFirst/LastChild pointer.  This precondition is asserted.
     * This function is O(1).
     */
    // Gecko: layout/generic/nsFrameList.cpp nsFrameList::UnhookFrameFromSiblings
    /* static */
    fn UnhookFrameFromSiblings(aFrame: nsIFrame) {
        MOZ_ASSERT!(aFrame.GetPrevSibling().is_some() && aFrame.GetNextSibling().is_some());
        let nextSibling = aFrame.GetNextSibling();
        let prevSibling = aFrame.GetPrevSibling().unwrap();
        aFrame.SetNextSibling(None);
        prevSibling.SetNextSibling(nextSibling);
        MOZ_ASSERT!(aFrame.GetPrevSibling().is_none() && aFrame.GetNextSibling().is_none());
    }
}

impl<'a> IntoIterator for &'a nsFrameList {
    type Item = nsIFrame;
    type IntoIter = nsFrameList_iterator;
    // `for (nsIFrame* f : list)` is `for f in &list`.
    fn into_iter(self) -> nsFrameList_iterator {
        self.begin()
    }
}

/**
 * A class representing a slice of a frame list.
 */
// Gecko: layout/generic/nsFrameList.h nsFrameList::Slice
#[derive(Clone, Copy, Debug)]
pub struct nsFrameList_Slice {
    // Our starting frame.
    mStart: Option<nsIFrame>,

    // The first frame that is NOT in the slice. May be null.
    mEnd: Option<nsIFrame>,
}

impl nsFrameList_Slice {
    // Implicit on purpose, so that we can easily create Slice from nsFrameList
    // via this impicit constructor.
    // Gecko: layout/generic/nsFrameList.h nsFrameList::Slice::Slice(const nsFrameList&)
    pub fn new__nsFrameList(aList: &nsFrameList) -> nsFrameList_Slice {
        nsFrameList_Slice { mStart: aList.FirstChild(), mEnd: None }
    }
    // Gecko: layout/generic/nsFrameList.h nsFrameList::Slice::Slice(nsIFrame*, nsIFrame*)
    pub fn new(aStart: Option<nsIFrame>, aEnd: Option<nsIFrame>) -> nsFrameList_Slice {
        nsFrameList_Slice { mStart: aStart, mEnd: aEnd }
    }

    // void operator delete(void*) = delete;  (a Slice is a plain value here)

    // Gecko: layout/generic/nsFrameList.h nsFrameList::Slice::begin, cbegin, end, cend
    pub fn begin(&self) -> nsFrameList_iterator {
        nsFrameList_Iterator::new(self.mStart)
    }
    pub fn cbegin(&self) -> nsFrameList_const_iterator {
        self.begin()
    }
    pub fn end(&self) -> nsFrameList_iterator {
        nsFrameList_Iterator::new(self.mEnd)
    }
    pub fn cend(&self) -> nsFrameList_const_iterator {
        self.end()
    }
}

impl IntoIterator for nsFrameList_Slice {
    type Item = nsIFrame;
    type IntoIter = std::iter::TakeWhile<nsFrameList_iterator, Box<dyn FnMut(&nsIFrame) -> bool>>;
    // `for (nsIFrame* f : slice)`: the frames from mStart up to (not including) mEnd.
    fn into_iter(self) -> Self::IntoIter {
        let end = self.mEnd;
        self.begin().take_while(Box::new(move |f: &nsIFrame| Some(*f) != end))
    }
}

// Gecko: layout/generic/nsFrameList.h nsFrameList::Iterator
#[derive(Debug)]
pub struct nsFrameList_Iterator<FrameTraversal: nsFrameList_FrameTraversal> {
    mCurrent: Option<nsIFrame>,
    mMarker: std::marker::PhantomData<FrameTraversal>,
}

impl<T: nsFrameList_FrameTraversal> Clone for nsFrameList_Iterator<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: nsFrameList_FrameTraversal> Copy for nsFrameList_Iterator<T> {}
// bool operator==(const Iterator<FrameTraversal>& aOther) const = default;  (and !=)
impl<T: nsFrameList_FrameTraversal> PartialEq for nsFrameList_Iterator<T> {
    fn eq(&self, aOther: &Self) -> bool {
        self.mCurrent == aOther.mCurrent
    }
}
impl<T: nsFrameList_FrameTraversal> Eq for nsFrameList_Iterator<T> {}

impl<FrameTraversal: nsFrameList_FrameTraversal> nsFrameList_Iterator<FrameTraversal> {
    // Gecko: layout/generic/nsFrameList.h nsFrameList::Iterator::Iterator
    pub fn new(aCurrent: Option<nsIFrame>) -> Self {
        nsFrameList_Iterator { mCurrent: aCurrent, mMarker: std::marker::PhantomData }
    }

    // Gecko: layout/generic/nsFrameList.h nsFrameList::Iterator::operator*
    pub fn operator_star(&self) -> Option<nsIFrame> {
        self.mCurrent
    }

    // Gecko: layout/generic/nsFrameList.h nsFrameList::Iterator::operator++()
    pub fn operator_inc(&mut self) -> &mut Self {
        self.mCurrent = FrameTraversal::Next(self.mCurrent.unwrap());
        self
    }
    // Gecko: layout/generic/nsFrameList.h nsFrameList::Iterator::operator--()
    pub fn operator_dec(&mut self) -> &mut Self {
        self.mCurrent = FrameTraversal::Prev(self.mCurrent.unwrap());
        self
    }
}

// The std iterator protocol: `it.next()` is `auto f = *it; ++it; return f;` that stops at null.
impl<FrameTraversal: nsFrameList_FrameTraversal> Iterator for nsFrameList_Iterator<FrameTraversal> {
    type Item = nsIFrame;
    fn next(&mut self) -> Option<nsIFrame> {
        let current = self.mCurrent?;
        self.mCurrent = FrameTraversal::Next(current);
        Some(current)
    }
}

// Gecko: layout/generic/nsFrameList.h (namespace mozilla) ChildListName
// #ifdef DEBUG_FRAME_DUMP
#[cfg(debug_assertions)]
pub fn ChildListName(aListID: FrameChildListID) -> &'static str {
    match aListID {
        FrameChildListID::Principal => "",
        FrameChildListID::Absolute => "AbsoluteList",
        FrameChildListID::PushedAbsolute => "PushedAbsoluteList",
        FrameChildListID::Overflow => "OverflowList",
        FrameChildListID::OverflowContainers => "OverflowContainersList",
        FrameChildListID::ExcessOverflowContainers => "ExcessOverflowContainersList",
        FrameChildListID::OverflowOutOfFlow => "OverflowOutOfFlowList",
        FrameChildListID::Float => "FloatList",
        FrameChildListID::Marker => "MarkerList",
        FrameChildListID::PushedFloats => "PushedFloatsList",
        FrameChildListID::NoReflowPrincipal => "NoReflowPrincipalList",
    }
    // MOZ_ASSERT_UNREACHABLE("unknown list"); return "UNKNOWN_FRAME_CHILD_LIST";
    // (the match is exhaustive in Rust)
}

// Gecko: layout/generic/nsFrameList.h mozilla::FrameChildListIDs
pub type FrameChildListIDs = EnumSet<FrameChildListID>;

// Gecko: layout/generic/nsFrameList.h mozilla::FrameChildList
pub struct FrameChildList {
    pub mList: nsFrameList,
    pub mID: FrameChildListID,
}

impl FrameChildList {
    // Gecko: layout/generic/nsFrameList.h FrameChildList::FrameChildList
    pub fn new(aList: &nsFrameList, aID: FrameChildListID) -> FrameChildList {
        FrameChildList { mList: aList.Clone(), mID: aID }
    }
}

/**
 * Simple "auto_ptr" for nsFrameLists allocated from the shell arena.
 * The frame list given to the constructor will be deallocated (if non-null)
 * in the destructor.  The frame list must then be empty.
 */
// Gecko: layout/generic/nsFrameList.h mozilla::AutoFrameListPtr
// NOT PORTED: the nsPresContext argument (it only reaches the PresShell for Delete()).
pub struct AutoFrameListPtr {
    mFrameList: Option<Box<nsFrameList>>,
}

impl AutoFrameListPtr {
    // Gecko: layout/generic/nsFrameList.h AutoFrameListPtr::AutoFrameListPtr
    pub fn new(aFrameList: Option<Box<nsFrameList>>) -> AutoFrameListPtr {
        AutoFrameListPtr { mFrameList: aFrameList }
    }
    // operator nsFrameList*() const and operator->() const
    pub fn operator_arrow(&self) -> Option<&nsFrameList> {
        self.mFrameList.as_deref()
    }
    pub fn operator_arrow_mut(&mut self) -> Option<&mut nsFrameList> {
        self.mFrameList.as_deref_mut()
    }
}

// Gecko: layout/generic/nsFrameList.cpp AutoFrameListPtr::~AutoFrameListPtr
impl Drop for AutoFrameListPtr {
    fn drop(&mut self) {
        if let Some(list) = &self.mFrameList {
            list.Delete();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests written from the documented behaviour of nsFrameList (Gecko has no gtest for it; it is
    // exercised through layout tests only).

    // `n` sibling frames under one parent, as the list's contents; also returns the parent.
    fn frames(n: usize) -> (nsIFrame, Vec<nsIFrame>) {
        let wm = WritingMode::default();
        WithFrameTree(|t| {
            let parent = t.AddFrame(None, nsRect::new(0, 0, 100, 100), wm, None);
            let kids = (0..n).map(|_| t.AddFrame(Some(parent), nsRect::new(0, 0, 10, 10), wm, None)).collect();
            (parent, kids)
        })
    }

    // A list over frames that AddFrame already linked.
    fn list_of(kids: &[nsIFrame]) -> nsFrameList {
        nsFrameList::new(kids.first().copied(), kids.last().copied())
    }

    fn collect(l: &nsFrameList) -> Vec<nsIFrame> {
        l.begin().collect()
    }

    #[test]
    fn empty_list() {
        let l = nsFrameList::default();
        assert!(l.IsEmpty() && !l.NotEmpty());
        assert_eq!(l.GetLength(), 0);
        assert_eq!(l.FirstChild(), None);
        assert_eq!(l.OnlyChild(), None);
        assert!(l.begin() == l.end());
        assert!(nsFrameList::EmptyList().IsEmpty());
    }

    #[test]
    fn length_index_and_contains() {
        let (_, k) = frames(4);
        let l = list_of(&k);
        assert_eq!(l.GetLength(), 4);
        assert_eq!(l.FrameAt(0), Some(k[0]));
        assert_eq!(l.FrameAt(3), Some(k[3]));
        assert_eq!(l.FrameAt(4), None);
        assert_eq!(l.IndexOf(k[2]), 2);
        assert!(l.ContainsFrame(k[1]));
        let (_, other) = frames(1);
        assert_eq!(l.IndexOf(other[0]), -1);
        assert!(!l.ContainsFrame(other[0]));
    }

    #[test]
    fn only_child() {
        let (_, k) = frames(1);
        assert_eq!(list_of(&k).OnlyChild(), Some(k[0]));
        let (_, k) = frames(2);
        assert_eq!(list_of(&k).OnlyChild(), None);
    }

    #[test]
    fn iterators_forward_and_backward() {
        let (_, k) = frames(3);
        let l = list_of(&k);
        assert_eq!(collect(&l), k);
        let mut rev: Vec<nsIFrame> = l.rbegin().collect();
        rev.reverse();
        assert_eq!(rev, k);
        let mut n = 0;
        for f in &l {
            assert_eq!(f, k[n]);
            n += 1;
        }
        let mut it = l.begin();
        it.operator_inc();
        assert_eq!(it.operator_star(), Some(k[1]));
        it.operator_dec();
        assert_eq!(it.operator_star(), Some(k[0]));
    }

    #[test]
    fn remove_frame_first_middle_last() {
        let (_, k) = frames(4);
        let mut l = list_of(&k);
        l.RemoveFrame(k[1]);
        assert_eq!(collect(&l), vec![k[0], k[2], k[3]]);
        assert_eq!(k[1].GetNextSibling(), None);
        assert_eq!(k[1].GetPrevSibling(), None);
        assert_eq!(k[2].GetPrevSibling(), Some(k[0]));
        l.RemoveFrame(k[0]);
        assert_eq!(l.FirstChild(), Some(k[2]));
        assert_eq!(k[2].GetPrevSibling(), None);
        l.RemoveFrame(k[3]);
        assert_eq!(l.LastChild(), Some(k[2]));
        l.RemoveFrame(k[2]);
        assert!(l.IsEmpty() && l.LastChild().is_none());
    }

    #[test]
    fn remove_first_and_last_child() {
        let (_, k) = frames(3);
        let mut l = list_of(&k);
        assert_eq!(l.RemoveFirstChild(), Some(k[0]));
        assert_eq!(l.RemoveLastChild(), Some(k[2]));
        assert_eq!(collect(&l), vec![k[1]]);
        assert_eq!(l.RemoveLastChild(), Some(k[1]));
        assert_eq!(l.RemoveFirstChild(), None);
        assert_eq!(l.RemoveLastChild(), None);
    }

    #[test]
    fn insert_frames_returns_slice_of_new_frames() {
        let (p, k) = frames(5);
        // Detach k[1], k[2] and k[3] as their own list, leaving k[0] and k[4] in `l`.
        let mut l = list_of(&k);
        let mut tail = l.TakeFramesAfter(Some(k[0])); // l=[k0], tail=[k1 k2 k3 k4]
        let last = tail.TakeFramesAfter(Some(k[3])); // tail=[k1 k2 k3], last=[k4]
        l.AppendFrames(None, last); // [k0 k4]
        assert_eq!(collect(&l), vec![k[0], k[4]]);
        let slice = l.InsertFrames(Some(p), Some(k[0]), tail);
        assert_eq!(collect(&l), k);
        let inserted: Vec<nsIFrame> = slice.into_iter().collect();
        assert_eq!(inserted, vec![k[1], k[2], k[3]]);
        assert_eq!(k[2].GetParent(), Some(p));
    }

    #[test]
    fn insert_frame_at_start_and_append() {
        let (_, k) = frames(3);
        let mut l = nsFrameList::default();
        l.InsertFrame(None, None, k[1]);
        l.InsertFrame(None, None, k[0]);
        l.AppendFrame(None, k[2]);
        assert_eq!(collect(&l), k);
        assert_eq!(l.LastChild(), Some(k[2]));
    }

    #[test]
    fn append_frames_reparents() {
        let wm = WritingMode::default();
        let (p1, k) = frames(2);
        let p2 = WithFrameTree(|t| t.AddFrame(None, nsRect::default(), wm, None));
        let mut l = nsFrameList::default();
        // Siblings in a list need to have the same parent when the list is non-empty; reparenting
        // them first to p2 as AppendFrames does for the new frames is the documented use.
        l.AppendFrames(Some(p2), list_of(&k));
        assert_eq!(k[0].GetParent(), Some(p2));
        assert_eq!(k[1].GetParent(), Some(p2));
        assert_ne!(p1, p2);
    }

    #[test]
    fn take_frames_before_and_after() {
        let (_, k) = frames(4);
        let mut l = list_of(&k);
        let before = l.TakeFramesBefore(Some(k[2]));
        assert_eq!(collect(&before), vec![k[0], k[1]]);
        assert_eq!(collect(&l), vec![k[2], k[3]]);
        assert_eq!(k[1].GetNextSibling(), None);

        let after = l.TakeFramesAfter(Some(k[2]));
        assert_eq!(collect(&after), vec![k[3]]);
        assert_eq!(collect(&l), vec![k[2]]);

        // The first frame: nothing before it. The last frame: nothing after it.
        assert!(l.TakeFramesBefore(Some(k[2])).IsEmpty());
        assert!(l.TakeFramesAfter(Some(k[2])).IsEmpty());
        // nullptr takes everything.
        let all = l.TakeFramesAfter(None);
        assert_eq!(collect(&all), vec![k[2]]);
        assert!(l.IsEmpty());
    }

    #[test]
    fn split_at_predicate() {
        let (_, k) = frames(4);
        let mut l = list_of(&k);
        let head = l.Split(|f| f == k[2]);
        assert_eq!(collect(&head), vec![k[0], k[1]]);
        assert_eq!(collect(&l), vec![k[2], k[3]]);
        // No match: everything moves to the returned list.
        let rest = l.Split(|_| false);
        assert_eq!(collect(&rest), vec![k[2], k[3]]);
        assert!(l.IsEmpty());
    }

    #[test]
    fn start_and_continue_remove_frame() {
        let (_, k) = frames(5);
        // Two lists: [k0 k1 k2] and [k3 k4] (AddFrame linked all five, so cut after k2).
        let mut a = list_of(&k);
        let mut b = a.TakeFramesAfter(Some(k[2]));
        // A middle frame: removed by StartRemoveFrame itself.
        assert!(a.StartRemoveFrame(k[1]));
        assert_eq!(collect(&a), vec![k[0], k[2]]);
        // The first frame of the second list, found by trying the lists in turn.
        let removed = a.StartRemoveFrame(k[3]) || b.ContinueRemoveFrame(k[3]);
        assert!(removed);
        assert_eq!(collect(&b), vec![k[4]]);
        // The last frame of the first list.
        assert!(a.StartRemoveFrame(k[2]));
        assert_eq!(collect(&a), vec![k[0]]);
        // The last frame of the second list is found by ContinueRemoveFrame alone.
        assert!(!a.ContinueRemoveFrame(k[4]) && b.ContinueRemoveFrame(k[4]));
        assert!(b.IsEmpty());
    }

    #[test]
    fn destroy_frames_in_reverse_order() {
        let (_, k) = frames(3);
        let mut l = list_of(&k);
        let mut ctx = FrameDestroyContext::default();
        l.DestroyFrames(&mut ctx);
        assert!(l.IsEmpty());
        assert_eq!(ctx.mDestroyedFrames, vec![k[2], k[1], k[0]]);
        assert_eq!(k[0].GetParent(), None);
    }

    #[test]
    fn destroy_frame() {
        let (_, k) = frames(2);
        let mut l = list_of(&k);
        let mut ctx = FrameDestroyContext::default();
        l.DestroyFrame(&mut ctx, k[0]);
        assert_eq!(collect(&l), vec![k[1]]);
        assert_eq!(ctx.mDestroyedFrames, vec![k[0]]);
    }

    #[test]
    fn clone_is_a_copy_of_the_ends_and_move_empties_the_source() {
        let (_, k) = frames(2);
        let mut l = list_of(&k);
        let c = l.Clone();
        assert_eq!(collect(&c), k);
        let mut dest = nsFrameList::default();
        dest.operator_assign_move(&mut l);
        assert_eq!(collect(&dest), k);
        assert!(l.IsEmpty());
    }

    #[test]
    fn append_if_nonempty() {
        let (_, k) = frames(1);
        let mut lists = Vec::new();
        nsFrameList::default().AppendIfNonempty(&mut lists, FrameChildListID::Float);
        assert!(lists.is_empty());
        list_of(&k).AppendIfNonempty(&mut lists, FrameChildListID::Float);
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].mID, FrameChildListID::Float);
        #[cfg(debug_assertions)]
        assert_eq!(ChildListName(lists[0].mID), "FloatList");
    }

    #[test]
    fn slice_of_whole_list() {
        let (_, k) = frames(3);
        let l = list_of(&k);
        let v: Vec<nsIFrame> = nsFrameList_Slice::new__nsFrameList(&l).into_iter().collect();
        assert_eq!(v, k);
    }

    #[test]
    fn prev_sibling_follows_next_sibling() {
        let (_, k) = frames(3);
        assert_eq!(k[1].GetPrevSibling(), Some(k[0]));
        k[0].SetNextSibling(Some(k[2]));
        assert_eq!(k[2].GetPrevSibling(), Some(k[0]));
        // k[1] no longer has a previous sibling because k[0] stopped pointing at it.
        assert_eq!(k[1].GetPrevSibling(), None);
    }
}
