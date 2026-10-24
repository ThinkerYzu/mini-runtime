// Translated from layout/base/nsLayoutUtils.h and nsLayoutUtils.cpp (GECKO_REV), PARTIALLY: only
// the static functions that ReflowInput and nsIFrame::ComputeSize call, and (at the end) the
// transform functions the box reads use (task 33620). Everything else of nsLayoutUtils (display
// lists, scrolling, ...) is NOT PORTED. Each function below is a whole function, in the order of
// nsLayoutUtils.h.
use crate::*;

// Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils (namespace of static functions)
pub struct nsLayoutUtils;

impl nsLayoutUtils {
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::FirstContinuationOrIBSplitSibling
    pub fn FirstContinuationOrIBSplitSibling(aFrame: nsIFrame) -> nsIFrame {
        let mut result = aFrame.FirstContinuation();

        if result.HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT) {
            while let Some(f) = result.GetProperty(nsIFrame::IBSplitPrevSibling()) {
                result = f;
            }
        }

        result
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::LastContinuationOrIBSplitSibling
    pub fn LastContinuationOrIBSplitSibling(aFrame: nsIFrame) -> nsIFrame {
        let mut result = aFrame.FirstContinuation();

        if result.HasAnyStateBits(NS_FRAME_PART_OF_IBSPLIT) {
            while let Some(f) = result.GetProperty(nsIFrame::IBSplitSibling()) {
                result = f;
            }
        }

        result.LastContinuation()
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetParentOrPlaceholderFor
    pub fn GetParentOrPlaceholderFor(aFrame: nsIFrame) -> Option<nsIFrame> {
        // This condition must match the condition in FindContainingBlocks in
        // RetainedDisplayListBuider.cpp, MarkFrameForDisplayIfVisible and
        // UnmarkFrameForDisplayIfVisible in nsDisplayList.cpp
        if aFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) && aFrame.GetPrevInFlow().is_none() {
            return aFrame.GetPlaceholderFrame();
        }
        aFrame.GetParent()
    }
}

impl nsLayoutUtils {
    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::IGNORE_PADDING and BAIL_IF_REFLOW_NEEDED
    // (flags for IntrinsicForAxis)
    /**
     * Ignore padding when computing the intrinsic size.
     */
    pub const IGNORE_PADDING: u32 = 0x01;
    /**
     * Return NS_INTRINSIC_ISIZE_UNKNOWN if a reflow is needed to get the size.
     */
    pub const BAIL_IF_REFLOW_NEEDED: u32 = 0x02;

    /*
     * Whether the frame is an nsBlockFrame which is not a wrapper block.
     */
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::IsNonWrapperBlock
    pub fn IsNonWrapperBlock(aFrame: nsIFrame) -> bool {
        aFrame.IsBlockFrameOrSubclass() && !aFrame.IsBlockWrapper()
    }

    /**
     * Use only for paddings / widths / heights, since it clamps negative calc()
     * to 0.
     */
    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::GetAbsoluteSize<LengthPercentageLike>
    // (the template is used with LengthPercentage, StyleSize and StyleMaxSize; those reach
    // ConvertsToLength() / ToLength() through StyleSizeLike, a bare LengthPercentage has its own)
    pub fn GetAbsoluteSize__SizeOrMaxSize<SizeOrMaxSize: StyleSizeLike>(aSize: &SizeOrMaxSize) -> Option<nscoord> {
        if !aSize.ConvertsToLength() {
            return None;
        }
        Some(std::cmp::max(0, aSize.ToLength()))
    }

    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::GetAbsoluteSize<LengthPercentage>
    pub fn GetAbsoluteSize__LengthPercentage(aSize: &LengthPercentage) -> Option<nscoord> {
        if !aSize.ConvertsToLength() {
            return None;
        }
        Some(std::cmp::max(0, aSize.ToLength()))
    }
}

// Gecko: layout/base/nsLayoutUtils.cpp GetPercentBSize<SizeOrMaxSize> (static)
//
// Only call on aSize for which GetAbsoluteSize returned Nothing().
//
// Bug 1363918: We can remove GetPercentBSize() after we've updated all of
// IntrinsicForAxis()'s callsites to pass it a percentage basis.
fn GetPercentBSize__SizeOrMaxSize_nsIFrame_bool<SizeOrMaxSize: StyleSizeLike>(aSize: &SizeOrMaxSize, aFrame: nsIFrame, aHorizontalAxis: bool) -> Option<nscoord> {
    if !aSize.IsLengthPercentage() {
        return None;
    }
    GetPercentBSize__LengthPercentage_nsIFrame_bool(aSize.AsLengthPercentage(), aFrame, aHorizontalAxis)
}

// Gecko: layout/base/nsLayoutUtils.cpp GetPercentBSize(const LengthPercentage&, ...) (static)
//
// Only call on aSize for which GetAbsoluteSize returned Nothing().
fn GetPercentBSize__LengthPercentage_nsIFrame_bool(aSize: &LengthPercentage, aFrame: nsIFrame, aHorizontalAxis: bool) -> Option<nscoord> {
    if !aSize.HasPercent() {
        return None;
    }

    MOZ_ASSERT!(!aSize.ConvertsToLength(), "GetAbsoluteSize should have handled this");

    // During reflow, ScrollContainerFrame::ReflowScrolledFrame uses
    // SetComputedHeight on the reflow input for its child to propagate its
    // computed height to the scrolled content. So here we skip to the scroll
    // frame that contains this scrolled content in order to get the same
    // behavior as layout when computing percentage heights.
    let f = aFrame.GetContainingBlock__uint32_t(SKIP_SCROLLED_FRAME);
    let f = match f {
        Some(f) => f,
        None => {
            MOZ_ASSERT_UNREACHABLE!("top of frame tree not a containing block");
            return None;
        }
    };

    // Helper to compute the block-size, max-block-size, and min-block-size later
    // in this function.
    // (a C++ generic lambda: a nested generic fn that takes its captures as parameters)
    fn GetBSize<SizeOrMaxSize: StyleSizeLike>(aSize: &SizeOrMaxSize, f: nsIFrame, aHorizontalAxis: bool) -> Option<nscoord> {
        nsLayoutUtils::GetAbsoluteSize__SizeOrMaxSize(aSize).or_else(|| GetPercentBSize__SizeOrMaxSize_nsIFrame_bool(aSize, f, aHorizontalAxis))
    }

    let wm = f.GetWritingMode();
    let pos = f.StylePosition();
    let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(f, std::ptr::null_mut());
    let mut bSize = GetBSize(&pos.BSize(wm, &anchorResolutionParams), f, aHorizontalAxis);
    if bSize.is_none() {
        let fType = f.Type();
        if fType != LayoutFrameType::Viewport && fType != LayoutFrameType::Canvas && fType != LayoutFrameType::PageContent {
            // There's no basis for the percentage height, so it acts like auto.
            // Should we consider a max-height < min-height pair a basis for
            // percentage heights?  The spec is somewhat unclear, and not doing
            // so is simpler and avoids troubling discontinuities in behavior,
            // so I'll choose not to. -LDB
            return None;
        }
        // For the viewport, canvas, and page-content kids, the percentage
        // basis is just the parent block-size.
        bSize = Some(f.BSize(wm));
        if bSize == Some(NS_UNCONSTRAINEDSIZE) {
            // We don't have a percentage basis after all
            return None;
        }
    }
    let mut bSize = bSize.unwrap();

    if let Some(maxBSize) = GetBSize(&pos.MaxBSize(wm, &anchorResolutionParams), f, aHorizontalAxis) {
        if maxBSize < bSize {
            bSize = maxBSize;
        }
    }

    if let Some(minBSize) = GetBSize(&pos.MinBSize(wm, &anchorResolutionParams), f, aHorizontalAxis) {
        if minBSize > bSize {
            bSize = minBSize;
        }
    }

    // If we're an abspos box, percentages in that case resolve against the
    // padding box.
    //
    // TODO: This could conceivably cause some problems with fieldsets (which are
    // the other place that wants to ignore padding), but solving that here
    // without hardcoding a check for f being a fieldset-content frame is a bit of
    // a pain.
    let resolvesAgainstPaddingBox = aFrame.IsAbsolutelyPositioned(None);
    bSize += GetBSizePercentBasisAdjustment(pos.mBoxSizing(), f, aHorizontalAxis, resolvesAgainstPaddingBox);

    bSize = std::cmp::max(bSize, 0);
    Some(std::cmp::max(aSize.Resolve(bSize), 0))
}

// If aSize can be resolved to a definite value, returns it; otherwise returns
// Nothing().
// Gecko: layout/base/nsLayoutUtils.cpp GetDefiniteSize(const LengthPercentage&, ...) (static)
fn GetDefiniteSize__LengthPercentage_nsIFrame_bool_Maybe(aSize: &LengthPercentage, aFrame: nsIFrame, aIsInlineAxis: bool, aPercentageBasis: &Option<LogicalSize>) -> Option<nscoord> {
    if aSize.ConvertsToLength() {
        return Some(aSize.ToLength());
    }

    if aPercentageBasis.is_none() {
        return None;
    }

    let wm = aFrame.GetWritingMode();
    let pb = if aIsInlineAxis { aPercentageBasis.unwrap().ISize(wm) } else { aPercentageBasis.unwrap().BSize(wm) };
    if pb == NS_UNCONSTRAINEDSIZE {
        return None;
    }
    Some(std::cmp::max(0, aSize.Resolve(pb)))
}

