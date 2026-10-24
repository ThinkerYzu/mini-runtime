// Translated from layout/generic/CSSOrderAwareFrameIterator.h and CSSOrderAwareFrameIterator.cpp (GECKO_REV).
//
// FLEX HELPERS API (what the nsFlexContainerFrame.rs translation can use; written by the WIRE fork):
//
//  Iterator (this file)
//    CSSOrderAwareFrameIterator            = CSSOrderAwareFrameIteratorT<nsFrameList_ForwardFrameTraversal>
//    ReverseCSSOrderAwareFrameIterator     = CSSOrderAwareFrameIteratorT<nsFrameList_BackwardFrameTraversal>
//    CSSOrderAwareFrameIteratorT::<T>::new(aContainer: nsIFrame, aListID: FrameChildListID,
//        aFilter: CSSOrderAwareFrameIterator_ChildFilter, aState: CSSOrderAwareFrameIterator_OrderState,
//        aOrderProp: CSSOrderAwareFrameIterator_OrderingProperty) -> Self   (all five arguments are given;
//        C++ defaults are SkipPlaceholders, Unknown, Order)
//    .get() -> nsIFrame, .ItemIndex() -> usize, .SetItemCount(usize), .SkipPlaceholders(), .AtEnd() -> bool,
//    .Next(), .Reset(aFilter), .IsValid(), .Invalidate(), .ItemsAreAlreadyInOrder(), .IsForward()
//    (`operator*` is get(); there is no std Iterator impl because C++ code loops `for (; !iter.AtEnd(); iter.Next())`.)
//  Frame state bits (nsFrameState.rs): NS_STATE_FLEX_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER,
//    NS_STATE_FLEX_IS_EMULATING_LEGACY_WEBKIT_BOX, NS_STATE_FLEX_SYNTHESIZE_BASELINE,
//    NS_STATE_FLEX_DID_PUSH_ITEMS, NS_STATE_FLEX_HAS_CHILD_NIFS, NS_STATE_FLEX_COMPUTED_INFO
//  nsStylePosition (nsStyleStruct.rs, added by the lead): mFlexBasis() mFlexGrow() mFlexShrink() mFlexWrap()
//    mFlexDirection() mOrder() mJustifyContent() mAlignContent() mAlignItems() mAlignSelf() mRowGap() mColumnGap()
//    UsedAlignSelf(Some(parent ServoArc<ComputedValues>)).
//  nsStyleVisibility: IsCollapse(), UseLegacyCollapseBehavior() (false, NOT PORTED).
//  nsIFrame: IsLegacyWebkitBox() (state bit, never set), GetMarginRect(), GetMarginRectRelativeToSelf(),
//    ShouldBreakBefore(BreakType) / ShouldBreakAfter(BreakType) (false, NOT PORTED), IsFlexItem(), IsFlexOrGridContainer().
//  nsContainerFrame (methods on nsIFrame): NoteNewChildren(aListID, &nsFrameList), DrainAndMergeSelfOverflowList() -> bool,
//    NormalizeChildLists() (no-op when unfragmented, panics when a continuation/overflow list/bit exists),
//    SanityCheckChildListsBeforeReflow(), SetDidPushItemsBitIfNeeded(aListID, aOldFrame) (no-ops).
//  Baseline::SynthesizeBOffsetFromBorderBox / PaddingBox / ContentBox / MarginBox(aFrame, aWM, aGroup).
//  nsLayoutUtils: GetStyleFrame(frame) -> nsIFrame, ResolveGapToLength(&StyleNonNegativeLengthPercentageOrNormal, basis),
//    GetFirstLineBaseline / GetFirstLinePosition (now with the flex and table-wrapper branch),
//    GetLastLineBaseline(aWM, frame, &mut nscoord) -> bool, IsAutoBSize, ComputeBSizeValueHandlingStretch, UnionChildOverflow.
//  Type aliases (ServoStyleConsts.rs): StyleFlexBasis (enum Size(StyleSize) | Content), StyleContentDistribution (.primary()),
//    StyleSelfAlignment (tuple struct, .0 is StyleAlignFlags), StyleFlexWrap, StyleFlexDirection,
//    StyleNonNegativeLengthPercentageOrNormal. AuCoord64 does not exist: use i64.
//  Dispatch wired: Reflow (nsLineLayoutAdapter.rs), IntrinsicISize (nsIFrame.rs), GetNaturalBaselineBOffset (nsBlockFrame.rs),
//    Append/Insert/RemoveFrame, DrainSelfOverflowList, MarkIntrinsicISizesDirty (+ MarkCachedFlexMeasurementsDirty of flex
//    items) in PresShell_FrameNeedsReflow, ComputeSize / ComputeSizeWithIntrinsicDimensions flexItemMainAxis,
//    ReflowInput::InitResizeFlags flex-basis percentage. Init arm in the constructor was already there (ctor fork).
//    Not dispatched (no virtual caller exists in this crate): UnionChildOverflow, UnionInFlowChildOverflow.
//
// ADAPTER: the C++ class template over the frame-list iterator type `Iterator` is generic over the
// traversal (nsFrameList_ForwardFrameTraversal or nsFrameList_BackwardFrameTraversal); the per-type
// members that the .cpp specializes (IsForward, begin, end, the comparators) are the trait
// CSSOrderAwareTraversal. The nested enums are top-level, named CSSOrderAwareFrameIterator_<Name>.
//
// NOT PORTED: the legacy "box-ordinal-group" ordering (OrderingProperty::BoxOrdinalGroup needs nsStyleXUL,
// which is not in this crate; selecting it panics), and the XUL treecols/treecol case of CanUse.
use crate::*;
use std::cmp::Ordering;

// Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::OrderState
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CSSOrderAwareFrameIterator_OrderState {
    Unknown,
    Ordered,
    Unordered,
}

// Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::ChildFilter
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CSSOrderAwareFrameIterator_ChildFilter {
    SkipPlaceholders,
    IncludeAll,
}

// Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::OrderingProperty
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CSSOrderAwareFrameIterator_OrderingProperty {
    Order,           // Default behavior: use "order".
    BoxOrdinalGroup, // Legacy behavior: use prefixed "box-ordinal-group".
}

// ADAPTER: the members the .cpp specializes for each iterator type.
pub trait CSSOrderAwareTraversal: nsFrameList_FrameTraversal {
    // Gecko: CSSOrderAwareFrameIteratorT::IsForward
    fn IsForward() -> bool;
    // Gecko: CSSOrderAwareFrameIteratorT::begin
    fn begin(aList: &nsFrameList) -> nsFrameList_Iterator<Self>
    where
        Self: Sized;
    // Gecko: CSSOrderAwareFrameIteratorT::end
    fn end(aList: &nsFrameList) -> nsFrameList_Iterator<Self>
    where
        Self: Sized;
    // The comparators: (a > b) - (a < b) for forward, (b > a) - (b < a) for reverse.
    fn Compare(a: i32, b: i32) -> Ordering;
}

impl CSSOrderAwareTraversal for nsFrameList_ForwardFrameTraversal {
    fn IsForward() -> bool {
        true
    }
    fn begin(aList: &nsFrameList) -> nsFrameList_Iterator<Self> {
        aList.begin()
    }
    fn end(aList: &nsFrameList) -> nsFrameList_Iterator<Self> {
        aList.end()
    }
    fn Compare(a: i32, b: i32) -> Ordering {
        a.cmp(&b)
    }
}

impl CSSOrderAwareTraversal for nsFrameList_BackwardFrameTraversal {
    fn IsForward() -> bool {
        false
    }
    fn begin(aList: &nsFrameList) -> nsFrameList_Iterator<Self> {
        aList.rbegin()
    }
    fn end(aList: &nsFrameList) -> nsFrameList_Iterator<Self> {
        aList.rend()
    }
    fn Compare(a: i32, b: i32) -> Ordering {
        b.cmp(&a)
    }
}

// Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT
// (see the long comment there: iterates a child list by CSS "order", stable, skipping placeholders by default)
pub struct CSSOrderAwareFrameIteratorT<Iterator: CSSOrderAwareTraversal> {
    mChildren: &'static nsFrameList,
    // Used if child list is already in ascending 'order'.
    mIter: Option<nsFrameList_Iterator<Iterator>>,
    mIterEnd: Option<nsFrameList_Iterator<Iterator>>,
    // Used if child list is *not* in ascending 'order'.
    // This array is pre-sorted in reverse order for a reverse iterator.
    mArray: Option<Vec<nsIFrame>>,
    mArrayIndex: usize,
    // The index of the current item (placeholders excluded).
    mItemIndex: usize,
    // The number of items (placeholders excluded).
    // It's only initialized and used in a reverse iterator.
    mItemCount: Option<usize>,
    // Skip placeholder children in the iteration?
    mSkipPlaceholders: bool,
    #[cfg(debug_assertions)]
    mContainer: nsIFrame,
    #[cfg(debug_assertions)]
    mListID: FrameChildListID,
}

// Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIterator
pub type CSSOrderAwareFrameIterator = CSSOrderAwareFrameIteratorT<nsFrameList_ForwardFrameTraversal>;
// Gecko: layout/generic/CSSOrderAwareFrameIterator.h ReverseCSSOrderAwareFrameIterator
pub type ReverseCSSOrderAwareFrameIterator = CSSOrderAwareFrameIteratorT<nsFrameList_BackwardFrameTraversal>;

// Gecko: layout/generic/CSSOrderAwareFrameIterator.cpp CanUse
// NOT PORTED: the XUL treecols/treecol part.
fn CanUse(aFrame: nsIFrame) -> bool {
    aFrame.IsFlexOrGridContainer()
}

// ADAPTER: the order of a frame under aOrderProp.
fn OrderOf(aFrame: nsIFrame, aOrderProp: CSSOrderAwareFrameIterator_OrderingProperty) -> i32 {
    match aOrderProp {
        CSSOrderAwareFrameIterator_OrderingProperty::BoxOrdinalGroup => {
            unimplemented!("NOT PORTED: box-ordinal-group (nsStyleXUL::mBoxOrdinal)")
        }
        CSSOrderAwareFrameIterator_OrderingProperty::Order => aFrame.StylePosition().mOrder(),
    }
}

impl<Iterator: CSSOrderAwareTraversal> CSSOrderAwareFrameIteratorT<Iterator> {
    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::CSSOrderAwareFrameIteratorT
    pub fn new(aContainer: nsIFrame, aListID: FrameChildListID, aFilter: CSSOrderAwareFrameIterator_ChildFilter, aState: CSSOrderAwareFrameIterator_OrderState, aOrderProp: CSSOrderAwareFrameIterator_OrderingProperty) -> Self {
        let mChildren = aContainer.GetChildList(aListID);
        let mut this = CSSOrderAwareFrameIteratorT {
            mChildren,
            mIter: None,
            mIterEnd: None,
            mArray: None,
            mArrayIndex: 0,
            mItemIndex: 0,
            mItemCount: None,
            mSkipPlaceholders: aFilter == CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders,
            #[cfg(debug_assertions)]
            mContainer: aContainer,
            #[cfg(debug_assertions)]
            mListID: aListID,
        };
        MOZ_ASSERT!(CanUse(aContainer), "Only use this iterator in a container that honors 'order'");

        let mut count: usize = 0;
        let mut isOrdered = aState != CSSOrderAwareFrameIterator_OrderState::Unordered;
        if aState == CSSOrderAwareFrameIterator_OrderState::Unknown {
            let mut maxOrder = i32::MIN;
            for child in mChildren {
                count += 1;

                let order = OrderOf(child, aOrderProp);

                if order < maxOrder {
                    isOrdered = false;
                    break;
                }
                maxOrder = order;
            }
        }
        if isOrdered {
            this.mIter = Some(Iterator::begin(mChildren));
            this.mIterEnd = Some(Iterator::end(mChildren));
        } else {
            count *= 2; // XXX somewhat arbitrary estimate for now...
            let mut array: Vec<nsIFrame> = Vec::with_capacity(count);
            let mut i = Iterator::begin(mChildren);
            let iEnd = Iterator::end(mChildren);
            while i != iEnd {
                array.push(i.operator_star().unwrap());
                i.operator_inc();
            }
            // mArray->StableSort(comparator) (Vec::sort_by is stable)
            array.sort_by(|a, b| Iterator::Compare(OrderOf(*a, aOrderProp), OrderOf(*b, aOrderProp)));
            this.mArray = Some(array);
        }

        if this.mSkipPlaceholders {
            this.SkipPlaceholders();
        }
        this
    }

    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::IsForward
    pub fn IsForward(&self) -> bool {
        Iterator::IsForward()
    }

    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::get (and operator*)
    pub fn get(&self) -> nsIFrame {
        MOZ_ASSERT!(!self.AtEnd());
        if let Some(iter) = &self.mIter {
            return iter.operator_star().unwrap();
        }
        self.mArray.as_ref().unwrap()[self.mArrayIndex]
    }

