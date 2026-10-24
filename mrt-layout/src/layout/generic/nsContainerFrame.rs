/* base class #1 for rendering objects that have child lists */
// Translated from layout/generic/nsContainerFrame.h, nsContainerFrame.cpp and
// nsContainerFrameInlines.h (GECKO_REV), together with the nsIFrame pieces that
// DoInlineIntrinsicISize needs (InlineMinISizeData and friends, nsIFrame.h / nsIFrame.cpp).
//
// How nsContainerFrame looks in Rust (see also CONVENTIONS.md, "Frames and styles"):
//  - `nsIFrame::GetParent()` returns an `nsIFrame` where Gecko returns an `nsContainerFrame*`, so
//    the members of nsContainerFrame are methods of `nsIFrame` (as nsContainerFrame_ComputeAutoSize
//    already was). `nsContainerFrame` below is the typed handle for "a pointer to a container
//    frame" (ContinuationTraversingState::mNextInFlow, static functions, the property descriptors).
//  - What is virtual in C++ is a `match` on the frame type: the plain name (`StealFrame`) is the
//    virtual call and `nsContainerFrame_StealFrame` is the call that names the base class. Frame
//    classes of other files that override the member have a `// <Class> => ...` line in the match;
//    for Block it calls nsBlockFrame's method, which calls the base class one by its
//    `nsContainerFrame_` name where C++ calls `nsContainerFrame::X`.
//  - `mFrames` (the principal child list) is `self.mFrames()`: a `&'static mut nsFrameList` into
//    the frame's FrameData (nsIFrame.rs), created on first use.
//  - The frame lists that Gecko keeps as frame properties (OverflowProperty, ...) are properties
//    of type `*mut nsFrameList` (a Box leaked with Box::into_raw, freed by the Destroy* members).
//  - Frames are never freed (they live in the arena of the frame tree), so "destroying" a frame
//    detaches it (nsIFrame_Destroy in nsIFrameLists.rs).
//
// NOT PORTED (all of them are marked where they would be): painting (BuildDisplayList*,
// DisplaySelectionOverlay, DisplayOverflowContainers), caret and selection (PeekOffset*), debug
// dumps (List*), widgets (SetSizeConstraints), absolute positioning (the AbsoluteContainingBlock
// calls), bidi/ruby, flex and grid child lists (NormalizeChildLists, PushIncompleteChildren, ...),
// overflow container reflow (ReflowOverflowContainerChildren after its early return),
// MarkIntrinsicISizesDirty. refcount logging (MOZ_COUNT_CTOR) is dropped.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::{Deref, DerefMut};

// Some macros for container classes to do sanity checking on
// width/height/x/y values computed during reflow.
// NOTE: AppUnitsPerCSSPixel value hardwired here to remove the
// dependency on nsDeviceContext.h.  It doesn't matter if it's a
// little off.
#[cfg(debug_assertions)]
// 10 million pixels, converted to app units. Note that this a bit larger
// than 1/4 of nscoord_MAX. So, if any content gets to be this large, we're
// definitely in danger of grazing up against nscoord_MAX; hence, it's ABSURD.
// Gecko: layout/generic/nsContainerFrame.h ABSURD_COORD
pub const ABSURD_COORD: nscoord = 10000000 * 60;
// Gecko: layout/generic/nsContainerFrame.h ABSURD_SIZE
#[cfg(debug_assertions)]
pub fn ABSURD_SIZE(_x: nscoord) -> bool {
    (_x < -ABSURD_COORD) || (_x > ABSURD_COORD)
}

// Gecko: layout/generic/nsIFrame.h MAX_REFLOW_DEPTH (the 64-bit Linux value)
pub const MAX_REFLOW_DEPTH: i32 = 1026;

// Define the MAX_FRAME_DEPTH to be the ContentSink's MAX_REFLOW_DEPTH plus
// 4 for the frames above the document's frames:
//  the Viewport, GFXScroll, ScrollPort, and Canvas
// Gecko: layout/generic/nsContainerFrame.cpp MAX_FRAME_DEPTH
pub const MAX_FRAME_DEPTH: i32 = MAX_REFLOW_DEPTH + 4;

// Gecko: layout/base/PresShellForwards.h mozilla::IntrinsicDirty
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IntrinsicDirty {
    // Do not mark intrinsic inline-sizes dirty.
    None,
    // Mark intrinsic inline-sizes dirty on the frame, its ancestors and descendants.
    FrameAncestorsAndDescendants,
    // Mark intrinsic inline-sizes dirty on the frame and its ancestors.
    FrameAndAncestors,
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::MarkIntrinsicISizesDirty (virtual)
// ADAPTER: the overrides of the classes that keep intrinsic-size caches: blocks, tables, flex and grid
// containers, text frames (which drop their text runs); nsIFrame's own clears the caches of a flex or
// grid item's measurements.
pub fn MarkIntrinsicISizesDirty(aFrame: nsIFrame) {
    // nsIFrame::MarkIntrinsicISizesDirty: a flex item's cached measurements depend on its intrinsic size.
    if aFrame.IsFlexItem() {
        nsFlexContainerFrame::MarkCachedFlexMeasurementsDirty(aFrame);
    }
    // (nsIFrame::MarkIntrinsicISizesDirty: the same for a grid item's cached measurements)
    if aFrame.IsGridItem() {
        nsGridContainerFrame::MarkCachedGridMeasurementsDirty(aFrame);
    }
    match aFrame.Type() {
        // Block => nsBlockFrame::MarkIntrinsicISizesDirty
        LayoutFrameType::Block => nsBlockFrame::from_frame(aFrame).MarkIntrinsicISizesDirty(),
        // Table => nsTableFrame::MarkIntrinsicISizesDirty
        LayoutFrameType::Table => nsTableFrame::from_frame(aFrame).MarkIntrinsicISizesDirty(),
        // FlexContainer => nsFlexContainerFrame::MarkIntrinsicISizesDirty
        LayoutFrameType::FlexContainer => nsFlexContainerFrame::from_frame(aFrame).MarkIntrinsicISizesDirty(),
        // GridContainer => nsGridContainerFrame::MarkIntrinsicISizesDirty
        LayoutFrameType::GridContainer => nsGridContainerFrame::from_frame(aFrame).MarkIntrinsicISizesDirty(),
        // Text => nsTextFrame::MarkIntrinsicISizesDirty
        LayoutFrameType::Text if aFrame.UsesRealTextFrame() => nsTextFrame::from_frame(aFrame).MarkIntrinsicISizesDirty(),
        _ => {}
    }
}

// Gecko: layout/base/PresShell.cpp PresShell::FrameNeedsReflow
// ADAPTER: there is no PresShell: the dirty roots are not kept (the whole tree is reflowed from the
// viewport, and the frame classes skip what is clean), and there are no reflow roots (no frame class
// here sets NS_FRAME_REFLOW_ROOT or NS_FRAME_DYNAMIC_REFLOW_ROOT), so ReflowRootHandling does not
// matter. NOT PORTED: mDidInitialize, mIsDestroying and the reflow-time assertions.
pub fn PresShell_FrameNeedsReflow(aFrame: nsIFrame, aIntrinsicDirty: IntrinsicDirty, aBitToAdd: nsFrameState) {
    MOZ_ASSERT!(aBitToAdd == NS_FRAME_IS_DIRTY || aBitToAdd == NS_FRAME_HAS_DIRTY_CHILDREN || aBitToAdd == 0, "Unexpected bits being added");

    let mut subtrees: Vec<nsIFrame> = vec![aFrame];

    while let Some(subtreeRoot) = subtrees.pop() {
        // Grab |wasDirty| now so we can go ahead and update the bits on
        // subtreeRoot.
        // (wasDirty decides whether subtreeRoot becomes a dirty root: there are none here)
        subtreeRoot.AddStateBits(aBitToAdd);

        // Mark the intrinsic widths as dirty on the frame, all of its ancestors,
        // and all of its descendants, if needed:

        if aIntrinsicDirty != IntrinsicDirty::None {
            // Mark argument and all ancestors dirty. (Unless we hit a reflow root
            // that should contain the reflow.
            let mut a = Some(subtreeRoot);
            while let Some(fr) = a {
                MarkIntrinsicISizesDirty(fr);
                if fr.IsAbsolutelyPositioned(None) {
                    // If we get here, 'a' is abspos, so its subtree's intrinsic sizing
                    // has no effect on its ancestors' intrinsic sizing. So, don't loop
                    // upwards any further.
                    break;
                }
                a = fr.GetParent();
            }
        }

        let frameAncestorAndDescendantISizesDirty = aIntrinsicDirty == IntrinsicDirty::FrameAncestorsAndDescendants;
        let dirty = aBitToAdd == NS_FRAME_IS_DIRTY;
        if frameAncestorAndDescendantISizesDirty || dirty {
            // Mark all descendants dirty (using an nsTArray stack rather than
            // recursion).
            // Note that ReflowInput::InitResizeFlags has some similar
            // code; see comments there for how and why it differs.
            let mut stack = vec![subtreeRoot];
            while let Some(f) = stack.pop() {
                if frameAncestorAndDescendantISizesDirty && f.IsPlaceholderFrame() {
                    // Call `GetOutOfFlowFrame` directly because we can get here from
                    // frame destruction and the placeholder might be already torn down.
                    if let Some(oof) = nsPlaceholderFrame::from_frame(f).GetOutOfFlowFrame() {
                        if !nsLayoutUtils::IsProperAncestorFrame(subtreeRoot, oof) {
                            // We have another distinct subtree we need to mark.
                            subtrees.push(oof);
                        }
                    }
                }

                let mut lists = Vec::new();
                f.GetChildLists(&mut lists);
                for l in lists {
                    let mut kid = l.mList.FirstChild();
                    while let Some(k) = kid {
                        if frameAncestorAndDescendantISizesDirty {
                            MarkIntrinsicISizesDirty(k);
                        }
                        if dirty {
                            k.AddStateBits(NS_FRAME_IS_DIRTY);
                        }
                        stack.push(k);
                        kid = k.GetNextSibling();
                    }
                }
            }
        }

        // Skip setting dirty bits up the tree if we weren't given a bit to add.
        if aBitToAdd == 0 {
            continue;
        }

        // Set NS_FRAME_HAS_DIRTY_CHILDREN bits (via nsIFrame::ChildIsDirty)
        // up the tree until we reach either a frame that's already dirty or
        // a reflow root.
        let mut f = subtreeRoot;
        loop {
            let Some(parent) = f.GetParent() else {
                // we've hit a reflow root or the root frame
                // (mDirtyRoots.Add(f): the reflow starts at the viewport)
                break;
            };

            let child = f;
            f = parent;
            let wasDirty = f.HasAnyStateBits(NS_FRAME_IS_DIRTY | NS_FRAME_HAS_DIRTY_CHILDREN);
            f.ChildIsDirty(child);
            NS_ASSERTION!(f.HasAnyStateBits(NS_FRAME_HAS_DIRTY_CHILDREN), "ChildIsDirty didn't do its job");
            if wasDirty {
                // This frame was already marked dirty.
                break;
            }
        }
    }
}

// Gecko: layout/generic/nsContainerFrame.h nsContainerFrame
// ADAPTER: a typed handle for `nsContainerFrame*`. The members are methods of nsIFrame (see the
// top of the file); this type has the static members and the descriptors, and it derefs to the
// frame like every class handle does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsContainerFrame(pub nsIFrame);

impl Deref for nsContainerFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/generic/nsContainerFrame.h NS_DECL_ABSTRACT_FRAME(nsContainerFrame) /
// NS_QUERYFRAME_HEAD(nsContainerFrame) in nsContainerFrame.cpp
impl nsContainerFrame {
    // static_cast<nsContainerFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsContainerFrame {
        nsContainerFrame(aFrame)
    }

    // do_QueryFrame<nsContainerFrame>(frame): every frame class that has child lists. The classes
    // that do not are the leaf frames: text, br, wbr, placeholders, page breaks and the simple
    // XUL leaves.
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsContainerFrame> {
        match aFrame.Type() {
            LayoutFrameType::None
            | LayoutFrameType::Text
            | LayoutFrameType::Br
            | LayoutFrameType::Wbr
            | LayoutFrameType::Placeholder
            | LayoutFrameType::PageBreak
            | LayoutFrameType::SimpleXULLeaf => None,
            _ => Some(nsContainerFrame(aFrame)),
        }
    }
}

// Gecko: layout/generic/nsContainerFrame.h ChildFrameMerger
pub type nsContainerFrame_ChildFrameMerger = fn(&mut nsFrameList, &mut nsFrameList, nsIFrame);

// Gecko: layout/generic/nsIFrame.h nsIFrame::ReflowChildFlags
// Option flags for ReflowChild(), FinishReflowChild()
MOZ_ENUM_CLASS_BITS! {
    // Gecko: layout/generic/nsIFrame.h nsIFrame::ReflowChildFlags
    pub struct nsIFrame_ReflowChildFlags: u32 {
        Default = 0,

        // Don't move the frame.
        NoMoveFrame = (1 << 0),

        // Only applies to ReflowChild; if true, don't delete the next-in-flow, even
        // if the reflow is fully complete.
        NoDeleteNextInFlowChild = 1 << 1,

        // Only applies to FinishReflowChild.  Tell it to call
        // ApplyRelativePositioning.
        ApplyRelativePositioning = 1 << 2,
    }
}

// Gecko: layout/generic/nsContainerFrame.h NS_DECLARE_FRAME_PROPERTY_FRAMELIST(prop)
// ADAPTER: ids of the frame properties of this file (Gecko uses the address of the descriptor).
const kOverflowPropertyId: u32 = 3000;
const kOverflowContainersPropertyId: u32 = 3001;
const kExcessOverflowContainersPropertyId: u32 = 3002;
const kFirstLetterPropertyId: u32 = 3003;
const kDebugReflowingWithInfiniteISizePropertyId: u32 = 3004;

// Gecko: layout/generic/nsContainerFrame.h FrameListPropertyDescriptor
// (the property holds an `nsFrameList*`; see the top of the file)
pub type nsContainerFrame_FrameListPropertyDescriptor = FramePropertyDescriptor<*mut nsFrameList>;

impl nsContainerFrame {
    // Gecko: layout/generic/nsContainerFrame.h NS_DECLARE_FRAME_PROPERTY_FRAMELIST(OverflowProperty)
    pub fn OverflowProperty() -> nsContainerFrame_FrameListPropertyDescriptor {
        FramePropertyDescriptor::new(kOverflowPropertyId)
    }
    // Gecko: layout/generic/nsContainerFrame.h NS_DECLARE_FRAME_PROPERTY_FRAMELIST(OverflowContainersProperty)
    pub fn OverflowContainersProperty() -> nsContainerFrame_FrameListPropertyDescriptor {
        FramePropertyDescriptor::new(kOverflowContainersPropertyId)
    }
    // Gecko: layout/generic/nsContainerFrame.h NS_DECLARE_FRAME_PROPERTY_FRAMELIST(ExcessOverflowContainersProperty)
    pub fn ExcessOverflowContainersProperty() -> nsContainerFrame_FrameListPropertyDescriptor {
        FramePropertyDescriptor::new(kExcessOverflowContainersPropertyId)
    }

    // Only really used on nsBlockFrame instances, but the caller thinks it could
    // have arbitrary nsContainerFrames.
    // Gecko: layout/generic/nsContainerFrame.h NS_DECLARE_FRAME_PROPERTY_WITHOUT_DTOR(FirstLetterProperty, nsIFrame)
    pub fn FirstLetterProperty() -> FramePropertyDescriptor<nsIFrame> {
        FramePropertyDescriptor::new(kFirstLetterPropertyId)
    }

    // Use this to suppress the ABSURD_SIZE assertions.
    // Gecko: layout/generic/nsContainerFrame.h NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE(DebugReflowingWithInfiniteISize, bool)
    #[cfg(debug_assertions)]
    pub fn DebugReflowingWithInfiniteISize() -> FramePropertyDescriptor<bool> {
        FramePropertyDescriptor::new(kDebugReflowingWithInfiniteISizePropertyId)
    }
}

// ADAPTER: nsIFrame::TakeProperty (nsIFrame.h): removes the property and returns its value.
impl nsIFrame {
    // Gecko: layout/generic/nsIFrame.h nsIFrame::TakeProperty
    pub fn TakeProperty<T: Clone + 'static>(&self, aProperty: FramePropertyDescriptor<T>) -> Option<T> {
        let id = aProperty.mId;
        let value = self.GetProperty(aProperty);
        if value.is_some() {
            self.RemoveProperty(FramePropertyDescriptor::<T>::new(id));
        }
        value
    }
}

// ADAPTER: a handle to a frame's own child list (nsContainerFrame::mFrames). The list lives inside the
// frame's data, which does not move (see FrameStore). The handle holds a raw pointer and makes a
// reference only for the length of one method call (Deref, DerefMut). It replaces a
// `&'static mut nsFrameList`: two of those to the same list can be live at once (a caller keeps one
// while a callee changes the list through another path), which Rust may assume never happens, and
// the optimizer did: release builds of a few tests read a stale list head.
#[derive(Clone, Copy)]
pub struct nsFrameListRef(*mut nsFrameList);

impl std::ops::Deref for nsFrameListRef {
    type Target = nsFrameList;
    fn deref(&self) -> &nsFrameList {
        // SAFETY: the list is in a frame's data, which stays in place; the reference lasts one call
        unsafe { &*self.0 }
    }
}

impl nsFrameListRef {
    pub fn as_ptr(&self) -> *mut nsFrameList {
        self.0
    }
    // ADAPTER: the 'static references of GetChildList and GetWritableChildList (Gecko returns a const
    // reference or a pointer to the member); a caller must not hold one across a change to the list.
    pub fn as_static(&self) -> &'static nsFrameList {
        unsafe { &*self.0 }
    }
    pub fn as_static_mut(&self) -> &'static mut nsFrameList {
        unsafe { &mut *self.0 }
    }
}

impl std::ops::DerefMut for nsFrameListRef {
    fn deref_mut(&mut self) -> &mut nsFrameList {
        // SAFETY: as above
        unsafe { &mut *self.0 }
    }
}

// ADAPTER: nsContainerFrame::mFrames. See the top of the file.
impl nsIFrame {
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::mFrames
    pub fn mFrames(&self) -> nsFrameListRef {
        WithFrameTree(|t| nsFrameListRef(&mut t.mFrames[self.0 as usize].mFrames as *mut nsFrameList))
    }
}

