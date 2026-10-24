// ADAPTER (not a translation of a whole file): the parts of layout/style/nsStyleStruct.h that
// translated layout code reads, on top of Stylo's computed `Box` struct (standalone `stylo`, servo
// configuration). In Gecko `nsStyleDisplay` is a C++ struct whose members are read as fields
// (`display->mFloat`); here the same names are methods (`display.mFloat()`).
use crate::*;
use mrt_style::style::properties::ComputedValues;
use mrt_style::style::servo_arc::Arc as ServoArc;

// Gecko: layout/style/nsStyleStruct.h UsedFloat
// Used value for the CSS 'float' property (logical 'inline-*' in the computed
// value will have been resolved to 'left' or 'right').
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum UsedFloat {
    None,
    Left,
    Right,
}

// Gecko: layout/style/nsStyleStruct.h UsedClear
// Used value for the CSS 'clear' property (logical 'inline-*' in the computed
// value will have been resolved to 'left' or 'right').
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum UsedClear {
    None,
    Left,
    Right,
    Both,
}

// Gecko's StyleFloat and StyleClear are generated from the Rust enums of the style crate; here they
// are those enums (same variant names).
pub type StyleFloat = mrt_style::style::values::specified::box_::Float;
pub type StyleClear = mrt_style::style::values::specified::box_::Clear;

// ADAPTER: Gecko's StyleShapeOutside (a Rust enum shared with C++). `shape-outside` is not part of
// the servo configuration of Stylo, so only `None` ever occurs; the other tags exist so that the
// translated `switch` keeps its shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleShapeOutside {
    None,
    Image,
    Box,
    Shape,
}

// Gecko: layout/style/nsStyleStruct.h nsStyleDisplay
// ADAPTER: wraps the computed style that holds Stylo's `Box` struct (the Arc keeps it alive;
// Gecko hands out a plain pointer).
#[derive(Clone)]
pub struct nsStyleDisplay {
    mStyle: ServoArc<ComputedValues>,
}

impl nsStyleDisplay {
    pub fn new(aStyle: ServoArc<ComputedValues>) -> nsStyleDisplay {
        nsStyleDisplay { mStyle: aStyle }
    }

    // mFloat, mClear: the computed values.
    pub fn mFloat(&self) -> StyleFloat {
        self.mStyle.get_box().float
    }
    pub fn mClear(&self) -> StyleClear {
        self.mStyle.get_box().clear
    }

    // mShapeOutside, mShapeMargin, mShapeImageThreshold: NOT PORTED, `shape-outside` is not in the
    // servo configuration of Stylo. mShapeOutside is always None; the other two are never read then.
    pub fn mShapeOutside(&self) -> StyleShapeOutside {
        StyleShapeOutside::None
    }
    pub fn mShapeMargin(&self) -> f32 {
        0.0
    }
    pub fn mShapeImageThreshold(&self) -> f32 {
        0.0
    }

    // Gecko: layout/generic/WritingModes.h nsStyleDisplay::UsedFloat
    // (defined at the end of WritingModes.h in Gecko, translated here with the struct)
    pub fn UsedFloat(&self, aCBWM: WritingMode) -> UsedFloat {
        match self.mFloat() {
            StyleFloat::None => UsedFloat::None,
            StyleFloat::Left => UsedFloat::Left,
            StyleFloat::Right => UsedFloat::Right,
            StyleFloat::InlineStart => {
                if aCBWM.IsBidiLTR() {
                    UsedFloat::Left
                } else {
                    UsedFloat::Right
                }
            }
            StyleFloat::InlineEnd => {
                if aCBWM.IsBidiLTR() {
                    UsedFloat::Right
                } else {
                    UsedFloat::Left
                }
            }
        }
    }

