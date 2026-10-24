/* DOM object returned from element.getComputedStyle() */
// Translated from layout/style/nsComputedDOMStyle.cpp (GECKO_REV): GetPropertyValue, the style and
// frame lookup of UpdateCurrentStyleSources, and the getters of the properties whose value depends on
// layout (width, height, the min and max sizes, the four offsets, padding and margin), with the
// helpers they use (IsNonReplacedInline, PaddingNeedsUsedValue, GetAdjustedValuesForBoxSizing,
// GetOffsetWidthFor and the three offset kinds, the SetValueTo* functions, the containing-block
// getters). Every other property is serialized by Stylo, as Gecko hands it to Servo
// (`Servo_GetResolvedValue`, mrt_style::computed).
//
// ADAPTER: there is no nsComputedDOMStyle object, no CSSValue and no flushes here. The caller (the
// page's layout thread) has flushed style and layout and asks for one property of one element; a
// value is returned as Gecko's `nsROCSSPrimitiveValue::GetCssText` writes it. The element's frames
// are its primary frame (the outer frame) and its style frame (the inner frame, the table frame of a
// table wrapper), as UpdateCurrentStyleSources finds them.
//
// NOT PORTED (GAPS B51): pseudo-elements' frames (a `::before` or `::after` value comes from its
// style alone); `transform`, `transform-origin` and `perspective-origin` (DoGetTransform's matrix and
// the percentages resolved against the frame: Stylo's computed value is given);
// `grid-template-rows` and `-columns` of a grid container (the tracks as laid out); `line-height:
// -moz-block-height`; `-moz-osx-font-smoothing`; anchor positioning and position fallbacks (the
// port has neither); scrollbar sizes and the visual viewport in GetUsedAbsoluteOffset (the port has
// no scroll container frames and no classic scrollbars); the native theme's used padding
// (HasNativeAppearance: the servo configuration has no `appearance`).
use crate::*;
use mrt_style::computed;
use mrt_style::mrt_dom::NodeId;
use mrt_style::style::properties::{ComputedValues, LonghandId, PropertyDeclarationId, PropertyId};
use mrt_style::style::servo_arc::Arc as ServoArc;
use mrt_style::style::values::computed::length_percentage::LengthPercentage as ComputedLP;
use mrt_style::StyleDom;
use mrt_style::style_traits::ToCss;

// Gecko: layout/style/nsComputedDOMStyle.h nsComputedDOMStyle (the members GetPropertyValue reads)
struct nsComputedDOMStyle {
    mComputedStyle: ServoArc<ComputedValues>,
    // The frames of the element, None without one (display: none, display: contents, a
    // pseudo-element).
    mOuterFrame: Option<nsIFrame>,
    mInnerFrame: Option<nsIFrame>,
}

// Gecko: xpcom/string/nsTSubstring.cpp nsTSubstring::AppendFloat(float), which
// nsStyleUtil::AppendCSSNumber calls: six significant digits, without trailing zeros.
fn AppendCSSNumber(aNumber: f32) -> String {
    if aNumber == 0.0 || !aNumber.is_finite() {
        return if aNumber.is_finite() { "0".to_string() } else { aNumber.to_string() };
    }
    let magnitude = (aNumber.abs() as f64).log10().floor() as i32;
    let decimals = (5 - magnitude).max(0) as usize;
    let mut s = format!("{:.*}", decimals, aNumber as f64);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" { "0".to_string() } else { s }
}

// Gecko: layout/style/nsROCSSPrimitiveValue.cpp GetCssText, CSS_PX
fn PixelsToCSSValue(aPixels: f32) -> String {
    AppendCSSNumber(aPixels) + "px"
}

// Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::AppUnitsToCSSValue
fn AppUnitsToCSSValue(aAppUnits: nscoord) -> String {
    PixelsToCSSValue(aAppUnits as f32 / AppUnitsPerCSSPixel() as f32)
}

// Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::SetValueToLengthPercentage
fn SetValueToLengthPercentage(aLength: &ComputedLP, aClampNegativeCalc: bool) -> String {
    if let Some(length) = aLength.to_length() {
        let mut length = length.px();
        if aClampNegativeCalc {
            length = length.max(0.0);
        }
        return PixelsToCSSValue(length);
    }
    if let Some(percentage) = aLength.to_percentage() {
        let mut result = percentage.0;
        if aClampNegativeCalc {
            result = result.max(0.0);
        }
        // nsROCSSPrimitiveValue::GetCssText, CSS_PERCENTAGE
        return AppendCSSNumber(result * 100.0) + "%";
    }
    // Servo_LengthPercentage_ToCss
    aLength.to_css_string()
}

// Gecko: layout/style/nsComputedDOMStyle.cpp IsNonReplacedInline
fn IsNonReplacedInline(aFrame: nsIFrame) -> bool {
    // FIXME: this should be IsInlineInsideStyle() since width/height
    // doesn't apply to ruby boxes.
    aFrame.StyleDisplay().IsInlineFlow()
        && !aFrame.IsReplaced()
        && !aFrame.IsFieldSetFrame()
        && !aFrame.IsBlockFrame()
        && !aFrame.IsScrollContainerFrame()
        && !aFrame.IsColumnSetWrapperFrame()
}

// Gecko: layout/style/nsComputedDOMStyle.cpp PaddingNeedsUsedValue
// ADAPTER: HasNativeAppearance is false (see the file's head).
fn PaddingNeedsUsedValue(aValue: &ComputedLP) -> bool {
    aValue.to_length().is_none()
}