impl nsIFrame {
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::GetContentInsertionFrame
    // (virtual; scroll frames, fieldsets and a few others override it)
    pub fn GetContentInsertionFrame(&self) -> Option<nsIFrame> {
        match self.Type() {
            // TableWrapper => nsTableWrapperFrame::GetContentInsertionFrame, TableCell => nsTableCellFrame::GetContentInsertionFrame
            LayoutFrameType::TableWrapper => nsTableWrapperFrame::from_frame(*self).GetContentInsertionFrame(),
            LayoutFrameType::TableCell => nsTableCellFrame::from_frame(*self).GetContentInsertionFrame(),
            // NOT PORTED: ScrollContainerFrame, nsFieldSetFrame, nsColumnSetFrame, nsComboboxControlFrame,
            // nsListControlFrame, nsTextControlFrame and nsHTMLFramesetFrame (the other overrides).
            _ => Some(*self),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::GetChildList
    // (virtual; nsBlockFrame, the table frames, ... override it and call this)
    pub fn GetChildList(&self, aListID: FrameChildListID) -> &'static nsFrameList {
        match self.Type() {
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).GetChildList(aListID),
            _ => self.nsContainerFrame_GetChildList(aListID),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::GetChildList
    pub fn nsContainerFrame_GetChildList(&self, aListID: FrameChildListID) -> &'static nsFrameList {
        // We only know about the principal child list, the overflow lists,
        // and the backdrop list.
        match aListID {
            FrameChildListID::Principal => self.mFrames().as_static(),
            FrameChildListID::Overflow => match self.GetOverflowFrames() {
                Some(list) => list,
                None => nsFrameList::EmptyList(),
            },
            FrameChildListID::OverflowContainers => match self.GetOverflowContainers() {
                Some(list) => list,
                None => nsFrameList::EmptyList(),
            },
            FrameChildListID::ExcessOverflowContainers => match self.GetExcessOverflowContainers() {
                Some(list) => list,
                None => nsFrameList::EmptyList(),
            },
            // nsSplittableFrame::GetChildList is nsIFrame::GetChildList (the absolute lists).
            _ => self.nsIFrame_GetChildList(aListID),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::GetChildLists
    // (virtual; subclasses add their own lists after calling this)
    pub fn GetChildLists(&self, aLists: &mut Vec<FrameChildList>) {
        match self.Type() {
            // Block => nsBlockFrame::GetChildLists
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).GetChildLists(aLists),
            _ => self.nsContainerFrame_GetChildLists(aLists),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::GetChildLists
    pub fn nsContainerFrame_GetChildLists(&self, aLists: &mut Vec<FrameChildList>) {
        self.mFrames().AppendIfNonempty(aLists, FrameChildListID::Principal);

        // Gecko walks the property list in its own order; the three lists are
        // looked up in a fixed order here.
        if let Some(list) = self.GetOverflowFrames() {
            list.AppendIfNonempty(aLists, FrameChildListID::Overflow);
        }
        if let Some(list) = self.GetOverflowContainers() {
            MOZ_ASSERT!(self.CanContainOverflowContainers(), "found unexpected OverflowContainersProperty");
            list.AppendIfNonempty(aLists, FrameChildListID::OverflowContainers);
        }
        if let Some(list) = self.GetExcessOverflowContainers() {
            MOZ_ASSERT!(self.CanContainOverflowContainers(), "found unexpected ExcessOverflowContainersProperty");
            list.AppendIfNonempty(aLists, FrameChildListID::ExcessOverflowContainers);
        }

        // nsSplittableFrame::GetChildLists is nsIFrame::GetChildLists (the absolute lists).
        self.nsIFrame_GetChildLists(aLists);
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::CanContainOverflowContainers
    // NOT PORTED: overflow containers are never created (fragmentation); only the frame classes
    // that can have them are listed.
    pub fn CanContainOverflowContainers(&self) -> bool {
        matches!(self.Type(), LayoutFrameType::Block | LayoutFrameType::Canvas | LayoutFrameType::Viewport | LayoutFrameType::Page | LayoutFrameType::PageContent | LayoutFrameType::ColumnSet | LayoutFrameType::FlexContainer | LayoutFrameType::GridContainer | LayoutFrameType::TableRowGroup | LayoutFrameType::TableRow | LayoutFrameType::TableCell | LayoutFrameType::Table | LayoutFrameType::TableWrapper)
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::Destroy
    // (virtual; nsInlineFrame and nsBlockFrame override it and call this)
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        match self.Type() {
            // Inline => nsInlineFrame::Destroy
            LayoutFrameType::Inline | LayoutFrameType::Line => nsInlineFrame::from_frame(*self).Destroy(aContext),
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).Destroy(aContext),
            // Table => nsTableFrame::Destroy, TableCol => nsTableColFrame::Destroy, TableColGroup =>
            // nsTableColGroupFrame::Destroy
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).Destroy(aContext),
            LayoutFrameType::TableCol => nsTableColFrame::from_frame(*self).Destroy(aContext),
            LayoutFrameType::TableColGroup => nsTableColGroupFrame::from_frame(*self).Destroy(aContext),
            // TableWrapper, TableRowGroup, TableRow, TableCell => their Destroy
            LayoutFrameType::TableWrapper => nsTableWrapperFrame::from_frame(*self).Destroy(aContext),
            LayoutFrameType::TableRowGroup => nsTableRowGroupFrame::from_frame(*self).Destroy(aContext),
            LayoutFrameType::TableRow => nsTableRowFrame::from_frame(*self).Destroy(aContext),
            LayoutFrameType::TableCell => nsTableCellFrame::from_frame(*self).Destroy(aContext),
            // Text => nsTextFrame::Destroy (nsContinuingTextFrame::Destroy is the same function here)
            LayoutFrameType::Text if self.GetContent().map_or(false, |c| c.IsText()) => nsTextFrame::from_frame(*self).Destroy(aContext),
            // Placeholder => nsPlaceholderFrame::Destroy
            LayoutFrameType::Placeholder => nsPlaceholderFrame::from_frame(*self).Destroy(aContext),
            _ => {
                if nsContainerFrame::do_QueryFrame(*self).is_some() {
                    self.nsContainerFrame_Destroy(aContext)
                } else {
                    self.nsSplittableFrame_Destroy(aContext)
                }
            }
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::Destroy
    pub fn nsContainerFrame_Destroy(&self, aContext: &mut FrameDestroyContext) {
        self.DestroyAbsoluteFrames(aContext);

        // Destroy frames on the principal child list.
        self.mFrames().DestroyFrames(aContext);

        // If we have any IB split siblings, clear their references to us.
        if self.HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT) {
            // NOT PORTED: {ib} splits. The IBSplitSibling and IBSplitPrevSibling properties are
            // set by the frame constructor, which is not translated, so no frame has them.
            // (nsContainerFrame.cpp lines 196-218: clears prevSib's IBSplitSibling and nextSib's
            // IBSplitPrevSibling properties, and the NS_FRAME_PART_OF_IBSPLIT bit in debug builds.)
        }

        // Destroy frames on the auxiliary frame lists and delete the lists.
        // (Gecko first checks which of the three properties the frame has by walking its property
        // list; looking them up one by one does the same.)
        if self.HasProperty(nsContainerFrame::OverflowProperty()) {
            self.SafelyDestroyFrameListProp(aContext, nsContainerFrame::OverflowProperty());
        }

        if self.HasProperty(nsContainerFrame::OverflowContainersProperty()) {
            MOZ_ASSERT!(self.CanContainOverflowContainers(), "this type of frame shouldn't have overflow containers");
            self.SafelyDestroyFrameListProp(aContext, nsContainerFrame::OverflowContainersProperty());
        }
        if self.HasProperty(nsContainerFrame::ExcessOverflowContainersProperty()) {
            MOZ_ASSERT!(self.CanContainOverflowContainers(), "this type of frame shouldn't have overflow containers");
            self.SafelyDestroyFrameListProp(aContext, nsContainerFrame::ExcessOverflowContainersProperty());
        }

        self.nsSplittableFrame_Destroy(aContext);
    }

    // Gecko: layout/generic/nsSplittableFrame.cpp nsSplittableFrame::Destroy (and RemoveFromFlow)
    pub fn nsSplittableFrame_Destroy(&self, aContext: &mut FrameDestroyContext) {
        // Disconnect from the flow list
        if self.GetPrevContinuation().is_some() || self.GetNextContinuation().is_some() {
            nsIFrame::RemoveFromFlow(*self);
        }

        // Let the base class destroy the frame
        self.nsIFrame_Destroy(aContext);
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ChildIsDirty
    // (virtual; nsBlockFrame overrides it)
    /* virtual */
    pub fn ChildIsDirty(&self, aChild: nsIFrame) {
        match self.Type() {
            // Block => nsBlockFrame::ChildIsDirty
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).ChildIsDirty(aChild),
            _ => self.nsContainerFrame_ChildIsDirty(aChild),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ChildIsDirty
    pub fn nsContainerFrame_ChildIsDirty(&self, aChild: nsIFrame) {
        // NS_ASSERTION(aChild->IsSubtreeDirty(), "child isn't actually dirty");
        // NOT PORTED: the assertion (IsSubtreeDirty is not translated; the dirty bits of the
        // child are set by the caller).

        self.AddStateBits(NS_FRAME_HAS_DIRTY_CHILDREN);
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::PeekOffsetNoAmount and
    // PeekOffsetCharacter
    // NOT PORTED: caret and selection movement.

    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::List, ListWithMatchedRules,
    // ListChildLists, ExtraContainerFrameInfo (DEBUG_FRAME_DUMP)
    // NOT PORTED: debug dumps.

    // nsContainerFrame methods

    /**
     * Called to set the initial list of frames. This happens after the frame
     * has been initialized.
     *
     * This is only called once for a given child list, and won't be called
     * at all for child lists with no initial list of frames.
     *
     * @param   aListID the child list identifier.
     * @param   aChildList list of child frames. Each of the frames has its
     *            NS_FRAME_IS_DIRTY bit set.  Must not be empty.
     *            This method cannot handle ChildListID::Absolute frames.
     * @see     #Init()
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::SetInitialChildList
    pub fn SetInitialChildList(&self, aListID: FrameChildListID, aChildList: nsFrameList) {
        match self.Type() {
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).SetInitialChildList(aListID, aChildList),
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).SetInitialChildList(aListID, aChildList),
            LayoutFrameType::TableColGroup => nsTableColGroupFrame::from_frame(*self).SetInitialChildList(aListID, aChildList),
            // NOT PORTED: the overrides of the flex, grid, table, scroll and column frames.
            _ => self.nsContainerFrame_SetInitialChildList(aListID, aChildList),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::SetInitialChildList
    pub fn nsContainerFrame_SetInitialChildList(&self, aListID: FrameChildListID, mut aChildList: nsFrameList) {
        #[cfg(debug_assertions)]
        {
            // nsIFrame::VerifyDirtyBitSet(aChildList);
            for f in &aChildList {
                MOZ_ASSERT!(f.HasAnyStateBits(NS_FRAME_IS_DIRTY), "dirty bit not set");
            }
            for f in &aChildList {
                MOZ_ASSERT!(f.GetParent() == Some(*self), "Unexpected parent");
            }
        }
        if aListID == FrameChildListID::Principal {
            MOZ_ASSERT!(self.mFrames().IsEmpty(), "unexpected second call to SetInitialChildList");
            self.mFrames().operator_assign_move(&mut aChildList);
        } else {
            MOZ_ASSERT_UNREACHABLE!("Unexpected child list");
        }
    }

    /**
     * This method is responsible for appending frames to the frame
     * list.  The implementation should append the frames to the specified
     * child list and then generate a reflow command.
     *
     * @param   aListID the child list identifier.
     * @param   aFrameList list of child frames to append. Each of the frames has
     *            its NS_FRAME_IS_DIRTY bit set.  Must not be empty.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::AppendFrames
    pub fn AppendFrames(&self, aListID: FrameChildListID, aFrameList: nsFrameList) {
        match self.Type() {
            // Inline => nsContainerFrame::AppendFrames (nsInlineFrame does not override it)
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).AppendFrames(aListID, aFrameList),
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).AppendFrames(aListID, aFrameList),
            LayoutFrameType::TableColGroup => nsTableColGroupFrame::from_frame(*self).AppendFrames(aListID, aFrameList),
            LayoutFrameType::TableWrapper => nsTableWrapperFrame::from_frame(*self).AppendFrames(aListID, aFrameList),
            LayoutFrameType::TableRowGroup => nsTableRowGroupFrame::from_frame(*self).AppendFrames(aListID, aFrameList),
            LayoutFrameType::TableRow => nsTableRowFrame::from_frame(*self).AppendFrames(aListID, aFrameList),
            LayoutFrameType::FlexContainer => nsFlexContainerFrame::from_frame(*self).AppendFrames(aListID, aFrameList),
            LayoutFrameType::GridContainer => nsGridContainerFrame::from_frame(*self).AppendFrames(aListID, aFrameList),
            _ => self.nsContainerFrame_AppendFrames(aListID, aFrameList),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::AppendFrames
    pub fn nsContainerFrame_AppendFrames(&self, aListID: FrameChildListID, aFrameList: nsFrameList) {
        MOZ_ASSERT!(aListID == FrameChildListID::Principal || aListID == FrameChildListID::NoReflowPrincipal, "unexpected child list");

        if aFrameList.IsEmpty() {
            return;
        }

        self.DrainSelfOverflowList(); // ensure the last frame is in mFrames
        self.mFrames().AppendFrames(Some(*self), aFrameList);

        if aListID != FrameChildListID::NoReflowPrincipal {
            PresShell_FrameNeedsReflow(*self, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
        }
    }

    /**
     * This method is responsible for inserting frames into the frame
     * list.  The implementation should insert the new frames into the specified
     * child list and then generate a reflow command.
     *
     * @param   aListID the child list identifier.
     * @param   aPrevFrame the frame to insert frames <b>after</b>
     * @param   aPrevFrameLine (optional) if present (i.e., not null), the line
     *            box that aPrevFrame is part of.
     * @param   aFrameList list of child frames to insert <b>after</b> aPrevFrame.
     *            Each of the frames has its NS_FRAME_IS_DIRTY bit set
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::InsertFrames
    pub fn InsertFrames(&self, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aPrevFrameLine: Option<&nsLineList_iterator>, aFrameList: nsFrameList) {
        match self.Type() {
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList),
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList),
            LayoutFrameType::TableColGroup => nsTableColGroupFrame::from_frame(*self).InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList),
            LayoutFrameType::TableWrapper => nsTableWrapperFrame::from_frame(*self).InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList),
            LayoutFrameType::TableRowGroup => nsTableRowGroupFrame::from_frame(*self).InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList),
            LayoutFrameType::TableRow => nsTableRowFrame::from_frame(*self).InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList),
            LayoutFrameType::FlexContainer => nsFlexContainerFrame::from_frame(*self).InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList),
            LayoutFrameType::GridContainer => nsGridContainerFrame::from_frame(*self).InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList),
            _ => self.nsContainerFrame_InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::InsertFrames
    pub fn nsContainerFrame_InsertFrames(&self, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aPrevFrameLine: Option<&nsLineList_iterator>, aFrameList: nsFrameList) {
        MOZ_ASSERT!(aListID == FrameChildListID::Principal || aListID == FrameChildListID::NoReflowPrincipal, "unexpected child list");
        NS_ASSERTION!(aPrevFrame.is_none() || aPrevFrame.unwrap().GetParent() == Some(*self), "inserting after sibling frame with different parent");

        if aFrameList.IsEmpty() {
            return;
        }

        self.DrainSelfOverflowList(); // ensure aPrevFrame is in mFrames
        self.mFrames().InsertFrames(Some(*self), aPrevFrame, aFrameList);

        if aListID != FrameChildListID::NoReflowPrincipal {
            PresShell_FrameNeedsReflow(*self, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
        }
    }

    /**
     * This method is responsible for removing a frame in the frame
     * list.  The implementation should do something with the removed frame
     * and then generate a reflow command. The implementation is responsible
     * for destroying the frame (the caller mustn't destroy it).
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::RemoveFrame
    pub fn RemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        match self.Type() {
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).RemoveFrame(aContext, aListID, aOldFrame),
            LayoutFrameType::Table => nsTableFrame::from_frame(*self).RemoveFrame(aContext, aListID, aOldFrame),
            LayoutFrameType::TableColGroup => nsTableColGroupFrame::from_frame(*self).RemoveFrame(aContext, aListID, aOldFrame),
            LayoutFrameType::TableWrapper => nsTableWrapperFrame::from_frame(*self).RemoveFrame(aContext, aListID, aOldFrame),
            LayoutFrameType::TableRowGroup => nsTableRowGroupFrame::from_frame(*self).RemoveFrame(aContext, aListID, aOldFrame),
            LayoutFrameType::TableRow => nsTableRowFrame::from_frame(*self).RemoveFrame(aContext, aListID, aOldFrame),
            LayoutFrameType::FlexContainer => nsFlexContainerFrame::from_frame(*self).RemoveFrame(aContext, aListID, aOldFrame),
            LayoutFrameType::GridContainer => nsGridContainerFrame::from_frame(*self).RemoveFrame(aContext, aListID, aOldFrame),
            _ => self.nsContainerFrame_RemoveFrame(aContext, aListID, aOldFrame),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::RemoveFrame
    pub fn nsContainerFrame_RemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        MOZ_ASSERT!(aListID == FrameChildListID::Principal || aListID == FrameChildListID::NoReflowPrincipal, "unexpected child list");

        let mut continuations: Vec<nsIFrame> = Vec::new();
        {
            let mut continuation = Some(aOldFrame);
            while let Some(c) = continuation {
                continuations.push(c);
                continuation = c.GetNextContinuation();
            }
        }

        let mut lastParent: Option<nsIFrame> = None;

        // Loop and destroy aOldFrame and all of its continuations.
        //
        // Request a reflow on the parent frames involved unless we were explicitly
        // told not to (FrameChildListID::NoReflowPrincipal).
        let generateReflowCommand = aListID != FrameChildListID::NoReflowPrincipal;
        for continuation in continuations.into_iter().rev() {
            let parent = continuation.GetParent().unwrap();

            // Please note that 'parent' may not actually be where 'continuation' lives.
            // We really MUST use StealFrame() and nothing else here.
            // @see nsInlineFrame::StealFrame for details.
            parent.StealFrame(continuation);
            continuation.Destroy(aContext);
            if generateReflowCommand && Some(parent) != lastParent {
                PresShell_FrameNeedsReflow(parent, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
                lastParent = Some(parent);
            }
        }
    }

    /**
     * Helper method to create next-in-flows if necessary. If aFrame
     * already has a next-in-flow then this method does
     * nothing. Otherwise, a new continuation frame is created and
     * linked into the flow. In addition, the new frame is inserted
     * into the principal child list after aFrame.
     * @note calling this method on a block frame is illegal. Use
     * nsBlockFrame::CreateContinuationFor() instead.
     * @return the next-in-flow <b>if and only if</b> one is created. If
     *         a next-in-flow already exists, nullptr will be returned.
     */
    /*
     * Create a next-in-flow for aFrame. Will return the newly created
     * frame <b>if and only if</b> a new frame is created; otherwise
     * nullptr is returned.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::CreateNextInFlow
    pub fn CreateNextInFlow(&self, aFrame: nsIFrame) -> Option<nsIFrame> {
        MOZ_ASSERT!(!self.IsBlockFrame(), "you should have called nsBlockFrame::CreateContinuationFor instead");
        MOZ_ASSERT!(self.mFrames().ContainsFrame(aFrame), "expected an in-flow child frame");

        let mut nextInFlow = aFrame.GetNextInFlow();
        if nextInFlow.is_none() {
            // Create a continuation frame for the child frame and insert it
            // into our child list.
            let created = nsCSSFrameConstructor_CreateContinuingFrame(aFrame, *self, true);
            nextInFlow = Some(created);
            self.mFrames().InsertFrame(None, Some(aFrame), created);

            // NS_FRAME_LOG(NS_FRAME_TRACE_NEW_FRAMES, ...): NOT PORTED, frame logging.

            return nextInFlow;
        }
        None
    }

    /**
     * Delete aNextInFlow and its next-in-flows.
     * @param aDeletingEmptyFrames if set, then the reflow for aNextInFlow's
     * content was complete before aNextInFlow, so aNextInFlow and its
     * next-in-flows no longer map any real content.
     */
    /**
     * Remove and delete aNextInFlow and its next-in-flows. Updates the sibling and
     * flow pointers
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::DeleteNextInFlowChild
    // (virtual; nsBlockFrame overrides it)
    pub fn DeleteNextInFlowChild(&self, aContext: &mut FrameDestroyContext, aNextInFlow: nsIFrame, aDeletingEmptyFrames: bool) {
        match self.Type() {
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).DeleteNextInFlowChild(aContext, aNextInFlow, aDeletingEmptyFrames),
            _ => self.nsContainerFrame_DeleteNextInFlowChild(aContext, aNextInFlow, aDeletingEmptyFrames),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::DeleteNextInFlowChild
    pub fn nsContainerFrame_DeleteNextInFlowChild(&self, aContext: &mut FrameDestroyContext, aNextInFlow: nsIFrame, aDeletingEmptyFrames: bool) {
        // (not under cfg(debug_assertions): MOZ_ASSERT! type-checks its condition in release builds too)
        let prevInFlow = aNextInFlow.GetPrevInFlow();
        MOZ_ASSERT!(prevInFlow.is_some(), "bad prev-in-flow");

        // If the next-in-flow has a next-in-flow then delete it, too (and
        // delete it first).
        // Do this in a loop so we don't overflow the stack for frames
        // with very many next-in-flows
        let nextNextInFlow = aNextInFlow.GetNextInFlow();
        if let Some(nextNextInFlow) = nextNextInFlow {
            let mut frames: Vec<nsIFrame> = Vec::new();
            let mut f = Some(nextNextInFlow);
            while let Some(fr) = f {
                frames.push(fr);
                f = fr.GetNextInFlow();
            }
            for delFrame in frames.into_iter().rev() {
                let parent = delFrame.GetParent().unwrap();
                parent.DeleteNextInFlowChild(aContext, delFrame, aDeletingEmptyFrames);
            }
        }

        // Take the next-in-flow out of the parent's child list
        self.StealFrame(aNextInFlow);

        // #ifdef DEBUG
        //   if (aDeletingEmptyFrames) {
        //     nsLayoutUtils::AssertTreeOnlyEmptyNextInFlows(aNextInFlow);
        //   }
        // #endif
        // NOT PORTED: the debug check (nsLayoutUtils::AssertTreeOnlyEmptyNextInFlows).

        // Delete the next-in-flow frame and its descendants. This will also
        // remove it from its next-in-flow/prev-in-flow chain.
        aNextInFlow.Destroy(aContext);

        #[cfg(debug_assertions)]
        MOZ_ASSERT!(prevInFlow.unwrap().GetNextInFlow().is_none(), "non null next-in-flow");
    }
}

impl nsContainerFrame {
    /**
     * Reparent aFrame from aOldParent to aNewParent.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ReparentFrame
    pub fn ReparentFrame(aFrame: nsIFrame, aOldParent: nsIFrame, aNewParent: nsIFrame) {
        NS_ASSERTION!(Some(aOldParent) == aFrame.GetParent(), "Parent not consistent with expectations");

        aFrame.SetParent(aNewParent);
    }

    /**
     * Reparent all the frames in aFrameList from aOldParent to aNewParent.
     *
     * Note: Reparenting a large frame list can be have huge performance impact.
     * For example, instead of using this method, nsInlineFrame uses a "lazy
     * reparenting" technique that it reparents a child frame just before
     * reflowing the child. (See InlineReflowInput::mSetParentDuringReflow.)
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ReparentFrames
    pub fn ReparentFrames(aFrameList: &nsFrameList, aOldParent: nsIFrame, aNewParent: nsIFrame) {
        for f in aFrameList {
            nsContainerFrame::ReparentFrame(f, aOldParent, aNewParent);
        }
    }

    /*
     * Converts the minimum and maximum sizes given in inner window app units to
     * outer window device pixel sizes and assigns these constraints to the
     * widget.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::SetSizeConstraints
    // NOT PORTED: widgets (nsIWidget::SetSizeConstraints); there are no windows.
}

// Inside a subclass of nsContainerFrame, C++ calls the static members unqualified
// (`ReparentFrame(aFrame, aOld, aNew)`); translated subclass code writes `self.ReparentFrame(...)`,
// which these methods answer. Outside a subclass, call `nsContainerFrame::ReparentFrame` directly.
impl nsIFrame {
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ReparentFrame
    pub fn ReparentFrame(&self, aFrame: nsIFrame, aOldParent: nsIFrame, aNewParent: nsIFrame) {
        nsContainerFrame::ReparentFrame(aFrame, aOldParent, aNewParent)
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ReparentFrames
    pub fn ReparentFrames(&self, aFrameList: &nsFrameList, aOldParent: nsIFrame, aNewParent: nsIFrame) {
        nsContainerFrame::ReparentFrames(aFrameList, aOldParent, aNewParent)
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::MergeSortedFrameLists
    pub fn MergeSortedFrameLists(&self, aDest: &mut nsFrameList, aSrc: &mut nsFrameList, aCommonAncestor: Option<nsIContent>) {
        nsContainerFrame::MergeSortedFrameLists(aDest, aSrc, aCommonAncestor)
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ReparentFloatsForInlineChild
    pub fn ReparentFloatsForInlineChild(&self, aOurLineContainer: nsIFrame, aFrame: Option<nsIFrame>, aReparentSiblings: bool) {
        nsContainerFrame::ReparentFloatsForInlineChild(aOurLineContainer, aFrame, aReparentSiblings)
    }
}

// ---------------------------------------------------------------------------------------------
// Intrinsic inline-size data (layout/generic/nsIFrame.h), used by DoInlineIntrinsicISize.
// ---------------------------------------------------------------------------------------------

/**
 * |InlineIntrinsicISize| represents the intrinsic inline size information
 * in inline layout.  Code that determines the intrinsic inline size of a
 * region of inline layout accumulates the result into this structure.
 * This pattern is needed because we need to maintain state
 * information about whitespace (for both collapsing and trimming).
 */
// Gecko: layout/generic/nsIFrame.h nsIFrame::InlineIntrinsicISizeData
#[derive(Clone)]
pub struct nsIFrame_InlineIntrinsicISizeData {
    // The line. This may be null if the inlines are not associated with
    // a block or if we just don't know the line.
    pub mLine: Option<nsLineList_iterator>,

    // The line container. Private, to ensure we always use SetLineContainer
    // to update it.
    //
    // Note that nsContainerFrame::DoInlineIntrinsicISize will clear the
    // |mLine| and |mLineContainer| fields when following a next-in-flow link,
    // so we must not assume these can always be dereferenced.
    mLineContainer: Option<nsIFrame>,

    // The max-content intrinsic inline size for all previous lines.
    pub mPrevLines: nscoord,

    // The max-content intrinsic inline size for the current line.  At a line
    // break (mandatory for max-content inline size; allowed for min-content
    // inline size), the caller should call |Break()|.
    pub mCurrentLine: nscoord,

    // This contains the inline size of the trimmable whitespace at the end of
    // |mCurrentLine|; it is zero if there is no such whitespace.
    pub mTrailingWhitespace: nscoord,

    // True if initial collapsable whitespace should be skipped.  This
    // should be true at the beginning of a block, after hard breaks
    // and when the last text ended with whitespace.
    pub mSkipWhitespace: bool,

    pub mFloats: Vec<nsIFrame_InlineIntrinsicISizeData_FloatInfo>,
}

impl Default for nsIFrame_InlineIntrinsicISizeData {
    fn default() -> Self {
        nsIFrame_InlineIntrinsicISizeData { mLine: None, mLineContainer: None, mPrevLines: 0, mCurrentLine: 0, mTrailingWhitespace: 0, mSkipWhitespace: true, mFloats: Vec::new() }
    }
}

impl nsIFrame_InlineIntrinsicISizeData {
    // Setter and getter for the lineContainer field:
    // Gecko: layout/generic/nsIFrame.h nsIFrame::InlineIntrinsicISizeData::SetLineContainer
    pub fn SetLineContainer(&mut self, aLineContainer: Option<nsIFrame>) {
        self.mLineContainer = aLineContainer;
    }
    // Gecko: layout/generic/nsIFrame.h nsIFrame::InlineIntrinsicISizeData::LineContainer
    pub fn LineContainer(&self) -> Option<nsIFrame> {
        self.mLineContainer
    }
}

// Floats encountered in the lines.
// Gecko: layout/generic/nsIFrame.h nsIFrame::InlineIntrinsicISizeData::FloatInfo
#[derive(Clone, Copy, Debug)]
pub struct nsIFrame_InlineIntrinsicISizeData_FloatInfo {
    mFrame: nsIFrame,
    mISize: nscoord,
}

impl nsIFrame_InlineIntrinsicISizeData_FloatInfo {
    // Gecko: layout/generic/nsIFrame.h FloatInfo::FloatInfo
    pub fn new(aFrame: nsIFrame, aISize: nscoord) -> Self {
        nsIFrame_InlineIntrinsicISizeData_FloatInfo { mFrame: aFrame, mISize: aISize }
    }
    // Gecko: layout/generic/nsIFrame.h FloatInfo::Frame
    pub fn Frame(&self) -> nsIFrame {
        self.mFrame
    }
    // Gecko: layout/generic/nsIFrame.h FloatInfo::ISize
    pub fn ISize(&self) -> nscoord {
        self.mISize
    }
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::InlineMinISizeData
#[derive(Clone)]
pub struct nsIFrame_InlineMinISizeData {
    base: nsIFrame_InlineIntrinsicISizeData,
    // Whether we're currently at the start of the line.  If we are, we
    // can't break (for example, between the text-indent and the first
    // word).
    pub mAtStartOfLine: bool,
}

// Inside a subclass of nsIFrame, C++ names the nested type without its qualifier.
pub type InlineMinISizeData = nsIFrame_InlineMinISizeData;

impl Default for nsIFrame_InlineMinISizeData {
    fn default() -> Self {
        nsIFrame_InlineMinISizeData { base: nsIFrame_InlineIntrinsicISizeData::default(), mAtStartOfLine: true }
    }
}

impl Deref for nsIFrame_InlineMinISizeData {
    type Target = nsIFrame_InlineIntrinsicISizeData;
    fn deref(&self) -> &nsIFrame_InlineIntrinsicISizeData {
        &self.base
    }
}

impl DerefMut for nsIFrame_InlineMinISizeData {
    fn deref_mut(&mut self) -> &mut nsIFrame_InlineIntrinsicISizeData {
        &mut self.base
    }
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::InlinePrefISizeData
#[derive(Clone)]
pub struct nsIFrame_InlinePrefISizeData {
    base: nsIFrame_InlineIntrinsicISizeData,
    // True if the current line contains nothing other than placeholders.
    pub mLineIsEmpty: bool,
}

pub type InlinePrefISizeData = nsIFrame_InlinePrefISizeData;

impl Default for nsIFrame_InlinePrefISizeData {
    fn default() -> Self {
        nsIFrame_InlinePrefISizeData { base: nsIFrame_InlineIntrinsicISizeData::default(), mLineIsEmpty: true }
    }
}

impl Deref for nsIFrame_InlinePrefISizeData {
    type Target = nsIFrame_InlineIntrinsicISizeData;
    fn deref(&self) -> &nsIFrame_InlineIntrinsicISizeData {
        &self.base
    }
}

impl DerefMut for nsIFrame_InlinePrefISizeData {
    fn deref_mut(&mut self) -> &mut nsIFrame_InlineIntrinsicISizeData {
        &mut self.base
    }
}

impl nsIFrame_InlineMinISizeData {
    // The default implementation for nsIFrame::AddInlineMinISize.
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::InlineMinISizeData::DefaultAddInlineMinISize
    pub fn DefaultAddInlineMinISize(&mut self, aFrame: nsIFrame, aISize: nscoord, aAllowBreak: bool) {
        let parent = aFrame.GetParent();
        MOZ_ASSERT!(parent.is_some(), "Must have a parent if we get here!");
        let parent = parent.unwrap();
        let mayBreak = aAllowBreak && !aFrame.CanContinueTextRun() && !parent.Style().ShouldSuppressLineBreak() && nsStyleTextAdapter::of(parent).WhiteSpaceCanWrap(parent);
        if mayBreak {
            self.OptionallyBreak(0);
        }
        self.mTrailingWhitespace = 0;
        self.mSkipWhitespace = false;
        self.mCurrentLine += aISize;
        self.mAtStartOfLine = false;
        if mayBreak {
            self.OptionallyBreak(0);
        }
    }

    // We need to distinguish forced and optional breaks for cases where the
    // current line total is negative. When it is, we need to ignore optional
    // breaks to prevent min-content inline size from ending up bigger than
    // max-content inline size.
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::InlineMinISizeData::ForceBreak
    pub fn ForceBreak(&mut self) {
        self.mCurrentLine -= self.mTrailingWhitespace;
        self.mPrevLines = std::cmp::max(self.mPrevLines, self.mCurrentLine);
        self.mCurrentLine = 0;
        self.mTrailingWhitespace = 0;

        for floatInfo in &self.base.mFloats {
            self.base.mPrevLines = std::cmp::max(floatInfo.ISize(), self.base.mPrevLines);
        }
        self.base.mFloats.clear();
        self.mSkipWhitespace = true;
    }

    // If the break here is actually taken, aHyphenWidth must be added to the
    // width of the current line.
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::InlineMinISizeData::OptionallyBreak
    pub fn OptionallyBreak(&mut self, aHyphenWidth: nscoord) {
        // If we can fit more content into a smaller width by staying on this
        // line (because we're still at a negative offset due to negative
        // text-indent or negative margin), don't break.  Otherwise, do the
        // same as ForceBreak.  it doesn't really matter when we accumulate
        // floats.
        if self.mCurrentLine + aHyphenWidth < 0 || self.mAtStartOfLine {
            return;
        }
        self.mCurrentLine += aHyphenWidth;
        self.ForceBreak();
    }
}

impl nsIFrame_InlinePrefISizeData {
    /**
     * Finish the current line and start a new line.
     *
     * @param aClearType controls whether isize of floats are considered
     * and what floats are kept for the next line:
     *  * |None| skips handling floats, which means no floats are
     *    removed, and isizes of floats are not considered either.
     *  * |Both| takes floats into consideration when computing isize
     *    of the current line, and removes all floats after that.
     *  * |Left| and |Right| do the same as |Both| except that they only
     *    remove floats on the given side, and any floats on the other
     *    side that are prior to a float on the given side that has a
     *    'clear' property that clears them.
     */
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::InlinePrefISizeData::ForceBreak
    // (the C++ default for aClearType is UsedClear::Both)
    pub fn ForceBreak(&mut self, aClearType: UsedClear) {
        // If this force break is not clearing any float, we can leave all the
        // floats to the next force break.
        if !self.mFloats.is_empty() && aClearType != UsedClear::None {
            // Preferred isize accumulated for floats that have already
            // been cleared past
            let mut floatsDone: nscoord = 0;
            // Preferred isize accumulated for floats that have not yet
            // been cleared past
            let mut floatsCurLeft: nscoord = 0;
            let mut floatsCurRight: nscoord = 0;

            for floatInfo in &self.base.mFloats {
                let floatDisp = floatInfo.Frame().StyleDisplay();
                let cbWM = floatInfo.Frame().GetParent().unwrap().GetWritingMode();
                let clearType = floatDisp.UsedClear(cbWM);
                if clearType == UsedClear::Left || clearType == UsedClear::Right || clearType == UsedClear::Both {
                    let floatsCur = NSCoordSaturatingAdd(floatsCurLeft, floatsCurRight);
                    if floatsCur > floatsDone {
                        floatsDone = floatsCur;
                    }
                    if clearType != UsedClear::Right {
                        floatsCurLeft = 0;
                    }
                    if clearType != UsedClear::Left {
                        floatsCurRight = 0;
                    }
                }

                let floatStyle = floatDisp.UsedFloat(cbWM);
                let floatsCur = if floatStyle == UsedFloat::Left { &mut floatsCurLeft } else { &mut floatsCurRight };
                let floatISize = floatInfo.ISize();
                // Negative-width floats don't change the available space so they
                // shouldn't change our intrinsic line isize either.
                *floatsCur = NSCoordSaturatingAdd(*floatsCur, std::cmp::max(0, floatISize));
            }

            let floatsCur = NSCoordSaturatingAdd(floatsCurLeft, floatsCurRight);
            if floatsCur > floatsDone {
                floatsDone = floatsCur;
            }

            self.base.mCurrentLine = NSCoordSaturatingAdd(self.base.mCurrentLine, floatsDone);

            if aClearType == UsedClear::Both {
                self.base.mFloats.clear();
            } else {
                // If the break type does not clear all floats, it means there may
                // be some floats whose isize should contribute to the intrinsic
                // isize of the next line. The code here scans the current mFloats
                // and keeps floats which are not cleared by this break. Note that
                // floats may be cleared directly or indirectly. See below.
                let mut newFloats: Vec<nsIFrame_InlineIntrinsicISizeData_FloatInfo> = Vec::new();
                MOZ_ASSERT!(aClearType == UsedClear::Left || aClearType == UsedClear::Right, "Other values should have been handled in other branches");
                let clearFloatType = if aClearType == UsedClear::Left { UsedFloat::Left } else { UsedFloat::Right };
                // Iterate the array in reverse so that we can stop when there are
                // no longer any floats we need to keep. See below.
                for floatInfo in self.base.mFloats.iter().rev() {
                    let floatDisp = floatInfo.Frame().StyleDisplay();
                    let cbWM = floatInfo.Frame().GetParent().unwrap().GetWritingMode();
                    if floatDisp.UsedFloat(cbWM) != clearFloatType {
                        newFloats.push(*floatInfo);
                    } else {
                        // This is a float on the side that this break directly clears
                        // which means we're not keeping it in mFloats. However, if
                        // this float clears floats on the opposite side (via a value
                        // of either 'both' or one of 'left'/'right'), any remaining
                        // (earlier) floats on that side would be indirectly cleared
                        // as well. Thus, we should break out of this loop and stop
                        // considering earlier floats to be kept in mFloats.
                        let clearType = floatDisp.UsedClear(cbWM);
                        if clearType != aClearType && clearType != UsedClear::None {
                            break;
                        }
                    }
                }
                newFloats.reverse();
                self.base.mFloats = newFloats;
            }
        }

        self.base.mCurrentLine = NSCoordSaturatingSubtract(self.base.mCurrentLine, self.base.mTrailingWhitespace, nscoord_MAX);
        self.base.mPrevLines = std::cmp::max(self.base.mPrevLines, self.base.mCurrentLine);
        self.base.mCurrentLine = 0;
        self.base.mTrailingWhitespace = 0;
        self.base.mSkipWhitespace = true;
        self.mLineIsEmpty = true;
    }

    // The default implementation for nsIFrame::AddInlinePrefISize.
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::InlinePrefISizeData::DefaultAddInlinePrefISize
    pub fn DefaultAddInlinePrefISize(&mut self, aISize: nscoord) {
        self.base.mCurrentLine = NSCoordSaturatingAdd(self.base.mCurrentLine, aISize);
        self.base.mTrailingWhitespace = 0;
        self.base.mSkipWhitespace = false;
        self.mLineIsEmpty = false;
    }
}

impl nsIFrame {
    /**
     * Add the min-content intrinsic inline size of a frame in a way suitable for
     * use in inline layout to an |InlineMinISizeData| object that
     * represents the intrinsic inline size information of all the previous
     * frames in the inline layout region.
     *
     * All *allowed* breakpoints within the frame determine what counts as
     * a line for the |InlineMinISizeData|.  This means that
     * |aData->mTrailingWhitespace| will always be zero (unlike for
     * AddInlinePrefISize).
     *
     * All the comments for |GetMinISize| apply, except that this function
     * is responsible for adding padding, border, and margin and for
     * considering the effects of 'inline-size', 'min-inline-size', and
     * 'max-inline-size'.
     *
     * This may be called on any frame.  Frames that do not participate in
     * line breaking can inherit the default implementation on nsIFrame,
     * which calls |GetMinISize|.
     */
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::AddInlineMinISize (virtual)
    pub fn AddInlineMinISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlineMinISizeData) {
        match self.Type() {
            // Inline => nsInlineFrame::AddInlineMinISize
            LayoutFrameType::Inline | LayoutFrameType::Line => nsInlineFrame::from_frame(*self).AddInlineMinISize(aInput, aData),
            // Text => nsTextFrame::AddInlineMinISize (TEMP: the TextFrameStub, see the adapter)
            LayoutFrameType::Text => self.nsTextFrame_AddInlineMinISize(aInput, aData),
            // Placeholder => nsPlaceholderFrame::AddInlineMinISize
            LayoutFrameType::Placeholder => self.nsPlaceholderFrame_AddInlineMinISize(aInput, aData),
            // Br => BRFrame::AddInlineMinISize
            LayoutFrameType::Br => BRFrame::from_frame(*self).AddInlineMinISize(aInput, aData),
            // NOT PORTED: the overrides of nsTextFrame, nsBRFrame, nsPlaceholderFrame,
            // nsFirstLetterFrame, nsRubyFrame, nsRubyBaseContainerFrame, nsImageFrame, ...
            _ => self.nsIFrame_AddInlineMinISize(aInput, aData),
        }
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::AddInlineMinISize
    pub fn nsIFrame_AddInlineMinISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlineMinISizeData) {
        // Note: we are one of the children that mPercentageBasisForChildren was
        // prepared for (i.e. our parent frame prepares the percentage basis for us,
        // not for our own children). Hence it's fine that we're resolving our
        // percentages sizes against this basis in IntrinsicForContainer().
        let isize = nsLayoutUtils::IntrinsicForContainer(aInput.mContext, *self, IntrinsicISizeType::MinISize, aInput.mPercentageBasisForChildren, 0, &StyleSizeOverrides::default());
        aData.DefaultAddInlineMinISize(*self, isize, true);
    }

    /**
     * Add the max-content intrinsic inline size of a frame in a way suitable for
     * use in inline layout to an |InlinePrefISizeData| object that
     * represents the intrinsic inline size information of all the previous
     * frames in the inline layout region.
     *
     * All the comments for |AddInlineMinISize| and |GetPrefISize| apply,
     * except that this fills in an |InlinePrefISizeData| structure
     * based on using all *mandatory* breakpoints within the frame.
     */
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::AddInlinePrefISize (virtual)
    pub fn AddInlinePrefISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlinePrefISizeData) {
        match self.Type() {
            // Inline => nsInlineFrame::AddInlinePrefISize
            LayoutFrameType::Inline | LayoutFrameType::Line => nsInlineFrame::from_frame(*self).AddInlinePrefISize(aInput, aData),
            // Text => nsTextFrame::AddInlinePrefISize (TEMP: the TextFrameStub, see the adapter)
            LayoutFrameType::Text => self.nsTextFrame_AddInlinePrefISize(aInput, aData),
            // Placeholder => nsPlaceholderFrame::AddInlinePrefISize
            LayoutFrameType::Placeholder => self.nsPlaceholderFrame_AddInlinePrefISize(aInput, aData),
            // Br => BRFrame::AddInlinePrefISize
            LayoutFrameType::Br => BRFrame::from_frame(*self).AddInlinePrefISize(aInput, aData),
            // NOT PORTED: the overrides listed in AddInlineMinISize.
            _ => self.nsIFrame_AddInlinePrefISize(aInput, aData),
        }
    }

    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::AddInlinePrefISize
    pub fn nsIFrame_AddInlinePrefISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlinePrefISizeData) {
        // Note: we are one of the children that mPercentageBasisForChildren was
        // prepared for (i.e. our parent frame prepares the percentage basis for us,
        // not for our own children). Hence it's fine that we're resolving our
        // percentages sizes against this basis in IntrinsicForContainer().
        let isize = nsLayoutUtils::IntrinsicForContainer(aInput.mContext, *self, IntrinsicISizeType::PrefISize, aInput.mPercentageBasisForChildren, 0, &StyleSizeOverrides::default());
        aData.DefaultAddInlinePrefISize(isize);
    }

    /**
     * Useful for line participants. Find the line container frame for this line.
     */
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::FindLineContainer
    pub fn FindLineContainer(&self) -> Option<nsIFrame> {
        MOZ_ASSERT!(self.IsLineParticipant());
        let mut parent = self.GetParent();
        while let Some(p) = parent {
            if !(p.IsLineParticipant() || p.CanContinueTextRun()) {
                break;
            }
            parent = p.GetParent();
        }
        parent
    }

    /**
     * Make this frame and all descendants dirty (if not already).
     * Exceptions: TableColGroup children.
     */
    // Gecko: layout/generic/nsIFrame.cpp nsIFrame::MarkSubtreeDirty
    // NOT PORTED: the TableColGroup exception.
    pub fn MarkSubtreeDirty(&self) {
        if self.HasAnyStateBits(NS_FRAME_IS_DIRTY) {
            return;
        }
        let mut stack: Vec<nsIFrame> = vec![*self];
        while let Some(f) = stack.pop() {
            f.AddStateBits(NS_FRAME_IS_DIRTY);
            let mut childLists: Vec<FrameChildList> = Vec::new();
            f.GetChildLists(&mut childLists);
            for childList in &childLists {
                for kid in &childList.mList {
                    stack.push(kid);
                }
            }
        }
    }

    /**
     * Return the first frame in our current flow.
     * (nsSplittableFrame::FirstContinuation, FirstInFlow: here a continuation chain is the in-flow
     * chain, see nsIFrame::GetPrevContinuation.)
     */
    // Gecko: layout/generic/nsSplittableFrame.cpp nsSplittableFrame::FirstContinuation
    pub fn FirstContinuation(&self) -> nsIFrame {
        let mut firstContinuation = *self;
        while let Some(prev) = firstContinuation.GetPrevContinuation() {
            firstContinuation = prev;
        }
        firstContinuation
    }
}

impl nsContainerFrame {
    /**
     * Helper for calculating intrinsic inline size for inline containers.
     *
     * @param aData the intrinsic inline size data, either an InlineMinISizeData
     *  or an InlinePrefISizeData
     * @param aHandleChildren a callback function invoked for each in-flow
     *  continuation, with the continuation frame and the intrinsic inline size
     *  data passed into it.
     */
    // Gecko: layout/generic/nsContainerFrameInlines.h nsContainerFrame::DoInlineIntrinsicISize
    // (`template <typename ISizeData, typename F>`; ISizeData is InlineMinISizeData or InlinePrefISizeData,
    // which Deref to InlineIntrinsicISizeData)
    pub fn DoInlineIntrinsicISize<ISizeData: DerefMut<Target = nsIFrame_InlineIntrinsicISizeData>, F: FnMut(nsIFrame, &mut ISizeData)>(aThis: nsIFrame, aData: &mut ISizeData, aHandleChildren: &mut F) {
        let GetMargin = |aCoord: &StyleMargin| -> nscoord {
            if !aCoord.IsLengthPercentage() {
                MOZ_ASSERT!(aCoord.IsAuto(), "Didn't resolve anchor functions first?");
                return 0;
            }
            aCoord.AsLengthPercentage().Resolve(0)
        };

        if aThis.GetPrevInFlow().is_some() {
            return; // Already added.
        }

        let wm = aThis.GetWritingMode();
        let startSide = wm.PhysicalSideForInlineAxis(LogicalEdge::Start);
        let endSide = wm.PhysicalSideForInlineAxis(LogicalEdge::End);

        let stylePadding = aThis.StylePadding();
        let styleBorder = aThis.StyleBorder();
        let styleMargin = aThis.StyleMargin();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aThis, std::ptr::null_mut());

        // This goes at the beginning no matter how things are broken and how
        // messy the bidi situations are, since per CSS2.1 section 8.6
        // (implemented in bug 328168), the startSide border is always on the
        // first line.
        // This frame is a first-in-flow, but it might have a previous bidi
        // continuation, in which case that continuation should handle the startSide
        // border.
        // For box-decoration-break:clone we setup clonePBM = startPBM + endPBM and
        // add that to each line.  For box-decoration-break:slice clonePBM is zero.
        let mut clonePBM: nscoord = 0; // PBM = PaddingBorderMargin
        let sliceBreak = styleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Slice;
        if aThis.GetPrevContinuation().is_none() || !sliceBreak {
            let startPBM =
                // clamp negative calc() to 0
                std::cmp::max(stylePadding.mPadding().Get__Side(startSide).Resolve(0), 0)
                + styleBorder.GetComputedBorder().Side(startSide)
                + GetMargin(&styleMargin.GetMargin__Side_AnchorPosResolutionParams(startSide, &anchorResolutionParams));
            if sliceBreak {
                aData.mCurrentLine += startPBM;
            } else {
                clonePBM = startPBM;
            }
        }

        let endPBM =
            // clamp negative calc() to 0
            std::cmp::max(stylePadding.mPadding().Get__Side(endSide).Resolve(0), 0)
            + styleBorder.GetComputedBorder().Side(endSide)
            + GetMargin(&styleMargin.GetMargin__Side_AnchorPosResolutionParams(endSide, &anchorResolutionParams));
        if !sliceBreak {
            clonePBM += endPBM;
            aData.mCurrentLine += clonePBM;
        }

        let savedLine = aData.mLine;
        let savedLineContainer = aData.LineContainer();

        let mut lastInFlow = aThis;
        let mut nif = Some(aThis);
        while let Some(n) = nif {
            if aData.mCurrentLine == 0 {
                aData.mCurrentLine = clonePBM;
            }
            aHandleChildren(n, aData);

            // After we advance to our next-in-flow, the stored line and line container
            // may no longer be correct. Just forget them.
            aData.mLine = None;
            aData.SetLineContainer(None);

            lastInFlow = n;
            nif = n.GetNextInFlow();
        }

        aData.mLine = savedLine;
        aData.SetLineContainer(savedLineContainer);

        // This goes at the end no matter how things are broken and how
        // messy the bidi situations are, since per CSS2.1 section 8.6
        // (implemented in bug 328168), the endSide border is always on the
        // last line.
        // We reached the last-in-flow, but it might have a next bidi
        // continuation, in which case that continuation should handle
        // the endSide border.
        if lastInFlow.GetNextContinuation().is_none() && sliceBreak {
            aData.mCurrentLine += endPBM;
        }
    }
}

impl nsIFrame {
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::DoInlineMinISize
    pub fn DoInlineMinISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlineMinISizeData) {
        let thisWM = self.GetWritingMode();
        let mut handleChildren = |frame: nsIFrame, data: &mut InlineMinISizeData| {
            for kid in &*frame.mFrames() {
                let kidInput = IntrinsicSizeInput::new__IntrinsicSizeInput_WritingMode_WritingMode(aInput, kid.GetWritingMode(), thisWM);
                kid.AddInlineMinISize(&kidInput, data);
            }
        };
        nsContainerFrame::DoInlineIntrinsicISize(*self, aData, &mut handleChildren);
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::DoInlinePrefISize
    pub fn DoInlinePrefISize(&self, aInput: &IntrinsicSizeInput, aData: &mut InlinePrefISizeData) {
        let thisWM = self.GetWritingMode();
        let mut handleChildren = |frame: nsIFrame, data: &mut InlinePrefISizeData| {
            for kid in &*frame.mFrames() {
                let kidInput = IntrinsicSizeInput::new__IntrinsicSizeInput_WritingMode_WritingMode(aInput, kid.GetWritingMode(), thisWM);
                kid.AddInlinePrefISize(&kidInput, data);
            }
        };
        nsContainerFrame::DoInlineIntrinsicISize(*self, aData, &mut handleChildren);
        aData.mLineIsEmpty = false;
    }

    /**
     * This is the CSS block concept of computing 'auto' widths, which most
     * classes derived from nsContainerFrame want.
     */
    // (the virtual call nsIFrame::ComputeAutoSize is in nsIFrame.rs; this is the base class one,
    // moved here from there)
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ComputeAutoSize
    pub fn nsContainerFrame_ComputeAutoSize(
        &self,
        aSizingInput: &SizeComputationInput,
        aWM: WritingMode,
        aCBSize: LogicalSize,
        aAvailableISize: nscoord,
        aMargin: LogicalSize,
        aBorderPadding: LogicalSize,
        aSizeOverrides: &StyleSizeOverrides,
        aFlags: ComputeSizeFlags,
    ) -> LogicalSize {
        // NOT PORTED: `isTableCaption = IsTableCaption()` (table-caption boxes need the table
        // frames); it is false for everything this runtime builds so far.
        let isTableCaption = false;
        // Skip table caption, which requires special sizing - see bug 1109571.
        if self.IsAbsolutelyPositionedWithDefiniteContainingBlock() && !isTableCaption {
            return self.ComputeAbsolutePosAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags);
        }
        let mut result = LogicalSize::new(aWM, 0xdeadbeef_u32 as i32, NS_UNCONSTRAINEDSIZE);
        if aFlags.contains(ComputeSizeFlag::ShrinkWrap) {
            // Delegate to nsIFrame::ComputeAutoSize() for computing the shrink-wrapping
            // size.
            result = self.nsIFrame_ComputeAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags);
        } else {
            *result.ISize_mut(aWM) = aAvailableISize - aMargin.ISize(aWM) - aBorderPadding.ISize(aWM);
        }

        // NOT PORTED: the table caption block (nsContainerFrame.cpp lines 657-688), which clamps the
        // inline-size of an orthogonal or too-narrow table caption to its min-content size.
        result
    }

    /**
     * Positions aKidFrame and its view (if requested), and then calls Reflow().
     * If the reflow status after reflowing the child is FullyComplete then any
     * next-in-flows are deleted using DeleteNextInFlowChild().
     *
     * @param aReflowInput the reflow input for aKidFrame.
     * @param aWM aPos's writing-mode (any writing mode will do).
     * @param aPos Position of the aKidFrame to be moved, in terms of aWM.
     * @param aContainerSize Size of the border-box of the containing frame.
     *
     * Note: If ReflowChildFlags::NoMoveFrame is requested, both aPos and
     * aContainerSize are ignored.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ReflowChild(nsIFrame*, nsPresContext*, ReflowOutput&, const ReflowInput&, const WritingMode&, const LogicalPoint&, const nsSize&, ReflowChildFlags, nsReflowStatus&, nsOverflowContinuationTracker*)
    pub fn ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
        &self,
        aKidFrame: nsIFrame,
        aPresContext: &nsPresContext,
        aDesiredSize: &mut ReflowOutput,
        aReflowInput: &ReflowInput,
        aWM: WritingMode,
        aPos: LogicalPoint,
        aContainerSize: nsSize,
        aFlags: nsIFrame_ReflowChildFlags,
        aStatus: &mut nsReflowStatus,
        mut aTracker: Option<&mut nsOverflowContinuationTracker>,
    ) {
        MOZ_ASSERT!(aReflowInput.mFrame == aKidFrame, "bad reflow input");
        if aWM.IsPhysicalRTL() {
            NS_ASSERTION!(aContainerSize.width != NS_UNCONSTRAINEDSIZE, "ReflowChild with unconstrained container width!");
        }
        MOZ_ASSERT!(
            *aDesiredSize.InkOverflow() == nsRect::new(0, 0, 0, 0) && *aDesiredSize.ScrollableOverflow() == nsRect::new(0, 0, 0, 0),
            "please reset the overflow areas before calling ReflowChild"
        );

        // Position the child frame and its view if requested.
        if nsIFrame_ReflowChildFlags::NoMoveFrame != (aFlags & nsIFrame_ReflowChildFlags::NoMoveFrame) {
            aKidFrame.SetPosition__WritingMode_LogicalPoint_nsSize(aWM, aPos, aContainerSize);
        }

        // Reflow the child frame
        aKidFrame.Reflow(aPresContext, aDesiredSize, aReflowInput, aStatus);

        // If the child frame is complete, delete any next-in-flows,
        // but only if the NoDeleteNextInFlowChild flag isn't set.
        if !aStatus.IsInlineBreakBefore() && aStatus.IsFullyComplete() && !(aFlags & nsIFrame_ReflowChildFlags::NoDeleteNextInFlowChild).to_bool() {
            if let Some(kidNextInFlow) = aKidFrame.GetNextInFlow() {
                // Remove all of the childs next-in-flows. Make sure that we ask
                // the right parent to do the removal (it's possible that the
                // parent is not this because we are executing pullup code)
                // nsOverflowContinuationTracker::AutoFinish fini(aTracker, aKidFrame);
                if let Some(t) = aTracker.as_deref_mut() {
                    t.BeginFinish(aKidFrame);
                }
                let mut context = FrameDestroyContext::default();
                kidNextInFlow.GetParent().unwrap().DeleteNextInFlowChild(&mut context, kidNextInFlow, true);
                if let Some(t) = aTracker.as_deref_mut() {
                    t.EndFinish(aKidFrame);
                }
            }
        }
    }

    // XXX temporary: hold on to a copy of the old physical version of
    //    ReflowChild so that we can convert callers incrementally.
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ReflowChild(nsIFrame*, nsPresContext*, ReflowOutput&, const ReflowInput&, nscoord, nscoord, ReflowChildFlags, nsReflowStatus&, nsOverflowContinuationTracker*)
    pub fn ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_nscoord_nscoord_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
        &self,
        aKidFrame: nsIFrame,
        aPresContext: &nsPresContext,
        aDesiredSize: &mut ReflowOutput,
        aReflowInput: &ReflowInput,
        aX: nscoord,
        aY: nscoord,
        aFlags: nsIFrame_ReflowChildFlags,
        aStatus: &mut nsReflowStatus,
        mut aTracker: Option<&mut nsOverflowContinuationTracker>,
    ) {
        MOZ_ASSERT!(aReflowInput.mFrame == aKidFrame, "bad reflow input");

        // Position the child frame and its view if requested.
        if nsIFrame_ReflowChildFlags::NoMoveFrame != (aFlags & nsIFrame_ReflowChildFlags::NoMoveFrame) {
            aKidFrame.SetPosition__nsPoint(nsPoint::new(aX, aY));
        }

        // Reflow the child frame
        aKidFrame.Reflow(aPresContext, aDesiredSize, aReflowInput, aStatus);

        // If the child frame is complete, delete any next-in-flows,
        // but only if the NoDeleteNextInFlowChild flag isn't set.
        if aStatus.IsFullyComplete() && !(aFlags & nsIFrame_ReflowChildFlags::NoDeleteNextInFlowChild).to_bool() {
            if let Some(kidNextInFlow) = aKidFrame.GetNextInFlow() {
                // Remove all of the childs next-in-flows. Make sure that we ask
                // the right parent to do the removal (it's possible that the
                // parent is not this because we are executing pullup code)
                // nsOverflowContinuationTracker::AutoFinish fini(aTracker, aKidFrame);
                if let Some(t) = aTracker.as_deref_mut() {
                    t.BeginFinish(aKidFrame);
                }
                let mut context = FrameDestroyContext::default();
                kidNextInFlow.GetParent().unwrap().DeleteNextInFlowChild(&mut context, kidNextInFlow, true);
                if let Some(t) = aTracker.as_deref_mut() {
                    t.EndFinish(aKidFrame);
                }
            }
        }
    }
}

impl nsContainerFrame {
    /**
     * The second half of frame reflow. Does the following:
     * - sets the frame's bounds
     * - sizes and positions (if requested) the frame's view. If the frame's final
     *   position differs from the current position and the frame itself does not
     *   have a view, then any child frames with views are positioned so they stay
     *   in sync
     * - sets the view's visibility, opacity, content transparency, and clip
     * - invoked the DidReflow() function
     *
     * @param aReflowInput the reflow input for aKidFrame.
     * @param aWM aPos's writing-mode (any writing mode will do).
     * @param aPos Position of the aKidFrame to be moved, in terms of aWM.
     * @param aContainerSize Size of the border-box of the containing frame.
     *
     * Note: If ReflowChildFlags::NoMoveFrame is requested, both aPos and
     * aContainerSize are ignored unless
     * ReflowChildFlags::ApplyRelativePositioning is requested.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::FinishReflowChild(nsIFrame*, nsPresContext*, const ReflowOutput&, const ReflowInput*, const WritingMode&, const LogicalPoint&, const nsSize&, nsIFrame::ReflowChildFlags)
    // NOT PORTED: views (PositionFrameView, PositionChildViews, SyncFrameViewAfterReflow); there are none.
    pub fn FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
        aKidFrame: nsIFrame,
        aPresContext: &nsPresContext,
        aDesiredSize: &ReflowOutput,
        aReflowInput: Option<&ReflowInput>,
        aWM: WritingMode,
        aPos: LogicalPoint,
        aContainerSize: nsSize,
        aFlags: nsIFrame_ReflowChildFlags,
    ) {
        MOZ_ASSERT!(aReflowInput.map_or(true, |ri| ri.mFrame == aKidFrame));
        MOZ_ASSERT!(aReflowInput.is_some() || aKidFrame.IsMathMLFrame() || aKidFrame.IsTableCellFrame(), "aReflowInput should be passed in almost all cases");

        if aWM.IsPhysicalRTL() {
            NS_ASSERTION!(aContainerSize.width != NS_UNCONSTRAINEDSIZE, "FinishReflowChild with unconstrained container width!");
        }

        let convertedSize = aDesiredSize.Size(aWM);
        let mut pos = aPos;

        if (aFlags & nsIFrame_ReflowChildFlags::ApplyRelativePositioning).to_bool() {
            MOZ_ASSERT!(aReflowInput.is_some(), "caller must have passed reflow input");
            // ApplyRelativePositioning in right-to-left writing modes needs to know
            // the updated frame width to set the normal position correctly.
            aKidFrame.SetSize__WritingMode_LogicalSize(aWM, convertedSize);

            let offsets = aReflowInput.unwrap().ComputedLogicalOffsets(aWM);
            ReflowInput::ApplyRelativePositioning__nsIFrame_WritingMode_LogicalMargin_LogicalPoint_nsSize(aKidFrame, aWM, offsets, &mut pos, aContainerSize);
        }

        if nsIFrame_ReflowChildFlags::NoMoveFrame != (aFlags & nsIFrame_ReflowChildFlags::NoMoveFrame) {
            aKidFrame.SetRect__WritingMode_LogicalRect_nsSize(aWM, LogicalRect::new__WritingMode_LogicalPoint_LogicalSize(aWM, pos, convertedSize), aContainerSize);
        } else {
            aKidFrame.SetSize__WritingMode_LogicalSize(aWM, convertedSize);
        }

        aKidFrame.DidReflow(aPresContext, aReflowInput);
    }

    // XXX temporary: hold on to a copy of the old physical version of
    //    FinishReflowChild so that we can convert callers incrementally.
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::FinishReflowChild(nsIFrame*, nsPresContext*, const ReflowOutput&, const ReflowInput*, nscoord, nscoord, ReflowChildFlags)
    pub fn FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_nscoord_nscoord_ReflowChildFlags(
        aKidFrame: nsIFrame,
        aPresContext: &nsPresContext,
        aDesiredSize: &ReflowOutput,
        aReflowInput: Option<&ReflowInput>,
        aX: nscoord,
        aY: nscoord,
        aFlags: nsIFrame_ReflowChildFlags,
    ) {
        MOZ_ASSERT!(!(aFlags & nsIFrame_ReflowChildFlags::ApplyRelativePositioning).to_bool(), "only the logical version supports ApplyRelativePositioning since ApplyRelativePositioning requires the container size");

        let pos = nsPoint::new(aX, aY);
        let size = aDesiredSize.PhysicalSize();

        if nsIFrame_ReflowChildFlags::NoMoveFrame != (aFlags & nsIFrame_ReflowChildFlags::NoMoveFrame) {
            aKidFrame.SetRect(nsRect::new(pos.x, pos.y, size.width, size.height));
        } else {
            aKidFrame.SetSize__nsSize(size);
        }

        aKidFrame.DidReflow(aPresContext, aReflowInput);
    }
}

impl nsIFrame {
    /**
     * Let the absolutely positioned containing block reflow any absolutely
     * positioned child frames that need to be reflowed.
     *
     * @param aStatus The reflow statuses of any reflowed absolute children will
     * be merged into aStatus; aside from that, this method won't modify aStatus.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ReflowAbsoluteFrames
    pub fn ReflowAbsoluteFrames(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        let absoluteContainer = self.GetAbsoluteContainingBlock();
        if let Some(absoluteContainer) = absoluteContainer {
            if absoluteContainer.PrepareAbsoluteFrames(*self) {
                // The containing block for the abs pos kids is formed by our padding edge.
                let wm = self.GetWritingMode();
                let mut cbRect = LogicalRect::new__WritingMode_LogicalPoint_LogicalSize(wm, LogicalPoint::new__WritingMode(wm), aDesiredSize.Size(wm));
                cbRect.Deflate__WritingMode_LogicalMargin(wm, *self.GetLogicalUsedBorder(wm).ApplySkipSides(self.PreReflowBlockLevelLogicalSkipSides()));
                // XXX: To optimize the performance, set the flags only when the CB width or
                // height actually changes.
                let mut flags = AbsPosReflowFlags::default();
                flags += AbsPosReflowFlag::AllowFragmentation;
                flags += AbsPosReflowFlag::CBWidthChanged;
                flags += AbsPosReflowFlag::CBHeightChanged;
                let mut absposStatus = nsReflowStatus::default();
                let containerSize = aDesiredSize.PhysicalSize();
                absoluteContainer.Reflow(*self, aPresContext, aReflowInput, &mut absposStatus, cbRect.GetPhysicalRect(wm, containerSize), flags, Some(&mut aDesiredSize.mOverflowAreas));
                aStatus.MergeCompletionStatusFrom(&absposStatus);
            }
        }
    }

    /**
     * A convenience method to call ReflowAbsoluteFrames() and
     * FinishAndStoreOverflow().
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::FinishReflowWithAbsoluteFrames
    pub fn FinishReflowWithAbsoluteFrames(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.ReflowAbsoluteFrames(aPresContext, aDesiredSize, aReflowInput, aStatus);
        self.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(aDesiredSize, Some(&aReflowInput.mStyleDisplay));
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::FinishAndStoreOverflow(ReflowOutput*, const nsStyleDisplay*)
    // (the (OverflowAreas&, nsSize) overload is in nsLineLayoutAdapter.rs, as a stand-in for
    // nsIFrame.cpp; it does not take the display)
    pub fn FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(&self, aMetrics: &mut ReflowOutput, aStyleDisplay: Option<&nsStyleDisplay>) {
        let size = nsSize::new(aMetrics.Width(), aMetrics.Height());
        self.FinishAndStoreOverflow(&mut aMetrics.mOverflowAreas, size);
    }

    // ==========================================================================
    /* Overflow containers are continuation frames that hold overflow. They
     * are created when the frame runs out of computed block-size, but still has
     * too much content to fit in the AvailableBSize. The parent creates a
     * continuation as usual, but marks it as NS_FRAME_IS_OVERFLOW_CONTAINER
     * and adds it to its next-in-flow's overflow container list, either by
     * adding it directly or by putting it in its own excess overflow containers
     * list (to be drained by the next-in-flow when it calls
     * ReflowOverflowContainerChildren). The parent continues reflow as if
     * the frame was complete once it ran out of computed block-size, but returns
     * a reflow status with either IsIncomplete() or IsOverflowIncomplete() equal
     * to true to request a next-in-flow. The parent's next-in-flow is then
     * responsible for calling ReflowOverflowContainerChildren to (drain and)
     * reflow these overflow continuations. Overflow containers do not affect
     * other frames' size or position during reflow (but do affect their
     * parent's overflow area).
     *
     * (The rest of the comment, nsContainerFrame.h lines 214-259, explains why overflow container
     * lists cannot be handled like the OverflowProperty list, the nsOverflowContinuationTracker
     * helper, and the ordering of flex and grid items.)
     */

    /**
     * Reflow overflow container children. They are invisible to normal reflow
     * (i.e. don't affect sizing or placement of other children) and inherit
     * width and horizontal position from their prev-in-flow.
     *
     * This method
     *   1. Pulls excess overflow containers from the prev-in-flow and adds
     *      them to our overflow container list
     *   2. Reflows all our overflow container kids
     *   3. Expands aOverflowRect as necessary to accomodate these children.
     *   4. Sets aStatus's mOverflowIncomplete flag (along with
     *      mNextInFlowNeedsReflow as necessary) if any overflow children
     *      are incomplete and
     *   5. Prepends a list of their continuations to our excess overflow
     *      container list, to be drained into our next-in-flow when it is
     *      reflowed.
     *
     * The caller is responsible for tracking any new overflow container
     * continuations it makes, removing them from its child list, and
     * making sure they are stored properly in the overflow container lists.
     * The nsOverflowContinuationTracker helper class should be used for this.
     *
     * @param aFlags is passed through to ReflowChild
     * @param aMergeFunc is passed to DrainExcessOverflowContainersList
     * @param aContainerSize is used only for converting logical coordinate to
     *        physical coordinate. If a tentative container size is used, caller
     *        may need to adjust the position of our overflow container children
     *        once the real size is known if our writing mode is vertical-rl.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ReflowOverflowContainerChildren
    pub fn ReflowOverflowContainerChildren(
        &self,
        aPresContext: &nsPresContext,
        aReflowInput: &ReflowInput,
        aOverflowRects: &mut OverflowAreas,
        aFlags: nsIFrame_ReflowChildFlags,
        aStatus: &mut nsReflowStatus,
        aMergeFunc: nsContainerFrame_ChildFrameMerger,
        aContainerSize: Option<nsSize>,
    ) {
        let overflowContainers = self.DrainExcessOverflowContainersList(aMergeFunc);
        if overflowContainers.is_none() {
            return; // nothing to reflow
        }

        // NOT PORTED: overflow containers (nsContainerFrame.cpp lines 880-1005): the tracker, the
        // ReflowInput of each overflow container with its size overrides, ReflowChild,
        // FinishReflowChild, CreateContinuingFrame for incomplete ones, and ConsiderChildOverflow.
        // Overflow containers are only created by fragmentation, which is not translated, so
        // the list is always empty here.
        unimplemented!("NOT PORTED: reflow of overflow container children (fragmentation)")
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::DisplayOverflowContainers and
    // DisplayAbsoluteFramesNotBuiltByPlaceholder
    // NOT PORTED: painting.

    /**
     * Move any frames on our overflow list to the end of our principal list.
     * @return true if there were any overflow frames
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::DrainSelfOverflowList
    // (virtual; nsBlockFrame, nsInlineFrame, nsFirstLineFrame and others override it)
    pub fn DrainSelfOverflowList(&self) -> bool {
        match self.Type() {
            // Inline, Line => nsInlineFrame::DrainSelfOverflowList, nsFirstLineFrame::DrainSelfOverflowList
            LayoutFrameType::Inline | LayoutFrameType::Line => nsInlineFrame::from_frame(*self).DrainSelfOverflowList(),
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).DrainSelfOverflowList(),
            // FlexContainer => nsFlexContainerFrame::DrainSelfOverflowList (DrainAndMergeSelfOverflowList)
            LayoutFrameType::FlexContainer => self.DrainAndMergeSelfOverflowList(),
            // GridContainer => nsGridContainerFrame::DrainSelfOverflowList (DrainAndMergeSelfOverflowList)
            LayoutFrameType::GridContainer => self.DrainAndMergeSelfOverflowList(),
            _ => self.nsContainerFrame_DrainSelfOverflowList(),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::DrainSelfOverflowList
    pub fn nsContainerFrame_DrainSelfOverflowList(&self) -> bool {
        let mut overflowFrames = AutoFrameListPtr::new(self.StealOverflowFrames());
        if let Some(list) = overflowFrames.operator_arrow_mut() {
            let taken = std::mem::take(list);
            self.mFrames().AppendFrames(None, taken);
            return true;
        }
        false
    }

    /**
     * Move all frames on our prev-in-flow's and our own ExcessOverflowContainers
     * lists to our OverflowContainers list.  If there are frames on multiple
     * lists they are merged using aMergeFunc.
     * @return a pointer to our OverflowContainers list, if any
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::DrainExcessOverflowContainersList
    pub fn DrainExcessOverflowContainersList(&self, aMergeFunc: nsContainerFrame_ChildFrameMerger) -> Option<&'static mut nsFrameList> {
        let mut overflowContainers = self.GetOverflowContainers();

        // Drain excess overflow containers from our prev-in-flow.
        if let Some(prev) = self.GetPrevInFlow() {
            let mut excessFrames = AutoFrameListPtr::new(prev.StealExcessOverflowContainers());
            if let Some(excess) = excessFrames.operator_arrow_mut() {
                excess.ApplySetParent(*self);
                if let Some(oc) = overflowContainers.as_deref_mut() {
                    // The default merge function is AppendFrames, so we use excessFrames as
                    // the destination and then assign the result to overflowContainers.
                    aMergeFunc(excess, oc, *self);
                    *oc = std::mem::take(excess);
                } else {
                    overflowContainers = Some(self.SetOverflowContainers(std::mem::take(excess)));
                }
            }
        }

        // Our own excess overflow containers from a previous reflow can still be
        // present if our next-in-flow hasn't been reflown yet.  Move any children
        // from it that don't have a continuation in this frame to the
        // OverflowContainers list.
        let mut selfExcessOCFrames = AutoFrameListPtr::new(self.StealExcessOverflowContainers());
        if let Some(selfExcess) = selfExcessOCFrames.operator_arrow_mut() {
            let mut toMove = nsFrameList::default();
            let mut child = selfExcess.FirstChild();
            while let Some(c) = child {
                let next = c.GetNextSibling();
                MOZ_ASSERT!(c.GetPrevInFlow().is_some(), "ExcessOverflowContainers frames must be continuations");
                if c.GetPrevInFlow().unwrap().GetParent() != Some(*self) {
                    selfExcess.RemoveFrame(c);
                    toMove.AppendFrame(None, c);
                }
                child = next;
            }

            // If there's any remaining excess overflow containers, put them back.
            if selfExcess.NotEmpty() {
                self.SetExcessOverflowContainers(std::mem::take(selfExcess));
            }

            if toMove.NotEmpty() {
                if let Some(oc) = overflowContainers.as_deref_mut() {
                    aMergeFunc(oc, &mut toMove, *self);
                } else {
                    overflowContainers = Some(self.SetOverflowContainers(toMove));
                }
            }
        }

        overflowContainers
    }

    /**
     * Removes aChild without destroying it and without requesting reflow.
     * Continuations are not affected.  Checks the principal and overflow lists,
     * and also the [excess] overflow containers lists if the frame bit
     * NS_FRAME_IS_OVERFLOW_CONTAINER is set.  It does not check any other lists.
     * aChild must be in one of the above mentioned lists, or an assertion is
     * triggered.
     *
     * Note: This method can destroy either overflow list or [excess] overflow
     * containers list if aChild is the only child in the list. Any pointer to the
     * list obtained prior to calling this method shouldn't be used.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::StealFrame
    // (virtual; nsBlockFrame and nsInlineFrame override it)
    pub fn StealFrame(&self, aChild: nsIFrame) {
        match self.Type() {
            // Inline, Line => nsInlineFrame::StealFrame
            LayoutFrameType::Inline | LayoutFrameType::Line => nsInlineFrame::from_frame(*self).StealFrame(aChild),
            // Block => nsBlockFrame::StealFrame
            LayoutFrameType::Block => nsBlockFrame::from_frame(*self).StealFrame(aChild),
            // NOT PORTED: the overrides of the table, flex and grid containers.
            _ => self.nsContainerFrame_StealFrame(aChild),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::StealFrame
    pub fn nsContainerFrame_StealFrame(&self, aChild: nsIFrame) {
        #[cfg(debug_assertions)]
        {
            if !self.mFrames().ContainsFrame(aChild) {
                let mut list = self.GetOverflowFrames();
                if list.as_ref().map_or(true, |l| !l.ContainsFrame(aChild)) {
                    list = self.GetOverflowContainers();
                    if list.as_ref().map_or(true, |l| !l.ContainsFrame(aChild)) {
                        list = self.GetExcessOverflowContainers();
                        MOZ_ASSERT!(list.as_ref().map_or(false, |l| l.ContainsFrame(aChild)), "aChild isn't our child or on a frame list not supported by StealFrame");
                    }
                }
            }
        }

        if self.MaybeStealOverflowContainerFrame(aChild) {
            return;
        }

        // NOTE nsColumnSetFrame and nsCanvasFrame have their overflow containers
        // on the normal lists so we might get here also if the frame bit
        // NS_FRAME_IS_OVERFLOW_CONTAINER is set.
        if self.mFrames().StartRemoveFrame(aChild) {
            return;
        }

        // We didn't find the child in our principal child list.
        // Maybe it's on the overflow list?
        if let Some(frameList) = self.GetOverflowFrames() {
            if frameList.ContinueRemoveFrame(aChild) {
                if frameList.IsEmpty() {
                    self.DestroyOverflowList();
                }
                return;
            }
        }

        MOZ_ASSERT_UNREACHABLE!("StealFrame: can't find aChild");
    }

    /**
     * Removes the next-siblings of aChild without destroying them and without
     * requesting reflow. Checks the principal and overflow lists (not
     * overflow containers / excess overflow containers). Does not check any
     * other auxiliary lists.
     * @param aChild a child frame or nullptr
     * @return If aChild is non-null, the next-siblings of aChild, if any.
     *         If aChild is null, all child frames on the principal list, if any.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::StealFramesAfter
    pub fn StealFramesAfter(&self, aChild: Option<nsIFrame>) -> nsFrameList {
        NS_ASSERTION!(aChild.map_or(true, |c| !c.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER)), "StealFramesAfter doesn't handle overflow containers");
        NS_ASSERTION!(!self.IsBlockFrame(), "unexpected call");

        let Some(aChild) = aChild else {
            return std::mem::take(&mut *self.mFrames());
        };

        for f in &*self.mFrames() {
            if f == aChild {
                return self.mFrames().TakeFramesAfter(Some(f));
            }
        }

        // We didn't find the child in the principal child list.
        // Maybe it's on the overflow list?
        if let Some(overflowFrames) = self.GetOverflowFrames() {
            for f in &*overflowFrames {
                if f == aChild {
                    return self.mFrames().TakeFramesAfter(Some(f));
                }
            }
        }

        NS_ASSERTION!(false, "StealFramesAfter: can't find aChild");
        nsFrameList::default()
    }

    /**
     * Builds display lists for the children. The background
     * of each child is placed in the Content() list (suitable for inline
     * children and other elements that behave like inlines,
     * but not for in-flow block children of blocks).  DOES NOT
     * paint the background/borders/outline of this frame. This should
     * probably be avoided and eventually removed. It's currently here
     * to emulate what nsContainerFrame::Paint did.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::BuildDisplayList,
    // BuildDisplayListForNonBlockChildren, nsDisplaySelectionOverlay, DisplaySelectionOverlay
    // NOT PORTED: painting.

    /**
     * Returns a CSS Box Alignment constant which the caller can use to align
     * the absolutely-positioned child (whose ReflowInput is aChildRI) within
     * a CSS Box Alignment area associated with this container. Used for
     * computing the static position of an absolutely positioned box.
     */
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::CSSAlignmentForAbsPosChild (virtual)
    pub fn CSSAlignmentForAbsPosChild(&self, aChildRI: &ReflowInput, aLogicalAxis: LogicalAxis) -> StyleAlignFlags {
        match self.Type() {
            // FlexContainer => nsFlexContainerFrame::CSSAlignmentForAbsPosChild
            LayoutFrameType::FlexContainer => nsFlexContainerFrame::from_frame(*self).CSSAlignmentForAbsPosChild(aChildRI, aLogicalAxis),
            _ => self.nsContainerFrame_CSSAlignmentForAbsPosChild(aChildRI, aLogicalAxis),
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::CSSAlignmentForAbsPosChild
    pub fn nsContainerFrame_CSSAlignmentForAbsPosChild(&self, aChildRI: &ReflowInput, aLogicalAxis: LogicalAxis) -> StyleAlignFlags {
        MOZ_ASSERT!(aChildRI.mFrame.IsAbsolutelyPositioned(None), "This method should only be called for abspos children");
        // For computing the static position of an absolutely positioned box,
        // `auto` takes from parent's `align-items`.
        let alignment = aChildRI.mStylePosition.UsedSelfAlignment__LogicalAxis_ComputedStyle(aLogicalAxis, Some(self.Style()));
        CSSAlignUtils::UsedAlignmentForAbsPos(aChildRI.mFrame, alignment, aLogicalAxis, self.GetWritingMode())
    }

    /**
     * Default implementation of `CSSAlignmentForAbsPosChild`, where we treat
     * the absolutely-positioned child as if it were in the flow of this
     * container frame, rather than doing special processing based on its type.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::CSSAlignmentForAbsPosChildWithinContainingBlock
    // NOT PORTED: the position-area default alignment (anchor positioning); the resolved position area
    // is always `none`, so the parameter is left out.
    pub fn CSSAlignmentForAbsPosChildWithinContainingBlock(&self, aSizingInput: &SizeComputationInput, aLogicalAxis: LogicalAxis, aCBSize: LogicalSize) -> StyleAlignFlags {
        MOZ_ASSERT!(aSizingInput.mFrame.IsAbsolutelyPositioned(None), "This method should only be called for abspos children");
        // When determining the position of absolutely-positioned boxes,
        // `auto` behaves as `normal`.
        let alignment = aSizingInput.mFrame.StylePosition().UsedSelfAlignment__LogicalAxis_ComputedStyle(aLogicalAxis, None);

        CSSAlignUtils::UsedAlignmentForAbsPos(aSizingInput.mFrame, alignment, aLogicalAxis, self.GetWritingMode())
    }

    // Incorporate the child overflow areas into aOverflowAreas.
    // If the child does not have a overflow, use the child area.
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ConsiderChildOverflow
    // (the C++ default for aFlags is OverflowAreaUnionFlags::None)
    pub fn ConsiderChildOverflow(&self, aOverflowAreas: &mut OverflowAreas, aChildFrame: nsIFrame, aFlags: OverflowAreaUnionFlags) {
        let childOverflows: OverflowAreas = {
            // NOT PORTED: contain: layout (StyleDisplay()->IsContainLayout() && SupportsContainLayoutAndPaint()):
            // the property is not read from the style yet, so the child's overflow areas are used.
            // if (StyleDisplay()->IsContainLayout() && SupportsContainLayoutAndPaint() &&
            //     !(aFlags & OverflowAreaUnionFlags::AsIfScrolled)) {
            //   return OverflowAreas(aChildFrame->InkOverflowRect(), nsRect()) + aChildFrame->GetPosition();
            // }
            // ADAPTER: GetActualAndNormalOverflowAreasRelativeToParent is the stored overflow
            // (GetOverflowAreasRelativeToParent in nsLineLayoutAdapter.rs); the transform-adjusted
            // variant is the same without transforms.
            aChildFrame.GetOverflowAreasRelativeToParent()
        };
        if (aFlags & OverflowAreaUnionFlags::ChildIsAbsPos).to_bool() {
            aOverflowAreas.UnionWithAbsoluteOverflowAreas(&childOverflows);
        } else {
            aOverflowAreas.UnionWith(&childOverflows);
        }
    }

    // ==========================================================================
    /* Overflow Frames are frames that did not fit and must be pulled by
     * our next-in-flow during its reflow. (The same concept for overflow
     * containers is called "excess frames". We should probably make the
     * names match.)
     */

    /**
     * Helper for DestroyFrom. DestroyAbsoluteFrames is called before
     * destroying frames on lists that can contain placeholders.
     * Derived classes must do that too, if they destroy such frame lists.
     * See nsBlockFrame::DestroyFrom for an example.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::DestroyAbsoluteFrames
    pub fn DestroyAbsoluteFrames(&self, aContext: &mut FrameDestroyContext) {
        if let Some(absCB) = self.GetAbsoluteContainingBlock() {
            absCB.DestroyFrames(aContext);
            self.MarkAsNotAbsoluteContainingBlock();
        }
    }

    /**
     * Helper for StealFrame.  Returns true if aChild was removed from its list.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::MaybeStealOverflowContainerFrame
    pub fn MaybeStealOverflowContainerFrame(&self, aChild: nsIFrame) -> bool {
        let mut removed = false;
        if aChild.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) {
            // Try removing from the overflow container list.
            removed = self.TryRemoveFrame(nsContainerFrame::OverflowContainersProperty(), aChild);
            if !removed {
                // It might be in the excess overflow container list.
                removed = self.TryRemoveFrame(nsContainerFrame::ExcessOverflowContainersProperty(), aChild);
            }
        }
        removed
    }

    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::BuildDisplayListForNonBlockChildren and
    // BuildDisplayListForInline
    // NOT PORTED: painting.

    /**
     * Get the frames on the overflow list, overflow containers list, or excess
     * overflow containers list. Can return null if there are no frames in the
     * list.
     *
     * The caller does NOT take ownership of the list; it's still owned by this
     * frame. A non-null return value indicates that the list is non-empty.
     */
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::GetOverflowFrames
    #[must_use]
    pub fn GetOverflowFrames(&self) -> Option<&'static mut nsFrameList> {
        let list = self.GetProperty(nsContainerFrame::OverflowProperty()).map(|p| unsafe { &mut *p });
        NS_ASSERTION!(list.as_ref().map_or(true, |l| !l.IsEmpty()), "Unexpected empty overflow list");
        list
    }
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::GetOverflowContainers
    #[must_use]
    pub fn GetOverflowContainers(&self) -> Option<&'static mut nsFrameList> {
        let list = self.GetProperty(nsContainerFrame::OverflowContainersProperty()).map(|p| unsafe { &mut *p });
        NS_ASSERTION!(list.as_ref().map_or(true, |l| !l.IsEmpty()), "Unexpected empty overflow containers list");
        list
    }
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::GetExcessOverflowContainers
    #[must_use]
    pub fn GetExcessOverflowContainers(&self) -> Option<&'static mut nsFrameList> {
        let list = self.GetProperty(nsContainerFrame::ExcessOverflowContainersProperty()).map(|p| unsafe { &mut *p });
        NS_ASSERTION!(list.as_ref().map_or(true, |l| !l.IsEmpty()), "Unexpected empty overflow containers list");
        list
    }

    /**
     * Same as the Get methods above, but also remove and the property from this
     * frame.
     *
     * The caller is responsible for deleting nsFrameList and either passing
     * ownership of the frames to someone else or destroying the frames. A
     * non-null return value indicates that the list is non-empty. The recommended
     * way to use this function it to assign its return value into an
     * AutoFrameListPtr.
     */
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::StealOverflowFrames
    // (`nsFrameList*` the caller owns is `Option<Box<nsFrameList>>`, which is what AutoFrameListPtr takes)
    #[must_use]
    pub fn StealOverflowFrames(&self) -> Option<Box<nsFrameList>> {
        let list = self.TakeProperty(nsContainerFrame::OverflowProperty()).map(|p| unsafe { Box::from_raw(p) });
        NS_ASSERTION!(list.as_ref().map_or(true, |l| !l.IsEmpty()), "Unexpected empty overflow list");
        list
    }
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::StealOverflowContainers
    #[must_use]
    pub fn StealOverflowContainers(&self) -> Option<Box<nsFrameList>> {
        let list = self.TakeProperty(nsContainerFrame::OverflowContainersProperty()).map(|p| unsafe { Box::from_raw(p) });
        NS_ASSERTION!(list.as_ref().map_or(true, |l| !l.IsEmpty()), "Unexpected empty overflow list");
        list
    }
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::StealExcessOverflowContainers
    #[must_use]
    pub fn StealExcessOverflowContainers(&self) -> Option<Box<nsFrameList>> {
        let list = self.TakeProperty(nsContainerFrame::ExcessOverflowContainersProperty()).map(|p| unsafe { Box::from_raw(p) });
        NS_ASSERTION!(list.as_ref().map_or(true, |l| !l.IsEmpty()), "Unexpected empty overflow list");
        list
    }

    /**
     * Set the overflow list, overflow containers list, or excess overflow
     * containers list. The argument must be a *non-empty* list.
     *
     * After this operation, the argument becomes an empty list.
     *
     * @return the frame list associated with the property.
     */
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::SetOverflowFrames
    // (the argument is moved into the new list, so the "becomes empty" part is the move)
    pub fn SetOverflowFrames(&self, aOverflowFrames: nsFrameList) -> &'static mut nsFrameList {
        MOZ_ASSERT!(aOverflowFrames.NotEmpty(), "Shouldn't be called");
        let list = Box::into_raw(Box::new(aOverflowFrames));
        self.SetProperty(nsContainerFrame::OverflowProperty(), list);
        unsafe { &mut *list }
    }
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::SetOverflowContainers
    pub fn SetOverflowContainers(&self, aOverflowContainers: nsFrameList) -> &'static mut nsFrameList {
        MOZ_ASSERT!(aOverflowContainers.NotEmpty(), "Shouldn't set an empty list!");
        MOZ_ASSERT!(self.GetProperty(nsContainerFrame::OverflowContainersProperty()).is_none(), "Shouldn't override existing list!");
        MOZ_ASSERT!(self.CanContainOverflowContainers(), "This type of frame can't have overflow containers!");
        let list = Box::into_raw(Box::new(aOverflowContainers));
        self.SetProperty(nsContainerFrame::OverflowContainersProperty(), list);
        unsafe { &mut *list }
    }
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::SetExcessOverflowContainers
    pub fn SetExcessOverflowContainers(&self, aExcessOverflowContainers: nsFrameList) -> &'static mut nsFrameList {
        MOZ_ASSERT!(aExcessOverflowContainers.NotEmpty(), "Shouldn't set an empty list!");
        MOZ_ASSERT!(self.GetProperty(nsContainerFrame::ExcessOverflowContainersProperty()).is_none(), "Shouldn't override existing list!");
        MOZ_ASSERT!(self.CanContainOverflowContainers(), "This type of frame can't have overflow containers!");
        let list = Box::into_raw(Box::new(aExcessOverflowContainers));
        self.SetProperty(nsContainerFrame::ExcessOverflowContainersProperty(), list);
        unsafe { &mut *list }
    }