    /**
     * Return the child index of the current item, placeholders not counted.
     * It's forbidden to call this method when the current frame is placeholder.
     */
    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::ItemIndex
    pub fn ItemIndex(&self) -> usize {
        MOZ_ASSERT!(!self.AtEnd());
        MOZ_ASSERT!(!self.get().IsPlaceholderFrame(), "MUST not call this when at a placeholder");
        MOZ_ASSERT!(self.IsForward() || self.mItemIndex < self.mItemCount.unwrap(), "Returning an out-of-range mItemIndex...");
        self.mItemIndex
    }

    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::SetItemCount
    pub fn SetItemCount(&mut self, aItemCount: usize) {
        MOZ_ASSERT!(self.mIter.is_some() || aItemCount <= self.mArray.as_ref().unwrap().len(), "item count mismatch");
        self.mItemCount = Some(aItemCount);
        // Note: it's OK if mItemIndex underflows -- ItemIndex()
        // will not be called unless there is at least one item.
        self.mItemIndex = if self.IsForward() { 0 } else { aItemCount.wrapping_sub(1) };
    }

    /**
     * Skip over placeholder children.
     */
    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::SkipPlaceholders
    pub fn SkipPlaceholders(&mut self) {
        if self.mIter.is_some() {
            while self.mIter != self.mIterEnd {
                let child = self.mIter.as_ref().unwrap().operator_star().unwrap();
                if !child.IsPlaceholderFrame() {
                    return;
                }
                self.mIter.as_mut().unwrap().operator_inc();
            }
        } else {
            let array = self.mArray.as_ref().unwrap();
            while self.mArrayIndex < array.len() {
                let child = array[self.mArrayIndex];
                if !child.IsPlaceholderFrame() {
                    return;
                }
                self.mArrayIndex += 1;
            }
        }
    }

    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::AtEnd
    pub fn AtEnd(&self) -> bool {
        MOZ_ASSERT!(self.mIter.is_some() || self.mArrayIndex <= self.mArray.as_ref().unwrap().len());
        if self.mIter.is_some() { self.mIter == self.mIterEnd } else { self.mArrayIndex >= self.mArray.as_ref().unwrap().len() }
    }

    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::Next
    pub fn Next(&mut self) {
        #[cfg(debug_assertions)]
        {
            MOZ_ASSERT!(!self.AtEnd());
            let list = self.mContainer.GetChildList(self.mListID);
            MOZ_ASSERT!(list.FirstChild() == self.mChildren.FirstChild() && list.LastChild() == self.mChildren.LastChild(), "the list of child frames must not change while iterating!");
        }
        if self.mSkipPlaceholders || !self.get().IsPlaceholderFrame() {
            if self.IsForward() {
                self.mItemIndex = self.mItemIndex.wrapping_add(1);
            } else {
                self.mItemIndex = self.mItemIndex.wrapping_sub(1);
            }
        }
        if self.mIter.is_some() {
            self.mIter.as_mut().unwrap().operator_inc();
        } else {
            self.mArrayIndex += 1;
        }
        if self.mSkipPlaceholders {
            self.SkipPlaceholders();
        }
    }

    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::Reset
    // (aFilter defaults to SkipPlaceholders in C++)
    pub fn Reset(&mut self, aFilter: CSSOrderAwareFrameIterator_ChildFilter) {
        if self.mIter.is_some() {
            self.mIter = Some(Iterator::begin(self.mChildren));
            self.mIterEnd = Some(Iterator::end(self.mChildren));
        } else {
            self.mArrayIndex = 0;
        }
        self.mItemIndex = if self.IsForward() { 0 } else { self.mItemCount.unwrap().wrapping_sub(1) };
        self.mSkipPlaceholders = aFilter == CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders;
        if self.mSkipPlaceholders {
            self.SkipPlaceholders();
        }
    }

    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::IsValid
    pub fn IsValid(&self) -> bool {
        self.mIter.is_some() || self.mArray.is_some()
    }

    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::Invalidate
    pub fn Invalidate(&mut self) {
        self.mIter = None;
        self.mArray = None;
    }

    // Gecko: layout/generic/CSSOrderAwareFrameIterator.h CSSOrderAwareFrameIteratorT::ItemsAreAlreadyInOrder
    pub fn ItemsAreAlreadyInOrder(&self) -> bool {
        self.mIter.is_some()
    }
}