// If aSize can be resolved to a definite value, returns it; otherwise returns
// Nothing().
// Gecko: layout/base/nsLayoutUtils.cpp GetDefiniteSize<SizeOrMaxSize> (static)
fn GetDefiniteSize__SizeOrMaxSize_nsIFrame_bool_Maybe<SizeOrMaxSize: StyleSizeLike>(aSize: &SizeOrMaxSize, aFrame: nsIFrame, aIsInlineAxis: bool, aPercentageBasis: &Option<LogicalSize>) -> Option<nscoord> {
    if !aSize.IsLengthPercentage() {
        return None;
    }
    GetDefiniteSize__LengthPercentage_nsIFrame_bool_Maybe(aSize.AsLengthPercentage(), aFrame, aIsInlineAxis, aPercentageBasis)
}

// NOTE: this function will be replaced by GetDefiniteSizeTakenByBoxSizing (bug
// 1363918). Please do not add new uses of this function.
//
// Get the amount of space to add or subtract out of aFrame's 'block-size' or
// property value due its borders and paddings, given the box-sizing value in
// aBoxSizing.
//
// aHorizontalAxis is true if our inline direction is horizontal and our block
// direction is vertical. aResolvesAgainstPaddingBox is true if padding should
// be added or not removed.
// Gecko: layout/base/nsLayoutUtils.cpp GetBSizePercentBasisAdjustment (static)
fn GetBSizePercentBasisAdjustment(aBoxSizing: StyleBoxSizing, aFrame: nsIFrame, aHorizontalAxis: bool, aResolvesAgainstPaddingBox: bool) -> nscoord {
    let mut adjustment: nscoord = 0;
    if aBoxSizing == StyleBoxSizing::BorderBox {
        let border = aFrame.StyleBorder().GetComputedBorder();
        adjustment -= if aHorizontalAxis { border.TopBottom() } else { border.LeftRight() };
    }
    if (aBoxSizing == StyleBoxSizing::BorderBox) == !aResolvesAgainstPaddingBox {
        let stylePadding = aFrame.StylePadding().mPadding();
        let paddingStart = stylePadding.Get__Side(if aHorizontalAxis { eSideTop } else { eSideLeft });
        let paddingEnd = stylePadding.Get__Side(if aHorizontalAxis { eSideBottom } else { eSideRight });

        // XXXbz Calling GetPercentBSize on padding values looks bogus, since
        // percent padding is always a percentage of the inline-size of the
        // containing block.  We should perhaps just treat non-absolute paddings
        // here as 0 instead, except that in some cases the width may in fact be
        // known.  See bug 1231059.
        let GetPadding = |aPadding: &LengthPercentage| -> Option<nscoord> {
            nsLayoutUtils::GetAbsoluteSize__LengthPercentage(aPadding).or_else(|| GetPercentBSize__LengthPercentage_nsIFrame_bool(aPadding, aFrame, aHorizontalAxis))
        };
        if let Some(pad) = GetPadding(paddingStart) {
            adjustment += if aResolvesAgainstPaddingBox { pad } else { -pad };
        }
        if let Some(pad) = GetPadding(paddingEnd) {
            adjustment += if aResolvesAgainstPaddingBox { pad } else { -pad };
        }
    }
    adjustment
}

// Get the amount of space taken out of aFrame's content area due to its
// borders and paddings given the box-sizing value in aBoxSizing.  We don't
// get aBoxSizing from the frame because some callers want to compute this for
// specific box-sizing values.
// aIsInlineAxis is true if we're computing for aFrame's inline axis.
// aIgnorePadding is true if padding should be ignored.
// Gecko: layout/base/nsLayoutUtils.cpp GetDefiniteSizeTakenByBoxSizing (static)
fn GetDefiniteSizeTakenByBoxSizing(aBoxSizing: StyleBoxSizing, aFrame: nsIFrame, aIsInlineAxis: bool, aIgnorePadding: bool, aPercentageBasis: &Option<LogicalSize>) -> nscoord {
    let mut sizeTakenByBoxSizing: nscoord = 0;
    if aBoxSizing == StyleBoxSizing::BorderBox {
        let isHorizontalAxis = aIsInlineAxis == !aFrame.GetWritingMode().IsVertical();
        let styleBorder = aFrame.StyleBorder();
        sizeTakenByBoxSizing = if isHorizontalAxis { styleBorder.GetComputedBorder().LeftRight() } else { styleBorder.GetComputedBorder().TopBottom() };
        if !aIgnorePadding {
            let stylePadding = aFrame.StylePadding().mPadding();
            let pStart = stylePadding.Get__Side(if isHorizontalAxis { eSideLeft } else { eSideTop });
            let pEnd = stylePadding.Get__Side(if isHorizontalAxis { eSideRight } else { eSideBottom });

            // XXXbz Calling GetPercentBSize on padding values looks bogus, since
            // percent padding is always a percentage of the inline-size of the
            // containing block.  We should perhaps just treat non-absolute paddings
            // here as 0 instead, except that in some cases the width may in fact be
            // known.  See bug 1231059.
            let GetPadding = |aPadding: &LengthPercentage| -> Option<nscoord> {
                if let Some(padding) = GetDefiniteSize__LengthPercentage_nsIFrame_bool_Maybe(aPadding, aFrame, aIsInlineAxis, aPercentageBasis) {
                    return Some(padding);
                }
                if aPercentageBasis.is_some() {
                    return None;
                }
                GetPercentBSize__LengthPercentage_nsIFrame_bool(aPadding, aFrame, isHorizontalAxis)
            };
            if let Some(pad) = GetPadding(pStart) {
                sizeTakenByBoxSizing += pad;
            }
            if let Some(pad) = GetPadding(pEnd) {
                sizeTakenByBoxSizing += pad;
            }
        }
    }
    sizeTakenByBoxSizing
}

/**
 * Handles only max-content and min-content, and
 * -moz-fit-content for min-width and max-width, since the others
 * (-moz-fit-content for width, and -moz-available) have no effect on
 * intrinsic widths.
 */
// Gecko: layout/base/nsLayoutUtils.cpp GetIntrinsicSize(ExtremumLength, ...) (static)
fn GetIntrinsicSize__ExtremumLength_gfxContext_nsIFrame_Maybe_SizeProperty_nscoord(mut aLength: nsIFrame_ExtremumLength, aRenderingContext: Option<gfxContext>, aFrame: nsIFrame, aISizeFromAspectRatio: Option<nscoord>, aProperty: nsIFrame_SizeProperty, aContentBoxToBoxSizingDiff: nscoord) -> Option<nscoord> {
    if aLength == nsIFrame_ExtremumLength::MozAvailable || aLength == nsIFrame_ExtremumLength::Stretch {
        return None;
    }

    if aLength == nsIFrame_ExtremumLength::FitContentFunction {
        // fit-content() should be handled by the caller.
        return None;
    }

    if aLength == nsIFrame_ExtremumLength::FitContent {
        match aProperty {
            nsIFrame_SizeProperty::Size => {
                // handle like 'width: auto'
                return None;
            }
            nsIFrame_SizeProperty::MaxSize => {
                // constrain large 'width' values down to max-content
                aLength = nsIFrame_ExtremumLength::MaxContent;
            }
            nsIFrame_SizeProperty::MinSize => {
                // constrain small 'width' or 'max-width' values up to min-content
                aLength = nsIFrame_ExtremumLength::MinContent;
            }
        }
    }

    NS_ASSERTION!(aLength == nsIFrame_ExtremumLength::MinContent || aLength == nsIFrame_ExtremumLength::MaxContent, "should have reduced everything remaining to one of these");

    // NOT PORTED: AutoMaybeDisableFontInflation (font size inflation is off on desktop).

    let mut result: nscoord;
    if let Some(iSize) = aISizeFromAspectRatio {
        result = iSize;
    } else {
        // Bug 1363918: We need to refactor this function to compute a percentage
        // basis when computing intrinsic sizes.
        let input = IntrinsicSizeInput::new(aRenderingContext, None, None);
        let type_ = if aLength == nsIFrame_ExtremumLength::MaxContent { IntrinsicISizeType::PrefISize } else { IntrinsicISizeType::MinISize };
        result = aFrame.IntrinsicISize(&input, type_);
    }

    result += aContentBoxToBoxSizingDiff;
    Some(result)
}

// Gecko: layout/base/nsLayoutUtils.cpp GetIntrinsicSize<SizeOrMaxSize> (static)
fn GetIntrinsicSize__SizeOrMaxSize_gfxContext_nsIFrame_Maybe_SizeProperty_nscoord<SizeOrMaxSize: StyleSizeLike>(aSize: &SizeOrMaxSize, aRenderingContext: Option<gfxContext>, aFrame: nsIFrame, aISizeFromAspectRatio: Option<nscoord>, aProperty: nsIFrame_SizeProperty, aContentBoxToBoxSizingDiff: nscoord) -> Option<nscoord> {
    let length = nsIFrame::ToExtremumLength(aSize)?;
    GetIntrinsicSize__ExtremumLength_gfxContext_nsIFrame_Maybe_SizeProperty_nscoord(length, aRenderingContext, aFrame, aISizeFromAspectRatio, aProperty, aContentBoxToBoxSizingDiff)
}