    /**
     * Destroy the overflow list, overflow containers list, or excess overflow
     * containers list.
     *
     * The list to be destroyed must be empty. That is, the caller is responsible
     * for either passing ownership of the frames to someone else or destroying
     * the frames before calling these methods.
     */
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::DestroyOverflowList
    pub fn DestroyOverflowList(&self) {
        let list = self.TakeProperty(nsContainerFrame::OverflowProperty());
        MOZ_ASSERT!(list.is_some() && unsafe { &*list.unwrap() }.IsEmpty());
        unsafe { Box::from_raw(list.unwrap()) }.Delete();
    }
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::DestroyOverflowContainers
    pub fn DestroyOverflowContainers(&self) {
        let list = self.TakeProperty(nsContainerFrame::OverflowContainersProperty());
        MOZ_ASSERT!(list.is_some() && unsafe { &*list.unwrap() }.IsEmpty());
        unsafe { Box::from_raw(list.unwrap()) }.Delete();
    }
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::DestroyExcessOverflowContainers
    pub fn DestroyExcessOverflowContainers(&self) {
        let list = self.TakeProperty(nsContainerFrame::ExcessOverflowContainersProperty());
        MOZ_ASSERT!(list.is_some() && unsafe { &*list.unwrap() }.IsEmpty());
        unsafe { Box::from_raw(list.unwrap()) }.Delete();
    }