impl nsComputedDOMStyle {
    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetPropertyValue(NonCustomCSSPropertyId, ...)
    fn GetPropertyValue(&self, aProp: &PropertyId) -> String {
        let longhand = match aProp.as_shorthand() {
            Err(PropertyDeclarationId::Longhand(longhand)) => longhand,
            // custom properties and shorthands are serialized by Servo
            _ => return computed::resolved_value(&self.mComputedStyle, aProp),
        };
        // Servo_ResolveLogicalProperty
        let longhand = longhand.to_physical(self.mComputedStyle.writing_mode);
        match longhand {
            LonghandId::Width => self.DoGetWidth(),
            LonghandId::Height => self.DoGetHeight(),
            LonghandId::MinWidth => self.DoGetMinWidth(),
            LonghandId::MinHeight => self.DoGetMinHeight(),
            LonghandId::Top => self.GetOffsetWidthFor(eSideTop),
            LonghandId::Right => self.GetOffsetWidthFor(eSideRight),
            LonghandId::Bottom => self.GetOffsetWidthFor(eSideBottom),
            LonghandId::Left => self.GetOffsetWidthFor(eSideLeft),
            LonghandId::PaddingTop => self.GetPaddingWidthFor(eSideTop),
            LonghandId::PaddingRight => self.GetPaddingWidthFor(eSideRight),
            LonghandId::PaddingBottom => self.GetPaddingWidthFor(eSideBottom),
            LonghandId::PaddingLeft => self.GetPaddingWidthFor(eSideLeft),
            LonghandId::MarginTop => self.GetMarginFor(eSideTop),
            LonghandId::MarginRight => self.GetMarginFor(eSideRight),
            LonghandId::MarginBottom => self.GetMarginFor(eSideBottom),
            LonghandId::MarginLeft => self.GetMarginFor(eSideLeft),
            // DoGetMaxWidth and DoGetMaxHeight give the computed value (SetValueToMaxSize), as Servo
            // serializes it; so do the properties Gecko serializes by Servo
            _ => computed::longhand_value(&self.mComputedStyle, longhand),
        }
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetAdjustedValuesForBoxSizing
    fn GetAdjustedValuesForBoxSizing(&self, aInnerFrame: nsIFrame) -> nsMargin {
        // We want the width/height of whatever parts 'width' or 'height' controls,
        // which can be different depending on the value of the 'box-sizing' property.
        let mut adjustment = nsMargin::default();
        if self.mComputedStyle.get_position().box_sizing == StyleBoxSizing::BorderBox {
            adjustment = aInnerFrame.GetUsedBorderAndPadding();
        }
        adjustment
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::DoGetHeight
    fn DoGetHeight(&self) -> String {
        if let Some(inner) = self.mInnerFrame.filter(|f| !IsNonReplacedInline(*f)) {
            let adjustedValues = self.GetAdjustedValuesForBoxSizing(inner);
            return AppUnitsToCSSValue(inner.GetContentRect().height + adjustedValues.TopBottom());
        }
        computed::longhand_value(&self.mComputedStyle, LonghandId::Height)
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::DoGetWidth
    fn DoGetWidth(&self) -> String {
        if let Some(inner) = self.mInnerFrame.filter(|f| !IsNonReplacedInline(*f)) {
            let adjustedValues = self.GetAdjustedValuesForBoxSizing(inner);
            return AppUnitsToCSSValue(inner.GetContentRect().width + adjustedValues.LeftRight());
        }
        computed::longhand_value(&self.mComputedStyle, LonghandId::Width)
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::ShouldHonorMinSizeAutoInAxis
    fn ShouldHonorMinSizeAutoInAxis(&self) -> bool {
        let Some(outer) = self.mOuterFrame else {
            // Per https://drafts.csswg.org/css-sizing/#valdef-width-auto
            // min-{width,height}:auto "resolves to zero when no box is generated".
            return false;
        };
        if outer.IsFlexOrGridItem() {
            // Flex and grid items have special significance for min-width:auto and
            // min-height:auto, so we faithfully report "auto" from getComputedStyle
            // for those frames:
            return true;
        }
        if outer.StylePosition().mAspectRatio() != StyleAspectRatio::auto() {
            // Frames with non-default CSS 'aspect-ratio' have special significance
            // for min-{width,height}:auto as well, so we faithfully report "auto"
            // for them too, per https://github.com/w3c/csswg-drafts/issues/11716
            return true;
        }

        // If we're not in one of the special cases above, then we return false
        // which means that "auto" will get reported as "0" in getComputedStyle.
        false
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::DoGetMinHeight
    fn DoGetMinHeight(&self) -> String {
        if self.mComputedStyle.get_position().min_height.IsAuto() && !self.ShouldHonorMinSizeAutoInAxis() {
            return PixelsToCSSValue(0.0);
        }
        computed::longhand_value(&self.mComputedStyle, LonghandId::MinHeight)
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::DoGetMinWidth
    fn DoGetMinWidth(&self) -> String {
        if self.mComputedStyle.get_position().min_width.IsAuto() && !self.ShouldHonorMinSizeAutoInAxis() {
            return PixelsToCSSValue(0.0);
        }
        computed::longhand_value(&self.mComputedStyle, LonghandId::MinWidth)
    }

    fn Inset(&self, aSide: Side) -> StyleInset {
        let pos = self.mComputedStyle.get_position();
        match aSide {
            eSideTop => pos.top.clone(),
            eSideRight => pos.right.clone(),
            eSideBottom => pos.bottom.clone(),
            eSideLeft => pos.left.clone(),
        }
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetOffsetWidthFor
    fn GetOffsetWidthFor(&self, aSide: Side) -> String {
        let mut position = self.mComputedStyle.get_box().position;
        if self.mOuterFrame.is_none() {
            // GetNonStaticPositionOffset or GetAbsoluteOffset don't handle elements
            // without frames in any sensible way. GetStaticOffset, however, is perfect
            // for that case.
            position = StylePositionProperty::Static;
        }

        match position {
            StylePositionProperty::Static => self.GetStaticOffset(aSide),
            StylePositionProperty::Sticky => self.GetNonStaticPositionOffset(aSide, false, nsComputedDOMStyle::GetScrollFrameContentSize),
            StylePositionProperty::Absolute | StylePositionProperty::Fixed => self.GetAbsoluteOffset(aSide),
            StylePositionProperty::Relative => self.GetNonStaticPositionOffset(aSide, true, nsComputedDOMStyle::GetCBContentSize),
        }
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetNonStaticPositionOffset
    // (the two PercentageBaseGetters are one function here, giving the width and the height)
    fn GetNonStaticPositionOffset(&self, aSide: Side, aResolveAuto: bool, aBaseGetter: fn(&Self) -> Option<nsSize>) -> String {
        let mut sign = 1;
        let mut coord = self.Inset(aSide);

        if coord.IsAuto() {
            if !aResolveAuto {
                return "auto".to_string();
            }
            coord = self.Inset(NS_OPPOSITE_SIDE(aSide));
            sign = -1;
        }
        if coord.IsAuto() {
            return PixelsToCSSValue(0.0);
        }

        let lp = coord.AsLengthPercentage();
        if let Some(length) = lp.to_length() {
            return PixelsToCSSValue(sign as f32 * length.px());
        }

        let Some(base) = aBaseGetter(self) else {
            return PixelsToCSSValue(0.0);
        };
        let percentageBase = if aSide == eSideLeft || aSide == eSideRight { base.width } else { base.height };

        AppUnitsToCSSValue(sign * lp.Resolve(percentageBase))
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetAbsoluteOffset
    fn GetAbsoluteOffset(&self, aSide: Side) -> String {
        let coord = self.Inset(aSide);
        let oppositeCoord = self.Inset(NS_OPPOSITE_SIDE(aSide));

        if coord.IsAuto() || oppositeCoord.IsAuto() {
            return AppUnitsToCSSValue(self.GetUsedAbsoluteOffset(aSide));
        }

        self.GetNonStaticPositionOffset(aSide, false, nsComputedDOMStyle::GetCBPaddingRectSize)
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetUsedAbsoluteOffset
    // ADAPTER: no scrollbar sizes and no AdjustViewportSizeForFixedPosition (see the file's head).
    fn GetUsedAbsoluteOffset(&self, aSide: Side) -> nscoord {
        let outer = self.mOuterFrame.expect("need a frame, so we can call GetContainingBlock()");

        let Some(container) = outer.GetContainingBlock__uint32_t(0) else {
            return 0;
        };
        let margin = outer.GetUsedMargin();
        let border = container.GetUsedBorder();
        let mut rect = outer.GetRect();
        let mut containerRect = container.GetRect();

        if container.IsGridContainerFrame() && outer.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) {
            containerRect = nsGridContainerFrame::GridItemCB(outer);
            rect.MoveBy__nscoord_nscoord(-containerRect.x, -containerRect.y);
        }

        match aSide {
            eSideTop => rect.y - margin.top - border.top,
            eSideRight => containerRect.width - rect.width - rect.x - margin.right - border.right,
            eSideBottom => containerRect.height - rect.height - rect.y - margin.bottom - border.bottom,
            eSideLeft => rect.x - margin.left - border.left,
        }
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetStaticOffset
    fn GetStaticOffset(&self, aSide: Side) -> String {
        let resolved = self.Inset(aSide);
        if resolved.IsAuto() {
            "auto".to_string()
        } else {
            SetValueToLengthPercentage(resolved.AsLengthPercentage(), false)
        }
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetPaddingWidthFor
    fn GetPaddingWidthFor(&self, aSide: Side) -> String {
        let p = self.mComputedStyle.get_padding();
        let padding = match aSide {
            eSideTop => &p.padding_top.0,
            eSideRight => &p.padding_right.0,
            eSideBottom => &p.padding_bottom.0,
            eSideLeft => &p.padding_left.0,
        };
        match self.mInnerFrame {
            Some(inner) if PaddingNeedsUsedValue(padding) => AppUnitsToCSSValue(inner.GetUsedPadding().Side(aSide)),
            _ => SetValueToLengthPercentage(padding, true),
        }
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetMarginFor
    fn GetMarginFor(&self, aSide: Side) -> String {
        // Use raw margin here, layout-dependent margins should be stored in used
        // margin.
        let m = self.mComputedStyle.get_margin();
        let margin = match aSide {
            eSideTop => m.margin_top.clone(),
            eSideRight => m.margin_right.clone(),
            eSideBottom => m.margin_bottom.clone(),
            eSideLeft => m.margin_left.clone(),
        };
        match self.mOuterFrame {
            Some(outer) if self.mInnerFrame.is_some() && !margin.ConvertsToLength() => {
                // For tables, GetUsedMargin always returns an empty margin, so we
                // should read the margin from the table wrapper frame instead.
                AppUnitsToCSSValue(outer.GetUsedMargin().Side(aSide))
            }
            // SetValueToMargin
            _ if !margin.IsLengthPercentage() => "auto".to_string(),
            _ => SetValueToLengthPercentage(margin.AsLengthPercentage(), false),
        }
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetCBContentWidth and
    // GetCBContentHeight
    fn GetCBContentSize(&self) -> Option<nsSize> {
        let cb = self.mOuterFrame?.GetContainingBlock__uint32_t(0)?;
        Some(cb.GetContentRect().Size())
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetCBPaddingRectWidth and
    // GetCBPaddingRectHeight
    fn GetCBPaddingRectSize(&self) -> Option<nsSize> {
        let cb = self.mOuterFrame?.GetContainingBlock__uint32_t(0)?;
        Some(cb.GetPaddingRectRelativeToSelf().Size())
    }

    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::GetScrollFrameContentWidth and
    // GetScrollFrameContentHeight
    // NOT PORTED: the port has no scroll container frames (GetNearestScrollContainerFrame finds
    // none), so the percentages of a sticky box's offsets resolve to 0.
    fn GetScrollFrameContentSize(&self) -> Option<nsSize> {
        None
    }
}

// Gecko: layout/style/nsComputedDOMStyle.cpp NS_OPPOSITE_SIDE
fn NS_OPPOSITE_SIDE(aSide: Side) -> Side {
    match aSide {
        eSideTop => eSideBottom,
        eSideRight => eSideLeft,
        eSideBottom => eSideTop,
        eSideLeft => eSideRight,
    }
}

/// `getComputedStyle(aElement, aPseudo).getPropertyValue(aPropertyName)` once style and layout are
/// flushed: the value, or "" for a name that is not a property, a node that is not a connected
/// element, or a pseudo-element the element does not have. `aPseudo` is the parsed pseudo-element
/// argument (None for the element itself).
///
/// Gecko: nsComputedDOMStyle::GetPropertyValue(const nsACString&) and UpdateCurrentStyleSources.
pub fn GetComputedPropertyValue(aStyleDom: &StyleDom, aElement: NodeId, aPseudo: Option<mrt_style::style::selector_parser::PseudoElement>, aPropertyName: &str) -> String {
    let Some(prop) = computed::lookup_property(aPropertyName) else {
        return String::new();
    };
    match nsComputedDOMStyle::new(aStyleDom, aElement, aPseudo) {
        Some(style) => style.GetPropertyValue(&prop),
        None => String::new(),
    }
}

impl nsComputedDOMStyle {
    // Gecko: layout/style/nsComputedDOMStyle.cpp nsComputedDOMStyle::UpdateCurrentStyleSources
    // (the flushes are the caller's; the style is the frame's when there is one, else one resolved
    // for the element, DoGetComputedStyleNoFlush)
    fn new(aStyleDom: &StyleDom, aElement: NodeId, aPseudo: Option<mrt_style::style::selector_parser::PseudoElement>) -> Option<nsComputedDOMStyle> {
        let dom = &aStyleDom.tree.dom;
        // We don't return styles for disconnected elements anymore, so don't go
        // through the trouble of flushing or what not.
        if dom.node(aElement).kind != mrt_style::mrt_dom::Kind::Element || !aStyleDom.tree.is_connected(aElement) {
            return None;
        }
        if let Some(pseudo) = aPseudo {
            // ADAPTER: a pseudo-element's style without its frame (see the file's head)
            let style = aStyleDom.pseudo_style_for_query(aElement, pseudo)?;
            return Some(nsComputedDOMStyle { mComputedStyle: style, mOuterFrame: None, mInnerFrame: None });
        }
        let outer = nsIContent::FromNodeId(aElement).GetPrimaryFrame();
        let inner = outer.map(nsLayoutUtils::GetStyleFrame);
        let style = match inner {
            Some(frame) => frame.Style(),
            None => aStyleDom.style_for_query(aElement)?,
        };
        Some(nsComputedDOMStyle { mComputedStyle: style, mOuterFrame: outer, mInnerFrame: inner })
    }
}

/// The names `getComputedStyle(...).length` and `item(i)` list (mrt_style::computed::property_names).
pub fn ComputedPropertyNames() -> &'static [&'static str] {
    computed::property_names()
}