// Gecko: layout/base/nsLayoutUtils.cpp GetFitContentSizeForMaxOrPreferredSize (static)
fn GetFitContentSizeForMaxOrPreferredSize(aType: IntrinsicISizeType, aProperty: nsIFrame_SizeProperty, aFrame: nsIFrame, aStyleSize: &LengthPercentage, aInitialValue: nscoord, aMinContentSize: nscoord, aMaxContentSize: nscoord) -> nscoord {
    MOZ_ASSERT!(aProperty != nsIFrame_SizeProperty::MinSize);

    let size: nscoord;
    // 1. Treat fit-content()'s arg as a plain LengthPercentage
    // However, we have to handle the cyclic percentage contribution first.
    //
    // https://drafts.csswg.org/css-sizing-3/#cyclic-percentage-contribution
    if aType == IntrinsicISizeType::MinISize && aFrame.IsPercentageResolvedAgainstZero__LengthPercentage_SizeProperty(aStyleSize, aProperty) {
        // Case (c) in the spec.
        // FIXME: This doesn't follow the spec for calc(). We should fix this in
        // Bug 1463700.
        size = 0;
    } else if let Some(length) = nsLayoutUtils::GetAbsoluteSize__LengthPercentage(aStyleSize) {
        size = length;
    } else {
        // As initial value. Case (a) and (b) in the spec.
        size = aInitialValue;
    }

    // 2. Clamp size by min-content and max-content.
    // (std::clamp panics when min > max; Gecko's std::clamp has undefined behavior then, so
    // use the same order: lower bound first.)
    if size < aMinContentSize {
        aMinContentSize
    } else if aMaxContentSize < size {
        aMaxContentSize
    } else {
        size
    }
}

/*
 * Add aOffsets which describes what to add on outside of the content box
 * aContentSize (controlled by 'box-sizing') and apply min/max properties.
 * We have to account for these properties after getting all the offsets
 * (margin, border, padding) because percentages do not operate linearly.
 * Doing this is ok because although percentages aren't handled linearly,
 * they are handled monotonically.
 *
 * @param aContentSize the content size calculated so far
                       (@see IntrinsicForAxis)
 * @param aStyleSize a 'width' or 'height' property value
 * @param aFixedMinSize if aStyleMinSize is a definite size then this contains
 *                      the value, otherwise Nothing()
 * @param aStyleMinSize a 'min-width' or 'min-height' property value
 * @param aFixedMaxSize if aStyleMaxSize is a definite size then this contains
 *                      the value, otherwise Nothing()
 * @param aStyleMaxSize a 'max-width' or 'max-height' property value
 * @param aISizeFromAspectRatio the content-box inline size computed from
 *                              aspect-ratio and the definite block size.
 *                              We use this value to resolve sizes in inline
 *                              axis with intrinsic keyword.
 * @param aFlags same as for IntrinsicForAxis
 */
// Gecko: layout/base/nsLayoutUtils.cpp AddIntrinsicSizeOffset (static)
fn AddIntrinsicSizeOffset(
    aRenderingContext: Option<gfxContext>,
    aFrame: nsIFrame,
    aOffsets: &nsIFrame_IntrinsicSizeOffsetData,
    aType: IntrinsicISizeType,
    aBoxSizing: StyleBoxSizing,
    aContentSize: nscoord,
    aStyleSize: &StyleSize,
    aFixedMinSize: Option<nscoord>,
    aStyleMinSize: &StyleSize,
    aFixedMaxSize: Option<nscoord>,
    aStyleMaxSize: &StyleMaxSize,
    aISizeFromAspectRatio: Option<nscoord>,
    aFlags: u32,
    aAxis: PhysicalAxis,
) -> nscoord {
    let padding = if aFlags & nsLayoutUtils::IGNORE_PADDING != 0 { 0 } else { aOffsets.padding };
    let contentBoxToBoxSizingDiff: nscoord;
    let boxSizingToMarginDiff: nscoord;

    // Note: |result| can be either the border-box size or the content-box size,
    // depending on the value of aBoxSizing.
    let mut result: nscoord;
    if aBoxSizing == StyleBoxSizing::BorderBox {
        contentBoxToBoxSizingDiff = padding + aOffsets.border;
        boxSizingToMarginDiff = aOffsets.margin;
        result = NSCoordSaturatingAdd(aContentSize, contentBoxToBoxSizingDiff);
    } else {
        MOZ_ASSERT!(aBoxSizing == StyleBoxSizing::ContentBox);
        contentBoxToBoxSizingDiff = 0;
        boxSizingToMarginDiff = padding + aOffsets.border + aOffsets.margin;
        result = aContentSize;
    }

    // Compute min-content/max-content for fit-content().
    let mut minContent: nscoord = 0;
    let mut maxContent: nscoord = NS_UNCONSTRAINEDSIZE;
    if aStyleSize.IsFitContentFunction() || aStyleMaxSize.IsFitContentFunction() || aStyleMinSize.IsFitContentFunction() {
        if let Some(iSize) = aISizeFromAspectRatio {
            minContent = iSize;
            maxContent = iSize;
        } else {
            // Bug 1363918: We need to refactor this function to compute a percentage
            // basis when computing intrinsic sizes.
            let input = IntrinsicSizeInput::new(aRenderingContext, None, None);
            minContent = aFrame.GetMinISize(&input);
            maxContent = aFrame.GetPrefISize(&input);
        }
        minContent += contentBoxToBoxSizingDiff;
        maxContent += contentBoxToBoxSizingDiff;
    }

    // Compute size.
    let isInlineAxis = aAxis == aFrame.GetWritingMode().PhysicalAxis(LogicalAxis::Inline);
    if aType == IntrinsicISizeType::MinISize && isInlineAxis && aFrame.IsPercentageResolvedAgainstZero__StyleSize_StyleMaxSize(aStyleSize, aStyleMaxSize) {
        // Apply the compressible min-content contribution rule only in aFrame's
        // *inline* axis, to maintain web-compatibility with Chrome and Safari.
        // https://drafts.csswg.org/css-sizing-3/#min-content-zero
        // XXX bug 1463700: this doesn't handle calc() according to spec
        result = 0;
    } else if let Some(size) = nsLayoutUtils::GetAbsoluteSize__SizeOrMaxSize(aStyleSize).or_else(|| {
        GetIntrinsicSize__SizeOrMaxSize_gfxContext_nsIFrame_Maybe_SizeProperty_nscoord(aStyleSize, aRenderingContext, aFrame, aISizeFromAspectRatio, nsIFrame_SizeProperty::Size, contentBoxToBoxSizingDiff)
    }) {
        result = size + boxSizingToMarginDiff;
    } else if aStyleSize.IsFitContentFunction() {
        // |result| here is the content size or border size, depends on
        // StyleBoxSizing. We use it as the initial value when handling the cyclic
        // percentage.
        let initial = result;
        let fitContentFuncSize = GetFitContentSizeForMaxOrPreferredSize(aType, nsIFrame_SizeProperty::Size, aFrame, aStyleSize.AsFitContentFunction(), initial, minContent, maxContent);
        // Add border and padding.
        result = NSCoordSaturatingAdd(fitContentFuncSize, boxSizingToMarginDiff);
    } else {
        result = NSCoordSaturatingAdd(result, boxSizingToMarginDiff);
    }

    // Compute max-size.
    let mut maxSize: Option<nscoord> = aFixedMaxSize.or_else(|| {
        GetIntrinsicSize__SizeOrMaxSize_gfxContext_nsIFrame_Maybe_SizeProperty_nscoord(aStyleMaxSize, aRenderingContext, aFrame, aISizeFromAspectRatio, nsIFrame_SizeProperty::MaxSize, contentBoxToBoxSizingDiff)
    });
    if let Some(maxSize) = maxSize.as_mut() {
        *maxSize += boxSizingToMarginDiff;
        if result > *maxSize {
            result = *maxSize;
        }
    } else if aStyleMaxSize.IsFitContentFunction() {
        let fitContentFuncSize = GetFitContentSizeForMaxOrPreferredSize(aType, nsIFrame_SizeProperty::MaxSize, aFrame, aStyleMaxSize.AsFitContentFunction(), NS_UNCONSTRAINEDSIZE, minContent, maxContent);
        let maxSize = NSCoordSaturatingAdd(fitContentFuncSize, boxSizingToMarginDiff);
        if result > maxSize {
            result = maxSize;
        }
    }

    // Compute min-size.
    let mut minSize: Option<nscoord> = aFixedMinSize.or_else(|| {
        GetIntrinsicSize__SizeOrMaxSize_gfxContext_nsIFrame_Maybe_SizeProperty_nscoord(aStyleMinSize, aRenderingContext, aFrame, aISizeFromAspectRatio, nsIFrame_SizeProperty::MinSize, contentBoxToBoxSizingDiff)
    });
    if let Some(minSize) = minSize.as_mut() {
        *minSize += boxSizingToMarginDiff;
        if result < *minSize {
            result = *minSize;
        }
    } else if aStyleMinSize.IsFitContentFunction() {
        let minSize = nsLayoutUtils::GetAbsoluteSize__LengthPercentage(aStyleMinSize.AsFitContentFunction());
        // FIXME: Bug 1463700, we should resolve only the percentage part to 0
        // such as min-width: fit-content(calc(50% + 50px)).
        let minSize = minSize.unwrap_or(0);
        let fitContentFuncSize = CSSMinMax(minSize, minContent, maxContent);
        let minSize = NSCoordSaturatingAdd(fitContentFuncSize, boxSizingToMarginDiff);
        if result < minSize {
            result = minSize;
        }
    }

    let borderPaddingMargin = contentBoxToBoxSizingDiff + boxSizingToMarginDiff;
    result = std::cmp::max(result, borderPaddingMargin);

    // NOT PORTED: native theming (aFrame->IsThemed(disp): the minimum widget size of nsITheme
    // raises result); frames are never themed here.
    result
}

// Gecko: layout/base/nsLayoutUtils.cpp AddStateBitToAncestors (static)
fn AddStateBitToAncestors(aFrame: nsIFrame, aBit: nsFrameState) {
    let mut f = Some(aFrame);
    while let Some(fr) = f {
        if fr.HasAnyStateBits(aBit) {
            break;
        }
        fr.AddStateBits(aBit);
        f = fr.GetParent();
    }
}