    /**
     * Moves any frames on both the prev-in-flow's overflow list and the
     * receiver's overflow to the receiver's child list.
     *
     * Resets the overlist pointers to nullptr, and updates the receiver's child
     * count and content mapping.
     *
     * @return true if any frames were moved and false otherwise
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::MoveOverflowToChildList
    pub fn MoveOverflowToChildList(&self) -> bool {
        let mut result = false;

        // Check for an overflow list with our prev-in-flow
        if let Some(prevInFlow) = self.GetPrevInFlow() {
            let mut prevOverflowFrames = AutoFrameListPtr::new(prevInFlow.StealOverflowFrames());
            if let Some(list) = prevOverflowFrames.operator_arrow_mut() {
                // Tables are special; they can have repeated header/footer
                // frames on mFrames at this point.
                NS_ASSERTION!(self.mFrames().IsEmpty() || self.IsTableFrame(), "bad overflow list");
                self.mFrames().AppendFrames(Some(*self), std::mem::take(list));
                result = true;
            }
        }

        // It's also possible that we have an overflow list for ourselves.
        self.DrainSelfOverflowList() || result
    }

    /**
     * Merge a sorted frame list into our overflow list. aList becomes empty after
     * this call.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::MergeSortedOverflow
    pub fn MergeSortedOverflow(&self, aList: &mut nsFrameList) {
        if aList.IsEmpty() {
            return;
        }
        MOZ_ASSERT!(!aList.FirstChild().unwrap().HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER), "this is the wrong list to put this child frame");
        MOZ_ASSERT!(aList.FirstChild().unwrap().GetParent() == Some(*self));
        if let Some(overflow) = self.GetOverflowFrames() {
            nsContainerFrame::MergeSortedFrameLists(overflow, aList, self.GetContent());
        } else {
            self.SetOverflowFrames(std::mem::take(aList));
        }
    }

    /**
     * Merge a sorted frame list into our excess overflow containers list. aList
     * becomes empty after this call.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::MergeSortedExcessOverflowContainers
    pub fn MergeSortedExcessOverflowContainers(&self, aList: &mut nsFrameList) {
        if aList.IsEmpty() {
            return;
        }
        MOZ_ASSERT!(aList.FirstChild().unwrap().HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER), "this is the wrong list to put this child frame");
        MOZ_ASSERT!(aList.FirstChild().unwrap().GetParent() == Some(*self));
        if let Some(eoc) = self.GetExcessOverflowContainers() {
            nsContainerFrame::MergeSortedFrameLists(eoc, aList, self.GetContent());
        } else {
            self.SetExcessOverflowContainers(std::mem::take(aList));
        }
    }
}

impl nsContainerFrame {
    /**
     * This is intended to be used as a ChildFrameMerger argument for
     * ReflowOverflowContainerChildren() and DrainExcessOverflowContainersList().
     */
    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::DefaultChildFrameMerge
    pub fn DefaultChildFrameMerge(aDest: &mut nsFrameList, aSrc: &mut nsFrameList, aParent: nsIFrame) {
        aDest.AppendFrames(None, std::mem::take(aSrc));
    }

    // Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::MergeSortedFrameListsFor
    pub fn MergeSortedFrameListsFor(aDest: &mut nsFrameList, aSrc: &mut nsFrameList, aParent: nsIFrame) {
        nsContainerFrame::MergeSortedFrameLists(aDest, aSrc, aParent.GetContent());
    }

    /**
     * Helper to find the first non-anonymous-box frame in the subtree rooted at
     * aFrame.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::GetFirstNonAnonBoxInSubtree
    // NOT PORTED: anonymous boxes are not told apart by pseudo type in this Stylo (a frame is an
    // anonymous box when the frame constructor says so: FrameData::mIsBlockWrapper); only the
    // block wrappers are skipped.
    pub fn GetFirstNonAnonBoxInSubtree(aFrame: Option<nsIFrame>) -> Option<nsIFrame> {
        let mut aFrame = aFrame;
        while let Some(f) = aFrame {
            // If aFrame isn't an anonymous container, or it's text or such, then it'll
            // do.
            if !f.IsBlockWrapper() {
                break;
            }
            aFrame = f.PrincipalChildList_FirstChild();
        }
        aFrame
    }

    /**
     * Moves all frames from aSrc into aDest such that the resulting aDest
     * is still sorted in document content order and continuation order. aSrc
     * becomes empty after this call.
     *
     * Precondition: both |aSrc| and |aDest| must be sorted to begin with.
     * @param aCommonAncestor a hint for nsLayoutUtils::CompareTreePosition
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::MergeSortedFrameLists
    // NOT PORTED: the DOM position comparison (nsContentUtils::CompareTreePosition). The runtime's
    // frames do not reference DOM nodes yet, so the lists are merged as they are: aSrc is appended
    // to aDest. That is the same result whenever the lists come from one continuation chain in
    // order, which is all that block and inline fragmentation do.
    pub fn MergeSortedFrameLists(aDest: &mut nsFrameList, aSrc: &mut nsFrameList, aCommonAncestor: Option<nsIContent>) {
        if aSrc.IsEmpty() {
            return;
        }
        aDest.AppendFrames(None, std::mem::take(aSrc));
        MOZ_ASSERT!(aSrc.IsEmpty());
    }
}

impl nsIFrame {
    /**
     * Basically same as MoveOverflowToChildList, except that this is for
     * handling inline children where children of prev-in-flow can be
     * pushed to overflow list even if a next-in-flow exists.
     *
     * @param aLineContainer the line container of the current frame.
     *
     * @return true if any frames were moved and false otherwise
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::MoveInlineOverflowToChildList
    pub fn MoveInlineOverflowToChildList(&self, aLineContainer: nsIFrame) -> bool {
        let mut result = false;

        // Check for an overflow list with our prev-in-flow
        if let Some(prevInFlow) = self.GetPrevInFlow() {
            let mut prevOverflowFrames = AutoFrameListPtr::new(prevInFlow.StealOverflowFrames());
            if let Some(list) = prevOverflowFrames.operator_arrow_mut() {
                // We may need to reparent floats from prev-in-flow to our line
                // container if the container has prev continuation.
                if aLineContainer.GetPrevContinuation().is_some() {
                    nsContainerFrame::ReparentFloatsForInlineChild(aLineContainer, list.FirstChild(), true);
                }
                // Prepend overflow frames to the list.
                self.mFrames().InsertFrames(Some(*self), None, std::mem::take(list));
                result = true;
            }
        }

        // It's also possible that we have overflow list for ourselves.
        self.DrainSelfOverflowList() || result
    }

    /**
     * Push aFromChild and its next siblings to the overflow list.
     *
     * @param aFromChild the first child frame to push. It is disconnected
     *          from aPrevSibling
     * @param aPrevSibling aFrameChild's previous sibling. Must not be null.
     *          It's an error to push a parent's first child frame.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::PushChildrenToOverflow
    pub fn PushChildrenToOverflow(&self, aFromChild: nsIFrame, aPrevSibling: nsIFrame) {
        MOZ_ASSERT!(aPrevSibling.GetNextSibling() == Some(aFromChild), "bad prev sibling");

        // Add the frames to our overflow list (let our next in flow drain
        // our overflow list when it is ready)
        let taken = self.mFrames().TakeFramesAfter(Some(aPrevSibling));
        self.SetOverflowFrames(taken);
    }

    /**
     * Iterate our children in our principal child list in the normal document
     * order, and append them (or their next-in-flows) to either our overflow list
     * or excess overflow container list according to their presence in
     * aPushedItems, aIncompleteItems, or aOverflowIncompleteItems.
     *
     * Note: This method is only intended for Grid / Flex containers.
     * aPushedItems, aIncompleteItems, and aOverflowIncompleteItems are expected
     * to contain only Grid / Flex items. That is, they should contain only
     * in-flow children.
     *
     * @return true if any items are moved; false otherwise.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::PushIncompleteChildren
    // NOT PORTED: flex and grid containers (nsContainerFrame.cpp lines 1213-1334); only they call it.
    pub fn PushIncompleteChildren(&self, aPushedItems: &std::collections::HashSet<nsIFrame>, aIncompleteItems: &std::collections::HashSet<nsIFrame>, aOverflowIncompleteItems: &std::collections::HashSet<nsIFrame>) -> bool {
        unimplemented!("NOT PORTED: PushIncompleteChildren (flex and grid fragmentation)")
    }

    /**
     * Prepare our child lists so that they are ready to reflow by the following
     * operations:
     *
     * - Merge overflow list from our prev-in-flow into our principal child list.
     * - Merge our own overflow list into our principal child list,
     * - Push any child's next-in-flows in our principal child list to our
     *   overflow list.
     * - Pull up any first-in-flow child we might have pushed from our
     *   next-in-flows.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::NormalizeChildLists
    // NOT PORTED: fragmentation (nsContainerFrame.cpp lines 1336-1561 at GECKO_REV): merging overflow
    // lists from the prev-in-flow, pulling up and pushing next-in-flows. Every step needs a
    // continuation, an overflow list or one of the two state bits; without any of them the function
    // does nothing, which is the only case this runtime reaches (no pagination or columns).
    pub fn NormalizeChildLists(&self) {
        MOZ_ASSERT!(self.IsFlexOrGridContainer(), "Only Flex / Grid containers can call this!");
        let fragmented = self.GetPrevInFlow().is_some()
            || self.GetNextInFlow().is_some()
            || self.GetOverflowFrames().is_some()
            || self.HasAnyStateBits(if self.IsFlexContainerFrame() { NS_STATE_FLEX_HAS_CHILD_NIFS | NS_STATE_FLEX_DID_PUSH_ITEMS } else { NS_STATE_GRID_HAS_CHILD_NIFS | NS_STATE_GRID_DID_PUSH_ITEMS });
        if fragmented {
            unimplemented!("NOT PORTED: NormalizeChildLists of a fragmented flex container");
        }
    }

    /**
     * Helper to implement AppendFrames / InsertFrames for flex / grid
     * containers.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::NoteNewChildren
    pub fn NoteNewChildren(&self, aListID: FrameChildListID, aFrameList: &nsFrameList) {
        MOZ_ASSERT!(aListID == FrameChildListID::Principal, "unexpected child list");
        MOZ_ASSERT!(self.IsFlexOrGridContainer(), "Only Flex / Grid containers can call this!");

        let didPushItemsBit = if self.IsFlexContainerFrame() { NS_STATE_FLEX_DID_PUSH_ITEMS } else { NS_STATE_GRID_DID_PUSH_ITEMS };
        let mut pif = self.GetPrevInFlow();
        while let Some(p) = pif {
            p.AddStateBits(didPushItemsBit);
            PresShell_FrameNeedsReflow(p, IntrinsicDirty::FrameAndAncestors, NS_FRAME_IS_DIRTY);
            pif = p.GetPrevInFlow();
        }
    }

    /**
     * Helper to implement DrainSelfOverflowList() for flex / grid containers.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::DrainAndMergeSelfOverflowList
    pub fn DrainAndMergeSelfOverflowList(&self) -> bool {
        MOZ_ASSERT!(self.IsFlexOrGridContainer(), "Only Flex / Grid containers can call this!");

        // Unlike nsContainerFrame::DrainSelfOverflowList, flex or grid containers
        // need to merge these lists so that the resulting mFrames is in document
        // content order.
        // NOTE: nsContainerFrame::AppendFrames/InsertFrames calls this method and
        // there are also direct calls from the fctor (FindAppendPrevSibling).
        let mut overflowFrames = AutoFrameListPtr::new(self.StealOverflowFrames());
        if let Some(list) = overflowFrames.operator_arrow_mut() {
            nsContainerFrame::MergeSortedFrameLists(&mut *self.mFrames(), list, self.GetContent());
            // We set a frame bit to push them again in Reflow() to avoid creating
            // multiple flex / grid items per flex / grid container fragment for the
            // same content.
            self.AddStateBits(if self.IsFlexContainerFrame() { NS_STATE_FLEX_HAS_CHILD_NIFS } else { NS_STATE_GRID_HAS_CHILD_NIFS });
            return true;
        }
        false
    }
}

impl nsContainerFrame {
    /**
     * Reparent floats whose placeholders are inline descendants of aFrame from
     * whatever block they're currently parented by to aOurBlock.
     * @param aReparentSiblings if this is true, we follow aFrame's
     * GetNextSibling chain reparenting them all
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ReparentFloatsForInlineChild
    // NOT PORTED: floats moving between the fragments of a block (nsBlockFrame::ReparentFloats,
    // nsLayoutUtils::GetFloatContainingBlock; nsContainerFrame.cpp lines 1889-1931). Callers only
    // reach it when the line container has a continuation, which needs fragmentation.
    pub fn ReparentFloatsForInlineChild(aOurLineContainer: nsIFrame, aFrame: Option<nsIFrame>, aReparentSiblings: bool) {
        // XXXbz this would be better if it took a nsFrameList or a frame
        // list slice....
        NS_ASSERTION!(aOurLineContainer.GetNextContinuation().is_some() || aOurLineContainer.GetPrevContinuation().is_some(), "Don't call this when we have no continuation, it's a waste");
        if aFrame.is_none() {
            NS_ASSERTION!(aReparentSiblings, "Why did we get called?");
            return;
        }
        unimplemented!("NOT PORTED: ReparentFloatsForInlineChild (fragmentation of blocks with floats)")
    }
}

impl nsIFrame {
    /**
     * Try to remove aChildToRemove from the frame list stored in aProp.
     * If aChildToRemove was removed from the aProp list and that list became
     * empty, then aProp is removed from this frame and deleted.
     * @note if aChildToRemove isn't on the aProp frame list, it might still be
     * removed from whatever list it happens to be on, so use this method
     * carefully.  This method is primarily meant for removing frames from the
     * [Excess]OverflowContainers lists.
     * @return true if aChildToRemove was removed from some list
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::TryRemoveFrame
    pub fn TryRemoveFrame(&self, aProp: nsContainerFrame_FrameListPropertyDescriptor, aChildToRemove: nsIFrame) -> bool {
        let id = aProp.mId;
        let list = self.GetProperty(aProp).map(|p| unsafe { &mut *p });
        if let Some(list) = list {
            if list.StartRemoveFrame(aChildToRemove) {
                // aChildToRemove *may* have been removed from this list.
                if list.IsEmpty() {
                    let p = self.TakeProperty(FramePropertyDescriptor::<*mut nsFrameList>::new(id)).unwrap();
                    unsafe { Box::from_raw(p) }.Delete();
                }
                return true;
            }
        }
        false
    }

    // ==========================================================================
    /*
     * Convenience methods for traversing continuations
     */

