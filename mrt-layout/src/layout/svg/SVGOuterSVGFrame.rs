// Reduced translation of layout/svg/SVGOuterSVGFrame.h and .cpp (GECKO_REV), with the sizing parts of
// dom/svg/SVGSVGElement.cpp, for an inline or block-level `<svg>` element in an HTML page.
//
// What is here: how the outer <svg> is sized, the part of SVGOuterSVGFrame that matters for the page's
// layout: GetIntrinsicSize, GetIntrinsicRatio, IntrinsicISize and ComputeSize (replaced element sizing
// through nsContainerFrame::ComputeSizeWithIntrinsicDimensions). The element's `width`, `height` and
// `viewBox` attributes are read by the frame constructor once, into SVGOuterSVGData, which stands for the
// parts of SVGSVGElement (mLengthAttributes, GetViewBoxInternal) that these functions read.
//
// ADAPTER: there is no frame class. The frame is a leaf block frame (the stand-in the constructor makes for
// replaced elements, see ConstructReplacedFrameStub) that carries SVGOuterSVGData as a frame property; the
// dispatchers in nsIFrame.rs (ComputeSize, IntrinsicISize, GetIntrinsicRatio) and nsLineLayoutAdapter.rs
// (GetIntrinsicSize) check the property first, where Gecko's virtual call reaches SVGOuterSVGFrame.
//
// NOT PORTED: everything else of SVGOuterSVGFrame: the children (an <svg> has no child frames: its
// descendants are skipped, like the content of an image), Reflow (the leaf block reflows to the size
// ComputeSize gives), painting, hit testing, the zoom and pan, the root-of-image and `object`/`embed` cases
// (IsRootOfImage, mIsInObjectOrEmbed), the `svg:root` case, `contain: size` and `contain-intrinsic-size`
// (ContainSizeAxesIfApplicable, FinishIntrinsicSize), animated attribute values, and the `preserveAspectRatio`
// attribute.
use crate::*;

// ADAPTER: the id of the frame property (this crate's own numbers; 7400..7499 belong to SVG frames).
const kSVGOuterSVGDataPropertyId: u32 = 7400;

pub fn SVGOuterSVGDataProperty() -> FramePropertyDescriptor<SVGOuterSVGData> {
    FramePropertyDescriptor::new(kSVGOuterSVGDataPropertyId)
}

// Gecko: dom/svg/SVGAnimatedLength.h SVGAnimatedLength (the base value only, in CSS px)
// ADAPTER: the length of the `width` or `height` attribute, resolved to CSS px when it is a number or has an
// absolute or font-relative unit, or a percentage. An attribute that is not set is 100% (the default value of
// SVGSVGElement::sLengthInfo) and `mExplicitlySet` is false.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SVGAnimatedLength {
    pub mIsPercentage: bool,
    // CSS px for a length; the percentage (50 for 50%) for a percentage.
    pub mValue: f32,
    pub mExplicitlySet: bool,
}

impl SVGAnimatedLength {
    // Gecko: dom/svg/SVGAnimatedLength.h SVGAnimatedLength::IsPercentage
    pub fn IsPercentage(&self) -> bool {
        self.mIsPercentage
    }

    // Gecko: dom/svg/SVGAnimatedLength.h SVGAnimatedLength::IsExplicitlySet
    pub fn IsExplicitlySet(&self) -> bool {
        self.mExplicitlySet
    }

    // Gecko: dom/svg/SVGAnimatedLength.h SVGAnimatedLength::GetAnimValueWithZoom
    // (no zoom, no animation: the value in CSS px)
    pub fn GetAnimValueWithZoom(&self) -> f32 {
        self.mValue
    }