// Gecko: layout/base/nsLayoutUtils.cpp MeasureIntrinsicContentSize (static)
// NOT PORTED: measuring the block-size of an orthogonal flow needs a dummy parent ReflowInput and
// a Reflow of the frame (nsLayoutUtils.cpp lines 4580-4620 at GECKO_REV); orthogonal writing
// modes are not supported yet.
fn MeasureIntrinsicContentSize(aContext: Option<gfxContext>, aFrame: nsIFrame, aPercentageBasis: &Option<LogicalSize>) -> nsSize {
    unimplemented!("NOT PORTED: nsLayoutUtils.cpp MeasureIntrinsicContentSize (orthogonal flows)")
}

impl nsLayoutUtils {
    /**
     * Get the contribution of aFrame to its containing block's intrinsic
     * size for the given physical axis.  This considers the child's intrinsic
     * width, its 'width', 'min-width', and 'max-width' properties (or 'height'
     * variations if that's what matches aAxis) and its padding, border and margin
     * in the corresponding dimension.
     * (see nsLayoutUtils.h for the parameters)
     */
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::IntrinsicForAxis
    // (aPercentageBasis is a Maybe<LogicalSize>, aSizeOverrides a StyleSizeOverrides; the
    // flags IGNORE_PADDING = 0x01 and BAIL_IF_REFLOW_NEEDED = 0x02.)
    pub fn IntrinsicForAxis(aAxis: PhysicalAxis, aRenderingContext: Option<gfxContext>, aFrame: nsIFrame, aType: IntrinsicISizeType, aPercentageBasis: Option<LogicalSize>, aFlags: u32, aMarginBoxMinSizeClamp: nscoord, aSizeOverrides: &StyleSizeOverrides) -> nscoord {
        MOZ_ASSERT!(aFrame.GetParent().is_some(), "IntrinsicForAxis called on frame not in tree");
        MOZ_ASSERT!(aFrame.GetParent().unwrap().Type() != LayoutFrameType::GridContainer || aPercentageBasis.is_some(), "grid layout should always pass a percentage basis");

        let horizontalAxis = aAxis == PhysicalAxis::Horizontal;

        // NOT PORTED: AutoMaybeDisableFontInflation (font size inflation is off on desktop).

        // We want the size this frame will contribute to the parent's inline-size,
        // so we work in the parent's writing mode; but if aFrame is orthogonal to
        // its parent, we'll need to look at its BSize instead of min/pref-ISize.
        let stylePos = aFrame.StylePosition();
        let boxSizing = stylePos.mBoxSizing();
        let ourInlineAxis = aFrame.GetWritingMode().PhysicalAxis(LogicalAxis::Inline);
        let isInlineAxis = aAxis == ourInlineAxis;

        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aFrame, std::ptr::null_mut());
        let mut styleMinISize = if horizontalAxis { stylePos.GetMinWidth(&anchorResolutionParams) } else { stylePos.GetMinHeight(&anchorResolutionParams) };
        let styleISizeOverride = if isInlineAxis { &aSizeOverrides.mStyleISize } else { &aSizeOverrides.mStyleBSize };
        let mut styleISize = match styleISizeOverride {
            Some(o) => AnchorResolvedSizeHelper::Overridden(o),
            None => {
                if horizontalAxis {
                    stylePos.GetWidth(&anchorResolutionParams)
                } else {
                    stylePos.GetHeight(&anchorResolutionParams)
                }
            }
        };
        let mut styleMaxISize = if horizontalAxis { stylePos.GetMaxWidth(&anchorResolutionParams) } else { stylePos.GetMaxHeight(&anchorResolutionParams) };

        let ResetIfKeywords = |aSize: &mut StyleSize, aMinSize: &mut StyleSize, aMaxSize: &mut StyleMaxSize| {
            if !aSize.IsLengthPercentage() {
                *aSize = AnchorResolvedSizeHelper::Auto();
            }
            if !aMinSize.IsLengthPercentage() {
                *aMinSize = AnchorResolvedSizeHelper::Auto();
            }
            if !aMaxSize.IsLengthPercentage() {
                *aMaxSize = AnchorResolvedMaxSizeHelper::None();
            }
        };
        // According to the spec, max-content and min-content should behave as the
        // property's initial values in block axis.
        // It also make senses to use the initial values for -moz-fit-content and
        // -moz-available for intrinsic size in block axis. Therefore, we reset them
        // if needed.
        if !isInlineAxis {
            ResetIfKeywords(&mut styleISize, &mut styleMinISize, &mut styleMaxISize);
        }

        // We build up the content box size, storing in |result|, and then
        // adding padding, border and margin in AddIntrinsicSizeOffset().
        let mut result: nscoord = 0;

        let fixedMaxISize = nsLayoutUtils::GetAbsoluteSize__SizeOrMaxSize(&styleMaxISize);
        let fixedMinISize: Option<nscoord>;

        // Treat "min-width: auto" as 0.
        if styleMinISize.IsAuto() {
            // NOTE: Technically, "auto" is supposed to behave like "min-content" on
            // flex items. However, we don't need to worry about that here, because
            // flex items' min-sizes are intentionally ignored until the flex
            // container explicitly considers them during space distribution.
            fixedMinISize = Some(0);
        } else {
            fixedMinISize = nsLayoutUtils::GetAbsoluteSize__SizeOrMaxSize(&styleMinISize);
        }

        // Handle elements with an intrinsic ratio (or size) and a specified
        // height, min-height, or max-height.
        // NOTE:
        // 1. We treat "min-height:auto" as "0" for the purpose of this code,
        // since that's what it means in all cases except for on flex items -- and
        // even there, we're supposed to ignore it (i.e. treat it as 0) until the
        // flex container explicitly considers it.
        // 2. The 'B' in |styleBSize|, |styleMinBSize|, and |styleMaxBSize|
        // represents the ratio-determining axis of |aFrame|. It could be the inline
        // axis or the block axis of |aFrame|. (So we are calculating the size
        // along the ratio-dependent axis in this if-branch.)
        let styleBSizeOverride = if isInlineAxis { &aSizeOverrides.mStyleBSize } else { &aSizeOverrides.mStyleISize };
        let mut styleBSize = match styleBSizeOverride {
            Some(o) => AnchorResolvedSizeHelper::Overridden(o),
            None => {
                if horizontalAxis {
                    stylePos.GetHeight(&anchorResolutionParams)
                } else {
                    stylePos.GetWidth(&anchorResolutionParams)
                }
            }
        };
        let mut styleMinBSize = if horizontalAxis { stylePos.GetMinHeight(&anchorResolutionParams) } else { stylePos.GetMinWidth(&anchorResolutionParams) };
        let mut styleMaxBSize = if horizontalAxis { stylePos.GetMaxHeight(&anchorResolutionParams) } else { stylePos.GetMaxWidth(&anchorResolutionParams) };

        // According to the spec, max-content and min-content should behave as the
        // property's initial values in block axis.
        // It also make senses to use the initial values for -moz-fit-content and
        // -moz-available for intrinsic size in block axis. Therefore, we reset them
        // if needed.
        if isInlineAxis {
            ResetIfKeywords(&mut styleBSize, &mut styleMinBSize, &mut styleMaxBSize);
        }

        let childWM = aFrame.GetWritingMode();
        let mut pmPercentageBasis: nscoord = NS_UNCONSTRAINEDSIZE;
        if let Some(pb) = aPercentageBasis {
            // The padding/margin percentage basis is the inline-size in the parent's
            // writing-mode.
            pmPercentageBasis = if aFrame.GetParent().unwrap().GetWritingMode().IsOrthogonalTo(&childWM) { pb.BSize(childWM) } else { pb.ISize(childWM) };
        }
        let offsetInRequestedAxis = if isInlineAxis { aFrame.IntrinsicISizeOffsets(pmPercentageBasis) } else { aFrame.IntrinsicBSizeOffsets(pmPercentageBasis) };

        let GetContentEdgeToBoxSizing = |aBoxSizing: StyleBoxSizing| -> LogicalSize {
            if aBoxSizing == StyleBoxSizing::ContentBox {
                return LogicalSize::new__WritingMode(childWM);
            }
            let offsetInOtherAxis = if isInlineAxis { aFrame.IntrinsicBSizeOffsets(pmPercentageBasis) } else { aFrame.IntrinsicISizeOffsets(pmPercentageBasis) };
            let inlineOffset = if isInlineAxis { &offsetInRequestedAxis } else { &offsetInOtherAxis };
            let blockOffset = if isInlineAxis { &offsetInOtherAxis } else { &offsetInRequestedAxis };
            LogicalSize::new(childWM, inlineOffset.BorderPadding(), blockOffset.BorderPadding())
        };

        // Helper to compute the block-size, max-block-size, and min-block-size later
        // in this function.
        // (a C++ generic lambda: a nested generic fn that takes its captures as parameters)
        fn GetBSize<SizeOrMaxSize: StyleSizeLike>(aSize: &SizeOrMaxSize, aFrame: nsIFrame, isInlineAxis: bool, aPercentageBasis: &Option<LogicalSize>, horizontalAxis: bool) -> Option<nscoord> {
            if let Some(bSize) = GetDefiniteSize__SizeOrMaxSize_nsIFrame_bool_Maybe(aSize, aFrame, !isInlineAxis, aPercentageBasis) {
                return Some(bSize);
            }
            if aPercentageBasis.is_some() {
                return None;
            }
            // Find percentage basis, then compute.
            GetPercentBSize__SizeOrMaxSize_nsIFrame_bool(aSize, aFrame, horizontalAxis)
        }

        let mut iSizeFromAspectRatio: Option<nscoord> = None;
        let mut contentEdgeToBoxSizing: Option<LogicalSize> = None;