    /**
     * Find the first frame that is a child of this frame's next-in-flows,
     * considering both their principal child lists and overflow lists.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::GetNextInFlowChild
    pub fn GetNextInFlowChild(&self, aState: &mut nsContainerFrame_ContinuationTraversingState, aIsInOverflow: Option<&mut bool>) -> Option<nsIFrame> {
        let mut aIsInOverflow = aIsInOverflow;
        while let Some(nextInFlow) = aState.mNextInFlow {
            // See if there is any frame in the container
            let frame = nextInFlow.mFrames().FirstChild();
            if frame.is_some() {
                if let Some(o) = aIsInOverflow.as_deref_mut() {
                    *o = false;
                }
                return frame;
            }
            // No frames in the principal list, try its overflow list
            if let Some(overflowFrames) = nextInFlow.GetOverflowFrames() {
                if let Some(o) = aIsInOverflow.as_deref_mut() {
                    *o = true;
                }
                return overflowFrames.FirstChild();
            }
            aState.mNextInFlow = nextInFlow.GetNextInFlow().map(nsContainerFrame);
        }
        None
    }

    /**
     * Remove the result of GetNextInFlowChild from its current parent and
     * append it to this frame's principal child list.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::PullNextInFlowChild
    pub fn PullNextInFlowChild(&self, aState: &mut nsContainerFrame_ContinuationTraversingState) -> Option<nsIFrame> {
        let mut isInOverflow = false;
        let frame = self.GetNextInFlowChild(aState, Some(&mut isInOverflow));
        if let Some(frame) = frame {
            let nextInFlow = aState.mNextInFlow.unwrap();
            if isInOverflow {
                let overflowFrames = nextInFlow.GetOverflowFrames().unwrap();
                overflowFrames.RemoveFirstChild();
                if overflowFrames.IsEmpty() {
                    nextInFlow.DestroyOverflowList();
                }
            } else {
                nextInFlow.mFrames().RemoveFirstChild();
            }

            // Move the frame to the principal frame list of this container
            self.mFrames().AppendFrame(Some(*self), frame);
        }
        frame
    }

    /**
     * Safely destroy the frames on the nsFrameList stored on aProp for this
     * frame then remove the property and delete the frame list.
     * Nothing happens if the property doesn't exist.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::SafelyDestroyFrameListProp
    // (the PresShell argument only reaches nsFrameList::Delete, see AutoFrameListPtr)
    pub fn SafelyDestroyFrameListProp(&self, aContext: &mut FrameDestroyContext, aProp: nsContainerFrame_FrameListPropertyDescriptor) {
        let id = aProp.mId;
        // Note that the last frame can be removed through another route and thus
        // delete the property -- that's why we fetch the property again before
        // removing each frame rather than fetching it once and iterating the list.
        loop {
            let Some(frameList) = self.GetProperty(FramePropertyDescriptor::<*mut nsFrameList>::new(id)).map(|p| unsafe { &mut *p }) else {
                break;
            };
            // Note: Similar to nsFrameList::DestroyFrames(), we remove the frames in
            // reverse order to avoid unnecessary updates to the first-continuation and
            // first-in-flow cache. If we delete them from front to back, updating the
            // cache has a O(n^2) time complexity.
            let frame = frameList.RemoveLastChild();
            if let Some(frame) = frame {
                frame.Destroy(aContext);
            } else {
                let p = self.TakeProperty(FramePropertyDescriptor::<*mut nsFrameList>::new(id)).unwrap();
                unsafe { Box::from_raw(p) }.Delete();
                return;
            }
        }
    }
}

// Gecko: layout/generic/nsContainerFrame.h nsContainerFrame::ContinuationTraversingState
pub struct nsContainerFrame_ContinuationTraversingState {
    pub mNextInFlow: Option<nsContainerFrame>,
}

impl nsContainerFrame_ContinuationTraversingState {
    // Gecko: layout/generic/nsContainerFrame.h ContinuationTraversingState::ContinuationTraversingState
    pub fn new(aFrame: nsIFrame) -> Self {
        nsContainerFrame_ContinuationTraversingState { mNextInFlow: aFrame.GetNextInFlow().map(nsContainerFrame) }
    }
}

impl nsIFrame {
    // ==========================================================================

    // Helper used by Progress and Meter frames. Returns true if the bar should
    // be rendered vertically, based on writing-mode and -moz-orient properties.
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ResolvedOrientationIsVertical
    // NOT PORTED: -moz-orient is not in this Stylo; only progress and meter use it.
    pub fn ResolvedOrientationIsVertical(&self) -> bool {
        unimplemented!("NOT PORTED: ResolvedOrientationIsVertical (-moz-orient)")
    }
}

impl nsIFrame {
    /**
     * Calculate the used values for 'width' and 'height' for a replaced element.
     *   http://www.w3.org/TR/CSS21/visudet.html#min-max-widths
     *
     * @param aAspectRatio the aspect ratio calculated by GetAspectRatio().
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ComputeSizeWithIntrinsicDimensions
    pub fn ComputeSizeWithIntrinsicDimensions(
        &self,
        aRenderingContext: Option<gfxContext>,
        aWM: WritingMode,
        aIntrinsicSize: &IntrinsicSize,
        aAspectRatio: AspectRatio,
        aCBSize: LogicalSize,
        aMargin: LogicalSize,
        aBorderPadding: LogicalSize,
        aSizeOverrides: &StyleSizeOverrides,
        mut aFlags: ComputeSizeFlags,
    ) -> LogicalSize {
        let stylePos = self.StylePosition();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(*self, std::ptr::null_mut());
        let styleISize = match &aSizeOverrides.mStyleISize {
            Some(s) => AnchorResolvedSizeHelper::Overridden(s),
            None => stylePos.ISize(aWM, &anchorResolutionParams),
        };

        let styleBSize = {
            let styleBSizeConsideringOverrides = match &aSizeOverrides.mStyleBSize {
                Some(s) => AnchorResolvedSizeHelper::Overridden(s),
                None => stylePos.BSize(aWM, &anchorResolutionParams),
            };
            if styleBSizeConsideringOverrides.BehavesLikeStretchOnBlockAxis() && aCBSize.BSize(aWM) != NS_UNCONSTRAINEDSIZE {
                // We've got a 'stretch' BSize; resolve it to a length:
                let stretchBSize = nsLayoutUtils::ComputeStretchBSize(aCBSize.BSize(aWM), aMargin.BSize(aWM), aBorderPadding.BSize(aWM), stylePos.mBoxSizing());
                // Note(dshin): This allocates.
                AnchorResolvedSizeHelper::LengthPercentage(LengthPercentage::FromAppUnits(stretchBSize))
            } else {
                styleBSizeConsideringOverrides
            }
        };

        let aspectRatio = aSizeOverrides.mAspectRatio.unwrap_or(aAspectRatio);

        let parentFrame = self.GetParent();
        let isGridItem = self.IsGridItem();

        // flexItemMainAxis is set if this frame is a flex item in a modern flexbox
        // layout. It indicates which logical axis (in this frame's own WM)
        // corresponds to its flex container's main axis.
        let mut flexItemMainAxis: Option<LogicalAxis> = None;
        if self.IsFlexItem() && !parentFrame.unwrap().HasAnyStateBits(NS_STATE_FLEX_IS_EMULATING_LEGACY_WEBKIT_BOX) {
            flexItemMainAxis = Some(if nsFlexContainerFrame::IsItemInlineAxisMainAxis(*self) { LogicalAxis::Inline } else { LogicalAxis::Block });
        }

        // Handle intrinsic sizes and their interaction with
        // {min-,max-,}{width,height} according to the rules in
        // https://www.w3.org/TR/CSS22/visudet.html#min-max-widths and
        // https://drafts.csswg.org/css-sizing-3/#intrinsic-sizes

        // Note: throughout the following section of the function, I avoid
        // a * (b / c) because of its reduced accuracy relative to a * b / c
        // or (a * b) / c (which are equivalent).

        let isAutoOrMaxContentISize = styleISize.IsAuto() || styleISize.IsMaxContent();
        let isAutoBSize = nsLayoutUtils::IsAutoBSize(&styleBSize, aCBSize.BSize(aWM));

        let boxSizingAdjust = if stylePos.mBoxSizing() == StyleBoxSizing::BorderBox { aBorderPadding } else { LogicalSize::new__WritingMode(aWM) };
        let boxSizingToMarginEdgeISize = aMargin.ISize(aWM) + aBorderPadding.ISize(aWM) - boxSizingAdjust.ISize(aWM);

        // We don't expect these initial values of iSize/bSize to be used, but this
        // silences a GCC warning about them being uninitialized.
        let minISize: nscoord;
        let maxISize: nscoord;
        let minBSize: nscoord;
        let maxBSize: nscoord;
        let mut iSize: nscoord = 0;
        let mut bSize: nscoord = 0;
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum FillCB {
            // No stretching or clamping in the relevant axis.
            No,
            // Stretch to fill the containing block size in the relevant axis while
            // preserving the aspect-ratio. If both axes are stretched, the aspect-ratio
            // will be disregarded.
            Stretch,
            // Clamp to fill the containing block size in the relevant axis while
            // preserving the aspect-ratio. If both axes are clamped, the aspect-ratio
            // is respected, and the dimensions do not overflow the containing block
            // size.
            Clamp,
        }

        let mut inlineFillCB = FillCB::No; // fill CB behavior in the inline axis
        let mut blockFillCB = FillCB::No; // fill CB behavior in the block axis

        // kFallbackIntrinsicSize (layout/base/LayoutConstants.h): 300 by 150 CSS pixels
        // (LayoutConstants.rs has it as NOT PORTED).
        let fallbackIntrinsicSize = LogicalSize::new__WritingMode_nsSize(aWM, nsSize::new(nsPresContext::CSSPixelsToAppUnits__int32_t(300), nsPresContext::CSSPixelsToAppUnits__int32_t(150)));
        let maybeIntrinsicISize = aIntrinsicSize.ISize(aWM);
        let hasIntrinsicISize = maybeIntrinsicISize.is_some();
        let intrinsicISize = std::cmp::max(0, maybeIntrinsicISize.unwrap_or(0));

        let maybeIntrinsicBSize = aIntrinsicSize.BSize(aWM);
        let hasIntrinsicBSize = maybeIntrinsicBSize.is_some();
        let intrinsicBSize = std::cmp::max(0, maybeIntrinsicBSize.unwrap_or(0));

        let mut iSizeToFillCB: Option<nscoord> = None;
        let mut bSizeToFillCB: Option<nscoord> = None;
        if !isAutoOrMaxContentISize {
            iSize = self
                .ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_SizeOrMaxSize_StyleSize_AspectRatio_ComputeSizeFlags(
                    aRenderingContext,
                    aWM,
                    aCBSize,
                    boxSizingAdjust,
                    boxSizingToMarginEdgeISize,
                    &styleISize,
                    &styleBSize,
                    aspectRatio,
                    aFlags,
                )
                .mISize;
        } else if isGridItem {
            // NOT PORTED: masonry (!parentFrame->IsMasonry(aWM, LogicalAxis::Inline) is always true)
            let parentFrame = parentFrame.unwrap();
            MOZ_ASSERT!(!self.IsTrueOverflowContainer());
            // 'auto' inline-size for grid-level box - apply 'stretch' as needed:
            let cbSize = aCBSize.ISize(aWM);
            if cbSize != NS_UNCONSTRAINEDSIZE {
                if !self.StyleMargin().HasInlineAxisAuto(aWM, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(*self, std::ptr::null_mut())) {
                    let inlineAxisAlignment = stylePos.UsedSelfAlignment__WritingMode_LogicalAxis_WritingMode_ComputedStyle(aWM, LogicalAxis::Inline, parentFrame.GetWritingMode(), Some(parentFrame.Style()));
                    if inlineAxisAlignment == StyleAlignFlags::STRETCH {
                        inlineFillCB = FillCB::Stretch;
                    }
                }
                if inlineFillCB != FillCB::No || aFlags.contains(ComputeSizeFlag::IClampMarginBoxMinSize) {
                    iSizeToFillCB = Some(std::cmp::max(0, cbSize - aBorderPadding.ISize(aWM) - aMargin.ISize(aWM)));
                }
            } else {
                // Reset this flag to avoid applying the clamping below.
                aFlags -= ComputeSizeFlag::IClampMarginBoxMinSize;
            }
        }

        // Flex items ignore their min & max sizing properties in their flex
        // container's main-axis. (Those properties get applied later in the flexbox
        // algorithm.)
        let isFlexItemInlineAxisMainAxis = flexItemMainAxis == Some(LogicalAxis::Inline);
        let maxISizeCoord = stylePos.MaxISize(aWM, &anchorResolutionParams);
        if !maxISizeCoord.IsNone() && !isFlexItemInlineAxisMainAxis {
            maxISize = self
                .ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_SizeOrMaxSize_StyleSize_AspectRatio_ComputeSizeFlags(
                    aRenderingContext,
                    aWM,
                    aCBSize,
                    boxSizingAdjust,
                    boxSizingToMarginEdgeISize,
                    &maxISizeCoord,
                    &styleBSize,
                    aspectRatio,
                    aFlags,
                )
                .mISize;
        } else {
            maxISize = nscoord_MAX;
        }

        let minISizeCoord = stylePos.MinISize(aWM, &anchorResolutionParams);
        if !minISizeCoord.IsAuto() && !isFlexItemInlineAxisMainAxis {
            minISize = self
                .ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_SizeOrMaxSize_StyleSize_AspectRatio_ComputeSizeFlags(
                    aRenderingContext,
                    aWM,
                    aCBSize,
                    boxSizingAdjust,
                    boxSizingToMarginEdgeISize,
                    &minISizeCoord,
                    &styleBSize,
                    aspectRatio,
                    aFlags,
                )
                .mISize;
        } else {
            // Treat "min-width: auto" as 0.
            // NOTE: Technically, "auto" is supposed to behave like "min-content" on
            // flex items. However, we don't need to worry about that here, because
            // flex items' min-sizes are intentionally ignored until the flex
            // container explicitly considers them during space distribution.
            minISize = 0;
        }

        if !isAutoBSize {
            bSize = nsLayoutUtils::ComputeBSizeValueHandlingStretch(aCBSize.BSize(aWM), aMargin.BSize(aWM), aBorderPadding.BSize(aWM), boxSizingAdjust.BSize(aWM), &styleBSize);
        } else if isGridItem {
            // NOT PORTED: masonry (!parentFrame->IsMasonry(aWM, LogicalAxis::Block) is always true)
            let parentFrame = parentFrame.unwrap();
            MOZ_ASSERT!(!self.IsTrueOverflowContainer());
            // 'auto' block-size for grid-level box - apply 'stretch' as needed:
            let cbSize = aCBSize.BSize(aWM);
            if cbSize != NS_UNCONSTRAINEDSIZE {
                if !self.StyleMargin().HasBlockAxisAuto(aWM, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(*self, std::ptr::null_mut())) {
                    let blockAxisAlignment = stylePos.UsedSelfAlignment__WritingMode_LogicalAxis_WritingMode_ComputedStyle(aWM, LogicalAxis::Block, parentFrame.GetWritingMode(), Some(parentFrame.Style()));
                    if blockAxisAlignment == StyleAlignFlags::STRETCH {
                        blockFillCB = FillCB::Stretch;
                    }
                }
                if blockFillCB != FillCB::No || aFlags.contains(ComputeSizeFlag::BClampMarginBoxMinSize) {
                    bSizeToFillCB = Some(std::cmp::max(0, cbSize - aBorderPadding.BSize(aWM) - aMargin.BSize(aWM)));
                }
            } else {
                // Reset this flag to avoid applying the clamping below.
                aFlags -= ComputeSizeFlag::BClampMarginBoxMinSize;
            }
        }

        // Flex items ignore their min & max sizing properties in their flex
        // container's main-axis. (Those properties get applied later in the flexbox
        // algorithm.)
        let isFlexItemBlockAxisMainAxis = flexItemMainAxis == Some(LogicalAxis::Block);
        let maxBSizeCoord = stylePos.MaxBSize(aWM, &anchorResolutionParams);
        if !nsLayoutUtils::IsAutoBSize(&maxBSizeCoord, aCBSize.BSize(aWM)) && !isFlexItemBlockAxisMainAxis {
            maxBSize = nsLayoutUtils::ComputeBSizeValueHandlingStretch(aCBSize.BSize(aWM), aMargin.BSize(aWM), aBorderPadding.BSize(aWM), boxSizingAdjust.BSize(aWM), &maxBSizeCoord);
        } else {
            maxBSize = nscoord_MAX;
        }

        let minBSizeCoord = stylePos.MinBSize(aWM, &anchorResolutionParams);
        if !nsLayoutUtils::IsAutoBSize(&minBSizeCoord, aCBSize.BSize(aWM)) && !isFlexItemBlockAxisMainAxis {
            minBSize = nsLayoutUtils::ComputeBSizeValueHandlingStretch(aCBSize.BSize(aWM), aMargin.BSize(aWM), aBorderPadding.BSize(aWM), boxSizingAdjust.BSize(aWM), &minBSizeCoord);
        } else {
            minBSize = 0;
        }

        NS_ASSERTION!(aCBSize.ISize(aWM) != NS_UNCONSTRAINEDSIZE, "Our containing block must not have unconstrained inline-size!");
        MOZ_ASSERT!(
            !(inlineFillCB != FillCB::No || aFlags.contains(ComputeSizeFlag::IClampMarginBoxMinSize)) || iSizeToFillCB.is_some(),
            "iSizeToFillCB must be valid when stretching or clamping in the inline axis!"
        );
        MOZ_ASSERT!(
            !(blockFillCB != FillCB::No || aFlags.contains(ComputeSizeFlag::BClampMarginBoxMinSize)) || bSizeToFillCB.is_some(),
            "bSizeToFillCB must be valid when stretching or clamping in the block axis!"
        );

        // Now calculate the used values for iSize and bSize:
        if isAutoOrMaxContentISize {
            if isAutoBSize {
                // 'auto' iSize, 'auto' bSize

                // Get tentative values - CSS 2.1 sections 10.3.2 and 10.6.2:

                let mut tentISize: nscoord;
                let mut tentBSize: nscoord;

                if hasIntrinsicISize {
                    tentISize = intrinsicISize;
                } else if hasIntrinsicBSize && aspectRatio.operator_bool() {
                    tentISize = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Inline, aWM, intrinsicBSize, boxSizingAdjust);
                } else if aspectRatio.operator_bool() {
                    tentISize = aCBSize.ISize(aWM) - aBorderPadding.ISize(aWM) - aMargin.ISize(aWM);
                    if tentISize < 0 {
                        tentISize = 0;
                    }
                } else {
                    tentISize = fallbackIntrinsicSize.ISize(aWM);
                }

                // If we need to clamp the inline size to fit the CB, we use the 'stretch'
                // or 'normal' codepath.  We use the ratio-preserving 'normal' codepath
                // unless we have 'stretch' in the other axis.
                if aFlags.contains(ComputeSizeFlag::IClampMarginBoxMinSize) && inlineFillCB != FillCB::Stretch && tentISize > iSizeToFillCB.unwrap() {
                    inlineFillCB = if blockFillCB == FillCB::Stretch { FillCB::Stretch } else { FillCB::Clamp };
                }

                // Honor aspect ratio if there's an intrinsic isize, even if there's an
                // intrinsic bsize.
                if aspectRatio.operator_bool() && (!hasIntrinsicBSize || hasIntrinsicISize) {
                    tentBSize = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Block, aWM, tentISize, boxSizingAdjust);
                } else if hasIntrinsicBSize {
                    tentBSize = intrinsicBSize;
                } else {
                    tentBSize = fallbackIntrinsicSize.BSize(aWM);
                }

                // (ditto the comment about clamping the inline size above)
                if aFlags.contains(ComputeSizeFlag::BClampMarginBoxMinSize) && blockFillCB != FillCB::Stretch && tentBSize > bSizeToFillCB.unwrap() {
                    blockFillCB = if inlineFillCB == FillCB::Stretch { FillCB::Stretch } else { FillCB::Clamp };
                }

                // The slash notation is the used value of "align-self / justify-self".
                if inlineFillCB == FillCB::Stretch {
                    tentISize = iSizeToFillCB.unwrap(); // * / 'stretch'
                    if blockFillCB == FillCB::Stretch {
                        tentBSize = bSizeToFillCB.unwrap(); // 'stretch' / 'stretch'
                    } else if aspectRatio.operator_bool() {
                        // * (except 'stretch') / 'stretch'
                        tentBSize = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Block, aWM, iSizeToFillCB.unwrap(), boxSizingAdjust);
                    }
                } else if blockFillCB == FillCB::Stretch {
                    tentBSize = bSizeToFillCB.unwrap(); // 'stretch' / * (except 'stretch')
                    if aspectRatio.operator_bool() {
                        tentISize = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Inline, aWM, bSizeToFillCB.unwrap(), boxSizingAdjust);
                    }
                } else if inlineFillCB == FillCB::Clamp && aspectRatio.operator_bool() {
                    tentISize = iSizeToFillCB.unwrap(); // * (except 'stretch') / 'normal'
                    tentBSize = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Block, aWM, iSizeToFillCB.unwrap(), boxSizingAdjust);
                    if blockFillCB == FillCB::Clamp && tentBSize > bSizeToFillCB.unwrap() {
                        // Clamp within the CB size with preserved aspect ratio.
                        tentBSize = bSizeToFillCB.unwrap(); // 'normal' / 'normal'
                        tentISize = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Inline, aWM, bSizeToFillCB.unwrap(), boxSizingAdjust);
                    }
                } else if blockFillCB == FillCB::Clamp && aspectRatio.operator_bool() {
                    // 'normal' / * (except 'normal' and 'stretch')
                    tentBSize = bSizeToFillCB.unwrap();
                    tentISize = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Inline, aWM, bSizeToFillCB.unwrap(), boxSizingAdjust);
                }

                // ComputeAutoSizeWithIntrinsicDimensions preserves the ratio when
                // applying the min/max-size.  We don't want that when we have 'stretch'
                // in either axis because tentISize/tentBSize is likely not according to
                // ratio now.
                if aspectRatio.operator_bool() && inlineFillCB != FillCB::Stretch && blockFillCB != FillCB::Stretch {
                    let autoSize = nsLayoutUtils::ComputeAutoSizeWithIntrinsicDimensions(minISize, minBSize, maxISize, maxBSize, tentISize, tentBSize);
                    // The nsSize that ComputeAutoSizeWithIntrinsicDimensions returns will
                    // actually contain logical values if the parameters passed to it were
                    // logical coordinates, so we do NOT perform a physical-to-logical
                    // conversion here, but just assign the fields directly to our result.
                    iSize = autoSize.width;
                    bSize = autoSize.height;
                } else {
                    // Not honoring an intrinsic ratio: clamp the dimensions independently.
                    iSize = CSSMinMax(tentISize, minISize, maxISize);
                    bSize = CSSMinMax(tentBSize, minBSize, maxBSize);
                }
            } else {
                // 'auto' iSize, non-'auto' bSize
                bSize = CSSMinMax(bSize, minBSize, maxBSize);
                if inlineFillCB == FillCB::Stretch {
                    iSize = iSizeToFillCB.unwrap();
                } else if aspectRatio.operator_bool() {
                    iSize = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Inline, aWM, bSize, boxSizingAdjust);
                } else if hasIntrinsicISize {
                    iSize = if aFlags.contains(ComputeSizeFlag::IClampMarginBoxMinSize) && intrinsicISize > iSizeToFillCB.unwrap() { iSizeToFillCB.unwrap() } else { intrinsicISize };
                } else {
                    iSize = fallbackIntrinsicSize.ISize(aWM);
                }
                iSize = CSSMinMax(iSize, minISize, maxISize);
            }
        } else if isAutoBSize {
            // non-'auto' iSize, 'auto' bSize
            iSize = CSSMinMax(iSize, minISize, maxISize);
            if blockFillCB == FillCB::Stretch {
                bSize = bSizeToFillCB.unwrap();
            } else if aspectRatio.operator_bool() {
                bSize = aspectRatio.ComputeRatioDependentSize(LogicalAxis::Block, aWM, iSize, boxSizingAdjust);
            } else if hasIntrinsicBSize {
                bSize = if aFlags.contains(ComputeSizeFlag::BClampMarginBoxMinSize) && intrinsicBSize > bSizeToFillCB.unwrap() { bSizeToFillCB.unwrap() } else { intrinsicBSize };
            } else {
                bSize = fallbackIntrinsicSize.BSize(aWM);
            }
            bSize = CSSMinMax(bSize, minBSize, maxBSize);
        } else {
            // non-'auto' iSize, non-'auto' bSize
            iSize = CSSMinMax(iSize, minISize, maxISize);
            bSize = CSSMinMax(bSize, minBSize, maxBSize);
        }

        LogicalSize::new(aWM, iSize, bSize)
    }

    // Compute tight bounds assuming this frame honours its border, background
    // and outline, its children's tight bounds, and nothing else.
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ComputeSimpleTightBounds
    // NOT PORTED: tight bounds are for painting (text decorations), a DrawTarget does not exist.

    /*
     * If this frame is dirty, marks all absolutely-positioned children of this
     * frame dirty. If this frame isn't dirty, or if there are no
     * absolutely-positioned children, does nothing.
     *
     * It's necessary to use PushDirtyBitToAbsoluteFrames() when you plan to
     * reflow this frame's absolutely-positioned children after the dirty bit on
     * this frame has already been cleared, which prevents ReflowInput from
     * propagating the dirty bit normally. This situation generally only arises
     * when a multipass layout algorithm is used.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::PushDirtyBitToAbsoluteFrames
    pub fn PushDirtyBitToAbsoluteFrames(&self) {
        if !self.HasAnyStateBits(NS_FRAME_IS_DIRTY) {
            return; // No dirty bit to push.
        }
        if !self.HasAbsolutelyPositionedChildren() {
            return; // No absolute children to push to.
        }
        self.GetAbsoluteContainingBlock().unwrap().MarkAllFramesDirty();
    }

    // Helper function that tests if the frame tree is too deep; if it is
    // it marks the frame as "unflowable", zeroes out the metrics, sets
    // the reflow status, and returns true. Otherwise, the frame is
    // unmarked "unflowable" and the metrics and reflow status are not
    // touched and false is returned.
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::IsFrameTreeTooDeep
    pub fn IsFrameTreeTooDeep(&self, aReflowInput: &ReflowInput, aMetrics: &mut ReflowOutput, aStatus: &mut nsReflowStatus) -> bool {
        if aReflowInput.mReflowDepth as i32 > MAX_FRAME_DEPTH {
            NS_WARNING!("frame tree too deep; setting zero size and returning");
            self.AddStateBits(NS_FRAME_TOO_DEEP_IN_FRAME_TREE);
            self.ClearOverflowRects();
            aMetrics.ClearSize();
            aMetrics.SetBlockStartAscent(0);
            aMetrics.mCarriedOutBEndMargin.Zero();
            aMetrics.mOverflowAreas.Clear();

            aStatus.Reset();
            if self.GetNextInFlow().is_some() {
                // Reflow depth might vary between reflows, so we might have
                // successfully reflowed and split this frame before.  If so, we
                // shouldn't delete its continuations.
                aStatus.SetIncomplete();
            }

            return true;
        }
        self.RemoveStateBits(NS_FRAME_TOO_DEEP_IN_FRAME_TREE);
        false
    }

    /**
     * @return true if we should avoid a page/column break in this frame.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::ShouldAvoidBreakInside
    pub fn ShouldAvoidBreakInside(&self, aReflowInput: &ReflowInput) -> bool {
        MOZ_ASSERT!(*self == aReflowInput.mFrame, "Caller should pass a ReflowInput for this frame!");

        let disp = self.StyleDisplay();
        let mayAvoidBreak = match disp.mBreakInside() {
            StyleBreakWithin::Auto => false,
            StyleBreakWithin::Avoid => true,
            // NOT PORTED: AvoidPage and AvoidColumn (the break-inside values of this Stylo
            // are only auto and avoid; see nsStyleStruct.rs).
        };

        if !mayAvoidBreak {
            return false;
        }
        if aReflowInput.mFlags.mIsTopOfPage {
            return false;
        }
        if self.IsAbsolutelyPositioned(Some(&disp)) {
            return false;
        }
        if self.GetPrevInFlow().is_some() {
            return false;
        }
        true
    }

    /**
     * To be called by |BuildDisplayLists| of this class or derived classes to add
     * a translucent overlay if this frame's content is selected.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::DisplaySelectionOverlay
    // NOT PORTED: painting.

    /**
     * Helper for ruby frames that want to accumulate the max ascent/descent of
     * their children.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::RubyMetricsIncludingChildren
    // NOT PORTED: ruby.

    // Gecko: layout/generic/nsContainerFrame.cpp nsContainerFrame::SanityCheckChildListsBeforeReflow and
    // SetDidPushItemsBitIfNeeded (DEBUG helpers for flex and grid containers)
    // NOT PORTED: flex and grid containers. (They are debug-only assertions about fragmented child
    // lists; an unfragmented container has nothing to check. Kept as empty functions so the flex
    // translation can call them where Gecko does.)
    pub fn SanityCheckChildListsBeforeReflow(&self) {
        MOZ_ASSERT!(self.IsFlexOrGridContainer(), "Only Flex / Grid containers can call this!");
    }
    pub fn SetDidPushItemsBitIfNeeded(&self, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        MOZ_ASSERT!(self.IsFlexOrGridContainer(), "Only Flex / Grid containers can call this!");
    }

    // (nsIFrame::ClearOverflowRects is in nsIFrame.rs)
}

// Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::ComputeAutoSizeWithIntrinsicDimensions
impl nsLayoutUtils {
    pub fn ComputeAutoSizeWithIntrinsicDimensions(minWidth: nscoord, minHeight: nscoord, mut maxWidth: nscoord, mut maxHeight: nscoord, tentWidth: nscoord, tentHeight: nscoord) -> nsSize {
        // Now apply min/max-width/height - CSS 2.1 sections 10.4 and 10.7:

        if minWidth > maxWidth {
            maxWidth = minWidth;
        }
        if minHeight > maxHeight {
            maxHeight = minHeight;
        }

        let heightAtMaxWidth: nscoord;
        let heightAtMinWidth: nscoord;
        let widthAtMaxHeight: nscoord;
        let widthAtMinHeight: nscoord;

        if tentWidth > 0 {
            let mut h = NSCoordMulDiv(maxWidth, tentHeight, tentWidth);
            if h < minHeight {
                h = minHeight;
            }
            heightAtMaxWidth = h;
            let mut h2 = NSCoordMulDiv(minWidth, tentHeight, tentWidth);
            if h2 > maxHeight {
                h2 = maxHeight;
            }
            heightAtMinWidth = h2;
        } else {
            heightAtMinWidth = CSSMinMax(tentHeight, minHeight, maxHeight);
            heightAtMaxWidth = heightAtMinWidth;
        }

        if tentHeight > 0 {
            let mut w = NSCoordMulDiv(maxHeight, tentWidth, tentHeight);
            if w < minWidth {
                w = minWidth;
            }
            widthAtMaxHeight = w;
            let mut w2 = NSCoordMulDiv(minHeight, tentWidth, tentHeight);
            if w2 > maxWidth {
                w2 = maxWidth;
            }
            widthAtMinHeight = w2;
        } else {
            widthAtMinHeight = CSSMinMax(tentWidth, minWidth, maxWidth);
            widthAtMaxHeight = widthAtMinHeight;
        }

        // The table at http://www.w3.org/TR/CSS21/visudet.html#min-max-widths :

        let width: nscoord;
        let height: nscoord;

        if tentWidth > maxWidth {
            if tentHeight > maxHeight {
                if (maxWidth as i64) * (tentHeight as i64) <= (maxHeight as i64) * (tentWidth as i64) {
                    width = maxWidth;
                    height = heightAtMaxWidth;
                } else {
                    width = widthAtMaxHeight;
                    height = maxHeight;
                }
            } else {
                // This also covers "(w > max-width) and (h < min-height)" since in
                // that case (max-width/w < 1), and with (h < min-height):
                //   max(max-width * h/w, min-height) == min-height
                width = maxWidth;
                height = heightAtMaxWidth;
            }
        } else if tentWidth < minWidth {
            if tentHeight < minHeight {
                if (minWidth as i64) * (tentHeight as i64) <= (minHeight as i64) * (tentWidth as i64) {
                    width = widthAtMinHeight;
                    height = minHeight;
                } else {
                    width = minWidth;
                    height = heightAtMinWidth;
                }
            } else {
                // This also covers "(w < min-width) and (h > max-height)" since in
                // that case (min-width/w > 1), and with (h > max-height):
                //   min(min-width * h/w, max-height) == max-height
                width = minWidth;
                height = heightAtMinWidth;
            }
        } else if tentHeight > maxHeight {
            width = widthAtMaxHeight;
            height = maxHeight;
        } else if tentHeight < minHeight {
            width = widthAtMinHeight;
            height = minHeight;
        } else {
            width = tentWidth;
            height = tentHeight;
        }

        nsSize::new(width, height)
    }
}

// ==========================================================================
/* The out-of-flow-related code below is for a hacky way of splitting
 * absolutely-positioned frames. Basically what we do is split the frame
 * in nsAbsoluteContainingBlock and pretend the continuation is an overflow
 * container. This isn't an ideal solution, but it lets us print the content
 * at least. See bug 154892.
 */

/**
 * Helper class for tracking overflow container continuations during reflow.
 *
 * A frame is related to two sets of overflow containers: those that /are/
 * its own children, and those that are /continuations/ of its children.
 * This tracker walks through those continuations (the frame's NIF's children)
 * and their prev-in-flows (a subset of the frame's normal and overflow
 * container children) in parallel. It allows the reflower to synchronously
 * walk its overflow continuations while it loops through and reflows its
 * children. This makes it possible to insert new continuations at the correct
 * place in the overflow containers list.
 *
 * The reflower is expected to loop through its children in the same order it
 * looped through them the last time (if there was a last time).
 * For each child, the reflower should either
 *   - call Skip for the child if was not reflowed in this pass
 *   - call Insert for the overflow continuation if the child was reflowed
 *     but has incomplete overflow
 *   - call Finished for the child if it was reflowed in this pass but
 *     is either complete or has a normal next-in-flow. This call can
 *     be skipped if the child did not previously have an overflow
 *     continuation.
 */
// Gecko: layout/generic/nsContainerFrame.h nsOverflowContinuationTracker
// (moved here from nsBlockFrameStub.rs, where it was an empty stand-in)
pub struct nsOverflowContinuationTracker {
    /* We hold a pointer to either the next-in-flow's overflow containers list
    or, if that doesn't exist, our frame's excess overflow containers list.
    We need to make sure that we drop that pointer if the list becomes
    empty and is deleted elsewhere. */
    mOverflowContList: Option<&'static mut nsFrameList>,
    /* We hold a pointer to the most recently-reflowed child that has an
    overflow container next-in-flow. We do this because it's a known
    good point; this pointer won't be deleted on us. We can use it to
    recover our place in the list. */
    mPrevOverflowCont: Option<nsIFrame>,
    /* This is a pointer to the next overflow container's prev-in-flow, which
    is (or should be) a child of our frame. When we hit this, we will need
    to increment this walker to the next overflow container. */
    mSentry: Option<nsIFrame>,
    /* Parent of all frames in mOverflowContList. If our mOverflowContList
    is an excessOverflowContainersProperty, or null, then this is our frame
    (the frame that was passed in to our constructor). Otherwise this is
    that frame's next-in-flow, and our mOverflowContList is mParent's
    overflowContainersProperty */
    mParent: nsIFrame,
    /* Tells SetUpListWalker whether or not to walk us past any continuations
    of overflow containers. aWalkOOFFrames is ignored when this is false. */
    mSkipOverflowContainerChildren: bool,
    /* Tells us whether to pay attention to OOF frames or non-OOF frames */
    mWalkOOFFrames: bool,
}

impl nsOverflowContinuationTracker {
    /**
     * Initializes an nsOverflowContinuationTracker to help track overflow
     * continuations of aFrame's children. Typically invoked on 'this'.
     *
     * aWalkOOFFrames determines whether the walker skips out-of-flow frames
     * or skips non-out-of-flow frames.
     *
     * Don't set aSkipOverflowContainerChildren to false unless you plan
     * to walk your own overflow container children. (Usually they are handled
     * by calling ReflowOverflowContainerChildren.) aWalkOOFFrames is ignored
     * if aSkipOverflowContainerChildren is false.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsOverflowContinuationTracker::nsOverflowContinuationTracker
    // (the C++ default for aSkipOverflowContainerChildren is true)
    pub fn new(aFrame: nsIFrame, aWalkOOFFrames: bool, aSkipOverflowContainerChildren: bool) -> nsOverflowContinuationTracker {
        let mut t = nsOverflowContinuationTracker {
            mOverflowContList: None,
            mPrevOverflowCont: None,
            mSentry: None,
            mParent: aFrame,
            mSkipOverflowContainerChildren: aSkipOverflowContainerChildren,
            mWalkOOFFrames: aWalkOOFFrames,
        };
        t.SetupOverflowContList();
        t
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsOverflowContinuationTracker::SetupOverflowContList
    fn SetupOverflowContList(&mut self) {
        MOZ_ASSERT!(self.mOverflowContList.is_none(), "already have list");
        let nif = self.mParent.GetNextInFlow();
        if let Some(nif) = nif {
            self.mOverflowContList = nif.GetOverflowContainers();
            if self.mOverflowContList.is_some() {
                self.mParent = nif;
                self.SetUpListWalker();
            }
        }
        if self.mOverflowContList.is_none() {
            self.mOverflowContList = self.mParent.GetExcessOverflowContainers();
            if self.mOverflowContList.is_some() {
                self.SetUpListWalker();
            }
        }
    }

    /**
     * Helper function to walk past overflow continuations whose prev-in-flow
     * isn't a normal child and to set mSentry and mPrevOverflowCont correctly.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsOverflowContinuationTracker::SetUpListWalker
    fn SetUpListWalker(&mut self) {
        NS_ASSERTION!(self.mSentry.is_none() && self.mPrevOverflowCont.is_none(), "forgot to reset mSentry or mPrevOverflowCont");
        if let Some(list) = self.mOverflowContList.as_deref() {
            let mut cur = list.FirstChild();
            if self.mSkipOverflowContainerChildren {
                while let Some(c) = cur {
                    if !c.GetPrevInFlow().unwrap().HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) {
                        break;
                    }
                    self.mPrevOverflowCont = Some(c);
                    cur = c.GetNextSibling();
                }
                while let Some(c) = cur {
                    if c.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) == self.mWalkOOFFrames {
                        break;
                    }
                    self.mPrevOverflowCont = Some(c);
                    cur = c.GetNextSibling();
                }
            }
            if let Some(c) = cur {
                self.mSentry = c.GetPrevInFlow();
            }
        }
    }

    /**
     * Helper function to step forward through the overflow continuations list.
     * Sets mSentry and mPrevOverflowCont, skipping over OOF or non-OOF frames
     * as appropriate. May only be called when we have already set up an
     * mOverflowContList; mOverflowContList cannot be null.
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsOverflowContinuationTracker::StepForward
    fn StepForward(&mut self) {
        MOZ_ASSERT!(self.mOverflowContList.is_some(), "null list");

        // Step forward
        if let Some(prev) = self.mPrevOverflowCont {
            self.mPrevOverflowCont = prev.GetNextSibling();
        } else {
            self.mPrevOverflowCont = self.mOverflowContList.as_deref().unwrap().FirstChild();
        }

        // Skip over oof or non-oof frames as appropriate
        if self.mSkipOverflowContainerChildren {
            let mut cur = self.mPrevOverflowCont.unwrap().GetNextSibling();
            while let Some(c) = cur {
                if c.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) == self.mWalkOOFFrames {
                    break;
                }
                self.mPrevOverflowCont = Some(c);
                cur = c.GetNextSibling();
            }
        }

        // Set up the sentry
        self.mSentry = match self.mPrevOverflowCont.unwrap().GetNextSibling() {
            Some(n) => n.GetPrevInFlow(),
            None => None,
        };
    }

    /**
     * This function adds an overflow continuation to our running list and
     * sets its NS_FRAME_IS_OVERFLOW_CONTAINER flag.
     *
     * aReflowStatus should preferably be specific to the recently-reflowed
     * child and not influenced by any of its siblings' statuses. This
     * function sets the NS_FRAME_IS_DIRTY bit on aOverflowCont if it needs
     * to be reflowed. (Its need for reflow depends on changes to its
     * prev-in-flow, not to its parent--for whom it is invisible, reflow-wise.)
     *
     * The caller MUST disconnect the frame from its parent's child list
     * if it was not previously an NS_FRAME_IS_OVERFLOW_CONTAINER (because
     * StealFrame is much more inefficient than disconnecting in place
     * during Reflow, which the caller is able to do but we are not).
     *
     * The caller MUST NOT disconnect the frame from its parent's
     * child list if it is already an NS_FRAME_IS_OVERFLOW_CONTAINER.
     * (In this case we will disconnect and reconnect it ourselves.)
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsOverflowContinuationTracker::Insert
    pub fn Insert(&mut self, aOverflowCont: nsIFrame, aReflowStatus: &mut nsReflowStatus) -> nsresult {
        MOZ_ASSERT!(!self.mSkipOverflowContainerChildren || self.mWalkOOFFrames == aOverflowCont.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW), "shouldn't insert frame that doesn't match walker type");
        MOZ_ASSERT!(aOverflowCont.GetPrevInFlow().is_some(), "overflow containers must have a prev-in-flow");

        let rv = NS_OK;
        let mut reparented = false;
        let mut addToList = self.mSentry.is_none() || Some(aOverflowCont) != self.mSentry.unwrap().GetNextInFlow();

        // If we have a list and aOverflowCont is already in it then don't try to
        // add it again.
        if addToList && aOverflowCont.GetParent() == Some(self.mParent) && aOverflowCont.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) && self.mOverflowContList.as_deref().map_or(false, |l| l.ContainsFrame(aOverflowCont)) {
            addToList = false;
            self.mPrevOverflowCont = aOverflowCont.GetPrevSibling();
        }

        if addToList {
            if aOverflowCont.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) {
                // aOverflowCont is in some other overflow container list,
                // steal it first
                NS_ASSERTION!(!self.mOverflowContList.as_deref().map_or(false, |l| l.ContainsFrame(aOverflowCont)), "overflow containers out of order");
                aOverflowCont.GetParent().unwrap().StealFrame(aOverflowCont);
            } else {
                aOverflowCont.AddStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER);
            }
            if self.mOverflowContList.is_none() {
                // Note: We don't use SetExcessOverflowContainers() since it requires
                // setting a non-empty list. It's OK to manually set an empty list to
                // ExcessOverflowContainersProperty() because we are going to insert
                // aOverflowCont to mOverflowContList below, which guarantees an nonempty
                // list in ExcessOverflowContainersProperty().
                let list = Box::into_raw(Box::new(nsFrameList::default()));
                self.mOverflowContList = Some(unsafe { &mut *list });
                self.mParent.SetProperty(nsContainerFrame::ExcessOverflowContainersProperty(), list);
                self.SetUpListWalker();
            }
            if aOverflowCont.GetParent() != Some(self.mParent) {
                reparented = true;
            }

            // If aOverflowCont has a prev/next-in-flow that might be in
            // mOverflowContList we need to find it and insert after/before it to
            // maintain the order amongst next-in-flows in this list.
            let pif = aOverflowCont.GetPrevInFlow();
            let nif = aOverflowCont.GetNextInFlow();
            if (pif.is_some() && pif.unwrap().GetParent() == Some(self.mParent) && pif != self.mPrevOverflowCont) || (nif.is_some() && nif.unwrap().GetParent() == Some(self.mParent) && self.mPrevOverflowCont.is_some()) {
                for f in &*self.mOverflowContList.as_deref().unwrap() {
                    if Some(f) == pif {
                        self.mPrevOverflowCont = pif;
                        break;
                    }
                    if Some(f) == nif {
                        self.mPrevOverflowCont = f.GetPrevSibling();
                        break;
                    }
                }
            }

            self.mOverflowContList.as_deref_mut().unwrap().InsertFrame(Some(self.mParent), self.mPrevOverflowCont, aOverflowCont);
            aReflowStatus.SetNextInFlowNeedsReflow();
        }

        // If we need to reflow it, mark it dirty
        if aReflowStatus.NextInFlowNeedsReflow() {
            aOverflowCont.MarkSubtreeDirty();
        }

        // It's in our list, just step forward
        self.StepForward();
        NS_ASSERTION!(
            self.mPrevOverflowCont == Some(aOverflowCont) || (self.mSkipOverflowContainerChildren && self.mPrevOverflowCont.unwrap().HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) != aOverflowCont.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW)),
            "OverflowContTracker in unexpected state"
        );

        if addToList {
            // Convert all non-overflow-container next-in-flows of aOverflowCont
            // into overflow containers and move them to our overflow
            // tracker. This preserves the invariant that the next-in-flows
            // of an overflow container are also overflow containers.
            let f = aOverflowCont.GetNextInFlow();
            if let Some(f) = f {
                if !f.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) || (!reparented && f.GetParent() == Some(self.mParent)) || (reparented && f.GetParent() != Some(self.mParent)) {
                    if !f.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) {
                        f.GetParent().unwrap().StealFrame(f);
                    }
                    self.Insert(f, aReflowStatus);
                }
            }
        }
        rv
    }

    /**
     * This function should be called for each child that isn't reflowed.
     * It increments our walker and sets the mOverflowIncomplete
     * reflow flag if it encounters an overflow continuation so that our
     * next-in-flow doesn't get prematurely deleted. It MUST be called on
     * each unreflowed child that has an overflow container continuation;
     * it MAY be called on other children, but it isn't necessary (doesn't
     * do anything).
     */
    // Gecko: layout/generic/nsContainerFrame.h nsOverflowContinuationTracker::Skip
    pub fn Skip(&mut self, aChild: nsIFrame, aReflowStatus: &mut nsReflowStatus) {
        if Some(aChild) == self.mSentry {
            self.StepForward();
            if aReflowStatus.IsComplete() {
                aReflowStatus.SetOverflowIncomplete();
            }
        }
    }