    // Gecko: layout/generic/WritingModes.h nsStyleDisplay::UsedClear
    pub fn UsedClear(&self, aCBWM: WritingMode) -> UsedClear {
        match self.mClear() {
            StyleClear::None => UsedClear::None,
            StyleClear::Left => UsedClear::Left,
            StyleClear::Right => UsedClear::Right,
            StyleClear::Both => UsedClear::Both,
            StyleClear::InlineStart => {
                if aCBWM.IsBidiLTR() {
                    UsedClear::Left
                } else {
                    UsedClear::Right
                }
            }
            StyleClear::InlineEnd => {
                if aCBWM.IsBidiLTR() {
                    UsedClear::Right
                } else {
                    UsedClear::Left
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Added for ReflowInput (layout/generic/ReflowInput.h): the style structs and helpers that the
// size computation reads. Same scheme as nsStyleDisplay above: a wrapper over the Arc of the
// frame's computed values, with Gecko's member names as methods.
// ---------------------------------------------------------------------------------------------

use mrt_style::style::values::computed::PositionProperty;
use mrt_style::style::values::generics::rect::Rect as StyleRect;
use mrt_style::style::values::specified::box_::{Display as StyleDisplayT, DisplayInside, DisplayOutside, Overflow};
pub use mrt_style::style::values::specified::box_::WillChangeBits as StyleWillChangeBits;

// Gecko's mozilla::StyleDisplay, generated by cbindgen from Stylo's Rust type (not in a header)
pub type StyleDisplay = StyleDisplayT;
// Gecko's mozilla::StyleDisplayOutside, generated by cbindgen from Stylo's Rust type (not in a header)
pub type StyleDisplayOutside = DisplayOutside;
// Gecko's mozilla::StyleDisplayInside, generated by cbindgen from Stylo's Rust type (not in a header)
pub type StyleDisplayInside = DisplayInside;
// Gecko's mozilla::StylePositionProperty, generated by cbindgen from Stylo's Rust type (not in a header)
pub type StylePositionProperty = PositionProperty;
// Gecko's mozilla::StyleOverflow, generated by cbindgen from Stylo's Rust type (not in a header)
pub type StyleOverflow = Overflow;
// Gecko's mozilla::StyleListStylePosition, generated by cbindgen from Stylo's Rust type (not in a header)
pub type StyleListStylePosition = mrt_style::style::properties::longhands::list_style_position::computed_value::T;
// Gecko's mozilla::StyleLineHeight, generated by cbindgen from Stylo's Rust type (not in a header)
pub type StyleLineHeight = mrt_style::style::values::computed::font::LineHeight;

// ADAPTER: Gecko's AnchorPosResolutionCache (anchor positioning is not supported, see
// AnchorPosResolutionParams).
pub struct AnchorPosResolutionCache;

// Gecko: layout/style/nsStyleStruct.h AnchorPosResolutionParams
// ADAPTER: anchor positioning (anchor(), anchor-size(), position-area) is not in this runtime's
// scope, so there is never an anchor to resolve; the parameters carry nothing. They are kept so
// that the style accessors (`ISize(wm, params)`, `GetMargin(side, wm, params)`, ...) and their
// callers keep the shape they have in Gecko.
#[derive(Clone, Copy, Default, Debug)]
pub struct AnchorPosResolutionParams;

impl AnchorPosResolutionParams {
    // Gecko: layout/style/nsStyleStruct.h AnchorPosResolutionParams::From(const nsIFrame*, AnchorPosResolutionCache*)
    pub fn From__nsIFrame_AnchorPosResolutionCache(aFrame: nsIFrame, aAnchorPosResolutionCache: *mut AnchorPosResolutionCache) -> AnchorPosResolutionParams {
        AnchorPosResolutionParams
    }
}

// Gecko: layout/style/nsStyleStruct.h AnchorPosOffsetResolutionParams
// ADAPTER: see AnchorPosResolutionParams. mCBSize is a pointer in Gecko; here an optional copy.
#[derive(Clone, Copy, Default, Debug)]
pub struct AnchorPosOffsetResolutionParams {
    pub mCBSize: Option<LogicalSize>,
    pub mBaseParams: AnchorPosResolutionParams,
}

impl AnchorPosOffsetResolutionParams {
    // Gecko: layout/style/nsStyleStruct.h AnchorPosOffsetResolutionParams::UseCBFrameSize
    pub fn UseCBFrameSize(aBaseParams: AnchorPosResolutionParams) -> AnchorPosOffsetResolutionParams {
        AnchorPosOffsetResolutionParams { mCBSize: None, mBaseParams: aBaseParams }
    }

    // Gecko: layout/style/nsStyleStruct.h AnchorPosOffsetResolutionParams::ExplicitCBFrameSize
    pub fn ExplicitCBFrameSize(aBaseParams: AnchorPosResolutionParams, aCBSize: Option<LogicalSize>) -> AnchorPosOffsetResolutionParams {
        AnchorPosOffsetResolutionParams { mCBSize: aCBSize, mBaseParams: aBaseParams }
    }
}

// Gecko: layout/style/nsStyleStruct.h AnchorResolvedSizeHelper
// ADAPTER: Gecko's AnchorResolvedSize is a reference-like wrapper; here the value itself (a clone
// of the style value), so the helper functions that build one return a StyleSize.
pub struct AnchorResolvedSizeHelper;

impl AnchorResolvedSizeHelper {
    // Gecko: AnchorResolvedSizeHelper::Auto
    pub fn Auto() -> StyleSize {
        StyleSize::Auto
    }

    // Gecko: AnchorResolvedSizeHelper::LengthPercentage
    pub fn LengthPercentage(aLengthPercentage: LengthPercentage) -> StyleSize {
        StyleSize::FromLengthPercentage(aLengthPercentage)
    }

    // Gecko: AnchorResolvedSizeHelper::Overridden
    pub fn Overridden(aSize: &StyleSize) -> StyleSize {
        aSize.clone()
    }
}

// ADAPTER: see AnchorResolvedSizeHelper; the max-size twin.
pub struct AnchorResolvedMaxSizeHelper;

impl AnchorResolvedMaxSizeHelper {
    // Gecko: AnchorResolvedMaxSizeHelper::None
    pub fn None() -> StyleMaxSize {
        StyleMaxSize::None
    }
}

impl nsStyleDisplay {
    // mDisplay, mOriginalDisplay, mPosition, mOverflowX, mOverflowY: the computed values.
    pub fn mDisplay(&self) -> StyleDisplay {
        self.mStyle.get_box().display
    }
    pub fn mOriginalDisplay(&self) -> StyleDisplay {
        self.mStyle.get_box().original_display
    }
    pub fn mPosition(&self) -> StylePositionProperty {
        self.mStyle.get_box().position
    }
    pub fn mOverflowX(&self) -> StyleOverflow {
        self.mStyle.get_box().overflow_x
    }
    pub fn mOverflowY(&self) -> StyleOverflow {
        self.mStyle.get_box().overflow_y
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::DisplayOutside
    pub fn DisplayOutside(&self) -> StyleDisplayOutside {
        self.mDisplay().outside()
    }
    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::DisplayInside
    pub fn DisplayInside(&self) -> StyleDisplayInside {
        self.mDisplay().inside()
    }
    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsListItem
    pub fn IsListItem(&self) -> bool {
        self.mDisplay().is_list_item()
    }
    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsInlineFlow
    pub fn IsInlineFlow(&self) -> bool {
        self.mDisplay().is_inline_flow()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsInlineInsideStyle
    // (StyleDisplay::IsInlineInside() is "inside is Flow and outside is Inline".)
    pub fn IsInlineInsideStyle(&self) -> bool {
        self.mDisplay().is_inline_flow()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsBlockOutsideStyle
    pub fn IsBlockOutsideStyle(&self) -> bool {
        self.DisplayOutside() == StyleDisplayOutside::Block
    }

    // Gecko: layout/style/nsStyleStructInlines.h nsStyleDisplay::IsInlineOutside(const nsIFrame*)
    pub fn IsInlineOutside(&self, aContextFrame: nsIFrame) -> bool {
        self.IsInlineOutsideStyle() && !self.IsFloating(aContextFrame) && !self.IsAbsolutelyPositioned(aContextFrame)
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsInlineOutsideStyle
    pub fn IsInlineOutsideStyle(&self) -> bool {
        self.mDisplay().outside() == StyleDisplayOutside::Inline
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsOriginalDisplayInlineOutside
    pub fn IsOriginalDisplayInlineOutside(&self) -> bool {
        self.mOriginalDisplay().outside() == StyleDisplayOutside::Inline
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsInnerTableStyle
    // (StyleDisplay::IsInternalTable(): outside is InternalTable.)
    pub fn IsInnerTableStyle(&self) -> bool {
        self.mDisplay().outside() == StyleDisplayOutside::InternalTable
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsFloatingStyle
    pub fn IsFloatingStyle(&self) -> bool {
        StyleFloat::None != self.mFloat()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsPositionedStyle
    pub fn IsPositionedStyle(&self) -> bool {
        self.mPosition() != StylePositionProperty::Static || self.mWillChangeBits().contains(StyleWillChangeBits::POSITION)
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::mWillChange.bits (the computed value's bits;
    // the feature list is not needed)
    pub fn mWillChangeBits(&self) -> StyleWillChangeBits {
        self.mStyle.get_box().will_change.bits
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::HasTransformStyle
    pub fn HasTransformStyle(&self) -> bool {
        self.HasTransformProperty()
            || self.HasIndividualTransform()
            || self.mStyle.get_box().transform_style == mrt_style::style::computed_values::transform_style::T::Preserve3d
            || self.mWillChangeBits().contains(StyleWillChangeBits::TRANSFORM)
            || !matches!(self.mStyle.get_box().offset_path, mrt_style::style::values::generics::motion::GenericOffsetPath::None)
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::HasTransformProperty
    pub fn HasTransformProperty(&self) -> bool {
        !self.mStyle.get_box().transform.0.is_empty()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::HasIndividualTransform
    pub fn HasIndividualTransform(&self) -> bool {
        use mrt_style::style::values::generics::transform::{Rotate, Scale, Translate};
        let b = self.mStyle.get_box();
        !matches!(b.rotate, Rotate::None) || !matches!(b.translate, Translate::None) || !matches!(b.scale, Scale::None)
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::HasPerspectiveStyle
    pub fn HasPerspectiveStyle(&self) -> bool {
        !matches!(self.mStyle.get_box().perspective, mrt_style::style::values::generics::box_::Perspective::None)
    }

    // Gecko: layout/style/nsStyleStructInlines.h nsStyleDisplay::IsFixedPosContainingBlockForContainLayoutAndPaintSupportingFrames
    pub fn IsFixedPosContainingBlockForContainLayoutAndPaintSupportingFrames(&self) -> bool {
        self.IsContainPaint() || self.IsContainLayout() || self.mWillChangeBits().contains(StyleWillChangeBits::CONTAIN)
    }

    // Gecko: layout/style/nsStyleStructInlines.h nsStyleDisplay::IsFixedPosContainingBlockForTransformSupportingFrames
    pub fn IsFixedPosContainingBlockForTransformSupportingFrames(&self) -> bool {
        // NOTE: Any CSS properties that influence the output of this function
        // should also look at mWillChange as necessary.
        self.HasTransformStyle() || self.HasPerspectiveStyle() || self.mWillChangeBits().contains(StyleWillChangeBits::PERSPECTIVE)
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleEffects::HasFilters and HasBackdropFilters
    // ADAPTER: the effects struct is read through the same computed values (there is no
    // nsStyleEffects adapter).
    pub fn HasFiltersOrBackdropFilters(&self) -> bool {
        let e = self.mStyle.get_effects();
        !e.filter.0.is_empty() || !e.backdrop_filter.0.is_empty()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsAbsolutelyPositionedStyle
    pub fn IsAbsolutelyPositionedStyle(&self) -> bool {
        StylePositionProperty::Absolute == self.mPosition() || StylePositionProperty::Fixed == self.mPosition()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsRelativelyOrStickyPositionedStyle
    pub fn IsRelativelyOrStickyPositionedStyle(&self) -> bool {
        StylePositionProperty::Relative == self.mPosition() || StylePositionProperty::Sticky == self.mPosition()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsRelativelyPositionedStyle
    pub fn IsRelativelyPositionedStyle(&self) -> bool {
        StylePositionProperty::Relative == self.mPosition()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsStickyPositionedStyle
    pub fn IsStickyPositionedStyle(&self) -> bool {
        StylePositionProperty::Sticky == self.mPosition()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsOutOfFlowStyle
    pub fn IsOutOfFlowStyle(&self) -> bool {
        self.IsAbsolutelyPositionedStyle() || self.IsFloatingStyle()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsScrollableOverflow
    pub fn IsScrollableOverflow(&self) -> bool {
        // Visible and Clip can be combined but not with other values,
        // so checking mOverflowX is enough.
        self.mOverflowX() != StyleOverflow::Visible && self.mOverflowX() != StyleOverflow::Clip
    }

    // Gecko: layout/style/nsStyleStructInlines.h nsStyleDisplay::IsFloating
    pub fn IsFloating(&self, aContextFrame: nsIFrame) -> bool {
        self.IsFloatingStyle() && !aContextFrame.IsInSVGTextSubtree()
    }

    // Gecko: layout/style/nsStyleStructInlines.h nsStyleDisplay::IsRelativelyOrStickyPositioned
    pub fn IsRelativelyOrStickyPositioned(&self, aContextFrame: nsIFrame) -> bool {
        self.IsRelativelyOrStickyPositionedStyle() && !aContextFrame.IsInSVGTextSubtree()
    }

    // Gecko: layout/style/nsStyleStructInlines.h nsStyleDisplay::IsRelativelyPositioned
    pub fn IsRelativelyPositioned(&self, aContextFrame: nsIFrame) -> bool {
        self.IsRelativelyPositionedStyle() && !aContextFrame.IsInSVGTextSubtree()
    }

    // Gecko: layout/style/nsStyleStructInlines.h nsStyleDisplay::IsStickyPositioned
    pub fn IsStickyPositioned(&self, aContextFrame: nsIFrame) -> bool {
        self.IsStickyPositionedStyle() && !aContextFrame.IsInSVGTextSubtree()
    }

    // Gecko: layout/style/nsStyleStructInlines.h nsStyleDisplay::IsAbsolutelyPositioned
    pub fn IsAbsolutelyPositioned(&self, aContextFrame: nsIFrame) -> bool {
        self.IsAbsolutelyPositionedStyle() && !aContextFrame.IsInSVGTextSubtree()
    }
}

// Added for nsBlockFrame.rs.
impl nsStyleDisplay {
    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsContainLayout
    // (mEffectiveContainment & StyleContain::LAYOUT, without the cases layout containment does not
    // apply to: non-atomic inline boxes, internal ruby boxes, internal table boxes.)
    pub fn IsContainLayout(&self) -> bool {
        use mrt_style::style::values::specified::box_::Contain;
        self.mStyle.get_box().contain.contains(Contain::LAYOUT) && !self.IsInlineFlow() && !self.IsInnerTableStyle()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::IsContainPaint
    // (the same restrictions as IsContainLayout)
    pub fn IsContainPaint(&self) -> bool {
        use mrt_style::style::values::specified::box_::Contain;
        self.mStyle.get_box().contain.contains(Contain::PAINT) && !self.IsInlineFlow() && !self.IsInnerTableStyle()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::mContainerType & (SIZE | INLINE_SIZE)
    // NOT PORTED: container queries: never a container. (Stylo 0.22 has `container-type`; reading it
    // is only useful once container size containment is ported.)
    pub fn mContainerType_SizeOrInlineSize(&self) -> bool {
        false
    }
}

// Gecko: layout/style/nsStyleStruct.h nsStylePosition
// ADAPTER: wraps the computed style that holds Stylo's `Position` struct. The accessors that take
// a WritingMode are translated from the end of layout/generic/WritingModes.h (WritingModes.rs keeps
// that C++ as a comment); in the servo configuration the computed `width`/`height` fields are the
// physical ones (Stylo maps `inline-size` etc. to them at cascade time), as in Gecko.
#[derive(Clone)]
pub struct nsStylePosition {
    mStyle: ServoArc<ComputedValues>,
}

impl nsStylePosition {
    pub fn new(aStyle: ServoArc<ComputedValues>) -> nsStylePosition {
        nsStylePosition { mStyle: aStyle }
    }

    // mBoxSizing, mAspectRatio: the computed values.
    pub fn mBoxSizing(&self) -> StyleBoxSizing {
        self.mStyle.get_position().box_sizing
    }
    pub fn mAspectRatio(&self) -> StyleAspectRatio {
        self.mStyle.get_position().aspect_ratio.clone()
    }
    // mFlexBasis: NOT PORTED (flex items are not translated; the only reader in ReflowInput.cpp is
    // the flex-item block of InitResizeFlags, which is stubbed).

    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::GetWidth, GetHeight, GetMinWidth, ...
    // (the AnchorResolved* return values are plain style values here, see AnchorPosResolutionParams)
    pub fn GetWidth(&self, aParams: &AnchorPosResolutionParams) -> StyleSize {
        self.mStyle.get_position().width.clone()
    }
    pub fn GetHeight(&self, aParams: &AnchorPosResolutionParams) -> StyleSize {
        self.mStyle.get_position().height.clone()
    }
    pub fn GetMinWidth(&self, aParams: &AnchorPosResolutionParams) -> StyleSize {
        self.mStyle.get_position().min_width.clone()
    }
    pub fn GetMinHeight(&self, aParams: &AnchorPosResolutionParams) -> StyleSize {
        self.mStyle.get_position().min_height.clone()
    }
    pub fn GetMaxWidth(&self, aParams: &AnchorPosResolutionParams) -> StyleMaxSize {
        self.mStyle.get_position().max_width.clone()
    }
    pub fn GetMaxHeight(&self, aParams: &AnchorPosResolutionParams) -> StyleMaxSize {
        self.mStyle.get_position().max_height.clone()
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::ISize
    pub fn ISize(&self, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> StyleSize {
        if aWM.IsVertical() { self.GetHeight(aParams) } else { self.GetWidth(aParams) }
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::MinISize
    pub fn MinISize(&self, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> StyleSize {
        if aWM.IsVertical() { self.GetMinHeight(aParams) } else { self.GetMinWidth(aParams) }
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::MaxISize
    pub fn MaxISize(&self, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> StyleMaxSize {
        if aWM.IsVertical() { self.GetMaxHeight(aParams) } else { self.GetMaxWidth(aParams) }
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::BSize
    pub fn BSize(&self, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> StyleSize {
        if aWM.IsVertical() { self.GetWidth(aParams) } else { self.GetHeight(aParams) }
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::MinBSize
    pub fn MinBSize(&self, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> StyleSize {
        if aWM.IsVertical() { self.GetMinWidth(aParams) } else { self.GetMinHeight(aParams) }
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::MaxBSize
    pub fn MaxBSize(&self, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> StyleMaxSize {
        if aWM.IsVertical() { self.GetMaxWidth(aParams) } else { self.GetMaxHeight(aParams) }
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::Size
    pub fn Size(&self, aAxis: LogicalAxis, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> StyleSize {
        if aAxis == LogicalAxis::Inline { self.ISize(aWM, aParams) } else { self.BSize(aWM, aParams) }
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::MinSize
    pub fn MinSize(&self, aAxis: LogicalAxis, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> StyleSize {
        if aAxis == LogicalAxis::Inline { self.MinISize(aWM, aParams) } else { self.MinBSize(aWM, aParams) }
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::MaxSize
    pub fn MaxSize(&self, aAxis: LogicalAxis, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> StyleMaxSize {
        if aAxis == LogicalAxis::Inline { self.MaxISize(aWM, aParams) } else { self.MaxBSize(aWM, aParams) }
    }

    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::GetAnchorResolvedInset(Side, params)
    pub fn GetAnchorResolvedInset__Side_AnchorPosOffsetResolutionParams(&self, aSide: Side, aParams: &AnchorPosOffsetResolutionParams) -> StyleInset {
        let pos = self.mStyle.get_position();
        match aSide {
            eSideTop => pos.top.clone(),
            eSideRight => pos.right.clone(),
            eSideBottom => pos.bottom.clone(),
            eSideLeft => pos.left.clone(),
        }
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::GetAnchorResolvedInset(LogicalSide, WritingMode, params)
    pub fn GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(&self, aSide: LogicalSide, aWM: WritingMode, aParams: &AnchorPosOffsetResolutionParams) -> StyleInset {
        self.GetAnchorResolvedInset__Side_AnchorPosOffsetResolutionParams(aWM.PhysicalSide(aSide), aParams)
    }

    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::ISizeCoordDependsOnContainer
    fn ISizeCoordDependsOnContainer<SizeOrMaxSize: StyleSizeLike>(aCoord: &SizeOrMaxSize) -> bool {
        if aCoord.IsLengthPercentage() {
            return aCoord.AsLengthPercentage().HasPercent();
        }
        aCoord.IsFitContent() || aCoord.BehavesLikeStretchOnInlineAxis()
    }

    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::BSizeCoordDependsOnContainer
    fn BSizeCoordDependsOnContainer<SizeOrMaxSize: StyleSizeLike>(aCoord: &SizeOrMaxSize) -> bool {
        if aCoord.IsLengthPercentage() {
            return aCoord.AsLengthPercentage().HasPercent();
        }
        aCoord.BehavesLikeStretchOnBlockAxis()
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::ISizeDependsOnContainer
    pub fn ISizeDependsOnContainer(aSize: &StyleSize) -> bool {
        aSize.IsAuto() || nsStylePosition::ISizeCoordDependsOnContainer(aSize)
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::MinISizeDependsOnContainer
    pub fn MinISizeDependsOnContainer(aSize: &StyleSize) -> bool {
        // NOTE: For a flex item, "min-inline-size:auto" is supposed to behave like
        // "min-content", which does depend on the container, so you might think we'd
        // need a special case for "flex item && min-inline-size:auto" here. However,
        // we don't actually need that special-case code, because flex items are
        // explicitly supposed to *ignore* their min-inline-size (i.e. behave like
        // it's 0) until the flex container explicitly considers it. So -- since the
        // flex container doesn't rely on this method, we don't need to worry about
        // special behavior for flex items' "min-inline-size:auto" values here.
        nsStylePosition::ISizeCoordDependsOnContainer(aSize)
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::MaxISizeDependsOnContainer
    pub fn MaxISizeDependsOnContainer(aSize: &StyleMaxSize) -> bool {
        // NOTE: The comment above MinISizeDependsOnContainer about flex items
        // applies here, too.
        nsStylePosition::ISizeCoordDependsOnContainer(aSize)
    }

    // Note that these functions count `auto` as depending on the container
    // since that's the case for absolutely positioned elements.
    // However, some callers do not care about this case and should check
    // for it, since it is the most common case.
    // FIXME: We should probably change the assumption to be the other way
    // around.
    // Gecko: layout/generic/WritingModes.h nsStylePosition::BSizeDependsOnContainer
    pub fn BSizeDependsOnContainer(aSize: &StyleSize) -> bool {
        aSize.BehavesLikeInitialValueOnBlockAxis() || nsStylePosition::BSizeCoordDependsOnContainer(aSize)
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::MinBSizeDependsOnContainer
    pub fn MinBSizeDependsOnContainer(aSize: &StyleSize) -> bool {
        nsStylePosition::BSizeCoordDependsOnContainer(aSize)
    }

    // Gecko: layout/generic/WritingModes.h nsStylePosition::MaxBSizeDependsOnContainer
    pub fn MaxBSizeDependsOnContainer(aSize: &StyleMaxSize) -> bool {
        nsStylePosition::BSizeCoordDependsOnContainer(aSize)
    }
}

// Gecko: layout/style/nsStyleStruct.h nsStyleMargin
#[derive(Clone)]
pub struct nsStyleMargin {
    mStyle: ServoArc<ComputedValues>,
}

impl nsStyleMargin {
    pub fn new(aStyle: ServoArc<ComputedValues>) -> nsStyleMargin {
        nsStyleMargin { mStyle: aStyle }
    }

    // mMargin.Get(side): the computed margin of a physical side.
    fn MarginOfSide(&self, aSide: Side) -> StyleMargin {
        let m = self.mStyle.get_margin();
        match aSide {
            eSideTop => m.margin_top.clone(),
            eSideRight => m.margin_right.clone(),
            eSideBottom => m.margin_bottom.clone(),
            eSideLeft => m.margin_left.clone(),
        }
    }

    // Returns false if any margin is layout-dependent in any way.
    // Percentage values and/or `anchor-size()` will do this.
    // Gecko: layout/style/nsStyleStruct.h nsStyleMargin::GetMargin(nsMargin&)
    pub fn GetMargin__nsMargin(&self, aMargin: &mut nsMargin) -> bool {
        let convertsToLength = AllPhysicalSides().all(|side| self.MarginOfSide(side).ConvertsToLength());

        if !convertsToLength {
            return false;
        }

        for side in AllPhysicalSides() {
            *aMargin.Side_mut(side) = self.MarginOfSide(side).AsLengthPercentage().ToLength();
        }
        true
    }

    // Attempt to return the resolved margin, resolving anchor functions, and
    // using a dummy percentage basis. If the resulting value returns true for
    // `HasPercent`, percentage value needs to be resolved with a proper basis at
    // a later point.
    // Gecko: layout/style/nsStyleStruct.h nsStyleMargin::GetMargin(Side, params)
    pub fn GetMargin__Side_AnchorPosResolutionParams(&self, aSide: Side, aParams: &AnchorPosResolutionParams) -> StyleMargin {
        self.MarginOfSide(aSide)
    }

    // As with other logical-coordinate accessors, definitions for these
    // are found in WritingModes.h.
    // Gecko: layout/generic/WritingModes.h nsStyleMargin::GetMargin(LogicalSide, WritingMode, params)
    pub fn GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(&self, aSide: LogicalSide, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> StyleMargin {
        self.GetMargin__Side_AnchorPosResolutionParams(aWM.PhysicalSide(aSide), aParams)
    }

    // Gecko: layout/generic/WritingModes.h nsStyleMargin::HasBlockAxisAuto
    pub fn HasBlockAxisAuto(&self, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> bool {
        self.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(LogicalSide::BStart, aWM, aParams).IsAuto()
            || self.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(LogicalSide::BEnd, aWM, aParams).IsAuto()
    }

    // Gecko: layout/generic/WritingModes.h nsStyleMargin::HasInlineAxisAuto
    pub fn HasInlineAxisAuto(&self, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> bool {
        self.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(LogicalSide::IStart, aWM, aParams).IsAuto()
            || self.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(LogicalSide::IEnd, aWM, aParams).IsAuto()
    }

    // Gecko: layout/generic/WritingModes.h nsStyleMargin::HasAuto
    pub fn HasAuto(&self, aAxis: LogicalAxis, aWM: WritingMode, aParams: &AnchorPosResolutionParams) -> bool {
        if aAxis == LogicalAxis::Inline { self.HasInlineAxisAuto(aWM, aParams) } else { self.HasBlockAxisAuto(aWM, aParams) }
    }
}

// Gecko: layout/style/nsStyleStruct.h nsStylePadding
#[derive(Clone)]
pub struct nsStylePadding {
    mStyle: ServoArc<ComputedValues>,
}

impl nsStylePadding {
    pub fn new(aStyle: ServoArc<ComputedValues>) -> nsStylePadding {
        nsStylePadding { mStyle: aStyle }
    }

    // mPadding: the four computed paddings as a StyleRect (top, right, bottom, left).
    // Gecko's NonNegativeLengthPercentage is the LengthPercentage itself (cbindgen flattens the
    // NonNegative wrapper), so the clones here are unwrapped LengthPercentages.
    pub fn mPadding(&self) -> StyleRect<LengthPercentage> {
        let p = self.mStyle.get_padding();
        StyleRect(p.padding_top.0.clone(), p.padding_right.0.clone(), p.padding_bottom.0.clone(), p.padding_left.0.clone())
    }

    // Gecko: layout/style/nsStyleStruct.h nsStylePadding::IsWidthDependent
    pub fn IsWidthDependent(&self) -> bool {
        let p = self.mPadding();
        !(p.0.ConvertsToLength() && p.1.ConvertsToLength() && p.2.ConvertsToLength() && p.3.ConvertsToLength())
    }

    // Gecko: layout/style/nsStyleStruct.h nsStylePadding::GetPadding
    pub fn GetPadding(&self, aPadding: &mut nsMargin) -> bool {
        if self.IsWidthDependent() {
            return false;
        }

        let p = self.mPadding();
        for side in AllPhysicalSides() {
            // Clamp negative calc() to 0.
            *aPadding.Side_mut(side) = std::cmp::max(p.Get__Side(side).ToLength(), 0);
        }
        true
    }
}

// Gecko: layout/style/nsStyleStruct.h nsStyleBorder
#[derive(Clone)]
pub struct nsStyleBorder {
    mStyle: ServoArc<ComputedValues>,
}

impl nsStyleBorder {
    pub fn new(aStyle: ServoArc<ComputedValues>) -> nsStyleBorder {
        nsStyleBorder { mStyle: aStyle }
    }

    // Return whether aStyle is a visible style.  Invisible styles cause
    // the relevant computed border width to be 0.
    // Note that this does *not* consider the effects of 'border-image':
    // if border-style is none, but there is a loaded border image,
    // HasVisibleStyle will be false even though there *is* a border.
    // Gecko: layout/style/nsStyleStruct.h nsStyleBorder::HasVisibleStyle
    pub fn HasVisibleStyle(&self, aSide: Side) -> bool {
        let b = self.mStyle.get_border();
        let style = match aSide {
            eSideTop => b.border_top_style,
            eSideRight => b.border_right_style,
            eSideBottom => b.border_bottom_style,
            eSideLeft => b.border_left_style,
        };
        // Gecko: IsVisibleBorderStyle
        !style.none_or_hidden()
    }

    // Get the computed border (plus rounding).
    // Gecko: layout/style/nsStyleStruct.h nsStyleBorder::GetComputedBorder
    // (Stylo's `BorderSideWidth` is an Au rounded down to a device pixel, which is what Gecko's mBorder
    // holds. Since Bug 1998285 it is not zeroed for a border style of none or hidden; that is done here.)
    pub fn GetComputedBorder(&self) -> nsMargin {
        let b = self.mStyle.get_border();
        let mut border = nsMargin::new(b.border_top_width.0.0, b.border_right_width.0.0, b.border_bottom_width.0.0, b.border_left_width.0.0);
        for side in AllPhysicalSides() {
            if !self.HasVisibleStyle(side) {
                *border.Side_mut(side) = 0;
            }
        }
        border
    }
}

// Gecko: layout/style/nsStyleStruct.h nsStyleText
#[derive(Clone)]
pub struct nsStyleText {
    mStyle: ServoArc<ComputedValues>,
}

impl nsStyleText {
    pub fn new(aStyle: ServoArc<ComputedValues>) -> nsStyleText {
        nsStyleText { mStyle: aStyle }
    }

    // mTextAlign: the computed value.
    pub fn mTextAlign(&self) -> StyleTextAlign {
        self.mStyle.get_inherited_text().text_align
    }
}

// Gecko: layout/style/nsStyleStruct.h nsStyleList
#[derive(Clone)]
pub struct nsStyleList {
    mStyle: ServoArc<ComputedValues>,
}

impl nsStyleList {
    pub fn new(aStyle: ServoArc<ComputedValues>) -> nsStyleList {
        nsStyleList { mStyle: aStyle }
    }

    // mListStylePosition: the computed value.
    pub fn mListStylePosition(&self) -> StyleListStylePosition {
        self.mStyle.get_list().list_style_position
    }
    // mListStyleType: NOT PORTED (only the font-size-inflation list margin adjustment reads it).
}

// Gecko: layout/style/nsStyleStruct.h nsStyleFont
// ADAPTER: only what the line-height computation reads. Gecko's mFont.size is `nsFont::size`
// (a StyleFontSize); here the computed font size in CSS pixels.
#[derive(Clone)]
pub struct nsStyleFont {
    mStyle: ServoArc<ComputedValues>,
}

impl nsStyleFont {
    pub fn new(aStyle: ServoArc<ComputedValues>) -> nsStyleFont {
        nsStyleFont { mStyle: aStyle }
    }

    // mLineHeight: the computed value.
    pub fn mLineHeight(&self) -> StyleLineHeight {
        self.mStyle.get_font().line_height.clone()
    }

    // mFont.size, in CSS pixels (Gecko: nsFont::size, a StyleCSSPixelLength after text zoom).
    pub fn mFontSizeInCSSPixels(&self) -> f32 {
        self.mStyle.get_font().font_size.computed_size().px()
    }

    // mFont: the font the style describes (see nsFont).
    // Gecko: nsFont also carries the font features and variations (NOT PORTED).
    pub fn mFont(&self) -> nsFont {
        let font = self.mStyle.get_font();
        nsFont {
            family: font.font_family.clone(),
            size: font.font_size.computed_size(),
            style: font.font_style,
            weight: font.font_weight,
        }
    }

    // mLanguage, mExplicitLanguage: NOT PORTED (the language of the text, used to choose a font);
    // fonts are not translated yet.
    pub fn mLanguage(&self) -> Option<nsAtom> {
        None
    }
    pub fn mExplicitLanguage(&self) -> bool {
        false
    }
}

// Gecko: layout/style/nsStyleStruct.h StyleLineHeight inline methods (IsNormal, IsNumber, IsLength,
// IsMozBlockHeight, AsNumber, AsLength).
pub trait StyleLineHeightExt {
    fn IsNormal(&self) -> bool;
    fn IsNumber(&self) -> bool;
    fn IsLength(&self) -> bool;
    fn IsMozBlockHeight(&self) -> bool;
}
impl StyleLineHeightExt for StyleLineHeight {
    fn IsNormal(&self) -> bool {
        matches!(self, StyleLineHeight::Normal)
    }
    fn IsNumber(&self) -> bool {
        matches!(self, StyleLineHeight::Number(_))
    }
    fn IsLength(&self) -> bool {
        matches!(self, StyleLineHeight::Length(_))
    }
    // `-moz-block-height` exists only in the gecko configuration of Stylo (used by the UA sheet
    // for some form controls); the servo configuration never produces it.
    fn IsMozBlockHeight(&self) -> bool {
        false
    }
}

// Gecko: layout/style/PseudoStyleType.h mozilla::PseudoStyleType
// ADAPTER: only the types layout code compares against. The servo configuration of Stylo has
// `::marker` and no `-moz-*` anonymous box types (frame constructors say what an anonymous box
// is, see FrameData::mIsBlockWrapper); everything else that is a pseudo-element is `Other`.
// The table anonymous boxes (MozTable ... MozCellContent) are names for the frame constructor to pass
// to its ResolveInheritingAnonymousBoxStyle and ResolveNonInheritingAnonymousBoxStyle; a ComputedStyle
// never reports them from GetPseudoType.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PseudoStyleType {
    NotPseudo,
    Before,
    After,
    Marker,
    Other,
    MozTable,
    MozInlineTable,
    MozTableWrapper,
    MozTableColumnGroup,
    MozTableColumn,
    MozTableRowGroup,
    MozTableRow,
    MozTableCell,
    MozCellContent,
    // `::-moz-anonymous-item`: the anonymous block that wraps a run of text and inline content in a flex
    // or grid container (ua.css: display: block).
    MozAnonymousItem,
    // `::-moz-block-inside-inline-wrapper`: the anonymous block of an {ib} split (frames that have
    // nsIFrame::IsBlockWrapper and no other pseudo type).
    MozBlockInsideInlineWrapper,
}

// Gecko: layout/style/ComputedStyle.h ComputedStyle::GetPseudoType
pub trait ComputedStyleExt {
    fn GetPseudoType(&self) -> PseudoStyleType;
}
impl ComputedStyleExt for ComputedValues {
    fn GetPseudoType(&self) -> PseudoStyleType {
        use mrt_style::style::servo::selector_parser::PseudoElement;
        match self.pseudo() {
            None => PseudoStyleType::NotPseudo,
            Some(PseudoElement::Before) => PseudoStyleType::Before,
            Some(PseudoElement::After) => PseudoStyleType::After,
            Some(PseudoElement::Marker) => PseudoStyleType::Marker,
            Some(_) => PseudoStyleType::Other,
        }
    }
}

// Gecko: gfx/thebes/gfxFont.h / gfx/src/nsFont.h nsFont
// ADAPTER: the size, family list, style and weight (what a gfxFontStyle and a gfxFontGroup are made
// from); the font features, variations and variants are not carried. The size is a computed CSS pixel
// length, as Gecko's `StyleFontSize` is after text zoom.
#[derive(Clone, Debug)]
pub struct nsFont {
    pub family: mrt_style::style::values::computed::font::FontFamily,
    pub size: ComputedLength,
    pub style: mrt_style::style::values::computed::font::FontStyle,
    pub weight: mrt_style::style::values::computed::font::FontWeight,
}

impl nsFont {
    // Gecko: gfx/src/nsFont.cpp nsFont::Equals (CalcDifference(aOther) == MaxDifference::eNone)
    // REDUCED to the members this nsFont carries (see above): style, weight, size, family.
    pub fn Equals(&self, aOther: &nsFont) -> bool {
        self.style == aOther.style && self.weight == aOther.weight && self.size == aOther.size && self.family == aOther.family
    }
}

// ADAPTER: style values that this Stylo configuration lacks (or that cbindgen generates in Gecko).
// Moved here from the TEMP section of BlockReflowState.rs and from nsLineLayoutAdapter.rs, which each
// had a copy.

// Gecko: layout/style/nsStyleConsts.h StyleFloatEdge (-moz-float-edge)
// NOT PORTED: -moz-float-edge is not in the servo configuration of Stylo; always content-box.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleFloatEdge {
    ContentBox,
    MarginBox,
}

// ADAPTER (generated by cbindgen from Stylo in Gecko, no header): mozilla::StyleBoxDecorationBreak
// NOT PORTED: always slice. (Stylo 0.22 has `box-decoration-break`; not read yet.)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleBoxDecorationBreak {
    Slice,
    Clone,
}

// ADAPTER (generated by cbindgen from Stylo in Gecko, no header): mozilla::StyleBreakWithin
// NOT PORTED: break-inside is not read from the style yet; always auto.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleBreakWithin {
    Auto,
    Avoid,
}

impl nsStyleBorder {
    // Gecko: layout/style/nsStyleStruct.h nsStyleBorder::mFloatEdge
    pub fn mFloatEdge(&self) -> StyleFloatEdge {
        StyleFloatEdge::ContentBox
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleBorder::mBoxDecorationBreak
    pub fn mBoxDecorationBreak(&self) -> StyleBoxDecorationBreak {
        StyleBoxDecorationBreak::Slice
    }
}

impl nsStyleDisplay {
    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::mBreakInside
    pub fn mBreakInside(&self) -> StyleBreakWithin {
        StyleBreakWithin::Auto
    }
}

// ADAPTER (agent A, tables): Gecko's StyleTableLayout, StyleBorderCollapse and StyleVisibility are
// generated from Stylo's Rust enums; here they are those enums (variants Auto/Fixed,
// Separate/Collapse, Visible/Hidden/Collapse).
pub type StyleTableLayout = mrt_style::style::properties::longhands::table_layout::computed_value::T;
pub type StyleBorderCollapse = mrt_style::style::properties::longhands::border_collapse::computed_value::T;
pub type StyleEmptyCells = mrt_style::style::properties::longhands::empty_cells::computed_value::T;
pub type StyleVisibility = mrt_style::style::properties::longhands::visibility::computed_value::T;

// Gecko: layout/style/nsStyleStruct.h nsStyleTable
// ADAPTER: wraps the computed style that holds Stylo's `Table` struct (`table-layout`).
#[derive(Clone)]
pub struct nsStyleTable {
    mStyle: ServoArc<ComputedValues>,
}

impl nsStyleTable {
    pub fn new(aStyle: ServoArc<ComputedValues>) -> nsStyleTable {
        nsStyleTable { mStyle: aStyle }
    }

    // Gecko: nsStyleTable::mLayoutStrategy
    pub fn mLayoutStrategy(&self) -> StyleTableLayout {
        self.mStyle.get_table().table_layout
    }
}

// Gecko: layout/style/nsStyleStruct.h nsStyleTableBorder
// ADAPTER: wraps the computed style that holds Stylo's `InheritedTable` struct.
#[derive(Clone)]
pub struct nsStyleTableBorder {
    mStyle: ServoArc<ComputedValues>,
}

impl nsStyleTableBorder {
    pub fn new(aStyle: ServoArc<ComputedValues>) -> nsStyleTableBorder {
        nsStyleTableBorder { mStyle: aStyle }
    }

    // Gecko: nsStyleTableBorder::mBorderCollapse
    pub fn mBorderCollapse(&self) -> StyleBorderCollapse {
        self.mStyle.get_inherited_table().border_collapse
    }

    // Gecko: nsStyleTableBorder::mEmptyCells
    pub fn mEmptyCells(&self) -> StyleEmptyCells {
        self.mStyle.get_inherited_table().empty_cells
    }

    // Gecko: nsStyleTableBorder::mBorderSpacing.width (a StyleCSSPixelLength; ToAppUnits() is
    // the app-unit value, which Stylo's computed `Length` already is in px, so convert).
    pub fn mBorderSpacingWidth_ToAppUnits(&self) -> nscoord {
        StyleCSSPixelLength_ToAppUnits(self.mStyle.get_inherited_table().border_spacing.0.width.0)
    }

    // Gecko: nsStyleTableBorder::mBorderSpacing.height
    pub fn mBorderSpacingHeight_ToAppUnits(&self) -> nscoord {
        StyleCSSPixelLength_ToAppUnits(self.mStyle.get_inherited_table().border_spacing.0.height.0)
    }
}

// Gecko: layout/style/nsStyleStruct.h nsStyleVisibility
// ADAPTER: wraps the computed style that holds Stylo's `InheritedBox` struct (`visibility`).
#[derive(Clone)]
pub struct nsStyleVisibility {
    mStyle: ServoArc<ComputedValues>,
}

impl nsStyleVisibility {
    pub fn new(aStyle: ServoArc<ComputedValues>) -> nsStyleVisibility {
        nsStyleVisibility { mStyle: aStyle }
    }

    // Gecko: nsStyleVisibility::mVisible
    pub fn mVisible(&self) -> StyleVisibility {
        self.mStyle.get_inherited_box().visibility
    }

    // Gecko: nsStyleVisibility::mDirection
    pub fn mDirection(&self) -> StyleDirection {
        self.mStyle.get_inherited_box().direction
    }

    // Gecko: nsStyleVisibility::IsVisible
    pub fn IsVisible(&self) -> bool {
        self.mVisible() == StyleVisibility::Visible
    }
}

impl nsIFrame {
    // Gecko: layout/style/nsStyleStructList.h (the generated Style<Struct>() accessors)
    pub fn StyleTable(&self) -> nsStyleTable {
        nsStyleTable::new(self.Style())
    }
    pub fn StyleTableBorder(&self) -> nsStyleTableBorder {
        nsStyleTableBorder::new(self.Style())
    }
    pub fn StyleVisibility(&self) -> nsStyleVisibility {
        nsStyleVisibility::new(self.Style())
    }
}

impl nsStyleDisplay {
    // Gecko: layout/style/nsStyleStruct.h nsStyleDisplay::BreakBefore, BreakAfter
    // NOT PORTED: forced page and column breaks (break-before/break-after) are not read from the style
    // yet; always false.
    pub fn BreakBefore(&self) -> bool {
        false
    }
    pub fn BreakAfter(&self) -> bool {
        false
    }
}

// Flexbox members of nsStylePosition (nsStyleStruct.h), for nsFlexContainerFrame.rs.
// ADAPTER: each member is a method that reads Stylo's `Position` struct (like mBoxSizing above).
impl nsStylePosition {
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mFlexBasis
    pub fn mFlexBasis(&self) -> StyleFlexBasis {
        self.mStyle.get_position().flex_basis.clone().into()
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mFlexGrow
    pub fn mFlexGrow(&self) -> f32 {
        self.mStyle.get_position().flex_grow.0
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mFlexShrink
    pub fn mFlexShrink(&self) -> f32 {
        self.mStyle.get_position().flex_shrink.0
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mFlexWrap
    pub fn mFlexWrap(&self) -> StyleFlexWrap {
        self.mStyle.get_position().flex_wrap
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mFlexDirection
    pub fn mFlexDirection(&self) -> StyleFlexDirection {
        self.mStyle.get_position().flex_direction
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mOrder
    pub fn mOrder(&self) -> i32 {
        self.mStyle.get_position().order
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mJustifyContent
    pub fn mJustifyContent(&self) -> StyleContentDistribution {
        self.mStyle.get_position().justify_content
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mAlignContent
    pub fn mAlignContent(&self) -> StyleContentDistribution {
        self.mStyle.get_position().align_content
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mAlignItems (a StyleItemPlacement; its AlignFlags as a StyleSelfAlignment)
    pub fn mAlignItems(&self) -> StyleSelfAlignment {
        mrt_style::style::values::specified::align::SelfAlignment(self.mStyle.get_position().align_items.0)
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mAlignSelf
    pub fn mAlignSelf(&self) -> StyleSelfAlignment {
        self.mStyle.get_position().align_self
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mRowGap, mColumnGap
    pub fn mRowGap(&self) -> StyleNonNegativeLengthPercentageOrNormal {
        self.mStyle.get_position().row_gap.clone()
    }
    pub fn mColumnGap(&self) -> StyleNonNegativeLengthPercentageOrNormal {
        self.mStyle.get_position().column_gap.clone()
    }

    // Grid members of nsStylePosition (nsStyleStruct.h), for nsGridContainerFrame.rs.
    // ADAPTER: references into Stylo's `Position` struct, valid as long as this wrapper.
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mGridTemplateColumns
    pub fn mGridTemplateColumns(&self) -> &StyleGridTemplateComponent {
        &self.mStyle.get_position().grid_template_columns
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mGridTemplateRows
    pub fn mGridTemplateRows(&self) -> &StyleGridTemplateComponent {
        &self.mStyle.get_position().grid_template_rows
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mGridAutoColumns
    pub fn mGridAutoColumns(&self) -> &StyleImplicitGridTracks {
        &self.mStyle.get_position().grid_auto_columns
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mGridAutoRows
    pub fn mGridAutoRows(&self) -> &StyleImplicitGridTracks {
        &self.mStyle.get_position().grid_auto_rows
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mGridAutoFlow
    pub fn mGridAutoFlow(&self) -> StyleGridAutoFlow {
        self.mStyle.get_position().grid_auto_flow
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mGridTemplateAreas
    pub fn mGridTemplateAreas(&self) -> &StyleGridTemplateAreas {
        &self.mStyle.get_position().grid_template_areas
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mGridColumnStart
    pub fn mGridColumnStart(&self) -> &StyleGridLine {
        &self.mStyle.get_position().grid_column_start
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mGridColumnEnd
    pub fn mGridColumnEnd(&self) -> &StyleGridLine {
        &self.mStyle.get_position().grid_column_end
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mGridRowStart
    pub fn mGridRowStart(&self) -> &StyleGridLine {
        &self.mStyle.get_position().grid_row_start
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mGridRowEnd
    pub fn mGridRowEnd(&self) -> &StyleGridLine {
        &self.mStyle.get_position().grid_row_end
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mJustifySelf
    pub fn mJustifySelf(&self) -> StyleSelfAlignment {
        self.mStyle.get_position().justify_self
    }
    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::mJustifyItems (`.computed` is the used value:
    // the AlignFlags of its StyleItemPlacement are those of the returned StyleSelfAlignment)
    pub fn mJustifyItems(&self) -> StyleSelfAlignment {
        mrt_style::style::values::specified::align::SelfAlignment(self.mStyle.get_position().justify_items.computed.0.0)
    }
    // Gecko: layout/style/nsStyleStructInlines.h nsStylePosition::UsedSelfAlignment (axis in the container's WM)
    pub fn UsedSelfAlignment__LogicalAxis_ComputedStyle(&self, aAlignContainerAxis: LogicalAxis, aAlignContainerStyle: Option<ServoArc<ComputedValues>>) -> StyleAlignFlags {
        if aAlignContainerAxis == LogicalAxis::Block {
            self.UsedAlignSelf(aAlignContainerStyle).0
        } else {
            self.UsedJustifySelf(aAlignContainerStyle).0
        }
    }
    // Gecko: layout/style/nsStyleStructInlines.h nsStylePosition::UsedSelfAlignment (axis in the subject's WM)
    pub fn UsedSelfAlignment__WritingMode_LogicalAxis_WritingMode_ComputedStyle(&self, aAlignSubjectWM: WritingMode, aAlignSubjectAxis: LogicalAxis, aAlignContainerWM: WritingMode, aAlignContainerStyle: Option<ServoArc<ComputedValues>>) -> StyleAlignFlags {
        let alignContainerAxis = aAlignSubjectWM.ConvertAxisTo(aAlignSubjectAxis, aAlignContainerWM);
        self.UsedSelfAlignment__LogicalAxis_ComputedStyle(alignContainerAxis, aAlignContainerStyle)
    }
    // (layout/style/nsStyleStructInlines.h nsStylePosition::UsedContentAlignment; the drift tool cannot locate it at the base revision, so no marker)
    pub fn UsedContentAlignment(&self, aAxis: LogicalAxis) -> StyleContentDistribution {
        if aAxis == LogicalAxis::Block { self.mAlignContent() } else { self.mJustifyContent() }
    }
    // Gecko: layout/style/nsStyleStruct.cpp nsStylePosition::UsedJustifySelf
    pub fn UsedJustifySelf(&self, aParent: Option<ServoArc<ComputedValues>>) -> StyleSelfAlignment {
        if self.mJustifySelf().0 != StyleAlignFlags::AUTO {
            return self.mJustifySelf();
        }
        if let Some(parent) = aParent {
            let inheritedJustifyItems = nsStylePosition::new(parent).mJustifyItems().0;
            return mrt_style::style::values::specified::align::SelfAlignment(inheritedJustifyItems & !StyleAlignFlags::LEGACY);
        }
        mrt_style::style::values::specified::align::SelfAlignment(StyleAlignFlags::NORMAL)
    }

    // Gecko: layout/style/nsStyleStruct.h nsStylePosition::UsedAlignSelf
    // (align-self:auto computes to the parent's align-items; with no parent, 'normal')
    pub fn UsedAlignSelf(&self, aParent: Option<ServoArc<ComputedValues>>) -> StyleSelfAlignment {
        if self.mAlignSelf().0 != StyleAlignFlags::AUTO {
            return self.mAlignSelf();
        }
        if let Some(parent) = aParent {
            return nsStylePosition::new(parent).mAlignItems();
        }
        mrt_style::style::values::specified::align::SelfAlignment(StyleAlignFlags::NORMAL)
    }
}

impl nsStyleVisibility {
    // Gecko: layout/style/nsStyleStruct.h nsStyleVisibility::UseLegacyCollapseBehavior
    // NOT PORTED: -moz-box-collapse is not in this Stylo configuration (mMozBoxCollapse is always
    // 'flex', the non-legacy value).
    pub fn UseLegacyCollapseBehavior(&self) -> bool {
        false
    }

    // Gecko: layout/style/nsStyleStruct.h nsStyleVisibility::IsCollapse
    pub fn IsCollapse(&self) -> bool {
        self.mVisible() == StyleVisibility::Collapse
    }
}