        let ignorePadding = (aFlags & nsLayoutUtils::IGNORE_PADDING) != 0 || aFrame.IsAbsolutelyPositioned(None);

        // If we have a specified width (or a specified 'min-width' greater
        // than the specified 'max-width', which works out to the same thing),
        // don't even bother getting the frame's intrinsic width, because in
        // this case GetAbsoluteSize(styleISize) will always succeed, so
        // we'll never need the intrinsic dimensions.
        if !styleISize.ConvertsToLength()
            && !styleISize.IsMinContent()
            && !styleISize.IsMaxContent()
            && !(styleISize.IsFitContentFunction() && styleISize.AsFitContentFunction().ConvertsToLength())
            && !(fixedMaxISize.is_some() && fixedMinISize.is_some() && fixedMaxISize.unwrap() <= fixedMinISize.unwrap())
        {
            if !isInlineAxis {
                let intrinsicSize = aFrame.GetIntrinsicSize();
                let intrinsicBSize = if horizontalAxis { intrinsicSize.width } else { intrinsicSize.height };
                if let Some(b) = intrinsicBSize {
                    result = b;
                } else {
                    // We don't have an intrinsic bsize and we need aFrame's block-dir size.
                    if aFlags & nsLayoutUtils::BAIL_IF_REFLOW_NEEDED != 0 {
                        return NS_INTRINSIC_ISIZE_UNKNOWN;
                    }
                    let size = MeasureIntrinsicContentSize(aRenderingContext, aFrame, &aPercentageBasis);
                    result = if horizontalAxis { size.Width() } else { size.Height() };
                }
            } else {
                // To resolve aFrame's intrinsic inline size, we first check if we can
                // resolve a block-axis percentage basis for aFrame's children. This can
                // influence their inline size contributions, e.g. if they have an
                // aspect-ratio and a percentage-based block size.
                let percentageBasisBSizeForFrame = match aPercentageBasis {
                    Some(pb) => pb.BSize(childWM),
                    None => NS_UNCONSTRAINEDSIZE,
                };
                let percentageBasisBSizeForChildren: nscoord;
                if aFrame.IsBlockContainer() {
                    // Compute and cache the box-sizing adjustment in contentEdgeToBoxSizing
                    // for later use within this function.
                    contentEdgeToBoxSizing = Some(GetContentEdgeToBoxSizing(boxSizing));

                    // aFrame is a containing block, so its block size (with min and max
                    // block size constraints applied) serves as the percentage basis for
                    // its children.
                    percentageBasisBSizeForChildren = nsIFrame::ComputeBSizeValueAsPercentageBasis(&styleBSize, &styleMinBSize, &styleMaxBSize, percentageBasisBSizeForFrame, contentEdgeToBoxSizing.unwrap().BSize(childWM));
                } else {
                    // aFrame is not a containing block, so its children share the same
                    // containing block as aFrame. Therefore, the percentage basis for
                    // aFrame's children is the same as that for aFrame.
                    percentageBasisBSizeForChildren = percentageBasisBSizeForFrame;
                }
                let input = IntrinsicSizeInput::new(aRenderingContext, aPercentageBasis, Some(LogicalSize::new(childWM, NS_UNCONSTRAINEDSIZE, percentageBasisBSizeForChildren)));
                result = aFrame.IntrinsicISize(&input, aType);
            }

            // If our BSize or min/max-BSize properties are set to values that we can
            // resolve and that will impose a constraint when transferred through our
            // aspect ratio (if we have one), then compute and apply that constraint.
            //
            // (Note: This helper-bool & lambda just let us optimize away the actual
            // transferring-and-clamping arithmetic, for the common case where we can
            // tell that none of the block-axis size properties establish a meaningful
            // transferred constraint.)
            let mightHaveBlockAxisConstraintToTransfer = (|| -> bool {
                if !styleBSize.BehavesLikeInitialValueOnBlockAxis() {
                    return true; // BSize property might have a constraint to transfer.
                }
                // Check for min-BSize values that would obviously produce zero in the
                // transferring logic that follows; zero is trivially-ignorable as a
                // transferred lower-bound. (These include the the property's initial
                // value, explicit 0, and values that are equivalent to these.)
                let minBSizeHasNoConstraintToTransfer = styleMinBSize.BehavesLikeInitialValueOnBlockAxis() || (styleMinBSize.IsLengthPercentage() && styleMinBSize.AsLengthPercentage().IsDefinitelyZero());
                if !minBSizeHasNoConstraintToTransfer {
                    return true; // min-BSize property might have a constraint to transfer.
                }
                if !styleMaxBSize.BehavesLikeInitialValueOnBlockAxis() {
                    return true; // max-BSize property might have a constraint to transfer.
                }
                false
            })();
            if mightHaveBlockAxisConstraintToTransfer {
                let ratio = aFrame.GetAspectRatio();
                if ratio.operator_bool() {
                    AddStateBitToAncestors(aFrame, NS_FRAME_DESCENDANT_INTRINSIC_ISIZE_DEPENDS_ON_BSIZE);

                    let bSizeTakenByBoxSizing = GetDefiniteSizeTakenByBoxSizing(boxSizing, aFrame, !isInlineAxis, ignorePadding, &aPercentageBasis);
                    if contentEdgeToBoxSizing.is_none() {
                        contentEdgeToBoxSizing = Some(GetContentEdgeToBoxSizing(boxSizing));
                    }

                    if let Some(mut bSize) = GetBSize(&styleBSize, aFrame, isInlineAxis, &aPercentageBasis, horizontalAxis) {
                        bSize = std::cmp::max(0, bSize - bSizeTakenByBoxSizing);
                        // We are computing the size of |aFrame|, so we use the inline & block
                        // dimensions of |aFrame|.
                        result = ratio.ComputeRatioDependentSize(if isInlineAxis { LogicalAxis::Inline } else { LogicalAxis::Block }, childWM, bSize, contentEdgeToBoxSizing.unwrap());
                        // We have got the iSizeForAspectRatio value, so we don't need to
                        // compute this again below.
                        iSizeFromAspectRatio = Some(result);
                    }

                    if let Some(mut maxBSize) = GetBSize(&styleMaxBSize, aFrame, isInlineAxis, &aPercentageBasis, horizontalAxis) {
                        maxBSize = std::cmp::max(0, maxBSize - bSizeTakenByBoxSizing);
                        let maxISize = ratio.ComputeRatioDependentSize(if isInlineAxis { LogicalAxis::Inline } else { LogicalAxis::Block }, childWM, maxBSize, contentEdgeToBoxSizing.unwrap());
                        result = std::cmp::min(result, maxISize);
                    }

                    if let Some(mut minBSize) = GetBSize(&styleMinBSize, aFrame, isInlineAxis, &aPercentageBasis, horizontalAxis) {
                        minBSize = std::cmp::max(0, minBSize - bSizeTakenByBoxSizing);
                        let minISize = ratio.ComputeRatioDependentSize(if isInlineAxis { LogicalAxis::Inline } else { LogicalAxis::Block }, childWM, minBSize, contentEdgeToBoxSizing.unwrap());
                        result = std::cmp::max(result, minISize);
                    }
                }
            }
        }

        // If we have an aspect-ratio and a definite block size of |aFrame|, we should
        // use them to resolve the sizes with intrinsic keywords in the inline axis.
        // If |aAxis| is the block axis of |aFrame|, intrinsic keywords should behaves
        // as auto, so we don't need this.
        // https://github.com/w3c/csswg-drafts/issues/5032
        let ar = aFrame.GetAspectRatio();
        if isInlineAxis && ar.operator_bool() && iSizeFromAspectRatio.is_none() && (nsIFrame::IsIntrinsicKeyword(&styleISize) || nsIFrame::IsIntrinsicKeyword(&styleMinISize) || nsIFrame::IsIntrinsicKeyword(&styleMaxISize)) {
            if let Some(mut bSize) = GetBSize(&styleBSize, aFrame, isInlineAxis, &aPercentageBasis, horizontalAxis) {
                if contentEdgeToBoxSizing.is_none() {
                    contentEdgeToBoxSizing = Some(GetContentEdgeToBoxSizing(boxSizing));
                }
                let bSizeTakenByBoxSizing = GetDefiniteSizeTakenByBoxSizing(boxSizing, aFrame, !isInlineAxis, ignorePadding, &aPercentageBasis);

                bSize -= bSizeTakenByBoxSizing;
                iSizeFromAspectRatio = Some(ar.ComputeRatioDependentSize(LogicalAxis::Inline, childWM, bSize, contentEdgeToBoxSizing.unwrap()));
            }
        }

        let contentBoxSize = result;
        result = AddIntrinsicSizeOffset(aRenderingContext, aFrame, &offsetInRequestedAxis, aType, boxSizing, result, &styleISize, fixedMinISize, &styleMinISize, fixedMaxISize, &styleMaxISize, iSizeFromAspectRatio, aFlags, aAxis);
        let overflow = result - aMarginBoxMinSizeClamp;
        if overflow > 0 {
            let newContentBoxSize = std::cmp::max(0 as nscoord, contentBoxSize - overflow);
            result -= contentBoxSize - newContentBoxSize;
        }