    /**
     * Begin/EndFinish() must be called for each child that is reflowed
     * but no longer has an overflow continuation. (It may be called for
     * other children, but in that case has no effect.) It increments our
     * walker and makes sure we drop any dangling pointers to its
     * next-in-flow. This function MUST be called before stealing or
     * deleting aChild's next-in-flow.
     * The AutoFinish helper object does that for you: ReflowChild does
     *   if (kidNextInFlow) {
     *     nsOverflowContinuationTracker::AutoFinish fini(tracker, kid);
     *     ... DeleteNextInFlowChild/StealFrame(kidNextInFlow) here ...
     *   }
     * as BeginFinish(kid) before and EndFinish(kid) after (Rust has no RAII helper that borrows
     * an Option<&mut> across the deletion, so callers call the two halves).
     */
    // Gecko: layout/generic/nsContainerFrame.cpp nsOverflowContinuationTracker::BeginFinish
    pub fn BeginFinish(&mut self, aChild: nsIFrame) {
        MOZ_ASSERT!(aChild.GetNextInFlow().is_some(), "supposed to call Finish *before* deleting next-in-flow!");

        let mut f = Some(aChild);
        while let Some(fr) = f {
            // We'll update these in EndFinish after the next-in-flows are gone.
            if Some(fr) == self.mPrevOverflowCont {
                self.mSentry = None;
                self.mPrevOverflowCont = None;
                break;
            }
            if Some(fr) == self.mSentry {
                self.mSentry = None;
                break;
            }
            f = fr.GetNextInFlow();
        }
    }