    // Gecko: dom/svg/SVGAnimatedLength.cpp SVGLength parsing (SVGLength::SetValueFromString), reduced:
    // `<number>` and `<number><unit>` with the units px, em, ex, pt, pc, cm, mm, in and %. Anything else
    // (including a negative value, which is an error for width and height) leaves the default 100%.
    // aFontSizePx resolves em and ex (ex is half an em, as Gecko's fallback does without font metrics).
    pub fn FromAttribute(aValue: Option<&str>, aFontSizePx: f32) -> SVGAnimatedLength {
        let default = SVGAnimatedLength { mIsPercentage: true, mValue: 100.0, mExplicitlySet: false };
        let Some(value) = aValue else {
            return default;
        };
        let value = value.trim();
        let split = value.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')).unwrap_or(value.len());
        // "1e" followed by a unit such as "em" is "1" and "em": back off an exponent letter that starts a unit
        let (number, unit) = value.split_at(split);
        let (number, unit) = if number.ends_with(['e', 'E']) && !unit.is_empty() { value.split_at(split - 1) } else { (number, unit) };
        let Ok(n) = number.parse::<f32>() else {
            return default;
        };
        if !n.is_finite() || n < 0.0 {
            return default;
        }
        let (is_percentage, value) = match unit {
            "" | "px" => (false, n),
            "%" => (true, n),
            "em" => (false, n * aFontSizePx),
            "ex" => (false, n * aFontSizePx * 0.5),
            "pt" => (false, n * 4.0 / 3.0),
            "pc" => (false, n * 16.0),
            "cm" => (false, n * 96.0 / 2.54),
            "mm" => (false, n * 96.0 / 25.4),
            "in" => (false, n * 96.0),
            _ => return default,
        };
        SVGAnimatedLength { mIsPercentage: is_percentage, mValue: value, mExplicitlySet: true }
    }
}

// ADAPTER: what SVGOuterSVGFrame reads from its SVGSVGElement: mLengthAttributes[ATTR_WIDTH] and
// [ATTR_HEIGHT], and the viewBox rectangle (GetViewBoxInternal; only its size matters here).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SVGOuterSVGData {
    pub mWidth: SVGAnimatedLength,
    pub mHeight: SVGAnimatedLength,
    // The width and height of the viewBox, when the attribute is a valid rectangle.
    pub mViewBox: Option<(f32, f32)>,
}

impl SVGOuterSVGData {
    // ADAPTER: made from the `width`, `height` and `viewBox` attributes and the font size of the element.
    pub fn FromAttributes(aWidth: Option<&str>, aHeight: Option<&str>, aViewBox: Option<&str>, aFontSizePx: f32) -> SVGOuterSVGData {
        SVGOuterSVGData {
            mWidth: SVGAnimatedLength::FromAttribute(aWidth, aFontSizePx),
            mHeight: SVGAnimatedLength::FromAttribute(aHeight, aFontSizePx),
            mViewBox: aViewBox.and_then(SVGOuterSVGData::ParseViewBox),
        }
    }

    // Gecko: dom/svg/SVGViewBox.cpp SVGViewBox::FromString (reduced to the size): four numbers separated by
    // white space and/or one comma; a negative width or height is an error.
    fn ParseViewBox(aValue: &str) -> Option<(f32, f32)> {
        let nums: Vec<f32> = aValue.split(|c: char| c == ',' || c.is_ascii_whitespace()).filter(|s| !s.is_empty()).map(|s| s.parse::<f32>()).collect::<Result<_, _>>().ok()?;
        if nums.len() != 4 || !nums.iter().all(|n| n.is_finite()) || nums[2] < 0.0 || nums[3] < 0.0 {
            return None;
        }
        Some((nums[2], nums[3]))
    }

    // Gecko: dom/svg/SVGSVGElement.cpp SVGSVGElement::GetIntrinsicRatio
    pub fn GetIntrinsicRatio(&self) -> AspectRatio {
        // (the `contain: size` check is NOT PORTED)
        // We only have an intrinsic size/ratio if our width and height attributes
        // are both specified and set to non-percentage values, or we have a viewBox
        // rect: https://svgwg.org/svg2-draft/coords.html#SizingSVGInCSS
        if !self.mWidth.IsPercentage() && !self.mHeight.IsPercentage() {
            // Use width/height ratio only if
            // 1. it's not a degenerate ratio, and
            // 2. width and height are non-negative numbers.
            // Otherwise, we use the viewbox rect.
            // https://github.com/w3c/csswg-drafts/issues/6286
            let w = self.mWidth.GetAnimValueWithZoom();
            let h = self.mHeight.GetAnimValueWithZoom();
            if w > 0.0 && h > 0.0 {
                return AspectRatio::FromSize__float_float_UseBoxSizing(w, h, UseBoxSizing::No);
            }
        }

        if let Some((w, h)) = self.mViewBox {
            return AspectRatio::FromSize__float_float_UseBoxSizing(w, h, UseBoxSizing::No);
        }

        AspectRatio::default()
    }
}