        result
    }

    /**
     * Calls IntrinsicForAxis with aFrame's parent's inline physical axis.
     */
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::IntrinsicForContainer
    pub fn IntrinsicForContainer(aRenderingContext: Option<gfxContext>, aFrame: nsIFrame, aType: IntrinsicISizeType, aPercentageBasis: Option<LogicalSize>, aFlags: u32, aSizeOverrides: &StyleSizeOverrides) -> nscoord {
        MOZ_ASSERT!(aFrame.GetParent().is_some());
        // We want the size aFrame will contribute to its parent's inline-size.
        let axis = aFrame.GetParent().unwrap().GetWritingMode().PhysicalAxis(LogicalAxis::Inline);
        nsLayoutUtils::IntrinsicForAxis(axis, aRenderingContext, aFrame, aType, aPercentageBasis, aFlags, NS_MAXSIZE, aSizeOverrides)
    }

    /**
     * Get the contribution of aFrame for the given physical axis and type as the
     * min-size property would give (flex and grid items only; see
     * nsLayoutUtils.h for the full comment).
     */
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::MinSizeContributionForAxis
    pub fn MinSizeContributionForAxis(aAxis: PhysicalAxis, aRC: Option<gfxContext>, aFrame: nsIFrame, aType: IntrinsicISizeType, aPercentageBasis: LogicalSize, aFlags: u32) -> nscoord {
        MOZ_ASSERT!(aFrame.IsFlexOrGridItem(), "only grid/flex items have this behavior currently");

        // Note: this method is only meant for grid/flex items.
        let stylePos = aFrame.StylePosition();
        let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aFrame, std::ptr::null_mut());
        let mut size = if aAxis == PhysicalAxis::Horizontal { stylePos.GetMinWidth(&anchorResolutionParams) } else { stylePos.GetMinHeight(&anchorResolutionParams) };
        let mut maxSize = if aAxis == PhysicalAxis::Horizontal { stylePos.GetMaxWidth(&anchorResolutionParams) } else { stylePos.GetMaxHeight(&anchorResolutionParams) };
        let childWM = aFrame.GetWritingMode();
        let ourInlineAxis = childWM.PhysicalAxis(LogicalAxis::Inline);
        // According to the spec, max-content and min-content should behave as the
        // property's initial values in block axis.
        // It also make senses to use the initial values for -moz-fit-content and
        // -moz-available for intrinsic size in block axis. Therefore, we reset them
        // if needed.
        if aAxis != ourInlineAxis {
            if size.BehavesLikeInitialValueOnBlockAxis() {
                size = AnchorResolvedSizeHelper::Auto();
            }
            if maxSize.BehavesLikeInitialValueOnBlockAxis() {
                maxSize = AnchorResolvedMaxSizeHelper::None();
            }
        }

        let mut fixedMinSize: Option<nscoord> = None;
        if size.IsAuto() {
            if aFrame.StyleDisplay().IsScrollableOverflow() {
                // min-[width|height]:auto with scrollable overflow computes to
                // zero.
                fixedMinSize = Some(0);
            } else {
                size = if aAxis == PhysicalAxis::Horizontal { stylePos.GetWidth(&anchorResolutionParams) } else { stylePos.GetHeight(&anchorResolutionParams) };
                // This is same as above: keywords should behaves as property's initial
                // values in block axis.
                if aAxis != ourInlineAxis && size.BehavesLikeInitialValueOnBlockAxis() {
                    size = AnchorResolvedSizeHelper::Auto();
                }

                fixedMinSize = nsLayoutUtils::GetAbsoluteSize__SizeOrMaxSize(&size);
                if fixedMinSize.is_some() {
                    // We have a definite width/height.  This is the "specified size" in:
                    // https://drafts.csswg.org/css-grid/#min-size-auto
                } else if aFrame.IsPercentageResolvedAgainstZero__StyleSize_StyleMaxSize(&size, &maxSize) {
                    // XXX bug 1463700: this doesn't handle calc() according to spec
                    fixedMinSize = Some(0);
                }
                // fall through - the caller will have to deal with "transferred size"
            }
        } else {
            fixedMinSize = nsLayoutUtils::GetAbsoluteSize__SizeOrMaxSize(&size);
            if fixedMinSize.is_none() && size.IsLengthPercentage() {
                MOZ_ASSERT!(size.HasPercent());
                fixedMinSize = Some(0);
            }
        }

        if fixedMinSize.is_none() {
            // Let the caller deal with the "content size" cases.
            return NS_UNCONSTRAINEDSIZE;
        }

        // NOT PORTED: AutoMaybeDisableFontInflation (font size inflation is off on desktop).

        // The padding/margin percentage basis is the inline-size in the parent's
        // writing-mode.
        let pmPercentageBasis: nscoord = if aFrame.GetParent().unwrap().GetWritingMode().IsOrthogonalTo(&childWM) { aPercentageBasis.BSize(childWM) } else { aPercentageBasis.ISize(childWM) };
        let offsets = if ourInlineAxis == aAxis { aFrame.IntrinsicISizeOffsets(pmPercentageBasis) } else { aFrame.IntrinsicBSizeOffsets(pmPercentageBasis) };
        let mut result: nscoord = 0;
        // Note: aISizeFromAspectRatio is Nothing() here because we don't handle
        // "content size" cases here (i.e. we've returned earlier when |fixedMinSize|
        // is Nothing()).
        result = AddIntrinsicSizeOffset(aRC, aFrame, &offsets, aType, stylePos.mBoxSizing(), result, &size, fixedMinSize, &size, None, &maxSize, None, aFlags, aAxis);

        result
    }

    /*
     * Convert LengthPercentage to nscoord when percentages depend on the
     * containing block size.
     * @param aPercentBasis The width or height of the containing block
     * (whichever the client wants to use for resolving percentages).
     */
    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ComputeCBDependentValue(nscoord, const LengthPercentage&)
    pub fn ComputeCBDependentValue__nscoord_LengthPercentage(aPercentBasis: nscoord, aCoord: &LengthPercentage) -> nscoord {
        NS_ASSERTION!(
            aPercentBasis != NS_UNCONSTRAINEDSIZE || !aCoord.HasPercent(),
            "Have unconstrained percentage basis when percentage resolution needed; this should only result from very large sizes, not attempts at intrinsic size calculation"
        );
        aCoord.Resolve(aPercentBasis)
    }

    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ComputeCBDependentValue(nscoord, const LengthPercentageOrAuto&)
    // NOT PORTED: LengthPercentageOrAuto (no caller in the translated files).

    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ComputeCBDependentValue(nscoord, const AnchorResolvedInset&)
    pub fn ComputeCBDependentValue__nscoord_AnchorResolvedInset(aPercentBasis: nscoord, aInset: &StyleInset) -> nscoord {
        if aInset.IsAuto() {
            // Callers are assumed to have handled other cases already.
            return 0;
        }
        NS_ASSERTION!(
            aPercentBasis != NS_UNCONSTRAINEDSIZE || !aInset.HasPercent(),
            "Have unconstrained percentage basis when percentage resolution needed; this should only result from very large sizes, not attempts at intrinsic size calculation"
        );
        aInset.AsLengthPercentage().Resolve(aPercentBasis)
    }

    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ComputeCBDependentValue(nscoord, const AnchorResolvedMargin&)
    pub fn ComputeCBDependentValue__nscoord_AnchorResolvedMargin(aPercentBasis: nscoord, aMargin: &StyleMargin) -> nscoord {
        if !aMargin.IsLengthPercentage() {
            MOZ_ASSERT!(aMargin.IsAuto(), "Didn't resolve anchor functions first?");
            return 0;
        }
        nsLayoutUtils::ComputeCBDependentValue__nscoord_LengthPercentage(aPercentBasis, aMargin.AsLengthPercentage())
    }

    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ComputeBSizeValue
    pub fn ComputeBSizeValue(aContainingBlockBSize: nscoord, aContentEdgeToBoxSizingBoxEdge: nscoord, aCoord: &LengthPercentage) -> nscoord {
        MOZ_ASSERT!(aContainingBlockBSize != nscoord_MAX || !aCoord.HasPercent(), "caller must deal with %% of unconstrained block-size");

        let result = aCoord.Resolve(aContainingBlockBSize);
        // Clamp calc(), and the subtraction for box-sizing.
        std::cmp::max(0, result - aContentEdgeToBoxSizingBoxEdge)
    }

    // Wrapper for ComputeBSizeValue that also handles 'stretch':
    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ComputeBSizeValueHandlingStretch
    pub fn ComputeBSizeValueHandlingStretch<SizeOrMaxSize: StyleSizeLike>(aContainingBlockBSize: nscoord, aMargin: nscoord, aBorderPadding: nscoord, aContentEdgeToBoxSizingBoxEdge: nscoord, aSize: &SizeOrMaxSize) -> nscoord {
        if aSize.BehavesLikeStretchOnBlockAxis() {
            // Note: we don't need to worry about accounting for "box-sizing" when
            // resolving 'stretch' here. This function unconditionally returns a
            // content-box size, and the content-box size of a stretched element is
            // the same regardless of whether whether the author is conceptually
            // asking us to stretch the content box vs. the border-box.
            return nsLayoutUtils::ComputeStretchContentBoxBSize(aContainingBlockBSize, aMargin, aBorderPadding);
        }
        nsLayoutUtils::ComputeBSizeValue(aContainingBlockBSize, aContentEdgeToBoxSizingBoxEdge, aSize.AsLengthPercentage())
    }

    /**
     * Returns the size that an element's box should take on, in order for its
     * margin-box to exactly reach a particular larger size (e.g. to fill its
     * containing block in a particular axis). The box in question can be either
     * the content-box or the border-box, determined by the aBoxSizing param.
     *
     * This function can be used to resolve the "stretch" size for the child box,
     * for example: https://drafts.csswg.org/css-sizing-4/#stretch-fit-sizing
     *
     * The returned value is floored at 0.
     *
     * There's a version for ISize and BSize; the only difference is that the
     * BSize version has an assertion to be sure that we're not inadvertently
     * doing arithmetic with the NS_UNCONSTRAINEDSIZE sentinel value in that
     * axis. (This sentinel has special meaning as a block-axis size but not as
     * an inline-axis size; hence, the assertion only makes sense for block-axis
     * sizes.)
     *
     * @param aSizeToFill
     *   The size that the child's margin-box should fill, in the axis in
     *   question -- e.g. the containing block size.  Assumed to be a constrained
     *   size; this function doesn't have any special treatment to handle the
     *   case where this is unconstrained.
     *
     * @param aMargin
     *   The sum of the child box's margins in the axis in question (using zero
     *   for any margins that should be ignored in computing the 'stretch' size;
     *   see bug 1932993 for one special case where this should happen).
     *
     * @param aBorderPadding
     *   The sum of the child box's border and padding in the axis in question.
     *
     * @param aBoxSizing
     *   The StyleBoxSizing enum that represents the box that the caller wants to
     *   resolve a size for. NOTE: it may or may not be appropriate to actually
     *   pass the true specified 'box-sizing' value for this param; it depends on
     *   what box the caller is trying to actually resolve. In many cases, we
     *   internally work with variables that unconditionally represent a
     *   content-box size, regardless of the 'box-sizing' value; and for those
     *   cases, it would be appropriate to unconditionally pass
     *   StyleBoxSizing::ContentBox to this function, or to just use the
     *   convenience-wrapper that has "ContentBox" in the function name.
     */
    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ComputeStretchBSize
    pub fn ComputeStretchBSize(aSizeToFill: nscoord, aMargin: nscoord, aBorderPadding: nscoord, aBoxSizing: StyleBoxSizing) -> nscoord {
        NS_ASSERTION!(aSizeToFill != NS_UNCONSTRAINEDSIZE, "We don't handle situations with unconstrained aSizeToFill; caller should handle that!");
        let mut stretchSize = aSizeToFill - aMargin;
        if aBoxSizing == StyleBoxSizing::ContentBox {
            stretchSize -= aBorderPadding;
        }
        std::cmp::max(0, stretchSize)
    }

    // Convenience wrapper that assumes we're resolving the content-box size:
    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ComputeStretchContentBoxBSize
    pub fn ComputeStretchContentBoxBSize(aSizeToFill: nscoord, aMargin: nscoord, aBorderPadding: nscoord) -> nscoord {
        nsLayoutUtils::ComputeStretchBSize(aSizeToFill, aMargin, aBorderPadding, StyleBoxSizing::ContentBox)
    }

    // Similar to the above convenience-wrapper, but now for inline-axis.
    // TODO(dholbert): would it be useful to add a box-sizing-aware version of
    // this API for the inline axis too, like we've got for the block axis?
    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ComputeStretchContentBoxISize
    pub fn ComputeStretchContentBoxISize(aSizeToFill: nscoord, aMargin: nscoord, aBorderPadding: nscoord) -> nscoord {
        std::cmp::max(0, aSizeToFill - aMargin - aBorderPadding)
    }

    /**
     * The "extremum length" values (see ExtremumLength) that return true from
     * 'BehavesLikeInitialValueOnBlockAxis()' were originally aimed at
     * inline-size (or width, as it was before logicalization). For now, we return
     * true for those here, so that we don't call ComputeBSizeValue with value
     * types that it doesn't understand. (See bug 1113216.)
     */
    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::IsAutoBSize
    pub fn IsAutoBSize<SizeOrMaxSize: StyleSizeLike>(aCoord: &SizeOrMaxSize, aCBBSize: nscoord) -> bool {
        // Note: percentages and 'stretch' both behave like 'auto' in the block
        // axis *if and only if* they're resolved against an unconstrained
        // block-size (on their containing block). That's what the second half of
        // this condition is handling.
        aCoord.BehavesLikeInitialValueOnBlockAxis() || (aCBBSize == nscoord_MAX && (aCoord.HasPercent() || aCoord.BehavesLikeStretchOnBlockAxis()))
    }

    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::IsPaddingZero
    pub fn IsPaddingZero(aLength: &LengthPercentage) -> bool {
        // clamp negative calc() to 0
        aLength.Resolve(nscoord_MAX) <= 0 && aLength.Resolve(0) <= 0
    }

    /**
     * Resolve a LengthPercentage to a definite size: an unconstrained percentage basis resolves
     * the percentage against zero.
     */
    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ResolveToLength<clampNegativeResultToZero>
    pub fn ResolveToLength<const clampNegativeResultToZero: bool>(aLengthPercentage: &LengthPercentage, aPercentageBasis: nscoord) -> nscoord {
        let value = if aPercentageBasis == NS_UNCONSTRAINEDSIZE { aLengthPercentage.Resolve(0) } else { aLengthPercentage.Resolve(aPercentageBasis) };
        if clampNegativeResultToZero { std::cmp::max(0, value) } else { value }
    }

    /**
     * Resolve a column-gap/row-gap to a definite size.
     * @note This method resolves 'normal' to zero.
     *   Callers who want different behavior should handle 'normal' on their own.
     */
    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ResolveGapToLength
    pub fn ResolveGapToLength(aGap: &StyleNonNegativeLengthPercentageOrNormal, aPercentageBasis: nscoord) -> nscoord {
        match aGap {
            mrt_style::style::values::generics::length::GenericLengthPercentageOrNormal::Normal => 0,
            mrt_style::style::values::generics::length::GenericLengthPercentageOrNormal::LengthPercentage(lp) => nsLayoutUtils::ResolveToLength::<true>(&lp.0, aPercentageBasis),
        }
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetStyleFrame
    // (the inner table of a table wrapper, which has the table's style; the frame itself otherwise)
    pub fn GetStyleFrame(aPrimaryFrame: nsIFrame) -> nsIFrame {
        if let Some(table) = nsTableWrapperFrame::do_QueryFrame(aPrimaryFrame) {
            // The inner table may be null, if aPrimaryFrame is mid-destruction
            return table.InnerTableFrame().0;
        }

        aPrimaryFrame
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::FontSizeInflationFor
    // NOT PORTED: font size inflation (a mobile feature; always off on desktop, where this returns 1.0).
    pub fn FontSizeInflationFor(aFrame: nsIFrame) -> f32 {
        1.0
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::FontSizeInflationEnabled
    // NOT PORTED: font size inflation; always disabled.
    pub fn FontSizeInflationEnabled(aPresContext: &nsPresContext) -> bool {
        false
    }
}

// CSS transforms: mapping a frame's rectangle to an ancestor's coordinates (task 33620, GAPS B159).
// ADAPTER: one document (no cross-document parents), one device pixel per CSS px (the app units per
// device pixel are AppUnitsPerCSSPixel), no visual viewport (ViewportType::Layout), no flags
// (TransformMatrixFlags: not IgnoreScrolling, not StopAtStackingContextAndDisplayPort), no SVG text.
impl nsLayoutUtils {
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::MatrixTransformRect(const nsRect&, const
    // Matrix4x4&, float)
    pub fn MatrixTransformRect(aBounds: nsRect, aMatrix: &Matrix4x4, aFactor: f32) -> nsRect {
        let factor = aFactor as f64;
        let image = gfxRectTyped::new(
            NSAppUnitsToDoublePixels(aBounds.x, factor),
            NSAppUnitsToDoublePixels(aBounds.y, factor),
            NSAppUnitsToDoublePixels(aBounds.width, factor),
            NSAppUnitsToDoublePixels(aBounds.height, factor),
        );

        // We clip from nscoord_MIN to nscoord_MAX which allows the resulting rect to
        // have a size that goes up to 2*nscoord_MAX. We then rely on
        // RoundGfxRectToAppRect to handle this situation intelligently to give us a
        // size representable as an nscoord by shifting the rect to its mid point and
        // shrinking the size to nscoord_MAX.
        let maxBounds = gfxRectTyped::new(nscoord_MIN as f64 / factor, nscoord_MIN as f64 / factor, nscoord_MAX as f64 / factor * 2.0, nscoord_MAX as f64 / factor * 2.0);

        let image = aMatrix.TransformAndClipBounds(image, maxBounds);

        nsLayoutUtils::RoundGfxRectToAppRect(image, aFactor)
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::GetTransformToAncestor
    // ADAPTER: returns the matrix and the ancestor reached (aOutAncestor).
    pub fn GetTransformToAncestor(aFrame: nsIFrame, aAncestor: nsIFrame) -> (Matrix4x4, Option<nsIFrame>) {
        let mut ctm = Matrix4x4::default();
        if aFrame == aAncestor {
            return (ctm, None);
        }
        let mut parent;
        (ctm, parent) = aFrame.GetTransformMatrix(aAncestor);
        if !aFrame.Combines3DTransformWithAncestors() {
            ctm.ProjectTo2D();
        }
        while let Some(cur) = parent {
            if cur == aAncestor {
                break;
            }
            let (m, p) = cur.GetTransformMatrix(aAncestor);
            ctm = ctm * m;
            parent = p;
            if !cur.Combines3DTransformWithAncestors() {
                ctm.ProjectTo2D();
            }
        }
        (ctm, parent)
    }

    // Gecko: layout/base/nsLayoutUtils.cpp TransformGfxRectToAncestor (static), without the cache
    // and the SVG text case; the second value is aPreservesAxisAlignedRectangles.
    fn TransformGfxRectToAncestor(aFrame: nsIFrame, aRect: gfxRectTyped<f32>, aAncestor: nsIFrame) -> (gfxRectTyped<f32>, bool) {
        let (ctm, _) = nsLayoutUtils::GetTransformToAncestor(aFrame, aAncestor);
        // Fill out the axis-alignment flag
        let preserves = ctm.Is2D() && ctm.PreservesAxisAlignedRectangles();
        let factor = AppUnitsPerCSSPixel() as f32;
        let maxBounds = gfxRectTyped::new(nscoord_MIN as f32 / factor, nscoord_MIN as f32 / factor, nscoord_MAX as f32 / factor * 2.0, nscoord_MAX as f32 / factor * 2.0);
        (ctm.TransformAndClipBounds(aRect, maxBounds), preserves)
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::TransformFrameRectToAncestor: `aRect`, in
    // `aFrame`'s coordinates, as the bounds of where it lands in `aAncestor`'s (an ancestor of
    // `aFrame`, or `aFrame` itself) through every transform between them.
    pub fn TransformFrameRectToAncestor(aFrame: nsIFrame, aRect: nsRect, aAncestor: nsIFrame) -> nsRect {
        nsLayoutUtils::TransformFrameRectToAncestorPreserving(aFrame, aRect, aAncestor).0
    }

    // Gecko: nsLayoutUtils::TransformFrameRectToAncestor with aPreservesAxisAlignedRectangles: also
    // whether the mapping keeps rectangles axis-aligned.
    pub fn TransformFrameRectToAncestorPreserving(aFrame: nsIFrame, aRect: nsRect, aAncestor: nsIFrame) -> (nsRect, bool) {
        let srcAppUnitsPerDevPixel = AppUnitsPerCSSPixel() as f32;
        let result = gfxRectTyped::new(
            NSAppUnitsToFloatPixels(aRect.x, srcAppUnitsPerDevPixel),
            NSAppUnitsToFloatPixels(aRect.y, srcAppUnitsPerDevPixel),
            NSAppUnitsToFloatPixels(aRect.width, srcAppUnitsPerDevPixel),
            NSAppUnitsToFloatPixels(aRect.height, srcAppUnitsPerDevPixel),
        );
        let (result, preserves) = nsLayoutUtils::TransformGfxRectToAncestor(aFrame, result, aAncestor);

        (nsLayoutUtils::ScaleThenRoundGfxRectToAppRect(result, AppUnitsPerCSSPixel() as f32), preserves)
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::ConstrainToCoordValues(double&, double&)
    pub fn ConstrainToCoordValues(aStart: &mut f64, aSize: &mut f64) {
        // ::ConstrainToCoordValues(double&)
        let constrain = |aVal: &mut f64| {
            if *aVal <= nscoord_MIN as f64 {
                *aVal = nscoord_MIN as f64;
            } else if *aVal >= nscoord_MAX as f64 {
                *aVal = nscoord_MAX as f64;
            }
        };
        let mut max = *aStart + *aSize;

        // Clamp the end points to within nscoord range
        constrain(aStart);
        constrain(&mut max);

        // Here we try to make sure that the resulting nsRect will continue to cover
        // as much of the area that was covered by the original gfx Rect as possible.

        // We must also clamp aSize to {0,nscoord_MAX} since nsRect::Width/Height()
        // can't return a value greater than nscoord_MAX. If aSize is greater than
        // nscoord_MAX then we reduce it to nscoord_MAX while keeping the rect
        // centered:

        *aSize = max - *aStart;
        // If the width if still greater than the max nscoord, then bring both
        // endpoints in by the same amount until it fits.
        if aSize.is_nan() {
            // Can happen if aStart is -inf and aSize is +inf for example.
            // If either aStart or aSize is NaN on entry to this function then the
            // calculations above this will make the other one NaN too, so this check
            // ensures that we do not return NaN for either value.
            *aStart = 0.0;
            *aSize = nscoord_MAX as f64;
        } else if *aSize > nscoord_MAX as f64 {
            let excess = (*aSize - nscoord_MAX as f64) / 2.0;
            *aStart += excess;
            *aSize = nscoord_MAX as f64;
        }
    }

    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::RoundGfxRectToAppRect: a rectangle in
    // pixels, scaled by `aFactor` and rounded out to app units.
    pub fn RoundGfxRectToAppRect<F: MatrixFloat + Into<f64>>(aRect: gfxRectTyped<F>, aFactor: f32) -> nsRect {
        // Get a new Rect whose units are app units by scaling by the specified
        // factor.
        // (ScaleRoundOut, in the rectangle's own float type)
        let factor = aFactor as f64;
        let (x, y, xMost, yMost): (f64, f64, f64, f64) = (aRect.x.into(), aRect.y.into(), aRect.XMost().into(), aRect.YMost().into());
        let right = (xMost * factor).ceil();
        let bottom = (yMost * factor).ceil();
        let sx = (x * factor).floor();
        let sy = (y * factor).floor();

        let mut retval = nsRect::default();

        let mut start = sx;
        let mut size = right - sx;
        // We now need to constrain our results to the max and min values for coords.
        nsLayoutUtils::ConstrainToCoordValues(&mut start, &mut size);
        // ConstrainToCoordValues ensures casting to nscoord is safe.
        retval.x = start as nscoord;
        retval.width = size as nscoord;

        start = sy;
        size = bottom - sy;
        nsLayoutUtils::ConstrainToCoordValues(&mut start, &mut size);
        retval.y = start as nscoord;
        retval.height = size as nscoord;

        let zero = F::from(0.0);
        if aRect.width == zero {
            retval.width = 0;
        }

        if aRect.height == zero {
            retval.height = 0;
        }

        retval
    }

    // Gecko: layout/base/nsLayoutUtils.h nsLayoutUtils::ScaleThenRoundGfxRectToAppRect
    pub fn ScaleThenRoundGfxRectToAppRect(aRect: gfxRectTyped<f32>, aFactor: f32) -> nsRect {
        // Get a new Rect whose units are app units by scaling by the specified
        // factor.
        let scaledRect = gfxRectTyped::new(aRect.x * aFactor, aRect.y * aFactor, aRect.width * aFactor, aRect.height * aFactor);

        let mut retval = nsRect::default();

        let mut start = scaledRect.x as f64;
        let mut size = scaledRect.width as f64;
        // We now need to constrain our results to the max and min values for coords.
        nsLayoutUtils::ConstrainToCoordValues(&mut start, &mut size);
        // ConstrainToCoordValues ensures converting to nscoord is safe.
        retval.x = NSToCoordRoundWithClamp__double(start);
        retval.width = NSToCoordRoundWithClamp__double(size);

        start = scaledRect.y as f64;
        size = scaledRect.height as f64;
        nsLayoutUtils::ConstrainToCoordValues(&mut start, &mut size);
        retval.y = NSToCoordRoundWithClamp__double(start);
        retval.height = NSToCoordRoundWithClamp__double(size);

        retval
    }
}

impl nsLayoutUtils {
    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::FindNearestCommonAncestorFrame
    // (FindNearestCommonAncestorFrameWithinBlock's caller: the parent chains compared)
    pub fn FindNearestCommonAncestorFrame(aFrame1: nsIFrame, aFrame2: nsIFrame) -> Option<nsIFrame> {
        let mut ancestors1 = Vec::new();
        let mut f = Some(aFrame1);
        while let Some(frame) = f {
            ancestors1.push(frame);
            f = frame.GetParent();
        }
        let mut f = Some(aFrame2);
        while let Some(frame) = f {
            if ancestors1.contains(&frame) {
                return Some(frame);
            }
            f = frame.GetParent();
        }
        None
    }

    // Gecko: layout/base/nsLayoutUtils.cpp nsLayoutUtils::TransformRect: `aRect`, in `aFromFrame`'s
    // coordinates, to `aToFrame`'s, through their nearest common ancestor. False (and `aRect`
    // unchanged) when they have none or the transform to `aToFrame` has no inverse
    // (NO_COMMON_ANCESTOR, NONINVERTIBLE_TRANSFORM).
    pub fn TransformRect(aFromFrame: nsIFrame, aToFrame: nsIFrame, aRect: &mut nsRect) -> bool {
        let Some(nearestCommonAncestor) = nsLayoutUtils::FindNearestCommonAncestorFrame(aFromFrame, aToFrame) else {
            return false;
        };
        let (downToDest, _) = nsLayoutUtils::GetTransformToAncestor(aToFrame, nearestCommonAncestor);
        // invert downToDest in place
        let Some(downToDest) = downToDest.MaybeInverse() else {
            return false;
        };
        *aRect = nsLayoutUtils::TransformFrameRectToAncestor(aFromFrame, *aRect, nearestCommonAncestor);

        let devPixelsPerAppUnitFromFrame = 1.0 / AppUnitsPerCSSPixel() as f32;
        let devPixelsPerAppUnitToFrame = 1.0 / AppUnitsPerCSSPixel() as f32;
        let toDevPixels = downToDest.ProjectRectBounds(
            gfxRectTyped::new(
                aRect.x as f32 * devPixelsPerAppUnitFromFrame,
                aRect.y as f32 * devPixelsPerAppUnitFromFrame,
                aRect.width as f32 * devPixelsPerAppUnitFromFrame,
                aRect.height as f32 * devPixelsPerAppUnitFromFrame,
            ),
            gfxRectTyped::new(-f32::MAX * devPixelsPerAppUnitFromFrame * 0.5, -f32::MAX * devPixelsPerAppUnitFromFrame * 0.5, f32::MAX * devPixelsPerAppUnitFromFrame, f32::MAX * devPixelsPerAppUnitFromFrame),
        );
        *aRect = nsRect::new(
            NSToCoordRoundWithClamp__float(toDevPixels.x / devPixelsPerAppUnitToFrame),
            NSToCoordRoundWithClamp__float(toDevPixels.y / devPixelsPerAppUnitToFrame),
            NSToCoordRoundWithClamp__float(toDevPixels.width / devPixelsPerAppUnitToFrame),
            NSToCoordRoundWithClamp__float(toDevPixels.height / devPixelsPerAppUnitToFrame),
        );
        true
    }
}