    // Gecko: layout/generic/nsContainerFrame.cpp nsOverflowContinuationTracker::EndFinish
    pub fn EndFinish(&mut self, aChild: nsIFrame) {
        if self.mOverflowContList.is_none() {
            return;
        }
        // Forget mOverflowContList if it was deleted.
        let eoc = self.mParent.GetExcessOverflowContainers().map(|l| l as *mut nsFrameList);
        let current = self.mOverflowContList.as_deref_mut().map(|l| l as *mut nsFrameList);
        if eoc != current {
            let oc = self.mParent.GetOverflowContainers().map(|l| l as *mut nsFrameList);
            if oc != current {
                // mOverflowContList was deleted
                self.mPrevOverflowCont = None;
                self.mSentry = None;
                self.mParent = aChild.GetParent().unwrap();
                self.mOverflowContList = None;
                self.SetupOverflowContList();
                return;
            }
        }
        // The list survived, update mSentry if needed.
        if self.mSentry.is_none() {
            if self.mPrevOverflowCont.is_none() {
                self.SetUpListWalker();
            } else {
                let saved = self.mPrevOverflowCont;
                // step backward to make StepForward() use our current mPrevOverflowCont
                self.mPrevOverflowCont = self.mPrevOverflowCont.unwrap().GetPrevSibling();
                self.StepForward();
                self.mPrevOverflowCont = saved;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// ADAPTER: pieces of other Gecko files that nsContainerFrame.cpp and nsInlineFrame.cpp call and
// that have no translated home yet. Each says which file it stands for.
// ---------------------------------------------------------------------------------------------

// nsCSSFrameConstructor::CreateContinuingFrame is in layout/base/nsCSSFrameConstructor.rs.

// Gecko: layout/generic/nsIFrame.h nsIFrame::IsMathMLFrame
// NOT PORTED: MathML; no frame is one.
impl nsIFrame {
    pub fn IsMathMLFrame(&self) -> bool {
        false
    }

    // Gecko: layout/generic/nsIFrame.h nsIFrame::MarkInReflow
    // (the debug flag part is dropped: the bit is what reflow code checks)
    pub fn MarkInReflow(&self) {
        self.AddStateBits(NS_FRAME_IN_REFLOW);
    }
}

impl IntrinsicSize {
    // Gecko: layout/generic/nsIFrame.h mozilla::IntrinsicSize::ISize
    pub fn ISize(&self, aWM: WritingMode) -> Option<nscoord> {
        if aWM.IsVertical() { self.height } else { self.width }
    }

    // Gecko: layout/generic/nsIFrame.h mozilla::IntrinsicSize::BSize
    pub fn BSize(&self, aWM: WritingMode) -> Option<nscoord> {
        if aWM.IsVertical() { self.width } else { self.height }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockReflowState_test_support::*;

    // Tests are written from the documented behaviour (the comments of nsContainerFrame.h and
    // nsContainerFrame.cpp, CSS 2.2 10.3.2/10.4 for the replaced element sizes); Firefox has no
    // gtest for this class.

    const AUTO: nscoord = NS_UNCONSTRAINEDSIZE;

    // A parent `p` with `n` children c0..., all in p's principal list, plus `loose` frames x0...
    // that have no parent. All Block frames.
    fn family(n: usize, loose: usize) -> (nsIFrame, Vec<nsIFrame>, Vec<nsIFrame>) {
        let mut html = String::from(r#"<div id=p style="margin:0;border:0;padding:0">"#);
        let mut tree: Vec<(String, Option<usize>)> = vec![("p".to_string(), None)];
        for i in 0..n {
            html += &format!("<div id=c{i}></div>");
            tree.push((format!("c{i}"), Some(0)));
        }
        html += "</div>";
        for i in 0..loose {
            html += &format!("<div id=x{i}></div>");
            tree.push((format!("x{i}"), None));
        }
        let tree_ref: Vec<(&str, Option<usize>)> = tree.iter().map(|(n, p)| (n.as_str(), *p)).collect();
        let f = frames(&html, &tree_ref, 100);
        // The parent is not a block: nsBlockFrame overrides most of what these tests exercise
        // (they test nsContainerFrame's own members; a Canvas frame is a plain container).
        f[0].SetType(LayoutFrameType::Canvas);
        (f[0], f[1..1 + n].to_vec(), f[1 + n..].to_vec())
    }

    fn list_of(frames: &[nsIFrame]) -> nsFrameList {
        // frames not linked yet: link them as siblings
        for w in frames.windows(2) {
            w[0].SetNextSibling(Some(w[1]));
        }
        nsFrameList::new(frames.first().copied(), frames.last().copied())
    }

    fn children(p: nsIFrame) -> Vec<nsIFrame> {
        p.mFrames().begin().collect()
    }

    #[test]
    fn mframes_is_the_list_add_frame_builds() {
        let (p, kids, _) = family(3, 0);
        assert_eq!(children(p), kids);
        assert_eq!(p.PrincipalChildList_FirstChild(), Some(kids[0]));
        assert!(kids[1].GetPrevSibling() == Some(kids[0]));
    }

    #[test]
    fn append_frames_adds_at_the_end_and_requests_reflow() {
        let (p, kids, loose) = family(2, 1);
        p.RemoveStateBits(NS_FRAME_HAS_DIRTY_CHILDREN);
        p.AppendFrames(FrameChildListID::Principal, list_of(&loose));
        assert_eq!(children(p), vec![kids[0], kids[1], loose[0]]);
        assert_eq!(loose[0].GetParent(), Some(p));
        // The reflow request marks the frame (FrameNeedsReflow with NS_FRAME_HAS_DIRTY_CHILDREN).
        assert!(p.HasAnyStateBits(NS_FRAME_HAS_DIRTY_CHILDREN));
    }

    #[test]
    fn no_reflow_principal_does_not_request_reflow() {
        let (p, _, loose) = family(1, 1);
        p.RemoveStateBits(NS_FRAME_HAS_DIRTY_CHILDREN);
        p.AppendFrames(FrameChildListID::NoReflowPrincipal, list_of(&loose));
        assert_eq!(p.mFrames().GetLength(), 2);
        assert!(!p.HasAnyStateBits(NS_FRAME_HAS_DIRTY_CHILDREN));
    }

    #[test]
    fn append_of_an_empty_list_does_nothing() {
        let (p, kids, _) = family(1, 0);
        p.RemoveStateBits(NS_FRAME_HAS_DIRTY_CHILDREN);
        p.AppendFrames(FrameChildListID::Principal, nsFrameList::default());
        assert_eq!(children(p), kids);
        assert!(!p.HasAnyStateBits(NS_FRAME_HAS_DIRTY_CHILDREN));
    }

    #[test]
    fn insert_frames_goes_after_the_previous_frame() {
        let (p, kids, loose) = family(2, 1);
        p.InsertFrames(FrameChildListID::Principal, Some(kids[0]), None, list_of(&loose));
        assert_eq!(children(p), vec![kids[0], loose[0], kids[1]]);
        assert_eq!(loose[0].GetParent(), Some(p));
    }

    #[test]
    fn insert_without_previous_frame_prepends() {
        let (p, kids, loose) = family(1, 1);
        p.InsertFrames(FrameChildListID::Principal, None, None, list_of(&loose));
        assert_eq!(children(p), vec![loose[0], kids[0]]);
    }

    #[test]
    fn set_initial_child_list_fills_the_principal_list() {
        let (p, _, loose) = family(0, 2);
        for x in &loose {
            x.SetParent(p);
        }
        p.SetInitialChildList(FrameChildListID::Principal, list_of(&loose));
        assert_eq!(children(p), loose);
    }

    #[test]
    fn remove_frame_destroys_the_frame_and_all_its_continuations() {
        // aOldFrame and the continuations in the same list: all are stolen and destroyed, the last
        // continuation first.
        let (p, kids, _) = family(3, 0);
        kids[0].HookNextInFlow(kids[1]);
        kids[1].HookNextInFlow(kids[2]);
        let mut ctx = FrameDestroyContext::default();
        p.RemoveFrame(&mut ctx, FrameChildListID::Principal, kids[0]);
        assert_eq!(ctx.mDestroyedFrames, vec![kids[2], kids[1], kids[0]]);
        assert!(p.mFrames().IsEmpty());
    }

    #[test]
    fn steal_frame_takes_the_frame_out_of_the_principal_list() {
        let (p, kids, _) = family(3, 0);
        p.StealFrame(kids[1]);
        assert_eq!(children(p), vec![kids[0], kids[2]]);
        // not destroyed: only unlinked
        assert!(kids[1].GetNextSibling().is_none() && kids[1].GetPrevSibling().is_none());
        p.StealFrame(kids[0]);
        p.StealFrame(kids[2]);
        assert!(p.mFrames().IsEmpty());
    }

    #[test]
    fn steal_frame_finds_the_frame_on_the_overflow_list_and_drops_the_empty_list() {
        let (p, kids, _) = family(2, 0);
        p.PushChildrenToOverflow(kids[1], kids[0]);
        assert!(p.GetOverflowFrames().is_some());
        p.StealFrame(kids[1]);
        assert!(p.GetOverflowFrames().is_none());
        assert_eq!(children(p), vec![kids[0]]);
    }

    #[test]
    fn push_children_to_overflow_and_drain_them_back() {
        let (p, kids, _) = family(3, 0);
        p.PushChildrenToOverflow(kids[1], kids[0]);
        assert_eq!(children(p), vec![kids[0]]);
        let overflow: Vec<nsIFrame> = p.GetOverflowFrames().unwrap().begin().collect();
        assert_eq!(overflow, vec![kids[1], kids[2]]);
        // GetChildList and GetChildLists see the overflow list
        assert_eq!(p.GetChildList(FrameChildListID::Overflow).GetLength(), 2);
        let mut lists = Vec::new();
        p.GetChildLists(&mut lists);
        assert_eq!(lists.iter().map(|l| l.mID).collect::<Vec<_>>(), vec![FrameChildListID::Principal, FrameChildListID::Overflow]);

        assert!(p.DrainSelfOverflowList());
        assert_eq!(children(p), kids);
        assert!(p.GetOverflowFrames().is_none());
        // nothing more to drain
        assert!(!p.DrainSelfOverflowList());
    }

    #[test]
    fn move_overflow_to_child_list_takes_the_prev_in_flows_overflow() {
        let (p1, kids, _) = family(2, 0);
        let (p2, _, _) = family(0, 0);
        p1.HookNextInFlow(p2);
        p1.PushChildrenToOverflow(kids[1], kids[0]);
        assert!(p2.MoveOverflowToChildList());
        assert_eq!(children(p2), vec![kids[1]]);
        assert_eq!(kids[1].GetParent(), Some(p2));
        assert!(p1.GetOverflowFrames().is_none());
        assert_eq!(children(p1), vec![kids[0]]);
        assert!(!p2.MoveOverflowToChildList());
    }

    #[test]
    fn steal_frames_after_returns_the_next_siblings() {
        let (p, kids, _) = family(4, 0);
        p.SetType(LayoutFrameType::Inline); // StealFramesAfter is not for blocks
        let taken = p.StealFramesAfter(Some(kids[1]));
        assert_eq!(taken.begin().collect::<Vec<_>>(), vec![kids[2], kids[3]]);
        assert_eq!(children(p), vec![kids[0], kids[1]]);
        // null: everything
        let all = p.StealFramesAfter(None);
        assert_eq!(all.GetLength(), 2);
        assert!(p.mFrames().IsEmpty());
    }

    #[test]
    fn create_next_in_flow_makes_one_continuation_after_the_frame() {
        let (p, kids, _) = family(2, 0);
        p.SetType(LayoutFrameType::Inline);
        kids[0].SetType(LayoutFrameType::Inline);
        let nif = p.CreateNextInFlow(kids[0]).expect("a new continuation");
        assert_eq!(children(p), vec![kids[0], nif, kids[1]]);
        assert_eq!(kids[0].GetNextInFlow(), Some(nif));
        assert_eq!(nif.GetPrevInFlow(), Some(kids[0]));
        assert_eq!(nif.GetParent(), Some(p));
        assert_eq!(nif.Type(), LayoutFrameType::Inline);
        // "If a next-in-flow already exists, nullptr will be returned."
        assert!(p.CreateNextInFlow(kids[0]).is_none());
        assert_eq!(p.mFrames().GetLength(), 3);
    }

    #[test]
    fn delete_next_in_flow_child_deletes_the_whole_chain_after_it() {
        let (p, kids, _) = family(3, 0);
        p.SetType(LayoutFrameType::Inline);
        kids[0].HookNextInFlow(kids[1]);
        kids[1].HookNextInFlow(kids[2]);
        let mut ctx = FrameDestroyContext::default();
        p.DeleteNextInFlowChild(&mut ctx, kids[1], true);
        // "If the next-in-flow has a next-in-flow then delete it, too (and delete it first)."
        assert_eq!(ctx.mDestroyedFrames, vec![kids[2], kids[1]]);
        assert!(p.mFrames().GetLength() == 1);
        assert!(kids[0].GetNextInFlow().is_none());
    }

    #[test]
    fn destroy_destroys_the_children_and_the_overflow_list_before_the_frame() {
        let (p, kids, _) = family(3, 0);
        p.PushChildrenToOverflow(kids[2], kids[1]);
        let mut ctx = FrameDestroyContext::default();
        p.Destroy(&mut ctx);
        // principal list from the end, then the overflow list, then the frame itself
        assert_eq!(ctx.mDestroyedFrames, vec![kids[1], kids[0], kids[2], p]);
        assert!(p.mFrames().IsEmpty());
        assert!(p.GetOverflowFrames().is_none());
    }

    #[test]
    fn destroying_a_frame_unlinks_it_from_its_flow() {
        let (p, kids, _) = family(3, 0);
        kids[0].HookNextInFlow(kids[1]);
        kids[1].HookNextInFlow(kids[2]);
        let mut ctx = FrameDestroyContext::default();
        kids[1].Destroy(&mut ctx);
        assert_eq!(kids[0].GetNextInFlow(), Some(kids[2]));
        assert_eq!(kids[2].GetPrevInFlow(), Some(kids[0]));
    }

    #[test]
    fn consider_child_overflow_unions_the_childs_overflow_at_its_position() {
        let (p, kids, _) = family(1, 0);
        kids[0].SetRect(nsRect::new(10 * PX, 20 * PX, 30 * PX, 5 * PX));
        let mut childAreas = OverflowAreas::new(nsRect::new(-4 * PX, 0, 50 * PX, 5 * PX), nsRect::new(0, 0, 30 * PX, 9 * PX));
        kids[0].FinishAndStoreOverflow(&mut childAreas, kids[0].GetSize());
        let mut mine = OverflowAreas::new(nsRect::new(0, 0, 5 * PX, 5 * PX), nsRect::new(0, 0, 5 * PX, 5 * PX));
        p.ConsiderChildOverflow(&mut mine, kids[0], OverflowAreaUnionFlags::None);
        // the child's overflow, moved by its position: ink x from 6 to 56, scrollable 10..40 by 20..29
        assert_eq!(*mine.InkOverflow(), nsRect::new(0, 0, 56 * PX, 25 * PX));
        assert_eq!(*mine.ScrollableOverflow(), nsRect::new(0, 0, 40 * PX, 29 * PX));
    }

    #[test]
    fn frame_tree_too_deep_zeroes_the_metrics() {
        let (p, _, _) = family(0, 0);
        let mut ri = root_input(p, 10, AUTO);
        let wm = p.GetWritingMode();
        let mut metrics = ReflowOutput::new__WritingMode(wm);
        *metrics.ISize_mut(wm) = 7;
        *metrics.BSize_mut(wm) = 8;
        let mut status = nsReflowStatus::new();

        ri.mReflowDepth = (MAX_FRAME_DEPTH + 1) as i16;
        assert!(p.IsFrameTreeTooDeep(&ri, &mut metrics, &mut status));
        assert!(p.HasAnyStateBits(NS_FRAME_TOO_DEEP_IN_FRAME_TREE));
        assert_eq!(metrics.PhysicalSize(), nsSize::new(0, 0));
        assert!(status.IsEmpty());

        // with a next-in-flow the reflow is incomplete, so that the continuation is kept
        let nif = family(0, 0).0;
        p.HookNextInFlow(nif);
        let mut status = nsReflowStatus::new();
        assert!(p.IsFrameTreeTooDeep(&ri, &mut metrics, &mut status));
        assert!(status.IsIncomplete());

        ri.mReflowDepth = 3;
        assert!(!p.IsFrameTreeTooDeep(&ri, &mut metrics, &mut status));
        assert!(!p.HasAnyStateBits(NS_FRAME_TOO_DEEP_IN_FRAME_TREE));
    }

    // ---- ReflowChild / FinishReflowChild ----

    fn reflow_child_setup(kid_style: &str) -> (Vec<nsIFrame>, nsReflowStatus) {
        let html = format!(r#"<div id=p style="margin:0;border:0;padding:0"><div id=k style="{kid_style}"></div></div>"#);
        let f = frames(&html, &[("p", None), ("k", Some(0))], 100);
        (f, nsReflowStatus::new())
    }

    #[test]
    fn reflow_child_positions_the_frame_then_reflows_it() {
        let (f, mut status) = reflow_child_setup("margin:0");
        let root = root_input(f[0], 100, AUTO);
        let ri = child_input(&root, f[1]);
        let wm = f[0].GetWritingMode();
        f[1].SetReflowStubSize(30 * PX, 10 * PX, 8 * PX);
        f[1].AddStateBits(NS_FRAME_IN_REFLOW);
        let mut metrics = ReflowOutput::new__WritingMode(wm);
        f[0].ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
            f[1],
            &nsPresContext::Current(),
            &mut metrics,
            &ri,
            wm,
            LogicalPoint::new(wm, 5 * PX, 7 * PX),
            nsSize::new(100 * PX, 0),
            nsIFrame_ReflowChildFlags::Default,
            &mut status,
            None,
        );
        // positioned (the size is set by FinishReflowChild), reflowed
        assert_eq!(f[1].GetPosition(), nsPoint::new(5 * PX, 7 * PX));
        assert_eq!(metrics.PhysicalSize(), nsSize::new(30 * PX, 10 * PX));
        assert!(status.IsFullyComplete());
        nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
            f[1],
            &nsPresContext::Current(),
            &metrics,
            Some(&ri),
            wm,
            LogicalPoint::new(wm, 5 * PX, 7 * PX),
            nsSize::new(100 * PX, 0),
            nsIFrame_ReflowChildFlags::Default,
        );
        assert_eq!(f[1].GetRect(), nsRect::new(5 * PX, 7 * PX, 30 * PX, 10 * PX));
        assert!(!f[1].HasAnyStateBits(NS_FRAME_IN_REFLOW), "DidReflow was called");
    }

    #[test]
    fn no_move_frame_leaves_the_position_alone() {
        let (f, mut status) = reflow_child_setup("margin:0");
        let root = root_input(f[0], 100, AUTO);
        let ri = child_input(&root, f[1]);
        let wm = f[0].GetWritingMode();
        f[1].SetRect(nsRect::new(11 * PX, 12 * PX, 0, 0));
        f[1].SetReflowStubSize(30 * PX, 10 * PX, 8 * PX);
        let mut metrics = ReflowOutput::new__WritingMode(wm);
        f[0].ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
            f[1], &nsPresContext::Current(), &mut metrics, &ri, wm, LogicalPoint::new(wm, 5 * PX, 7 * PX), nsSize::new(100 * PX, 0), nsIFrame_ReflowChildFlags::NoMoveFrame, &mut status, None,
        );
        assert_eq!(f[1].GetPosition(), nsPoint::new(11 * PX, 12 * PX));
        // "If ReflowChildFlags::NoMoveFrame is requested, both aPos and aContainerSize are ignored":
        // FinishReflowChild only sets the size.
        nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
            f[1], &nsPresContext::Current(), &metrics, Some(&ri), wm, LogicalPoint::new(wm, 5 * PX, 7 * PX), nsSize::new(100 * PX, 0), nsIFrame_ReflowChildFlags::NoMoveFrame,
        );
        assert_eq!(f[1].GetRect(), nsRect::new(11 * PX, 12 * PX, 30 * PX, 10 * PX));
    }

    #[test]
    fn finish_reflow_child_applies_relative_positioning_when_asked() {
        let (f, _) = reflow_child_setup("position:relative;left:5px;top:2px;margin:0");
        let root = root_input(f[0], 100, AUTO);
        let ri = child_input(&root, f[1]);
        let wm = f[0].GetWritingMode();
        let mut metrics = ReflowOutput::new__WritingMode(wm);
        *metrics.ISize_mut(wm) = 30 * PX;
        *metrics.BSize_mut(wm) = 10 * PX;
        nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
            f[1], &nsPresContext::Current(), &metrics, Some(&ri), wm, LogicalPoint::new(wm, 10 * PX, 10 * PX), nsSize::new(100 * PX, 0), nsIFrame_ReflowChildFlags::ApplyRelativePositioning,
        );
        assert_eq!(f[1].GetRect(), nsRect::new(15 * PX, 12 * PX, 30 * PX, 10 * PX));
        // the normal position is remembered
        assert_eq!(f[1].GetProperty(nsIFrame::NormalPositionProperty()), Some(nsPoint::new(10 * PX, 10 * PX)));
    }

    #[test]
    fn complete_reflow_deletes_the_next_in_flows_unless_told_not_to() {
        for (flags, deleted) in [(nsIFrame_ReflowChildFlags::Default, true), (nsIFrame_ReflowChildFlags::NoDeleteNextInFlowChild, false)] {
            let html = r#"<div id=p style="margin:0;border:0;padding:0"><div id=k></div><div id=k2></div></div>"#;
            let f = frames(html, &[("p", None), ("k", Some(0)), ("k2", Some(0))], 100);
            f[0].SetType(LayoutFrameType::Canvas); // a plain container (nsBlockFrame overrides DeleteNextInFlowChild)
            f[1].HookNextInFlow(f[2]);
            let root = root_input(f[0], 100, AUTO);
            let ri = child_input(&root, f[1]);
            let wm = f[0].GetWritingMode();
            let mut metrics = ReflowOutput::new__WritingMode(wm);
            let mut status = nsReflowStatus::new();
            f[0].ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
                f[1], &nsPresContext::Current(), &mut metrics, &ri, wm, LogicalPoint::new(wm, 0, 0), nsSize::new(100 * PX, 0), flags, &mut status, None,
            );
            assert_eq!(f[1].GetNextInFlow().is_none(), deleted);
            assert_eq!(f[0].mFrames().GetLength(), if deleted { 1 } else { 2 });
        }
    }

    // ---- sizes of replaced elements ----

    // A frame for `<div style=...>` with a parent, and the intrinsic size/ratio given.
    fn replaced_size(style: &str, intrinsic: IntrinsicSize, ratio: f32, cb_i: i32, flags: ComputeSizeFlags) -> (nscoord, nscoord) {
        let html = format!(r#"<div id=p style="margin:0;border:0;padding:0"><div id=r style="{style}"></div></div>"#);
        let f = frames(&html, &[("p", None), ("r", Some(0))], 1000);
        let wm = f[1].GetWritingMode();
        let s = f[1].ComputeSizeWithIntrinsicDimensions(
            None,
            wm,
            &intrinsic,
            AspectRatio::new(ratio, UseBoxSizing::No),
            LogicalSize::new(wm, cb_i * PX, AUTO),
            LogicalSize::new__WritingMode(wm),
            LogicalSize::new__WritingMode(wm),
            &StyleSizeOverrides::default(),
            flags,
        );
        (s.ISize(wm), s.BSize(wm))
    }

    fn px(w: i32, h: i32) -> IntrinsicSize {
        IntrinsicSize { width: Some(w * PX), height: Some(h * PX) }
    }

    #[test]
    fn auto_sizes_use_the_intrinsic_size() {
        // CSS 2.2 10.3.2 / 10.6.2: 'auto' width and height of a replaced element with an intrinsic size.
        let s = replaced_size("", px(100, 50), 2.0, 1000, ComputeSizeFlags::default());
        assert_eq!(s, (100 * PX, 50 * PX));
    }

    #[test]
    fn auto_height_follows_the_ratio_from_a_specified_width() {
        let s = replaced_size("width:200px", px(100, 50), 2.0, 1000, ComputeSizeFlags::default());
        assert_eq!(s, (200 * PX, 100 * PX));
    }

    #[test]
    fn auto_width_follows_the_ratio_from_a_specified_height() {
        let s = replaced_size("height:20px", px(100, 50), 2.0, 1000, ComputeSizeFlags::default());
        assert_eq!(s, (40 * PX, 20 * PX));
    }

    #[test]
    fn min_and_max_sizes_keep_the_ratio() {
        // CSS 2.2 10.4: the constraint violation table. 100x50 with min-width 300: 300x150.
        let s = replaced_size("min-width:300px", px(100, 50), 2.0, 1000, ComputeSizeFlags::default());
        assert_eq!(s, (300 * PX, 150 * PX));
        // max-width 50: 50x25
        let s = replaced_size("max-width:50px", px(100, 50), 2.0, 1000, ComputeSizeFlags::default());
        assert_eq!(s, (50 * PX, 25 * PX));
    }

    #[test]
    fn no_intrinsic_size_and_no_ratio_uses_300_by_150() {
        // CSS 2.2 10.3.2: "otherwise, the used width becomes 300px" (and 150px high).
        let s = replaced_size("", IntrinsicSize::default(), 0.0, 1000, ComputeSizeFlags::default());
        assert_eq!(s, (300 * PX, 150 * PX));
    }

    #[test]
    fn a_ratio_without_intrinsic_size_fills_the_containing_block() {
        // CSS 2.2 10.3.2: a ratio but no intrinsic width/height: "the used width is the width of the
        // containing block".
        let s = replaced_size("", IntrinsicSize::default(), 2.0, 400, ComputeSizeFlags::default());
        assert_eq!(s, (400 * PX, 200 * PX));
    }

    #[test]
    fn auto_size_with_intrinsic_dimensions_follows_the_constraint_violation_table() {
        // CSS 2.2 10.4 table: (w, h) tentative 400x200 (ratio 2).
        let t = |minw, minh, maxw, maxh, w, h| nsLayoutUtils::ComputeAutoSizeWithIntrinsicDimensions(minw, minh, maxw, maxh, w, h);
        // no violation
        assert_eq!(t(0, 0, 1000, 1000, 400, 200), nsSize::new(400, 200));
        // w > max-width: width = max-width, height = max(max-width * h/w, min-height)
        assert_eq!(t(0, 0, 200, 1000, 400, 200), nsSize::new(200, 100));
        // w < min-width: width = min-width, height = min(min-width * h/w, max-height)
        assert_eq!(t(800, 0, 1000, 1000, 400, 200), nsSize::new(800, 400));
        // h > max-height: height = max-height, width = max(max-height * w/h, min-width)
        assert_eq!(t(0, 0, 1000, 50, 400, 200), nsSize::new(100, 50));
        // h < min-height: height = min-height, width = min(min-height * w/h, max-width)
        assert_eq!(t(0, 400, 1000, 1000, 400, 200), nsSize::new(800, 400));
        // both too large: the one that is the smaller reduction wins (w > max-width and h > max-height)
        // max-width/w = 0.25 < max-height/h = 0.5 => width = max-width
        assert_eq!(t(0, 0, 100, 100, 400, 200), nsSize::new(100, 50));
        // a max smaller than the min is raised to the min
        assert_eq!(t(300, 0, 100, 1000, 400, 200), nsSize::new(300, 150));
    }

    // ---- intrinsic inline-size data ----

    #[test]
    fn min_data_force_break_starts_a_new_line() {
        let mut d = InlineMinISizeData::default();
        d.mCurrentLine = 15 * PX;
        d.mTrailingWhitespace = 5 * PX;
        d.mSkipWhitespace = false;
        d.ForceBreak();
        // the trailing whitespace is not part of the line
        assert_eq!(d.mPrevLines, 10 * PX);
        assert_eq!(d.mCurrentLine, 0);
        assert_eq!(d.mTrailingWhitespace, 0);
        assert!(d.mSkipWhitespace);
    }

    #[test]
    fn min_data_optional_break_is_ignored_at_the_start_of_a_line_and_for_negative_lines() {
        let mut d = InlineMinISizeData::default();
        assert!(d.mAtStartOfLine);
        d.mCurrentLine = 10 * PX;
        d.OptionallyBreak(0);
        assert_eq!((d.mPrevLines, d.mCurrentLine), (0, 10 * PX));
        d.mAtStartOfLine = false;
        d.mCurrentLine = -3 * PX;
        d.OptionallyBreak(0);
        assert_eq!(d.mCurrentLine, -3 * PX, "staying on the line fits more into a smaller width");
        d.mCurrentLine = 10 * PX;
        d.OptionallyBreak(2 * PX);
        // the hyphen is added to the line that ends here
        assert_eq!((d.mPrevLines, d.mCurrentLine), (12 * PX, 0));
    }

    #[test]
    fn default_add_inline_min_isize_breaks_around_a_non_text_frame() {
        let (p, kids, _) = family(1, 0);
        let mut d = InlineMinISizeData::default();
        d.DefaultAddInlineMinISize(kids[0], 40 * PX, true);
        // may break before (nothing yet: at the start of the line) and after (the frame is a line)
        assert_eq!((d.mPrevLines, d.mCurrentLine), (40 * PX, 0));
        let mut d = InlineMinISizeData::default();
        d.DefaultAddInlineMinISize(kids[0], 40 * PX, false);
        assert_eq!((d.mPrevLines, d.mCurrentLine), (0, 40 * PX));
        assert!(!d.mSkipWhitespace && !d.mAtStartOfLine);
        let _ = p;
    }

    #[test]
    fn pref_data_add_and_force_break() {
        let mut d = InlinePrefISizeData::default();
        assert!(d.mLineIsEmpty);
        d.DefaultAddInlinePrefISize(30 * PX);
        d.DefaultAddInlinePrefISize(20 * PX);
        assert_eq!(d.mCurrentLine, 50 * PX);
        assert!(!d.mLineIsEmpty && !d.mSkipWhitespace);
        d.mTrailingWhitespace = 4 * PX;
        d.ForceBreak(UsedClear::Both);
        assert_eq!(d.mPrevLines, 46 * PX);
        assert_eq!(d.mCurrentLine, 0);
        assert!(d.mLineIsEmpty && d.mSkipWhitespace);
    }

    #[test]
    fn pref_data_force_break_adds_the_floats_cleared_past() {
        // The float widths of the floats a break clears past are added to the current line:
        // a left float of 20 and a right float of 30 next to each other are 50 wide.
        let html = r#"<div id=p style="margin:0"><div id=a style="float:left"></div><div id=b style="float:right"></div></div>"#;
        let f = frames(html, &[("p", None), ("a", Some(0)), ("b", Some(0))], 100);
        let mut d = InlinePrefISizeData::default();
        d.mFloats.push(nsIFrame_InlineIntrinsicISizeData_FloatInfo::new(f[1], 20 * PX));
        d.mFloats.push(nsIFrame_InlineIntrinsicISizeData_FloatInfo::new(f[2], 30 * PX));
        d.mCurrentLine = 10 * PX;
        // UsedClear::None: floats stay for the next break
        d.ForceBreak(UsedClear::None);
        assert_eq!(d.mPrevLines, 10 * PX);
        assert_eq!(d.mFloats.len(), 2);
        d.mCurrentLine = 10 * PX;
        d.ForceBreak(UsedClear::Both);
        assert_eq!(d.mPrevLines, 60 * PX);
        assert!(d.mFloats.is_empty());
    }

    #[test]
    fn pref_data_clearing_float_starts_a_new_row_of_floats() {
        // b has clear:left, so it goes below a: the widest of the rows counts (30, not 50).
        let html = r#"<div id=p style="margin:0"><div id=a style="float:left"></div><div id=b style="float:right;clear:left"></div></div>"#;
        let f = frames(html, &[("p", None), ("a", Some(0)), ("b", Some(0))], 100);
        let mut d = InlinePrefISizeData::default();
        d.mFloats.push(nsIFrame_InlineIntrinsicISizeData_FloatInfo::new(f[1], 20 * PX));
        d.mFloats.push(nsIFrame_InlineIntrinsicISizeData_FloatInfo::new(f[2], 30 * PX));
        d.ForceBreak(UsedClear::Both);
        assert_eq!(d.mPrevLines, 30 * PX);
    }

    #[test]
    fn do_inline_isize_adds_padding_border_and_margin_of_an_empty_inline() {
        // An inline without children contributes its start and end padding+border+margin:
        // (5 + 1 + 2) * 2.
        let html = r#"<div id=p style="margin:0"><span id=s style="display:inline;padding:0 5px;border:1px solid;margin:0 2px"></span></div>"#;
        let f = frames(html, &[("p", None), ("s", Some(0))], 100);
        f[1].SetType(LayoutFrameType::Inline);
        let input = IntrinsicSizeInput::new(Some(gfxContext), None, None);
        let mut min = InlineMinISizeData::default();
        f[1].AddInlineMinISize(&input, &mut min);
        min.ForceBreak();
        assert_eq!(min.mPrevLines, 16 * PX);
        let mut pref = InlinePrefISizeData::default();
        f[1].AddInlinePrefISize(&input, &mut pref);
        assert_eq!(pref.mCurrentLine, 16 * PX);
        assert!(!pref.mLineIsEmpty);
    }

    #[test]
    fn do_inline_isize_sums_nested_inlines() {
        let html = r#"<div id=p style="margin:0"><span id=s style="display:inline;padding:0 3px"><span id=t style="display:inline;margin:0 4px"></span></span></div>"#;
        let f = frames(html, &[("p", None), ("s", Some(0)), ("t", Some(1))], 100);
        f[1].SetType(LayoutFrameType::Inline);
        f[2].SetType(LayoutFrameType::Inline);
        let input = IntrinsicSizeInput::new(Some(gfxContext), None, None);
        let mut pref = InlinePrefISizeData::default();
        f[1].AddInlinePrefISize(&input, &mut pref);
        assert_eq!(pref.mCurrentLine, (3 + 4 + 4 + 3) * PX);
    }

    #[test]
    fn continuations_are_not_added_twice() {
        // "Already added": a frame with a prev-in-flow adds nothing; the first-in-flow walks all
        // the continuations.
        let html = r#"<div id=p style="margin:0"><span id=s style="display:inline;padding:0 5px"></span></div>"#;
        let f = frames(html, &[("p", None), ("s", Some(0))], 100);
        f[1].SetType(LayoutFrameType::Inline);
        f[0].SetType(LayoutFrameType::Inline);
        let nif = f[0].CreateNextInFlow(f[1]);
        let _ = nif;
        let input = IntrinsicSizeInput::new(Some(gfxContext), None, None);
        let mut pref = InlinePrefISizeData::default();
        f[0].mFrames().LastChild().unwrap().AddInlinePrefISize(&input, &mut pref);
        assert_eq!(pref.mCurrentLine, 0);
        // the start padding at the beginning and the end padding at the last-in-flow: 5 + 5
        f[1].AddInlinePrefISize(&input, &mut pref);
        assert_eq!(pref.mCurrentLine, 10 * PX);
    }

    // ---- overflow continuation tracker ----

    #[test]
    fn tracker_without_overflow_containers_does_nothing() {
        let (p, kids, _) = family(2, 0);
        let mut t = nsOverflowContinuationTracker::new(p, false, true);
        let mut status = nsReflowStatus::new();
        t.Skip(kids[0], &mut status);
        assert!(status.IsFullyComplete());
    }
}