// Gecko: layout/generic/nsIFrame.h nsIFrame::GetIntrinsicSize, SVGOuterSVGFrame::GetIntrinsicSize
// ADAPTER: a free function; the frame is the leaf block that carries SVGOuterSVGData.
pub fn SVGOuterSVGFrame_GetIntrinsicSize(aData: &SVGOuterSVGData) -> IntrinsicSize {
    // XXXjwatt Note that here we want to return the CSS width/height if they're
    // specified and we're embedded inside an nsIObjectLoadingContent.

    // (contain: size and contain-intrinsic-size: NOT PORTED)

    let mut intrinsicSize = IntrinsicSize::default();

    if !aData.mWidth.IsPercentage() {
        let val = nsPresContext::CSSPixelsToAppUnits__float(aData.mWidth.GetAnimValueWithZoom());
        intrinsicSize.width = Some(std::cmp::max(val, 0));
    }

    if !aData.mHeight.IsPercentage() {
        let val = nsPresContext::CSSPixelsToAppUnits__float(aData.mHeight.GetAnimValueWithZoom());
        intrinsicSize.height = Some(std::cmp::max(val, 0));
    }

    intrinsicSize
}

// Gecko: layout/svg/SVGOuterSVGFrame.cpp SVGOuterSVGFrame::GetIntrinsicRatio
pub fn SVGOuterSVGFrame_GetIntrinsicRatio(aData: &SVGOuterSVGData) -> AspectRatio {
    // (else SVGDisplayContainerFrame::GetIntrinsicRatio: no ratio)
    aData.GetIntrinsicRatio()
}

// Gecko: layout/svg/SVGOuterSVGFrame.cpp SVGOuterSVGFrame::IntrinsicISize
pub fn SVGOuterSVGFrame_IntrinsicISize(aFrame: nsIFrame, aData: &SVGOuterSVGData, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
    let wm = aFrame.GetWritingMode();
    if aType == IntrinsicISizeType::MinISize {
        let size = SVGOuterSVGFrame_GetIntrinsicSize(aData);
        return (if wm.IsVertical() { size.height } else { size.width }).unwrap_or(0);
    }

    let result: nscoord;
    let isize = if wm.IsVertical() { aData.mHeight } else { aData.mWidth };

    // (ContainIntrinsicISize: NOT PORTED)
    if isize.IsPercentage() {
        // If we are here, our inline size attribute is a percentage either
        // explicitly (via an attribute value) or implicitly (by being unset, which
        // is treated as 100%). The following if-condition, deciding to return
        // either the fallback intrinsic size or zero, is made to match blink and
        // webkit's behavior for webcompat.
        if isize.IsExplicitlySet() || aFrame.StylePosition().ISize(wm, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aFrame, std::ptr::null_mut())).HasPercent() || !aFrame.GetAspectRatio().operator_bool() {
            // kFallbackIntrinsicSize
            result = nsPresContext::CSSPixelsToAppUnits__int32_t(if wm.IsVertical() { 150 } else { 300 });
        } else {
            result = 0;
        }
    } else {
        let r = nsPresContext::CSSPixelsToAppUnits__float(isize.GetAnimValueWithZoom());
        result = if r < 0 { 0 } else { r };
    }

    let _ = aInput;
    result
}

// Gecko: layout/svg/SVGOuterSVGFrame.cpp SVGOuterSVGFrame::ComputeSize
// NOT PORTED: the root-of-image / object / embed case and the zoom of the root element.
pub fn SVGOuterSVGFrame_ComputeSize(
    aFrame: nsIFrame,
    aData: &SVGOuterSVGData,
    aSizingInput: &SizeComputationInput,
    aWritingMode: WritingMode,
    aCBSize: LogicalSize,
    aAvailableISize: nscoord,
    aMargin: LogicalSize,
    aBorderPadding: LogicalSize,
    aSizeOverrides: &StyleSizeOverrides,
    aFlags: ComputeSizeFlags,
) -> nsIFrame_SizeComputationResult {
    let cbSize = aCBSize;
    let intrinsicSize = SVGOuterSVGFrame_GetIntrinsicSize(aData);
    let ratio = aFrame.GetAspectRatio();
    let _ = aAvailableISize;

    nsIFrame_SizeComputationResult {
        mLogicalSize: aFrame.ComputeSizeWithIntrinsicDimensions(aSizingInput.mRenderingContext, aWritingMode, &intrinsicSize, ratio, cbSize, aMargin, aBorderPadding, aSizeOverrides, aFlags),
        mAspectRatioUsage: nsIFrame_AspectRatioUsage::None,
    }
}
