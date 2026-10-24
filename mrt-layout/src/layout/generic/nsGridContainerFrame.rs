// Translated from layout/generic/nsGridContainerFrame.h and nsGridContainerFrame.cpp (GECKO_REV), stage 2b.
// PART 1 of 6: the header, and the .cpp up to the end of struct TrackSizingFunctions (line 1911).
//
// The file follows the order of the .cpp. The header's declarations come first (the frame class, its
// nested helper types that are used before the .cpp defines them), then the .cpp.
//
// Rust shape (see CONVENTIONS.md and the notes of nsFlexContainerFrame.rs):
//  - `nsGridContainerFrame` is a `Copy` handle over the frame (an nsIFrame of type GridContainer) that derefs
//    to nsIFrame, as the C++ class derives from nsContainerFrame. The data members (`mCachedIntrinsicSizes`,
//    `mBaseline`) live in one heap block per frame behind a frame property (nsGridContainerFrame_Members).
//  - What is virtual in C++ is a `match` on the frame type in nsIFrame.rs / nsLineLayoutAdapter.rs /
//    nsContainerFrame.rs (`Reflow`, `IntrinsicISize`, `AppendFrames`, ...).
//  - The nested types `nsGridContainerFrame::X` are top-level `nsGridContainerFrame_X`, each with the short
//    alias that the C++ code uses inside the class (`pub type TrackSize = nsGridContainerFrame_TrackSize;`).
//    All other code uses the short names.
//  - Nested enums with bitwise operators (TrackSize::StateBits, GridItemInfo::StateBits) are MOZ_ENUM_CLASS_BITS
//    newtypes: `TrackSize::eFrozen` is `TrackSize_StateBits::eFrozen`, `bool(mState & eFrozen)` is
//    `(mState & TrackSize_StateBits::eFrozen).to_bool()`. `ItemState` is `GridItemInfo_StateBits`.
//  - `LineRange` has a union of `uint32_t mStart` and `int32_t mUntranslatedStart`: the field is the u32
//    `mStart` and `mUntranslatedStart()` / `mUntranslatedStart_mut()` give the same bits as i32 (same for the end).
//  - `GridItemInfo::mBaselineOffset` and `mState` are `mutable` in C++: they are `PerLogicalAxis<Cell<..>>`
//    here, read with `.get()` and written with `.set(..)` (`item.mState[axis].set(item.mState[axis].get() | bits)`).
//  - `mozilla::EnumeratedArray<E, T, N>` is the small types PerLogicalAxis, PerBaseline and
//    EnumeratedArray_GridIntrinsicSizeType below (index by the enum as in C++).
//  - Frame properties that hold a C++ object (`NS_DECLARE_FRAME_PROPERTY_DELETABLE(Prop, T)`) store a leaked
//    `Box<T>` pointer as `usize` (as nsFlexContainerFrame.rs does). Each such type has `Prop()` (the descriptor),
//    `GetProp(frame) -> Option<&'static mut T>`, `SetProp(frame, Box<T>)` and `RemoveProp(frame)`; where the C++
//    uses `GetOrCreateDeletableProperty`, `GetOrCreateProp(frame)` (types with Default).
//  - Pointers into an `nsTArray<GridItemInfo>` that are stored in a struct (ItemBaselineData::mGridItem,
//    FindItemInGridOrderResult::mItem) are indexes into that array, as the field comments say.
//  - `TrackSizingFunctions` borrows the style values it was made from, so it has a lifetime parameter.
//
// NOT PORTED: subgrid (the data stays; IsSubgrid() is false because the frame state bits are never set, the
// function that would create a subgrid panics), masonry (IsMasonry() is false), fragmentation, absolutely
// positioned children, painting (BuildDisplayList), accessibility, the devtools data (ComputedGridTrackInfo,
// ComputedGridLineInfo, GetGridFrameWithComputedInfo, the Grid DOM API properties), dump functions
// (DEBUG_FRAME_DUMP, `Dump()`), the nsILineIterator implementation, MOZ_LOG logging (GRID_LOG expands to
// nothing). Each is marked where it occurs. Refcount logging (MOZ_COUNT_CTOR) is not ported.
use crate::*;
use mrt_style::style::values::generics::grid::{GenericGridTemplateComponent, GenericTrackBreadth, GenericTrackListValue, GenericTrackRepeat, GenericTrackSize, RepeatCount};
use mrt_style::style::servo_arc::Arc as ServoArc;
use std::cell::Cell;
use std::collections::HashMap;
use std::ops::{Deref, Index, IndexMut};

// ---------------------------------------------------------------------------------------------
// ADAPTER: mozilla::EnumeratedArray for the enums this file indexes arrays with.
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::PerLogicalAxis
// (`mozilla::EnumeratedArray<LogicalAxis, T, 2>`): Block is index 0, Inline is index 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PerLogicalAxis<T>(pub [T; 2]);

impl<T> Index<LogicalAxis> for PerLogicalAxis<T> {
    type Output = T;
    fn index(&self, aAxis: LogicalAxis) -> &T {
        &self.0[aAxis as usize]
    }
}
impl<T> IndexMut<LogicalAxis> for PerLogicalAxis<T> {
    fn index_mut(&mut self, aAxis: LogicalAxis) -> &mut T {
        &mut self.0[aAxis as usize]
    }
}

// Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::PerBaseline
// (`mozilla::EnumeratedArray<BaselineSharingGroup, T, 2>`): First is index 0, Last is index 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PerBaseline<T>(pub [T; 2]);

impl<T> Index<BaselineSharingGroup> for PerBaseline<T> {
    type Output = T;
    fn index(&self, aGroup: BaselineSharingGroup) -> &T {
        &self.0[aGroup as usize]
    }
}
impl<T> IndexMut<BaselineSharingGroup> for PerBaseline<T> {
    fn index_mut(&mut self, aGroup: BaselineSharingGroup) -> &mut T {
        &mut self.0[aGroup as usize]
    }
}

// ADAPTER: `EnumeratedArray<GridIntrinsicSizeType, T>` (the size is MaxContiguousEnumValue<GridIntrinsicSizeType>
// plus one, see below).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EnumeratedArray_GridIntrinsicSizeType<T>(pub [T; 3]);

impl<T> Index<GridIntrinsicSizeType> for EnumeratedArray_GridIntrinsicSizeType<T> {
    type Output = T;
    fn index(&self, aType: GridIntrinsicSizeType) -> &T {
        &self.0[aType as usize]
    }
}
impl<T> IndexMut<GridIntrinsicSizeType> for EnumeratedArray_GridIntrinsicSizeType<T> {
    fn index_mut(&mut self, aType: GridIntrinsicSizeType) -> &mut T {
        &mut self.0[aType as usize]
    }
}

// ---------------------------------------------------------------------------------------------
// The header: layout/generic/nsGridContainerFrame.h
// ---------------------------------------------------------------------------------------------

// Gecko: layout/generic/nsGridContainerFrame.h mozilla::ComputedGridTrackInfo, mozilla::ComputedGridLineInfo
// NOT PORTED: devtools data (nsGridContainerFrame.h lines 36-80 at GECKO_REV); only
// GetGridFrameWithComputedInfo and the Grid DOM API read them. The frame properties GridColTrackInfo,
// GridRowTrackInfo, GridColumnLineInfo, GridRowLineInfo, GridFragmentInfo and their Get* accessors
// (GetComputedTemplateColumns, GetComputedTemplateRows, GetComputedTemplateColumnLines,
// GetComputedTemplateRowLines, GetGridFragmentInfo) are not ported either.

// Gecko: layout/generic/nsFrameStateBits.h, group GridContainer (nsGridContainerFrame). The bit numbers are
// this crate's own (see nsFrameState.rs); a grid container is never a flex container, so the bits that the
// flex container uses are reused where the meaning is the same, and the free numbers 42, 43, 49, 52, 58, 59
// are used for the others.
// True iff the normal flow children are already in CSS 'order' in the order they occur in the child frame list.
pub const NS_STATE_GRID_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER: nsFrameState = 1 << 51;
// True iff some first-in-flow in-flow children were pushed (fragmentation: NOT PORTED but defined).
pub const NS_STATE_GRID_DID_PUSH_ITEMS: nsFrameState = 1 << 54;
// True if the container has no grid items; may lie if there is a pending reflow.
pub const NS_STATE_GRID_SYNTHESIZE_BASELINE: nsFrameState = 1 << 53;
// True iff the grid container is a subgrid in the column / row axis (NOT PORTED, never set).
pub const NS_STATE_GRID_IS_COL_SUBGRID: nsFrameState = 1 << 52;
pub const NS_STATE_GRID_IS_ROW_SUBGRID: nsFrameState = 1 << 58;
// True iff the grid container has a subgrid item in the column / row axis (NOT PORTED, never set).
pub const NS_STATE_GRID_HAS_COL_SUBGRID_ITEM: nsFrameState = 1 << 59;
pub const NS_STATE_GRID_HAS_ROW_SUBGRID_ITEM: nsFrameState = 1 << 49;
// True iff some child frames were pushed to the next-in-flow's child list (fragmentation: NOT PORTED but defined).
pub const NS_STATE_GRID_HAS_CHILD_NIFS: nsFrameState = 1 << 55;
// True iff the grid container has masonry layout in the column / row axis (NOT PORTED, never set).
pub const NS_STATE_GRID_IS_COL_MASONRY: nsFrameState = 1 << 42;
pub const NS_STATE_GRID_IS_ROW_MASONRY: nsFrameState = 1 << 43;
// True iff computed grid info (for devtools) was generated (NOT PORTED but defined).
pub const NS_STATE_GRID_COMPUTED_INFO: nsFrameState = 1 << 56;

// Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame
// ADAPTER: a handle (see the notes at the top of the file).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct nsGridContainerFrame(pub nsIFrame);

impl Deref for nsGridContainerFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// ADAPTER: the data members of the C++ class nsGridContainerFrame that are plain data, in one heap block per
// frame reached through a frame property (see nsFlexContainerFrame_Members for the scheme). The block lives as
// long as the frame (frames are never freed).
pub struct nsGridContainerFrame_Members {
    // Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::mCachedIntrinsicSizes
    pub mCachedIntrinsicSizes: IntrinsicISizesCache,
    // Our baselines, one per BaselineSharingGroup per axis.
    // Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::mBaseline
    pub mBaseline: PerLogicalAxis<PerBaseline<nscoord>>,
}

// (ids: this crate's own numbers; 7200..7299 belong to the grid container and its items.)
// Cross-part items: these are `pub fn`s written out, not NS_DECLARE_FRAME_PROPERTY_DELETABLE!, because the
// macro defines a private function and the parts are separate modules until they are joined.
pub fn GridMembersProperty() -> FramePropertyDescriptor<usize> {
    FramePropertyDescriptor::new(7200)
}
pub fn CachedBAxisMeasurementProperty() -> FramePropertyDescriptor<usize> {
    FramePropertyDescriptor::new(7201)
}
pub fn SubgridProperty() -> FramePropertyDescriptor<usize> {
    FramePropertyDescriptor::new(7202)
}
pub fn UsedTrackSizesProperty() -> FramePropertyDescriptor<usize> {
    FramePropertyDescriptor::new(7203)
}
pub fn SharedGridDataProperty() -> FramePropertyDescriptor<usize> {
    FramePropertyDescriptor::new(7204)
}
// Gecko: layout/generic/nsGridContainerFrame.h NS_DECLARE_FRAME_PROPERTY_DELETABLE(GridItemContainingBlockRect, nsRect)
// (the containing block of an abs.pos. child: NOT PORTED, abspos boxes get no frame)
pub fn GridItemContainingBlockRect() -> FramePropertyDescriptor<nsRect> {
    FramePropertyDescriptor::new(7205)
}
pub fn ImplicitNamedAreasProperty() -> FramePropertyDescriptor<usize> {
    FramePropertyDescriptor::new(7206)
}
pub fn ExplicitNamedAreasProperty() -> FramePropertyDescriptor<usize> {
    FramePropertyDescriptor::new(7207)
}

// ADAPTER: reading, writing and removing a frame property that holds a leaked `Box<T>` as a `usize`.
// `GetProperty` on the frame gives the number; these turn it into the object. Single threaded (the frame
// tree is thread local), so the `&'static mut` is not shared across threads.
pub fn GridPropGet<T>(aFrame: nsIFrame, aProp: FramePropertyDescriptor<usize>) -> Option<&'static mut T> {
    aFrame.GetProperty(aProp).map(|p| unsafe { &mut *(p as *mut T) })
}
pub fn GridPropSet<T: 'static>(aFrame: nsIFrame, aProp: FramePropertyDescriptor<usize>, aValue: Box<T>) {
    aFrame.SetOwnedPointerProperty::<T>(aProp, Box::into_raw(aValue) as usize);
}
pub fn GridPropRemove<T>(aFrame: nsIFrame, aProp: FramePropertyDescriptor<usize>) {
    if let Some(p) = aFrame.GetProperty(aProp.clone_desc()) {
        drop(unsafe { Box::from_raw(p as *mut T) });
        aFrame.RemoveProperty(aProp);
    }
}
pub fn GridPropGetOrCreate<T: Default + 'static>(aFrame: nsIFrame, aProp: FramePropertyDescriptor<usize>) -> &'static mut T {
    match aFrame.GetProperty(aProp.clone_desc()) {
        Some(p) => unsafe { &mut *(p as *mut T) },
        None => {
            let p = Box::into_raw(Box::new(T::default()));
            aFrame.SetOwnedPointerProperty::<T>(aProp, p as usize);
            unsafe { &mut *p }
        }
    }
}

impl<T> FramePropertyDescriptor<T> {
    // ADAPTER: the descriptor is a number, copying it is free (FramePropertyDescriptor has no Clone derive).
    pub fn clone_desc(&self) -> FramePropertyDescriptor<T> {
        FramePropertyDescriptor::new(self.mId)
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp NS_NewGridContainerFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (what the C++ constructor
// passes as kClassID), see NS_NewFlexContainerFrame.
pub fn NS_NewGridContainerFrame(aFrame: nsIFrame) -> nsGridContainerFrame {
    aFrame.SetType(LayoutFrameType::GridContainer);
    nsGridContainerFrame(aFrame)
}

// Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::ImplicitNamedAreas
// (`HashMap<AtomHashKey, NamedArea, AtomHashKey>`; nsAtom is `&'static str` in this crate)
pub type ImplicitNamedAreas = HashMap<nsAtom, NamedArea>;
// Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::NamedArea (`mozilla::StyleNamedArea`)
pub type NamedArea = StyleNamedArea;
// Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::ExplicitNamedAreas
// (`StyleOwnedSlice<NamedArea>`; the slice is owned by the style's grid-template-areas value and is only
// pointed to by the frame property, see GetExplicitNamedAreas)
pub type ExplicitNamedAreas = mrt_style::style::OwnedSlice<NamedArea>;

// Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::SizingConstraint
// https://drafts.csswg.org/css-sizing/#constraints
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsGridContainerFrame_SizingConstraint {
    MinContent,   // sizing under min-content constraint
    MaxContent,   // sizing under max-content constraint
    NoConstraint, // no constraint, used during Reflow
}
pub type SizingConstraint = nsGridContainerFrame_SizingConstraint;

// Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::FindItemInGridOrderResult
pub struct nsGridContainerFrame_FindItemInGridOrderResult {
    // The first(last) item in (reverse) grid order.
    // (a `const GridItemInfo*` in C++: here the index into the `aGridItems` array that was searched)
    pub mItem: Option<usize>,
    // Does the above item span the first(last) track?
    pub mIsInEdgeTrack: bool,
}
pub type FindItemInGridOrderResult = nsGridContainerFrame_FindItemInGridOrderResult;

// Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::BaselineSet
// (an unscoped C++ enum with the values used as bit masks; `BaselineSet::eFirst` here)
MOZ_ENUM_CLASS_BITS! {
    pub struct nsGridContainerFrame_BaselineSet: u32 {
        eNone = 0x0,
        eFirst = 0x1,
        eLast = 0x2,
        eBoth = 0x1 | 0x2,
    }
}
pub type BaselineSet = nsGridContainerFrame_BaselineSet;

// Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::Fragmentainer
// (the helpers for ReflowChildren; fragmentation is NOT PORTED but the type is kept)
#[derive(Clone, Copy, Debug)]
pub struct nsGridContainerFrame_Fragmentainer {
    /**
     * The distance from the first grid container fragment's block-axis content
     * edge to the fragmentainer end.
     */
    pub mToFragmentainerEnd: nscoord,
    /**
     * True if the current fragment is at the start of the fragmentainer.
     */
    pub mIsTopOfPage: bool,
    /**
     * Is there a Class C break opportunity at the start content edge?
     */
    pub mCanBreakAtStart: bool,
    /**
     * Is there a Class C break opportunity at the end content edge?
     */
    pub mCanBreakAtEnd: bool,
    /**
     * Is the grid container's block-size unconstrained?
     */
    pub mIsAutoBSize: bool,
}
pub type Fragmentainer = nsGridContainerFrame_Fragmentainer;

impl nsGridContainerFrame {
    // static_cast<nsGridContainerFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsGridContainerFrame {
        nsGridContainerFrame(aFrame)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp NS_QUERYFRAME_HEAD(nsGridContainerFrame)
    // do_QueryFrame<nsGridContainerFrame>(frame)
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsGridContainerFrame> {
        if aFrame.Type() == LayoutFrameType::GridContainer {
            Some(nsGridContainerFrame(aFrame))
        } else {
            None
        }
    }

    // ADAPTER: this frame's data members (see nsGridContainerFrame_Members). Created on first use.
    // This is the constructor body of the C++ class (every baseline starts as NS_INTRINSIC_ISIZE_UNKNOWN).
    // Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::nsGridContainerFrame
    pub fn Members(&self) -> &'static mut nsGridContainerFrame_Members {
        let p = match self.0.GetProperty(GridMembersProperty()) {
            Some(p) => p,
            None => {
                let p = Box::into_raw(Box::new(nsGridContainerFrame_Members {
                    mCachedIntrinsicSizes: IntrinsicISizesCache::default(),
                    mBaseline: PerLogicalAxis([PerBaseline([NS_INTRINSIC_ISIZE_UNKNOWN; 2]); 2]),
                })) as usize;
                self.0.SetOwnedPointerProperty::<nsGridContainerFrame_Members>(GridMembersProperty(), p);
                p
            }
        };
        unsafe { &mut *(p as *mut nsGridContainerFrame_Members) }
    }

    // Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::GetNaturalBaselineBOffset
    pub fn GetNaturalBaselineBOffset(&self, aWM: WritingMode, aBaselineGroup: BaselineSharingGroup, aExportContext: BaselineExportContext) -> Option<nscoord> {
        if self.StyleDisplay().IsContainLayout() || self.HasAnyStateBits(NS_STATE_GRID_SYNTHESIZE_BASELINE) {
            return None;
        }
        Some(self.GetBBaseline(aBaselineGroup))
    }

    // Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::GetImplicitNamedAreas
    // (the map is owned by the frame property)
    pub fn GetImplicitNamedAreas(&self) -> Option<&'static mut ImplicitNamedAreas> {
        GridPropGet::<ImplicitNamedAreas>(self.0, ImplicitNamedAreasProperty())
    }

    // Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::GetExplicitNamedAreas
    // ADAPTER: the property holds a pointer to the style's own slice (InitImplicitNamedAreas stores it); the
    // style value outlives the reflow that reads it.
    pub fn GetExplicitNamedAreas(&self) -> Option<&'static ExplicitNamedAreas> {
        self.0.GetProperty(ExplicitNamedAreasProperty()).map(|p| unsafe { &*(p as *const ExplicitNamedAreas) })
    }

    // using nsContainerFrame::IsMasonry;
    // NOT PORTED: masonry. `IsMasonry(LogicalAxis)` is `IsMasonry__LogicalAxis` (defined in the .cpp part).
    /** Return true if this frame has masonry layout in any axis. */
    pub fn IsMasonry__(&self) -> bool {
        self.HasAnyStateBits(NS_STATE_GRID_IS_ROW_MASONRY | NS_STATE_GRID_IS_COL_MASONRY)
    }
    pub fn IsColMasonry(&self) -> bool {
        self.HasAnyStateBits(NS_STATE_GRID_IS_COL_MASONRY)
    }
    pub fn IsRowMasonry(&self) -> bool {
        self.HasAnyStateBits(NS_STATE_GRID_IS_ROW_MASONRY)
    }

    /** Return true if this frame is subgridded in its aAxis. */
    pub fn IsSubgrid__LogicalAxis(&self, aAxis: LogicalAxis) -> bool {
        self.HasAnyStateBits(if aAxis == LogicalAxis::Block { NS_STATE_GRID_IS_ROW_SUBGRID } else { NS_STATE_GRID_IS_COL_SUBGRID })
    }
    pub fn IsColSubgrid(&self) -> bool {
        self.IsSubgrid__LogicalAxis(LogicalAxis::Inline)
    }
    pub fn IsRowSubgrid(&self) -> bool {
        self.IsSubgrid__LogicalAxis(LogicalAxis::Block)
    }
    /** Return true if this frame is subgridded in any axis. */
    pub fn IsSubgrid__(&self) -> bool {
        self.HasAnyStateBits(NS_STATE_GRID_IS_ROW_SUBGRID | NS_STATE_GRID_IS_COL_SUBGRID)
    }

    /** Return true if this frame has an item that is subgridded in our aAxis. */
    pub fn HasSubgridItems__LogicalAxis(&self, aAxis: LogicalAxis) -> bool {
        self.HasAnyStateBits(if aAxis == LogicalAxis::Block { NS_STATE_GRID_HAS_ROW_SUBGRID_ITEM } else { NS_STATE_GRID_HAS_COL_SUBGRID_ITEM })
    }
    /** Return true if this frame has any subgrid items. */
    pub fn HasSubgridItems__(&self) -> bool {
        self.HasAnyStateBits(NS_STATE_GRID_HAS_ROW_SUBGRID_ITEM | NS_STATE_GRID_HAS_COL_SUBGRID_ITEM)
    }

    // Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::GetBBaseline
    pub fn GetBBaseline(&self, aBaselineGroup: BaselineSharingGroup) -> nscoord {
        self.Members().mBaseline[LogicalAxis::Block][aBaselineGroup]
    }
    // Gecko: layout/generic/nsGridContainerFrame.h nsGridContainerFrame::GetIBaseline
    pub fn GetIBaseline(&self, aBaselineGroup: BaselineSharingGroup) -> nscoord {
        self.Members().mBaseline[LogicalAxis::Inline][aBaselineGroup]
    }
}

// ---------------------------------------------------------------------------------------------
// The .cpp: layout/generic/nsGridContainerFrame.cpp
// ---------------------------------------------------------------------------------------------

// using AlignJustifyFlag = CSSAlignUtils::AlignJustifyFlag;
// using AlignJustifyFlags = CSSAlignUtils::AlignJustifyFlags;
// using GridItemCachedBAxisMeasurement = nsGridContainerFrame::CachedBAxisMeasurement;
pub type GridItemCachedBAxisMeasurement = nsGridContainerFrame_CachedBAxisMeasurement;
// using GridTemplate = StyleGridTemplateComponent;
pub type GridTemplate = GenericGridTemplateComponent<LengthPercentage, i32>;
// using NameList = StyleOwnedSlice<StyleCustomIdent>;
pub type NameList = mrt_style::style::OwnedSlice<StyleCustomIdent>;
// using TrackListValue = StyleGenericTrackListValue<LengthPercentage, StyleInteger>;
pub type TrackListValue = GenericTrackListValue<LengthPercentage, i32>;
// using TrackRepeat = StyleGenericTrackRepeat<LengthPercentage, StyleInteger>;
pub type TrackRepeat = GenericTrackRepeat<LengthPercentage, i32>;
// using TrackSize = nsGridContainerFrame::TrackSize;
pub type TrackSize = nsGridContainerFrame_TrackSize;
// (the `using SizingConstraint` is the alias above)

// (StyleTrackBreadth, StyleTrackSize and StyleImplicitGridTracks are aliases in ServoStyleConsts.rs.)

// GRID_LOG(...) is MOZ_LOG(gGridContainerLog, LogLevel::Debug, ...)
// NOT PORTED: logging (the LazyLogModule gGridContainerLog is not ported either).
macro_rules! GRID_LOG {
    ($($t:tt)*) => {};
}

// These are the limits that we choose to clamp grid line numbers to.
// http://drafts.csswg.org/css-grid/#overlarge-grids
pub const kMaxLine: i32 = 10000;
pub const kMinLine: i32 = -10000;
// The maximum line number, in the zero-based translated grid.
pub const kTranslatedMaxLine: u32 = (kMaxLine - kMinLine) as u32;
pub const kAutoLine: u32 = kTranslatedMaxLine + 3457u32;

pub const kIsSubgridBits: nsFrameState = NS_STATE_GRID_IS_COL_SUBGRID | NS_STATE_GRID_IS_ROW_SUBGRID;

// ADAPTER: the tag of a StyleTrackBreadth with Gecko's names (cbindgen's `StyleTrackBreadth::Tag`; Stylo's
// `Fr` is `Flex` there).
// (servo/ServoStyleConsts.h StyleTrackBreadth::Tag; generated by cbindgen, so no drift marker)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleTrackBreadth_Tag {
    Breadth,
    Flex,
    Auto,
    MinContent,
    MaxContent,
}

// ADAPTER: Gecko's StyleTrackBreadth methods (ServoStyleConstsInlines.h and the cbindgen output).
pub trait StyleTrackBreadthExt {
    fn tag(&self) -> StyleTrackBreadth_Tag;
    fn IsBreadth(&self) -> bool;
    fn IsFlex(&self) -> bool;
    fn IsAuto(&self) -> bool;
    fn IsMinContent(&self) -> bool;
    fn IsMaxContent(&self) -> bool;
    fn AsBreadth(&self) -> &LengthPercentage;
    fn AsFlex(&self) -> f32;
    // Gecko: layout/style/ServoStyleConstsInlines.h StyleTrackBreadth::HasPercent
    fn HasPercent(&self) -> bool;
}

impl StyleTrackBreadthExt for StyleTrackBreadth {
    fn tag(&self) -> StyleTrackBreadth_Tag {
        match self {
            GenericTrackBreadth::Breadth(_) => StyleTrackBreadth_Tag::Breadth,
            GenericTrackBreadth::Flex(_) => StyleTrackBreadth_Tag::Flex,
            GenericTrackBreadth::Auto => StyleTrackBreadth_Tag::Auto,
            GenericTrackBreadth::MinContent => StyleTrackBreadth_Tag::MinContent,
            GenericTrackBreadth::MaxContent => StyleTrackBreadth_Tag::MaxContent,
        }
    }
    fn IsBreadth(&self) -> bool {
        matches!(self, GenericTrackBreadth::Breadth(_))
    }
    fn IsFlex(&self) -> bool {
        matches!(self, GenericTrackBreadth::Flex(_))
    }
    fn IsAuto(&self) -> bool {
        matches!(self, GenericTrackBreadth::Auto)
    }
    fn IsMinContent(&self) -> bool {
        matches!(self, GenericTrackBreadth::MinContent)
    }
    fn IsMaxContent(&self) -> bool {
        matches!(self, GenericTrackBreadth::MaxContent)
    }
    fn AsBreadth(&self) -> &LengthPercentage {
        match self {
            GenericTrackBreadth::Breadth(lp) => lp,
            _ => panic!("AsBreadth on a track breadth that is not a <length-percentage>"),
        }
    }
    fn AsFlex(&self) -> f32 {
        match self {
            GenericTrackBreadth::Flex(f) => f.0,
            _ => panic!("AsFlex on a track breadth that is not a <flex>"),
        }
    }
    fn HasPercent(&self) -> bool {
        self.IsBreadth() && self.AsBreadth().HasPercent()
    }
}

// ADAPTER: Gecko's StyleTrackSize methods (the cbindgen output; GetMax and GetMin are the template
// specializations at the top of the .cpp, below).
pub trait StyleTrackSizeExt {
    fn IsBreadth(&self) -> bool;
    fn IsMinmax(&self) -> bool;
    fn IsFitContent(&self) -> bool;
    fn AsBreadth(&self) -> &StyleTrackBreadth;
    // `AsMinmax()._0` and `._1` are `.0` and `.1` of the returned pair
    fn AsMinmax(&self) -> (&StyleTrackBreadth, &StyleTrackBreadth);
    fn AsFitContent(&self) -> &StyleTrackBreadth;
    fn GetMax(&self) -> &StyleTrackBreadth;
    fn GetMin(&self) -> &StyleTrackBreadth;
}

// ADAPTER: the C++ `static const StyleTrackBreadth kAuto = StyleTrackBreadth::Auto();` (a Rust static would
// need Sync): one leaked value per thread.
fn kAutoTrackBreadth() -> &'static StyleTrackBreadth {
    thread_local! {
        static K: &'static StyleTrackBreadth = Box::leak(Box::new(GenericTrackBreadth::Auto));
    }
    K.with(|k| *k)
}

// ADAPTER: the C++ `static const StyleTrackSize kAutoTrackSize = StyleTrackSize::Breadth(StyleTrackBreadth::Auto());`
fn kAutoTrackSize() -> &'static StyleTrackSize {
    thread_local! {
        static K: &'static StyleTrackSize = Box::leak(Box::new(GenericTrackSize::Breadth(GenericTrackBreadth::Auto)));
    }
    K.with(|k| *k)
}

impl StyleTrackSizeExt for StyleTrackSize {
    fn IsBreadth(&self) -> bool {
        matches!(self, GenericTrackSize::Breadth(_))
    }
    fn IsMinmax(&self) -> bool {
        matches!(self, GenericTrackSize::Minmax(..))
    }
    fn IsFitContent(&self) -> bool {
        matches!(self, GenericTrackSize::FitContent(_))
    }
    fn AsBreadth(&self) -> &StyleTrackBreadth {
        match self {
            GenericTrackSize::Breadth(b) => b,
            _ => panic!("AsBreadth on a track size that is not a breadth"),
        }
    }
    fn AsMinmax(&self) -> (&StyleTrackBreadth, &StyleTrackBreadth) {
        match self {
            GenericTrackSize::Minmax(min, max) => (min, max),
            _ => panic!("AsMinmax on a track size that is not minmax()"),
        }
    }
    fn AsFitContent(&self) -> &StyleTrackBreadth {
        match self {
            GenericTrackSize::FitContent(b) => b,
            _ => panic!("AsFitContent on a track size that is not fit-content()"),
        }
    }

    // template <>
    // inline const StyleTrackBreadth& StyleTrackSize::GetMax() const {
    // Gecko: layout/generic/nsGridContainerFrame.cpp mozilla::StyleTrackSize::GetMax
    fn GetMax(&self) -> &StyleTrackBreadth {
        if self.IsBreadth() {
            return self.AsBreadth();
        }
        if self.IsMinmax() {
            return self.AsMinmax().1;
        }
        MOZ_ASSERT!(self.IsFitContent());
        self.AsFitContent()
    }

    // template <>
    // inline const StyleTrackBreadth& StyleTrackSize::GetMin() const {
    // Gecko: layout/generic/nsGridContainerFrame.cpp mozilla::StyleTrackSize::GetMin
    fn GetMin(&self) -> &StyleTrackBreadth {
        let kAuto = kAutoTrackBreadth();
        if self.IsBreadth() {
            // <flex> behaves like minmax(auto, <flex>)
            return if self.AsBreadth().IsFlex() { kAuto } else { self.AsBreadth() };
        }
        if self.IsMinmax() {
            return self.AsMinmax().0;
        }
        MOZ_ASSERT!(self.IsFitContent());
        kAuto
    }
}

// ADAPTER: Gecko's StyleGridTemplateComponent methods from ServoStyleConstsInlines.h that the grid code calls
// (RepeatAutoIndex, HasRepeatAuto, TrackListValues, GetRepeatAutoValue); LineNameLists is the template
// specialization at the top of the .cpp. (IsNone, IsTrackList, IsSubgrid, IsMasonry, AsTrackList are
// StyleGridTemplateComponentInlines in ServoStyleConsts.rs.)
pub trait StyleGridTemplateComponentInlines {
    fn GetRepeatAutoValue(&self) -> Option<&TrackRepeat>;
    fn RepeatAutoIndex(&self) -> Option<usize>;
    fn HasRepeatAuto(&self) -> bool;
    fn TrackListValues(&self) -> &[TrackListValue];
    fn LineNameLists(&self, aIsSubgrid: bool) -> &[NameList];
}

impl StyleGridTemplateComponentInlines for GridTemplate {
    // Gecko: layout/style/ServoStyleConstsInlines.h StyleGridTemplateComponent::GetRepeatAutoValue
    fn GetRepeatAutoValue(&self) -> Option<&TrackRepeat> {
        let index = self.RepeatAutoIndex()?;
        match &self.TrackListValues()[index] {
            GenericTrackListValue::TrackRepeat(r) => Some(r),
            _ => None,
        }
    }

    // Gecko: layout/style/ServoStyleConstsInlines.h StyleGridTemplateComponent::RepeatAutoIndex
    fn RepeatAutoIndex(&self) -> Option<usize> {
        match self {
            GenericGridTemplateComponent::TrackList(list) => {
                if list.has_auto_repeat() {
                    Some(list.auto_repeat_index)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    // Gecko: layout/style/ServoStyleConstsInlines.h StyleGridTemplateComponent::HasRepeatAuto
    fn HasRepeatAuto(&self) -> bool {
        self.RepeatAutoIndex().is_some()
    }

    // Gecko: layout/style/ServoStyleConstsInlines.h StyleGridTemplateComponent::TrackListValues
    fn TrackListValues(&self) -> &[TrackListValue] {
        match self {
            GenericGridTemplateComponent::TrackList(list) => &list.values[..],
            _ => &[],
        }
    }

    // template <>
    // inline Span<const StyleOwnedSlice<StyleCustomIdent>>
    // GridTemplate::LineNameLists(bool aIsSubgrid) const {
    // Gecko: layout/generic/nsGridContainerFrame.cpp mozilla::StyleGridTemplateComponent::LineNameLists
    fn LineNameLists(&self, aIsSubgrid: bool) -> &[NameList] {
        if self.IsTrackList() {
            if let GenericGridTemplateComponent::TrackList(list) = self {
                return &list.line_names[..];
            }
        }
        if self.IsSubgrid() && aIsSubgrid {
            // For subgrid, we need to resolve <line-name-list> from each
            // StyleGenericLineNameListValue, so return empty.
            return &[];
        }
        MOZ_ASSERT!(self.IsNone() || self.IsMasonry() || (self.IsSubgrid() && !aIsSubgrid));
        &[]
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp ClampToCSSMaxBSize
fn ClampToCSSMaxBSize__nscoord_ReflowInput(mut aSize: nscoord, aReflowInput: &ReflowInput) -> nscoord {
    let maxSize = aReflowInput.ComputedMaxBSize();
    if maxSize != NS_UNCONSTRAINEDSIZE {
        MOZ_ASSERT!(aReflowInput.ComputedMinBSize() <= maxSize);
        aSize = std::cmp::min(aSize, maxSize);
    }
    aSize
}

// Same as above and set aStatus INCOMPLETE if aSize wasn't clamped.
// (If we clamp aSize it means our size is less than the break point,
// i.e. we're effectively breaking in our overflow, so we should leave
// aStatus as is (it will likely be set to OVERFLOW_INCOMPLETE later)).
// Gecko: layout/generic/nsGridContainerFrame.cpp ClampToCSSMaxBSize
fn ClampToCSSMaxBSize__nscoord_ReflowInput_nsReflowStatus(mut aSize: nscoord, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) -> nscoord {
    let maxSize = aReflowInput.ComputedMaxBSize();
    if maxSize != NS_UNCONSTRAINEDSIZE {
        MOZ_ASSERT!(aReflowInput.ComputedMinBSize() <= maxSize);
        if aSize < maxSize {
            aStatus.SetIncomplete();
        } else {
            aSize = maxSize;
        }
    } else {
        aStatus.SetIncomplete();
    }
    aSize
}

// ADAPTER: the C++ `template <typename Size> IsPercentOfIndefiniteSize` is called with a LengthPercentage
// and with a StyleTrackBreadth, both of which have HasPercent(); the two Rust types have no common trait of
// Gecko's, so this one is the bound.
pub trait GridHasPercent {
    fn GridHasPercent(&self) -> bool;
}
impl GridHasPercent for LengthPercentage {
    fn GridHasPercent(&self) -> bool {
        LengthPercentageExt::HasPercent(self)
    }
}
impl GridHasPercent for StyleTrackBreadth {
    fn GridHasPercent(&self) -> bool {
        StyleTrackBreadthExt::HasPercent(self)
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp IsPercentOfIndefiniteSize
pub fn IsPercentOfIndefiniteSize<Size: GridHasPercent>(aCoord: &Size, aPercentBasis: nscoord) -> bool {
    aPercentBasis == NS_UNCONSTRAINEDSIZE && aCoord.GridHasPercent()
}

// Gecko: layout/generic/nsGridContainerFrame.cpp ResolveToDefiniteSize
pub fn ResolveToDefiniteSize(aBreadth: &StyleTrackBreadth, aPercentBasis: nscoord) -> nscoord {
    MOZ_ASSERT!(aBreadth.IsBreadth());
    if IsPercentOfIndefiniteSize(aBreadth.AsBreadth(), aPercentBasis) {
        return 0;
    }
    std::cmp::max(0, aBreadth.AsBreadth().Resolve(aPercentBasis))
}

// Synthesize a baseline from a border box.  For an alphabetical baseline
// this is the end edge of the border box.  For a central baseline it's
// the center of the border box.
// https://drafts.csswg.org/css-align-3/#synthesize-baseline
// For a 'first baseline' the measure is from the border-box start edge and
// for a 'last baseline' the measure is from the border-box end edge.
//
// The 'LogicalAxis aAxis' represents the axis (in terms of aWM) that the
// baseline corresponds to.  (Typically, baselines are a measurement in the
// block axis; e.g. for English horizontal-tb text, a traditional baseline
// would be a y-axis measurement.  But in some cases (e.g. orthogonal WMs), we
// may need to synthesize a baseline in a child's inline axis, which is when
// this function might receive an aAxis of LogicalAxis::Inline. In that case, we
// assume that the writing mode's preference for central vs. alphabetic
// baselines is irrelevant, since that's a choice about its block-axis
// baselines, and we just unconditionally use the alphabetic baseline
// (e.g. border-box bottom edge).
// Gecko: layout/generic/nsGridContainerFrame.cpp SynthesizeBaselineFromBorderBox
pub fn SynthesizeBaselineFromBorderBox(aGroup: BaselineSharingGroup, aWM: WritingMode, aAxis: LogicalAxis, aBorderBoxSize: nscoord) -> nscoord {
    let useAlphabeticBaseline = if aAxis == LogicalAxis::Inline { true } else { aWM.IsAlphabeticalBaseline() };

    if aGroup == BaselineSharingGroup::First {
        return if useAlphabeticBaseline { aBorderBoxSize } else { aBorderBoxSize / 2 };
    }
    MOZ_ASSERT!(aGroup == BaselineSharingGroup::Last);
    // Round up for central baseline offset, to be consistent with eFirst.
    if useAlphabeticBaseline { 0 } else { (aBorderBoxSize / 2) + (aBorderBoxSize % 2) }
}

// The helper struct to hold the box sizing adjustment.
// Gecko: layout/generic/nsGridContainerFrame.cpp BoxSizingAdjustment
// ADAPTER: `const ComputedStyle& mStyle` is a clone of the style Arc (the style accessors are built from it).
pub struct BoxSizingAdjustment {
    mWM: WritingMode,
    mStyle: ServoArc<ComputedStyle>,
    // The wrapped value we would like to use for the box sizing adjustment.
    mValue: Option<LogicalSize>,
}

impl BoxSizingAdjustment {
    // BoxSizingAdjustment() = delete;
    // Gecko: layout/generic/nsGridContainerFrame.cpp BoxSizingAdjustment::BoxSizingAdjustment
    pub fn new(aWM: WritingMode, aStyle: &ServoArc<ComputedStyle>) -> BoxSizingAdjustment {
        BoxSizingAdjustment { mWM: aWM, mStyle: aStyle.clone(), mValue: None }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp BoxSizingAdjustment::EnsureAndGet
    pub fn EnsureAndGet(&mut self) -> &LogicalSize {
        if self.mValue.is_some() {
            return self.mValue.as_ref().unwrap();
        }

        if nsStylePosition::new(self.mStyle.clone()).mBoxSizing() != StyleBoxSizing::BorderBox {
            // Use default, (0, 0).
            self.mValue = Some(LogicalSize::new__WritingMode(self.mWM));
            return self.mValue.as_ref().unwrap();
        }

        let padding = nsStylePadding::new(self.mStyle.clone()).mPadding();
        let border = LogicalMargin::new__WritingMode_nsMargin(self.mWM, nsStyleBorder::new(self.mStyle.clone()).GetComputedBorder());
        // We can use zero percentage basis since this is only called from
        // intrinsic sizing code.
        let percentageBasis: nscoord = 0;
        let iBP = std::cmp::max(padding.GetIStart(self.mWM).Resolve(percentageBasis), 0) + std::cmp::max(padding.GetIEnd(self.mWM).Resolve(percentageBasis), 0) + border.IStartEnd(self.mWM);
        let bBP = std::cmp::max(padding.GetBStart(self.mWM).Resolve(percentageBasis), 0) + std::cmp::max(padding.GetBEnd(self.mWM).Resolve(percentageBasis), 0) + border.BStartEnd(self.mWM);
        self.mValue = Some(LogicalSize::new(self.mWM, iBP, bBP));
        self.mValue.as_ref().unwrap()
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp GetPercentageBasisForAR
pub fn GetPercentageBasisForAR(aRatioDeterminingAxis: LogicalAxis, aWM: WritingMode, aContainingBlockSize: &Option<LogicalSize>) -> Option<nscoord> {
    let aContainingBlockSize = match aContainingBlockSize {
        None => return None,
        Some(s) => s,
    };

    let basis = aContainingBlockSize.Size(aRatioDeterminingAxis, aWM);
    // If the basis is unconstrained (because we are still computing the
    // containing block size), we should treat it as no basis.
    if basis == NS_UNCONSTRAINEDSIZE { None } else { Some(basis) }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp ComputeTransferredSize
// (a template over `Type`, an AnchorResolvedSize or AnchorResolvedMaxSize: StyleSize or StyleMaxSize here)
pub fn ComputeTransferredSize<Type: StyleSizeLike>(aRatioDeterminingSize: &Type, aAxis: LogicalAxis, aWM: WritingMode, aAspectRatio: &AspectRatio, aBoxSizingAdjustment: &mut BoxSizingAdjustment, aContainingBlockSize: &Option<LogicalSize>) -> Option<nscoord> {
    // Use GetOrthogonalAxis() to get the ratio-determining axis.
    let basis = GetPercentageBasisForAR(GetOrthogonalAxis(aAxis), aWM, aContainingBlockSize);
    let rdSize: nscoord;
    if aRatioDeterminingSize.ConvertsToLength() {
        rdSize = aRatioDeterminingSize.ToLength();
    } else if aRatioDeterminingSize.HasPercent() && basis.is_some() {
        rdSize = aRatioDeterminingSize.AsLengthPercentage().Resolve(basis.unwrap());
    } else {
        // Either we are not using LengthPercentage or there is no percentage basis.
        return None;
    }
    Some(aAspectRatio.ComputeRatioDependentSize(aAxis, aWM, rdSize, *aBoxSizingAdjustment.EnsureAndGet()))
}

// A cached result for a grid item's block-axis measuring reflow. This
// cache prevents us from doing exponential reflows in cases of deeply
// nested grid frames.
//
// We store the cached value in the grid item's frame property table.
//
// We cache the following as a "key"
//   - The size of the grid area in the item's inline axis
//   - The item's block axis baseline padding
// ...and we cache the following as the "value",
//   - The item's border-box BSize
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::CachedBAxisMeasurement
pub struct nsGridContainerFrame_CachedBAxisMeasurement {
    mKey: nsGridContainerFrame_CachedBAxisMeasurement_Key,
    mBSize: nscoord,
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::CachedBAxisMeasurement::Key
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct nsGridContainerFrame_CachedBAxisMeasurement_Key {
    pub mCBSizeInItemInlineAxis: nscoord,
    pub mBaselinePaddingInItemBlockAxis: nscoord,
}

impl nsGridContainerFrame_CachedBAxisMeasurement_Key {
    // Gecko: layout/generic/nsGridContainerFrame.cpp CachedBAxisMeasurement::Key::Key
    pub fn new(aFrame: nsIFrame, aCBSize: &LogicalSize) -> Self {
        let mut key = nsGridContainerFrame_CachedBAxisMeasurement_Key { mCBSizeInItemInlineAxis: 0, mBaselinePaddingInItemBlockAxis: 0 };
        key.Update(aFrame, aCBSize);
        key
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp CachedBAxisMeasurement::Key::Update
    pub fn Update(&mut self, aFrame: nsIFrame, aCBSize: &LogicalSize) {
        self.mCBSizeInItemInlineAxis = aCBSize.ISize(aFrame.GetWritingMode());
        // (GetProperty of a small-value property: absent is zero)
        self.mBaselinePaddingInItemBlockAxis = aFrame.GetProperty(nsIFrame::BBaselinePadProperty()).unwrap_or(0);
    }
}

impl nsGridContainerFrame_CachedBAxisMeasurement {
    // NS_DECLARE_FRAME_PROPERTY_DELETABLE(Prop, CachedBAxisMeasurement)
    pub fn Prop() -> FramePropertyDescriptor<usize> {
        CachedBAxisMeasurementProperty()
    }
    pub fn GetProp(aFrame: nsIFrame) -> Option<&'static mut nsGridContainerFrame_CachedBAxisMeasurement> {
        GridPropGet::<nsGridContainerFrame_CachedBAxisMeasurement>(aFrame, Self::Prop())
    }
    pub fn SetProp(aFrame: nsIFrame, aValue: Box<nsGridContainerFrame_CachedBAxisMeasurement>) {
        GridPropSet(aFrame, Self::Prop(), aValue)
    }
    pub fn RemoveProp(aFrame: nsIFrame) {
        GridPropRemove::<nsGridContainerFrame_CachedBAxisMeasurement>(aFrame, Self::Prop())
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp CachedBAxisMeasurement::CachedBAxisMeasurement
    pub fn new(aFrame: nsIFrame, aCBSize: &LogicalSize, aBSize: nscoord) -> Self {
        nsGridContainerFrame_CachedBAxisMeasurement { mKey: nsGridContainerFrame_CachedBAxisMeasurement_Key::new(aFrame, aCBSize), mBSize: aBSize }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp CachedBAxisMeasurement::IsValidFor
    pub fn IsValidFor(&self, aFrame: nsIFrame, aCBSize: &LogicalSize) -> bool {
        if aFrame.IsSubtreeDirty() {
            return false;
        }
        self.mKey == nsGridContainerFrame_CachedBAxisMeasurement_Key::new(aFrame, aCBSize)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp CachedBAxisMeasurement::BSize
    pub fn BSize(&self) -> nscoord {
        self.mBSize
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp CachedBAxisMeasurement::Update
    pub fn Update(&mut self, aFrame: nsIFrame, aCBSize: &LogicalSize, aBSize: nscoord) {
        self.mKey.Update(aFrame, aCBSize);
        self.mBSize = aBSize;
    }
}

// The input sizes for calculating the number of repeat(auto-fill/fit) tracks.
// https://drafts.csswg.org/css-grid-2/#auto-repeat
// Gecko: layout/generic/nsGridContainerFrame.cpp RepeatTrackSizingInput
#[derive(Clone, Copy)]
pub struct RepeatTrackSizingInput {
    pub mMin: LogicalSize,
    pub mSize: LogicalSize,
    pub mMax: LogicalSize,
}

impl RepeatTrackSizingInput {
    // explicit RepeatTrackSizingInput(WritingMode aWM)
    // Gecko: layout/generic/nsGridContainerFrame.cpp RepeatTrackSizingInput::RepeatTrackSizingInput
    pub fn new__WritingMode(aWM: WritingMode) -> RepeatTrackSizingInput {
        RepeatTrackSizingInput { mMin: LogicalSize::new(aWM, 0, 0), mSize: LogicalSize::new(aWM, NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE), mMax: LogicalSize::new(aWM, NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE) }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp RepeatTrackSizingInput::RepeatTrackSizingInput
    pub fn new(aMin: &LogicalSize, aSize: &LogicalSize, aMax: &LogicalSize) -> RepeatTrackSizingInput {
        RepeatTrackSizingInput { mMin: *aMin, mSize: *aSize, mMax: *aMax }
    }

    // This should be used in intrinsic sizing (i.e. when we can't initialize
    // the sizes directly from ReflowInput values).
    // Gecko: layout/generic/nsGridContainerFrame.cpp RepeatTrackSizingInput::InitFromStyle
    pub fn InitFromStyle(&mut self, aAxis: LogicalAxis, aWM: WritingMode, aFrame: nsIFrame, aStyle: &ServoArc<ComputedStyle>, aAspectRatio: &AspectRatio, aContainingBlockSize: &Option<LogicalSize>) {
        let pos = nsStylePosition::new(aStyle.clone());
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aFrame, std::ptr::null_mut());
        let mut boxSizingAdjustment = BoxSizingAdjustment::new(aWM, aStyle);
        let cbSizeInAxis = match aContainingBlockSize {
            Some(s) => s.Size(aAxis, aWM),
            None => NS_UNCONSTRAINEDSIZE,
        };

        // auto adjustForBoxSizing = [aWM, aAxis, &boxSizingAdjustment](nscoord aSize) {
        //   return std::max(aSize - boxSizingAdjustment.EnsureAndGet().Size(aAxis, aWM), 0);
        // };
        fn adjustForBoxSizing(aWM: WritingMode, aAxis: LogicalAxis, boxSizingAdjustment: &mut BoxSizingAdjustment, aSize: nscoord) -> nscoord {
            std::cmp::max(aSize - boxSizingAdjustment.EnsureAndGet().Size(aAxis, aWM), 0)
        }

        let styleMinSize = pos.MinSize(aAxis, aWM, &anchorResolutionParams);
        let min: nscoord;
        {
            let mut m = self.mMin.Size(aAxis, aWM);
            if styleMinSize.ConvertsToLength() {
                m = adjustForBoxSizing(aWM, aAxis, &mut boxSizingAdjustment, styleMinSize.ToLength());
            } else if styleMinSize.HasPercent() && cbSizeInAxis != NS_UNCONSTRAINEDSIZE {
                m = adjustForBoxSizing(aWM, aAxis, &mut boxSizingAdjustment, styleMinSize.AsLengthPercentage().Resolve(cbSizeInAxis));
            } else if aAspectRatio.operator_bool() && styleMinSize.BehavesLikeInitialValue(aAxis) {
                // Use GetOrthogonalAxis() to get the ratio-determining axis. Same for max
                // and size below in this function.
                let styleRDMinSize = pos.MinSize(GetOrthogonalAxis(aAxis), aWM, &anchorResolutionParams);
                if let Some(resolvedMinSize) = ComputeTransferredSize(&styleRDMinSize, aAxis, aWM, aAspectRatio, &mut boxSizingAdjustment, aContainingBlockSize) {
                    m = resolvedMinSize;
                }
            }
            *self.mMin.Size_mut(aAxis, aWM) = m;
            min = m;
        }

        let styleMaxSize = pos.MaxSize(aAxis, aWM, &anchorResolutionParams);
        let max: nscoord;
        {
            let mut m = self.mMax.Size(aAxis, aWM);
            if styleMaxSize.ConvertsToLength() {
                m = std::cmp::max(min, adjustForBoxSizing(aWM, aAxis, &mut boxSizingAdjustment, styleMaxSize.ToLength()));
            } else if styleMaxSize.HasPercent() && cbSizeInAxis != NS_UNCONSTRAINEDSIZE {
                m = std::cmp::max(min, adjustForBoxSizing(aWM, aAxis, &mut boxSizingAdjustment, styleMaxSize.AsLengthPercentage().Resolve(cbSizeInAxis)));
            } else if aAspectRatio.operator_bool() && styleMaxSize.BehavesLikeInitialValue(aAxis) {
                let styleRDMaxSize = pos.MaxSize(GetOrthogonalAxis(aAxis), aWM, &anchorResolutionParams);
                if let Some(resolvedMaxSize) = ComputeTransferredSize(&styleRDMaxSize, aAxis, aWM, aAspectRatio, &mut boxSizingAdjustment, aContainingBlockSize) {
                    m = std::cmp::max(min, resolvedMaxSize);
                }
            }
            *self.mMax.Size_mut(aAxis, aWM) = m;
            max = m;
        }

        // When computing the intrinsic inline size, disregard the explicit
        // inline-size property as it should not affect the final result.
        let styleSize = if aAxis == LogicalAxis::Inline { AnchorResolvedSizeHelper::Auto() } else { pos.BSize(aWM, &anchorResolutionParams) };
        {
            let mut size = self.mSize.Size(aAxis, aWM);
            if styleSize.ConvertsToLength() {
                size = std::cmp::min(std::cmp::max(adjustForBoxSizing(aWM, aAxis, &mut boxSizingAdjustment, styleSize.ToLength()), min), max);
            } else if styleSize.HasPercent() && cbSizeInAxis != NS_UNCONSTRAINEDSIZE {
                size = std::cmp::min(std::cmp::max(adjustForBoxSizing(aWM, aAxis, &mut boxSizingAdjustment, styleSize.AsLengthPercentage().Resolve(cbSizeInAxis)), min), max);
            } else if aAspectRatio.operator_bool() && styleSize.BehavesLikeInitialValue(aAxis) {
                let styleRDSize = pos.Size(GetOrthogonalAxis(aAxis), aWM, &anchorResolutionParams);
                if let Some(resolvedSize) = ComputeTransferredSize(&styleRDSize, aAxis, aWM, aAspectRatio, &mut boxSizingAdjustment, aContainingBlockSize) {
                    size = std::cmp::min(std::cmp::max(resolvedSize, min), max);
                }
            }
            *self.mSize.Size_mut(aAxis, aWM) = size;
        }
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp GridLineSide
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GridLineSide {
    BeforeGridGap,
    AfterGridGap,
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::TrackSize::StateBits
// (`enum StateBits : uint16_t`; the values after "These are only used in the masonry axis" share bits with the
// *MinSizing ones, as in C++)
MOZ_ENUM_CLASS_BITS! {
    pub struct nsGridContainerFrame_TrackSize_StateBits: u16 {
        eAutoMinSizing = 1 << 0,
        eMinContentMinSizing = 1 << 1,
        eMaxContentMinSizing = 1 << 2,
        eMinOrMaxContentMinSizing = (1 << 1) | (1 << 2),
        eIntrinsicMinSizing = ((1 << 1) | (1 << 2)) | (1 << 0),
        eModified = 1 << 3,
        eAutoMaxSizing = 1 << 4,
        eMinContentMaxSizing = 1 << 5,
        eMaxContentMaxSizing = 1 << 6,
        eAutoOrMaxContentMaxSizing = (1 << 4) | (1 << 6),
        eIntrinsicMaxSizing = ((1 << 4) | (1 << 6)) | (1 << 5),
        eFlexMaxSizing = 1 << 7,
        eFrozen = 1 << 8,
        eSkipGrowUnlimited1 = 1 << 9,
        eSkipGrowUnlimited2 = 1 << 10,
        eSkipGrowUnlimited = (1 << 9) | (1 << 10),
        eBreakBefore = 1 << 11,
        eApplyFitContentClamping = 1 << 12,
        eInfinitelyGrowable = 1 << 13,

        // These are only used in the masonry axis.  They share the same value
        // as *MinSizing above, but that's OK because we don't use those in
        // the masonry axis.
        //
        // This track corresponds to an item margin-box size that is stretching.
        eItemStretchSize = 1 << 0,
        // This bit says that we should clamp that size to mLimit.
        eClampToLimit = 1 << 1,
        // This bit says that the corresponding item has `auto` margin(s).
        eItemHasAutoMargin = 1 << 2,
    }
}
pub type TrackSize_StateBits = nsGridContainerFrame_TrackSize_StateBits;

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::TrackSize
// (static_asserts: trivially copyable and destructible = Clone + Copy here)
#[derive(Clone, Copy, Debug, Default)]
pub struct nsGridContainerFrame_TrackSize {
    // Base size of this track.
    // https://drafts.csswg.org/css-grid-2/#base-size
    pub mBase: nscoord,

    // Growth limit of this track.
    // https://drafts.csswg.org/css-grid-2/#growth-limit
    pub mLimit: nscoord,

    pub mPosition: nscoord, // zero until we apply 'align/justify-content'
    // mBaselineSubtreeSize is the size of a baseline-aligned subtree within
    // this track.  One subtree per baseline-sharing group (per track).
    pub mBaselineSubtreeSize: PerBaseline<nscoord>,
    pub mState: TrackSize_StateBits,
}

impl nsGridContainerFrame_TrackSize {
    pub fn IsFrozen(&self) -> bool {
        (self.mState & TrackSize_StateBits::eFrozen).to_bool()
    }

    pub fn IsDefiniteMaxSizing(aStateBits: TrackSize_StateBits) -> bool {
        (aStateBits & (TrackSize_StateBits::eIntrinsicMaxSizing | TrackSize_StateBits::eFlexMaxSizing)).0 == 0
    }

    // NOT PORTED: DumpStateBits and Dump (#ifdef DEBUG, TrackSize::DumpStateBits and TrackSize::Dump at
    // nsGridContainerFrame.cpp lines 10579-10615 at GECKO_REV).

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::TrackSize::Initialize
    pub fn Initialize(&mut self, aPercentageBasis: nscoord, aSize: &StyleTrackSize) -> TrackSize_StateBits {
        type Tag = StyleTrackBreadth_Tag;

        MOZ_ASSERT!(self.mBase == 0 && self.mLimit == 0 && self.mState.0 == 0, "track size data is expected to be initialized to zero");
        self.mBaselineSubtreeSize[BaselineSharingGroup::First] = 0;
        self.mBaselineSubtreeSize[BaselineSharingGroup::Last] = 0;

        let min = aSize.GetMin();
        let max = aSize.GetMax();

        let mut minSizeTag = min.tag();
        let mut maxSizeTag = max.tag();
        if aSize.IsFitContent() {
            // In layout, fit-content(size) behaves as minmax(auto, max-content), with
            // 'size' as an additional upper-bound.
            if !IsPercentOfIndefiniteSize(aSize.AsFitContent(), aPercentageBasis) {
                self.mState = TrackSize_StateBits::eApplyFitContentClamping;
            }
            minSizeTag = Tag::Auto;
            maxSizeTag = Tag::MaxContent;
        }
        if IsPercentOfIndefiniteSize(min, aPercentageBasis) {
            // https://drafts.csswg.org/css-grid-2/#valdef-grid-template-columns-length-percentage-0
            // "If the inline or block size of the grid container is indefinite,
            //  <percentage> values relative to that size are treated as 'auto'."
            minSizeTag = Tag::Auto;
        }
        if IsPercentOfIndefiniteSize(max, aPercentageBasis) {
            maxSizeTag = Tag::Auto;
        }

        // https://drafts.csswg.org/css-grid-2/#algo-init
        match minSizeTag {
            Tag::Auto => {
                self.mState |= TrackSize_StateBits::eAutoMinSizing;
            }
            Tag::MinContent => {
                self.mState |= TrackSize_StateBits::eMinContentMinSizing;
            }
            Tag::MaxContent => {
                self.mState |= TrackSize_StateBits::eMaxContentMinSizing;
            }
            _ => {
                MOZ_ASSERT!(!min.IsFlex(), "<flex> min-sizing is invalid as a track size");
                self.mBase = ResolveToDefiniteSize(min, aPercentageBasis);
            }
        }
        match maxSizeTag {
            Tag::Auto => {
                self.mState |= TrackSize_StateBits::eAutoMaxSizing;
                self.mLimit = NS_UNCONSTRAINEDSIZE;
            }
            Tag::MinContent | Tag::MaxContent => {
                self.mState |= if maxSizeTag == Tag::MinContent { TrackSize_StateBits::eMinContentMaxSizing } else { TrackSize_StateBits::eMaxContentMaxSizing };
                self.mLimit = NS_UNCONSTRAINEDSIZE;
            }
            Tag::Flex => {
                self.mState |= TrackSize_StateBits::eFlexMaxSizing;
                self.mLimit = NS_UNCONSTRAINEDSIZE;
            }
            _ => {
                self.mLimit = ResolveToDefiniteSize(max, aPercentageBasis);
                if self.mLimit < self.mBase {
                    self.mLimit = self.mBase;
                }
            }
        }
        self.mState
    }
}

// Indicates if we are in intrinsic sizing step 3 (spanning items not
// spanning any flex tracks) or step 4 (spanning items that span one or more
// flex tracks).
// https://drafts.csswg.org/css-grid-2/#algo-content
// Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingStep
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrackSizingStep {
    NotFlex, // https://drafts.csswg.org/css-grid-2/#algo-spanning-items
    Flex,    // https://drafts.csswg.org/css-grid-2/#algo-spanning-flex-items
}

// Sizing phases, used in intrinsic sizing steps 3 and 4.
// https://drafts.csswg.org/css-grid-2/#algo-spanning-items
// Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingPhase
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrackSizingPhase {
    IntrinsicMinimums,
    ContentBasedMinimums,
    MaxContentMinimums,
    IntrinsicMaximums,
    MaxContentMaximums,
}

// Used for grid items intrinsic size types.
// See CachedIntrinsicSizes which uses this for content contributions.
// Gecko: layout/generic/nsGridContainerFrame.cpp GridIntrinsicSizeType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GridIntrinsicSizeType {
    // MinContribution is the "minimum contribution", defined at
    // https://drafts.csswg.org/css-grid-2/#min-size-contribution
    MinContribution,
    MinContentContribution,
    MaxContentContribution,
}

// Gecko: layout/generic/nsGridContainerFrame.cpp kAllGridIntrinsicSizeTypes
pub const kAllGridIntrinsicSizeTypes: [GridIntrinsicSizeType; 3] = [GridIntrinsicSizeType::MinContribution, GridIntrinsicSizeType::MinContentContribution, GridIntrinsicSizeType::MaxContentContribution];

// Glue to make mozilla::EnumeratedArray work with GridIntrinsicSizeType.
// (MaxContiguousEnumValue<GridIntrinsicSizeType> is MaxContentContribution: the array size 3 of
// EnumeratedArray_GridIntrinsicSizeType)

// Convert a track sizing phase into which GridIntrinsicSizeType is applicable.
// Gecko: layout/generic/nsGridContainerFrame.cpp SizeTypeForPhase
pub fn SizeTypeForPhase(aPhase: TrackSizingPhase) -> GridIntrinsicSizeType {
    match aPhase {
        TrackSizingPhase::IntrinsicMinimums => GridIntrinsicSizeType::MinContribution,
        TrackSizingPhase::ContentBasedMinimums | TrackSizingPhase::IntrinsicMaximums => GridIntrinsicSizeType::MinContentContribution,
        TrackSizingPhase::MaxContentMinimums | TrackSizingPhase::MaxContentMaximums => GridIntrinsicSizeType::MaxContentContribution,
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::TrackPlan
// (`CopyableTArray<TrackSize>`: a Vec; Clone is the copy constructor and `Assign`)
#[derive(Clone, Default, Debug)]
pub struct nsGridContainerFrame_TrackPlan {
    mTrackSizes: Vec<TrackSize>,
}
pub type TrackPlan = nsGridContainerFrame_TrackPlan;

impl nsGridContainerFrame_TrackPlan {
    // TrackPlan() = default; is Default::default()

    // explicit TrackPlan(size_t aCapacity) : mTrackSizes(aCapacity) {}
    // (the nsTArray(size_t) constructor makes aCapacity default-constructed elements)
    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackPlan::TrackPlan
    pub fn new__size_t(aCapacity: usize) -> TrackPlan {
        TrackPlan { mTrackSizes: vec![TrackSize::default(); aCapacity] }
    }

    // The copy and move constructors and the assignment operators are Clone and Rust's moves.

    pub fn Length(&self) -> usize {
        self.mTrackSizes.len()
    }

    pub fn SetLength(&mut self, aSize: usize) {
        self.mTrackSizes.resize(aSize, TrackSize::default());
    }

    pub fn IsEmpty(&self) -> bool {
        self.mTrackSizes.is_empty()
    }

    pub fn Assign(&mut self, aRHS: &TrackPlan) {
        self.mTrackSizes.clone_from(&aRHS.mTrackSizes);
    }

    // Returns the index of the new element (C++ returns a pointer to it; use `plan[index]`).
    pub fn AppendElement(&mut self, aElement: TrackSize) -> usize {
        self.mTrackSizes.push(aElement);
        self.mTrackSizes.len() - 1
    }

    pub fn LastElement(&mut self) -> &mut TrackSize {
        self.mTrackSizes.last_mut().unwrap()
    }

    pub fn ClearAndRetainStorage(&mut self) {
        self.mTrackSizes.clear();
    }

    pub fn ZeroInitialize(&mut self) {
        // PodZero(mTrackSizes.Elements(), mTrackSizes.Length());
        for t in self.mTrackSizes.iter_mut() {
            *t = TrackSize::default();
        }
    }

    // using iterator = nsTArray<TrackSize>::iterator; begin() / end()
    pub fn iter(&self) -> std::slice::Iter<'_, TrackSize> {
        self.mTrackSizes.iter()
    }
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, TrackSize> {
        self.mTrackSizes.iter_mut()
    }

    // void Initialize(TrackSizingPhase aPhase, const Tracks& aTracks);
    // nscoord DistributeToFlexTrackSizes(...);
    // (defined in the .cpp, in the last part of this file)
}

impl Index<usize> for nsGridContainerFrame_TrackPlan {
    type Output = TrackSize;
    fn index(&self, aIndex: usize) -> &TrackSize {
        &self.mTrackSizes[aIndex]
    }
}
impl IndexMut<usize> for nsGridContainerFrame_TrackPlan {
    fn index_mut(&mut self, aIndex: usize) -> &mut TrackSize {
        &mut self.mTrackSizes[aIndex]
    }
}
impl<'a> IntoIterator for &'a nsGridContainerFrame_TrackPlan {
    type Item = &'a TrackSize;
    type IntoIter = std::slice::Iter<'a, TrackSize>;
    fn into_iter(self) -> Self::IntoIter {
        self.mTrackSizes.iter()
    }
}
impl<'a> IntoIterator for &'a mut nsGridContainerFrame_TrackPlan {
    type Item = &'a mut TrackSize;
    type IntoIter = std::slice::IterMut<'a, TrackSize>;
    fn into_iter(self) -> Self::IntoIter {
        self.mTrackSizes.iter_mut()
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ItemPlan::FitContentClamper
// (`std::function<bool(uint32_t aTrack, nscoord aMinSize, nscoord* aSize)>`; `const FitContentClamper&` is
// `&ItemPlan_FitContentClamper`, a nullable one is `Option<&ItemPlan_FitContentClamper>`)
pub type ItemPlan_FitContentClamper = dyn Fn(u32, nscoord, &mut nscoord) -> bool;

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ItemPlan
#[derive(Clone, Default, Debug)]
pub struct nsGridContainerFrame_ItemPlan {
    mTrackSizes: Vec<TrackSize>,
}
pub type ItemPlan = nsGridContainerFrame_ItemPlan;

impl nsGridContainerFrame_ItemPlan {
    // ItemPlan() = default; is Default::default()

    // explicit ItemPlan(size_t aCapacity) : mTrackSizes(aCapacity) {}
    // Gecko: layout/generic/nsGridContainerFrame.cpp ItemPlan::ItemPlan
    pub fn new__size_t(aCapacity: usize) -> ItemPlan {
        ItemPlan { mTrackSizes: vec![TrackSize::default(); aCapacity] }
    }

    pub fn SetLength(&mut self, aSize: usize) {
        self.mTrackSizes.resize(aSize, TrackSize::default());
    }

    // void Initialize(TrackSizingPhase aPhase, const nsTArray<uint32_t>& aGrowableTracks, const Tracks& aTracks);
    // nscoord GrowTracksToLimit(nscoord aAvailableSpace, const nsTArray<uint32_t>& aGrowableTracks,
    //                           const FitContentClamper& aFitContentClamper);
    // uint32_t MarkExcludedTracks(uint32_t aNumGrowable, const nsTArray<uint32_t>& aGrowableTracks,
    //     TrackSize::StateBits aMinSizingSelector, TrackSize::StateBits aMaxSizingSelector,
    //     TrackSize::StateBits aSkipFlag);   (MarkExcludedTracks__uint32_t_nsTArray_StateBits_StateBits_StateBits)
    // uint32_t MarkExcludedTracks(TrackSizingPhase aPhase, const nsTArray<uint32_t>& aGrowableTracks,
    //     SizingConstraint aConstraint);   (MarkExcludedTracks__TrackSizingPhase_nsTArray_SizingConstraint)
    // void GrowSelectedTracksUnlimited(nscoord aAvailableSpace, const nsTArray<uint32_t>& aGrowableTracks,
    //     uint32_t aNumGrowable, const FitContentClamper& aFitContentClamper);
    // (defined in the .cpp, in the last part of this file; the doc comments are copied there)
}

impl Index<usize> for nsGridContainerFrame_ItemPlan {
    type Output = TrackSize;
    fn index(&self, aIndex: usize) -> &TrackSize {
        &self.mTrackSizes[aIndex]
    }
}
impl IndexMut<usize> for nsGridContainerFrame_ItemPlan {
    fn index_mut(&mut self, aIndex: usize) -> &mut TrackSize {
        &mut self.mTrackSizes[aIndex]
    }
}

/**
 * A LineRange can be definite or auto - when it's definite it represents
 * a consecutive set of tracks between a starting line and an ending line.
 * Before it's definite it can also represent an auto position with a span,
 * where mStart == kAutoLine and mEnd is the (non-zero positive) span.
 * For normal-flow items, the invariant mStart < mEnd holds when both
 * lines are definite.
 *
 * For abs.pos. grid items, mStart and mEnd may both be kAutoLine, meaning
 * "attach this side to the grid container containing block edge".
 * Additionally, mStart <= mEnd holds when both are definite (non-kAutoLine),
 * i.e. the invariant is slightly relaxed compared to normal flow items.
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineRange
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsGridContainerFrame_LineRange {
    /**
     * @note We'll use the signed member while resolving definite positions
     * to line numbers (1-based), which may become negative for implicit lines
     * to the top/left of the explicit grid.  PlaceGridItems() then translates
     * the whole grid to a 0,0 origin and we'll use the unsigned member from
     * there on.
     * (the C++ union of `mStart` and `mUntranslatedStart`: the field is `mStart`, see `mUntranslatedStart()`)
     */
    pub mStart: u32,
    pub mEnd: u32,
}
pub type LineRange = nsGridContainerFrame_LineRange;

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineRange::LineRange()  (protected)
impl Default for nsGridContainerFrame_LineRange {
    fn default() -> LineRange {
        LineRange { mStart: 0, mEnd: 0 }
    }
}

impl nsGridContainerFrame_LineRange {
    // Gecko: layout/generic/nsGridContainerFrame.cpp LineRange::LineRange(int32_t aStart, int32_t aEnd)
    pub fn new(aStart: i32, aEnd: i32) -> LineRange {
        let r = LineRange { mStart: aStart as u32, mEnd: aEnd as u32 };
        #[cfg(debug_assertions)]
        {
            if !r.IsAutoAuto() {
                if r.IsAuto() {
                    MOZ_ASSERT!(aEnd >= kMinLine && aEnd <= kMaxLine, "invalid span");
                } else {
                    MOZ_ASSERT!(aStart >= kMinLine && aStart <= kMaxLine, "invalid start line");
                    MOZ_ASSERT!(aEnd == kAutoLine as i32 || (aEnd >= kMinLine && aEnd <= kMaxLine), "invalid end line");
                }
            }
        }
        r
    }

    // The union member `int32_t mUntranslatedStart` / `mUntranslatedEnd`: the same bits as mStart / mEnd.
    pub fn mUntranslatedStart(&self) -> i32 {
        self.mStart as i32
    }
    pub fn mUntranslatedEnd(&self) -> i32 {
        self.mEnd as i32
    }
    pub fn mUntranslatedStart_mut(&mut self) -> &mut i32 {
        // SAFETY: u32 and i32 have the same size and alignment, every bit pattern is valid for both.
        unsafe { &mut *(&mut self.mStart as *mut u32 as *mut i32) }
    }
    pub fn mUntranslatedEnd_mut(&mut self) -> &mut i32 {
        unsafe { &mut *(&mut self.mEnd as *mut u32 as *mut i32) }
    }

    pub fn IsAutoAuto(&self) -> bool {
        self.mStart == kAutoLine && self.mEnd == kAutoLine
    }
    pub fn IsAuto(&self) -> bool {
        self.mStart == kAutoLine
    }
    pub fn IsDefinite(&self) -> bool {
        self.mStart != kAutoLine
    }
    pub fn Extent(&self) -> u32 {
        MOZ_ASSERT!(self.mEnd != kAutoLine, "Extent is undefined for abs.pos. 'auto'");
        if self.IsAuto() {
            MOZ_ASSERT!(self.mEnd >= 1 && self.mEnd < kMaxLine as u32, "invalid span");
            return self.mEnd;
        }
        self.mEnd - self.mStart
    }

    /**
     * Return an object suitable for iterating this range.
     */
    pub fn Range(&self) -> std::ops::Range<u32> {
        // IntegerRange<uint32_t>(mStart, mEnd)
        self.mStart..self.mEnd
    }

    /**
     * Resolve this auto range to start at aStart, making it definite.
     * @param aClampMaxLine the maximum allowed line number (zero-based)
     * Precondition: this range IsAuto()
     */
    pub fn ResolveAutoPosition(&mut self, aStart: u32, aClampMaxLine: u32) {
        MOZ_ASSERT!(self.IsAuto(), "Why call me?");
        self.mStart = aStart;
        self.mEnd += aStart;
        // Clamp to aClampMaxLine, which is where kMaxLine is in the explicit
        // grid in a non-subgrid axis; this implements clamping per
        // https://drafts.csswg.org/css-grid-2/#overlarge-grids
        // In a subgrid axis it's the end of the grid in that axis.
        if self.mStart >= aClampMaxLine {
            self.mEnd = aClampMaxLine;
            self.mStart = self.mEnd - 1;
        } else if self.mEnd > aClampMaxLine {
            self.mEnd = aClampMaxLine;
        }
    }
    /**
     * Translate the lines to account for (empty) removed tracks.  This method
     * is only for grid items and should only be called after placement.
     * aNumRemovedTracks contains a count for each line in the grid how many
     * tracks were removed between the start of the grid and that line.
     */
    pub fn AdjustForRemovedTracks(&mut self, aNumRemovedTracks: &Vec<u32>) {
        MOZ_ASSERT!(self.mStart != kAutoLine, "invalid resolved line for a grid item");
        MOZ_ASSERT!(self.mEnd != kAutoLine, "invalid resolved line for a grid item");
        let numRemovedTracks = aNumRemovedTracks[self.mStart as usize];
        MOZ_ASSERT!(numRemovedTracks == aNumRemovedTracks[self.mEnd as usize], "tracks that a grid item spans can't be removed");
        self.mStart -= numRemovedTracks;
        self.mEnd -= numRemovedTracks;
    }
    /**
     * Translate the lines to account for (empty) removed tracks.  This method
     * is only for abs.pos. children and should only be called after placement.
     * Same as for in-flow items, but we don't touch 'auto' lines here and we
     * also need to adjust areas that span into the removed tracks.
     */
    pub fn AdjustAbsPosForRemovedTracks(&mut self, aNumRemovedTracks: &Vec<u32>) {
        if self.mStart != kAutoLine {
            self.mStart -= aNumRemovedTracks[self.mStart as usize];
        }
        if self.mEnd != kAutoLine {
            MOZ_ASSERT!(self.mStart == kAutoLine || self.mEnd > self.mStart, "invalid line range");
            self.mEnd -= aNumRemovedTracks[self.mEnd as usize];
        }
    }

    /**
     * Return the contribution of this line range for step 2 in
     * https://drafts.csswg.org/css-grid-2/#auto-placement-algo
     */
    pub fn HypotheticalEnd(&self) -> u32 {
        self.mEnd
    }

    // void ToPositionAndLength(const TrackPlan& aTrackPlan, nscoord* aPos, nscoord* aLength) const;
    // nscoord ToLength(const TrackPlan& aTrackPlan) const;
    // void ToPositionAndLengthForAbsPos(const Tracks& aTracks, nscoord aGridOrigin, nscoord* aPos,
    //                                   nscoord* aLength) const;
    // (defined in the .cpp, in the fifth part of this file)

    pub fn Translate(&mut self, aOffset: i32) {
        MOZ_ASSERT!(self.IsDefinite());
        self.mStart = self.mStart.wrapping_add(aOffset as u32);
        self.mEnd = self.mEnd.wrapping_add(aOffset as u32);
    }

    /** Swap the start/end sides of this range. */
    pub fn ReverseDirection(&mut self, aGridEnd: u32) {
        MOZ_ASSERT!(self.IsDefinite());
        MOZ_ASSERT!(aGridEnd >= self.mEnd);
        let newStart = aGridEnd - self.mEnd;
        self.mEnd = aGridEnd - self.mStart;
        self.mStart = newStart;
    }
}

/**
 * Helper class to construct a LineRange from translated lines.
 * The ctor only accepts translated definite line numbers.
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::TranslatedLineRange
// (`: public LineRange`: Deref to the LineRange, `.into()` / `From` gives the LineRange value)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsGridContainerFrame_TranslatedLineRange(pub LineRange);
pub type TranslatedLineRange = nsGridContainerFrame_TranslatedLineRange;

impl Deref for nsGridContainerFrame_TranslatedLineRange {
    type Target = LineRange;
    fn deref(&self) -> &LineRange {
        &self.0
    }
}
impl From<nsGridContainerFrame_TranslatedLineRange> for LineRange {
    fn from(aRange: TranslatedLineRange) -> LineRange {
        aRange.0
    }
}

impl nsGridContainerFrame_TranslatedLineRange {
    // Gecko: layout/generic/nsGridContainerFrame.cpp TranslatedLineRange::TranslatedLineRange
    pub fn new(aStart: u32, aEnd: u32) -> TranslatedLineRange {
        MOZ_ASSERT!(aStart < aEnd && aEnd <= kTranslatedMaxLine);
        let mut r = LineRange::default();
        r.mStart = aStart;
        r.mEnd = aEnd;
        nsGridContainerFrame_TranslatedLineRange(r)
    }
}

/**
 * A GridArea is the area in the grid for a grid item.
 * The area is represented by two LineRanges, both of which can be auto
 * (@see LineRange) in intermediate steps while the item is being placed.
 * @see PlaceGridItems
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridArea
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsGridContainerFrame_GridArea {
    pub mCols: LineRange,
    pub mRows: LineRange,
}
pub type GridArea = nsGridContainerFrame_GridArea;

impl nsGridContainerFrame_GridArea {
    pub fn new(aCols: &LineRange, aRows: &LineRange) -> GridArea {
        GridArea { mCols: *aCols, mRows: *aRows }
    }
    pub fn IsDefinite(&self) -> bool {
        self.mCols.IsDefinite() && self.mRows.IsDefinite()
    }
    // LineRange& LineRangeForAxis(LogicalAxis aAxis)
    pub fn LineRangeForAxis_mut(&mut self, aAxis: LogicalAxis) -> &mut LineRange {
        if aAxis == LogicalAxis::Inline { &mut self.mCols } else { &mut self.mRows }
    }
    // const LineRange& LineRangeForAxis(LogicalAxis aAxis) const
    pub fn LineRangeForAxis(&self, aAxis: LogicalAxis) -> &LineRange {
        if aAxis == LogicalAxis::Inline { &self.mCols } else { &self.mRows }
    }
}

// ADAPTER: the C++ member pointers `LineRange GridArea::*` (aMajor / aMinor of FindFirstItemInGridOrder and
// friends) are functions that select the member.
pub type LineRangeSelector = fn(&GridArea) -> &LineRange;
pub type LineRangeSelectorMut = fn(&mut GridArea) -> &mut LineRange;
// `&GridArea::mCols`
pub fn GridArea_mCols(aArea: &GridArea) -> &LineRange {
    &aArea.mCols
}
// `&GridArea::mRows`
pub fn GridArea_mRows(aArea: &GridArea) -> &LineRange {
    &aArea.mRows
}
pub fn GridArea_mCols_mut(aArea: &mut GridArea) -> &mut LineRange {
    &mut aArea.mCols
}
pub fn GridArea_mRows_mut(aArea: &mut GridArea) -> &mut LineRange {
    &mut aArea.mRows
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridItemInfo::StateBits
// (item state per axis, `enum StateBits : uint16_t`; `using ItemState = GridItemInfo::StateBits;`)
MOZ_ENUM_CLASS_BITS! {
    pub struct nsGridContainerFrame_GridItemInfo_StateBits: u16 {
        // Does the item span a flex track?
        eIsFlexing = 0x1,

        // First or last baseline alignment preference. They are mutually exclusive.
        // This does *NOT* represent the baseline alignment group. See the member
        // variable for that.
        // <https://drafts.csswg.org/css-align-3/#baseline-alignment-preference>
        eFirstBaseline = 0x2,
        eLastBaseline = 0x4,
        eIsBaselineAligned = 0x2 | 0x4,

        // One of e[Self|Content]Baseline is set when eIsBaselineAligned is true
        eSelfBaseline = 0x8, // is it *-self:[last ]baseline alignment?
        // Ditto *-content:[last ]baseline. Mutually exclusive w. eSelfBaseline.
        eContentBaseline = 0x10,

        // The baseline affects the margin or padding on the item's end side when
        // this bit is set.  In a grid-axis it's always set for eLastBaseline and
        // always unset for eFirstBaseline.  In a masonry-axis, it's set for
        // baseline groups in the EndStretch set and unset for the StartStretch set.
        eEndSideBaseline = 0x20,

        // Set when the grid item is in the last baseline sharing group, otherwise
        // assume the first baseline sharing group. The baseline sharing group might
        // differ from the specified baseline alignment due to baseline alignment
        // rules.
        eLastBaselineSharingGroup = 0x40,

        eAllBaselineBits = (0x2 | 0x4) | 0x8 | 0x10 | 0x20 | 0x40,

        // Automatic Minimum Size is content based. If not set, automatic minimum
        // size is zero.
        // https://drafts.csswg.org/css-grid-2/#min-size-auto
        // https://drafts.csswg.org/css-grid-2/#content-based-minimum-size
        eContentBasedAutoMinSize = 0x80,
        // Clamp per https://drafts.csswg.org/css-grid-2/#min-size-auto
        eClampMarginBoxMinSize = 0x100,
        eIsSubgrid = 0x200,
        // set on subgrids and items in subgrids if they are adjacent to the grid
        // start/end edge (excluding grid-aligned abs.pos. frames)
        eStartEdge = 0x400,
        eEndEdge = 0x800,
        eEdgeBits = 0x400 | 0x800,
        // Set if this item was auto-placed in this axis.
        eAutoPlacement = 0x1000,
        // Set if this item is the last item in its track (masonry layout only)
        eIsLastItemInMasonryTrack = 0x2000,

        // Bits set during the track sizing step.
        eTrackSizingBits = 0x1 | 0x80 | 0x100,
    }
}
pub type GridItemInfo_StateBits = nsGridContainerFrame_GridItemInfo_StateBits;
pub type ItemState = nsGridContainerFrame_GridItemInfo_StateBits;

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridItemInfo
// (the copy constructor is Clone; the copy assignment is deleted: no Copy)
#[derive(Clone, Debug)]
pub struct nsGridContainerFrame_GridItemInfo {
    pub mFrame: nsIFrame,
    pub mArea: GridArea,

    // Offset from the margin edge to the baseline (LogicalAxis index).  It's from
    // the start edge for first baseline sharing group, otherwise from the end
    // edge.
    // It's mutable since we update the value fairly late (just before reflowing
    // the item).
    pub mBaselineOffset: PerLogicalAxis<Cell<nscoord>>,

    // State bits per axis.
    pub mState: PerLogicalAxis<Cell<ItemState>>,
}
pub type GridItemInfo = nsGridContainerFrame_GridItemInfo;

impl nsGridContainerFrame_GridItemInfo {
    // Gecko: layout/generic/nsGridContainerFrame.cpp GridItemInfo::GridItemInfo
    pub fn new(aFrame: nsIFrame, aArea: &GridArea) -> GridItemInfo {
        let item = GridItemInfo { mFrame: aFrame, mArea: *aArea, mBaselineOffset: PerLogicalAxis([Cell::new(0), Cell::new(0)]), mState: PerLogicalAxis([Cell::new(ItemState::default()), Cell::new(ItemState::default())]) };
        item.mState[LogicalAxis::Block].set(if item.mArea.mRows.mStart == kAutoLine { ItemState::eAutoPlacement } else { ItemState::default() });
        item.mState[LogicalAxis::Inline].set(if item.mArea.mCols.mStart == kAutoLine { ItemState::eAutoPlacement } else { ItemState::default() });

        if let Some(gridFrame) = nsGridContainerFrame::GetGridContainerFrame(item.mFrame) {
            let parentWM = aFrame.GetParent().unwrap().GetWritingMode();
            let isOrthogonal = parentWM.IsOrthogonalTo(&gridFrame.GetWritingMode());
            if gridFrame.IsColSubgrid() {
                let axis = if isOrthogonal { LogicalAxis::Block } else { LogicalAxis::Inline };
                item.mState[axis].set(item.mState[axis].get() | ItemState::eIsSubgrid);
            }
            if gridFrame.IsRowSubgrid() {
                let axis = if isOrthogonal { LogicalAxis::Inline } else { LogicalAxis::Block };
                item.mState[axis].set(item.mState[axis].get() | ItemState::eIsSubgrid);
            }
        }
        item
    }

    // GridItemInfo& operator=(const GridItemInfo&) = delete;

    pub fn BaselineAlignmentAffectsEndSide(state: ItemState) -> bool {
        (state & ItemState::eEndSideBaseline).to_bool()
    }

    /**
     * Inhibit subgrid layout unless the item is placed in the first "track" in
     * a parent masonry-axis, or has definite placement or spans all tracks in
     * the parent grid-axis.
     * TODO: this is stricter than what the Masonry proposal currently states
     *       (bug 1627581)
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp GridItemInfo::MaybeInhibitSubgridInMasonry
    // NOT PORTED: masonry (aParent.IsRowMasonry() / IsColMasonry() are always false, so nothing changes);
    // the body is translated.
    pub fn MaybeInhibitSubgridInMasonry(&mut self, aParent: nsGridContainerFrame, aGridAxisTrackCount: u32) {
        if self.IsSubgrid__LogicalAxis(LogicalAxis::Inline) && aParent.IsRowMasonry() && self.mArea.mRows.mStart != 0 && self.mArea.mCols.Extent() != aGridAxisTrackCount && (self.mState[LogicalAxis::Inline].get() & ItemState::eAutoPlacement).to_bool() {
            self.InhibitSubgrid(aParent, LogicalAxis::Inline);
            return;
        }
        if self.IsSubgrid__LogicalAxis(LogicalAxis::Block) && aParent.IsColMasonry() && self.mArea.mCols.mStart != 0 && self.mArea.mRows.Extent() != aGridAxisTrackCount && (self.mState[LogicalAxis::Block].get() & ItemState::eAutoPlacement).to_bool() {
            self.InhibitSubgrid(aParent, LogicalAxis::Block);
        }
    }

    /**
     * Inhibit subgridding in aAxis for this item.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp GridItemInfo::InhibitSubgrid
    pub fn InhibitSubgrid(&mut self, aParent: nsGridContainerFrame, aAxis: LogicalAxis) {
        MOZ_ASSERT!(self.IsSubgrid__LogicalAxis(aAxis));
        let mut bit = NS_STATE_GRID_IS_COL_SUBGRID;
        if aParent.GetWritingMode().IsOrthogonalTo(&self.mFrame.GetWritingMode()) != (aAxis == LogicalAxis::Block) {
            bit = NS_STATE_GRID_IS_ROW_SUBGRID;
        }
        MOZ_ASSERT!(self.SubgridFrame().HasAnyStateBits(bit));
        self.SubgridFrame().RemoveStateBits(bit);
        self.mState[aAxis].set(self.mState[aAxis].get() & !ItemState::eIsSubgrid);
    }

    /**
     * Return a copy of this item with its row/column data swapped.
     */
    pub fn Transpose(&self) -> GridItemInfo {
        let info = GridItemInfo::new(self.mFrame, &GridArea::new(&self.mArea.mRows, &self.mArea.mCols));
        info.mState[LogicalAxis::Block].set(self.mState[LogicalAxis::Inline].get());
        info.mState[LogicalAxis::Inline].set(self.mState[LogicalAxis::Block].get());
        info.mBaselineOffset[LogicalAxis::Block].set(self.mBaselineOffset[LogicalAxis::Inline].get());
        info.mBaselineOffset[LogicalAxis::Inline].set(self.mBaselineOffset[LogicalAxis::Block].get());
        info
    }

    // Reset bits in mState in aAxis that were set during the track sizing step.
    // Gecko: layout/generic/nsGridContainerFrame.cpp GridItemInfo::ResetTrackSizingBits
    pub fn ResetTrackSizingBits(&self, aAxis: LogicalAxis) {
        self.mState[aAxis].set(self.mState[aAxis].get() & !ItemState::eTrackSizingBits);
    }

    /** Swap the start/end sides in aAxis. */
    // Gecko: layout/generic/nsGridContainerFrame.cpp GridItemInfo::ReverseDirection
    pub fn ReverseDirection(&mut self, aAxis: LogicalAxis, aGridEnd: u32) {
        self.mArea.LineRangeForAxis_mut(aAxis).ReverseDirection(aGridEnd);
        let state = self.mState[aAxis].get();
        let mut newState = state & !ItemState::eEdgeBits;
        if (state & ItemState::eStartEdge).to_bool() {
            newState |= ItemState::eEndEdge;
        }
        if (state & ItemState::eEndEdge).to_bool() {
            newState |= ItemState::eStartEdge;
        }
        self.mState[aAxis].set(newState);
    }

    // Is this item a subgrid in the given container axis?
    pub fn IsSubgrid__LogicalAxis(&self, aAxis: LogicalAxis) -> bool {
        (self.mState[aAxis].get() & ItemState::eIsSubgrid).to_bool()
    }

    // Is this item a subgrid in either axis?
    pub fn IsSubgrid__(&self) -> bool {
        self.IsSubgrid__LogicalAxis(LogicalAxis::Inline) || self.IsSubgrid__LogicalAxis(LogicalAxis::Block)
    }

    // Return the (inner) grid container frame associated with this subgrid item.
    pub fn SubgridFrame(&self) -> nsGridContainerFrame {
        MOZ_ASSERT!(self.IsSubgrid__());
        let gridFrame = nsGridContainerFrame::GetGridContainerFrame(self.mFrame);
        MOZ_ASSERT!(gridFrame.is_some() && gridFrame.unwrap().IsSubgrid__());
        gridFrame.unwrap()
    }

    /**
     * Adjust our grid areas to account for removed auto-fit tracks in aAxis.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp GridItemInfo::AdjustForRemovedTracks
    pub fn AdjustForRemovedTracks(&mut self, aAxis: LogicalAxis, aNumRemovedTracks: &Vec<u32>) {
        let abspos = self.mFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW);
        {
            let lines = self.mArea.LineRangeForAxis_mut(aAxis);
            if abspos {
                lines.AdjustAbsPosForRemovedTracks(aNumRemovedTracks);
            } else {
                lines.AdjustForRemovedTracks(aNumRemovedTracks);
            }
        }
        if self.IsSubgrid__() {
            if let Some(subgrid) = nsGridContainerFrame_Subgrid::GetProp(self.SubgridFrame().0) {
                let lines = subgrid.mArea.LineRangeForAxis_mut(aAxis);
                if abspos {
                    lines.AdjustAbsPosForRemovedTracks(aNumRemovedTracks);
                } else {
                    lines.AdjustForRemovedTracks(aNumRemovedTracks);
                }
            }
        }
    }

    /**
     * If the item is [align|justify]-self:[last ]baseline aligned in the given
     * axis then set aBaselineOffset to the baseline offset and return aAlign.
     * Otherwise, return a fallback alignment.
     */
    pub fn GetSelfBaseline(&self, aAlign: StyleAlignFlags, aAxis: LogicalAxis, aBaselineOffset: &mut nscoord) -> StyleAlignFlags {
        MOZ_ASSERT!(aAlign == StyleAlignFlags::BASELINE || aAlign == StyleAlignFlags::LAST_BASELINE);
        if !(self.mState[aAxis].get() & ItemState::eSelfBaseline).to_bool() {
            return if aAlign == StyleAlignFlags::BASELINE { StyleAlignFlags::SELF_START } else { StyleAlignFlags::SELF_END };
        }
        *aBaselineOffset = self.mBaselineOffset[aAxis].get();
        aAlign
    }

    // Return true if we should use MinContribution on items that do not span
    // any flex tracks to determine the minimum contribution, and if we should
    // set the eContentBasedAutoMinSize flag on grid items.
    //
    // In part this is determined by whether or not the minimum contribution
    // of the item is content-based.
    // https://drafts.csswg.org/css-grid-2/#min-size-auto
    //
    // @note the caller should also check that the item has a span length of 1,
    // and that the item's track has a min track sizing function that is 'auto'.
    // Gecko: layout/generic/nsGridContainerFrame.cpp GridItemInfo::MinContributionDependsOnAutoMinSize
    pub fn MinContributionDependsOnAutoMinSize(&self, aContainerWM: WritingMode, aContainerAxis: LogicalAxis) -> bool {
        MOZ_ASSERT!(self.mArea.LineRangeForAxis(aContainerAxis).Extent() == 1, "Should not be called with grid items that span multiple tracks.");
        let itemAxis = aContainerWM.ConvertAxisTo(aContainerAxis, self.mFrame.GetWritingMode());
        let styleFrame = if self.mFrame.IsTableWrapperFrame() { self.mFrame.PrincipalChildList_FirstChild().unwrap() } else { self.mFrame };
        let pos = styleFrame.StylePosition();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(styleFrame, std::ptr::null_mut());
        let size = pos.Size(aContainerAxis, aContainerWM, &anchorResolutionParams);
        // max-content and min-content should behave as initial value in block axis.
        // FIXME: Bug 567039: moz-fit-content and -moz-available are not supported
        // for block size dimension on sizing properties (e.g. height), so we
        // treat it as `auto`.
        let mut isAuto = size.BehavesLikeInitialValue(itemAxis);
        // This check for HasPercent is intended to correspond to whether or not
        // the item's preferred size depends on the size of its containing block.
        //
        // TODO alaskanemily: This probably shouldn't be a special case here.
        // This is part of how EnsureContributions with the MinSize flag is
        // implemented, where this forces ResolveIntrinsicSizeForNonSpanningItems
        // to use MinSize instead of Min/MaxContentContribution, which
        // EnsureContributions will then translate to/from MinContentContribution
        //
        // https://drafts.csswg.org/css-grid-2/#algo-single-span-items
        // Section "For auto minimums"
        if !isAuto && !size.HasPercent() {
            return false;
        }
        let minSize = pos.MinSize(aContainerAxis, aContainerWM, &anchorResolutionParams);
        // max-content and min-content should behave as initial value in block axis.
        // FIXME: Bug 567039: moz-fit-content and -moz-available are not supported
        // for block size dimension on sizing properties (e.g. height), so we
        // treat it as `auto`.
        isAuto = minSize.BehavesLikeInitialValue(itemAxis);
        isAuto && !self.mFrame.StyleDisplay().IsScrollableOverflow()
    }

    // NOT PORTED: Dump (#ifdef DEBUG, nsGridContainerFrame.cpp lines 1436-1488 at GECKO_REV).

    // (the C++ takes `const GridItemInfo*`; here the references)
    pub fn IsStartRowLessThan(a: &GridItemInfo, b: &GridItemInfo) -> bool {
        a.mArea.mRows.mStart < b.mArea.mRows.mStart
    }

    // Sorting functions for 'masonry-auto-flow:next'.  We sort the items that
    // were placed into the first track by the Grid placement algorithm first
    // (to honor that placement).  All other items will be placed by the Masonry
    // layout algorithm (their Grid placement in the masonry axis is irrelevant).
    pub fn RowMasonryOrdered(a: &GridItemInfo, b: &GridItemInfo) -> bool {
        a.mArea.mRows.mStart == 0 && b.mArea.mRows.mStart != 0 && !a.mFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW)
    }
    pub fn ColMasonryOrdered(a: &GridItemInfo, b: &GridItemInfo) -> bool {
        a.mArea.mCols.mStart == 0 && b.mArea.mCols.mStart != 0 && !a.mFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW)
    }

    // Sorting functions for 'masonry-auto-flow:definite-first'.  Similar to
    // the above, but here we also sort items with a definite item placement in
    // the grid axis in track order before 'auto'-placed items. We also sort all
    // continuations first since they use the same placement as their
    // first-in-flow (we treat them as "definite" regardless of eAutoPlacement).
    pub fn RowMasonryDefiniteFirst(a: &GridItemInfo, b: &GridItemInfo) -> bool {
        let isContinuationA = a.mFrame.GetPrevInFlow().is_some();
        let isContinuationB = b.mFrame.GetPrevInFlow().is_some();
        if isContinuationA != isContinuationB {
            return isContinuationA;
        }
        let masonryA = a.mArea.mRows.mStart;
        let gridA = a.mState[LogicalAxis::Inline].get() & ItemState::eAutoPlacement;
        let masonryB = b.mArea.mRows.mStart;
        let gridB = b.mState[LogicalAxis::Inline].get() & ItemState::eAutoPlacement;
        (if masonryA == 0 { masonryB != 0 } else { masonryB != 0 && gridA.0 < gridB.0 }) && !a.mFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW)
    }
    pub fn ColMasonryDefiniteFirst(a: &GridItemInfo, b: &GridItemInfo) -> bool {
        MOZ_ASSERT!(a.mFrame.GetPrevInFlow().is_none() && b.mFrame.GetPrevInFlow().is_none(), "fragmentation not supported in inline axis");
        let masonryA = a.mArea.mCols.mStart;
        let gridA = a.mState[LogicalAxis::Block].get() & ItemState::eAutoPlacement;
        let masonryB = b.mArea.mCols.mStart;
        let gridB = b.mState[LogicalAxis::Block].get() & ItemState::eAutoPlacement;
        (if masonryA == 0 { masonryB != 0 } else { masonryB != 0 && gridA.0 < gridB.0 }) && !a.mFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW)
    }

    // Return true if this items block size is dependent on the size of the
    // container it is in.
    // Gecko: layout/generic/nsGridContainerFrame.cpp GridItemInfo::IsBSizeDependentOnContainerSize
    pub fn IsBSizeDependentOnContainerSize(&self, aContainerWM: WritingMode) -> bool {
        // const auto IsDependentOnContainerSize = [](const auto& size) -> bool {
        //   // XXXdholbert The BehavesLikeStretchOnInlineAxis usage seems like
        //   // maybe it should be considering block-axis instead?
        //   return size.HasPercent() || size.BehavesLikeStretchOnInlineAxis();
        // };
        fn IsDependentOnContainerSize<S: StyleSizeLike>(size: &S) -> bool {
            size.HasPercent() || size.BehavesLikeStretchOnInlineAxis()
        }

        let stylePos = self.mFrame.StylePosition();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.mFrame, std::ptr::null_mut());
        let isItemAutoSize = IsDependentOnContainerSize(&stylePos.BSize(aContainerWM, &anchorResolutionParams)) || IsDependentOnContainerSize(&stylePos.MinBSize(aContainerWM, &anchorResolutionParams)) || IsDependentOnContainerSize(&stylePos.MaxBSize(aContainerWM, &anchorResolutionParams));

        isItemAutoSize
    }
}

// MOZ_MAKE_ENUM_CLASS_BITWISE_OPERATORS(ItemState): in MOZ_ENUM_CLASS_BITS.

// Each subgrid stores this data about its items etc on a frame property.
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Subgrid
// NOT PORTED: subgrid (the data stays so that the code which reads it translates; nothing creates one).
pub struct nsGridContainerFrame_Subgrid {
    // The subgrid's items.
    pub mGridItems: Vec<GridItemInfo>,
    // The subgrid's abs.pos. items.
    pub mAbsPosItems: Vec<GridItemInfo>,
    // The subgrid's area as a grid item, i.e. in its parent's grid space.
    pub mArea: GridArea,
    // The (inner) grid size for the subgrid, zero-based.
    pub mGridColEnd: u32,
    pub mGridRowEnd: u32,
    // The margin+border+padding for the subgrid box in its parent grid's WM.
    // (This also includes the size of any scrollbars.)
    pub mMarginBorderPadding: LogicalMargin,
    // Does the subgrid frame have orthogonal writing-mode to its parent grid
    // container?
    pub mIsOrthogonal: bool,
}
pub type Subgrid = nsGridContainerFrame_Subgrid;

impl nsGridContainerFrame_Subgrid {
    // Gecko: layout/generic/nsGridContainerFrame.cpp Subgrid::Subgrid
    pub fn new(aArea: &GridArea, aIsOrthogonal: bool, aCBWM: WritingMode) -> Subgrid {
        Subgrid { mGridItems: Vec::new(), mAbsPosItems: Vec::new(), mArea: *aArea, mGridColEnd: 0, mGridRowEnd: 0, mMarginBorderPadding: LogicalMargin::new__WritingMode(aCBWM), mIsOrthogonal: aIsOrthogonal }
    }

    // Return the relevant line range for the subgrid column axis.
    pub fn SubgridCols(&self) -> &LineRange {
        if self.mIsOrthogonal { &self.mArea.mRows } else { &self.mArea.mCols }
    }
    // Return the relevant line range for the subgrid row axis.
    pub fn SubgridRows(&self) -> &LineRange {
        if self.mIsOrthogonal { &self.mArea.mCols } else { &self.mArea.mRows }
    }

    // NS_DECLARE_FRAME_PROPERTY_DELETABLE(Prop, Subgrid)
    pub fn Prop() -> FramePropertyDescriptor<usize> {
        SubgridProperty()
    }
    pub fn GetProp(aFrame: nsIFrame) -> Option<&'static mut Subgrid> {
        GridPropGet::<Subgrid>(aFrame, Self::Prop())
    }
    pub fn SetProp(aFrame: nsIFrame, aValue: Box<Subgrid>) {
        GridPropSet(aFrame, Self::Prop(), aValue)
    }
    pub fn RemoveProp(aFrame: nsIFrame) {
        GridPropRemove::<Subgrid>(aFrame, Self::Prop())
    }
}

/**
 * Track size data for use by subgrids (which don't do sizing of their own
 * in a subgridded axis).  A non-subgrid container stores its resolved sizes,
 * but only if it has any subgrid children.  A subgrid always stores one.
 * In a subgridded axis, we copy the parent's sizes (see CopyUsedTrackSizes).
 *
 * This struct us stored on a frame property, which may be null before the track
 * sizing step for the given container.  A null property is semantically
 * equivalent to mCanResolveLineRangeSize being false in both axes.
 * @note the axis used to access this data is in the grid container's own
 * writing-mode, same as in other track-sizing functions.
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::UsedTrackSizes
pub struct nsGridContainerFrame_UsedTrackSizes {
    // This only has valid sizes when mCanResolveLineRangeSize is true in
    // the same axis.  It may have zero tracks (a grid with only abs.pos.
    // subgrids/items may have zero tracks).
    pub mTrackPlans: PerLogicalAxis<TrackPlan>,
    // True if mTrackPlans can be used to resolve line range sizes in an axis.
    pub mCanResolveLineRangeSize: PerLogicalAxis<bool>,
}
pub type UsedTrackSizes = nsGridContainerFrame_UsedTrackSizes;

impl Default for nsGridContainerFrame_UsedTrackSizes {
    // UsedTrackSizes() : mCanResolveLineRangeSize{false, false} {}
    fn default() -> UsedTrackSizes {
        UsedTrackSizes { mTrackPlans: PerLogicalAxis::default(), mCanResolveLineRangeSize: PerLogicalAxis([false, false]) }
    }
}

impl nsGridContainerFrame_UsedTrackSizes {
    // void ResolveTrackSizesForAxis(nsGridContainerFrame* aFrame, LogicalAxis aAxis, gfxContext& aRC);
    // void ResolveSubgridTrackSizesForAxis(nsGridContainerFrame* aFrame, LogicalAxis aAxis, Subgrid* aSubgrid,
    //                                      gfxContext& aRC, nscoord aContentBoxSize);
    // (defined in the .cpp, in the third part of this file)

    // NS_DECLARE_FRAME_PROPERTY_DELETABLE(Prop, UsedTrackSizes)
    pub fn Prop() -> FramePropertyDescriptor<usize> {
        UsedTrackSizesProperty()
    }
    pub fn GetProp(aFrame: nsIFrame) -> Option<&'static mut UsedTrackSizes> {
        GridPropGet::<UsedTrackSizes>(aFrame, Self::Prop())
    }
    pub fn SetProp(aFrame: nsIFrame, aValue: Box<UsedTrackSizes>) {
        GridPropSet(aFrame, Self::Prop(), aValue)
    }
    pub fn RemoveProp(aFrame: nsIFrame) {
        GridPropRemove::<UsedTrackSizes>(aFrame, Self::Prop())
    }
    // GetOrCreateDeletableProperty(UsedTrackSizes::Prop())
    pub fn GetOrCreateProp(aFrame: nsIFrame) -> &'static mut UsedTrackSizes {
        GridPropGetOrCreate::<UsedTrackSizes>(aFrame, Self::Prop())
    }
}

/**
 * Encapsulates CSS track-sizing functions.
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::TrackSizingFunctions
pub struct nsGridContainerFrame_TrackSizingFunctions<'a> {
    // Some style data references, for easy access.
    pub mTemplate: &'a GridTemplate,
    pub mTrackListValues: &'a [TrackListValue],
    pub mAutoSizing: &'a StyleImplicitGridTracks,
    // An array from expanded track sizes (without expanding auto-repeat, which is
    // included just once at `mRepeatAutoStart`).
    //
    // Each entry contains two indices, the first into mTrackListValues, and a
    // second one inside mTrackListValues' repeat value, if any, or zero
    // otherwise.
    pub mExpandedTracks: Vec<(usize, usize)>,
    // Offset from the start of the implicit grid to the first explicit track.
    pub mExplicitGridOffset: u32,
    // The index of the repeat(auto-fill/fit) track, or zero if there is none.
    // Relative to mExplicitGridOffset (repeat tracks are explicit by definition).
    pub mRepeatAutoStart: u32,
    // The (hypothetical) index of the last such repeat() track.
    pub mRepeatAutoEnd: u32,
    // True if there is a specified repeat(auto-fill/fit) track.
    pub mHasRepeatAuto: bool,
    // True if this track (relative to mRepeatAutoStart) is a removed auto-fit.
    // Indexed relative to mExplicitGridOffset + mRepeatAutoStart.
    pub mRemovedRepeatTracks: Vec<bool>,
}
pub type TrackSizingFunctions<'a> = nsGridContainerFrame_TrackSizingFunctions<'a>;

// enum { ForSubgridFallbackTag };  (the tag of the private constructor below)
// Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::ForSubgridFallbackTag
#[derive(Clone, Copy)]
pub struct TrackSizingFunctions_ForSubgridFallbackTag;

impl<'a> nsGridContainerFrame_TrackSizingFunctions<'a> {
    // private:
    // TrackSizingFunctions(const GridTemplate& aTemplate, const StyleImplicitGridTracks& aAutoSizing,
    //                      const Maybe<size_t>& aRepeatAutoIndex, bool aIsSubgrid)
    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::TrackSizingFunctions
    fn new__GridTemplate_StyleImplicitGridTracks_Maybe_bool(aTemplate: &'a GridTemplate, aAutoSizing: &'a StyleImplicitGridTracks, aRepeatAutoIndex: &Option<usize>, aIsSubgrid: bool) -> TrackSizingFunctions<'a> {
        let repeatAutoStart = aRepeatAutoIndex.unwrap_or(0) as u32;
        let mut this = TrackSizingFunctions {
            mTemplate: aTemplate,
            mTrackListValues: aTemplate.TrackListValues(),
            mAutoSizing: aAutoSizing,
            mExpandedTracks: Vec::new(),
            mExplicitGridOffset: 0,
            mRepeatAutoStart: repeatAutoStart,
            mRepeatAutoEnd: repeatAutoStart,
            mHasRepeatAuto: aRepeatAutoIndex.is_some(),
            mRemovedRepeatTracks: Vec::new(),
        };
        MOZ_ASSERT!(!this.mHasRepeatAuto || !aIsSubgrid, "a track-list for a subgrid can't have an <auto-repeat> track");
        if !aIsSubgrid {
            this.ExpandNonRepeatAutoTracks();
        }

        #[cfg(debug_assertions)]
        {
            if this.mHasRepeatAuto {
                MOZ_ASSERT!(this.mExpandedTracks.len() >= 1);
                let maxTrack = (kMaxLine - 1) as usize;
                // If the exanded tracks are out of range of the maximum track, we
                // can't compare the repeat-auto start. It will be removed later during
                // grid item placement in that situation.
                if this.mExpandedTracks.len() < maxTrack {
                    MOZ_ASSERT!((this.mRepeatAutoStart as usize) < this.mExpandedTracks.len());
                }
            }
        }
        this
    }

    // public:
    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::TrackSizingFunctions
    pub fn new__GridTemplate_StyleImplicitGridTracks_bool(aGridTemplate: &'a GridTemplate, aAutoSizing: &'a StyleImplicitGridTracks, aIsSubgrid: bool) -> TrackSizingFunctions<'a> {
        Self::new__GridTemplate_StyleImplicitGridTracks_Maybe_bool(aGridTemplate, aAutoSizing, &aGridTemplate.RepeatAutoIndex(), aIsSubgrid)
    }

    // private:
    // TrackSizingFunctions(const GridTemplate& aGridTemplate, const StyleImplicitGridTracks& aAutoSizing,
    //                      decltype(ForSubgridFallbackTag))
    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::TrackSizingFunctions
    fn new__GridTemplate_StyleImplicitGridTracks_ForSubgridFallbackTag(aGridTemplate: &'a GridTemplate, aAutoSizing: &'a StyleImplicitGridTracks, _aTag: TrackSizingFunctions_ForSubgridFallbackTag) -> TrackSizingFunctions<'a> {
        Self::new__GridTemplate_StyleImplicitGridTracks_Maybe_bool(aGridTemplate, aAutoSizing, &None, /* aIsSubgrid */ true)
    }

    /**
     * This is used in a subgridded axis to resolve sizes before its parent's
     * sizes are known for intrinsic sizing purposes.  It copies the slice of
     * the nearest non-subgridded axis' track sizing functions spanned by
     * the subgrid.
     *
     * FIXME: this was written before there was a spec... the spec now says:
     * "If calculating the layout of a grid item in this step depends on
     *  the available space in the block axis, assume the available space
     *  that it would have if any row with a definite max track sizing
     *  function had that size and all other rows were infinite."
     * https://drafts.csswg.org/css-grid-2/#subgrid-sizing
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::ForSubgridFallback
    // NOT PORTED: subgrid (nsGridContainerFrame.cpp lines 1566-1612 at GECKO_REV; the result would borrow the
    // style of an ancestor grid container found by walking the subgrid chain).
    pub fn ForSubgridFallback(aSubgridFrame: nsGridContainerFrame, aSubgrid: &Subgrid, aParentGridContainer: nsGridContainerFrame, aParentAxis: LogicalAxis) -> ! {
        unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid): TrackSizingFunctions::ForSubgridFallback")
    }

    /**
     * Initialize the number of auto-fill/fit tracks to use.
     * This can be zero if no auto-fill/fit track was specified, or if the repeat
     * begins after the maximum allowed track.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::InitRepeatTracks
    pub fn InitRepeatTracks(&mut self, aGridGap: &StyleNonNegativeLengthPercentageOrNormal, aMinSize: nscoord, aSize: nscoord, aMaxSize: nscoord) {
        let maxTrack: u32 = (kMaxLine - 1) as u32;
        // Check for a repeat after the maximum allowed track.
        if self.mRepeatAutoStart >= maxTrack {
            self.mHasRepeatAuto = false;
            self.mRepeatAutoStart = 0;
            self.mRepeatAutoEnd = 0;
            return;
        }
        let mut repeatTracks: u32 = self.CalculateRepeatFillCount(aGridGap, aMinSize, aSize, aMaxSize) * self.NumRepeatTracks();
        // Clamp the number of repeat tracks to the maximum possible track.
        repeatTracks = std::cmp::min(repeatTracks, maxTrack - self.mRepeatAutoStart);
        self.SetNumRepeatTracks(repeatTracks);
        // Blank out the removed flags for each of these tracks.
        self.mRemovedRepeatTracks.clear();
        self.mRemovedRepeatTracks.resize(repeatTracks as usize, false);
        for track in self.mRemovedRepeatTracks.iter_mut() {
            *track = false;
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::CalculateRepeatFillCount
    pub fn CalculateRepeatFillCount(&self, aGridGap: &StyleNonNegativeLengthPercentageOrNormal, aMinSize: nscoord, aSize: nscoord, aMaxSize: nscoord) -> u32 {
        if !self.mHasRepeatAuto {
            return 0;
        }
        // At this point no tracks will have been collapsed, so the RepeatEndDelta
        // should not be negative.
        MOZ_ASSERT!(self.RepeatEndDelta() >= 0);
        // Note that this uses NumRepeatTracks and mRepeatAutoStart/End, although
        // the result of this method is used to change those values to a fully
        // expanded value. Spec quotes are from
        // https://drafts.csswg.org/css-grid-2/#repeat-notation
        let numTracks: u32 = (self.mExpandedTracks.len() as i64 + self.RepeatEndDelta() as i64) as u32;
        MOZ_ASSERT!(numTracks >= 1, "expected at least the repeat() track");
        if numTracks >= kMaxLine as u32 {
            // The fixed tracks plus an entire repetition is either larger or as
            // large as the maximum track, so we do not need to measure how many
            // repetitions will fit. This also avoids needing to check for if
            // kMaxLine - numTracks would underflow at the end where we clamp the
            // result.
            return 1;
        }
        let maxFill: nscoord = if aSize != NS_UNCONSTRAINEDSIZE { aSize } else { aMaxSize };
        if maxFill == NS_UNCONSTRAINEDSIZE && aMinSize == 0 {
            // "Otherwise, the specified track list repeats only once."
            return 1;
        }
        let mut repeatTrackSum: nscoord = 0;
        // Note that one repeat() track size is included in |sum| in this loop.
        let mut sum: nscoord = 0;
        let percentBasis: nscoord = aSize;
        for i in 0..numTracks {
            // "treating each track as its max track sizing function if that is
            // definite or as its minimum track sizing function otherwise"
            // https://drafts.csswg.org/css-grid-2/#valdef-repeat-auto-fill
            let mut trackSize: nscoord;
            {
                let sizingFunction = self.SizingFor(i);
                let maxCoord = sizingFunction.GetMax();
                let minCoord = sizingFunction.GetMin();
                if maxCoord.IsBreadth() && minCoord.IsBreadth() {
                    // If the max is less than the min, then the max will be floored by
                    // the min (essentially yielding minmax(min, min))
                    // https://drafts.csswg.org/css-grid-2/#funcdef-grid-template-columns-minmax
                    let minSize = ResolveToDefiniteSize(minCoord, percentBasis);
                    let maxSize = ResolveToDefiniteSize(maxCoord, percentBasis);
                    trackSize = std::cmp::max(maxSize, minSize);
                } else {
                    let mut coord = maxCoord;
                    if !coord.IsBreadth() {
                        coord = minCoord;
                        if !coord.IsBreadth() {
                            return 1;
                        }
                    }
                    trackSize = ResolveToDefiniteSize(coord, percentBasis);
                }
            }

            if i >= self.mRepeatAutoStart && i < self.mRepeatAutoEnd {
                // Use a minimum 1px for the repeat() track-size.
                if trackSize < AppUnitsPerCSSPixel() {
                    trackSize = AppUnitsPerCSSPixel();
                }
                repeatTrackSum += trackSize;
            }
            sum += trackSize;
        }
        let gridGap: nscoord = nsLayoutUtils::ResolveGapToLength(aGridGap, aSize);
        if numTracks > 1 {
            // Add grid-gaps for all the tracks including the repeat() track.
            sum += gridGap * (numTracks as nscoord - 1);
        }
        // Calculate the max number of tracks that fits without overflow.
        let available: nscoord = if maxFill != NS_UNCONSTRAINEDSIZE { maxFill } else { aMinSize };
        let spaceToFill: nscoord = available - sum;
        if spaceToFill <= 0 {
            // "if any number of repetitions would overflow, then 1 repetition"
            return 1;
        }
        // Calculate the max number of tracks that fits without overflow.
        // Since we already have one repetition in sum, we can simply add one grid
        // gap for each element in the repeat.
        // div_t q = div(spaceToFill, repeatTrackSum + gridGap * NumRepeatTracks());
        let divisor: nscoord = repeatTrackSum + gridGap * self.NumRepeatTracks() as nscoord;
        let q_quot = spaceToFill / divisor;
        let q_rem = spaceToFill % divisor;
        // The +1 here is for the one repeat track we already accounted for above.
        let mut numRepeatTracks: u32 = (q_quot + 1) as u32;
        if q_rem != 0 && maxFill == NS_UNCONSTRAINEDSIZE {
            // "Otherwise, if the grid container has a definite min size in
            // the relevant axis, the number of repetitions is the largest possible
            // positive integer that fulfills that minimum requirement."
            numRepeatTracks += 1; // one more to ensure the grid is at least min-size
        }
        // Clamp the number of repeat tracks so that the last line <= kMaxLine.
        // (note that |numTracks| already includes one repeat() track)
        MOZ_ASSERT!(numTracks >= self.NumRepeatTracks());
        let maxRepeatTrackCount: u32 = kMaxLine as u32 - numTracks;
        let maxRepetitions: u32 = maxRepeatTrackCount / self.NumRepeatTracks();
        std::cmp::min(numRepeatTracks, maxRepetitions)
    }

    /**
     * Compute the explicit grid end line number (in a zero-based grid).
     * @param aGridTemplateAreasEnd 'grid-template-areas' end line in this axis
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::ComputeExplicitGridEnd
    pub fn ComputeExplicitGridEnd(&self, aGridTemplateAreasEnd: u32) -> u32 {
        let mut end: u32 = self.NumExplicitTracks() + 1;
        end = std::cmp::max(end, aGridTemplateAreasEnd);
        end = std::cmp::min(end, kMaxLine as u32);
        end
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::SizingFor
    pub fn SizingFor(&self, aTrackIndex: u32) -> &StyleTrackSize {
        // |aIndex| is the relative index to mAutoSizing. A negative value means it
        // is the last Nth element.
        // auto getImplicitSize = [this](int32_t aIndex) -> const StyleTrackSize& { ... }
        fn getImplicitSize<'b>(this: &'b TrackSizingFunctions<'_>, aIndex: i32) -> &'b StyleTrackSize {
            let kAutoTrackSize = kAutoTrackSize();
            MOZ_ASSERT!(!(this.mAutoSizing.0.len() == 1 && this.mAutoSizing.0[0] == *kAutoTrackSize), "It's impossible to have one track with auto value because we filter out this case during parsing");

            if this.mAutoSizing.0.is_empty() {
                return kAutoTrackSize;
            }

            // If multiple track sizes are given, the pattern is repeated as necessary
            // to find the size of the implicit tracks.
            let mut i: i32 = aIndex % this.mAutoSizing.0.len() as i32;
            if i < 0 {
                i += this.mAutoSizing.0.len() as i32;
            }
            &this.mAutoSizing.0[i as usize]
        }

        if aTrackIndex < self.mExplicitGridOffset {
            // The last implicit grid track before the explicit grid receives the
            // last specified size, and so on backwards. Therefore we pass the
            // negative relative index to imply that we should get the implicit size
            // from the last Nth specified grid auto size.
            return getImplicitSize(self, aTrackIndex as i32 - self.mExplicitGridOffset as i32);
        }
        let mut index: u32 = aTrackIndex - self.mExplicitGridOffset;
        MOZ_ASSERT!(self.mRepeatAutoStart <= self.mRepeatAutoEnd);

        if index >= self.mRepeatAutoStart {
            if index < self.mRepeatAutoEnd {
                // Expand the repeat tracks.
                let indices = &self.mExpandedTracks[self.mRepeatAutoStart as usize];
                let value = &self.mTrackListValues[indices.0];

                // We expect the default to be used for all track repeats.
                MOZ_ASSERT!(indices.1 == 0);

                let repeatTracks: &[StyleTrackSize] = match value {
                    GenericTrackListValue::TrackRepeat(r) => &r.track_sizes[..],
                    _ => panic!("AsTrackRepeat on a track list value that is a track size"),
                };

                // Find the repeat track to use, skipping over any collapsed tracks.
                let finalRepeatIndex: u32 = index - self.mRepeatAutoStart;
                let mut repeatWithCollapsed: u32 = 0;
                // NOTE: We need SizingFor before the final collapsed tracks are known.
                // We know that it's invalid to have empty mRemovedRepeatTracks when
                // there are any repeat tracks, so we can detect that situation here.
                if self.mRemovedRepeatTracks.is_empty() {
                    repeatWithCollapsed = finalRepeatIndex;
                } else {
                    // Count up through the repeat tracks, until we have seen
                    // finalRepeatIndex number of non-collapsed tracks.
                    let mut repeatNoCollapsed: u32 = 0;
                    while repeatNoCollapsed < finalRepeatIndex {
                        if !self.mRemovedRepeatTracks[repeatWithCollapsed as usize] {
                            repeatNoCollapsed += 1;
                        }
                        repeatWithCollapsed += 1;
                    }
                    // If we stopped iterating on a collapsed track, continue to the next
                    // non-collapsed track.
                    while self.mRemovedRepeatTracks[repeatWithCollapsed as usize] {
                        repeatWithCollapsed += 1;
                    }
                }
                return &repeatTracks[repeatWithCollapsed as usize % repeatTracks.len()];
            } else {
                // The index is after the repeat auto range, adjust it to skip over the
                // repeat value. This will have no effect if there is no auto repeat,
                // since then RepeatEndDelta will return zero.
                index = (index as i64 - self.RepeatEndDelta() as i64) as u32;
            }
        }
        if index as usize >= self.mExpandedTracks.len() {
            return getImplicitSize(self, (index as usize - self.mExpandedTracks.len()) as i32);
        }
        let indices = &self.mExpandedTracks[index as usize];
        let value = &self.mTrackListValues[indices.0];
        match value {
            GenericTrackListValue::TrackSize(s) => {
                MOZ_ASSERT!(indices.1 == 0);
                s
            }
            GenericTrackListValue::TrackRepeat(r) => &r.track_sizes[..][indices.1],
        }
    }
    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::MaxSizingFor
    pub fn MaxSizingFor(&self, aTrackIndex: u32) -> &StyleTrackBreadth {
        self.SizingFor(aTrackIndex).GetMax()
    }
    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::MinSizingFor
    pub fn MinSizingFor(&self, aTrackIndex: u32) -> &StyleTrackBreadth {
        self.SizingFor(aTrackIndex).GetMin()
    }
    pub fn NumExplicitTracks(&self) -> u32 {
        (self.mExpandedTracks.len() as i64 + self.RepeatEndDelta() as i64) as u32
    }
    pub fn NumRepeatTracks(&self) -> u32 {
        self.mRepeatAutoEnd - self.mRepeatAutoStart
    }
    // The difference between mExplicitGridEnd and mSizingFunctions.Length().
    pub fn RepeatEndDelta(&self) -> i32 {
        if self.mHasRepeatAuto { self.NumRepeatTracks() as i32 - 1 } else { 0 }
    }
    pub fn SetNumRepeatTracks(&mut self, aNumRepeatTracks: u32) {
        MOZ_ASSERT!(self.mHasRepeatAuto || aNumRepeatTracks == 0);
        self.mRepeatAutoEnd = self.mRepeatAutoStart + aNumRepeatTracks;
    }

    // Store mTrackListValues into mExpandedTracks with `repeat(INTEGER, ...)`
    // tracks expanded.
    // Gecko: layout/generic/nsGridContainerFrame.cpp TrackSizingFunctions::ExpandNonRepeatAutoTracks
    pub fn ExpandNonRepeatAutoTracks(&mut self) {
        let trackListValues = self.mTrackListValues;
        for i in 0..trackListValues.len() {
            let value = &trackListValues[i];
            let repeat = match value {
                GenericTrackListValue::TrackSize(_) => {
                    self.mExpandedTracks.push((i, 0));
                    continue;
                }
                GenericTrackListValue::TrackRepeat(repeat) => repeat,
            };
            let count = match repeat.count {
                RepeatCount::Number(n) => n,
                _ => {
                    MOZ_ASSERT!(i == self.mRepeatAutoStart as usize);
                    self.mRepeatAutoStart = self.mExpandedTracks.len() as u32;
                    self.mRepeatAutoEnd = self.mRepeatAutoStart + repeat.track_sizes.len() as u32;
                    self.mExpandedTracks.push((i, 0));
                    continue;
                }
            };
            for _j in 0..count {
                let trackSizesCount = repeat.track_sizes.len();
                for k in 0..trackSizesCount {
                    self.mExpandedTracks.push((i, k));
                }
            }
        }
        if self.mExpandedTracks.len() > (kMaxLine - 1) as usize {
            self.mExpandedTracks.truncate((kMaxLine - 1) as usize);
            if self.mHasRepeatAuto && self.mRepeatAutoStart > (kMaxLine - 1) as u32 {
                // The `repeat(auto-fill/fit)` track is outside the clamped grid.
                self.mHasRepeatAuto = false;
            }
        }
    }
}

// Translated from layout/generic/nsGridContainerFrame.cpp (GECKO_REV), part 2 of 6: .cpp lines 1912-3650
// (LineNameMap, struct Tracks, SharedGridData, GridReflowInput). Joined with the other parts into
// nsGridContainerFrame.rs at the end.
//
// Rust shape (see CONVENTIONS.md and the notes at the top of nsGridContainerFrame.rs):
//  - Nested C++ types are `nsGridContainerFrame_<Name>` with the short alias the C++ code uses.
//  - `LineNameMap<'a>` borrows the style data (name lists) for 'a; `SmallPointerArray<const NameList>` is
//    `Vec<&'a NameList>`; `Span<T>` is `&[T]`.
//  - `nsAtom*` parameters (area and line names) are `&str` here: Stylo's `CustomIdent` holds a string_cache
//    atom that derefs to `str`, so `name.AsAtom() == aName` is `&*name.0 == aName`.
//  - `std::function<bool(uint32_t, nscoord, nscoord*)>` (FitContentClamper) is
//    `Option<&dyn Fn(u32, nscoord, &mut nscoord) -> bool>`; a null std::function is `None`.
//  - `GridReflowInput` owns a clone of the frame's style (`mGridStyle`, an nsStylePosition wrapper over
//    an Arc of the computed values); the TrackSizingFunctions that borrow from it are given a 'static lifetime
//    by an unsafe cast (see GridReflowInput::new__nsGridContainerFrame_gfxContext_ReflowInput_nsStylePosition_WritingMode):
//    the Arc is never replaced for the life of the GridReflowInput and the values live on the Arc's heap block.
//
// NOT PORTED (each marked where it occurs): subgrid (ExpandRepeatLineNamesForSubgrid and the subgrid item
// collection helpers of GridReflowInput), fragmentation (GridReflowInput::InitializeForContinuation), devtools
// data (LineNameMap::GetResolvedLineNamesForComputedGridTrackInfo and GetExplicitLineNamesAtIndex,
// Tracks::Dump).

// ADAPTER: Gecko's GetOppositeBaselineSharingGroup (nsIFrame.h / WritingModes.h) is not in this crate yet.
// (layout/generic/nsIFrame.h mozilla::GetOppositeBaselineSharingGroup; the drift tool cannot locate it at the base revision, so no marker)
#[allow(dead_code)]
pub fn GetOppositeBaselineSharingGroup(aGroup: BaselineSharingGroup) -> BaselineSharingGroup {
    if aGroup == BaselineSharingGroup::First { BaselineSharingGroup::Last } else { BaselineSharingGroup::First }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap
/**
 * Utility class to find line names.  It provides an interface to lookup line
 * names with a dynamic number of repeat(auto-fill/fit) tracks taken into
 * account.
 */
pub struct nsGridContainerFrame_LineNameMap<'a> {
    // The min/max line number (1-based) for clamping.
    pub mClampMinLine: i32,
    pub mClampMaxLine: i32,

    // Some style data references, for easy access.
    mStylePosition: &'a nsStylePosition,
    mAreas: Option<&'a ImplicitNamedAreas>,
    // The expanded list of line-names. Each entry is usually a single NameList,
    // but can be multiple in the case where repeat() expands to something that
    // has a line name list at the end.
    mExpandedLineNames: Vec<Vec<&'a NameList>>,
    // The repeat(auto-fill/fit) track value, if any. (always empty for subgrid)
    mTrackAutoRepeatLineNames: &'a [NameList],
    // The index of the repeat(auto-fill/fit) track, or zero if there is none.
    mRepeatAutoStart: u32,
    // The index one past the end of the repeat(auto-fill/fit) tracks. Equal to
    // mRepeatAutoStart if there are no repeat(auto-fill/fit) tracks.
    mRepeatAutoEnd: u32,
    // The total number of repeat tracks minus 1.
    mRepeatEndDelta: i32,
    // The end of the line name lists with repeat(auto-fill/fit) tracks accounted
    // for.
    mTemplateLinesEnd: u32,

    // The parent line map, or null if this map isn't for a subgrid.
    mParentLineNameMap: Option<&'a nsGridContainerFrame_LineNameMap<'a>>,
    // The subgrid's range, or null if this map isn't for a subgrid.
    mRange: Option<&'a LineRange>,
    // True if the subgrid/parent axes progresses in the same direction.
    mIsSameDirection: bool,
    // True if the subgrid and parent have orthogonal writing modes.
    mIsOrthogonal: bool,

    // True if there is a specified repeat(auto-fill/fit) track.
    mHasRepeatAuto: bool,
}
pub type LineNameMap<'a> = nsGridContainerFrame_LineNameMap<'a>;

impl<'a> nsGridContainerFrame_LineNameMap<'a> {
    /**
     * Create a LineNameMap.
     * @param aStylePosition the style for the grid container
     * @param aImplicitNamedAreas the implicit areas for the grid container
     * @param aGridTemplate is the grid-template-rows/columns data for this axis
     * @param aParentLineNameMap the parent grid's map parallel to this map, or
     *                           null if this map isn't for a subgrid
     * @param aRange the subgrid's range in the parent grid, or null
     * @param aIsSameDirection true if our axis progresses in the same direction
     *                              in the subgrid and parent
     * @param aIsOrthogonal true if the subgrid and parent have orthongonal
     *                           writing modes
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::LineNameMap
    // ADAPTER: the C++ defaults (aIsSameDirection = true, aIsOrthogonal = false) must be given by the caller.
    pub fn new(
        aStylePosition: &'a nsStylePosition,
        aImplicitNamedAreas: Option<&'a ImplicitNamedAreas>,
        aTracks: &TrackSizingFunctions<'a>,
        aParentLineNameMap: Option<&'a LineNameMap<'a>>,
        aRange: Option<&'a LineRange>,
        aIsSameDirection: bool,
        aIsOrthogonal: bool,
    ) -> LineNameMap<'a> {
        let mut this = LineNameMap {
            mClampMinLine: 0,
            mClampMaxLine: 0,
            mStylePosition: aStylePosition,
            mAreas: aImplicitNamedAreas,
            mExpandedLineNames: Vec::new(),
            mTrackAutoRepeatLineNames: &[],
            mRepeatAutoStart: aTracks.mRepeatAutoStart,
            mRepeatAutoEnd: aTracks.mRepeatAutoEnd,
            mRepeatEndDelta: aTracks.RepeatEndDelta(),
            mTemplateLinesEnd: 0,
            mParentLineNameMap: aParentLineNameMap,
            mRange: aRange,
            mIsSameDirection: aIsSameDirection,
            mIsOrthogonal: aIsOrthogonal,
            mHasRepeatAuto: aTracks.mHasRepeatAuto,
        };
        if let Some(aRange) = aRange {
            // subgrid case
            this.mClampMinLine = 1;
            this.mClampMaxLine = 1 + aRange.Extent() as i32;
            // MOZ_ASSERT(aTracks.mTemplate.IsSubgrid(), "Should be subgrid type");
            match aTracks.mTemplate {
                GenericGridTemplateComponent::Subgrid(l) => this.ExpandRepeatLineNamesForSubgrid(l),
                _ => MOZ_ASSERT_UNREACHABLE!("Should be subgrid type"),
            }
            // we've expanded all subgrid auto-fill lines in
            // ExpandRepeatLineNamesForSubgrid()
            this.mRepeatAutoStart = 0;
            this.mRepeatAutoEnd = this.mRepeatAutoStart;
            this.mHasRepeatAuto = false;
        } else {
            this.mClampMinLine = kMinLine;
            this.mClampMaxLine = kMaxLine;
            if this.mHasRepeatAuto {
                // aTracks.mTemplate.GetRepeatAutoValue()->line_names.AsSpan()
                // (GetRepeatAutoValue: the track list's value at auto_repeat_index, a repeat)
                this.mTrackAutoRepeatLineNames = match aTracks.mTemplate {
                    GenericGridTemplateComponent::TrackList(list) => match &list.values[list.auto_repeat_index] {
                        GenericTrackListValue::TrackRepeat(repeat) => &repeat.line_names[..],
                        _ => unreachable!("auto_repeat_index must point to a repeat"),
                    },
                    _ => unreachable!("mHasRepeatAuto needs a track list"),
                };
            }
            this.ExpandRepeatLineNames(aTracks);
        }
        if this.mHasRepeatAuto {
            // We need mTemplateLinesEnd to be after all line names.
            // mExpandedLineNames has one repetition of the repeat(auto-fit/fill)
            // track name lists already, so we must subtract the number of repeat
            // track name lists to get to the number of non-repeat tracks, minus 2
            // because the first and last line name lists are shared with the
            // preceding and following non-repeat line name lists. We then add
            // mRepeatEndDelta to include the interior line name lists from repeat
            // tracks.
            // (unsigned arithmetic in C++: wraps)
            this.mTemplateLinesEnd = (this.mExpandedLineNames.len() as u32)
                .wrapping_sub((this.mTrackAutoRepeatLineNames.len() as u32).wrapping_sub(2))
                .wrapping_add(this.mRepeatEndDelta as u32);
        } else {
            this.mTemplateLinesEnd = this.mExpandedLineNames.len() as u32;
        }
        MOZ_ASSERT!(this.mHasRepeatAuto || this.mRepeatEndDelta <= 0);
        MOZ_ASSERT!(
            !this.mHasRepeatAuto || aRange.is_some() || (this.mExpandedLineNames.len() >= 2 && this.mRepeatAutoStart as usize <= this.mExpandedLineNames.len())
        );
        this
    }

    // Store line names into mExpandedLineNames with `repeat(INTEGER, ...)`
    // expanded for non-subgrid.
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::ExpandRepeatLineNames
    pub fn ExpandRepeatLineNames(&mut self, aTracks: &TrackSizingFunctions<'a>) {
        let lineNameLists: &'a [NameList] = aTracks.mTemplate.LineNameLists(false);

        let trackListValues: &'a [TrackListValue] = aTracks.mTrackListValues;
        let mut nameListToMerge: Option<&'a NameList> = None;
        // NOTE(emilio): We rely on std::move clearing out the array.
        // (std::mem::take does the same here)
        let mut names: Vec<&'a NameList> = Vec::new();
        let end: u32 = std::cmp::min(lineNameLists.len() as u32, (self.mClampMaxLine + 1) as u32);
        for i in 0..end {
            if let Some(n) = nameListToMerge.take() {
                names.push(n);
            }
            names.push(&lineNameLists[i as usize]);
            if i as usize >= trackListValues.len() {
                self.mExpandedLineNames.push(std::mem::take(&mut names));
                continue;
            }
            let value = &trackListValues[i as usize];
            let repeat = match value {
                GenericTrackListValue::TrackSize(_) => {
                    self.mExpandedLineNames.push(std::mem::take(&mut names));
                    continue;
                }
                GenericTrackListValue::TrackRepeat(repeat) => repeat,
            };
            let count = match &repeat.count {
                RepeatCount::Number(n) => Some(*n),
                _ => None,
            };
            let count = match count {
                None => {
                    let repeatNames: &'a [NameList] = &repeat.line_names;
                    // If the repeat was truncated due to more than kMaxLine tracks, then
                    // the repeat will no longer be set on mRepeatAutoStart).
                    MOZ_ASSERT!(!self.mHasRepeatAuto || self.mRepeatAutoStart as usize == self.mExpandedLineNames.len());
                    MOZ_ASSERT!(repeatNames.len() >= 2);
                    for j in 0..(repeatNames.len() - 1) {
                        names.push(&repeatNames[j]);
                        self.mExpandedLineNames.push(std::mem::take(&mut names));
                    }
                    nameListToMerge = Some(&repeatNames[repeatNames.len() - 1]);
                    continue;
                }
                Some(n) => n,
            };
            for _j in 0..count {
                if let Some(n) = nameListToMerge.take() {
                    names.push(n);
                }
                let trackSizesCount = repeat.track_sizes.len();
                let repeatLineNames: &'a [NameList] = &repeat.line_names;
                MOZ_ASSERT!(repeatLineNames.len() == trackSizesCount || repeatLineNames.len() == trackSizesCount + 1);
                for k in 0..trackSizesCount {
                    names.push(&repeatLineNames[k]);
                    self.mExpandedLineNames.push(std::mem::take(&mut names));
                }
                if repeatLineNames.len() == trackSizesCount + 1 {
                    nameListToMerge = Some(&repeatLineNames[trackSizesCount]);
                }
            }
        }

        if self.mExpandedLineNames.len() > self.mClampMaxLine as u32 as usize {
            self.mExpandedLineNames.truncate(self.mClampMaxLine as usize);
        }
    }

    // Store line names into mExpandedLineNames with `repeat(INTEGER, ...)`
    // expanded, and all `repeat(...)` expanded for subgrid.
    // https://drafts.csswg.org/css-grid-2/#resolved-track-list-subgrid
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::ExpandRepeatLineNamesForSubgrid
    // NOT PORTED: subgrid (original: nsGridContainerFrame.cpp lines 2051-2133 at GECKO_REV).
    pub fn ExpandRepeatLineNamesForSubgrid(&mut self, aStyleLineNameList: &'a StyleLineNameList) {
        unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)")
    }

    /**
     * Find the aNth occurrence of aName, searching forward if aNth is positive,
     * and in reverse if aNth is negative (aNth == 0 is invalid), starting from
     * aFromIndex (not inclusive), and return a 1-based line number.
     * Also take into account there is an unconditional match at the lines in
     * aImplicitLines.
     * Return zero if aNth occurrences can't be found.  In that case, aNth has
     * been decremented with the number of occurrences that were found (if any).
     *
     * E.g. to search for "A 2" forward from the start of the grid: aName is "A"
     * aNth is 2 and aFromIndex is zero.  To search for "A -2", aNth is -2 and
     * aFromIndex is ExplicitGridEnd + 1 (which is the line "before" the last
     * line when we're searching in reverse).  For "span A 2", aNth is 2 when
     * used on a grid-[row|column]-end property and -2 for a *-start property,
     * and aFromIndex is the line (which we should skip) on the opposite property.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::FindNamedLine
    pub fn FindNamedLine(&self, aName: &str, aNth: &mut i32, aFromIndex: u32, aImplicitLines: &Vec<u32>) -> u32 {
        MOZ_ASSERT!(!aName.is_empty());
        MOZ_ASSERT!(*aNth != 0);
        if *aNth > 0 {
            return self.FindLine(aName, aNth, aFromIndex, aImplicitLines);
        }
        let mut nth: i32 = -*aNth;
        let line = self.RFindLine(aName, &mut nth, aFromIndex, aImplicitLines) as i32;
        *aNth = -nth;
        line as u32
    }

    /**
     * Return a set of lines in aImplicitLines which matches the area name aName
     * on aSide.  For example, for aName "a" and aSide being an end side, it
     * returns the line numbers which would match "a-end" in the relevant axis.
     * For subgrids it includes searching the relevant axis in all ancestor
     * grids too (within this subgrid's spanned area).  If an ancestor has
     * opposite direction, we switch aSide to the opposite logical side so we
     * match on the same physical side as the original subgrid we're resolving
     * the name for.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::FindNamedAreas
    pub fn FindNamedAreas(&self, aName: &str, mut aSide: LogicalSide, aImplicitLines: &mut Vec<u32>) {
        // True if we're currently in a map that has the same direction as 'this'.
        let mut sameDirectionAsThis = true;
        let mut min: u32 = if self.mParentLineNameMap.is_none() { 1 } else { self.mClampMinLine as u32 };
        let mut max: u32 = self.mClampMaxLine as u32;
        let mut map: &LineNameMap = self;
        loop {
            let mut line = map.FindNamedArea__nsAtom_LogicalSide_int32_t_int32_t(aName, aSide, min as i32, max as i32);
            if line > 0 {
                if sameDirectionAsThis {
                    line -= min - 1;
                } else {
                    line = max - line + 1;
                }
                aImplicitLines.push(line);
            }
            let parent = map.mParentLineNameMap;
            let parent = match parent {
                None => {
                    if aImplicitLines.len() > 1 {
                        // Remove duplicates and sort in ascending order.
                        aImplicitLines.sort();
                        let mut i = 0;
                        while i < aImplicitLines.len() {
                            let prev = aImplicitLines[i];
                            let mut j = i + 1;
                            let start = j;
                            while j < aImplicitLines.len() && aImplicitLines[j] == prev {
                                j += 1;
                            }
                            if j != start {
                                aImplicitLines.drain(start..j);
                            }
                            i += 1;
                        }
                    }
                    return;
                }
                Some(p) => p,
            };
            if !map.mIsSameDirection {
                aSide = GetOppositeSide(aSide);
                sameDirectionAsThis = !sameDirectionAsThis;
            }
            if map.mIsOrthogonal {
                aSide = MakeLogicalSide(GetOrthogonalAxis(GetAxis(aSide)), GetEdge(aSide));
            }
            min = map.TranslateToParentMap(min);
            max = map.TranslateToParentMap(max);
            if min > max {
                MOZ_ASSERT!(!map.mIsSameDirection);
                std::mem::swap(&mut min, &mut max);
            }
            map = parent;
        }
    }

    /**
     * Return true if any implicit named areas match aName, in this map or
     * in any of our ancestor maps.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::HasImplicitNamedArea
    pub fn HasImplicitNamedArea(&self, aName: &str) -> bool {
        let mut map: &LineNameMap = self;
        loop {
            // (mAreas->has(aName); the key is the area's atom, which derefs to str)
            if let Some(areas) = map.mAreas {
                if areas.iter().any(|(k, _)| &**k == aName) {
                    return true;
                }
            }
            match map.mParentLineNameMap {
                Some(p) => map = p,
                None => break,
            }
        }
        false
    }

    // For generating line name data for devtools.
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::GetResolvedLineNamesForComputedGridTrackInfo
    // NOT PORTED: devtools data (only the Grid DOM API reads it). Original (nsGridContainerFrame.cpp lines 2229-2241):
    //   nsTArray<nsTArray<StyleCustomIdent>> GetResolvedLineNamesForComputedGridTrackInfo() const {
    //     for each expandedLine in mExpandedLineNames: line = concatenation of every name in every chunk; result.AppendElement(line)
    //   }
    pub fn GetResolvedLineNamesForComputedGridTrackInfo(&self) {
        unimplemented!("NOT PORTED: devtools grid info (GetResolvedLineNamesForComputedGridTrackInfo)")
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::GetExplicitLineNamesAtIndex
    // NOT PORTED: devtools data (only the Grid DOM API reads it). Original (nsGridContainerFrame.cpp lines 2243-2254):
    //   nsTArray<RefPtr<nsAtom>> GetExplicitLineNamesAtIndex(uint32_t aIndex) const {
    //     if (aIndex < mTemplateLinesEnd) { for each nameList in GetLineNamesAt(aIndex): for each name: lineNames.AppendElement(name.AsAtom()) }
    //   }
    pub fn GetExplicitLineNamesAtIndex(&self, aIndex: u32) {
        unimplemented!("NOT PORTED: devtools grid info (GetExplicitLineNamesAtIndex)")
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::ExpandedLineNames
    pub fn ExpandedLineNames(&self) -> &Vec<Vec<&'a NameList>> {
        &self.mExpandedLineNames
    }
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::TrackAutoRepeatLineNames
    pub fn TrackAutoRepeatLineNames(&self) -> &'a [NameList] {
        self.mTrackAutoRepeatLineNames
    }
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::HasRepeatAuto
    pub fn HasRepeatAuto(&self) -> bool {
        self.mHasRepeatAuto
    }
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::NumRepeatTracks
    pub fn NumRepeatTracks(&self) -> u32 {
        self.mRepeatAutoEnd - self.mRepeatAutoStart
    }
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::RepeatAutoStart
    pub fn RepeatAutoStart(&self) -> u32 {
        self.mRepeatAutoStart
    }

    // Return true if this map represents a subgridded axis.
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::IsSubgridded
    fn IsSubgridded(&self) -> bool {
        self.mParentLineNameMap.is_some()
    }

    /**
     * @see FindNamedLine, this function searches forward.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::FindLine
    fn FindLine(&self, aName: &str, aNth: &mut i32, aFromIndex: u32, aImplicitLines: &Vec<u32>) -> u32 {
        MOZ_ASSERT!(*aNth > 0);
        let mut nth: i32 = *aNth;
        // For a subgrid we need to search to the end of the grid rather than
        // the end of the local name list, since ancestors might match.
        let end: u32 = if self.IsSubgridded() { self.mClampMaxLine as u32 } else { self.mTemplateLinesEnd };
        let mut line: u32;
        let mut i: u32 = aFromIndex;
        while i < end {
            line = i + 1;
            if self.Contains__uint32_t_nsAtom(i, aName) || aImplicitLines.contains(&line) {
                nth -= 1;
                if nth == 0 {
                    return line;
                }
            }
            i = line;
        }
        for &implicitLine in aImplicitLines.iter() {
            if implicitLine > i {
                // implicitLine is after the lines we searched above so it's last.
                // (grid-template-areas has more tracks than
                // grid-template-[rows|columns])
                nth -= 1;
                if nth == 0 {
                    return implicitLine;
                }
            }
        }
        MOZ_ASSERT!(nth > 0, "should have returned a valid line above already");
        *aNth = nth;
        0
    }

    /**
     * @see FindNamedLine, this function searches in reverse.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::RFindLine
    fn RFindLine(&self, aName: &str, aNth: &mut i32, mut aFromIndex: u32, aImplicitLines: &Vec<u32>) -> u32 {
        MOZ_ASSERT!(*aNth > 0);
        if aFromIndex == 0 {
            return 0; // There are no named lines beyond the start of the explicit
                      // grid.
        }
        aFromIndex -= 1; // (shift aFromIndex so we can treat it as inclusive)
        let mut nth: i32 = *aNth;
        // Implicit lines may be beyond the explicit grid so we match those
        // first if it's within the mTemplateLinesEnd..aFromIndex range.
        // aImplicitLines is presumed sorted.
        // For a subgrid we need to search to the end of the grid rather than
        // the end of the local name list, since ancestors might match.
        let end: u32 = if self.IsSubgridded() { self.mClampMaxLine as u32 } else { self.mTemplateLinesEnd };
        for &implicitLine in aImplicitLines.iter().rev() {
            if implicitLine <= end {
                break;
            }
            if implicitLine < aFromIndex {
                nth -= 1;
                if nth == 0 {
                    return implicitLine;
                }
            }
        }
        let mut i: u32 = std::cmp::min(aFromIndex, end);
        while i != 0 {
            if self.Contains__uint32_t_nsAtom(i - 1, aName) || aImplicitLines.contains(&i) {
                nth -= 1;
                if nth == 0 {
                    return i;
                }
            }
            i -= 1;
        }
        MOZ_ASSERT!(nth > 0, "should have returned a valid line above already");
        *aNth = nth;
        0
    }

    // Return true if aName exists at aIndex in this map or any parent map.
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::Contains(uint32_t, nsAtom*)
    fn Contains__uint32_t_nsAtom(&self, mut aIndex: u32, aName: &str) -> bool {
        let mut map: &LineNameMap = self;
        loop {
            if aIndex < map.mTemplateLinesEnd && map.HasNameAt(aIndex, aName) {
                return true;
            }
            let parent = match map.mParentLineNameMap {
                None => return false,
                Some(p) => p,
            };
            let line = map.TranslateToParentMap(aIndex + 1);
            MOZ_ASSERT!(line >= 1, "expected a 1-based line number");
            aIndex = line - 1;
            map = parent;
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::Contains(Span<const StyleCustomIdent>, nsAtom*)
    fn Contains__Span_nsAtom(aNames: &[StyleCustomIdent], aName: &str) -> bool {
        for name in aNames {
            if &*name.0 == aName {
                return true;
            }
        }
        false
    }

    // Return true if aName exists at aIndex in this map.
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::HasNameAt
    fn HasNameAt(&self, aIndex: u32, aName: &str) -> bool {
        let nameLists = self.GetLineNamesAt(aIndex);
        for nameList in nameLists {
            if Self::Contains__Span_nsAtom(nameList, aName) {
                return true;
            }
        }
        false
    }

    // Get the line names at an index.
    // This accounts for auto repeat. The results may be spread over multiple name
    // lists returned in the array, which is done to avoid unneccessarily copying
    // the arrays to concatenate them.
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::GetLineNamesAt
    fn GetLineNamesAt(&self, aIndex: u32) -> Vec<&'a NameList> {
        let mut names: Vec<&'a NameList> = Vec::new();
        // The index into mExpandedLineNames to use, if aIndex doesn't point to a
        // name inside of a auto repeat.
        let mut repeatAdjustedIndex: u32 = aIndex;
        // Note: For subgrid, |mHasRepeatAuto| is always false because we have
        // expanded it in the constructor of LineNameMap.
        if self.mHasRepeatAuto {
            // If the index is inside of the auto repeat, use the repeat line
            // names. Otherwise, if the index is past the end of the repeat it must
            // be adjusted to acount for the repeat tracks.
            // mExpandedLineNames has the first and last line name lists from the
            // repeat in it already, so we can just ignore aIndex == mRepeatAutoStart
            // and treat when aIndex == mRepeatAutoEnd the same as any line after the
            // the repeat.
            let maxRepeatLine: u32 = self.mTrackAutoRepeatLineNames.len() as u32 - 1;
            if aIndex > self.mRepeatAutoStart && aIndex < self.mRepeatAutoEnd {
                // The index is inside the auto repeat. Calculate the lines to use,
                // including the previous repetitions final names when we roll over
                // from one repetition to the next.
                let repeatIndex: u32 = (aIndex - self.mRepeatAutoStart) % maxRepeatLine;
                if repeatIndex == 0 {
                    // The index is at the start of a new repetition. The start of the
                    // first repetition is intentionally ignored above, so this will
                    // consider both the end of the previous repetition and the start
                    // the one that contains aIndex.
                    names.push(&self.mTrackAutoRepeatLineNames[maxRepeatLine as usize]);
                }
                names.push(&self.mTrackAutoRepeatLineNames[repeatIndex as usize]);
                return names;
            }
            if aIndex != self.mRepeatAutoStart && aIndex >= self.mRepeatAutoEnd {
                // Adjust the index to account for the line names of the repeat.
                // (unsigned arithmetic in C++: wraps)
                repeatAdjustedIndex = repeatAdjustedIndex.wrapping_sub(self.mRepeatEndDelta as u32);
                repeatAdjustedIndex = repeatAdjustedIndex.wrapping_add((self.mTrackAutoRepeatLineNames.len() as u32).wrapping_sub(2));
            }
        }
        MOZ_ASSERT!((repeatAdjustedIndex as usize) < self.mExpandedLineNames.len(), "Incorrect repeatedAdjustedIndex");
        MOZ_ASSERT!(names.is_empty());
        // The index is not inside the repeat tracks, or no repeat tracks exist.
        let nameLists = &self.mExpandedLineNames[repeatAdjustedIndex as usize];
        for nameList in nameLists {
            names.push(nameList);
        }
        names
    }

    // Translate a subgrid line (1-based) to a parent line (1-based).
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::TranslateToParentMap
    fn TranslateToParentMap(&self, aLine: u32) -> u32 {
        let range = self.mRange.unwrap();
        if self.mIsSameDirection {
            return aLine + range.mStart;
        }
        MOZ_ASSERT!(range.mEnd + 1 >= aLine);
        range.mEnd - (aLine - 1) + 1
    }

    /**
     * Return the 1-based line that match aName in 'grid-template-areas'
     * on the side aSide.  Clamp the result to aMin..aMax but require
     * that some part of the area is inside for it to match.
     * Return zero if there is no match.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::FindNamedArea(nsAtom*, LogicalSide, int32_t, int32_t)
    fn FindNamedArea__nsAtom_LogicalSide_int32_t_int32_t(&self, aName: &str, aSide: LogicalSide, aMin: i32, aMax: i32) -> u32 {
        if let Some(area) = self.FindNamedArea__nsAtom(aName) {
            let start: i32 = if IsBlock(aSide) { area.rows.start as i32 } else { area.columns.start as i32 };
            let end: i32 = if IsBlock(aSide) { area.rows.end as i32 } else { area.columns.end as i32 };
            if IsStart(aSide) {
                if start >= aMin {
                    if start <= aMax {
                        return start as u32;
                    }
                } else if end >= aMin {
                    return aMin as u32;
                }
            } else {
                if end <= aMax {
                    if end >= aMin {
                        return end as u32;
                    }
                } else if start <= aMax {
                    return aMax as u32;
                }
            }
        }
        0 // no match
    }

    /**
     * A convenience method to lookup a name in 'grid-template-areas'.
     * @return null if not found
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::LineNameMap::FindNamedArea(nsAtom*)
    fn FindNamedArea__nsAtom(&self, aName: &str) -> Option<&StyleNamedArea> {
        if self.mStylePosition.mGridTemplateAreas().IsNone() {
            return None;
        }
        let areas = self.mStylePosition.mGridTemplateAreas().AsAreas();
        for area in areas.areas.iter() {
            if &*area.name == aName {
                return Some(area);
            }
        }
        None
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp CachedIntrinsicSizes (forward declaration; defined in part 4)

// Some data we collect for aligning baseline-aligned items.
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::ItemBaselineData
pub struct nsGridContainerFrame_Tracks_ItemBaselineData {
    pub mBaselineTrack: u32,
    pub mBaseline: nscoord,
    pub mSize: nscoord,
    // ADAPTER: C++ `GridItemInfo* mGridItem`; an index into the aGridItems array the caller passes.
    pub mGridItem: usize,
}
pub type ItemBaselineData = nsGridContainerFrame_Tracks_ItemBaselineData;
pub type Tracks_ItemBaselineData = nsGridContainerFrame_Tracks_ItemBaselineData;

impl nsGridContainerFrame_Tracks_ItemBaselineData {
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::ItemBaselineData::IsBaselineTrackLessThan
    pub fn IsBaselineTrackLessThan(a: &ItemBaselineData, b: &ItemBaselineData) -> bool {
        a.mBaselineTrack < b.mBaselineTrack
    }
}

/**
 * A masonry axis has four baseline alignment sets and each set can have
 * a first- and last-baseline alignment group, for a total of eight possible
 * baseline alignment groups, as follows:
 *   set 1: the first item in each `start` or `stretch` grid track
 *   set 2: the last item in each `start` grid track
 *   set 3: the last item in each `end` or `stretch` grid track
 *   set 4: the first item in each `end` grid track
 * (`start`/`end`/`stretch` refers to the relevant `align/justify-tracks`
 * value of the (grid-axis) start track for the item) Baseline-alignment for
 * set 1 and 2 always adjusts the item's padding or margin on the start side,
 * and set 3 and 4 on the end side, for both first- and last-baseline groups
 * in the set. (This is similar to regular grid which always adjusts
 * first-baseline groups on the start side and last-baseline groups on the
 * end-side.  The crux is that those groups are always aligned to the track's
 * start/end side respectively.)
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::BaselineAlignmentSet::ItemSet
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsGridContainerFrame_Tracks_BaselineAlignmentSet_ItemSet {
    FirstItems,
    LastItems,
}
pub type BaselineAlignmentSet_ItemSet = nsGridContainerFrame_Tracks_BaselineAlignmentSet_ItemSet;

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::BaselineAlignmentSet::TrackAlignmentSet
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsGridContainerFrame_Tracks_BaselineAlignmentSet_TrackAlignmentSet {
    StartStretch,
    EndStretch,
}
pub type BaselineAlignmentSet_TrackAlignmentSet = nsGridContainerFrame_Tracks_BaselineAlignmentSet_TrackAlignmentSet;

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::BaselineAlignmentSet
#[derive(Clone, Copy, Debug)]
pub struct nsGridContainerFrame_Tracks_BaselineAlignmentSet {
    pub mItemSet: BaselineAlignmentSet_ItemSet,
    pub mTrackAlignmentSet: BaselineAlignmentSet_TrackAlignmentSet,
}
pub type BaselineAlignmentSet = nsGridContainerFrame_Tracks_BaselineAlignmentSet;
pub type Tracks_BaselineAlignmentSet = nsGridContainerFrame_Tracks_BaselineAlignmentSet;

impl Default for nsGridContainerFrame_Tracks_BaselineAlignmentSet {
    // (the C++ default member initializers: FirstItems, StartStretch)
    fn default() -> Self {
        BaselineAlignmentSet {
            mItemSet: BaselineAlignmentSet_ItemSet::FirstItems,
            mTrackAlignmentSet: BaselineAlignmentSet_TrackAlignmentSet::StartStretch,
        }
    }
}

impl nsGridContainerFrame_Tracks_BaselineAlignmentSet {
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::BaselineAlignmentSet::MatchTrackAlignment
    pub fn MatchTrackAlignment(&self, aTrackAlignment: StyleAlignFlags) -> bool {
        if self.mTrackAlignmentSet == BaselineAlignmentSet_TrackAlignmentSet::StartStretch {
            return aTrackAlignment == StyleAlignFlags::START
                || (aTrackAlignment == StyleAlignFlags::STRETCH && self.mItemSet == BaselineAlignmentSet_ItemSet::FirstItems);
        }
        aTrackAlignment == StyleAlignFlags::END || (aTrackAlignment == StyleAlignFlags::STRETCH && self.mItemSet == BaselineAlignmentSet_ItemSet::LastItems)
    }
}

// ADAPTER: C++ `EnumeratedArray<GridIntrinsicSizeType, nscoord>` (three entries, indexed by the enum).
#[derive(Clone, Copy, Debug, Default)]
pub struct Tracks_SpanningItemData_Sizes(pub [nscoord; 3]);

impl std::ops::Index<GridIntrinsicSizeType> for Tracks_SpanningItemData_Sizes {
    type Output = nscoord;
    fn index(&self, aType: GridIntrinsicSizeType) -> &nscoord {
        &self.0[aType as usize]
    }
}
impl std::ops::IndexMut<GridIntrinsicSizeType> for Tracks_SpanningItemData_Sizes {
    fn index_mut(&mut self, aType: GridIntrinsicSizeType) -> &mut nscoord {
        &mut self.0[aType as usize]
    }
}

// Some data we collect on each item that spans more than one track for step 3
// and 4 of the Track Sizing Algorithm in ResolveIntrinsicSize below.
// https://drafts.csswg.org/css-grid-2/#algo-spanning-items
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::SpanningItemData
#[derive(Clone)]
pub struct nsGridContainerFrame_Tracks_SpanningItemData {
    pub mSpan: u32,
    pub mState: TrackSize_StateBits,
    pub mLineRange: LineRange,
    pub mSizes: Tracks_SpanningItemData_Sizes,
    pub mFrame: nsIFrame,
}
pub type SpanningItemData = nsGridContainerFrame_Tracks_SpanningItemData;
pub type Tracks_SpanningItemData = nsGridContainerFrame_Tracks_SpanningItemData;

impl nsGridContainerFrame_Tracks_SpanningItemData {
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::SpanningItemData::IsSpanLessThan
    pub fn IsSpanLessThan(a: &SpanningItemData, b: &SpanningItemData) -> bool {
        a.mSpan < b.mSpan
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::SpanningItemData::SizeContributionForPhase
    pub fn SizeContributionForPhase(&self, aPhase: TrackSizingPhase) -> nscoord {
        self.mSizes[SizeTypeForPhase(aPhase)]
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::SpanningItemData::Dump
    // NOT PORTED: debug dump (printf of the members).
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::FitContentClamper
// ADAPTER: `std::function<bool(uint32_t aTrack, nscoord aMinSize, nscoord* aSize)>`; `const FitContentClamper&` is
// `&FitContentClamper` and a null function (the C++ default argument) is `Option<&FitContentClamper>` = None.
pub type nsGridContainerFrame_Tracks_FitContentClamper = dyn Fn(u32, nscoord, &mut nscoord) -> bool;
pub type Tracks_FitContentClamper = nsGridContainerFrame_Tracks_FitContentClamper;
pub type FitContentClamper = nsGridContainerFrame_Tracks_FitContentClamper;

/**
 * State for the tracks in one dimension.
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks
#[derive(Clone)]
pub struct nsGridContainerFrame_Tracks {
    pub mSizes: TrackPlan,
    pub mContentBoxSize: nscoord,
    pub mGridGap: nscoord,

    // Stores per-track baselines for grid baseline calculations.
    pub mBaselines: Vec<PerBaseline<Option<nscoord>>>,

    // The union of the track min/max-sizing state bits in this axis.
    pub mStateUnion: TrackSize_StateBits,
    pub mAxis: LogicalAxis,
    // Used for aligning a baseline-aligned subtree of items.  The only possible
    // values are StyleAlignFlags::{START,END,CENTER,AUTO}.  AUTO means there are
    // no baseline-aligned items in any track in that axis.
    // There is one alignment value for each BaselineSharingGroup.
    pub mBaselineSubtreeAlign: PerBaseline<StyleAlignFlags>,
    // True if track positions and sizes are final in this axis.
    pub mCanResolveLineRangeSize: bool,
    // True if this axis has masonry layout.
    pub mIsMasonry: bool,
}
pub type Tracks = nsGridContainerFrame_Tracks;

impl nsGridContainerFrame_Tracks {
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::Tracks
    pub fn new(aAxis: LogicalAxis) -> Tracks {
        Tracks {
            mSizes: TrackPlan::default(),
            mContentBoxSize: NS_UNCONSTRAINEDSIZE,
            mGridGap: NS_UNCONSTRAINEDSIZE,
            mBaselines: Vec::new(),
            mStateUnion: nsGridContainerFrame_TrackSize_StateBits(0),
            mAxis: aAxis,
            // mBaselineSubtreeAlign[BaselineSharingGroup::First] = StyleAlignFlags::AUTO;
            // mBaselineSubtreeAlign[BaselineSharingGroup::Last] = StyleAlignFlags::AUTO;
            mBaselineSubtreeAlign: PerBaseline([StyleAlignFlags::AUTO, StyleAlignFlags::AUTO]),
            mCanResolveLineRangeSize: false,
            mIsMasonry: false,
        }
    }

    // void Initialize(const TrackSizingFunctions& aFunctions, const NonNegativeLengthPercentageOrNormal& aGridGap,
    //                 uint32_t aNumTracks, nscoord aContentBoxSize);
    // (defined in part 4: Tracks::Initialize)

    // TrackSize::StateBits StateBitsForRange(const LineRange& aRange) const;
    // (defined in part 4: Tracks::StateBitsForRange)

    // void CalculateItemBaselines(nsTArray<ItemBaselineData>& aBaselineItems, BaselineSharingGroup aBaselineGroup);
    // void InitializeItemBaselines(GridReflowInput& aGridRI, nsTArray<GridItemInfo>& aGridItems);
    // void InitializeItemBaselinesInMasonryAxis(...);
    // void AlignBaselineSubtree(const GridItemInfo& aGridItem) const;
    // (defined in part 4)

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::SelectorForPhase
    pub fn SelectorForPhase(aPhase: TrackSizingPhase, aConstraint: SizingConstraint) -> TrackSize_StateBits {
        match aPhase {
            TrackSizingPhase::IntrinsicMinimums => TrackSize_StateBits::eIntrinsicMinSizing,
            TrackSizingPhase::ContentBasedMinimums => {
                if aConstraint == SizingConstraint::MinContent {
                    TrackSize_StateBits::eIntrinsicMinSizing
                } else {
                    TrackSize_StateBits::eMinOrMaxContentMinSizing
                }
            }
            TrackSizingPhase::MaxContentMinimums => {
                if aConstraint == SizingConstraint::MaxContent {
                    TrackSize_StateBits::eMaxContentMinSizing | TrackSize_StateBits::eAutoMinSizing
                } else {
                    TrackSize_StateBits::eMaxContentMinSizing
                }
            }
            TrackSizingPhase::IntrinsicMaximums => TrackSize_StateBits::eIntrinsicMaxSizing,
            TrackSizingPhase::MaxContentMaximums => TrackSize_StateBits::eAutoOrMaxContentMaxSizing,
        }
    }

    // bool GrowSizeForSpanningItems(...);  (defined in part 4)

    // Helper method for calculating CachedIntrinsicSizes.mMinSizeClamp.
    //
    // The caller should set ItemState::eClampMarginBoxMinSize on the
    // corresponding grid item if this returns something.
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::ComputeMinSizeClamp(TrackSizingFunctions, nscoord, LineRange)
    pub fn ComputeMinSizeClamp__TrackSizingFunctions_nscoord_LineRange(&self, aFunctions: &TrackSizingFunctions, aPercentageBasis: nscoord, aLineRange: &LineRange) -> Option<nscoord> {
        self.ComputeMinSizeClamp__TrackSizingFunctions_nscoord_LineRange_StateBits(aFunctions, aPercentageBasis, aLineRange, self.StateBitsForRange(aLineRange))
    }

    // More efficient version of ComputeMinSizeClamp if the caller has already
    // computed the state bits for this line range.
    // Maybe<nscoord> ComputeMinSizeClamp(const TrackSizingFunctions& aFunctions, nscoord aPercentageBasis,
    //                                    const LineRange& aLineRange, const TrackSize::StateBits aState) const;
    // (defined in part 4: ComputeMinSizeClamp__TrackSizingFunctions_nscoord_LineRange_StateBits)

    // void ResolveIntrinsicSize(...); void ResolveIntrinsicSizeForNonSpanningItems(...);  (defined in part 4)

    // Helper method that returns the track size to use in §12.5.1.2
    // https://drafts.csswg.org/css-grid-2/#extra-space
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::StartSizeInDistribution
    pub fn StartSizeInDistribution(aPhase: TrackSizingPhase, aSize: &TrackSize) -> nscoord {
        match aPhase {
            TrackSizingPhase::IntrinsicMinimums | TrackSizingPhase::ContentBasedMinimums | TrackSizingPhase::MaxContentMinimums => aSize.mBase,
            TrackSizingPhase::IntrinsicMaximums | TrackSizingPhase::MaxContentMaximums => {
                if aSize.mLimit == NS_UNCONSTRAINEDSIZE {
                    return aSize.mBase;
                }
                aSize.mLimit
            }
        }
    }

    /**
     * Collect the tracks which are growable (matching the sizing step/phase
     * and sizing constraint) into aGrowableTracks, and return the amount of
     * space that can be used to grow those tracks. This method implements
     * CSS Grid 2 §12.5.1.2.
     * https://drafts.csswg.org/css-grid-2/#extra-space
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::CollectGrowable
    pub fn CollectGrowable(&self, aStep: TrackSizingStep, aPhase: TrackSizingPhase, aAvailableSpace: nscoord, aRange: &LineRange, aConstraint: SizingConstraint, aGrowableTracks: &mut Vec<u32>) -> nscoord {
        MOZ_ASSERT!(aAvailableSpace > 0, "why call me?");
        let mut space: nscoord = aAvailableSpace - self.mGridGap * (aRange.Extent() - 1) as nscoord;
        let selector: TrackSize_StateBits = Self::SelectorForPhase(aPhase, aConstraint);
        for i in aRange.Range() {
            let sz: &TrackSize = &self.mSizes[i as usize];
            space -= Self::StartSizeInDistribution(aPhase, sz);
            if space <= 0 {
                return 0;
            }
            // Only flex tracks can be modified during step 4.
            if aStep == TrackSizingStep::Flex && !(sz.mState & TrackSize_StateBits::eFlexMaxSizing).to_bool() {
                continue;
            }
            if (sz.mState & selector).to_bool() {
                aGrowableTracks.push(i);
            }
        }
        if aGrowableTracks.is_empty() { 0 } else { space }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::CopyPlanToSize
    pub fn CopyPlanToSize(&mut self, aPhase: TrackSizingPhase, aTrackPlan: &TrackPlan, aNeedInfinitelyGrowableFlag: bool) {
        MOZ_ASSERT!(aTrackPlan.Length() == self.mSizes.Length());
        let len = std::cmp::min(aTrackPlan.Length(), self.mSizes.Length());
        for k in 0..len {
            let plan = &aTrackPlan[k];
            let sz = &mut self.mSizes[k];
            MOZ_ASSERT!(plan.mBase >= 0);
            match aPhase {
                TrackSizingPhase::IntrinsicMinimums | TrackSizingPhase::ContentBasedMinimums | TrackSizingPhase::MaxContentMinimums => {
                    sz.mBase = plan.mBase;
                }
                TrackSizingPhase::IntrinsicMaximums => {
                    if (plan.mState & TrackSize_StateBits::eModified).to_bool() {
                        if sz.mLimit == NS_UNCONSTRAINEDSIZE && aNeedInfinitelyGrowableFlag {
                            sz.mState |= TrackSize_StateBits::eInfinitelyGrowable;
                        }
                        sz.mLimit = plan.mBase;
                    }
                }
                TrackSizingPhase::MaxContentMaximums => {
                    if (plan.mState & TrackSize_StateBits::eModified).to_bool() {
                        sz.mLimit = plan.mBase;
                    }
                    sz.mState &= !TrackSize_StateBits::eInfinitelyGrowable;
                }
            }
        }
    }

    /**
     * Distribute aAvailableSpace to the planned base size for aGrowableTracks
     * up to their limits, then distribute the remaining space beyond the limits.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::DistributeToTrackSizes
    pub fn DistributeToTrackSizes(
        &self,
        aStep: TrackSizingStep,
        aPhase: TrackSizingPhase,
        aAvailableSpace: nscoord,
        aTrackPlan: &mut TrackPlan,
        aItemPlan: &mut ItemPlan,
        aGrowableTracks: &Vec<u32>,
        aConstraint: SizingConstraint,
        aFunctions: &TrackSizingFunctions,
        aFitContentClamper: Option<&Tracks_FitContentClamper>,
    ) {
        aItemPlan.Initialize(aPhase, aGrowableTracks, self);
        let mut space: nscoord = aAvailableSpace;
        if aStep == TrackSizingStep::Flex {
            space = aTrackPlan.DistributeToFlexTrackSizes(space, aGrowableTracks, aFunctions, self);
        } else {
            space = aItemPlan.GrowTracksToLimit(space, aGrowableTracks, aFitContentClamper);
        }

        if space > 0 {
            let numGrowable: u32 = aItemPlan.MarkExcludedTracks__TrackSizingPhase_nsTArray_SizingConstraint(aPhase, aGrowableTracks, aConstraint);
            aItemPlan.GrowSelectedTracksUnlimited(space, aGrowableTracks, numGrowable, aFitContentClamper);
        }

        for &track in aGrowableTracks.iter() {
            let itemIncurredSize: nscoord = aItemPlan[track as usize].mBase;
            let plannedSize: &mut nscoord = &mut aTrackPlan[track as usize].mBase;
            if *plannedSize < itemIncurredSize {
                *plannedSize = itemIncurredSize;
            }
        }
    }

    /**
     * Distribute aAvailableSize to the tracks.  This implements 12.6 at:
     * https://drafts.csswg.org/css-grid-2/#algo-grow-tracks
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::DistributeFreeSpace
    pub fn DistributeFreeSpace(&mut self, aAvailableSize: nscoord) {
        let numTracks: u32 = self.mSizes.Length() as u32;
        if numTracks == 0 || aAvailableSize <= 0 {
            return;
        }
        if aAvailableSize == NS_UNCONSTRAINEDSIZE {
            for i in 0..self.mSizes.Length() {
                let sz = &mut self.mSizes[i];
                sz.mBase = sz.mLimit;
            }
        } else {
            // Compute free space and count growable tracks.
            let mut space: nscoord = aAvailableSize;
            let mut numGrowable: u32 = numTracks;
            for i in 0..self.mSizes.Length() {
                let sz = &self.mSizes[i];
                space -= sz.mBase;
                MOZ_ASSERT!(sz.mBase <= sz.mLimit);
                if sz.mBase == sz.mLimit {
                    numGrowable -= 1;
                }
            }
            // Distribute the free space evenly to the growable tracks. If not exactly
            // divisable the remainder is added to the leading tracks.
            while space > 0 && numGrowable != 0 {
                let spacePerTrack: nscoord = std::cmp::max(space / numGrowable as nscoord, 1);
                for i in 0..self.mSizes.Length() {
                    let sz = &mut self.mSizes[i];
                    if sz.mBase == sz.mLimit {
                        continue;
                    }
                    let newBase: nscoord = sz.mBase + spacePerTrack;
                    if newBase >= sz.mLimit {
                        space -= sz.mLimit - sz.mBase;
                        sz.mBase = sz.mLimit;
                        numGrowable -= 1;
                    } else {
                        space -= spacePerTrack;
                        sz.mBase = newBase;
                    }
                    if space <= 0 {
                        break;
                    }
                }
            }
        }
    }

    // float FindFrUnitSize(const LineRange& aRange, const nsTArray<uint32_t>& aFlexTracks,
    //                      const TrackSizingFunctions& aFunctions, nscoord aSpaceToFill) const;
    // float FindUsedFlexFraction(GridReflowInput& aGridRI, const nsTArray<GridItemInfo>& aGridItems,
    //                            const nsTArray<uint32_t>& aFlexTracks, const TrackSizingFunctions& aFunctions,
    //                            nscoord aAvailableSize) const;
    // void StretchFlexibleTracks(...); void CalculateSizes(...); void AlignJustifyContent(...);
    // nscoord TotalTrackSizeWithoutAlignment(const nsGridContainerFrame* aGridContainerFrame) const;
    // (defined in part 4)

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::GridLineEdge
    pub fn GridLineEdge(&self, aLine: u32, aSide: GridLineSide) -> nscoord {
        if self.mSizes.IsEmpty() {
            // https://drafts.csswg.org/css-grid-2/#grid-definition
            // "... the explicit grid still contains one grid line in each axis."
            MOZ_ASSERT!(aLine == 0, "We should only resolve line 1 in an empty grid");
            return 0;
        }
        MOZ_ASSERT!(aLine as usize <= self.mSizes.Length(), "mSizes is too small");
        if aSide == GridLineSide::BeforeGridGap {
            if aLine == 0 {
                return 0;
            }
            let sz: &TrackSize = &self.mSizes[(aLine - 1) as usize];
            return sz.mPosition + sz.mBase;
        }
        if aLine as usize == self.mSizes.Length() {
            return self.mContentBoxSize;
        }
        self.mSizes[aLine as usize].mPosition
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::SumOfGridTracksAndGaps
    pub fn SumOfGridTracksAndGaps(&self) -> nscoord {
        self.SumOfGridTracks() + self.SumOfGridGaps()
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::SumOfGridTracks
    pub fn SumOfGridTracks(&self) -> nscoord {
        let mut result: nscoord = 0;
        for i in 0..self.mSizes.Length() {
            result += self.mSizes[i].mBase;
        }
        result
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::SumOfGridGaps
    pub fn SumOfGridGaps(&self) -> nscoord {
        let len = self.mSizes.Length();
        if len > 1 { (len - 1) as nscoord * self.mGridGap } else { 0 }
    }

    /**
     * Break before aRow, i.e. set the eBreakBefore flag on aRow and set the grid
     * gap before aRow to zero (and shift all rows after it by the removed gap).
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::BreakBeforeRow
    pub fn BreakBeforeRow(&mut self, aRow: u32) {
        MOZ_ASSERT!(self.mAxis == LogicalAxis::Block, "Should only be fragmenting in the block axis (between rows)");
        let mut prevRowEndPos: nscoord = 0;
        if aRow != 0 {
            let prevSz = &self.mSizes[(aRow - 1) as usize];
            prevRowEndPos = prevSz.mPosition + prevSz.mBase;
        }
        let sz = &mut self.mSizes[aRow as usize];
        let gap: nscoord = sz.mPosition - prevRowEndPos;
        sz.mState |= TrackSize_StateBits::eBreakBefore;
        if gap != 0 {
            let len = self.mSizes.Length() as u32;
            for i in aRow..len {
                self.mSizes[i as usize].mPosition -= gap;
            }
        }
    }

    /**
     * Set the size of aRow to aSize and adjust the position of all rows after it.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::ResizeRow
    pub fn ResizeRow(&mut self, aRow: u32, aNewSize: nscoord) {
        MOZ_ASSERT!(self.mAxis == LogicalAxis::Block, "Should only be fragmenting in the block axis (between rows)");
        MOZ_ASSERT!(aNewSize >= 0);
        let sz = &mut self.mSizes[aRow as usize];
        let delta: nscoord = aNewSize - sz.mBase;
        NS_WARNING_ASSERTION!(delta != 0, "Useless call to ResizeRow");
        sz.mBase = aNewSize;
        let numRows = self.mSizes.Length() as u32;
        for r in (aRow + 1)..numRows {
            self.mSizes[r as usize].mPosition += delta;
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::ResolveSize
    pub fn ResolveSize(&self, aRange: &LineRange) -> nscoord {
        MOZ_ASSERT!(self.mCanResolveLineRangeSize);
        MOZ_ASSERT!(aRange.Extent() > 0, "grid items cover at least one track");
        aRange.ToLength(&self.mSizes)
    }

    // Returns the offset for the given track and baseline sharing group, or the
    // opposite group in the same track if unavailable.
    // Returns `Nothing()` if neither is set.
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::GetBaseline
    pub fn GetBaseline(&self, aTrack: u32, aBaselineSharingGroup: BaselineSharingGroup) -> Option<nscoord> {
        if aTrack as usize >= self.mBaselines.len() {
            return None;
        }

        let trackBaselines = &self.mBaselines[aTrack as usize];
        if let Some(b) = trackBaselines[aBaselineSharingGroup] {
            return Some(b);
        }
        if let Some(b) = trackBaselines[GetOppositeBaselineSharingGroup(aBaselineSharingGroup)] {
            // First and last baseline offsets are from the start or end edges
            // respectively and requesting the opposite baseline requires adjusting
            // the offset to the opposite side.
            return Some(self.mSizes[aTrack as usize].mBase - b);
        }

        None
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Tracks::Dump
// NOT PORTED: debug dump (nsGridContainerFrame.cpp lines 3098-3147 at GECKO_REV, #ifdef DEBUG printf code).

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::SharedGridData::RowData
#[derive(Clone, Copy, Debug, Default)]
pub struct nsGridContainerFrame_SharedGridData_RowData {
    pub mBase: nscoord, // the original track size
    pub mGap: nscoord,  // the original gap before a track
}
pub type SharedGridData_RowData = nsGridContainerFrame_SharedGridData_RowData;

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::SharedGridData
pub struct nsGridContainerFrame_SharedGridData {
    pub mCols: Tracks,
    pub mRows: Tracks,
    pub mOriginalRowData: Vec<SharedGridData_RowData>,
    pub mGridItems: Vec<GridItemInfo>,
    pub mAbsPosItems: Vec<GridItemInfo>,
    pub mGenerateComputedGridInfo: bool,
}
pub type SharedGridData = nsGridContainerFrame_SharedGridData;

impl nsGridContainerFrame_SharedGridData {
    // ADAPTER: removes the property (frees the leaked Box); the C++ property destructor does this.
    pub fn RemoveProp(aFrame: nsIFrame) {
        GridPropRemove::<SharedGridData>(aFrame, SharedGridData_Prop());
    }
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::SharedGridData::SharedGridData
    pub fn new() -> SharedGridData {
        SharedGridData {
            mCols: Tracks::new(LogicalAxis::Inline),
            mRows: Tracks::new(LogicalAxis::Block),
            mOriginalRowData: Vec::new(),
            mGridItems: Vec::new(),
            mAbsPosItems: Vec::new(),
            mGenerateComputedGridInfo: false,
        }
    }
}

/**
 * Only set on the first-in-flow.  Continuations will Initialize() their
 * GridReflowInput from it.
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::SharedGridData::Prop
// ADAPTER: NS_DECLARE_FRAME_PROPERTY_DELETABLE(Prop, SharedGridData); the value is a leaked Box pointer
// as a usize (like the flex members), id 7210 is this property's number. `pub` so the other parts can call it.
pub fn SharedGridData_Prop() -> FramePropertyDescriptor<usize> {
    FramePropertyDescriptor::new(7210)
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput
pub struct nsGridContainerFrame_GridReflowInput<'a> {
    pub mIter: CSSOrderAwareFrameIterator,
    pub mGridStyle: nsStylePosition,
    pub mCols: Tracks,
    pub mRows: Tracks,
    pub mColFunctions: TrackSizingFunctions<'static>,
    pub mRowFunctions: TrackSizingFunctions<'static>,
    /**
     * Info about each (normal flow) grid item.
     */
    pub mGridItems: Vec<GridItemInfo>,
    /**
     * Info about each grid-aligned abs.pos. child.
     */
    pub mAbsPosItems: Vec<GridItemInfo>,

    /**
     * @note mReflowInput may be null when using the 2nd ctor above. In this case
     * we'll construct a dummy parent reflow input if we need it to calculate
     * min/max-content contributions when sizing tracks.
     */
    pub mReflowInput: Option<&'a ReflowInput<'a>>,
    pub mRenderingContext: gfxContext,
    pub mFrame: nsGridContainerFrame,
    /** [weak] owned by mFrame's first-in-flow. */
    pub mSharedGridData: *mut SharedGridData,
    /** Computed border+padding with mSkipSides applied. */
    pub mBorderPadding: LogicalMargin,
    /**
     * BStart of this fragment in "grid space" (i.e. the concatenation of content
     * areas of all fragments).  Equal to mRows.mSizes[mStartRow].mPosition,
     * or, if this fragment starts after the last row, the ConsumedBSize().
     */
    pub mFragBStart: nscoord,
    /** The start row for this fragment. */
    pub mStartRow: u32,
    /**
     * The start row for the next fragment, if any.  If mNextFragmentStartRow ==
     * mStartRow then there are no rows in this fragment.
     */
    pub mNextFragmentStartRow: u32,
    /** Our tentative ApplySkipSides bits. */
    pub mSkipSides: LogicalSides,
    pub mWM: WritingMode,
    /** Initialized lazily, when we find the fragmentainer. */
    pub mInFragmentainer: bool,
    /** Set when the grid itself is having its intrinsic size measured. */
    pub mIsGridIntrinsicSizing: bool,
}
pub type GridReflowInput<'a> = nsGridContainerFrame_GridReflowInput<'a>;

impl<'a> nsGridContainerFrame_GridReflowInput<'a> {
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::GridReflowInput(nsGridContainerFrame*, const ReflowInput&)
    pub fn new__nsGridContainerFrame_ReflowInput(aFrame: nsGridContainerFrame, aRI: &'a ReflowInput<'a>) -> GridReflowInput<'a> {
        // (aRI.mRenderingContext does not exist in this crate: gfxContext carries nothing)
        Self::new__nsGridContainerFrame_gfxContext_ReflowInput_nsStylePosition_WritingMode(aFrame, gfxContext::default(), Some(aRI), aRI.mStylePosition.clone(), aRI.GetWritingMode())
    }
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::GridReflowInput(nsGridContainerFrame*, gfxContext&)
    pub fn new__nsGridContainerFrame_gfxContext(aFrame: nsGridContainerFrame, aRC: gfxContext) -> GridReflowInput<'a> {
        Self::new__nsGridContainerFrame_gfxContext_ReflowInput_nsStylePosition_WritingMode(aFrame, aRC, None, aFrame.StylePosition(), aFrame.GetWritingMode())
    }

    /**
     * Initialize our track sizes and grid item info using the shared
     * state from aGridContainerFrame first-in-flow.
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::InitializeForContinuation
    // NOT PORTED: fragmentation (continuations of a grid container; original: nsGridContainerFrame.cpp lines 3184-3302 at GECKO_REV).
    pub fn InitializeForContinuation(&mut self, aGridContainerFrame: nsGridContainerFrame, aConsumedBSize: nscoord) {
        unimplemented!("NOT PORTED: fragmentation (grid container continuations)")
    }

    // Recursive helper for CollectSubgridItemsForAxis().
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::CollectSubgridItemsForAxisHelper
    // NOT PORTED: subgrid (original: nsGridContainerFrame.cpp lines 3361-3407 at GECKO_REV).
    pub fn CollectSubgridItemsForAxisHelper(aAxis: LogicalAxis, aContainerWM: WritingMode, aRangeInAxis: &LineRange, aRangeInOppositeAxis: &LineRange, aItem: &GridItemInfo, aItems: &Vec<GridItemInfo>, aResult: &mut Vec<GridItemInfo>) {
        unimplemented!("NOT PORTED: subgrid (CollectSubgridItemsForAxisHelper)")
    }

    // Copy all descendant items from all our subgrid children that are subgridded
    // in aAxis recursively into aResult.  All item grid area's and state are
    // translated to our coordinates.
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::CollectSubgridItemsForAxis
    // NOT PORTED: subgrid. Original (nsGridContainerFrame.cpp lines 3410-3426):
    //   for (const auto& item : mGridItems) {
    //     if (item.IsSubgrid(aAxis)) {
    //       const auto oppositeAxis = GetOrthogonalAxis(aAxis);
    //       auto* subgrid = item.SubgridFrame()->GetProperty(Subgrid::Prop());
    //       CollectSubgridItemsForAxisHelper(aAxis, mWM, item.mArea.LineRangeForAxis(aAxis),
    //                                        item.mArea.LineRangeForAxis(oppositeAxis), item, subgrid->mGridItems, aResult);
    //     }
    //   }
    pub fn CollectSubgridItemsForAxis(&self, aAxis: LogicalAxis, aResult: &mut Vec<GridItemInfo>) {
        unimplemented!("NOT PORTED: subgrid (CollectSubgridItemsForAxis)")
    }

    /**
     * Recursive helper for CopyBaselineMetricsToSubgridItems().
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::CopyBaselineMetricsToSubgridItemsHelper
    // NOT PORTED: subgrid (original: nsGridContainerFrame.cpp lines 3444-3519 at GECKO_REV).
    pub fn CopyBaselineMetricsToSubgridItemsHelper(aAxis: LogicalAxis, aContainerWM: WritingMode, aSubgridFrame: nsIFrame, aContainerGridItems: &Vec<GridItemInfo>, aContainerGridItemsIdx: &mut usize) {
        unimplemented!("NOT PORTED: subgrid (CopyBaselineMetricsToSubgridItemsHelper)")
    }

    /**
     * This function here is responsible for propagating baseline-alignment
     * metrics for subgrid-items from mGridItems over to the "canonical"
     * GridItemInfo structs for those grid items (which live on the subgrid that
     * owns them). ... (see nsGridContainerFrame.cpp lines 3521-3534 at GECKO_REV)
     */
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::CopyBaselineMetricsToSubgridItems
    // NOT PORTED: subgrid. Original (nsGridContainerFrame.cpp lines 3535-3562):
    //   size_t subgridItemIdx = aOriginalLength;
    //   for (size_t i = 0; (i < aOriginalLength && subgridItemIdx < mGridItems.Length()); i++) {
    //     const auto& item = mGridItems[i];
    //     if (item.IsSubgrid(aAxis)) {
    //       CopyBaselineMetricsToSubgridItemsHelper(aAxis, mWM, item.SubgridFrame(), mGridItems, subgridItemIdx);
    //     }
    //   }
    pub fn CopyBaselineMetricsToSubgridItems(&mut self, aAxis: LogicalAxis, aOriginalLength: usize) {
        unimplemented!("NOT PORTED: subgrid (CopyBaselineMetricsToSubgridItems)")
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::TracksFor (non-const)
    // ADAPTER: the const overload is `TracksFor`, the non-const one `TracksFor_mut`.
    pub fn TracksFor_mut(&mut self, aAxis: LogicalAxis) -> &mut Tracks {
        if aAxis == LogicalAxis::Block { &mut self.mRows } else { &mut self.mCols }
    }
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::TracksFor (const)
    pub fn TracksFor(&self, aAxis: LogicalAxis) -> &Tracks {
        if aAxis == LogicalAxis::Block { &self.mRows } else { &self.mCols }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::GridReflowInput(nsGridContainerFrame*, gfxContext&, const ReflowInput*, const nsStylePosition*, const WritingMode&) (private)
    fn new__nsGridContainerFrame_gfxContext_ReflowInput_nsStylePosition_WritingMode(
        aFrame: nsGridContainerFrame,
        aRenderingContext: gfxContext,
        aReflowInput: Option<&'a ReflowInput<'a>>,
        aGridStyle: nsStylePosition,
        aWM: WritingMode,
    ) -> GridReflowInput<'a> {
        // ADAPTER: the functions borrow the template and the implicit track lists out of the computed values
        // that aGridStyle keeps alive (an Arc whose heap block does not move, and which this struct never
        // replaces), so the borrows are extended to 'static here instead of making the struct self-referential.
        let (colFunctions, rowFunctions) = unsafe {
            let style: &'static nsStylePosition = &*(&aGridStyle as *const nsStylePosition);
            (
                TrackSizingFunctions::new__GridTemplate_StyleImplicitGridTracks_bool(style.mGridTemplateColumns(), style.mGridAutoColumns(), aFrame.IsColSubgrid()),
                TrackSizingFunctions::new__GridTemplate_StyleImplicitGridTracks_bool(style.mGridTemplateRows(), style.mGridAutoRows(), aFrame.IsRowSubgrid()),
            )
        };
        let mut this = GridReflowInput {
            mIter: CSSOrderAwareFrameIterator::new(
                aFrame.0,
                FrameChildListID::Principal,
                CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders,
                CSSOrderAwareFrameIterator_OrderState::Unknown,
                CSSOrderAwareFrameIterator_OrderingProperty::Order,
            ),
            mGridStyle: aGridStyle,
            mCols: Tracks::new(LogicalAxis::Inline),
            mRows: Tracks::new(LogicalAxis::Block),
            mColFunctions: colFunctions,
            mRowFunctions: rowFunctions,
            mGridItems: Vec::new(),
            mAbsPosItems: Vec::new(),
            mReflowInput: aReflowInput,
            mRenderingContext: aRenderingContext,
            mFrame: aFrame,
            mSharedGridData: std::ptr::null_mut(),
            mBorderPadding: LogicalMargin::new__WritingMode(aWM),
            mFragBStart: 0,
            mStartRow: 0,
            mNextFragmentStartRow: 0,
            mSkipSides: LogicalSides::new__WritingMode(aFrame.GetWritingMode()),
            mWM: aWM,
            mInFragmentainer: false,
            mIsGridIntrinsicSizing: false,
        };
        MOZ_ASSERT!(aReflowInput.map_or(true, |ri| ri.mFrame == this.mFrame.0));
        if let Some(aReflowInput) = aReflowInput {
            this.mBorderPadding = aReflowInput.ComputedLogicalBorderPadding(this.mWM);
            this.mSkipSides = aFrame.PreReflowBlockLevelLogicalSkipSides();
            this.mBorderPadding.ApplySkipSides(this.mSkipSides);
        }
        this.mCols.mIsMasonry = aFrame.IsColMasonry();
        this.mRows.mIsMasonry = aFrame.IsRowMasonry();
        MOZ_ASSERT!(!(this.mCols.mIsMasonry && this.mRows.mIsMasonry), "can't have masonry layout in both axes");
        this
    }

    // void CalculateTrackSizesForAxis(LogicalAxis aAxis, const Grid& aGrid, nscoord aCBSize, SizingConstraint aConstraint);
    // void InvalidateTrackSizesForAxis(LogicalAxis aAxis);   (defined in part 3)
    // LogicalSize PercentageBasisFor(LogicalAxis aAxis, const GridItemInfo& aGridItem) const;
    // LogicalRect ContainingBlockFor(const GridArea& aArea) const;
    // LogicalRect ContainingBlockForAbsPos(const GridArea& aArea, const LogicalPoint& aGridOrigin, const LogicalRect& aGridCB) const;
    // void AlignJustifyContentInMasonryAxis(nscoord aMasonryBoxSize, nscoord aContentBoxSize);
    // void AlignJustifyTracksInMasonryAxis(const LogicalSize& aContentSize, const nsSize& aContainerSize);
    // (defined in parts 3 and 5)
}

// Part P3 of the translation of layout/generic/nsGridContainerFrame.cpp (GECKO_REV): .cpp lines 3651-5660
// (struct Grid, the used-track-size helpers, CalculateTrackSizesForAxis, AlignSelf / JustifySelf, the
// implicit named areas, line resolution, grid item placement). The parts are joined into one file at the end.
//
// Assumptions about the other parts (the design note's rules; see the report of this part):
//  - raw pointers stand for the C++ pointers to stack objects and frame-property values (`const Grid*
//    mParentGrid`, `const LineNameMap* mColNameMap`, `ImplicitNamedAreas* mAreas`), as CONVENTIONS.md says
//    for pointers that Gecko shares and replaces;
//  - `Tracks`, `TrackSizingFunctions`, `GridReflowInput` are used through `&`/`&mut` borrows. The C++ code
//    passes `*this` and a member of `*this` in the same call (`tracks.CalculateSizes(*this, mGridItems, ...)`):
//    Rust cannot borrow that, so those calls go through raw pointers (ADAPTER comments).

// ADAPTER: Gecko's NS_Atomize(const nsAString&). nsAtom is `&'static str` in this crate, so an atom is an
// interned string: the same text gives the same `&'static str`.
thread_local! {
    static ATOM_TABLE: std::cell::RefCell<HashMap<String, &'static str>> = std::cell::RefCell::new(HashMap::new());
}
pub fn NS_Atomize(aString: &str) -> nsAtom {
    ATOM_TABLE.with(|t| {
        let mut t = t.borrow_mut();
        if let Some(a) = t.get(aString) {
            return *a;
        }
        let a: &'static str = Box::leak(aString.to_string().into_boxed_str());
        t.insert(aString.to_string(), a);
        a
    })
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::CellMap::Cell
#[derive(Clone, Copy, Default)]
pub struct nsGridContainerFrame_Grid_CellMap_Cell {
    pub mIsOccupied: bool,
}
pub type CellMap_Cell = nsGridContainerFrame_Grid_CellMap_Cell;

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::CellMap
/**
 * A CellMap holds state for each cell in the grid.
 * It's row major.  It's sparse in the sense that it only has enough rows to
 * cover the last row that has a grid item.  Each row only has enough entries
 * to cover columns that are occupied *on that row*, i.e. it's not a full
 * matrix covering the entire implicit grid.  An absent Cell means that it's
 * unoccupied by any grid item.
 */
#[derive(Default)]
pub struct nsGridContainerFrame_Grid_CellMap {
    pub mCells: Vec<Vec<CellMap_Cell>>,
}
pub type CellMap = nsGridContainerFrame_Grid_CellMap;

impl nsGridContainerFrame_Grid_CellMap {
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::CellMap::Fill
    pub fn Fill(&mut self, aGridArea: &GridArea) {
        MOZ_ASSERT!(aGridArea.IsDefinite());
        MOZ_ASSERT!(aGridArea.mRows.mStart < aGridArea.mRows.mEnd);
        MOZ_ASSERT!(aGridArea.mCols.mStart < aGridArea.mCols.mEnd);
        let numRows = aGridArea.mRows.mEnd;
        let numCols = aGridArea.mCols.mEnd;
        // mCells.EnsureLengthAtLeast(numRows);
        if (self.mCells.len() as u32) < numRows {
            self.mCells.resize_with(numRows as usize, Default::default);
        }
        for i in aGridArea.mRows.mStart..numRows {
            let cellsInRow = &mut self.mCells[i as usize];
            // cellsInRow.EnsureLengthAtLeast(numCols);
            if (cellsInRow.len() as u32) < numCols {
                cellsInRow.resize_with(numCols as usize, Default::default);
            }
            for j in aGridArea.mCols.mStart..numCols {
                cellsInRow[j as usize].mIsOccupied = true;
            }
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::CellMap::IsEmptyCol
    pub fn IsEmptyCol(&self, aCol: u32) -> bool {
        for row in &self.mCells {
            if (aCol as usize) < row.len() && row[aCol as usize].mIsOccupied {
                return false;
            }
        }
        true
    }
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::CellMap::IsEmptyRow
    pub fn IsEmptyRow(&self, aRow: u32) -> bool {
        if aRow as usize >= self.mCells.len() {
            return true;
        }
        for cell in &self.mCells[aRow as usize] {
            if cell.mIsOccupied {
                return false;
            }
        }
        true
    }
    // NOT PORTED: CellMap::Dump (#ifdef DEBUG, printf).
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid
/**
 * The Grid implements grid item placement and the state of the grid -
 * the size of the explicit/implicit grid, which cells are occupied etc.
 */
pub struct nsGridContainerFrame_Grid {
    /**
     * State for each cell in the grid.
     */
    pub mCellMap: CellMap,
    /**
     * @see HasImplicitNamedArea.
     */
    // ADAPTER: a raw pointer to the HashMap that the frame property holds (null when there is none).
    pub mAreas: *mut ImplicitNamedAreas,
    /**
     * The last column grid line (1-based) in the explicit grid.
     * (i.e. the number of explicit columns + 1)
     */
    pub mExplicitGridColEnd: u32,
    /**
     * The last row grid line (1-based) in the explicit grid.
     * (i.e. the number of explicit rows + 1)
     */
    pub mExplicitGridRowEnd: u32,
    // Same for the implicit grid, except these become zero-based after
    // resolving definite lines.
    pub mGridColEnd: u32,
    pub mGridRowEnd: u32,

    /**
     * Offsets from the start of the implicit grid to the start of the translated
     * explicit grid.  They are zero if there are no implicit lines before 1,1.
     * e.g. "grid-column: span 3 / 1" makes mExplicitGridOffsetCol = 3 and the
     * corresponding GridArea::mCols will be 0 / 3 in the zero-based translated
     * grid.
     */
    pub mExplicitGridOffsetCol: u32,
    pub mExplicitGridOffsetRow: u32,

    /**
     * Our parent grid if any.
     */
    pub mParentGrid: *const Grid,

    /**
     * Our LineNameMaps.
     */
    // ADAPTER: LineNameMap borrows style data ('a); the pointer erases that, as the C++ pointer does.
    pub mColNameMap: *const LineNameMap<'static>,
    pub mRowNameMap: *const LineNameMap<'static>,
}
pub type Grid = nsGridContainerFrame_Grid;

// ADAPTER: Gecko's `typedef std::pair<int32_t, int32_t> LinePair` (a nested typedef of Grid).
pub type Grid_LinePair = (i32, i32);
pub type LinePair = Grid_LinePair;

impl nsGridContainerFrame_Grid {
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::Grid
    // (`explicit Grid(const Grid* aParentGrid = nullptr)`; the other members are uninitialised in C++, zero here)
    pub fn new(aParentGrid: *const Grid) -> Grid {
        Grid {
            mCellMap: CellMap::default(),
            mAreas: std::ptr::null_mut(),
            mExplicitGridColEnd: 0,
            mExplicitGridRowEnd: 0,
            mGridColEnd: 0,
            mGridRowEnd: 0,
            mExplicitGridOffsetCol: 0,
            mExplicitGridOffsetRow: 0,
            mParentGrid: aParentGrid,
            mColNameMap: std::ptr::null(),
            mRowNameMap: std::ptr::null(),
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::AutoIfOutside
    /**
     * Return aLine if it's inside the aMin..aMax range (inclusive),
     * otherwise return kAutoLine.
     */
    pub fn AutoIfOutside(aLine: i32, aMin: i32, aMax: i32) -> i32 {
        MOZ_ASSERT!(aMin <= aMax);
        if aLine < aMin || aLine > aMax {
            return kAutoLine as i32;
        }
        aLine
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::InflateGridFor
    /**
     * Inflate the implicit grid to include aArea.
     * @param aArea may be definite or auto
     */
    pub fn InflateGridFor(&mut self, aArea: &GridArea) {
        self.mGridColEnd = std::cmp::max(self.mGridColEnd, aArea.mCols.HypotheticalEnd());
        self.mGridRowEnd = std::cmp::max(self.mGridRowEnd, aArea.mRows.HypotheticalEnd());
        MOZ_ASSERT!(self.mGridColEnd <= kTranslatedMaxLine && self.mGridRowEnd <= kTranslatedMaxLine);
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::HasImplicitNamedArea
    pub fn HasImplicitNamedArea(&self, aName: nsAtom) -> bool {
        // mAreas && mAreas->has(aName)
        !self.mAreas.is_null() && unsafe { (*self.mAreas).contains_key(aName) }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::IsNameWithSuffix
    // Return true if aString ends in aSuffix and has at least one character
    // before the suffix. Assign aIndex to where the suffix starts.
    // (ADAPTER: the index is a byte index; the suffixes are ASCII, and the caller uses it to slice the same string.)
    pub fn IsNameWithSuffix(aString: nsAtom, aSuffix: &str, aIndex: &mut u32) -> bool {
        if aString.ends_with(aSuffix) {
            *aIndex = (aString.len() - aSuffix.len()) as u32;
            return *aIndex != 0;
        }
        false
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::IsNameWithEndSuffix
    pub fn IsNameWithEndSuffix(aString: nsAtom, aIndex: &mut u32) -> bool {
        Grid::IsNameWithSuffix(aString, "-end", aIndex)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::IsNameWithStartSuffix
    pub fn IsNameWithStartSuffix(aString: nsAtom, aIndex: &mut u32) -> bool {
        Grid::IsNameWithSuffix(aString, "-start", aIndex)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::ParentLineMapForAxis
    // Return the relevant parent LineNameMap for the given subgrid axis aAxis.
    pub fn ParentLineMapForAxis(&self, aIsOrthogonal: bool, aAxis: LogicalAxis) -> *const LineNameMap<'static> {
        if self.mParentGrid.is_null() {
            return std::ptr::null();
        }
        let isRows = aIsOrthogonal == (aAxis == LogicalAxis::Inline);
        unsafe {
            if isRows {
                (*self.mParentGrid).mRowNameMap
            } else {
                (*self.mParentGrid).mColNameMap
            }
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::SetLineMaps
    pub fn SetLineMaps(&mut self, aColNameMap: *const LineNameMap<'static>, aRowNameMap: *const LineNameMap<'static>) {
        self.mColNameMap = aColNameMap;
        self.mRowNameMap = aRowNameMap;
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp SubgridComputeMarginBorderPadding
/**
 * Compute margin+border+padding for aGridItem.mFrame (a subgrid) and store it
 * on its Subgrid property (and return that property).
 * aPercentageBasis is in the grid item's writing-mode.
 */
// NOT PORTED: subgrid. Original: nsGridContainerFrame.cpp lines 4011-4059 at GECKO_REV (it uses the Subgrid
// frame property, ScrollContainerFrame and nsFieldSetFrame, none of which exist here).
fn SubgridComputeMarginBorderPadding(aGridItem: &GridItemInfo, aPercentageBasis: &LogicalSize) -> ! {
    unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)")
}

// Gecko: layout/generic/nsGridContainerFrame.cpp CopyUsedTrackSizes
// NOT PORTED: subgrid. Original: nsGridContainerFrame.cpp lines 4061-4140 at GECKO_REV (copies the parent grid's
// used track sizes into a subgrid's TrackPlan).
fn CopyUsedTrackSizes(aResult: &mut TrackPlan, aUsedTrackSizesFrame: nsGridContainerFrame, aUsedTrackSizes: &UsedTrackSizes, aSubgridFrame: nsGridContainerFrame, aSubgrid: &Subgrid, aSubgridAxis: LogicalAxis) -> ! {
    unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)")
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::UsedTrackSizes::ResolveTrackSizesForAxis
impl nsGridContainerFrame_UsedTrackSizes {
    pub fn ResolveTrackSizesForAxis(&mut self, aFrame: nsGridContainerFrame, aAxis: LogicalAxis, aRC: gfxContext) {
        if self.mCanResolveLineRangeSize[aAxis] {
            return;
        }
        if !aFrame.IsSubgrid__() {
            // We can't resolve sizes in this axis at this point. aFrame is the top grid
            // container, which will store its final track sizes later once they're
            // resolved in this axis (in GridReflowInput::CalculateTrackSizesForAxis).
            // The single caller of this method only needs track sizes for
            // calculating a CB size and it will treat it as indefinite when
            // this happens.
            return;
        }
        // NOT PORTED: subgrid. The rest of the function runs only when aFrame is a subgrid:
        //   auto* parent = aFrame->ParentGridContainerForSubgrid();
        //   auto* parentSizes = parent->GetOrCreateDeletableProperty(UsedTrackSizes::Prop());
        //   ... CopyUsedTrackSizes / ResolveSubgridTrackSizesForAxis (nsGridContainerFrame.cpp lines 4160-4185).
        unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)")
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::UsedTrackSizes::ResolveSubgridTrackSizesForAxis
    // NOT PORTED: subgrid. Original:
    //   GridReflowInput gridRI(aFrame, aRC);
    //   gridRI.mGridItems = aSubgrid->mGridItems.Clone();
    //   Grid grid;
    //   grid.mGridColEnd = aSubgrid->mGridColEnd;
    //   grid.mGridRowEnd = aSubgrid->mGridRowEnd;
    //   gridRI.CalculateTrackSizesForAxis(aAxis, grid, aContentBoxSize, SizingConstraint::NoConstraint);
    //   const auto& tracks = gridRI.TracksFor(aAxis);
    //   mTrackPlans[aAxis].Assign(tracks.mSizes);
    //   mCanResolveLineRangeSize[aAxis] = tracks.mCanResolveLineRangeSize;
    pub fn ResolveSubgridTrackSizesForAxis(&mut self, aFrame: nsGridContainerFrame, aAxis: LogicalAxis, aSubgrid: &mut Subgrid, aRC: gfxContext, aContentBoxSize: nscoord) {
        unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)")
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::CalculateTrackSizesForAxis
impl nsGridContainerFrame_GridReflowInput<'_> {
    /**
     * Calculate our track sizes in the given axis.
     */
    pub fn CalculateTrackSizesForAxis(&mut self, aAxis: LogicalAxis, aGrid: &Grid, aContentBoxSize: nscoord, aConstraint: SizingConstraint) {
        // ADAPTER: C++ holds `tracks` (a member of *this) and passes *this and other members along with it
        // (Tracks::CalculateSizes(*this, mGridItems, sizingFunctions, ...)). Rust cannot borrow that, so the
        // members are reached through raw pointers, exactly the aliasing the C++ code has.
        let this: *mut GridReflowInput = self;
        let tracks: *mut Tracks = self.TracksFor_mut(aAxis);
        let sizingFunctions: *const TrackSizingFunctions = if aAxis == LogicalAxis::Inline { &self.mColFunctions } else { &self.mRowFunctions };
        let gapStyle = if aAxis == LogicalAxis::Inline { self.mGridStyle.mColumnGap() } else { self.mGridStyle.mRowGap() };
        unsafe {
            let tracks = &mut *tracks;
            let sizingFunctions = &*sizingFunctions;
            if tracks.mIsMasonry {
                // See comment on nsGridContainerFrame::MasonryLayout().
                tracks.Initialize(sizingFunctions, &gapStyle, 2, aContentBoxSize);
                tracks.mCanResolveLineRangeSize = true;
                return;
            }
            let gridEnd = if aAxis == LogicalAxis::Inline { aGrid.mGridColEnd } else { aGrid.mGridRowEnd };
            let fallbackTrackSizing: Option<TrackSizingFunctions> = None;

            let useParentGaps = false;
            let isSubgriddedAxis = (*this).mFrame.IsSubgrid__LogicalAxis(aAxis);
            if !isSubgriddedAxis {
                tracks.Initialize(sizingFunctions, &gapStyle, gridEnd, aContentBoxSize);
            } else {
                // NOT PORTED: subgrid. Original (nsGridContainerFrame.cpp lines 4228-4251 at GECKO_REV): resolve
                // the gap, copy the parent grid's used track sizes (CopyUsedTrackSizes) or fall back to
                // TrackSizingFunctions::ForSubgridFallback.
                unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)")
            }

            // We run the Track Sizing Algorithm in non-subgridded axes, and in some
            // cases in a subgridded axis when our parent track sizes aren't resolved yet.
            if !isSubgriddedAxis || fallbackTrackSizing.is_some() {
                let origGridItemCount = (*this).mGridItems.len();
                let hasSubgridItems = (*this).mFrame.HasSubgridItems__LogicalAxis(aAxis);
                if hasSubgridItems {
                    // NOT PORTED: subgrid. Original:
                    //   AutoTArray<GridItemInfo, 8> collectedItems;
                    //   CollectSubgridItemsForAxis(aAxis, collectedItems);
                    //   mGridItems.AppendElements(collectedItems);
                    unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)")
                }
                tracks.CalculateSizes(
                    &*this,
                    &mut (*this).mGridItems,
                    match &fallbackTrackSizing {
                        Some(f) => f,
                        None => sizingFunctions,
                    },
                    aContentBoxSize,
                    if aAxis == LogicalAxis::Inline { GridArea_mCols } else { GridArea_mRows },
                    aConstraint,
                );

                if hasSubgridItems {
                    // NOT PORTED: subgrid. Original:
                    //   if (hasSubgridItems && StaticPrefs::layout_css_grid_subgrid_baselines_enabled()) {
                    //     CopyBaselineMetricsToSubgridItems(aAxis, origGridItemCount);
                    //   }
                }
                (*this).mGridItems.truncate(origGridItemCount);
            }
            if isSubgriddedAxis {
                // XXXdholbert This is a bit hacky, but this is something that
                // tracks.CalculateSizes does internally (unconditionally, if there are
                // baseline-aligned items), and it seems like subgrids need to do it too,
                // or else they hit the "unexpected baseline subtree alignment"
                // fatal-assert when aligning their children with the baseline-alignment
                // information that they received from the outer grid.
                // (This might be entirely unnecessary? Aside from the default ::AUTO
                // value, it looks like the ::First entry is always set to ::START and
                // the ::Last entry is always set to ::END...)
                tracks.mBaselineSubtreeAlign[BaselineSharingGroup::First] = StyleAlignFlags::START;
                tracks.mBaselineSubtreeAlign[BaselineSharingGroup::Last] = StyleAlignFlags::END;
            }

            if aContentBoxSize != NS_UNCONSTRAINEDSIZE {
                let alignment = (*this).mGridStyle.UsedContentAlignment(tracks.mAxis);
                tracks.AlignJustifyContent(&(*this).mGridStyle, alignment, (*this).mWM, aContentBoxSize, isSubgriddedAxis);
            } else if !useParentGaps {
                let gridGap = tracks.mGridGap;
                let mut pos: nscoord = 0;
                for sz in tracks.mSizes.iter_mut() {
                    sz.mPosition = pos;
                    pos += sz.mBase + gridGap;
                }
            }

            if aConstraint == SizingConstraint::NoConstraint && ((*this).mFrame.HasSubgridItems__() || (*this).mFrame.IsSubgrid__()) {
                (*this).mFrame.StoreUsedTrackSizes(aAxis, &tracks.mSizes);
            }

            // positions and sizes are now final
            tracks.mCanResolveLineRangeSize = true;
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridReflowInput::InvalidateTrackSizesForAxis
    /**
     * Invalidate track sizes for the given axis by clearing track sizing bits for
     * all grid items and mark the track sizes and positions as needing recompute.
     *
     * This helper must be called before invoking CalculateTrackSizesForAxis()
     * again in aAxis; otherwise, assertions will fire.
     */
    pub fn InvalidateTrackSizesForAxis(&mut self, aAxis: LogicalAxis) {
        for item in self.mGridItems.iter_mut() {
            item.ResetTrackSizingBits(aAxis);
        }
        self.TracksFor_mut(aAxis).mCanResolveLineRangeSize = false;
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp AlignJustifySelf
// Align an item's margin box in its aAxis inside aCBSize.
fn AlignJustifySelf(aAlignment: StyleAlignFlags, aAxis: LogicalAxis, aFlags: AlignJustifyFlags, aBaselineAdjust: nscoord, aCBSize: nscoord, aRI: &ReflowInput, aChildSize: &LogicalSize, aPos: &mut LogicalPoint) {
    MOZ_ASSERT!(aAlignment != StyleAlignFlags::AUTO, "unexpected 'auto' computed value for normal flow grid item");

    // NOTE: this is the resulting frame offset (border box).
    let offset = CSSAlignUtils::AlignJustifySelf(aAlignment, aAxis, aFlags, aBaselineAdjust, aCBSize, aRI, aChildSize);

    // Set the position (aPos) for the requested alignment.
    if offset != 0 {
        let wm = aRI.GetWritingMode();
        let pos: &mut nscoord = if aAxis == LogicalAxis::Block { aPos.B_mut(wm) } else { aPos.I_mut(wm) };
        if aFlags.contains(AlignJustifyFlag::SameSide) {
            *pos += offset;
        } else {
            *pos += -offset;
        }
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp AlignSelf
pub fn AlignSelf(aGridItem: &GridItemInfo, aAlignSelf: StyleAlignFlags, aCBSize: nscoord, aCBWM: WritingMode, aRI: &ReflowInput, aSize: &LogicalSize, aFlags: AlignJustifyFlags, aPos: &mut LogicalPoint) {
    let mut aAlignSelf = aAlignSelf;
    let mut flags = aFlags;
    if aAlignSelf.contains(StyleAlignFlags::SAFE) {
        flags += AlignJustifyFlag::OverflowSafe;
    }
    aAlignSelf &= !StyleAlignFlags::FLAG_BITS;

    let childWM = aRI.GetWritingMode();
    if aCBWM.ParallelAxisStartsOnSameSide(LogicalAxis::Block, &childWM) {
        flags += AlignJustifyFlag::SameSide;
    }

    if (aGridItem.mState[LogicalAxis::Block].get() & ItemState::eLastBaselineSharingGroup).to_bool() {
        flags += AlignJustifyFlag::LastBaselineSharingGroup;
    }

    // Grid's 'align-self' axis is never parallel to the container's inline axis.
    if aAlignSelf == StyleAlignFlags::LEFT || aAlignSelf == StyleAlignFlags::RIGHT {
        aAlignSelf = StyleAlignFlags::START;
    }
    if aAlignSelf == StyleAlignFlags::NORMAL {
        aAlignSelf = StyleAlignFlags::STRETCH;
    }

    let mut baselineAdjust: nscoord = 0;
    if aAlignSelf == StyleAlignFlags::BASELINE || aAlignSelf == StyleAlignFlags::LAST_BASELINE {
        aAlignSelf = aGridItem.GetSelfBaseline(aAlignSelf, LogicalAxis::Block, &mut baselineAdjust);
    }

    let bAxisInChildWM = aCBWM.ConvertAxisTo(LogicalAxis::Block, childWM);
    AlignJustifySelf(aAlignSelf, bAxisInChildWM, flags, baselineAdjust, aCBSize, aRI, aSize, aPos);
}

// Gecko: layout/generic/nsGridContainerFrame.cpp JustifySelf
pub fn JustifySelf(aGridItem: &GridItemInfo, aJustifySelf: StyleAlignFlags, aCBSize: nscoord, aCBWM: WritingMode, aRI: &ReflowInput, aSize: &LogicalSize, aFlags: AlignJustifyFlags, aPos: &mut LogicalPoint) {
    let mut aJustifySelf = aJustifySelf;
    let mut flags = aFlags;
    if aJustifySelf.contains(StyleAlignFlags::SAFE) {
        flags += AlignJustifyFlag::OverflowSafe;
    }
    aJustifySelf &= !StyleAlignFlags::FLAG_BITS;

    let childWM = aRI.GetWritingMode();
    if aCBWM.ParallelAxisStartsOnSameSide(LogicalAxis::Inline, &childWM) {
        flags += AlignJustifyFlag::SameSide;
    }

    if (aGridItem.mState[LogicalAxis::Inline].get() & ItemState::eLastBaselineSharingGroup).to_bool() {
        flags += AlignJustifyFlag::LastBaselineSharingGroup;
    }

    if aJustifySelf == StyleAlignFlags::NORMAL {
        aJustifySelf = StyleAlignFlags::STRETCH;
    }

    let mut baselineAdjust: nscoord = 0;
    // Grid's 'justify-self' axis is always parallel to the container's inline
    // axis, so justify-self:left|right always applies.
    if aJustifySelf == StyleAlignFlags::LEFT {
        aJustifySelf = if aCBWM.IsBidiLTR() { StyleAlignFlags::START } else { StyleAlignFlags::END };
    } else if aJustifySelf == StyleAlignFlags::RIGHT {
        aJustifySelf = if aCBWM.IsBidiLTR() { StyleAlignFlags::END } else { StyleAlignFlags::START };
    } else if aJustifySelf == StyleAlignFlags::BASELINE || aJustifySelf == StyleAlignFlags::LAST_BASELINE {
        aJustifySelf = aGridItem.GetSelfBaseline(aJustifySelf, LogicalAxis::Inline, &mut baselineAdjust);
    }

    let iAxisInChildWM = aCBWM.ConvertAxisTo(LogicalAxis::Inline, childWM);
    AlignJustifySelf(aJustifySelf, iAxisInChildWM, flags, baselineAdjust, aCBSize, aRI, aSize, aPos);
}

// Gecko: layout/generic/nsGridContainerFrame.cpp GetAlignJustifyValue
pub fn GetAlignJustifyValue(aAlignment: StyleAlignFlags, aWM: WritingMode, aIsAlign: bool, aOverflowSafe: &mut bool) -> StyleAlignFlags {
    let mut aAlignment = aAlignment;
    *aOverflowSafe = aAlignment.contains(StyleAlignFlags::SAFE);
    aAlignment &= !StyleAlignFlags::FLAG_BITS;

    // Map some alignment values to 'start' / 'end'.
    if aAlignment == StyleAlignFlags::LEFT || aAlignment == StyleAlignFlags::RIGHT {
        if aIsAlign {
            // Grid's 'align-content' axis is never parallel to the inline axis.
            return StyleAlignFlags::START;
        }
        let isStart = aWM.IsBidiLTR() == (aAlignment == StyleAlignFlags::LEFT);
        return if isStart { StyleAlignFlags::START } else { StyleAlignFlags::END };
    }
    if aAlignment == StyleAlignFlags::FLEX_START {
        return StyleAlignFlags::START; // same as 'start' for Grid
    }
    if aAlignment == StyleAlignFlags::FLEX_END {
        return StyleAlignFlags::END; // same as 'end' for Grid
    }
    aAlignment
}

// Gecko: layout/generic/nsGridContainerFrame.cpp GetAlignJustifyDistributionFallback
pub fn GetAlignJustifyDistributionFallback(aDistribution: &StyleContentDistribution, aOverflowSafe: &mut bool) -> Option<StyleAlignFlags> {
    // See "4.3. Distributed Alignment" for the default fallback alignment values:
    // https://drafts.csswg.org/css-align-3/#distribution-values
    //
    // TODO: Extend this function to handle explicitly specified fallback
    // alignment once the CSS Alignment Module introduces that syntax:
    // https://github.com/w3c/csswg-drafts/issues/1002.
    if aDistribution.primary() == StyleAlignFlags::SPACE_BETWEEN {
        *aOverflowSafe = true;
        return Some(StyleAlignFlags::START);
    }
    if aDistribution.primary() == StyleAlignFlags::SPACE_AROUND || aDistribution.primary() == StyleAlignFlags::SPACE_EVENLY {
        *aOverflowSafe = true;
        return Some(StyleAlignFlags::CENTER);
    }
    if aDistribution.primary() == StyleAlignFlags::STRETCH {
        *aOverflowSafe = false;
        return Some(StyleAlignFlags::START);
    }
    None
}

//----------------------------------------------------------------------

// Frame class boilerplate
// =======================

// NS_QUERYFRAME_HEAD(nsGridContainerFrame) / NS_IMPL_FRAMEARENA_HELPERS / NS_NewGridContainerFrame:
// see `do_QueryFrame` and `NS_NewGridContainerFrame` in the first part of this file (ADAPTER: a frame is made by
// FrameTree::AddFrame and given its class there).

//----------------------------------------------------------------------

// nsGridContainerFrame Method Implementations
// ===========================================

impl nsGridContainerFrame {
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridItemCB
    pub fn GridItemCB(aChild: nsIFrame) -> nsRect {
        MOZ_ASSERT!(aChild.IsAbsolutelyPositioned(None));
        let cb = aChild.GetProperty(GridItemContainingBlockRect());
        MOZ_ASSERT!(cb.is_some(), "this method must only be called on grid items, and the grid container should've reflowed this item by now and set up cb");
        cb.unwrap()
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::AddImplicitNamedAreasInternal
    // (ADAPTER: `ImplicitNamedAreas*& aAreas` is a `&mut *mut` to the HashMap the frame property holds)
    pub fn AddImplicitNamedAreasInternal(&self, aNameList: &NameList, aAreas: &mut *mut ImplicitNamedAreas) {
        for nameIdent in aNameList.iter() {
            let name: nsAtom = NS_Atomize(&*nameIdent.0);
            let mut indexOfSuffix: u32 = 0;
            if Grid::IsNameWithStartSuffix(name, &mut indexOfSuffix) || Grid::IsNameWithEndSuffix(name, &mut indexOfSuffix) {
                // Extract the name that was found earlier.
                let areaName = &name[..indexOfSuffix as usize];

                // Lazily create the ImplicitNamedAreas.
                if aAreas.is_null() {
                    *aAreas = Box::into_raw(Box::new(ImplicitNamedAreas::new()));
                    self.SetProperty(ImplicitNamedAreasProperty(), *aAreas as usize);
                }

                let name = NS_Atomize(areaName);
                // auto addPtr = aAreas->lookupForAdd(name); if (!addPtr) { aAreas->add(addPtr, name, NamedArea{...}) }
                unsafe {
                    (**aAreas).entry(name).or_insert_with(|| MakeNamedArea(name));
                }
            }
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::AddImplicitNamedAreas(Span<LineNameList>)
    pub fn AddImplicitNamedAreas__Span_LineNameList(&self, aLineNameLists: &[NameList]) {
        // https://drafts.csswg.org/css-grid-2/#implicit-named-areas
        // Note: recording these names for fast lookup later is just an optimization.
        let mut areas = self.GetImplicitNamedAreasPtr();
        let len = std::cmp::min(aLineNameLists.len(), kMaxLine as usize) as u32;
        for i in 0..len {
            self.AddImplicitNamedAreasInternal(&aLineNameLists[i as usize], &mut areas);
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::AddImplicitNamedAreas(Span<StyleLineNameListValue>)
    pub fn AddImplicitNamedAreas__Span_StyleLineNameListValue(&self, aLineNameList: &[StyleLineNameListValue]) {
        // https://drafts.csswg.org/css-grid-2/#implicit-named-areas
        // Note: recording these names for fast lookup later is just an optimization.
        let mut count: u32 = 0;
        let mut areas = self.GetImplicitNamedAreasPtr();
        for nameList in aLineNameList {
            match nameList {
                StyleLineNameListValue::Repeat(repeat) => {
                    for repeatNameList in repeat.line_names.iter() {
                        self.AddImplicitNamedAreasInternal(repeatNameList, &mut areas);
                        count += 1;
                    }
                }
                StyleLineNameListValue::LineNames(names) => {
                    self.AddImplicitNamedAreasInternal(names, &mut areas);
                    count += 1;
                }
            }

            if count as usize >= kMaxLine as usize {
                break;
            }
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::InitImplicitNamedAreas
    pub fn InitImplicitNamedAreas(&self, aStyle: &nsStylePosition) {
        let areas = self.GetImplicitNamedAreasPtr();
        if !areas.is_null() {
            // Clear it, but reuse the hashtable itself for now.  We'll remove it
            // below if it isn't needed anymore.
            unsafe { (*areas).clear() };
        }
        // C++ lambda `Add`; a nested fn that takes its capture (`this`) as a parameter.
        fn Add(aThis: &nsGridContainerFrame, aTemplate: &GridTemplate, aIsSubgrid: bool) {
            aThis.AddImplicitNamedAreas__Span_LineNameList(aTemplate.LineNameLists(aIsSubgrid));
            for value in aTemplate.TrackListValues() {
                if let StyleTrackListValue::TrackRepeat(repeat) = value {
                    aThis.AddImplicitNamedAreas__Span_LineNameList(&repeat.line_names);
                }
            }

            if aIsSubgrid {
                if let GridTemplate::Subgrid(subgrid) = aTemplate {
                    // For subgrid, |aTemplate.LineNameLists(aIsSubgrid)| returns an empty
                    // list so we have to manually add each item.
                    aThis.AddImplicitNamedAreas__Span_StyleLineNameListValue(&subgrid.line_names);
                }
            }
        }
        Add(self, aStyle.mGridTemplateColumns(), self.IsColSubgrid());
        Add(self, aStyle.mGridTemplateRows(), self.IsRowSubgrid());
        let areas = self.GetImplicitNamedAreasPtr();
        if !areas.is_null() && unsafe { (*areas).len() } == 0 {
            // RemoveProperty deletes the value (the property is deletable in C++).
            GridPropRemove::<ImplicitNamedAreas>(self.0, ImplicitNamedAreasProperty());
        }
    }
}

// ADAPTER: StyleGridLine::LineName() (`ident.AsAtom()`): the line name as a Gecko-style atom.
fn GridLine_LineName(aLine: &StyleGridLine) -> nsAtom {
    NS_Atomize(&*aLine.ident.0)
}

// ADAPTER: GetRepeatAutoValue() of StyleGridTemplateComponent (ServoStyleConstsInlines.h): the repeat(auto-fill |
// auto-fit) value of the track list. Only called when the sizing functions say there is one.
fn GridTemplateRepeatAuto(aTemplate: &GridTemplate) -> &StyleTrackRepeat {
    match aTemplate {
        mrt_style::style::values::generics::grid::GenericGridTemplateComponent::TrackList(list) => match &list.values[list.auto_repeat_index] {
            StyleTrackListValue::TrackRepeat(r) => r,
            _ => panic!("auto_repeat_index does not point at a repeat"),
        },
        _ => panic!("GetRepeatAutoValue() on a template that is not a track list"),
    }
}

// ADAPTER: GetImplicitNamedAreas() as the raw pointer that the C++ `ImplicitNamedAreas*` is (null when none).
impl nsGridContainerFrame {
    pub fn GetImplicitNamedAreasPtr(&self) -> *mut ImplicitNamedAreas {
        match self.GetImplicitNamedAreas() {
            Some(a) => a as *mut ImplicitNamedAreas,
            None => std::ptr::null_mut(),
        }
    }
}

// ADAPTER: `NamedArea{StyleAtom(do_AddRef(name)), {0, 0}, {0, 0}}`: Stylo's NamedArea holds a Stylo Atom.
fn MakeNamedArea(aName: nsAtom) -> StyleNamedArea {
    StyleNamedArea {
        name: mrt_style::style::Atom::from(aName),
        rows: mrt_style::style::values::specified::position::UnsignedRange { start: 0, end: 0 },
        columns: mrt_style::style::values::specified::position::UnsignedRange { start: 0, end: 0 },
    }
}

impl nsGridContainerFrame_Grid {
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::ResolveLine
    /**
     * Return a line number for (non-auto) aLine, per:
     * https://drafts.csswg.org/css-grid-2/#line-placement
     * @param aLine style data for the line (must be non-auto)
     * @param aNth a number of lines to find from aFromIndex, negative if the
     *             search should be in reverse order.  In the case aLine has
     *             a specified line name, it's permitted to pass in zero which
     *             will be treated as one.
     * @param aFromIndex the zero-based index to start counting from
     * @param aLineNameList the explicit named lines
     * @param aSide the axis+edge we're resolving names for (e.g. if we're
                    resolving a grid-row-start line, pass LogicalSide::BStart)
     * @param aExplicitGridEnd the last line in the explicit grid
     * @param aStyle the StylePosition() for the grid container
     * @return a definite line (1-based), clamped to
     *   the mClampMinLine..mClampMaxLine range
     */
    pub fn ResolveLine(&self, aLine: &StyleGridLine, aNth: i32, aFromIndex: u32, aNameMap: &LineNameMap, aSide: LogicalSide, aExplicitGridEnd: u32, aStyle: &nsStylePosition) -> i32 {
        MOZ_ASSERT!(!aLine.IsAuto());
        let mut aNth = aNth.clamp(kMinLine, kMaxLine);
        let mut line: i32 = 0;
        if GridLine_LineName(aLine).is_empty() {
            MOZ_ASSERT!(aNth != 0, "css-grid 9.2: <integer> must not be zero.");
            line = aFromIndex as i32 + aNth;
        } else {
            if aNth == 0 {
                // <integer> was omitted; treat it as 1.
                aNth = 1;
            }
            let isNameOnly = !aLine.is_span && aLine.line_num == 0;
            if isNameOnly {
                let mut implicitLines: Vec<u32> = Vec::new();
                aNameMap.FindNamedAreas(GridLine_LineName(aLine), aSide, &mut implicitLines);
                if !implicitLines.is_empty() || aNameMap.HasImplicitNamedArea(GridLine_LineName(aLine)) {
                    // aName is a named area - look for explicit lines named
                    // <name>-start/-end depending on which side we're resolving.
                    // https://drafts.csswg.org/css-grid-2/#grid-placement-slot
                    let mut lineName = String::from(GridLine_LineName(aLine));
                    if IsStart(aSide) {
                        lineName.push_str("-start");
                    } else {
                        lineName.push_str("-end");
                    }
                    let name = NS_Atomize(&lineName);
                    line = aNameMap.FindNamedLine(name, &mut aNth, aFromIndex, &implicitLines) as i32;
                }
            }

            if line == 0 {
                // If LineName() ends in -start/-end, try the prefix as a named area.
                let mut implicitLines: Vec<u32> = Vec::new();
                let mut index: u32 = 0;
                let useStart = Grid::IsNameWithStartSuffix(GridLine_LineName(aLine), &mut index);
                if useStart || Grid::IsNameWithEndSuffix(GridLine_LineName(aLine), &mut index) {
                    let side = MakeLogicalSide(GetAxis(aSide), if useStart { LogicalEdge::Start } else { LogicalEdge::End });
                    let name = NS_Atomize(&GridLine_LineName(aLine)[..index as usize]);
                    aNameMap.FindNamedAreas(name, side, &mut implicitLines);
                }
                line = aNameMap.FindNamedLine(GridLine_LineName(aLine), &mut aNth, aFromIndex, &implicitLines) as i32;
            }

            if line == 0 {
                MOZ_ASSERT!(aNth != 0, "we found all N named lines but 'line' is zero!");
                let edgeLine: i32;
                if aLine.is_span {
                    // https://drafts.csswg.org/css-grid-2/#grid-placement-span-int
                    // 'span <custom-ident> N'
                    edgeLine = if IsStart(aSide) { 1 } else { aExplicitGridEnd as i32 };
                } else {
                    // https://drafts.csswg.org/css-grid-2/#grid-placement-int
                    // '<custom-ident> N'
                    edgeLine = if aNth < 0 { 1 } else { aExplicitGridEnd as i32 };
                }
                // "If not enough lines with that name exist, all lines in the implicit
                // grid are assumed to have that name..."
                line = edgeLine + aNth;
            }
        }
        // Note: at this point, 'line' might be outside of aNameMap's allowed range,
        // [mClampMinLin, mClampMaxLine]. This is fine; we'll clamp once we've
        // resolved *both* the start and end line -- in particular, we clamp in
        // ResolveLineRange(). If we clamped here, it'd be premature -- if one line
        // is definite and the other is specified as a span to some named line
        // (i.e. we need to perform a name-search that starts from the definite
        // line), then it matters whether we clamp the definite line before or after
        // that search. See https://bugzilla.mozilla.org/show_bug.cgi?id=1800566#c6
        // for more.
        line
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::ResolveLineRangeHelper
    /**
     * Helper method for ResolveLineRange.
     * @see ResolveLineRange
     * @return a pair (start,end) of lines
     */
    pub fn ResolveLineRangeHelper(&self, aStart: &StyleGridLine, aEnd: &StyleGridLine, aNameMap: &LineNameMap, aAxis: LogicalAxis, aExplicitGridEnd: u32, aStyle: &nsStylePosition) -> LinePair {
        MOZ_ASSERT!(kAutoLine as i32 > kMaxLine);
        let startNum = aStart.line_num.clamp(kMinLine, kMaxLine);
        let endNum = aEnd.line_num.clamp(kMinLine, kMaxLine);

        if aStart.is_span {
            if aEnd.is_span || aEnd.IsAuto() {
                // https://drafts.csswg.org/css-grid-2/#grid-placement-errors
                if GridLine_LineName(aStart).is_empty() {
                    // span <integer> / span *
                    // span <integer> / auto
                    return (kAutoLine as i32, startNum);
                }
                // span <custom-ident> / span *
                // span <custom-ident> / auto
                return (kAutoLine as i32, 1); // XXX subgrid explicit size instead of 1?
            }

            let from: u32 = if endNum < 0 { aExplicitGridEnd + 1 } else { 0 };
            let end = self.ResolveLine(aEnd, endNum, from, aNameMap, MakeLogicalSide(aAxis, LogicalEdge::End), aExplicitGridEnd, aStyle);
            let span = if startNum == 0 { 1 } else { startNum };
            if end <= 1 {
                // The end is at or before the first explicit line, thus all lines before
                // it match <custom-ident> since they're implicit.
                let start = std::cmp::max(end - span, aNameMap.mClampMinLine);
                return (start, end);
            }
            let start = self.ResolveLine(aStart, -span, end as u32, aNameMap, MakeLogicalSide(aAxis, LogicalEdge::Start), aExplicitGridEnd, aStyle);
            return (start, end);
        }

        let mut start: i32 = kAutoLine as i32;
        if aStart.IsAuto() {
            if aEnd.IsAuto() {
                // auto / auto
                return (start, 1); // XXX subgrid explicit size instead of 1?
            }
            if aEnd.is_span {
                if GridLine_LineName(aEnd).is_empty() {
                    // auto / span <integer>
                    MOZ_ASSERT!(endNum != 0);
                    return (start, endNum);
                }
                // https://drafts.csswg.org/css-grid-2/#grid-placement-errors
                // auto / span <custom-ident>
                return (start, 1); // XXX subgrid explicit size instead of 1?
            }
        } else {
            let from: u32 = if startNum < 0 { aExplicitGridEnd + 1 } else { 0 };
            start = self.ResolveLine(aStart, startNum, from, aNameMap, MakeLogicalSide(aAxis, LogicalEdge::Start), aExplicitGridEnd, aStyle);
            if aEnd.IsAuto() {
                // A "definite line / auto" should resolve the auto to 'span 1'.
                // The error handling in ResolveLineRange will make that happen and also
                // clamp the end line correctly if we return "start / start".
                return (start, start);
            }
        }

        let from: u32;
        let nth = if endNum == 0 { 1 } else { endNum };
        if aEnd.is_span {
            if start < 0 {
                if GridLine_LineName(aEnd).is_empty() {
                    return (start, start + nth);
                }
                from = 0;
            } else {
                if start >= aExplicitGridEnd as i32 {
                    // The start is at or after the last explicit line, thus all lines
                    // after it match <custom-ident> since they're implicit.
                    return (start, std::cmp::min(start + nth, aNameMap.mClampMaxLine));
                }
                from = start as u32;
            }
        } else {
            from = if endNum < 0 { aExplicitGridEnd + 1 } else { 0 };
        }
        let end = self.ResolveLine(aEnd, nth, from, aNameMap, MakeLogicalSide(aAxis, LogicalEdge::End), aExplicitGridEnd, aStyle);
        if start == kAutoLine as i32 {
            // auto / definite line
            start = std::cmp::max(aNameMap.mClampMinLine, end - 1);
        }
        (start, end)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::ResolveLineRange
    /**
     * Return a LineRange based on the given style data. Non-auto lines
     * are resolved to a definite line number (1-based) per:
     * https://drafts.csswg.org/css-grid-2/#line-placement
     * with placement errors corrected per:
     * https://drafts.csswg.org/css-grid-2/#grid-placement-errors
     * @param aStyle the StylePosition() for the grid container
     * @param aStart style data for the start line
     * @param aEnd style data for the end line
     * @param aLineNameList the explicit named lines
     * @param aAxis the axis we're resolving names in
     * @param aExplicitGridEnd the last line in the explicit grid
     * @param aStyle the StylePosition() for the grid container
     */
    pub fn ResolveLineRange(&self, aStart: &StyleGridLine, aEnd: &StyleGridLine, aNameMap: &LineNameMap, aAxis: LogicalAxis, aExplicitGridEnd: u32, aStyle: &nsStylePosition) -> LineRange {
        let mut r = self.ResolveLineRangeHelper(aStart, aEnd, aNameMap, aAxis, aExplicitGridEnd, aStyle);
        MOZ_ASSERT!(r.1 != kAutoLine as i32);

        if r.0 == kAutoLine as i32 {
            // r.second is a span, clamp it to aNameMap.mClampMaxLine - 1 so that
            // the returned range has a HypotheticalEnd <= aNameMap.mClampMaxLine.
            // https://drafts.csswg.org/css-grid-2/#overlarge-grids
            r.1 = std::cmp::min(r.1, aNameMap.mClampMaxLine - 1);
        } else {
            // Clamp the lines to be within our limits, per
            // https://drafts.csswg.org/css-grid-2/#overlarge-grids
            // Note that our limits here might come from the [kMinLine, kMaxLine]
            // extremes; or, they might just be the bounds of a subgrid's explicit
            // grid. We use the same clamping approach either way, per
            // https://drafts.csswg.org/css-grid-2/#subgrid-implicit ("using the same
            // procedure as for clamping placement in an overly-large grid").
            //
            // Note that these two clamped assignments might collapse our range to
            // have both edges pointing at the same line (spanning 0 tracks); this
            // might happen here if e.g. r.first were mClampMaxLine, and r.second gets
            // clamped from some higher number down to mClampMaxLine. We'll handle this
            // by shifting the inner line (r.first in this hypothetical) inwards by 1,
            // in the #grid-placement-errors section; that achieves the outcome of
            // the #overlarge-grids clamping spec text that says "its span must be
            // truncated to 1" when clamping an item that was completely outside the
            // limits.
            r.0 = r.0.clamp(aNameMap.mClampMinLine, aNameMap.mClampMaxLine);
            r.1 = r.1.clamp(aNameMap.mClampMinLine, aNameMap.mClampMaxLine);

            // Handle grid placement errors.
            // https://drafts.csswg.org/css-grid-2/#grid-placement-errors
            if r.0 > r.1 {
                std::mem::swap(&mut r.0, &mut r.1);
            } else if r.0 == r.1 {
                // (This is #grid-placement-errors fixup, but it's also where we ensure
                // that any #overlarge-grids fixup that we did above will end up
                // truncating the range to a span of 1 rather than 0 -- i.e. sliding
                // inwards if needed.)
                if r.0 == aNameMap.mClampMaxLine {
                    r.0 = aNameMap.mClampMaxLine - 1;
                }
                r.1 = r.0 + 1;
            }
        }
        LineRange::new(r.0, r.1)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::PlaceDefinite
    /**
     * Return a GridArea with non-auto lines placed at a definite line (1-based)
     * with placement errors resolved.  One or both positions may still
     * be 'auto'.
     * @param aChild the grid item
     * @param aStyle the StylePosition() for the grid container
     */
    pub fn PlaceDefinite(&self, aChild: nsIFrame, aColLineNameMap: &LineNameMap, aRowLineNameMap: &LineNameMap, aStyle: &nsStylePosition) -> GridArea {
        let itemStyle = aChild.StylePosition();
        GridArea::new(
            &self.ResolveLineRange(itemStyle.mGridColumnStart(), itemStyle.mGridColumnEnd(), aColLineNameMap, LogicalAxis::Inline, self.mExplicitGridColEnd, aStyle),
            &self.ResolveLineRange(itemStyle.mGridRowStart(), itemStyle.mGridRowEnd(), aRowLineNameMap, LogicalAxis::Block, self.mExplicitGridRowEnd, aStyle),
        )
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::ResolveAbsPosLineRange
    // NOT PORTED: absolutely positioned children get no frame in this runtime, so nothing calls this (the code
    // is translated because it is plain arithmetic).
    /**
     * As above but for an abs.pos. child.  Any 'auto' lines will be represented
     * by kAutoLine in the LineRange result.
     * @param aGridStart the first line in the final, but untranslated grid
     * @param aGridEnd the last line in the final, but untranslated grid
     */
    pub fn ResolveAbsPosLineRange(&self, aStart: &StyleGridLine, aEnd: &StyleGridLine, aNameMap: &LineNameMap, aAxis: LogicalAxis, aExplicitGridEnd: u32, aGridStart: i32, aGridEnd: i32, aStyle: &nsStylePosition) -> LineRange {
        if aStart.IsAuto() {
            if aEnd.IsAuto() {
                return LineRange::new(kAutoLine as i32, kAutoLine as i32);
            }
            let from: u32 = if aEnd.line_num < 0 { aExplicitGridEnd + 1 } else { 0 };
            let mut end = self.ResolveLine(aEnd, aEnd.line_num, from, aNameMap, MakeLogicalSide(aAxis, LogicalEdge::End), aExplicitGridEnd, aStyle);
            if aEnd.is_span {
                end += 1;
            }
            // A line outside the existing grid is treated as 'auto' for abs.pos (10.1).
            end = Grid::AutoIfOutside(end, aGridStart, aGridEnd);
            return LineRange::new(kAutoLine as i32, end);
        }

        if aEnd.IsAuto() {
            let from: u32 = if aStart.line_num < 0 { aExplicitGridEnd + 1 } else { 0 };
            let mut start = self.ResolveLine(aStart, aStart.line_num, from, aNameMap, MakeLogicalSide(aAxis, LogicalEdge::Start), aExplicitGridEnd, aStyle);
            if aStart.is_span {
                start = std::cmp::max(aGridEnd - start, aGridStart);
            }
            start = Grid::AutoIfOutside(start, aGridStart, aGridEnd);
            return LineRange::new(start, kAutoLine as i32);
        }

        let r = self.ResolveLineRange(aStart, aEnd, aNameMap, aAxis, aExplicitGridEnd, aStyle);
        if r.IsAuto() {
            MOZ_ASSERT!(aStart.is_span && aEnd.is_span, "span / span is the only case leading to IsAuto here -- we dealt with the other cases above");
            // The second span was ignored per 9.2.1.  For abs.pos., 10.1 says that this
            // case should result in "auto / auto" unlike normal flow grid items.
            return LineRange::new(kAutoLine as i32, kAutoLine as i32);
        }

        LineRange::new(Grid::AutoIfOutside(r.mUntranslatedStart(), aGridStart, aGridEnd), Grid::AutoIfOutside(r.mUntranslatedEnd(), aGridStart, aGridEnd))
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::PlaceAbsPos
    // NOT PORTED: absolutely positioned children (see ResolveAbsPosLineRange).
    /**
     * Return a GridArea for abs.pos. item with non-auto lines placed at
     * a definite line (1-based) with placement errors resolved.  One or both
     * positions may still be 'auto'.
     * @param aChild the abs.pos. grid item to place
     * @param aStyle the StylePosition() for the grid container
     */
    pub fn PlaceAbsPos(&self, aChild: nsIFrame, aColLineNameMap: &LineNameMap, aRowLineNameMap: &LineNameMap, aStyle: &nsStylePosition) -> GridArea {
        let itemStyle = aChild.StylePosition();
        let gridColStart = 1 - self.mExplicitGridOffsetCol as i32;
        let gridRowStart = 1 - self.mExplicitGridOffsetRow as i32;
        GridArea::new(
            &self.ResolveAbsPosLineRange(itemStyle.mGridColumnStart(), itemStyle.mGridColumnEnd(), aColLineNameMap, LogicalAxis::Inline, self.mExplicitGridColEnd, gridColStart, self.mGridColEnd as i32, aStyle),
            &self.ResolveAbsPosLineRange(itemStyle.mGridRowStart(), itemStyle.mGridRowEnd(), aRowLineNameMap, LogicalAxis::Block, self.mExplicitGridRowEnd, gridRowStart, self.mGridRowEnd as i32, aStyle),
        )
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::FindAutoCol
    /**
     * Find the first column in row aLockedRow starting at aStartCol where aArea
     * could be placed without overlapping other items.  The returned column may
     * cause aArea to overflow the current implicit grid bounds if placed there.
     */
    pub fn FindAutoCol(&self, aStartCol: u32, aLockedRow: u32, aArea: &GridArea) -> u32 {
        let extent = aArea.mCols.Extent();
        let iStart = aLockedRow;
        let iEnd = iStart + aArea.mRows.Extent();
        let mut candidate = aStartCol;
        let mut i = iStart;
        while i < iEnd {
            if i as usize >= self.mCellMap.mCells.len() {
                break;
            }
            let cellsInRow = &self.mCellMap.mCells[i as usize];
            let len = cellsInRow.len() as u32;
            let lastCandidate = candidate;
            // Find the first gap in the current row that's at least 'extent' wide.
            // ('gap' tracks how wide the current column gap is.)
            let mut j = candidate;
            let mut gap = 0;
            while j < len && gap < extent {
                if !cellsInRow[j as usize].mIsOccupied {
                    gap += 1;
                    j += 1;
                    continue;
                }
                candidate = j + 1;
                gap = 0;
                j += 1;
            }
            if lastCandidate < candidate && i != iStart {
                // Couldn't fit 'extent' tracks at 'lastCandidate' here so we must
                // restart from the beginning with the new 'candidate'.
                i = iStart;
            } else {
                i += 1;
            }
        }
        candidate
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::PlaceAutoCol
    /**
     * Place aArea in the first column (in row aArea->mRows.mStart) starting at
     * aStartCol without overlapping other items.  The resulting aArea may
     * overflow the current implicit grid bounds.
     * @param aClampMaxColLine the maximum allowed column line number (zero-based)
     * Pre-condition: aArea->mRows.IsDefinite() is true.
     * Post-condition: aArea->IsDefinite() is true.
     */
    pub fn PlaceAutoCol(&self, aStartCol: u32, aArea: &mut GridArea, aClampMaxColLine: u32) {
        MOZ_ASSERT!(aArea.mRows.IsDefinite() && aArea.mCols.IsAuto());
        let col = self.FindAutoCol(aStartCol, aArea.mRows.mStart, aArea);
        aArea.mCols.ResolveAutoPosition(col, aClampMaxColLine);
        MOZ_ASSERT!(aArea.IsDefinite());
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::FindAutoRow
    /**
     * Find the first row in column aLockedCol starting at aStartRow where aArea
     * could be placed without overlapping other items.  The returned row may
     * cause aArea to overflow the current implicit grid bounds if placed there.
     */
    pub fn FindAutoRow(&self, aLockedCol: u32, aStartRow: u32, aArea: &GridArea) -> u32 {
        let extent = aArea.mRows.Extent();
        let jStart = aLockedCol;
        let jEnd = jStart + aArea.mCols.Extent();
        let iEnd = self.mCellMap.mCells.len() as u32;
        let mut candidate = aStartRow;
        // Find the first gap in the rows that's at least 'extent' tall.
        // ('gap' tracks how tall the current row gap is.)
        let mut i = candidate;
        let mut gap = 0;
        while i < iEnd && gap < extent {
            gap += 1; // tentative, but we may reset it below if a column is occupied
            let cellsInRow = &self.mCellMap.mCells[i as usize];
            let clampedJEnd = std::cmp::min(jEnd, cellsInRow.len() as u32);
            // Check if the current row is unoccupied from jStart to jEnd.
            let mut j = jStart;
            while j < clampedJEnd {
                if cellsInRow[j as usize].mIsOccupied {
                    // Couldn't fit 'extent' rows at 'candidate' here; we hit something
                    // at row 'i'.  So, try the row after 'i' as our next candidate.
                    candidate = i + 1;
                    gap = 0;
                    break;
                }
                j += 1;
            }
            i += 1;
        }
        candidate
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::PlaceAutoRow
    /**
     * Place aArea in the first row (in column aArea->mCols.mStart) starting at
     * aStartRow without overlapping other items. The resulting aArea may
     * overflow the current implicit grid bounds.
     * @param aClampMaxRowLine the maximum allowed row line number (zero-based)
     * Pre-condition: aArea->mCols.IsDefinite() is true.
     * Post-condition: aArea->IsDefinite() is true.
     */
    pub fn PlaceAutoRow(&self, aStartRow: u32, aArea: &mut GridArea, aClampMaxRowLine: u32) {
        MOZ_ASSERT!(aArea.mCols.IsDefinite() && aArea.mRows.IsAuto());
        let row = self.FindAutoRow(aArea.mCols.mStart, aStartRow, aArea);
        aArea.mRows.ResolveAutoPosition(row, aClampMaxRowLine);
        MOZ_ASSERT!(aArea.IsDefinite());
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::PlaceAutoAutoInRowOrder
    /**
     * Place aArea in the first column starting at aStartCol,aStartRow without
     * causing it to overlap other items or overflow mGridColEnd.
     * If there's no such column in aStartRow, continue in position 1,aStartRow+1.
     * @param aClampMaxColLine the maximum allowed column line number (zero-based)
     * @param aClampMaxRowLine the maximum allowed row line number (zero-based)
     * Pre-condition: aArea->mCols.IsAuto() && aArea->mRows.IsAuto() is true.
     * Post-condition: aArea->IsDefinite() is true.
     */
    pub fn PlaceAutoAutoInRowOrder(&self, aStartCol: u32, aStartRow: u32, aArea: &mut GridArea, aClampMaxColLine: u32, aClampMaxRowLine: u32) {
        MOZ_ASSERT!(aArea.mCols.IsAuto() && aArea.mRows.IsAuto());
        let colExtent = aArea.mCols.Extent();
        let gridRowEnd = self.mGridRowEnd;
        let gridColEnd = self.mGridColEnd;
        let mut col = aStartCol;
        let mut row = aStartRow;
        while row < gridRowEnd {
            col = self.FindAutoCol(col, row, aArea);
            if col + colExtent <= gridColEnd {
                break;
            }
            col = 0;
            row += 1;
        }
        MOZ_ASSERT!(row < gridRowEnd || col == 0, "expected column 0 for placing in a new row");
        aArea.mCols.ResolveAutoPosition(col, aClampMaxColLine);
        aArea.mRows.ResolveAutoPosition(row, aClampMaxRowLine);
        MOZ_ASSERT!(aArea.IsDefinite());
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::PlaceAutoAutoInColOrder
    /**
     * Place aArea in the first row starting at aStartCol,aStartRow without
     * causing it to overlap other items or overflow mGridRowEnd.
     * If there's no such row in aStartCol, continue in position aStartCol+1,1.
     * @param aClampMaxColLine the maximum allowed column line number (zero-based)
     * @param aClampMaxRowLine the maximum allowed row line number (zero-based)
     * Pre-condition: aArea->mCols.IsAuto() && aArea->mRows.IsAuto() is true.
     * Post-condition: aArea->IsDefinite() is true.
     */
    pub fn PlaceAutoAutoInColOrder(&self, aStartCol: u32, aStartRow: u32, aArea: &mut GridArea, aClampMaxColLine: u32, aClampMaxRowLine: u32) {
        MOZ_ASSERT!(aArea.mCols.IsAuto() && aArea.mRows.IsAuto());
        let rowExtent = aArea.mRows.Extent();
        let gridRowEnd = self.mGridRowEnd;
        let gridColEnd = self.mGridColEnd;
        let mut col = aStartCol;
        let mut row = aStartRow;
        while col < gridColEnd {
            row = self.FindAutoRow(col, row, aArea);
            if row + rowExtent <= gridRowEnd {
                break;
            }
            row = 0;
            col += 1;
        }
        MOZ_ASSERT!(col < gridColEnd || row == 0, "expected row 0 for placing in a new column");
        aArea.mCols.ResolveAutoPosition(col, aClampMaxColLine);
        aArea.mRows.ResolveAutoPosition(row, aClampMaxRowLine);
        MOZ_ASSERT!(aArea.IsDefinite());
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::CalculateAdjustForAutoFitElements
    /**
     * Calculates the empty tracks in a repeat(auto-fit).
     * @param aOutNumEmptyLines Outputs the number of tracks which are empty.
     * @param aSizingFunctions Sizing functions for the relevant axis.
     * @param aNumGridLines Number of grid lines for the relevant axis.
     * @param aIsEmptyFunc Functor to check if a cell is empty. This should be
     * mCellMap.IsColEmpty or mCellMap.IsRowEmpty, depending on the axis.
     */
    pub fn CalculateAdjustForAutoFitElements<IsEmptyFuncT: Fn(u32) -> bool>(aOutNumEmptyLines: &mut u32, aSizingFunctions: &mut TrackSizingFunctions, aNumGridLines: u32, aIsEmptyFunc: IsEmptyFuncT) -> Option<Vec<u32>> {
        let mut trackAdjust: Option<Vec<u32>> = None;
        let numEmptyLines: &mut u32 = aOutNumEmptyLines;
        *numEmptyLines = 0;
        if aSizingFunctions.NumRepeatTracks() > 0 {
            MOZ_ASSERT!(aSizingFunctions.mHasRepeatAuto);
            // Since this loop is concerned with just the repeat tracks, we
            // iterate from 0..NumRepeatTracks() which is the natural range of
            // mRemoveRepeatTracks. This means we have to add
            // (mExplicitGridOffset + mRepeatAutoStart) to get a zero-based
            // index for arrays like mCellMap/aIsEmptyFunc and trackAdjust. We'll then
            // fill out the trackAdjust array for all the remaining lines.
            let repeatStart = aSizingFunctions.mExplicitGridOffset + aSizingFunctions.mRepeatAutoStart;
            let numRepeats = aSizingFunctions.NumRepeatTracks();
            for i in 0..numRepeats {
                if *numEmptyLines != 0 {
                    MOZ_ASSERT!(trackAdjust.is_some());
                    trackAdjust.as_mut().unwrap()[(repeatStart + i) as usize] = *numEmptyLines;
                }
                if aIsEmptyFunc(repeatStart + i) {
                    *numEmptyLines += 1;
                    if trackAdjust.is_none() {
                        // trackAdjust.emplace(aNumGridLines); SetLength; PodZero
                        trackAdjust = Some(vec![0; aNumGridLines as usize]);
                    }

                    aSizingFunctions.mRemovedRepeatTracks[i as usize] = true;
                }
            }
            // Fill out the trackAdjust array for all the tracks after the repeats.
            if *numEmptyLines != 0 {
                for line in (repeatStart + numRepeats)..aNumGridLines {
                    trackAdjust.as_mut().unwrap()[line as usize] = *numEmptyLines;
                }
            }
        }

        trackAdjust
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::SubgridPlaceGridItems
    // NOT PORTED: subgrid. Original: nsGridContainerFrame.cpp lines 5091-5170 at GECKO_REV (places the items of a
    // subgrid into the parent's tracks).
    pub fn SubgridPlaceGridItems(&mut self, aParentGridRI: &mut GridReflowInput, aParentGrid: &mut Grid, aGridItem: &GridItemInfo) {
        unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)")
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Grid::PlaceGridItems
    /**
     * Place all child frames into the grid and expand the (implicit) grid as
     * needed.  The allocated GridAreas are stored in the GridAreaProperty
     * frame property on the child frame.
     * @param aRepeatSizing the container's [min-|max-]*size - used to determine
     *   the number of repeat(auto-fill/fit) tracks.
     */
    pub fn PlaceGridItems(&mut self, aGridRI: &mut GridReflowInput, aSizes: &RepeatTrackSizingInput) {
        MOZ_ASSERT!(self.mCellMap.mCells.is_empty(), "unexpected entries in cell map");

        self.mAreas = aGridRI.mFrame.GetImplicitNamedAreasPtr();

        if aGridRI.mFrame.HasSubgridItems__() || aGridRI.mFrame.IsSubgrid__() {
            // NOT PORTED: subgrid (UsedTrackSizes is only stored for subgrids and their parents). Original:
            //   if (auto* uts = aGridRI.mFrame->GetUsedTrackSizes()) {
            //     uts->mCanResolveLineRangeSize = {false, false};
            //     uts->mTrackPlans[LogicalAxis::Inline].ClearAndRetainStorage();
            //     uts->mTrackPlans[LogicalAxis::Block].ClearAndRetainStorage();
            //   }
        }

        // SubgridPlaceGridItems will set these if we find any subgrid items.
        aGridRI.mFrame.RemoveStateBits(NS_STATE_GRID_HAS_COL_SUBGRID_ITEM | NS_STATE_GRID_HAS_ROW_SUBGRID_ITEM);

        // https://drafts.csswg.org/css-grid-2/#grid-definition
        // Initialize the end lines of the Explicit Grid (mExplicitGridCol[Row]End).
        // This is determined by the larger of the number of rows/columns defined
        // by 'grid-template-areas' and the 'grid-template-rows'/'-columns', plus one.
        // Also initialize the Implicit Grid (mGridCol[Row]End) to the same values.
        // Note that this is for a grid with a 1,1 origin.  We'll change that
        // to a 0,0 based grid after placing definite lines.
        // ADAPTER: an owned copy of the style wrapper (a ServoArc clone), so that it can be used while aGridRI is
        // borrowed mutably.
        let gridStyle: nsStylePosition = aGridRI.mGridStyle.clone();
        let areas: Option<&StyleTemplateAreas> = match gridStyle.mGridTemplateAreas() {
            StyleGridTemplateAreas::None => None,
            StyleGridTemplateAreas::Areas(a) => Some(&*a.0),
        };
        let mut parentLineNameMap: *const LineNameMap<'static> = std::ptr::null();
        let mut subgridRange: *const LineRange = std::ptr::null();
        let mut subgridAxisIsSameDirection = true;
        let mut subgridIsOrthogonal = false;
        if !aGridRI.mFrame.IsColSubgrid() {
            aGridRI.mColFunctions.InitRepeatTracks(&gridStyle.mColumnGap(), aSizes.mMin.ISize(aGridRI.mWM), aSizes.mSize.ISize(aGridRI.mWM), aSizes.mMax.ISize(aGridRI.mWM));
            let areaCols = match areas {
                Some(a) => a.width + 1,
                None => 1,
            };
            self.mExplicitGridColEnd = aGridRI.mColFunctions.ComputeExplicitGridEnd(areaCols);
        } else {
            // NOT PORTED: subgrid. Original:
            //   const auto* subgrid = aGridRI.mFrame->GetProperty(Subgrid::Prop());
            //   subgridRange = &subgrid->SubgridCols();
            //   uint32_t extent = subgridRange->Extent();
            //   mExplicitGridColEnd = extent + 1;  // the grid is 1-based at this point
            //   parentLineNameMap = ParentLineMapForAxis(subgrid->mIsOrthogonal, LogicalAxis::Inline);
            //   auto parentWM = aGridRI.mFrame->ParentGridContainerForSubgrid()->GetWritingMode();
            //   subgridAxisIsSameDirection = aGridRI.mWM.ParallelAxisStartsOnSameSide(LogicalAxis::Inline, parentWM);
            //   subgridIsOrthogonal = subgrid->mIsOrthogonal;
            unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)")
        }
        self.mGridColEnd = self.mExplicitGridColEnd;
        let colLineNameMap = LineNameMap::new(&gridStyle, unsafe { self.mAreas.as_ref() }, &aGridRI.mColFunctions, unsafe { parentLineNameMap.as_ref() }, unsafe { subgridRange.as_ref() }, subgridAxisIsSameDirection, subgridIsOrthogonal);

        if !aGridRI.mFrame.IsRowSubgrid() {
            let containBSize = aGridRI.mFrame.ContainIntrinsicBSize(0);
            let repeatTrackSizingBSize: nscoord = {
                // This clamping only applies to auto sizes.
                if containBSize.is_some() && aSizes.mSize.BSize(aGridRI.mWM) == NS_UNCONSTRAINEDSIZE {
                    CSSMinMax(containBSize.unwrap(), aSizes.mMin.BSize(aGridRI.mWM), aSizes.mMax.BSize(aGridRI.mWM))
                } else {
                    aSizes.mSize.BSize(aGridRI.mWM)
                }
            };
            aGridRI.mRowFunctions.InitRepeatTracks(&gridStyle.mRowGap(), aSizes.mMin.BSize(aGridRI.mWM), repeatTrackSizingBSize, aSizes.mMax.BSize(aGridRI.mWM));
            let areaRows = match areas {
                Some(a) => a.strings.len() as u32 + 1,
                None => 1,
            };
            self.mExplicitGridRowEnd = aGridRI.mRowFunctions.ComputeExplicitGridEnd(areaRows);
            parentLineNameMap = std::ptr::null();
            subgridRange = std::ptr::null();
        } else {
            // NOT PORTED: subgrid. Original (the row twin of the column branch above):
            //   subgridRange = &subgrid->SubgridRows(); mExplicitGridRowEnd = extent + 1;
            //   parentLineNameMap = ParentLineMapForAxis(subgrid->mIsOrthogonal, LogicalAxis::Block); ...
            unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)")
        }
        self.mGridRowEnd = self.mExplicitGridRowEnd;
        let rowLineNameMap = LineNameMap::new(&gridStyle, unsafe { self.mAreas.as_ref() }, &aGridRI.mRowFunctions, unsafe { parentLineNameMap.as_ref() }, unsafe { subgridRange.as_ref() }, subgridAxisIsSameDirection, subgridIsOrthogonal);

        let isSubgridOrItemInSubgrid = aGridRI.mFrame.IsSubgrid__() || !self.mParentGrid.is_null();
        // The C++ lambda SetSubgridChildEdgeBits captures `this` and reads mGridColEnd / mGridRowEnd when it is
        // called; a nested fn that takes those two values as parameters.
        fn SetSubgridChildEdgeBits(aIsSubgridOrItemInSubgrid: bool, aGridColEnd: u32, aGridRowEnd: u32, aItem: &mut GridItemInfo) {
            if aIsSubgridOrItemInSubgrid {
                let area = &aItem.mArea;
                let (colStart, colEnd, rowStart, rowEnd) = (area.mCols.mStart, area.mCols.mEnd, area.mRows.mStart, area.mRows.mEnd);
                if colStart == 0 {
                    aItem.mState[LogicalAxis::Inline].set(aItem.mState[LogicalAxis::Inline].get() | ItemState::eStartEdge);
                }
                if colEnd == aGridColEnd {
                    aItem.mState[LogicalAxis::Inline].set(aItem.mState[LogicalAxis::Inline].get() | ItemState::eEndEdge);
                }
                if rowStart == 0 {
                    aItem.mState[LogicalAxis::Block].set(aItem.mState[LogicalAxis::Block].get() | ItemState::eStartEdge);
                }
                if rowEnd == aGridRowEnd {
                    aItem.mState[LogicalAxis::Block].set(aItem.mState[LogicalAxis::Block].get() | ItemState::eEndEdge);
                }
            }
        }

        self.SetLineMaps(&colLineNameMap as *const LineNameMap<'_> as *const LineNameMap<'static>, &rowLineNameMap as *const LineNameMap<'_> as *const LineNameMap<'static>);

        // https://drafts.csswg.org/css-grid-2/#line-placement
        // Resolve definite positions per spec chapter 8.3.
        let mut minCol: i32 = 1;
        let mut minRow: i32 = 1;
        aGridRI.mGridItems.clear(); // ClearAndRetainStorage
        aGridRI.mIter.Reset(CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders);

        // NOT PORTED: use counters (needToRecordAutoFlowCounter: telemetry for
        // eUseCounter_custom_GridAutoFlowInitialValueChange; it needs the document and
        // ComputedStyle::HasAuthorSpecifiedGridAutoFlow).

        while !aGridRI.mIter.AtEnd() {
            let child = aGridRI.mIter.get();
            let area = self.PlaceDefinite(child, &colLineNameMap, &rowLineNameMap, &gridStyle);
            aGridRI.mGridItems.push(GridItemInfo::new(child, &area));
            MOZ_ASSERT!(aGridRI.mIter.ItemIndex() == aGridRI.mGridItems.len() - 1, "ItemIndex() is broken");

            let area = &aGridRI.mGridItems.last().unwrap().mArea;
            if area.mCols.IsDefinite() {
                minCol = std::cmp::min(minCol, area.mCols.mUntranslatedStart());
            }
            if area.mRows.IsDefinite() {
                minRow = std::cmp::min(minRow, area.mRows.mUntranslatedStart());
            }
            aGridRI.mIter.Next();
        }

        // Translate the whole grid so that the top-/left-most area is at 0,0.
        self.mExplicitGridOffsetCol = (1 - minCol) as u32; // minCol/Row is always <= 1, see above
        self.mExplicitGridOffsetRow = (1 - minRow) as u32;
        aGridRI.mColFunctions.mExplicitGridOffset = self.mExplicitGridOffsetCol;
        aGridRI.mRowFunctions.mExplicitGridOffset = self.mExplicitGridOffsetRow;
        let offsetToColZero = self.mExplicitGridOffsetCol as i32 - 1;
        let offsetToRowZero = self.mExplicitGridOffsetRow as i32 - 1;
        let isRowMasonry = aGridRI.mFrame.IsRowMasonry();
        let isColMasonry = aGridRI.mFrame.IsColMasonry();
        let isMasonry = isColMasonry || isRowMasonry;
        self.mGridColEnd = (self.mGridColEnd as i32 + offsetToColZero) as u32;
        self.mGridRowEnd = (self.mGridRowEnd as i32 + offsetToRowZero) as u32;
        let gridAxisTrackCount = if isRowMasonry { self.mGridColEnd } else { self.mGridRowEnd };
        aGridRI.mIter.Reset(CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders);
        while !aGridRI.mIter.AtEnd() {
            let idx = aGridRI.mIter.ItemIndex();
            {
                let area = &mut aGridRI.mGridItems[idx].mArea;
                if area.mCols.IsDefinite() {
                    area.mCols.mStart = (area.mCols.mUntranslatedStart() + offsetToColZero) as u32;
                    area.mCols.mEnd = (area.mCols.mUntranslatedEnd() + offsetToColZero) as u32;
                }
                if area.mRows.IsDefinite() {
                    area.mRows.mStart = (area.mRows.mUntranslatedStart() + offsetToRowZero) as u32;
                    area.mRows.mEnd = (area.mRows.mUntranslatedEnd() + offsetToRowZero) as u32;
                }
            }
            if aGridRI.mGridItems[idx].mArea.IsDefinite() {
                if isMasonry {
                    aGridRI.mGridItems[idx].MaybeInhibitSubgridInMasonry(aGridRI.mFrame, gridAxisTrackCount);
                }
                if aGridRI.mGridItems[idx].IsSubgrid__() {
                    let mut grid = Grid::new(self);
                    // ADAPTER: a copy of the item, because the item lives in aGridRI which is lent mutably too.
                    let itemCopy = aGridRI.mGridItems[idx].clone();
                    grid.SubgridPlaceGridItems(aGridRI, self, &itemCopy);
                }
                let item = &mut aGridRI.mGridItems[idx];
                self.mCellMap.Fill(&item.mArea);
                self.InflateGridFor(&item.mArea);
                SetSubgridChildEdgeBits(isSubgridOrItemInSubgrid, self.mGridColEnd, self.mGridRowEnd, item);
            }
            aGridRI.mIter.Next();
        }

        // https://drafts.csswg.org/css-grid-2/#auto-placement-algo
        // Step 1, place 'auto' items that have one definite position -
        // definite row (column) for grid-auto-flow:row (column).
        let flowStyle = gridStyle.mGridAutoFlow();
        let isRowOrder = if isMasonry { isRowMasonry } else { flowStyle.contains(StyleGridAutoFlow::ROW) };
        let isSparse = !flowStyle.contains(StyleGridAutoFlow::DENSE);
        let clampMaxColLine = (colLineNameMap.mClampMaxLine + offsetToColZero) as u32;
        let clampMaxRowLine = (rowLineNameMap.mClampMaxLine + offsetToRowZero) as u32;
        // `LineRange& major = isRowOrder ? area.mRows : area.mCols` (a reference into the area): ADAPTER, the
        // nested fns pick the range, since Rust cannot keep both references across the calls below.
        fn Major(aArea: &GridArea, aIsRowOrder: bool) -> &LineRange {
            if aIsRowOrder { &aArea.mRows } else { &aArea.mCols }
        }
        fn Minor(aArea: &GridArea, aIsRowOrder: bool) -> &LineRange {
            if aIsRowOrder { &aArea.mCols } else { &aArea.mRows }
        }
        // We need 1 cursor per row (or column) if placement is sparse.
        {
            let mut cursors: Option<HashMap<u32, u32>> = None;
            if isSparse {
                cursors = Some(HashMap::new());
            }
            // placeAutoMinorFunc = isRowOrder ? &Grid::PlaceAutoCol : &Grid::PlaceAutoRow (a member function pointer)
            let clampMaxLine = if isRowOrder { clampMaxColLine } else { clampMaxRowLine };
            aGridRI.mIter.Reset(CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders);
            while !aGridRI.mIter.AtEnd() {
                let idx = aGridRI.mIter.ItemIndex();
                let item = &mut aGridRI.mGridItems[idx];
                if Major(&item.mArea, isRowOrder).IsDefinite() && Minor(&item.mArea, isRowOrder).IsAuto() {
                    // Items with 'auto' in the minor dimension only.
                    let majorStart = Major(&item.mArea, isRowOrder).mStart;
                    let cursor = if isSparse { *cursors.as_ref().unwrap().get(&majorStart).unwrap_or(&0) } else { 0 };
                    if isRowOrder {
                        self.PlaceAutoCol(cursor, &mut item.mArea, clampMaxLine);
                    } else {
                        self.PlaceAutoRow(cursor, &mut item.mArea, clampMaxLine);
                    }
                    if isMasonry {
                        item.MaybeInhibitSubgridInMasonry(aGridRI.mFrame, gridAxisTrackCount);
                    }
                    if item.IsSubgrid__() {
                        let mut grid = Grid::new(self);
                        let itemCopy = item.clone();
                        grid.SubgridPlaceGridItems(aGridRI, self, &itemCopy);
                    }
                    let item = &mut aGridRI.mGridItems[idx];
                    self.mCellMap.Fill(&item.mArea);
                    SetSubgridChildEdgeBits(isSubgridOrItemInSubgrid, self.mGridColEnd, self.mGridRowEnd, item);
                    if isSparse {
                        let major = Major(&item.mArea, isRowOrder).mStart;
                        let minorEnd = Minor(&item.mArea, isRowOrder).mEnd;
                        cursors.as_mut().unwrap().insert(major, minorEnd); // InsertOrUpdate
                    }
                }
                let item = &aGridRI.mGridItems[idx];
                self.InflateGridFor(&item.mArea); // Step 2, inflating for auto items too
                aGridRI.mIter.Next();
            }
        }

        // XXX NOTE possible spec issue.
        // XXX It's unclear if the remaining major-dimension auto and
        // XXX auto in both dimensions should use the same cursor or not,
        // XXX https://www.w3.org/Bugs/Public/show_bug.cgi?id=16044
        // XXX seems to indicate it shouldn't.
        // XXX https://drafts.csswg.org/css-grid-2/#auto-placement-algo
        // XXX now says it should (but didn't in earlier versions)

        // Step 3, place the remaining grid items
        let mut cursorMajor: u32 = 0; // for 'dense' these two cursors will stay at 0,0
        let mut cursorMinor: u32 = 0;
        // placeAutoMajorFunc = isRowOrder ? &Grid::PlaceAutoRow : &Grid::PlaceAutoCol
        let clampMaxMajorLine = if isRowOrder { clampMaxRowLine } else { clampMaxColLine };
        aGridRI.mIter.Reset(CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders);
        while !aGridRI.mIter.AtEnd() {
            let idx = aGridRI.mIter.ItemIndex();
            let item = &mut aGridRI.mGridItems[idx];
            MOZ_ASSERT!(aGridRI.mIter.get() == item.mFrame, "iterator out of sync with aState.mGridItems");
            if Major(&item.mArea, isRowOrder).IsAuto() {
                if Minor(&item.mArea, isRowOrder).IsDefinite() {
                    // Items with 'auto' in the major dimension only.
                    if isSparse {
                        if Minor(&item.mArea, isRowOrder).mStart < cursorMinor {
                            cursorMajor += 1;
                        }
                        cursorMinor = Minor(&item.mArea, isRowOrder).mStart;
                    }
                    if isRowOrder {
                        self.PlaceAutoRow(cursorMajor, &mut item.mArea, clampMaxMajorLine);
                    } else {
                        self.PlaceAutoCol(cursorMajor, &mut item.mArea, clampMaxMajorLine);
                    }
                    if isSparse {
                        cursorMajor = Major(&item.mArea, isRowOrder).mStart;
                    }
                } else {
                    // Items with 'auto' in both dimensions.
                    if isRowOrder {
                        self.PlaceAutoAutoInRowOrder(cursorMinor, cursorMajor, &mut item.mArea, clampMaxColLine, clampMaxRowLine);
                    } else {
                        self.PlaceAutoAutoInColOrder(cursorMajor, cursorMinor, &mut item.mArea, clampMaxColLine, clampMaxRowLine);
                    }
                    if isSparse {
                        cursorMajor = Major(&item.mArea, isRowOrder).mStart;
                        cursorMinor = Minor(&item.mArea, isRowOrder).mEnd;
                        #[cfg(debug_assertions)]
                        {
                            let gridMajorEnd = if isRowOrder { self.mGridRowEnd } else { self.mGridColEnd };
                            let gridMinorEnd = if isRowOrder { self.mGridColEnd } else { self.mGridRowEnd };
                            MOZ_ASSERT!(cursorMajor <= gridMajorEnd, "we shouldn't need to place items further than 1 track past the current end of the grid, in major dimension");
                            MOZ_ASSERT!(cursorMinor <= gridMinorEnd, "we shouldn't add implicit minor tracks for auto/auto");
                        }
                    }
                }
                if isMasonry {
                    item.MaybeInhibitSubgridInMasonry(aGridRI.mFrame, gridAxisTrackCount);
                }
                if item.IsSubgrid__() {
                    let mut grid = Grid::new(self);
                    let itemCopy = item.clone();
                    grid.SubgridPlaceGridItems(aGridRI, self, &itemCopy);
                }
                let item = &mut aGridRI.mGridItems[idx];
                self.mCellMap.Fill(&item.mArea);
                self.InflateGridFor(&item.mArea);
                SetSubgridChildEdgeBits(isSubgridOrItemInSubgrid, self.mGridColEnd, self.mGridRowEnd, item);
                // XXXmats it might be possible to optimize this a bit for masonry layout
                // if this item was placed in the 2nd row && !isSparse, or the 1st row
                // is full.  Still gotta inflate the grid for all items though to make
                // the grid large enough...
            }
            aGridRI.mIter.Next();
        }

        // Force all items into the 1st/2nd track and have span 1 in the masonry axis.
        // (See comment on nsGridContainerFrame::MasonryLayout().)
        if isMasonry {
            let masonryAxis = if isRowMasonry { LogicalAxis::Block } else { LogicalAxis::Inline };
            aGridRI.mIter.Reset(CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders);
            while !aGridRI.mIter.AtEnd() {
                let idx = aGridRI.mIter.ItemIndex();
                let masonryRange = aGridRI.mGridItems[idx].mArea.LineRangeForAxis_mut(masonryAxis);
                masonryRange.mStart = std::cmp::min(masonryRange.mStart, 1u32);
                masonryRange.mEnd = masonryRange.mStart + 1u32;
                aGridRI.mIter.Next();
            }
        }

        if let Some(absCB) = aGridRI.mFrame.GetAbsoluteContainingBlock() {
            if absCB.PrepareAbsoluteFrames(aGridRI.mFrame.0) {
                // 10.1. With a Grid Container as Containing Block
                // https://drafts.csswg.org/css-grid-2/#abspos-items
                // We only resolve definite lines here; we'll align auto positions to the
                // grid container later during reflow.
                let children = absCB.GetChildList();
                let offsetToColZero = self.mExplicitGridOffsetCol as i32 - 1;
                let offsetToRowZero = self.mExplicitGridOffsetRow as i32 - 1;
                // Untranslate the grid again temporarily while resolving abs.pos. lines.
                // (AutoRestore: the two values are put back after the loop)
                let zeroOffsetGridColEnd = self.mGridColEnd;
                let zeroOffsetGridRowEnd = self.mGridRowEnd;
                self.mGridColEnd = (self.mGridColEnd as i32 - offsetToColZero) as u32;
                self.mGridRowEnd = (self.mGridRowEnd as i32 - offsetToRowZero) as u32;
                aGridRI.mAbsPosItems.clear();
                for child in children {
                    let placed = self.PlaceAbsPos(child, &colLineNameMap, &rowLineNameMap, &gridStyle);
                    aGridRI.mAbsPosItems.push(GridItemInfo::new(child, &placed));
                    let info = aGridRI.mAbsPosItems.last_mut().unwrap();
                    let area = &mut info.mArea;
                    if area.mCols.mUntranslatedStart() != kAutoLine as i32 {
                        area.mCols.mStart = (area.mCols.mUntranslatedStart() + offsetToColZero) as u32;
                        if isColMasonry {
                            // XXXmats clamp any non-auto line to 0 or 1. This is intended to
                            // allow authors to address the start/end of the masonry box.
                            // This is experimental at this point though and needs author feedback
                            // and spec work to sort out what is desired and how it should work.
                            // See https://github.com/w3c/csswg-drafts/issues/4650
                            area.mCols.mStart = std::cmp::min(area.mCols.mStart, 1u32);
                        }
                    }
                    if area.mCols.mUntranslatedEnd() != kAutoLine as i32 {
                        area.mCols.mEnd = (area.mCols.mUntranslatedEnd() + offsetToColZero) as u32;
                        if isColMasonry {
                            // ditto
                            area.mCols.mEnd = std::cmp::min(area.mCols.mEnd, 1u32);
                        }
                    }
                    if area.mRows.mUntranslatedStart() != kAutoLine as i32 {
                        area.mRows.mStart = (area.mRows.mUntranslatedStart() + offsetToRowZero) as u32;
                        if isRowMasonry {
                            // ditto
                            area.mRows.mStart = std::cmp::min(area.mRows.mStart, 1u32);
                        }
                    }
                    if area.mRows.mUntranslatedEnd() != kAutoLine as i32 {
                        area.mRows.mEnd = (area.mRows.mUntranslatedEnd() + offsetToRowZero) as u32;
                        if isRowMasonry {
                            // ditto
                            area.mRows.mEnd = std::cmp::min(area.mRows.mEnd, 1u32);
                        }
                    }
                    if isMasonry {
                        info.MaybeInhibitSubgridInMasonry(aGridRI.mFrame, gridAxisTrackCount);
                    }

                    // An abs.pos. subgrid with placement auto/1 or -1/auto technically
                    // doesn't span any parent tracks.  Inhibit subgridding in this case.
                    if info.IsSubgrid__LogicalAxis(LogicalAxis::Inline) {
                        if info.mArea.mCols.mStart == zeroOffsetGridColEnd || info.mArea.mCols.mEnd == 0 {
                            info.InhibitSubgrid(aGridRI.mFrame, LogicalAxis::Inline);
                        }
                    }
                    if info.IsSubgrid__LogicalAxis(LogicalAxis::Block) {
                        if info.mArea.mRows.mStart == zeroOffsetGridRowEnd || info.mArea.mRows.mEnd == 0 {
                            info.InhibitSubgrid(aGridRI.mFrame, LogicalAxis::Block);
                        }
                    }

                    // if (info->IsSubgrid()) { Grid grid(this); grid.SubgridPlaceGridItems(...) }
                    // NOT PORTED: subgrid (C1).
                    MOZ_ASSERT!(!info.IsSubgrid__(), "NOT PORTED: abs.pos. subgrids");
                }
                self.mGridColEnd = zeroOffsetGridColEnd;
                self.mGridRowEnd = zeroOffsetGridRowEnd;
            }
        }

        // Count empty 'auto-fit' tracks in the repeat() range.
        // |colAdjust| will have a count for each line in the grid of how many
        // tracks were empty between the start of the grid and that line.

        let mut colAdjust: Option<Vec<u32>> = None;
        let mut numEmptyCols: u32 = 0;
        if aGridRI.mColFunctions.mHasRepeatAuto && matches!(GridTemplateRepeatAuto(gridStyle.mGridTemplateColumns()).count, StyleRepeatCount::AutoFit) {
            let cellMap = &self.mCellMap;
            colAdjust = Grid::CalculateAdjustForAutoFitElements(&mut numEmptyCols, &mut aGridRI.mColFunctions, self.mGridColEnd + 1, |i: u32| -> bool { cellMap.IsEmptyCol(i) });
        }

        // Do similar work for the row tracks, with the same logic.
        let mut rowAdjust: Option<Vec<u32>> = None;
        let mut numEmptyRows: u32 = 0;
        if aGridRI.mRowFunctions.mHasRepeatAuto && matches!(GridTemplateRepeatAuto(gridStyle.mGridTemplateRows()).count, StyleRepeatCount::AutoFit) {
            let cellMap = &self.mCellMap;
            rowAdjust = Grid::CalculateAdjustForAutoFitElements(&mut numEmptyRows, &mut aGridRI.mRowFunctions, self.mGridRowEnd + 1, |i: u32| -> bool { cellMap.IsEmptyRow(i) });
        }
        MOZ_ASSERT!((numEmptyCols > 0) == colAdjust.is_some());
        MOZ_ASSERT!((numEmptyRows > 0) == rowAdjust.is_some());
        // Remove the empty 'auto-fit' tracks we found above, if any.
        if numEmptyCols != 0 || numEmptyRows != 0 {
            // Adjust the line numbers in the grid areas.
            for item in aGridRI.mGridItems.iter_mut() {
                if numEmptyCols != 0 {
                    item.AdjustForRemovedTracks(LogicalAxis::Inline, colAdjust.as_ref().unwrap());
                }
                if numEmptyRows != 0 {
                    item.AdjustForRemovedTracks(LogicalAxis::Block, rowAdjust.as_ref().unwrap());
                }
            }
            for item in aGridRI.mAbsPosItems.iter_mut() {
                if numEmptyCols != 0 {
                    item.AdjustForRemovedTracks(LogicalAxis::Inline, colAdjust.as_ref().unwrap());
                }
                if numEmptyRows != 0 {
                    item.AdjustForRemovedTracks(LogicalAxis::Block, rowAdjust.as_ref().unwrap());
                }
            }
            // Adjust the grid size.
            self.mGridColEnd -= numEmptyCols;
            self.mExplicitGridColEnd -= numEmptyCols;
            self.mGridRowEnd -= numEmptyRows;
            self.mExplicitGridRowEnd -= numEmptyRows;
            // Adjust the track mapping to unmap the removed tracks.
            let colRepeatCount = aGridRI.mColFunctions.NumRepeatTracks();
            aGridRI.mColFunctions.SetNumRepeatTracks(colRepeatCount - numEmptyCols);
            let rowRepeatCount = aGridRI.mRowFunctions.NumRepeatTracks();
            aGridRI.mRowFunctions.SetNumRepeatTracks(rowRepeatCount - numEmptyRows);
        }

        // Update the line boundaries of the implicit grid areas, if needed.
        if !self.mAreas.is_null() && aGridRI.mFrame.HasAnyStateBits(NS_STATE_GRID_COMPUTED_INFO) {
            for (_, areaInfo) in unsafe { (*self.mAreas).iter_mut() } {
                // Resolve the lines for the area. We use the name of the area as the
                // name of the lines, knowing that the line placement algorithm will
                // add the -start and -end suffixes as appropriate for layout.
                let mut lineStartAndEnd = StyleGridLine::default();
                lineStartAndEnd.ident = mrt_style::style::values::CustomIdent(areaInfo.name.clone());

                let columnLines = self.ResolveLineRange(&lineStartAndEnd, &lineStartAndEnd, &colLineNameMap, LogicalAxis::Inline, self.mExplicitGridColEnd, &gridStyle);

                let rowLines = self.ResolveLineRange(&lineStartAndEnd, &lineStartAndEnd, &rowLineNameMap, LogicalAxis::Block, self.mExplicitGridRowEnd, &gridStyle);

                // Put the resolved line indices back into the area structure.
                areaInfo.columns.start = columnLines.mStart + self.mExplicitGridOffsetCol;
                areaInfo.columns.end = columnLines.mEnd + self.mExplicitGridOffsetCol;
                areaInfo.rows.start = rowLines.mStart + self.mExplicitGridOffsetRow;
                areaInfo.rows.end = rowLines.mEnd + self.mExplicitGridOffsetRow;
            }
        }
    }
}

// P4 of nsGridContainerFrame.rs: nsGridContainerFrame.cpp lines 5661-7767 (Gecko revision GECKO_REV).
//
// Assumptions about the other parts (see the P4 report): `Tracks::mSizes` is a `TrackPlan` with
// `Index<usize>`/`IndexMut<usize>`, `Length()`, `SetLength(n)`, `ZeroInitialize()`, `IsEmpty()`, `iter()`,
// `iter_mut()`, `LastElement()`, `Clone`; `GridItemInfo::mState` and `mBaselineOffset` are plain
// `PerLogicalAxis` fields (the C++ `mutable` is a `&mut GridItemInfo` here); the arrays indexed by
// GridIntrinsicSizeType (`EnumeratedArray<GridIntrinsicSizeType, T>`) are `[T; 3]` indexed by `type as usize`.

// ADAPTER: Gecko's INFINITE_ISIZE_COORD (layout/generic/nsIFrame.h: nscoord(NS_MAXSIZE)).
pub const INFINITE_ISIZE_COORD: nscoord = NS_MAXSIZE;


// Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::Initialize
impl Tracks {
    pub fn Initialize(&mut self, aFunctions: &TrackSizingFunctions, aGridGap: &StyleNonNegativeLengthPercentageOrNormal, aNumTracks: u32, aContentBoxSize: nscoord) {
        self.mSizes.SetLength(aNumTracks as usize);
        self.mSizes.ZeroInitialize();
        for i in 0..self.mSizes.Length() as u32 {
            let sz = &mut self.mSizes[i as usize];
            self.mStateUnion |= sz.Initialize(aContentBoxSize, aFunctions.SizingFor(i));
            if self.mIsMasonry {
                sz.mBase = aContentBoxSize;
                sz.mLimit = aContentBoxSize;
            }
        }
        self.mGridGap = nsLayoutUtils::ResolveGapToLength(aGridGap, aContentBoxSize);
        self.mContentBoxSize = aContentBoxSize;
    }
}

// ADAPTER: nsStylePosition::UsedSelfAlignment(LogicalAxis, const ComputedStyle*) of Gecko
// (layout/style/nsStyleStruct.cpp), written on top of UsedAlignSelf/UsedJustifySelf, which exist.
fn UsedSelfAlignment(aPos: &nsStylePosition, aAxis: LogicalAxis, aParent: Option<mrt_style::style::servo_arc::Arc<ComputedStyle>>) -> StyleAlignFlags {
    if aAxis == LogicalAxis::Block {
        aPos.UsedAlignSelf(aParent).0
    } else {
        aPos.UsedJustifySelf(aParent).0
    }
}

// ADAPTER: Stylo's computed TrackBreadth is a Rust enum; Gecko's cbindgen type has the methods
// AsBreadth() (a LengthPercentage) and AsFlex() (a struct whose `_0` is the flex factor).
fn TrackBreadth_AsBreadth(aBreadth: &StyleTrackBreadth) -> &LengthPercentage {
    match aBreadth {
        StyleTrackBreadth::Breadth(lp) => lp,
        _ => unreachable!("StyleTrackBreadth::AsBreadth on a non-breadth value"),
    }
}
fn TrackBreadth_AsFlex(aBreadth: &StyleTrackBreadth) -> f32 {
    match aBreadth {
        StyleTrackBreadth::Flex(f) => f.0,
        _ => unreachable!("StyleTrackBreadth::AsFlex on a non-flex value"),
    }
}
// ADAPTER: StyleTrackSize::AsFitContent().AsBreadth()
fn TrackSize_AsFitContentBreadth(aSize: &StyleTrackSize) -> &LengthPercentage {
    match aSize {
        StyleTrackSize::FitContent(b) => TrackBreadth_AsBreadth(b),
        _ => unreachable!("StyleTrackSize::AsFitContent on a non-fit-content value"),
    }
}

// ADAPTER: the C++ `EnumSet<GridIntrinsicSizeType>` of CachedIntrinsicSizes::EnsureContributions and
// ResolveIntrinsicSize (the crate's EnumSet stand-in has no `-=` and no iteration).
#[derive(Clone, Copy, Default)]
struct GridIntrinsicSizeTypeSet([bool; 3]);
impl GridIntrinsicSizeTypeSet {
    fn contains(&self, aType: GridIntrinsicSizeType) -> bool {
        self.0[aType as usize]
    }
}
impl std::ops::AddAssign<GridIntrinsicSizeType> for GridIntrinsicSizeTypeSet {
    fn add_assign(&mut self, aType: GridIntrinsicSizeType) {
        self.0[aType as usize] = true;
    }
}
impl std::ops::SubAssign<GridIntrinsicSizeType> for GridIntrinsicSizeTypeSet {
    fn sub_assign(&mut self, aType: GridIntrinsicSizeType) {
        self.0[aType as usize] = false;
    }
}

/**
 * Reflow aChild in the given aAvailableSize.
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp MeasuringReflow
// (ADAPTER: Rust has no default arguments: pass NS_MAXSIZE for aIMinSizeClamp and aBMinSizeClamp.)
fn MeasuringReflow(aChild: nsIFrame, aReflowInput: Option<&ReflowInput>, aRC: Option<gfxContext>, aAvailableSize: LogicalSize, aCBSize: LogicalSize, aIMinSizeClamp: nscoord, aBMinSizeClamp: nscoord) -> nscoord {
    MOZ_ASSERT!(aChild.IsGridItem(), "aChild should be a grid item!");
    let parent = nsGridContainerFrame::from_frame(aChild.GetParent().unwrap());
    let pc = aChild.PresContext();
    let dummyParentState: Option<ReflowInput>;
    let rs: &ReflowInput = match aReflowInput {
        Some(ri) => ri,
        None => {
            MOZ_ASSERT!(!parent.HasAnyStateBits(NS_FRAME_IN_REFLOW));
            let mut initFlags = ReflowInput_InitFlags::default();
            initFlags += ReflowInput_InitFlag::DummyParentReflowInput;
            dummyParentState = Some(ReflowInput::new__nsPresContext_nsIFrame_gfxContext_LogicalSize_InitFlags(&pc, parent.0, aRC.unwrap_or(gfxContext), LogicalSize::new(parent.GetWritingMode(), 0, NS_UNCONSTRAINEDSIZE), initFlags));
            dummyParentState.as_ref().unwrap()
        }
    };
    // NOT PORTED: `#ifdef DEBUG` parent->SetProperty(nsContainerFrame::DebugReflowingWithInfiniteISize(), true)
    // (it only suppresses ABSURD_SIZE warnings).
    let wm = aChild.GetWritingMode();
    let mut csFlags = ComputeSizeFlags::default();
    csFlags += ComputeSizeFlag::IsGridMeasuringReflow;
    // Shrink-wrap grid items that will be aligned (rather than stretched) in
    // their own inline axis.
    if !parent.GridItemShouldStretch(aChild, LogicalAxis::Inline) {
        csFlags += ComputeSizeFlag::ShrinkWrap;
    }
    if aAvailableSize.ISize(wm) == INFINITE_ISIZE_COORD {
        csFlags += ComputeSizeFlag::ShrinkWrap;
    }
    if aIMinSizeClamp != NS_MAXSIZE {
        csFlags += ComputeSizeFlag::IClampMarginBoxMinSize;
    }
    if aBMinSizeClamp != NS_MAXSIZE {
        csFlags += ComputeSizeFlag::BClampMarginBoxMinSize;
        aChild.SetProperty(nsIFrame::BClampMarginBoxMinSizeProperty(), aBMinSizeClamp);
    } else {
        aChild.RemoveProperty(nsIFrame::BClampMarginBoxMinSizeProperty());
    }
    let mut childRI = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
        &pc,
        rs,
        aChild,
        aAvailableSize,
        Some(aCBSize),
        EnumSet::default(),
        StyleSizeOverrides::default(),
        csFlags,
        std::ptr::null_mut(),
    );

    // FIXME (perf): It would be faster to do this only if the previous reflow of
    // the child was not a measuring reflow, and only if the child does some of
    // the things that are affected by ComputeSizeFlag::IsGridMeasuringReflow.
    childRI.SetBResize(true);
    // Not 100% sure this is needed, but be conservative for now:
    childRI.SetBResizeForPercentages(true);

    let mut childSize = ReflowOutput::new__ReflowInput(&childRI);
    let mut childStatus = nsReflowStatus::new();
    let flags = nsIFrame_ReflowChildFlags::NoMoveFrame | nsIFrame_ReflowChildFlags::NoDeleteNextInFlowChild;

    // Reflowing the child might invalidate the cache, so we declare the variable
    // inside the if-statement to ensure it isn't accessed after it may have
    // become invalid.
    if let Some(cachedMeasurement) = GridItemCachedBAxisMeasurement::GetProp(aChild) {
        if cachedMeasurement.IsValidFor(aChild, &aCBSize) {
            *childSize.BSize_mut(wm) = cachedMeasurement.BSize();
            *childSize.ISize_mut(wm) = aChild.ISize(wm);
            nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(aChild, &pc, &childSize, Some(&childRI), wm, LogicalPoint::new__WritingMode(wm), nsSize::new(0, 0), flags);
            GRID_LOG!("[perf] MeasuringReflow accepted cached value=%d, child=%p, aCBSize.ISize=%d", cachedMeasurement.BSize(), aChild, aCBSize.ISize(wm));
            return cachedMeasurement.BSize();
        }
    }

    parent.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
        aChild,
        &pc,
        &mut childSize,
        &childRI,
        wm,
        LogicalPoint::new__WritingMode(wm),
        nsSize::new(0, 0),
        flags,
        &mut childStatus,
        None,
    );
    nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(aChild, &pc, &childSize, Some(&childRI), wm, LogicalPoint::new__WritingMode(wm), nsSize::new(0, 0), flags);
    // NOT PORTED: `#ifdef DEBUG` parent->RemoveProperty(nsContainerFrame::DebugReflowingWithInfiniteISize())

    if let Some(cachedMeasurement) = GridItemCachedBAxisMeasurement::GetProp(aChild) {
        cachedMeasurement.Update(aChild, &aCBSize, childSize.BSize(wm));
        GRID_LOG!("[perf] MeasuringReflow rejected but updated cached value=%d, child=%p, aCBSize.ISize=%d", cachedMeasurement.BSize(), aChild, aCBSize.ISize(wm));
    } else {
        let cachedMeasurement = GridItemCachedBAxisMeasurement::new(aChild, &aCBSize, childSize.BSize(wm));
        GridItemCachedBAxisMeasurement::SetProp(aChild, Box::new(cachedMeasurement));
        GRID_LOG!("[perf] MeasuringReflow created new cached value=%d, child=%p, aCBSize.ISize=%d", childSize.BSize(wm), aChild, aCBSize.ISize(wm));
    }

    childSize.BSize(wm)
}

/**
 * Return the accumulated margin+border+padding in aAxis for aFrame (a subgrid)
 * and its ancestor subgrids.
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp SubgridAccumulatedMarginBorderPadding
// NOT PORTED: subgrid (a subgrid is a grid container whose grid-template-* is `subgrid`; this runtime
// panics when it meets one, see nsGridContainerFrame_P6.rs). Only a subgrid and its descendants call this.
// Original (nsGridContainerFrame.cpp lines 5792-5840 at GECKO_REV).
fn SubgridAccumulatedMarginBorderPadding(aFrame: nsIFrame, aSubgrid: &Subgrid, aResultWM: WritingMode, aAxis: LogicalAxis) -> LogicalMargin {
    unimplemented!("NOT PORTED: subgrid (SubgridAccumulatedMarginBorderPadding)")
}

/**
 * Return the [min|max]-content contribution of aChild to its parent (i.e.
 * the child's margin-box) in aAxis.
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp ContentContribution
// (ADAPTER: no default arguments: pass NS_MAXSIZE for aMinSizeClamp and `&StyleSizeOverrides::default()`.)
fn ContentContribution(aGridItem: &GridItemInfo, aGridRI: &GridReflowInput, aAxis: LogicalAxis, aPercentageBasis: LogicalSize, aConstraint: IntrinsicISizeType, aMinSizeClamp: nscoord, aOverrides: &StyleSizeOverrides) -> nscoord {
    let child = aGridItem.mFrame;

    let gridWM = aGridRI.mWM;
    let mut extraMargin: nscoord = 0;
    if child.GetParent() != Some(aGridRI.mFrame.0) {
        // |child| is a subgrid descendant, so it contributes its subgrids'
        // margin+border+padding for any edge tracks that it spans.
        // NOT PORTED: subgrid (nsGridContainerFrame.cpp lines 5861-5899 at GECKO_REV: the accumulated
        // margin+border+padding of the subgrid and half of its gap delta become extraMargin).
        unimplemented!("NOT PORTED: subgrid (a grid item whose parent is not the grid container)");
    }

    let rc = aGridRI.mRenderingContext;
    let axis = gridWM.PhysicalAxis(aAxis);
    let mut size = nsLayoutUtils::IntrinsicForAxis(axis, Some(rc), child, aConstraint, Some(aPercentageBasis), nsLayoutUtils::BAIL_IF_REFLOW_NEEDED, aMinSizeClamp, aOverrides);
    let childWM = child.GetWritingMode();
    let isOrthogonal = childWM.IsOrthogonalTo(&gridWM);
    let childAxis = if isOrthogonal { GetOrthogonalAxis(aAxis) } else { aAxis };
    if size == NS_INTRINSIC_ISIZE_UNKNOWN && childAxis == LogicalAxis::Block {
        if aGridRI.mIsGridIntrinsicSizing && aAxis == LogicalAxis::Block {
            // We may reach here while computing the grid container's min-content
            // contribution in ComputeIntrinsicISize(), potentially during row size
            // resolution. In this context, the main reason for computing row sizes is
            // to transfer the child's block-size to the inline-axis via aspect-ratio,
            // contributing to the grid container's intrinsic inline-size in a later
            // column size resolution. Since an indefinite block-size cannot be
            // transferred in this way, we can safely skip MeasuringReflow() and
            // simply use zero as a dummy value because the value does not affect the
            // result.
            size = 0;
        } else {
            // We need to reflow the child to find its BSize contribution.
            let mut availISize = INFINITE_ISIZE_COORD;
            let mut availBSize = NS_UNCONSTRAINEDSIZE;
            // The next two variables are MinSizeClamp values in the child's axes.
            let mut iMinSizeClamp = NS_MAXSIZE;
            let mut bMinSizeClamp = NS_MAXSIZE;
            let mut cbSize = aPercentageBasis;
            // Below, we try to resolve the child's grid-area size in its inline-axis
            // to use as the CB/Available size in the MeasuringReflow that follows.
            // (The `child->GetParent() != aGridRI.mFrame` branch, an item that is a child of a subgrid
            // descendant, is NOT PORTED: it panicked above; nsGridContainerFrame.cpp lines 5932-5969.)
            let inlineAxisInChildWM = if isOrthogonal { LogicalAxis::Block } else { LogicalAxis::Inline };
            let colSize = cbSize.Size(inlineAxisInChildWM, childWM);
            if colSize != NS_UNCONSTRAINEDSIZE {
                MOZ_ASSERT!(aGridRI.mCols.mCanResolveLineRangeSize, "Grid column sizes should be resolvable!");
                if isOrthogonal {
                    availBSize = colSize;
                    if (aGridItem.mState[aAxis].get() & ItemState::eClampMarginBoxMinSize).to_bool() {
                        bMinSizeClamp = colSize;
                    }
                } else {
                    availISize = colSize;
                    if (aGridItem.mState[aAxis].get() & ItemState::eClampMarginBoxMinSize).to_bool() {
                        iMinSizeClamp = colSize;
                    }
                }
            }
            if isOrthogonal == (aAxis == LogicalAxis::Inline) {
                bMinSizeClamp = aMinSizeClamp;
            } else {
                iMinSizeClamp = aMinSizeClamp;
            }
            let availableSize = LogicalSize::new(childWM, availISize, availBSize);
            size = MeasuringReflow(child, aGridRI.mReflowInput, Some(rc), availableSize, cbSize, iMinSizeClamp, bMinSizeClamp);
        }
        size += child.GetLogicalUsedMargin(childWM).BStartEnd(childWM);
        let overflow = size - aMinSizeClamp;
        if overflow > 0 {
            let contentSize = child.ContentBSize(childWM);
            let newContentSize = std::cmp::max(0, contentSize - overflow);
            // XXXmats deal with percentages better, see bug 1300369 comment 27.
            size -= contentSize - newContentSize;
        }
    }
    MOZ_ASSERT!(aGridItem.mBaselineOffset[aAxis].get() >= 0, "baseline offset should be non-negative at this point");
    MOZ_ASSERT!((aGridItem.mState[aAxis].get() & ItemState::eIsBaselineAligned).to_bool() || aGridItem.mBaselineOffset[aAxis].get() == 0, "baseline offset should be zero when not baseline-aligned");
    size += aGridItem.mBaselineOffset[aAxis].get();
    size += extraMargin;
    std::cmp::max(size, 0)
}

// Gecko: layout/generic/nsGridContainerFrame.cpp CachedIntrinsicSizes
struct CachedIntrinsicSizes {
    // EnumeratedArray<GridIntrinsicSizeType, Maybe<nscoord>> (indexed by `type as usize`)
    mSizes: [Option<nscoord>; 3],

    // The item's percentage basis for intrinsic sizing purposes.
    mPercentageBasis: LogicalSize,

    // "if the grid item spans only grid tracks that have a fixed max track
    // sizing function, its automatic minimum size in that dimension is
    // further clamped to less than or equal to the size necessary to fit its
    // margin box within the resulting grid area (flooring at zero)"
    // https://drafts.csswg.org/css-grid-2/#min-size-auto
    // This is the clamp value to use for that:
    mMinSizeClamp: nscoord,
}

impl CachedIntrinsicSizes {
    // Gecko: layout/generic/nsGridContainerFrame.cpp CachedIntrinsicSizes::CachedIntrinsicSizes
    fn new(aGridItem: &GridItemInfo, aGridRI: &GridReflowInput, aAxis: LogicalAxis) -> CachedIntrinsicSizes {
        CachedIntrinsicSizes { mSizes: [None; 3], mPercentageBasis: aGridRI.PercentageBasisFor(aAxis, aGridItem), mMinSizeClamp: NS_MAXSIZE }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp CachedIntrinsicSizes::EnsureContributions
    fn EnsureContributions(&mut self, mut aTypes: GridIntrinsicSizeTypeSet, aGridItem: &GridItemInfo, aGridRI: &GridReflowInput, aAxis: LogicalAxis) {
        // max-content and min-content should behave as initial value in block axis.
        // XXXalaskanemily: The specifics might have changed in the spec?
        // https://drafts.csswg.org/css-sizing-3/#valdef-width-min-content
        // https://drafts.csswg.org/css-sizing-3/#valdef-width-max-content

        // If we need to calculate GridIntrinsicSizeType::MinContribution, we might
        // need to substitute GridIntrinsicSizeType::MinContentContribution instead.
        // Per https://drafts.csswg.org/css-grid-2/#algo-single-span-items
        // Section "For auto minimums":
        //  * "if the item's computed preferred size behaves as auto or depends on
        //    the size of its containing block in the relevant axis," then we do in
        //    fact need the used minimum size."
        //  * "...else the item's minimum contribution is its min-content
        //    contribution" in which case we make a recursive call to compute
        //    GridIntrinsicSizeType::MinContentContribution instead, and do a fixup
        //    to place that value in the MinContentContribution slot.
        // Note that we use BehavesLikeInitialValue and HasPercent to implement
        // the spec check for "behaves as auto or depends on the size of its
        // containing block".
        // We make a similar check in MinContributionDependsOnAutoMinSize as
        // an earlier test for whether we need the used minimum size.
        if aTypes.contains(GridIntrinsicSizeType::MinContribution) {
            let child = aGridItem.mFrame;
            let stylePos = child.StylePosition();
            let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(child, std::ptr::null_mut());
            let cbwm = aGridRI.mWM;
            let styleSize = stylePos.Size(aAxis, cbwm, &anchorResolutionParams);
            let axisInItemWM = cbwm.ConvertAxisTo(aAxis, child.GetWritingMode());
            // FIXME: Bug 567039: moz-fit-content and -moz-available are not
            // supported for block size dimension on sizing properties (e.g. height),
            // so we treat it as `auto`.
            if !styleSize.BehavesLikeInitialValue(axisInItemWM) && !styleSize.HasPercent() {
                // Calculate without MinSize, but ensuring MinContentContribution.
                aTypes -= GridIntrinsicSizeType::MinContribution;
                aTypes += GridIntrinsicSizeType::MinContentContribution;
                self.EnsureContributions(aTypes, aGridItem, aGridRI, aAxis);
                // Copy the MinSize from the MinContentContribution.
                self.mSizes[GridIntrinsicSizeType::MinContribution as usize] = self.mSizes[GridIntrinsicSizeType::MinContentContribution as usize];
                return;
            }
        }

        for type_ in kAllGridIntrinsicSizeTypes.iter().copied() {
            if !aTypes.contains(type_) {
                continue;
            }
            if self.mSizes[type_ as usize].is_none() {
                self.mSizes[type_ as usize] = Some(CachedIntrinsicSizes::ComputeContribution(type_, aGridItem, aGridRI, aAxis, self.mPercentageBasis, self.mMinSizeClamp));
            }
        }
    }

    // Computes the MinSize, MinContentContribution, or MaxContentContribution of
    // an item in the given axis.
    // This helps to implement EnsureContributions. It's here to prevent other
    // places from using it, as it is not general purpose and requires that the
    // caller has made checks for when we will use the MinContentContribution as
    // the MinSize, as EnsureContributions does.
    // Gecko: layout/generic/nsGridContainerFrame.cpp CachedIntrinsicSizes::ComputeContribution
    fn ComputeContribution(aType: GridIntrinsicSizeType, aGridItem: &GridItemInfo, aGridRI: &GridReflowInput, aAxis: LogicalAxis, aPercentageBasis: LogicalSize, aMinSizeClamp: nscoord) -> nscoord {
        let containerWM = aGridRI.mWM;
        let rc = aGridRI.mRenderingContext;
        match aType {
            GridIntrinsicSizeType::MinContentContribution => ContentContribution(aGridItem, aGridRI, aAxis, aPercentageBasis, IntrinsicISizeType::MinISize, aMinSizeClamp, &StyleSizeOverrides::default()),
            GridIntrinsicSizeType::MaxContentContribution => ContentContribution(aGridItem, aGridRI, aAxis, aPercentageBasis, IntrinsicISizeType::PrefISize, aMinSizeClamp, &StyleSizeOverrides::default()),
            GridIntrinsicSizeType::MinContribution => {
                // Compute the min-size contribution for a grid item, as defined at
                // https://drafts.csswg.org/css-grid-2/#min-size-contribution
                let child = aGridItem.mFrame;
                let stylePos = child.StylePosition();
                let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(child, std::ptr::null_mut());
                let axisInItemWM = containerWM.ConvertAxisTo(aAxis, child.GetWritingMode());
                #[cfg(debug_assertions)]
                {
                    // The caller must handle this case separately.
                    // See EnsureContributions.
                    let styleSize = stylePos.Size(aAxis, containerWM, &anchorResolutionParams);
                    MOZ_ASSERT!(styleSize.BehavesLikeInitialValue(axisInItemWM) || styleSize.HasPercent(), "Should have been caught in EnsureContributions");
                }
                // https://drafts.csswg.org/css-grid-2/#min-size-auto
                // This calculates the min-content contribution from either a definite
                // min-width (or min-height depending on aAxis), or the
                // "specified / transferred size" for min-width:auto if
                // overflow == visible (as min-width:0 otherwise), or
                // NS_UNCONSTRAINEDSIZE for other min-width intrinsic values
                // (which results in always taking the "content size" part below).
                MOZ_ASSERT!(aGridItem.mBaselineOffset[aAxis].get() >= 0, "baseline offset should be non-negative at this point");
                MOZ_ASSERT!((aGridItem.mState[aAxis].get() & ItemState::eIsBaselineAligned).to_bool() || aGridItem.mBaselineOffset[aAxis].get() == 0, "baseline offset should be zero when not baseline-aligned");
                let styleMinSize = stylePos.MinSize(aAxis, containerWM, &anchorResolutionParams);

                // max-content and min-content should behave as initial value in block
                // axis.
                // FIXME: Bug 567039: moz-fit-content and -moz-available are not
                // supported for block size dimension on sizing properties
                // (e.g. height), so we treat it as `auto`.
                let isAuto = styleMinSize.BehavesLikeInitialValue(axisInItemWM);
                let mut s = aGridItem.mBaselineOffset[aAxis].get();

                // Check if the min-size style of the grid item is auto and the
                // minimum contribution is content-based.
                // While the eContentBasedAutoMinSize flag is not synonymous with
                // an item having content-based automatic minimum contribution,
                // the previous checks should catch the other cases in which the
                // automatic minimum contribution is zero instead.
                //
                // See bug 1951821 for this discrepency between the flag's usage
                // and the specification:
                // https://drafts.csswg.org/css-grid-2/#min-size-auto
                if !isAuto || (aGridItem.mState[aAxis].get() & ItemState::eContentBasedAutoMinSize).to_bool() {
                    let contrib = nsLayoutUtils::MinSizeContributionForAxis(containerWM.PhysicalAxis(aAxis), Some(rc), child, IntrinsicISizeType::MinISize, aPercentageBasis, 0);
                    if contrib == NS_UNCONSTRAINEDSIZE {
                        s = contrib;
                    } else {
                        s += contrib;
                    }

                    if (axisInItemWM == LogicalAxis::Inline && nsIFrame::ToExtremumLength(&styleMinSize).is_some()) || (isAuto && !child.StyleDisplay().IsScrollableOverflow()) {
                        // "if the item's computed preferred size behaves as auto or
                        // depends on the size of its containing block in the relevant
                        // axis, its minimum contribution is the outer size that would
                        // result from assuming the item's used minimum size as its
                        // preferred size"
                        //
                        // The "auto or depends on the size of its containing block" is
                        // checked above with ItemState::eContentBasedAutoMinSize.
                        //
                        // https://drafts.csswg.org/css-grid-2/#minimum-contribution
                        let mut overrides = StyleSizeOverrides::default();
                        if axisInItemWM == LogicalAxis::Inline {
                            overrides.mStyleISize = Some(styleMinSize.clone());
                        } else {
                            overrides.mStyleBSize = Some(styleMinSize.clone());
                        }
                        // Now calculate the "content size" part and return whichever is
                        // smaller.
                        MOZ_ASSERT!(isAuto || s == NS_UNCONSTRAINEDSIZE);
                        s = std::cmp::min(s, ContentContribution(aGridItem, aGridRI, aAxis, aPercentageBasis, IntrinsicISizeType::MinISize, aMinSizeClamp, &overrides));
                    }
                }
                s
            }
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp CachedIntrinsicSizes::SizesOrDefault
    fn SizesOrDefault(&self) -> [nscoord; 3] {
        let mut sizes: [nscoord; 3] = [0; 3];
        for type_ in kAllGridIntrinsicSizeTypes.iter().copied() {
            sizes[type_ as usize] = self.mSizes[type_ as usize].unwrap_or(0);
        }
        sizes
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::CalculateSizes
impl Tracks {
    pub fn CalculateSizes(&mut self, aGridRI: &GridReflowInput, aGridItems: &Vec<GridItemInfo>, aFunctions: &TrackSizingFunctions, aContentBoxSize: nscoord, aRange: LineRangeSelector, aConstraint: SizingConstraint) {
        // Implement the intrinsic sizing algorithm, step 12.5 as described in:
        // https://drafts.csswg.org/css-grid-2/#algo-content
        let mut percentageBasis = aContentBoxSize;
        if percentageBasis == NS_UNCONSTRAINEDSIZE {
            percentageBasis = 0;
        }
        // 12.5 step 1: Shim baseline-aligned items so their intrinsic size
        // contributions reflect their baseline alignment
        // https://drafts.csswg.org/css-grid-2/#algo-baseline-shims
        self.InitializeItemBaselines(aGridRI, aGridItems);

        // 12.5 steps 2-5
        self.ResolveIntrinsicSize(aGridRI, aGridItems, aFunctions, aRange, percentageBasis, aConstraint);

        // Neither 12.6 nor 12.7 will occur under min content constraint.
        if aConstraint != SizingConstraint::MinContent {
            let mut freeSpace = aContentBoxSize;
            if freeSpace != NS_UNCONSTRAINEDSIZE {
                freeSpace -= self.SumOfGridGaps();
            }
            // 12.6 maximize tracks by distributing free space.
            // https://drafts.csswg.org/css-grid-2/#algo-grow-tracks
            self.DistributeFreeSpace(freeSpace);

            // 12.7 Expand flexible tracks.
            // https://drafts.csswg.org/css-grid-2/#algo-flex-tracks
            self.StretchFlexibleTracks(aGridRI, aGridItems, aFunctions, freeSpace);
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::StateBitsForRange
    pub fn StateBitsForRange(&self, aRange: &LineRange) -> TrackSize_StateBits {
        MOZ_ASSERT!(!aRange.IsAuto(), "must have a definite range");
        let mut state = TrackSize_StateBits::default();
        for i in aRange.Range() {
            state |= self.mSizes[i as usize].mState;
        }
        state
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp AddSubgridContribution
// NOT PORTED: subgrid (only ResolveIntrinsicSize calls this, for an item that is a subgrid); the code
// is translated.
fn AddSubgridContribution(aSize: &mut TrackSize, aMarginBorderPadding: nscoord) {
    if (aSize.mState & TrackSize_StateBits::eIntrinsicMinSizing).to_bool() {
        aSize.mBase = std::cmp::max(aSize.mBase, aMarginBorderPadding);
        aSize.mLimit = std::cmp::max(aSize.mLimit, aSize.mBase);
    }
    // XXX maybe eFlexMaxSizing too?
    // (once we implement https://github.com/w3c/csswg-drafts/issues/2177)
    if (aSize.mState & (TrackSize_StateBits::eIntrinsicMaxSizing | TrackSize_StateBits::eApplyFitContentClamping)).to_bool() {
        aSize.mLimit = std::cmp::max(aSize.mLimit, aMarginBorderPadding);
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::ComputeMinSizeClamp
// (the overload with aState; the header's overload that computes the state is
// ComputeMinSizeClamp__TrackSizingFunctions_nscoord_LineRange in nsGridContainerFrame_P2.rs)
impl Tracks {
    pub fn ComputeMinSizeClamp__TrackSizingFunctions_nscoord_LineRange_StateBits(&self, aFunctions: &TrackSizingFunctions, aPercentageBasis: nscoord, aLineRange: &LineRange, aState: TrackSize_StateBits) -> Option<nscoord> {
        if !TrackSize::IsDefiniteMaxSizing(aState) {
            return None;
        }
        let mut minSizeClamp: nscoord = 0;
        for i in aLineRange.Range() {
            minSizeClamp += TrackBreadth_AsBreadth(aFunctions.MaxSizingFor(i)).Resolve(aPercentageBasis);
        }
        minSizeClamp += self.mGridGap * (aLineRange.Extent() as nscoord - 1);
        Some(minSizeClamp)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::ResolveIntrinsicSizeForNonSpanningItems
    // 
    pub fn ResolveIntrinsicSizeForNonSpanningItems(&mut self, aGridRI: &GridReflowInput, aFunctions: &TrackSizingFunctions, aPercentageBasis: nscoord, aConstraint: SizingConstraint, aRange: &LineRange, aGridItem: &GridItemInfo) {
        // Calculate track sizes for fit non-spanning items.
        // https://drafts.csswg.org/css-grid-2/#algo-single-span-items
        let mut cache = CachedIntrinsicSizes::new(aGridItem, aGridRI, self.mAxis);
        let szState = self.mSizes[aRange.mStart as usize].mState;

        // Contribution type to use as the base size.
        // This is a Maybe as we might not need to calculate a contribution at all,
        // for instance if the base sizing function is a definite length.
        let mut baseSizeType: Option<GridIntrinsicSizeType> = None;
        if (szState & TrackSize_StateBits::eAutoMinSizing).to_bool() {
            // "For auto minimums:"
            // "If the track has an 'auto' min track sizing function and the grid
            // container is being sized under a min-/max-content constraint, set
            // the track's base size to the maximum of its items' limited
            // min-content contributions"
            if aGridItem.MinContributionDependsOnAutoMinSize(aGridRI.mWM, self.mAxis) {
                // Clamp it if it's spanning a definite track max-sizing function.
                if let Some(minSizeClamp) = self.ComputeMinSizeClamp__TrackSizingFunctions_nscoord_LineRange(aFunctions, aPercentageBasis, aRange) {
                    cache.mMinSizeClamp = minSizeClamp;
                    aGridItem.mState[self.mAxis].set(aGridItem.mState[self.mAxis].get() | ItemState::eClampMarginBoxMinSize);
                }
                // Use the content-based contribution.
                baseSizeType = Some(if aConstraint == SizingConstraint::MaxContent { GridIntrinsicSizeType::MaxContentContribution } else { GridIntrinsicSizeType::MinContentContribution });
            } else {
                // Use the minimum contribution.
                // Note that this could still become MinContentContribution in practice.
                // MinContributionDependsOnAutoMinSize can return false when the item's
                // size depends on the size of its containing block. In that case, using
                // EnsureContributions to compute MinSize will instead compute
                // MinContentContribution, which will then be placed in the MinSize
                // slot on the cache.
                baseSizeType = Some(GridIntrinsicSizeType::MinContribution);
            }
        } else if (szState & TrackSize_StateBits::eMinContentMinSizing).to_bool() {
            // "For min-content minimums:"
            // "If the track has a 'min-content' min track sizing function, set its
            // base size to the maximum of the items' min-content contributions"
            baseSizeType = Some(GridIntrinsicSizeType::MinContentContribution);
        } else if (szState & TrackSize_StateBits::eMaxContentMinSizing).to_bool() {
            // "For max-content minimums:"
            // "If the track has a 'max-content' min track sizing function, set its
            // base size to the maximum of the items' max-content contributions"
            baseSizeType = Some(GridIntrinsicSizeType::MaxContentContribution);
        }

        // Size of fit-content maximum, if any.
        let mut fitContentClamp: Option<nscoord> = None;
        // Contribution type to use as the growth limit.
        // This is a Maybe as we might not need to calculate a contribution at all,
        // for instance if the growth limit sizing function is a definite length.
        let mut limitType: Option<GridIntrinsicSizeType> = None;
        if (szState & TrackSize_StateBits::eMinContentMaxSizing).to_bool() {
            // "For min-content maximums:"
            // "If the track has a 'min-content' max track sizing function, set its
            // growth limit to the maximum of the items' min-content contributions"
            limitType = Some(GridIntrinsicSizeType::MinContentContribution);
        } else if (szState & (TrackSize_StateBits::eAutoMaxSizing | TrackSize_StateBits::eMaxContentMaxSizing)).to_bool() {
            // "For max-content maximums:"
            // "If the track has a 'max-content' max track sizing function, set its
            // growth limit to the maximum of the items' max-content contributions"
            limitType = Some(GridIntrinsicSizeType::MaxContentContribution);
            if (szState & TrackSize_StateBits::eApplyFitContentClamping).to_bool() {
                // "For fit-content() maximums, furthermore clamp this growth limit by
                // the fit-content() argument."
                fitContentClamp = Some(TrackSize_AsFitContentBreadth(aFunctions.SizingFor(aRange.mStart)).Resolve(aPercentageBasis));
            }
        }

        // Even if it was possible to use the minimum contribution as the limit in
        // the spec, this could get trashed by the checks for whether the item's auto
        // minimum size depends on the size implemented in
        // GridItemInfo::MinContributionDependsOnAutoMinSize and
        // CachedIntrinsicSizes::EnsureContributions.
        MOZ_ASSERT!(limitType != Some(GridIntrinsicSizeType::MinContribution), "We should never be using the minimum contribution as the limit size.");

        // Accumulate the required size types and compute the contributions.
        {
            let mut sizeTypesToCalculate = GridIntrinsicSizeTypeSet::default();
            for maybeType in [baseSizeType, limitType] {
                if let Some(t) = maybeType {
                    sizeTypesToCalculate += t;
                }
            }
            cache.EnsureContributions(sizeTypesToCalculate, aGridItem, aGridRI, self.mAxis);
        }

        let sz = &mut self.mSizes[aRange.mStart as usize];
        if let Some(t) = baseSizeType {
            sz.mBase = std::cmp::max(sz.mBase, cache.mSizes[t as usize].unwrap());
        }

        // Limit based on max size type.
        if let Some(t) = limitType {
            if sz.mLimit == NS_UNCONSTRAINEDSIZE {
                sz.mLimit = 0; // Use only the contribution instead.
            }
            sz.mLimit = std::cmp::max(sz.mLimit, cache.mSizes[t as usize].unwrap());
            if let Some(clamp) = fitContentClamp {
                // "furthermore clamp this growth limit by the fit-content() argument."
                sz.mLimit = std::cmp::min(sz.mLimit, clamp);
            }
        }

        // "In all cases, if a track's growth limit is now less than its base size,
        // increase the growth limit to match the base size."
        sz.mLimit = std::cmp::max(sz.mLimit, sz.mBase);
    }
}

impl Tracks {
    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::CalculateItemBaselines
    // (ADAPTER: ItemBaselineData::mGridItem is an index into the grid items, so the grid items are a
    // parameter here; C++ reaches them through the pointer.)
    pub fn CalculateItemBaselines(&mut self, aBaselineItems: &mut Vec<ItemBaselineData>, aBaselineGroup: BaselineSharingGroup, aGridItems: &Vec<GridItemInfo>) {
        if aBaselineItems.is_empty() {
            return;
        }

        // Sort the collected items on their baseline track.
        aBaselineItems.sort_by(|a, b| a.mBaselineTrack.cmp(&b.mBaselineTrack));

        MOZ_ASSERT!(self.mSizes.Length() > 0, "having an item implies at least one track");

        // Make sure we have enough space to store the baselines. Use the highest
        // track number (+1 to account for the 0 based indexing).
        let baselineCount = aBaselineItems.last().unwrap().mBaselineTrack as usize + 1;
        if self.mBaselines.len() < baselineCount {
            self.mBaselines.resize_with(baselineCount, Default::default);
        }

        let mut maxBaseline: nscoord = 0;
        let mut maxDescent: nscoord = 0;
        let mut currentTrack: u32 = kAutoLine; // guaranteed to not match any item
        let mut trackStartIndex: usize = 0;
        let len = aBaselineItems.len();
        let mut i: usize = 0;
        loop {
            // Find the maximum baseline and descent in the current track.
            if i != len {
                let item = &aBaselineItems[i];
                if currentTrack == item.mBaselineTrack {
                    maxBaseline = std::cmp::max(maxBaseline, item.mBaseline);
                    maxDescent = std::cmp::max(maxDescent, item.mSize - item.mBaseline);
                    i += 1;
                    continue;
                }
            }
            // Iterate the current track again and update the baseline offsets making
            // all items baseline-aligned within this group in this track.
            for j in trackStartIndex..i {
                let item = &aBaselineItems[j];
                aGridItems[item.mGridItem].mBaselineOffset[self.mAxis].set(maxBaseline - item.mBaseline);
                MOZ_ASSERT!(aGridItems[item.mGridItem].mBaselineOffset[self.mAxis].get() >= 0);
            }
            if i != 0 {
                // Store the size of this baseline-aligned subtree.
                self.mSizes[currentTrack as usize].mBaselineSubtreeSize[aBaselineGroup] = maxBaseline + maxDescent;

                // Record the baseline for the current track.
                self.mBaselines[currentTrack as usize][aBaselineGroup] = Some(maxBaseline);
            }
            if i == len {
                break;
            }
            // Initialize data for the next track with baseline-aligned items.
            let item = &aBaselineItems[i];
            currentTrack = item.mBaselineTrack;
            trackStartIndex = i;
            maxBaseline = item.mBaseline;
            maxDescent = item.mSize - item.mBaseline;
            i += 1;
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::InitializeItemBaselines
    pub fn InitializeItemBaselines(&mut self, aGridRI: &GridReflowInput, aGridItems: &Vec<GridItemInfo>) {
        MOZ_ASSERT!(!self.mIsMasonry);
        if aGridRI.mFrame.IsSubgrid__LogicalAxis(self.mAxis) {
            // A grid container's subgridded axis doesn't have a baseline.
            return;
        }

        let mut firstBaselineItems: Vec<ItemBaselineData> = Vec::new();
        let mut lastBaselineItems: Vec<ItemBaselineData> = Vec::new();
        let containerWM = aGridRI.mWM;
        let containerStyle = aGridRI.mFrame.Style();

        for gridItemIndex in 0..aGridItems.len() {
            if aGridItems[gridItemIndex].IsSubgrid__LogicalAxis(self.mAxis) {
                // A subgrid itself is never baseline-aligned.
                continue;
            }

            let child = aGridItems[gridItemIndex].mFrame;
            let mut baselineTrack: u32 = kAutoLine;
            let mut state = ItemState::default();
            let childWM = child.GetWritingMode();

            let isOrthogonal = containerWM.IsOrthogonalTo(&childWM);
            let isInlineAxis = self.mAxis == LogicalAxis::Inline; // i.e. columns
            let itemHasBaselineParallelToTrack = isInlineAxis == isOrthogonal;

            // [align|justify]-self:[last ]baseline.
            let mut selfAlignment = UsedSelfAlignment(&child.StylePosition(), self.mAxis, Some(containerStyle.clone()));
            selfAlignment &= !StyleAlignFlags::FLAG_BITS;
            if selfAlignment == StyleAlignFlags::BASELINE {
                state |= ItemState::eFirstBaseline | ItemState::eSelfBaseline;
            } else if selfAlignment == StyleAlignFlags::LAST_BASELINE {
                state |= ItemState::eLastBaseline | ItemState::eSelfBaseline;
            }

            // https://drafts.csswg.org/css-align-3/#baseline-align-content
            // Baseline content-alignment can only apply if the align-content axis is
            // parallel with the box’s block axis; otherwise the fallback alignment is
            // used.
            if !isInlineAxis {
                // Handle align-content:[last ]baseline (if present)
                let mut alignContent = child.StylePosition().mAlignContent().primary();
                alignContent &= !StyleAlignFlags::FLAG_BITS;
                if alignContent == StyleAlignFlags::BASELINE || alignContent == StyleAlignFlags::LAST_BASELINE {
                    let selfAlignEdge = if alignContent == StyleAlignFlags::BASELINE { StyleAlignFlags::SELF_START } else { StyleAlignFlags::SELF_END };
                    let mut validCombo = selfAlignment == StyleAlignFlags::NORMAL || selfAlignment == StyleAlignFlags::STRETCH || selfAlignment == selfAlignEdge;
                    if !validCombo {
                        // We're doing alignment in the axis that's orthogonal to mAxis here.
                        let alignAxis = GetOrthogonalAxis(self.mAxis);
                        // |sameSide| is true if the container's start side in this axis is
                        // the same as the child's start side, in the child's parallel axis.
                        let sameSide = containerWM.ParallelAxisStartsOnSameSide(alignAxis, &childWM);
                        if selfAlignment == StyleAlignFlags::LEFT {
                            selfAlignment = if containerWM.IsBidiLTR() { StyleAlignFlags::START } else { StyleAlignFlags::END };
                        } else if selfAlignment == StyleAlignFlags::RIGHT {
                            selfAlignment = StyleAlignFlags::START;
                        }

                        if selfAlignment == StyleAlignFlags::START || selfAlignment == StyleAlignFlags::FLEX_START {
                            validCombo = sameSide == (alignContent == StyleAlignFlags::BASELINE);
                        } else if selfAlignment == StyleAlignFlags::END || selfAlignment == StyleAlignFlags::FLEX_END {
                            validCombo = sameSide == (alignContent == StyleAlignFlags::LAST_BASELINE);
                        }
                    }
                    if validCombo {
                        let area = &aGridItems[gridItemIndex].mArea;
                        if alignContent == StyleAlignFlags::BASELINE {
                            state |= ItemState::eFirstBaseline | ItemState::eContentBaseline;
                            baselineTrack = area.mRows.mStart;
                        } else if alignContent == StyleAlignFlags::LAST_BASELINE {
                            state |= ItemState::eLastBaseline | ItemState::eContentBaseline;
                            baselineTrack = area.mRows.mEnd - 1;
                        }
                    }
                }
            }

            if (state & ItemState::eIsBaselineAligned).to_bool() {
                // The item is baseline aligned, so calculate the baseline sharing group.
                // <https://drafts.csswg.org/css-align-3/#baseline-terms>
                let isFirstBaseline = (state & ItemState::eFirstBaseline).to_bool();
                let baselineAlignment = if isFirstBaseline { BaselineSharingGroup::First } else { BaselineSharingGroup::Last };
                // Baseline alignment occurs along `mAxis`, but baselines are defined in
                // the orthogonal axis (the axis of the baseline context that defines the
                // baseline sharing group).
                let baselineWM = WritingMode::DetermineWritingModeForBaselineSynthesis(&containerWM, &childWM, GetOrthogonalAxis(self.mAxis));

                let sameSideInBaselineWM = containerWM.ParallelAxisStartsOnSameSide(self.mAxis, &baselineWM);
                let mut baselineSharingGroup = BaselineSharingGroup::First;
                if sameSideInBaselineWM != isFirstBaseline {
                    baselineSharingGroup = BaselineSharingGroup::Last;
                    state |= ItemState::eLastBaselineSharingGroup;

                    baselineTrack = (if isInlineAxis { aGridItems[gridItemIndex].mArea.mCols.mEnd } else { aGridItems[gridItemIndex].mArea.mRows.mEnd }) - 1;
                } else {
                    baselineTrack = if isInlineAxis { aGridItems[gridItemIndex].mArea.mCols.mStart } else { aGridItems[gridItemIndex].mArea.mRows.mStart };
                }

                // XXXmats if |child| is a descendant of a subgrid then the metrics
                // below needs to account for the accumulated MPB somehow...

                let rc = aGridRI.mRenderingContext;
                // XXX figure out if we can avoid/merge this reflow with the main reflow.
                // XXX (after bug 1174569 is sorted out)
                //
                // XXX How should we handle percentage padding here? (bug 1330866)
                // XXX (see ::ContentContribution and how it deals with percentages)
                // XXX What if the true baseline after line-breaking differs from this
                // XXX hypothetical baseline based on an infinite inline size?
                // XXX Maybe we should just call ::ContentContribution here instead?
                let cbSize = aGridRI.PercentageBasisFor(self.mAxis, &aGridItems[gridItemIndex]);
                let mut avail = LogicalSize::new(childWM, INFINITE_ISIZE_COORD, NS_UNCONSTRAINEDSIZE);
                let inlineAxisInChildWM = if isOrthogonal { LogicalAxis::Block } else { LogicalAxis::Inline };
                let colSize = cbSize.Size(inlineAxisInChildWM, childWM);
                if colSize != NS_UNCONSTRAINEDSIZE {
                    *avail.Size_mut(inlineAxisInChildWM, childWM) = colSize;
                }
                MeasuringReflow(child, aGridRI.mReflowInput, Some(rc), avail, cbSize, NS_MAXSIZE, NS_MAXSIZE);

                let grid = nsGridContainerFrame::do_QueryFrame(child);
                let frameSize = if isInlineAxis { child.ISize(containerWM) } else { child.BSize(containerWM) };
                let margin = child.GetLogicalUsedMargin(containerWM);
                let alignSize = frameSize + (if isInlineAxis { margin.IStartEnd(containerWM) } else { margin.BStartEnd(containerWM) });

                let mut baseline: Option<nscoord> = None;
                if let Some(grid) = grid {
                    baseline = Some(if isOrthogonal == isInlineAxis { grid.GetBBaseline(baselineAlignment) } else { grid.GetIBaseline(baselineAlignment) });
                } else {
                    if itemHasBaselineParallelToTrack {
                        baseline = child.GetNaturalBaselineBOffset(childWM, baselineAlignment, BaselineExportContext::Other);
                    }

                    if baseline.is_none() {
                        // If baseline alignment is specified on a grid item whose size in
                        // that axis depends on the size of an intrinsically-sized track, that
                        // item does not participate in baseline alignment, and instead uses
                        // its fallback alignment as if that were originally specified.
                        // https://drafts.csswg.org/css-grid-2/#row-align

                        // Check if the item crosses any tracks that are intrinsically sized.
                        let range = aGridItems[gridItemIndex].mArea.LineRangeForAxis(self.mAxis).Range();
                        let isTrackAutoSize = range.into_iter().any(|track| {
                            let intrinsicSizeFlags = TrackSize_StateBits::eIntrinsicMinSizing | TrackSize_StateBits::eIntrinsicMaxSizing | TrackSize_StateBits::eApplyFitContentClamping | TrackSize_StateBits::eFlexMaxSizing;
                            (self.mSizes[track as usize].mState & intrinsicSizeFlags).to_bool()
                        });

                        // If either the track or the item is not auto sized, then the item
                        // participates in baseline alignment.
                        if !isTrackAutoSize || !aGridItems[gridItemIndex].IsBSizeDependentOnContainerSize(containerWM) {
                            // We're synthesizing the baseline from the child's border-box
                            // (frameSize is the size of the border-box). See:
                            // https://drafts.csswg.org/css-align-3/#baseline-export.

                            if containerWM.IsCentralBaseline() {
                                // We want to use the exact same central position within the
                                // frame, regardless of which side we're measuring from. To
                                // achieve that, we round *up* if we're in the first baseline
                                // sharing group, and *down* if we're in the last baseline sharing
                                // group.
                                let isFirstBaselineSharingGroup = baselineSharingGroup == BaselineSharingGroup::First;
                                baseline = Some(frameSize / 2 + (if isFirstBaselineSharingGroup { 0 } else { frameSize % 2 }));
                            } else {
                                // The baseline offset is measured from the block-{start,end} edge
                                // of the container, using the block axis of 'baselineWM' (which
                                // may differ from the child or container’s writing mode).
                                //
                                // If we're synthesizing a baseline from the edge nearest to the
                                // container's reference side (start for the first baseline group,
                                // end for the last), the offset is `0`. Otherwise, it's from the
                                // opposite edge, so we use `frameSize`.
                                //
                                // This logic depends on whether we're in the first or last
                                // baseline-sharing group, and whether the line is inverted (e.g.,
                                // in vertical-rl mode), which affects which edge is considered
                                // the "start" or "end".
                                baseline = Some(if isFirstBaseline == baselineWM.IsLineInverted() { 0 } else { frameSize });
                            }
                        }
                    }
                }

                if let Some(baseline) = baseline {
                    let mut finalBaseline = baseline;
                    NS_ASSERTION!(finalBaseline != NS_INTRINSIC_ISIZE_UNKNOWN, "about to use an unknown baseline");

                    let marginAdjust: nscoord;
                    if baselineSharingGroup == BaselineSharingGroup::First {
                        marginAdjust = if isInlineAxis { margin.IStart(containerWM) } else { margin.BStart(containerWM) };
                    } else {
                        marginAdjust = if isInlineAxis { margin.IEnd(containerWM) } else { margin.BEnd(containerWM) };

                        // This flag is used in ::AlignSelf(...) to check whether the item is
                        // last baseline aligned, but this flag should go away.
                        state |= ItemState::eEndSideBaseline;
                    }
                    finalBaseline += marginAdjust;

                    let baselineItems = if baselineSharingGroup == BaselineSharingGroup::First { &mut firstBaselineItems } else { &mut lastBaselineItems };
                    // (mGridItem is an index into aGridItems)
                    baselineItems.push(ItemBaselineData { mBaselineTrack: baselineTrack, mBaseline: finalBaseline, mSize: alignSize, mGridItem: gridItemIndex });
                } else {
                    state &= !ItemState::eAllBaselineBits;
                }
            }

            MOZ_ASSERT!((state & (ItemState::eFirstBaseline | ItemState::eLastBaseline)) != (ItemState::eFirstBaseline | ItemState::eLastBaseline), "first/last baseline bits are mutually exclusive");
            MOZ_ASSERT!((state & (ItemState::eSelfBaseline | ItemState::eContentBaseline)) != (ItemState::eSelfBaseline | ItemState::eContentBaseline), "*-self and *-content baseline bits are mutually exclusive");
            MOZ_ASSERT!(!(state & (ItemState::eFirstBaseline | ItemState::eLastBaseline)).to_bool() == !(state & (ItemState::eSelfBaseline | ItemState::eContentBaseline)).to_bool(), "first/last bit requires self/content bit and vice versa");

            aGridItems[gridItemIndex].mState[self.mAxis].set(aGridItems[gridItemIndex].mState[self.mAxis].get() | state);
            aGridItems[gridItemIndex].mBaselineOffset[self.mAxis].set(0);
        }

        if firstBaselineItems.is_empty() && lastBaselineItems.is_empty() {
            return;
        }

        // TODO: CSS Align spec issue - how to align a baseline subtree in a track?
        // https://lists.w3.org/Archives/Public/www-style/2016May/0141.html
        self.mBaselineSubtreeAlign[BaselineSharingGroup::First] = StyleAlignFlags::START;
        self.mBaselineSubtreeAlign[BaselineSharingGroup::Last] = StyleAlignFlags::END;

        self.CalculateItemBaselines(&mut firstBaselineItems, BaselineSharingGroup::First, aGridItems);
        self.CalculateItemBaselines(&mut lastBaselineItems, BaselineSharingGroup::Last, aGridItems);
    }

    // TODO: we store the wrong baseline group offset in some cases (bug 1632200)
    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::InitializeItemBaselinesInMasonryAxis
    // NOT PORTED: masonry layout (nsGridContainerFrame.cpp lines 6759-6953 at GECKO_REV; only a masonry
    // axis calls this, and IsMasonry() is always false in this runtime).
    pub fn InitializeItemBaselinesInMasonryAxis(&mut self, aGridRI: &GridReflowInput, aGridItems: &Vec<GridItemInfo>, aSet: BaselineAlignmentSet, aContainerSize: nsSize, aTrackSizes: &mut Vec<nscoord>, aFirstBaselineItems: &mut Vec<ItemBaselineData>, aLastBaselineItems: &mut Vec<ItemBaselineData>) {
        unimplemented!("NOT PORTED: masonry layout (InitializeItemBaselinesInMasonryAxis)")
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::AlignBaselineSubtree
    // 
    pub fn AlignBaselineSubtree(&self, aGridItem: &GridItemInfo) {
        if self.mIsMasonry {
            return;
        }
        let state = aGridItem.mState[self.mAxis].get();
        if !(state & ItemState::eIsBaselineAligned).to_bool() {
            return;
        }
        let area = &aGridItem.mArea;
        let baselineTrack: i32;
        let isFirstBaseline = (state & ItemState::eFirstBaseline).to_bool();
        if isFirstBaseline {
            baselineTrack = (if self.mAxis == LogicalAxis::Block { area.mRows.mStart } else { area.mCols.mStart }) as i32;
        } else {
            baselineTrack = (if self.mAxis == LogicalAxis::Block { area.mRows.mEnd } else { area.mCols.mEnd }) as i32 - 1;
        }
        let sz = &self.mSizes[baselineTrack as usize];
        let baselineGroup = if isFirstBaseline { BaselineSharingGroup::First } else { BaselineSharingGroup::Last };
        let delta = sz.mBase - sz.mBaselineSubtreeSize[baselineGroup];
        let subtreeAlign = self.mBaselineSubtreeAlign[baselineGroup];
        if subtreeAlign == StyleAlignFlags::START {
            if (state & ItemState::eLastBaseline).to_bool() {
                aGridItem.mBaselineOffset[self.mAxis].set(aGridItem.mBaselineOffset[self.mAxis].get() + delta);
            }
        } else if subtreeAlign == StyleAlignFlags::END {
            if isFirstBaseline {
                aGridItem.mBaselineOffset[self.mAxis].set(aGridItem.mBaselineOffset[self.mAxis].get() + delta);
            }
        } else if subtreeAlign == StyleAlignFlags::CENTER {
            aGridItem.mBaselineOffset[self.mAxis].set(aGridItem.mBaselineOffset[self.mAxis].get() + delta / 2);
        } else {
            MOZ_ASSERT_UNREACHABLE!("unexpected baseline subtree alignment");
        }
    }
}

impl Tracks {
    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::GrowSizeForSpanningItems
    // (ADAPTER: the iterator pair aIter, aIterEnd is the slice aItems; no default arguments: pass `None` and
    // `false` for aFitContentClamper and aNeedInfinitelyGrowableFlag.)
    pub fn GrowSizeForSpanningItems(&mut self, aStep: TrackSizingStep, aPhase: TrackSizingPhase, aItems: &[SpanningItemData], aTracks: &mut Vec<u32>, aTrackPlan: &mut TrackPlan, aItemPlan: &mut ItemPlan, aConstraint: SizingConstraint, aIsGridIntrinsicSizing: bool, aFunctions: &TrackSizingFunctions, aFitContentClamper: Option<&Tracks_FitContentClamper>, aNeedInfinitelyGrowableFlag: bool) -> bool {
        let isMaxSizingPhase = aPhase == TrackSizingPhase::IntrinsicMaximums || aPhase == TrackSizingPhase::MaxContentMaximums;
        let mut needToUpdateSizes = false;
        aTrackPlan.Initialize(aPhase, self);
        for item in aItems {
            if !(item.mState & Tracks::SelectorForPhase(aPhase, aConstraint)).to_bool() {
                continue;
            }
            if isMaxSizingPhase {
                for i in item.mLineRange.Range() {
                    aTrackPlan[i as usize].mState |= TrackSize_StateBits::eModified;
                }
            }
            if aStep == TrackSizingStep::Flex && aIsGridIntrinsicSizing {
                // We could only ever grow flex tracks, and when measuring we shouldn't
                // grow flex tracks, so the remaining space will always be zero.
                continue;
            }
            let mut space = item.SizeContributionForPhase(aPhase);
            if space <= 0 {
                continue;
            }
            aTracks.clear();
            space = self.CollectGrowable(aStep, aPhase, space, &item.mLineRange, aConstraint, aTracks);
            if space > 0 {
                self.DistributeToTrackSizes(aStep, aPhase, space, aTrackPlan, aItemPlan, aTracks, aConstraint, aFunctions, aFitContentClamper);
                needToUpdateSizes = true;
            }
        }
        if isMaxSizingPhase {
            needToUpdateSizes = true;
        }
        if needToUpdateSizes {
            self.CopyPlanToSize(aPhase, aTrackPlan, aNeedInfinitelyGrowableFlag);
        }
        needToUpdateSizes
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::ResolveIntrinsicSize
    pub fn ResolveIntrinsicSize(&mut self, aGridRI: &GridReflowInput, aGridItems: &Vec<GridItemInfo>, aFunctions: &TrackSizingFunctions, aRange: LineRangeSelector, aPercentageBasis: nscoord, aConstraint: SizingConstraint) {
        // Intrinsic sizing algorithm 12.5 steps 2-5
        // https://drafts.csswg.org/css-grid-2/#algo-content
        //
        // We're also setting eIsFlexing on the item state here to speed up
        // FindUsedFlexFraction later.

        // nonFlexSpanningItems has spanning items that do not span any flex tracks.
        // flexSpanningItems has spanning items that span one or more flex tracks.
        let mut nonFlexSpanningItems: Vec<SpanningItemData> = Vec::new();
        let mut flexSpanningItems: Vec<SpanningItemData> = Vec::new();
        // max span of items in `nonFlexSpanningItems` and `flexSpanningItems`.
        let mut maxSpan: u32 = 0;

        let orthogonalAxis = GetOrthogonalAxis(self.mAxis);
        let isMasonryInOtherAxis = aGridRI.mFrame.IsMasonry__LogicalAxis(orthogonalAxis);

        for gridItem in aGridItems.iter() {
            MOZ_ASSERT!(!(gridItem.mState[self.mAxis].get() & (ItemState::eContentBasedAutoMinSize | ItemState::eIsFlexing | ItemState::eClampMarginBoxMinSize)).to_bool(), "Why are any of these bits set already?");

            let lineRange = aRange(&gridItem.mArea).clone();
            let state = self.StateBitsForRange(&lineRange);
            // Set flex sizing flag as soon as possible to ensure
            // MinContributionDependsOnAutoMinSize will function properly.
            if (state & TrackSize_StateBits::eFlexMaxSizing).to_bool() {
                gridItem.mState[self.mAxis].set(gridItem.mState[self.mAxis].get() | ItemState::eIsFlexing);
            }

            // If we have masonry layout in the other axis then skip this item unless
            // it's in the first masonry track, or has definite placement in this axis,
            // or spans all tracks in this axis (since that implies it will be placed
            // at line 1 regardless of layout results of other items).
            if isMasonryInOtherAxis && gridItem.mArea.LineRangeForAxis(orthogonalAxis).mStart != 0 && (gridItem.mState[self.mAxis].get() & ItemState::eAutoPlacement).to_bool() && gridItem.mArea.LineRangeForAxis(self.mAxis).Extent() as usize != self.mSizes.Length() {
                continue;
            }

            let span = lineRange.Extent();
            if (gridItem.mState[self.mAxis].get() & ItemState::eIsSubgrid).to_bool() {
                // NOT PORTED: subgrid (nsGridContainerFrame.cpp lines 7091-7115 at GECKO_REV: the item's
                // margin+border+padding is added to the start and end tracks with AddSubgridContribution).
                unimplemented!("NOT PORTED: subgrid (a grid item that is a subgrid)");
            }

            // Set eContentBasedAutoMinSize if and only if the grid item has
            // content-based automatic minimum size. This is the case if all of the
            // following are true of the item:
            // 1. its computed overflow is not a scrollable overflow value
            // 2. it spans at least one track in that axis whose min track sizing
            // function is auto
            // 3. if it spans more than one track in that axis, none of those tracks
            // are flexible
            // https://drafts.csswg.org/css-grid-2/#min-size-auto
            if !gridItem.mFrame.StyleDisplay().IsScrollableOverflow() && (state & TrackSize_StateBits::eAutoMinSizing).to_bool() && (span == 1 || !(state & TrackSize_StateBits::eFlexMaxSizing).to_bool()) {
                gridItem.mState[self.mAxis].set(gridItem.mState[self.mAxis].get() | ItemState::eContentBasedAutoMinSize);
            }

            if span == 1 {
                // Step 2. Size tracks to fit non-spanning items.
                // https://drafts.csswg.org/css-grid-2/#algo-single-span-items
                self.ResolveIntrinsicSizeForNonSpanningItems(aGridRI, aFunctions, aPercentageBasis, aConstraint, &lineRange, gridItem);
            } else {
                // Collect information for step 3.
                // https://drafts.csswg.org/css-grid-2/#algo-spanning-items

                let items: &mut Vec<SpanningItemData>;
                if (state & TrackSize_StateBits::eFlexMaxSizing).to_bool() {
                    // Set eIsFlexing on the item state here to speed up
                    // FindUsedFlexFraction later.
                    gridItem.mState[self.mAxis].set(gridItem.mState[self.mAxis].get() | ItemState::eIsFlexing);
                    items = &mut flexSpanningItems;
                } else {
                    items = &mut nonFlexSpanningItems;
                }

                if (state & (TrackSize_StateBits::eIntrinsicMinSizing | TrackSize_StateBits::eIntrinsicMaxSizing)).to_bool() {
                    maxSpan = std::cmp::max(maxSpan, span);
                    let mut cache = CachedIntrinsicSizes::new(gridItem, aGridRI, self.mAxis);

                    // Calculate data for "Automatic Minimum Size" clamping, if needed.
                    if (gridItem.mState[self.mAxis].get() & ItemState::eContentBasedAutoMinSize).to_bool() {
                        if let Some(minSizeClamp) = self.ComputeMinSizeClamp__TrackSizingFunctions_nscoord_LineRange_StateBits(aFunctions, aPercentageBasis, &lineRange, state) {
                            cache.mMinSizeClamp = minSizeClamp;
                            gridItem.mState[self.mAxis].set(gridItem.mState[self.mAxis].get() | ItemState::eClampMarginBoxMinSize);
                        }
                    }

                    // Collect the various grid item size contributions we need.
                    let mut sizeTypesToCalculate = GridIntrinsicSizeTypeSet::default();
                    // For 3.1
                    let mut selector = Tracks::SelectorForPhase(TrackSizingPhase::IntrinsicMinimums, aConstraint);

                    if (state & selector).to_bool() {
                        sizeTypesToCalculate += GridIntrinsicSizeType::MinContribution;
                    }

                    // For 3.2 and 3.5
                    selector = Tracks::SelectorForPhase(TrackSizingPhase::IntrinsicMaximums, aConstraint) | Tracks::SelectorForPhase(TrackSizingPhase::ContentBasedMinimums, aConstraint);
                    if (state & selector).to_bool() {
                        sizeTypesToCalculate += GridIntrinsicSizeType::MinContentContribution;
                    }

                    // For 3.3 and 3.6
                    selector = Tracks::SelectorForPhase(TrackSizingPhase::MaxContentMinimums, aConstraint) | Tracks::SelectorForPhase(TrackSizingPhase::MaxContentMaximums, aConstraint);
                    if (state & selector).to_bool() {
                        sizeTypesToCalculate += GridIntrinsicSizeType::MaxContentContribution;
                    }

                    cache.EnsureContributions(sizeTypesToCalculate, gridItem, aGridRI, self.mAxis);
                    items.push(SpanningItemData { mSpan: span, mState: state, mLineRange: lineRange.clone(), mSizes: Tracks_SpanningItemData_Sizes(cache.SizesOrDefault()), mFrame: gridItem.mFrame });
                }
            }

            MOZ_ASSERT!(!(gridItem.mState[self.mAxis].get() & ItemState::eClampMarginBoxMinSize).to_bool() || (gridItem.mState[self.mAxis].get() & ItemState::eContentBasedAutoMinSize).to_bool(), "clamping only applies to Automatic Minimum Size");
        }

        MOZ_ASSERT!(maxSpan != 1, "Should only count spans greater than 1");
        // Step 3 - Increase sizes to accommodate spanning items crossing
        // content-sized tracks.
        if maxSpan != 0 {
            // ADAPTER: the C++ lambda captures aFunctions and aPercentageBasis by reference; Tracks_FitContentClamper is
            // a `dyn Fn + 'static` here, so the limits of the tracks that have a definite max sizing function are
            // computed up front (ResolveToDefiniteSize has the same result when the lambda calls it later).
            let fitContentLimits: Vec<Option<nscoord>> = (0..self.mSizes.Length() as u32).map(|i| if aFunctions.MaxSizingFor(i).IsBreadth() { Some(ResolveToDefiniteSize(aFunctions.MaxSizingFor(i), aPercentageBasis)) } else { None }).collect();
            let fitContentClamper = move |aTrack: u32, aMinSize: nscoord, aSize: &mut nscoord| -> bool {
                let fitContentLimit = fitContentLimits[aTrack as usize].unwrap();
                if *aSize > fitContentLimit {
                    *aSize = std::cmp::max(aMinSize, fitContentLimit);
                    return true;
                }
                false
            };

            // Step 3 should "Repeat incrementally for items with greater spans until
            // all items have been considered."
            // Sort the collected items on span length, shortest first. There's no need
            // for a stable sort here since the sizing isn't order dependent within
            // a group of items with the same span length.
            // We don't need to sort flexSpanningItems, those items are all considered
            // "together, rather than grouped by span size" for step 4.
            nonFlexSpanningItems.sort_by(|a, b| a.mSpan.cmp(&b.mSpan));

            let mut tracks: Vec<u32> = Vec::with_capacity(maxSpan as usize);
            let mut plan = TrackPlan::new__size_t(self.mSizes.Length());
            plan.SetLength(self.mSizes.Length());
            let mut itemPlan = ItemPlan::new__size_t(self.mSizes.Length());
            itemPlan.SetLength(self.mSizes.Length());

            // Start / end iterator for items of the same span length:
            let mut spanGroupStart: usize = 0;
            let mut spanGroupEnd: usize = spanGroupStart;
            let end: usize = nonFlexSpanningItems.len();

            // nonFlexSpanningItems is sorted by span size. Each iteration will process
            // one span size.
            while spanGroupStart != end {
                let span = nonFlexSpanningItems[spanGroupStart].mSpan;
                let mut stateBitsForSpan = TrackSize_StateBits::default();
                MOZ_ASSERT!(spanGroupEnd == spanGroupStart);
                // Find the end of this group if items with the same span size.
                // Accumulate state bits for the items with this span size to avoid
                // calculations below that are not applicable to any of those items.
                loop {
                    stateBitsForSpan |= self.StateBitsForRange(&nonFlexSpanningItems[spanGroupEnd].mLineRange);
                    spanGroupEnd += 1;
                    if !(spanGroupEnd != end && nonFlexSpanningItems[spanGroupEnd].mSpan == span) {
                        break;
                    }
                }
                MOZ_ASSERT!(!(stateBitsForSpan & TrackSize_StateBits::eFlexMaxSizing).to_bool(), "Non-flex spanning items should not include any flex tracks");
                let mut updatedBase = false; // Did we update any mBase in step 3.1..3.3?
                let mut phase = TrackSizingPhase::IntrinsicMinimums;
                if (stateBitsForSpan & Tracks::SelectorForPhase(phase, aConstraint)).to_bool() {
                    // Step 3.1 MinSize to intrinsic min-sizing.
                    updatedBase = self.GrowSizeForSpanningItems(TrackSizingStep::NotFlex, phase, &nonFlexSpanningItems[spanGroupStart..spanGroupEnd], &mut tracks, &mut plan, &mut itemPlan, aConstraint, aGridRI.mIsGridIntrinsicSizing, aFunctions, None, false);
                }

                phase = TrackSizingPhase::ContentBasedMinimums;
                if (stateBitsForSpan & Tracks::SelectorForPhase(phase, aConstraint)).to_bool() {
                    // Step 3.2 MinContentContribution to min-/max-content (and 'auto' when
                    // sizing under a min-content constraint) min-sizing.
                    updatedBase |= self.GrowSizeForSpanningItems(TrackSizingStep::NotFlex, phase, &nonFlexSpanningItems[spanGroupStart..spanGroupEnd], &mut tracks, &mut plan, &mut itemPlan, aConstraint, aGridRI.mIsGridIntrinsicSizing, aFunctions, None, false);
                }

                phase = TrackSizingPhase::MaxContentMinimums;
                if (stateBitsForSpan & Tracks::SelectorForPhase(phase, aConstraint)).to_bool() {
                    // Step 3.3 MaxContentContribution to max-content (and 'auto' when
                    // sizing under a max-content constraint) min-sizing.
                    updatedBase |= self.GrowSizeForSpanningItems(TrackSizingStep::NotFlex, phase, &nonFlexSpanningItems[spanGroupStart..spanGroupEnd], &mut tracks, &mut plan, &mut itemPlan, aConstraint, aGridRI.mIsGridIntrinsicSizing, aFunctions, None, false);
                }

                if updatedBase {
                    // Step 3.4
                    for sz in self.mSizes.iter_mut() {
                        if sz.mBase > sz.mLimit {
                            sz.mLimit = sz.mBase;
                        }
                    }
                }

                phase = TrackSizingPhase::IntrinsicMaximums;
                let mut willRunStep3_6 = false;
                if (stateBitsForSpan & Tracks::SelectorForPhase(phase, aConstraint)).to_bool() {
                    willRunStep3_6 = (stateBitsForSpan & TrackSize_StateBits::eAutoOrMaxContentMaxSizing).to_bool();
                    // Step 3.5 MinContentContribution to intrinsic max-sizing.
                    self.GrowSizeForSpanningItems(TrackSizingStep::NotFlex, phase, &nonFlexSpanningItems[spanGroupStart..spanGroupEnd], &mut tracks, &mut plan, &mut itemPlan, aConstraint, aGridRI.mIsGridIntrinsicSizing, aFunctions, Some(&fitContentClamper), willRunStep3_6);
                }
                if willRunStep3_6 {
                    // Step 2.6 MaxContentContribution to max-content max-sizing.
                    phase = TrackSizingPhase::MaxContentMaximums;
                    self.GrowSizeForSpanningItems(TrackSizingStep::NotFlex, phase, &nonFlexSpanningItems[spanGroupStart..spanGroupEnd], &mut tracks, &mut plan, &mut itemPlan, aConstraint, aGridRI.mIsGridIntrinsicSizing, aFunctions, Some(&fitContentClamper), false);
                }

                spanGroupStart = spanGroupEnd;
            }

            // Step 4
            let mut stateBitsForSpan = TrackSize_StateBits::default();
            for spanningData in flexSpanningItems.iter() {
                let bits = self.StateBitsForRange(&spanningData.mLineRange);
                MOZ_ASSERT!((bits & TrackSize_StateBits::eFlexMaxSizing).to_bool(), "All flex spanning items should have at least one flex track");
                stateBitsForSpan |= bits;
            }
            let mut updatedBase = false; // Did we update any mBase in step 4.1..4.3?
            let mut phase = TrackSizingPhase::IntrinsicMinimums;
            if (stateBitsForSpan & Tracks::SelectorForPhase(phase, aConstraint)).to_bool() {
                // Step 4.1 MinSize to intrinsic min-sizing.
                updatedBase = self.GrowSizeForSpanningItems(TrackSizingStep::Flex, phase, &flexSpanningItems[..], &mut tracks, &mut plan, &mut itemPlan, aConstraint, aGridRI.mIsGridIntrinsicSizing, aFunctions, None, false);
            }

            phase = TrackSizingPhase::ContentBasedMinimums;
            if (stateBitsForSpan & Tracks::SelectorForPhase(phase, aConstraint)).to_bool() {
                // Step 4.2 MinContentContribution to min-/max-content (and 'auto' when
                // sizing under a min-content constraint) min-sizing.
                updatedBase |= self.GrowSizeForSpanningItems(TrackSizingStep::Flex, phase, &flexSpanningItems[..], &mut tracks, &mut plan, &mut itemPlan, aConstraint, aGridRI.mIsGridIntrinsicSizing, aFunctions, None, false);
            }

            phase = TrackSizingPhase::MaxContentMinimums;
            if (stateBitsForSpan & Tracks::SelectorForPhase(phase, aConstraint)).to_bool() {
                // Step 4.3 MaxContentContribution to max-content (and 'auto' when
                // sizing under a max-content constraint) min-sizing.
                updatedBase |= self.GrowSizeForSpanningItems(TrackSizingStep::Flex, phase, &flexSpanningItems[..], &mut tracks, &mut plan, &mut itemPlan, aConstraint, aGridRI.mIsGridIntrinsicSizing, aFunctions, None, false);
            }

            if updatedBase {
                // Step 4.4
                for sz in self.mSizes.iter_mut() {
                    if sz.mBase > sz.mLimit {
                        sz.mLimit = sz.mBase;
                    }
                }
            }
        }

        // Step 5 - If any track still has an infinite growth limit, set its growth
        // limit to its base size.
        for sz in self.mSizes.iter_mut() {
            if sz.mLimit == NS_UNCONSTRAINEDSIZE {
                sz.mLimit = sz.mBase;
            }
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::FindFrUnitSize
    pub fn FindFrUnitSize(&self, aRange: &LineRange, aFlexTracks: &Vec<u32>, aFunctions: &TrackSizingFunctions, aSpaceToFill: nscoord) -> f32 {
        MOZ_ASSERT!(aSpaceToFill > 0 && !aFlexTracks.is_empty());
        let mut flexFactorSum: f32 = 0.0;
        let mut leftOverSpace = aSpaceToFill;
        for i in aRange.Range() {
            let sz = &self.mSizes[i as usize];
            if (sz.mState & TrackSize_StateBits::eFlexMaxSizing).to_bool() {
                flexFactorSum += TrackBreadth_AsFlex(aFunctions.MaxSizingFor(i));
            } else {
                leftOverSpace -= sz.mBase;
                if leftOverSpace <= 0 {
                    return 0.0;
                }
            }
        }
        let mut restart: bool;
        let mut hypotheticalFrSize: f32;
        let mut flexTracks: Vec<u32> = aFlexTracks.clone();
        let mut numFlexTracks = flexTracks.len() as u32;
        loop {
            restart = false;

            // 12.7.1.2: If flexFactorSum is less than 1, set it to 1 instead.
            hypotheticalFrSize = leftOverSpace as f32 / f32::max(flexFactorSum, 1.0);
            for track in flexTracks.iter_mut() {
                if *track == kAutoLine {
                    continue; // Track marked as inflexible in a prev. iter of this loop.
                }
                let flexFactor = TrackBreadth_AsFlex(aFunctions.MaxSizingFor(*track));
                let base = self.mSizes[*track as usize].mBase;
                if flexFactor * hypotheticalFrSize < base as f32 {
                    // 12.7.1.4: Treat this track as inflexible.
                    *track = kAutoLine;
                    flexFactorSum -= flexFactor;
                    leftOverSpace -= base;
                    numFlexTracks -= 1;
                    if numFlexTracks == 0 || leftOverSpace <= 0 {
                        return 0.0;
                    }
                    restart = true;
                    // break; XXX (bug 1176621 comment 16) measure which is more common
                }
            }
            if !restart {
                break;
            }
        }
        hypotheticalFrSize
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::FindUsedFlexFraction
    pub fn FindUsedFlexFraction(&self, aGridRI: &GridReflowInput, aGridItems: &Vec<GridItemInfo>, aFlexTracks: &Vec<u32>, aFunctions: &TrackSizingFunctions, aAvailableSize: nscoord) -> f32 {
        if aAvailableSize != NS_UNCONSTRAINEDSIZE {
            // Use all of the grid tracks and a 'space to fill' of the available space.
            let range = TranslatedLineRange::new(0, self.mSizes.Length() as u32);
            return self.FindFrUnitSize(&range, aFlexTracks, aFunctions, aAvailableSize);
        }

        // The used flex fraction is the maximum of:
        // ... each flexible track's base size divided by its flex factor (which is
        // floored at 1).
        let mut fr: f32 = 0.0;
        for &track in aFlexTracks.iter() {
            let flexFactor = TrackBreadth_AsFlex(aFunctions.MaxSizingFor(track));
            let possiblyDividedBaseSize = if flexFactor > 1.0 { self.mSizes[track as usize].mBase as f32 / flexFactor } else { self.mSizes[track as usize].mBase as f32 };
            fr = f32::max(fr, possiblyDividedBaseSize);
        }
        // ... the result of 'finding the size of an fr' for each item that spans
        // a flex track with its max-content contribution as 'space to fill'
        for item in aGridItems.iter() {
            if (item.mState[self.mAxis].get() & ItemState::eIsFlexing).to_bool() {
                // XXX optimize: bug 1194446
                let percentageBasis = aGridRI.PercentageBasisFor(self.mAxis, item);
                let mut spaceToFill = ContentContribution(item, aGridRI, self.mAxis, percentageBasis, IntrinsicISizeType::PrefISize, NS_MAXSIZE, &StyleSizeOverrides::default());
                let range = if self.mAxis == LogicalAxis::Inline { &item.mArea.mCols } else { &item.mArea.mRows };
                MOZ_ASSERT!(range.Extent() >= 1);
                let spannedGaps = range.Extent() - 1;
                if spannedGaps > 0 {
                    spaceToFill -= self.mGridGap * spannedGaps as nscoord;
                }
                if spaceToFill <= 0 {
                    continue;
                }
                // ... and all its spanned tracks as input.
                let mut itemFlexTracks: Vec<u32> = Vec::new();
                for i in range.Range() {
                    if (self.mSizes[i as usize].mState & TrackSize_StateBits::eFlexMaxSizing).to_bool() {
                        itemFlexTracks.push(i);
                    }
                }
                let itemFr = self.FindFrUnitSize(range, &itemFlexTracks, aFunctions, spaceToFill);
                fr = f32::max(fr, itemFr);
            }
        }
        fr
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::StretchFlexibleTracks
    pub fn StretchFlexibleTracks(&mut self, aGridRI: &GridReflowInput, aGridItems: &Vec<GridItemInfo>, aFunctions: &TrackSizingFunctions, mut aAvailableSize: nscoord) {
        if aAvailableSize <= 0 {
            return;
        }
        let mut flexTracks: Vec<u32> = Vec::with_capacity(self.mSizes.Length());
        for i in 0..self.mSizes.Length() as u32 {
            if (self.mSizes[i as usize].mState & TrackSize_StateBits::eFlexMaxSizing).to_bool() {
                flexTracks.push(i);
            }
        }
        if flexTracks.is_empty() {
            return;
        }
        let mut minSize: nscoord = 0;
        let mut maxSize: nscoord = NS_UNCONSTRAINEDSIZE;
        if let Some(ri) = aGridRI.mReflowInput {
            minSize = if self.mAxis == LogicalAxis::Block { ri.ComputedMinBSize() } else { ri.ComputedMinISize() };
            maxSize = if self.mAxis == LogicalAxis::Block { ri.ComputedMaxBSize() } else { ri.ComputedMaxISize() };
        }
        let mut origSizes: Option<TrackPlan> = None;
        let mut applyMinMax = (minSize != 0 || maxSize != NS_UNCONSTRAINEDSIZE) && aAvailableSize == NS_UNCONSTRAINEDSIZE;
        // We iterate twice at most.  The 2nd time if the grid size changed after
        // applying a min/max-size (can only occur if aAvailableSize is indefinite).
        loop {
            let fr = self.FindUsedFlexFraction(aGridRI, aGridItems, &flexTracks, aFunctions, aAvailableSize);
            if fr != 0.0 {
                for &i in flexTracks.iter() {
                    let flexFactor = TrackBreadth_AsFlex(aFunctions.MaxSizingFor(i));
                    let flexLength = NSToCoordRound__float(flexFactor * fr);
                    if flexLength > self.mSizes[i as usize].mBase {
                        if applyMinMax && origSizes.is_none() {
                            origSizes = Some(self.mSizes.clone());
                        }
                        self.mSizes[i as usize].mBase = flexLength;
                    }
                }
            }
            if applyMinMax {
                applyMinMax = false;
                // https://drafts.csswg.org/css-grid-2/#algo-flex-tracks
                // "If using this flex fraction would cause the grid to be smaller than
                // the grid container’s min-width/height (or larger than the grid
                // container’s max-width/height), then redo this step, treating the free
                // space as definite [...]"
                let sumOfGridGaps = self.SumOfGridGaps();
                let newSize = self.SumOfGridTracks() + sumOfGridGaps;
                if newSize > maxSize {
                    aAvailableSize = maxSize;
                } else if newSize < minSize {
                    aAvailableSize = minSize;
                }
                if aAvailableSize != NS_UNCONSTRAINEDSIZE {
                    aAvailableSize = std::cmp::max(0, aAvailableSize - sumOfGridGaps);
                    // Restart with the original track sizes and definite aAvailableSize.
                    if let Some(orig) = origSizes.take() {
                        self.mSizes = orig;
                    } // else, no mSizes[].mBase were changed above so it's still correct
                    if aAvailableSize == 0 {
                        break; // zero available size wouldn't change any sizes though...
                    }
                    continue;
                }
            }
            break;
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::AlignJustifyContent
    pub fn AlignJustifyContent(&mut self, aStyle: &nsStylePosition, mut aAligmentStyleValue: StyleContentDistribution, aWM: WritingMode, aContentBoxSize: nscoord, aIsSubgriddedAxis: bool) {
        let isAlign = self.mAxis == LogicalAxis::Block;
        // Align-/justify-content doesn't apply in a subgridded axis.
        // Gap properties do apply though so we need to stretch/position the tracks
        // to center-align the gaps with the parent's gaps.
        if aIsSubgriddedAxis {
            let gap = if isAlign { aStyle.mRowGap() } else { aStyle.mColumnGap() };
            if matches!(gap, mrt_style::style::values::generics::length::GenericLengthPercentageOrNormal::Normal) {
                return;
            }
            let len = self.mSizes.Length();
            if len <= 1 {
                return;
            }
            // This stores the gap deltas between the subgrid gap and the gaps in
            // the used track sizes (as encoded in its tracks' mPosition):
            let numGaps = len - 1;
            let mut gapDeltas: Vec<nscoord> = vec![0; numGaps];
            for i in 0..numGaps {
                let sz1 = &self.mSizes[i];
                let sz2 = &self.mSizes[i + 1];
                let currentGap = sz2.mPosition - (sz1.mPosition + sz1.mBase);
                gapDeltas[i] = self.mGridGap - currentGap;
            }
            // Recompute the tracks' size/position so that they end up with
            // a subgrid-gap centered on the original track gap.
            let mut currentPos = self.mSizes[0].mPosition;
            let mut lastHalfDelta: nscoord = 0;
            for i in 0..numGaps {
                let delta = gapDeltas[i];
                let mut halfDelta: nscoord = 0;
                let roundingError = NSCoordDivRem(delta, 2, &mut halfDelta);
                let sz = &mut self.mSizes[i];
                let newSize = sz.mBase - (halfDelta + roundingError) - lastHalfDelta;
                lastHalfDelta = halfDelta;
                // If the gap delta (in particular 'halfDelta + lastHalfDelta') is larger
                // than the current track size, newSize can be negative. Don't let the new
                // track size (mBase) be negative.
                sz.mBase = std::cmp::max(newSize, 0);
                sz.mPosition = currentPos;
                currentPos += newSize + self.mGridGap;
            }
            let lastTrack = self.mSizes.LastElement();
            let newSize = lastTrack.mBase - lastHalfDelta;
            lastTrack.mBase = std::cmp::max(newSize, 0);
            lastTrack.mPosition = currentPos;
            return;
        }

        if self.mSizes.IsEmpty() {
            return;
        }

        let mut overflowSafe = false;
        let mut alignment = GetAlignJustifyValue(aAligmentStyleValue.primary(), aWM, isAlign, &mut overflowSafe);
        if alignment == StyleAlignFlags::NORMAL {
            alignment = StyleAlignFlags::STRETCH;
            // we may need a fallback for 'stretch' below
            aAligmentStyleValue = StyleContentDistribution::new(alignment);
        }

        // Compute the free space and count auto-sized tracks.
        let mut numAutoTracks: usize = 0;
        let mut space: nscoord = 0;
        if alignment != StyleAlignFlags::START {
            let mut trackSizeSum: nscoord = 0;
            if aIsSubgriddedAxis {
                numAutoTracks = self.mSizes.Length();
            } else {
                for sz in self.mSizes.iter() {
                    trackSizeSum += sz.mBase;
                    if (sz.mState & TrackSize_StateBits::eAutoMaxSizing).to_bool() {
                        numAutoTracks += 1;
                    }
                }
            }
            space = aContentBoxSize - trackSizeSum - self.SumOfGridGaps();
            // Use the fallback value instead when applicable.
            if space < 0 || (alignment == StyleAlignFlags::SPACE_BETWEEN && self.mSizes.Length() == 1) {
                let fallback = GetAlignJustifyDistributionFallback(&aAligmentStyleValue, &mut overflowSafe);
                if let Some(fallback) = fallback {
                    alignment = fallback;
                }
            }
            if space == 0 || (space < 0 && overflowSafe) {
                // XXX check that this makes sense also for [last ]baseline (bug 1151204).
                alignment = StyleAlignFlags::START;
            }
        }

        // Optimize the cases where we just need to set each track's position.
        let mut pos: nscoord = 0;
        let mut distribute = true;
        if alignment == StyleAlignFlags::BASELINE || alignment == StyleAlignFlags::LAST_BASELINE {
            NS_WARNING!("NYI: 'first/last baseline' (bug 1151204)"); // XXX
            alignment = StyleAlignFlags::START;
        }
        if alignment == StyleAlignFlags::START {
            distribute = false;
        } else if alignment == StyleAlignFlags::END {
            pos = space;
            distribute = false;
        } else if alignment == StyleAlignFlags::CENTER {
            pos = space / 2;
            distribute = false;
        } else if alignment == StyleAlignFlags::STRETCH {
            distribute = numAutoTracks != 0;
        }
        if !distribute {
            for sz in self.mSizes.iter_mut() {
                sz.mPosition = pos;
                pos += sz.mBase + self.mGridGap;
            }
            return;
        }

        // Distribute free space to/between tracks and set their position.
        MOZ_ASSERT!(space > 0, "should've handled that on the fallback path above");
        let mut between: nscoord = 0;
        let mut roundingError: nscoord;
        if alignment == StyleAlignFlags::STRETCH {
            MOZ_ASSERT!(numAutoTracks > 0, "we handled numAutoTracks == 0 above");
            // The outer loop typically only runs once - it repeats only in a masonry
            // axis when some stretchable items reach their `max-size`.
            // It's O(n^2) worst case; if all items are stretchable with a `max-size`
            // and exactly one item reaches its `max-size` each round.
            while space != 0 {
                pos = 0;
                let mut spacePerTrack: nscoord = 0;
                roundingError = NSCoordDivRem(space, numAutoTracks, &mut spacePerTrack);
                space = 0;
                for sz in self.mSizes.iter_mut() {
                    sz.mPosition = pos;
                    if !(sz.mState & TrackSize_StateBits::eAutoMaxSizing).to_bool() {
                        pos += sz.mBase + self.mGridGap;
                        continue;
                    }
                    let mut stretch = spacePerTrack;
                    if roundingError != 0 {
                        roundingError -= 1;
                        stretch += 1;
                    }
                    let mut newBase = sz.mBase + stretch;
                    if self.mIsMasonry && (sz.mState & TrackSize_StateBits::eClampToLimit).to_bool() {
                        let clampedSize = std::cmp::min(newBase, sz.mLimit);
                        let sizeOverLimit = newBase - clampedSize;
                        if sizeOverLimit > 0 {
                            newBase = clampedSize;
                            sz.mState &= !(sz.mState & TrackSize_StateBits::eAutoMaxSizing);
                            // This repeats the outer loop to distribute the superfluous space:
                            space += sizeOverLimit;
                            numAutoTracks -= 1;
                            if numAutoTracks == 0 {
                                // ... except if we don't have any stretchable items left.
                                space = 0;
                            }
                        }
                    }
                    sz.mBase = newBase;
                    pos += newBase + self.mGridGap;
                }
                MOZ_ASSERT!(roundingError == 0, "we didn't distribute all rounding error?");
            }
            return;
        }
        if alignment == StyleAlignFlags::SPACE_BETWEEN {
            MOZ_ASSERT!(self.mSizes.Length() > 1, "should've used a fallback above");
            roundingError = NSCoordDivRem(space, self.mSizes.Length() - 1, &mut between);
        } else if alignment == StyleAlignFlags::SPACE_AROUND {
            roundingError = NSCoordDivRem(space, self.mSizes.Length(), &mut between);
            pos = between / 2;
        } else if alignment == StyleAlignFlags::SPACE_EVENLY {
            roundingError = NSCoordDivRem(space, self.mSizes.Length() + 1, &mut between);
            pos = between;
        } else {
            MOZ_ASSERT_UNREACHABLE!("unknown align-/justify-content value");
            between = 0; // just to avoid a compiler warning
            roundingError = 0; // just to avoid a compiler warning
        }
        between += self.mGridGap;
        for sz in self.mSizes.iter_mut() {
            sz.mPosition = pos;
            let mut spacing = between;
            if roundingError != 0 {
                roundingError -= 1;
                spacing += 1;
            }
            pos += sz.mBase + spacing;
        }
        MOZ_ASSERT!(roundingError == 0, "we didn't distribute all rounding error?");
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp Tracks::TotalTrackSizeWithoutAlignment
    pub fn TotalTrackSizeWithoutAlignment(&self, aGridContainerFrame: &nsGridContainerFrame) -> nscoord {
        if aGridContainerFrame.IsSubgrid__LogicalAxis(self.mAxis) {
            // TODO: Investigate whether GridLineEdge here may include extra packing
            // space introduced by align-content or justify-content, and if that could
            // lead to inconsistent metrics vs. the non-subgrid path.
            return self.GridLineEdge(self.mSizes.Length() as u32, GridLineSide::BeforeGridGap);
        }

        // This method allows for the possibility that AlignJustifyContent() might not
        // be called yet. Therefore, we can't use GridLineEdge() here, as mPosition
        // may not be calculated.
        self.SumOfGridTracksAndGaps()
    }
}

// Part 5 of nsGridContainerFrame.rs: nsGridContainerFrame.cpp lines 7768-9412 at GECKO_REV
// (LineRange::ToPositionAndLength .. ComputeIntrinsicContentBSize). Joined into one file at the end.

// Gecko: layout/generic/nsGridContainerFrame.cpp LineRange::ToPositionAndLength
impl nsGridContainerFrame_LineRange {
    pub fn ToPositionAndLength(&self, aTrackSizes: &TrackPlan, aPos: &mut nscoord, aLength: &mut nscoord) {
        MOZ_ASSERT!(self.mStart != kAutoLine && self.mEnd != kAutoLine, "expected a definite LineRange");
        MOZ_ASSERT!(self.mStart < self.mEnd);
        let startPos: nscoord = aTrackSizes[self.mStart as usize].mPosition;
        let sz: &TrackSize = &aTrackSizes[(self.mEnd - 1) as usize];
        *aPos = startPos;
        *aLength = (sz.mPosition + sz.mBase) - startPos;
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp LineRange::ToLength
    pub fn ToLength(&self, aTrackSizes: &TrackPlan) -> nscoord {
        MOZ_ASSERT!(self.mStart != kAutoLine && self.mEnd != kAutoLine, "expected a definite LineRange");
        MOZ_ASSERT!(self.mStart < self.mEnd);
        let startPos: nscoord = aTrackSizes[self.mStart as usize].mPosition;
        let sz: &TrackSize = &aTrackSizes[(self.mEnd - 1) as usize];
        (sz.mPosition + sz.mBase) - startPos
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp LineRange::ToPositionAndLengthForAbsPos
    // NOT PORTED: absolutely positioned children get no frame in this runtime, so nothing calls this
    // (ContainingBlockForAbsPos is its only caller); the code is translated as it is.
    pub fn ToPositionAndLengthForAbsPos(&self, aTracks: &Tracks, aGridOrigin: nscoord, aPos: &mut nscoord, aLength: &mut nscoord) {
        // kAutoLine for abspos children contributes the corresponding edge
        // of the grid container's padding-box.
        if self.mEnd == kAutoLine {
            if self.mStart == kAutoLine {
                // done
            } else {
                let endPos: nscoord = *aPos + *aLength;
                let side = if self.mStart as usize == aTracks.mSizes.Length() { GridLineSide::BeforeGridGap } else { GridLineSide::AfterGridGap };
                let startPos: nscoord = aTracks.GridLineEdge(self.mStart, side);
                *aPos = aGridOrigin + startPos;
                *aLength = std::cmp::max(endPos - *aPos, 0);
            }
        } else {
            if self.mStart == kAutoLine {
                let side = if self.mEnd == 0 { GridLineSide::AfterGridGap } else { GridLineSide::BeforeGridGap };
                let endPos: nscoord = aTracks.GridLineEdge(self.mEnd, side);
                *aLength = std::cmp::max(aGridOrigin + endPos, 0);
            } else if self.mStart != self.mEnd {
                let mut pos: nscoord = 0;
                self.ToPositionAndLength(&aTracks.mSizes, &mut pos, aLength);
                *aPos = aGridOrigin + pos;
            } else {
                // The grid area only covers removed 'auto-fit' tracks.
                let pos: nscoord = aTracks.GridLineEdge(self.mStart, GridLineSide::BeforeGridGap);
                *aPos = aGridOrigin + pos;
                *aLength = 0;
            }
        }
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp GridReflowInput::PercentageBasisFor
impl<'a> nsGridContainerFrame_GridReflowInput<'a> {
    pub fn PercentageBasisFor(&self, aAxis: LogicalAxis, aGridItem: &GridItemInfo) -> LogicalSize {
        let wm = aGridItem.mFrame.GetWritingMode();
        let itemParent = aGridItem.mFrame.GetParent().unwrap();
        if itemParent != self.mFrame.0 {
            // The item comes from a descendant subgrid.  Use the subgrid's
            // used track sizes to resolve the grid area size, if present.
            MOZ_ASSERT!(itemParent.IsGridContainerFrame());
            let subgridFrame = nsGridContainerFrame::from_frame(itemParent);
            MOZ_ASSERT!(subgridFrame.IsSubgrid__());
            if let Some(uts) = subgridFrame.GetUsedTrackSizes() {
                let subgridWM = subgridFrame.GetWritingMode();
                let mut cbSize = LogicalSize::new(subgridWM, NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE);
                if !subgridFrame.IsColSubgrid() && uts.mCanResolveLineRangeSize[LogicalAxis::Inline] {
                    // NOTE: At this point aGridItem.mArea is in this->mFrame coordinates
                    // and thus may have been transposed.  The range values in a non-
                    // subgridded axis still has its original values in subgridFrame's
                    // coordinates though.
                    let subgridIAxisInGridWM = subgridWM.ConvertAxisTo(LogicalAxis::Inline, self.mWM);
                    let range = aGridItem.mArea.LineRangeForAxis(subgridIAxisInGridWM);
                    *cbSize.ISize_mut(subgridWM) = range.ToLength(&uts.mTrackPlans[LogicalAxis::Inline]);
                }
                if !subgridFrame.IsRowSubgrid() && uts.mCanResolveLineRangeSize[LogicalAxis::Block] {
                    let subgridBAxisInGridWM = subgridWM.ConvertAxisTo(LogicalAxis::Block, self.mWM);
                    let range = aGridItem.mArea.LineRangeForAxis(subgridBAxisInGridWM);
                    *cbSize.BSize_mut(subgridWM) = range.ToLength(&uts.mTrackPlans[LogicalAxis::Block]);
                }
                return cbSize.ConvertTo(wm, subgridWM);
            }

            return LogicalSize::new(wm, NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE);
        }

        // Get row size and column size for the grid area occupied by aGridItem.
        let colSize: nscoord = if self.mCols.mCanResolveLineRangeSize { self.mCols.ResolveSize(&aGridItem.mArea.mCols) } else { NS_UNCONSTRAINEDSIZE };
        let rowSize: nscoord = if self.mRows.mCanResolveLineRangeSize { self.mRows.ResolveSize(&aGridItem.mArea.mRows) } else { NS_UNCONSTRAINEDSIZE };
        if !wm.IsOrthogonalTo(&self.mWM) { LogicalSize::new(wm, colSize, rowSize) } else { LogicalSize::new(wm, rowSize, colSize) }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp GridReflowInput::ContainingBlockFor
    pub fn ContainingBlockFor(&self, aArea: &GridArea) -> LogicalRect {
        let mut i: nscoord = 0;
        let mut b: nscoord = 0;
        let mut iSize: nscoord = 0;
        let mut bSize: nscoord = 0;
        MOZ_ASSERT!(aArea.mCols.Extent() > 0, "grid items cover at least one track");
        MOZ_ASSERT!(aArea.mRows.Extent() > 0, "grid items cover at least one track");
        aArea.mCols.ToPositionAndLength(&self.mCols.mSizes, &mut i, &mut iSize);
        aArea.mRows.ToPositionAndLength(&self.mRows.mSizes, &mut b, &mut bSize);
        LogicalRect::new(self.mWM, i, b, iSize, bSize)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp GridReflowInput::ContainingBlockForAbsPos
    // NOT PORTED: absolutely positioned children (no frame in this runtime); only ReflowAbsoluteChildren calls it.
    pub fn ContainingBlockForAbsPos(&self, aArea: &GridArea, aGridOrigin: &LogicalPoint, aGridCB: &LogicalRect) -> LogicalRect {
        let mut i: nscoord = aGridCB.IStart(self.mWM);
        let mut b: nscoord = aGridCB.BStart(self.mWM);
        let mut iSize: nscoord = aGridCB.ISize(self.mWM);
        let mut bSize: nscoord = aGridCB.BSize(self.mWM);
        aArea.mCols.ToPositionAndLengthForAbsPos(&self.mCols, aGridOrigin.I(self.mWM), &mut i, &mut iSize);
        aArea.mRows.ToPositionAndLengthForAbsPos(&self.mRows, aGridOrigin.B(self.mWM), &mut b, &mut bSize);
        LogicalRect::new(self.mWM, i, b, iSize, bSize)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp GridReflowInput::AlignJustifyContentInMasonryAxis
    // NOT PORTED: masonry layout (IsMasonry() is always false here, nothing calls this).
    // Original (nsGridContainerFrame.cpp lines 7901-7946 at GECKO_REV):
    //   void GridReflowInput::AlignJustifyContentInMasonryAxis(nscoord aMasonryBoxSize, nscoord aContentBoxSize) {
    //     if (aContentBoxSize == NS_UNCONSTRAINEDSIZE) { aContentBoxSize = aMasonryBoxSize; }
    //     auto& masonryAxisTracks = mRows.mIsMasonry ? mRows : mCols;
    //     ... stretch the "masonry box" or align one track sized to it with masonryAxisTracks.AlignJustifyContent(...)
    //   }
    pub fn AlignJustifyContentInMasonryAxis(&mut self, aMasonryBoxSize: nscoord, aContentBoxSize: nscoord) {
        unimplemented!("NOT PORTED: masonry layout (AlignJustifyContentInMasonryAxis)")
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp GridReflowInput::AlignJustifyTracksInMasonryAxis
    // NOT PORTED: masonry layout (IsMasonry() is always false here, nothing calls this).
    // Original (nsGridContainerFrame.cpp lines 7948-7982 at GECKO_REV): moves the items by the offset of the
    // "masonry box" (masonryAxisTracks.mSizes[0].mPosition) with item.mFrame->MovePositionBy(wm, logicalDelta).
    pub fn AlignJustifyTracksInMasonryAxis(&mut self, aContentSize: &LogicalSize, aContainerSize: &nsSize) {
        unimplemented!("NOT PORTED: masonry layout (AlignJustifyTracksInMasonryAxis)")
    }
}

/**
 * Return a Fragmentainer object if we have a fragmentainer frame in our
 * ancestor chain of containing block (CB) reflow inputs.  We'll only
 * continue traversing the ancestor chain as long as the CBs have
 * the same writing-mode and have overflow:visible.
 */
// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GetNearestFragmentainer
// NOT PORTED: fragmentation (columns and pages): this runtime lays out one unfragmented fragment, so there
// is never a fragmentainer. Original: nsGridContainerFrame.cpp lines 7984-8034 at GECKO_REV (walks mCBReflowInput
// up to a Canvas frame in a paginated context or a ColumnSet and fills a Fragmentainer).
impl nsGridContainerFrame {
    pub fn GetNearestFragmentainer(&self, aGridRI: &GridReflowInput) -> Option<Fragmentainer> {
        let data: Option<Fragmentainer> = None;
        // const ReflowInput* gridRI = aGridRI.mReflowInput;
        // if (!gridRI->IsInFragmentedContext()) { return data; }
        // NOT PORTED: fragmentation. IsInFragmentedContext() is never true for this runtime's reflow inputs.
        MOZ_ASSERT!(!aGridRI.mReflowInput.unwrap().IsInFragmentedContext(), "NOT PORTED: grid fragmentation");
        data
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ReflowInFlowChild
impl nsGridContainerFrame {
    pub fn ReflowInFlowChild(
        &self,
        aChild: nsIFrame,
        aGridItemInfo: Option<&GridItemInfo>,
        aContainerSize: nsSize,
        aStretchBSize: Option<nscoord>,
        aFragmentainer: Option<&Fragmentainer>,
        aGridRI: &GridReflowInput,
        aContentArea: &LogicalRect,
        aDesiredSize: &mut ReflowOutput,
        aStatus: &mut nsReflowStatus,
    ) {
        let pc = self.PresContext();
        let containerSC = self.Style();
        let wm: WritingMode = aGridRI.mReflowInput.unwrap().GetWritingMode();
        let isGridItem = aGridItemInfo.is_some();
        MOZ_ASSERT!(isGridItem == !aChild.IsPlaceholderFrame());
        let mut cb = LogicalRect::new__WritingMode(wm);
        let childWM: WritingMode = aChild.GetWritingMode();
        let mut isConstrainedBSize = false;
        let mut toFragmentainerEnd: nscoord = 0;
        // The part of the child's grid area that's in previous container fragments.
        let mut consumedGridAreaBSize: nscoord = 0;
        if isGridItem {
            let aGridItemInfo = aGridItemInfo.unwrap();
            MOZ_ASSERT!(aGridItemInfo.mFrame == aChild);
            let area: &GridArea = &aGridItemInfo.mArea;
            MOZ_ASSERT!(area.IsDefinite());
            cb = aGridRI.ContainingBlockFor(area);
            if aFragmentainer.is_some() && !wm.IsOrthogonalTo(&childWM) {
                let aFragmentainer = aFragmentainer.unwrap();
                // |gridAreaBOffset| is the offset of the child's grid area in this
                // container fragment (if negative, that distance is the child CB size
                // consumed in previous container fragments).  Note that cb.BStart
                // (initially) and aState.mFragBStart are in "global" grid coordinates
                // (like all track positions).
                let gridAreaBOffset: nscoord = cb.BStart(wm) - aGridRI.mFragBStart;
                consumedGridAreaBSize = std::cmp::max(0, -gridAreaBOffset);
                *cb.BStart_mut(wm) = std::cmp::max(0, gridAreaBOffset);
                if aFragmentainer.mToFragmentainerEnd != NS_UNCONSTRAINEDSIZE {
                    toFragmentainerEnd = aFragmentainer.mToFragmentainerEnd - aGridRI.mFragBStart - cb.BStart(wm);
                    toFragmentainerEnd = std::cmp::max(toFragmentainerEnd, 0);
                    isConstrainedBSize = true;
                }
            }
            cb += aContentArea.Origin(wm);
            aGridRI.mRows.AlignBaselineSubtree(aGridItemInfo);
            aGridRI.mCols.AlignBaselineSubtree(aGridItemInfo);
            // Setup [align|justify]-content:[last ]baseline related frame properties.
            // These are added to the padding in SizeComputationInput::InitOffsets.
            // (a negative value signals the value is for 'last baseline' and should be
            //  added to the (logical) end padding)
            // (C++ `Prop` is a pointer to the property descriptor.)
            let SetProp = |aGridAxis: LogicalAxis, aProp: FramePropertyDescriptor<nscoord>| {
                let state = aGridItemInfo.mState[aGridAxis].get();
                let mut baselineAdjust: nscoord = if (state & ItemState::eContentBaseline).to_bool() { aGridItemInfo.mBaselineOffset[aGridAxis].get() } else { 0 };
                if baselineAdjust < 0 {
                    // This happens when the subtree overflows its track.
                    // XXX spec issue? it's unclear how to handle this.
                    baselineAdjust = 0;
                } else if (state & ItemState::eLastBaseline).to_bool() {
                    // FIXME: We're not setting the ItemState::eEndSideBaseline flag any
                    // more as the new baseline sharing group calculation handles most of
                    // the cases we need.  For non-masonry grids this flag was always set
                    // for LAST_BASELINE items, so we're just mimicking that behavior here.
                    // That said, masonry grids might not work 100% any more..
                    baselineAdjust = -baselineAdjust;
                }
                if baselineAdjust != 0 {
                    aChild.SetProperty(aProp, baselineAdjust);
                } else {
                    aChild.RemoveProperty(aProp);
                }
            };
            let isOrthogonal = wm.IsOrthogonalTo(&childWM);
            SetProp(LogicalAxis::Block, if isOrthogonal { nsIFrame::IBaselinePadProperty() } else { nsIFrame::BBaselinePadProperty() });
            SetProp(LogicalAxis::Inline, if isOrthogonal { nsIFrame::BBaselinePadProperty() } else { nsIFrame::IBaselinePadProperty() });
        } else {
            // By convention, for frames that perform CSS Box Alignment, we position
            // placeholder children at the start corner of their alignment container,
            // and in this case that's usually the grid's content-box.
            // ("Usually" - the exception is when the grid *also* forms the
            // abs.pos. containing block. In that case, the alignment container isn't
            // the content-box -- it's some grid area instead.  But that case doesn't
            // require any special handling here, because we handle it later using a
            // special flag (ReflowInput::InitFlag::StaticPosIsCBOrigin) which will make
            // us ignore the placeholder's position entirely.)
            cb = *aContentArea;
            aChild.AddStateBits(PLACEHOLDER_STATICPOS_NEEDS_CSSALIGN);
        }

        let mut reflowSize = cb.Size(wm);
        if isConstrainedBSize {
            *reflowSize.BSize_mut(wm) = toFragmentainerEnd;
        }
        let mut childCBSize: LogicalSize = reflowSize.ConvertTo(childWM, wm);

        // Setup the ClampMarginBoxMinSize reflow flags and property, if needed.
        let mut csFlags = ComputeSizeFlags::default();
        if let Some(aGridItemInfo) = aGridItemInfo {
            let childIAxisInWM = childWM.ConvertAxisTo(LogicalAxis::Inline, wm);
            // Clamp during reflow if we're stretching in that axis.
            if self.GridItemShouldStretch(aChild, LogicalAxis::Inline) {
                if (aGridItemInfo.mState[childIAxisInWM].get() & ItemState::eClampMarginBoxMinSize).to_bool() {
                    csFlags += ComputeSizeFlag::IClampMarginBoxMinSize;
                }
            } else {
                csFlags += ComputeSizeFlag::ShrinkWrap;
            }

            let childBAxisInWM = GetOrthogonalAxis(childIAxisInWM);
            if self.GridItemShouldStretch(aChild, LogicalAxis::Block) && (aGridItemInfo.mState[childBAxisInWM].get() & ItemState::eClampMarginBoxMinSize).to_bool() {
                csFlags += ComputeSizeFlag::BClampMarginBoxMinSize;
                aChild.SetProperty(nsIFrame::BClampMarginBoxMinSizeProperty(), childCBSize.BSize(childWM));
            } else {
                aChild.RemoveProperty(nsIFrame::BClampMarginBoxMinSizeProperty());
            }

            if (aGridItemInfo.mState[childIAxisInWM].get() & ItemState::eContentBasedAutoMinSize).to_bool() {
                csFlags += ComputeSizeFlag::IApplyAutoMinSize;
            }
        }

        if !isConstrainedBSize {
            *childCBSize.BSize_mut(childWM) = NS_UNCONSTRAINEDSIZE;
        }
        let percentBasis = cb.Size(wm).ConvertTo(childWM, wm);
        let mut childRI = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            &pc,
            aGridRI.mReflowInput.unwrap(),
            aChild,
            childCBSize,
            Some(percentBasis),
            EnumSet::default(),
            StyleSizeOverrides::default(),
            csFlags,
            std::ptr::null_mut(),
        );
        childRI.mFlags.mIsTopOfPage = if let Some(f) = aFragmentainer { f.mIsTopOfPage } else { false };

        // FIXME (perf): It would be faster to do this only if the previous reflow of
        // the child was a measuring reflow, and only if the child does some of the
        // things that are affected by ComputeSizeFlag::IsGridMeasuringReflow.
        childRI.SetBResize(true);
        childRI.SetBResizeForPercentages(true);

        // If the child is stretching in its block axis, and we might be fragmenting
        // it in that axis, then setup a frame property to tell
        // nsBlockFrame::ComputeFinalSize the size.
        if isConstrainedBSize && !wm.IsOrthogonalTo(&childWM) {
            let stretch = childRI.mStylePosition.BSize(childWM, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(childRI.mFrame, std::ptr::null_mut())).IsAuto()
                && self.GridItemShouldStretch(aChild, LogicalAxis::Block);
            if stretch {
                aChild.SetProperty(nsIFrame::FragStretchBSizeProperty(), aStretchBSize.unwrap());
            } else {
                aChild.RemoveProperty(nsIFrame::FragStretchBSizeProperty());
            }
        }

        // We need the width of the child before we can correctly convert
        // the writing-mode of its origin, so we reflow at (0, 0) using a dummy
        // aContainerSize, and then pass the correct position to FinishReflowChild.
        let mut childSize = ReflowOutput::new__ReflowInput(&childRI);
        let dummyContainerSize = nsSize::default();

        self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
            aChild,
            &pc,
            &mut childSize,
            &childRI,
            childWM,
            LogicalPoint::new__WritingMode(childWM),
            dummyContainerSize,
            nsIFrame_ReflowChildFlags::Default,
            aStatus,
            None,
        );

        // childPos here initially represents the position that the child would have
        // (expressed as an istart,bstart corner *in its own writing-mode*) if it
        // were placed at the cb origin:
        let mut childPos: LogicalPoint = cb.Origin(wm).ConvertRectOriginTo(childWM, wm, childSize.PhysicalSize(), aContainerSize);

        // Apply align/justify-self and reflow again if that affects the size.
        if isGridItem {
            let aGridItemInfo = aGridItemInfo.unwrap();
            let mut size: LogicalSize = childSize.Size(childWM); // from the ReflowChild()
            let mut applyItemSelfAlignment = |aAxis: LogicalAxis, mut aCBSize: nscoord| {
                let mut align: StyleSelfAlignment = mrt_style::style::values::specified::align::SelfAlignment(childRI.mStylePosition.UsedSelfAlignment__LogicalAxis_ComputedStyle(aAxis, Some(containerSC.clone())));
                let state = aGridItemInfo.mState[aAxis].get();
                let mut flags = AlignJustifyFlags::default();
                if self.IsMasonry__LogicalAxis(aAxis) {
                    // In a masonry axis, we inhibit applying 'stretch' and auto-margins
                    // here since AlignJustifyTracksInMasonryAxis deals with that.
                    // The only other {align,justify}-{self,content} values that have an
                    // effect are '[last] baseline', the rest behave as 'start'.
                    if !(state & ItemState::eSelfBaseline).to_bool() {
                        align = mrt_style::style::values::specified::align::SelfAlignment(StyleAlignFlags::START);
                    } else {
                        let group = if (state & ItemState::eFirstBaseline).to_bool() { BaselineSharingGroup::First } else { BaselineSharingGroup::Last };
                        let itemStart = aGridItemInfo.mArea.LineRangeForAxis(aAxis).mStart;
                        aCBSize = aGridRI.TracksFor(aAxis).mSizes[itemStart as usize].mBaselineSubtreeSize[group];
                    }
                    flags += AlignJustifyFlag::IgnoreAutoMargins;
                } else if (state & ItemState::eContentBaseline).to_bool() {
                    align = mrt_style::style::values::specified::align::SelfAlignment(if (state & ItemState::eFirstBaseline).to_bool() { StyleAlignFlags::SELF_START } else { StyleAlignFlags::SELF_END });
                }
                if aAxis == LogicalAxis::Block {
                    AlignSelf(aGridItemInfo, align.0, aCBSize, wm, &childRI, &size, flags, &mut childPos);
                } else {
                    JustifySelf(aGridItemInfo, align.0, aCBSize, wm, &childRI, &size, flags, &mut childPos);
                }
            };
            if aStatus.IsComplete() {
                applyItemSelfAlignment(LogicalAxis::Block, cb.BSize(wm) - consumedGridAreaBSize);
            }
            applyItemSelfAlignment(LogicalAxis::Inline, cb.ISize(wm));
        } // else, AbsoluteContainingBlock.cpp will handle align/justify-self.

        nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
            aChild,
            &pc,
            &childSize,
            Some(&childRI),
            childWM,
            childPos,
            aContainerSize,
            nsIFrame_ReflowChildFlags::ApplyRelativePositioning,
        );
        self.ConsiderChildOverflow(&mut aDesiredSize.mOverflowAreas, aChild, OverflowAreaUnionFlags::None);
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ReflowInFragmentainer
// NOT PORTED: fragmentation (nsGridContainerFrame.cpp lines 8256-8477 at GECKO_REV).
impl nsGridContainerFrame {
    pub fn ReflowInFragmentainer(
        &self,
        aGridRI: &mut GridReflowInput,
        aContentArea: &LogicalRect,
        aDesiredSize: &mut ReflowOutput,
        aStatus: &mut nsReflowStatus,
        aFragmentainer: &mut Fragmentainer,
        aContainerSize: &nsSize,
    ) -> nscoord {
        unimplemented!("NOT PORTED: grid fragmentation (ReflowInFragmentainer)")
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ReflowRowsInFragmentainer
    // NOT PORTED: fragmentation (nsGridContainerFrame.cpp lines 8479-8729 at GECKO_REV).
    // (The C++ item list is `nsTArray<const GridItemInfo*>`; here it would be indexes into aGridRI.mGridItems.)
    pub fn ReflowRowsInFragmentainer(
        &self,
        aGridRI: &mut GridReflowInput,
        aContentArea: &LogicalRect,
        aDesiredSize: &mut ReflowOutput,
        aStatus: &mut nsReflowStatus,
        aFragmentainer: &mut Fragmentainer,
        aContainerSize: &nsSize,
        aItems: &Vec<usize>,
        aStartRow: u32,
        aEndRow: u32,
        aBSize: nscoord,
        aAvailableSize: nscoord,
    ) -> nscoord {
        unimplemented!("NOT PORTED: grid fragmentation (ReflowRowsInFragmentainer)")
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::MasonryLayout
    // NOT PORTED: masonry layout (nsGridContainerFrame.cpp lines 8731-9226 at GECKO_REV).
    pub fn MasonryLayout(
        &self,
        aGridRI: &mut GridReflowInput,
        aContentArea: &LogicalRect,
        aConstraint: SizingConstraint,
        aDesiredSize: &mut ReflowOutput,
        aStatus: &mut nsReflowStatus,
        aFragmentainer: Option<&mut Fragmentainer>,
        aContainerSize: &nsSize,
    ) -> nscoord {
        unimplemented!("NOT PORTED: masonry layout (MasonryLayout)")
    }
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ParentGridContainerForSubgrid
impl nsGridContainerFrame {
    pub fn ParentGridContainerForSubgrid(&self) -> nsGridContainerFrame {
        MOZ_ASSERT!(self.IsSubgrid__());
        let mut p: nsIFrame = self.GetParent().unwrap();
        while p.GetContent() == self.GetContent() {
            p = p.GetParent().unwrap();
        }
        MOZ_ASSERT!(p.IsGridContainerFrame());
        let parent = nsGridContainerFrame::from_frame(p);
        MOZ_ASSERT!(parent.HasSubgridItems__());
        parent
    }

    // ADAPTER: nsIFrame::HidesContentForLayout (content-visibility is not read from the style: content is never
    // hidden; the same stand-in as nsFlexContainerFrame.rs has).
    fn HidesContentForLayout(&self) -> bool {
        false
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ReflowChildren
    pub fn ReflowChildren(&self, aGridRI: &mut GridReflowInput, aContentArea: &LogicalRect, aContainerSize: &nsSize, aDesiredSize: &mut ReflowOutput, aStatus: &mut nsReflowStatus) -> nscoord {
        let wm: WritingMode = aGridRI.mReflowInput.unwrap().GetWritingMode();
        let mut bSize: nscoord = aContentArea.BSize(wm);
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");
        if self.HidesContentForLayout() {
            return bSize;
        }

        let mut ocBounds = OverflowAreas::default();
        let mut ocStatus = nsReflowStatus::new();
        if self.GetPrevInFlow().is_some() {
            self.ReflowOverflowContainerChildren(&self.PresContext(), aGridRI.mReflowInput.unwrap(), &mut ocBounds, nsIFrame_ReflowChildFlags::Default, &mut ocStatus, nsContainerFrame::MergeSortedFrameListsFor, None);
        }

        let mut fragmentainer: Option<Fragmentainer> = self.GetNearestFragmentainer(aGridRI);
        // MasonryLayout() can only handle fragmentation in the masonry-axis,
        // so we let ReflowInFragmentainer() deal with grid-axis fragmentation
        // in the else-clause below.
        if self.IsMasonry__() && !(self.IsColMasonry() && fragmentainer.is_some()) {
            aGridRI.mInFragmentainer = fragmentainer.is_some();
            let sz: nscoord = self.MasonryLayout(aGridRI, aContentArea, SizingConstraint::NoConstraint, aDesiredSize, aStatus, fragmentainer.as_mut(), aContainerSize);
            if self.IsRowMasonry() {
                bSize = aGridRI.mReflowInput.unwrap().ComputedBSize();
                if bSize == NS_UNCONSTRAINEDSIZE {
                    bSize = aGridRI.mReflowInput.unwrap().ApplyMinMaxBSize(sz, 0);
                }
            }
        } else if fragmentainer.is_some() {
            if self.IsColMasonry() && self.GetPrevInFlow().is_none() {
                // First we do an unconstrained reflow to resolve the item placement
                // which is then kept as-is in the constrained reflow below.
                self.MasonryLayout(aGridRI, aContentArea, SizingConstraint::NoConstraint, aDesiredSize, aStatus, None, aContainerSize);
            }
            aGridRI.mInFragmentainer = true;
            bSize = self.ReflowInFragmentainer(aGridRI, aContentArea, aDesiredSize, aStatus, fragmentainer.as_mut().unwrap(), aContainerSize);
        } else {
            aGridRI.mIter.Reset(CSSOrderAwareFrameIterator_ChildFilter::IncludeAll);
            while !aGridRI.mIter.AtEnd() {
                let child: nsIFrame = aGridRI.mIter.get();
                let info: Option<&GridItemInfo> = if !child.IsPlaceholderFrame() { Some(&aGridRI.mGridItems[aGridRI.mIter.ItemIndex()]) } else { None };
                let mut childStatus = nsReflowStatus::new();
                self.ReflowInFlowChild(child, info, *aContainerSize, None, None, aGridRI, aContentArea, aDesiredSize, &mut childStatus);
                MOZ_ASSERT!(childStatus.IsComplete(), "child should be complete in unconstrained reflow");
                aStatus.MergeCompletionStatusFrom(&childStatus);
                aGridRI.mIter.Next();
            }
        }

        // Merge overflow container bounds and status.
        aDesiredSize.mOverflowAreas.UnionWith(&ocBounds);
        aStatus.MergeCompletionStatusFrom(&ocStatus);

        bSize
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ReflowAbsoluteChildren
    pub fn ReflowAbsoluteChildren(&self, aGridRI: &mut GridReflowInput, aContentArea: &LogicalRect, aContentBSize: nscoord, aDesiredSize: &mut ReflowOutput, aStatus: &mut nsReflowStatus) {
        let reflowInput = aGridRI.mReflowInput.unwrap();
        let wm = reflowInput.GetWritingMode();
        let absoluteContainer = self.GetAbsoluteContainingBlock();
        // We have prepared the absolute frames in Grid::PlaceGridItems() or in
        // GridReflowInput::InitializeForContinuation().
        let Some(absoluteContainer) = absoluteContainer else {
            return;
        };
        if !absoluteContainer.HasAbsoluteFrames() {
            return;
        }
        // 'gridOrigin' is the origin of the grid (the start of the first track),
        // with respect to the grid container's padding-box (CB).
        let pad = reflowInput.ComputedLogicalPadding(wm);
        let gridOrigin = LogicalPoint::new(wm, pad.IStart(wm), pad.BStart(wm));
        let gridContentBSize = if aGridRI.mInFragmentainer && !aGridRI.mRows.mSizes.IsEmpty() { aGridRI.mRows.GridLineEdge(aGridRI.mRows.mSizes.Length() as u32, GridLineSide::BeforeGridGap) } else { aContentBSize };
        let gridCB = LogicalRect::new(wm, 0, 0, aContentArea.ISize(wm) + pad.IStartEnd(wm), gridContentBSize + pad.BStartEnd(wm));
        let gridCBPhysicalSize = gridCB.Size(wm).GetPhysicalSize(wm);
        let mut i: usize = 0;
        for child in absoluteContainer.GetChildList() {
            MOZ_ASSERT!(i < aGridRI.mAbsPosItems.len());
            MOZ_ASSERT!(aGridRI.mAbsPosItems[i].mFrame == child);
            let area = aGridRI.mAbsPosItems[i].mArea;
            let itemCB = aGridRI.ContainingBlockForAbsPos(&area, &gridOrigin, &gridCB);
            // AbsoluteContainingBlock::Reflow uses physical coordinates.
            child.SetProperty(GridItemContainingBlockRect(), itemCB.GetPhysicalRect(wm, gridCBPhysicalSize));
            i += 1;
        }
        let border = reflowInput.ComputedPhysicalBorder();
        let borderShift = nsPoint::new(border.left, border.top);
        let paddingRect = nsRect::new__nsPoint_nsSize(borderShift, gridCBPhysicalSize);
        // XXX: To optimize the performance, set the flags only when the CB width
        // or height actually changes.
        let mut flags = AbsPosReflowFlags::default();
        flags += AbsPosReflowFlag::AllowFragmentation;
        flags += AbsPosReflowFlag::CBWidthChanged;
        flags += AbsPosReflowFlag::CBHeightChanged;
        flags += AbsPosReflowFlag::IsGridContainerCB;
        let mut absposStatus = nsReflowStatus::default();
        absoluteContainer.Reflow(self.0, &self.PresContext(), reflowInput, &mut absposStatus, paddingRect, flags, Some(&mut aDesiredSize.mOverflowAreas));
        aStatus.MergeCompletionStatusFrom(&absposStatus);
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ComputeBSizeForResolvingRowSizes
    pub fn ComputeBSizeForResolvingRowSizes(&self, aGridRI: &mut GridReflowInput, aComputedBSize: nscoord, aContainIntrinsicBSize: Option<nscoord>) -> nscoord {
        if aComputedBSize != NS_UNCONSTRAINEDSIZE {
            // We don't need to apply the min/max constraints to the computed block-size
            // because ReflowInput (specifically when computing the block-size in
            // nsIFrame::ComputeSize()) has already clamped the block-size.
            return aComputedBSize;
        }

        if let Some(containIntrinsicBSize) = aContainIntrinsicBSize {
            // We have an unconstrained block-size, but we also have a specified
            // 'contain-intrinsic-block-size'. We apply the min/max constraints to the
            // value, and use that for track sizing.
            return aGridRI.mReflowInput.unwrap().ApplyMinMaxBSize(containIntrinsicBSize, 0);
        }

        NS_UNCONSTRAINEDSIZE
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ComputeIntrinsicContentBSize
    pub fn ComputeIntrinsicContentBSize(&self, aGridRI: &GridReflowInput, aComputedBSize: nscoord, aBSizeForResolvingRowSizes: nscoord, aContainIntrinsicBSize: Option<nscoord>) -> nscoord {
        MOZ_ASSERT!(
            aComputedBSize == NS_UNCONSTRAINEDSIZE || aGridRI.mReflowInput.unwrap().ShouldApplyAutomaticMinimumOnBlockAxis(),
            "Why call this method when intrinsic content block-size is not needed?"
        );

        if aComputedBSize == NS_UNCONSTRAINEDSIZE {
            // When we have an unconstrained block-size, the intrinsic content
            // block-size would have been determined after we resolved the row sizes the
            // first time. Just return that value.
            return aBSizeForResolvingRowSizes;
        }

        if let Some(containIntrinsicBSize) = aContainIntrinsicBSize {
            // We have a specified 'contain-intrinsic-block-size' which we need to
            // honor.
            return containIntrinsicBSize;
        }

        if self.IsRowMasonry() {
            // There aren't any tracks to derive a block-size from, if we're doing
            // masonry rather than forming rows in the block direction.
            return aBSizeForResolvingRowSizes;
        }

        aGridRI.mRows.TotalTrackSizeWithoutAlignment(self)
    }
}

// Part 6 of the translation of layout/generic/nsGridContainerFrame.cpp (GECKO_REV): .cpp lines 9413-11024,
// from nsGridContainerFrame::Reflow to the end of the file. The parts are joined into one file
// (nsGridContainerFrame.rs) at the end of the work.
//
// What is here: Reflow, the subgrid/masonry state bits, Init, DidSetComputedStyle, the intrinsic inline-size,
// the child-list mutators, the baselines, GridItemShouldStretch, the static helpers, and the bodies of
// TrackPlan and ItemPlan.
//
// NOT PORTED (each is marked where it occurs): painting (BuildDisplayList), the debug frame dump (GetFrameName,
// ExtraContainerFrameInfo, TrackSize::Dump*), the devtools data in Reflow (NS_STATE_GRID_COMPUTED_INFO,
// GetGridFrameWithComputedInfo), the nsILineIterator implementation, fragmentation (continuations,
// InitializeForContinuation, SharedGridData, the fragmentainer baselines), masonry (the masonry branches of
// ComputeIntrinsicISize and Reflow), subgrid (the branches that need a parent grid's track sizes), the
// scrolled-content overflow of Reflow (there are no scroll frames), and fieldset in GetGridContainerFrame.

impl nsGridContainerFrame {
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.NormalizeChildLists();

        if self.IsHiddenByContentVisibilityOfInFlowParentForLayout() {
            return;
        }

        self.MarkInReflow();
        // NOT PORTED: DO_GLOBAL_REFLOW_COUNT
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        // GRID_LOG("Reflow grid container frame %p", this);
        // NOT PORTED: logging.

        if self.IsFrameTreeTooDeep(aReflowInput, aDesiredSize, aStatus) {
            return;
        }

        #[cfg(debug_assertions)]
        {
            // mDidPushItemsBitMayLie = false;
            // SanityCheckChildListsBeforeReflow();
            // NOT PORTED: debug checks of the fragmentation child lists.
        }

        for perAxisBaseline in self.Members().mBaseline.0.iter_mut() {
            for baseline in perAxisBaseline.0.iter_mut() {
                *baseline = NS_INTRINSIC_ISIZE_UNKNOWN;
            }
        }

        let stylePos = &aReflowInput.mStylePosition;
        let prevInFlow = self.GetPrevInFlow().map(nsGridContainerFrame::from_frame);
        if prevInFlow.is_none() {
            self.InitImplicitNamedAreas(stylePos);
        } else {
            MOZ_ASSERT!(prevInFlow.unwrap().HasAnyStateBits(kIsSubgridBits) == self.HasAnyStateBits(kIsSubgridBits), "continuations should have same kIsSubgridBits");
        }
        let mut gridRI = GridReflowInput::new__nsGridContainerFrame_ReflowInput(*self, aReflowInput);
        if gridRI.mIter.ItemsAreAlreadyInOrder() {
            self.AddStateBits(NS_STATE_GRID_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER);
        } else {
            self.RemoveStateBits(NS_STATE_GRID_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER);
        }
        if gridRI.mIter.AtEnd() || aReflowInput.mStyleDisplay.IsContainLayout() {
            // We have no grid items, or we're layout-contained. So, we have no
            // baseline, and our parent should synthesize a baseline if needed.
            self.AddStateBits(NS_STATE_GRID_SYNTHESIZE_BASELINE);
        } else {
            self.RemoveStateBits(NS_STATE_GRID_SYNTHESIZE_BASELINE);
        }
        let computedBSize = aReflowInput.ComputedBSize();
        let computedISize = aReflowInput.ComputedISize();

        // XXX Technically incorrect: 'contain-intrinsic-block-size: none' is
        // treated as 0, ignoring our row sizes, when really we should use them but
        // *they* should be computed as if we had no children. To be fixed in bug
        // 1488878.
        let containIntrinsicBSize: Option<nscoord> = aReflowInput.mFrame.ContainIntrinsicBSize(0);
        let wm = gridRI.mWM;

        let mut consumedBSize: nscoord = 0;
        let mut contentBSize: nscoord = 0;
        if prevInFlow.is_none() {
            let mut grid = Grid::new(std::ptr::null());
            if !self.IsSubgrid__() {
                let repeatSizing = RepeatTrackSizingInput::new(&aReflowInput.ComputedMinSize__(), &aReflowInput.ComputedSize__(), &aReflowInput.ComputedMaxSize__());
                grid.PlaceGridItems(&mut gridRI, &repeatSizing);
            } else {
                // auto* subgrid = GetProperty(Subgrid::Prop());
                // MOZ_ASSERT(subgrid, "an ancestor forgot to call PlaceGridItems?");
                // gridRI.mGridItems = subgrid->mGridItems.Clone();
                // gridRI.mAbsPosItems = subgrid->mAbsPosItems.Clone();
                // grid.mGridColEnd = subgrid->mGridColEnd;
                // grid.mGridRowEnd = subgrid->mGridRowEnd;
                // NOT PORTED: subgrid (IsSubgrid() is never true in this runtime).
                unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)");
            }

            // Resolve the column sizes with the grid container's inline size.
            // 12.1.1: https://drafts.csswg.org/css-grid-2/#algo-grid-sizing
            gridRI.CalculateTrackSizesForAxis(LogicalAxis::Inline, &grid, computedISize, SizingConstraint::NoConstraint);

            let mut bSizeForResolvingRowSizes = self.ComputeBSizeForResolvingRowSizes(&mut gridRI, computedBSize, containIntrinsicBSize);

            // Resolve the row sizes with the determined bSizeForResolvingRowSizes.
            // 12.1.2: https://drafts.csswg.org/css-grid-2/#algo-grid-sizing
            //
            // If bSizeForResolvingRowSizes is unconstrained, that's fine. It forces
            // percent-valued row sizes to be treated as 'auto', yielding an intrinsic
            // content block-size needed later to *actually* resolve percent-valued row
            // gaps and row sizes.
            gridRI.CalculateTrackSizesForAxis(LogicalAxis::Block, &grid, bSizeForResolvingRowSizes, SizingConstraint::NoConstraint);

            // Invalidate the column sizes before re-resolving them.
            gridRI.InvalidateTrackSizesForAxis(LogicalAxis::Inline);

            // Re-resolve the column sizes.
            // 12.1.3: https://drafts.csswg.org/css-grid-2/#algo-grid-sizing
            gridRI.CalculateTrackSizesForAxis(LogicalAxis::Inline, &grid, computedISize, SizingConstraint::NoConstraint);

            // If our bSizeForResolvingRowSizes is still indefinite, replace it with
            // the sum of the row sizes we just resolved, then re-resolve the row
            // sizes against that value. We skip this for masonry, which doesn't need
            // two-pass row sizes resolution.
            if bSizeForResolvingRowSizes == NS_UNCONSTRAINEDSIZE && !self.IsRowMasonry() {
                bSizeForResolvingRowSizes = std::cmp::max(gridRI.mRows.TotalTrackSizeWithoutAlignment(self), gridRI.mReflowInput.unwrap().ComputedMinBSize());

                NS_ASSERTION!(bSizeForResolvingRowSizes != NS_UNCONSTRAINEDSIZE, "The block-size for re-resolving the row sizes should be definite in non-masonry layout!");

                // Invalidate the row sizes before re-resolving them.
                gridRI.InvalidateTrackSizesForAxis(LogicalAxis::Block);

                // Re-resolve the row sizes.
                // 12.1.4: https://drafts.csswg.org/css-grid-2/#algo-grid-sizing
                gridRI.CalculateTrackSizesForAxis(LogicalAxis::Block, &grid, bSizeForResolvingRowSizes, SizingConstraint::NoConstraint);
            }

            if computedBSize == NS_UNCONSTRAINEDSIZE || aReflowInput.ShouldApplyAutomaticMinimumOnBlockAxis() {
                // We either have an unconstrained block-size, or we have a definite
                // block-size derived from the inline-size (transferred via aspect-ratio)
                // and need to apply the automatic content-based minimum sizes on the
                // block-axis. In both case, we need to compute the intrinsic
                // content block-size.
                contentBSize = self.ComputeIntrinsicContentBSize(&gridRI, computedBSize, bSizeForResolvingRowSizes, containIntrinsicBSize);
            }
        } else {
            // consumedBSize = CalcAndCacheConsumedBSize();
            // gridRI.InitializeForContinuation(this, consumedBSize);
            // if (containIntrinsicBSize) {
            //   contentBSize = *containIntrinsicBSize;
            // } else {
            //   const uint32_t numRows = gridRI.mRows.mSizes.Length();
            //   contentBSize =
            //       gridRI.mRows.GridLineEdge(numRows, GridLineSide::AfterGridGap);
            // }
            // NOT PORTED: fragmentation (grid container continuations).
            unimplemented!("NOT PORTED: fragmentation (grid container continuations)");
        }
        if computedBSize == NS_UNCONSTRAINEDSIZE {
            contentBSize = aReflowInput.ApplyMinMaxBSize(contentBSize, 0);
        } else if aReflowInput.ShouldApplyAutomaticMinimumOnBlockAxis() {
            contentBSize = aReflowInput.ApplyMinMaxBSize(contentBSize, 0);
            contentBSize = std::cmp::max(contentBSize, computedBSize);
        } else {
            contentBSize = computedBSize;
        }
        if contentBSize != NS_UNCONSTRAINEDSIZE {
            contentBSize = std::cmp::max(contentBSize - consumedBSize, 0);
        }
        // ADAPTER: `auto& bp = gridRI.mBorderPadding;` is a copy here (gridRI is borrowed mutably by
        // ReflowChildren below; nothing reads gridRI.mBorderPadding after bp.BEnd is changed).
        let mut bp = gridRI.mBorderPadding;
        let mut contentArea = LogicalRect::new(wm, bp.IStart(wm), bp.BStart(wm), computedISize, contentBSize);

        if prevInFlow.is_none() {
            if !self.IsRowSubgrid() {
                if !gridRI.mRows.mIsMasonry {
                    // Apply 'align-content' to the grid.
                    let alignment = stylePos.mAlignContent();
                    gridRI.mRows.AlignJustifyContent(stylePos, alignment, wm, contentBSize, false);
                }
            } else {
                // if (computedBSize == NS_UNCONSTRAINEDSIZE) {
                //   contentBSize = gridRI.mRows.GridLineEdge(rowSizes.Length(),
                //                                            GridLineSide::BeforeGridGap);
                //   contentArea.BSize(wm) = std::max(contentBSize, nscoord(0));
                // }
                // NOT PORTED: subgrid.
                unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)");
            }
            // Save the final row sizes for use by subgrids, if needed.
            if self.HasSubgridItems__() || self.IsSubgrid__() {
                self.StoreUsedTrackSizes(LogicalAxis::Block, &gridRI.mRows.mSizes);
            }
        }

        let mut containerSize = contentArea.Size(wm).GetPhysicalSize(wm);
        let mut repositionChildren = false;
        if containerSize.width == NS_UNCONSTRAINEDSIZE && wm.IsVerticalRL() {
            // Note that writing-mode:vertical-rl is the only case where the block
            // logical direction progresses in a negative physical direction, and
            // therefore block-dir coordinate conversion depends on knowing the width
            // of the coordinate space in order to translate between the logical and
            // physical origins.
            //
            // A masonry axis size may be unconstrained, otherwise in a regular grid
            // our intrinsic size is always known by now.  We'll re-position
            // the children below once our size is known.
            repositionChildren = true;
            containerSize.width = 0;
        }
        containerSize.width += bp.LeftRight(wm);
        containerSize.height += bp.TopBottom(wm);

        contentBSize = self.ReflowChildren(&mut gridRI, &contentArea, &containerSize, aDesiredSize, aStatus);
        // if (Style()->GetPseudoType() == PseudoStyleType::MozScrolledContent) { ... }
        // NOT PORTED: scrolled content (nsGridContainerFrame.cpp lines 9632-9677 at GECKO_REV): there are no scroll
        // frames, so no frame is a ::-moz-scrolled-content grid container.
        self.ReflowAbsoluteChildren(&mut gridRI, &contentArea, contentBSize, aDesiredSize, aStatus);
        contentBSize = std::cmp::max(contentBSize - consumedBSize, 0);

        // Skip our block-end border if we're INCOMPLETE.
        if !aStatus.IsComplete() && !gridRI.mSkipSides.BEnd() && self.StyleBorder().mBoxDecorationBreak() != StyleBoxDecorationBreak::Clone {
            *bp.BEnd_mut(wm) = 0;
        }

        let mut desiredSize = LogicalSize::new(wm, computedISize + bp.IStartEnd(wm), contentBSize + bp.BStartEnd(wm));
        aDesiredSize.SetSize(wm, &desiredSize);
        let frameRect = nsRect::new(0, 0, aDesiredSize.Width(), aDesiredSize.Height());
        aDesiredSize.mOverflowAreas.UnionAllWith(frameRect);

        if repositionChildren {
            let physicalDelta = nsPoint::new(aDesiredSize.Width() - bp.LeftRight(wm), 0);
            for item in gridRI.mGridItems.iter() {
                let child = item.mFrame;
                child.MovePositionBy(physicalDelta);
                self.ConsiderChildOverflow(&mut aDesiredSize.mOverflowAreas, child, OverflowAreaUnionFlags::None);
            }
        }

        // TODO: fix align-tracks alignment in fragments
        if (self.IsRowMasonry() && prevInFlow.is_none()) || self.IsColMasonry() {
            // gridRI.AlignJustifyTracksInMasonryAxis(contentArea.Size(wm), aDesiredSize.PhysicalSize());
            // NOT PORTED: masonry.
            unimplemented!("NOT PORTED: masonry layout");
        }

        // Convert INCOMPLETE -> OVERFLOW_INCOMPLETE and zero bsize if we're an OC.
        if self.HasAnyStateBits(NS_FRAME_IS_OVERFLOW_CONTAINER) {
            if !aStatus.IsComplete() {
                aStatus.SetOverflowIncomplete();
                aStatus.SetNextInFlowNeedsReflow();
            }
            contentBSize = 0;
            *desiredSize.BSize_mut(wm) = contentBSize + bp.BStartEnd(wm);
            aDesiredSize.SetSize(wm, &desiredSize);
        }

        if !gridRI.mInFragmentainer {
            MOZ_ASSERT!(gridRI.mIter.IsValid());
            let sz = frameRect.Size();
            let colsLength = gridRI.mCols.mSizes.Length() as u32;
            self.CalculateBaselines(BaselineSet::eBoth, Some(&mut gridRI.mIter), Some(&gridRI.mGridItems), &gridRI.mCols, 0, colsLength, wm, sz, bp.IStart(wm), bp.IEnd(wm), desiredSize.ISize(wm));
            let rowsLength = gridRI.mRows.mSizes.Length() as u32;
            self.CalculateBaselines(BaselineSet::eBoth, Some(&mut gridRI.mIter), Some(&gridRI.mGridItems), &gridRI.mRows, 0, rowsLength, wm, sz, bp.BStart(wm), bp.BEnd(wm), desiredSize.BSize(wm));
        } else {
            // Only compute 'first baseline' if this fragment contains the first track.
            // ... (the fragmentainer case: nsGridContainerFrame.cpp lines 9726-9772 at GECKO_REV)
            // NOT PORTED: fragmentation (gridRI.mInFragmentainer is never set: ReflowInFragmentainer is not ported).
            unimplemented!("NOT PORTED: fragmentation (baselines in a fragmentainer)");
        }

        if self.HasAnyStateBits(NS_STATE_GRID_COMPUTED_INFO) {
            // This state bit will never be cleared, since reflow can be called
            // multiple times in fragmented grids, and it's challenging to scope
            // the bit to only that sequence of calls. This is relatively harmless
            // since this bit is only set by accessing a ChromeOnly property, and
            // therefore can't unduly slow down normal web browsing.
            //
            // (the ComputedGridTrackInfo, ComputedGridLineInfo and explicit named areas properties for devtools:
            // nsGridContainerFrame.cpp lines 9776-10033 at GECKO_REV)
            // NOT PORTED: devtools data (only GetGridFrameWithComputedInfo sets the bit, and it is not ported).
            unimplemented!("NOT PORTED: devtools grid info (NS_STATE_GRID_COMPUTED_INFO)");
        }

        if prevInFlow.is_none() {
            if !aStatus.IsFullyComplete() {
                // SharedGridData* sharedGridData =
                //     GetOrCreateDeletableProperty(SharedGridData::Prop());
                // ... (nsGridContainerFrame.cpp lines 10037-10069 at GECKO_REV: stores the track sizes and
                // the grid items for the continuations)
                // NOT PORTED: fragmentation (nothing reflows a continuation).
                unimplemented!("NOT PORTED: fragmentation (SharedGridData for continuations)");
            } else if self.GetNextInFlow().is_none() {
                SharedGridData::RemoveProp(self.0);
            }
        }

        self.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(aDesiredSize, None);
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::UpdateSubgridFrameState
    pub fn UpdateSubgridFrameState(&self) {
        let oldBits: nsFrameState = self.GetStateBits() & kIsSubgridBits;
        let newBits: nsFrameState = self.ComputeSelfSubgridMasonryBits() & kIsSubgridBits;
        if newBits != oldBits {
            self.RemoveStateBits(kIsSubgridBits);
            if newBits == 0 {
                Subgrid::RemoveProp(self.0);
            } else {
                self.AddStateBits(newBits);
            }
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ComputeSelfSubgridMasonryBits
    pub fn ComputeSelfSubgridMasonryBits(&self) -> nsFrameState {
        let mut bits: nsFrameState = 0;
        let pos = self.StylePosition();

        // We can only have masonry layout in one axis.
        if pos.mGridTemplateRows().IsMasonry() {
            bits |= NS_STATE_GRID_IS_ROW_MASONRY;
        } else if pos.mGridTemplateColumns().IsMasonry() {
            bits |= NS_STATE_GRID_IS_COL_MASONRY;
        }

        // NOTE: The rest of this function is only relevant if we're a subgrid;
        // hence, we return early as soon as we rule out that possibility.

        // 'contain:layout/paint' makes us an "independent formatting context",
        // which prevents us from being a subgrid in this case (but not always).
        // We will also need to check our containing scroll frame for this property.
        // https://drafts.csswg.org/css-display-3/#establish-an-independent-formatting-context
        if nsGridContainerFrame::ShouldInhibitSubgridDueToIFC(self.0) {
            return bits;
        }

        // Skip over our scroll frame and such if we have it, to find our "parent
        // grid", if we have one.

        // After this loop, 'parent' will represent the parent of the outermost frame
        // that shares our content node. (Normally this is just our parent frame, but
        // if we're e.g. a scrolled frame, then this will be the parent of our
        // wrapper-scrollable-frame.) If 'parent' turns out to be a grid container,
        // then it's our "parent grid", and we could potentially be a subgrid of it.
        let mut parent = self.GetParent();
        while let Some(p) = parent {
            if p.GetContent() != self.GetContent() {
                break;
            }
            // If we find our containing frame (e.g. our scroll frame) can't be a
            // subgrid, then we can't be a subgrid, for the same reasons as above. This
            // can happen when this frame is itself a grid item with "overflow:scroll"
            // or similar.
            if nsGridContainerFrame::ShouldInhibitSubgridDueToIFC(p) {
                return bits;
            }
            parent = p.GetParent();
        }
        let parentGrid = parent.and_then(nsGridContainerFrame::do_QueryFrame);
        if parentGrid.is_some() {
            let parent = parent.unwrap();
            let isOrthogonal = self.GetWritingMode().IsOrthogonalTo(&parent.GetWritingMode());
            let mut isColSubgrid = pos.mGridTemplateColumns().IsSubgrid();
            // Subgridding a parent masonry axis makes us use masonry layout too,
            // unless our other axis is a masonry axis.
            if isColSubgrid && parent.HasAnyStateBits(if isOrthogonal { NS_STATE_GRID_IS_ROW_MASONRY } else { NS_STATE_GRID_IS_COL_MASONRY }) {
                isColSubgrid = false;
                if !self.HasAnyStateBits(NS_STATE_GRID_IS_ROW_MASONRY) {
                    bits |= NS_STATE_GRID_IS_COL_MASONRY;
                }
            }
            if isColSubgrid {
                bits |= NS_STATE_GRID_IS_COL_SUBGRID;
            }

            let mut isRowSubgrid = pos.mGridTemplateRows().IsSubgrid();
            if isRowSubgrid && parent.HasAnyStateBits(if isOrthogonal { NS_STATE_GRID_IS_COL_MASONRY } else { NS_STATE_GRID_IS_ROW_MASONRY }) {
                isRowSubgrid = false;
                if !self.HasAnyStateBits(NS_STATE_GRID_IS_COL_MASONRY) {
                    bits |= NS_STATE_GRID_IS_ROW_MASONRY;
                }
            }
            if isRowSubgrid {
                bits |= NS_STATE_GRID_IS_ROW_SUBGRID;
            }
        }
        bits
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::Init
    // ADAPTER: the frame is created by FrameTree::AddFrame (see nsFlexContainerFrame::Init).
    pub fn Init(&self, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: Option<nsIFrame>) {
        self.nsContainerFrame_Init(aContent, aParent, aPrevInFlow);

        // if (HasAnyStateBits(NS_FRAME_FONT_INFLATION_CONTAINER)) { AddStateBits(NS_FRAME_FONT_INFLATION_FLOW_ROOT); }
        // NOT PORTED: font inflation.

        let mut bits: nsFrameState = 0;
        if aPrevInFlow.is_none() {
            bits = self.ComputeSelfSubgridMasonryBits();
        } else {
            bits = aPrevInFlow.unwrap().GetStateBits() & (NS_STATE_GRID_IS_ROW_MASONRY | NS_STATE_GRID_IS_COL_MASONRY | kIsSubgridBits | NS_STATE_GRID_HAS_COL_SUBGRID_ITEM | NS_STATE_GRID_HAS_ROW_SUBGRID_ITEM);
        }
        self.AddStateBits(bits);
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::DidSetComputedStyle
    pub fn DidSetComputedStyle(&self, aOldStyle: Option<()>) {
        self.nsContainerFrame_DidSetComputedStyle(aOldStyle);

        if aOldStyle.is_none() {
            return; // Init() already initialized the bits.
        }
        self.UpdateSubgridFrameState();
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ComputeIntrinsicISize
    pub fn ComputeIntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        // GRID_LOG("Compute %s isize for grid container frame %p",
        //          aType == IntrinsicISizeType::MinISize ? "min" : "pref", this);
        // NOT PORTED: logging.

        if let Some(containISize) = self.ContainIntrinsicISize() {
            return containISize;
        }

        // Calculate the sum of column sizes under intrinsic sizing.
        // https://drafts.csswg.org/css-grid-2/#intrinsic-sizes
        self.NormalizeChildLists();
        let mut gridRI = GridReflowInput::new__nsGridContainerFrame_gfxContext(*self, aInput.mContext.unwrap());
        // Ensure we do not measure flex tracks against unconstrained bounds.
        gridRI.mIsGridIntrinsicSizing = true;
        self.InitImplicitNamedAreas(&gridRI.mGridStyle); // XXX optimize

        // The min/sz/max sizes are the input to the "repeat-to-fill" algorithm:
        // https://drafts.csswg.org/css-grid-2/#auto-repeat
        // They're only used for auto-repeat so we skip computing them otherwise.
        let mut repeatSizing = RepeatTrackSizingInput::new__WritingMode(gridRI.mWM);
        if !self.IsColSubgrid() && gridRI.mColFunctions.mHasRepeatAuto {
            repeatSizing.InitFromStyle(LogicalAxis::Inline, gridRI.mWM, gridRI.mFrame.0, &gridRI.mFrame.Style(), &gridRI.mFrame.GetAspectRatio(), &aInput.mContainingBlockSize);
        }
        if (!self.IsRowSubgrid() && gridRI.mRowFunctions.mHasRepeatAuto && !gridRI.mGridStyle.mGridAutoFlow().contains(StyleGridAutoFlow::ROW)) || self.IsColMasonry() {
            // Only 'grid-auto-flow:column' can create new implicit columns, so that's
            // the only case where our block-size can affect the number of columns.
            // Masonry layout always depends on how many rows we have though.
            repeatSizing.InitFromStyle(LogicalAxis::Block, gridRI.mWM, gridRI.mFrame.0, &gridRI.mFrame.Style(), &gridRI.mFrame.GetAspectRatio(), &aInput.mContainingBlockSize);
        }

        let mut grid = Grid::new(std::ptr::null());
        if !self.IsSubgrid__() {
            grid.PlaceGridItems(&mut gridRI, &repeatSizing); // XXX optimize
        } else {
            // auto* subgrid = GetProperty(Subgrid::Prop());
            // gridRI.mGridItems = subgrid->mGridItems.Clone();
            // gridRI.mAbsPosItems = subgrid->mAbsPosItems.Clone();
            // grid.mGridColEnd = subgrid->mGridColEnd;
            // grid.mGridRowEnd = subgrid->mGridRowEnd;
            // NOT PORTED: subgrid.
            unimplemented!("NOT PORTED: subgrid (grid-template-*: subgrid)");
        }

        let constraint = if aType == IntrinsicISizeType::MinISize { SizingConstraint::MinContent } else { SizingConstraint::MaxContent };
        if self.IsColMasonry() {
            // ReflowOutput desiredSize(gridRI.mWM);
            // nsSize containerSize;
            // LogicalRect contentArea(gridRI.mWM);
            // nsReflowStatus status;
            // gridRI.mRows.mSizes.SetLength(grid.mGridRowEnd);
            // gridRI.CalculateTrackSizesForAxis(LogicalAxis::Inline, grid,
            //                                   NS_UNCONSTRAINEDSIZE, constraint);
            // return MasonryLayout(gridRI, contentArea, constraint, desiredSize, status,
            //                      nullptr, containerSize);
            // NOT PORTED: masonry.
            unimplemented!("NOT PORTED: masonry layout");
        }

        if grid.mGridColEnd == 0 {
            return 0;
        }

        gridRI.CalculateTrackSizesForAxis(LogicalAxis::Inline, &grid, NS_UNCONSTRAINEDSIZE, constraint);

        let contentBoxBSize: nscoord = match aInput.mPercentageBasisForChildren {
            Some(pb) => pb.BSize(gridRI.mWM),
            None => NS_UNCONSTRAINEDSIZE,
        };

        // Resolve row sizes so that when we re-resolve the column sizes, grid items
        // with percent-valued block-sizes (and aspect ratios) have definite row
        // sizes as the percentage basis. Their resolved block-size can then
        // transfer to the inline-axis, contributing correctly to the grid
        // container's intrinsic inline-size.
        gridRI.CalculateTrackSizesForAxis(LogicalAxis::Block, &grid, contentBoxBSize, SizingConstraint::NoConstraint);

        // Invalidate the column sizes before re-resolving them.
        gridRI.InvalidateTrackSizesForAxis(LogicalAxis::Inline);

        // Re-resolve the column sizes, using the resolved row sizes established
        // above. See 12.1.3 of the Grid Sizing Algorithm for more scenarios where
        // re-resolving the column sizes is necessary:
        // https://drafts.csswg.org/css-grid-2/#algo-grid-sizing
        gridRI.CalculateTrackSizesForAxis(LogicalAxis::Inline, &grid, NS_UNCONSTRAINEDSIZE, constraint);

        gridRI.mCols.TotalTrackSizeWithoutAlignment(self)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        let firstCont = nsGridContainerFrame::from_frame(self.FirstContinuation());
        if firstCont != *self {
            return firstCont.IntrinsicISize(aInput, aType);
        }
        let this = *self;
        self.Members().mCachedIntrinsicSizes.GetOrSet(self.0, aType, aInput, || this.ComputeIntrinsicISize(aInput, aType))
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::MarkIntrinsicISizesDirty
    pub fn MarkIntrinsicISizesDirty(&self) {
        self.Members().mCachedIntrinsicSizes.Clear();
        for perAxisBaseline in self.Members().mBaseline.0.iter_mut() {
            for baseline in perAxisBaseline.0.iter_mut() {
                *baseline = NS_INTRINSIC_ISIZE_UNKNOWN;
            }
        }
        self.nsContainerFrame_MarkIntrinsicISizesDirty();
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::BuildDisplayList
    // NOT PORTED: painting (display lists). nsGridContainerFrame.cpp lines 10273-10304 at GECKO_REV.

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::DrainSelfOverflowList
    pub fn DrainSelfOverflowList(&self) -> bool {
        self.DrainAndMergeSelfOverflowList()
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::AppendFrames
    pub fn AppendFrames(&self, aListID: FrameChildListID, aFrameList: nsFrameList) {
        self.NoteNewChildren(aListID, &aFrameList);
        self.nsContainerFrame_AppendFrames(aListID, aFrameList);
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::InsertFrames
    pub fn InsertFrames(&self, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aPrevFrameLine: Option<&nsLineList_iterator>, aFrameList: nsFrameList) {
        self.NoteNewChildren(aListID, &aFrameList);
        self.nsContainerFrame_InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList);
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::RemoveFrame
    pub fn RemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        MOZ_ASSERT!(aListID == FrameChildListID::Principal, "unexpected child list");

        #[cfg(debug_assertions)]
        self.SetDidPushItemsBitIfNeeded(aListID, aOldFrame);

        self.nsContainerFrame_RemoveFrame(aContext, aListID, aOldFrame);
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::SynthesizeBaseline
    // ADAPTER: aGridItems is the array that aGridOrderItem.mItem indexes (C++ holds a pointer to the item).
    pub fn SynthesizeBaseline(&self, aGridOrderItem: &FindItemInGridOrderResult, aGridItems: &Vec<GridItemInfo>, aAxis: LogicalAxis, aGroup: BaselineSharingGroup, aCBPhysicalSize: nsSize, aCBSize: nscoord, aCBWM: WritingMode) -> nscoord {
        let item = match aGridOrderItem.mItem {
            Some(index) => &aGridItems[index],
            None => {
                // No item in this fragment - synthesize a baseline from our border-box.
                return SynthesizeBaselineFromBorderBox(aGroup, aCBWM, aAxis, aCBSize);
            }
        };

        let child = item.mFrame;
        let grid = nsGridContainerFrame::do_QueryFrame(child);
        let childWM = child.GetWritingMode();
        let isOrthogonal = aCBWM.IsOrthogonalTo(&childWM);
        let childAxis: LogicalAxis = aCBWM.ConvertAxisTo(aAxis, childWM);
        let baseline: nscoord;
        let start: nscoord;
        let size: nscoord;

        if aAxis == LogicalAxis::Block {
            start = child.GetLogicalNormalPosition(aCBWM, aCBPhysicalSize).B(aCBWM);
            size = child.BSize(aCBWM);
            if grid.is_some() && aGridOrderItem.mIsInEdgeTrack {
                let grid = grid.unwrap();
                baseline = if isOrthogonal { grid.GetIBaseline(aGroup) } else { grid.GetBBaseline(aGroup) };
            } else if !isOrthogonal && aGridOrderItem.mIsInEdgeTrack {
                // This assertion is mostly for documentation purposes; it must hold,
                // given the checks in our 'if' statements. (We know aAxis is
                // LogicalAxis::Block, and isOrthogonal is false, which means childAxis
                // must be LogicalAxis::Block). If instead we got here with a childAxis of
                // LogicalAxis::Inline, then our call to
                // Baseline::SynthesizeBaselineFromBorderBox might incorrectly think
                // it makes sense to use a central baseline, in an axis where that
                // doesn't make sense.
                MOZ_ASSERT!(childAxis == LogicalAxis::Block, "unexpected childAxis");
                baseline = child.GetNaturalBaselineBOffset(childWM, aGroup, BaselineExportContext::Other).unwrap_or_else(|| Baseline::SynthesizeBOffsetFromBorderBox(child, childWM, aGroup));
            } else {
                baseline = SynthesizeBaselineFromBorderBox(aGroup, childWM, childAxis, size);
            }
        } else {
            start = child.GetLogicalNormalPosition(aCBWM, aCBPhysicalSize).I(aCBWM);
            size = child.ISize(aCBWM);
            if grid.is_some() && aGridOrderItem.mIsInEdgeTrack {
                let grid = grid.unwrap();
                baseline = if isOrthogonal { grid.GetBBaseline(aGroup) } else { grid.GetIBaseline(aGroup) };
            } else if isOrthogonal && aGridOrderItem.mIsInEdgeTrack {
                baseline = child.GetNaturalBaselineBOffset(childWM, aGroup, BaselineExportContext::Other).unwrap_or_else(|| SynthesizeBaselineFromBorderBox(aGroup, childWM, childAxis, size));
            } else {
                baseline = SynthesizeBaselineFromBorderBox(aGroup, childWM, childAxis, size);
            }
        }
        if aGroup == BaselineSharingGroup::First { start + baseline } else { aCBSize - start - size + baseline }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::CalculateBaselines
    // ADAPTER: aIter is `Option<&mut ...>` (C++ passes a pointer that may be null), aGridItems is
    // `Option<&Vec<GridItemInfo>>`.
    pub fn CalculateBaselines(&self, aBaselineSet: BaselineSet, mut aIter: Option<&mut CSSOrderAwareFrameIterator>, aGridItems: Option<&Vec<GridItemInfo>>, aTracks: &Tracks, aFragmentStartTrack: u32, aFirstExcludedTrack: u32, aWM: WritingMode, aCBPhysicalSize: nsSize, aCBBorderPaddingStart: nscoord, aCBBorderPaddingEnd: nscoord, aCBSize: nscoord) {
        let axis = aTracks.mAxis;

        let firstBaseline: Option<nscoord> = aTracks.GetBaseline(0, BaselineSharingGroup::First);
        if !(aBaselineSet & BaselineSet::eFirst).to_bool() {
            self.Members().mBaseline[axis][BaselineSharingGroup::First] = SynthesizeBaselineFromBorderBox(BaselineSharingGroup::First, aWM, axis, aCBSize);
        } else if firstBaseline.is_none() {
            let gridOrderFirstItem: FindItemInGridOrderResult = nsGridContainerFrame::FindFirstItemInGridOrder(
                aIter.as_deref_mut().unwrap(),
                aGridItems.unwrap(),
                if axis == LogicalAxis::Block { GridArea_mRows } else { GridArea_mCols },
                if axis == LogicalAxis::Block { GridArea_mCols } else { GridArea_mRows },
                aFragmentStartTrack,
            );
            let baseline = self.SynthesizeBaseline(&gridOrderFirstItem, aGridItems.unwrap(), axis, BaselineSharingGroup::First, aCBPhysicalSize, aCBSize, aWM);
            self.Members().mBaseline[axis][BaselineSharingGroup::First] = baseline;
        } else {
            // We have a 'first baseline' group in the start track in this fragment.
            // Convert it from track to grid container border-box coordinates.
            MOZ_ASSERT!(!aGridItems.unwrap().is_empty());
            let gapBeforeStartTrack: nscoord = if aFragmentStartTrack == 0 {
                aTracks.GridLineEdge(aFragmentStartTrack, GridLineSide::AfterGridGap)
            } else {
                0 // no content gap at start of fragment
            };
            self.Members().mBaseline[axis][BaselineSharingGroup::First] = aCBBorderPaddingStart + gapBeforeStartTrack + firstBaseline.unwrap();
        }

        // (a `Length() - 1` on an empty array wraps in C++ and GetBaseline then returns Nothing)
        let lastBaseline: Option<nscoord> = aTracks.GetBaseline((aTracks.mBaselines.len() as u32).wrapping_sub(1), BaselineSharingGroup::Last);
        if !(aBaselineSet & BaselineSet::eLast).to_bool() {
            self.Members().mBaseline[axis][BaselineSharingGroup::Last] = SynthesizeBaselineFromBorderBox(BaselineSharingGroup::Last, aWM, axis, aCBSize);
        } else if lastBaseline.is_none() {
            // For finding items for the 'last baseline' we need to create a reverse
            // iterator ('aIter' is the forward iterator from the GridReflowInput).
            type Iter = ReverseCSSOrderAwareFrameIterator;
            let orderState = if aIter.as_deref().unwrap().ItemsAreAlreadyInOrder() { CSSOrderAwareFrameIterator_OrderState::Ordered } else { CSSOrderAwareFrameIterator_OrderState::Unordered };
            let mut iter = Iter::new(self.0, FrameChildListID::Principal, CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders, orderState, CSSOrderAwareFrameIterator_OrderingProperty::Order);
            iter.SetItemCount(aGridItems.unwrap().len());
            let gridOrderLastItem: FindItemInGridOrderResult = nsGridContainerFrame::FindLastItemInGridOrder(
                &mut iter,
                aGridItems.unwrap(),
                if axis == LogicalAxis::Block { GridArea_mRows } else { GridArea_mCols },
                if axis == LogicalAxis::Block { GridArea_mCols } else { GridArea_mRows },
                aFragmentStartTrack,
                aFirstExcludedTrack,
            );
            let baseline = self.SynthesizeBaseline(&gridOrderLastItem, aGridItems.unwrap(), axis, BaselineSharingGroup::Last, aCBPhysicalSize, aCBSize, aWM);
            self.Members().mBaseline[axis][BaselineSharingGroup::Last] = baseline;
        } else {
            // We have a 'last baseline' group in the end track in this fragment.
            // Convert it from track to grid container border-box coordinates.
            MOZ_ASSERT!(!aGridItems.unwrap().is_empty());
            let borderBoxStartToEndOfEndTrack = aCBBorderPaddingStart + aTracks.GridLineEdge(aFirstExcludedTrack, GridLineSide::BeforeGridGap) - aTracks.GridLineEdge(aFragmentStartTrack, GridLineSide::BeforeGridGap);
            self.Members().mBaseline[axis][BaselineSharingGroup::Last] = (aCBSize - borderBoxStartToEndOfEndTrack) + lastBaseline.unwrap();
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GetFrameName, ExtraContainerFrameInfo
    // NOT PORTED: debug frame dump (DEBUG_FRAME_DUMP).

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::FindFirstItemInGridOrder
    // ADAPTER: the result holds the index of the item in aGridItems (C++ holds a pointer to it).
    pub fn FindFirstItemInGridOrder(aIter: &mut CSSOrderAwareFrameIterator, aGridItems: &Vec<GridItemInfo>, aMajor: LineRangeSelector, aMinor: LineRangeSelector, aFragmentStartTrack: u32) -> FindItemInGridOrderResult {
        let mut result = FindItemInGridOrderResult { mItem: None, mIsInEdgeTrack: false };
        let mut minMajor: u32 = kTranslatedMaxLine + 1;
        let mut minMinor: u32 = kTranslatedMaxLine + 1;
        aIter.Reset(CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders);
        while !aIter.AtEnd() {
            let item: &GridItemInfo = &aGridItems[aIter.ItemIndex()];
            if aMajor(&item.mArea).mEnd <= aFragmentStartTrack {
                aIter.Next();
                continue; // item doesn't span any track in this fragment
            }
            let major: u32 = aMajor(&item.mArea).mStart;
            let minor: u32 = aMinor(&item.mArea).mStart;
            if major < minMajor || (major == minMajor && minor < minMinor) {
                minMajor = major;
                minMinor = minor;
                result.mItem = Some(aIter.ItemIndex());
                result.mIsInEdgeTrack = major == 0u32;
            }
            aIter.Next();
        }
        result
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::FindLastItemInGridOrder
    pub fn FindLastItemInGridOrder(aIter: &mut ReverseCSSOrderAwareFrameIterator, aGridItems: &Vec<GridItemInfo>, aMajor: LineRangeSelector, aMinor: LineRangeSelector, aFragmentStartTrack: u32, aFirstExcludedTrack: u32) -> FindItemInGridOrderResult {
        let mut result = FindItemInGridOrderResult { mItem: None, mIsInEdgeTrack: false };
        let mut maxMajor: i32 = -1;
        let mut maxMinor: i32 = -1;
        aIter.Reset(CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders);
        let lastMajorTrack: i32 = aFirstExcludedTrack as i32 - 1;
        while !aIter.AtEnd() {
            let item: &GridItemInfo = &aGridItems[aIter.ItemIndex()];
            // Subtract 1 from the end line to get the item's last track index.
            let major: i32 = aMajor(&item.mArea).mEnd as i32 - 1;
            // Currently, this method is only called with aFirstExcludedTrack ==
            // the first track in the next fragment, so we take the opportunity
            // to assert this item really belongs to this fragment.
            MOZ_ASSERT!(aMajor(&item.mArea).mStart < aFirstExcludedTrack, "found an item that belongs to some later fragment");
            if major < aFragmentStartTrack as i32 {
                aIter.Next();
                continue; // item doesn't span any track in this fragment
            }
            let minor: i32 = aMinor(&item.mArea).mEnd as i32 - 1;
            MOZ_ASSERT!(minor >= 0 && major >= 0, "grid item must have span >= 1");
            if major > maxMajor || (major == maxMajor && minor > maxMinor) {
                maxMajor = major;
                maxMinor = minor;
                result.mItem = Some(aIter.ItemIndex());
                result.mIsInEdgeTrack = major == lastMajorTrack;
            }
            aIter.Next();
        }
        result
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GetUsedTrackSizes
    pub fn GetUsedTrackSizes(&self) -> Option<&'static mut UsedTrackSizes> {
        UsedTrackSizes::GetProp(self.0)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::StoreUsedTrackSizes
    pub fn StoreUsedTrackSizes(&self, aAxis: LogicalAxis, aSizes: &TrackPlan) {
        let uts = UsedTrackSizes::GetOrCreateProp(self.0);
        uts.mTrackPlans[aAxis].Assign(aSizes);
        uts.mCanResolveLineRangeSize[aAxis] = true;
        // XXX is resetting these bits necessary?
        for sz in uts.mTrackPlans[aAxis].iter_mut() {
            sz.mState &= !(TrackSize_StateBits::eFrozen | TrackSize_StateBits::eSkipGrowUnlimited | TrackSize_StateBits::eInfinitelyGrowable);
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::SetInitialChildList
    #[cfg(debug_assertions)]
    pub fn SetInitialChildList(&self, aListID: FrameChildListID, aChildList: nsFrameList) {
        // ChildListIDs supportedLists = {FrameChildListID::Principal};
        MOZ_ASSERT!(aListID == FrameChildListID::Principal, "unexpected child list");
        self.nsContainerFrame_SetInitialChildList(aListID, aChildList)
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::TrackSize::DumpStateBits, TrackSize::Dump
    // NOT PORTED: debug dump (printf of the track state).

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::IsMasonry
    pub fn IsMasonry__LogicalAxis(&self, aAxis: LogicalAxis) -> bool {
        self.HasAnyStateBits(if aAxis == LogicalAxis::Block { NS_STATE_GRID_IS_ROW_MASONRY } else { NS_STATE_GRID_IS_COL_MASONRY })
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GridItemShouldStretch
    pub fn GridItemShouldStretch(&self, aChild: nsIFrame, aAxis: LogicalAxis) -> bool {
        MOZ_ASSERT!(aChild.IsGridItem());

        if aChild.IsGridContainerFrame() {
            // The subgrid is always stretched in its subgridded dimensions.
            // https://drafts.csswg.org/css-grid-2/#subgrid-box-alignment
            let gridContainer = nsGridContainerFrame::from_frame(aChild);
            if gridContainer.IsSubgrid__LogicalAxis(aAxis) {
                return true;
            }
        }

        let wm = aChild.GetWritingMode();
        if aChild.StyleMargin().HasAuto(aAxis, wm, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aChild, std::ptr::null_mut())) {
            // Per https://drafts.csswg.org/css-grid-2/#auto-margins, any 'auto' margin
            // in an axis disables the alignment property in that axis.
            return false;
        }

        let cbwm = self.GetWritingMode();
        // nsContainerFrame::IsMasonry(WritingMode, LogicalAxis)
        if self.IsMasonry__LogicalAxis(wm.ConvertAxisTo(aAxis, cbwm)) {
            // The child is in the container's masonry-axis.
            // AlignJustifyTracksInMasonryAxis will stretch it, so we don't report that
            // here.
            return false;
        }

        let alignment: StyleAlignFlags = aChild.StylePosition().UsedSelfAlignment__WritingMode_LogicalAxis_WritingMode_ComputedStyle(wm, aAxis, cbwm, Some(self.Style()));
        // An item with 'normal' alignment that is a replaced frame should use its
        // natural size, and not fill the grid area.
        // https://drafts.csswg.org/css-grid-2/#grid-item-sizing
        if alignment == StyleAlignFlags::NORMAL {
            return !aChild.HasReplacedSizing();
        }
        alignment == StyleAlignFlags::STRETCH
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ShouldInhibitSubgridDueToIFC
    pub fn ShouldInhibitSubgridDueToIFC(aFrame: nsIFrame) -> bool {
        // Just checking for things that make us establish an independent formatting
        // context (IFC) and hence prevent us from being a subgrid:
        // * Out-of-flow (e.g. abspos) frames also establish an IFC.  Note, our
        // NS_FRAME_OUT_OF_FLOW bit potentially isn't set yet, so we check our style.
        // * contain:layout and contain:paint each make us establish an IFC.
        let display = aFrame.StyleDisplay();
        display.IsContainLayout() || display.IsContainPaint() || display.mContainerType_SizeOrInlineSize() || display.IsAbsolutelyPositionedStyle()
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GetGridContainerFrame
    // ADAPTER: aFrame may be a frame or an Option (C++ takes a pointer that can be null).
    pub fn GetGridContainerFrame(aFrame: impl Into<Option<nsIFrame>>) -> Option<nsGridContainerFrame> {
        let mut gridFrame: Option<nsGridContainerFrame> = None;

        if let Some(aFrame) = aFrame.into() {
            let inner = aFrame;
            if aFrame.IsFieldSetFrame() {
                // inner = static_cast<nsFieldSetFrame*>(aFrame)->GetInner();
                // NOT PORTED: fieldset frames (nsFieldSetFrame is not translated; IsFieldSetFrame() is false).
                unimplemented!("NOT PORTED: nsFieldSetFrame::GetInner");
            }
            // Since "Get" methods like GetInner and GetContentInsertionFrame can
            // return null, we check the return values before dereferencing. Our
            // calling pattern makes this unlikely, but we're being careful.
            let insertionFrame: Option<nsIFrame> = inner.GetContentInsertionFrame();
            let possibleGridFrame = insertionFrame.unwrap_or(aFrame);
            gridFrame = if possibleGridFrame.IsGridContainerFrame() { Some(nsGridContainerFrame::from_frame(possibleGridFrame)) } else { None };
        }
        gridFrame
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::GetGridFrameWithComputedInfo
    // NOT PORTED: devtools grid info (Element.getGridFragments): it forces a reflow with
    // NS_STATE_GRID_COMPUTED_INFO and reads the ComputedGridTrackInfo properties, which are not ported.
    pub fn GetGridFrameWithComputedInfo(aFrame: nsIFrame) -> Option<nsGridContainerFrame> {
        unimplemented!("NOT PORTED: devtools grid info (GetGridFrameWithComputedInfo)")
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::MarkCachedGridMeasurementsDirty
    pub fn MarkCachedGridMeasurementsDirty(aItemFrame: nsIFrame) {
        MOZ_ASSERT!(aItemFrame.IsGridItem());
        nsGridContainerFrame_CachedBAxisMeasurement::RemoveProp(aItemFrame);
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::IsLineIteratorFlowRTL, GetNumLines,
    // GetLine, FindLineContaining, CheckLineOrder, FindFrameAt
    // NOT PORTED: the nsILineIterator implementation (nsGridContainerFrame.cpp lines 10750-10813 at GECKO_REV): it
    // serves caret movement and selection, which this runtime does not have.
}

// Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::TrackPlan::Initialize
impl nsGridContainerFrame_TrackPlan {
    pub fn Initialize(&mut self, aPhase: TrackSizingPhase, aTracks: &Tracks) {
        MOZ_ASSERT!(self.Length() == aTracks.mSizes.Length());
        for (plan, sz) in self.iter_mut().zip(aTracks.mSizes.iter()) {
            plan.mBase = Tracks::StartSizeInDistribution(aPhase, sz);
            MOZ_ASSERT!(aPhase == TrackSizingPhase::MaxContentMaximums || !(sz.mState & TrackSize_StateBits::eInfinitelyGrowable).to_bool(), "forgot to reset the eInfinitelyGrowable bit?");
            plan.mState = sz.mState;
        }
    }

    // Distribute space to all flex tracks this item spans.
    // https://drafts.csswg.org/css-grid-2/#algo-spanning-flex-items
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::TrackPlan::DistributeToFlexTrackSizes
    pub fn DistributeToFlexTrackSizes(&mut self, aAvailableSpace: nscoord, aGrowableTracks: &Vec<u32>, aFunctions: &TrackSizingFunctions, aTracks: &Tracks) -> nscoord {
        let mut space: nscoord = aAvailableSpace;
        // Measure used fraction.
        let mut totalFr: f64 = 0.0;
        // TODO alaskanemily: we should be subtracting definite-sized tracks from
        // the available space below.
        for &track in aGrowableTracks.iter() {
            MOZ_ASSERT!((aTracks.mSizes[track as usize].mState & TrackSize_StateBits::eFlexMaxSizing).to_bool(), "Only flex-sized tracks should be growable during step 4");
            // aFunctions.MaxSizingFor(track).AsFlex()._0
            totalFr += FlexFractionOf(aFunctions.MaxSizingFor(track)) as f64;
        }
        MOZ_ASSERT!(totalFr >= 0.0, "flex fractions must be non-negative.");

        let mut frSize: f64 = aAvailableSpace as f64;
        if totalFr > 1.0 {
            frSize /= totalFr;
        }
        // Distribute the space to the tracks proportionally to the fractional
        // sizes.
        for &track in aGrowableTracks.iter() {
            let sz = &mut self[track as usize];
            if sz.IsFrozen() {
                continue;
            }
            let trackFr: f64 = FlexFractionOf(aFunctions.MaxSizingFor(track)) as f64;
            let mut size: nscoord = NSToCoordRoundWithClamp__double(frSize * trackFr);
            // This shouldn't happen in theory, but it could happen due to a
            // combination of floating-point error during the multiplication above
            // and loss of precision in the cast.
            if size > space {
                size = space;
                space = 0;
            } else {
                space -= size;
            }
            sz.mBase = std::cmp::max(sz.mBase, size);
        }
        space
    }
}

// ADAPTER: `StyleTrackBreadth::AsFlex()._0` of the C++ code (cbindgen's tagged-union accessor): the value of an
// `fr` track breadth.
fn FlexFractionOf(aBreadth: &mrt_style::style::values::generics::grid::GenericTrackBreadth<LengthPercentage>) -> f32 {
    match aBreadth {
        mrt_style::style::values::generics::grid::GenericTrackBreadth::Flex(fr) => fr.0,
        _ => panic!("AsFlex() on a track breadth that is not an fr"),
    }
}

impl nsGridContainerFrame_ItemPlan {
    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ItemPlan::Initialize
    pub fn Initialize(&mut self, aPhase: TrackSizingPhase, aGrowableTracks: &Vec<u32>, aTracks: &Tracks) {
        for &track in aGrowableTracks.iter() {
            let plan = &mut self[track as usize];
            let sz = &aTracks.mSizes[track as usize];
            plan.mBase = Tracks::StartSizeInDistribution(aPhase, sz);
            let unlimited = (sz.mState & TrackSize_StateBits::eInfinitelyGrowable).to_bool();
            plan.mLimit = if unlimited { NS_UNCONSTRAINEDSIZE } else { sz.mLimit };
            plan.mState = sz.mState;
        }
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ItemPlan::GrowTracksToLimit
    // ADAPTER: aFitContentClamper is `Option<&ItemPlan_FitContentClamper>` (C++ tests the std::function for emptiness).
    pub fn GrowTracksToLimit(&mut self, aAvailableSpace: nscoord, aGrowableTracks: &Vec<u32>, aFitContentClamper: Option<&ItemPlan_FitContentClamper>) -> nscoord {
        MOZ_ASSERT!(aAvailableSpace > 0 && aGrowableTracks.len() > 0);
        let mut space: nscoord = aAvailableSpace;
        let mut numGrowable: u32 = aGrowableTracks.len() as u32;
        loop {
            let spacePerTrack: nscoord = std::cmp::max::<nscoord>(space / numGrowable as nscoord, 1);
            for &track in aGrowableTracks.iter() {
                let sz = &mut self[track as usize];
                if sz.IsFrozen() {
                    continue;
                }
                let newBase: nscoord = sz.mBase + spacePerTrack;
                let mut limit: nscoord = sz.mLimit;
                if (sz.mState & TrackSize_StateBits::eApplyFitContentClamping).to_bool() {
                    if let Some(aFitContentClamper) = aFitContentClamper {
                        // Clamp the limit to the fit-content() size, for §12.5.2 step 5/6.
                        aFitContentClamper(track, sz.mBase, &mut limit);
                    }
                }
                if newBase > limit {
                    let consumed: nscoord = limit - sz.mBase;
                    if consumed > 0 {
                        space -= consumed;
                        sz.mBase = limit;
                    }
                    sz.mState |= TrackSize_StateBits::eFrozen;
                    numGrowable -= 1;
                    if numGrowable == 0 {
                        return space;
                    }
                } else {
                    sz.mBase = newBase;
                    space -= spacePerTrack;
                }
                MOZ_ASSERT!(space >= 0);
                if space == 0 {
                    return 0;
                }
            }
        }
        // MOZ_ASSERT_UNREACHABLE("we don't exit the loop above except by return");
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ItemPlan::MarkExcludedTracks
    // (the overload with TrackSizingPhase)
    pub fn MarkExcludedTracks__TrackSizingPhase_nsTArray_SizingConstraint(&mut self, aPhase: TrackSizingPhase, aGrowableTracks: &Vec<u32>, aConstraint: SizingConstraint) -> u32 {
        let mut numGrowable: u32 = aGrowableTracks.len() as u32;
        if aPhase == TrackSizingPhase::IntrinsicMaximums || aPhase == TrackSizingPhase::MaxContentMaximums {
            // "when handling any intrinsic growth limit: all affected tracks"
            return numGrowable;
        }

        let mut selector: TrackSize_StateBits = Tracks::SelectorForPhase(aPhase, aConstraint);
        numGrowable = self.MarkExcludedTracks__uint32_t_nsTArray_StateBits_StateBits_StateBits(numGrowable, aGrowableTracks, TrackSize_StateBits::eMaxContentMinSizing, TrackSize_StateBits::eMaxContentMaxSizing, TrackSize_StateBits::eSkipGrowUnlimited1);
        // Note that eMaxContentMinSizing is always included. We do those first:
        selector &= !TrackSize_StateBits::eMaxContentMinSizing;
        if selector.to_bool() {
            numGrowable = self.MarkExcludedTracks__uint32_t_nsTArray_StateBits_StateBits_StateBits(numGrowable, aGrowableTracks, selector, TrackSize_StateBits::eIntrinsicMaxSizing, TrackSize_StateBits::eSkipGrowUnlimited2);
        }
        numGrowable
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ItemPlan::MarkExcludedTracks
    // (the overload with the selectors)
    pub fn MarkExcludedTracks__uint32_t_nsTArray_StateBits_StateBits_StateBits(&mut self, aNumGrowable: u32, aGrowableTracks: &Vec<u32>, aMinSizingSelector: TrackSize_StateBits, aMaxSizingSelector: TrackSize_StateBits, aSkipFlag: TrackSize_StateBits) -> u32 {
        let mut foundOneSelected = false;
        let mut foundOneGrowable = false;
        let mut numGrowable: u32 = aNumGrowable;
        for &track in aGrowableTracks.iter() {
            let sz = &mut self[track as usize];
            let state = sz.mState;
            if (state & aMinSizingSelector).to_bool() {
                foundOneSelected = true;
                if (state & aMaxSizingSelector).to_bool() {
                    foundOneGrowable = true;
                    continue;
                }
                sz.mState |= aSkipFlag;
                MOZ_ASSERT!(numGrowable != 0);
                numGrowable -= 1;
            }
        }
        // 12.5 "if there are no such tracks, then all affected tracks"
        if foundOneSelected && !foundOneGrowable {
            for &track in aGrowableTracks.iter() {
                self[track as usize].mState &= !aSkipFlag;
            }
            numGrowable = aNumGrowable;
        }
        numGrowable
    }

    // Gecko: layout/generic/nsGridContainerFrame.cpp nsGridContainerFrame::ItemPlan::GrowSelectedTracksUnlimited
    pub fn GrowSelectedTracksUnlimited(&mut self, aAvailableSpace: nscoord, aGrowableTracks: &Vec<u32>, mut aNumGrowable: u32, aFitContentClamper: Option<&ItemPlan_FitContentClamper>) {
        MOZ_ASSERT!(aAvailableSpace > 0 && aGrowableTracks.len() > 0 && aNumGrowable as usize <= aGrowableTracks.len());
        let mut space: nscoord = aAvailableSpace;
        let mut didClamp = false;
        while aNumGrowable != 0 {
            let spacePerTrack: nscoord = std::cmp::max::<nscoord>(space / aNumGrowable as nscoord, 1);
            for &track in aGrowableTracks.iter() {
                let sz = &mut self[track as usize];
                if (sz.mState & TrackSize_StateBits::eSkipGrowUnlimited).to_bool() {
                    continue; // an excluded track
                }
                let mut delta: nscoord = spacePerTrack;
                let mut newBase: nscoord = sz.mBase + delta;
                if (sz.mState & TrackSize_StateBits::eApplyFitContentClamping).to_bool() {
                    if let Some(aFitContentClamper) = aFitContentClamper {
                        // Clamp newBase to the fit-content() size, for §12.5.2 step 5/6.
                        if aFitContentClamper(track, sz.mBase, &mut newBase) {
                            didClamp = true;
                            delta = newBase - sz.mBase;
                            MOZ_ASSERT!(delta >= 0, "track size shouldn't shrink");
                            sz.mState |= TrackSize_StateBits::eSkipGrowUnlimited1;
                            aNumGrowable -= 1;
                        }
                    }
                }
                sz.mBase = newBase;
                space -= delta;
                MOZ_ASSERT!(space >= 0);
                if space == 0 {
                    return;
                }
            }
        }
        MOZ_ASSERT!(didClamp, "we don't exit the loop above except by return, unless we clamped some track's size");
    }
}

// Tests of the grid container. Gecko has no gtest for nsGridContainerFrame (its tests are WPT and
// reftests): these are written from the behaviour of CSS Grid Layout Module Level 1 and run on real
// Stylo styles with HTML snippets, like the flex tests.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::*;
    use mrt_style::mrt_dom::Dom;
    use mrt_style::StyleDom;

    const BODY: &str = "<!DOCTYPE html><html><head><style>body{margin:0}</style></head><body>";

    // Parses, styles and lays `html` out at 1000 x 800 CSS px. The frame tree is this test thread's.
    fn layout(aBody: &str) -> PageLayout {
        let html = format!("{BODY}{aBody}</body></html>");
        let mut sd = StyleDom::new(Dom::parse(&html), (1000.0, 800.0));
        sd.resolve();
        // the frames read their content from the DOM (see nsIContent)
        let sd = KeepDocumentAlive(sd);
        LayoutDocument(&sd, 1000, 800)
    }

    fn find_all(aFrame: nsIFrame, aType: LayoutFrameType) -> Vec<nsIFrame> {
        fn walk(f: nsIFrame, t: LayoutFrameType, out: &mut Vec<nsIFrame>) {
            if f.Type() == t {
                out.push(f);
            }
            let mut lists = Vec::new();
            f.GetChildLists(&mut lists);
            for l in lists {
                let mut c = l.mList.FirstChild();
                while let Some(k) = c {
                    walk(k, t, out);
                    c = k.GetNextSibling();
                }
            }
        }
        let mut result = Vec::new();
        walk(aFrame, aType, &mut result);
        result
    }

    fn children(aFrame: nsIFrame) -> Vec<nsIFrame> {
        let mut result = Vec::new();
        let mut c = aFrame.GetChildList(FrameChildListID::Principal).FirstChild();
        while let Some(k) = c {
            result.push(k);
            c = k.GetNextSibling();
        }
        result
    }

    // The rect of a frame in CSS px relative to its parent: (x, y, width, height)
    fn rect(aFrame: nsIFrame) -> (f64, f64, f64, f64) {
        let r = aFrame.GetRect();
        let px = |v: nscoord| v as f64 / AppUnitsPerCSSPixel() as f64;
        (px(r.x), px(r.y), px(r.width), px(r.height))
    }

    // Lays out `aBody`, takes the first grid container and returns it with its items, in tree order.
    fn container(aBody: &str) -> (PageLayout, nsIFrame, Vec<nsIFrame>) {
        let result = layout(aBody);
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer)[0];
        let items = children(c);
        (result, c, items)
    }

    fn rects(aItems: &[nsIFrame]) -> Vec<(f64, f64, f64, f64)> {
        aItems.iter().map(|i| rect(*i)).collect()
    }

    // The item rects of the first grid container in `aBody`
    fn g(aBody: &str) -> Vec<(f64, f64, f64, f64)> {
        let (_, _, items) = container(aBody);
        rects(&items)
    }

    // `n` empty boxes of the given style
    fn boxes(n: usize, aStyle: &str) -> String {
        format!("<div style='{aStyle}'></div>").repeat(n)
    }

    #[test]
    fn fixed_tracks_place_items_in_order_row_by_row() {
        // 7.2 and 8.5: two 100px columns, auto rows (height of the items), items fill row by row.
        let r = g(&format!("<div style='display:grid;grid-template-columns:100px 100px'>{}</div>", boxes(3, "height:20px")));
        assert_eq!(r, vec![(0.0, 0.0, 100.0, 20.0), (100.0, 0.0, 100.0, 20.0), (0.0, 20.0, 100.0, 20.0)]);
    }

    #[test]
    fn the_container_is_block_level_and_as_high_as_its_rows() {
        let (result, c, _) = container(&format!("<div style='display:grid;grid-template-columns:100px 100px'>{}</div>", boxes(3, "height:20px")));
        assert_eq!(rect(c).2, 1000.0);
        assert_eq!(rect(c).3, 40.0);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(40.0));
    }

    #[test]
    fn fr_tracks_share_the_free_space_in_proportion() {
        // 7.2.3: 1fr 2fr in 300px is 100 and 200
        let r = g(&format!("<div style='display:grid;width:300px;grid-template-columns:1fr 2fr'>{}</div>", boxes(2, "height:10px")));
        assert_eq!(r, vec![(0.0, 0.0, 100.0, 10.0), (100.0, 0.0, 200.0, 10.0)]);
    }

    #[test]
    fn fixed_and_fr_tracks_mix() {
        let r = g(&format!("<div style='display:grid;width:300px;grid-template-columns:50px 1fr 1fr'>{}</div>", boxes(3, "height:10px")));
        assert_eq!(r.iter().map(|x| (x.0, x.2)).collect::<Vec<_>>(), vec![(0.0, 50.0), (50.0, 125.0), (175.0, 125.0)]);
    }

    #[test]
    fn fr_sum_below_one_leaves_free_space() {
        // 7.2.3.1: if the sum of the flex factors is less than 1 they take that fraction of the free space
        let r = g(&format!("<div style='display:grid;width:400px;grid-template-columns:0.25fr 0.25fr'>{}</div>", boxes(2, "height:10px")));
        assert_eq!(r.iter().map(|x| (x.0, x.2)).collect::<Vec<_>>(), vec![(0.0, 100.0), (100.0, 100.0)]);
    }

    #[test]
    fn fr_track_is_floored_at_the_min_content_of_its_item() {
        // 1fr is minmax(auto, 1fr): the track cannot be smaller than the min-content of a 120px wide item
        let r = g("<div style='display:grid;width:200px;grid-template-columns:1fr 1fr'><div style='width:150px;height:10px'></div><div style='height:10px'></div></div>");
        assert_eq!(r[0].2, 150.0);
        assert_eq!(r[1].0, 150.0);
        assert_eq!(r[1].2, 50.0);
    }

    #[test]
    fn minmax_zero_fr_does_not_floor_at_content() {
        let r = g("<div style='display:grid;width:200px;grid-template-columns:minmax(0,1fr) minmax(0,1fr)'><div style='width:150px;height:10px'></div><div style='height:10px'></div></div>");
        // each track is 100px; the first item overflows its track
        assert_eq!(r[1].0, 100.0);
        assert_eq!(r[1].2, 100.0);
    }

    #[test]
    fn auto_columns_stretch_equally_to_fill_the_container() {
        // 11.8 stretch auto tracks: 300px and two auto tracks of empty items: 150 each
        let r = g(&format!("<div style='display:grid;width:300px;grid-template-columns:auto auto'>{}</div>", boxes(2, "height:10px")));
        assert_eq!(r.iter().map(|x| (x.0, x.2)).collect::<Vec<_>>(), vec![(0.0, 150.0), (150.0, 150.0)]);
    }

    #[test]
    fn auto_tracks_are_sized_by_the_widest_content() {
        // with justify-content:start the auto tracks do not stretch: they are max-content wide
        let r = g("<div style='display:grid;justify-content:start;grid-template-columns:auto auto'><div style='width:70px;height:10px'></div><div style='width:30px;height:10px'></div><div style='width:20px;height:5px'></div></div>");
        assert_eq!(r[0].2, 70.0);
        assert_eq!(r[1].0, 70.0);
        assert_eq!(r[1].2, 30.0);
        // the third item has its own width and is not stretched
        assert_eq!(r[2].2, 20.0);
    }

    #[test]
    fn auto_rows_are_as_high_as_the_tallest_item() {
        let r = g("<div style='display:grid;grid-template-columns:50px 50px'><div style='height:10px'></div><div style='height:30px'></div><div style='height:5px'></div></div>");
        // first row is 30 high; items stretch in their area (auto height): the first has a height so it keeps it
        assert_eq!(r[0].3, 10.0);
        assert_eq!(r[1].3, 30.0);
        assert_eq!(r[2].1, 30.0);
    }

    #[test]
    fn minmax_with_content_bounds_grows_to_max_content() {
        let r = g("<div style='display:grid;justify-content:start;grid-template-columns:minmax(40px, max-content)'><div style='width:90px;height:10px'></div></div>");
        assert_eq!(r[0].2, 90.0);
    }

    #[test]
    fn minmax_fixed_bounds_clamp_the_track() {
        let r = g("<div style='display:grid;justify-content:start;grid-template-columns:minmax(40px, 60px)'><div style='width:90px;height:10px'></div></div>");
        // the track is 60px (growth limit); the item keeps its own width and overflows
        assert_eq!(r[0].2, 90.0);
        let r = g("<div style='display:grid;justify-content:start;grid-template-columns:minmax(40px, 60px) 10px'><div style='height:10px'></div><div></div></div>");
        // stretch normal: an auto width item fills the track: 60px (auto tracks max is the limit, here max 60)
        assert!(r[0].2 >= 40.0 && r[0].2 <= 60.0);
    }

    #[test]
    fn min_content_and_max_content_tracks() {
        let r = g("<div style='display:grid;justify-content:start;grid-template-columns:min-content max-content'><div style='width:50px;height:10px'></div><div style='width:80px;height:10px'></div></div>");
        assert_eq!(r[0].2, 50.0);
        assert_eq!(r[1].0, 50.0);
        assert_eq!(r[1].2, 80.0);
    }

    #[test]
    fn fit_content_clamps_the_max_content() {
        let r = g("<div style='display:grid;justify-content:start;grid-template-columns:fit-content(60px) fit-content(200px);font:10px monospace'><div style='height:10px'>aaaa aaaa aaaa</div><div style='height:10px'>x</div></div>");
        // 11.? fit-content(60px): the max-content of the text is wider than 60, the min-content narrower
        assert_eq!(r[1].0, 60.0);
    }

    #[test]
    fn repeat_with_a_count_expands_the_tracks() {
        let r = g(&format!("<div style='display:grid;grid-template-columns:repeat(3, 40px)'>{}</div>", boxes(4, "height:10px")));
        assert_eq!(r.iter().map(|x| (x.0, x.1, x.2)).collect::<Vec<_>>(), vec![(0.0, 0.0, 40.0), (40.0, 0.0, 40.0), (80.0, 0.0, 40.0), (0.0, 10.0, 40.0)]);
    }

    #[test]
    fn repeat_with_several_track_sizes() {
        let r = g(&format!("<div style='display:grid;grid-template-columns:repeat(2, 10px 20px)'>{}</div>", boxes(4, "height:10px")));
        assert_eq!(r.iter().map(|x| (x.0, x.2)).collect::<Vec<_>>(), vec![(0.0, 10.0), (10.0, 20.0), (30.0, 10.0), (40.0, 20.0)]);
    }

    #[test]
    fn repeat_auto_fill_fits_as_many_tracks_as_the_container_allows() {
        // 7.2.3.2: 350px holds three 100px tracks; the fourth item goes to the next row
        let r = g(&format!("<div style='display:grid;width:350px;grid-template-columns:repeat(auto-fill, 100px)'>{}</div>", boxes(4, "height:10px")));
        assert_eq!(r.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(0.0, 0.0), (100.0, 0.0), (200.0, 0.0), (0.0, 10.0)]);
    }

    #[test]
    fn repeat_auto_fill_with_minmax_fr_fills_the_width() {
        // 450px / (100 min): 4 tracks of 112.5
        let r = g(&format!("<div style='display:grid;width:450px;grid-template-columns:repeat(auto-fill, minmax(100px, 1fr))'>{}</div>", boxes(2, "height:10px")));
        assert_eq!(r[0].2, 112.5);
        assert_eq!(r[1].0, 112.5);
    }

    #[test]
    fn repeat_auto_fit_collapses_empty_tracks() {
        // 7.2.3.2: with auto-fit the empty tracks collapse to zero and 1fr tracks take the whole width
        let r = g(&format!("<div style='display:grid;width:400px;grid-template-columns:repeat(auto-fit, minmax(100px, 1fr))'>{}</div>", boxes(2, "height:10px")));
        assert_eq!(r[0].2, 200.0);
        assert_eq!(r[1].0, 200.0);
        assert_eq!(r[1].2, 200.0);
    }

    #[test]
    fn repeat_auto_fill_keeps_the_empty_tracks() {
        let r = g(&format!("<div style='display:grid;width:400px;grid-template-columns:repeat(auto-fill, minmax(100px, 1fr))'>{}</div>", boxes(2, "height:10px")));
        assert_eq!(r[0].2, 100.0);
        assert_eq!(r[1].0, 100.0);
    }

    #[test]
    fn column_and_row_gaps_separate_the_tracks() {
        let r = g(&format!("<div style='display:grid;grid-template-columns:50px 50px;column-gap:10px;row-gap:5px'>{}</div>", boxes(4, "height:20px")));
        assert_eq!(r.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(0.0, 0.0), (60.0, 0.0), (0.0, 25.0), (60.0, 25.0)]);
    }

    #[test]
    fn the_gap_shorthand_sets_both_gaps() {
        let r = g(&format!("<div style='display:grid;grid-template-columns:50px 50px;gap:7px 3px'>{}</div>", boxes(4, "height:20px")));
        assert_eq!(r.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(0.0, 0.0), (53.0, 0.0), (0.0, 27.0), (53.0, 27.0)]);
    }

    #[test]
    fn gaps_are_subtracted_before_fr_tracks_are_sized() {
        // 300 - 2 * 10 = 280 shared by three 1fr
        let r = g(&format!("<div style='display:grid;width:300px;grid-template-columns:1fr 1fr 1fr;column-gap:10px'>{}</div>", boxes(3, "height:10px")));
        assert!((r[0].2 - 93.333).abs() < 0.05);
        assert!((r[2].0 - 206.667).abs() < 0.05);
    }

    #[test]
    fn a_percentage_gap_resolves_against_the_content_box() {
        let r = g(&format!("<div style='display:grid;width:400px;grid-template-columns:50px 50px;column-gap:10%'>{}</div>", boxes(2, "height:10px")));
        assert_eq!(r[1].0, 90.0);
    }

    #[test]
    fn the_container_height_includes_row_gaps() {
        let (_, c, _) = container(&format!("<div style='display:grid;grid-template-columns:50px;row-gap:8px'>{}</div>", boxes(3, "height:10px")));
        assert_eq!(rect(c).3, 46.0);
    }

    #[test]
    fn items_are_placed_by_line_numbers() {
        // 8.3: grid-column: 2 / 4 and grid-row: 2 / 3
        let r = g("<div style='display:grid;grid-template-columns:repeat(4, 25px);grid-template-rows:repeat(3, 10px)'><div style='grid-column:2 / 4;grid-row:2 / 3'></div></div>");
        assert_eq!(r, vec![(25.0, 10.0, 50.0, 10.0)]);
    }

    #[test]
    fn start_and_end_properties_work_separately() {
        let r = g("<div style='display:grid;grid-template-columns:repeat(4, 25px);grid-template-rows:repeat(3, 10px)'><div style='grid-column-start:3;grid-column-end:5;grid-row-start:3'></div></div>");
        assert_eq!(r, vec![(50.0, 20.0, 50.0, 10.0)]);
    }

    #[test]
    fn negative_line_numbers_count_from_the_end() {
        // -1 is the last explicit line: column 4 is 3 / -1... lines 1..5, -2 is line 4
        let r = g("<div style='display:grid;grid-template-columns:repeat(4, 25px);grid-template-rows:10px'><div style='grid-column:-3 / -1'></div></div>");
        assert_eq!(r, vec![(50.0, 0.0, 50.0, 10.0)]);
    }

    #[test]
    fn span_gives_the_number_of_tracks() {
        let r = g("<div style='display:grid;grid-template-columns:repeat(4, 25px);grid-template-rows:repeat(3, 10px)'><div style='grid-column:2 / span 2;grid-row:span 2'></div></div>");
        assert_eq!(r, vec![(25.0, 0.0, 50.0, 20.0)]);
    }

    #[test]
    fn named_lines_place_items() {
        let r = g("<div style='display:grid;grid-template-columns:[a] 20px [b] 30px [c] 40px [d]'><div style='grid-column:b / d;height:10px'></div></div>");
        assert_eq!(r, vec![(20.0, 0.0, 70.0, 10.0)]);
    }

    #[test]
    fn named_lines_with_a_count_and_span() {
        let r = g("<div style='display:grid;grid-template-columns:[x] 10px [x] 10px [x] 10px'><div style='grid-column:x 2 / span 1;height:10px'></div></div>");
        assert_eq!(r, vec![(10.0, 0.0, 10.0, 10.0)]);
    }

    #[test]
    fn named_areas_place_items() {
        // 7.3: grid-template-areas and grid-area
        let r = g("<div style='display:grid;grid-template-columns:50px 50px;grid-template-rows:10px 10px;grid-template-areas:\"h h\" \"s m\"'><div style='grid-area:m'></div><div style='grid-area:h'></div><div style='grid-area:s'></div></div>");
        assert_eq!(r, vec![(50.0, 10.0, 50.0, 10.0), (0.0, 0.0, 100.0, 10.0), (0.0, 10.0, 50.0, 10.0)]);
    }

    #[test]
    fn grid_area_with_four_lines() {
        let r = g("<div style='display:grid;grid-template-columns:repeat(3, 10px);grid-template-rows:repeat(3, 10px)'><div style='grid-area:2 / 2 / 4 / 4'></div></div>");
        assert_eq!(r, vec![(10.0, 10.0, 20.0, 20.0)]);
    }

    #[test]
    fn implicit_named_lines_from_areas_are_usable() {
        // an area `a` makes the lines `a-start` and `a-end`
        let r = g("<div style='display:grid;grid-template-columns:10px 20px;grid-template-rows:10px;grid-template-areas:\"a b\"'><div style='grid-column:b-start / b-end'></div></div>");
        assert_eq!(r, vec![(10.0, 0.0, 20.0, 10.0)]);
    }

    #[test]
    fn auto_placement_skips_occupied_cells() {
        // 8.5: the explicit item takes cell 1; the auto items go around it
        let r = g("<div style='display:grid;grid-template-columns:repeat(2, 20px);grid-template-rows:repeat(2, 10px)'><div style='grid-column:1;grid-row:1'></div><div></div><div></div></div>");
        assert_eq!(r.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(0.0, 0.0), (20.0, 0.0), (0.0, 10.0)]);
    }

    #[test]
    fn auto_flow_column_fills_columns_first() {
        let r = g("<div style='display:grid;grid-auto-flow:column;grid-template-rows:repeat(2, 10px);grid-auto-columns:30px'><div></div><div></div><div></div></div>");
        assert_eq!(r.iter().map(|x| (x.0, x.1, x.2)).collect::<Vec<_>>(), vec![(0.0, 0.0, 30.0), (0.0, 10.0, 30.0), (30.0, 0.0, 30.0)]);
    }

    #[test]
    fn sparse_packing_never_goes_back() {
        // 8.5: with `row` (sparse) the cursor only moves forward: the small item after the wide one
        // does not fill the hole before it.
        let r = g("<div style='display:grid;grid-template-columns:repeat(3, 10px);grid-auto-rows:10px'><div></div><div style='grid-column:span 3'></div><div></div></div>");
        assert_eq!(r.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(0.0, 0.0), (0.0, 10.0), (0.0, 20.0)]);
    }

    #[test]
    fn dense_packing_fills_the_holes() {
        let r = g("<div style='display:grid;grid-auto-flow:row dense;grid-template-columns:repeat(3, 10px);grid-auto-rows:10px'><div></div><div style='grid-column:span 3'></div><div></div></div>");
        assert_eq!(r.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(0.0, 0.0), (0.0, 10.0), (10.0, 0.0)]);
    }

    #[test]
    fn dense_column_flow() {
        let r = g("<div style='display:grid;grid-auto-flow:column dense;grid-template-rows:repeat(3, 10px);grid-auto-columns:10px'><div></div><div style='grid-row:span 3'></div><div></div></div>");
        assert_eq!(r.iter().map(|x| (x.0, x.1)).collect::<Vec<_>>(), vec![(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)]);
    }

    #[test]
    fn implicit_rows_use_grid_auto_rows() {
        let r = g(&format!("<div style='display:grid;grid-template-columns:50px;grid-auto-rows:25px'>{}</div>", boxes(3, "")));
        assert_eq!(r.iter().map(|x| (x.1, x.3)).collect::<Vec<_>>(), vec![(0.0, 25.0), (25.0, 25.0), (50.0, 25.0)]);
    }

    #[test]
    fn grid_auto_rows_cycle_through_the_list() {
        let r = g(&format!("<div style='display:grid;grid-template-columns:50px;grid-auto-rows:10px 20px'>{}</div>", boxes(3, "")));
        assert_eq!(r.iter().map(|x| (x.1, x.3)).collect::<Vec<_>>(), vec![(0.0, 10.0), (10.0, 20.0), (30.0, 10.0)]);
    }

    #[test]
    fn implicit_columns_are_created_by_items_placed_beyond_the_explicit_grid() {
        let r = g("<div style='display:grid;grid-template-columns:20px;grid-auto-columns:30px;grid-template-rows:10px'><div style='grid-column:3'></div></div>");
        assert_eq!(r, vec![(50.0, 0.0, 30.0, 10.0)]);
    }

    #[test]
    fn implicit_tracks_before_the_explicit_grid() {
        // a negative placement beyond the explicit grid creates implicit tracks at the start
        let r = g("<div style='display:grid;grid-template-columns:20px;grid-auto-columns:30px;grid-template-rows:10px'><div style='grid-column:-3 / -2'></div><div style='grid-column:1'></div></div>");
        assert_eq!(r[0].0, 0.0);
        assert_eq!(r[1].0, 30.0);
    }

    #[test]
    fn justify_content_center_moves_the_whole_grid() {
        // 10.5 / 10.6: tracks fixed 100px in 300px: centered at 100
        let r = g(&format!("<div style='display:grid;width:300px;justify-content:center;grid-template-columns:50px 50px'>{}</div>", boxes(2, "height:10px")));
        assert_eq!(r.iter().map(|x| x.0).collect::<Vec<_>>(), vec![100.0, 150.0]);
    }

    #[test]
    fn justify_content_end() {
        let r = g(&format!("<div style='display:grid;width:300px;justify-content:end;grid-template-columns:50px 50px'>{}</div>", boxes(2, "height:10px")));
        assert_eq!(r.iter().map(|x| x.0).collect::<Vec<_>>(), vec![200.0, 250.0]);
    }

    #[test]
    fn justify_content_space_between() {
        let r = g(&format!("<div style='display:grid;width:300px;justify-content:space-between;grid-template-columns:50px 50px 50px'>{}</div>", boxes(3, "height:10px")));
        assert_eq!(r.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0.0, 125.0, 250.0]);
    }

    #[test]
    fn justify_content_space_around() {
        let r = g(&format!("<div style='display:grid;width:300px;justify-content:space-around;grid-template-columns:50px 50px'>{}</div>", boxes(2, "height:10px")));
        // free space 200: 50 around each track: 50, 150
        assert_eq!(r.iter().map(|x| x.0).collect::<Vec<_>>(), vec![50.0, 200.0]);
    }

    #[test]
    fn justify_content_space_evenly() {
        let r = g(&format!("<div style='display:grid;width:300px;justify-content:space-evenly;grid-template-columns:50px 50px'>{}</div>", boxes(2, "height:10px")));
        // free space 200 in 3 gaps of 66.67
        assert!((r[0].0 - 66.667).abs() < 0.05);
        assert!((r[1].0 - 183.333).abs() < 0.05);
    }

    #[test]
    fn justify_content_stretch_grows_auto_tracks_only() {
        let r = g(&format!("<div style='display:grid;width:300px;justify-content:stretch;grid-template-columns:50px auto'>{}</div>", boxes(2, "height:10px")));
        assert_eq!(r.iter().map(|x| (x.0, x.2)).collect::<Vec<_>>(), vec![(0.0, 50.0), (50.0, 250.0)]);
    }

    #[test]
    fn align_content_center_with_a_fixed_height() {
        let r = g(&format!("<div style='display:grid;height:100px;align-content:center;grid-template-rows:20px 20px'>{}</div>", boxes(2, "")));
        assert_eq!(r.iter().map(|x| x.1).collect::<Vec<_>>(), vec![30.0, 50.0]);
    }

    #[test]
    fn align_content_end_and_space_between() {
        let r = g(&format!("<div style='display:grid;height:100px;align-content:end;grid-template-rows:20px 20px'>{}</div>", boxes(2, "")));
        assert_eq!(r.iter().map(|x| x.1).collect::<Vec<_>>(), vec![60.0, 80.0]);
        let r = g(&format!("<div style='display:grid;height:100px;align-content:space-between;grid-template-rows:20px 20px'>{}</div>", boxes(2, "")));
        assert_eq!(r.iter().map(|x| x.1).collect::<Vec<_>>(), vec![0.0, 80.0]);
    }

    #[test]
    fn align_content_stretch_grows_auto_rows() {
        // the initial value `normal` behaves as stretch for a grid: two auto rows share 100px
        let r = g(&format!("<div style='display:grid;height:100px;grid-template-columns:50px'>{}</div>", boxes(2, "")));
        assert_eq!(r.iter().map(|x| (x.1, x.3)).collect::<Vec<_>>(), vec![(0.0, 50.0), (50.0, 50.0)]);
    }

    #[test]
    fn auto_height_items_stretch_to_their_area_by_default() {
        let r = g("<div style='display:grid;grid-template-columns:50px 50px'><div></div><div style='height:30px'></div></div>");
        assert_eq!(r[0].3, 30.0);
        assert_eq!(r[0].2, 50.0);
    }

    #[test]
    fn justify_items_and_align_items_start_end_center() {
        let body = |a: &str| format!("<div style='display:grid;grid-template-columns:100px;grid-template-rows:60px;{a}'><div style='width:20px;height:10px'></div></div>");
        assert_eq!(g(&body("justify-items:start;align-items:start")), vec![(0.0, 0.0, 20.0, 10.0)]);
        assert_eq!(g(&body("justify-items:end;align-items:end")), vec![(80.0, 50.0, 20.0, 10.0)]);
        assert_eq!(g(&body("justify-items:center;align-items:center")), vec![(40.0, 25.0, 20.0, 10.0)]);
    }

    #[test]
    fn justify_self_and_align_self_override_the_items_values() {
        let r = g("<div style='display:grid;justify-items:start;grid-template-columns:100px;grid-template-rows:60px'><div style='width:20px;height:10px;justify-self:end;align-self:center'></div></div>");
        assert_eq!(r, vec![(80.0, 25.0, 20.0, 10.0)]);
    }

    #[test]
    fn stretch_only_applies_to_auto_sizes() {
        let r = g("<div style='display:grid;grid-template-columns:100px;grid-template-rows:60px'><div style='width:20px'></div></div>");
        assert_eq!(r, vec![(0.0, 0.0, 20.0, 60.0)]);
    }

    #[test]
    fn auto_margins_center_the_item_in_its_area() {
        // 10.2 auto margins absorb the free space and disable the alignment
        let r = g("<div style='display:grid;grid-template-columns:100px;grid-template-rows:60px'><div style='width:20px;height:10px;margin:auto'></div></div>");
        assert_eq!(r, vec![(40.0, 25.0, 20.0, 10.0)]);
    }

    #[test]
    fn a_left_auto_margin_pushes_the_item_right() {
        let r = g("<div style='display:grid;grid-template-columns:100px'><div style='width:20px;height:10px;margin-left:auto'></div></div>");
        assert_eq!(r, vec![(80.0, 0.0, 20.0, 10.0)]);
    }

    #[test]
    fn item_margins_are_inside_the_grid_area() {
        // an auto-width item fills the area minus its margins
        let r = g("<div style='display:grid;grid-template-columns:100px;grid-template-rows:50px'><div style='margin:5px 10px 15px 20px'></div></div>");
        assert_eq!(r, vec![(20.0, 5.0, 70.0, 30.0)]);
    }

    #[test]
    fn item_margins_contribute_to_auto_track_sizes() {
        let r = g("<div style='display:grid;justify-content:start;grid-template-columns:auto auto'><div style='width:20px;height:10px;margin:0 5px'></div><div style='width:10px;height:10px'></div></div>");
        assert_eq!(r[0].0, 5.0);
        assert_eq!(r[1].0, 30.0);
    }

    #[test]
    fn items_spanning_auto_tracks_distribute_their_size() {
        // 11.5.1: a 100px wide item spanning two auto tracks makes them 100px together. The item's
        // minimum contribution is 0 (an empty item has an automatic minimum of 0), so the space is
        // only added to the growth limits and the tracks stay at their base sizes: the width goes to the
        // last track. (Checked against Firefox 152: the second item is 0px wide.)
        let r = g("<div style='display:grid;justify-content:start;grid-template-columns:auto auto'><div style='grid-column:span 2;width:100px;height:10px'></div><div style='height:5px'></div></div>");
        assert_eq!(r[0].2, 100.0);
        assert_eq!(r[1].2, 0.0);
    }

    #[test]
    fn a_spanning_item_only_grows_the_tracks_that_need_it() {
        // the 100px spanning item has 30px left over after the tracks' own 60 and 10: it is shared
        // equally, 15 each (so the second track starts at 75; checked against Firefox 152)
        let r = g("<div style='display:grid;justify-content:start;grid-template-columns:auto auto'><div style='width:60px;height:10px'></div><div style='height:10px;width:10px'></div><div style='grid-column:1 / span 2;width:100px;height:10px'></div></div>");
        assert_eq!(r[1].0, 75.0);
        assert_eq!(r[2].2, 100.0);
    }

    #[test]
    fn nested_grids_lay_out_in_their_area() {
        let result = layout("<div style='display:grid;grid-template-columns:100px 200px'><div></div><div style='display:grid;grid-template-columns:1fr 1fr'><div style='height:10px'></div><div></div></div></div>");
        let grids = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer);
        assert_eq!(grids.len(), 2);
        assert_eq!(rect(grids[1]).0, 100.0);
        assert_eq!(rect(grids[1]).2, 200.0);
        let inner = rects(&children(grids[1]));
        assert_eq!(inner, vec![(0.0, 0.0, 100.0, 10.0), (100.0, 0.0, 100.0, 10.0)]);
    }

    #[test]
    fn percentage_item_sizes_resolve_against_the_grid_area() {
        // 6.2 item percentages resolve against the grid area, not the container
        let r = g("<div style='display:grid;width:400px;grid-template-columns:100px 300px'><div style='width:50%;height:10px'></div><div style='width:50%;height:10px'></div></div>");
        assert_eq!(r[0].2, 50.0);
        assert_eq!(r[1].2, 150.0);
    }

    #[test]
    fn a_percentage_track_resolves_against_the_container() {
        let r = g("<div style='display:grid;width:400px;grid-template-columns:25% 75%'><div style='height:10px'></div><div></div></div>");
        assert_eq!(r.iter().map(|x| (x.0, x.2)).collect::<Vec<_>>(), vec![(0.0, 100.0), (100.0, 300.0)]);
    }

    #[test]
    fn text_in_an_item_sets_the_auto_track_to_its_max_content() {
        let (_, c, items) = container("<div style='display:grid;justify-content:start;grid-template-columns:auto;font:10px monospace'><div>aaaa bbbb</div></div>");
        // max-content is the whole text on one line; wider than its min-content (one word)
        let w = rect(items[0]).2;
        assert!(w > 0.0);
        let (_, _, narrow) = container("<div style='display:grid;justify-content:start;grid-template-columns:min-content;font:10px monospace'><div>aaaa bbbb</div></div>");
        assert!(rect(narrow[0]).2 < w);
        assert!(rect(narrow[0]).3 > rect(items[0]).3, "the min-content wraps to two lines");
        let _ = c;
    }

    #[test]
    fn text_directly_in_the_container_is_an_anonymous_item() {
        let (_, _, items) = container("<div style='display:grid;grid-template-columns:100px 100px;font:10px monospace'>hello<div style='height:30px'></div></div>");
        assert_eq!(items.len(), 2);
        assert_eq!(rect(items[0]).0, 0.0);
        assert_eq!(rect(items[1]).0, 100.0);
    }

    #[test]
    fn inline_grid_shrinks_to_fit_its_content() {
        let result = layout("<div style='display:inline-grid;grid-template-columns:30px 40px'><div style='height:10px'></div><div></div></div>");
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer)[0];
        assert_eq!(rect(c).2, 70.0);
        assert_eq!(rect(c).3, 10.0);
    }

    #[test]
    fn inline_grid_with_auto_tracks_is_max_content_wide() {
        let result = layout("<div style='display:inline-grid;grid-template-columns:auto auto'><div style='width:30px;height:10px'></div><div style='width:50px'></div></div>");
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer)[0];
        assert_eq!(rect(c).2, 80.0);
    }

    #[test]
    fn a_grid_in_a_float_shrinks_to_fit() {
        let result = layout("<div style='float:left'><div style='display:grid;grid-template-columns:30px auto'><div style='height:10px'></div><div style='width:20px'></div></div></div>");
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer)[0];
        assert_eq!(rect(c).2, 50.0);
    }

    #[test]
    fn a_grid_in_an_inline_block_shrinks_to_fit() {
        let result = layout("<div style='display:inline-block'><div style='display:grid;grid-template-columns:30px auto'><div style='height:10px'></div><div style='width:25px'></div></div></div>");
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer)[0];
        assert_eq!(rect(c).2, 55.0);
    }

    #[test]
    fn a_fixed_height_container_keeps_its_height() {
        let (_, c, _) = container("<div style='display:grid;height:200px;grid-template-columns:50px'><div style='height:10px'></div></div>");
        assert_eq!(rect(c).3, 200.0);
    }

    #[test]
    fn padding_and_border_of_the_container_offset_the_tracks() {
        let (_, c, items) = container("<div style='display:grid;padding:5px;border:3px solid;grid-template-columns:50px'><div style='height:10px'></div></div>");
        assert_eq!(rect(c).2, 1000.0);
        assert_eq!(rect(c).3, 10.0 + 16.0);
        assert_eq!(rect(items[0]), (8.0, 8.0, 50.0, 10.0));
    }

    #[test]
    fn fr_tracks_use_the_content_box_of_the_container() {
        let r = g("<div style='display:grid;width:200px;padding:0 20px;box-sizing:border-box;grid-template-columns:1fr'><div style='height:10px'></div></div>");
        assert_eq!(r[0].2, 160.0);
        assert_eq!(r[0].0, 20.0);
    }

    #[test]
    fn padding_border_of_an_item_is_inside_its_area() {
        let r = g("<div style='display:grid;grid-template-columns:100px;grid-template-rows:50px'><div style='padding:5px;border:1px solid'></div></div>");
        assert_eq!(r, vec![(0.0, 0.0, 100.0, 50.0)]);
    }

    #[test]
    fn item_padding_adds_to_the_auto_track_size() {
        let r = g("<div style='display:grid;justify-content:start;grid-template-columns:auto'><div style='width:20px;height:10px;padding:5px;border:1px solid'></div></div>");
        assert_eq!(r[0].2, 32.0);
    }

    #[test]
    fn a_min_height_on_an_item_enlarges_its_row() {
        let r = g("<div style='display:grid;grid-template-columns:50px 50px'><div style='min-height:40px'></div><div style='height:10px'></div><div style='height:5px'></div></div>");
        assert_eq!(r[0].3, 40.0);
        assert_eq!(r[2].1, 40.0);
    }

    #[test]
    fn a_max_width_limits_a_stretched_item() {
        let r = g("<div style='display:grid;grid-template-columns:100px'><div style='max-width:60px;height:10px'></div></div>");
        assert_eq!(r[0].2, 60.0);
        assert_eq!(r[0].0, 0.0);
    }

    #[test]
    fn an_item_with_an_aspect_ratio_gets_its_height_from_the_width() {
        let r = g("<div style='display:grid;grid-template-columns:100px;align-items:start'><div style='aspect-ratio:2'></div></div>");
        assert_eq!(r[0].2, 100.0);
        assert_eq!(r[0].3, 50.0);
    }

    #[test]
    fn order_changes_the_placement_order() {
        // 6.2 / 8.5: auto placement follows the order-modified document order
        let r = g("<div style='display:grid;grid-template-columns:50px 50px'><div style='order:2;height:10px'></div><div style='height:10px'></div></div>");
        // the frames keep document order; positions follow the order
        assert_eq!(r[0].0, 50.0);
        assert_eq!(r[1].0, 0.0);
    }

    #[test]
    fn display_none_items_take_no_cell() {
        let r = g("<div style='display:grid;grid-template-columns:50px 50px'><div style='display:none'></div><div style='height:10px'></div><div style='height:10px'></div></div>");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0, 0.0);
        assert_eq!(r[1].0, 50.0);
    }

    #[test]
    fn a_grid_inside_a_flex_item() {
        let result = layout("<div style='display:flex;width:300px'><div style='flex:1'><div style='display:grid;grid-template-columns:1fr 1fr'><div style='height:10px'></div><div></div></div></div></div>");
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer)[0];
        assert_eq!(rect(c).2, 300.0);
        assert_eq!(rects(&children(c)), vec![(0.0, 0.0, 150.0, 10.0), (150.0, 0.0, 150.0, 10.0)]);
    }

    #[test]
    fn a_flex_container_as_a_grid_item() {
        let result = layout("<div style='display:grid;grid-template-columns:200px'><div style='display:flex'><div style='width:50px;height:10px'></div></div></div>");
        let f = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer)[0];
        assert_eq!(rect(f).2, 200.0);
        assert_eq!(rect(f).3, 10.0);
    }

    #[test]
    fn the_intrinsic_width_of_a_grid_with_fixed_tracks_is_their_sum_plus_gaps() {
        let result = layout("<div style='float:left'><div style='display:grid;grid-template-columns:30px 40px;column-gap:6px'><div style='height:10px'></div><div></div></div></div>");
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer)[0];
        assert_eq!(rect(c).2, 76.0);
    }

    #[test]
    fn grid_template_columns_none_makes_one_implicit_auto_column() {
        let r = g("<div style='display:grid;grid-template-columns:none;font:10px monospace'><div style='height:10px'></div><div style='height:10px'></div></div>");
        // one implicit auto column (stretched to the container), the items stack
        assert_eq!(r, vec![(0.0, 0.0, 1000.0, 10.0), (0.0, 10.0, 1000.0, 10.0)]);
    }

    #[test]
    fn an_empty_grid_has_no_height() {
        let (_, c, _) = container("<div style='display:grid;grid-template-columns:50px'></div>");
        assert_eq!(rect(c).3, 0.0);
    }

    #[test]
    fn explicit_empty_rows_still_take_their_size() {
        let (_, c, _) = container("<div style='display:grid;grid-template-columns:50px;grid-template-rows:30px 30px'></div>");
        assert_eq!(rect(c).3, 60.0);
    }

    #[test]
    fn rows_given_in_percent_of_an_auto_height_container_behave_as_auto() {
        let (_, c, _) = container("<div style='display:grid;grid-template-columns:50px;grid-template-rows:50%'><div style='height:20px'></div></div>");
        assert_eq!(rect(c).3, 20.0);
    }

    #[test]
    fn items_overlap_when_placed_in_the_same_cell() {
        let r = g("<div style='display:grid;grid-template-columns:50px;grid-template-rows:10px'><div style='grid-area:1/1'></div><div style='grid-area:1/1'></div></div>");
        assert_eq!(r[0], r[1]);
    }

    #[test]
    fn a_table_as_a_grid_item_is_sized_through_its_wrapper() {
        let result = layout("<div style='display:grid;grid-template-columns:200px'><table><tr><td style='width:50px;height:10px'></td></tr></table></div>");
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::GridContainer)[0];
        let items = children(c);
        assert_eq!(items.len(), 1);
        assert_eq!(rect(items[0]).0, 0.0);
    }
}
