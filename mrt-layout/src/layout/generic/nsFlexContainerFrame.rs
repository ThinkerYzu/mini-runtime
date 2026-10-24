// Translated from layout/generic/nsFlexContainerFrame.h and nsFlexContainerFrame.cpp (GECKO_REV), stage 2b.
//
// The file follows the order of the .cpp (the helper classes FlexboxAxisTracker, FlexItem and FlexLine
// are defined in the .cpp at their point of first use; the header's FlexboxAxisInfo comes first).
//
// Rust shape (see CONVENTIONS.md):
//  - `nsFlexContainerFrame` is a `Copy` handle over the frame (an nsIFrame of type FlexContainer) that
//    derefs to nsIFrame, as the C++ class derives from nsContainerFrame. The data members
//    (`mCachedIntrinsicSizes`, `mFirstBaseline`, `mLastBaseline`) live in one heap block per frame behind a
//    frame property, as in nsBlockFrame.rs and nsTableCellFrame.rs (see nsFlexContainerFrame_Members).
//  - What is virtual in C++ is a `match` on the frame type in nsIFrame.rs / nsLineLayoutAdapter.rs /
//    nsContainerFrame.rs (`Reflow`, `IntrinsicISize`, `GetNaturalBaselineBOffset`, `AppendFrames`, ...).
//  - `FlexItem`, `FlexLine`, `FlexboxAxisTracker`, `StrutInfo` and the other nested classes of
//    nsFlexContainerFrame are top-level types of this file with their C++ names (the nesting `Outer::Inner` is
//    only a namespace in C++ here, see CONVENTIONS.md on nested types: `nsFlexContainerFrame_FlexItem` is
//    not used because the names are unique in the crate).
//  - `FlexItem::mAscent` and `mAscentForLast` are `mutable` in C++ and are `Cell<nscoord>` here.
//  - AuCoord64 (mozilla/gfx/Coord.h) is not translated (see gfx/src/nsCoord.rs); it is `i64` here.
//  - `FlexItem&`/`FlexLine&` that C++ takes from an `nsTArray<FlexLine>` are indexes into the `Vec`, or
//    `&mut` borrows of the elements where the borrow checker allows, as the C++ lines say.
//  - The log macros FLEX_LOG, FLEX_ITEM_LOG and FLEX_LOGV (MOZ_LOG on the "FlexContainer" log module) are
//    macros that expand to nothing (logging is not ported).
//
// NOT PORTED: painting (BuildDisplayList), accessibility, the devtools data (ComputedFlexItemInfo,
// ComputedFlexLineInfo, ComputedFlexContainerInfo, GetFlexFrameWithComputedInfo, CreateOrClearFlexContainerInfo,
// CreateFlexLineAndFlexItemInfo, ComputeFlexDirections, UpdateFlexLineAndItemInfo: they only serve
// Element.getAsFlexContainer()), fragmentation and pagination (the code is translated; every path that needs
// a constrained available block-size is marked where Gecko's helper is not ported), absolutely positioned
// children (they get no frame in this runtime), the legacy -webkit-box emulation (nsStyleXUL is not in this
// crate: IsLegacyWebkitBox() is always false) and the nsILineIterator implementation (caret and selection).
// Each is marked where it occurs. Refcount logging (MOZ_COUNT_CTOR) is not ported.
use crate::*;
use std::cell::Cell;
use std::ops::Deref;

// Gecko: layout/generic/nsFlexContainerFrame.cpp FLEX_LOG, FLEX_ITEM_LOG, FLEX_LOGV
// NOT PORTED: logging.
macro_rules! FLEX_LOG {
    ($($t:tt)*) => {};
}
macro_rules! FLEX_ITEM_LOG {
    ($($t:tt)*) => {};
}
macro_rules! FLEX_LOGV {
    ($($t:tt)*) => {};
}

// Gecko: layout/generic/nsFlexContainerFrame.h nsFlexContainerFrame
// ADAPTER: a handle (see the notes at the top of the file).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct nsFlexContainerFrame(pub nsIFrame);

impl Deref for nsFlexContainerFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/generic/nsFlexContainerFrame.h ComputedFlexItemInfo, ComputedFlexLineInfo,
// ComputedFlexContainerInfo
// NOT PORTED: devtools data (nsFlexContainerFrame.h lines 21-70 at GECKO_REV); only
// GetFlexFrameWithComputedInfo and the CreateFlexLineAndFlexItemInfo family fill them.

// ADAPTER: the C++ initializer list `{ComputeSizeFlag::ShrinkWrap}` (EnumSet has no such constructor here).
fn FlexShrinkWrapFlags() -> ComputeSizeFlags {
    let mut flags = ComputeSizeFlags::default();
    flags += ComputeSizeFlag::ShrinkWrap;
    flags
}

// Gecko: layout/generic/nsFlexContainerFrame.h FlexboxAxisInfo
// (the constructor and its two helpers are in the .cpp, below)
pub struct FlexboxAxisInfo {
    // Is our main axis the inline axis? (Are we 'flex-direction:row[-reverse]'?)
    pub mIsRowOriented: bool,

    // Is our main axis in the opposite direction as mWM's corresponding axis?
    // (e.g. RTL vs LTR)
    pub mIsMainAxisReversed: bool,

    // Is our cross axis in the opposite direction as mWM's corresponding axis?
    // (e.g. BTT vs TTB)
    pub mIsCrossAxisReversed: bool,
}

// ADAPTER: the members of the C++ class nsFlexContainerFrame that are plain data, in one heap block per
// frame reached through a frame property (see nsBlockFrame_Members for the scheme). The block lives as long
// as the frame (frames are never freed).
pub struct nsFlexContainerFrame_Members {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::mCachedIntrinsicSizes
    pub mCachedIntrinsicSizes: IntrinsicISizesCache,
    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::mFirstBaseline
    pub mFirstBaseline: nscoord,
    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::mLastBaseline
    pub mLastBaseline: nscoord,
}

// (ids: this crate's own numbers; 7100..7199 belong to the flex container and its items.)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(FlexMembersProperty, usize, 7100);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(CachedFlexItemDataProperty, usize, 7101);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(SharedFlexDataProperty, usize, 7102);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(PerFragmentFlexDataProperty, usize, 7103);

// Gecko: layout/generic/nsFlexContainerFrame.cpp NS_NewFlexContainerFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class (what the C++ constructor
// passes as kClassID).
pub fn NS_NewFlexContainerFrame(aFrame: nsIFrame) -> nsFlexContainerFrame {
    aFrame.SetType(LayoutFrameType::FlexContainer);
    nsFlexContainerFrame(aFrame)
}

// Returns the OrderState enum we should pass to CSSOrderAwareFrameIterator
// (depending on whether aFlexContainer has
// NS_STATE_FLEX_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER state bit).
// Gecko: layout/generic/nsFlexContainerFrame.cpp OrderStateForIter
pub(crate) fn OrderStateForIter(aFlexContainer: nsFlexContainerFrame) -> CSSOrderAwareFrameIterator_OrderState {
    if aFlexContainer.HasAnyStateBits(NS_STATE_FLEX_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER) {
        CSSOrderAwareFrameIterator_OrderState::Ordered
    } else {
        CSSOrderAwareFrameIterator_OrderState::Unordered
    }
}

// Returns the OrderingProperty enum that we should pass to
// CSSOrderAwareFrameIterator (depending on whether it's a legacy box).
// Gecko: layout/generic/nsFlexContainerFrame.cpp OrderingPropertyForIter
pub(crate) fn OrderingPropertyForIter(aFlexContainer: nsFlexContainerFrame) -> CSSOrderAwareFrameIterator_OrderingProperty {
    if aFlexContainer.IsLegacyWebkitBox() {
        CSSOrderAwareFrameIterator_OrderingProperty::BoxOrdinalGroup
    } else {
        CSSOrderAwareFrameIterator_OrderingProperty::Order
    }
}

// Returns the "align-items" value that's equivalent to the legacy "box-align"
// value in the given style struct.
// Gecko: layout/generic/nsFlexContainerFrame.cpp ConvertLegacyStyleToAlignItems
// NOT PORTED: legacy -webkit-box (nsStyleXUL::mBoxAlign is not in this crate; IsLegacyWebkitBox() is
// always false, so nothing calls it). Original (nsFlexContainerFrame.cpp lines 83-105 at GECKO_REV):
//   switch (aStyleXUL->mBoxAlign) { Stretch => STRETCH, Start => FLEX_START, Center => CENTER,
//   Baseline => BASELINE, End => FLEX_END }
fn ConvertLegacyStyleToAlignItems() -> StyleAlignFlags {
    unimplemented!("NOT PORTED: legacy -webkit-box (ConvertLegacyStyleToAlignItems)")
}

// Returns the "justify-content" value that's equivalent to the legacy
// "box-pack" value in the given style struct.
// Gecko: layout/generic/nsFlexContainerFrame.cpp ConvertLegacyStyleToJustifyContent
// NOT PORTED: legacy -webkit-box (nsStyleXUL::mBoxPack). Original (lines 109-124 at GECKO_REV):
//   switch (aStyleXUL->mBoxPack) { Start => FLEX_START, Center => CENTER, End => FLEX_END,
//   Justify => SPACE_BETWEEN }
fn ConvertLegacyStyleToJustifyContent() -> StyleContentDistribution {
    unimplemented!("NOT PORTED: legacy -webkit-box (ConvertLegacyStyleToJustifyContent)")
}

// Check if the size is auto or it is a keyword in the block axis.
// |aIsInline| should represent whether aSize is in the inline axis, from the
// perspective of the writing mode of the flex item that the size comes from.
//
// max-content and min-content should behave as property's initial value.
// Bug 567039: We treat -moz-fit-content and -moz-available as property's
// initial value for now.
// Gecko: layout/generic/nsFlexContainerFrame.cpp IsAutoOrEnumOnBSize
fn IsAutoOrEnumOnBSize(aSize: &StyleSize, aIsInline: bool) -> bool {
    aSize.IsAuto() || (!aIsInline && !aSize.IsLengthPercentage())
}

// Returns true if the flex container should be treated as a single-line
// container.
// Gecko: layout/generic/nsFlexContainerFrame.cpp IsSingleLine
fn IsSingleLine(aFlexContainer: nsIFrame, aStylePos: &nsStylePosition) -> bool {
    MOZ_ASSERT!(aFlexContainer.IsFlexContainerFrame());

    if aFlexContainer.IsLegacyWebkitBox() {
        // For legacy -webkit-{inline-}box, ignore the flex-wrap property.
        // These containers are always treated as single-line.
        return true;
    }
    // ADAPTER: Stylo's servo configuration makes FlexWrap a set of flags (NOWRAP, WRAP, WRAP_REVERSE).
    aStylePos.mFlexWrap() == StyleFlexWrap::NOWRAP
}

// Encapsulates our flex container's main & cross axes. This class is backed by
// a FlexboxAxisInfo helper member variable, and it adds some convenience APIs
// on top of what that struct offers.
// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::FlexboxAxisTracker
// (the constructor, MainAxisStartSide and CrossAxisStartSide are with the other FlexboxAxis* functions)
pub struct FlexboxAxisTracker {
    // The flex container's writing mode.
    mWM: WritingMode,
    mAxisInfo: FlexboxAxisInfo,
}

impl FlexboxAxisTracker {
    // Accessors:
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::MainAxis
    pub fn MainAxis(&self) -> LogicalAxis {
        if self.IsRowOriented() { LogicalAxis::Inline } else { LogicalAxis::Block }
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::CrossAxis
    pub fn CrossAxis(&self) -> LogicalAxis {
        if self.IsRowOriented() { LogicalAxis::Block } else { LogicalAxis::Inline }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::MainAxisEndSide
    pub fn MainAxisEndSide(&self) -> LogicalSide {
        GetOppositeSide(self.MainAxisStartSide())
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::CrossAxisEndSide
    pub fn CrossAxisEndSide(&self) -> LogicalSide {
        GetOppositeSide(self.CrossAxisStartSide())
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::MainAxisPhysicalStartSide
    pub fn MainAxisPhysicalStartSide(&self) -> Side {
        self.mWM.PhysicalSide(self.MainAxisStartSide())
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::MainAxisPhysicalEndSide
    pub fn MainAxisPhysicalEndSide(&self) -> Side {
        self.mWM.PhysicalSide(self.MainAxisEndSide())
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::CrossAxisPhysicalStartSide
    pub fn CrossAxisPhysicalStartSide(&self) -> Side {
        self.mWM.PhysicalSide(self.CrossAxisStartSide())
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::CrossAxisPhysicalEndSide
    pub fn CrossAxisPhysicalEndSide(&self) -> Side {
        self.mWM.PhysicalSide(self.CrossAxisEndSide())
    }

    // Returns the flex container's writing mode.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::GetWritingMode
    pub fn GetWritingMode(&self) -> WritingMode {
        self.mWM
    }

    // Returns true if our main axis is in the reverse direction of our
    // writing mode's corresponding axis. (From 'flex-direction: *-reverse')
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::IsMainAxisReversed
    pub fn IsMainAxisReversed(&self) -> bool {
        self.mAxisInfo.mIsMainAxisReversed
    }
    // Returns true if our cross axis is in the reverse direction of our
    // writing mode's corresponding axis. (From 'flex-wrap: *-reverse')
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::IsCrossAxisReversed
    pub fn IsCrossAxisReversed(&self) -> bool {
        self.mAxisInfo.mIsCrossAxisReversed
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::IsRowOriented
    pub fn IsRowOriented(&self) -> bool {
        self.mAxisInfo.mIsRowOriented
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::IsColumnOriented
    pub fn IsColumnOriented(&self) -> bool {
        !self.IsRowOriented()
    }

    // aSize is expected to match the flex container's WritingMode.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::MainComponent(const LogicalSize&)
    pub fn MainComponent__LogicalSize(&self, aSize: LogicalSize) -> nscoord {
        if self.IsRowOriented() { aSize.ISize(self.mWM) } else { aSize.BSize(self.mWM) }
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::MainComponent(const LayoutDeviceIntSize&)
    // NOT PORTED: LayoutDeviceIntSize (only the devtools data code uses it).

    // aSize is expected to match the flex container's WritingMode.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::CrossComponent(const LogicalSize&)
    pub fn CrossComponent__LogicalSize(&self, aSize: LogicalSize) -> nscoord {
        if self.IsRowOriented() { aSize.BSize(self.mWM) } else { aSize.ISize(self.mWM) }
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::CrossComponent(const LayoutDeviceIntSize&)
    // NOT PORTED: LayoutDeviceIntSize (only the devtools data code uses it).

    // NOTE: aMargin is expected to use the flex container's WritingMode.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::MarginSizeInMainAxis
    pub fn MarginSizeInMainAxis(&self, aMargin: LogicalMargin) -> nscoord {
        // If we're row-oriented, our main axis is the inline axis.
        if self.IsRowOriented() { aMargin.IStartEnd(self.mWM) } else { aMargin.BStartEnd(self.mWM) }
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::MarginSizeInCrossAxis
    pub fn MarginSizeInCrossAxis(&self, aMargin: LogicalMargin) -> nscoord {
        // If we're row-oriented, our cross axis is the block axis.
        if self.IsRowOriented() { aMargin.BStartEnd(self.mWM) } else { aMargin.IStartEnd(self.mWM) }
    }

    /**
     * Converts a "flex-relative" point (a main-axis & cross-axis coordinate)
     * into a LogicalPoint, using the flex container's writing mode.
     *
     *  @arg aMainCoord  The main-axis coordinate -- i.e an offset from the
     *                   main-start edge of the flex container's content box.
     *  @arg aCrossCoord The cross-axis coordinate -- i.e an offset from the
     *                   cross-start edge of the flex container's content box.
     *  @arg aContainerMainSize  The main size of flex container's content box.
     *  @arg aContainerCrossSize The cross size of flex container's content box.
     *  @return A LogicalPoint, with the flex container's writing mode, that
     *          represents the same position. The logical coordinates are
     *          relative to the flex container's content box.
     */
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::LogicalPointFromFlexRelativePoint
    pub fn LogicalPointFromFlexRelativePoint(&self, aMainCoord: nscoord, aCrossCoord: nscoord, aContainerMainSize: nscoord, aContainerCrossSize: nscoord) -> LogicalPoint {
        let logicalCoordInMainAxis = if self.IsMainAxisReversed() { aContainerMainSize - aMainCoord } else { aMainCoord };
        let logicalCoordInCrossAxis = if self.IsCrossAxisReversed() { aContainerCrossSize - aCrossCoord } else { aCrossCoord };

        if self.IsRowOriented() {
            LogicalPoint::new(self.mWM, logicalCoordInMainAxis, logicalCoordInCrossAxis)
        } else {
            LogicalPoint::new(self.mWM, logicalCoordInCrossAxis, logicalCoordInMainAxis)
        }
    }

    /**
     * Converts a "flex-relative" size (a main-axis & cross-axis size)
     * into a LogicalSize, using the flex container's writing mode.
     *
     *  @arg aMainSize  The main-axis size.
     *  @arg aCrossSize The cross-axis size.
     *  @return A LogicalSize, with the flex container's writing mode, that
     *          represents the same size.
     */
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::LogicalSizeFromFlexRelativeSizes
    pub fn LogicalSizeFromFlexRelativeSizes(&self, aMainSize: nscoord, aCrossSize: nscoord) -> LogicalSize {
        if self.IsRowOriented() {
            LogicalSize::new(self.mWM, aMainSize, aCrossSize)
        } else {
            LogicalSize::new(self.mWM, aCrossSize, aMainSize)
        }
    }

    /**
     * Converts a "flex-relative" ascent (the distance from the flex container's
     * content-box cross-start edge to its baseline) into a logical ascent (the
     * distance from the flex container's content-box block-start edge to its
     * baseline).
     */
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::LogicalAscentFromFlexRelativeAscent
    pub fn LogicalAscentFromFlexRelativeAscent(&self, aFlexRelativeAscent: nscoord, aContentBoxCrossSize: nscoord) -> nscoord {
        if self.IsCrossAxisReversed() { aContentBoxCrossSize - aFlexRelativeAscent } else { aFlexRelativeAscent }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::IsMainAxisHorizontal
    pub fn IsMainAxisHorizontal(&self) -> bool {
        // If we're row-oriented, and our writing mode is NOT vertical,
        // or we're column-oriented and our writing mode IS vertical,
        // then our main axis is horizontal. This handles all cases:
        self.IsRowOriented() != self.mWM.IsVertical()
    }

    // Returns true if this flex item's inline axis in aItemWM is parallel (or
    // antiparallel) to the container's main axis. Returns false, otherwise.
    //
    // Note: this is a helper used before constructing FlexItem. Inside of flex
    // reflow code, FlexItem::IsInlineAxisMainAxis() is equivalent & more optimal.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::IsInlineAxisMainAxis
    pub fn IsInlineAxisMainAxis(&self, aItemWM: WritingMode) -> bool {
        self.IsRowOriented() != self.GetWritingMode().IsOrthogonalTo(&aItemWM)
    }

    // Maps justify-*: 'left' or 'right' to 'start' or 'end'.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::ResolveJustifyLeftRight
    pub fn ResolveJustifyLeftRight(&self, aFlags: StyleAlignFlags) -> StyleAlignFlags {
        MOZ_ASSERT!(aFlags == StyleAlignFlags::LEFT || aFlags == StyleAlignFlags::RIGHT, "This helper accepts only 'LEFT' or 'RIGHT' flags!");

        let wm = self.GetWritingMode();
        let isJustifyLeft = aFlags == StyleAlignFlags::LEFT;
        if self.IsColumnOriented() {
            if !wm.IsVertical() {
                // Container's alignment axis (main axis) is *not* parallel to the
                // line-left <-> line-right axis or the physical left <-> physical right
                // axis, so we map both 'left' and 'right' to 'start'.
                return StyleAlignFlags::START;
            }

            MOZ_ASSERT!(wm.PhysicalAxis(self.MainAxis()) == PhysicalAxis::Horizontal, "Vertical column-oriented flex container's main axis should be parallel to physical left <-> right axis!");
            // Map 'left' or 'right' to 'start' or 'end', depending on its block flow
            // direction.
            return if isJustifyLeft == wm.IsVerticalLR() { StyleAlignFlags::START } else { StyleAlignFlags::END };
        }

        MOZ_ASSERT!(self.MainAxis() == LogicalAxis::Inline, "Row-oriented flex container's main axis should be parallel to line-left <-> line-right axis!");

        // If we get here, we're operating on the flex container's inline axis,
        // so we map 'left' to whichever of 'start' or 'end' corresponds to the
        // *line-relative* left side; and similar for 'right'.
        if isJustifyLeft == wm.IsBidiLTR() { StyleAlignFlags::START } else { StyleAlignFlags::END }
    }

    // Delete copy-constructor & reassignment operator, to prevent accidental
    // (unnecessary) copying.
    // (FlexboxAxisTracker is not Clone here.)
}

/**
 * Represents a flex item.
 * Includes the various pieces of input that the Flexbox Layout Algorithm uses
 * to resolve a flexible width.
 */
// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::FlexItem
// (the members are at the end of the C++ class; they are here too, since a Rust struct has them in one place)
#[derive(Clone)]
pub struct FlexItem {
    // Values that we already know in constructor, and remain unchanged:
    // The flex item's frame.
    mFrame: nsIFrame,
    mFlexGrow: f32,
    mFlexShrink: f32,
    mAspectRatio: AspectRatio,

    // The flex item's writing mode.
    mWM: WritingMode,

    // The flex container's writing mode.
    mCBWM: WritingMode,

    // The flex container's main axis in flex container's writing mode.
    mMainAxis: LogicalAxis,

    // Stored in flex container's writing mode.
    mBorderPadding: LogicalMargin,

    // Stored in flex container's writing mode. Its value can change when we
    // resolve "auto" marigns.
    mMargin: LogicalMargin,

    // These are non-const so that we can lazily update them with the item's
    // intrinsic size (obtained via a "measuring" reflow), when necessary.
    // (e.g. for "flex-basis:auto;height:auto" & "min-height:auto")
    mFlexBaseSize: nscoord,
    mMainMinSize: nscoord,
    mMainMaxSize: nscoord,

    // mCrossMinSize and mCrossMaxSize are not changed after constructor.
    mCrossMinSize: nscoord,
    mCrossMaxSize: nscoord,

    // Values that we compute after constructor:
    mMainSize: nscoord,
    mMainPosn: nscoord,
    mCrossSize: nscoord,
    mCrossPosn: nscoord,

    // Mutable b/c it's set & resolved lazily, sometimes via const pointer. See
    // comment above SetAscent().
    // We initialize this to ASK_FOR_BASELINE, and opportunistically fill it in
    // with a real value if we end up reflowing this flex item. (But if we don't
    // reflow this flex item, then this sentinel tells us that we don't know it
    // yet & anyone who cares will need to explicitly request it.)
    //
    // Both mAscent and mAscentForLast are distance from the frame's border-box
    // block-start edge.
    mAscent: Cell<nscoord>,
    mAscentForLast: Cell<nscoord>,

    // Temporary state, while we're resolving flexible widths (for our main size)
    // XXXdholbert To save space, we could use a union to make these variables
    // overlay the same memory as some other member vars that aren't touched
    // until after main-size has been resolved. In particular, these could share
    // memory with mMainPosn through mAscent, and mIsStretched.
    mShareOfWeightSoFar: f64,

    mIsFrozen: bool,
    mHadMinViolation: bool,
    mHadMaxViolation: bool,

    // Did this item get a preliminary reflow, to measure its desired height?
    mHadMeasuringReflow: bool,

    // See IsStretched() documentation.
    mIsStretched: bool,

    // Is this item a "strut" left behind by an element with visibility:collapse?
    mIsStrut: bool,

    // See IsInlineAxisMainAxis() documentation. This is not changed after
    // constructor.
    mIsInlineAxisMainAxis: bool,

    // Does this item need to resolve a min-[width|height]:auto (in main-axis)?
    //
    // Note: mNeedsMinSizeAutoResolution needs to be declared towards the end of
    // the member variables since it's initialized in a method that depends on
    // other members declared above such as mCBWM, mMainAxis, and
    // mIsInlineAxisMainAxis.
    mNeedsMinSizeAutoResolution: bool,

    // Should we take care to treat this item's resolved BSize as indefinite?
    mTreatBSizeAsIndefinite: bool,

    // Does this item have an auto margin in either main or cross axis?
    mHasAnyAutoMargin: bool,

    // Does this item have a content-based flex base size (and is that a size in
    // its block-axis)?
    mIsFlexBaseSizeContentBSize: bool,

    // Does this item have a content-based resolved auto min size (and is that a
    // size in its block-axis)?
    mIsMainMinSizeContentBSize: bool,

    // If this item is {first,last}-baseline-aligned using 'align-self', which of
    // its FlexLine's baseline sharing groups does it participate in?
    mBaselineSharingGroup: BaselineSharingGroup,

    // My "align-self" computed value (with "auto" swapped out for parent"s
    // "align-items" value, in our constructor).
    mAlignSelf: StyleAlignFlags,

    // Flags for 'align-self' (safe/unsafe/legacy).
    mAlignSelfFlags: StyleAlignFlags,
}

impl FlexItem {
    // The default member initializers of the C++ class, for the two constructors.
    // ADAPTER: not in C++.
    fn WithDefaultMembers(aFrame: nsIFrame, aWM: WritingMode, aCBWM: WritingMode, aMainAxis: LogicalAxis) -> FlexItem {
        FlexItem {
            mFrame: aFrame,
            mFlexGrow: 0.0,
            mFlexShrink: 0.0,
            mAspectRatio: AspectRatio::default(),
            mWM: aWM,
            mCBWM: aCBWM,
            mMainAxis: aMainAxis,
            mBorderPadding: LogicalMargin::new__WritingMode(aCBWM),
            mMargin: LogicalMargin::new__WritingMode(aCBWM),
            mFlexBaseSize: 0,
            mMainMinSize: 0,
            mMainMaxSize: 0,
            mCrossMinSize: 0,
            mCrossMaxSize: 0,
            mMainSize: 0,
            mMainPosn: 0,
            mCrossSize: 0,
            mCrossPosn: 0,
            mAscent: Cell::new(ReflowOutput::ASK_FOR_BASELINE),
            mAscentForLast: Cell::new(ReflowOutput::ASK_FOR_BASELINE),
            mShareOfWeightSoFar: 0.0,
            mIsFrozen: false,
            mHadMinViolation: false,
            mHadMaxViolation: false,
            mHadMeasuringReflow: false,
            mIsStretched: false,
            mIsStrut: false,
            mIsInlineAxisMainAxis: true,
            mNeedsMinSizeAutoResolution: false,
            mTreatBSizeAsIndefinite: false,
            mHasAnyAutoMargin: false,
            mIsFlexBaseSizeContentBSize: false,
            mIsMainMinSizeContentBSize: false,
            mBaselineSharingGroup: BaselineSharingGroup::First,
            mAlignSelf: StyleAlignFlags::AUTO,
            mAlignSelfFlags: StyleAlignFlags::empty(),
        }
    }

    // Clone existing FlexItem for its underlying frame's continuation.
    // @param aContinuation a continuation in our next-in-flow chain.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::CloneFor
    pub fn CloneFor(&self, aContinuation: nsIFrame) -> FlexItem {
        MOZ_ASSERT!(self.Frame() == aContinuation.FirstInFlow(), "aContinuation should be in aItem's continuation chain!");
        let mut item = self.clone();
        item.mFrame = aContinuation;
        item.mHadMeasuringReflow = false;
        item
    }

    // Accessors
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::Frame
    pub fn Frame(&self) -> nsIFrame {
        self.mFrame
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::FlexBaseSize
    pub fn FlexBaseSize(&self) -> nscoord {
        self.mFlexBaseSize
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::MainMinSize
    pub fn MainMinSize(&self) -> nscoord {
        MOZ_ASSERT!(!self.mNeedsMinSizeAutoResolution, "Someone's using an unresolved 'auto' main min-size");
        self.mMainMinSize
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::MainMaxSize
    pub fn MainMaxSize(&self) -> nscoord {
        self.mMainMaxSize
    }

    // Note: These return the main-axis position and size of our *content box*.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::MainSize
    pub fn MainSize(&self) -> nscoord {
        self.mMainSize
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::MainPosition
    pub fn MainPosition(&self) -> nscoord {
        self.mMainPosn
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::CrossMinSize
    pub fn CrossMinSize(&self) -> nscoord {
        self.mCrossMinSize
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::CrossMaxSize
    pub fn CrossMaxSize(&self) -> nscoord {
        self.mCrossMaxSize
    }

    // Note: These return the cross-axis position and size of our *content box*.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::CrossSize
    pub fn CrossSize(&self) -> nscoord {
        self.mCrossSize
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::CrossPosition
    pub fn CrossPosition(&self) -> nscoord {
        self.mCrossPosn
    }

    // Lazy getter for mAscent or mAscentForLast.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::ResolvedAscent
    pub fn ResolvedAscent(&self, aUseFirstBaseline: bool) -> nscoord {
        // XXX We should be using the *container's* writing-mode (mCBWM) here,
        // instead of the item's (mWM). This is essentially bug 1155322.
        let ascent = if aUseFirstBaseline { &self.mAscent } else { &self.mAscentForLast };
        if ascent.get() != ReflowOutput::ASK_FOR_BASELINE {
            return ascent.get();
        }

        // Use GetFirstLineBaseline() or GetLastLineBaseline() as appropriate:
        let mut value = ascent.get();
        let found = if aUseFirstBaseline {
            nsLayoutUtils::GetFirstLineBaseline(self.mWM, self.mFrame, &mut value)
        } else {
            nsLayoutUtils::GetLastLineBaseline(self.mWM, self.mFrame, &mut value)
        };
        if found {
            ascent.set(value);
            return value;
        }

        // If the nsLayoutUtils getter fails, then ask the frame directly:
        let baselineGroup = if aUseFirstBaseline { BaselineSharingGroup::First } else { BaselineSharingGroup::Last };
        if let Some(baseline) = self.mFrame.GetNaturalBaselineBOffset(self.mWM, baselineGroup, BaselineExportContext::Other) {
            // Offset for last baseline from `GetNaturalBaselineBOffset` originates
            // from the frame's block end, so convert it back.
            let value = if baselineGroup == BaselineSharingGroup::First { baseline } else { self.mFrame.BSize(self.mWM) - baseline };
            ascent.set(value);
            return value;
        }

        // We couldn't determine a baseline, so we synthesize one from border box:
        let value = Baseline::SynthesizeBOffsetFromBorderBox(self.mFrame, self.mCBWM, BaselineSharingGroup::First);
        ascent.set(value);
        value
    }

    // Convenience methods to compute the main & cross size of our *margin-box*.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::OuterMainSize
    pub fn OuterMainSize(&self) -> nscoord {
        self.mMainSize + self.MarginBorderPaddingSizeInMainAxis()
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::OuterCrossSize
    pub fn OuterCrossSize(&self) -> nscoord {
        self.mCrossSize + self.MarginBorderPaddingSizeInCrossAxis()
    }

    // Convenience method to return the content-box block-size.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::BSize
    pub fn BSize(&self) -> nscoord {
        if self.IsBlockAxisMainAxis() { self.MainSize() } else { self.CrossSize() }
    }

    // Convenience method to return the measured content-box block-size computed
    // in nsFlexContainerFrame::MeasureBSizeForFlexItem().
    // (FlexItem::MeasuredBSize is below, with the cache.)

    // Convenience methods to synthesize a style main size or a style cross size
    // with box-size considered, to provide the size overrides when constructing
    // ReflowInput for flex items.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::StyleMainSize
    pub fn StyleMainSize(&self) -> StyleSize {
        let mut mainSize = self.MainSize();
        if self.Frame().StylePosition().mBoxSizing() == StyleBoxSizing::BorderBox {
            mainSize += self.BorderPaddingSizeInMainAxis();
        }
        StyleSize::FromAppUnits(mainSize)
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::StyleCrossSize
    pub fn StyleCrossSize(&self) -> StyleSize {
        let mut crossSize = self.CrossSize();
        if self.Frame().StylePosition().mBoxSizing() == StyleBoxSizing::BorderBox {
            crossSize += self.BorderPaddingSizeInCrossAxis();
        }
        StyleSize::FromAppUnits(crossSize)
    }

    // Returns the distance between this FlexItem's baseline and the cross-start
    // edge of its margin-box. Used in baseline alignment.
    //
    // (This function needs to be told which physical start side we're measuring
    // the baseline from, so that it can look up the appropriate components from
    // margin.)
    // (FlexItem::BaselineOffsetFromOuterCrossEdge is below.)

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::ShareOfWeightSoFar
    pub fn ShareOfWeightSoFar(&self) -> f64 {
        self.mShareOfWeightSoFar
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsFrozen
    pub fn IsFrozen(&self) -> bool {
        self.mIsFrozen
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::HadMinViolation
    pub fn HadMinViolation(&self) -> bool {
        MOZ_ASSERT!(!self.mIsFrozen, "min violation has no meaning for frozen items.");
        self.mHadMinViolation
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::HadMaxViolation
    pub fn HadMaxViolation(&self) -> bool {
        MOZ_ASSERT!(!self.mIsFrozen, "max violation has no meaning for frozen items.");
        self.mHadMaxViolation
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::WasMinClamped
    pub fn WasMinClamped(&self) -> bool {
        MOZ_ASSERT!(self.mIsFrozen, "min clamping has no meaning for unfrozen items.");
        self.mHadMinViolation
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::WasMaxClamped
    pub fn WasMaxClamped(&self) -> bool {
        MOZ_ASSERT!(self.mIsFrozen, "max clamping has no meaning for unfrozen items.");
        self.mHadMaxViolation
    }

    // Indicates whether this item received a preliminary "measuring" reflow
    // before its actual reflow.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::HadMeasuringReflow
    pub fn HadMeasuringReflow(&self) -> bool {
        self.mHadMeasuringReflow
    }

    // Indicates whether this item's computed cross-size property is 'auto'.
    // (FlexItem::IsCrossSizeAuto is below.)

    // Indicates whether the cross-size property is set to something definite,
    // for the purpose of preferred aspect ratio calculations.
    // (FlexItem::IsCrossSizeDefinite is below.)

    // Indicates whether this item's cross-size has been stretched (from having
    // "align-self: stretch" with an auto cross-size and no auto margins in the
    // cross axis).
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsStretched
    pub fn IsStretched(&self) -> bool {
        self.mIsStretched
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsFlexBaseSizeContentBSize
    pub fn IsFlexBaseSizeContentBSize(&self) -> bool {
        self.mIsFlexBaseSizeContentBSize
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsMainMinSizeContentBSize
    pub fn IsMainMinSizeContentBSize(&self) -> bool {
        self.mIsMainMinSizeContentBSize
    }

    // Indicates whether we need to resolve an 'auto' value for the main-axis
    // min-[width|height] property.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::NeedsMinSizeAutoResolution
    pub fn NeedsMinSizeAutoResolution(&self) -> bool {
        self.mNeedsMinSizeAutoResolution
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::HasAnyAutoMargin
    pub fn HasAnyAutoMargin(&self) -> bool {
        self.mHasAnyAutoMargin
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::ItemBaselineSharingGroup
    pub fn ItemBaselineSharingGroup(&self) -> BaselineSharingGroup {
        MOZ_ASSERT!(self.mAlignSelf == StyleAlignFlags::BASELINE || self.mAlignSelf == StyleAlignFlags::LAST_BASELINE, "mBaselineSharingGroup only gets a meaningful value for baseline-aligned items");
        self.mBaselineSharingGroup
    }

    // Indicates whether this item is a "strut" left behind by an element with
    // visibility:collapse.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsStrut
    pub fn IsStrut(&self) -> bool {
        self.mIsStrut
    }

    // The main axis and cross axis are relative to mCBWM.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::MainAxis
    pub fn MainAxis(&self) -> LogicalAxis {
        self.mMainAxis
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::CrossAxis
    pub fn CrossAxis(&self) -> LogicalAxis {
        GetOrthogonalAxis(self.mMainAxis)
    }

    // IsInlineAxisMainAxis() returns true if this item's inline axis is parallel
    // (or antiparallel) to the container's main axis. Otherwise (i.e. if this
    // item's inline axis is orthogonal to the container's main axis), this
    // function returns false. The next 3 methods are all other ways of asking
    // the same question, and only exist for readability at callsites (depending
    // on which axes those callsites are reasoning about).
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsInlineAxisMainAxis
    pub fn IsInlineAxisMainAxis(&self) -> bool {
        self.mIsInlineAxisMainAxis
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsInlineAxisCrossAxis
    pub fn IsInlineAxisCrossAxis(&self) -> bool {
        !self.mIsInlineAxisMainAxis
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsBlockAxisMainAxis
    pub fn IsBlockAxisMainAxis(&self) -> bool {
        !self.mIsInlineAxisMainAxis
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsBlockAxisCrossAxis
    pub fn IsBlockAxisCrossAxis(&self) -> bool {
        self.mIsInlineAxisMainAxis
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::GetWritingMode
    pub fn GetWritingMode(&self) -> WritingMode {
        self.mWM
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::ContainingBlockWM
    pub fn ContainingBlockWM(&self) -> WritingMode {
        self.mCBWM
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::AlignSelf
    pub fn AlignSelf(&self) -> StyleAlignFlags {
        self.mAlignSelf
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::AlignSelfFlags
    pub fn AlignSelfFlags(&self) -> StyleAlignFlags {
        self.mAlignSelfFlags
    }

    // Returns the flex factor (flex-grow or flex-shrink), depending on
    // 'aIsUsingFlexGrow'.
    //
    // Asserts fatally if called on a frozen item (since frozen items are not
    // flexible).
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::GetFlexFactor
    pub fn GetFlexFactor(&self, aIsUsingFlexGrow: bool) -> f32 {
        MOZ_ASSERT!(!self.IsFrozen(), "shouldn't need flex factor after item is frozen");

        if aIsUsingFlexGrow { self.mFlexGrow } else { self.mFlexShrink }
    }

    // Returns the weight that we should use in the "resolving flexible lengths"
    // algorithm.  If we're using the flex grow factor, we just return that;
    // otherwise, we return the "scaled flex shrink factor" (scaled by our flex
    // base size, so that when both large and small items are shrinking, the large
    // items shrink more).
    //
    // I'm calling this a "weight" instead of a "[scaled] flex-[grow|shrink]
    // factor", to more clearly distinguish it from the actual flex-grow &
    // flex-shrink factors.
    //
    // Asserts fatally if called on a frozen item (since frozen items are not
    // flexible).
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::GetWeight
    pub fn GetWeight(&self, aIsUsingFlexGrow: bool) -> f32 {
        MOZ_ASSERT!(!self.IsFrozen(), "shouldn't need weight after item is frozen");

        if aIsUsingFlexGrow {
            return self.mFlexGrow;
        }

        // We're using flex-shrink --> return mFlexShrink * mFlexBaseSize
        if self.mFlexBaseSize == 0 {
            // Special-case for mFlexBaseSize == 0 -- we have no room to shrink, so
            // regardless of mFlexShrink, we should just return 0.
            // (This is really a special-case for when mFlexShrink is infinity, to
            // avoid performing mFlexShrink * mFlexBaseSize = inf * 0 = undefined.)
            return 0.0;
        }
        self.mFlexShrink * self.mFlexBaseSize as f32
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::TreatBSizeAsIndefinite
    pub fn TreatBSizeAsIndefinite(&self) -> bool {
        self.mTreatBSizeAsIndefinite
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::GetAspectRatio
    pub fn GetAspectRatio(&self) -> AspectRatio {
        self.mAspectRatio
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::HasAspectRatio
    pub fn HasAspectRatio(&self) -> bool {
        self.mAspectRatio.operator_bool()
    }

    // Getters for margin:
    // ===================
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::Margin
    pub fn Margin(&self) -> LogicalMargin {
        self.mMargin
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::PhysicalMargin
    pub fn PhysicalMargin(&self) -> nsMargin {
        self.mMargin.GetPhysicalMargin(self.mCBWM)
    }

    // Returns the margin component for a given LogicalSide in flex container's
    // writing-mode.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::GetMarginComponentForSide
    pub fn GetMarginComponentForSide(&self, aSide: LogicalSide) -> nscoord {
        self.mMargin.Side(aSide, self.mCBWM)
    }

    // Returns the total space occupied by this item's margins in the given axis
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::MarginSizeInMainAxis
    pub fn MarginSizeInMainAxis(&self) -> nscoord {
        self.mMargin.StartEnd(self.MainAxis(), self.mCBWM)
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::MarginSizeInCrossAxis
    pub fn MarginSizeInCrossAxis(&self) -> nscoord {
        self.mMargin.StartEnd(self.CrossAxis(), self.mCBWM)
    }

    // Getters for border/padding
    // ==========================
    // Returns the total space occupied by this item's borders and padding in
    // the given axis
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::BorderPadding
    pub fn BorderPadding(&self) -> LogicalMargin {
        self.mBorderPadding
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::BorderPaddingSizeInMainAxis
    pub fn BorderPaddingSizeInMainAxis(&self) -> nscoord {
        self.mBorderPadding.StartEnd(self.MainAxis(), self.mCBWM)
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::BorderPaddingSizeInCrossAxis
    pub fn BorderPaddingSizeInCrossAxis(&self) -> nscoord {
        self.mBorderPadding.StartEnd(self.CrossAxis(), self.mCBWM)
    }

    // Getter for combined margin/border/padding
    // =========================================
    // Returns the total space occupied by this item's margins, borders and
    // padding in the given axis
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::MarginBorderPaddingSizeInMainAxis
    pub fn MarginBorderPaddingSizeInMainAxis(&self) -> nscoord {
        self.MarginSizeInMainAxis() + self.BorderPaddingSizeInMainAxis()
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::MarginBorderPaddingSizeInCrossAxis
    pub fn MarginBorderPaddingSizeInCrossAxis(&self) -> nscoord {
        self.MarginSizeInCrossAxis() + self.BorderPaddingSizeInCrossAxis()
    }

    // Setters
    // =======
    // Helper to set the resolved value of min-[width|height]:auto for the main
    // axis. (Should only be used if NeedsMinSizeAutoResolution() returns true.)
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::UpdateMainMinSize
    pub fn UpdateMainMinSize(&mut self, aNewMinSize: nscoord) {
        NS_ASSERTION!(aNewMinSize >= 0, "How did we end up with a negative min-size?");
        MOZ_ASSERT!(self.mMainMaxSize == NS_UNCONSTRAINEDSIZE || self.mMainMaxSize >= aNewMinSize, "Should only use this function for resolving min-size:auto, and main max-size should be an upper-bound for resolved val");
        MOZ_ASSERT!(
            self.mNeedsMinSizeAutoResolution && (self.mMainMinSize == 0 || self.mFrame.IsThemed()),
            "Should only use this function for resolving min-size:auto, so we shouldn't already have a nonzero min-size established (unless it's a themed-widget-imposed minimum size)"
        );

        if aNewMinSize > self.mMainMinSize {
            self.mMainMinSize = aNewMinSize;
            // Also clamp main-size to be >= new min-size:
            self.mMainSize = std::cmp::max(self.mMainSize, aNewMinSize);
        }
        self.mNeedsMinSizeAutoResolution = false;
    }

    // This sets our flex base size, and then sets our main size to the
    // resulting "hypothetical main size" (the base size clamped to our
    // main-axis [min,max] sizing constraints).
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetFlexBaseSizeAndMainSize
    pub fn SetFlexBaseSizeAndMainSize(&mut self, aNewFlexBaseSize: nscoord) {
        MOZ_ASSERT!(!self.mIsFrozen || self.mFlexBaseSize == NS_UNCONSTRAINEDSIZE, "flex base size shouldn't change after we're frozen (unless we're just resolving an intrinsic size)");
        self.mFlexBaseSize = aNewFlexBaseSize;

        // Before we've resolved flexible lengths, we keep mMainSize set to
        // the 'hypothetical main size', which is the flex base size, clamped
        // to the [min,max] range:
        self.mMainSize = CSSMinMax(self.mFlexBaseSize, self.mMainMinSize, self.mMainMaxSize);

        FLEX_ITEM_LOG!(self.mFrame, "Set flex base size: %d, hypothetical main size: %d", self.mFlexBaseSize, self.mMainSize);
    }

    // Setters used while we're resolving flexible lengths
    // ---------------------------------------------------

    // Sets the main-size of our flex item's content-box.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetMainSize
    pub fn SetMainSize(&mut self, aNewMainSize: nscoord) {
        MOZ_ASSERT!(!self.mIsFrozen, "main size shouldn't change after we're frozen");
        self.mMainSize = aNewMainSize;
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetShareOfWeightSoFar
    pub fn SetShareOfWeightSoFar(&mut self, aNewShare: f64) {
        MOZ_ASSERT!(!self.mIsFrozen || aNewShare == 0.0, "shouldn't be giving this item any share of the weight after it's frozen");
        self.mShareOfWeightSoFar = aNewShare;
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::Freeze
    pub fn Freeze(&mut self) {
        self.mIsFrozen = true;
        // Now that we are frozen, the meaning of mHadMinViolation and
        // mHadMaxViolation changes to indicate min and max clamping. Clear
        // both of the member variables so that they are ready to be set
        // as clamping state later, if necessary.
        self.mHadMinViolation = false;
        self.mHadMaxViolation = false;
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetHadMinViolation
    pub fn SetHadMinViolation(&mut self) {
        MOZ_ASSERT!(!self.mIsFrozen, "shouldn't be changing main size & having violations after we're frozen");
        self.mHadMinViolation = true;
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetHadMaxViolation
    pub fn SetHadMaxViolation(&mut self) {
        MOZ_ASSERT!(!self.mIsFrozen, "shouldn't be changing main size & having violations after we're frozen");
        self.mHadMaxViolation = true;
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::ClearViolationFlags
    pub fn ClearViolationFlags(&mut self) {
        MOZ_ASSERT!(!self.mIsFrozen, "shouldn't be altering violation flags after we're frozen");
        self.mHadMinViolation = false;
        self.mHadMaxViolation = false;
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetWasMinClamped
    pub fn SetWasMinClamped(&mut self) {
        MOZ_ASSERT!(!self.mHadMinViolation && !self.mHadMaxViolation, "only clamp once");
        // This reuses the mHadMinViolation member variable to track clamping
        // events. This is allowable because mHadMinViolation only reflects
        // a violation up until the item is frozen.
        MOZ_ASSERT!(self.mIsFrozen, "shouldn't set clamping state when we are unfrozen");
        self.mHadMinViolation = true;
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetWasMaxClamped
    pub fn SetWasMaxClamped(&mut self) {
        MOZ_ASSERT!(!self.mHadMinViolation && !self.mHadMaxViolation, "only clamp once");
        // This reuses the mHadMaxViolation member variable to track clamping
        // events. This is allowable because mHadMaxViolation only reflects
        // a violation up until the item is frozen.
        MOZ_ASSERT!(self.mIsFrozen, "shouldn't set clamping state when we are unfrozen");
        self.mHadMaxViolation = true;
    }

    // Setters for values that are determined after we've resolved our main size
    // -------------------------------------------------------------------------

    // Sets the main-axis position of our flex item's content-box.
    // (This is the distance between the main-start edge of the flex container
    // and the main-start edge of the flex item's content-box.)
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetMainPosition
    pub fn SetMainPosition(&mut self, aPosn: nscoord) {
        MOZ_ASSERT!(self.mIsFrozen, "main size should be resolved before this");
        self.mMainPosn = aPosn;
    }

    // Sets the cross-size of our flex item's content-box.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetCrossSize
    pub fn SetCrossSize(&mut self, aCrossSize: nscoord) {
        MOZ_ASSERT!(!self.mIsStretched, "Cross size shouldn't be modified after it's been stretched");
        self.mCrossSize = aCrossSize;
    }

    // Sets the cross-axis position of our flex item's content-box.
    // (This is the distance between the cross-start edge of the flex container
    // and the cross-start edge of the flex item.)
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetCrossPosition
    pub fn SetCrossPosition(&mut self, aPosn: nscoord) {
        MOZ_ASSERT!(self.mIsFrozen, "main size should be resolved before this");
        self.mCrossPosn = aPosn;
    }

    // After a FlexItem has had a reflow, this method can be used to cache its
    // (possibly-unresolved) ascent, in case it's needed later for
    // baseline-alignment or to establish the container's baseline.
    // (NOTE: This can be marked 'const' even though it's modifying mAscent,
    // because mAscent is mutable. It's nice for this to be 'const', because it
    // means our final reflow can iterate over const FlexItem pointers, and we
    // can be sure it's not modifying those FlexItems, except via this method.)
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetAscent
    pub fn SetAscent(&self, aAscent: nscoord) {
        self.mAscent.set(aAscent); // NOTE: this may be ASK_FOR_BASELINE
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetHadMeasuringReflow
    pub fn SetHadMeasuringReflow(&mut self) {
        self.mHadMeasuringReflow = true;
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetIsFlexBaseSizeContentBSize
    pub fn SetIsFlexBaseSizeContentBSize(&mut self) {
        self.mIsFlexBaseSizeContentBSize = true;
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetIsMainMinSizeContentBSize
    pub fn SetIsMainMinSizeContentBSize(&mut self) {
        self.mIsMainMinSizeContentBSize = true;
    }

    // Setter for margin components (for resolving "auto" margins)
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::SetMarginComponentForSide
    pub fn SetMarginComponentForSide(&mut self, aSide: LogicalSide, aLength: nscoord) {
        MOZ_ASSERT!(self.mIsFrozen, "main size should be resolved before this");
        *self.mMargin.Side_mut(aSide, self.mCBWM) = aLength;
    }

    // (FlexItem::ResolveStretchedCrossSize and ResolveFlexBaseSizeFromAspectRatio are below.)

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::NumAutoMarginsInMainAxis
    pub fn NumAutoMarginsInMainAxis(&self) -> u32 {
        self.NumAutoMarginsInAxis(self.MainAxis())
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::NumAutoMarginsInCrossAxis
    pub fn NumAutoMarginsInCrossAxis(&self) -> u32 {
        self.NumAutoMarginsInAxis(self.CrossAxis())
    }
}

/**
 * Represents a single flex line in a flex container.
 * Manages an array of the FlexItems that are in the line.
 */
// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::FlexLine
pub struct FlexLine {
    // Sum of FlexItems' outer hypothetical main sizes and all main-axis
    // {row,columnm}-gaps between items.
    // (i.e. their flex base sizes, clamped via their min/max-size properties,
    // plus their main-axis margin/border/padding, plus the sum of the gaps.)
    //
    // This variable uses a 64-bit coord type to avoid integer overflow in case
    // several of the individual items have huge hypothetical main sizes, which
    // can happen with percent-width table-layout:fixed descendants. We have to
    // avoid integer overflow in order to shrink items properly in that scenario.
    // (AuCoord64 is i64 here.)
    mTotalOuterHypotheticalMainSize: i64,

    // Stores this line's flex items.
    mItems: Vec<FlexItem>,

    // Number of *frozen* FlexItems in this line, based on FlexItem::IsFrozen().
    // Mostly used for optimization purposes, e.g. to bail out early from loops
    // when we can tell they have nothing left to do.
    mNumFrozenItems: u32,
    // Number of items that are not collapsed.
    mNumNonCollapsedItems: u32,

    // Sum of margin/border/padding for the FlexItems in this FlexLine.
    mTotalItemMBP: nscoord,

    mLineCrossSize: nscoord,
    mFirstBaselineOffset: nscoord,
    mLastBaselineOffset: nscoord,

    // Maintain size of each {row,column}-gap in the main axis
    mMainGapSize: nscoord,
}

impl FlexLine {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::FlexLine
    pub fn new(aMainGapSize: nscoord) -> FlexLine {
        FlexLine {
            mTotalOuterHypotheticalMainSize: 0,
            mItems: Vec::new(),
            mNumFrozenItems: 0,
            mNumNonCollapsedItems: 0,
            mTotalItemMBP: 0,
            mLineCrossSize: 0,
            mFirstBaselineOffset: nscoord_MIN,
            mLastBaselineOffset: nscoord_MIN,
            mMainGapSize: aMainGapSize,
        }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::SumOfGaps
    pub fn SumOfGaps(&self) -> nscoord {
        if self.mNumNonCollapsedItems != 0 { (self.mNumNonCollapsedItems as nscoord - 1) * self.mMainGapSize } else { 0 }
    }

    // Returns the sum of our FlexItems' outer hypothetical main sizes plus the
    // sum of main axis {row,column}-gaps between items.
    // ("outer" = margin-box, and "hypothetical" = before flexing)
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::TotalOuterHypotheticalMainSize
    pub fn TotalOuterHypotheticalMainSize(&self) -> i64 {
        self.mTotalOuterHypotheticalMainSize
    }

    // Accessors for our FlexItems & information about them:
    //
    // Note: Callers must use IsEmpty() to ensure that the FlexLine is non-empty
    // before calling accessors that return FlexItem.
    // ADAPTER: C++ has `FirstItem()` const and non-const overloads; the non-const one is FirstItem_mut.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::FirstItem
    pub fn FirstItem(&self) -> &FlexItem {
        &self.mItems[0]
    }
    pub fn FirstItem_mut(&mut self) -> &mut FlexItem {
        &mut self.mItems[0]
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::LastItem
    pub fn LastItem(&self) -> &FlexItem {
        self.mItems.last().unwrap()
    }
    pub fn LastItem_mut(&mut self) -> &mut FlexItem {
        self.mItems.last_mut().unwrap()
    }

    // The "startmost"/"endmost" is from the perspective of the flex container's
    // writing-mode, not from the perspective of the flex-relative main axis.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::StartmostItem
    pub fn StartmostItem(&self, aAxisTracker: &FlexboxAxisTracker) -> &FlexItem {
        if aAxisTracker.IsMainAxisReversed() { self.LastItem() } else { self.FirstItem() }
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::EndmostItem
    pub fn EndmostItem(&self, aAxisTracker: &FlexboxAxisTracker) -> &FlexItem {
        if aAxisTracker.IsMainAxisReversed() { self.FirstItem() } else { self.LastItem() }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        self.mItems.is_empty()
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::NumItems
    pub fn NumItems(&self) -> u32 {
        self.mItems.len() as u32
    }

    // ADAPTER: C++ has `Items()` const and non-const overloads; the non-const one is Items_mut.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::Items
    pub fn Items(&self) -> &Vec<FlexItem> {
        &self.mItems
    }
    pub fn Items_mut(&mut self) -> &mut Vec<FlexItem> {
        &mut self.mItems
    }

    // Adds the last flex item's hypothetical outer main-size and
    // margin/border/padding to our totals. This should be called exactly once for
    // each flex item, after we've determined that this line is the correct home
    // for that item.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::AddLastItemToMainSizeTotals
    pub fn AddLastItemToMainSizeTotals(&mut self) {
        let lastItem = self.mItems.last().unwrap();

        // Update our various bookkeeping member-vars:
        if lastItem.IsFrozen() {
            self.mNumFrozenItems += 1;
        }

        // If the item added was not the first non-collapsed item in the line,
        // we add in any gap space as needed.
        if !lastItem.IsStrut() {
            if self.mNumNonCollapsedItems != 0 {
                self.mTotalOuterHypotheticalMainSize += self.mMainGapSize as i64;
            }
            self.mNumNonCollapsedItems += 1;
        }

        self.mTotalItemMBP += lastItem.MarginBorderPaddingSizeInMainAxis();
        self.mTotalOuterHypotheticalMainSize += lastItem.OuterMainSize() as i64;
    }

    // Computes the cross-size and baseline position of this FlexLine, based on
    // its FlexItems.
    // (FlexLine::ComputeCrossSizeAndBaseline is below.)

    // Returns the cross-size of this line.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::LineCrossSize
    pub fn LineCrossSize(&self) -> nscoord {
        self.mLineCrossSize
    }

    // Setter for line cross-size -- needed for cases where the flex container
    // imposes a cross-size on the line. (e.g. for single-line flexbox, or for
    // multi-line flexbox with 'align-content: stretch')
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::SetLineCrossSize
    pub fn SetLineCrossSize(&mut self, aLineCrossSize: nscoord) {
        self.mLineCrossSize = aLineCrossSize;
    }

    /**
     * Returns the offset within this line where any baseline-aligned FlexItems
     * should place their baseline. The return value represents a distance from
     * the line's cross-start edge.
     *
     * If there are no baseline-aligned FlexItems, returns nscoord_MIN.
     */
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::FirstBaselineOffset
    pub fn FirstBaselineOffset(&self) -> nscoord {
        self.mFirstBaselineOffset
    }

    /**
     * Returns the offset within this line where any last baseline-aligned
     * FlexItems should place their baseline. Opposite the case of the first
     * baseline offset, this represents a distance from the line's cross-end
     * edge (since last baseline-aligned items are flush to the cross-end edge).
     *
     * If there are no last baseline-aligned FlexItems, returns nscoord_MIN.
     */
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::LastBaselineOffset
    pub fn LastBaselineOffset(&self) -> nscoord {
        self.mLastBaselineOffset
    }

    // (FlexLine::ExtractBaselineOffset is below.)

    /**
     * Returns the gap size in the main axis for this line. Used for gap
     * calculations.
     */
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::MainGapSize
    pub fn MainGapSize(&self) -> nscoord {
        self.mMainGapSize
    }

    // (FlexLine::ResolveFlexibleLengths, PositionItemsInMainAxis, PositionItemsInCrossAxis,
    // FreezeItemsEarly and FreezeOrRestoreEachFlexibleSize are below.)
}

// The "startmost"/"endmost" is from the perspective of the flex container's
// writing-mode, not from the perspective of the flex-relative cross axis.
// Gecko: layout/generic/nsFlexContainerFrame.cpp StartmostLine
fn StartmostLine<'a>(aLines: &'a [FlexLine], aAxisTracker: &FlexboxAxisTracker) -> &'a FlexLine {
    if aAxisTracker.IsCrossAxisReversed() { aLines.last().unwrap() } else { &aLines[0] }
}
// Gecko: layout/generic/nsFlexContainerFrame.cpp EndmostLine
fn EndmostLine<'a>(aLines: &'a [FlexLine], aAxisTracker: &FlexboxAxisTracker) -> &'a FlexLine {
    if aAxisTracker.IsCrossAxisReversed() { &aLines[0] } else { aLines.last().unwrap() }
}

// Information about a strut left behind by a FlexItem that's been collapsed
// using "visibility:collapse".
// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::StrutInfo
pub struct StrutInfo {
    pub mItemIdx: u32,        // Index in the child list.
    pub mStrutCrossSize: nscoord, // The cross-size of this strut.
}

impl StrutInfo {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp StrutInfo::StrutInfo
    pub fn new(aItemIdx: u32, aStrutCrossSize: nscoord) -> StrutInfo {
        StrutInfo { mItemIdx: aItemIdx, mStrutCrossSize: aStrutCrossSize }
    }
}

// Flex data shared by the flex container frames in a continuation chain, owned
// by the first-in-flow. The data is initialized at the end of the
// first-in-flow's Reflow().
// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::SharedFlexData
// (the property is SharedFlexDataProperty, declared at the top of the file)
pub struct SharedFlexData {
    // The flex lines generated in DoFlexLayout() by our first-in-flow.
    pub mLines: Vec<FlexLine>,

    // The final content main/cross size computed by DoFlexLayout.
    pub mContentBoxMainSize: nscoord,
    pub mContentBoxCrossSize: nscoord,
}

impl SharedFlexData {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp SharedFlexData::SharedFlexData (the default member initializers)
    pub fn new() -> SharedFlexData {
        SharedFlexData { mLines: Vec::new(), mContentBoxMainSize: NS_UNCONSTRAINEDSIZE, mContentBoxCrossSize: NS_UNCONSTRAINEDSIZE }
    }

    // Update this struct. Called by the first-in-flow.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp SharedFlexData::Update
    pub fn Update(&mut self, aFlr: FlexLayoutResult) {
        self.mLines = aFlr.mLines;
        self.mContentBoxMainSize = aFlr.mContentBoxMainSize;
        self.mContentBoxCrossSize = aFlr.mContentBoxCrossSize;
    }

    // ADAPTER: `GetProperty(SharedFlexData::Prop())` (the struct is boxed behind a usize property).
    pub fn Get(aFrame: nsIFrame) -> Option<&'static mut SharedFlexData> {
        aFrame.GetProperty(SharedFlexDataProperty()).map(|p| unsafe { &mut *(p as *mut SharedFlexData) })
    }
    // ADAPTER: `SetProperty(SharedFlexData::Prop(), new SharedFlexData)`
    pub fn Set(aFrame: nsIFrame, aData: SharedFlexData) {
        let p = Box::into_raw(Box::new(aData)) as usize;
        aFrame.SetOwnedPointerProperty::<SharedFlexData>(SharedFlexDataProperty(), p);
    }
}

// Flex data stored in every flex container's in-flow fragment (continuation).
//
// It's intended to prevent quadratic operations resulting from each fragment
// having to walk its full prev-in-flow chain, and also serves as an argument to
// the flex container next-in-flow's ReflowChildren(), to compute the position
// offset for each flex item.
// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::PerFragmentFlexData
pub struct PerFragmentFlexData {
    // Suppose D is the distance from a flex container fragment's content-box
    // block-start edge to whichever is larger of either (a) the block-end edge of
    // its children, or (b) the available space's block-end edge. (Note: in case
    // (b), D is conceptually the sum of the block-size of the children, the
    // packing space before & in between them, and part of the packing space after
    // them.)
    //
    // This variable stores the sum of the D values for the current flex container
    // fragments and for all its previous fragments
    pub mCumulativeContentBoxBSize: nscoord,

    // This variable accumulates FirstLineOrFirstItemBAxisMetrics::mBEndEdgeShift,
    // for the current flex container fragment and for all its previous fragments.
    // See the comment of mBEndEdgeShift for its computation details. In short,
    // this value is the net block-end edge shift, accumulated for the children in
    // all the previous fragments. This number is non-negative.
    //
    // This value is also used to grow a flex container's block-size if the
    // container's computed block-size is unconstrained. For example: a tall item
    // may be pushed to the next page/column, which leaves some wasted area at the
    // bottom of the current flex container fragment, and causes the flex
    // container fragments to be (collectively) larger than the hypothetical
    // unfragmented size. Another example: a tall flex item may be broken into
    // multiple fragments, and those fragments may have a larger collective
    // block-size as compared to the item's original unfragmented size; the
    // container would need to increase its block-size to account for this.
    pub mCumulativeBEndEdgeShift: nscoord,
}

impl PerFragmentFlexData {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp PerFragmentFlexData::PerFragmentFlexData (the default member initializers)
    pub fn new() -> PerFragmentFlexData {
        PerFragmentFlexData { mCumulativeContentBoxBSize: 0, mCumulativeBEndEdgeShift: 0 }
    }

    // ADAPTER: `GetProperty(PerFragmentFlexData::Prop())` (boxed behind a usize property).
    pub fn Get(aFrame: nsIFrame) -> Option<&'static mut PerFragmentFlexData> {
        aFrame.GetProperty(PerFragmentFlexDataProperty()).map(|p| unsafe { &mut *(p as *mut PerFragmentFlexData) })
    }
    // ADAPTER: `SetProperty(PerFragmentFlexData::Prop(), new PerFragmentFlexData(...))`
    pub fn Set(aFrame: nsIFrame, aData: PerFragmentFlexData) {
        let p = Box::into_raw(Box::new(aData)) as usize;
        aFrame.SetOwnedPointerProperty::<PerFragmentFlexData>(PerFragmentFlexDataProperty(), p);
    }
}

// Gecko: layout/generic/nsFlexContainerFrame.h nsFlexContainerFrame::FlexLayoutResult
pub struct FlexLayoutResult {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLayoutResult::mLines
    pub mLines: Vec<FlexLine>,

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLayoutResult::mPlaceholders
    pub mPlaceholders: Vec<nsIFrame>,

    // True if this flex container has at least one item with visibility:collapse.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLayoutResult::mHasCollapsedItems
    pub mHasCollapsedItems: bool,

    // The final content main size.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLayoutResult::mContentBoxMainSize
    pub mContentBoxMainSize: nscoord,

    // The final content cross size.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLayoutResult::mContentBoxCrossSize
    pub mContentBoxCrossSize: nscoord,

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLayoutResult::mAscent
    pub mAscent: nscoord,
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLayoutResult::mAscentForLast
    pub mAscentForLast: nscoord,
}

impl Default for FlexLayoutResult {
    fn default() -> FlexLayoutResult {
        FlexLayoutResult {
            mLines: Vec::new(),
            mPlaceholders: Vec::new(),
            mHasCollapsedItems: false,
            mContentBoxMainSize: NS_UNCONSTRAINEDSIZE,
            mContentBoxCrossSize: NS_UNCONSTRAINEDSIZE,
            mAscent: NS_UNCONSTRAINEDSIZE,
            mAscentForLast: NS_UNCONSTRAINEDSIZE,
        }
    }
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp BuildStrutInfoFromCollapsedItems
fn BuildStrutInfoFromCollapsedItems(aLines: &Vec<FlexLine>, aStruts: &mut Vec<StrutInfo>) {
    MOZ_ASSERT!(aStruts.is_empty(), "We should only build up StrutInfo once per reflow, so aStruts should be empty when this is called");

    let mut itemIdxInContainer: u32 = 0;
    for line in aLines {
        for item in line.Items() {
            if item.Frame().StyleVisibility().IsCollapse() {
                // Note the cross size of the line as the item's strut size.
                aStruts.push(StrutInfo::new(itemIdxInContainer, line.LineCrossSize()));
            }
            itemIdxInContainer += 1;
        }
    }
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp SimplifyAlignOrJustifyContentForOneItem
fn SimplifyAlignOrJustifyContentForOneItem(aAlignmentVal: StyleContentDistribution, aIsAlign: bool) -> StyleAlignFlags {
    // Mask away any explicit fallback, to get the main (non-fallback) part of
    // the specified value:
    let mut specified = aAlignmentVal.primary();

    // XXX strip off <overflow-position> bits until we implement it (bug 1311892)
    specified &= !StyleAlignFlags::FLAG_BITS;

    // FIRST: handle a special-case for "justify-content:stretch" (or equivalent),
    // which requires that we ignore any author-provided explicit fallback value.
    if specified == StyleAlignFlags::NORMAL {
        // In a flex container, *-content: "'normal' behaves as 'stretch'".
        // Do that conversion early, so it benefits from our 'stretch' special-case.
        // https://drafts.csswg.org/css-align-3/#distribution-flex
        specified = StyleAlignFlags::STRETCH;
    }
    if !aIsAlign && specified == StyleAlignFlags::STRETCH {
        // In a flex container, in "justify-content Axis: [...] 'stretch' behaves
        // as 'flex-start' (ignoring the specified fallback alignment, if any)."
        // https://drafts.csswg.org/css-align-3/#distribution-flex
        // So, we just directly return 'flex-start', & ignore explicit fallback..
        return StyleAlignFlags::FLEX_START;
    }

    // TODO: Check for an explicit fallback value (and if it's present, use it)
    // here once we parse it, see https://github.com/w3c/csswg-drafts/issues/1002.

    // If there's no explicit fallback, use the implied fallback values for
    // space-{between,around,evenly} (since those values only make sense with
    // multiple alignment subjects), and otherwise just use the specified value:
    if specified == StyleAlignFlags::SPACE_BETWEEN {
        return StyleAlignFlags::FLEX_START;
    }
    if specified == StyleAlignFlags::SPACE_AROUND || specified == StyleAlignFlags::SPACE_EVENLY {
        return StyleAlignFlags::CENTER;
    }
    specified
}

impl nsFlexContainerFrame {
    // static_cast<nsFlexContainerFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsFlexContainerFrame {
        nsFlexContainerFrame(aFrame)
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp NS_QUERYFRAME_HEAD(nsFlexContainerFrame)
    // do_QueryFrame<nsFlexContainerFrame>(frame)
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsFlexContainerFrame> {
        if aFrame.Type() == LayoutFrameType::FlexContainer {
            Some(nsFlexContainerFrame(aFrame))
        } else {
            None
        }
    }

    // ADAPTER: this frame's data members (see nsFlexContainerFrame_Members). Created on first use.
    fn Members(&self) -> &'static mut nsFlexContainerFrame_Members {
        let p = match self.0.GetProperty(FlexMembersProperty()) {
            Some(p) => p,
            None => {
                let p = Box::into_raw(Box::new(nsFlexContainerFrame_Members {
                    mCachedIntrinsicSizes: IntrinsicISizesCache::default(),
                    mFirstBaseline: NS_INTRINSIC_ISIZE_UNKNOWN,
                    mLastBaseline: NS_INTRINSIC_ISIZE_UNKNOWN,
                })) as usize;
                self.0.SetOwnedPointerProperty::<nsFlexContainerFrame_Members>(FlexMembersProperty(), p);
                p
            }
        };
        unsafe { &mut *(p as *mut nsFlexContainerFrame_Members) }
    }

    // ADAPTER: nsFlexContainerFrame::mCachedIntrinsicSizes
    pub fn mCachedIntrinsicSizes(&self) -> &'static mut IntrinsicISizesCache {
        &mut self.Members().mCachedIntrinsicSizes
    }

    // ADAPTER: nsFlexContainerFrame::mFirstBaseline, mLastBaseline
    pub fn mFirstBaseline(&self) -> nscoord {
        self.Members().mFirstBaseline
    }
    pub fn SetFirstBaseline(&self, aBaseline: nscoord) {
        self.Members().mFirstBaseline = aBaseline;
    }
    pub fn mLastBaseline(&self) -> nscoord {
        self.Members().mLastBaseline
    }
    pub fn SetLastBaseline(&self, aBaseline: nscoord) {
        self.Members().mLastBaseline = aBaseline;
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::DrainSelfOverflowList
    pub fn DrainSelfOverflowList(&self) -> bool {
        self.DrainAndMergeSelfOverflowList()
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::AppendFrames
    pub fn AppendFrames(&self, aListID: FrameChildListID, aFrameList: nsFrameList) {
        self.NoteNewChildren(aListID, &aFrameList);
        self.nsContainerFrame_AppendFrames(aListID, aFrameList);
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::InsertFrames
    pub fn InsertFrames(&self, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aPrevFrameLine: Option<&nsLineList_iterator>, aFrameList: nsFrameList) {
        self.NoteNewChildren(aListID, &aFrameList);
        self.nsContainerFrame_InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList);
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::RemoveFrame
    pub fn RemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        MOZ_ASSERT!(aListID == FrameChildListID::Principal, "unexpected child list");

        #[cfg(debug_assertions)]
        self.SetDidPushItemsBitIfNeeded(aListID, aOldFrame);

        self.nsContainerFrame_RemoveFrame(aContext, aListID, aOldFrame);
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::CSSAlignmentForAbsPosChild
    // (absolutely positioned children have no frame in this runtime, so nothing calls it)
    pub fn CSSAlignmentForAbsPosChild(&self, aChildRI: &ReflowInput, aLogicalAxis: LogicalAxis) -> StyleAlignFlags {
        let axisTracker = FlexboxAxisTracker::new(*self);

        // If we're row-oriented and the caller is asking about our inline axis (or
        // alternately, if we're column-oriented and the caller is asking about our
        // block axis), then the caller is really asking about our *main* axis.
        // Otherwise, the caller is asking about our cross axis.
        let isMainAxis = axisTracker.IsRowOriented() == (aLogicalAxis == LogicalAxis::Inline);
        let containerStylePos = self.StylePosition();
        let isAxisReversed = if isMainAxis { axisTracker.IsMainAxisReversed() } else { axisTracker.IsCrossAxisReversed() };

        let mut alignment = StyleAlignFlags::empty();
        let mut alignmentFlags = StyleAlignFlags::empty();
        if isMainAxis {
            // We're aligning in the main axis: align according to 'justify-content'.
            // (We don't care about justify-self; it has no effect on children of flex
            // containers, unless https://github.com/w3c/csswg-drafts/issues/7644
            // changes that.)
            alignment = SimplifyAlignOrJustifyContentForOneItem(containerStylePos.mJustifyContent(), /*aIsAlign = */ false);
        } else {
            // We're aligning in the cross axis: align according to 'align-self'.
            // (We don't care about align-content; it has no effect on abspos flex
            // children, per https://github.com/w3c/csswg-drafts/issues/7596 )
            alignment = aChildRI.mStylePosition.UsedAlignSelf(Some(self.Style())).0;
            // Extract and strip align flag bits
            alignmentFlags = alignment & StyleAlignFlags::FLAG_BITS;
            alignment &= !StyleAlignFlags::FLAG_BITS;

            if alignment == StyleAlignFlags::NORMAL {
                // "the 'normal' keyword behaves as 'start' on replaced
                // absolutely-positioned boxes, and behaves as 'stretch' on all other
                // absolutely-positioned boxes."
                // https://drafts.csswg.org/css-align/#align-abspos
                alignment = if aChildRI.mFrame.IsReplaced() { StyleAlignFlags::START } else { StyleAlignFlags::STRETCH };
            }
        }

        if alignment == StyleAlignFlags::STRETCH {
            // The default fallback alignment for 'stretch' is 'flex-start'.
            alignment = StyleAlignFlags::FLEX_START;
        }

        // Resolve flex-start, flex-end, auto, left, right, baseline, last baseline;
        if alignment == StyleAlignFlags::FLEX_START {
            alignment = if isAxisReversed { StyleAlignFlags::END } else { StyleAlignFlags::START };
        } else if alignment == StyleAlignFlags::FLEX_END {
            alignment = if isAxisReversed { StyleAlignFlags::START } else { StyleAlignFlags::END };
        } else if alignment == StyleAlignFlags::LEFT || alignment == StyleAlignFlags::RIGHT {
            MOZ_ASSERT!(isMainAxis, "Only justify-* can have 'left' and 'right'!");
            alignment = axisTracker.ResolveJustifyLeftRight(alignment);
        } else if alignment == StyleAlignFlags::BASELINE {
            alignment = StyleAlignFlags::START;
        } else if alignment == StyleAlignFlags::LAST_BASELINE {
            alignment = StyleAlignFlags::END;
        }

        MOZ_ASSERT!(alignment != StyleAlignFlags::STRETCH, "We should've converted 'stretch' to the fallback alignment!");
        MOZ_ASSERT!(alignment != StyleAlignFlags::FLEX_START && alignment != StyleAlignFlags::FLEX_END, "AbsoluteContainingBlock doesn't know how to handle flex-relative axis for flex containers!");

        alignment | alignmentFlags
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::UsedAlignSelfAndFlagsForItem
    pub fn UsedAlignSelfAndFlagsForItem(&self, aFlexItem: nsIFrame) -> (StyleAlignFlags, StyleAlignFlags) {
        MOZ_ASSERT!(aFlexItem.IsFlexItem());

        if self.IsLegacyWebkitBox() {
            // For -webkit-{inline-}box, we need to:
            // (1) Use prefixed "box-align" instead of "align-items" to determine the
            //     container's cross-axis alignment behavior.
            // (2) Suppress the ability for flex items to override that with their own
            //     cross-axis alignment. (The legacy box model doesn't support this.)
            // So, each FlexItem simply copies the container's converted "align-items"
            // value and disregards their own "align-self" property.
            // NOT PORTED: legacy -webkit-box (ConvertLegacyStyleToAlignItems(StyleXUL()))
            let alignSelf = ConvertLegacyStyleToAlignItems();
            let flags = StyleAlignFlags::empty();
            return (alignSelf, flags);
        }

        // Note: we don't need to call nsLayoutUtils::GetStyleFrame(aFlexItem) because
        // the table wrapper frame inherits 'align-self' property from the table
        // frame.
        let mut usedAlignSelf = aFlexItem.StylePosition().UsedAlignSelf(Some(self.Style()));
        if usedAlignSelf.0 == StyleAlignFlags::NORMAL {
            // For flex items, 'align-self:normal' behaves as 'align-self:stretch'.
            // https://drafts.csswg.org/css-align-3/#align-flex
            usedAlignSelf = mrt_style::style::values::specified::align::SelfAlignment(StyleAlignFlags::STRETCH);
        }

        // Store the <overflow-position> bits in flags, and strip the bits from the
        // used align-self value.
        let flags = usedAlignSelf.0 & StyleAlignFlags::FLAG_BITS;
        let alignSelf = usedAlignSelf.0 & !StyleAlignFlags::FLAG_BITS;
        (alignSelf, flags)
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::GenerateFlexItemForChild
    pub fn GenerateFlexItemForChild(&self, aLine: &mut FlexLine, aChildFrame: nsIFrame, aParentReflowInput: &ReflowInput, aAxisTracker: &FlexboxAxisTracker, aTentativeContentBoxCrossSize: nscoord) {
        let flexWM = aAxisTracker.GetWritingMode();
        let childWM = aChildFrame.GetWritingMode();

        // Note: we use GetStyleFrame() to access the sizing & flex properties here.
        // This lets us correctly handle table wrapper frames as flex items since
        // their inline-size and block-size properties are always 'auto'. In order for
        // 'flex-basis:auto' to actually resolve to the author's specified inline-size
        // or block-size, we need to dig through to the inner table.
        let styleFrame = nsLayoutUtils::GetStyleFrame(aChildFrame);
        let stylePos = styleFrame.StylePosition();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(styleFrame, std::ptr::null_mut());

        // Construct a StyleSizeOverrides for this flex item so that its ReflowInput
        // below will use and resolve its flex base size rather than its corresponding
        // preferred main size property (only for modern CSS flexbox).
        let mut sizeOverrides = StyleSizeOverrides::default();
        if !self.IsLegacyWebkitBox() {
            let styleFlexBaseSize: StyleSize;

            // When resolving flex base size, flex items use their 'flex-basis' property
            // in place of their preferred main size (e.g. 'width') for sizing purposes,
            // *unless* they have 'flex-basis:auto' in which case they use their
            // preferred main size after all.
            let flexBasis = stylePos.mFlexBasis();
            let styleMainSize = stylePos.Size(aAxisTracker.MainAxis(), flexWM, &anchorResolutionParams);
            if nsFlexContainerFrame::IsUsedFlexBasisContent(&flexBasis, &styleMainSize) {
                // If we get here, we're resolving the flex base size for a flex item, and
                // we fall into the flexbox spec section 9.2 step 3, substep C (if we have
                // a definite cross size) or E (if not).
                styleFlexBaseSize = StyleSize::MaxContent;
            } else if let (StyleFlexBasis::Size(basisSize), false) = (&flexBasis, flexBasis.IsAuto()) {
                // For all other non-'auto' flex-basis values, we just swap in the
                // flex-basis itself for the preferred main-size property.
                styleFlexBaseSize = basisSize.clone();
            } else {
                // else: flex-basis is 'auto', which is deferring to some explicit value
                // in the preferred main size.
                MOZ_ASSERT!(flexBasis.IsAuto());
                styleFlexBaseSize = styleMainSize.clone();
            }

            // Provide the size override for the preferred main size property.
            if aAxisTracker.IsInlineAxisMainAxis(childWM) {
                sizeOverrides.mStyleISize = Some(styleFlexBaseSize);
            } else {
                sizeOverrides.mStyleBSize = Some(styleFlexBaseSize);
            }

            // 'flex-basis' should works on the inner table frame for a table flex item,
            // just like how 'height' works on a table element.
            sizeOverrides.mApplyOverridesVerbatim = true;
        }

        // Create temporary reflow input just for sizing -- to get hypothetical
        // main-size and the computed values of min / max main-size property.
        // (This reflow input will _not_ be used for reflow.)
        let presContext = self.PresContext();
        let mut childRI = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            &presContext,
            aParentReflowInput,
            aChildFrame,
            aParentReflowInput.ComputedSize__WritingMode(childWM),
            None,
            EnumSet::default(),
            sizeOverrides,
            FlexShrinkWrapFlags(),
            std::ptr::null_mut(),
        );

        // FLEX GROW & SHRINK WEIGHTS
        // --------------------------
        let flexGrow: f32;
        let flexShrink: f32;
        if self.IsLegacyWebkitBox() {
            // NOT PORTED: legacy -webkit-box (aChildFrame->StyleXUL()->mBoxFlex)
            unimplemented!("NOT PORTED: legacy -webkit-box (box-flex)");
        } else {
            flexGrow = stylePos.mFlexGrow();
            flexShrink = stylePos.mFlexShrink();
        }

        // MAIN SIZES (flex base size, min/max size)
        // -----------------------------------------
        let computedSizeInFlexWM = childRI.ComputedSize__WritingMode(flexWM);
        let computedMinSizeInFlexWM = childRI.ComputedMinSize__WritingMode(flexWM);
        let computedMaxSizeInFlexWM = childRI.ComputedMaxSize__WritingMode(flexWM);

        let flexBaseSize = aAxisTracker.MainComponent__LogicalSize(computedSizeInFlexWM);
        let mainMinSize = aAxisTracker.MainComponent__LogicalSize(computedMinSizeInFlexWM);
        let mainMaxSize = aAxisTracker.MainComponent__LogicalSize(computedMaxSizeInFlexWM);

        // This is enforced by the ReflowInput where these values come from:
        MOZ_ASSERT!(mainMinSize <= mainMaxSize, "min size is larger than max size");

        // CROSS SIZES (tentative cross size, min/max cross size)
        // ------------------------------------------------------
        // Grab the cross size from the reflow input. This might be the right value,
        // or we might resolve it to something else in SizeItemInCrossAxis(); hence,
        // it's tentative. See comment under "Cross Size Determination" for more.
        let tentativeCrossSize = aAxisTracker.CrossComponent__LogicalSize(computedSizeInFlexWM);
        let crossMinSize = aAxisTracker.CrossComponent__LogicalSize(computedMinSizeInFlexWM);
        let crossMaxSize = aAxisTracker.CrossComponent__LogicalSize(computedMaxSizeInFlexWM);

        // Construct the flex item!
        aLine.Items_mut().push(FlexItem::new(&childRI, flexGrow, flexShrink, flexBaseSize, mainMinSize, mainMaxSize, tentativeCrossSize, crossMinSize, crossMaxSize, aAxisTracker));
        let item = aLine.Items_mut().last_mut().unwrap();

        // We may be about to do computations based on our item's cross-size
        // (e.g. using it as a constraint when measuring our content in the
        // main axis, or using it with the preferred aspect ratio to obtain a main
        // size). BEFORE WE DO THAT, we need let the item "pre-stretch" its cross size
        // (if it's got 'align-self:stretch'), for a certain case where the spec says
        // the stretched cross size is considered "definite". That case is if we
        // have a single-line (nowrap) flex container which itself has a definite
        // cross-size.  Otherwise, we'll wait to do stretching, since (in other
        // cases) we don't know how much the item should stretch yet.
        if IsSingleLine(aParentReflowInput.mFrame, &aParentReflowInput.mStylePosition) {
            // Is container's cross size "definite"?
            // - If it's column-oriented, then "yes", because its cross size is its
            // inline-size which is always definite from its descendants' perspective.
            // - Otherwise (if it's row-oriented), then we check the actual size
            // and call it definite if it's not NS_UNCONSTRAINEDSIZE.
            if aAxisTracker.IsColumnOriented() || aTentativeContentBoxCrossSize != NS_UNCONSTRAINEDSIZE {
                // Container's cross size is "definite", so we can resolve the item's
                // stretched cross size using that.
                item.ResolveStretchedCrossSize(aTentativeContentBoxCrossSize);
            }
        }

        // Before thinking about freezing the item at its base size, we need to give
        // it a chance to recalculate the base size from its cross size and aspect
        // ratio (since its cross size might've *just* now become definite due to
        // 'stretch' above)
        item.ResolveFlexBaseSizeFromAspectRatio(&childRI);

        // If we're inflexible, we can just freeze to our hypothetical main-size
        // up-front.
        if flexGrow == 0.0 && flexShrink == 0.0 {
            item.Freeze();
            if flexBaseSize < mainMinSize {
                item.SetWasMinClamped();
            } else if flexBaseSize > mainMaxSize {
                item.SetWasMaxClamped();
            }
        }

        // Resolve "flex-basis:auto" and/or "min-[width|height]:auto" (which might
        // require us to reflow the item to measure content height)
        self.ResolveAutoFlexBasisAndMinSize(item, &childRI, aAxisTracker);
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::PartiallyResolveAutoMinSize
    pub fn PartiallyResolveAutoMinSize(&self, aFlexItem: &FlexItem, aItemReflowInput: &ReflowInput, aAxisTracker: &FlexboxAxisTracker) -> nscoord {
        MOZ_ASSERT!(aFlexItem.NeedsMinSizeAutoResolution(), "only call for FlexItems that need min-size auto resolution");

        let itemWM = aFlexItem.GetWritingMode();
        let cbWM = aAxisTracker.GetWritingMode();
        let anchorResolutionParams = AnchorPosResolutionParams::From__SizeComputationInput_bool(aItemReflowInput, false);
        let mainStyleSize = aItemReflowInput.mStylePosition.Size(aAxisTracker.MainAxis(), cbWM, &anchorResolutionParams);
        let maxMainStyleSize = aItemReflowInput.mStylePosition.MaxSize(aAxisTracker.MainAxis(), cbWM, &anchorResolutionParams);
        let boxSizingAdjust = if aItemReflowInput.mStylePosition.mBoxSizing() == StyleBoxSizing::BorderBox { aFlexItem.BorderPadding().Size(cbWM) } else { LogicalSize::new__WritingMode(cbWM) };

        // Return the percentage basis in cbWM for computing the specified size
        // suggestion.
        let PercentageBasisForItem = || -> LogicalSize {
            // If this flex item is a compressible replaced element, according to the
            // list in CSS Sizing 3 §5.2.2, then CSS Sizing 3 §5.2.1c requires us to
            // resolve the percentage part of the preferred main size property against
            // zero, yielding a definite specified size suggestion. Here we can use a
            // zero percentage basis to fulfill this requirement.
            if aFlexItem.Frame().IsPercentageResolvedAgainstZero__StyleSize_StyleMaxSize(&mainStyleSize, &maxMainStyleSize) {
                return LogicalSize::new(cbWM, 0, 0);
            }
            aItemReflowInput.mContainingBlockSize.ConvertTo(cbWM, itemWM)
        };

        // Compute the specified size suggestion, which is the main-size property if
        // it's definite.
        let mut specifiedSizeSuggestion: nscoord = nscoord_MAX;

        if aAxisTracker.IsRowOriented() {
            // TODO(dholbert): We need to handle 'stretch' (and its prefixed aliases)
            // here; that's tracked in bug 1936942.  (Note that we do handle 'stretch'
            // in our column-oriented "else" clause below, via the call to
            // ComputeBSizeValueHandlingStretch.)
            if mainStyleSize.IsLengthPercentage() {
                // NOTE: We ignore extremum inline-size. This is OK because the caller is
                // responsible for computing the min-content inline-size and min()'ing it
                // with the value we return.
                specifiedSizeSuggestion = aFlexItem.Frame().ComputeISizeValue__WritingMode_LogicalSize_LogicalSize_LengthPercentage(cbWM, PercentageBasisForItem(), boxSizingAdjust, mainStyleSize.AsLengthPercentage());
            }
        } else {
            // NOTE: We ignore specified block-sizes that behave as 'auto', as
            // identified by IsAutoBSize(); that's OK because the caller is responsible
            // for computing the content-based block-size and and min()'ing it with the
            // value we return.
            let percentageBasisBSize = PercentageBasisForItem().BSize(cbWM);
            if !nsLayoutUtils::IsAutoBSize(&mainStyleSize, percentageBasisBSize) {
                specifiedSizeSuggestion = nsLayoutUtils::ComputeBSizeValueHandlingStretch(percentageBasisBSize, aFlexItem.MarginSizeInMainAxis(), aFlexItem.BorderPaddingSizeInMainAxis(), boxSizingAdjust.BSize(cbWM), &mainStyleSize);
            }
        }

        if specifiedSizeSuggestion != nscoord_MAX {
            // We have the specified size suggestion. Return it now since we don't need
            // to consider transferred size suggestion.
            FLEX_LOGV!("Specified size suggestion: %d", specifiedSizeSuggestion);
            return specifiedSizeSuggestion;
        }

        // Compute the transferred size suggestion, which is the cross size converted
        // through the aspect ratio (if the item is replaced, and it has an aspect
        // ratio and a definite cross size).
        let aspectRatio = aFlexItem.GetAspectRatio();
        if aFlexItem.Frame().IsReplaced() && aspectRatio.operator_bool() && aFlexItem.IsCrossSizeDefinite(aItemReflowInput) {
            // We have a usable aspect ratio. (not going to divide by 0)
            let mut transferredSizeSuggestion = aspectRatio.ComputeRatioDependentSize(aFlexItem.MainAxis(), cbWM, aFlexItem.CrossSize(), boxSizingAdjust);

            // Clamp the transferred size suggestion by any definite min and max
            // cross size converted through the aspect ratio.
            transferredSizeSuggestion = aFlexItem.ClampMainSizeViaCrossAxisConstraints(transferredSizeSuggestion, aItemReflowInput);

            FLEX_LOGV!("Transferred size suggestion: %d", transferredSizeSuggestion);
            return transferredSizeSuggestion;
        }

        nscoord_MAX
    }

    // Note: If & when we handle "min-height: min-content" for flex items,
    // we may want to resolve that in this function, too.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::ResolveAutoFlexBasisAndMinSize
    pub fn ResolveAutoFlexBasisAndMinSize(&self, aFlexItem: &mut FlexItem, aItemReflowInput: &ReflowInput, aAxisTracker: &FlexboxAxisTracker) {
        // (Note: We can guarantee that the flex-basis will have already been
        // resolved if the main axis is the same as the item's inline
        // axis. Inline-axis values should always be resolvable without reflow.)
        let isMainSizeAuto = !aFlexItem.IsInlineAxisMainAxis() && NS_UNCONSTRAINEDSIZE == aFlexItem.FlexBaseSize();

        let isMainMinSizeAuto = aFlexItem.NeedsMinSizeAutoResolution();

        if !isMainSizeAuto && !isMainMinSizeAuto {
            // Nothing to do; this function is only needed for flex items
            // with a used flex-basis of "auto" or a min-main-size of "auto".
            return;
        }

        FLEX_ITEM_LOG!(aFlexItem.Frame(), "Resolving auto main size? %s; resolving auto min main size? %s", YesOrNo(isMainSizeAuto), YesOrNo(isMainMinSizeAuto));

        let mut resolvedMinSize: nscoord = 0; // (only set/used if isMainMinSizeAuto==true)
        let mut minSizeNeedsToMeasureContent = false; // assume the best
        if isMainMinSizeAuto {
            if self.IsLegacyWebkitBox() {
                // Allow flex items in a legacy flex container to shrink below their
                // automatic minimum size by setting the resolved minimum size to zero.
                // This behavior is not in the spec, but it aligns with blink and webkit's
                // implementation.
                resolvedMinSize = 0;
            } else {
                // Resolve the min-size, except for considering the min-content size.
                // (We'll consider that later, if we need to.)
                resolvedMinSize = self.PartiallyResolveAutoMinSize(aFlexItem, aItemReflowInput, aAxisTracker);
            }
            if resolvedMinSize > 0 {
                // If resolvedMinSize were already at 0, we could skip calculating content
                // size suggestion because it can't go any lower.
                minSizeNeedsToMeasureContent = true;
            }
        }

        let flexBasisNeedsToMeasureContent = isMainSizeAuto;

        // Measure content, if needed (w/ intrinsic-width method or a reflow)
        if minSizeNeedsToMeasureContent || flexBasisNeedsToMeasureContent {
            // Compute the content size suggestion, which is the min-content size in the
            // main axis.
            let mut contentSizeSuggestion: nscoord = nscoord_MAX;

            if aFlexItem.IsInlineAxisMainAxis() {
                if minSizeNeedsToMeasureContent {
                    // Compute the flex item's content size suggestion, which is the
                    // 'min-content' size on the main axis.
                    // https://drafts.csswg.org/css-flexbox-1/#content-size-suggestion
                    let cbWM = aAxisTracker.GetWritingMode();
                    let itemWM = aFlexItem.GetWritingMode();
                    let availISize: nscoord = 0; // for min-content size
                    let mut sizeOverrides = StyleSizeOverrides::default();
                    sizeOverrides.mStyleISize = Some(StyleSize::Auto);
                    if aFlexItem.IsStretched() {
                        sizeOverrides.mStyleBSize = Some(aFlexItem.StyleCrossSize());
                    }
                    let sizeInItemWM = aFlexItem.Frame().ComputeSize(
                        aItemReflowInput,
                        itemWM,
                        aItemReflowInput.mContainingBlockSize,
                        availISize,
                        aItemReflowInput.ComputedLogicalMargin(itemWM).Size(itemWM),
                        aItemReflowInput.ComputedLogicalBorderPadding(itemWM).Size(itemWM),
                        &sizeOverrides,
                        FlexShrinkWrapFlags(),
                    );

                    contentSizeSuggestion = aAxisTracker.MainComponent__LogicalSize(sizeInItemWM.mLogicalSize.ConvertTo(cbWM, itemWM));
                }
                NS_ASSERTION!(!flexBasisNeedsToMeasureContent, "flex-basis:auto should have been resolved in the reflow input, for horizontal flexbox. It shouldn't need special handling here");
            } else {
                // If this item is flexible (in its block axis)...
                // OR if we're measuring its 'auto' min-BSize, with its main-size (in its
                // block axis) being something non-"auto"...
                // THEN: we assume that the computed BSize that we're reflowing with now
                // could be different from the one we'll use for this flex item's
                // "actual" reflow later on.  In that case, we need to be sure the flex
                // item treats this as a block-axis resize (regardless of whether there
                // are actually any ancestors being resized in that axis).
                // (Note: We don't have to do this for the inline axis, because
                // InitResizeFlags will always turn on mIsIResize on when it sees that
                // the computed ISize is different from current ISize, and that's all we
                // need.)
                let forceBResizeForMeasuringReflow = !aFlexItem.IsFrozen() || // Is the item flexible?
                    !flexBasisNeedsToMeasureContent; // Are we *only* measuring it for
                                                     // 'min-block-size:auto'?

                let flexContainerRI = aItemReflowInput.mParentReflowInput.unwrap();
                let contentBSize = self.MeasureFlexItemContentBSize(aFlexItem, forceBResizeForMeasuringReflow, flexContainerRI);
                if minSizeNeedsToMeasureContent {
                    contentSizeSuggestion = contentBSize;
                }
                if flexBasisNeedsToMeasureContent {
                    aFlexItem.SetFlexBaseSizeAndMainSize(contentBSize);
                    aFlexItem.SetIsFlexBaseSizeContentBSize();
                }
            }

            if minSizeNeedsToMeasureContent {
                // Clamp the content size suggestion by any definite min and max cross
                // size converted through the aspect ratio.
                if aFlexItem.HasAspectRatio() {
                    contentSizeSuggestion = aFlexItem.ClampMainSizeViaCrossAxisConstraints(contentSizeSuggestion, aItemReflowInput);
                }

                FLEX_LOGV!("Content size suggestion: %d", contentSizeSuggestion);
                resolvedMinSize = std::cmp::min(resolvedMinSize, contentSizeSuggestion);

                // Clamp the resolved min main size by the max main size if it's definite.
                if aFlexItem.MainMaxSize() != NS_UNCONSTRAINEDSIZE {
                    resolvedMinSize = std::cmp::min(resolvedMinSize, aFlexItem.MainMaxSize());
                }
                // (C++ has an `else if (MOZ_UNLIKELY(resolvedMinSize > nscoord_MAX))` branch for a bogus
                // size above nscoord_MAX; nscoord is i32 here, so the value cannot exceed it.)
                FLEX_LOGV!("Resolved auto min main size: %d", resolvedMinSize);

                if resolvedMinSize == contentSizeSuggestion {
                    // When we are here, we've measured the item's content-based size, and
                    // we used it as the resolved auto min main size. Record the fact so
                    // that we can use it to determine whether we allow a flex item to grow
                    // its block-size in ReflowFlexItem().
                    aFlexItem.SetIsMainMinSizeContentBSize();
                }
            }
        }

        if isMainMinSizeAuto {
            aFlexItem.UpdateMainMinSize(resolvedMinSize);
        }
    }
}

/**
 * A cached result for a flex item's block-axis measuring reflow. This cache
 * prevents us from doing exponential reflows in cases of deeply nested flex
 * and scroll frames.
 *
 * We store the cached value in the flex item's frame property table, for
 * simplicity.
 *
 * Right now, we cache the following as a "key", from the item's ReflowInput:
 *   - its ComputedSize
 *   - its min/max block size (in case its ComputedBSize is unconstrained)
 *   - its AvailableBSize
 * ...and we cache the following as the "value", from the item's ReflowOutput:
 *   - its final content-box BSize
 *
 * The assumption here is that a given flex item measurement from our "value"
 * won't change unless one of the pieces of the "key" change, or the flex
 * item's intrinsic size is marked as dirty (due to a style or DOM change).
 * (The latter will cause the cached value to be discarded, in
 * nsIFrame::MarkIntrinsicISizesDirty.)
 *
 * Note that the components of "Key" (mComputed{MinB,MaxB,}Size and
 * mAvailableBSize) are sufficient to catch any changes to the flex container's
 * size that the item may care about for its measuring reflow. Specifically:
 *  - If the item cares about the container's size (e.g. if it has a percent
 *    height and the container's height changes, in a horizontal-WM container)
 *    then that'll be detectable via the item's ReflowInput's "ComputedSize()"
 *    differing from the value in our Key.  And the same applies for the
 *    inline axis.
 *  - If the item is fragmentable (pending bug 939897) and its measured BSize
 *    depends on where it gets fragmented, then that sort of change can be
 *    detected due to the item's ReflowInput's "AvailableBSize()" differing
 *    from the value in our Key.
 *
 * One particular case to consider (& need to be sure not to break when
 * changing this class): the flex item's computed BSize may change between
 * measuring reflows due to how the mIsFlexContainerMeasuringBSize flag affects
 * size computation (see bug 1336708). This is one reason we need to use the
 * computed BSize as part of the key.
 */
// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::CachedBAxisMeasurement
#[derive(Clone)]
pub struct CachedBAxisMeasurement {
    mKey: CachedBAxisMeasurement_Key,

    // This could/should be const, but it's non-const for now just because it's
    // assigned via a series of steps in the constructor body:
    mBSize: nscoord,
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::CachedBAxisMeasurement::Key
#[derive(Clone, PartialEq)]
struct CachedBAxisMeasurement_Key {
    mComputedSize: LogicalSize,
    mComputedMinBSize: nscoord,
    mComputedMaxBSize: nscoord,
    mAvailableBSize: nscoord,
}

impl CachedBAxisMeasurement_Key {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp Key::Key
    fn new(aRI: &ReflowInput) -> CachedBAxisMeasurement_Key {
        CachedBAxisMeasurement_Key {
            mComputedSize: aRI.ComputedSize__(),
            mComputedMinBSize: aRI.ComputedMinBSize(),
            mComputedMaxBSize: aRI.ComputedMaxBSize(),
            mAvailableBSize: aRI.AvailableBSize(),
        }
    }
}

impl CachedBAxisMeasurement {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedBAxisMeasurement::CachedBAxisMeasurement
    pub fn new(aReflowInput: &ReflowInput, aReflowOutput: &ReflowOutput) -> CachedBAxisMeasurement {
        // To get content-box bsize, we have to subtract off border & padding
        // (and floor at 0 in case the border/padding are too large):
        let itemWM = aReflowInput.GetWritingMode();
        let borderBoxBSize = aReflowOutput.BSize(itemWM);
        let mut bSize = borderBoxBSize - aReflowInput.ComputedLogicalBorderPadding(itemWM).BStartEnd(itemWM);
        bSize = std::cmp::max(0, bSize);
        CachedBAxisMeasurement { mKey: CachedBAxisMeasurement_Key::new(aReflowInput), mBSize: bSize }
    }

    /**
     * Returns true if this cached flex item measurement is valid for (i.e. can
     * be expected to match the output of) a measuring reflow whose input
     * parameters are given via aReflowInput.
     */
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedBAxisMeasurement::IsValidFor
    pub fn IsValidFor(&self, aReflowInput: &ReflowInput) -> bool {
        self.mKey == CachedBAxisMeasurement_Key::new(aReflowInput)
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedBAxisMeasurement::BSize
    pub fn BSize(&self) -> nscoord {
        self.mBSize
    }
}

/**
 * A cached copy of various metrics from a flex item's most recent final reflow.
 * It can be used to determine whether we can optimize away the flex item's
 * final reflow, when we perform an incremental reflow of its flex container.
 */
// Gecko: layout/generic/nsFlexContainerFrame.cpp CachedFinalReflowMetrics
#[derive(Clone)]
pub struct CachedFinalReflowMetrics {
    // The flex item's border and padding, in its own writing-mode, that it used
    // used during its most recent "final reflow".
    mBorderPadding: LogicalMargin,

    // The flex item's content-box size, in its own writing-mode, that it used
    // during its most recent "final reflow".
    mSize: LogicalSize,

    // True if the flex item's BSize was considered "indefinite" in its most
    // recent "final reflow". (For a flex item "final reflow", this is fully
    // determined by the mTreatBSizeAsIndefinite flag in ReflowInput. See the
    // flag's documentation for more information.)
    mTreatBSizeAsIndefinite: bool,
}

impl CachedFinalReflowMetrics {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedFinalReflowMetrics::CachedFinalReflowMetrics(const ReflowInput&, const ReflowOutput&)
    pub fn new__ReflowInput_ReflowOutput(aReflowInput: &ReflowInput, aReflowOutput: &ReflowOutput) -> CachedFinalReflowMetrics {
        CachedFinalReflowMetrics::new__WritingMode_ReflowInput_ReflowOutput(aReflowInput.GetWritingMode(), aReflowInput, aReflowOutput)
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedFinalReflowMetrics::CachedFinalReflowMetrics(const FlexItem&, const LogicalSize&)
    pub fn new__FlexItem_LogicalSize(aItem: &FlexItem, aSize: LogicalSize) -> CachedFinalReflowMetrics {
        CachedFinalReflowMetrics { mBorderPadding: aItem.BorderPadding().ConvertTo(aItem.GetWritingMode(), aItem.ContainingBlockWM()), mSize: aSize, mTreatBSizeAsIndefinite: aItem.TreatBSizeAsIndefinite() }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedFinalReflowMetrics::Size
    pub fn Size(&self) -> &LogicalSize {
        &self.mSize
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedFinalReflowMetrics::BorderPadding
    pub fn BorderPadding(&self) -> &LogicalMargin {
        &self.mBorderPadding
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedFinalReflowMetrics::TreatBSizeAsIndefinite
    pub fn TreatBSizeAsIndefinite(&self) -> bool {
        self.mTreatBSizeAsIndefinite
    }

    // A convenience constructor with a WritingMode argument.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedFinalReflowMetrics::CachedFinalReflowMetrics(WritingMode, const ReflowInput&, const ReflowOutput&)
    fn new__WritingMode_ReflowInput_ReflowOutput(aWM: WritingMode, aReflowInput: &ReflowInput, aReflowOutput: &ReflowOutput) -> CachedFinalReflowMetrics {
        let borderPadding = aReflowInput.ComputedLogicalBorderPadding(aWM);
        CachedFinalReflowMetrics { mBorderPadding: borderPadding, mSize: aReflowOutput.Size(aWM) - borderPadding.Size(aWM), mTreatBSizeAsIndefinite: aReflowInput.mFlags.mTreatBSizeAsIndefinite }
    }
}

/**
 * When we instantiate/update a CachedFlexItemData, this enum must be used to
 * indicate the sort of reflow whose results we're capturing. This impacts
 * what we cache & how we use the cached information.
 */
// Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItemReflowType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlexItemReflowType {
    // A reflow to measure the block-axis size of a flex item (as an input to the
    // flex layout algorithm).
    Measuring,

    // A reflow with the flex item's "final" size at the end of the flex layout
    // algorithm.
    Final,
}

/**
 * This class stores information about the conditions and results for the most
 * recent ReflowChild call that we made on a given flex item.  This information
 * helps us reason about whether we can assume that a subsequent ReflowChild()
 * invocation is unnecessary & skippable.
 */
// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::CachedFlexItemData
pub struct CachedFlexItemData {
    // If the flex container needs a measuring reflow for the flex item, then the
    // resulting block-axis measurements can be cached here.  If no measurement
    // has been needed so far, then this member will be Nothing().
    pub mBAxisMeasurement: Option<CachedBAxisMeasurement>,

    // The metrics that the corresponding flex item used in its most recent
    // "final reflow". (Note: the assumption here is that this reflow was this
    // item's most recent reflow of any type.  If the item ends up undergoing a
    // subsequent measuring reflow, then this value needs to be cleared, because
    // at that point it's no longer an accurate way of reasoning about the
    // current state of the frame tree.)
    pub mFinalReflowMetrics: Option<CachedFinalReflowMetrics>,
}

impl CachedFlexItemData {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedFlexItemData::CachedFlexItemData
    pub fn new(aReflowInput: &ReflowInput, aReflowOutput: &ReflowOutput, aType: FlexItemReflowType) -> CachedFlexItemData {
        let mut data = CachedFlexItemData { mBAxisMeasurement: None, mFinalReflowMetrics: None };
        data.Update__ReflowInput_ReflowOutput_FlexItemReflowType(aReflowInput, aReflowOutput, aType);
        data
    }

    // This method is intended to be called after we perform either a "measuring
    // reflow" or a "final reflow" for a given flex item.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedFlexItemData::Update(const ReflowInput&, const ReflowOutput&, FlexItemReflowType)
    pub fn Update__ReflowInput_ReflowOutput_FlexItemReflowType(&mut self, aReflowInput: &ReflowInput, aReflowOutput: &ReflowOutput, aType: FlexItemReflowType) {
        if aType == FlexItemReflowType::Measuring {
            self.mBAxisMeasurement = None;
            self.mBAxisMeasurement = Some(CachedBAxisMeasurement::new(aReflowInput, aReflowOutput));
            // Clear any cached "last final reflow metrics", too, because now the most
            // recent reflow was *not* a "final reflow".
            self.mFinalReflowMetrics = None;
            return;
        }

        MOZ_ASSERT!(aType == FlexItemReflowType::Final);
        self.mFinalReflowMetrics = None;
        self.mFinalReflowMetrics = Some(CachedFinalReflowMetrics::new__ReflowInput_ReflowOutput(aReflowInput, aReflowOutput));
    }

    // This method is intended to be called for situations where we decide to
    // skip a final reflow because we've just done a measuring reflow which left
    // us (and our descendants) with the correct sizes. In this scenario, we
    // still want to cache the size as if we did a final reflow (because we've
    // determined that the recent measuring reflow was sufficient).  That way,
    // our flex container can still skip a final reflow for this item in the
    // future as long as conditions are right.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CachedFlexItemData::Update(const FlexItem&, const LogicalSize&)
    pub fn Update__FlexItem_LogicalSize(&mut self, aItem: &FlexItem, aSize: LogicalSize) {
        MOZ_ASSERT!(self.mFinalReflowMetrics.is_none(), "This version of the method is only intended to be called when the most recent reflow was a 'measuring reflow'; and that should have cleared out mFinalReflowMetrics");

        self.mFinalReflowMetrics = None; // Just in case this assert^ fails.
        self.mFinalReflowMetrics = Some(CachedFinalReflowMetrics::new__FlexItem_LogicalSize(aItem, aSize));
    }

    // ADAPTER: `aFrame->GetProperty(CachedFlexItemData::Prop())` (boxed behind a usize property).
    pub fn Get(aFrame: nsIFrame) -> Option<&'static mut CachedFlexItemData> {
        aFrame.GetProperty(CachedFlexItemDataProperty()).map(|p| unsafe { &mut *(p as *mut CachedFlexItemData) })
    }
    // ADAPTER: `aFrame->SetProperty(CachedFlexItemData::Prop(), new CachedFlexItemData(...))`
    pub fn Set(aFrame: nsIFrame, aData: CachedFlexItemData) -> &'static mut CachedFlexItemData {
        let p = Box::into_raw(Box::new(aData));
        aFrame.SetOwnedPointerProperty::<CachedFlexItemData>(CachedFlexItemDataProperty(), p as usize);
        unsafe { &mut *p }
    }
}

impl nsFlexContainerFrame {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::MarkCachedFlexMeasurementsDirty
    pub fn MarkCachedFlexMeasurementsDirty(aItemFrame: nsIFrame) {
        MOZ_ASSERT!(aItemFrame.IsFlexItem());
        if let Some(cache) = CachedFlexItemData::Get(aItemFrame) {
            cache.mBAxisMeasurement = None;
            cache.mFinalReflowMetrics = None;
        }
    }

    // ADAPTER: returns a copy of the cached measurement (C++ returns a reference into the frame property).
    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::MeasureBSizeForFlexItem
    pub fn MeasureBSizeForFlexItem(&self, aItem: &mut FlexItem, aChildReflowInput: &mut ReflowInput) -> CachedBAxisMeasurement {
        let cachedData = CachedFlexItemData::Get(aItem.Frame());

        if let Some(cachedData) = &cachedData {
            if let Some(measurement) = &cachedData.mBAxisMeasurement {
                if !aItem.Frame().IsSubtreeDirty() && measurement.IsValidFor(aChildReflowInput) {
                    FLEX_ITEM_LOG!(aItem.Frame(), "[perf] Accepted cached measurement: block-size %d", measurement.BSize());
                    return measurement.clone();
                }
                FLEX_ITEM_LOG!(aItem.Frame(), "[perf] Rejected cached measurement: block-size %d", measurement.BSize());
            } else {
                FLEX_ITEM_LOG!(aItem.Frame(), "[perf] No cached measurement");
            }
        } else {
            FLEX_ITEM_LOG!(aItem.Frame(), "[perf] No cached measurement");
        }

        // CachedFlexItemData is stored in item's writing mode, so we pass
        // aChildReflowInput into ReflowOutput's constructor.
        let mut childReflowOutput = ReflowOutput::new__ReflowInput(aChildReflowInput);
        let mut childStatus = nsReflowStatus::new();

        let flags = nsIFrame_ReflowChildFlags::NoMoveFrame;
        let outerWM = self.GetWritingMode();
        let dummyPosition = LogicalPoint::new__WritingMode(outerWM);
        let dummyContainerSize = nsSize::default();

        // We use NoMoveFrame, so the position and container size used here are
        // unimportant.
        self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
            aItem.Frame(),
            &self.PresContext(),
            &mut childReflowOutput,
            aChildReflowInput,
            outerWM,
            dummyPosition,
            dummyContainerSize,
            flags,
            &mut childStatus,
            None,
        );
        aItem.SetHadMeasuringReflow();

        // We always use unconstrained available block-size to measure flex items,
        // which means they should always complete.
        MOZ_ASSERT!(childStatus.IsComplete(), "We gave flex item unconstrained available block-size, so it should be complete");

        // Tell the child we're done with its initial reflow.
        // (Necessary for e.g. GetBaseline() to work below w/out asserting)
        nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
            aItem.Frame(),
            &self.PresContext(),
            &childReflowOutput,
            Some(aChildReflowInput),
            outerWM,
            dummyPosition,
            dummyContainerSize,
            flags,
        );

        aItem.SetAscent(childReflowOutput.BlockStartAscent());

        // Update (or add) our cached measurement, so that we can hopefully skip this
        // measuring reflow the next time around:
        let cachedData = CachedFlexItemData::Get(aItem.Frame());
        let cachedData = if let Some(cachedData) = cachedData {
            cachedData.Update__ReflowInput_ReflowOutput_FlexItemReflowType(aChildReflowInput, &childReflowOutput, FlexItemReflowType::Measuring);
            cachedData
        } else {
            CachedFlexItemData::Set(aItem.Frame(), CachedFlexItemData::new(aChildReflowInput, &childReflowOutput, FlexItemReflowType::Measuring))
        };
        cachedData.mBAxisMeasurement.clone().unwrap()
    }

    /* virtual */
    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::MarkIntrinsicISizesDirty
    pub fn MarkIntrinsicISizesDirty(&self) {
        self.mCachedIntrinsicSizes().Clear();
        self.nsContainerFrame_MarkIntrinsicISizesDirty();
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::MeasureFlexItemContentBSize
    pub fn MeasureFlexItemContentBSize(&self, aFlexItem: &mut FlexItem, aForceBResizeForMeasuringReflow: bool, aParentReflowInput: &ReflowInput) -> nscoord {
        FLEX_ITEM_LOG!(aFlexItem.Frame(), "Measuring item's content block-size");

        // Set up a reflow input for measuring the flex item's content block-size:
        let wm = aFlexItem.Frame().GetWritingMode();
        let mut availSize = aParentReflowInput.ComputedSize__WritingMode(wm);
        *availSize.BSize_mut(wm) = NS_UNCONSTRAINEDSIZE;

        let mut sizeOverrides = StyleSizeOverrides::default();
        if aFlexItem.IsStretched() {
            sizeOverrides.mStyleISize = Some(aFlexItem.StyleCrossSize());
            FLEX_LOGV!("Cross size override: %d", aFlexItem.CrossSize());
        }
        sizeOverrides.mStyleBSize = Some(StyleSize::Auto);

        let presContext = self.PresContext();
        let mut childRIForMeasuringBSize = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            &presContext,
            aParentReflowInput,
            aFlexItem.Frame(),
            availSize,
            None,
            EnumSet::default(),
            sizeOverrides,
            FlexShrinkWrapFlags(),
            std::ptr::null_mut(),
        );

        // When measuring flex item's content block-size, disregard the item's
        // min-block-size and max-block-size by resetting both to to their
        // unconstraining (extreme) values. The flexbox layout algorithm does still
        // explicitly clamp both sizes when resolving the target main size.
        childRIForMeasuringBSize.SetComputedMinBSize(0);
        childRIForMeasuringBSize.SetComputedMaxBSize(NS_UNCONSTRAINEDSIZE);

        if aForceBResizeForMeasuringReflow {
            childRIForMeasuringBSize.SetBResize(true);
            // Not 100% sure this is needed, but be conservative for now:
            childRIForMeasuringBSize.SetBResizeForPercentages(true);
        }

        let measurement = self.MeasureBSizeForFlexItem(aFlexItem, &mut childRIForMeasuringBSize);

        measurement.BSize()
    }
}

impl FlexItem {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::FlexItem(ReflowInput&, float, float, nscoord, nscoord, nscoord, nscoord, nscoord, nscoord, const FlexboxAxisTracker&)
    pub fn new(aFlexItemReflowInput: &ReflowInput, aFlexGrow: f32, aFlexShrink: f32, aFlexBaseSize: nscoord, aMainMinSize: nscoord, aMainMaxSize: nscoord, aTentativeCrossSize: nscoord, aCrossMinSize: nscoord, aCrossMaxSize: nscoord, aAxisTracker: &FlexboxAxisTracker) -> FlexItem {
        let frame = aFlexItemReflowInput.mFrame;
        let wm = aFlexItemReflowInput.GetWritingMode();
        let cbWM = aAxisTracker.GetWritingMode();
        let mut item = FlexItem::WithDefaultMembers(frame, wm, cbWM, aAxisTracker.MainAxis());
        item.mFlexGrow = aFlexGrow;
        item.mFlexShrink = aFlexShrink;
        item.mAspectRatio = frame.GetAspectRatio();
        item.mBorderPadding = aFlexItemReflowInput.ComputedLogicalBorderPadding(cbWM);
        item.mMargin = aFlexItemReflowInput.ComputedLogicalMargin(cbWM);
        item.mMainMinSize = aMainMinSize;
        item.mMainMaxSize = aMainMaxSize;
        item.mCrossMinSize = aCrossMinSize;
        item.mCrossMaxSize = aCrossMaxSize;
        item.mCrossSize = aTentativeCrossSize;
        item.mIsInlineAxisMainAxis = aAxisTracker.IsInlineAxisMainAxis(item.mWM);
        item.mNeedsMinSizeAutoResolution = item.IsMinSizeAutoResolutionNeeded();
        // mAlignSelf, mHasAnyAutoMargin see below

        MOZ_ASSERT!(!item.mFrame.IsPlaceholderFrame(), "placeholder frames should not be treated as flex items");
        MOZ_ASSERT!(item.mFrame.IsFlexItem(), "mFrame must be a flex item!");
        MOZ_ASSERT!(item.mIsInlineAxisMainAxis == nsFlexContainerFrame::IsItemInlineAxisMainAxis(item.mFrame), "public API should be consistent with internal state (about whether flex item's inline axis is flex container's main axis)");

        let container = nsFlexContainerFrame(item.mFrame.GetParent().unwrap());
        let (alignSelf, alignSelfFlags) = container.UsedAlignSelfAndFlagsForItem(item.mFrame);
        item.mAlignSelf = alignSelf;
        item.mAlignSelfFlags = alignSelfFlags;

        // Our main-size is considered definite if any of these are true:
        // (a) main axis is the item's inline axis.
        // (b) flex container has definite main size.
        // (c) flex item has a definite flex basis.
        //
        // Hence, we need to take care to treat the final main-size as *indefinite*
        // if none of these conditions are satisfied.
        if item.mIsInlineAxisMainAxis {
            // The item's block-axis is the flex container's cross axis. We don't need
            // any special handling to treat cross sizes as indefinite, because the
            // cases where we stomp on the cross size with a definite value are all...
            // - situations where the spec requires us to treat the cross size as
            // definite; specifically, `align-self:stretch` whose cross size is
            // definite.
            // - situations where definiteness doesn't matter (e.g. for an element with
            // an aspect ratio, which for now are all leaf nodes and hence
            // can't have any percent-height descendants that would care about the
            // definiteness of its size. (Once bug 1528375 is fixed, we might need to
            // be more careful about definite vs. indefinite sizing on flex items with
            // aspect ratios.)
            item.mTreatBSizeAsIndefinite = false;
        } else {
            // The item's block-axis is the flex container's main axis. So, the flex
            // item's main size is its BSize, and is considered definite under certain
            // conditions laid out for definite flex-item main-sizes in the spec.
            let containerRI = aFlexItemReflowInput.mParentReflowInput.unwrap();
            if aAxisTracker.IsRowOriented() || (containerRI.ComputedBSize() != NS_UNCONSTRAINEDSIZE && !containerRI.mFlags.mTreatBSizeAsIndefinite) {
                // The flex *container* has a definite main-size (either by being
                // row-oriented [and using its own inline size which is by definition
                // definite, or by being column-oriented and having a definite
                // block-size).  The spec says this means all of the flex items'
                // post-flexing main sizes should *also* be treated as definite.
                item.mTreatBSizeAsIndefinite = false;
            } else if aFlexBaseSize != NS_UNCONSTRAINEDSIZE {
                // The flex item has a definite flex basis, which we'll treat as making
                // its main-size definite.
                item.mTreatBSizeAsIndefinite = false;
            } else {
                // Otherwise, we have to treat the item's BSize as indefinite.
                item.mTreatBSizeAsIndefinite = true;
            }
        }

        item.SetFlexBaseSizeAndMainSize(aFlexBaseSize);

        let styleMargin = &aFlexItemReflowInput.mStyleMargin;
        let anchorResolutionParams = AnchorPosResolutionParams::From__SizeComputationInput_bool(aFlexItemReflowInput, false);
        item.mHasAnyAutoMargin = styleMargin.HasInlineAxisAuto(item.mCBWM, &anchorResolutionParams) || styleMargin.HasBlockAxisAuto(item.mCBWM, &anchorResolutionParams);

        // Assert that any "auto" margin components are set to 0.
        // (We'll resolve them later; until then, we want to treat them as 0-sized.)
        #[cfg(debug_assertions)]
        {
            for side in [LogicalSide::BStart, LogicalSide::BEnd, LogicalSide::IStart, LogicalSide::IEnd] {
                if styleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(side, item.mCBWM, &anchorResolutionParams).IsAuto() {
                    MOZ_ASSERT!(item.GetMarginComponentForSide(side) == 0, "Someone else tried to resolve our auto margin");
                }
            }
        }

        if item.mAlignSelf == StyleAlignFlags::BASELINE || item.mAlignSelf == StyleAlignFlags::LAST_BASELINE {
            // Check which of the item's baselines we're meant to use (first vs. last)
            let usingItemFirstBaseline = item.mAlignSelf == StyleAlignFlags::BASELINE;
            if item.IsBlockAxisCrossAxis() {
                // The flex item wants to be aligned in the cross axis using one of its
                // baselines; and the cross axis is the item's block axis, so
                // baseline-alignment in that axis makes sense.

                // To determine the item's baseline sharing group, we check whether the
                // item's block axis has the same vs. opposite flow direction as the
                // corresponding LogicalAxis on the flex container.  We do this by
                // getting the physical side that corresponds to these axes' "logical
                // start" sides, and we compare those physical sides to find out if
                // they're the same vs. opposite.
                let itemBlockStartSide = item.mWM.PhysicalSide(LogicalSide::BStart);

                // (Note: this is *not* the "flex-start" side; rather, it's the *logical*
                // i.e. WM-relative block-start or inline-start side.)
                let containerStartSideInCrossAxis = item.mCBWM.PhysicalSide(MakeLogicalSide(aAxisTracker.CrossAxis(), LogicalEdge::Start));

                // We already know these two Sides (the item's block-start and the
                // container's 'logical start' side for its cross axis) are in the same
                // physical axis, since we're inside of a check for
                // FlexItem::IsBlockAxisCrossAxis().  So these two Sides must be either
                // the same physical side or opposite from each other.  If the Sides are
                // the same, then the flow direction is the same, which means the item's
                // {first,last} baseline participates in the {first,last}
                // baseline-sharing group in its FlexLine.  Otherwise, the flow direction
                // is opposite, and so the item's {first,last} baseline participates in
                // the opposite i.e. {last,first} baseline-sharing group.  This is
                // roughly per css-align-3 section 9.2, specifically the definition of
                // what makes baseline alignment preferences "compatible".
                let itemBlockAxisFlowDirMatchesContainer = itemBlockStartSide == containerStartSideInCrossAxis;
                item.mBaselineSharingGroup = if itemBlockAxisFlowDirMatchesContainer == usingItemFirstBaseline { BaselineSharingGroup::First } else { BaselineSharingGroup::Last };
            } else {
                // The flex item wants to be aligned in the cross axis using one of its
                // baselines, but we cannot get its baseline because the FlexItem's block
                // axis is *orthogonal* to the container's cross axis. To handle this, we
                // are supposed to synthesize a baseline from the item's border box and
                // using that for baseline alignment.
                item.mBaselineSharingGroup = if usingItemFirstBaseline { BaselineSharingGroup::First } else { BaselineSharingGroup::Last };
            }
        }
        item
    }

    // Simplified constructor for creating a special "strut" FlexItem, for a child
    // with visibility:collapse. The strut has 0 main-size, and it only exists to
    // impose a minimum cross size on whichever FlexLine it ends up in.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::FlexItem(nsIFrame*, nscoord, WritingMode, const FlexboxAxisTracker&)
    pub fn new__nsIFrame_nscoord_WritingMode_FlexboxAxisTracker(aChildFrame: nsIFrame, aCrossSize: nscoord, aContainerWM: WritingMode, aAxisTracker: &FlexboxAxisTracker) -> FlexItem {
        let mut item = FlexItem::WithDefaultMembers(aChildFrame, aChildFrame.GetWritingMode(), aContainerWM, aAxisTracker.MainAxis());
        item.mCrossSize = aCrossSize;
        // Struts don't do layout, so its WM doesn't matter at this point. So, we
        // just share container's WM for simplicity:
        item.mIsFrozen = true;
        item.mIsStrut = true; // (this is the constructor for making struts, after all)
        item.mAlignSelf = StyleAlignFlags::FLEX_START;
        MOZ_ASSERT!(item.mFrame.StyleVisibility().IsCollapse(), "Should only make struts for children with 'visibility:collapse'");
        MOZ_ASSERT!(!item.mFrame.IsPlaceholderFrame(), "placeholder frames should not be treated as flex items");
        MOZ_ASSERT!(!item.mFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW), "out-of-flow frames should not be treated as flex items");
        item
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsMinSizeAutoResolutionNeeded
    fn IsMinSizeAutoResolutionNeeded(&self) -> bool {
        // We'll need special behavior for "min-[width|height]:auto" (whichever is in
        // the flex container's main axis) iff:
        // (a) its computed value is "auto", and
        // (b) the item is *not* a scroll container. (A scroll container's automatic
        //     minimum size is zero.)
        // https://drafts.csswg.org/css-flexbox-1/#min-size-auto
        //
        // Note that the scroll container case is redefined to be looking at the
        // computed value instead, see https://github.com/w3c/csswg-drafts/issues/7714
        let mainMinSize = self.Frame().StylePosition().MinSize(self.MainAxis(), self.ContainingBlockWM(), &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.Frame(), std::ptr::null_mut()));

        // "min-{height,width}:stretch" never produces an automatic minimum size. You
        // might think it would result in an automatic min-size if the containing
        // block size is indefinite, but "stretch" is instead treated as 0px in that
        // case rather than auto. This WPT requires this behavior:
        // https://wpt.live/css/css-sizing/stretch/indefinite-4.html More details &
        // discussion here: https://github.com/w3c/csswg-drafts/issues/11006
        if mainMinSize.BehavesLikeStretchOnBlockAxis() {
            return false;
        }
        IsAutoOrEnumOnBSize(&mainMinSize, self.IsInlineAxisMainAxis()) && !self.Frame().StyleDisplay().IsScrollableOverflow()
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::MeasuredBSize
    pub fn MeasuredBSize(&self) -> Option<nscoord> {
        let cachedData = CachedFlexItemData::Get(self.Frame().FirstInFlow());
        match cachedData {
            Some(d) => d.mBAxisMeasurement.as_ref().map(|m| m.BSize()),
            None => None,
        }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::BaselineOffsetFromOuterCrossEdge
    pub fn BaselineOffsetFromOuterCrossEdge(&self, aStartSide: Side, aUseFirstLineBaseline: bool) -> nscoord {
        // NOTE:
        //  * We only use baselines for aligning in the flex container's cross axis.
        //  * Baselines are a measurement in the item's block axis.
        if self.IsBlockAxisMainAxis() {
            // We get here if the item's block axis is *orthogonal* the container's
            // cross axis. For example, a flex item with writing-mode:horizontal-tb in a
            // column-oriented flex container. We need to synthesize the item's baseline
            // from its border-box edge.
            let isMainAxisHorizontal = self.mCBWM.PhysicalAxis(self.MainAxis()) == PhysicalAxis::Horizontal;

            // When the main axis is horizontal, the synthesized baseline is the bottom
            // edge of the item's border-box. Otherwise, when the main axis is vertical,
            // the left edge. This is for compatibility with Google Chrome.
            let mut marginTopOrLeftToBaseline = if isMainAxisHorizontal { self.PhysicalMargin().top } else { self.PhysicalMargin().left };
            if self.mCBWM.IsAlphabeticalBaseline() {
                marginTopOrLeftToBaseline += if isMainAxisHorizontal { self.CrossSize() } else { 0 };
            } else {
                MOZ_ASSERT!(self.mCBWM.IsCentralBaseline());
                marginTopOrLeftToBaseline += self.CrossSize() / 2;
            }

            return if aStartSide == eSideTop || aStartSide == eSideLeft { marginTopOrLeftToBaseline } else { self.OuterCrossSize() - marginTopOrLeftToBaseline };
        }

        // We get here if the item's block axis is parallel (or antiparallel) to the
        // container's cross axis. We call ResolvedAscent() to get the item's
        // baseline. If the item has no baseline, the method will synthesize one from
        // the border-box edge.
        MOZ_ASSERT!(self.IsBlockAxisCrossAxis(), "Only expecting to be doing baseline computations when the cross axis is the block axis");

        let itemBlockStartSide = self.mWM.PhysicalSide(LogicalSide::BStart);

        let marginBStartToBaseline = self.ResolvedAscent(aUseFirstLineBaseline) + self.PhysicalMargin().Side(itemBlockStartSide);

        if aStartSide == itemBlockStartSide { marginBStartToBaseline } else { self.OuterCrossSize() - marginBStartToBaseline }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsCrossSizeAuto
    pub fn IsCrossSizeAuto(&self) -> bool {
        let styleFrame = nsLayoutUtils::GetStyleFrame(self.mFrame);
        let stylePos = styleFrame.StylePosition();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(styleFrame, std::ptr::null_mut());
        // Check whichever component is in the flex container's cross axis.
        // (IsInlineAxisCrossAxis() tells us whether that's our ISize or BSize, in
        // terms of our own WritingMode, mWM.)
        if self.IsInlineAxisCrossAxis() { stylePos.ISize(self.mWM, &anchorResolutionParams).IsAuto() } else { stylePos.BSize(self.mWM, &anchorResolutionParams).IsAuto() }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::IsCrossSizeDefinite
    pub fn IsCrossSizeDefinite(&self, aItemReflowInput: &ReflowInput) -> bool {
        if self.IsStretched() {
            // Definite cross-size, imposed via 'align-self:stretch' & flex container.
            return true;
        }

        let pos = &aItemReflowInput.mStylePosition;
        let anchorResolutionParams = AnchorPosResolutionParams::From__SizeComputationInput_bool(aItemReflowInput, false);
        let itemWM = self.GetWritingMode();

        // The logic here should be similar to the logic for isAutoISize/isAutoBSize
        // in nsContainerFrame::ComputeSizeWithIntrinsicDimensions().
        if self.IsInlineAxisCrossAxis() {
            return !pos.ISize(itemWM, &anchorResolutionParams).IsAuto();
        }

        let cbBSize = aItemReflowInput.mContainingBlockSize.BSize(itemWM);
        !nsLayoutUtils::IsAutoBSize(&pos.BSize(itemWM, &anchorResolutionParams), cbBSize)
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::ResolveFlexBaseSizeFromAspectRatio
    pub fn ResolveFlexBaseSizeFromAspectRatio(&mut self, aItemReflowInput: &ReflowInput) {
        // This implements the Flex Layout Algorithm Step 3B:
        // https://drafts.csswg.org/css-flexbox-1/#algo-main-item
        // If the flex item has ...
        //  - an aspect ratio,
        //  - a [used] flex-basis of 'content', and
        //  - a definite cross size
        // then the flex base size is calculated from its inner cross size and the
        // flex item's preferred aspect ratio.
        if self.HasAspectRatio()
            && nsFlexContainerFrame::IsUsedFlexBasisContent(
                &aItemReflowInput.mStylePosition.mFlexBasis(),
                &aItemReflowInput.mStylePosition.Size(self.MainAxis(), self.mCBWM, &AnchorPosResolutionParams::From__SizeComputationInput_bool(aItemReflowInput, false)),
            )
            && self.IsCrossSizeDefinite(aItemReflowInput)
        {
            let contentBoxSizeToBoxSizingAdjust = if aItemReflowInput.mStylePosition.mBoxSizing() == StyleBoxSizing::BorderBox { self.BorderPadding().Size(self.mCBWM) } else { LogicalSize::new__WritingMode(self.mCBWM) };
            let mainSizeFromRatio = self.mAspectRatio.ComputeRatioDependentSize(self.MainAxis(), self.mCBWM, self.CrossSize(), contentBoxSizeToBoxSizingAdjust);
            self.SetFlexBaseSizeAndMainSize(mainSizeFromRatio);
        }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::NumAutoMarginsInAxis
    fn NumAutoMarginsInAxis(&self, aAxis: LogicalAxis) -> u32 {
        let mut numAutoMargins: u32 = 0;
        let styleMargin = self.mFrame.StyleMargin();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.mFrame, std::ptr::null_mut());
        for edge in [LogicalEdge::Start, LogicalEdge::End] {
            let side = MakeLogicalSide(aAxis, edge);
            if styleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(side, self.mCBWM, &anchorResolutionParams).IsAuto() {
                numAutoMargins += 1;
            }
        }

        // Mostly for clarity:
        MOZ_ASSERT!(numAutoMargins <= 2, "We're just looking at one item along one dimension, so we should only have examined 2 margins");

        numAutoMargins
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::CanMainSizeInfluenceCrossSize
    pub fn CanMainSizeInfluenceCrossSize(&self) -> bool {
        if self.mIsStretched {
            // We've already had our cross-size stretched for "align-self:stretch").
            // The container is imposing its cross size on us.
            return false;
        }

        if self.mIsStrut {
            // Struts (for visibility:collapse items) have a predetermined size;
            // no need to measure anything.
            return false;
        }

        if self.HasAspectRatio() {
            // For flex items that have an aspect ratio (and maintain it, i.e. are
            // not stretched, which we already checked above): changes to main-size
            // *do* influence the cross size.
            return true;
        }

        if self.IsInlineAxisCrossAxis() {
            // If we get here, this function is really asking: "can changes to this
            // item's block size have an influence on its inline size"?

            // If a flex item's intrinsic inline size or its descendants' inline size
            // contributions depend on the item's block size, the answer is "yes".
            if self.mFrame.HasAnyStateBits(NS_FRAME_DESCENDANT_INTRINSIC_ISIZE_DEPENDS_ON_BSIZE) {
                return true;
            }
            // For blocks and tables, the answer is "no" (aside from the above special
            // case).
            if self.mFrame.IsBlockFrame() || self.mFrame.IsTableWrapperFrame() {
                // XXXdholbert (Maybe use an IsFrameOfType query or something more
                // general to test this across all frame types? For now, I'm just
                // optimizing for block and table, since those are common containers that
                // can contain arbitrarily-large subtrees (and that reliably have ISize
                // being unaffected by BSize, per CSS2).  So optimizing away needless
                // relayout is possible & especially valuable for these containers.)
                return false;
            }
            // Other opt-outs can go here, as they're identified as being useful
            // (particularly for containers where an extra reflow is expensive). But in
            // general, we have to assume that a flexed BSize *could* influence the
            // ISize. Some examples where this can definitely happen:
            // * Intrinsically-sized multicol with fixed-ISize columns, which adds
            // columns (i.e. grows in inline axis) depending on its block size.
            // * Intrinsically-sized multi-line column-oriented flex container, which
            // adds flex lines (i.e. grows in inline axis) depending on its block size.
        }

        // Default assumption, if we haven't proven otherwise: the resolved main size
        // *can* change the cross size.
        true
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::ClampMainSizeViaCrossAxisConstraints
    pub fn ClampMainSizeViaCrossAxisConstraints(&self, aMainSize: nscoord, aItemReflowInput: &ReflowInput) -> nscoord {
        MOZ_ASSERT!(self.HasAspectRatio(), "Caller should've checked the ratio is valid!");

        let contentBoxSizeToBoxSizingAdjust = if aItemReflowInput.mStylePosition.mBoxSizing() == StyleBoxSizing::BorderBox { self.BorderPadding().Size(self.mCBWM) } else { LogicalSize::new__WritingMode(self.mCBWM) };

        let mainMinSizeFromRatio = self.mAspectRatio.ComputeRatioDependentSize(self.MainAxis(), self.mCBWM, self.CrossMinSize(), contentBoxSizeToBoxSizingAdjust);
        let mut clampedMainSize = std::cmp::max(aMainSize, mainMinSizeFromRatio);

        if self.CrossMaxSize() != NS_UNCONSTRAINEDSIZE {
            let mainMaxSizeFromRatio = self.mAspectRatio.ComputeRatioDependentSize(self.MainAxis(), self.mCBWM, self.CrossMaxSize(), contentBoxSizeToBoxSizingAdjust);
            clampedMainSize = std::cmp::min(clampedMainSize, mainMaxSizeFromRatio);
        }

        clampedMainSize
    }
}

/**
 * Returns true if aFrame or any of its children have the
 * NS_FRAME_CONTAINS_RELATIVE_BSIZE flag set -- i.e. if any of these frames (or
 * their descendants) might have a relative-BSize dependency on aFrame (or its
 * ancestors).
 */
// Gecko: layout/generic/nsFlexContainerFrame.cpp FrameHasRelativeBSizeDependency
fn FrameHasRelativeBSizeDependency(aFrame: nsIFrame) -> bool {
    if aFrame.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE) {
        return true;
    }
    // for (const auto& childList : aFrame->ChildLists())
    let mut childLists: Vec<FrameChildList> = Vec::new();
    aFrame.GetChildLists(&mut childLists);
    for childList in &childLists {
        for childFrame in &childList.mList {
            if childFrame.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE) {
                return true;
            }
        }
    }
    false
}

impl FlexItem {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::NeedsFinalReflow
    pub fn NeedsFinalReflow(&self, aParentReflowInput: &ReflowInput) -> bool {
        // NOTE: StaticPrefs::layout_flexbox_item_final_reflow_optimization_enabled() is true by default.

        // NOTE: We can have continuations from an earlier constrained reflow.
        if self.mFrame.GetPrevInFlow().is_some() || self.mFrame.GetNextInFlow().is_some() {
            // This is an item has continuation(s). Reflow it.
            FLEX_ITEM_LOG!(self.mFrame, "[frag] Item needed a final reflow due to continuation(s)");
            return true;
        }

        // A flex item can grow its block-size in a fragmented context if there's any
        // force break within it (bug 1663079), or if it has a repeated table header
        // or footer (bug 1744363). We currently always reflow it.
        //
        // Bug 1815294: investigate if we can design a more specific condition to
        // prevent triggering O(n^2) behavior when printing a deeply-nested flex
        // container.
        if aParentReflowInput.IsInFragmentedContext() {
            FLEX_ITEM_LOG!(self.mFrame, "[frag] Item needed both a measuring reflow and a final reflow due to being in a fragmented context");
            return true;
        }

        // Flex item's final content-box size (in terms of its own writing-mode):
        let finalSize = if self.mIsInlineAxisMainAxis { LogicalSize::new(self.mWM, self.mMainSize, self.mCrossSize) } else { LogicalSize::new(self.mWM, self.mCrossSize, self.mMainSize) };

        if self.HadMeasuringReflow() {
            // We've already reflowed this flex item once, to measure it. In that
            // reflow, did its frame happen to end up with the correct final size
            // that the flex container would like it to have?
            if finalSize != self.mFrame.ContentSize(self.mWM) {
                // The measuring reflow left the item with a different size than its
                // final flexed size. So, we need to reflow to give it the correct size.
                FLEX_ITEM_LOG!(self.mFrame, "[perf] Item needed both a measuring reflow and a final reflow due to measured size disagreeing with final size");
                return true;
            }

            if FrameHasRelativeBSizeDependency(self.mFrame) {
                // This item has descendants with relative BSizes who may care that its
                // size may now be considered "definite" in the final reflow (whereas it
                // was indefinite during the measuring reflow).
                FLEX_ITEM_LOG!(self.mFrame, "[perf] Item needed both a measuring reflow and a final reflow due to BSize potentially becoming definite");
                return true;
            }

            // If we get here, then this flex item had a measuring reflow, it left us
            // with the correct size, none of its descendants care that its BSize may
            // now be considered definite, and it can fit into the available block-size.
            // So it doesn't need a final reflow.
            //
            // We now cache this size as if we had done a final reflow (because we've
            // determined that the measuring reflow was effectively equivalent).  This
            // way, in our next time through flex layout, we may be able to skip both
            // the measuring reflow *and* the final reflow (if conditions are the same
            // as they are now).
            if let Some(cache) = CachedFlexItemData::Get(self.mFrame) {
                cache.Update__FlexItem_LogicalSize(self, finalSize);
            }

            return false;
        }

        // This item didn't receive a measuring reflow (at least, not during this
        // reflow of our flex container).  We may still be able to skip reflowing it
        // (i.e. return false from this function), if its subtree is clean & its most
        // recent "final reflow" had it at the correct content-box size &
        // definiteness.
        // Let's check for each condition that would still require us to reflow:
        if self.mFrame.IsSubtreeDirty() {
            FLEX_ITEM_LOG!(self.mFrame, "[perf] Item needed a final reflow due to its subtree being dirty");
            return true;
        }

        // Cool; this item & its subtree haven't experienced any style/content
        // changes that would automatically require a reflow.

        // Did we cache the metrics from its most recent "final reflow"?
        let cache = CachedFlexItemData::Get(self.mFrame);
        let Some(cache) = cache else {
            FLEX_ITEM_LOG!(self.mFrame, "[perf] Item needed a final reflow due to lacking a cached mFinalReflowMetrics (maybe cache was cleared)");
            return true;
        };
        let Some(finalReflowMetrics) = &cache.mFinalReflowMetrics else {
            FLEX_ITEM_LOG!(self.mFrame, "[perf] Item needed a final reflow due to lacking a cached mFinalReflowMetrics (maybe cache was cleared)");
            return true;
        };

        // Does the cached size match our current size?
        if *finalReflowMetrics.Size() != finalSize {
            FLEX_ITEM_LOG!(self.mFrame, "[perf] Item needed a final reflow due to having a different content box size vs. its most recent final reflow");
            return true;
        }

        // Does the cached border and padding match our current ones?
        //
        // Note: this is just to detect cases where we have a percent padding whose
        // basis has changed. Any other sort of change to BorderPadding() (e.g. a new
        // specified value) should result in the frame being marked dirty via proper
        // change hint (see nsStylePadding::CalcDifference()), which will force it to
        // reflow.
        if *finalReflowMetrics.BorderPadding() != self.BorderPadding().ConvertTo(self.mWM, self.mCBWM) {
            FLEX_ITEM_LOG!(self.mFrame, "[perf] Item needed a final reflow due to having a different border and padding vs. its most recent final reflow");
            return true;
        }

        // The flex container is giving this flex item the same size that the item
        // had on its most recent "final reflow". But if its definiteness changed and
        // one of the descendants cares, then it would still need a reflow.
        if finalReflowMetrics.TreatBSizeAsIndefinite() != self.mTreatBSizeAsIndefinite && FrameHasRelativeBSizeDependency(self.mFrame) {
            FLEX_ITEM_LOG!(self.mFrame, "[perf] Item needed a final reflow due to having its BSize change definiteness & having a rel-BSize child");
            return true;
        }

        // If we get here, we can skip the final reflow! (The item's subtree isn't
        // dirty, and our current conditions are sufficiently similar to the most
        // recent "final reflow" that it should have left our subtree in the correct
        // state.)
        FLEX_ITEM_LOG!(self.mFrame, "[perf] Item didn't need a final reflow");
        false
    }
}

// layout/generic/nsFlexContainerFrame.cpp lines 2749-4632 at GECKO_REV (the position trackers through
// FlexLine::PositionItemsInCrossAxis).
//
// ADAPTER notes for this part:
//  - The C++ classes PositionTracker, MainAxisPositionTracker, CrossAxisPositionTracker and
//    SingleLineCrossAxisPositionTracker use inheritance: each subclass holds a `PositionTracker` in `mBase`
//    and derefs to it (CONVENTIONS.md, "Inheritance"). The derived classes' `= delete` redeclarations of the
//    frame-related methods (CrossAxisPositionTracker) are comments.
//  - The `ComputedFlexLineInfo* aLineInfo` parameters of FlexLine::FreezeItemsEarly and
//    ResolveFlexibleLengths are NOT PORTED (devtools): the parameter is dropped and the lines that use it are
//    left as comments.
//  - AuCoord64 is i64 (see the top of the file); AuCoord64::FromRound is AuCoord64FromRound below.

// ADAPTER: AuCoord64::FromRound (mozilla/gfx/Coord.h) rounds half up, like NSToCoordRound.
fn AuCoord64FromRound(aValue: f64) -> i64 {
    (aValue + 0.5).floor() as i64
}

// ADAPTER: AuCoord64::ToMinMaxClamped, clamped to the nscoord range.
fn AuCoord64ToMinMaxClamped(aValue: i64) -> nscoord {
    aValue.clamp(nscoord_MIN as i64, nscoord_MAX as i64) as nscoord
}

// Keeps track of our position along a particular axis (where a '0' position
// corresponds to the 'start' edge of that axis).
// This class shouldn't be instantiated directly -- rather, it should only be
// instantiated via its subclasses defined below.
// Gecko: layout/generic/nsFlexContainerFrame.cpp PositionTracker
pub struct PositionTracker {
    // Member data:
    // The position we're tracking.
    mPosition: nscoord,

    // The flex container's writing mode.
    mWM: WritingMode,

    // The axis along which we're moving.
    mAxis: LogicalAxis,

    // Is the axis along which we're moving reversed (e.g. LTR vs RTL) with
    // respect to the corresponding axis on the flex container's WM?
    mIsAxisReversed: bool,
}

impl PositionTracker {
    // Protected constructor, to be sure we're only instantiated via a subclass.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp PositionTracker::PositionTracker
    fn new(aWM: WritingMode, aAxis: LogicalAxis, aIsAxisReversed: bool) -> PositionTracker {
        PositionTracker { mPosition: 0, mWM: aWM, mAxis: aAxis, mIsAxisReversed: aIsAxisReversed }
    }

    // Accessor for the current value of the position that we're tracking.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp PositionTracker::Position
    pub fn Position(&self) -> nscoord {
        self.mPosition
    }
    // Gecko: layout/generic/nsFlexContainerFrame.cpp PositionTracker::Axis
    pub fn Axis(&self) -> LogicalAxis {
        self.mAxis
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp PositionTracker::StartSide
    pub fn StartSide(&self) -> LogicalSide {
        MakeLogicalSide(self.mAxis, if self.mIsAxisReversed { LogicalEdge::End } else { LogicalEdge::Start })
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp PositionTracker::EndSide
    pub fn EndSide(&self) -> LogicalSide {
        MakeLogicalSide(self.mAxis, if self.mIsAxisReversed { LogicalEdge::Start } else { LogicalEdge::End })
    }

    // Advances our position across the start edge of the given margin, in the
    // axis we're tracking.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp PositionTracker::EnterMargin
    pub fn EnterMargin(&mut self, aMargin: LogicalMargin) {
        self.mPosition += aMargin.Side(self.StartSide(), self.mWM);
    }

    // Advances our position across the end edge of the given margin, in the axis
    // we're tracking.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp PositionTracker::ExitMargin
    pub fn ExitMargin(&mut self, aMargin: LogicalMargin) {
        self.mPosition += aMargin.Side(self.EndSide(), self.mWM);
    }

    // Advances our current position from the start side of a child frame's
    // border-box to the frame's upper or left edge (depending on our axis).
    // (Note that this is a no-op if our axis grows in the same direction as
    // the corresponding logical axis.)
    // Gecko: layout/generic/nsFlexContainerFrame.cpp PositionTracker::EnterChildFrame
    pub fn EnterChildFrame(&mut self, aChildFrameSize: nscoord) {
        if self.mIsAxisReversed {
            self.mPosition += aChildFrameSize;
        }
    }

    // Advances our current position from a frame's upper or left border-box edge
    // (whichever is in the axis we're tracking) to the 'end' side of the frame
    // in the axis that we're tracking. (Note that this is a no-op if our axis
    // is reversed with respect to the corresponding logical axis.)
    // Gecko: layout/generic/nsFlexContainerFrame.cpp PositionTracker::ExitChildFrame
    pub fn ExitChildFrame(&mut self, aChildFrameSize: nscoord) {
        if !self.mIsAxisReversed {
            self.mPosition += aChildFrameSize;
        }
    }

    // Delete copy-constructor & reassignment operator, to prevent accidental
    // (unnecessary) copying.
    // (PositionTracker is not Clone here.)
}

// Tracks our position in the main axis, when we're laying out flex items.
// The "0" position represents the main-start edge of the flex container's
// content-box.
// Gecko: layout/generic/nsFlexContainerFrame.cpp MainAxisPositionTracker
pub struct MainAxisPositionTracker {
    mBase: PositionTracker,
    mPackingSpaceRemaining: nscoord,
    mNumAutoMarginsInMainAxis: u32,
    mNumPackingSpacesRemaining: u32,
    mJustifyContent: StyleContentDistribution,
}

impl std::ops::Deref for MainAxisPositionTracker {
    type Target = PositionTracker;
    fn deref(&self) -> &PositionTracker {
        &self.mBase
    }
}
impl std::ops::DerefMut for MainAxisPositionTracker {
    fn deref_mut(&mut self) -> &mut PositionTracker {
        &mut self.mBase
    }
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp MainAxisPositionTracker::~MainAxisPositionTracker
#[cfg(debug_assertions)]
impl Drop for MainAxisPositionTracker {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            MOZ_ASSERT!(self.mNumPackingSpacesRemaining == 0, "miscounted the number of packing spaces");
            MOZ_ASSERT!(self.mNumAutoMarginsInMainAxis == 0, "miscounted the number of auto margins");
        }
    }
}

impl MainAxisPositionTracker {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp MainAxisPositionTracker::MainAxisPositionTracker
    pub fn new(aAxisTracker: &FlexboxAxisTracker, aLine: &FlexLine, aJustifyContent: StyleContentDistribution, aContentBoxMainSize: nscoord) -> MainAxisPositionTracker {
        let mut this = MainAxisPositionTracker {
            mBase: PositionTracker::new(aAxisTracker.GetWritingMode(), aAxisTracker.MainAxis(), aAxisTracker.IsMainAxisReversed()),
            // we chip away at this below
            mPackingSpaceRemaining: aContentBoxMainSize,
            mNumAutoMarginsInMainAxis: 0,
            mNumPackingSpacesRemaining: 0,
            mJustifyContent: aJustifyContent,
        };
        // Extract the flag portion of mJustifyContent and strip off the flag bits
        // NOTE: This must happen before any assignment to mJustifyContent to
        // avoid overwriting the flag bits.
        let mut justifyContentFlags = this.mJustifyContent.primary() & StyleAlignFlags::FLAG_BITS;
        this.mJustifyContent = StyleContentDistribution::new(this.mJustifyContent.primary() & !StyleAlignFlags::FLAG_BITS);

        // 'normal' behaves as 'stretch', and 'stretch' behaves as 'flex-start',
        // in the main axis
        // https://drafts.csswg.org/css-align-3/#propdef-justify-content
        if this.mJustifyContent.primary() == StyleAlignFlags::NORMAL || this.mJustifyContent.primary() == StyleAlignFlags::STRETCH {
            this.mJustifyContent = StyleContentDistribution::new(StyleAlignFlags::FLEX_START);
        }

        // mPackingSpaceRemaining is initialized to the container's main size.  Now
        // we'll subtract out the main sizes of our flex items, so that it ends up
        // with the *actual* amount of packing space.
        for item in aLine.Items() {
            this.mPackingSpaceRemaining -= item.OuterMainSize();
            this.mNumAutoMarginsInMainAxis += item.NumAutoMarginsInMainAxis();
        }

        // Subtract space required for row/col gap from the remaining packing space
        this.mPackingSpaceRemaining -= aLine.SumOfGaps();

        // If packing space is negative or we only have one item, 'space-between'
        // falls back to 'safe flex-start'[1], and 'space-around' & 'space-evenly'
        // fall back to 'safe center'. In those cases, it's simplest to just pretend
        // we have a different 'justify-content' value and share code.
        // https://drafts.csswg.org/css-align-3/#distribution-values
        if this.mPackingSpaceRemaining < 0 || aLine.NumItems() == 1 {
            if this.mJustifyContent.primary() == StyleAlignFlags::SPACE_BETWEEN {
                // [1]  XXXdholbert Per spec, we should do...
                //   justifyContentFlags = StyleAlignFlags::SAFE;
                // ...here, but for now let's not do that since it causes overflow to be
                // inadvertently clipped in situations with a reversed main axis!
                // See https://github.com/w3c/csswg-drafts/issues/11937
                this.mJustifyContent = StyleContentDistribution::new(StyleAlignFlags::FLEX_START);
            } else if this.mJustifyContent.primary() == StyleAlignFlags::SPACE_AROUND || this.mJustifyContent.primary() == StyleAlignFlags::SPACE_EVENLY {
                justifyContentFlags = StyleAlignFlags::SAFE;
                this.mJustifyContent = StyleContentDistribution::new(StyleAlignFlags::CENTER);
            }
        }

        if this.mPackingSpaceRemaining <= 0 {
            // No available packing space to use for resolving auto margins.
            this.mNumAutoMarginsInMainAxis = 0;
            // If packing space is negative and <overflow-position> is set to 'safe'
            // all justify options fall back to 'start'
            if justifyContentFlags.contains(StyleAlignFlags::SAFE) {
                this.mJustifyContent = StyleContentDistribution::new(StyleAlignFlags::START);
            }
        }

        // Map 'left'/'right' to 'start'/'end'
        if this.mJustifyContent.primary() == StyleAlignFlags::LEFT || this.mJustifyContent.primary() == StyleAlignFlags::RIGHT {
            this.mJustifyContent = StyleContentDistribution::new(aAxisTracker.ResolveJustifyLeftRight(this.mJustifyContent.primary()));
        }

        // Map 'start'/'end' to 'flex-start'/'flex-end'.
        if this.mJustifyContent.primary() == StyleAlignFlags::START {
            this.mJustifyContent = StyleContentDistribution::new(if aAxisTracker.IsMainAxisReversed() { StyleAlignFlags::FLEX_END } else { StyleAlignFlags::FLEX_START });
        } else if this.mJustifyContent.primary() == StyleAlignFlags::END {
            this.mJustifyContent = StyleContentDistribution::new(if aAxisTracker.IsMainAxisReversed() { StyleAlignFlags::FLEX_START } else { StyleAlignFlags::FLEX_END });
        }

        // Figure out how much space we'll set aside for auto margins or
        // packing spaces, and advance past any leading packing-space.
        if this.mNumAutoMarginsInMainAxis == 0 && this.mPackingSpaceRemaining != 0 && !aLine.IsEmpty() {
            if this.mJustifyContent.primary() == StyleAlignFlags::FLEX_START {
                // All packing space should go at the end --> nothing to do here.
            } else if this.mJustifyContent.primary() == StyleAlignFlags::FLEX_END {
                // All packing space goes at the beginning
                this.mBase.mPosition += this.mPackingSpaceRemaining;
            } else if this.mJustifyContent.primary() == StyleAlignFlags::CENTER {
                // Half the packing space goes at the beginning
                this.mBase.mPosition += this.mPackingSpaceRemaining / 2;
            } else if this.mJustifyContent.primary() == StyleAlignFlags::SPACE_BETWEEN || this.mJustifyContent.primary() == StyleAlignFlags::SPACE_AROUND || this.mJustifyContent.primary() == StyleAlignFlags::SPACE_EVENLY {
                nsFlexContainerFrame::CalculatePackingSpace(aLine.NumItems(), this.mJustifyContent, &mut this.mBase.mPosition, &mut this.mNumPackingSpacesRemaining, &mut this.mPackingSpaceRemaining);
            } else {
                MOZ_ASSERT_UNREACHABLE!("Unexpected justify-content value");
            }
        }

        MOZ_ASSERT!(this.mNumPackingSpacesRemaining == 0 || this.mNumAutoMarginsInMainAxis == 0, "extra space should either go to packing space or to auto margins, but not to both");
        this
    }

    // Advances past the gap space (if any) between two flex items
    // Gecko: layout/generic/nsFlexContainerFrame.cpp MainAxisPositionTracker::TraverseGap
    pub fn TraverseGap(&mut self, aGapSize: nscoord) {
        self.mBase.mPosition += aGapSize;
    }

    // If aItem has any 'auto' margins in the main axis, this method updates the
    // corresponding values in its margin.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp MainAxisPositionTracker::ResolveAutoMarginsInMainAxis
    pub fn ResolveAutoMarginsInMainAxis(&mut self, aItem: &mut FlexItem) {
        if self.mNumAutoMarginsInMainAxis != 0 {
            let styleMargin = aItem.Frame().StyleMargin();
            let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aItem.Frame(), std::ptr::null_mut());
            for side in [self.StartSide(), self.EndSide()] {
                if styleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(side, self.mBase.mWM, &anchorResolutionParams).IsAuto() {
                    // NOTE: This integer math will skew the distribution of remainder
                    // app-units towards the end, which is fine.
                    let curAutoMarginSize: nscoord = self.mPackingSpaceRemaining / self.mNumAutoMarginsInMainAxis as nscoord;

                    MOZ_ASSERT!(aItem.GetMarginComponentForSide(side) == 0, "Expecting auto margins to have value '0' before we resolve them");
                    aItem.SetMarginComponentForSide(side, curAutoMarginSize);

                    self.mNumAutoMarginsInMainAxis -= 1;
                    self.mPackingSpaceRemaining -= curAutoMarginSize;
                }
            }
        }
    }

    // Advances past the packing space (if any) between two flex items
    // Gecko: layout/generic/nsFlexContainerFrame.cpp MainAxisPositionTracker::TraversePackingSpace
    pub fn TraversePackingSpace(&mut self) {
        if self.mNumPackingSpacesRemaining != 0 {
            MOZ_ASSERT!(
                self.mJustifyContent.primary() == StyleAlignFlags::SPACE_BETWEEN || self.mJustifyContent.primary() == StyleAlignFlags::SPACE_AROUND || self.mJustifyContent.primary() == StyleAlignFlags::SPACE_EVENLY,
                "mNumPackingSpacesRemaining only applies for space-between/space-around/space-evenly"
            );

            MOZ_ASSERT!(self.mPackingSpaceRemaining >= 0, "ran out of packing space earlier than we expected");

            // NOTE: This integer math will skew the distribution of remainder
            // app-units towards the end, which is fine.
            let curPackingSpace: nscoord = self.mPackingSpaceRemaining / self.mNumPackingSpacesRemaining as nscoord;

            self.mBase.mPosition += curPackingSpace;
            self.mNumPackingSpacesRemaining -= 1;
            self.mPackingSpaceRemaining -= curPackingSpace;
        }
    }
}

// Utility class for managing our position along the cross axis along
// the whole flex container (at a higher level than a single line).
// The "0" position represents the cross-start edge of the flex container's
// content-box.
// Gecko: layout/generic/nsFlexContainerFrame.cpp CrossAxisPositionTracker
pub struct CrossAxisPositionTracker {
    mBase: PositionTracker,
    mPackingSpaceRemaining: nscoord,
    mNumPackingSpacesRemaining: u32,
    mAlignContent: StyleContentDistribution,

    mCrossGapSize: nscoord,
}

impl std::ops::Deref for CrossAxisPositionTracker {
    type Target = PositionTracker;
    fn deref(&self) -> &PositionTracker {
        &self.mBase
    }
}
impl std::ops::DerefMut for CrossAxisPositionTracker {
    fn deref_mut(&mut self) -> &mut PositionTracker {
        &mut self.mBase
    }
}

impl CrossAxisPositionTracker {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CrossAxisPositionTracker::CrossAxisPositionTracker
    pub fn new(aLines: &mut Vec<FlexLine>, aReflowInput: &ReflowInput, aContentBoxCrossSize: nscoord, aIsCrossSizeDefinite: bool, aAxisTracker: &FlexboxAxisTracker, aCrossGapSize: nscoord) -> CrossAxisPositionTracker {
        let mut this = CrossAxisPositionTracker {
            mBase: PositionTracker::new(aAxisTracker.GetWritingMode(), aAxisTracker.CrossAxis(), aAxisTracker.IsCrossAxisReversed()),
            mPackingSpaceRemaining: 0,
            mNumPackingSpacesRemaining: 0,
            mAlignContent: aReflowInput.mStylePosition.mAlignContent(),
            mCrossGapSize: aCrossGapSize,
        };
        // Extract and strip the flag bits from alignContent
        let mut alignContentFlags = this.mAlignContent.primary() & StyleAlignFlags::FLAG_BITS;
        this.mAlignContent = StyleContentDistribution::new(this.mAlignContent.primary() & !StyleAlignFlags::FLAG_BITS);

        // 'normal' behaves as 'stretch'
        if this.mAlignContent.primary() == StyleAlignFlags::NORMAL {
            this.mAlignContent = StyleContentDistribution::new(StyleAlignFlags::STRETCH);
        }

        if IsSingleLine(aReflowInput.mFrame, &aReflowInput.mStylePosition) {
            MOZ_ASSERT!(aLines.len() == 1, "If we're styled as single-line, we should only have 1 line");
            // "If the flex container is single-line and has a definite cross size, the
            // cross size of the flex line is the flex container's inner cross size."
            //
            // SOURCE: https://drafts.csswg.org/css-flexbox/#algo-cross-line
            // NOTE: This means (by definition) that there's no packing space, which
            // means we don't need to be concerned with "align-content" at all and we
            // can return early. This is handy, because this is the usual case (for
            // single-line flexbox).
            if aIsCrossSizeDefinite {
                aLines[0].SetLineCrossSize(aContentBoxCrossSize);
                return this;
            }

            // "If the flex container is single-line, then clamp the line's
            // cross-size to be within the container's computed min and max cross-size
            // properties."
            let clamped = aReflowInput.ApplyMinMaxBSize(aLines[0].LineCrossSize(), 0);
            aLines[0].SetLineCrossSize(clamped);
        }

        // NOTE: The rest of this function should essentially match
        // MainAxisPositionTracker's constructor, though with FlexLines instead of
        // FlexItems, and with the additional value "stretch" (and of course with
        // cross sizes instead of main sizes.)

        // Figure out how much packing space we have (container's cross size minus
        // all the lines' cross sizes).  Also, share this loop to count how many
        // lines we have. (We need that count in some cases below.)
        this.mPackingSpaceRemaining = aContentBoxCrossSize;
        let mut numLines: u32 = 0;
        for line in aLines.iter() {
            this.mPackingSpaceRemaining -= line.LineCrossSize();
            numLines += 1;
        }

        // Subtract space required for row/col gap from the remaining packing space
        MOZ_ASSERT!(numLines >= 1, "GenerateFlexLines should've produced at least 1 line");
        this.mPackingSpaceRemaining -= aCrossGapSize * (numLines as nscoord - 1);

        // If packing space is negative, 'stretch' behaves like 'flex-start',
        // 'space-between' behaves like 'safe flex-start', and 'space-around' and
        // 'space-evenly' behave like 'safe center'. In those cases, it's simplest to
        // just pretend we have a different 'align-content' value and share code. (If
        // we only have one line, all of the 'space-*' keywords fall back as well, but
        // 'stretch' doesn't because even a single line can still stretch.)
        // https://drafts.csswg.org/css-align-3/#distribution-values
        if this.mPackingSpaceRemaining < 0 && this.mAlignContent.primary() == StyleAlignFlags::STRETCH {
            this.mAlignContent = StyleContentDistribution::new(StyleAlignFlags::FLEX_START);
        } else if this.mPackingSpaceRemaining < 0 || numLines == 1 {
            if this.mAlignContent.primary() == StyleAlignFlags::SPACE_BETWEEN {
                alignContentFlags = StyleAlignFlags::SAFE;
                this.mAlignContent = StyleContentDistribution::new(StyleAlignFlags::FLEX_START);
            } else if this.mAlignContent.primary() == StyleAlignFlags::SPACE_AROUND || this.mAlignContent.primary() == StyleAlignFlags::SPACE_EVENLY {
                alignContentFlags = StyleAlignFlags::SAFE;
                this.mAlignContent = StyleContentDistribution::new(StyleAlignFlags::CENTER);
            }
        }

        // If <overflow-position> is 'safe' and packing space is negative
        // all align options fall back to 'start'
        if alignContentFlags.contains(StyleAlignFlags::SAFE) && this.mPackingSpaceRemaining < 0 {
            this.mAlignContent = StyleContentDistribution::new(StyleAlignFlags::START);
        }

        // Map 'start'/'end' to 'flex-start'/'flex-end'.
        if this.mAlignContent.primary() == StyleAlignFlags::START {
            this.mAlignContent = StyleContentDistribution::new(if aAxisTracker.IsCrossAxisReversed() { StyleAlignFlags::FLEX_END } else { StyleAlignFlags::FLEX_START });
        } else if this.mAlignContent.primary() == StyleAlignFlags::END {
            this.mAlignContent = StyleContentDistribution::new(if aAxisTracker.IsCrossAxisReversed() { StyleAlignFlags::FLEX_START } else { StyleAlignFlags::FLEX_END });
        }

        // Figure out how much space we'll set aside for packing spaces, and advance
        // past any leading packing-space.
        if this.mPackingSpaceRemaining != 0 {
            if this.mAlignContent.primary() == StyleAlignFlags::BASELINE || this.mAlignContent.primary() == StyleAlignFlags::LAST_BASELINE {
                // TODO: Bug 1480850 will implement 'align-content: [first/last] baseline'
                // for flexbox. Until then, behaves as if align-content is 'flex-start' by
                // doing nothing.
            } else if this.mAlignContent.primary() == StyleAlignFlags::FLEX_START {
                // All packing space should go at the end --> nothing to do here.
            } else if this.mAlignContent.primary() == StyleAlignFlags::FLEX_END {
                // All packing space goes at the beginning
                this.mBase.mPosition += this.mPackingSpaceRemaining;
            } else if this.mAlignContent.primary() == StyleAlignFlags::CENTER {
                // Half the packing space goes at the beginning
                this.mBase.mPosition += this.mPackingSpaceRemaining / 2;
            } else if this.mAlignContent.primary() == StyleAlignFlags::SPACE_BETWEEN || this.mAlignContent.primary() == StyleAlignFlags::SPACE_AROUND || this.mAlignContent.primary() == StyleAlignFlags::SPACE_EVENLY {
                nsFlexContainerFrame::CalculatePackingSpace(numLines, this.mAlignContent, &mut this.mBase.mPosition, &mut this.mNumPackingSpacesRemaining, &mut this.mPackingSpaceRemaining);
            } else if this.mAlignContent.primary() == StyleAlignFlags::STRETCH {
                // Split space equally between the lines:
                MOZ_ASSERT!(this.mPackingSpaceRemaining > 0, "negative packing space should make us use 'flex-start' instead of 'stretch' (and we shouldn't bother with this code if we have 0 packing space)");

                let mut numLinesLeft = numLines;
                for line in aLines.iter_mut() {
                    // Our share is the amount of space remaining, divided by the number
                    // of lines remainig.
                    MOZ_ASSERT!(numLinesLeft > 0, "miscalculated num lines");
                    let shareOfExtraSpace: nscoord = this.mPackingSpaceRemaining / numLinesLeft as nscoord;
                    let newSize: nscoord = line.LineCrossSize() + shareOfExtraSpace;
                    line.SetLineCrossSize(newSize);

                    this.mPackingSpaceRemaining -= shareOfExtraSpace;
                    numLinesLeft -= 1;
                }
                MOZ_ASSERT!(numLinesLeft == 0, "miscalculated num lines");
            } else {
                MOZ_ASSERT_UNREACHABLE!("Unexpected align-content value");
            }
        }
        this
    }

    // Advances past the gap (if any) between two flex lines
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CrossAxisPositionTracker::TraverseGap
    pub fn TraverseGap(&mut self) {
        self.mBase.mPosition += self.mCrossGapSize;
    }

    // Advances past the given FlexLine
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CrossAxisPositionTracker::TraverseLine
    pub fn TraverseLine(&mut self, aLine: &FlexLine) {
        self.mBase.mPosition += aLine.LineCrossSize();
    }

    // Redeclare the frame-related methods from PositionTracker with
    // = delete, to be sure (at compile time) that no client code can invoke
    // them. (Unlike the other PositionTracker derived classes, this class here
    // deals with FlexLines, not with individual FlexItems or frames.)
    //   void EnterMargin(const LogicalMargin& aMargin) = delete;
    //   void ExitMargin(const LogicalMargin& aMargin) = delete;
    //   void EnterChildFrame(nscoord aChildFrameSize) = delete;
    //   void ExitChildFrame(nscoord aChildFrameSize) = delete;
    // (Rust cannot delete an inherited method; callers simply must not call them.)

    // Advances past the packing space (if any) between two flex lines
    // Gecko: layout/generic/nsFlexContainerFrame.cpp CrossAxisPositionTracker::TraversePackingSpace
    pub fn TraversePackingSpace(&mut self) {
        if self.mNumPackingSpacesRemaining != 0 {
            MOZ_ASSERT!(
                self.mAlignContent.primary() == StyleAlignFlags::SPACE_BETWEEN || self.mAlignContent.primary() == StyleAlignFlags::SPACE_AROUND || self.mAlignContent.primary() == StyleAlignFlags::SPACE_EVENLY,
                "mNumPackingSpacesRemaining only applies for space-between/space-around/space-evenly"
            );

            MOZ_ASSERT!(self.mPackingSpaceRemaining >= 0, "ran out of packing space earlier than we expected");

            // NOTE: This integer math will skew the distribution of remainder
            // app-units towards the end, which is fine.
            let curPackingSpace: nscoord = self.mPackingSpaceRemaining / self.mNumPackingSpacesRemaining as nscoord;

            self.mBase.mPosition += curPackingSpace;
            self.mNumPackingSpacesRemaining -= 1;
            self.mPackingSpaceRemaining -= curPackingSpace;
        }
    }
}

// Utility class for managing our position along the cross axis, *within* a
// single flex line.
// Gecko: layout/generic/nsFlexContainerFrame.cpp SingleLineCrossAxisPositionTracker
pub struct SingleLineCrossAxisPositionTracker {
    mBase: PositionTracker,
}

impl std::ops::Deref for SingleLineCrossAxisPositionTracker {
    type Target = PositionTracker;
    fn deref(&self) -> &PositionTracker {
        &self.mBase
    }
}
impl std::ops::DerefMut for SingleLineCrossAxisPositionTracker {
    fn deref_mut(&mut self) -> &mut PositionTracker {
        &mut self.mBase
    }
}

impl SingleLineCrossAxisPositionTracker {
    // Resets our position to the cross-start edge of this line.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp SingleLineCrossAxisPositionTracker::ResetPosition
    pub fn ResetPosition(&mut self) {
        self.mBase.mPosition = 0;
    }
}

//----------------------------------------------------------------------

// Frame class boilerplate
// =======================

// (NS_QUERYFRAME_HEAD, NS_IMPL_FRAMEARENA_HELPERS, NS_NewFlexContainerFrame and the destructor are with
// the top of the file: do_QueryFrame, NS_NewFlexContainerFrame.)

//----------------------------------------------------------------------

// nsFlexContainerFrame Method Implementations
// ===========================================

impl nsFlexContainerFrame {
    /* virtual */
    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::Init
    // ADAPTER: the frame is created by FrameTree::AddFrame (see nsBlockFrame::Init).
    pub fn Init(&self, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: Option<nsIFrame>) {
        self.nsContainerFrame_Init(aContent, aParent, aPrevInFlow);

        // if (HasAnyStateBits(NS_FRAME_FONT_INFLATION_CONTAINER)) { AddStateBits(NS_FRAME_FONT_INFLATION_FLOW_ROOT); }
        // NOT PORTED: font inflation.

        // auto displayInside = StyleDisplay()->DisplayInside();
        // If this frame is for a scrollable element, then it will actually have
        // "display:block", and its *parent frame* will have the real
        // flex-flavored display value. So in that case, check the parent frame to
        // find out if we're legacy.
        //
        // TODO(emilio): Maybe ::-moz-scrolled-content and co should inherit `display`
        // (or a blockified version thereof, to not hit bug 456484).
        // if (displayInside == StyleDisplayInside::Flow) { ...; displayInside = GetParent()->StyleDisplay()->DisplayInside(); }
        // if (displayInside == StyleDisplayInside::WebkitBox) { AddStateBits(NS_STATE_FLEX_IS_EMULATING_LEGACY_WEBKIT_BOX); }
        // NOT PORTED: legacy -webkit-box emulation (there is no `-webkit-box` display in this Stylo build, and a
        // flex container is never the inner part of a scrollable element here: scroll frames are not ported).
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::GetFrameName
    // NOT PORTED: debug frame dump (DEBUG_FRAME_DUMP).

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::BuildDisplayList
    // NOT PORTED: painting (display lists). nsFlexContainerFrame.cpp lines 2977-3010 at GECKO_REV.
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::FreezeItemsEarly
// NOT PORTED: aLineInfo (devtools).
impl FlexLine {
    fn FreezeItemsEarly(&mut self, aIsUsingFlexGrow: bool) {
        // After we've established the type of flexing we're doing (growing vs.
        // shrinking), and before we try to flex any items, we freeze items that
        // obviously *can't* flex.
        //
        // Quoting the spec:
        //  # Freeze, setting its target main size to its hypothetical main size...
        //  #  - any item that has a flex factor of zero
        //  #  - if using the flex grow factor: any item that has a flex base size
        //  #    greater than its hypothetical main size
        //  #  - if using the flex shrink factor: any item that has a flex base size
        //  #    smaller than its hypothetical main size
        //  https://drafts.csswg.org/css-flexbox/#resolve-flexible-lengths
        //
        // (NOTE: At this point, item->MainSize() *is* the item's hypothetical
        // main size, since SetFlexBaseSizeAndMainSize() sets it up that way, and the
        // item hasn't had a chance to flex away from that yet.)

        // Since this loop only operates on unfrozen flex items, we can break as
        // soon as we have seen all of them.
        let mut numUnfrozenItemsToBeSeen: u32 = self.NumItems() - self.mNumFrozenItems;
        for item in self.mItems.iter_mut() {
            if numUnfrozenItemsToBeSeen == 0 {
                break;
            }

            if !item.IsFrozen() {
                numUnfrozenItemsToBeSeen -= 1;
                let mut shouldFreeze = 0.0 == item.GetFlexFactor(aIsUsingFlexGrow);
                if !shouldFreeze {
                    if aIsUsingFlexGrow {
                        if item.FlexBaseSize() > item.MainSize() {
                            shouldFreeze = true;
                        }
                    } else {
                        // using flex-shrink
                        if item.FlexBaseSize() < item.MainSize() {
                            shouldFreeze = true;
                        }
                    }
                }
                if shouldFreeze {
                    // Freeze item! (at its hypothetical main size)
                    item.Freeze();
                    if item.FlexBaseSize() < item.MainSize() {
                        item.SetWasMinClamped();
                    } else if item.FlexBaseSize() > item.MainSize() {
                        item.SetWasMaxClamped();
                    }
                    self.mNumFrozenItems += 1;
                }
            }
        }

        MOZ_ASSERT!(numUnfrozenItemsToBeSeen == 0, "miscounted frozen items?");
    }

    // Based on the sign of aTotalViolation, this function freezes a subset of our
    // flexible sizes, and restores the remaining ones to their initial pref sizes.
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::FreezeOrRestoreEachFlexibleSize
    fn FreezeOrRestoreEachFlexibleSize(&mut self, aTotalViolation: nscoord, aIsFinalIteration: bool) {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum FreezeType {
            eFreezeEverything,
            eFreezeMinViolations,
            eFreezeMaxViolations,
        }
        use FreezeType::*;

        let freezeType: FreezeType;
        if aTotalViolation == 0 {
            freezeType = eFreezeEverything;
        } else if aTotalViolation > 0 {
            freezeType = eFreezeMinViolations;
        } else {
            // aTotalViolation < 0
            freezeType = eFreezeMaxViolations;
        }

        // Since this loop only operates on unfrozen flex items, we can break as
        // soon as we have seen all of them.
        let mut numUnfrozenItemsToBeSeen: u32 = self.NumItems() - self.mNumFrozenItems;
        for item in self.mItems.iter_mut() {
            if numUnfrozenItemsToBeSeen == 0 {
                break;
            }

            if !item.IsFrozen() {
                numUnfrozenItemsToBeSeen -= 1;

                MOZ_ASSERT!(!item.HadMinViolation() || !item.HadMaxViolation(), "Can have either min or max violation, but not both");

                let hadMinViolation = item.HadMinViolation();
                let hadMaxViolation = item.HadMaxViolation();
                if eFreezeEverything == freezeType || (eFreezeMinViolations == freezeType && hadMinViolation) || (eFreezeMaxViolations == freezeType && hadMaxViolation) {
                    MOZ_ASSERT!(item.MainSize() >= item.MainMinSize(), "Freezing item at a size below its minimum");
                    MOZ_ASSERT!(item.MainSize() <= item.MainMaxSize(), "Freezing item at a size above its maximum");

                    item.Freeze();
                    if hadMinViolation {
                        item.SetWasMinClamped();
                    } else if hadMaxViolation {
                        item.SetWasMaxClamped();
                    }
                    self.mNumFrozenItems += 1;
                } else if aIsFinalIteration {
                    // XXXdholbert If & when bug 765861 is fixed, we should upgrade this
                    // assertion to be fatal except in documents with enormous lengths.
                    NS_ERROR!("Final iteration still has unfrozen items, this shouldn't happen unless there was nscoord under/overflow.");
                    item.Freeze();
                    self.mNumFrozenItems += 1;
                } // else, we'll reset this item's main size to its flex base size on the
                  // next iteration of this algorithm.

                if !item.IsFrozen() {
                    // Clear this item's violation(s), now that we've dealt with them
                    item.ClearViolationFlags();
                }
            }
        }

        MOZ_ASSERT!(numUnfrozenItemsToBeSeen == 0, "miscounted frozen items?");
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::ResolveFlexibleLengths
    // NOT PORTED: aLineInfo (devtools).
    pub fn ResolveFlexibleLengths(&mut self, aFlexContainerMainSize: nscoord) {
        // In this function, we use 64-bit coord type to avoid integer overflow in
        // case several of the individual items have huge hypothetical main sizes,
        // which can happen with percent-width table-layout:fixed descendants. Here we
        // promote the container's main size to 64-bit to make the arithmetic
        // convenient.
        let flexContainerMainSize: i64 = aFlexContainerMainSize as i64;

        // Before we start resolving sizes: if we have an aLineInfo structure to fill
        // out, we inform it of each item's base size, and we initialize the "delta"
        // for each item to 0. (And if the flex algorithm wants to grow or shrink the
        // item, we'll update this delta further down.)
        // NOT PORTED: aLineInfo (devtools):
        //   if (aLineInfo) { uint32_t itemIndex = 0; for (FlexItem& item : Items()) {
        //     aLineInfo->mItems[itemIndex].mMainBaseSize = item.FlexBaseSize();
        //     aLineInfo->mItems[itemIndex].mMainDeltaSize = 0; ++itemIndex; } }

        // Determine whether we're going to be growing or shrinking items.
        let isUsingFlexGrow = self.mTotalOuterHypotheticalMainSize < flexContainerMainSize;

        // NOT PORTED: aLineInfo (devtools):
        //   if (aLineInfo) { aLineInfo->mGrowthState = isUsingFlexGrow ? Growing : Shrinking; }

        // Do an "early freeze" for flex items that obviously can't flex in the
        // direction we've chosen:
        self.FreezeItemsEarly(isUsingFlexGrow);

        if self.mNumFrozenItems == self.NumItems() {
            // All our items are frozen, so we have no flexible lengths to resolve,
            // and we aren't being asked to generate computed line info.
            FLEX_LOG!("No flexible length to resolve");
            return;
        }
        MOZ_ASSERT!(!self.IsEmpty(), "empty lines should take the early-return above");

        FLEX_LOG!("Resolving flexible lengths for items");

        // Subtract space occupied by our items' margins/borders/padding/gaps, so
        // we can just be dealing with the space available for our flex items' content
        // boxes.
        let totalItemMBPAndGaps: i64 = self.mTotalItemMBP as i64 + self.SumOfGaps() as i64;
        let spaceAvailableForFlexItemsContentBoxes: i64 = flexContainerMainSize - totalItemMBPAndGaps;

        let mut origAvailableFreeSpace: Option<i64> = None;

        // NOTE: I claim that this chunk of the algorithm (the looping part) needs to
        // run the loop at MOST NumItems() times.  This claim should hold up
        // because we'll freeze at least one item on each loop iteration, and once
        // we've run out of items to freeze, there's nothing left to do.  However,
        // in most cases, we'll break out of this loop long before we hit that many
        // iterations.
        for iterationCounter in 0..self.NumItems() {
            // Set every not-yet-frozen item's used main size to its
            // flex base size, and subtract all the used main sizes from our
            // total amount of space to determine the 'available free space'
            // (positive or negative) to be distributed among our flexible items.
            let mut availableFreeSpace: i64 = spaceAvailableForFlexItemsContentBoxes;
            for item in self.mItems.iter_mut() {
                if !item.IsFrozen() {
                    item.SetMainSize(item.FlexBaseSize());
                }
                availableFreeSpace -= item.MainSize() as i64;
            }

            FLEX_LOGV!("Available free space: %d; flex items should \"%s\"", availableFreeSpace, if isUsingFlexGrow { "grow" } else { "shrink" });

            // The sign of our free space should agree with the type of flexing
            // (grow/shrink) that we're doing. Any disagreement should've made us use
            // the other type of flexing, or should've been resolved in
            // FreezeItemsEarly.
            //
            // Note: it's possible that an individual flex item has huge
            // margin/border/padding that makes either its
            // MarginBorderPaddingSizeInMainAxis() or OuterMainSize() negative due to
            // integer overflow. If that happens, the accumulated
            // mTotalOuterHypotheticalMainSize or mTotalItemMBP could be negative due to
            // that one item's negative (overflowed) size. Likewise, a huge main gap
            // size between flex items can also make our accumulated SumOfGaps()
            // negative. In these case, we throw up our hands and don't require
            // isUsingFlexGrow to agree with availableFreeSpace. Luckily, we won't get
            // stuck in the algorithm below, and just distribute the wrong
            // availableFreeSpace with the wrong grow/shrink factors.
            MOZ_ASSERT!(
                !(self.mTotalOuterHypotheticalMainSize >= 0 && self.mTotalItemMBP >= 0 && totalItemMBPAndGaps >= 0) || (isUsingFlexGrow && availableFreeSpace >= 0) || (!isUsingFlexGrow && availableFreeSpace <= 0),
                "availableFreeSpace's sign should match isUsingFlexGrow"
            );

            // If we have any free space available, give each flexible item a portion
            // of availableFreeSpace.
            if availableFreeSpace != 0 {
                // The first time we do this, we initialize origAvailableFreeSpace.
                if origAvailableFreeSpace.is_none() {
                    origAvailableFreeSpace = Some(availableFreeSpace);
                }

                // STRATEGY: On each item, we compute & store its "share" of the total
                // weight that we've seen so far:
                //   curWeight / weightSum
                //
                // Then, when we go to actually distribute the space (in the next loop),
                // we can simply walk backwards through the elements and give each item
                // its "share" multiplied by the remaining available space.
                //
                // SPECIAL CASE: If the sum of the weights is larger than the
                // maximum representable double (overflowing to infinity), then we can't
                // sensibly divide out proportional shares anymore. In that case, we
                // simply treat the flex item(s) with the largest weights as if
                // their weights were infinite (dwarfing all the others), and we
                // distribute all of the available space among them.
                let mut weightSum: f64 = 0.0;
                let mut flexFactorSum: f64 = 0.0;
                let mut largestWeight: f64 = 0.0;
                let mut numItemsWithLargestWeight: u32 = 0;

                // Since this loop only operates on unfrozen flex items, we can break as
                // soon as we have seen all of them.
                let mut numUnfrozenItemsToBeSeen: u32 = self.NumItems() - self.mNumFrozenItems;
                for item in self.mItems.iter_mut() {
                    if numUnfrozenItemsToBeSeen == 0 {
                        break;
                    }

                    if !item.IsFrozen() {
                        numUnfrozenItemsToBeSeen -= 1;

                        let curWeight: f64 = item.GetWeight(isUsingFlexGrow) as f64;
                        let curFlexFactor: f64 = item.GetFlexFactor(isUsingFlexGrow) as f64;
                        MOZ_ASSERT!(curWeight >= 0.0, "weights are non-negative");
                        MOZ_ASSERT!(curFlexFactor >= 0.0, "flex factors are non-negative");

                        weightSum += curWeight;
                        flexFactorSum += curFlexFactor;

                        if weightSum.is_finite() {
                            if curWeight == 0.0 {
                                item.SetShareOfWeightSoFar(0.0);
                            } else {
                                item.SetShareOfWeightSoFar(curWeight / weightSum);
                            }
                        } // else, the sum of weights overflows to infinity, in which
                          // case we don't bother with "SetShareOfWeightSoFar" since
                          // we know we won't use it. (instead, we'll just give every
                          // item with the largest weight an equal share of space.)

                        // Update our largest-weight tracking vars
                        if curWeight > largestWeight {
                            largestWeight = curWeight;
                            numItemsWithLargestWeight = 1;
                        } else if curWeight == largestWeight {
                            numItemsWithLargestWeight += 1;
                        }
                    }
                }

                MOZ_ASSERT!(numUnfrozenItemsToBeSeen == 0, "miscounted frozen items?");

                if weightSum != 0.0 {
                    MOZ_ASSERT!(flexFactorSum != 0.0, "flex factor sum can't be 0, if a weighted sum of its components (weightSum) is nonzero");
                    if flexFactorSum < 1.0 {
                        // Our unfrozen flex items don't want all of the original free space!
                        // (Their flex factors add up to something less than 1.)
                        // Hence, make sure we don't distribute any more than the portion of
                        // our original free space that these items actually want.
                        let totalDesiredPortionOfOrigFreeSpace: i64 = AuCoord64FromRound(origAvailableFreeSpace.unwrap() as f64 * flexFactorSum);

                        // Clamp availableFreeSpace to be no larger than that ^^.
                        // (using min or max, depending on sign).
                        // This should not change the sign of availableFreeSpace (except
                        // possibly by setting it to 0), as enforced by this assertion:
                        NS_ASSERTION!(
                            totalDesiredPortionOfOrigFreeSpace == 0 || ((totalDesiredPortionOfOrigFreeSpace > 0) == (availableFreeSpace > 0)),
                            "When we reduce available free space for flex factors < 1, we shouldn't change the sign of the free space..."
                        );

                        if availableFreeSpace > 0 {
                            availableFreeSpace = std::cmp::min(availableFreeSpace, totalDesiredPortionOfOrigFreeSpace);
                        } else {
                            availableFreeSpace = std::cmp::max(availableFreeSpace, totalDesiredPortionOfOrigFreeSpace);
                        }
                    }

                    FLEX_LOGV!("Distributing available space:");
                    // Since this loop only operates on unfrozen flex items, we can break as
                    // soon as we have seen all of them.
                    numUnfrozenItemsToBeSeen = self.NumItems() - self.mNumFrozenItems;

                    // NOTE: It's important that we traverse our items in *reverse* order
                    // here, for correct width distribution according to the items'
                    // "ShareOfWeightSoFar" progressively-calculated values.
                    for item in self.mItems.iter_mut().rev() {
                        if numUnfrozenItemsToBeSeen == 0 {
                            break;
                        }

                        if !item.IsFrozen() {
                            numUnfrozenItemsToBeSeen -= 1;

                            // To avoid rounding issues, we compute the change in size for this
                            // item, and then subtract it from the remaining available space.
                            let mut sizeDelta: i64 = 0;
                            if weightSum.is_finite() {
                                let myShareOfRemainingSpace: f64 = item.ShareOfWeightSoFar();

                                MOZ_ASSERT!(myShareOfRemainingSpace >= 0.0 && myShareOfRemainingSpace <= 1.0, "my share should be nonnegative fractional amount");

                                if myShareOfRemainingSpace == 1.0 {
                                    // (We special-case 1.0 to avoid float error from converting
                                    // availableFreeSpace from integer*1.0 --> double --> integer)
                                    sizeDelta = availableFreeSpace;
                                } else if myShareOfRemainingSpace > 0.0 {
                                    sizeDelta = AuCoord64FromRound(availableFreeSpace as f64 * myShareOfRemainingSpace);
                                }
                            } else if item.GetWeight(isUsingFlexGrow) as f64 == largestWeight {
                                // Total flexibility is infinite, so we're just distributing
                                // the available space equally among the items that are tied for
                                // having the largest weight (and this is one of those items).
                                sizeDelta = AuCoord64FromRound(availableFreeSpace as f64 / numItemsWithLargestWeight as f64);
                                numItemsWithLargestWeight -= 1;
                            }

                            availableFreeSpace -= sizeDelta;

                            item.SetMainSize(item.MainSize() + AuCoord64ToMinMaxClamped(sizeDelta));
                            FLEX_LOGV!("  Flex item %p receives %d, for a total of %d", item.Frame(), sizeDelta, item.MainSize());
                        }
                    }

                    MOZ_ASSERT!(numUnfrozenItemsToBeSeen == 0, "miscounted frozen items?");

                    // If we have an aLineInfo structure to fill out, capture any
                    // size changes that may have occurred in the previous loop.
                    // NOT PORTED: aLineInfo (devtools): nsFlexContainerFrame.cpp lines 3396-3420 at GECKO_REV.
                }
            }

            // Fix min/max violations:
            let mut totalViolation: nscoord = 0; // keeps track of adjustments for min/max
            FLEX_LOGV!("Checking for violations:");

            // Since this loop only operates on unfrozen flex items, we can break as
            // soon as we have seen all of them.
            let mut numUnfrozenItemsToBeSeen: u32 = self.NumItems() - self.mNumFrozenItems;
            for item in self.mItems.iter_mut() {
                if numUnfrozenItemsToBeSeen == 0 {
                    break;
                }

                if !item.IsFrozen() {
                    numUnfrozenItemsToBeSeen -= 1;

                    if item.MainSize() < item.MainMinSize() {
                        // min violation
                        totalViolation += item.MainMinSize() - item.MainSize();
                        item.SetMainSize(item.MainMinSize());
                        item.SetHadMinViolation();
                    } else if item.MainSize() > item.MainMaxSize() {
                        // max violation
                        totalViolation += item.MainMaxSize() - item.MainSize();
                        item.SetMainSize(item.MainMaxSize());
                        item.SetHadMaxViolation();
                    }
                }
            }

            MOZ_ASSERT!(numUnfrozenItemsToBeSeen == 0, "miscounted frozen items?");

            self.FreezeOrRestoreEachFlexibleSize(totalViolation, iterationCounter + 1 == self.NumItems());

            FLEX_LOGV!("Total violation: %d", totalViolation);

            if self.mNumFrozenItems == self.NumItems() {
                break;
            }

            MOZ_ASSERT!(totalViolation != 0, "Zero violation should've made us freeze all items & break");
        }

        #[cfg(debug_assertions)]
        {
            // Post-condition: all items should've been frozen.
            // Make sure the counts match:
            MOZ_ASSERT!(self.mNumFrozenItems == self.NumItems(), "All items should be frozen");

            // For good measure, check each item directly, in case our counts are busted:
            for item in self.Items() {
                MOZ_ASSERT!(item.IsFrozen(), "All items should be frozen");
            }
        }
    }
}

impl SingleLineCrossAxisPositionTracker {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp SingleLineCrossAxisPositionTracker::SingleLineCrossAxisPositionTracker
    pub fn new(aAxisTracker: &FlexboxAxisTracker) -> SingleLineCrossAxisPositionTracker {
        SingleLineCrossAxisPositionTracker { mBase: PositionTracker::new(aAxisTracker.GetWritingMode(), aAxisTracker.CrossAxis(), aAxisTracker.IsCrossAxisReversed()) }
    }
}

impl FlexLine {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::ComputeCrossSizeAndBaseline
    pub fn ComputeCrossSizeAndBaseline(&mut self, aAxisTracker: &FlexboxAxisTracker) {
        // NOTE: in these "cross{Start,End}ToFurthest{First,Last}Baseline" variables,
        // the "first/last" term is referring to the flex *line's* baseline-sharing
        // groups, which may or may not match any flex *item's* exact align-self
        // value. See the code that sets FlexItem::mBaselineSharingGroup for more
        // details.
        let mut crossStartToFurthestFirstBaseline: nscoord = nscoord_MIN;
        let mut crossEndToFurthestFirstBaseline: nscoord = nscoord_MIN;
        let mut crossStartToFurthestLastBaseline: nscoord = nscoord_MIN;
        let mut crossEndToFurthestLastBaseline: nscoord = nscoord_MIN;

        let mut largestOuterCrossSize: nscoord = 0;
        for item in self.Items() {
            let curOuterCrossSize: nscoord = item.OuterCrossSize();

            if (item.AlignSelf() == StyleAlignFlags::BASELINE || item.AlignSelf() == StyleAlignFlags::LAST_BASELINE) && item.NumAutoMarginsInCrossAxis() == 0 {
                let usingItemFirstBaseline = item.AlignSelf() == StyleAlignFlags::BASELINE;

                // Find distance from our item's cross-start and cross-end margin-box
                // edges to its baseline.
                //
                // Here's a diagram of a flex-item that we might be doing this on.
                // "mmm" is the margin-box, "bbb" is the border-box. The bottom of
                // the text "BASE" is the baseline.
                //
                // ---(cross-start)---
                //                ___              ___            ___
                //   mmmmmmmmmmmm  |                |margin-start  |
                //   m          m  |               _|_   ___       |
                //   m bbbbbbbb m  |curOuterCrossSize     |        |crossStartToBaseline
                //   m b      b m  |                      |ascent  |
                //   m b BASE b m  |                     _|_      _|_
                //   m b      b m  |                               |
                //   m bbbbbbbb m  |                               |crossEndToBaseline
                //   m          m  |                               |
                //   mmmmmmmmmmmm _|_                             _|_
                //
                // ---(cross-end)---
                //
                // We already have the curOuterCrossSize, margin-start, and the ascent.
                // * We can get crossStartToBaseline by adding margin-start + ascent.
                // * If we subtract that from the curOuterCrossSize, we get
                //   crossEndToBaseline.

                let crossStartToBaseline: nscoord = item.BaselineOffsetFromOuterCrossEdge(aAxisTracker.CrossAxisPhysicalStartSide(), usingItemFirstBaseline);
                let crossEndToBaseline: nscoord = curOuterCrossSize - crossStartToBaseline;

                // Now, update our "largest" values for these (across all the flex items
                // in this flex line), so we can use them in computing the line's cross
                // size below:
                if item.ItemBaselineSharingGroup() == BaselineSharingGroup::First {
                    crossStartToFurthestFirstBaseline = std::cmp::max(crossStartToFurthestFirstBaseline, crossStartToBaseline);
                    crossEndToFurthestFirstBaseline = std::cmp::max(crossEndToFurthestFirstBaseline, crossEndToBaseline);
                } else {
                    crossStartToFurthestLastBaseline = std::cmp::max(crossStartToFurthestLastBaseline, crossStartToBaseline);
                    crossEndToFurthestLastBaseline = std::cmp::max(crossEndToFurthestLastBaseline, crossEndToBaseline);
                }
            } else {
                largestOuterCrossSize = std::cmp::max(largestOuterCrossSize, curOuterCrossSize);
            }
        }

        // The line's baseline offset is the distance from the line's edge to the
        // furthest item-baseline. The item(s) with that baseline will be exactly
        // aligned with the line's edge.
        self.mFirstBaselineOffset = crossStartToFurthestFirstBaseline;
        self.mLastBaselineOffset = crossEndToFurthestLastBaseline;

        // The line's cross-size is the larger of:
        //  (a) [largest cross-start-to-baseline + largest baseline-to-cross-end] of
        //      all baseline-aligned items with no cross-axis auto margins...
        // and
        //  (b) [largest cross-start-to-baseline + largest baseline-to-cross-end] of
        //      all last baseline-aligned items with no cross-axis auto margins...
        // and
        //  (c) largest cross-size of all other children.
        // (The sums of two nscoord_MIN values stay inside i32 here, as in C++ where nscoord_MIN is -2^30.)
        self.mLineCrossSize = std::cmp::max(std::cmp::max(crossStartToFurthestFirstBaseline + crossEndToFurthestFirstBaseline, crossStartToFurthestLastBaseline + crossEndToFurthestLastBaseline), largestOuterCrossSize);
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::ExtractBaselineOffset
    pub fn ExtractBaselineOffset(&self, aBaselineGroup: BaselineSharingGroup) -> nscoord {
        let LastBaselineOffsetFromStartEdge = || -> nscoord {
            // Convert the distance to be relative from the line's cross-start edge.
            let offset = self.LastBaselineOffset();
            if offset != nscoord_MIN { self.LineCrossSize() - offset } else { offset }
        };

        let PrimaryBaseline = || -> nscoord {
            if aBaselineGroup == BaselineSharingGroup::First { self.FirstBaselineOffset() } else { LastBaselineOffsetFromStartEdge() }
        };
        let SecondaryBaseline = || -> nscoord {
            if aBaselineGroup == BaselineSharingGroup::First { LastBaselineOffsetFromStartEdge() } else { self.FirstBaselineOffset() }
        };

        let primaryBaseline = PrimaryBaseline();
        if primaryBaseline != nscoord_MIN {
            return primaryBaseline;
        }
        SecondaryBaseline()
    }
}

impl FlexItem {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::ResolveStretchedCrossSize
    pub fn ResolveStretchedCrossSize(&mut self, aLineCrossSize: nscoord) {
        // We stretch IFF we are align-self:stretch, have no auto margins in
        // cross axis, and have cross-axis size property == "auto". If any of those
        // conditions don't hold up, we won't stretch.
        // https://drafts.csswg.org/css-flexbox-1/#valdef-align-items-stretch
        if self.mAlignSelf != StyleAlignFlags::STRETCH || self.NumAutoMarginsInCrossAxis() != 0 || !self.IsCrossSizeAuto() {
            return;
        }

        // If we've already been stretched, we can bail out early, too.
        // No need to redo the calculation.
        if self.mIsStretched {
            return;
        }

        // Reserve space for margins & border & padding, and then use whatever
        // remains as our item's cross-size (clamped to its min/max range).
        let mut stretchedSize: nscoord = aLineCrossSize - self.MarginBorderPaddingSizeInCrossAxis();

        stretchedSize = CSSMinMax(stretchedSize, self.mCrossMinSize, self.mCrossMaxSize);

        // Update the cross-size & make a note that it's stretched, so we know to
        // override the reflow input's computed cross-size in our final reflow.
        self.SetCrossSize(stretchedSize);
        self.mIsStretched = true;
    }
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp FindFlexItemBlockFrame
fn FindFlexItemBlockFrame(aFrame: nsIFrame) -> Option<nsBlockFrame> {
    if let Some(block) = nsBlockFrame::do_QueryFrame(aFrame) {
        return Some(block);
    }
    // for (nsIFrame* f : aFrame->PrincipalChildList())
    let mut f = aFrame.PrincipalChildList_FirstChild();
    while let Some(child) = f {
        if let Some(block) = FindFlexItemBlockFrame(child) {
            return Some(block);
        }
        f = child.GetNextSibling();
    }
    None
}

impl FlexItem {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexItem::BlockFrame
    pub fn BlockFrame(&self) -> Option<nsBlockFrame> {
        FindFlexItemBlockFrame(self.Frame())
    }
}

impl SingleLineCrossAxisPositionTracker {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp SingleLineCrossAxisPositionTracker::ResolveAutoMarginsInCrossAxis
    pub fn ResolveAutoMarginsInCrossAxis(&mut self, aLine: &FlexLine, aItem: &mut FlexItem) {
        // Subtract the space that our item is already occupying, to see how much
        // space (if any) is available for its auto margins.
        let mut spaceForAutoMargins: nscoord = aLine.LineCrossSize() - aItem.OuterCrossSize();

        if spaceForAutoMargins <= 0 {
            return; // No available space  --> nothing to do
        }

        let mut numAutoMargins: u32 = aItem.NumAutoMarginsInCrossAxis();
        if numAutoMargins == 0 {
            return; // No auto margins --> nothing to do.
        }

        // OK, we have at least one auto margin and we have some available space.
        // Give each auto margin a share of the space.
        let styleMargin = aItem.Frame().StyleMargin();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aItem.Frame(), std::ptr::null_mut());
        for side in [self.StartSide(), self.EndSide()] {
            if styleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(side, self.mBase.mWM, &anchorResolutionParams).IsAuto() {
                MOZ_ASSERT!(aItem.GetMarginComponentForSide(side) == 0, "Expecting auto margins to have value '0' before we update them");

                // NOTE: integer divison is fine here; numAutoMargins is either 1 or 2.
                // If it's 2 & spaceForAutoMargins is odd, 1st margin gets smaller half.
                let curAutoMarginSize: nscoord = spaceForAutoMargins / numAutoMargins as nscoord;
                aItem.SetMarginComponentForSide(side, curAutoMarginSize);
                numAutoMargins -= 1;
                spaceForAutoMargins -= curAutoMarginSize;
            }
        }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp SingleLineCrossAxisPositionTracker::EnterAlignPackingSpace
    pub fn EnterAlignPackingSpace(&mut self, aLine: &FlexLine, aItem: &FlexItem, aAxisTracker: &FlexboxAxisTracker) {
        // We don't do align-self alignment on items that have auto margins
        // in the cross axis.
        if aItem.NumAutoMarginsInCrossAxis() != 0 {
            return;
        }

        let mut alignSelf: StyleAlignFlags = aItem.AlignSelf();
        // NOTE: 'stretch' behaves like 'flex-start' once we've stretched any
        // auto-sized items (which we've already done).
        if alignSelf == StyleAlignFlags::STRETCH {
            alignSelf = StyleAlignFlags::FLEX_START;
        }

        // Map 'self-start'/'self-end' to 'start'/'end'
        if alignSelf == StyleAlignFlags::SELF_START || alignSelf == StyleAlignFlags::SELF_END {
            let logCrossAxis = if aAxisTracker.IsRowOriented() { LogicalAxis::Block } else { LogicalAxis::Inline };
            let cWM = aAxisTracker.GetWritingMode();
            let sameStart = cWM.ParallelAxisStartsOnSameSide(logCrossAxis, &aItem.GetWritingMode());
            alignSelf = if sameStart == (alignSelf == StyleAlignFlags::SELF_START) { StyleAlignFlags::START } else { StyleAlignFlags::END };
        }

        // Map 'start'/'end' to 'flex-start'/'flex-end'.
        if alignSelf == StyleAlignFlags::START {
            alignSelf = if aAxisTracker.IsCrossAxisReversed() { StyleAlignFlags::FLEX_END } else { StyleAlignFlags::FLEX_START };
        } else if alignSelf == StyleAlignFlags::END {
            alignSelf = if aAxisTracker.IsCrossAxisReversed() { StyleAlignFlags::FLEX_START } else { StyleAlignFlags::FLEX_END };
        }

        // 'align-self' falls back to 'flex-start' if it is 'center'/'flex-end' and we
        // have cross axis overflow
        // XXX we should really be falling back to 'start' as of bug 1472843
        if aLine.LineCrossSize() < aItem.OuterCrossSize() && aItem.AlignSelfFlags().contains(StyleAlignFlags::SAFE) {
            alignSelf = StyleAlignFlags::FLEX_START;
        }

        if alignSelf == StyleAlignFlags::FLEX_START {
            // No space to skip over -- we're done.
        } else if alignSelf == StyleAlignFlags::FLEX_END {
            self.mBase.mPosition += aLine.LineCrossSize() - aItem.OuterCrossSize();
        } else if alignSelf == StyleAlignFlags::CENTER || alignSelf == StyleAlignFlags::ANCHOR_CENTER {
            // TODO(dshin, Bug 1909339): For now, treat `anchor-center` as `center`.
            // Note: If cross-size is odd, the "after" space will get the extra unit.
            self.mBase.mPosition += (aLine.LineCrossSize() - aItem.OuterCrossSize()) / 2;
        } else if alignSelf == StyleAlignFlags::BASELINE || alignSelf == StyleAlignFlags::LAST_BASELINE {
            let usingItemFirstBaseline = alignSelf == StyleAlignFlags::BASELINE;

            // The first-baseline sharing group gets (collectively) aligned to the
            // FlexLine's cross-start side, and similarly the last-baseline sharing
            // group gets snapped to the cross-end side.
            let isFirstBaselineSharingGroup = aItem.ItemBaselineSharingGroup() == BaselineSharingGroup::First;
            let alignSide: Side = if isFirstBaselineSharingGroup { aAxisTracker.CrossAxisPhysicalStartSide() } else { aAxisTracker.CrossAxisPhysicalEndSide() };

            // To compute the aligned position for our flex item, we determine:
            // (1) The distance from the item's alignSide edge to the item's relevant
            //     baseline.
            let itemBaselineOffset: nscoord = aItem.BaselineOffsetFromOuterCrossEdge(alignSide, usingItemFirstBaseline);

            // (2) The distance between the FlexLine's alignSide edge and the relevant
            //     baseline-sharing-group's baseline position.
            let lineBaselineOffset: nscoord = if isFirstBaselineSharingGroup { aLine.FirstBaselineOffset() } else { aLine.LastBaselineOffset() };

            NS_ASSERTION!(lineBaselineOffset >= itemBaselineOffset, "failed at finding largest baseline offset");

            // (3) The difference between the above offsets, which tells us how far we
            //     need to shift the item away from the FlexLine's alignSide edge so
            //     that its baseline is at the proper position for its group.
            let itemOffsetFromLineEdge: nscoord = lineBaselineOffset - itemBaselineOffset;

            if isFirstBaselineSharingGroup {
                // alignSide is the line's cross-start edge. mPosition is already there.
                // From there, we step *forward* by the baseline adjustment:
                self.mBase.mPosition += itemOffsetFromLineEdge;
            } else {
                // alignSide is the line's cross-end edge. Advance mPosition to align
                // item with that edge (as in FLEX_END case)...
                self.mBase.mPosition += aLine.LineCrossSize() - aItem.OuterCrossSize();
                // ...and step *back* by the baseline adjustment:
                self.mBase.mPosition -= itemOffsetFromLineEdge;
            }
        } else {
            MOZ_ASSERT_UNREACHABLE!("Unexpected align-self value");
        }
    }
}

impl FlexboxAxisInfo {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisInfo::FlexboxAxisInfo
    pub fn new(aFlexContainer: nsIFrame) -> FlexboxAxisInfo {
        MOZ_ASSERT!(aFlexContainer.IsFlexContainerFrame(), "Only flex containers may be passed to this constructor!");
        let mut info = FlexboxAxisInfo { mIsRowOriented: true, mIsMainAxisReversed: false, mIsCrossAxisReversed: false };
        if aFlexContainer.IsLegacyWebkitBox() {
            info.InitAxesFromLegacyProps(aFlexContainer);
        } else {
            info.InitAxesFromModernProps(aFlexContainer);
        }
        info
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisInfo::InitAxesFromLegacyProps
    // NOT PORTED: legacy -webkit-box (nsStyleXUL::mBoxOrient, mBoxDirection are not in this crate;
    // IsLegacyWebkitBox() is always false). Original (nsFlexContainerFrame.cpp lines 4127-4147 at GECKO_REV):
    //   mIsRowOriented = (boxOrientIsVertical == wmIsVertical);
    //   mIsMainAxisReversed = styleXUL->mBoxDirection == StyleBoxDirection::Reverse;
    //   mIsCrossAxisReversed = false;
    fn InitAxesFromLegacyProps(&mut self, aFlexContainer: nsIFrame) {
        unimplemented!("NOT PORTED: legacy -webkit-box (FlexboxAxisInfo::InitAxesFromLegacyProps)")
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisInfo::InitAxesFromModernProps
    fn InitAxesFromModernProps(&mut self, aFlexContainer: nsIFrame) {
        let stylePos = aFlexContainer.StylePosition();
        let flexDirection = stylePos.mFlexDirection();

        // Determine main axis:
        match flexDirection {
            StyleFlexDirection::Row => {
                self.mIsRowOriented = true;
                self.mIsMainAxisReversed = false;
            }
            StyleFlexDirection::RowReverse => {
                self.mIsRowOriented = true;
                self.mIsMainAxisReversed = true;
            }
            StyleFlexDirection::Column => {
                self.mIsRowOriented = false;
                self.mIsMainAxisReversed = false;
            }
            StyleFlexDirection::ColumnReverse => {
                self.mIsRowOriented = false;
                self.mIsMainAxisReversed = true;
            }
        }

        // "flex-wrap: wrap-reverse" reverses our cross axis.
        self.mIsCrossAxisReversed = stylePos.mFlexWrap() == StyleFlexWrap::WRAP_REVERSE;
    }
}

impl FlexboxAxisTracker {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::FlexboxAxisTracker
    pub fn new(aFlexContainer: nsFlexContainerFrame) -> FlexboxAxisTracker {
        FlexboxAxisTracker { mWM: aFlexContainer.GetWritingMode(), mAxisInfo: FlexboxAxisInfo::new(aFlexContainer.0) }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::MainAxisStartSide
    pub fn MainAxisStartSide(&self) -> LogicalSide {
        MakeLogicalSide(self.MainAxis(), if self.IsMainAxisReversed() { LogicalEdge::End } else { LogicalEdge::Start })
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexboxAxisTracker::CrossAxisStartSide
    pub fn CrossAxisStartSide(&self) -> LogicalSide {
        MakeLogicalSide(self.CrossAxis(), if self.IsCrossAxisReversed() { LogicalEdge::End } else { LogicalEdge::Start })
    }
}

impl nsFlexContainerFrame {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::GenerateFlexLines
    pub fn GenerateFlexLines(&self, aReflowInput: &ReflowInput, aTentativeContentBoxMainSize: nscoord, aTentativeContentBoxCrossSize: nscoord, aStruts: &Vec<StrutInfo>, aAxisTracker: &FlexboxAxisTracker, aMainGapSize: nscoord, aPlaceholders: &mut Vec<nsIFrame>, aLines: &mut Vec<FlexLine>, aHasCollapsedItems: &mut bool) {
        MOZ_ASSERT!(aLines.is_empty(), "Expecting outparam to start out empty");

        // ADAPTER: `FlexLine* curLine` is the index of the last line (the Vec may reallocate).
        // auto ConstructNewFlexLine = [&aLines, aMainGapSize]() { return aLines.EmplaceBack(aMainGapSize); };

        // We have at least one FlexLine. Even an empty flex container has a single
        // (empty) flex line.
        aLines.push(FlexLine::new(aMainGapSize));
        let mut curLine: usize = aLines.len() - 1;

        let wrapThreshold: nscoord;
        if IsSingleLine(aReflowInput.mFrame, &aReflowInput.mStylePosition) {
            // Not wrapping. Set threshold to sentinel value that tells us not to wrap.
            wrapThreshold = NS_UNCONSTRAINEDSIZE;
        } else {
            // Wrapping! Set wrap threshold to flex container's content-box main-size.
            let mut threshold = aTentativeContentBoxMainSize;

            // If the flex container doesn't have a definite content-box main-size
            // (e.g. if main axis is vertical & 'height' is 'auto'), make sure we at
            // least wrap when we hit its max main-size.
            if threshold == NS_UNCONSTRAINEDSIZE {
                let flexContainerMaxMainSize = aAxisTracker.MainComponent__LogicalSize(aReflowInput.ComputedMaxSize__());
                threshold = flexContainerMaxMainSize;
            }
            wrapThreshold = threshold;
        }

        // Tracks the index of the next strut, in aStruts (and when this hits
        // aStruts.Length(), that means there are no more struts):
        let mut nextStrutIdx: u32 = 0;

        // Overall index of the current flex item in the flex container. (This gets
        // checked against entries in aStruts.)
        let mut itemIdxInContainer: u32 = 0;

        let mut iter = CSSOrderAwareFrameIterator::new(self.0, FrameChildListID::Principal, CSSOrderAwareFrameIterator_ChildFilter::IncludeAll, CSSOrderAwareFrameIterator_OrderState::Unknown, OrderingPropertyForIter(*self));

        self.AddOrRemoveStateBits(NS_STATE_FLEX_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER, iter.ItemsAreAlreadyInOrder());

        let useMozBoxCollapseBehavior = self.StyleVisibility().UseLegacyCollapseBehavior();

        while !iter.AtEnd() {
            let childFrame = iter.get();
            // Don't create flex items / lines for placeholder frames:
            if childFrame.IsPlaceholderFrame() {
                aPlaceholders.push(childFrame);
                iter.Next();
                continue;
            }

            let collapsed = childFrame.StyleVisibility().IsCollapse();
            *aHasCollapsedItems = *aHasCollapsedItems || collapsed;

            if useMozBoxCollapseBehavior && collapsed {
                // Legacy visibility:collapse behavior: make a 0-sized strut. (No need to
                // bother with aStruts and remembering cross size.)
                aLines[curLine].Items_mut().push(FlexItem::new__nsIFrame_nscoord_WritingMode_FlexboxAxisTracker(childFrame, 0, aReflowInput.GetWritingMode(), aAxisTracker));
            } else if (nextStrutIdx as usize) < aStruts.len() && aStruts[nextStrutIdx as usize].mItemIdx == itemIdxInContainer {
                // Use the simplified "strut" FlexItem constructor:
                aLines[curLine].Items_mut().push(FlexItem::new__nsIFrame_nscoord_WritingMode_FlexboxAxisTracker(childFrame, aStruts[nextStrutIdx as usize].mStrutCrossSize, aReflowInput.GetWritingMode(), aAxisTracker));
                nextStrutIdx += 1;
            } else {
                self.GenerateFlexItemForChild(&mut aLines[curLine], childFrame, aReflowInput, aAxisTracker, aTentativeContentBoxCrossSize);
            }

            // Check if we need to wrap the newly appended item to a new line, i.e. if
            // its outer hypothetical main size pushes our line over the threshold.
            // But we don't wrap if the line-length is unconstrained, nor do we wrap if
            // this was the first item on the line.
            if wrapThreshold != NS_UNCONSTRAINEDSIZE && aLines[curLine].Items().len() > 1 {
                // If the line will be longer than wrapThreshold or at least as long as
                // nscoord_MAX because of the newly appended item, then wrap and move the
                // item to a new line.
                let mut newOuterSize: i64 = aLines[curLine].TotalOuterHypotheticalMainSize();
                newOuterSize += aLines[curLine].LastItem().OuterMainSize() as i64;

                // Account for gap between this line's previous item and this item.
                newOuterSize += aMainGapSize as i64;

                if newOuterSize >= nscoord_MAX as i64 || newOuterSize > wrapThreshold as i64 {
                    aLines.push(FlexLine::new(aMainGapSize));
                    curLine = aLines.len() - 1;

                    // Get the previous line after adding a new line because the address can
                    // change if nsTArray needs to reallocate a new space for the new line.
                    // Move the item from the end of prevLine to the end of curLine.
                    let moved = aLines[curLine - 1].Items_mut().pop().unwrap();
                    aLines[curLine].Items_mut().push(moved);
                }
            }

            // Update the line's bookkeeping about how large its items collectively are.
            aLines[curLine].AddLastItemToMainSizeTotals();
            itemIdxInContainer += 1;
            iter.Next();
        }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::GenerateFlexLayoutResult
    pub fn GenerateFlexLayoutResult(&self) -> FlexLayoutResult {
        MOZ_ASSERT!(self.GetPrevInFlow().is_some(), "This should be called by non-first-in-flows!");

        let data = SharedFlexData::Get(self.FirstInFlow());
        MOZ_ASSERT!(data.is_some(), "SharedFlexData should be set by our first-in-flow!");
        let data = data.unwrap();

        let mut flr = FlexLayoutResult::default();

        // The order state of the children is consistent across entire continuation
        // chain due to calling nsContainerFrame::NormalizeChildLists() at the
        // beginning of Reflow(), so we can align our state bit with our
        // prev-in-flow's state. Setup here before calling OrderStateForIter() below.
        self.AddOrRemoveStateBits(NS_STATE_FLEX_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER, self.GetPrevInFlow().unwrap().HasAnyStateBits(NS_STATE_FLEX_NORMAL_FLOW_CHILDREN_IN_CSS_ORDER));

        // Construct flex items for this flex container fragment from existing flex
        // items in SharedFlexData.
        let mut iter = CSSOrderAwareFrameIterator::new(self.0, FrameChildListID::Principal, CSSOrderAwareFrameIterator_ChildFilter::SkipPlaceholders, OrderStateForIter(*self), OrderingPropertyForIter(*self));

        // auto ConstructNewFlexLine = [&flr]() {
        //   // Use zero main gap size since it doesn't matter in flex container's
        //   // next-in-flows. We've computed flex items' positions in first-in-flow.
        //   return flr.mLines.EmplaceBack(0); };
        // ADAPTER: `currentLine` is the index of the last line.

        // We have at least one FlexLine. Even an empty flex container has a single
        // (empty) flex line.
        flr.mLines.push(FlexLine::new(0));
        let mut currentLine: usize = 0;

        if !iter.AtEnd() {
            let mut child = iter.get();
            let mut childFirstInFlow = child.FirstInFlow();

            // We are iterating nested for-loops over the FlexLines and FlexItems
            // generated by GenerateFlexLines() and cached in flex container's
            // first-in-flow. For each flex item, check if its frame (must be a
            // first-in-flow) is the first-in-flow of the first child frame in this flex
            // container continuation. If so, clone the data from that FlexItem into a
            // FlexLine. When we find a match for the item, we know that the next child
            // frame might have its first-in-flow as the next item in the same original
            // line. In this case, we'll put the cloned data in the same line here as
            // well.
            for line in data.mLines.iter() {
                // If currentLine is empty, either it is the first line, or all the items
                // in the previous line have been placed in our prev-in-flows. No need to
                // construct a new line.
                if !flr.mLines[currentLine].IsEmpty() {
                    flr.mLines.push(FlexLine::new(0));
                    currentLine = flr.mLines.len() - 1;
                }
                for item in line.Items() {
                    if item.Frame() == childFirstInFlow {
                        flr.mLines[currentLine].Items_mut().push(item.CloneFor(child));
                        iter.Next();
                        if iter.AtEnd() {
                            // We've constructed flex items for all children. No need to check
                            // rest of the items.
                            break;
                        }
                        child = iter.get();
                        childFirstInFlow = child.FirstInFlow();
                    }
                }
                if iter.AtEnd() {
                    // We've constructed flex items for all children. No need to check
                    // rest of the lines.
                    break;
                }
            }
        }

        flr.mContentBoxMainSize = data.mContentBoxMainSize;
        flr.mContentBoxCrossSize = data.mContentBoxCrossSize;

        flr
    }
}

// Returns the largest outer hypothetical main-size of any line in |aLines|.
// (i.e. the hypothetical main-size of the largest line)
// Gecko: layout/generic/nsFlexContainerFrame.cpp GetLargestLineMainSize
fn GetLargestLineMainSize(aLines: &Vec<FlexLine>) -> i64 {
    let mut largestLineOuterSize: i64 = 0;
    for line in aLines {
        largestLineOuterSize = std::cmp::max(largestLineOuterSize, line.TotalOuterHypotheticalMainSize());
    }
    largestLineOuterSize
}

impl nsFlexContainerFrame {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::ComputeMainSize
    pub fn ComputeMainSize(&self, aReflowInput: &ReflowInput, aAxisTracker: &FlexboxAxisTracker, aTentativeContentBoxMainSize: nscoord, aLines: &mut Vec<FlexLine>) -> nscoord {
        if aAxisTracker.IsRowOriented() {
            // Row-oriented --> our main axis is the inline axis, so our main size
            // is our inline size (which should already be resolved).
            return aTentativeContentBoxMainSize;
        }

        let shouldApplyAutomaticMinimumOnBlockAxis = aReflowInput.ShouldApplyAutomaticMinimumOnBlockAxis();
        if aTentativeContentBoxMainSize != NS_UNCONSTRAINEDSIZE && !shouldApplyAutomaticMinimumOnBlockAxis {
            // Column-oriented case, with fixed BSize:
            // Just use our fixed block-size because we always assume the available
            // block-size is unconstrained, and the reflow input has already done the
            // appropriate min/max-BSize clamping.
            return aTentativeContentBoxMainSize;
        }

        // Column-oriented case, with size-containment in block axis:
        // Behave as if we had no content and just use our MinBSize.
        if let Some(containBSize) = aReflowInput.mFrame.ContainIntrinsicBSize(0) {
            return aReflowInput.ApplyMinMaxBSize(containBSize, 0);
        }

        let largestLineMainSize = GetLargestLineMainSize(aLines);
        let contentBSize = aReflowInput.ApplyMinMaxBSize(AuCoord64ToMinMaxClamped(largestLineMainSize), 0);

        // If the clamped largest FlexLine length is larger than the tentative main
        // size (which is resolved by aspect-ratio), we extend it to contain the
        // entire FlexLine.
        // https://drafts.csswg.org/css-sizing-4/#aspect-ratio-minimum
        if shouldApplyAutomaticMinimumOnBlockAxis {
            // Column-oriented case, with auto BSize which is resolved by
            // aspect-ratio.
            return std::cmp::max(contentBSize, aTentativeContentBoxMainSize);
        }

        // Column-oriented case, with auto BSize:
        // Resolve auto BSize to the largest FlexLine length, clamped to our
        // computed min/max main-size properties.
        contentBSize
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::ComputeCrossSize
    pub fn ComputeCrossSize(&self, aReflowInput: &ReflowInput, aAxisTracker: &FlexboxAxisTracker, aTentativeContentBoxCrossSize: nscoord, aSumLineCrossSizes: nscoord, aIsDefinite: &mut bool) -> nscoord {
        if aAxisTracker.IsColumnOriented() {
            // Column-oriented --> our cross axis is the inline axis, so our cross size
            // is our inline size (which should already be resolved).
            *aIsDefinite = true;
            // FIXME: Bug 1661847 - there are cases where aTentativeContentBoxCrossSize
            // (i.e. aReflowInput.ComputedISize()) might not be the right thing to
            // return here. Specifically: if our cross size is an intrinsic size, and we
            // have flex items that are flexible and have aspect ratios, then we may
            // need to take their post-flexing main sizes into account (multiplied
            // through their aspect ratios to get their cross sizes), in order to
            // determine their flex line's size & the flex container's cross size (e.g.
            // as `aSumLineCrossSizes`).
            return aTentativeContentBoxCrossSize;
        }

        let shouldApplyAutomaticMinimumOnBlockAxis = aReflowInput.ShouldApplyAutomaticMinimumOnBlockAxis();
        let computedBSize = aReflowInput.ComputedBSize();
        if computedBSize != NS_UNCONSTRAINEDSIZE && !shouldApplyAutomaticMinimumOnBlockAxis {
            // Row-oriented case (cross axis is block-axis), with fixed BSize:
            *aIsDefinite = true;

            // Just use our fixed block-size because we always assume the available
            // block-size is unconstrained, and the reflow input has already done the
            // appropriate min/max-BSize clamping.
            return computedBSize;
        }

        // Row-oriented case, with size-containment in block axis:
        // Behave as if we had no content and just use our MinBSize.
        if let Some(containBSize) = aReflowInput.mFrame.ContainIntrinsicBSize(0) {
            *aIsDefinite = true;
            return aReflowInput.ApplyMinMaxBSize(containBSize, 0);
        }

        // The cross size must not be definite in the following cases.
        *aIsDefinite = false;

        let contentBSize = aReflowInput.ApplyMinMaxBSize(aSumLineCrossSizes, 0);
        // If the content block-size is larger than the effective computed
        // block-size, we extend the block-size to contain all the content.
        // https://drafts.csswg.org/css-sizing-4/#aspect-ratio-minimum
        if shouldApplyAutomaticMinimumOnBlockAxis {
            // Row-oriented case (cross axis is block-axis), with auto BSize which is
            // resolved by aspect-ratio or content size.
            return std::cmp::max(contentBSize, computedBSize);
        }

        // Row-oriented case (cross axis is block axis), with auto BSize:
        // Shrink-wrap our line(s), subject to our min-size / max-size
        // constraints in that (block) axis.
        contentBSize
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::ComputeAvailableSizeForItems
    pub fn ComputeAvailableSizeForItems(&self, aReflowInput: &ReflowInput, aBorderPadding: LogicalMargin) -> LogicalSize {
        let wm = self.GetWritingMode();
        let mut availableBSize: nscoord = aReflowInput.AvailableBSize();

        if availableBSize != NS_UNCONSTRAINEDSIZE {
            // Available block-size is constrained. Subtract block-start border and
            // padding from it.
            availableBSize -= aBorderPadding.BStart(wm);

            if aReflowInput.mStyleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone {
                // We have box-decoration-break:clone. Subtract block-end border and
                // padding from the available block-size as well.
                availableBSize -= aBorderPadding.BEnd(wm);
            }

            // Available block-size can became negative after subtracting block-axis
            // border and padding. Per spec, to guarantee progress, fragmentainers are
            // assumed to have a minimum block size of 1px regardless of their used
            // size. https://drafts.csswg.org/css-break/#breaking-rules
            availableBSize = std::cmp::max(nsPresContext::CSSPixelsToAppUnits__int32_t(1), availableBSize);
        }

        LogicalSize::new(wm, aReflowInput.ComputedISize(), availableBSize)
    }
}

impl FlexLine {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::PositionItemsInMainAxis
    pub fn PositionItemsInMainAxis(&mut self, aJustifyContent: StyleContentDistribution, aContentBoxMainSize: nscoord, aAxisTracker: &FlexboxAxisTracker) {
        let mut mainAxisPosnTracker = MainAxisPositionTracker::new(aAxisTracker, self, aJustifyContent, aContentBoxMainSize);
        let mut hadItemBefore = false;
        for item in self.mItems.iter_mut() {
            let strut = item.IsStrut();
            if hadItemBefore && !strut {
                mainAxisPosnTracker.TraverseGap(self.mMainGapSize);
            }

            let itemMainBorderBoxSize: nscoord = item.MainSize() + item.BorderPaddingSizeInMainAxis();

            // Resolve any main-axis 'auto' margins on aChild to an actual value.
            mainAxisPosnTracker.ResolveAutoMarginsInMainAxis(item);

            // Advance our position tracker to child's upper-left content-box corner,
            // and use that as its position in the main axis.
            mainAxisPosnTracker.EnterMargin(item.Margin());
            mainAxisPosnTracker.EnterChildFrame(itemMainBorderBoxSize);

            item.SetMainPosition(mainAxisPosnTracker.Position());

            mainAxisPosnTracker.ExitChildFrame(itemMainBorderBoxSize);
            mainAxisPosnTracker.ExitMargin(item.Margin());
            mainAxisPosnTracker.TraversePackingSpace();
            hadItemBefore |= !strut;
        }
    }
}

impl nsFlexContainerFrame {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::SizeItemInCrossAxis
    pub fn SizeItemInCrossAxis(&self, aChildReflowInput: &mut ReflowInput, aItem: &mut FlexItem) {
        // If cross axis is the item's inline axis, just use ISize from reflow input,
        // and don't bother with a full reflow.
        if aItem.IsInlineAxisCrossAxis() {
            aItem.SetCrossSize(aChildReflowInput.ComputedISize());
            return;
        }

        MOZ_ASSERT!(!aItem.HadMeasuringReflow(), "We shouldn't need more than one measuring reflow");

        if aItem.AlignSelf() == StyleAlignFlags::STRETCH {
            // This item's got "align-self: stretch", so we probably imposed a
            // stretched computed cross-size on it during its previous
            // reflow. We're not imposing that BSize for *this* "measuring" reflow, so
            // we need to tell it to treat this reflow as a resize in its block axis
            // (regardless of whether any of its ancestors are actually being resized).
            // (Note: we know that the cross axis is the item's *block* axis -- if it
            // weren't, then we would've taken the early-return above.)
            aChildReflowInput.SetBResize(true);
            // Not 100% sure this is needed, but be conservative for now:
            aChildReflowInput.SetBResizeForPercentages(true);
        }

        // Potentially reflow the item, and get the sizing info.
        let measurement = self.MeasureBSizeForFlexItem(aItem, aChildReflowInput);

        // Save the sizing info that we learned from this reflow
        // -----------------------------------------------------

        // Tentatively store the child's desired content-box cross-size.
        aItem.SetCrossSize(measurement.BSize());
    }
}

impl FlexLine {
    // Gecko: layout/generic/nsFlexContainerFrame.cpp FlexLine::PositionItemsInCrossAxis
    pub fn PositionItemsInCrossAxis(&mut self, aLineStartPosition: nscoord, aAxisTracker: &FlexboxAxisTracker) {
        let mut lineCrossAxisPosnTracker = SingleLineCrossAxisPositionTracker::new(aAxisTracker);

        // ADAPTER: the trackers read the line (`*this`) while the loop changes its items, so the items are
        // moved out of the line for the loop (they are only read through `aItem`, never through `aLine`).
        let mut items = std::mem::take(&mut self.mItems);
        for item in items.iter_mut() {
            // First, stretch the item's cross size (if appropriate), and resolve any
            // auto margins in this axis.
            item.ResolveStretchedCrossSize(self.mLineCrossSize);
            lineCrossAxisPosnTracker.ResolveAutoMarginsInCrossAxis(self, item);

            // Compute the cross-axis position of this item
            let itemCrossBorderBoxSize: nscoord = item.CrossSize() + item.BorderPaddingSizeInCrossAxis();
            lineCrossAxisPosnTracker.EnterAlignPackingSpace(self, item, aAxisTracker);
            lineCrossAxisPosnTracker.EnterMargin(item.Margin());
            lineCrossAxisPosnTracker.EnterChildFrame(itemCrossBorderBoxSize);

            item.SetCrossPosition(aLineStartPosition + lineCrossAxisPosnTracker.Position());

            // Back out to cross-axis edge of the line.
            lineCrossAxisPosnTracker.ResetPosition();
        }
        self.mItems = items;
    }
}

// nsFlexContainerFrame.cpp lines 4633-6732 at GECKO_REV (Reflow through the nsILineIterator members).

// Gecko: layout/generic/nsIFrame.h nsIFrame::HidesContentForLayout
// NOT PORTED: content-visibility (the property is not read from the style); content is never hidden.
impl nsFlexContainerFrame {
    fn HidesContentForLayout(&self) -> bool {
        false
    }
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::Reflow
impl nsFlexContainerFrame {
    pub fn Reflow(&self, aPresContext: &nsPresContext, aReflowOutput: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.NormalizeChildLists();

        if self.IsHiddenByContentVisibilityOfInFlowParentForLayout() {
            return;
        }

        self.MarkInReflow();
        // NOT PORTED: DO_GLOBAL_REFLOW_COUNT
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");
        NS_WARNING_ASSERTION!(aReflowInput.ComputedISize() != NS_UNCONSTRAINEDSIZE, "Unconstrained inline size; this should only result from huge sizes (not intrinsic sizing w/ orthogonal flows)");

        FLEX_LOG!("Reflow flex container frame %p", this);

        if self.IsFrameTreeTooDeep(aReflowInput, aReflowOutput, aStatus) {
            return;
        }

        #[cfg(debug_assertions)]
        {
            // NOT PORTED: mDidPushItemsBitMayLie = false; SanityCheckChildListsBeforeReflow() (debug checks of
            // the fragmentation child lists).
        }

        // We (and our children) can only depend on our ancestor's bsize if we have
        // a percent-bsize, or if we're positioned and we have "block-start" and
        // "block-end" set and have block-size:auto.  (There are actually other cases,
        // too -- e.g. if our parent is itself a block-dir flex container and we're
        // flexible -- but we'll let our ancestors handle those sorts of cases.)
        //
        // TODO(emilio): the !bsize.IsLengthPercentage() preserves behavior, but it's
        // too conservative. min/max-content don't really depend on the container.
        let wm = aReflowInput.GetWritingMode();
        let stylePos = self.StylePosition();
        let anchorResolutionParams = AnchorPosOffsetResolutionParams::UseCBFrameSize(AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.0, std::ptr::null_mut()));
        let bsize = stylePos.BSize(wm, &anchorResolutionParams.mBaseParams);
        if bsize.HasPercent()
            || (self.StyleDisplay().IsAbsolutelyPositionedStyle()
                && (bsize.IsAuto() || !bsize.IsLengthPercentage())
                && !stylePos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BStart, wm, &anchorResolutionParams).IsAuto()
                && !stylePos.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BEnd, wm, &anchorResolutionParams).IsAuto())
        {
            self.AddStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE);
        }

        let axisTracker = FlexboxAxisTracker::new(*self);

        // Check to see if we need to create a computed info structure, to
        // be filled out for use by devtools.
        // NOT PORTED: devtools data (CreateOrClearFlexContainerInfo); there is never a containerInfo.

        let mut flr: FlexLayoutResult;
        let mut fragmentData = PerFragmentFlexData::new();
        let prevInFlow = self.GetPrevInFlow();
        if prevInFlow.is_none() {
            let tentativeContentBoxSize = aReflowInput.ComputedSize__();
            let tentativeContentBoxMainSize = axisTracker.MainComponent__LogicalSize(tentativeContentBoxSize);
            let tentativeContentBoxCrossSize = axisTracker.CrossComponent__LogicalSize(tentativeContentBoxSize);

            // Calculate gap sizes for main and cross axis. We only need them in
            // DoFlexLayout in the first-in-flow, so no need to worry about consumed
            // block-size.
            let mainGapStyle = if axisTracker.IsRowOriented() { stylePos.mColumnGap() } else { stylePos.mRowGap() };
            let crossGapStyle = if axisTracker.IsRowOriented() { stylePos.mRowGap() } else { stylePos.mColumnGap() };
            let mainGapSize = nsLayoutUtils::ResolveGapToLength(&mainGapStyle, tentativeContentBoxMainSize);
            let crossGapSize = nsLayoutUtils::ResolveGapToLength(&crossGapStyle, tentativeContentBoxCrossSize);

            // When fragmenting a flex container, we run the flex algorithm without
            // regards to pagination in order to compute the flex container's desired
            // content-box size. https://drafts.csswg.org/css-flexbox-1/#pagination-algo
            //
            // Note: For a multi-line column-oriented flex container, the sample
            // algorithm suggests we wrap the flex line at the block-end edge of a
            // column/page, but we do not implement it intentionally. This brings the
            // layout result closer to the one as if there's no fragmentation.
            let mut struts: Vec<StrutInfo> = Vec::new();
            flr = self.DoFlexLayout(aReflowInput, tentativeContentBoxMainSize, tentativeContentBoxCrossSize, &axisTracker, mainGapSize, crossGapSize, &mut struts);

            if !struts.is_empty() {
                // We're restarting flex layout, with new knowledge of collapsed items.
                flr.mLines.clear();
                flr.mPlaceholders.clear();
                flr = self.DoFlexLayout(aReflowInput, tentativeContentBoxMainSize, tentativeContentBoxCrossSize, &axisTracker, mainGapSize, crossGapSize, &mut struts);
            }
        } else {
            flr = self.GenerateFlexLayoutResult();
            let fragmentDataProp = PerFragmentFlexData::Get(prevInFlow.unwrap());
            MOZ_ASSERT!(fragmentDataProp.is_some(), "PerFragmentFlexData should be set in our prev-in-flow!");
            let p = fragmentDataProp.unwrap();
            fragmentData = PerFragmentFlexData { mCumulativeContentBoxBSize: p.mCumulativeContentBoxBSize, mCumulativeBEndEdgeShift: p.mCumulativeBEndEdgeShift };
        }

        let mut contentBoxSize = axisTracker.LogicalSizeFromFlexRelativeSizes(flr.mContentBoxMainSize, flr.mContentBoxCrossSize);

        let consumedBSize = self.CalcAndCacheConsumedBSize();
        let effectiveContentBSize = contentBoxSize.BSize(wm) - consumedBSize;
        let mut borderPadding = aReflowInput.ComputedLogicalBorderPadding(wm);
        if aReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE {
            // We assume we are the last fragment by using
            // PreReflowBlockLevelLogicalSkipSides(), and skip block-end border and
            // padding if needed.
            borderPadding.ApplySkipSides(self.PreReflowBlockLevelLogicalSkipSides());
        }

        // Determine this frame's tentative border-box size. This is used for logical
        // to physical coordinate conversion when positioning children.
        //
        // Note that vertical-rl writing-mode is the only case where the block flow
        // direction progresses in a negative physical direction, and therefore block
        // direction coordinate conversion depends on knowing the width of the
        // coordinate space in order to translate between the logical and physical
        // origins. As a result, if our final border-box block-size is different from
        // this tentative one, and we are in vertical-rl writing mode, we need to
        // adjust our children's position after reflowing them.
        let tentativeBorderBoxSize = LogicalSize::new(wm, contentBoxSize.ISize(wm) + borderPadding.IStartEnd(wm), std::cmp::min(effectiveContentBSize + borderPadding.BStartEnd(wm), aReflowInput.AvailableBSize()));
        let containerSize = tentativeBorderBoxSize.GetPhysicalSize(wm);

        let mut ocBounds = OverflowAreas::default();
        let mut ocStatus = nsReflowStatus::new();
        if prevInFlow.is_some() {
            self.ReflowOverflowContainerChildren(aPresContext, aReflowInput, &mut ocBounds, nsIFrame_ReflowChildFlags::Default, &mut ocStatus, nsContainerFrame::MergeSortedFrameListsFor, Some(containerSize));
        }

        let availableSizeForItems = self.ComputeAvailableSizeForItems(aReflowInput, borderPadding);
        let (childrenBEndEdge, childrenStatus) = self.ReflowChildren(aReflowInput, containerSize, availableSizeForItems, borderPadding, &axisTracker, &mut flr, &mut fragmentData);

        let mut mayNeedNextInFlow = false;
        if aReflowInput.IsInFragmentedContext() {
            // This fragment's contribution to the flex container's cumulative
            // content-box block-size, if it turns out that this is the final vs.
            // non-final fragment:
            //
            // * If it turns out we *are* the final fragment, then this fragment's
            // content-box contribution is the distance from the start of our content
            // box to the block-end edge of our children (note the borderPadding
            // subtraction is just to get us to a content-box-relative offset here):
            let bSizeContributionIfFinalFragment = childrenBEndEdge - borderPadding.BStart(wm);

            // * If it turns out we're *not* the final fragment, then this fragment's
            // content-box extends to the edge of the availableSizeForItems (at least),
            // regardless of whether we actually have items at that location:
            let bSizeContributionIfNotFinalFragment = std::cmp::max(bSizeContributionIfFinalFragment, availableSizeForItems.BSize(wm));

            // mCumulativeBEndEdgeShift was updated in ReflowChildren(), and our
            // children's block-size may grow in fragmented context. If our block-size
            // and max-block-size are unconstrained, then we allow the flex container to
            // grow to accommodate any children whose sizes grew as a result of
            // fragmentation.
            if aReflowInput.ComputedBSize() == NS_UNCONSTRAINEDSIZE {
                *contentBoxSize.BSize_mut(wm) = aReflowInput.ApplyMinMaxBSize(contentBoxSize.BSize(wm) + fragmentData.mCumulativeBEndEdgeShift, 0);

                if childrenStatus.IsComplete() {
                    // All of the children fit! We know that we're using a content-based
                    // block-size, and we know our children's block-size may have grown due
                    // to fragmentation. So we allow ourselves to grow our block-size here
                    // to contain the block-end edge of our last child (subject to our
                    // min/max constraints).
                    *contentBoxSize.BSize_mut(wm) = aReflowInput.ApplyMinMaxBSize(std::cmp::max(contentBoxSize.BSize(wm), fragmentData.mCumulativeContentBoxBSize + bSizeContributionIfFinalFragment), 0);
                } else {
                    // As in the if-branch above, we extend our block-size, but in this case
                    // we know that a child didn't fit and might overshot our available
                    // size, so we assume this fragment won't be the final fragment, and
                    // hence it should contribute bSizeContributionIfNotFinalFragment
                    // (subject to our min/max constraints).
                    *contentBoxSize.BSize_mut(wm) = aReflowInput.ApplyMinMaxBSize(std::cmp::max(contentBoxSize.BSize(wm), fragmentData.mCumulativeContentBoxBSize + bSizeContributionIfNotFinalFragment), 0);

                    if aReflowInput.ComputedMaxBSize() == NS_UNCONSTRAINEDSIZE {
                        mayNeedNextInFlow = true;
                    } else {
                        // The definite max-block-size can be the upper bound of our
                        // content-box block-size. We should check whether we need a
                        // next-in-flow.
                        mayNeedNextInFlow = contentBoxSize.BSize(wm) - consumedBSize > availableSizeForItems.BSize(wm);
                    }
                }
            } else {
                mayNeedNextInFlow = contentBoxSize.BSize(wm) - consumedBSize > availableSizeForItems.BSize(wm);
            }
            fragmentData.mCumulativeContentBoxBSize += bSizeContributionIfNotFinalFragment;

            // If we may need a next-in-flow, we'll need to skip block-end border and
            // padding.
            if mayNeedNextInFlow && aReflowInput.mStyleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Slice {
                *borderPadding.BEnd_mut(wm) = 0;
            }
        }

        self.PopulateReflowOutput(aReflowOutput, aReflowInput, aStatus, contentBoxSize, borderPadding, consumedBSize, mayNeedNextInFlow, childrenBEndEdge, &childrenStatus, &axisTracker, &mut flr);

        if wm.IsVerticalRL() {
            // If the final border-box block-size is different from the tentative one,
            // adjust our children's position.
            let deltaBCoord = tentativeBorderBoxSize.BSize(wm) - aReflowOutput.Size(wm).BSize(wm);
            if deltaBCoord != 0 {
                let delta = LogicalPoint::new(wm, 0, deltaBCoord);
                for line in &flr.mLines {
                    for item in line.Items() {
                        item.Frame().MovePositionBy__WritingMode_LogicalPoint(wm, delta);
                    }
                }
            }
        }

        // Overflow area = union(my overflow area, children's overflow areas)
        aReflowOutput.SetOverflowAreasToDesiredBounds();
        self.UnionInFlowChildOverflow(&mut aReflowOutput.mOverflowAreas, false);

        // Merge overflow container bounds and status.
        aReflowOutput.mOverflowAreas.UnionWith(&ocBounds);
        aStatus.MergeCompletionStatusFrom(&ocStatus);

        self.FinishReflowWithAbsoluteFrames(&self.PresContext(), aReflowOutput, aReflowInput, aStatus);

        // Finally update our line and item measurements in our containerInfo.
        // NOT PORTED: devtools data (UpdateFlexLineAndItemInfo).

        // If we are the first-in-flow, we want to store data for our next-in-flows,
        // or clear the existing data if it is not needed.
        if prevInFlow.is_none() {
            let sharedData = SharedFlexData::Get(self.0);
            if !aStatus.IsFullyComplete() {
                match sharedData {
                    Some(sharedData) => sharedData.Update(flr),
                    None => {
                        let mut sharedData = SharedFlexData::new();
                        sharedData.Update(flr);
                        SharedFlexData::Set(self.0, sharedData);
                    }
                }
            } else if sharedData.is_some() && self.GetNextInFlow().is_none() {
                // We are fully-complete, so no next-in-flow is needed. However, if we
                // report SetInlineLineBreakBeforeAndReset() in an incremental reflow, our
                // next-in-flow might still exist. It can be reflowed again before us if
                // it is an overflow container. Delete the existing data only if we don't
                // have a next-in-flow.
                self.RemoveProperty(SharedFlexDataProperty());
            }
        }

        let fragmentDataProp = PerFragmentFlexData::Get(self.0);
        if !aStatus.IsFullyComplete() {
            match fragmentDataProp {
                Some(p) => {
                    p.mCumulativeContentBoxBSize = fragmentData.mCumulativeContentBoxBSize;
                    p.mCumulativeBEndEdgeShift = fragmentData.mCumulativeBEndEdgeShift;
                }
                None => PerFragmentFlexData::Set(self.0, PerFragmentFlexData { mCumulativeContentBoxBSize: fragmentData.mCumulativeContentBoxBSize, mCumulativeBEndEdgeShift: fragmentData.mCumulativeBEndEdgeShift }),
            }
        } else if fragmentDataProp.is_some() && self.GetNextInFlow().is_none() {
            // Similar to the condition to remove SharedFlexData, delete the
            // existing data only if we don't have a next-in-flow.
            self.RemoveProperty(PerFragmentFlexDataProperty());
        }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::GetNaturalBaselineBOffset
    pub fn GetNaturalBaselineBOffset(&self, aWM: WritingMode, aBaselineGroup: BaselineSharingGroup, aExportContext: BaselineExportContext) -> Option<nscoord> {
        if self.StyleDisplay().IsContainLayout() || self.HasAnyStateBits(NS_STATE_FLEX_SYNTHESIZE_BASELINE) {
            return None;
        }
        Some(if aBaselineGroup == BaselineSharingGroup::First { self.mFirstBaseline() } else { self.mLastBaseline() })
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::UnionInFlowChildOverflow
    pub fn UnionInFlowChildOverflow(&self, aOverflowAreas: &mut OverflowAreas, aAsIfScrolled: bool) {
        // The CSS Overflow spec [1] requires that a scrollable container's
        // scrollable overflow should include the following areas.
        //
        // a) "the box's own content and padding areas": we treat the *content* as
        // the scrolled inner frame's theoretical content-box that's intrinsically
        // sized to the union of all the flex items' margin boxes, _without_
        // relative positioning applied. The *padding areas* is just inflation on
        // top of the theoretical content-box by the flex container's padding.
        //
        // b) "the margin areas of grid item and flex item boxes for which the box
        // establishes a containing block": a) already includes the flex items'
        // normal-positioned margin boxes into the scrollable overflow, but their
        // relative-positioned margin boxes should also be included because relpos
        // children are still flex items.
        //
        // [1] https://drafts.csswg.org/css-overflow-3/#scrollable.
        // NOT PORTED: scrolling (Style()->GetPseudoType() == PseudoStyleType::MozScrolledContent): there are
        // no scrolled-content frames, so only aAsIfScrolled makes this true.
        let isScrolledContent = aAsIfScrolled;
        let mut anyScrolledContentItem = false;
        // Union of normal-positioned margin boxes for all the items.
        let mut itemMarginBoxes = nsRect::default();
        // Overflow areas containing the union of relative-positioned and
        // stick-positioned margin boxes of relpos items.
        //
        // Note for sticky-positioned margin boxes, we only union it with the ink
        // overflow to avoid circular dependencies with the scroll container. (The
        // scroll position and the scroll container's size impact the sticky position,
        // so we don't want the sticky position to impact them.)
        let mut relPosItemMarginBoxes = OverflowAreas::default();
        let useMozBoxCollapseBehavior = self.StyleVisibility().UseLegacyCollapseBehavior();
        for f in &*self.mFrames() {
            if useMozBoxCollapseBehavior && f.StyleVisibility().IsCollapse() {
                continue;
            }
            self.ConsiderChildOverflow(aOverflowAreas, f, if aAsIfScrolled { OverflowAreaUnionFlags::AsIfScrolled } else { OverflowAreaUnionFlags::None });
            if !isScrolledContent {
                continue;
            }
            if f.IsPlaceholderFrame() {
                continue;
            }
            anyScrolledContentItem = true;
            if f.IsRelativelyOrStickyPositioned() {
                let marginRect = f.GetMarginRectRelativeToSelf();
                itemMarginBoxes = itemMarginBoxes.Union(marginRect + f.GetNormalPosition());
                if f.IsRelativelyPositioned() {
                    relPosItemMarginBoxes.UnionAllWith(marginRect + f.GetPosition());
                } else {
                    MOZ_ASSERT!(f.IsStickyPositioned());
                    relPosItemMarginBoxes.UnionWith(&OverflowAreas::new(marginRect + f.GetPosition(), nsRect::default()));
                }
            } else {
                itemMarginBoxes = itemMarginBoxes.Union(f.GetMarginRect());
            }
        }

        if anyScrolledContentItem {
            itemMarginBoxes.Inflate__nsMargin(self.GetUsedPadding());
            aOverflowAreas.UnionAllWith(itemMarginBoxes);
            aOverflowAreas.UnionWith(&relPosItemMarginBoxes);
        }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::UnionChildOverflow
    pub fn UnionChildOverflow(&self, aOverflowAreas: &mut OverflowAreas, aAsIfScrolled: bool) {
        self.UnionInFlowChildOverflow(aOverflowAreas, aAsIfScrolled);
        // Union with child frames, skipping the principal list since we already
        // handled those above.
        nsLayoutUtils::UnionChildOverflow(self.0, aOverflowAreas, &[FrameChildListID::Principal]);
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::CalculatePackingSpace
    pub fn CalculatePackingSpace(aNumThingsToPack: u32, aAlignVal: StyleContentDistribution, aFirstSubjectOffset: &mut nscoord, aNumPackingSpacesRemaining: &mut u32, aPackingSpaceRemaining: &mut nscoord) {
        let val = aAlignVal.primary();
        MOZ_ASSERT!(val == StyleAlignFlags::SPACE_BETWEEN || val == StyleAlignFlags::SPACE_AROUND || val == StyleAlignFlags::SPACE_EVENLY, "Unexpected alignment value");

        MOZ_ASSERT!(*aPackingSpaceRemaining >= 0, "Should not be called with negative packing space");

        // Note: In the aNumThingsToPack==1 case, the fallback behavior for
        // 'space-between' depends on precise information about the axes that we
        // don't have here. So, for that case, we just depend on the caller to
        // explicitly convert 'space-{between,around,evenly}' keywords to the
        // appropriate fallback alignment and skip this function.
        MOZ_ASSERT!(aNumThingsToPack > 1, "Should not be called unless there's more than 1 thing to pack");

        // Packing spaces between items:
        *aNumPackingSpacesRemaining = aNumThingsToPack - 1;

        if val == StyleAlignFlags::SPACE_BETWEEN {
            // No need to reserve space at beginning/end, so we're done.
            return;
        }

        // We need to add 1 or 2 packing spaces, split between beginning/end, for
        // space-around / space-evenly:
        let numPackingSpacesForEdges: usize = if val == StyleAlignFlags::SPACE_AROUND { 1 } else { 2 };

        // How big will each "full" packing space be:
        let packingSpaceSize: nscoord = *aPackingSpaceRemaining / (*aNumPackingSpacesRemaining as nscoord + numPackingSpacesForEdges as nscoord);
        // How much packing-space are we allocating to the edges:
        let totalEdgePackingSpace: nscoord = numPackingSpacesForEdges as nscoord * packingSpaceSize;

        // Use half of that edge packing space right now:
        *aFirstSubjectOffset += totalEdgePackingSpace / 2;
        // ...but we need to subtract all of it right away, so that we won't
        // hand out any of it to intermediate packing spaces.
        *aPackingSpaceRemaining -= totalEdgePackingSpace;
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::CreateOrClearFlexContainerInfo
    // NOT PORTED: devtools data (ComputedFlexContainerInfo; nsFlexContainerFrame.cpp lines 5074-5095 at
    // GECKO_REV): only Element.getAsFlexContainer() reads it, and Reflow() never gets a containerInfo here.

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::FlexItemConsumedBSize
    // (used by ReflowChildren, so it is translated)
    pub fn FlexItemConsumedBSize(aItem: &FlexItem) -> nscoord {
        // nsSplittableFrame* f = do_QueryFrame(aItem.Frame());
        // NOT PORTED: pagination (ConsumedBSize of an nsSplittableFrame): there are no fragmented items here,
        // so nothing has been consumed.
        let _ = aItem;
        0
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::CreateFlexLineAndFlexItemInfo,
    // ComputeFlexDirections, UpdateFlexLineAndItemInfo, GetFlexFrameWithComputedInfo
    // NOT PORTED: devtools data (nsFlexContainerFrame.cpp lines 5102-5250 at GECKO_REV).
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::IsItemInlineAxisMainAxis
/* static */
impl nsFlexContainerFrame {
    pub fn IsItemInlineAxisMainAxis(aFrame: nsIFrame) -> bool {
        MOZ_ASSERT!(aFrame.IsFlexItem(), "expecting arg to be a flex item");
        let flexItemWM = aFrame.GetWritingMode();
        let flexContainer = aFrame.GetParent().unwrap();

        if flexContainer.IsLegacyWebkitBox() {
            // For legacy boxes, the main axis is determined by "box-orient", and we can
            // just directly check if that's vertical, and compare that to whether the
            // item's WM is also vertical:
            // NOT PORTED: legacy -webkit-box (flexContainer->StyleXUL()->mBoxOrient); never a legacy box here.
            unimplemented!("NOT PORTED: legacy -webkit-box (box-orient)");
        }

        // For modern CSS flexbox, we get our return value by asking two questions
        // and comparing their answers.
        // Question 1: does aFrame have the same inline axis as its flex container?
        let itemInlineAxisIsParallelToParent = !flexItemWM.IsOrthogonalTo(&flexContainer.GetWritingMode());

        // Question 2: is aFrame's flex container row-oriented? (This tells us
        // whether the flex container's main axis is its inline axis.)
        let flexDirection = flexContainer.StylePosition().mFlexDirection();
        let flexContainerIsRowOriented = flexDirection == StyleFlexDirection::Row || flexDirection == StyleFlexDirection::RowReverse;

        // aFrame's inline axis is its flex container's main axis IFF the above
        // questions have the same answer.
        flexContainerIsRowOriented == itemInlineAxisIsParallelToParent
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::IsUsedFlexBasisContent
    /* static */
    pub fn IsUsedFlexBasisContent(aFlexBasis: &StyleFlexBasis, aMainSize: &StyleSize) -> bool {
        // We have a used flex-basis of 'content' if flex-basis explicitly has that
        // value, OR if flex-basis is 'auto' (deferring to the main-size property)
        // and the main-size property is also 'auto'.
        // See https://drafts.csswg.org/css-flexbox-1/#valdef-flex-basis-auto
        if aFlexBasis.IsContent() {
            return true;
        }
        aFlexBasis.IsAuto() && aMainSize.IsAuto()
    }
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::DoFlexLayout
// (the devtools parameter aContainerInfo is dropped: NOT PORTED)
impl nsFlexContainerFrame {
    pub fn DoFlexLayout(&self, aReflowInput: &ReflowInput, aTentativeContentBoxMainSize: nscoord, aTentativeContentBoxCrossSize: nscoord, aAxisTracker: &FlexboxAxisTracker, aMainGapSize: nscoord, aCrossGapSize: nscoord, aStruts: &mut Vec<StrutInfo>) -> FlexLayoutResult {
        let mut flr = FlexLayoutResult::default();

        self.GenerateFlexLines(aReflowInput, aTentativeContentBoxMainSize, aTentativeContentBoxCrossSize, aStruts, aAxisTracker, aMainGapSize, &mut flr.mPlaceholders, &mut flr.mLines, &mut flr.mHasCollapsedItems);

        if (flr.mLines.len() == 1 && flr.mLines[0].IsEmpty()) || aReflowInput.mStyleDisplay.IsContainLayout() {
            // We have no flex items, or we're layout-contained. So, we have no
            // baseline, and our parent should synthesize a baseline if needed.
            self.AddStateBits(NS_STATE_FLEX_SYNTHESIZE_BASELINE);
        } else {
            self.RemoveStateBits(NS_STATE_FLEX_SYNTHESIZE_BASELINE);
        }

        // Construct our computed info if we've been asked to do so. This is
        // necessary to do now so we can capture some computed values for
        // FlexItems during layout that would not otherwise be saved (like
        // size adjustments). We'll later fix up the line properties,
        // because the correct values aren't available yet.
        // NOT PORTED: devtools data (aContainerInfo: CreateFlexLineAndFlexItemInfo, ComputeFlexDirections).

        flr.mContentBoxMainSize = self.ComputeMainSize(aReflowInput, aAxisTracker, aTentativeContentBoxMainSize, &mut flr.mLines);

        for line in flr.mLines.iter_mut() {
            line.ResolveFlexibleLengths(flr.mContentBoxMainSize);
        }

        // Cross Size Determination - Flexbox spec section 9.4
        // https://drafts.csswg.org/css-flexbox-1/#cross-sizing
        // ===================================================
        // Calculate the hypothetical cross size of each item:

        // 'sumLineCrossSizes' includes the size of all gaps between lines. We
        // initialize it with the sum of all the gaps, and add each line's cross size
        // at the end of the following for-loop.
        let mut sumLineCrossSizes: nscoord = aCrossGapSize * (flr.mLines.len() as nscoord - 1);
        for line in flr.mLines.iter_mut() {
            for item in line.Items_mut().iter_mut() {
                // The item may already have the correct cross-size; only recalculate
                // if the item's main size resolution (flexing) could have influenced it:
                if item.CanMainSizeInfluenceCrossSize() {
                    let mut sizeOverrides = StyleSizeOverrides::default();
                    if item.IsInlineAxisMainAxis() {
                        sizeOverrides.mStyleISize = Some(item.StyleMainSize());
                    } else {
                        sizeOverrides.mStyleBSize = Some(item.StyleMainSize());
                    }
                    FLEX_ITEM_LOG!(item.Frame(), "Sizing item in cross axis");
                    FLEX_LOGV!("Main size override: %d", item.MainSize());

                    let wm = item.GetWritingMode();
                    let mut availSize = aReflowInput.ComputedSize__WritingMode(wm);
                    *availSize.BSize_mut(wm) = NS_UNCONSTRAINEDSIZE;
                    let presContext = self.PresContext();
                    let mut childReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                        &presContext,
                        aReflowInput,
                        item.Frame(),
                        availSize,
                        None,
                        EnumSet::default(),
                        sizeOverrides,
                        FlexShrinkWrapFlags(),
                        std::ptr::null_mut(),
                    );
                    if item.IsBlockAxisMainAxis() && item.TreatBSizeAsIndefinite() {
                        childReflowInput.mFlags.mTreatBSizeAsIndefinite = true;
                    }

                    self.SizeItemInCrossAxis(&mut childReflowInput, item);
                }
            }
            // Now that we've finished with this line's items, size the line itself:
            line.ComputeCrossSizeAndBaseline(aAxisTracker);
            sumLineCrossSizes += line.LineCrossSize();
        }

        let mut isCrossSizeDefinite = false;
        flr.mContentBoxCrossSize = self.ComputeCrossSize(aReflowInput, aAxisTracker, aTentativeContentBoxCrossSize, sumLineCrossSizes, &mut isCrossSizeDefinite);

        // Set up state for cross-axis alignment, at a high level (outside the
        // scope of a particular flex line)
        let mut crossAxisPosnTracker = CrossAxisPositionTracker::new(&mut flr.mLines, aReflowInput, flr.mContentBoxCrossSize, isCrossSizeDefinite, aAxisTracker, aCrossGapSize);

        // Now that we know the cross size of each line (including
        // "align-content:stretch" adjustments, from the CrossAxisPositionTracker
        // constructor), we can create struts for any flex items with
        // "visibility: collapse" (and restart flex layout).
        // Make sure to only do this if we had no struts.
        if aStruts.is_empty() && flr.mHasCollapsedItems && !self.StyleVisibility().UseLegacyCollapseBehavior() {
            BuildStrutInfoFromCollapsedItems(&flr.mLines, aStruts);
            if !aStruts.is_empty() {
                // Restart flex layout, using our struts.
                return flr;
            }
        }

        // If the flex container is row-oriented, it should derive its first/last
        // baseline from the WM-relative startmost/endmost FlexLine if any items in
        // the line participate in baseline alignment.
        // https://drafts.csswg.org/css-flexbox-1/#flex-baselines
        //
        // Initialize the relevant variables here so that we can establish baselines
        // while iterating FlexLine later (while crossAxisPosnTracker is conveniently
        // pointing at the cross-start edge of that line, which the line's baseline
        // offset is measured from).
        // ADAPTER: the C++ pointers `const FlexLine*` are indexes into flr.mLines.
        let numLines = flr.mLines.len();
        let mut lineForFirstBaseline: Option<usize> = None;
        let mut lineForLastBaseline: Option<usize> = None;
        if aAxisTracker.IsRowOriented() {
            lineForFirstBaseline = Some(if aAxisTracker.IsCrossAxisReversed() { numLines - 1 } else { 0 });
            lineForLastBaseline = Some(if aAxisTracker.IsCrossAxisReversed() { 0 } else { numLines - 1 });
        } else {
            // For column-oriented flex container, use sentinel value to prompt us to
            // get baselines from the startmost/endmost items.
            flr.mAscent = nscoord_MIN;
            flr.mAscentForLast = nscoord_MIN;
        }

        let justifyContent = if aReflowInput.mFrame.IsLegacyWebkitBox() {
            // NOT PORTED: legacy -webkit-box (ConvertLegacyStyleToJustifyContent(StyleXUL()))
            ConvertLegacyStyleToJustifyContent()
        } else {
            aReflowInput.mStylePosition.mJustifyContent()
        };

        // Flex Container Baselines - Flexbox spec section 8.5
        // https://drafts.csswg.org/css-flexbox-1/#flex-baselines
        // ADAPTER: the C++ lambda ComputeAscentFromLine captures crossAxisPosnTracker, flr and aReflowInput;
        // it is a nested function that takes them as parameters.
        fn ComputeAscentFromLine(aLine: &FlexLine, aBaselineGroup: BaselineSharingGroup, aAxisTracker: &FlexboxAxisTracker, aCrossAxisPosition: nscoord, aContentBoxCrossSize: nscoord, aReflowInput: &ReflowInput) -> nscoord {
            MOZ_ASSERT!(aAxisTracker.IsRowOriented(), "This makes sense only if we are row-oriented!");

            // baselineOffsetInLine is a distance from the line's cross-start edge.
            let baselineOffsetInLine = aLine.ExtractBaselineOffset(aBaselineGroup);

            if baselineOffsetInLine == nscoord_MIN {
                // No "first baseline"-aligned or "last baseline"-aligned items in
                // aLine. Return a sentinel value to prompt us to get baseline from the
                // startmost or endmost FlexItem after we've reflowed it.
                return nscoord_MIN;
            }

            // This "ascent" variable is a distance from the flex container's
            // content-box block-start edge.
            let ascent = aAxisTracker.LogicalAscentFromFlexRelativeAscent(aCrossAxisPosition + baselineOffsetInLine, aContentBoxCrossSize);

            // Convert "ascent" variable to a distance from border-box start or end
            // edge, per documentation for FlexLayoutResult ascent members.
            let wm = aAxisTracker.GetWritingMode();
            if aBaselineGroup == BaselineSharingGroup::First {
                return ascent + aReflowInput.ComputedLogicalBorderPadding(wm).BStart(wm);
            }
            aContentBoxCrossSize - ascent + aReflowInput.ComputedLogicalBorderPadding(wm).BEnd(wm)
        }

        for lineIndex in 0..numLines {
            let line = &mut flr.mLines[lineIndex];
            // Main-Axis Alignment - Flexbox spec section 9.5
            // https://drafts.csswg.org/css-flexbox-1/#main-alignment
            // ==============================================
            line.PositionItemsInMainAxis(justifyContent, flr.mContentBoxMainSize, aAxisTracker);

            // See if we need to extract some computed info for this line.
            // NOT PORTED: devtools data (lineInfo.mCrossStart = crossAxisPosnTracker.Position()).

            // Cross-Axis Alignment - Flexbox spec section 9.6
            // https://drafts.csswg.org/css-flexbox-1/#cross-alignment
            // ===============================================
            line.PositionItemsInCrossAxis(crossAxisPosnTracker.Position(), aAxisTracker);

            if lineForFirstBaseline == Some(lineIndex) {
                flr.mAscent = ComputeAscentFromLine(line, BaselineSharingGroup::First, aAxisTracker, crossAxisPosnTracker.Position(), flr.mContentBoxCrossSize, aReflowInput);
            }
            if lineForLastBaseline == Some(lineIndex) {
                flr.mAscentForLast = ComputeAscentFromLine(line, BaselineSharingGroup::Last, aAxisTracker, crossAxisPosnTracker.Position(), flr.mContentBoxCrossSize, aReflowInput);
            }

            crossAxisPosnTracker.TraverseLine(line);
            crossAxisPosnTracker.TraversePackingSpace();

            if lineIndex != numLines - 1 {
                crossAxisPosnTracker.TraverseGap();
            }
        }

        flr
    }
}

// This data structure is used in fragmentation, storing the block coordinate
// metrics when reflowing 1) the BStart-most line in each fragment of a
// row-oriented flex container or, 2) the BStart-most item in each fragment of a
// single-line column-oriented flex container.
//
// When we lay out a row-oriented flex container fragment, its first line might
// contain one or more monolithic items that were pushed from the previous
// fragment specifically to avoid having those monolithic items overlap the
// page/column break. The situation is similar for single-row column-oriented
// flex container fragments, but a bit simpler; only their first item might have
// been pushed to avoid overlapping a page/column break.
//
// We'll have to place any such pushed items at the block-start edge of the
// current fragment's content-box, which is as close as we can get them to their
// theoretical/unfragmented position (without slicing them); but it does
// represent a shift away from their theoretical/unfragmented position (which
// was somewhere in the previous fragment).
//
// When that happens, we need to record the maximum such shift that we had to
// perform so that we can apply the same block-endwards shift to "downstream"
// items (items towards the block-end edge) that we could otherwise collide
// with. We also potentially apply the same shift when computing the block-end
// edge of this flex container fragment's content-box so that we don't
// inadvertently shift the last item (or line-of-items) to overlap the flex
// container's border, or content beyond the flex container.
//
// We use this structure to keep track of several metrics, in service of this
// goal. This structure is also necessary to adjust PerFragmentFlexData at the
// end of ReflowChildren().
//
// Note: "First" in the struct name means "BStart-most", not the order in the
// flex line array or flex item array.
// Gecko: layout/generic/nsFlexContainerFrame.cpp FirstLineOrFirstItemBAxisMetrics
#[derive(Default)]
pub struct FirstLineOrFirstItemBAxisMetrics {
    // This value stores the block-end edge shift for 1) the BStart-most line in
    // the current fragment of a row-oriented flex container, or 2) the
    // BStart-most item in the current fragment of a single-line column-oriented
    // flex container. This number is non-negative.
    //
    // This value may become positive when any item is a first-in-flow and also
    // satisfies either the above condition 1) or 2), since that's a hint that it
    // could be monolithic or have a monolithic first descendant, and therefore an
    // item that might incur a page/column-break-dodging position-shift that this
    // variable needs to track.
    //
    // This value also stores the fragmentation-imposed growth in the block-size
    // of a) the BStart-most line in the current fragment of a row-oriented flex
    // container, or b) the BStart-most item in the current fragment of a
    // single-line column-oriented flex container. This number is non-negative.
    pub mBEndEdgeShift: nscoord,

    // The first and second value in the pair store the max block-end edges for
    // items before and after applying the per-item position-shift in the block
    // axis. We only record the block-end edges for items with first-in-flow
    // frames placed in the current flex container fragment. This is used only by
    // row-oriented flex containers.
    pub mMaxBEndEdge: Option<(nscoord, nscoord)>,
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::ReflowChildren
impl nsFlexContainerFrame {
    pub fn ReflowChildren(&self, aReflowInput: &ReflowInput, aContainerSize: nsSize, aAvailableSizeForItems: LogicalSize, aBorderPadding: LogicalMargin, aAxisTracker: &FlexboxAxisTracker, aFlr: &mut FlexLayoutResult, aFragmentData: &mut PerFragmentFlexData) -> (nscoord, nsReflowStatus) {
        use std::collections::HashSet;

        if self.HidesContentForLayout() {
            return (0, nsReflowStatus::new());
        }

        // Before giving each child a final reflow, calculate the origin of the
        // flex container's content box (with respect to its border-box), so that
        // we can compute our flex item's final positions.
        let flexWM = aReflowInput.GetWritingMode();
        let containerContentBoxOrigin = aBorderPadding.StartOffset(flexWM);

        // The block-end of children is relative to the flex container's border-box.
        let mut maxBlockEndEdgeOfChildren = containerContentBoxOrigin.B(flexWM);

        let mut bAxisMetrics = FirstLineOrFirstItemBAxisMetrics::default();
        let mut pushedItems: HashSet<nsIFrame> = HashSet::new();
        let mut incompleteItems: HashSet<nsIFrame> = HashSet::new();
        let mut overflowIncompleteItems: HashSet<nsIFrame> = HashSet::new();

        let isSingleLine = IsSingleLine(aReflowInput.mFrame, &aReflowInput.mStylePosition);
        // ADAPTER: the C++ pointers `const FlexLine&` / `const FlexItem*` that are compared by address are
        // indexes into aFlr.mLines and into the line's items.
        let numLines = aFlr.mLines.len();
        let startmostLineIdx: usize = if aAxisTracker.IsCrossAxisReversed() { numLines - 1 } else { 0 };
        let endmostLineIdx: usize = if aAxisTracker.IsCrossAxisReversed() { 0 } else { numLines - 1 };
        let startmostLine = &aFlr.mLines[startmostLineIdx];
        let endmostLine = &aFlr.mLines[endmostLineIdx];
        let startmostItemIdx: Option<usize> = if startmostLine.IsEmpty() { None } else { Some(if aAxisTracker.IsMainAxisReversed() { startmostLine.NumItems() as usize - 1 } else { 0 }) };
        let endmostItemIdx: Option<usize> = if endmostLine.IsEmpty() { None } else { Some(if aAxisTracker.IsMainAxisReversed() { 0 } else { endmostLine.NumItems() as usize - 1 }) };

        let mut endmostItemOrLineHasBreakAfter = false;
        // If true, push all remaining flex items to the container's next-in-flow.
        let mut shouldPushRemainingItems = false;

        // FINAL REFLOW: Give each child frame another chance to reflow.
        for lineIdx in 0..numLines {
            // Iterate flex lines from the startmost to endmost (relative to flex
            // container's writing-mode).
            let actualLineIdx = if aAxisTracker.IsCrossAxisReversed() { numLines - lineIdx - 1 } else { lineIdx };
            let line = &aFlr.mLines[actualLineIdx];
            MOZ_ASSERT!(lineIdx != 0 || actualLineIdx == startmostLineIdx, "Logic for finding startmost line should be consistent!");
            let isStartmostLine = actualLineIdx == startmostLineIdx;
            let isEndmostLine = actualLineIdx == endmostLineIdx;

            // These two variables can be set when we are a row-oriented flex container
            // during fragmentation.
            let mut lineHasBreakBefore = false;
            let mut lineHasBreakAfter = false;

            let numItems = line.Items().len();
            for itemIdx in 0..numItems {
                // Iterate flex items from the startmost to endmost (relative to flex
                // container's writing-mode).
                let actualItemIdx = if aAxisTracker.IsMainAxisReversed() { numItems - itemIdx - 1 } else { itemIdx };
                let item = &line.Items()[actualItemIdx];
                MOZ_ASSERT!(lineIdx != 0 || itemIdx != 0 || Some(actualItemIdx) == startmostItemIdx, "Logic for finding startmost item should be consistent!");
                let isStartmostItem = isStartmostLine && Some(actualItemIdx) == startmostItemIdx;
                let isEndmostItem = isEndmostLine && Some(actualItemIdx) == endmostItemIdx;

                let mut framePos = aAxisTracker.LogicalPointFromFlexRelativePoint(item.MainPosition(), item.CrossPosition(), aFlr.mContentBoxMainSize, aFlr.mContentBoxCrossSize);
                // This variable records the item's block-end edge before we give it a
                // per-item-position-shift, if the item is a first-in-flow in the
                // startmost line of a row-oriented flex container fragment. It is used to
                // determine the block-end edge shift for the startmost line at the end of
                // the outer loop.
                let mut frameBPosBeforePerItemShift: Option<nscoord> = None;

                if item.Frame().GetPrevInFlow().is_some() {
                    // The item is a continuation. Lay it out at the beginning of the
                    // available space.
                    *framePos.B_mut(flexWM) = 0;
                } else if self.GetPrevInFlow().is_some() {
                    // The item we're placing is not a continuation; though we're placing it
                    // into a flex container fragment which *is* a continuation. To compute
                    // the item's correct position in this fragment, we adjust the item's
                    // theoretical/unfragmented block-direction position by subtracting the
                    // cumulative content-box block-size for all the previous fragments and
                    // adding the cumulative block-end edge shift.
                    //
                    // Note that the item's position in this fragment has not been finalized
                    // yet. At this point, we've adjusted the item's
                    // theoretical/unfragmented position to be relative to the block-end
                    // edge of the previous container fragment's content-box. Later, we'll
                    // compute per-item position-shift to finalize its position.
                    *framePos.B_mut(flexWM) -= aFragmentData.mCumulativeContentBoxBSize;
                    *framePos.B_mut(flexWM) += aFragmentData.mCumulativeBEndEdgeShift;

                    // This helper gets the per-item position-shift in the block-axis.
                    let GetPerItemPositionShiftToBEnd = |aFramePos: LogicalPoint| -> nscoord {
                        if aFramePos.B(flexWM) >= 0 {
                            // The item final position might be in current flex container
                            // fragment or in any of the later fragments. No adjustment needed.
                            return 0;
                        }

                        // The item's block position is negative, but we want to place it at
                        // the content-box block-start edge of this container fragment. To
                        // achieve this, return a negated (positive) value to make the final
                        // block position zero.
                        //
                        // This scenario occurs when fragmenting a row-oriented flex container
                        // where this item is pushed to this container fragment.
                        -aFramePos.B(flexWM)
                    };

                    if aAxisTracker.IsRowOriented() {
                        if isStartmostLine {
                            frameBPosBeforePerItemShift = Some(framePos.B(flexWM));
                            *framePos.B_mut(flexWM) += GetPerItemPositionShiftToBEnd(framePos);
                        } else {
                            // We've computed two things for the startmost line during the outer
                            // loop's first iteration: 1) how far the block-end edge had to
                            // shift and 2) how large the block-size needed to grow. Here, we
                            // just shift all items in the rest of the lines the same amount.
                            *framePos.B_mut(flexWM) += bAxisMetrics.mBEndEdgeShift;
                        }
                    } else {
                        MOZ_ASSERT!(aAxisTracker.IsColumnOriented());
                        if isSingleLine {
                            if isStartmostItem {
                                bAxisMetrics.mBEndEdgeShift = GetPerItemPositionShiftToBEnd(framePos);
                            }
                            *framePos.B_mut(flexWM) += bAxisMetrics.mBEndEdgeShift;
                        } else {
                            // Bug 1806717: We need a more sophisticated solution for multi-line
                            // column-oriented flex container when each line has a different
                            // position-shift value. For now, we don't shift them.
                        }
                    }
                }

                // Adjust available block-size for the item. (We compute it here because
                // framePos is still relative to the container's content-box.)
                //
                // Note: The available block-size can become negative if item's
                // block-direction position is below available space's block-end.
                let availableBSizeForItem = if aAvailableSizeForItems.BSize(flexWM) == NS_UNCONSTRAINEDSIZE { NS_UNCONSTRAINEDSIZE } else { aAvailableSizeForItems.BSize(flexWM) - framePos.B(flexWM) };

                // Adjust framePos to be relative to the container's border-box
                // (i.e. its frame rect), instead of the container's content-box:
                framePos += containerContentBoxOrigin;

                // Check if we can skip reflowing the item because it will be pushed to
                // our next-in-flow -- i.e. if there was a forced break before it, or its
                // position is beyond the available space's block-end.
                let mut itemInPushedItems = false;
                if shouldPushRemainingItems {
                    FLEX_ITEM_LOG!(item.Frame(), "[frag] Item needed to be pushed to container's next-in-flow due to a forced break before it");
                    pushedItems.insert(item.Frame());
                    itemInPushedItems = true;
                } else if availableBSizeForItem != NS_UNCONSTRAINEDSIZE && availableBSizeForItem <= 0 {
                    // The item's position is beyond the available space, so we have to push
                    // it.
                    //
                    // Note: Even if all of our items are beyond the available space & get
                    // pushed here, we'll be guaranteed to place at least one of them (and
                    // make progress) in one of the flex container's *next* fragment. It's
                    // because ComputeAvailableSizeForItems() always reserves at least 1px
                    // available block-size for its children, and we consume all available
                    // block-size and add it to
                    // PerFragmentFlexData::mCumulativeContentBoxBSize even if we are not
                    // laying out any child.
                    FLEX_ITEM_LOG!(item.Frame(), "[frag] Item needed to be pushed to container's next-in-flow due to being positioned beyond block-end edge of available space");
                    pushedItems.insert(item.Frame());
                    itemInPushedItems = true;
                } else if item.NeedsFinalReflow(aReflowInput) {
                    // The available size must be in item's writing-mode.
                    let itemWM = item.GetWritingMode();
                    let availableSize = LogicalSize::new(flexWM, aAvailableSizeForItems.ISize(flexWM), availableBSizeForItem).ConvertTo(itemWM, flexWM);

                    let isAdjacentWithBStart = framePos.B(flexWM) == containerContentBoxOrigin.B(flexWM);
                    let childStatus = self.ReflowFlexItem(aAxisTracker, aReflowInput, item, framePos, isAdjacentWithBStart, availableSize, aContainerSize);

                    if aReflowInput.IsInFragmentedContext() {
                        let itemHasBreakBefore = item.Frame().ShouldBreakBefore(aReflowInput.mBreakType) || childStatus.IsInlineBreakBefore();
                        if itemHasBreakBefore {
                            if aAxisTracker.IsRowOriented() {
                                lineHasBreakBefore = true;
                            } else if isSingleLine {
                                if isStartmostItem {
                                    if self.GetPrevInFlow().is_none() && !aReflowInput.mFlags.mIsTopOfPage {
                                        // If we are first-in-flow and not at top-of-page, early
                                        // return here to propagate forced break-before from the
                                        // startmost item to the flex container.
                                        let mut childrenStatus = nsReflowStatus::new();
                                        childrenStatus.SetInlineLineBreakBeforeAndReset();
                                        return (0, childrenStatus);
                                    }
                                } else {
                                    shouldPushRemainingItems = true;
                                }
                            } else {
                                // Bug 1806717: We haven't implemented fragmentation for
                                // multi-line column-oriented flex container, so we just ignore
                                // forced breaks for now.
                            }
                        }
                    }

                    let shouldPushItem = (|| -> bool {
                        if shouldPushRemainingItems {
                            return true;
                        }
                        if availableBSizeForItem == NS_UNCONSTRAINEDSIZE {
                            // If the available block-size is unconstrained, then we're not
                            // fragmenting and we don't want to push the item.
                            return false;
                        }
                        if isAdjacentWithBStart {
                            // The flex item is adjacent with block-start of the container's
                            // content-box. Don't push it, or we'll trap in an infinite loop.
                            return false;
                        }
                        if item.Frame().BSize(item.Frame().GetWritingMode()) <= availableBSizeForItem {
                            return false;
                        }
                        // NOT PORTED: `aAxisTracker.IsColumnOriented() && item.Frame()->StyleDisplay()->mBreakBefore ==
                        // StyleBreakBetween::Avoid` (break-before is not read from the style: never 'avoid').
                        true
                    })();
                    if shouldPushItem {
                        FLEX_ITEM_LOG!(item.Frame(), "[frag] Item needed to be pushed to container's next-in-flow because it encounters a forced break before it, or its block-size is larger than the available space");
                        pushedItems.insert(item.Frame());
                        itemInPushedItems = true;
                    } else if childStatus.IsIncomplete() {
                        incompleteItems.insert(item.Frame());
                    } else if childStatus.IsOverflowIncomplete() {
                        overflowIncompleteItems.insert(item.Frame());
                    }

                    if aReflowInput.IsInFragmentedContext() {
                        let itemHasBreakAfter = item.Frame().ShouldBreakAfter(aReflowInput.mBreakType) || childStatus.IsInlineBreakAfter();
                        if itemHasBreakAfter {
                            if aAxisTracker.IsRowOriented() {
                                lineHasBreakAfter = true;
                            } else if isSingleLine {
                                shouldPushRemainingItems = true;
                                if isEndmostItem {
                                    endmostItemOrLineHasBreakAfter = true;
                                }
                            } else {
                                // Bug 1806717: We haven't implemented fragmentation for
                                // multi-line column-oriented flex container, so we just ignore
                                // forced breaks for now.
                            }
                        }
                    }
                } else {
                    // We already reflowed the item with the right content-box size, so we
                    // can simply move it into place.
                    self.MoveFlexItemToFinalPosition(item, framePos, aContainerSize);
                }

                if !itemInPushedItems {
                    let borderBoxBSize = item.Frame().BSize(flexWM);
                    let bEndEdgeAfterPerItemShift = framePos.B(flexWM) + borderBoxBSize;

                    // The item (or a fragment thereof) was placed in this flex container
                    // fragment. Update the max block-end edge with the item's block-end
                    // edge.
                    maxBlockEndEdgeOfChildren = std::cmp::max(maxBlockEndEdgeOfChildren, bEndEdgeAfterPerItemShift);

                    if let Some(frameBPosBeforePerItemShift) = frameBPosBeforePerItemShift {
                        // Make the block-end edge relative to flex container's border-box
                        // because bEndEdgeAfterPerItemShift is relative to the border-box.
                        let bEndEdgeBeforePerItemShift = containerContentBoxOrigin.B(flexWM) + frameBPosBeforePerItemShift + borderBoxBSize;

                        if let Some((before, after)) = &mut bAxisMetrics.mMaxBEndEdge {
                            *before = std::cmp::max(*before, bEndEdgeBeforePerItemShift);
                            *after = std::cmp::max(*after, bEndEdgeAfterPerItemShift);
                        } else {
                            bAxisMetrics.mMaxBEndEdge = Some((bEndEdgeBeforePerItemShift, bEndEdgeAfterPerItemShift));
                        }
                    }

                    if item.Frame().GetPrevInFlow().is_some() {
                        // Items with a previous-continuation may experience some
                        // fragmentation-imposed growth in their block-size; we compute that
                        // here.
                        let bSizeOfThisFragment = item.Frame().ContentSize(flexWM).BSize(flexWM);
                        let consumedBSize = nsFlexContainerFrame::FlexItemConsumedBSize(item);
                        let unfragmentedBSize = item.BSize();
                        let mut bSizeGrowthOfThisFragment: nscoord = 0;

                        if consumedBSize >= unfragmentedBSize {
                            // The item's block-size has been grown to exceed the unfragmented
                            // block-size in the previous fragments.
                            bSizeGrowthOfThisFragment = bSizeOfThisFragment;
                        } else if consumedBSize + bSizeOfThisFragment >= unfragmentedBSize {
                            // The item's block-size just grows in the current fragment to
                            // exceed the unfragmented block-size.
                            bSizeGrowthOfThisFragment = consumedBSize + bSizeOfThisFragment - unfragmentedBSize;
                        }

                        if aAxisTracker.IsRowOriented() {
                            if isStartmostLine {
                                bAxisMetrics.mBEndEdgeShift = std::cmp::max(bAxisMetrics.mBEndEdgeShift, bSizeGrowthOfThisFragment);
                            }
                        } else {
                            MOZ_ASSERT!(aAxisTracker.IsColumnOriented());
                            if isSingleLine {
                                if isStartmostItem {
                                    MOZ_ASSERT!(bAxisMetrics.mBEndEdgeShift == 0, "The item's frame is a continuation, so it shouldn't shift!");
                                    bAxisMetrics.mBEndEdgeShift = bSizeGrowthOfThisFragment;
                                }
                            } else {
                                // Bug 1806717: We need a more sophisticated solution for
                                // multi-line column-oriented flex container when each line has a
                                // different block-size growth value. For now, we don't deal with
                                // them.
                            }
                        }
                    }
                }

                // If the item has auto margins, and we were tracking the UsedMargin
                // property, set the property to the computed margin values.
                if item.HasAnyAutoMargin() {
                    if item.Frame().GetProperty(nsIFrame::UsedMarginProperty()).is_some() {
                        item.Frame().SetProperty(nsIFrame::UsedMarginProperty(), item.PhysicalMargin());
                    }
                }
            }

            if aReflowInput.IsInFragmentedContext() && aAxisTracker.IsRowOriented() {
                // Propagate forced break values from the flex items to its flex line.
                if lineHasBreakBefore {
                    if isStartmostLine {
                        if self.GetPrevInFlow().is_none() && !aReflowInput.mFlags.mIsTopOfPage {
                            // If we are first-in-flow and not at top-of-page, early return here
                            // to propagate forced break-before from the startmost line to the
                            // flex container.
                            let mut childrenStatus = nsReflowStatus::new();
                            childrenStatus.SetInlineLineBreakBeforeAndReset();
                            return (0, childrenStatus);
                        }
                    } else {
                        // Current non-startmost line has forced break-before, so push all the
                        // items in this line.
                        for item in line.Items() {
                            pushedItems.insert(item.Frame());
                            incompleteItems.remove(&item.Frame());
                            overflowIncompleteItems.remove(&item.Frame());
                        }
                        shouldPushRemainingItems = true;
                    }
                }
                if lineHasBreakAfter {
                    shouldPushRemainingItems = true;
                    if isEndmostLine {
                        endmostItemOrLineHasBreakAfter = true;
                    }
                }
            }

            // Now we've finished processing all the items in the startmost line.
            // Determine the amount by which the startmost line's block-end edge has
            // shifted, so we can apply the same shift for the remaining lines.
            if self.GetPrevInFlow().is_some() && aAxisTracker.IsRowOriented() && isStartmostLine {
                if let Some((before, after)) = bAxisMetrics.mMaxBEndEdge {
                    bAxisMetrics.mBEndEdgeShift = std::cmp::max(bAxisMetrics.mBEndEdgeShift, after - before);
                }
            }
        }

        if !aFlr.mPlaceholders.is_empty() {
            self.ReflowPlaceholders(aReflowInput, &mut aFlr.mPlaceholders, containerContentBoxOrigin, aContainerSize);
        }

        let mut childrenStatus = nsReflowStatus::new();
        if !pushedItems.is_empty() || !incompleteItems.is_empty() {
            childrenStatus.SetIncomplete();
        } else if !overflowIncompleteItems.is_empty() {
            childrenStatus.SetOverflowIncomplete();
        } else if endmostItemOrLineHasBreakAfter {
            childrenStatus.SetInlineLineBreakAfter(UsedClear::None);
        }
        // ADAPTER: PushIncompleteChildren is NOT PORTED (fragmentation); with no pushed or incomplete item
        // it has nothing to do, so it is only called when there is something to push.
        if !pushedItems.is_empty() || !incompleteItems.is_empty() || !overflowIncompleteItems.is_empty() {
            self.PushIncompleteChildren(&pushedItems, &incompleteItems, &overflowIncompleteItems);
        }

        // TODO: Try making this a fatal assertion after we fix bug 1751260.
        NS_ASSERTION!(childrenStatus.IsFullyComplete() || aAvailableSizeForItems.BSize(flexWM) != NS_UNCONSTRAINEDSIZE, "We shouldn't have any incomplete children if the available block-size is unconstrained!");

        if !pushedItems.is_empty() {
            self.AddStateBits(NS_STATE_FLEX_DID_PUSH_ITEMS);
        }

        if self.GetPrevInFlow().is_some() {
            aFragmentData.mCumulativeBEndEdgeShift += bAxisMetrics.mBEndEdgeShift;
        }

        (maxBlockEndEdgeOfChildren, childrenStatus)
    }
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::PopulateReflowOutput
impl nsFlexContainerFrame {
    pub fn PopulateReflowOutput(&self, aReflowOutput: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus, aContentBoxSize: LogicalSize, aBorderPadding: LogicalMargin, aConsumedBSize: nscoord, aMayNeedNextInFlow: bool, aMaxBlockEndEdgeOfChildren: nscoord, aChildrenStatus: &nsReflowStatus, aAxisTracker: &FlexboxAxisTracker, aFlr: &mut FlexLayoutResult) {
        let flexWM = aReflowInput.GetWritingMode();

        // Compute flex container's desired size (in its own writing-mode).
        let mut desiredSizeInFlexWM = LogicalSize::new__WritingMode(flexWM);
        *desiredSizeInFlexWM.ISize_mut(flexWM) = aContentBoxSize.ISize(flexWM) + aBorderPadding.IStartEnd(flexWM);

        // Unconditionally skip adding block-end border and padding for now. We add it
        // lower down, after we've established baseline and decided whether bottom
        // border-padding fits (if we're fragmented).
        let effectiveContentBSizeWithBStartBP = aContentBoxSize.BSize(flexWM) - aConsumedBSize + aBorderPadding.BStart(flexWM);
        let mut blockEndContainerBP = aBorderPadding.BEnd(flexWM);

        if aMayNeedNextInFlow {
            // We assume our status should be reported as incomplete because we may need
            // a next-in-flow.
            let mut isStatusIncomplete = true;

            let availableBSizeMinusBEndBP = aReflowInput.AvailableBSize() - aBorderPadding.BEnd(flexWM);

            if aMaxBlockEndEdgeOfChildren <= availableBSizeMinusBEndBP {
                // Consume all the available block-size.
                *desiredSizeInFlexWM.BSize_mut(flexWM) = availableBSizeMinusBEndBP;
            } else {
                // This case happens if we have some tall unbreakable children exceeding
                // the available block-size.
                *desiredSizeInFlexWM.BSize_mut(flexWM) = std::cmp::min(effectiveContentBSizeWithBStartBP, aMaxBlockEndEdgeOfChildren);

                if (aReflowInput.ComputedBSize() != NS_UNCONSTRAINEDSIZE || aChildrenStatus.IsFullyComplete()) && aMaxBlockEndEdgeOfChildren >= effectiveContentBSizeWithBStartBP {
                    // We have some tall unbreakable child that's sticking off the end of
                    // our fragment, *and* forcing us to consume all of our remaining
                    // content block-size and call ourselves complete.
                    //
                    // - If we have a definite block-size: we get here if the tall child
                    //   makes us reach that block-size.
                    // - If we have a content-based block-size: we get here if the tall
                    //   child makes us reach the content-based block-size from a
                    //   theoretical unfragmented layout, *and* all our children are
                    //   complete. (Note that if we have some incomplete child, then we
                    //   instead prefer to return an incomplete status, so we can get a
                    //   next-in-flow to include that child's requested next-in-flow, in the
                    //   spirit of having a block-size that fits the content.)
                    //
                    // TODO: the auto-height case might need more subtlety; see bug 1828977.
                    isStatusIncomplete = false;

                    // We also potentially need to get the unskipped block-end border and
                    // padding (if we assumed it'd be skipped as part of our tentative
                    // assumption that we'd be incomplete).
                    if aReflowInput.mStyleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Slice {
                        blockEndContainerBP = aReflowInput.ComputedLogicalBorderPadding(flexWM).BEnd(flexWM);
                    }
                }
            }

            if isStatusIncomplete {
                aStatus.SetIncomplete();
            }
        } else {
            // Our own effective content-box block-size can fit within the available
            // block-size.
            *desiredSizeInFlexWM.BSize_mut(flexWM) = effectiveContentBSizeWithBStartBP;
        }

        // Now, we account for how the block-end border and padding (if any) impacts
        // our desired size. If adding it pushes us over the available block-size,
        // then we become incomplete (unless we already weren't asking for any
        // block-size, in which case we stay complete to avoid looping forever).
        //
        // NOTE: If we have auto block-size, we allow our block-end border and padding
        // to push us over the available block-size without requesting a continuation,
        // for consistency with the behavior of "display:block" elements.
        let effectiveContentBSizeWithBStartEndBP = desiredSizeInFlexWM.BSize(flexWM) + blockEndContainerBP;

        if aReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE && effectiveContentBSizeWithBStartEndBP > aReflowInput.AvailableBSize() && desiredSizeInFlexWM.BSize(flexWM) != 0 && aReflowInput.ComputedBSize() != NS_UNCONSTRAINEDSIZE {
            // We couldn't fit with the block-end border and padding included, so we'll
            // need a continuation.
            aStatus.SetIncomplete();

            if aReflowInput.mStyleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Slice {
                blockEndContainerBP = 0;
            }
        }

        // The variable "blockEndContainerBP" now accurately reflects how much (if
        // any) block-end border and padding we want for this frame, so we can proceed
        // to add it in.
        *desiredSizeInFlexWM.BSize_mut(flexWM) += blockEndContainerBP;

        if aStatus.IsComplete() && !aChildrenStatus.IsFullyComplete() {
            aStatus.SetOverflowIncomplete();
            aStatus.SetNextInFlowNeedsReflow();
        }

        // If we are the first-in-flow and not fully complete (either our block-size
        // or any of our flex items cannot fit in the available block-size), and the
        // style requires us to avoid breaking inside, set the status to prompt our
        // parent to push us to the next page/column.
        if self.GetPrevInFlow().is_none() && !aStatus.IsFullyComplete() && self.ShouldAvoidBreakInside(aReflowInput) {
            aStatus.SetInlineLineBreakBeforeAndReset();
            return;
        }

        // Propagate forced break values from flex items or flex lines.
        if aChildrenStatus.IsInlineBreakBefore() {
            aStatus.SetInlineLineBreakBeforeAndReset();
        }
        if aChildrenStatus.IsInlineBreakAfter() {
            aStatus.SetInlineLineBreakAfter(UsedClear::None);
        }

        // If we haven't established a baseline for the container yet, i.e. if we
        // don't have any flex item in the startmost flex line that participates in
        // baseline alignment, then use the startmost flex item to derive the
        // container's baseline.
        {
            let line = StartmostLine(&aFlr.mLines, aAxisTracker);
            if aFlr.mAscent == nscoord_MIN && !line.IsEmpty() {
                let item = line.StartmostItem(aAxisTracker);
                aFlr.mAscent = item.Frame().GetLogicalPosition__WritingMode_nsSize(flexWM, desiredSizeInFlexWM.GetPhysicalSize(flexWM)).B(flexWM) + item.ResolvedAscent(true);
            }
        }

        // Likewise, if we don't have any flex item in the endmost flex line that
        // participates in last baseline alignment, then use the endmost flex item to
        // derived the container's last baseline.
        {
            let line = EndmostLine(&aFlr.mLines, aAxisTracker);
            if aFlr.mAscentForLast == nscoord_MIN && !line.IsEmpty() {
                let item = line.EndmostItem(aAxisTracker);
                let lastAscent = item.Frame().GetLogicalPosition__WritingMode_nsSize(flexWM, desiredSizeInFlexWM.GetPhysicalSize(flexWM)).B(flexWM) + item.ResolvedAscent(false);

                aFlr.mAscentForLast = desiredSizeInFlexWM.BSize(flexWM) - lastAscent;
            }
        }

        if aFlr.mAscent == nscoord_MIN {
            // Still don't have our baseline set -- this happens if we have no
            // children, if our children are huge enough that they have nscoord_MIN
            // as their baseline, or our content is hidden in which case, we'll use the
            // wrong baseline (but no big deal).
            NS_WARNING_ASSERTION!(self.HidesContentForLayout() || aFlr.mLines[0].IsEmpty(), "Have flex items but didn't get an ascent - that's odd (or there are just gigantic sizes involved)");
            // Per spec, synthesize baseline from the flex container's content box
            // (i.e. use block-end side of content-box)
            // XXXdholbert This only makes sense if parent's writing mode is
            // horizontal (& even then, really we should be using the BSize in terms
            // of the parent's writing mode, not ours). Clean up in bug 1155322.
            aFlr.mAscent = effectiveContentBSizeWithBStartBP;
        }

        if aFlr.mAscentForLast == nscoord_MIN {
            // Still don't have our last baseline set -- this happens if we have no
            // children, if our children are huge enough that they have nscoord_MIN
            // as their baseline, or our content is hidden in which case, we'll use the
            // wrong baseline (but no big deal).
            NS_WARNING_ASSERTION!(self.HidesContentForLayout() || aFlr.mLines[0].IsEmpty(), "Have flex items but didn't get an ascent - that's odd (or there are just gigantic sizes involved)");
            // Per spec, synthesize baseline from the flex container's content box
            // (i.e. use block-end side of content-box)
            // XXXdholbert This only makes sense if parent's writing mode is
            // horizontal (& even then, really we should be using the BSize in terms
            // of the parent's writing mode, not ours). Clean up in bug 1155322.
            aFlr.mAscentForLast = blockEndContainerBP;
        }

        if self.HasAnyStateBits(NS_STATE_FLEX_SYNTHESIZE_BASELINE) {
            // This will force our parent to call GetLogicalBaseline, which will
            // synthesize a margin-box baseline.
            aReflowOutput.SetBlockStartAscent(ReflowOutput::ASK_FOR_BASELINE);
        } else {
            // XXXdholbert aFlr.mAscent needs to be in terms of our parent's
            // writing-mode here. See bug 1155322.
            aReflowOutput.SetBlockStartAscent(aFlr.mAscent);
        }

        // Cache the container baselines so that our parent can baseline-align us.
        self.SetFirstBaseline(aFlr.mAscent);
        self.SetLastBaseline(aFlr.mAscentForLast);

        // Convert flex container's final desired size to parent's WM, for outparam.
        aReflowOutput.SetSize(flexWM, &desiredSizeInFlexWM);
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::MoveFlexItemToFinalPosition
    pub fn MoveFlexItemToFinalPosition(&self, aItem: &FlexItem, aFramePos: LogicalPoint, aContainerSize: nsSize) {
        let outerWM = aItem.ContainingBlockWM();
        let display = aItem.Frame().StyleDisplay();
        let mut pos = aFramePos;
        if display.IsRelativelyOrStickyPositionedStyle() {
            // If the item is relatively positioned, look up its offsets (cached from
            // previous reflow). A sticky positioned item can pass a dummy
            // logicalOffsets into ApplyRelativePositioning().
            let mut logicalOffsets = LogicalMargin::new__WritingMode(outerWM);
            if display.IsRelativelyPositionedStyle() {
                let cachedOffsets = aItem.Frame().GetProperty(nsIFrame::ComputedOffsetProperty());
                MOZ_ASSERT!(cachedOffsets.is_some(), "relpos previously-reflowed frame should've cached its offsets");
                logicalOffsets = LogicalMargin::new__WritingMode_nsMargin(outerWM, cachedOffsets.unwrap());
            }
            ReflowInput::ApplyRelativePositioning__nsIFrame_WritingMode_LogicalMargin_LogicalPoint_nsSize(aItem.Frame(), outerWM, logicalOffsets, &mut pos, aContainerSize);
        }

        FLEX_ITEM_LOG!(aItem.Frame(), "Moving item to its desired position %s", ToString(pos).c_str());
        aItem.Frame().SetPosition__WritingMode_LogicalPoint_nsSize(outerWM, pos, aContainerSize);
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::ReflowFlexItem
    pub fn ReflowFlexItem(&self, aAxisTracker: &FlexboxAxisTracker, aReflowInput: &ReflowInput, aItem: &FlexItem, aFramePos: LogicalPoint, aIsAdjacentWithBStart: bool, aAvailableSize: LogicalSize, aContainerSize: nsSize) -> nsReflowStatus {
        FLEX_ITEM_LOG!(aItem.Frame(), "Doing final reflow");

        // Returns true if we should use 'auto' in block axis's StyleSizeOverrides to
        // allow fragmentation-imposed block-size growth.
        let ComputeBSizeOverrideWithAuto = || -> bool {
            if !aReflowInput.IsInFragmentedContext() {
                return false;
            }
            if aItem.Frame().IsReplaced() {
                // Disallow fragmentation-imposed block-size growth for replaced elements
                // since they are monolithic, and cannot be fragmented.
                return false;
            }
            if aItem.HasAspectRatio() {
                // Aspect-ratio's automatic content-based minimum size doesn't work
                // properly in a fragmented context (Bug 1868284) when we use 'auto'
                // block-size to apply the fragmentation-imposed block-size growth.
                // Disable it for now so that items with aspect-ratios can still use their
                // known block-sizes (from flex layout algorithm) in final reflow.
                return false;
            }
            if aItem.IsBlockAxisMainAxis() {
                if aItem.IsFlexBaseSizeContentBSize() {
                    // The flex item resolved its indefinite flex-basis to the content
                    // block-size.
                    if aItem.IsMainMinSizeContentBSize() {
                        // The item's flex base size and main min-size are both content
                        // block-size. We interpret this content-based block-size as
                        // permission to apply fragmentation-imposed block-size growth.
                        return true;
                    }
                    if aReflowInput.ComputedBSize() == NS_UNCONSTRAINEDSIZE {
                        // The flex container has an indefinite block-size. We allow the
                        // item's to apply fragmentation-imposed block-size growth.
                        return true;
                    }
                }
                return false;
            }

            MOZ_ASSERT!(aItem.IsBlockAxisCrossAxis());
            MOZ_ASSERT!(aItem.IsStretched(), "No need to override block-size with 'auto' if the item is not stretched in the cross axis!");

            let measuredBSize = aItem.MeasuredBSize();
            if let Some(measuredBSize) = measuredBSize {
                if aItem.CrossSize() == measuredBSize {
                    // The item has a measured content-based block-size due to having an
                    // indefinite cross-size. If its cross-size is equal to the content-based
                    // block-size, then it is the tallest item that established the cross-size
                    // of the flex line. We allow it apply fragmentation-imposed block-size
                    // growth.
                    //
                    // Note: We only allow the tallest item to grow because it is likely to
                    // have the most impact on the overall flex container block-size growth.
                    // This is not a perfect solution since other shorter items in the same
                    // line might also have fragmentation-imposed block-size growth, but
                    // currently there is no reliable way to detect whether they will outgrow
                    // the tallest item.
                    return true;
                }
            }
            false
        };

        let mut sizeOverrides = StyleSizeOverrides::default();
        let mut overrideBSizeWithAuto = false;

        // Override flex item's main size.
        if aItem.IsInlineAxisMainAxis() {
            sizeOverrides.mStyleISize = Some(aItem.StyleMainSize());
            FLEX_LOGV!("Main size (inline-size) override: %d", aItem.MainSize());
        } else {
            overrideBSizeWithAuto = ComputeBSizeOverrideWithAuto();
            if overrideBSizeWithAuto {
                sizeOverrides.mStyleBSize = Some(StyleSize::Auto);
                FLEX_LOGV!("Main size (block-size) override: Auto");
            } else {
                sizeOverrides.mStyleBSize = Some(aItem.StyleMainSize());
                FLEX_LOGV!("Main size (block-size) override: %d", aItem.MainSize());
            }
        }

        // Override flex item's cross size if it was stretched in the cross axis (in
        // which case we're imposing a cross size).
        if aItem.IsStretched() {
            if aItem.IsInlineAxisCrossAxis() {
                sizeOverrides.mStyleISize = Some(aItem.StyleCrossSize());
                FLEX_LOGV!("Cross size (inline-size) override: %d", aItem.CrossSize());
            } else {
                overrideBSizeWithAuto = ComputeBSizeOverrideWithAuto();
                if overrideBSizeWithAuto {
                    sizeOverrides.mStyleBSize = Some(StyleSize::Auto);
                    FLEX_LOGV!("Cross size (block-size) override: Auto");
                } else {
                    sizeOverrides.mStyleBSize = Some(aItem.StyleCrossSize());
                    FLEX_LOGV!("Cross size (block-size) override: %d", aItem.CrossSize());
                }
            }
        }
        if sizeOverrides.mStyleBSize.is_some() {
            // We are overriding the block-size. For robustness, we always assume that
            // this represents a block-axis resize for the frame. This may be
            // conservative, but we do capture all the conditions in the block-axis
            // (checked in NeedsFinalReflow()) that make this item require a final
            // reflow. This sets relevant flags in ReflowInput::InitResizeFlags().
            aItem.Frame().SetHasBSizeChange(true);
        }

        let presContext = self.PresContext();
        let mut childReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            &presContext,
            aReflowInput,
            aItem.Frame(),
            aAvailableSize,
            None,
            EnumSet::default(),
            sizeOverrides,
            FlexShrinkWrapFlags(),
            std::ptr::null_mut(),
        );
        if overrideBSizeWithAuto {
            // If we use 'auto' to override the item's block-size, set the item's
            // original block-size to min-size as a lower bound.
            childReflowInput.SetComputedMinBSize(aItem.BSize());

            // Set the item's block-size as the percentage basis so that its children
            // can resolve percentage sizes correctly.
            childReflowInput.SetPercentageBasisInBlockAxis(aItem.BSize());
        }

        if aItem.TreatBSizeAsIndefinite() && aItem.IsBlockAxisMainAxis() {
            childReflowInput.mFlags.mTreatBSizeAsIndefinite = true;
        }

        if aItem.IsStretched() && aItem.IsBlockAxisCrossAxis() {
            // This item is stretched (in the cross axis), and that axis is its block
            // axis.  That stretching effectively gives it a relative BSize.
            // XXXdholbert This flag only makes a difference if we use the flex items'
            // frame-state when deciding whether to reflow them -- and we don't, as of
            // the changes in bug 851607. So this has no effect right now, but it might
            // make a difference if we optimize to use dirty bits in the
            // future. (Reftests flexbox-resizeviewport-1.xhtml and -2.xhtml are
            // intended to catch any regressions here, if we end up relying on this bit
            // & neglecting to set it.)
            aItem.Frame().AddStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE);
        }

        if !aIsAdjacentWithBStart {
            // mIsTopOfPage bit in childReflowInput is carried over from aReflowInput.
            // However, if this item's position is not adjacent with the flex
            // container's content-box block-start edge, we should clear it.
            childReflowInput.mFlags.mIsTopOfPage = false;
        }

        // NOTE: Be very careful about doing anything else with childReflowInput
        // after this point, because some of its methods (e.g. SetComputedWidth)
        // internally call InitResizeFlags and stomp on mVResize & mHResize.

        FLEX_ITEM_LOG!(aItem.Frame(), "Reflowing item at its desired position %s", ToString(aFramePos).c_str());

        // CachedFlexItemData is stored in item's writing mode, so we pass
        // aChildReflowInput into ReflowOutput's constructor.
        let mut childReflowOutput = ReflowOutput::new__ReflowInput(&childReflowInput);
        let mut childStatus = nsReflowStatus::new();
        let outerWM = aReflowInput.GetWritingMode();
        self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
            aItem.Frame(),
            &presContext,
            &mut childReflowOutput,
            &childReflowInput,
            outerWM,
            aFramePos,
            aContainerSize,
            nsIFrame_ReflowChildFlags::Default,
            &mut childStatus,
            None,
        );

        // XXXdholbert Perhaps we should call CheckForInterrupt here; see bug 1495532.

        nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
            aItem.Frame(),
            &presContext,
            &childReflowOutput,
            Some(&childReflowInput),
            outerWM,
            aFramePos,
            aContainerSize,
            nsIFrame_ReflowChildFlags::ApplyRelativePositioning,
        );

        aItem.SetAscent(childReflowOutput.BlockStartAscent());

        // Update our cached flex item info:
        if let Some(cached) = CachedFlexItemData::Get(aItem.Frame()) {
            cached.Update__ReflowInput_ReflowOutput_FlexItemReflowType(&childReflowInput, &childReflowOutput, FlexItemReflowType::Final);
        } else {
            CachedFlexItemData::Set(aItem.Frame(), CachedFlexItemData::new(&childReflowInput, &childReflowOutput, FlexItemReflowType::Final));
        }

        childStatus
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::ReflowPlaceholders
    // (placeholders only exist for absolutely positioned boxes, which get no frame in this runtime, so the
    // array is always empty; the code is translated)
    pub fn ReflowPlaceholders(&self, aReflowInput: &ReflowInput, aPlaceholders: &mut Vec<nsIFrame>, aContentBoxOrigin: LogicalPoint, aContainerSize: nsSize) {
        let outerWM = aReflowInput.GetWritingMode();

        // As noted in this method's documentation, we'll reflow every entry in
        // |aPlaceholders| at the container's content-box origin.
        for &placeholder in aPlaceholders.iter() {
            MOZ_ASSERT!(placeholder.IsPlaceholderFrame(), "placeholders array should only contain placeholder frames");
            let wm = placeholder.GetWritingMode();
            let availSize = aReflowInput.ComputedSize__WritingMode(wm);
            let presContext = self.PresContext();
            // ADAPTER: the C++ line `ReflowInput(presContext, aReflowInput, placeholder, availSize)` uses the
            // constructor's default arguments, written out here.
            let childReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                &presContext,
                aReflowInput,
                placeholder,
                availSize,
                None,
                EnumSet::default(),
                StyleSizeOverrides::default(),
                ComputeSizeFlags::default(),
                std::ptr::null_mut(),
            );
            // No need to set the -webkit-line-clamp related flags when reflowing
            // a placeholder.
            let mut childReflowOutput = ReflowOutput::new__WritingMode(outerWM);
            let mut childStatus = nsReflowStatus::new();
            self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
                placeholder,
                &presContext,
                &mut childReflowOutput,
                &childReflowInput,
                outerWM,
                aContentBoxOrigin,
                aContainerSize,
                nsIFrame_ReflowChildFlags::Default,
                &mut childStatus,
                None,
            );

            nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
                placeholder,
                &presContext,
                &childReflowOutput,
                Some(&childReflowInput),
                outerWM,
                aContentBoxOrigin,
                aContainerSize,
                nsIFrame_ReflowChildFlags::Default,
            );

            // Mark the placeholder frame to indicate that it's not actually at the
            // element's static position, because we need to apply CSS Alignment after
            // we determine the OOF's size:
            placeholder.AddStateBits(PLACEHOLDER_STATICPOS_NEEDS_CSSALIGN);
        }
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::ComputeIntrinsicISize
    pub fn ComputeIntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        FLEX_LOG!("Compute %s isize for flex container frame %p", if aType == IntrinsicISizeType::MinISize { "min" } else { "pref" }, this);

        if let Some(containISize) = self.ContainIntrinsicISize() {
            return containISize;
        }

        let mut containerISize: nscoord = 0;
        let stylePos = self.StylePosition();
        let axisTracker = FlexboxAxisTracker::new(*self);

        let mainGapSize: nscoord;
        if axisTracker.IsRowOriented() {
            mainGapSize = nsLayoutUtils::ResolveGapToLength(&stylePos.mColumnGap(), NS_UNCONSTRAINEDSIZE);
        } else {
            mainGapSize = nsLayoutUtils::ResolveGapToLength(&stylePos.mRowGap(), NS_UNCONSTRAINEDSIZE);
        }

        let isSingleLine = IsSingleLine(self.0, &stylePos);
        let flexWM = self.GetWritingMode();

        // The loop below sets aside space for a gap before each item besides the
        // first. This bool helps us handle that special-case.
        let mut onFirstChild = true;

        for childFrame in &*self.mFrames() {
            // Skip out-of-flow children because they don't participate in flex layout.
            if childFrame.IsPlaceholderFrame() {
                continue;
            }

            if childFrame.StyleVisibility().IsCollapse() {
                // If we're collapsed, we don't take space in the main axis.
                continue;
            }

            let childWM = childFrame.GetWritingMode();
            let childInput = IntrinsicSizeInput::new__IntrinsicSizeInput_WritingMode_WritingMode(aInput, childWM, flexWM);
            let styleFrame = nsLayoutUtils::GetStyleFrame(childFrame);
            let childAnchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(styleFrame, std::ptr::null_mut());
            let childStylePos = styleFrame.StylePosition();

            // A flex item with a definite block size can transfer its block size to the
            // inline-axis via its own aspect-ratio or serve as a percentage basis for
            // its children with aspect-ratios. Both can influence the item's intrinsic
            // inline size contribution to the flex container's intrinsic inline size.
            //
            // This helper function determines whether we should "pre-stretch" a flex
            // item's cross size (with that size considered to be definite) based on the
            // flex container's definite cross size.
            //
            // Note: The logic here is similar to the "pre-stretch" in
            // GenerateFlexItemForChild().
            let childShouldStretchCrossSize = (|| -> bool {
                if !isSingleLine || axisTracker.IsColumnOriented() {
                    // We only perform "pre-stretch" for the item's cross size if the flex
                    // container is single-line and row-oriented.
                    return false;
                }
                match &aInput.mPercentageBasisForChildren {
                    None => return false,
                    Some(basis) if basis.BSize(flexWM) == NS_UNCONSTRAINEDSIZE => {
                        // The flex container does not have a definite cross size to stretch the
                        // items.
                        //
                        // Note: if the flex container has a definite cross size (for items to
                        // pre-stretch to fill), it should be passed down in
                        // mPercentageBasisForChildren -- specifically in the BSize component,
                        // given that we know the flex container is row-oriented at this point.
                        return false;
                    }
                    _ => {}
                }
                let (alignSelf, _flags) = self.UsedAlignSelfAndFlagsForItem(childFrame);
                if alignSelf != StyleAlignFlags::STRETCH || !childStylePos.BSize(flexWM, &childAnchorResolutionParams).IsAuto() || childFrame.StyleMargin().HasBlockAxisAuto(flexWM, &childAnchorResolutionParams) {
                    // Similar to FlexItem::ResolveStretchedCrossSize(), we only stretch
                    // the item if it satisfies all the following conditions:
                    // - used align-self value is 'stretch' (CSSAlignmentForFlexItem() has
                    //   converted 'normal' to 'stretch')
                    // - a cross-axis size property of value "auto"
                    // - no auto margins in the cross-axis
                    // https://drafts.csswg.org/css-flexbox-1/#valdef-align-items-stretch
                    return false;
                }
                // Let's stretch the item's cross size.
                true
            })();

            let mut sizeOverrides = StyleSizeOverrides::default();
            if childShouldStretchCrossSize {
                let offsetData = childFrame.IntrinsicBSizeOffsets(NS_UNCONSTRAINEDSIZE);
                let boxSizingToMarginEdgeSize = if childStylePos.mBoxSizing() == StyleBoxSizing::ContentBox { offsetData.MarginBorderPadding() } else { offsetData.margin };
                let stretchedCrossSize = std::cmp::max(0, aInput.mPercentageBasisForChildren.unwrap().BSize(flexWM) - boxSizingToMarginEdgeSize);
                let stretchedStyleCrossSize = StyleSize::FromAppUnits(stretchedCrossSize);
                // The size override is in the child's own writing mode.
                if flexWM.IsOrthogonalTo(&childWM) {
                    sizeOverrides.mStyleISize = Some(stretchedStyleCrossSize);
                } else {
                    sizeOverrides.mStyleBSize = Some(stretchedStyleCrossSize);
                }
            }
            let childISize = nsLayoutUtils::IntrinsicForContainer(childInput.mContext, childFrame, aType, childInput.mPercentageBasisForChildren, 0, &sizeOverrides);

            // * For a row-oriented single-line flex container, the intrinsic
            // {min/pref}-isize is the sum of its items' {min/pref}-isizes and
            // (n-1) column gaps.
            // * For a column-oriented flex container, the intrinsic min isize
            // is the max of its items' min isizes.
            // * For a row-oriented multi-line flex container, the intrinsic
            // pref isize is former (sum), and its min isize is the latter (max).
            if axisTracker.IsRowOriented() && (isSingleLine || aType == IntrinsicISizeType::PrefISize) {
                containerISize += childISize;
                if !onFirstChild {
                    containerISize += mainGapSize;
                }
                onFirstChild = false;
            } else {
                // (col-oriented, or MinISize for multi-line row flex container)
                containerISize = std::cmp::max(containerISize, childISize);
            }
        }

        containerISize
    }

    // Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        let this = *self;
        self.mCachedIntrinsicSizes().GetOrSet(self.0, aType, aInput, || this.ComputeIntrinsicISize(aInput, aType))
    }
}

// Gecko: layout/generic/nsFlexContainerFrame.cpp nsFlexContainerFrame::GetNumLines, IsLineIteratorFlowRTL,
// GetLine, FindLineContaining, CheckLineOrder, FindFrameAt
// NOT PORTED: the nsILineIterator implementation (nsFlexContainerFrame.cpp lines 6631-6732 at GECKO_REV): it
// serves caret movement and selection, which this runtime does not have. Nothing calls it (CanProvideLineIterator
// and GetLineIterator are not translated either).

// Tests of flex layout. Gecko's tests for this code are reftests (layout/reftests/flexbox,
// web-platform-tests css/css-flexbox), which cannot run here; these are written from the CSS Flexbox
// Level 1 algorithm (sections 9.2 to 9.7) and CSS Box Alignment on real Stylo styles, on HTML snippets whose
// numbers are chosen so the expected values can be worked out by hand. Positions are relative to the
// flex container's border box (the items' parent); the container is `#c` where it matters, 1000px wide
// otherwise (the viewport), with no margin on body.
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

    // All the frames of `aType` below `aFrame`, in tree order
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

    // The children of a frame (its principal list), in tree order
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

    // Lays out `aBody`, takes the first flex container and returns it with its items, in tree order.
    fn container(aBody: &str) -> (PageLayout, nsIFrame, Vec<nsIFrame>) {
        let result = layout(aBody);
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer)[0];
        let items = children(c);
        (result, c, items)
    }

    // The rects of all the items
    fn rects(aItems: &[nsIFrame]) -> Vec<(f64, f64, f64, f64)> {
        aItems.iter().map(|i| rect(*i)).collect()
    }

    // `n` empty boxes of the given style
    fn boxes(n: usize, aStyle: &str) -> String {
        format!("<div style='{aStyle}'></div>").repeat(n)
    }

    #[test]
    fn a_row_container_lays_its_items_out_in_a_row_at_their_widths() {
        // 9.2 and 9.7: items with flex-grow 0 keep their flex base size (their width), and
        // align-items:stretch (the initial value) stretches an auto-height item to the line.
        let (result, c, items) = container("<div id=c style='display:flex'><div style='width:50px;height:20px'></div><div style='width:70px'></div></div>");
        assert_eq!(c.Type(), LayoutFrameType::FlexContainer);
        // the container is a block-level box: as wide as the viewport; as high as its one line
        assert_eq!(rect(c).2, 1000.0);
        assert_eq!(rect(c).3, 20.0);
        assert_eq!(rects(&items), vec![(0.0, 0.0, 50.0, 20.0), (50.0, 0.0, 70.0, 20.0)]);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(20.0));
    }

    #[test]
    fn flex_grow_shares_the_free_space_in_proportion_to_the_factors() {
        // 9.7: 300px of free space (flex-basis 0, so all of the container) goes 1:2.
        let (_, _, items) = container("<div style='display:flex;width:300px'><div style='flex:1 1 0;height:20px'></div><div style='flex:2 1 0'></div></div>");
        assert_eq!(rects(&items), vec![(0.0, 0.0, 100.0, 20.0), (100.0, 0.0, 200.0, 20.0)]);
    }

    #[test]
    fn flex_grow_distributes_only_the_free_space_on_top_of_the_base_sizes() {
        // 100 + 40 + 40 = 180 of 300px: 120 free, shared 1:3 as 30 and 90 (grow factors 1 and 3).
        let (_, _, items) = container("<div style='display:flex;width:300px;height:10px'><div style='width:100px;flex-grow:0'></div><div style='width:40px;flex-grow:1'></div><div style='width:40px;flex-grow:3'></div></div>");
        // free space 120 goes only to the two growing items, 1:3
        let r = rects(&items);
        assert_eq!(r[0].2, 100.0);
        assert_eq!(r[1].2, 40.0 + 30.0);
        assert_eq!(r[2].2, 40.0 + 90.0);
        assert_eq!(r[2].0, 100.0 + 70.0);
    }

    #[test]
    fn flex_shrink_takes_the_overflow_in_proportion_to_factor_times_base_size() {
        // 9.7: two 60px items in 100px, flex-shrink 1 and 3: the 20px of overflow is shared as
        // 1*60 : 3*60, so 5px and 15px.
        let (_, _, items) = container("<div style='display:flex;width:100px'><div style='width:60px;height:10px;flex-shrink:1;min-width:0'></div><div style='width:60px;height:10px;flex-shrink:3;min-width:0'></div></div>");
        assert_eq!(rects(&items), vec![(0.0, 0.0, 55.0, 10.0), (55.0, 0.0, 45.0, 10.0)]);
    }

    #[test]
    fn equal_items_shrink_equally_by_default() {
        // flex-shrink is 1 for all items by default
        let (_, _, items) = container("<div style='display:flex;width:100px'><div style='width:60px;height:10px;min-width:0'></div><div style='width:60px;height:10px;min-width:0'></div></div>");
        assert_eq!(rects(&items), vec![(0.0, 0.0, 50.0, 10.0), (50.0, 0.0, 50.0, 10.0)]);
    }

    #[test]
    fn flex_basis_replaces_the_width_unless_it_is_auto() {
        // 9.2 step 3: a flex-basis other than auto is the flex base size, width is ignored;
        // with flex-basis:auto the width is used.
        let (_, _, items) = container("<div style='display:flex;width:300px'><div style='width:20px;height:10px;flex:0 0 80px'></div><div style='width:30px;height:10px;flex:0 0 auto'></div><div style='width:20px;height:10px;flex:0 0 10%'></div></div>");
        let r = rects(&items);
        assert_eq!(r[0].2, 80.0);
        assert_eq!(r[1].2, 30.0);
        // a percentage flex-basis resolves against the container's width: 10% of 300
        assert_eq!(r[2].2, 30.0);
        assert_eq!(r[1].0, 80.0);
        assert_eq!(r[2].0, 110.0);
    }

    #[test]
    fn the_flex_shorthand_with_one_number_is_grow_with_a_zero_basis() {
        // `flex: 1` is `1 1 0%`
        let (_, _, items) = container("<div style='display:flex;width:90px'><div style='flex:1;height:10px'></div><div style='flex:1;height:10px'></div><div style='flex:1;height:10px'></div></div>");
        assert_eq!(rects(&items).iter().map(|r| r.2).collect::<Vec<_>>(), vec![30.0, 30.0, 30.0]);
    }

    #[test]
    fn a_max_width_clamps_a_growing_item_and_the_rest_goes_to_the_others() {
        // 9.7 step 4 (loop): the first item wants 100, is clamped to 50 (max violation), is frozen,
        // and the remaining 150 goes to the second.
        let (_, _, items) = container("<div style='display:flex;width:200px'><div style='flex:1 1 0;max-width:50px;height:10px'></div><div style='flex:1 1 0;height:10px'></div></div>");
        assert_eq!(rects(&items), vec![(0.0, 0.0, 50.0, 10.0), (50.0, 0.0, 150.0, 10.0)]);
    }

    #[test]
    fn a_min_width_stops_a_shrinking_item() {
        // 9.7 step 4: shrinking would give 50 and 50; the first has min-width 70 (min violation),
        // so it is frozen at 70 and the second takes the rest: 30.
        let (_, _, items) = container("<div style='display:flex;width:100px'><div style='width:60px;min-width:70px;height:10px'></div><div style='width:60px;min-width:0;height:10px'></div></div>");
        assert_eq!(rects(&items), vec![(0.0, 0.0, 70.0, 10.0), (70.0, 0.0, 30.0, 10.0)]);
    }

    #[test]
    fn wrap_breaks_items_into_lines() {
        // 9.3: three 40px items in 100px: two fit on the first line, one on the second.
        let (_, c, items) = container(&format!("<div style='display:flex;flex-wrap:wrap;width:100px'>{}</div>", boxes(3, "width:40px;height:20px")));
        assert_eq!(rects(&items), vec![(0.0, 0.0, 40.0, 20.0), (40.0, 0.0, 40.0, 20.0), (0.0, 20.0, 40.0, 20.0)]);
        assert_eq!(rect(c).3, 40.0);
    }

    #[test]
    fn the_line_cross_size_is_the_tallest_item_of_the_line() {
        // 9.4 step 8: each line is as high as its tallest item (margin box)
        let (_, c, items) = container("<div style='display:flex;flex-wrap:wrap;width:100px;align-items:flex-start'><div style='width:60px;height:10px'></div><div style='width:60px;height:30px'></div><div style='width:60px;height:5px;margin-top:5px'></div></div>");
        assert_eq!(rects(&items), vec![(0.0, 0.0, 60.0, 10.0), (0.0, 10.0, 60.0, 30.0), (0.0, 45.0, 60.0, 5.0)]);
        assert_eq!(rect(c).3, 10.0 + 30.0 + 10.0);
    }

    #[test]
    fn wrap_reverse_stacks_the_lines_from_the_cross_end() {
        // flex-wrap:wrap-reverse: the first line is at the cross-end
        let (_, _, items) = container(&format!("<div style='display:flex;flex-wrap:wrap-reverse;width:100px;height:60px;align-content:flex-start'>{}</div>", boxes(3, "width:40px;height:20px")));
        // the container is 60 high; the lines pack at the cross-start edge, which is the bottom
        assert_eq!(rects(&items), vec![(0.0, 40.0, 40.0, 20.0), (40.0, 40.0, 40.0, 20.0), (0.0, 20.0, 40.0, 20.0)]);
    }

    #[test]
    fn a_column_container_stacks_its_items_and_takes_their_height() {
        // 9.2: flex-direction:column, auto height: the container's height is the sum of the items'
        // base sizes; items are stretched to the container's width (1000).
        let (result, c, items) = container("<div style='display:flex;flex-direction:column'><div style='height:10px'></div><div style='height:20px'></div><div style='height:30px'></div></div>");
        assert_eq!(rect(c).3, 60.0);
        assert_eq!(rects(&items), vec![(0.0, 0.0, 1000.0, 10.0), (0.0, 10.0, 1000.0, 20.0), (0.0, 30.0, 1000.0, 30.0)]);
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(60.0));
    }

    #[test]
    fn column_items_grow_into_a_definite_height() {
        // a 100px tall column with flex 1 and flex 3 and auto heights: 25 and 75
        let (_, _, items) = container("<div style='display:flex;flex-direction:column;height:100px'><div style='flex:1'></div><div style='flex:3'></div></div>");
        assert_eq!(rects(&items), vec![(0.0, 0.0, 1000.0, 25.0), (0.0, 25.0, 1000.0, 75.0)]);
    }

    #[test]
    fn column_items_with_content_height_get_it_from_a_measuring_reflow() {
        // flex-basis:auto with height:auto in a column container is content-based: the item is as
        // high as its line (9.2 step 3E).
        let (_, c, items) = container("<div style='display:flex;flex-direction:column'><div style='line-height:20px'>one</div><div style='line-height:30px'>two</div></div>");
        assert_eq!(rects(&items).iter().map(|r| (r.1, r.3)).collect::<Vec<_>>(), vec![(0.0, 20.0), (20.0, 30.0)]);
        assert_eq!(rect(c).3, 50.0);
    }

    #[test]
    fn row_reverse_starts_at_the_right_edge() {
        // flex-direction:row-reverse: the main-start edge is the right edge
        let (_, _, items) = container("<div style='display:flex;flex-direction:row-reverse;width:100px'><div style='width:20px;height:10px'></div><div style='width:30px;height:10px'></div></div>");
        assert_eq!(rects(&items), vec![(80.0, 0.0, 20.0, 10.0), (50.0, 0.0, 30.0, 10.0)]);
    }

    #[test]
    fn column_reverse_starts_at_the_bottom() {
        let (_, _, items) = container("<div style='display:flex;flex-direction:column-reverse;height:100px;width:50px'><div style='height:20px'></div><div style='height:30px'></div></div>");
        assert_eq!(rects(&items), vec![(0.0, 80.0, 50.0, 20.0), (0.0, 50.0, 50.0, 30.0)]);
    }

    #[test]
    fn justify_content_distributes_the_free_space_on_the_main_axis() {
        // 9.5 and css-align-3: two 20px items in 100px, 60px of free space
        let run = |aValue: &str| -> Vec<f64> {
            let (_, _, items) = container(&format!("<div style='display:flex;width:100px;justify-content:{aValue}'>{}</div>", boxes(2, "width:20px;height:10px")));
            rects(&items).iter().map(|r| r.0).collect()
        };
        assert_eq!(run("flex-start"), vec![0.0, 20.0]);
        assert_eq!(run("flex-end"), vec![60.0, 80.0]);
        assert_eq!(run("center"), vec![30.0, 50.0]);
        assert_eq!(run("space-between"), vec![0.0, 80.0]);
        // space-around: 15 on each side of each item
        assert_eq!(run("space-around"), vec![15.0, 65.0]);
        // space-evenly: three gaps of 20
        assert_eq!(run("space-evenly"), vec![20.0, 60.0]);
    }

    #[test]
    fn justify_content_has_no_effect_when_there_is_no_free_space() {
        // a negative free space: flex-end/center still start at... 9.5: overflow is distributed to the
        // end for flex-start; center and flex-end overflow equally/at the start side
        let (_, _, items) = container("<div style='display:flex;width:100px;justify-content:flex-start'><div style='width:70px;height:10px;flex-shrink:0'></div><div style='width:70px;height:10px;flex-shrink:0'></div></div>");
        assert_eq!(rects(&items).iter().map(|r| r.0).collect::<Vec<_>>(), vec![0.0, 70.0]);
        // space-between with negative free space behaves as flex-start
        let (_, _, items) = container("<div style='display:flex;width:100px;justify-content:space-between'><div style='width:70px;height:10px;flex-shrink:0'></div><div style='width:70px;height:10px;flex-shrink:0'></div></div>");
        assert_eq!(rects(&items).iter().map(|r| r.0).collect::<Vec<_>>(), vec![0.0, 70.0]);
    }

    #[test]
    fn align_items_places_items_in_the_cross_axis() {
        // 9.6: a 50px high container, a 10px high item
        let run = |aValue: &str| -> (f64, f64) {
            let (_, _, items) = container(&format!("<div style='display:flex;height:50px;align-items:{aValue}'><div style='width:20px;{}'></div></div>", if aValue == "stretch" { "" } else { "height:10px" }));
            let r = rect(items[0]);
            (r.1, r.3)
        };
        assert_eq!(run("flex-start"), (0.0, 10.0));
        assert_eq!(run("flex-end"), (40.0, 10.0));
        assert_eq!(run("center"), (20.0, 10.0));
        // stretch: an auto height item takes the line's cross size, which is the container's 50px
        assert_eq!(run("stretch"), (0.0, 50.0));
    }

    #[test]
    fn align_self_overrides_align_items() {
        let (_, _, items) = container("<div style='display:flex;height:50px;align-items:flex-start'><div style='width:20px;height:10px'></div><div style='width:20px;height:10px;align-self:flex-end'></div><div style='width:20px;height:10px;align-self:center'></div></div>");
        assert_eq!(rects(&items).iter().map(|r| r.1).collect::<Vec<_>>(), vec![0.0, 40.0, 20.0]);
    }

    #[test]
    fn a_stretched_item_with_a_max_height_is_not_taller_than_it() {
        let (_, _, items) = container("<div style='display:flex;height:50px'><div style='width:20px;max-height:30px'></div></div>");
        assert_eq!(rect(items[0]).3, 30.0);
    }

    #[test]
    fn align_content_distributes_the_lines_in_a_multi_line_container() {
        // 9.4 step 9: two 40px items wrapping to two 20px lines in a 100px high container, 60px free.
        let run = |aValue: &str| -> Vec<f64> {
            let (_, _, items) = container(&format!("<div style='display:flex;flex-wrap:wrap;width:50px;height:100px;align-items:flex-start;align-content:{aValue}'>{}</div>", boxes(2, "width:40px;height:20px")));
            rects(&items).iter().map(|r| r.1).collect()
        };
        assert_eq!(run("flex-start"), vec![0.0, 20.0]);
        assert_eq!(run("flex-end"), vec![60.0, 80.0]);
        assert_eq!(run("center"), vec![30.0, 50.0]);
        assert_eq!(run("space-between"), vec![0.0, 80.0]);
        // stretch (the initial value `normal` behaves as stretch): each line gets 30px more
        assert_eq!(run("stretch"), vec![0.0, 50.0]);
        assert_eq!(run("normal"), vec![0.0, 50.0]);
    }

    #[test]
    fn gaps_separate_items_and_lines() {
        // css-align-3 gaps in flexbox: column-gap between items in a line, row-gap between lines.
        let (_, c, items) = container(&format!("<div style='display:flex;gap:10px;width:200px'>{}</div>", boxes(3, "width:30px;height:10px")));
        assert_eq!(rects(&items).iter().map(|r| r.0).collect::<Vec<_>>(), vec![0.0, 40.0, 80.0]);
        assert_eq!(rect(c).3, 10.0);
        let (_, c, items) = container(&format!("<div style='display:flex;flex-wrap:wrap;column-gap:4px;row-gap:5px;width:70px;align-items:flex-start'>{}</div>", boxes(3, "width:30px;height:20px")));
        assert_eq!(rects(&items).iter().map(|r| (r.0, r.1)).collect::<Vec<_>>(), vec![(0.0, 0.0), (34.0, 0.0), (0.0, 25.0)]);
        assert_eq!(rect(c).3, 45.0);
    }

    #[test]
    fn gaps_take_part_in_shrinking() {
        // two 60px items and a 20px gap in 100px: 140 - 100 = 40 of overflow, 20 each
        let (_, _, items) = container("<div style='display:flex;column-gap:20px;width:100px'><div style='width:60px;height:10px;min-width:0'></div><div style='width:60px;height:10px;min-width:0'></div></div>");
        assert_eq!(rects(&items).iter().map(|r| (r.0, r.2)).collect::<Vec<_>>(), vec![(0.0, 40.0), (60.0, 40.0)]);
    }

    #[test]
    fn order_changes_the_order_of_the_items_in_the_line() {
        // 5.4: items are laid out in order-modified document order
        let (_, _, items) = container("<div style='display:flex;width:100px'><div style='width:10px;height:5px;order:2'></div><div style='width:20px;height:5px;order:1'></div><div style='width:30px;height:5px'></div></div>");
        // tree order is kept in the frame tree; positions follow the order property:
        // third (order 0) first at 0, second (order 1) at 30, first (order 2) at 50
        assert_eq!(rects(&items).iter().map(|r| r.0).collect::<Vec<_>>(), vec![50.0, 30.0, 0.0]);
    }

    #[test]
    fn auto_margins_absorb_the_free_space() {
        // 8.1: auto margins in the main axis take the free space before justify-content applies
        let (_, _, items) = container("<div style='display:flex;width:100px;justify-content:center'><div style='width:20px;height:10px;margin-left:auto'></div></div>");
        assert_eq!(rect(items[0]).0, 80.0);
        let (_, _, items) = container("<div style='display:flex;width:100px'><div style='width:20px;height:10px;margin:0 auto'></div></div>");
        assert_eq!(rect(items[0]).0, 40.0);
        // two items, the auto margin of the first pushes the second to the end
        let (_, _, items) = container("<div style='display:flex;width:100px'><div style='width:20px;height:10px;margin-right:auto'></div><div style='width:20px;height:10px'></div></div>");
        assert_eq!(rect(items[0]).0, 0.0);
        let (_, _, items) = container("<div style='display:flex;width:100px'><div style='width:20px;height:10px'></div><div style='width:20px;height:10px;margin-left:auto'></div></div>");
        assert_eq!(rect(items[1]).0, 80.0);
    }

    #[test]
    fn auto_margins_in_both_axes_center_the_item() {
        // 8.1: `margin:auto` centers the item in both axes, even when align-items is flex-start
        let (_, _, items) = container("<div style='display:flex;width:100px;height:50px;align-items:flex-start'><div style='width:20px;height:10px;margin:auto'></div></div>");
        assert_eq!(rect(items[0]), (40.0, 20.0, 20.0, 10.0));
    }

    #[test]
    fn a_cross_axis_auto_margin_prevents_stretching() {
        let (_, _, items) = container("<div style='display:flex;height:50px'><div style='width:20px;margin-top:auto'></div></div>");
        // the auto margin makes align-self:stretch not apply; the item is 0 high at the bottom
        assert_eq!(rect(items[0]).1, 50.0);
        assert_eq!(rect(items[0]).3, 0.0);
    }

    #[test]
    fn margins_padding_and_borders_are_part_of_the_item_size() {
        // flex-basis and width are content-box sizes (box-sizing:content-box); the outer size adds
        // padding, border and margin
        let (_, _, items) = container("<div style='display:flex;width:200px'><div style='width:50px;height:10px;padding:0 5px;border:2px solid;margin:0 3px'></div><div style='width:10px;height:10px'></div></div>");
        let r = rects(&items);
        // x = margin-left, the box is 50 + 10 + 4 = 64 wide
        assert_eq!(r[0], (3.0, 0.0, 64.0, 14.0));
        assert_eq!(r[1].0, 3.0 + 64.0 + 3.0);
        // box-sizing:border-box: the width includes padding and border
        let (_, _, items) = container("<div style='display:flex;width:200px'><div style='box-sizing:border-box;width:50px;height:10px;padding:0 5px;border:2px solid'></div></div>");
        assert_eq!(rect(items[0]).2, 50.0);
    }

    #[test]
    fn percentage_sizes_resolve_against_the_container() {
        // percentage width against the container's width; percentage height against a definite height
        let (_, _, items) = container("<div style='display:flex;width:200px;height:100px;align-items:flex-start'><div style='width:50%;height:50%'></div></div>");
        assert_eq!(rect(items[0]), (0.0, 0.0, 100.0, 50.0));
        // percentage padding resolves against the container's inline size too
        let (_, _, items) = container("<div style='display:flex;width:200px'><div style='height:10px;padding-left:10%;width:0'></div></div>");
        assert_eq!(rect(items[0]).2, 20.0);
    }

    #[test]
    fn an_item_with_text_is_as_wide_as_its_text_and_as_high_as_its_line() {
        // 9.2 step 3: flex-basis:auto, width:auto: the item's max-content size
        let (_, _, items) = container("<div style='display:flex'><div style='line-height:20px'>some text</div></div>");
        let r = rect(items[0]);
        assert_eq!(r.3, 20.0);
        // the same text in an inline-block measures the same width
        let result = layout("<span id=s style='display:inline-block;line-height:20px'>some text</span>");
        let ib = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::Block).into_iter().find(|f| f.StyleDisplay().IsInlineFlow() == false && rect(*f).2 > 0.0 && rect(*f).2 < 500.0 && f.GetContent().map_or(false, |c| c.IsHTMLElement("span"))).unwrap();
        assert_eq!(r.2, rect(ib).2);
    }

    #[test]
    fn text_directly_in_a_flex_container_is_an_anonymous_item() {
        // 4: each contiguous run of text is wrapped in an anonymous block flex item
        let (_, c, items) = container("<div style='display:flex;width:300px;line-height:20px'>abc<div style='width:10px'></div>def</div>");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].Type(), LayoutFrameType::Block);
        assert_eq!(rect(items[1]).2, 10.0);
        assert_eq!(rect(c).3, 20.0);
        // the anonymous items are laid out in a row
        assert!(rect(items[1]).0 > 0.0 && rect(items[2]).0 > rect(items[1]).0);
    }

    #[test]
    fn min_width_auto_keeps_an_item_from_shrinking_below_its_longest_word() {
        // 4.5: the automatic minimum size of an item is its min-content size
        let run = |aMin: &str| -> f64 {
            let (_, _, items) = container(&format!("<div style='display:flex;width:20px'><div style='{aMin}'>unbreakablewordhere</div></div>"));
            rect(items[0]).2
        };
        let word = run("");
        assert!(word > 20.0, "an item holds its word: {word}");
        // min-width:0 lets it shrink to the container
        assert_eq!(run("min-width:0"), 20.0);
        // a specified width smaller than the content is the specified size suggestion (min() of the two)
        assert_eq!(run("width:5px"), 5.0);
        // and with a larger one, the content size suggestion wins
        assert_eq!(run("width:500px;flex-shrink:1"), word);
    }

    #[test]
    fn a_text_item_wraps_when_it_is_shrunk() {
        // min-width:auto is the longest word, so an item of several words shrinks and wraps
        let (_, _, items) = container("<div style='display:flex;width:60px'><div style='line-height:20px'>aa bb cc dd ee ff gg hh ii jj</div></div>");
        let r = rect(items[0]);
        assert_eq!(r.2, 60.0);
        assert!(r.3 > 20.0, "the text wrapped: {}", r.3);
    }

    #[test]
    fn nested_flex_containers() {
        // an item that is itself a flex container: a column that stretches an inner row
        let (_, _, items) = container("<div style='display:flex;flex-direction:column;height:100px;width:200px'><div style='display:flex;flex:1'><div style='width:30px'></div><div style='flex:1'></div></div></div>");
        let inner = items[0];
        assert_eq!(inner.Type(), LayoutFrameType::FlexContainer);
        assert_eq!(rect(inner), (0.0, 0.0, 200.0, 100.0));
        let kids = children(inner);
        // the inner items stretch to the inner container's 100px height
        assert_eq!(rects(&kids), vec![(0.0, 0.0, 30.0, 100.0), (30.0, 0.0, 170.0, 100.0)]);
    }

    #[test]
    fn a_flex_container_in_a_flex_item_is_sized_by_its_content() {
        let (_, _, items) = container("<div style='display:flex'><div style='display:flex'><div style='width:30px;height:10px'></div><div style='width:20px;height:10px'></div></div></div>");
        // the item's max-content width is the sum of its items' widths (the intrinsic size of a flex container)
        assert_eq!(rect(items[0]).2, 50.0);
        assert_eq!(rect(items[0]).3, 10.0);
    }

    #[test]
    fn an_inline_flex_container_shrink_wraps() {
        // inline-flex: an inline-level box whose width is its max-content width (sum of the items)
        let result = layout("<div><span style='display:inline-flex'><div style='width:30px;height:10px'></div><div style='width:20px;height:10px'></div></span></div>");
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer)[0];
        assert_eq!(rect(c).2, 50.0);
        assert_eq!(rect(c).3, 10.0);
    }

    #[test]
    fn an_inline_flex_container_with_growing_items_is_as_wide_as_the_item_widths_and_margins() {
        // the shrink-to-fit width of the container is the sum of the items' intrinsic contributions
        // (their widths plus margins); flex-grow does not enter
        let result = layout("<span style='display:inline-flex'><div style='flex:1 0 auto;width:40px;height:10px;margin:0 5px'></div><div style='flex:2 0 auto;width:20px;height:10px'></div></span>");
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer)[0];
        assert_eq!(rect(c).2, 40.0 + 10.0 + 20.0);
    }

    #[test]
    fn the_intrinsic_width_of_a_flex_container_ignores_flex_basis() {
        // nsFlexContainerFrame::ComputeIntrinsicISize sums the items' own intrinsic widths; an empty
        // item with `flex-basis:40px` contributes 0 (plus its margins), as in Gecko
        let result = layout("<span style='display:inline-flex'><div style='flex:1 0 40px;height:10px;margin:0 5px'></div></span>");
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer)[0];
        assert_eq!(rect(c).2, 10.0);
    }

    #[test]
    fn an_aspect_ratio_item_gets_its_main_size_from_the_cross_size() {
        // 9.2 step 3B: aspect-ratio 2/1 with a definite height of 10px gives a flex base size of 20px
        let (_, _, items) = container("<div style='display:flex;align-items:flex-start'><div style='aspect-ratio:2/1;height:10px'></div></div>");
        assert_eq!(rect(items[0]), (0.0, 0.0, 20.0, 10.0));
    }

    #[test]
    fn baseline_alignment_lines_up_the_text_baselines() {
        // 9.6: align-items:baseline: the item with the larger ascent is at the cross-start edge,
        // the other is moved down by the difference
        let (_, _, items) = container("<div style='display:flex;align-items:baseline'><div style='font-size:10px;line-height:10px'>a</div><div style='font-size:40px;line-height:40px'>b</div></div>");
        let r = rects(&items);
        assert_eq!(r[1].1, 0.0);
        assert!(r[0].1 > 0.0, "the small item is below the top: {:?}", r);
        // both baselines are at the same y: top + ascent differ by exactly the offset
        let small = items[0].GetLogicalBaseline__WritingMode(items[0].GetWritingMode());
        let large = items[1].GetLogicalBaseline__WritingMode(items[1].GetWritingMode());
        assert_eq!(r[0].1 + small as f64 / 60.0, r[1].1 + large as f64 / 60.0);
    }

    #[test]
    fn visibility_collapse_removes_an_item_but_keeps_the_line_height() {
        // 4.4: a collapsed item is a strut: no main size, but it keeps the line's cross size
        let (_, c, items) = container("<div style='display:flex;width:100px'><div style='width:30px;height:30px;visibility:collapse'></div><div style='width:20px;height:10px'></div></div>");
        assert_eq!(rect(items[1]).0, 0.0);
        assert_eq!(rect(c).3, 30.0);
    }

    #[test]
    fn display_none_items_make_no_item_and_no_gap() {
        let (_, _, items) = container("<div style='display:flex;gap:10px;width:100px'><div style='width:20px;height:10px'></div><div style='display:none'></div><div style='width:20px;height:10px'></div></div>");
        assert_eq!(items.len(), 2);
        assert_eq!(rect(items[1]).0, 30.0);
    }

    #[test]
    fn a_flex_container_inside_a_block_takes_part_in_the_block_flow() {
        // the container is a block-level box: following blocks start below it; margins do not collapse through
        let result = layout("<div style='display:flex;height:30px;margin-bottom:10px'></div><p style='margin:0;line-height:20px'>x</p>");
        assert_eq!(result.RootElementHeightInCSSPixels(), Some(30.0 + 10.0 + 20.0));
    }

    #[test]
    fn the_margins_of_flex_items_do_not_collapse() {
        // 3: the margins of flex items do not collapse with each other or with the container's
        let (_, c, items) = container("<div style='display:flex;flex-direction:column'><div style='height:10px;margin:10px 0'></div><div style='height:10px;margin:10px 0'></div></div>");
        assert_eq!(rects(&items).iter().map(|r| r.1).collect::<Vec<_>>(), vec![10.0, 40.0]);
        assert_eq!(rect(c).3, 10.0 + 10.0 + 10.0 + 10.0 + 10.0 + 10.0);
    }

    #[test]
    fn an_empty_flex_container_has_no_height() {
        let (_, c, items) = container("<div style='display:flex'></div>");
        assert!(items.is_empty());
        assert_eq!(rect(c).3, 0.0);
    }

    #[test]
    fn the_container_border_and_padding_offset_the_items() {
        let (_, c, items) = container("<div style='display:flex;border:2px solid;padding:3px;width:100px'><div style='width:20px;height:10px'></div></div>");
        assert_eq!(rect(items[0]).0, 5.0);
        assert_eq!(rect(items[0]).1, 5.0);
        // content-box width:100 plus border and padding
        assert_eq!(rect(c).2, 110.0);
        assert_eq!(rect(c).3, 10.0 + 10.0);
    }

    #[test]
    fn a_container_height_and_min_height_apply() {
        let (_, c, _) = container("<div style='display:flex;min-height:40px'><div style='height:10px'></div></div>");
        assert_eq!(rect(c).3, 40.0);
        let (_, c, items) = container("<div style='display:flex;max-height:15px'><div style='height:30px'></div></div>");
        assert_eq!(rect(c).3, 15.0);
        // the item keeps its own size and overflows
        assert_eq!(rect(items[0]).3, 30.0);
    }

    #[test]
    fn the_baseline_of_a_flex_container_is_that_of_its_first_item() {
        // 8.5: an inline-flex container's baseline is the first item's baseline; next to text in a
        // line the container's text baseline is on the line's baseline
        let result = layout("<div style='line-height:20px'>a<span style='display:inline-flex;line-height:20px'><div>b</div></span>c</div>");
        let c = find_all(result.mRootElementFrame.unwrap(), LayoutFrameType::FlexContainer)[0];
        let item = children(c)[0];
        let wm = c.GetWritingMode();
        let cb = c.GetLogicalBaseline__WritingMode(wm);
        let ib = item.GetLogicalBaseline__WritingMode(wm);
        assert_eq!(cb, ib + item.GetLogicalPosition__WritingMode_nsSize(wm, c.GetSize()).B(wm));
    }
}
