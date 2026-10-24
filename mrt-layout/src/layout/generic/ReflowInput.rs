/* struct containing the input to nsIFrame::Reflow */
// Translated from layout/generic/ReflowInput.h and ReflowInput.cpp (GECKO_REV), in the order of the
// header, with the .cpp bodies in place of its declarations; the functions that only the .cpp has
// (static helpers) stand right before their first user. Scope: everything block and inline layout
// needs. What is left out stays as a stub with `NOT PORTED` (absolute and fixed positioning,
// sticky positioning, anchor positioning, flex- and grid-specific paths, tables beyond the type
// checks, scroll containers, column sets, ruby, font size inflation, print/page handling).
//
// Notes on the Rust shape:
//  - `struct ReflowInput : public SizeComputationInput` is composition: `ReflowInput` holds a
//    `SizeComputationInput` (field `mSizeComputationInput`) and `Deref`s to it, so the inherited
//    members and methods (`mFrame`, `ComputedLogicalMargin(wm)`, ...) are used as `self.mFrame`,
//    `self.ComputedLogicalMargin(wm)`, as in C++. The protected members of SizeComputationInput
//    are private to this file (the C++ subclasses are all in this file too).
//  - `const ReflowInput*` / `ReflowInput&` links to parent reflow inputs are `&'a ReflowInput<'a>`;
//    the parent outlives its children, as in Gecko where they are stack objects.
//  - `nsFloatManager*`, `nsLineLayout*` and `nsIFrame**` (mDiscoveredClearance) are raw pointers,
//    exactly because Gecko shares and replaces them (nsAutoFloatManager); dereferencing is `unsafe`.
//  - `mLineHeight` is `mutable` in C++ (a cache filled by the const GetLineHeight); it is a `Cell`.
//  - Default arguments are written out at the call sites; a function with a defaulted parameter is
//    one definition and keeps its name.
use crate::*;
use std::cell::Cell;
use std::ops::{Deref, DerefMut};

// Gecko: layout/generic/ReflowInput.h (forward declarations: nsLineLayout, nsHypotheticalPosition,
// nsPlaceholderFrame, nsReflowStatus, gfxContext, nsFloatManager, nsIPercentBSizeObserver)
// (nsLineLayout is translated in nsLineLayout.rs; the placeholder struct that stood here is gone.)

/**
 * @return aValue clamped to [aMinValue, aMaxValue].
 *
 * @note This function needs to handle aMinValue > aMaxValue. In that case,
 *       aMinValue is returned. That's why we cannot use std::clamp()
 *       since it asserts max >= min.
 * @note If aMinValue and aMaxValue are computed min block-size and max
 *       block-size, it is simpler to use ReflowInput::ApplyMinMaxBSize().
 *       Similarly, there is ReflowInput::ApplyMinMaxISize() for clamping an
 *       inline-size.
 * @see https://www.w3.org/TR/CSS22/visudet.html#min-max-widths
 * @see https://www.w3.org/TR/CSS22/visudet.html#min-max-heights
 */
// Gecko: layout/generic/ReflowInput.h mozilla::CSSMinMax
pub fn CSSMinMax<NumericType: PartialOrd + Copy>(aValue: NumericType, aMinValue: NumericType, aMaxValue: NumericType) -> NumericType {
    let mut result = aValue;
    if aMaxValue < result {
        result = aMaxValue;
    }
    if aMinValue > result {
        result = aMinValue;
    }
    result
}

// Gecko: layout/generic/ReflowInput.cpp AnchorPosResolutionParams::From(const SizeComputationInput*, bool)
// ADAPTER: anchor positioning is not supported (see AnchorPosResolutionParams in nsStyleStruct.rs),
// so there is no override to compute.
impl AnchorPosResolutionParams {
    pub fn From__SizeComputationInput_bool(aSizingInput: &SizeComputationInput, aIgnorePositionArea: bool) -> AnchorPosResolutionParams {
        // NOT PORTED:
        // auto override = AutoResolutionOverrideParams{
        //     aSizingInput->mFrame, aSizingInput->mAnchorPosResolutionCache};
        // if (aIgnorePositionArea) {
        //   override.mPositionAreaInUse = false;
        // }
        // return {aSizingInput->mFrame, aSizingInput->mFrame->StyleDisplay()->mPosition,
        //         aSizingInput->mAnchorPosResolutionCache, override};
        AnchorPosResolutionParams
    }
}

// Gecko: layout/generic/ReflowInput.cpp CheckNextInFlowParenthood
fn CheckNextInFlowParenthood(aFrame: nsIFrame, aParent: nsIFrame) -> bool {
    let frameNext = aFrame.GetNextInFlow();
    let parentNext = aParent.GetNextInFlow();
    match (frameNext, parentNext) {
        (Some(f), Some(p)) => f.GetParent() == Some(p),
        _ => false,
    }
}

/**
 * Adjusts the margin for a list (ol, ul), if necessary, depending on
 * font inflation settings. Unfortunately, because bullets from a list are
 * placed in the margin area, we only have ~40px in which to place the
 * bullets. When they are inflated, however, this causes problems, since
 * the text takes up more space than is available in the margin.
 *
 * This method will return a small amount (in app units) by which the
 * margin can be adjusted, so that the space is available for list
 * bullets to be rendered with font inflation enabled.
 */
// Gecko: layout/generic/ReflowInput.cpp FontSizeInflationListMarginAdjustment
fn FontSizeInflationListMarginAdjustment(aFrame: nsIFrame) -> nscoord {
    // As an optimization we check this block frame specific bit up front before
    // we even check if the frame is a block frame. That's only valid so long as
    // we also have the `IsBlockFrameOrSubclass()` call below. Calling that is
    // expensive though, and we want to avoid it if we know `HasMarker()` would
    // return false.
    if !aFrame.HasAnyStateBits(NS_BLOCK_HAS_MARKER) {
        return 0;
    }

    // On desktop font inflation is disabled, so this will always early exit
    // quickly, but checking the frame state bit is still quicker then this call
    // and very likely to early exit on its own so we check this second.
    let inflation = nsLayoutUtils::FontSizeInflationFor(aFrame);
    if inflation <= 1.0f32 {
        return 0;
    }

    // NOT PORTED: the rest of the function (ReflowInput.cpp lines 88-118). Font size inflation is
    // never on (FontSizeInflationFor is 1.0), so nothing below can run. It computes
    // `nsPresContext::CSSPixelsToAppUnits(40) * (inflation - 1)` for an ordered list marker
    // whose list-style-type is not disc, circle, square or a disclosure type.
    0
}

// Gecko: layout/generic/ReflowInput.h SizeComputationInput
// A base class of ReflowInput that computes only the padding,
// border, and margin, since those values are needed more often.
#[derive(Clone)]
pub struct SizeComputationInput {
    // The frame being reflowed.
    pub mFrame: nsIFrame,

    // Rendering context to use for measurement.
    pub mRenderingContext: Option<gfxContext>,

    // Cache for anchor resolution in this computation.
    pub mAnchorPosResolutionCache: *mut AnchorPosResolutionCache,

    // cached copy of the frame's writing-mode, for logical coordinates
    mWritingMode: WritingMode,

    // Cached mFrame->IsThemed().
    mIsThemed: bool,

    // Computed margin values
    mComputedMargin: LogicalMargin,

    // Cached copy of the border + padding values
    mComputedBorderPadding: LogicalMargin,

    // Computed padding values
    mComputedPadding: LogicalMargin,
}

impl SizeComputationInput {
    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::ComputedPhysicalMargin
    pub fn ComputedPhysicalMargin(&self) -> nsMargin {
        self.mComputedMargin.GetPhysicalMargin(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::ComputedPhysicalBorderPadding
    pub fn ComputedPhysicalBorderPadding(&self) -> nsMargin {
        self.mComputedBorderPadding.GetPhysicalMargin(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::ComputedPhysicalBorder
    pub fn ComputedPhysicalBorder(&self) -> nsMargin {
        self.ComputedLogicalBorder(self.mWritingMode).GetPhysicalMargin(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::ComputedPhysicalPadding
    pub fn ComputedPhysicalPadding(&self) -> nsMargin {
        self.mComputedPadding.GetPhysicalMargin(self.mWritingMode)
    }

    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::ComputedLogicalMargin
    pub fn ComputedLogicalMargin(&self, aWM: WritingMode) -> LogicalMargin {
        self.mComputedMargin.ConvertTo(aWM, self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::ComputedLogicalBorderPadding
    pub fn ComputedLogicalBorderPadding(&self, aWM: WritingMode) -> LogicalMargin {
        self.mComputedBorderPadding.ConvertTo(aWM, self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::ComputedLogicalPadding
    pub fn ComputedLogicalPadding(&self, aWM: WritingMode) -> LogicalMargin {
        self.mComputedPadding.ConvertTo(aWM, self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::ComputedLogicalBorder
    pub fn ComputedLogicalBorder(&self, aWM: WritingMode) -> LogicalMargin {
        (self.mComputedBorderPadding - self.mComputedPadding).ConvertTo(aWM, self.mWritingMode)
    }

    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::SetComputedLogicalMargin
    pub fn SetComputedLogicalMargin(&mut self, aWM: WritingMode, aMargin: LogicalMargin) {
        self.mComputedMargin = aMargin.ConvertTo(self.mWritingMode, aWM);
    }
    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::SetComputedLogicalBorderPadding
    pub fn SetComputedLogicalBorderPadding(&mut self, aWM: WritingMode, aBorderPadding: LogicalMargin) {
        self.mComputedBorderPadding = aBorderPadding.ConvertTo(self.mWritingMode, aWM);
    }
    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::SetComputedLogicalPadding
    pub fn SetComputedLogicalPadding(&mut self, aWM: WritingMode, aPadding: LogicalMargin) {
        self.mComputedPadding = aPadding.ConvertTo(self.mWritingMode, aWM);
    }

    // Gecko: layout/generic/ReflowInput.h SizeComputationInput::GetWritingMode
    pub fn GetWritingMode(&self) -> WritingMode {
        self.mWritingMode
    }

    // Callers using this constructor must call InitOffsets on their own.
    // Gecko: layout/generic/ReflowInput.cpp SizeComputationInput::SizeComputationInput(nsIFrame*, gfxContext*, AnchorPosResolutionCache*)
    pub fn new__nsIFrame_gfxContext_AnchorPosResolutionCache(aFrame: nsIFrame, aRenderingContext: Option<gfxContext>, aAnchorPosResolutionCache: *mut AnchorPosResolutionCache) -> SizeComputationInput {
        let mWritingMode = aFrame.GetWritingMode();
        SizeComputationInput {
            mFrame: aFrame,
            mRenderingContext: aRenderingContext,
            mAnchorPosResolutionCache: aAnchorPosResolutionCache,
            mWritingMode,
            mIsThemed: aFrame.IsThemed(),
            mComputedMargin: LogicalMargin::new__WritingMode(mWritingMode),
            mComputedBorderPadding: LogicalMargin::new__WritingMode(mWritingMode),
            mComputedPadding: LogicalMargin::new__WritingMode(mWritingMode),
        }
    }

    // Gecko: layout/generic/ReflowInput.cpp SizeComputationInput::SizeComputationInput(nsIFrame*, gfxContext*, WritingMode, nscoord, const Maybe<LogicalMargin>&, const Maybe<LogicalMargin>&)
    pub fn new__nsIFrame_gfxContext_WritingMode_nscoord_Maybe_Maybe(
        aFrame: nsIFrame,
        aRenderingContext: Option<gfxContext>,
        aContainingBlockWritingMode: WritingMode,
        aContainingBlockISize: nscoord,
        aBorder: Option<LogicalMargin>,
        aPadding: Option<LogicalMargin>,
    ) -> SizeComputationInput {
        let mut result = SizeComputationInput::new__nsIFrame_gfxContext_AnchorPosResolutionCache(aFrame, aRenderingContext, std::ptr::null_mut());
        MOZ_ASSERT!(!result.mFrame.IsTableColFrame());
        let frameType = result.mFrame.Type();
        result.InitOffsets(aContainingBlockWritingMode, aContainingBlockISize, frameType, ComputeSizeFlags::default(), aBorder, aPadding, None);
        result
    }
}

impl SizeComputationInput {
    /**
     * Computes margin values from the specified margin style information, and
     * fills in the mComputedMargin member.
     *
     * @param aCBWM Writing mode of the containing block
     * @param aPercentBasis
     *    Inline size of the containing block (in its own writing mode), to use
     *    for resolving percentage margin values in the inline and block axes.
     * @return true if the margin is dependent on the containing block size.
     */
    // Gecko: layout/generic/ReflowInput.cpp SizeComputationInput::ComputeMargin
    fn ComputeMargin(&mut self, aCBWM: WritingMode, mut aPercentBasis: nscoord, aFrameType: LayoutFrameType) -> bool {
        // SVG text frames have no margin.
        if self.mFrame.IsInSVGTextSubtree() {
            return false;
        }

        if aFrameType == LayoutFrameType::Table {
            // Table frame's margin is inherited to the table wrapper frame via the
            // ::-moz-table-wrapper rule in ua.css, so don't set any margins for it.
            self.SetComputedLogicalMargin(self.mWritingMode, LogicalMargin::new__WritingMode(self.mWritingMode));
            return false;
        }

        // If style style can provide us the margin directly, then use it.
        let styleMargin = self.mFrame.StyleMargin();
        let mut margin = nsMargin::default();
        let isLayoutDependent = !styleMargin.GetMargin__nsMargin(&mut margin);
        if isLayoutDependent {
            // We have to compute the value. Note that this calculation is
            // performed according to the writing mode of the containing block
            // (http://dev.w3.org/csswg/css-writing-modes-3/#orthogonal-flows)
            if aPercentBasis == NS_UNCONSTRAINEDSIZE {
                aPercentBasis = 0;
            }
            let mut m = LogicalMargin::new__WritingMode(aCBWM);
            let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.mFrame, self.mAnchorPosResolutionCache);
            // for (const LogicalSide side : LogicalSides::All)
            for side in [LogicalSide::BStart, LogicalSide::BEnd, LogicalSide::IStart, LogicalSide::IEnd] {
                *m.Side_mut(side, aCBWM) = nsLayoutUtils::ComputeCBDependentValue__nscoord_AnchorResolvedMargin(aPercentBasis, &styleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(side, aCBWM, &anchorResolutionParams));
            }
            self.SetComputedLogicalMargin(aCBWM, m);
        } else {
            self.SetComputedLogicalMargin(self.mWritingMode, LogicalMargin::new__WritingMode_nsMargin(self.mWritingMode, margin));
        }

        // ... but font-size-inflation-based margin adjustment uses the
        // frame's writing mode
        let marginAdjustment = FontSizeInflationListMarginAdjustment(self.mFrame);

        if marginAdjustment > 0 {
            let mut m = self.ComputedLogicalMargin(self.mWritingMode);
            *m.IStart_mut(self.mWritingMode) += marginAdjustment;
            self.SetComputedLogicalMargin(self.mWritingMode, m);
        }

        isLayoutDependent
    }

    /**
     * Computes padding values from the specified padding style information, and
     * fills in the mComputedPadding member.
     *
     * @param aCBWM Writing mode of the containing block
     * @param aPercentBasis
     *    Inline size of the containing block (in its own writing mode), to use
     *    for resolving percentage padding values in the inline and block axes.
     * @return true if the padding is dependent on the containing block size.
     */
    // Gecko: layout/generic/ReflowInput.cpp SizeComputationInput::ComputePadding
    fn ComputePadding(&mut self, aCBWM: WritingMode, mut aPercentBasis: nscoord, aFrameType: LayoutFrameType) -> bool {
        // If style can provide us the padding directly, then use it.
        let stylePadding = self.mFrame.StylePadding();
        let mut padding = nsMargin::default();
        let isCBDependent = !stylePadding.GetPadding(&mut padding);
        // a table row/col group, row/col doesn't have padding
        // XXXldb Neither do border-collapse tables.
        if LayoutFrameType::TableRowGroup == aFrameType || LayoutFrameType::TableColGroup == aFrameType || LayoutFrameType::TableRow == aFrameType || LayoutFrameType::TableCol == aFrameType {
            self.SetComputedLogicalPadding(self.mWritingMode, LogicalMargin::new__WritingMode(self.mWritingMode));
        } else if isCBDependent {
            // We have to compute the value. This calculation is performed
            // according to the writing mode of the containing block
            // (http://dev.w3.org/csswg/css-writing-modes-3/#orthogonal-flows)
            // clamp negative calc() results to 0
            if aPercentBasis == NS_UNCONSTRAINEDSIZE {
                aPercentBasis = 0;
            }
            let mut p = LogicalMargin::new__WritingMode(aCBWM);
            let mPadding = stylePadding.mPadding();
            // for (const LogicalSide side : LogicalSides::All)
            for side in [LogicalSide::BStart, LogicalSide::BEnd, LogicalSide::IStart, LogicalSide::IEnd] {
                *p.Side_mut(side, aCBWM) = std::cmp::max(0, nsLayoutUtils::ComputeCBDependentValue__nscoord_LengthPercentage(aPercentBasis, mPadding.Get__LogicalSide_WritingMode(side, aCBWM)));
            }
            self.SetComputedLogicalPadding(aCBWM, p);
        } else {
            self.SetComputedLogicalPadding(self.mWritingMode, LogicalMargin::new__WritingMode_nsMargin(self.mWritingMode, padding));
        }
        isCBDependent
    }

    // Gecko: layout/generic/ReflowInput.cpp UpdateProp
    fn UpdateProp(aFrame: nsIFrame, aProperty: FramePropertyDescriptor<nsMargin>, aNeeded: bool, aNewValue: nsMargin) {
        if aNeeded {
            // Gecko: if the property has a value, assign to it; otherwise AddProperty(new value).
            // SetProperty does both.
            aFrame.SetProperty(aProperty, aNewValue);
        } else {
            aFrame.RemoveProperty(aProperty);
        }
    }

    // Gecko: layout/generic/ReflowInput.cpp SizeComputationInput::InitOffsets
    fn InitOffsets(&mut self, aCBWM: WritingMode, aPercentBasis: nscoord, aFrameType: LayoutFrameType, aFlags: ComputeSizeFlags, aBorder: Option<LogicalMargin>, aPadding: Option<LogicalMargin>, aDisplay: Option<&nsStyleDisplay>) {
        let presContext = self.mFrame.PresContext();

        // Compute margins from the specified margin style information. These
        // become the default computed values, and may be adjusted below
        // XXX fix to provide 0,0 for the top&bottom margins for
        // inline-non-replaced elements
        let needMarginProp = self.ComputeMargin(aCBWM, aPercentBasis, aFrameType);
        // Note that ComputeMargin() simplistically resolves 'auto' margins to 0.
        // In formatting contexts where this isn't correct, some later code will
        // need to update the UsedMargin() property with the actual resolved value.
        // One example of this is ::CalculateBlockSideMargins().
        SizeComputationInput::UpdateProp(self.mFrame, nsIFrame::UsedMarginProperty(), needMarginProp, self.ComputedPhysicalMargin());

        let wm = self.GetWritingMode();
        let disp = self.mFrame.StyleDisplayWithOptionalParam(aDisplay);
        let mut needPaddingProp: bool;
        if self.mIsThemed {
            // NOT PORTED: native theme padding (presContext->Theme()->GetWidgetPadding, ReflowInput.cpp
            // lines 2476-2482); mIsThemed is never true here.
            unimplemented!("NOT PORTED: themed frames");
        } else if self.mFrame.IsInSVGTextSubtree() {
            self.SetComputedLogicalPadding(wm, LogicalMargin::new__WritingMode(wm));
            needPaddingProp = false;
        } else if let Some(padding) = aPadding {
            // padding is an input arg
            self.SetComputedLogicalPadding(wm, padding);
            let mut stylePadding = nsMargin::default();
            // If the caller passes a padding that doesn't match our style (like
            // nsTextControlFrame might due due to theming), then we also need a
            // padding prop.
            needPaddingProp = !self.mFrame.StylePadding().GetPadding(&mut stylePadding) || padding.GetPhysicalMargin(wm) != stylePadding;
        } else {
            needPaddingProp = self.ComputePadding(aCBWM, aPercentBasis, aFrameType);
        }

        // Add [align|justify]-content:baseline padding contribution.
        // (the C++ lambda captures `this`, `wm` and `needPaddingProp`)
        let ApplyBaselinePadding = |this: &mut SizeComputationInput, needPaddingProp: &mut bool, aAxis: LogicalAxis, aProp: FramePropertyDescriptor<nscoord>| {
            // bool found; nscoord val = mFrame->GetProperty(aProp, &found);
            if let Some(mut val) = this.mFrame.GetProperty(aProp) {
                NS_ASSERTION!(val != 0, "zero in this property is useless");
                let side: LogicalSide;
                if val > 0 {
                    side = MakeLogicalSide(aAxis, LogicalEdge::Start);
                } else {
                    side = MakeLogicalSide(aAxis, LogicalEdge::End);
                    val = -val;
                }
                *this.mComputedPadding.Side_mut(side, wm) += val;
                *needPaddingProp = true;
                if aAxis == LogicalAxis::Block && val > 0 {
                    // We have a baseline-adjusted block-axis start padding, so
                    // we need this to mark lines dirty when mIsBResize is true:
                    this.mFrame.AddStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE);
                }
            }
        };
        if !aFlags.contains(ComputeSizeFlag::IsGridMeasuringReflow) {
            ApplyBaselinePadding(self, &mut needPaddingProp, LogicalAxis::Block, nsIFrame::BBaselinePadProperty());
        }
        if !aFlags.contains(ComputeSizeFlag::ShrinkWrap) {
            ApplyBaselinePadding(self, &mut needPaddingProp, LogicalAxis::Inline, nsIFrame::IBaselinePadProperty());
        }

        let border: LogicalMargin;
        if self.mIsThemed {
            // NOT PORTED: native theme border (GetWidgetBorder, ReflowInput.cpp lines 2531-2536).
            unimplemented!("NOT PORTED: themed frames");
        } else if self.mFrame.IsInSVGTextSubtree() {
            // Do nothing since the border local variable is initialized all zero.
            border = LogicalMargin::new__WritingMode(wm);
        } else if let Some(b) = aBorder {
            // border is an input arg
            border = b;
        } else {
            border = LogicalMargin::new__WritingMode_nsMargin(wm, self.mFrame.StyleBorder().GetComputedBorder());
        }
        self.SetComputedLogicalBorderPadding(wm, border + self.ComputedLogicalPadding(wm));

        if aFrameType == LayoutFrameType::Scrollbar {
            // scrollbars may have had their width or height smashed to zero
            // by the associated scrollframe, in which case we must not report
            // any padding or border.
            let size = self.mFrame.GetSize();
            if size.width == 0 || size.height == 0 {
                self.SetComputedLogicalPadding(wm, LogicalMargin::new__WritingMode(wm));
                self.SetComputedLogicalBorderPadding(wm, LogicalMargin::new__WritingMode(wm));
            }
        }

        let hasPaddingChange: bool;
        if let Some(oldPadding) = self.mFrame.GetProperty(nsIFrame::UsedPaddingProperty()) {
            // Note: If a padding change is already detectable without resolving the
            // percentage, e.g. a padding is changing from 50px to 50%,
            // nsIFrame::DidSetComputedStyle() will cache the old padding in
            // UsedPaddingProperty().
            hasPaddingChange = oldPadding != self.ComputedPhysicalPadding();
        } else {
            // Our padding may have changed, but we can't tell at this point.
            hasPaddingChange = needPaddingProp;
        }
        // Keep mHasPaddingChange bit set until we've done reflow. We'll clear it in
        // nsIFrame::DidReflow()
        self.mFrame.SetHasPaddingChange(self.mFrame.HasPaddingChange() || hasPaddingChange);

        SizeComputationInput::UpdateProp(self.mFrame, nsIFrame::UsedPaddingProperty(), needPaddingProp, self.ComputedPhysicalPadding());
    }

    /**
     * Convert StyleSize or StyleMaxSize to nscoord when percentages depend on the
     * inline size of the containing block, and enumerated values are for inline
     * size, min-inline-size, or max-inline-size.  Does not handle auto inline
     * sizes.
     */
    // Gecko: layout/generic/ReflowInput.cpp SizeComputationInput::ComputeISizeValue
    fn ComputeISizeValue<SizeOrMaxSize: StyleSizeLike>(&self, aContainingBlockSize: LogicalSize, aBoxSizing: StyleBoxSizing, aSize: &SizeOrMaxSize) -> nscoord {
        let wm = self.GetWritingMode();
        let borderPadding = self.ComputedLogicalBorderPadding(wm);
        let margin = self.ComputedLogicalMargin(wm);
        let contentEdgeToBoxSizing = if aBoxSizing == StyleBoxSizing::BorderBox { borderPadding.Size(wm) } else { LogicalSize::new__WritingMode(wm) };
        let boxSizingToMarginEdgeISize = borderPadding.IStartEnd(wm) + margin.IStartEnd(wm) - contentEdgeToBoxSizing.ISize(wm);

        // Get the bSize with anchor functions resolved, and manually resolve
        // 'stretch'-like sizes as well (we should do this a bit cleaner,
        // see bug 2000035):
        let mut bSize = self.mFrame.StylePosition().BSize(wm, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.mFrame, self.mAnchorPosResolutionCache));
        if bSize.BehavesLikeStretchOnBlockAxis() {
            if NS_UNCONSTRAINEDSIZE == aContainingBlockSize.BSize(wm) {
                bSize = AnchorResolvedSizeHelper::Auto();
            } else {
                let stretchBSize = nsLayoutUtils::ComputeStretchBSize(aContainingBlockSize.BSize(wm), margin.BStartEnd(wm), borderPadding.BStartEnd(wm), aBoxSizing);
                bSize = AnchorResolvedSizeHelper::LengthPercentage(LengthPercentage::FromAppUnits(stretchBSize));
            }
        }

        self.mFrame
            .ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_SizeOrMaxSize_StyleSize_AspectRatio_ComputeSizeFlags(
                self.mRenderingContext,
                wm,
                aContainingBlockSize,
                contentEdgeToBoxSizing,
                boxSizingToMarginEdgeISize,
                aSize,
                &bSize,
                self.mFrame.GetAspectRatio(),
                ComputeSizeFlags::default(),
            )
            .mISize
    }

    /**
     * Wrapper for SizeComputationInput::ComputeBSizeValue (defined below, which
     * itself is a wrapper for nsLayoutUtils::ComputeBSizeValue). This one just
     * handles 'stretch' sizes first.
     */
    // Gecko: layout/generic/ReflowInput.cpp SizeComputationInput::ComputeBSizeValueHandlingStretch
    fn ComputeBSizeValueHandlingStretch<SizeOrMaxSize: StyleSizeLike>(&self, aContainingBlockBSize: nscoord, aBoxSizing: StyleBoxSizing, aSize: &SizeOrMaxSize) -> nscoord {
        if aSize.BehavesLikeStretchOnBlockAxis() {
            let wm = self.GetWritingMode();
            return nsLayoutUtils::ComputeStretchContentBoxBSize(aContainingBlockBSize, self.ComputedLogicalMargin(wm).Size(wm).BSize(wm), self.ComputedLogicalBorderPadding(wm).Size(wm).BSize(wm));
        }
        self.ComputeBSizeValue(aContainingBlockBSize, aBoxSizing, aSize.AsLengthPercentage())
    }

    /**
     * Wrapper for nsLayoutUtils::ComputeBSizeValue, which automatically figures
     * out the value to pass for its aContentEdgeToBoxSizingBoxEdge param.
     */
    // Gecko: layout/generic/ReflowInput.cpp SizeComputationInput::ComputeBSizeValue
    fn ComputeBSizeValue(&self, aContainingBlockBSize: nscoord, aBoxSizing: StyleBoxSizing, aSize: &LengthPercentage) -> nscoord {
        let wm = self.GetWritingMode();
        let mut inside = 0;
        if aBoxSizing == StyleBoxSizing::BorderBox {
            inside = self.ComputedLogicalBorderPadding(wm).BStartEnd(wm);
        }
        nsLayoutUtils::ComputeBSizeValue(aContainingBlockBSize, inside, aSize)
    }
}

// Gecko: layout/generic/ReflowInput.h ReflowInput::Flags
// (the C++ struct is memset to zero; bit fields are plain bools here)
#[derive(Clone, Copy, Default, Debug)]
pub struct ReflowInput_Flags {
    // Cached mFrame->IsReplaced().
    pub mIsReplaced: bool,

    // used by tables to communicate special reflow (in process) to handle
    // percent bsize frames inside cells which may not have computed bsizes
    pub mSpecialBSizeReflow: bool,

    // nothing in the frame's next-in-flow (or its descendants) is changing
    pub mNextInFlowUntouched: bool,

    // Is the current context at the top of a page?  When true, we force
    // something that's too tall for a page/column to fit anyway to avoid
    // infinite loops.
    pub mIsTopOfPage: bool,

    // parent frame is an ScrollContainerFrame and it is assuming a horizontal
    // scrollbar
    pub mAssumingHScrollbar: bool,

    // parent frame is an ScrollContainerFrame and it is assuming a vertical
    // scrollbar
    pub mAssumingVScrollbar: bool,

    // Is frame a different inline-size than before?
    pub mIsIResize: bool,

    // Is frame (potentially) a different block-size than before?
    // This includes cases where the block-size is 'auto' and the
    // contents or width have changed.
    pub mIsBResize: bool,

    // Has this frame changed block-size in a way that affects
    // block-size percentages on frames for which it is the containing
    // block?  This includes a change between 'auto' and a length that
    // doesn't actually change the frame's block-size.  It does not
    // include cases where the block-size is 'auto' and the frame's
    // contents have changed.
    //
    // In the current code, this is only true when mIsBResize is also
    // true, although it doesn't necessarily need to be that way (e.g.,
    // in the case of a frame changing from 'auto' to a length that
    // produces the same height).
    pub mIsBResizeForPercentages: bool,

    // tables are splittable, this should happen only inside a page and never
    // insider a column frame
    pub mTableIsSplittable: bool,

    // Does frame height depend on an ancestor table-cell?
    pub mHeightDependsOnAncestorCell: bool,

    // Is this the final reflow of an orthogonal table-cell, after row sizing?
    pub mOrthogonalCellFinalReflow: bool,

    // nsColumnSetFrame is balancing columns
    pub mIsColumnBalancing: bool,

    // We have an ancestor nsColumnSetFrame performing the last column balancing
    // reflow. The available block-size of the last column might become
    // unconstrained.
    pub mIsInLastColumnBalancingReflow: bool,

    // We have an ancestor nsColumnSetFrame performing a measuring reflow. The
    // available block-size becomes unconstrained.
    //
    // Note: only the top-level fragmentainer (multicol or print page sequence)
    // can initiate a measuring reflow, so nested fragmentainers will do a
    // measuring reflow only when the top-level one is doing it. See
    // nsColumnSetFrame::Reflow() and nsPageSequenceFrame::Reflow() for details.
    pub mIsInFragmentainerMeasuringReflow: bool,

    // True if ColumnSetWrapperFrame has a constrained block-size, and is going
    // to consume all of its block-size in this fragment. This bit is passed to
    // nsColumnSetFrame to determine whether to give up balancing and create
    // overflow columns.
    pub mColumnSetWrapperHasNoBSizeLeft: bool,

    // If this flag is set, the BSize of this frame should be considered
    // indefinite for the purposes of percent resolution on child frames (we
    // should behave as if ComputedBSize() were NS_UNCONSTRAINEDSIZE when doing
    // percent resolution against this.ComputedBSize()).  For example: flex
    // items may have their ComputedBSize() resolved ahead-of-time by their
    // flex container, and yet their BSize might have to be considered
    // indefinite per https://drafts.csswg.org/css-flexbox/#definite-sizes
    pub mTreatBSizeAsIndefinite: bool,

    // a "fake" reflow input made in order to be the parent of a real one
    pub mDummyParentReflowInput: bool,

    // Should this frame reflow its place-holder children? If the available
    // height of this frame didn't change, but its in a paginated environment
    // (e.g. columns), it should always reflow its placeholder children.
    pub mMustReflowPlaceholders: bool,

    // the STATIC_POS_IS_CB_ORIGIN ctor flag
    pub mStaticPosIsCBOrigin: bool,

    // If set, the following two flags indicate that:
    // (1) this frame is absolutely-positioned (or fixed-positioned).
    // (2) this frame's static position depends on the CSS Box Alignment.
    // (3) we do need to compute the static position, because the frame's
    //     {Inline and/or Block} offsets actually depend on it.
    // When these bits are set, the offset values (IStart/IEnd, BStart/BEnd)
    // represent the "start" edge of the frame's CSS Box Alignment container
    // area, in that axis -- and these offsets need to be further-resolved
    // (with CSS Box Alignment) after we know the OOF frame's size.
    // NOTE: The "I" and "B" (for "Inline" and "Block") refer the axes of the
    // *containing block's writing-mode*, NOT mFrame's own writing-mode. This
    // is purely for convenience, since that's the writing-mode we're dealing
    // with when we set & react to these bits.
    pub mIOffsetsNeedCSSAlign: bool,
    pub mBOffsetsNeedCSSAlign: bool,

    // Is this frame or one of its ancestors being reflowed in a different
    // continuation than the one in which it was previously reflowed?  In
    // other words, has it moved to a different column or page than it was in
    // the previous reflow?
    //
    // FIXME: For now, we only ensure that this is set correctly for blocks.
    // This is okay because the only thing that uses it only cares about
    // whether there's been a fragment change within the same block formatting
    // context.
    pub mMovedBlockFragments: bool,

    // Is the block-size computed by aspect-ratio and inline size (i.e. block
    // axis is the ratio-dependent axis)? We set this flag so that we can check
    // whether to apply automatic content-based minimum sizes once we know the
    // children's block-size (after reflowing them).
    // https://drafts.csswg.org/css-sizing-4/#aspect-ratio-minimum
    pub mIsBSizeSetByAspectRatio: bool,

    // If true, then children of this frame can generate class A breakpoints
    // for paginated reflow.
    pub mCanHaveClassABreakpoints: bool,
}

// Values for |aFlags| passed to constructor
// Gecko: layout/generic/ReflowInput.h ReflowInput::InitFlag
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ReflowInput_InitFlag {
    // Indicates that the parent of this reflow input is "fake" (see
    // mDummyParentReflowInput in mFlags).
    DummyParentReflowInput,

    // Indicates that the calling function will initialize the reflow input, and
    // that the constructor should not call Init().
    CallerWillInit,

    // The caller wants the abs.pos. static-position resolved at the origin of
    // the containing block, i.e. at LogicalPoint(0, 0). (Note that this
    // doesn't necessarily mean that (0, 0) is the *correct* static position
    // for the frame in question.)
    // @note In a Grid container's masonry axis we'll always use
    // the placeholder's position in that axis regardless of this flag.
    StaticPosIsCBOrigin,
}
impl EnumSetValue for ReflowInput_InitFlag {
    fn BitFor(self) -> u32 {
        1 << (self as u32)
    }
}
// Gecko: layout/generic/ReflowInput.h ReflowInput::InitFlags
pub type ReflowInput_InitFlags = EnumSet<ReflowInput_InitFlag>;

// Use "No" to request SetComputedISize/SetComputedBSize not to reset resize
// flags.
// Gecko: layout/generic/ReflowInput.h ReflowInput::ResetResizeFlags
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReflowInput_ResetResizeFlags {
    No,
    Yes,
}

// mBorderPadding and mFrame are both in mBoxContainer's writing-mode.
// Gecko: layout/generic/ReflowInput.h ReflowInput::HypotheticalBoxContainerInfo
#[derive(Clone, Copy)]
pub struct ReflowInput_HypotheticalBoxContainerInfo {
    pub mBoxContainer: nsIFrame,
    pub mBorderPadding: LogicalMargin,
    pub mContentBoxSize: LogicalSize,
}

// Gecko: layout/generic/ReflowInput.cpp nsHypotheticalPosition
pub struct nsHypotheticalPosition {
    // offset from inline-start edge of containing block (which is a padding edge)
    pub mIStart: nscoord,
    // offset from block-start edge of containing block (which is a padding edge)
    pub mBStart: nscoord,
    pub mWritingMode: WritingMode,
}

/**
 * State passed to a frame during reflow.
 *
 * @see nsIFrame#Reflow()
 */
// Gecko: layout/generic/ReflowInput.h ReflowInput
// (a ReflowInput can be copied: the C++ copy constructor is the compiler's, `Clone` here)
#[derive(Clone)]
pub struct ReflowInput<'a> {
    // `struct ReflowInput : public SizeComputationInput`, see the notes at the top of the file.
    mSizeComputationInput: SizeComputationInput,

    // the reflow inputs are linked together. this is the pointer to the
    // parent's reflow input
    pub mParentReflowInput: Option<&'a ReflowInput<'a>>,

    // A non-owning pointer to the float manager associated with this area,
    // which points to the object owned by nsAutoFloatManager::mNew.
    pub mFloatManager: *mut nsFloatManager,

    // LineLayout object (only for inline reflow; set to nullptr otherwise)
    pub mLineLayout: *mut nsLineLayout,

    // The appropriate reflow input for the containing block (for
    // percentage widths, etc.) of this reflow input's frame. It will be setup
    // properly in InitCBReflowInput().
    pub mCBReflowInput: Option<&'a ReflowInput<'a>>,

    // The amount the in-flow position of the block is moving vertically relative
    // to its previous in-flow position (i.e. the amount the line containing the
    // block is moving).
    // This should be zero for anything which is not a block outside, and it
    // should be zero for anything which has a non-block parent.
    // The intended use of this value is to allow the accurate determination
    // of the potential impact of a float
    // This takes on an arbitrary value the first time a block is reflowed
    pub mBlockDelta: nscoord,

    // Our saved containing block dimensions.
    pub mContainingBlockSize: LogicalSize,

    // Cached pointers to the various style structs used during initialization.
    // ADAPTER: the C++ members are `const nsStyleX*`, null until Init(); the wrappers here cannot
    // be null, so they are filled when the ReflowInput is created (and again in Init(), as in C++).
    pub mStyleDisplay: nsStyleDisplay,
    pub mStylePosition: nsStylePosition,
    pub mStyleBorder: nsStyleBorder,
    pub mStyleMargin: nsStyleMargin,

    pub mBreakType: BreakType,

    // a frame (e.g. nsTableCellFrame) which may need to generate a special
    // reflow for percent bsize calculations
    pub mPercentBSizeObserver: Option<&'a dyn nsIPercentBSizeObserver>,

    // CSS margin collapsing sometimes requires us to reflow
    // optimistically assuming that margins collapse to see if clearance
    // is required. When we discover that clearance is required, we
    // store the frame in which clearance was discovered to the location
    // requested here.
    pub mDiscoveredClearance: *mut Option<nsIFrame>,

    pub mFlags: ReflowInput_Flags,

    pub mStyleSizeOverrides: StyleSizeOverrides,

    pub mComputeSizeFlags: ComputeSizeFlags,

    // This value keeps track of how deeply nested a given reflow input
    // is from the top of the frame tree.
    pub mReflowDepth: i16,

    // The available size in which to reflow the frame. The space represents the
    // amount of room for the frame's margin, border, padding, and content area.
    //
    // The available inline-size should be constrained. The frame's inline-size
    // you choose should fit within it.

    // In galley mode, the available block-size is always unconstrained, and only
    // page mode or multi-column layout involves a constrained available
    // block-size.
    //
    // An unconstrained available block-size means you can choose whatever size
    // you want. If the value is constrained, the frame's block-start border,
    // padding, and content, must fit. If a frame is fully-complete after reflow,
    // then its block-end border, padding, and margin (and similar for its
    // fully-complete ancestors) will need to fit within this available
    // block-size. However, if a frame is monolithic, it may choose a block-size
    // larger than the available block-size.
    mAvailableSize: LogicalSize,

    // ADAPTER: not a member of the C++ class. nsBlockFrame::Reflow const_casts its `const ReflowInput&`
    // to change the available block-size and the float manager (as Gecko does). A Rust `&ReflowInput`
    // of a type without interior mutability is passed as `noalias readonly`, which makes such a write
    // undefined; a Cell (a type with an UnsafeCell inside) makes the compiler assume nothing about the
    // memory behind the reference.
    _mNotFrozen: std::cell::Cell<()>,

    // The computed size specifies the frame's content area, and it does not apply
    // to inline non-replaced elements.
    //
    // For block-level frames, the computed inline-size is based on the
    // inline-size of the containing block, the margin/border/padding areas, and
    // the min/max inline-size.
    //
    // For non-replaced block-level frames in the flow and floated, if the
    // computed block-size is NS_UNCONSTRAINEDSIZE, you should choose a block-size
    // to shrink wrap around the normal flow child frames. The block-size must be
    // within the limit of the min/max block-size if there is such a limit.
    mComputedSize: LogicalSize,

    // Computed values for 'inset' properties. Only applies to 'positioned'
    // elements.
    mComputedOffsets: LogicalMargin,

    // Computed value for 'min-inline-size'/'min-block-size'.
    mComputedMinSize: LogicalSize,

    // Computed value for 'max-inline-size'/'max-block-size'.
    mComputedMaxSize: LogicalSize,

    // Percentage basis in the block axis for the purpose of percentage resolution
    // on children.
    //
    // This will be ignored when mTreatBSizeAsIndefinite flag is true, or when a
    // customized containing block size is provided via ReflowInput's constructor
    // or Init(). When this percentage basis exists, it will be used to replace
    // the containing block's ComputedBSize() in
    // ComputeContainingBlockRectangle().
    //
    // This is currently used in a special scenario where we treat certain
    // sized-to-content flex items as having an 'auto' block-size for their final
    // reflow to accomodate fragmentation-imposed block-size growth. This sort of
    // flex item does nonetheless have a known block-size (from the flex layout
    // algorithm) that it needs to use as a definite percentage-basis for its
    // children during its final reflow; and we represent that here.
    mPercentageBasisInBlockAxis: Option<nscoord>,

    // Cache the used line-height property.
    // (`mutable` in C++)
    mLineHeight: Cell<nscoord>,
}

impl<'a> Deref for ReflowInput<'a> {
    type Target = SizeComputationInput;
    fn deref(&self) -> &SizeComputationInput {
        &self.mSizeComputationInput
    }
}

impl<'a> DerefMut for ReflowInput<'a> {
    fn deref_mut(&mut self) -> &mut SizeComputationInput {
        &mut self.mSizeComputationInput
    }
}

impl<'a> ReflowInput<'a> {
    // ADAPTER: the default member initializers of the C++ class (`= nullptr`, `= 0`,
    // `{mWritingMode}`, ...), for the constructors below.
    fn WithDefaultMembers(aBase: SizeComputationInput, aAvailableSize: LogicalSize) -> ReflowInput<'a> {
        let wm = aBase.mWritingMode;
        let frame = aBase.mFrame;
        ReflowInput {
            mParentReflowInput: None,
            mFloatManager: std::ptr::null_mut(),
            mLineLayout: std::ptr::null_mut(),
            mCBReflowInput: None,
            mBlockDelta: 0,
            mContainingBlockSize: LogicalSize::new__WritingMode(wm),
            mStyleDisplay: frame.StyleDisplay(),
            mStylePosition: frame.StylePosition(),
            mStyleBorder: frame.StyleBorder(),
            mStyleMargin: frame.StyleMargin(),
            mBreakType: BreakType::Auto,
            mPercentBSizeObserver: None,
            mDiscoveredClearance: std::ptr::null_mut(),
            mFlags: ReflowInput_Flags::default(),
            mStyleSizeOverrides: StyleSizeOverrides::default(),
            mComputeSizeFlags: ComputeSizeFlags::default(),
            mReflowDepth: 0,
            mAvailableSize: aAvailableSize,
            _mNotFrozen: std::cell::Cell::new(()),
            mComputedSize: LogicalSize::new__WritingMode(wm),
            mComputedOffsets: LogicalMargin::new__WritingMode(wm),
            mComputedMinSize: LogicalSize::new__WritingMode(wm),
            mComputedMaxSize: LogicalSize::new(wm, NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE),
            mPercentageBasisInBlockAxis: None,
            mLineHeight: Cell::new(NS_UNCONSTRAINEDSIZE),
            mSizeComputationInput: aBase,
        }
    }

    // Physical accessors for the private fields. They are needed for
    // compatibility with not-yet-updated code. New code should use the accessors
    // for logical coordinates, unless the code really works on physical
    // coordinates.
    // Gecko: layout/generic/ReflowInput.h ReflowInput::AvailableWidth
    pub fn AvailableWidth(&self) -> nscoord {
        self.mAvailableSize.Width(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::AvailableHeight
    pub fn AvailableHeight(&self) -> nscoord {
        self.mAvailableSize.Height(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedWidth
    pub fn ComputedWidth(&self) -> nscoord {
        self.mComputedSize.Width(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedHeight
    pub fn ComputedHeight(&self) -> nscoord {
        self.mComputedSize.Height(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMinWidth
    pub fn ComputedMinWidth(&self) -> nscoord {
        self.mComputedMinSize.Width(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMaxWidth
    pub fn ComputedMaxWidth(&self) -> nscoord {
        self.mComputedMaxSize.Width(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMinHeight
    pub fn ComputedMinHeight(&self) -> nscoord {
        self.mComputedMinSize.Height(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMaxHeight
    pub fn ComputedMaxHeight(&self) -> nscoord {
        self.mComputedMaxSize.Height(self.mWritingMode)
    }

    // Logical accessors for private fields in mWritingMode.
    // Gecko: layout/generic/ReflowInput.h ReflowInput::AvailableISize
    pub fn AvailableISize(&self) -> nscoord {
        self.mAvailableSize.ISize(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::AvailableBSize
    pub fn AvailableBSize(&self) -> nscoord {
        self.mAvailableSize.BSize(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedISize
    pub fn ComputedISize(&self) -> nscoord {
        self.mComputedSize.ISize(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedBSize
    pub fn ComputedBSize(&self) -> nscoord {
        self.mComputedSize.BSize(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMinISize
    pub fn ComputedMinISize(&self) -> nscoord {
        self.mComputedMinSize.ISize(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMaxISize
    pub fn ComputedMaxISize(&self) -> nscoord {
        self.mComputedMaxSize.ISize(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMinBSize
    pub fn ComputedMinBSize(&self) -> nscoord {
        self.mComputedMinSize.BSize(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMaxBSize
    pub fn ComputedMaxBSize(&self) -> nscoord {
        self.mComputedMaxSize.BSize(self.mWritingMode)
    }

    // WARNING: In general, adjusting available inline-size or block-size is not
    // safe because ReflowInput has members whose values depend on the available
    // size passing through the constructor. For example,
    // CalculateBlockSideMargins() is called during initialization, and uses
    // AvailableSize(). Make sure your use case doesn't lead to stale member
    // values in ReflowInput!
    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetAvailableISize
    pub fn SetAvailableISize(&mut self, aAvailableISize: nscoord) {
        *self.mAvailableSize.ISize_mut(self.mWritingMode) = aAvailableISize;
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetAvailableBSize
    pub fn SetAvailableBSize(&mut self, aAvailableBSize: nscoord) {
        *self.mAvailableSize.BSize_mut(self.mWritingMode) = aAvailableBSize;
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetComputedMinISize
    pub fn SetComputedMinISize(&mut self, aMinISize: nscoord) {
        *self.mComputedMinSize.ISize_mut(self.mWritingMode) = aMinISize;
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetComputedMaxISize
    pub fn SetComputedMaxISize(&mut self, aMaxISize: nscoord) {
        *self.mComputedMaxSize.ISize_mut(self.mWritingMode) = aMaxISize;
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetComputedMinBSize
    pub fn SetComputedMinBSize(&mut self, aMinBSize: nscoord) {
        *self.mComputedMinSize.BSize_mut(self.mWritingMode) = aMinBSize;
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetComputedMaxBSize
    pub fn SetComputedMaxBSize(&mut self, aMaxBSize: nscoord) {
        *self.mComputedMaxSize.BSize_mut(self.mWritingMode) = aMaxBSize;
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetPercentageBasisInBlockAxis
    pub fn SetPercentageBasisInBlockAxis(&mut self, aBSize: nscoord) {
        self.mPercentageBasisInBlockAxis = Some(aBSize);
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::AvailableSize()
    pub fn AvailableSize__(&self) -> LogicalSize {
        self.mAvailableSize
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedSize()
    pub fn ComputedSize__(&self) -> LogicalSize {
        self.mComputedSize
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedSizeWithBSizeFallback
    pub fn ComputedSizeWithBSizeFallback<F: FnOnce() -> nscoord>(&self, aFallback: F) -> LogicalSize {
        let mut size = self.mComputedSize;
        if size.BSize(self.mWritingMode) == NS_UNCONSTRAINEDSIZE {
            *size.BSize_mut(self.mWritingMode) = self.ApplyMinMaxBSize(aFallback(), 0);
        }
        size
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMinSize()
    pub fn ComputedMinSize__(&self) -> LogicalSize {
        self.mComputedMinSize
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMaxSize()
    pub fn ComputedMaxSize__(&self) -> LogicalSize {
        self.mComputedMaxSize
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::AvailableSize(WritingMode)
    pub fn AvailableSize__WritingMode(&self, aWM: WritingMode) -> LogicalSize {
        self.AvailableSize__().ConvertTo(aWM, self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedSize(WritingMode)
    pub fn ComputedSize__WritingMode(&self, aWM: WritingMode) -> LogicalSize {
        self.ComputedSize__().ConvertTo(aWM, self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMinSize(WritingMode)
    pub fn ComputedMinSize__WritingMode(&self, aWM: WritingMode) -> LogicalSize {
        self.ComputedMinSize__().ConvertTo(aWM, self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedMaxSize(WritingMode)
    pub fn ComputedMaxSize__WritingMode(&self, aWM: WritingMode) -> LogicalSize {
        self.ComputedMaxSize__().ConvertTo(aWM, self.mWritingMode)
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedSizeWithPadding
    pub fn ComputedSizeWithPadding(&self, aWM: WritingMode) -> LogicalSize {
        self.ComputedSize__WritingMode(aWM) + self.ComputedLogicalPadding(aWM).Size(aWM)
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedSizeWithBorderPadding
    pub fn ComputedSizeWithBorderPadding(&self, aWM: WritingMode) -> LogicalSize {
        self.ComputedSize__WritingMode(aWM) + self.ComputedLogicalBorderPadding(aWM).Size(aWM)
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedSizeWithMarginBorderPadding
    pub fn ComputedSizeWithMarginBorderPadding(&self, aWM: WritingMode) -> LogicalSize {
        self.ComputedSizeWithBorderPadding(aWM) + self.ComputedLogicalMargin(aWM).Size(aWM)
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedPhysicalSize
    pub fn ComputedPhysicalSize(&self) -> nsSize {
        self.mComputedSize.GetPhysicalSize(self.mWritingMode)
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedPhysicalOffsets
    pub fn ComputedPhysicalOffsets(&self) -> nsMargin {
        self.mComputedOffsets.GetPhysicalMargin(self.mWritingMode)
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedLogicalOffsets
    pub fn ComputedLogicalOffsets(&self, aWM: WritingMode) -> LogicalMargin {
        self.mComputedOffsets.ConvertTo(aWM, self.mWritingMode)
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetComputedLogicalOffsets
    pub fn SetComputedLogicalOffsets(&mut self, aWM: WritingMode, aOffsets: LogicalMargin) {
        self.mComputedOffsets = aOffsets.ConvertTo(self.mWritingMode, aWM);
    }

    // Return ReflowInput's computed size including border-padding, with
    // unconstrained dimensions replaced by zero.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ComputedSizeAsContainerIfConstrained
    pub fn ComputedSizeAsContainerIfConstrained(&self) -> nsSize {
        BorderBoxSizeAsContainerIfConstrained(self.mWritingMode, self.ComputedSize__(), self.mComputedBorderPadding)
    }

    // Return the physical content box relative to the frame itself.
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ComputedPhysicalContentBoxRelativeToSelf
    pub fn ComputedPhysicalContentBoxRelativeToSelf(&self) -> nsRect {
        let bp = self.ComputedPhysicalBorderPadding();
        nsRect::new__nsPoint_nsSize(nsPoint::new(bp.left, bp.top), self.ComputedPhysicalSize())
    }

    // Get the writing mode of the containing block, to resolve float/clear
    // logical sides appropriately.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::GetCBWritingMode
    pub fn GetCBWritingMode(&self) -> WritingMode {
        match self.mCBReflowInput {
            Some(cbri) => cbri.GetWritingMode(),
            None => self.mFrame.GetContainingBlock__uint32_t(0).unwrap().GetWritingMode(),
        }
    }
}

// Return the physical border-box size by combining aContentBoxSize and
// aBorderPadding, with unconstrained dimensions replaced by zero.
// Gecko: layout/generic/ReflowInput.cpp BorderBoxSizeAsContainerIfConstrained
fn BorderBoxSizeAsContainerIfConstrained(aWM: WritingMode, aContentBoxSize: LogicalSize, aBorderPadding: LogicalMargin) -> nsSize {
    let mut size = aContentBoxSize;
    if size.ISize(aWM) == NS_UNCONSTRAINEDSIZE {
        *size.ISize_mut(aWM) = 0;
    } else {
        *size.ISize_mut(aWM) += aBorderPadding.IStartEnd(aWM);
    }
    if size.BSize(aWM) == NS_UNCONSTRAINEDSIZE {
        *size.BSize_mut(aWM) = 0;
    } else {
        *size.BSize_mut(aWM) += aBorderPadding.BStartEnd(aWM);
    }
    size.GetPhysicalSize(aWM)
}

impl<'a> ReflowInput<'a> {
    // Accessors for the resize flags.
    // Gecko: layout/generic/ReflowInput.h ReflowInput::IsIResize
    pub fn IsIResize(&self) -> bool {
        self.mFlags.mIsIResize
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::IsBResize
    pub fn IsBResize(&self) -> bool {
        self.mFlags.mIsBResize
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::IsBResizeForWM
    pub fn IsBResizeForWM(&self, aWM: WritingMode) -> bool {
        if aWM.IsOrthogonalTo(&self.mWritingMode) { self.mFlags.mIsIResize } else { self.mFlags.mIsBResize }
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::IsBResizeForPercentagesForWM
    pub fn IsBResizeForPercentagesForWM(&self, aWM: WritingMode) -> bool {
        // This uses the relatively-accurate mIsBResizeForPercentages flag
        // when the writing modes are parallel, and is a bit more
        // pessimistic when orthogonal.
        if !aWM.IsOrthogonalTo(&self.mWritingMode) { self.mFlags.mIsBResizeForPercentages } else { self.IsIResize() }
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetIResize
    pub fn SetIResize(&mut self, aValue: bool) {
        self.mFlags.mIsIResize = aValue;
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetBResize
    pub fn SetBResize(&mut self, aValue: bool) {
        self.mFlags.mIsBResize = aValue;
    }
    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetBResizeForPercentages
    pub fn SetBResizeForPercentages(&mut self, aValue: bool) {
        self.mFlags.mIsBResizeForPercentages = aValue;
    }

    // Note: The copy constructor is written by the compiler automatically. You
    // can use that and then override specific values if you want, or you can
    // call Init as desired...

    /**
     * Initialize a ROOT reflow input.
     *
     * @param aPresContext Must be equal to aFrame->PresContext().
     * @param aFrame The frame for whose reflow input is being constructed.
     * @param aRenderingContext The rendering context to be used for measurements.
     * @param aAvailableSpace The available space to reflow aFrame (in aFrame's
     *        writing-mode). See comments for mAvailableSize for more information.
     * @param aFlags A set of flags used for additional boolean parameters (see
     *        InitFlags above).
     */
    // Initialize a <b>root</b> reflow input with a rendering context to
    // use for measuring things.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ReflowInput(nsPresContext*, nsIFrame*, gfxContext*, const LogicalSize&, InitFlags)
    pub fn new__nsPresContext_nsIFrame_gfxContext_LogicalSize_InitFlags(aPresContext: &nsPresContext, aFrame: nsIFrame, aRenderingContext: gfxContext, aAvailableSpace: LogicalSize, aFlags: ReflowInput_InitFlags) -> ReflowInput<'a> {
        let base = SizeComputationInput::new__nsIFrame_gfxContext_AnchorPosResolutionCache(aFrame, Some(aRenderingContext), std::ptr::null_mut());
        let mut ri = ReflowInput::WithDefaultMembers(base, aAvailableSpace);
        // MOZ_ASSERT(aRenderingContext, "no rendering context"): a gfxContext value is never null.
        // MOZ_ASSERT(aPresContext, "no pres context"): neither is a reference.
        // MOZ_ASSERT(aFrame, "no frame"): neither is a handle.
        // MOZ_ASSERT(aPresContext == aFrame->PresContext(), "wrong pres context"): there is one
        // pres context per thread here.

        if aFlags.contains(ReflowInput_InitFlag::DummyParentReflowInput) {
            ri.mFlags.mDummyParentReflowInput = true;
        }
        if aFlags.contains(ReflowInput_InitFlag::StaticPosIsCBOrigin) {
            ri.mFlags.mStaticPosIsCBOrigin = true;
        }

        if !aFlags.contains(ReflowInput_InitFlag::CallerWillInit) {
            ri.Init(aPresContext, None, None, None);
        }
        // When we encounter a PageContent frame this will be set to true.
        ri.mFlags.mCanHaveClassABreakpoints = false;
        ri
    }

    /**
     * Initialize a reflow input for a child frame's reflow. Some parts of the
     * state are copied from the parent's reflow input. The remainder is computed.
     *
     * @param aPresContext Must be equal to aFrame->PresContext().
     * @param aParentReflowInput A reference to an ReflowInput object that
     *        is to be the parent of this object.
     * @param aFrame The frame for whose reflow input is being constructed.
     * @param aAvailableSpace The available space to reflow aFrame (in aFrame's
     *        writing-mode). See comments for mAvailableSize for more information.
     * @param aContainingBlockSize An optional size (in aFrame's writing mode),
     *        specifying the containing block size to use instead of the default
     *        size computed by ComputeContainingBlockRectangle(). If
     *        InitFlag::CallerWillInit is used, this is ignored. Pass it via
     *        Init() instead.
     * @param aFlags A set of flags used for additional boolean parameters (see
     *        InitFlags above).
     * @param aStyleSizeOverrides The style data used to override mFrame's when we
     *        call nsIFrame::ComputeSize() internally.
     * @param aComputeSizeFlags A set of flags used when we call
     *        nsIFrame::ComputeSize() internally.
     * @param aAnchorResolutionCache A cache of referenced anchors to be populated
     *        (If specified) for this reflowed frame. Should live for the lifetime
     *        of this ReflowInput.
     */
    // Initialize a reflow input for a child frame's reflow. Some state
    // is copied from the parent reflow input; the remaining state is
    // computed.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ReflowInput(nsPresContext*, const ReflowInput&, nsIFrame*, const LogicalSize&, const Maybe<LogicalSize>&, InitFlags, const StyleSizeOverrides&, ComputeSizeFlags, AnchorPosResolutionCache*)
    pub fn new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
        aPresContext: &nsPresContext,
        aParentReflowInput: &'a ReflowInput<'a>,
        aFrame: nsIFrame,
        aAvailableSpace: LogicalSize,
        aContainingBlockSize: Option<LogicalSize>,
        aFlags: ReflowInput_InitFlags,
        aSizeOverrides: StyleSizeOverrides,
        aComputeSizeFlags: ComputeSizeFlags,
        aAnchorPosResolutionCache: *mut AnchorPosResolutionCache,
    ) -> ReflowInput<'a> {
        let base = SizeComputationInput::new__nsIFrame_gfxContext_AnchorPosResolutionCache(aFrame, aParentReflowInput.mRenderingContext, aAnchorPosResolutionCache);
        let mut ri = ReflowInput::WithDefaultMembers(base, aAvailableSpace);
        // the member initializer list of the C++ constructor
        ri.mParentReflowInput = Some(aParentReflowInput);
        ri.mFloatManager = aParentReflowInput.mFloatManager;
        ri.mLineLayout = if ri.mFrame.IsLineParticipant() { aParentReflowInput.mLineLayout } else { std::ptr::null_mut() };
        ri.mBreakType = aParentReflowInput.mBreakType;
        ri.mPercentBSizeObserver = match aParentReflowInput.mPercentBSizeObserver {
            Some(observer) if observer.NeedsToObserve(&ri) => Some(observer),
            _ => None,
        };
        ri.mFlags = aParentReflowInput.mFlags;
        ri.mStyleSizeOverrides = aSizeOverrides;
        ri.mComputeSizeFlags = aComputeSizeFlags;
        ri.mReflowDepth = aParentReflowInput.mReflowDepth + 1;

        // MOZ_ASSERT(aPresContext, "no pres context"), MOZ_ASSERT(aFrame, "no frame"),
        // MOZ_ASSERT(aPresContext == aFrame->PresContext(), "wrong pres context"): see the root constructor.
        MOZ_ASSERT!(!ri.mFlags.mSpecialBSizeReflow || !aFrame.HasAnyStateBits(NS_FRAME_IS_DIRTY | NS_FRAME_HAS_DIRTY_CHILDREN), "frame should be clean when getting special bsize reflow");

        if ri.mWritingMode.IsOrthogonalTo(&aParentReflowInput.GetWritingMode()) {
            // If the block establishes an orthogonal flow, set up its AvailableISize
            // per https://drafts.csswg.org/css-writing-modes/#orthogonal-auto
            let mWritingMode = ri.mWritingMode;

            let GetISizeConstraint = |aFrame: nsIFrame, aFixed: Option<&mut bool>| -> nscoord {
                let mut limit = NS_UNCONSTRAINEDSIZE;
                let pos = aFrame.StylePosition();
                // Don't add to anchor resolution cache, since this function is called for
                // other frames.
                let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aFrame, std::ptr::null_mut());
                if let Some(size) = nsLayoutUtils::GetAbsoluteSize__SizeOrMaxSize(&pos.ISize(mWritingMode, &anchorResolutionParams)) {
                    limit = size;
                    if let Some(fixed) = aFixed {
                        *fixed = true;
                    }
                } else if let Some(maxSize) = nsLayoutUtils::GetAbsoluteSize__SizeOrMaxSize(&pos.MaxISize(mWritingMode, &anchorResolutionParams)) {
                    limit = maxSize;
                }
                if limit != NS_UNCONSTRAINEDSIZE {
                    if let Some(minSize) = nsLayoutUtils::GetAbsoluteSize__SizeOrMaxSize(&pos.MinISize(mWritingMode, &anchorResolutionParams)) {
                        limit = std::cmp::max(limit, minSize);
                    }
                }
                limit
            };

            // See if the containing block has a fixed size we should respect:
            let cb = ri.mFrame.GetContainingBlock__uint32_t(0).unwrap();
            let mut isFixed = false;
            let mut cbLimit = match aContainingBlockSize {
                Some(s) => s.ISize(mWritingMode),
                None => NS_UNCONSTRAINEDSIZE,
            };
            if cbLimit != NS_UNCONSTRAINEDSIZE {
                isFixed = true;
            } else {
                cbLimit = GetISizeConstraint(cb, Some(&mut isFixed));
            }

            if isFixed {
                ri.SetAvailableISize(cbLimit);
            } else {
                // If the CB size wasn't fixed, we consider the nearest scroll container
                // and the ICB.

                let mut scLimit = NS_UNCONSTRAINEDSIZE;
                // If the containing block was not a scroll container itself, look up the
                // parent chain for a scroller size that we should respect.
                // XXX Could maybe use nsLayoutUtils::GetNearestScrollContainerFrame here,
                // but unsure if we need the additional complexity it supports?
                if !cb.IsScrollContainerFrame() {
                    let mut p = ri.mFrame.GetParent();
                    while let Some(pf) = p {
                        if pf.IsScrollContainerFrame() {
                            scLimit = GetISizeConstraint(pf, None);
                            // Only the closest ancestor scroller is relevant, so quit as soon
                            // as we've found one (whether or not it had fixed sizing).
                            break;
                        }
                        p = pf.GetParent();
                    }
                }

                let icbSize = LogicalSize::new__WritingMode_nsSize(mWritingMode, GetICBSize(aPresContext, ri.mFrame));
                let icbLimit = icbSize.ISize(mWritingMode);

                ri.SetAvailableISize(std::cmp::min(icbLimit, std::cmp::min(scLimit, cbLimit)));

                // Record that this frame needs to be invalidated on a resize reflow.
                // NOT PORTED: mFrame->PresShell()->AddOrthogonalFlow(mFrame) (the list of
                // orthogonal flows that a viewport resize reflow invalidates; a first layout has
                // no resize).
            }
        }

        // Note: mFlags was initialized as a copy of aParentReflowInput.mFlags up in
        // this constructor's init list, so the only flags that we need to explicitly
        // initialize here are those that may need a value other than our parent's.
        ri.mFlags.mNextInFlowUntouched = aParentReflowInput.mFlags.mNextInFlowUntouched && CheckNextInFlowParenthood(aFrame, aParentReflowInput.mFrame);
        ri.mFlags.mAssumingHScrollbar = false;
        ri.mFlags.mAssumingVScrollbar = false;
        ri.mFlags.mIsColumnBalancing = false;
        ri.mFlags.mColumnSetWrapperHasNoBSizeLeft = false;
        ri.mFlags.mTreatBSizeAsIndefinite = false;
        ri.mFlags.mDummyParentReflowInput = false;
        ri.mFlags.mStaticPosIsCBOrigin = aFlags.contains(ReflowInput_InitFlag::StaticPosIsCBOrigin);
        ri.mFlags.mIOffsetsNeedCSSAlign = false;
        ri.mFlags.mBOffsetsNeedCSSAlign = false;

        // We don't want the mOrthogonalCellFinalReflow flag to be inherited; it's up
        // to the table row frame to set it for its direct children as needed.
        ri.mFlags.mOrthogonalCellFinalReflow = false;

        // aPresContext->IsPaginated() and the named pages pref should have been
        // checked when constructing the root ReflowInput.
        if aParentReflowInput.mFlags.mCanHaveClassABreakpoints {
            MOZ_ASSERT!(aPresContext.IsPaginated(), "mCanHaveClassABreakpoints set during non-paginated reflow.");
        }

        {
            match ri.mFrame.Type() {
                LayoutFrameType::PageContent => {
                    // PageContent requires paginated reflow.
                    MOZ_ASSERT!(aPresContext.IsPaginated(), "nsPageContentFrame should not be in non-paginated reflow");
                    MOZ_ASSERT!(!ri.mFlags.mCanHaveClassABreakpoints, "mFlags.mCanHaveClassABreakpoints should have been initalized to false before we found nsPageContentFrame");
                    ri.mFlags.mCanHaveClassABreakpoints = true;
                }
                LayoutFrameType::Block | LayoutFrameType::Canvas | LayoutFrameType::FlexContainer | LayoutFrameType::GridContainer => {
                    if ri.mFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) {
                        // Never allow breakpoints inside of out-of-flow frames.
                        ri.mFlags.mCanHaveClassABreakpoints = false;
                    } else {
                        // This frame type can have class A breakpoints, inherit this flag
                        // from the parent (this is done for all flags during construction).
                        // This also includes Canvas frames, as each PageContent frame always
                        // has exactly one child which is a Canvas frame.
                        // Do NOT include the subclasses of BlockFrame here, as the ones for
                        // which this could be applicable (ColumnSetWrapper and the MathML
                        // frames) cannot have class A breakpoints.
                        MOZ_ASSERT!(ri.mFlags.mCanHaveClassABreakpoints == aParentReflowInput.mFlags.mCanHaveClassABreakpoints);
                    }
                }
                _ => {
                    ri.mFlags.mCanHaveClassABreakpoints = false;
                }
            }
        }

        if aFlags.contains(ReflowInput_InitFlag::DummyParentReflowInput) || (ri.mParentReflowInput.unwrap().mFlags.mDummyParentReflowInput && ri.mFrame.IsTableFrame()) {
            ri.mFlags.mDummyParentReflowInput = true;
        }

        if !aFlags.contains(ReflowInput_InitFlag::CallerWillInit) {
            ri.Init(aPresContext, aContainingBlockSize, None, None);
        }
        ri
    }

    /**
     * This method initializes various data members. It is automatically called by
     * the constructors if InitFlags::CallerWillInit is *not* used.
     *
     * @param aContainingBlockSize An optional size (in mFrame's writing mode),
     *        specifying the containing block size to use instead of the default
     *        size computed by ComputeContainingBlockRectangle().
     * @param aBorder An optional border (in mFrame's writing mode). If given, use
     *        it instead of the border computed from mFrame's StyleBorder.
     * @param aPadding An optional padding (in mFrame's writing mode). If given,
     *        use it instead of the padding computing from mFrame's StylePadding.
     */
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::Init
    pub fn Init(&mut self, aPresContext: &nsPresContext, aContainingBlockSize: Option<LogicalSize>, aBorder: Option<LogicalMargin>, aPadding: Option<LogicalMargin>) {
        // LAYOUT_WARN_IF_FALSE is a debug-only warning macro
        NS_WARNING_ASSERTION!(self.AvailableISize() != NS_UNCONSTRAINEDSIZE, "have unconstrained inline-size; this should only result from very large sizes, not attempts at intrinsic inline-size calculation");

        self.mStylePosition = self.mFrame.StylePosition();
        self.mStyleDisplay = self.mFrame.StyleDisplay();
        self.mStyleBorder = self.mFrame.StyleBorder();
        self.mStyleMargin = self.mFrame.StyleMargin();

        self.InitCBReflowInput();

        let type_ = self.mFrame.Type();
        if type_ == LayoutFrameType::Placeholder {
            // Placeholders have a no-op Reflow method that doesn't need the rest of
            // this initialization, so we bail out early.
            let wm = self.mWritingMode;
            self.mComputedSize.SizeTo(wm, 0, 0);
            return;
        }

        self.mFlags.mIsReplaced = self.mFrame.IsReplaced();

        self.InitConstraints(aPresContext, aContainingBlockSize, aBorder, aPadding, type_);

        self.InitResizeFlags(aPresContext, type_);
        self.InitDynamicReflowRoot();

        let parent = self.mFrame.GetParent();
        if parent.map_or(false, |p| p.HasAnyStateBits(NS_FRAME_IN_CONSTRAINED_BSIZE)) && !(parent.unwrap().IsScrollContainerFrame() && parent.unwrap().StyleDisplay().mOverflowY() != StyleOverflow::Hidden) {
            self.mFrame.AddStateBits(NS_FRAME_IN_CONSTRAINED_BSIZE);
        } else if type_ == LayoutFrameType::SVGForeignObject {
            // An SVG foreignObject frame is inherently constrained block-size.
            self.mFrame.AddStateBits(NS_FRAME_IN_CONSTRAINED_BSIZE);
        } else {
            let anchorResolutionParams = AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false);
            let bSizeCoord = self.mStylePosition.BSize(self.mWritingMode, &anchorResolutionParams);
            let maxBSizeCoord = self.mStylePosition.MaxBSize(self.mWritingMode, &anchorResolutionParams);
            if (!bSizeCoord.BehavesLikeInitialValueOnBlockAxis() || !maxBSizeCoord.BehavesLikeInitialValueOnBlockAxis())
                // Don't set NS_FRAME_IN_CONSTRAINED_BSIZE on body or html elements.
                && self.mFrame.GetContent().map_or(false, |c| !c.IsAnyOfHTMLElements(&[nsGkAtoms::body, nsGkAtoms::html]))
            {
                // If our block-size was specified as a percentage, then this could
                // actually resolve to 'auto', based on:
                // http://www.w3.org/TR/CSS21/visudet.html#the-height-property
                let mut containingBlk = Some(self.mFrame);
                while let Some(cblk) = containingBlk {
                    let stylePos = cblk.StylePosition();
                    // It's for containing block, so don't add to anchor resolution cache
                    let containingBlkAnchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(cblk, std::ptr::null_mut());
                    let bSizeCoord = stylePos.BSize(self.mWritingMode, &containingBlkAnchorResolutionParams);
                    let maxBSizeCoord = stylePos.MaxBSize(self.mWritingMode, &containingBlkAnchorResolutionParams);
                    if (bSizeCoord.IsLengthPercentage() && !bSizeCoord.HasPercent()) || (maxBSizeCoord.IsLengthPercentage() && !maxBSizeCoord.HasPercent()) {
                        self.mFrame.AddStateBits(NS_FRAME_IN_CONSTRAINED_BSIZE);
                        break;
                    } else if bSizeCoord.HasPercent() || maxBSizeCoord.HasPercent() {
                        containingBlk = cblk.GetContainingBlock__uint32_t(0);
                        if containingBlk.is_none() {
                            // If we've reached the top of the tree, then we don't have
                            // a constrained block-size.
                            self.mFrame.RemoveStateBits(NS_FRAME_IN_CONSTRAINED_BSIZE);
                            break;
                        }

                        continue;
                    } else {
                        self.mFrame.RemoveStateBits(NS_FRAME_IN_CONSTRAINED_BSIZE);
                        break;
                    }
                }
            } else {
                self.mFrame.RemoveStateBits(NS_FRAME_IN_CONSTRAINED_BSIZE);
            }
        }

        if self.mParentReflowInput.map_or(false, |p| p.GetWritingMode().IsOrthogonalTo(&self.mWritingMode)) {
            // Orthogonal frames are always reflowed with an unconstrained
            // dimension to avoid incomplete reflow across an orthogonal
            // boundary. Normally this is the block-size, but for column sets
            // with auto-height it's the inline-size, so that they can add
            // columns in the container's block direction
            if type_ == LayoutFrameType::ColumnSet && self.mStylePosition.ISize(self.mWritingMode, &AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false)).IsAuto() {
                self.SetComputedISize(NS_UNCONSTRAINEDSIZE, ReflowInput_ResetResizeFlags::No);
            } else {
                self.SetAvailableBSize(NS_UNCONSTRAINEDSIZE);
            }
        }

        if self.mFrame.GetContainSizeAxes().mBContained {
            // In the case that a box is size contained in block axis, we want to ensure
            // that it is also monolithic. We do this by setting AvailableBSize() to an
            // unconstrained size to avoid fragmentation.
            self.SetAvailableBSize(NS_UNCONSTRAINEDSIZE);
        }

        NS_WARNING_ASSERTION!(
            (self.mStyleDisplay.IsInlineOutsideStyle() && !self.mFrame.IsReplaced()) || type_ == LayoutFrameType::Text || self.ComputedISize() != NS_UNCONSTRAINEDSIZE,
            "have unconstrained inline-size; this should only result from very large sizes, not attempts at intrinsic inline-size calculation"
        );
    }
}

// Gecko: layout/generic/ReflowInput.cpp GetICBSize
fn GetICBSize(aPresContext: &nsPresContext, aFrame: nsIFrame) -> nsSize {
    if !aPresContext.IsPaginated() {
        return aPresContext.GetVisibleArea().Size();
    }
    let mut f = aFrame.GetParent();
    while let Some(frame) = f {
        if frame.IsPageContentFrame() {
            return frame.GetSize();
        }
        f = frame.GetParent();
    }
    aPresContext.GetPageSize()
}

// For "normal" we use the font's normal line height (em height + leading).
// If both internal leading and  external leading specified by font itself are
// zeros, we should compensate this by creating extra (external) leading.
// This is necessary because without this compensation, normal line height might
// look too tight.
// Gecko: layout/generic/ReflowInput.cpp GetNormalLineHeight
fn GetNormalLineHeight(aFontMetrics: &nsFontMetrics) -> nscoord {
    let externalLeading: nscoord = aFontMetrics.ExternalLeading();
    let internalLeading: nscoord = aFontMetrics.InternalLeading();
    let emHeight: nscoord = aFontMetrics.EmHeight();
    if internalLeading == 0 && externalLeading == 0 {
        return NSToCoordRound__float(emHeight as f32 * ReflowInput::kNormalLineHeightFactor);
    }
    emHeight + internalLeading + externalLeading
}

// Gecko: layout/generic/ReflowInput.cpp ComputeLineHeight
fn ComputeLineHeight(aLh: &StyleLineHeight, aFont: &nsFont, aLanguage: Option<nsAtom>, aExplicitLanguage: bool, aPresContext: Option<&nsPresContext>, aIsVertical: bool, aBlockBSize: nscoord, aFontSizeInflation: f32) -> nscoord {
    if let StyleLineHeight::Length(l) = aLh {
        let mut result = l.0.ToAppUnits();
        if aFontSizeInflation != 1.0f32 {
            result = NSToCoordRound__float(result as f32 * aFontSizeInflation);
        }
        return result;
    }

    if let StyleLineHeight::Number(n) = aLh {
        // For factor units the computed value of the line-height property
        // is found by multiplying the factor by the font's computed size
        // (adjusted for min-size prefs and text zoom).
        return aFont.size.ScaledBy(n.0 * aFontSizeInflation).ToAppUnits();
    }

    MOZ_ASSERT!(aLh.IsNormal() || aLh.IsMozBlockHeight());
    if aLh.IsMozBlockHeight() && aBlockBSize != NS_UNCONSTRAINEDSIZE {
        return aBlockBSize;
    }

    let mut size = aFont.size;
    size.ScaleBy(aFontSizeInflation);

    if let Some(aPresContext) = aPresContext {
        let mut font: nsFont = aFont.clone();
        font.size = size;
        let mut params = nsFontMetrics_Params::default();
        params.language = aLanguage;
        params.explicitLanguage = aExplicitLanguage;
        params.orientation = if aIsVertical {
            nsFontMetrics_FontOrientation::eVertical
        } else {
            nsFontMetrics_FontOrientation::eHorizontal
        };
        // NOT PORTED: params.userFontSet, textPerf and featureValueLookup of the pres context.
        let fm: nsFontMetrics = aPresContext.GetMetricsFor(&font, &params);
        return GetNormalLineHeight(&fm);
    }
    // If we don't have a pres context, use a 1.2em fallback.
    size.ScaleBy(ReflowInput::kNormalLineHeightFactor);
    size.ToAppUnits()
}

impl<'a> ReflowInput<'a> {
    // Gecko: layout/generic/ReflowInput.h ReflowInput::kNormalLineHeightFactor
    pub const kNormalLineHeightFactor: f32 = 1.2f32;

    /**
     * Get the used line-height property. The return value will be >= 0.
     */
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::GetLineHeight
    pub fn GetLineHeight(&self) -> nscoord {
        if self.mLineHeight.get() != NS_UNCONSTRAINEDSIZE {
            return self.mLineHeight.get();
        }

        let blockBSize = if nsLayoutUtils::IsNonWrapperBlock(self.mFrame) {
            self.ComputedBSize()
        } else {
            match self.mCBReflowInput {
                Some(cbri) => cbri.ComputedBSize(),
                None => NS_UNCONSTRAINEDSIZE,
            }
        };
        self.mLineHeight.set(ReflowInput::CalcLineHeight__ComputedStyle_nsPresContext_nsIContent_nscoord_float(
            &self.mFrame.Style(),
            Some(&self.mFrame.PresContext()),
            self.mFrame.GetContent().as_ref(),
            blockBSize,
            nsLayoutUtils::FontSizeInflationFor(self.mFrame),
        ));
        self.mLineHeight.get()
    }

    /**
     * Set the used line-height. aLineHeight must be >= 0.
     */
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::SetLineHeight
    pub fn SetLineHeight(&mut self, aLineHeight: nscoord) {
        MOZ_ASSERT!(aLineHeight >= 0, "aLineHeight must be >= 0!");

        if self.mLineHeight.get() != aLineHeight {
            self.mLineHeight.set(aLineHeight);
            // Setting used line height can change a frame's block-size if mFrame's
            // block-size behaves as auto.
            let presContext = self.mFrame.PresContext();
            let frameType = self.mFrame.Type();
            self.InitResizeFlags(&presContext, frameType);
        }
    }

    /**
     * Calculate the used line-height property without a reflow input instance.
     * The return value will be >= 0.
     *
     * @param aBlockBSize The computed block size of the content rect of the block
     *                    that the line should fill. Only used with
     *                    line-height:-moz-block-height. NS_UNCONSTRAINEDSIZE
     *                    results in a normal line-height for
     *                    line-height:-moz-block-height.
     * @param aFontSizeInflation The result of the appropriate
     *                           nsLayoutUtils::FontSizeInflationFor call,
     *                           or 1.0 if during intrinsic size
     *                           calculation.
     */
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::CalcLineHeight(const ComputedStyle&, nsPresContext*, const nsIContent*, nscoord, float)
    pub fn CalcLineHeight__ComputedStyle_nsPresContext_nsIContent_nscoord_float(aStyle: &mrt_style::style::servo_arc::Arc<mrt_style::style::properties::ComputedValues>, aPresContext: Option<&nsPresContext>, aContent: Option<&nsIContent>, aBlockBSize: nscoord, aFontSizeInflation: f32) -> nscoord {
        let styleFont = nsStyleFont::new(aStyle.clone());
        let lh = styleFont.mLineHeight();
        let wm = WritingMode::new__ComputedStyle(aStyle);
        let vertical = wm.IsVertical() && !wm.IsSideways();
        ReflowInput::CalcLineHeight__StyleLineHeight_nsStyleFont_nsPresContext_bool_nsIContent_nscoord_float(&lh, &styleFont, aPresContext, vertical, aContent, aBlockBSize, aFontSizeInflation)
    }

    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::CalcLineHeight(const StyleLineHeight&, const nsStyleFont&, nsPresContext*, bool, const nsIContent*, nscoord, float)
    pub fn CalcLineHeight__StyleLineHeight_nsStyleFont_nsPresContext_bool_nsIContent_nscoord_float(
        aLh: &StyleLineHeight,
        aRelativeToFont: &nsStyleFont,
        aPresContext: Option<&nsPresContext>,
        aIsVertical: bool,
        aContent: Option<&nsIContent>,
        aBlockBSize: nscoord,
        aFontSizeInflation: f32,
    ) -> nscoord {
        let lineHeight = ComputeLineHeight(aLh, &aRelativeToFont.mFont(), aRelativeToFont.mLanguage(), aRelativeToFont.mExplicitLanguage(), aPresContext, aIsVertical, aBlockBSize, aFontSizeInflation);

        NS_ASSERTION!(lineHeight >= 0, "ComputeLineHeight screwed up");

        // NOT PORTED: HTMLInputElement::FromNodeOrNull(aContent) and IsSingleLineTextControl()
        // (ReflowInput.cpp lines 2816-2829): for web compatibility, a single-line text input
        // cannot have a line-height smaller than 'normal'. That needs the input element's type
        // and the font metrics for 'normal'.
        lineHeight
    }

    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::CalcLineHeightForCanvas
    pub fn CalcLineHeightForCanvas(aLh: &StyleLineHeight, aRelativeToFont: &nsFont, aLanguage: Option<nsAtom>, aExplicitLanguage: bool, aPresContext: Option<&nsPresContext>, aWM: WritingMode) -> nscoord {
        ComputeLineHeight(aLh, aRelativeToFont, aLanguage, aExplicitLanguage, aPresContext, aWM.IsVertical() && !aWM.IsSideways(), NS_UNCONSTRAINEDSIZE, 1.0f32)
    }

    /**
     * Apply the mComputed(Min/Max)ISize constraints to the content
     * size computed so far.
     */
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ApplyMinMaxISize
    pub fn ApplyMinMaxISize(&self, mut aISize: nscoord) -> nscoord {
        if NS_UNCONSTRAINEDSIZE != self.ComputedMaxISize() {
            aISize = std::cmp::min(aISize, self.ComputedMaxISize());
        }
        std::cmp::max(aISize, self.ComputedMinISize())
    }

    /**
     * Apply the mComputed(Min/Max)BSize constraints to the content
     * size computed so far.
     *
     * @param aBSize The block-size that we've computed an to which we want to
     *        apply min/max constraints.
     * @param aConsumed The amount of the computed block-size that was consumed by
     *        our prev-in-flows.
     */
    // Gecko: layout/generic/ReflowInput.h ReflowInput::ApplyMinMaxBSize
    // (aConsumed defaults to 0 in C++)
    pub fn ApplyMinMaxBSize(&self, mut aBSize: nscoord, aConsumed: nscoord) -> nscoord {
        aBSize += aConsumed;

        if NS_UNCONSTRAINEDSIZE != self.ComputedMaxBSize() {
            aBSize = std::cmp::min(aBSize, self.ComputedMaxBSize());
        }

        if NS_UNCONSTRAINEDSIZE != self.ComputedMinBSize() {
            aBSize = std::cmp::max(aBSize, self.ComputedMinBSize());
        }

        aBSize - aConsumed
    }

    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ShouldReflowAllKids
    pub fn ShouldReflowAllKids(&self) -> bool {
        // Note that we could make a stronger optimization for IsBResize if
        // we use it in a ShouldReflowChild test that replaces the current
        // checks of NS_FRAME_IS_DIRTY | NS_FRAME_HAS_DIRTY_CHILDREN, if it
        // were tested there along with NS_FRAME_CONTAINS_RELATIVE_BSIZE.
        // This would need to be combined with a slight change in which
        // frames NS_FRAME_CONTAINS_RELATIVE_BSIZE is marked on.
        self.mFrame.HasAnyStateBits(NS_FRAME_IS_DIRTY) || self.IsIResize() || (self.IsBResize() && self.mFrame.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE)) || self.mFlags.mIsInLastColumnBalancingReflow
    }

    // This method doesn't apply min/max computed widths to the value passed in.
    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetComputedWidth
    pub fn SetComputedWidth(&mut self, aComputedWidth: nscoord) {
        if self.mWritingMode.IsVertical() {
            self.SetComputedBSize(aComputedWidth, ReflowInput_ResetResizeFlags::Yes);
        } else {
            self.SetComputedISize(aComputedWidth, ReflowInput_ResetResizeFlags::Yes);
        }
    }

    // This method doesn't apply min/max computed heights to the value passed in.
    // Gecko: layout/generic/ReflowInput.h ReflowInput::SetComputedHeight
    pub fn SetComputedHeight(&mut self, aComputedHeight: nscoord) {
        if self.mWritingMode.IsVertical() {
            self.SetComputedISize(aComputedHeight, ReflowInput_ResetResizeFlags::Yes);
        } else {
            self.SetComputedBSize(aComputedHeight, ReflowInput_ResetResizeFlags::Yes);
        }
    }

    // This method doesn't apply min/max computed inline-sizes to the value passed
    // in.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::SetComputedISize
    // (aFlags defaults to ResetResizeFlags::Yes in C++)
    pub fn SetComputedISize(&mut self, aComputedISize: nscoord, aFlags: ReflowInput_ResetResizeFlags) {
        // It'd be nice to assert that |frame| is not in reflow, but this fails
        // because viewport frames reset the computed isize on a copy of their reflow
        // input when reflowing fixed-pos kids.  In that case we actually don't want
        // to mess with the resize flags, because comparing the frame's rect to the
        // munged computed isize is pointless.
        NS_WARNING_ASSERTION!(aComputedISize >= 0, "Invalid computed inline-size!");
        if self.ComputedISize() != aComputedISize {
            *self.mComputedSize.ISize_mut(self.mWritingMode) = std::cmp::max(0, aComputedISize);
            if aFlags == ReflowInput_ResetResizeFlags::Yes {
                let presContext = self.mFrame.PresContext();
                let frameType = self.mFrame.Type();
                self.InitResizeFlags(&presContext, frameType);
            }
        }
    }

    // These methods don't apply min/max computed block-sizes to the value passed
    // in.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::SetComputedBSize
    pub fn SetComputedBSize(&mut self, aComputedBSize: nscoord, aFlags: ReflowInput_ResetResizeFlags) {
        // It'd be nice to assert that |frame| is not in reflow, but this fails
        // for the same reason as above.
        NS_WARNING_ASSERTION!(aComputedBSize >= 0, "Invalid computed block-size!");
        if self.ComputedBSize() != aComputedBSize {
            *self.mComputedSize.BSize_mut(self.mWritingMode) = std::cmp::max(0, aComputedBSize);
            if aFlags == ReflowInput_ResetResizeFlags::Yes {
                let presContext = self.mFrame.PresContext();
                let frameType = self.mFrame.Type();
                self.InitResizeFlags(&presContext, frameType);
            }
        }
    }

    // Gecko: layout/generic/ReflowInput.h ReflowInput::WillReflowAgainForClearance
    pub fn WillReflowAgainForClearance(&self) -> bool {
        // mDiscoveredClearance && *mDiscoveredClearance
        !self.mDiscoveredClearance.is_null() && unsafe { (*self.mDiscoveredClearance).is_some() }
    }

    // Returns true if we should apply automatic minimum on the block axis.
    //
    // The automatic minimum size in the ratio-dependent axis of a box with a
    // preferred aspect ratio that is neither a replaced element nor a scroll
    // container is its min-content size clamped from above by its maximum size.
    //
    // https://drafts.csswg.org/css-sizing-4/#aspect-ratio-minimum
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ShouldApplyAutomaticMinimumOnBlockAxis
    pub fn ShouldApplyAutomaticMinimumOnBlockAxis(&self) -> bool {
        MOZ_ASSERT!(!self.mFrame.HasReplacedSizing());
        self.mFlags.mIsBSizeSetByAspectRatio
            && !self.mStyleDisplay.IsScrollableOverflow()
            && self.mStylePosition.MinBSize(self.GetWritingMode(), &AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false)).IsAuto()
            && !self.mFrame.GetContainSizeAxes().mBContained
    }

    // Returns true if mFrame has a constrained available block-size, or if mFrame
    // is a continuation. When this method returns true, mFrame can be considered
    // to be in a "fragmented context."
    //
    // Note: this method usually returns true when mFrame is in a paged
    // environment (e.g. printing) or has a multi-column container ancestor.
    // However, this doesn't include several cases when we're intentionally
    // performing layout in a fragmentation-ignoring way, e.g. 1) mFrame is a flex
    // or grid item, and this ReflowInput is for a measuring reflow with an
    // unconstrained available block-size, or 2) mFrame is (or is inside of) an
    // element that forms an orthogonal writing-mode.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::IsInFragmentedContext
    pub fn IsInFragmentedContext(&self) -> bool {
        // We consider mFrame with a prev-in-flow being in a fragmented context
        // because nsColumnSetFrame can reflow its last column with an unconstrained
        // available block-size.
        self.AvailableBSize() != NS_UNCONSTRAINEDSIZE || self.mFrame.GetPrevInFlow().is_some()
    }

    // Compute the offsets for a relative position element
    //
    // @param aWM the writing mode of aCBSize and the returned offsets.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ComputeRelativeOffsets
    pub fn ComputeRelativeOffsets(aWM: WritingMode, aFrame: nsIFrame, aCBSize: LogicalSize) -> LogicalMargin {
        // In relative positioning, anchor functions are always invalid;
        // anchor-resolved insets should no longer contain any reflow to anchor
        // functions.
        let mut offsets = LogicalMargin::new__WritingMode(aWM);
        let position = aFrame.StylePosition();
        let anchorResolutionParams = AnchorPosOffsetResolutionParams::UseCBFrameSize(AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aFrame, std::ptr::null_mut()));

        // Compute the 'inlineStart' and 'inlineEnd' values. 'inlineStart'
        // moves the boxes to the end of the line, and 'inlineEnd' moves the
        // boxes to the start of the line. The computed values are always:
        // inlineStart=-inlineEnd
        let inlineStart = position.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::IStart, aWM, &anchorResolutionParams);
        let inlineEnd = position.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::IEnd, aWM, &anchorResolutionParams);
        let inlineStartIsAuto = inlineStart.IsAuto();
        let mut inlineEndIsAuto = inlineEnd.IsAuto();

        // If neither 'inlineStart' nor 'inlineEnd' is auto, then we're
        // over-constrained and we ignore one of them
        if !inlineStartIsAuto && !inlineEndIsAuto {
            inlineEndIsAuto = true;
        }

        if inlineStartIsAuto {
            if inlineEndIsAuto {
                // If both are 'auto' (their initial values), the computed values are 0
                *offsets.IStart_mut(aWM) = 0;
                *offsets.IEnd_mut(aWM) = 0;
            } else {
                // 'inlineEnd' isn't being treated as 'auto' so compute its value
                *offsets.IEnd_mut(aWM) = if inlineEnd.IsAuto() { 0 } else { nsLayoutUtils::ComputeCBDependentValue__nscoord_AnchorResolvedInset(aCBSize.ISize(aWM), &inlineEnd) };

                // Computed value for 'inlineStart' is minus the value of 'inlineEnd'
                *offsets.IStart_mut(aWM) = -offsets.IEnd(aWM);
            }
        } else {
            NS_ASSERTION!(inlineEndIsAuto, "unexpected specified constraint");

            // 'InlineStart' isn't 'auto' so compute its value
            *offsets.IStart_mut(aWM) = nsLayoutUtils::ComputeCBDependentValue__nscoord_AnchorResolvedInset(aCBSize.ISize(aWM), &inlineStart);

            // Computed value for 'inlineEnd' is minus the value of 'inlineStart'
            *offsets.IEnd_mut(aWM) = -offsets.IStart(aWM);
        }

        // Compute the 'blockStart' and 'blockEnd' values. The 'blockStart'
        // and 'blockEnd' properties move relatively positioned elements in
        // the block progression direction. They also must be each other's
        // negative
        let blockStart = position.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BStart, aWM, &anchorResolutionParams);
        let blockEnd = position.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BEnd, aWM, &anchorResolutionParams);
        let mut blockStartIsAuto = blockStart.IsAuto();
        let mut blockEndIsAuto = blockEnd.IsAuto();

        // Check for percentage based values and a containing block block-size
        // that depends on the content block-size. Treat them like 'auto'
        if NS_UNCONSTRAINEDSIZE == aCBSize.BSize(aWM) {
            if blockStart.HasPercent() {
                blockStartIsAuto = true;
            }
            if blockEnd.HasPercent() {
                blockEndIsAuto = true;
            }
        }

        // If neither is 'auto', 'block-end' is ignored
        if !blockStartIsAuto && !blockEndIsAuto {
            blockEndIsAuto = true;
        }

        if blockStartIsAuto {
            if blockEndIsAuto {
                // If both are 'auto' (their initial values), the computed values are 0
                *offsets.BStart_mut(aWM) = 0;
                *offsets.BEnd_mut(aWM) = 0;
            } else {
                // 'blockEnd' isn't being treated as 'auto' so compute its value
                *offsets.BEnd_mut(aWM) = if blockEnd.IsAuto() { 0 } else { nsLayoutUtils::ComputeCBDependentValue__nscoord_AnchorResolvedInset(aCBSize.BSize(aWM), &blockEnd) };

                // Computed value for 'blockStart' is minus the value of 'blockEnd'
                *offsets.BStart_mut(aWM) = -offsets.BEnd(aWM);
            }
        } else {
            NS_ASSERTION!(blockEndIsAuto, "unexpected specified constraint");

            // 'blockStart' isn't 'auto' so compute its value
            *offsets.BStart_mut(aWM) = nsLayoutUtils::ComputeCBDependentValue__nscoord_AnchorResolvedInset(aCBSize.BSize(aWM), &blockStart);

            // Computed value for 'blockEnd' is minus the value of 'blockStart'
            *offsets.BEnd_mut(aWM) = -offsets.BStart(aWM);
        }

        // Convert the offsets to physical coordinates and store them on the frame
        let physicalOffsets = offsets.GetPhysicalMargin(aWM);
        // Gecko: if (nsMargin* prop = GetProperty(...)) { *prop = physicalOffsets; } else { AddProperty(..., new nsMargin(physicalOffsets)); }
        aFrame.SetProperty(nsIFrame::ComputedOffsetProperty(), physicalOffsets);

        NS_ASSERTION!(offsets.IStart(aWM) == -offsets.IEnd(aWM) && offsets.BStart(aWM) == -offsets.BEnd(aWM), "ComputeRelativeOffsets should return valid results!");

        offsets
    }

    // If aFrame is a relatively or sticky positioned element, adjust aPosition
    // appropriately.
    //
    // @param aComputedOffsets aFrame's relative offset, either from the cached
    //        nsIFrame::ComputedOffsetProperty() or ComputedPhysicalOffsets().
    //        Note: This parameter is used only when aFrame is relatively
    //        positioned, not sticky positioned.
    // @param aPosition [in/out] Pass aFrame's normal position (pre-relative
    //        positioning), and this method will update it to indicate aFrame's
    //        actual position.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ApplyRelativePositioning(nsIFrame*, const nsMargin&, nsPoint*)
    pub fn ApplyRelativePositioning__nsIFrame_nsMargin_nsPoint(aFrame: nsIFrame, aComputedOffsets: nsMargin, aPosition: &mut nsPoint) {
        if !aFrame.IsRelativelyOrStickyPositioned() {
            NS_ASSERTION!(
                !aFrame.HasProperty(nsIFrame::NormalPositionProperty()),
                "We assume that changing the 'position' property causes frame reconstruction.  If that ever changes, this code should call aFrame->RemoveProperty(nsIFrame::NormalPositionProperty())"
            );
            return;
        }

        // Store the normal position
        aFrame.SetProperty(nsIFrame::NormalPositionProperty(), *aPosition);

        let display = aFrame.StyleDisplay();
        if StylePositionProperty::Relative == display.mPosition() {
            *aPosition += nsPoint::new(aComputedOffsets.left, aComputedOffsets.top);
        }
        // For sticky positioned elements, we'll leave them until the scroll container
        // reflows and calls StickyScrollContainer::UpdatePositions() to update their
        // positions.
        // NOT PORTED: sticky positioning (StickyScrollContainer).
    }

    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ApplyRelativePositioning(nsIFrame*, WritingMode, const LogicalMargin&, LogicalPoint*, const nsSize&)
    pub fn ApplyRelativePositioning__nsIFrame_WritingMode_LogicalMargin_LogicalPoint_nsSize(aFrame: nsIFrame, aWritingMode: WritingMode, aComputedOffsets: LogicalMargin, aPosition: &mut LogicalPoint, aContainerSize: nsSize) {
        // Subtract the size of the frame from the container size that we
        // use for converting between the logical and physical origins of
        // the frame. This accounts for the fact that logical origins in RTL
        // coordinate systems are at the top right of the frame instead of
        // the top left.
        let frameSize = aFrame.GetSize();
        let mut pos = aPosition.GetPhysicalPoint(aWritingMode, aContainerSize - frameSize);
        ReflowInput::ApplyRelativePositioning__nsIFrame_nsMargin_nsPoint(aFrame, aComputedOffsets.GetPhysicalMargin(aWritingMode), &mut pos);
        *aPosition = LogicalPoint::new__WritingMode_nsPoint_nsSize(aWritingMode, pos, aContainerSize - frameSize);
    }

    // Resolve any block-axis 'auto' margins (if any) for an absolutely positioned
    // frame. aMargin and aOffsets are both outparams (though we only touch
    // aOffsets if the position is overconstrained)
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ComputeAbsPosBlockAutoMargin
    pub fn ComputeAbsPosBlockAutoMargin(aAvailMarginSpace: nscoord, aContainingBlockWM: WritingMode, aIsMarginBStartAuto: bool, aIsMarginBEndAuto: bool, aMargin: &mut LogicalMargin) {
        if aIsMarginBStartAuto {
            if aIsMarginBEndAuto {
                // Both 'margin-top' and 'margin-bottom' are 'auto', so they get
                // equal values
                *aMargin.BStart_mut(aContainingBlockWM) = aAvailMarginSpace / 2;
                *aMargin.BEnd_mut(aContainingBlockWM) = aAvailMarginSpace - aMargin.BStart(aContainingBlockWM);
            } else {
                // Just margin-block-start is 'auto'
                *aMargin.BStart_mut(aContainingBlockWM) = aAvailMarginSpace;
            }
        } else {
            if aIsMarginBEndAuto {
                // Just margin-block-end is 'auto'
                *aMargin.BEnd_mut(aContainingBlockWM) = aAvailMarginSpace;
            }
            // Else, both margins are non-auto. See comment in the inline version.
        }
    }

    // Resolve any inline-axis 'auto' margins (if any) for an absolutely
    // positioned frame. aMargin and aOffsets are both outparams (though we only
    // touch aOffsets if the position is overconstrained)
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ComputeAbsPosInlineAutoMargin
    pub fn ComputeAbsPosInlineAutoMargin(aAvailMarginSpace: nscoord, aContainingBlockWM: WritingMode, aIsMarginIStartAuto: bool, aIsMarginIEndAuto: bool, aMargin: &mut LogicalMargin) {
        if aIsMarginIStartAuto {
            if aIsMarginIEndAuto {
                if aAvailMarginSpace < 0 {
                    // Note that this case is different from the neither-'auto'
                    // case below, where the spec says to ignore 'left'/'right'.
                    // Ignore the specified value for 'margin-right'.
                    *aMargin.IEnd_mut(aContainingBlockWM) = aAvailMarginSpace;
                } else {
                    // Both 'margin-left' and 'margin-right' are 'auto', so they get
                    // equal values
                    *aMargin.IStart_mut(aContainingBlockWM) = aAvailMarginSpace / 2;
                    *aMargin.IEnd_mut(aContainingBlockWM) = aAvailMarginSpace - aMargin.IStart(aContainingBlockWM);
                }
            } else {
                // Just 'margin-left' is 'auto'
                *aMargin.IStart_mut(aContainingBlockWM) = aAvailMarginSpace;
            }
        } else {
            if aIsMarginIEndAuto {
                // Just 'margin-right' is 'auto'
                *aMargin.IEnd_mut(aContainingBlockWM) = aAvailMarginSpace;
            }
            // Else, both margins are non-auto. This margin box would align to the
            // inset-reduced containing block, so it's not overconstrained.
        }
    }
}

// Gecko: layout/generic/ReflowInput.cpp MightBeContainingBlockFor
fn MightBeContainingBlockFor(aMaybeContainingBlock: nsIFrame, aFrame: nsIFrame, aStyleDisplay: &nsStyleDisplay) -> bool {
    // Keep this in sync with nsIFrame::GetContainingBlock.
    if aFrame.IsAbsolutelyPositioned(Some(aStyleDisplay)) && Some(aMaybeContainingBlock) == aFrame.GetParent() {
        return true;
    }
    aMaybeContainingBlock.IsBlockContainer()
}

impl<'a> ReflowInput<'a> {
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::InitCBReflowInput
    fn InitCBReflowInput(&mut self) {
        self.mCBReflowInput = self.mParentReflowInput;
        let Some(cbri) = self.mCBReflowInput else { return };
        if self.mParentReflowInput.unwrap().mFlags.mDummyParentReflowInput {
            return;
        }
        // To avoid a long walk up the frame tree check if the parent frame can be a
        // containing block for mFrame.
        if MightBeContainingBlockFor(cbri.mFrame, self.mFrame, &self.mStyleDisplay) && Some(cbri.mFrame) == self.mFrame.GetContainingBlock__uint32_t_nsStyleDisplay(0, &self.mStyleDisplay) {
            // Inner table frames need to use the containing block of the outer
            // table frame.
            if self.mFrame.IsTableFrame() {
                MOZ_ASSERT!(self.mParentReflowInput.unwrap().mCBReflowInput.is_some(), "Inner table frames shouldn't be reflow roots");
                self.mCBReflowInput = self.mParentReflowInput.unwrap().mCBReflowInput;
            }
        } else if self.mParentReflowInput.unwrap().mCBReflowInput.is_some() {
            self.mCBReflowInput = self.mParentReflowInput.unwrap().mCBReflowInput;
        }
    }
}

/* Check whether CalcQuirkContainingBlockHeight would stop on the
 * given reflow input, using its block as a height.  (essentially
 * returns false for any case in which CalcQuirkContainingBlockHeight
 * has a "continue" in its main loop.)
 *
 * XXX Maybe refactor CalcQuirkContainingBlockHeight so it uses
 * this function as well
 */
// Gecko: layout/generic/ReflowInput.cpp IsQuirkContainingBlockHeight
fn IsQuirkContainingBlockHeight(rs: &ReflowInput, aFrameType: LayoutFrameType) -> bool {
    if LayoutFrameType::Block == aFrameType || LayoutFrameType::ScrollContainer == aFrameType {
        // Note: This next condition could change due to a style change,
        // but that would cause a style reflow anyway, which means we're ok.
        if NS_UNCONSTRAINEDSIZE == rs.ComputedHeight() {
            if !rs.mFrame.IsAbsolutelyPositioned(Some(&rs.mStyleDisplay)) {
                return false;
            }
        }
    }
    true
}

impl<'a> ReflowInput<'a> {
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::InitResizeFlags
    fn InitResizeFlags(&mut self, aPresContext: &nsPresContext, aFrameType: LayoutFrameType) {
        self.SetIResize(false);
        self.SetBResize(false);
        self.SetBResizeForPercentages(false);

        let wm = self.mWritingMode; // just a shorthand
        // We should report that we have a resize in the inline dimension if
        // *either* the border-box size or the content-box size in that
        // dimension has changed.  It might not actually be necessary to do
        // this if the border-box size has changed and the content-box size
        // has not changed, but since we've historically used the flag to mean
        // border-box size change, continue to do that. It's possible for
        // the content-box size to change without a border-box size change or
        // a style change given (1) a fixed width (possibly fixed by max-width
        // or min-width), box-sizing:border-box, and percentage padding;
        // (2) box-sizing:content-box, M% width, and calc(Npx - M%) padding.
        //
        // However, we don't actually have the information at this point to tell
        // whether the content-box size has changed, since both style data and the
        // UsedPaddingProperty() have already been updated in
        // SizeComputationInput::InitOffsets(). So, we check the HasPaddingChange()
        // bit for the cases where it's possible for the content-box size to have
        // changed without either (a) a change in the border-box size or (b) an
        // nsChangeHint_NeedDirtyReflow change hint due to change in border or
        // padding.
        //
        // We don't clear the HasPaddingChange() bit here, since sometimes we
        // construct reflow input (e.g. in nsBlockFrame::ReflowBlockFrame to compute
        // margin collapsing) without reflowing the frame. Instead, we clear it in
        // nsIFrame::DidReflow().
        let isIResize =
            // is the border-box resizing?
            self.mFrame.ISize(wm) != self.ComputedISize() + self.ComputedLogicalBorderPadding(wm).IStartEnd(wm) ||
            // or is the content-box resizing?  (see comment above)
            self.mFrame.HasPaddingChange();

        if self.mFrame.HasAnyStateBits(NS_FRAME_FONT_INFLATION_FLOW_ROOT) && nsLayoutUtils::FontSizeInflationEnabled(aPresContext) {
            // NOT PORTED: font size inflation (ReflowInput.cpp lines 713-792): updates the
            // nsFontInflationData of the flow root and, when the effective width changed, marks the
            // subtree dirty and marks the intrinsic inline-sizes of all descendants dirty.
            // FontSizeInflationEnabled() is always false.
            unimplemented!("NOT PORTED: font size inflation");
        }

        let isDirty = self.mFrame.HasAnyStateBits(NS_FRAME_IS_DIRTY);
        self.SetIResize(!isDirty && isIResize);
        let anchorResolutionParams = AnchorPosOffsetResolutionParams::UseCBFrameSize(AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false));

        let bSize = self.mStylePosition.BSize(wm, &anchorResolutionParams.mBaseParams);
        let minBSize = self.mStylePosition.MinBSize(wm, &anchorResolutionParams.mBaseParams);
        let maxBSize = self.mStylePosition.MaxBSize(wm, &anchorResolutionParams.mBaseParams);
        // XXX Should we really need to null check mCBReflowInput?  (We do for
        // at least nsBoxFrame).
        if self.mFrame.HasBSizeChange() {
            // When we have an nsChangeHint_UpdateComputedBSize, we'll set a bit
            // on the frame to indicate we're resizing.  This might catch cases,
            // such as a change between auto and a length, where the box doesn't
            // actually resize but children with percentages resize (since those
            // percentages become auto if their containing block is auto).
            self.SetBResize(true);
            self.SetBResizeForPercentages(true);
            // We don't clear the HasBSizeChange state here, since sometimes we
            // construct a ReflowInput (e.g. in nsBlockFrame::ReflowBlockFrame to
            // compute margin collapsing) without reflowing the frame. Instead, we
            // clear it in nsIFrame::DidReflow.
        } else if self.mCBReflowInput.map_or(false, |cbri| cbri.IsBResizeForPercentagesForWM(wm)) && (bSize.HasPercent() || minBSize.HasPercent() || maxBSize.HasPercent()) {
            // We have a percentage (or calc-with-percentage) block-size, and the
            // value it's relative to has changed.
            self.SetBResize(true);
            self.SetBResizeForPercentages(true);
        } else if aFrameType == LayoutFrameType::TableCell
            && (self.mFlags.mSpecialBSizeReflow || self.mFrame.FirstInFlow().HasAnyStateBits(NS_TABLE_CELL_HAD_SPECIAL_REFLOW))
            && self.mFrame.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE)
        {
            // Need to set the bit on the cell so that
            // mCBReflowInput->IsBResize() is set correctly below when
            // reflowing descendant.
            self.SetBResize(true);
            self.SetBResizeForPercentages(true);
        } else if self.mCBReflowInput.is_some() && self.mFrame.IsBlockWrapper() {
            // XXX Is this problematic for relatively positioned inlines acting
            // as containing block for absolutely positioned elements?
            // Possibly; in that case we should at least be checking
            // IsSubtreeDirty(), I'd think.
            let cbri = self.mCBReflowInput.unwrap();
            self.SetBResize(cbri.IsBResizeForWM(wm));
            self.SetBResizeForPercentages(cbri.IsBResizeForPercentagesForWM(wm));
        } else if self.ComputedBSize() == NS_UNCONSTRAINEDSIZE {
            // We have an 'auto' block-size.
            if eCompatibility_NavQuirks == aPresContext.CompatibilityMode() && self.mCBReflowInput.is_some() {
                // FIXME: This should probably also check IsIResize().
                let resize = self.mCBReflowInput.unwrap().IsBResizeForWM(wm);
                self.SetBResize(resize);
            } else {
                let resize = self.IsIResize();
                self.SetBResize(resize);
            }
            let isSubtreeDirty = self.mFrame.HasAnyStateBits(NS_FRAME_IS_DIRTY | NS_FRAME_HAS_DIRTY_CHILDREN);
            let resize = self.IsBResize() ||
                isSubtreeDirty ||
                // For an inner table frame, copy IsBResize from its wrapper.
                (aFrameType == LayoutFrameType::Table && self.mParentReflowInput.unwrap().IsBResize());
            self.SetBResize(resize);
        } else {
            // We have a non-'auto' block-size, i.e., a length.  Set the BResize
            // flag to whether the size is actually different.
            let resize = self.mFrame.BSize(wm) != self.ComputedBSize() + self.ComputedLogicalBorderPadding(wm).BStartEnd(wm);
            self.SetBResize(resize);
        }

        let mut dependsOnCBBSize = (nsStylePosition::BSizeDependsOnContainer(&bSize) &&
            // FIXME: condition this on not-abspos?
            !bSize.IsAuto())
            || nsStylePosition::MinBSizeDependsOnContainer(&minBSize)
            || nsStylePosition::MaxBSizeDependsOnContainer(&maxBSize)
            || self.mStylePosition.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BStart, wm, &anchorResolutionParams).HasPercent()
            || !self.mStylePosition.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BEnd, wm, &anchorResolutionParams).IsAuto()
            // We assume orthogonal flows depend on the containing-block's BSize,
            // as that will commonly provide the available inline size. This is not
            // always strictly needed, but orthogonal flows are rare enough that
            // attempting to be more precise seems overly complex.
            || self.mCBReflowInput.map_or(false, |cbri| cbri.GetWritingMode().IsOrthogonalTo(&wm));

        // If mFrame is a flex item, and mFrame's block axis is the flex container's
        // main axis (e.g. in a column-oriented flex container with same
        // writing-mode), then its block-size depends on its CB size, if its
        // flex-basis has a percentage.
        if self.mFrame.IsFlexItem() && !nsFlexContainerFrame::IsItemInlineAxisMainAxis(self.mFrame) {
            let flexBasis = self.mStylePosition.mFlexBasis();
            dependsOnCBBSize |= match &flexBasis {
                StyleFlexBasis::Size(size) => size.HasPercent(),
                StyleFlexBasis::Content => false,
            };
        }

        if self.mFrame.StyleFont().mLineHeight().IsMozBlockHeight() {
            // line-height depends on block bsize
            self.mFrame.AddStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE);
            // but only on containing blocks if this frame is not a suitable block
            dependsOnCBBSize |= !nsLayoutUtils::IsNonWrapperBlock(self.mFrame);
        }

        // If we're the descendant of a table cell that performs special bsize
        // reflows and we could be the child that requires them, always set
        // the block-axis resize in case this is the first pass before the
        // special bsize reflow.  However, don't do this if it actually is
        // the special bsize reflow, since in that case it will already be
        // set correctly above if we need it set.
        if !self.IsBResize()
            && self.mCBReflowInput.map_or(false, |cbri| (cbri.mFrame.IsTableCellFrame() || cbri.mFlags.mHeightDependsOnAncestorCell) && !cbri.mFlags.mSpecialBSizeReflow)
            && dependsOnCBBSize
        {
            self.SetBResize(true);
            self.mFlags.mHeightDependsOnAncestorCell = true;
        }

        // Set NS_FRAME_CONTAINS_RELATIVE_BSIZE if it's needed.

        // It would be nice to check that |ComputedBSize != NS_UNCONSTRAINEDSIZE|
        // &&ed with the percentage bsize check.  However, this doesn't get
        // along with table special bsize reflows, since a special bsize
        // reflow (a quirk that makes such percentage height work on children
        // of table cells) can cause not just a single percentage height to
        // become fixed, but an entire descendant chain of percentage height
        // to become fixed.
        if dependsOnCBBSize && self.mCBReflowInput.is_some() {
            let mCBReflowInput = self.mCBReflowInput.unwrap();
            let mut rs: &ReflowInput = &*self;
            let mut hitCBReflowInput = false;
            loop {
                match rs.mParentReflowInput {
                    Some(p) => rs = p,
                    None => break,
                }

                if rs.mFrame.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE) {
                    break; // no need to go further
                }
                rs.mFrame.AddStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE);

                // Keep track of whether we've hit the containing block, because
                // we need to go at least that far.
                if std::ptr::eq(rs, mCBReflowInput) {
                    hitCBReflowInput = true;
                }

                // XXX What about orthogonal flows? It doesn't make sense to
                // keep propagating this bit across an orthogonal boundary,
                // where the meaning of BSize changes. Bug 1175517.

                // while (!hitCBReflowInput || (NavQuirks && !IsQuirkContainingBlockHeight(rs, rs->mFrame->Type())))
                if !(!hitCBReflowInput || (eCompatibility_NavQuirks == aPresContext.CompatibilityMode() && !IsQuirkContainingBlockHeight(rs, rs.mFrame.Type()))) {
                    break;
                }
            }
            // Note: We actually don't need to set the
            // NS_FRAME_CONTAINS_RELATIVE_BSIZE bit for the cases
            // where we hit the early break statements in
            // CalcQuirkContainingBlockHeight. But it doesn't hurt
            // us to set the bit in these cases.
        }
        if self.mFrame.HasAnyStateBits(NS_FRAME_IS_DIRTY) {
            // If we're reflowing everything, then we'll find out if we need
            // to re-set this.
            self.mFrame.RemoveStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE);
        }
    }

    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::InitDynamicReflowRoot
    fn InitDynamicReflowRoot(&mut self) {
        if self.mFrame.CanBeDynamicReflowRoot() {
            self.mFrame.AddStateBits(NS_FRAME_DYNAMIC_REFLOW_ROOT);
        } else {
            self.mFrame.RemoveStateBits(NS_FRAME_DYNAMIC_REFLOW_ROOT);
        }
    }
}

// This will not be converted to abstract coordinates because it's only
// used in CalcQuirkContainingBlockHeight
// Gecko: layout/generic/ReflowInput.cpp GetBlockMarginBorderPadding
fn GetBlockMarginBorderPadding(aReflowInput: Option<&ReflowInput>) -> nscoord {
    let mut result = 0;
    let Some(aReflowInput) = aReflowInput else { return result };

    // zero auto margins
    let mut margin = aReflowInput.ComputedPhysicalMargin();
    if NS_AUTOMARGIN == margin.top {
        margin.top = 0;
    }
    if NS_AUTOMARGIN == margin.bottom {
        margin.bottom = 0;
    }

    result += margin.top + margin.bottom;
    result += aReflowInput.ComputedPhysicalBorderPadding().top + aReflowInput.ComputedPhysicalBorderPadding().bottom;

    result
}

/* Get the height based on the viewport of the containing block specified
 * in aReflowInput when the containing block has mComputedHeight ==
 * NS_UNCONSTRAINEDSIZE This will walk up the chain of containing blocks looking
 * for a computed height until it finds the canvas frame, or it encounters a
 * frame that is not a block, area, or scroll frame. This handles compatibility
 * with IE (see bug 85016 and bug 219693)
 *
 * When we encounter scrolledContent block frames, we skip over them,
 * since they are guaranteed to not be useful for computing the containing
 * block.
 *
 * See also IsQuirkContainingBlockHeight.
 */
// Gecko: layout/generic/ReflowInput.cpp CalcQuirkContainingBlockHeight
fn CalcQuirkContainingBlockHeight(aCBReflowInput: &ReflowInput) -> nscoord {
    let mut firstAncestorRI: Option<&ReflowInput> = None; // a candidate for html frame
    let mut secondAncestorRI: Option<&ReflowInput> = None; // a candidate for body frame

    // initialize the default to NS_UNCONSTRAINEDSIZE as this is the containings
    // block computed height when this function is called. It is possible that we
    // don't alter this height especially if we are restricted to one level
    let mut result = NS_UNCONSTRAINEDSIZE;

    let mut ri: Option<&ReflowInput> = Some(aCBReflowInput);
    while let Some(r) = ri {
        // C++: `for (; ri; ri = ri->mParentReflowInput)`; a `continue` below moves to the parent.
        let next = r.mParentReflowInput;
        let frameType = r.mFrame.Type();
        // if the ancestor is auto height then skip it and continue up if it
        // is the first block frame and possibly the body/html
        if LayoutFrameType::Block == frameType || LayoutFrameType::ScrollContainer == frameType {
            secondAncestorRI = firstAncestorRI;
            firstAncestorRI = Some(r);

            // If the current frame we're looking at is positioned, we don't want to
            // go any further (see bug 221784).  The behavior we want here is: 1) If
            // not auto-height, use this as the percentage base.  2) If auto-height,
            // or treating the height as indefinite, keep looking, unless the frame is
            // positioned.
            if r.ComputedHeight() == NS_UNCONSTRAINEDSIZE || r.mFlags.mTreatBSizeAsIndefinite {
                if r.mFrame.IsAbsolutelyPositioned(Some(&r.mStyleDisplay)) {
                    break;
                } else {
                    ri = next;
                    continue;
                }
            }
        } else if LayoutFrameType::Canvas == frameType {
            // Always continue on to the height calculation
        } else if LayoutFrameType::PageContent == frameType {
            let prevInFlow = r.mFrame.GetPrevInFlow();
            // only use the page content frame for a height basis if it is the first
            // in flow
            if prevInFlow.is_some() {
                break;
            }
        } else {
            break;
        }

        // if the ancestor is the page content frame then the percent base is
        // the avail height, otherwise it is the computed height
        result = if LayoutFrameType::PageContent == frameType { r.AvailableHeight() } else { r.ComputedHeight() };
        // if unconstrained - don't sutract borders - would result in huge height
        if NS_UNCONSTRAINEDSIZE == result {
            return result;
        }

        // if we got to the canvas or page content frame, then subtract out
        // margin/border/padding for the BODY and HTML elements
        if (LayoutFrameType::Canvas == frameType) || (LayoutFrameType::PageContent == frameType) {
            result -= GetBlockMarginBorderPadding(firstAncestorRI);
            result -= GetBlockMarginBorderPadding(secondAncestorRI);

            #[cfg(debug_assertions)]
            {
                // make sure the first ancestor is the HTML and the second is the BODY
                if let Some(first) = firstAncestorRI {
                    if let Some(frameContent) = first.mFrame.GetContent() {
                        NS_ASSERTION!(frameContent.IsHTMLElement(nsGkAtoms::html), "First ancestor is not HTML");
                    }
                }
                if let Some(second) = secondAncestorRI {
                    if let Some(frameContent) = second.mFrame.GetContent() {
                        NS_ASSERTION!(frameContent.IsHTMLElement(nsGkAtoms::body), "Second ancestor is not BODY");
                    }
                }
            }
        }
        // if we got to the html frame (a block child of the canvas) ...
        else if LayoutFrameType::Block == frameType && r.mParentReflowInput.map_or(false, |p| p.mFrame.IsCanvasFrame()) {
            // ... then subtract out margin/border/padding for the BODY element
            result -= GetBlockMarginBorderPadding(secondAncestorRI);
        }
        break;
    }

    // Make sure not to return a negative height here!
    std::cmp::max(result, 0)
}

/**
 * Returns true iff a pre-order traversal of the normal child
 * frames rooted at aFrame finds no non-empty frame before aDescendant.
 */
// Gecko: layout/generic/ReflowInput.cpp AreAllEarlierInFlowFramesEmpty
fn AreAllEarlierInFlowFramesEmpty(aFrame: nsIFrame, aDescendant: nsIFrame, aFound: &mut bool) -> bool {
    if aFrame == aDescendant {
        *aFound = true;
        return true;
    }
    if aFrame.IsPlaceholderFrame() {
        let ph = nsPlaceholderFrame::from_frame(aFrame);
        MOZ_ASSERT!(ph.IsSelfEmpty() && aFrame.PrincipalChildList_FirstChild().is_none());
        ph.SetLineIsEmptySoFar(true);
    } else {
        if !aFrame.IsSelfEmpty() {
            *aFound = false;
            return false;
        }
        let mut child = aFrame.PrincipalChildList_FirstChild();
        while let Some(f) = child {
            child = f.GetNextSibling();
            let allEmpty = AreAllEarlierInFlowFramesEmpty(f, aDescendant, aFound);
            if *aFound || !allEmpty {
                return allEmpty;
            }
        }
    }
    *aFound = false;
    true
}

impl<'a> ReflowInput<'a> {
    // Compute the content-box size of the containing block frame in mFrame's
    // writing-mode (mWritingMode).
    //
    // Note: the block-size in the return value may be unconstrained.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ComputeContainingBlockRectangle
    fn ComputeContainingBlockRectangle(&self, aPresContext: &nsPresContext, aContainingBlockRI: &ReflowInput) -> LogicalSize {
        let mut cbSize = aContainingBlockRI.ComputedSize__();
        let wm = aContainingBlockRI.GetWritingMode();

        if aContainingBlockRI.mFlags.mTreatBSizeAsIndefinite {
            *cbSize.BSize_mut(wm) = NS_UNCONSTRAINEDSIZE;
        } else if let Some(percentageBasis) = aContainingBlockRI.mPercentageBasisInBlockAxis {
            MOZ_ASSERT!(cbSize.BSize(wm) == NS_UNCONSTRAINEDSIZE, "Why provide a percentage basis when the containing block's block-size is definite?");
            *cbSize.BSize_mut(wm) = percentageBasis;
        }

        let IsQuirky = |aSize: &StyleSize| -> bool { aSize.ConvertsToPercentage() || aSize.BehavesLikeStretchOnBlockAxis() };
        let anchorResolutionParams = AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false);
        // In quirks mode, if an element has a percent height (or a 'stretch' height,
        // which is kinda like a special version of 100%), then it gets its
        // containing block by looking for an ancestor with a non-auto height.
        // Note: We don't emulate this quirk for percents in calc(), or in vertical
        // writing modes, or if the containing block is a flex or grid item.
        if !wm.IsVertical() && NS_UNCONSTRAINEDSIZE == cbSize.BSize(wm) {
            if eCompatibility_NavQuirks == aPresContext.CompatibilityMode()
                && !aContainingBlockRI.mFrame.IsFlexOrGridItem()
                && (IsQuirky(&self.mStylePosition.GetHeight(&anchorResolutionParams))
                    || (self.mFrame.IsTableWrapperFrame() && IsQuirky(&self.mFrame.PrincipalChildList_FirstChild().unwrap().StylePosition().GetHeight(&anchorResolutionParams))))
            {
                *cbSize.BSize_mut(wm) = CalcQuirkContainingBlockHeight(aContainingBlockRI);
            }
        }

        cbSize.ConvertTo(self.GetWritingMode(), wm)
    }

    // Returns the nearest containing block for aFrame. Also returns its border &
    // padding and content-box size. These are returned in the coordinate space of
    // the containing block.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::GetHypotheticalBoxContainer
    fn GetHypotheticalBoxContainer(&self, aFrame: nsIFrame) -> ReflowInput_HypotheticalBoxContainerInfo {
        let cb = aFrame.GetContainingBlock__uint32_t(0).unwrap();
        NS_ASSERTION!(cb != self.mFrame, "How did that happen?");

        // If cb is currently being reflowed, find its ReflowInput.
        let mut ri: Option<&ReflowInput> = None;
        if cb.HasAnyStateBits(NS_FRAME_IN_REFLOW) {
            ri = self.mParentReflowInput.map(|p| p as &ReflowInput);
            while let Some(r) = ri {
                if r.mFrame == cb {
                    break;
                }
                ri = r.mParentReflowInput.map(|p| p as &ReflowInput);
            }
        }

        let wm = cb.GetWritingMode();
        if let Some(ri) = ri {
            return ReflowInput_HypotheticalBoxContainerInfo { mBoxContainer: cb, mBorderPadding: ri.ComputedLogicalBorderPadding(wm), mContentBoxSize: ri.ComputedSize__WritingMode(wm) };
        }

        // Didn't find a ReflowInput for cb. Just compute the information we want, on
        // the assumption that cb already knows its size. This really ought to be true
        // by now.
        NS_ASSERTION!(!cb.HasAnyStateBits(NS_FRAME_IN_REFLOW), "cb shouldn't be in reflow; we'll lie if it is");
        ReflowInput_HypotheticalBoxContainerInfo { mBoxContainer: cb, mBorderPadding: cb.GetLogicalUsedBorderAndPadding(wm), mContentBoxSize: cb.ContentSize(wm) }
    }

    // Calculate the position of the hypothetical box that the placeholder frame
    // (for a position:fixed/absolute element) would have if it were in the flow
    // (i.e., positioned statically).
    //
    // The position of the hypothetical box is relative to the padding edge of the
    // absolute containing block (aCBReflowInput->mFrame). The writing mode of the
    // hypothetical box will have the same block direction as the absolute
    // containing block, but it may differ in the inline direction.
    //
    // @param aCBPaddingBoxSize the padding-box size of the absolute containing
    // block, in its own writing-mode.
    //
    // In the code below, |aCBReflowInput->mFrame| is the absolute containing block,
    // while |blockContainer| is the nearest block container of the placeholder
    // frame, which may be different from the absolute containing block.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::CalculateHypotheticalPosition
    // NOT PORTED: the scrollbar sizes of a viewport containing block (there is no scroll container).
    fn CalculateHypotheticalPosition(&self, aPlaceholderFrame: nsIFrame, aCBReflowInput: &ReflowInput, aCBPaddingBoxSize: LogicalSize, aHypotheticalPos: &mut nsHypotheticalPosition) {
        let cbwm = aCBReflowInput.GetWritingMode();
        let ReflowInput_HypotheticalBoxContainerInfo { mBoxContainer: blockContainer, mBorderPadding: blockContainerBP, mContentBoxSize: blockContainerContentBoxSize } = self.GetHypotheticalBoxContainer(aPlaceholderFrame);
        let wm = blockContainer.GetWritingMode();
        let blockContainerContentIStart = blockContainerBP.IStart(wm);

        let anchorResolutionParams = AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false);
        let styleISize = self.mStylePosition.ISize(wm, &anchorResolutionParams);
        let isAutoISize = styleISize.IsAuto();

        // If it's a replaced element and it has a 'auto' value for 'inline size', see
        // if we can get the intrinsic size. This will allow us to exactly determine
        // both the inline edges.
        let mut intrinsicSize: Option<nsSize> = None;
        if self.mFlags.mIsReplaced && isAutoISize {
            intrinsicSize = self.mFrame.GetIntrinsicSize().ToSize();
        }

        // See if we can calculate what the box inline size would have been if
        // the element had been in the flow
        let mut boxISize: Option<nscoord> = None;
        if self.mStyleDisplay.IsOriginalDisplayInlineOutside() && !self.mFlags.mIsReplaced {
            // For non-replaced inline-level elements the 'inline size' property
            // doesn't apply, so we don't know what the inline size would have
            // been without reflowing it
        } else {
            // It's either a replaced inline-level element or a block-level element

            // Determine the total amount of inline direction
            // border/padding/margin that the element would have had if it had
            // been in the flow. Note that we ignore any 'auto' and 'inherit'
            // values
            let mut contentEdgeToBoxSizingISize: nscoord = 0;
            let mut boxSizingToMarginEdgeISize: nscoord = 0;
            self.CalculateBorderPaddingMargin(LogicalAxis::Inline, blockContainerContentBoxSize.ISize(wm), &mut contentEdgeToBoxSizingISize, &mut boxSizingToMarginEdgeISize);

            if self.mFlags.mIsReplaced && isAutoISize {
                // It's a replaced element with an 'auto' inline size so the box inline
                // size is its intrinsic size plus any border/padding/margin
                if let Some(intrinsicSize) = intrinsicSize {
                    boxISize = Some(LogicalSize::new__WritingMode_nsSize(wm, intrinsicSize).ISize(wm) + contentEdgeToBoxSizingISize + boxSizingToMarginEdgeISize);
                }
            } else if isAutoISize {
                // The box inline size is the block container's inline size
                boxISize = Some(blockContainerContentBoxSize.ISize(wm));
            } else {
                // We need to compute it. It's important we do this, because if it's
                // percentage based this computed value may be different from the computed
                // value calculated using the absolute containing block width
                let mut contentEdgeToBoxSizingBSize: nscoord = 0;
                let mut dummy: nscoord = 0;
                self.CalculateBorderPaddingMargin(LogicalAxis::Block, blockContainerContentBoxSize.ISize(wm), &mut contentEdgeToBoxSizingBSize, &mut dummy);

                let contentISize = self
                    .mFrame
                    .ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_SizeOrMaxSize_StyleSize_AspectRatio_ComputeSizeFlags(
                        self.mRenderingContext,
                        wm,
                        blockContainerContentBoxSize,
                        LogicalSize::new(wm, contentEdgeToBoxSizingISize, contentEdgeToBoxSizingBSize),
                        boxSizingToMarginEdgeISize,
                        &styleISize,
                        &self.mStylePosition.BSize(wm, &anchorResolutionParams),
                        self.mFrame.GetAspectRatio(),
                        ComputeSizeFlags::default(),
                    )
                    .mISize;
                boxISize = Some(contentISize + contentEdgeToBoxSizingISize + boxSizingToMarginEdgeISize);
            }
        }

        // Get the placeholder offset in the coordinate space of its block container.
        // XXXbz the placeholder is not fully reflowed yet if our containing block is
        // relatively positioned...
        let blockContainerSize = BorderBoxSizeAsContainerIfConstrained(wm, blockContainerContentBoxSize, blockContainerBP);
        let placeholderOffset = LogicalPoint::new__WritingMode_nsPoint_nsSize(wm, aPlaceholderFrame.GetOffsetToIgnoringScrolling(blockContainer), blockContainerSize);

        // First, determine the hypothetical box's mBStart.  We want to check the
        // content insertion frame of blockContainer for block-ness, but make
        // sure to compute all coordinates in the coordinate system of
        // blockContainer.
        let blockFrame = blockContainer.GetContentInsertionFrame().filter(|f| f.IsBlockFrameOrSubclass()).map(nsBlockFrame::from_frame);
        if let Some(blockFrame) = blockFrame {
            // Use a null containerSize to convert a LogicalPoint functioning as a
            // vector into a physical nsPoint vector.
            let nullContainerSize = nsSize::default();
            let blockOffset = LogicalPoint::new__WritingMode_nsPoint_nsSize(wm, blockFrame.GetOffsetToIgnoringScrolling(blockContainer), nullContainerSize);
            let mut isValid = false;
            let iter = nsBlockInFlowLineIterator::new__nsBlockFrame_nsIFrame_bool(blockFrame, aPlaceholderFrame, &mut isValid);
            if !isValid {
                // Give up.  We're probably dealing with somebody using
                // position:absolute inside native-anonymous content anyway.
                aHypotheticalPos.mBStart = placeholderOffset.B(wm);
            } else {
                NS_ASSERTION!(iter.GetContainer() == blockFrame, "Found placeholder in wrong block!");
                let lineBox = iter.GetLine();

                // How we determine the hypothetical box depends on whether the element
                // would have been inline-level or block-level
                let lineBounds = lineBox.GetBounds().ConvertTo(wm, lineBox.mWritingMode, lineBox.mContainerSize);
                if self.mStyleDisplay.IsOriginalDisplayInlineOutside() {
                    // Use the block-start of the inline box which the placeholder lives in
                    // as the hypothetical box's block-start.
                    aHypotheticalPos.mBStart = lineBounds.BStart(wm) + blockOffset.B(wm);
                } else {
                    // The element would have been block-level which means it would
                    // be below the line containing the placeholder frame, unless
                    // all the frames before it are empty.  In that case, it would
                    // have been just before this line.
                    // XXXbz the line box is not fully reflowed yet if our
                    // containing block is relatively positioned...
                    if lineBox != iter.End() {
                        let mut firstFrame = lineBox.mFirstChild;
                        let mut allEmpty = false;
                        let placeholder = nsPlaceholderFrame::from_frame(aPlaceholderFrame);
                        if firstFrame == Some(aPlaceholderFrame) {
                            placeholder.SetLineIsEmptySoFar(true);
                            allEmpty = true;
                        } else {
                            let prev = aPlaceholderFrame.GetPrevSibling();
                            if let Some(prev) = prev {
                                if prev.IsPlaceholderFrame() {
                                    let ph = nsPlaceholderFrame::from_frame(prev);
                                    if ph.GetLineIsEmptySoFar(&mut allEmpty) {
                                        placeholder.SetLineIsEmptySoFar(allEmpty);
                                    }
                                }
                            }
                        }
                        if !allEmpty {
                            let mut found = false;
                            while let Some(f) = firstFrame {
                                // See bug 223064
                                allEmpty = AreAllEarlierInFlowFramesEmpty(f, aPlaceholderFrame, &mut found);
                                if found || !allEmpty {
                                    break;
                                }
                                firstFrame = f.GetNextSibling();
                            }
                            placeholder.SetLineIsEmptySoFar(allEmpty);
                        }
                        NS_ASSERTION!(firstFrame.is_some(), "Couldn't find placeholder!");

                        if allEmpty {
                            // The top of the hypothetical box is the top of the line
                            // containing the placeholder, since there is nothing in the
                            // line before our placeholder except empty frames.
                            aHypotheticalPos.mBStart = lineBounds.BStart(wm) + blockOffset.B(wm);
                        } else {
                            // The top of the hypothetical box is just below the line
                            // containing the placeholder.
                            aHypotheticalPos.mBStart = lineBounds.BEnd(wm) + blockOffset.B(wm);
                        }
                    } else {
                        // Just use the placeholder's block-offset wrt the containing block
                        aHypotheticalPos.mBStart = placeholderOffset.B(wm);
                    }
                }
            }
        } else {
            // blockContainer is not a block, so it's probably something like a XUL box,
            // etc. Just use the placeholder's block-offset
            aHypotheticalPos.mBStart = placeholderOffset.B(wm);
        }

        // Second, determine the hypothetical box's mIStart.
        // How we determine the hypothetical box depends on whether the element
        // would have been inline-level or block-level
        if self.mStyleDisplay.IsOriginalDisplayInlineOutside() || self.mFlags.mIOffsetsNeedCSSAlign {
            // The placeholder represents the IStart edge of the hypothetical box.
            // (Or if mFlags.mIOffsetsNeedCSSAlign is set, it represents the IStart
            // edge of the Alignment Container.)
            aHypotheticalPos.mIStart = placeholderOffset.I(wm);
        } else {
            aHypotheticalPos.mIStart = blockContainerContentIStart;
        }

        // The current coordinate space is that of the nearest block to the
        // placeholder. Convert to the coordinate space of the absolute containing
        // block.
        let cbFrame = aCBReflowInput.mFrame;
        let cbOffset = blockContainer.GetOffsetToIgnoringScrolling(cbFrame);
        let cbSize: nsSize;
        if cbFrame.Type() == LayoutFrameType::Viewport {
            // When the containing block is the ViewportFrame, i.e. we are calculating
            // the static position for a fixed-positioned frame, we need to adjust the
            // origin to exclude the scrollbar or scrollbar-gutter area. The
            // ViewportFrame's containing block rect is adjusted in
            // AbsoluteContainingBlock::ReflowAbsoluteFrame(), and it will add the
            // rect's origin to the fixed-positioned frame's final position if needed.
            //
            // Note: The origin of the containing block rect is adjusted in
            // ViewportFrame::AdjustReflowInputForScrollbars(). Ensure the code there
            // remains in sync with the logic here.
            // (The ScrollContainerFrame child and its scrollbar sizes: NOT PORTED, there is none.)

            // ViewportFrame has no border or padding, so the padding-box size is equal
            // to the border-box size (cbSize) that we are computing.
            cbSize = aCBPaddingBoxSize.GetPhysicalSize(cbwm);
        } else {
            cbSize = aCBReflowInput.ComputedSizeAsContainerIfConstrained();
        }

        let logCBOffs = LogicalPoint::new__WritingMode_nsPoint_nsSize(wm, cbOffset, cbSize - blockContainerSize);
        aHypotheticalPos.mIStart += logCBOffs.I(wm);
        aHypotheticalPos.mBStart += logCBOffs.B(wm);

        // If block direction doesn't match (whether orthogonal or antiparallel),
        // we'll have to convert aHypotheticalPos to be in terms of cbwm.
        // This upcoming conversion must be taken into account for border offsets.
        let hypotheticalPosWillUseCbwm = cbwm.GetBlockDir() != wm.GetBlockDir();
        // The specified offsets are relative to the absolute containing block's
        // padding edge and our current values are relative to the border edge, so
        // translate.
        let border = aCBReflowInput.ComputedLogicalBorder(wm);
        if hypotheticalPosWillUseCbwm && !wm.ParallelAxisStartsOnSameSide(LogicalAxis::Inline, &cbwm) {
            aHypotheticalPos.mIStart += border.IEnd(wm);
        } else {
            aHypotheticalPos.mIStart -= border.IStart(wm);
        }

        if hypotheticalPosWillUseCbwm && !wm.ParallelAxisStartsOnSameSide(LogicalAxis::Block, &cbwm) {
            aHypotheticalPos.mBStart += border.BEnd(wm);
        } else {
            aHypotheticalPos.mBStart -= border.BStart(wm);
        }
        // At this point, we have computed aHypotheticalPos using the writing mode
        // of the placeholder's block container.

        if hypotheticalPosWillUseCbwm {
            // If the block direction we used in calculating aHypotheticalPos does not
            // match the absolute containing block's, we need to convert here so that
            // aHypotheticalPos is usable in relation to the absolute containing block.
            // This requires computing or measuring the abspos frame's block-size,
            // which is not otherwise required/used here (as aHypotheticalPos
            // records only the block-start coordinate).

            // This is similar to the inline-size calculation for a replaced
            // inline-level element or a block-level element (above), except that
            // 'auto' sizing is handled differently in the block direction for non-
            // replaced elements and replaced elements lacking an intrinsic size.

            // Determine the total amount of block direction
            // border/padding/margin that the element would have had if it had
            // been in the flow. Note that we ignore any 'auto' and 'inherit'
            // values.
            let mut insideBoxSizing: nscoord = 0;
            let mut outsideBoxSizing: nscoord = 0;
            self.CalculateBorderPaddingMargin(LogicalAxis::Block, blockContainerContentBoxSize.BSize(wm), &mut insideBoxSizing, &mut outsideBoxSizing);

            let boxBSize: nscoord;
            let styleBSize = self.mStylePosition.BSize(wm, &anchorResolutionParams);
            let isAutoBSize = nsLayoutUtils::IsAutoBSize(&styleBSize, blockContainerContentBoxSize.BSize(wm));
            if isAutoBSize {
                if self.mFlags.mIsReplaced && intrinsicSize.is_some() {
                    // It's a replaced element with an 'auto' block size so the box
                    // block size is its intrinsic size plus any border/padding/margin
                    boxBSize = LogicalSize::new__WritingMode_nsSize(wm, intrinsicSize.unwrap()).BSize(wm) + outsideBoxSizing + insideBoxSizing;
                } else {
                    // XXX Bug 1191801
                    // Figure out how to get the correct boxBSize here (need to reflow the
                    // positioned frame?)
                    boxBSize = 0;
                }
            } else if styleBSize.BehavesLikeStretchOnBlockAxis() {
                MOZ_ASSERT!(blockContainerContentBoxSize.BSize(wm) != NS_UNCONSTRAINEDSIZE, "If we're 'stretch' with unconstrained size, isAutoBSize should be true which should make us skip this code");
                // TODO(dholbert) The 'insideBoxSizing' and 'outsideBoxSizing' usages
                // here aren't quite right, because we're supposed to be passing margin
                // and borderPadding specifically.  The arithmetic seems to work out in
                // testcases though.
                boxBSize = nsLayoutUtils::ComputeStretchContentBoxBSize(blockContainerContentBoxSize.BSize(wm), outsideBoxSizing, insideBoxSizing);
            } else {
                // We need to compute it. It's important we do this, because if it's
                // percentage-based this computed value may be different from the
                // computed value calculated using the absolute containing block height.
                boxBSize = nsLayoutUtils::ComputeBSizeValue(blockContainerContentBoxSize.BSize(wm), insideBoxSizing, styleBSize.AsLengthPercentage()) + insideBoxSizing + outsideBoxSizing;
            }

            let boxSize = LogicalSize::new(wm, boxISize.unwrap_or(0), boxBSize);

            let mut origin = LogicalPoint::new(wm, aHypotheticalPos.mIStart, aHypotheticalPos.mBStart);
            origin = origin.ConvertRectOriginTo(cbwm, wm, boxSize.GetPhysicalSize(wm), cbSize);

            aHypotheticalPos.mIStart = origin.I(cbwm);
            aHypotheticalPos.mBStart = origin.B(cbwm);
            aHypotheticalPos.mWritingMode = cbwm;
        } else {
            aHypotheticalPos.mWritingMode = wm;
        }
    }

    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::InitAbsoluteConstraints
    // NOT PORTED: anchor positioning (the insets are the style's, there is no anchor() to resolve),
    // masonry (the NS_STATE_GRID_IS_*_MASONRY branch).
    fn InitAbsoluteConstraints(&mut self, aCBReflowInput: &ReflowInput, aCBSize: LogicalSize) {
        let wm = self.GetWritingMode();
        let cbwm = aCBReflowInput.GetWritingMode();
        NS_WARNING_ASSERTION!(aCBSize.BSize(cbwm) != NS_UNCONSTRAINEDSIZE, "containing block bsize must be constrained");

        NS_ASSERTION!(!self.mFrame.IsTableFrame(), "InitAbsoluteConstraints should not be called on table frames");
        MOZ_ASSERT!(self.mFrame.IsAbsolutelyPositioned(Some(&self.mStyleDisplay)), "InitAbsoluteConstraints should be called on abspos or fixedpos frames!");

        let anchorResolutionParams = AnchorPosOffsetResolutionParams::ExplicitCBFrameSize(AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false), Some(aCBSize));
        let iStartOffset = self.mStylePosition.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::IStart, cbwm, &anchorResolutionParams);
        let iEndOffset = self.mStylePosition.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::IEnd, cbwm, &anchorResolutionParams);
        let bStartOffset = self.mStylePosition.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BStart, cbwm, &anchorResolutionParams);
        let bEndOffset = self.mStylePosition.GetAnchorResolvedInset__LogicalSide_WritingMode_AnchorPosOffsetResolutionParams(LogicalSide::BEnd, cbwm, &anchorResolutionParams);
        let mut iStartIsAuto = iStartOffset.IsAuto();
        let mut iEndIsAuto = iEndOffset.IsAuto();
        let mut bStartIsAuto = bStartOffset.IsAuto();
        let bEndIsAuto = bEndOffset.IsAuto();

        // If both 'inline-start' and 'inline-end' are 'auto' or both 'block-start'
        // and 'block-end' are 'auto', then compute the hypothetical box position
        // where the element would have if it were in the flow.
        let mut hypotheticalPos = nsHypotheticalPosition { mIStart: 0, mBStart: 0, mWritingMode: WritingMode::default() };
        if (iStartIsAuto && iEndIsAuto) || (bStartIsAuto && bEndIsAuto) {
            let placeholderFrame = self.mFrame.GetPlaceholderFrame();
            MOZ_ASSERT!(placeholderFrame.is_some(), "no placeholder frame");
            let placeholderFrame = placeholderFrame.unwrap();
            let placeholderParent = placeholderFrame.GetParent();
            MOZ_ASSERT!(placeholderParent.is_some(), "shouldn't have unparented placeholders");
            let placeholderParent = placeholderParent.unwrap();

            if placeholderFrame.HasAnyStateBits(PLACEHOLDER_STATICPOS_NEEDS_CSSALIGN) {
                MOZ_ASSERT!(placeholderParent.IsFlexOrGridContainer(), "This flag should only be set on grid/flex children");
                // If the (as-yet unknown) static position will determine the inline
                // and/or block offsets, set flags to note those offsets aren't valid
                // until we can do CSS Box Alignment on the OOF frame.
                self.mFlags.mIOffsetsNeedCSSAlign = iStartIsAuto && iEndIsAuto;
                self.mFlags.mBOffsetsNeedCSSAlign = bStartIsAuto && bEndIsAuto;
            }

            if self.mFlags.mStaticPosIsCBOrigin {
                hypotheticalPos.mWritingMode = cbwm;
                // (The masonry placeholder position: NOT PORTED, masonry.)
            } else {
                // XXXmats all this is broken for orthogonal writing-modes: bug 1521988.
                self.CalculateHypotheticalPosition(placeholderFrame, aCBReflowInput, aCBSize, &mut hypotheticalPos);
                if aCBReflowInput.mFrame.IsGridContainerFrame() {
                    // 'hypotheticalPos' is relative to the padding rect of the CB *frame*.
                    // In grid layout the CB is the grid area rectangle, so we translate
                    // 'hypotheticalPos' to be relative that rectangle here.
                    let cb = nsGridContainerFrame::GridItemCB(self.mFrame);
                    let mut left: nscoord = 0;
                    let mut right: nscoord = 0;
                    if cbwm.IsBidiLTR() {
                        left = cb.X();
                    } else {
                        right = aCBReflowInput.ComputedWidth() + aCBReflowInput.ComputedPhysicalPadding().LeftRight() - cb.XMost();
                    }
                    let offsets = LogicalMargin::new__WritingMode_nsMargin(cbwm, nsMargin::new(cb.Y(), right, 0, left));
                    hypotheticalPos.mIStart -= offsets.IStart(cbwm);
                    hypotheticalPos.mBStart -= offsets.BStart(cbwm);
                }
            }
        }

        let mut offsets = LogicalMargin::new__WritingMode(cbwm);

        // Handle auto inset values, as per [1].
        // Technically superceded by a new section [2], but none of the browsers seem
        // to follow this behaviour.
        //
        // [1] https://drafts.csswg.org/css-position-3/#abspos-old
        // [2] https://drafts.csswg.org/css-position-3/#resolving-insets
        if iStartIsAuto {
            *offsets.IStart_mut(cbwm) = 0;
        } else {
            *offsets.IStart_mut(cbwm) = nsLayoutUtils::ComputeCBDependentValue__nscoord_AnchorResolvedInset(aCBSize.ISize(cbwm), &iStartOffset);
        }
        if iEndIsAuto {
            *offsets.IEnd_mut(cbwm) = 0;
        } else {
            *offsets.IEnd_mut(cbwm) = nsLayoutUtils::ComputeCBDependentValue__nscoord_AnchorResolvedInset(aCBSize.ISize(cbwm), &iEndOffset);
        }

        if iStartIsAuto && iEndIsAuto {
            if cbwm.IsInlineReversed() != hypotheticalPos.mWritingMode.IsInlineReversed() {
                *offsets.IEnd_mut(cbwm) = hypotheticalPos.mIStart;
                iEndIsAuto = false;
            } else {
                *offsets.IStart_mut(cbwm) = hypotheticalPos.mIStart;
                iStartIsAuto = false;
            }
        }

        if bStartIsAuto {
            *offsets.BStart_mut(cbwm) = 0;
        } else {
            *offsets.BStart_mut(cbwm) = nsLayoutUtils::ComputeCBDependentValue__nscoord_AnchorResolvedInset(aCBSize.BSize(cbwm), &bStartOffset);
        }
        if bEndIsAuto {
            *offsets.BEnd_mut(cbwm) = 0;
        } else {
            *offsets.BEnd_mut(cbwm) = nsLayoutUtils::ComputeCBDependentValue__nscoord_AnchorResolvedInset(aCBSize.BSize(cbwm), &bEndOffset);
        }

        if bStartIsAuto && bEndIsAuto {
            // Treat 'top' like 'static-position'
            *offsets.BStart_mut(cbwm) = hypotheticalPos.mBStart;
            bStartIsAuto = false;
        }

        self.SetComputedLogicalOffsets(cbwm, offsets);
        let isOrthogonal = wm.IsOrthogonalTo(&cbwm);
        if isOrthogonal {
            if bStartIsAuto || bEndIsAuto {
                self.mComputeSizeFlags += ComputeSizeFlag::ShrinkWrap;
            }
        } else if iStartIsAuto || iEndIsAuto {
            self.mComputeSizeFlags += ComputeSizeFlag::ShrinkWrap;
        }

        {
            // NOT PORTED: AutoMaybeDisableFontInflation (font size inflation is off on desktop).

            let size = self.mFrame.ComputeSize(
                &self.mSizeComputationInput,
                wm,
                aCBSize.ConvertTo(wm, cbwm),
                aCBSize.ConvertTo(wm, cbwm).ISize(wm), // XXX or AvailableISize()?
                self.ComputedLogicalMargin(wm).Size(wm) + self.ComputedLogicalOffsets(wm).Size(wm),
                self.ComputedLogicalBorderPadding(wm).Size(wm),
                &StyleSizeOverrides::default(),
                self.mComputeSizeFlags,
            );
            self.mComputedSize = size.mLogicalSize;
            NS_ASSERTION!(self.ComputedISize() >= 0, "Bogus inline-size");
            NS_ASSERTION!(self.ComputedBSize() == NS_UNCONSTRAINEDSIZE || self.ComputedBSize() >= 0, "Bogus block-size");

            self.mFlags.mIsBSizeSetByAspectRatio = size.mAspectRatioUsage == nsIFrame_AspectRatioUsage::ToComputeBSize;
        }

        let margin = self.ComputedLogicalMargin(cbwm);
        let borderPadding = self.ComputedLogicalBorderPadding(cbwm);
        let computedSize = self.mComputedSize.ConvertTo(cbwm, wm);

        let iSizeIsAuto = self.mStylePosition.ISize(cbwm, &anchorResolutionParams.mBaseParams).IsAuto();
        if iStartIsAuto {
            // We know 'inset-inline-end' is not 'auto' anymore thanks to the
            // hypothetical box code above. Solve for 'inset-inline-start'.
            if iSizeIsAuto {
                *offsets.IStart_mut(cbwm) = NS_AUTOOFFSET;
            } else {
                *offsets.IStart_mut(cbwm) = aCBSize.ISize(cbwm) - offsets.IEnd(cbwm) - computedSize.ISize(cbwm) - margin.IStartEnd(cbwm) - borderPadding.IStartEnd(cbwm);
            }
        } else if iEndIsAuto {
            // We know 'inset-inline-start' is not 'auto' anymore thanks to the
            // hypothetical box code above. Solve for 'inset-inline-end'.
            if iSizeIsAuto {
                *offsets.IEnd_mut(cbwm) = NS_AUTOOFFSET;
            } else {
                *offsets.IEnd_mut(cbwm) = aCBSize.ISize(cbwm) - offsets.IStart(cbwm) - computedSize.ISize(cbwm) - margin.IStartEnd(cbwm) - borderPadding.IStartEnd(cbwm);
            }
        }

        let bSizeIsAuto = self.mStylePosition.BSize(cbwm, &anchorResolutionParams.mBaseParams).BehavesLikeInitialValueOnBlockAxis();
        if bStartIsAuto {
            // Solve for 'inset-block-start'.
            if bSizeIsAuto {
                *offsets.BStart_mut(cbwm) = NS_AUTOOFFSET;
            } else {
                *offsets.BStart_mut(cbwm) = aCBSize.BSize(cbwm) - margin.BStartEnd(cbwm) - borderPadding.BStartEnd(cbwm) - computedSize.BSize(cbwm) - offsets.BEnd(cbwm);
            }
        } else if bEndIsAuto {
            // Solve for 'inset-block-end'.
            if bSizeIsAuto {
                *offsets.BEnd_mut(cbwm) = NS_AUTOOFFSET;
            } else {
                *offsets.BEnd_mut(cbwm) = aCBSize.BSize(cbwm) - margin.BStartEnd(cbwm) - borderPadding.BStartEnd(cbwm) - computedSize.BSize(cbwm) - offsets.BStart(cbwm);
            }
        }

        self.SetComputedLogicalOffsets(cbwm, offsets);
    }
}

// XXX refactor this code to have methods for each set of properties
// we are computing: width,height,line-height; margin; offsets

impl<'a> ReflowInput<'a> {
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::InitConstraints
    fn InitConstraints(&mut self, aPresContext: &nsPresContext, aContainingBlockSize: Option<LogicalSize>, aBorder: Option<LogicalMargin>, aPadding: Option<LogicalMargin>, aFrameType: LayoutFrameType) {
        let wm = self.GetWritingMode();
        let mut cbSize = aContainingBlockSize.unwrap_or(LogicalSize::new(self.mWritingMode, NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE));

        // If this is a reflow root, then set the computed width and
        // height equal to the available space
        if self.mParentReflowInput.is_none() || self.mFlags.mDummyParentReflowInput {
            // XXXldb This doesn't mean what it used to!
            let display = self.mStyleDisplay.clone();
            let computeSizeFlags = self.mComputeSizeFlags;
            self.InitOffsets(wm, cbSize.ISize(wm), aFrameType, computeSizeFlags, aBorder, aPadding, Some(&display));
            // Override mComputedMargin since reflow roots start from the
            // frame's boundary, which is inside the margin.
            self.SetComputedLogicalMargin(wm, LogicalMargin::new__WritingMode(wm));
            self.SetComputedLogicalOffsets(wm, LogicalMargin::new__WritingMode(wm));

            let borderPadding = self.ComputedLogicalBorderPadding(wm);
            let availableISize = self.AvailableISize();
            self.SetComputedISize(std::cmp::max(0, availableISize - borderPadding.IStartEnd(wm)), ReflowInput_ResetResizeFlags::No);
            let availableBSize = self.AvailableBSize();
            self.SetComputedBSize(if availableBSize != NS_UNCONSTRAINEDSIZE { std::cmp::max(0, availableBSize - borderPadding.BStartEnd(wm)) } else { NS_UNCONSTRAINEDSIZE }, ReflowInput_ResetResizeFlags::No);

            let mWritingMode = self.mWritingMode;
            self.mComputedMinSize.SizeTo(mWritingMode, 0, 0);
            self.mComputedMaxSize.SizeTo(mWritingMode, NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE);
        } else {
            // Get the containing block's reflow input
            let cbri = self.mCBReflowInput;
            MOZ_ASSERT!(cbri.is_some(), "no containing block");
            let cbri = cbri.unwrap();
            MOZ_ASSERT!(self.mFrame.GetParent().is_some());

            // If we weren't given a containing block size, then compute one.
            if aContainingBlockSize.is_none() {
                cbSize = self.ComputeContainingBlockRectangle(aPresContext, cbri);
            }

            // See if the containing block height is based on the size of its
            // content
            if NS_UNCONSTRAINEDSIZE == cbSize.BSize(wm) {
                // See if the containing block is a cell frame which needs
                // to use the mComputedHeight of the cell instead of what the cell block
                // passed in.
                // XXX It seems like this could lead to bugs with min-height and friends
                if cbri.mParentReflowInput.is_some() && cbri.mFrame.IsTableCellFrame() {
                    *cbSize.BSize_mut(wm) = cbri.ComputedSize__WritingMode(wm).BSize(wm);
                }
            }

            // XXX Might need to also pass the CB height (not width) for page boxes,
            // too, if we implement them.

            // For calculating positioning offsets, margins, borders and
            // padding, we use the writing mode of the containing block
            let cbwm = cbri.GetWritingMode();
            let display = self.mStyleDisplay.clone();
            let computeSizeFlags = self.mComputeSizeFlags;
            self.InitOffsets(cbwm, cbSize.ConvertTo(cbwm, wm).ISize(cbwm), aFrameType, computeSizeFlags, aBorder, aPadding, Some(&display));

            // For calculating the size of this box, we use its own writing mode
            let mut blockSize = self.mStylePosition.BSize(wm, &AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false));
            if blockSize.BehavesLikeStretchOnBlockAxis() {
                // Resolve 'stretch' to either 'auto' or the stretched bsize, depending
                // on whether our containing block has a definite bsize.
                // TODO(dholbert): remove this in bug 2000035.
                if NS_UNCONSTRAINEDSIZE == cbSize.BSize(wm) {
                    blockSize = AnchorResolvedSizeHelper::Auto();
                } else {
                    let stretchBSize = nsLayoutUtils::ComputeStretchBSize(cbSize.BSize(wm), self.ComputedLogicalMargin(wm).BStartEnd(wm), self.ComputedLogicalBorderPadding(wm).BStartEnd(wm), self.mStylePosition.mBoxSizing());
                    blockSize = AnchorResolvedSizeHelper::LengthPercentage(LengthPercentage::FromAppUnits(stretchBSize));
                }
            }
            let mut isAutoBSize = blockSize.BehavesLikeInitialValueOnBlockAxis();

            // Check for a percentage based block size and a containing block
            // block size that depends on the content block size
            if blockSize.HasPercent() {
                if NS_UNCONSTRAINEDSIZE == cbSize.BSize(wm) {
                    // this if clause enables %-blockSize on replaced inline frames,
                    // such as images.  See bug 54119.  The else clause "blockSizeUnit =
                    // eStyleUnit_Auto;" used to be called exclusively.
                    if self.mFlags.mIsReplaced && self.mStyleDisplay.IsInlineOutsideStyle() {
                        // Get the containing block's reflow input
                        NS_ASSERTION!(true, "no containing block");
                        // in quirks mode, get the cb height using the special quirk method
                        if !wm.IsVertical() && eCompatibility_NavQuirks == aPresContext.CompatibilityMode() {
                            if !cbri.mFrame.IsTableCellFrame() && !cbri.mFrame.IsFlexOrGridItem() {
                                *cbSize.BSize_mut(wm) = CalcQuirkContainingBlockHeight(cbri);
                                if cbSize.BSize(wm) == NS_UNCONSTRAINEDSIZE {
                                    isAutoBSize = true;
                                }
                            } else {
                                isAutoBSize = true;
                            }
                        }
                        // in standard mode, use the cb block size.  if it's "auto",
                        // as will be the case by default in BODY, use auto block size
                        // as per CSS2 spec.
                        else {
                            let computedBSize = cbri.ComputedSize__WritingMode(wm).BSize(wm);
                            if NS_UNCONSTRAINEDSIZE != computedBSize {
                                *cbSize.BSize_mut(wm) = computedBSize;
                            } else {
                                isAutoBSize = true;
                            }
                        }
                    } else {
                        // default to interpreting the blockSize like 'auto'
                        isAutoBSize = true;
                    }
                }
            }

            // Compute our offsets if the element is relatively positioned.  We
            // need the correct containing block inline-size and block-size
            // here, which is why we need to do it after all the quirks-n-such
            // above. (If the element is sticky positioned, we need to wait
            // until the scroll container knows its size, so we compute offsets
            // from StickyScrollContainer::UpdatePositions.)
            if self.mStyleDisplay.IsRelativelyPositioned(self.mFrame) {
                let offsets = ReflowInput::ComputeRelativeOffsets(cbwm, self.mFrame, cbSize.ConvertTo(cbwm, wm));
                self.SetComputedLogicalOffsets(cbwm, offsets);
            } else {
                // Initialize offsets to 0
                self.SetComputedLogicalOffsets(wm, LogicalMargin::new__WritingMode(wm));
            }

            // Calculate the computed values for min and max properties.  Note that
            // this MUST come after we've computed our border and padding.
            self.ComputeMinMaxValues(cbSize);

            // Calculate the computed inlineSize and blockSize.
            // This varies by frame type.

            if self.IsInternalTableFrame() {
                // Internal table elements. The rules vary depending on the type.
                // Calculate the computed isize
                let mut rowOrRowGroup = false;
                let inlineSize = self.mStylePosition.ISize(wm, &AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false));
                let mut isAutoISize = inlineSize.IsAuto();
                if StyleDisplay::TableRow == self.mStyleDisplay.mDisplay() || StyleDisplay::TableRowGroup == self.mStyleDisplay.mDisplay() {
                    // 'inlineSize' property doesn't apply to table rows and row groups
                    isAutoISize = true;
                    rowOrRowGroup = true;
                }

                // calc() with both percentages and lengths act like auto on internal
                // table elements
                if isAutoISize || inlineSize.HasLengthAndPercentage() {
                    if self.AvailableISize() != NS_UNCONSTRAINEDSIZE && !rowOrRowGroup {
                        // Internal table elements don't have margins. Only tables and
                        // cells have border and padding
                        let value = std::cmp::max(0, self.AvailableISize() - self.ComputedLogicalBorderPadding(wm).IStartEnd(wm));
                        self.SetComputedISize(value, ReflowInput_ResetResizeFlags::No);
                    } else {
                        let value = self.AvailableISize();
                        self.SetComputedISize(value, ReflowInput_ResetResizeFlags::No);
                    }
                    NS_ASSERTION!(self.ComputedISize() >= 0, "Bogus computed isize");
                } else {
                    let value = self.ComputeISizeValue(cbSize, self.mStylePosition.mBoxSizing(), &inlineSize);
                    self.SetComputedISize(value, ReflowInput_ResetResizeFlags::No);
                }

                // Calculate the computed block size
                if StyleDisplay::TableColumn == self.mStyleDisplay.mDisplay() || StyleDisplay::TableColumnGroup == self.mStyleDisplay.mDisplay() {
                    // 'blockSize' property doesn't apply to table columns and column groups
                    isAutoBSize = true;
                }
                // calc() with both percentages and lengths acts like 'auto' on internal
                // table elements
                if isAutoBSize || blockSize.HasLengthAndPercentage() {
                    self.SetComputedBSize(NS_UNCONSTRAINEDSIZE, ReflowInput_ResetResizeFlags::No);
                } else {
                    let value = self.ComputeBSizeValue(cbSize.BSize(wm), self.mStylePosition.mBoxSizing(), blockSize.AsLengthPercentage());
                    self.SetComputedBSize(value, ReflowInput_ResetResizeFlags::No);
                }

                // Doesn't apply to internal table elements
                let mWritingMode = self.mWritingMode;
                self.mComputedMinSize.SizeTo(mWritingMode, 0, 0);
                self.mComputedMaxSize.SizeTo(mWritingMode, NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE);
            } else if self.mFrame.IsAbsolutelyPositioned(Some(&self.mStyleDisplay)) &&
                // The absolute constraints are needed only for abspos
                // first-in-flow, not continuations.
                self.mFrame.GetPrevInFlow().is_none()
            {
                self.InitAbsoluteConstraints(cbri, cbSize.ConvertTo(cbri.GetWritingMode(), wm));
            } else {
                // NOT PORTED: AutoMaybeDisableFontInflation (font size inflation is off on desktop).

                let alignCB: nsIFrame = (|| {
                    let cb = self.mFrame.GetParent().unwrap();
                    if cb.IsTableWrapperFrame() {
                        let alignCBParent = cb.GetParent();
                        if let Some(alignCBParent) = alignCBParent {
                            if alignCBParent.IsGridContainerFrame() {
                                return alignCBParent;
                            }
                        }
                    }
                    // NOT PORTED: `if (cb->Style()->GetPseudoType() == PseudoStyleType::MozColumnSpanWrapper)`
                    // (ReflowInput.cpp lines 2291-2306): the alignment container of a column-span wrapper is
                    // the first ancestor that is not one; column spanners are not supported.
                    cb
                })();

                let isInlineLevel: bool = (|| {
                    if self.mFrame.IsTableFrame() {
                        // An inner table frame is not inline-level, even if it happens to
                        // have 'display:inline-table'. (That makes its table-wrapper frame be
                        // inline-level, but not the inner table frame)
                        return false;
                    }
                    if self.mStyleDisplay.IsInlineOutsideStyle() {
                        return true;
                    }
                    if self.mFlags.mIsReplaced && (self.mStyleDisplay.IsInnerTableStyle() || self.mStyleDisplay.DisplayOutside() == StyleDisplayOutside::TableCaption) {
                        // Internal table values on replaced elements behave as inline
                        // https://drafts.csswg.org/css-tables-3/#table-structure
                        //
                        //     ... it is handled instead as though the author had declared
                        //     either 'block' (for 'table' display) or 'inline' (for all
                        //     other values)"
                        //
                        // FIXME(emilio): The only test that covers this is
                        // table-anonymous-objects-211.xht, which fails on other browsers (but
                        // differently to us, if you just remove this condition).
                        return true;
                    }
                    if self.mFrame.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW) && !self.mStyleDisplay.IsAbsolutelyPositionedStyle() {
                        // Floats are treated as inline-level and also shrink-wrap.
                        return true;
                    }
                    false
                })();

                if self.mParentReflowInput.unwrap().mFlags.mOrthogonalCellFinalReflow {
                    // This is the "extra" reflow for the inner content of an orthogonal
                    // table cell, after the row size has been determined; so we want to
                    // respect the cell's size without further adjustment. Its rect may
                    // not yet be correct, however, so we base our size on the parent
                    // reflow input's available size, adjusted for border widths.
                    MOZ_ASSERT!(self.mFrame.GetParent().unwrap().IsTableCellFrame(), "unexpected mOrthogonalCellFinalReflow flag!");
                    let parent = self.mParentReflowInput.unwrap();
                    cbSize = parent.AvailableSize__().ConvertTo(wm, parent.GetWritingMode());
                    cbSize -= parent.ComputedLogicalBorder(wm).Size(wm);
                    self.SetAvailableISize(cbSize.ISize(wm));
                } else {
                    let shouldShrinkWrap: bool = (|| {
                        if isInlineLevel {
                            return true;
                        }
                        if self.mFlags.mIsReplaced && !alignCB.IsFlexOrGridContainer() {
                            // Shrink-wrap replaced elements when in-flow (out of flows are
                            // handled above). We exclude replaced elements in grid or flex
                            // contexts, where we don't want to shrink-wrap unconditionally (so
                            // that stretching can happen). When grid/flex explicitly want
                            // shrink-wrapping, they can request it directly using the relevant
                            // flag.
                            return true;
                        }
                        if !alignCB.IsGridContainerFrame() && self.mWritingMode.IsOrthogonalTo(&alignCB.GetWritingMode()) {
                            // Shrink-wrap blocks that are orthogonal to their container (unless
                            // we're in a grid?)
                            return true;
                        }
                        false
                    })();

                    if shouldShrinkWrap {
                        self.mComputeSizeFlags += ComputeSizeFlag::ShrinkWrap;
                    }

                    if cbSize.ISize(wm) == NS_UNCONSTRAINEDSIZE {
                        // For orthogonal flows, where we found a parent orthogonal-limit for
                        // AvailableISize() in Init(), we'll use the same here as well.
                        *cbSize.ISize_mut(wm) = self.AvailableISize();
                    }
                }

                let size = self.mFrame.ComputeSize(
                    &self.mSizeComputationInput,
                    wm,
                    cbSize,
                    self.AvailableISize(),
                    self.ComputedLogicalMargin(wm).Size(wm),
                    self.ComputedLogicalBorderPadding(wm).Size(wm),
                    &self.mStyleSizeOverrides,
                    self.mComputeSizeFlags,
                );

                self.mComputedSize = size.mLogicalSize;
                NS_ASSERTION!(self.ComputedISize() >= 0, "Bogus inline-size");
                NS_ASSERTION!(self.ComputedBSize() == NS_UNCONSTRAINEDSIZE || self.ComputedBSize() >= 0, "Bogus block-size");

                self.mFlags.mIsBSizeSetByAspectRatio = size.mAspectRatioUsage == nsIFrame_AspectRatioUsage::ToComputeBSize;

                let shouldCalculateBlockSideMargins: bool = (|| {
                    if isInlineLevel {
                        return false;
                    }
                    if self.mFrame.IsTableFrame() {
                        return false;
                    }
                    if alignCB.IsFlexOrGridContainer() {
                        // Exclude flex and grid items.
                        return false;
                    }
                    // ADAPTER: Stylo's servo configuration has no pseudo type to ask; the frame constructor
                    // registered the outside ::marker with its list item (SetMarkerFrameForListItem).
                    if self.mFrame.IsOutsideMarkerFrame() {
                        // Exclude outside ::markers.
                        return false;
                    }
                    // NOT PORTED: `pseudoType == PseudoStyleType::MozColumnContent` (excluded because
                    // -moz-column-content cannot have any margin); there are no column frames.
                    true
                })();

                if shouldCalculateBlockSideMargins {
                    self.CalculateBlockSideMargins();
                }
            }
        }

        // Save our containing block dimensions
        self.mContainingBlockSize = cbSize;
    }
}

impl<'a> ReflowInput<'a> {
    // Calculates the computed values for the 'min-inline-size',
    // 'max-inline-size', 'min-block-size', and 'max-block-size' properties, and
    // stores them in the assorted data members
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::ComputeMinMaxValues
    fn ComputeMinMaxValues(&mut self, aCBSize: LogicalSize) {
        let wm = self.GetWritingMode();

        let anchorResolutionParams = AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false);
        let minISize = self.mStylePosition.MinISize(wm, &anchorResolutionParams);
        let maxISize = self.mStylePosition.MaxISize(wm, &anchorResolutionParams);
        let minBSize = self.mStylePosition.MinBSize(wm, &anchorResolutionParams);
        let maxBSize = self.mStylePosition.MaxBSize(wm, &anchorResolutionParams);

        let minWidgetSize = LogicalSize::new__WritingMode(wm);
        if self.mIsThemed {
            // NOT PORTED: native theme minimum size (pc->Theme()->GetMinimumWidgetSize and the
            // conversion to a content-box LogicalSize, ReflowInput.cpp lines 2943-2953);
            // mIsThemed is never true here.
            unimplemented!("NOT PORTED: themed frames");
        }

        // NOTE: min-width:auto resolves to 0, except on a flex item. (But
        // even there, it's supposed to be ignored (i.e. treated as 0) until
        // the flex container explicitly resolves & considers it.)
        if minISize.IsAuto() {
            self.SetComputedMinISize(0);
        } else {
            let value = self.ComputeISizeValue(aCBSize, self.mStylePosition.mBoxSizing(), &minISize);
            self.SetComputedMinISize(value);
        }

        if self.mIsThemed {
            self.SetComputedMinISize(std::cmp::max(self.ComputedMinISize(), minWidgetSize.ISize(wm)));
        }

        if maxISize.IsNone() {
            // Specified value of 'none'
            self.SetComputedMaxISize(NS_UNCONSTRAINEDSIZE);
        } else {
            let value = self.ComputeISizeValue(aCBSize, self.mStylePosition.mBoxSizing(), &maxISize);
            self.SetComputedMaxISize(value);
        }

        // If the computed value of 'min-width' is greater than the value of
        // 'max-width', 'max-width' is set to the value of 'min-width'
        if self.ComputedMinISize() > self.ComputedMaxISize() {
            let value = self.ComputedMinISize();
            self.SetComputedMaxISize(value);
        }

        // Check for percentage based values and a containing block height that
        // depends on the content height. Treat them like the initial value.
        // Likewise, check for calc() with percentages on internal table elements;
        // that's treated as the initial value too.
        let isInternalTableFrame = self.IsInternalTableFrame();
        let bPercentageBasis: nscoord = aCBSize.BSize(wm);
        // (a C++ generic lambda over StyleSize and StyleMaxSize; a Rust closure cannot be generic,
        // so it is a nested generic function that takes the captured values as parameters)
        fn BSizeBehavesAsInitialValue<SizeOrMaxSize: StyleSizeLike>(aBSize: &SizeOrMaxSize, isInternalTableFrame: bool, bPercentageBasis: nscoord) -> bool {
            if nsLayoutUtils::IsAutoBSize(aBSize, bPercentageBasis) {
                return true;
            }
            if isInternalTableFrame {
                return aBSize.HasLengthAndPercentage();
            }
            false
        }

        // NOTE: min-height:auto resolves to 0, except on a flex item. (But
        // even there, it's supposed to be ignored (i.e. treated as 0) until
        // the flex container explicitly resolves & considers it.)
        if BSizeBehavesAsInitialValue(&minBSize, isInternalTableFrame, bPercentageBasis) {
            self.SetComputedMinBSize(0);
        } else {
            let value = self.ComputeBSizeValueHandlingStretch(bPercentageBasis, self.mStylePosition.mBoxSizing(), &minBSize);
            self.SetComputedMinBSize(value);
        }

        if self.mIsThemed {
            self.SetComputedMinBSize(std::cmp::max(self.ComputedMinBSize(), minWidgetSize.BSize(wm)));
        }

        if BSizeBehavesAsInitialValue(&maxBSize, isInternalTableFrame, bPercentageBasis) {
            // Specified value of 'none'
            self.SetComputedMaxBSize(NS_UNCONSTRAINEDSIZE);
        } else {
            let value = self.ComputeBSizeValueHandlingStretch(bPercentageBasis, self.mStylePosition.mBoxSizing(), &maxBSize);
            self.SetComputedMaxBSize(value);
        }

        // If the computed value of 'min-height' is greater than the value of
        // 'max-height', 'max-height' is set to the value of 'min-height'
        if self.ComputedMinBSize() > self.ComputedMaxBSize() {
            let value = self.ComputedMinBSize();
            self.SetComputedMaxBSize(value);
        }
    }

    // aInsideBoxSizing returns the part of the padding, border, and margin
    // in the aAxis dimension that goes inside the edge given by box-sizing;
    // aOutsideBoxSizing returns the rest.
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::CalculateBorderPaddingMargin
    fn CalculateBorderPaddingMargin(&self, aAxis: LogicalAxis, aContainingBlockSize: nscoord, aInsideBoxSizing: &mut nscoord, aOutsideBoxSizing: &mut nscoord) {
        let wm = self.GetWritingMode();
        let startSide = wm.PhysicalSide(MakeLogicalSide(aAxis, LogicalEdge::Start));
        let endSide = wm.PhysicalSide(MakeLogicalSide(aAxis, LogicalEdge::End));

        let styleBorder = self.mStyleBorder.GetComputedBorder();
        let borderStartEnd = styleBorder.Side(startSide) + styleBorder.Side(endSide);

        let paddingStartEnd: nscoord;
        let marginStartEnd: nscoord;

        // See if the style system can provide us the padding directly
        let stylePadding = self.mFrame.StylePadding();
        let mut padding = nsMargin::default();
        if stylePadding.GetPadding(&mut padding) {
            paddingStartEnd = padding.Side(startSide) + padding.Side(endSide);
        } else {
            // We have to compute the start and end values
            let mPadding = stylePadding.mPadding();
            let start = nsLayoutUtils::ComputeCBDependentValue__nscoord_LengthPercentage(aContainingBlockSize, mPadding.Get__Side(startSide));
            let end = nsLayoutUtils::ComputeCBDependentValue__nscoord_LengthPercentage(aContainingBlockSize, mPadding.Get__Side(endSide));
            paddingStartEnd = start + end;
        }

        // See if the style system can provide us the margin directly
        let mut margin = nsMargin::default();
        if self.mStyleMargin.GetMargin__nsMargin(&mut margin) {
            marginStartEnd = margin.Side(startSide) + margin.Side(endSide);
        } else {
            // If the margin is 'auto', ComputeCBDependentValue() will return 0. The
            // correct margin value will be computed later in InitAbsoluteConstraints
            // (which is caller of this function, via CalculateHypotheticalPosition).
            let anchorResolutionParams = AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false);
            let start = nsLayoutUtils::ComputeCBDependentValue__nscoord_AnchorResolvedMargin(aContainingBlockSize, &self.mStyleMargin.GetMargin__Side_AnchorPosResolutionParams(startSide, &anchorResolutionParams));
            let end = nsLayoutUtils::ComputeCBDependentValue__nscoord_AnchorResolvedMargin(aContainingBlockSize, &self.mStyleMargin.GetMargin__Side_AnchorPosResolutionParams(endSide, &anchorResolutionParams));
            marginStartEnd = start + end;
        }

        let mut outside = paddingStartEnd + borderStartEnd + marginStartEnd;
        let mut inside = 0;
        if self.mStylePosition.mBoxSizing() == StyleBoxSizing::BorderBox {
            inside = borderStartEnd + paddingStartEnd;
        }
        outside -= inside;
        *aInsideBoxSizing = inside;
        *aOutsideBoxSizing = outside;
    }

    // This code enforces section 10.3.3 of the CSS2 spec for this formula:
    //
    // 'margin-left' + 'border-left-width' + 'padding-left' + 'width' +
    //   'padding-right' + 'border-right-width' + 'margin-right'
    //   = width of containing block
    //
    // Note: the width unit is not auto when this is called
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::CalculateBlockSideMargins
    fn CalculateBlockSideMargins(&mut self) {
        MOZ_ASSERT!(!self.mFrame.IsTableFrame(), "Inner table frame cannot have computed margins!");

        // Calculations here are done in the containing block's writing mode,
        // which is where margins will eventually be applied: we're calculating
        // margins that will be used by the container in its inline direction,
        // which in the case of an orthogonal contained block will correspond to
        // the block direction of this reflow input. So in the orthogonal-flow
        // case, "CalculateBlock*Side*Margins" will actually end up adjusting
        // the BStart/BEnd margins; those are the "sides" of the block from its
        // container's point of view.
        let cbWM = self.GetCBWritingMode();

        let availISizeCBWM = self.AvailableSize__WritingMode(cbWM).ISize(cbWM);
        let computedISizeCBWM = self.ComputedSize__WritingMode(cbWM).ISize(cbWM);
        if availISizeCBWM == NS_UNCONSTRAINEDSIZE || computedISizeCBWM == NS_UNCONSTRAINEDSIZE {
            // For orthogonal flows, where we found a parent orthogonal-limit
            // for AvailableISize() in Init(), we don't have meaningful sizes to
            // adjust.  Act like the sum is already correct (below).
            return;
        }

        NS_WARNING_ASSERTION!(
            NS_UNCONSTRAINEDSIZE != computedISizeCBWM && NS_UNCONSTRAINEDSIZE != availISizeCBWM,
            "have unconstrained inline-size; this should only result from very large sizes, not attempts at intrinsic inline-size calculation"
        );

        let mut margin = self.ComputedLogicalMargin(cbWM);
        let borderPadding = self.ComputedLogicalBorderPadding(cbWM);
        let sum = margin.IStartEnd(cbWM) + borderPadding.IStartEnd(cbWM) + computedISizeCBWM;
        if sum == availISizeCBWM {
            // The sum is already correct
            return;
        }

        // Determine the start and end margin values. The isize value
        // remains constant while we do this.

        // Calculate how much space is available for margins
        let availMarginSpace = availISizeCBWM - sum;

        // If the available margin space is negative, then don't follow the
        // usual overconstraint rules.
        if availMarginSpace < 0 {
            *margin.IEnd_mut(cbWM) += availMarginSpace;
            self.SetComputedLogicalMargin(cbWM, margin);
            return;
        }

        let anchorResolutionParams = AnchorPosResolutionParams::From__SizeComputationInput_bool(&self.mSizeComputationInput, false);
        // The css2 spec clearly defines how block elements should behave
        // in section 10.3.3.
        let mut isAutoStartMargin = self.mStyleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(LogicalSide::IStart, cbWM, &anchorResolutionParams).IsAuto();
        let mut isAutoEndMargin = self.mStyleMargin.GetMargin__LogicalSide_WritingMode_AnchorPosResolutionParams(LogicalSide::IEnd, cbWM, &anchorResolutionParams).IsAuto();
        if !isAutoStartMargin && !isAutoEndMargin {
            // Neither margin is 'auto' so we're over constrained. Use the
            // 'direction' property of the parent to tell which margin to
            // ignore
            // First check if there is an HTML alignment that we should honor
            let textAlign: Option<StyleTextAlign> = self.mParentReflowInput.map(|p| p.mFrame.StyleText().mTextAlign());
            if textAlign.map_or(false, |ta| ta == StyleTextAlign::MozLeft || ta == StyleTextAlign::MozCenter || ta == StyleTextAlign::MozRight) {
                let textAlign = textAlign.unwrap();
                if self.mParentReflowInput.unwrap().mWritingMode.IsBidiLTR() {
                    isAutoStartMargin = textAlign != StyleTextAlign::MozLeft;
                    isAutoEndMargin = textAlign != StyleTextAlign::MozRight;
                } else {
                    isAutoStartMargin = textAlign != StyleTextAlign::MozRight;
                    isAutoEndMargin = textAlign != StyleTextAlign::MozLeft;
                }
            }
            // Otherwise apply the CSS rules, and ignore one margin by forcing
            // it to 'auto', depending on 'direction'.
            else {
                isAutoEndMargin = true;
            }
        }

        // Logic which is common to blocks and tables
        // The computed margins need not be zero because the 'auto' could come from
        // overconstraint or from HTML alignment so values need to be accumulated

        if isAutoStartMargin {
            if isAutoEndMargin {
                // Both margins are 'auto' so the computed addition should be equal
                let forStart = availMarginSpace / 2;
                *margin.IStart_mut(cbWM) += forStart;
                *margin.IEnd_mut(cbWM) += availMarginSpace - forStart;
            } else {
                *margin.IStart_mut(cbWM) += availMarginSpace;
            }
        } else if isAutoEndMargin {
            *margin.IEnd_mut(cbWM) += availMarginSpace;
        }
        self.SetComputedLogicalMargin(cbWM, margin);

        if isAutoStartMargin || isAutoEndMargin {
            // Update the UsedMargin property if we were tracking it already.
            if self.mFrame.GetProperty(nsIFrame::UsedMarginProperty()).is_some() {
                self.mFrame.SetProperty(nsIFrame::UsedMarginProperty(), margin.GetPhysicalMargin(cbWM));
            }
        }
    }

    /**
     * @return true if mFrame is an internal table frame, i.e. an
     * ns[RowGroup|ColGroup|Row|Cell]Frame.  (We exclude nsTableColFrame
     * here since we never setup a ReflowInput for those.)
     */
    // Gecko: layout/generic/ReflowInput.cpp ReflowInput::IsInternalTableFrame
    fn IsInternalTableFrame(&self) -> bool {
        self.mFrame.IsTableRowGroupFrame() || self.mFrame.IsTableColGroupFrame() || self.mFrame.IsTableRowFrame() || self.mFrame.IsTableCellFrame()
    }
}

// Tests: layout/generic has no gtest for ReflowInput (Firefox tests it through reftests), so these
// are written from the documented behaviour (CSS 2.2 sections 10.2 to 10.7, CSS Sizing, and the
// comments in ReflowInput.h / ReflowInput.cpp). Frames get their styles from real HTML run through
// Stylo (mrt-style); every element is picked by its id and its style comes from its `style=""`.
#[cfg(test)]
mod tests {
    use super::*;
    use mrt_style::mrt_dom::{Dom, Kind, NodeId};
    use mrt_style::style::properties::ComputedValues;
    use mrt_style::style::servo_arc::Arc as ServoArc;
    use mrt_style::StyleDom;

    // Parses `html`, resolves styles, and returns the computed style of each element with an id.
    fn styles_of(html: &str, ids: &[&str]) -> Vec<ServoArc<ComputedValues>> {
        let mut sd = StyleDom::new(Dom::parse(html), (1489.0, 844.0));
        sd.resolve();
        ids.iter()
            .map(|want| {
                let dom = &sd.tree.dom;
                let node = (0..dom.nodes.len() as NodeId)
                    .find(|&n| dom.node(n).kind == Kind::Element && dom.attrs_of(n).iter().any(|a| dom.names.get(a.name) == "id" && dom.attr_value(a) == *want))
                    .unwrap_or_else(|| panic!("no element with id {want}"));
                sd.computed(node).expect("no computed style")
            })
            .collect()
    }

    const PX: i32 = 60;

    // A chain of frames, one per (id, type): the first is the root (width `root_width` px, height
    // `root_height` px), each next one is the child of the previous. Every frame is given the
    // content of an HTML element named like its id when that is "html" or "body".
    fn frames(html: &str, chain: &[(&str, LayoutFrameType)], root_width: i32, root_height: i32) -> Vec<nsIFrame> {
        let ids: Vec<&str> = chain.iter().map(|c| c.0).collect();
        let styles = styles_of(html, &ids);
        let result: Vec<nsIFrame> = WithFrameTree(|t| {
            let mut result: Vec<nsIFrame> = Vec::new();
            for (i, style) in styles.into_iter().enumerate() {
                let wm = WritingMode::new__ComputedStyle(&style);
                let rect = if i == 0 { nsRect::new(0, 0, root_width * PX, root_height * PX) } else { nsRect::new(0, 0, 0, 0) };
                let f = t.AddFrame(result.last().copied(), rect, wm, Some(style));
                result.push(f);
            }
            result
        });
        for (f, (id, ty)) in result.iter().zip(chain.iter()) {
            f.SetType(*ty);
            if *id == "html" || *id == "body" {
                f.SetContent(Some(nsIContent::HTMLElement(id)));
            }
        }
        result
    }

    // The reflow input of the root frame: available inline-size `avail_i` px, block-size unconstrained.
    fn root_input<'a>(f: nsIFrame, avail_i: i32) -> ReflowInput<'a> {
        let wm = f.GetWritingMode();
        ReflowInput::new__nsPresContext_nsIFrame_gfxContext_LogicalSize_InitFlags(&nsPresContext::Current(), f, gfxContext, LogicalSize::new(wm, avail_i * PX, NS_UNCONSTRAINEDSIZE), EnumSet::default())
    }

    // The reflow input of a child of the frame that `parent` is the reflow input of; the available
    // inline-size is the parent's content inline-size, as a block parent would pass it.
    fn child_input<'a>(parent: &'a ReflowInput<'a>, f: nsIFrame) -> ReflowInput<'a> {
        let wm = f.GetWritingMode();
        let avail = LogicalSize::new(wm, parent.ComputedISize(), NS_UNCONSTRAINEDSIZE);
        ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            &nsPresContext::Current(),
            parent,
            f,
            avail,
            None,
            EnumSet::default(),
            StyleSizeOverrides::default(),
            ComputeSizeFlags::default(),
            std::ptr::null_mut(),
        )
    }

    // Two nested blocks: a root of `root_width` px and a child with the given style.
    fn block_in_root(child_style: &str, root_width: i32) -> (nsIFrame, nsIFrame) {
        let html = format!(r#"<div id=root style="margin:0;padding:0;border:0"><div id=c style="{child_style}"></div></div>"#);
        let f = frames(&html, &[("root", LayoutFrameType::Block), ("c", LayoutFrameType::Block)], root_width, 0);
        (f[0], f[1])
    }

    #[test]
    fn a_root_reflow_input_takes_the_available_space_minus_border_and_padding() {
        let html = r#"<div id=root style="margin:7px;padding:10px;border:5px solid"></div>"#;
        let f = frames(html, &[("root", LayoutFrameType::Block)], 400, 0);
        let wm = f[0].GetWritingMode();
        let ri = ReflowInput::new__nsPresContext_nsIFrame_gfxContext_LogicalSize_InitFlags(&nsPresContext::Current(), f[0], gfxContext, LogicalSize::new(wm, 400 * PX, 300 * PX), EnumSet::default());
        // Reflow roots start from the frame's boundary, inside the margin (ReflowInput.cpp, InitConstraints).
        assert_eq!(ri.ComputedISize(), (400 - 30) * PX);
        assert_eq!(ri.ComputedBSize(), (300 - 30) * PX);
        assert!(ri.ComputedLogicalMargin(wm).IsAllZero());
        assert_eq!(ri.ComputedLogicalBorderPadding(wm).IStartEnd(wm), 30 * PX);
        assert_eq!((ri.ComputedMinISize(), ri.ComputedMaxISize()), (0, NS_UNCONSTRAINEDSIZE));
    }

    #[test]
    fn an_auto_width_block_fills_the_containing_block_minus_margin_border_padding() {
        let (root, c) = block_in_root("margin:0 10px;padding:0 5px;border:2px solid", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        let wm = ri.GetWritingMode();
        // 400 - (10+10) margins - (5+5) padding - (2+2) borders = 366
        assert_eq!(ri.ComputedISize(), 366 * PX);
        assert_eq!(ri.ComputedLogicalMargin(wm).IStartEnd(wm), 20 * PX);
        assert_eq!(ri.ComputedLogicalBorderPadding(wm).IStartEnd(wm), 14 * PX);
        assert_eq!(ri.ComputedLogicalPadding(wm).IStartEnd(wm), 10 * PX);
        assert_eq!(ri.ComputedLogicalBorder(wm).IStartEnd(wm), 4 * PX);
        // 'height: auto' is unconstrained until the block's contents are reflowed.
        assert_eq!(ri.ComputedBSize(), NS_UNCONSTRAINEDSIZE);
        assert_eq!(ri.mContainingBlockSize.ISize(wm), 400 * PX);
    }

    #[test]
    fn a_percent_width_is_a_fraction_of_the_containing_block_and_the_end_margin_takes_the_rest() {
        let (root, c) = block_in_root("width:50%", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        let wm = ri.GetWritingMode();
        assert_eq!(ri.ComputedISize(), 200 * PX);
        // CSS 2.2 10.3.3: neither margin is 'auto', so the block is over-constrained and the
        // 'margin-right' is ignored (CalculateBlockSideMargins), which gives it the remaining 200px.
        assert_eq!(ri.ComputedLogicalMargin(wm).IStart(wm), 0);
        assert_eq!(ri.ComputedLogicalMargin(wm).IEnd(wm), 200 * PX);
    }

    #[test]
    fn auto_margins_center_a_block_with_a_width() {
        let (root, c) = block_in_root("width:100px;margin:0 auto", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        let wm = ri.GetWritingMode();
        assert_eq!(ri.ComputedISize(), 100 * PX);
        assert_eq!(ri.ComputedLogicalMargin(wm).IStart(wm), 150 * PX);
        assert_eq!(ri.ComputedLogicalMargin(wm).IEnd(wm), 150 * PX);
    }

    #[test]
    fn an_odd_amount_of_free_space_goes_to_the_end_margin_when_both_margins_are_auto() {
        // 401px of containing block, 100px of block: 301px / 2 = 150.5px; the app units are 18060,
        // which divide evenly, so use a width that leaves an odd number of app units.
        let (root, c) = block_in_root("width:100.0166667px;margin:0 auto", 400); // 6001 app units
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        let wm = ri.GetWritingMode();
        let free = 400 * PX - ri.ComputedISize();
        assert_eq!(free % 2, 1);
        assert_eq!(ri.ComputedLogicalMargin(wm).IStart(wm), free / 2);
        assert_eq!(ri.ComputedLogicalMargin(wm).IEnd(wm), free - free / 2);
    }

    #[test]
    fn a_single_auto_margin_absorbs_the_free_space() {
        let (root, c) = block_in_root("width:100px;margin-left:auto;margin-right:20px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        let wm = ri.GetWritingMode();
        assert_eq!(ri.ComputedLogicalMargin(wm).IStart(wm), 280 * PX);
        assert_eq!(ri.ComputedLogicalMargin(wm).IEnd(wm), 20 * PX);

        let (root, c) = block_in_root("width:100px;margin-left:20px;margin-right:auto", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!(ri.ComputedLogicalMargin(wm).IStart(wm), 20 * PX);
        assert_eq!(ri.ComputedLogicalMargin(wm).IEnd(wm), 280 * PX);
    }

    #[test]
    fn a_block_wider_than_its_container_has_a_negative_end_margin() {
        // CSS 2.2 10.3.3: if the sum is larger than the containing block, 'margin-right' is
        // treated as negative ("the usual overconstraint rules" do not apply).
        let (root, c) = block_in_root("width:500px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        let wm = ri.GetWritingMode();
        assert_eq!(ri.ComputedISize(), 500 * PX);
        assert_eq!(ri.ComputedLogicalMargin(wm).IEnd(wm), -100 * PX);
    }

    #[test]
    fn text_align_moz_center_on_the_parent_centers_an_over_constrained_block() {
        // The HTML <center> / align=center rule: when neither margin is auto, the parent's
        // `text-align: -moz-center` makes both margins auto (CalculateBlockSideMargins).
        let html = r#"<div id=root style="margin:0;padding:0;border:0;text-align:-moz-center"><div id=c style="width:100px"></div></div>"#;
        let f = frames(html, &[("root", LayoutFrameType::Block), ("c", LayoutFrameType::Block)], 400, 0);
        let rri = root_input(f[0], 400);
        let ri = child_input(&rri, f[1]);
        let wm = ri.GetWritingMode();
        assert_eq!(ri.ComputedLogicalMargin(wm).IStart(wm), 150 * PX);
        assert_eq!(ri.ComputedLogicalMargin(wm).IEnd(wm), 150 * PX);
    }

    #[test]
    fn min_width_and_max_width_clamp_the_width() {
        let (root, c) = block_in_root("width:500px;max-width:300px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!((ri.ComputedISize(), ri.ComputedMaxISize()), (300 * PX, 300 * PX));

        let (root, c) = block_in_root("width:100px;min-width:350px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!((ri.ComputedISize(), ri.ComputedMinISize()), (350 * PX, 350 * PX));

        // 'min-width' wins over 'max-width' (CSS 2.2 10.4): max-width is raised to min-width.
        let (root, c) = block_in_root("width:100px;min-width:350px;max-width:200px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!((ri.ComputedISize(), ri.ComputedMinISize(), ri.ComputedMaxISize()), (350 * PX, 350 * PX, 350 * PX));
    }

    #[test]
    fn min_and_max_width_percentages_and_the_default_values() {
        let (root, c) = block_in_root("max-width:25%;min-width:10%", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!((ri.ComputedMinISize(), ri.ComputedMaxISize()), (40 * PX, 100 * PX));
        // auto width fills 400px, then max-width clamps it.
        assert_eq!(ri.ComputedISize(), 100 * PX);

        let (root, c) = block_in_root("", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!((ri.ComputedMinISize(), ri.ComputedMaxISize()), (0, NS_UNCONSTRAINEDSIZE));
        assert_eq!((ri.ComputedMinBSize(), ri.ComputedMaxBSize()), (0, NS_UNCONSTRAINEDSIZE));
    }

    #[test]
    fn border_box_sizing_includes_padding_and_border_in_the_width() {
        let (root, c) = block_in_root("width:200px;box-sizing:border-box;padding:10px;border:5px solid", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        let wm = ri.GetWritingMode();
        // The content box is 200 - 2*(10+5) = 170px.
        assert_eq!(ri.ComputedISize(), 170 * PX);
        assert_eq!(ri.ComputedSizeWithBorderPadding(wm).ISize(wm), 200 * PX);

        // The same with min-width and max-width.
        let (root, c) = block_in_root("box-sizing:border-box;padding:10px;border:5px solid;min-width:100px;max-width:150px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!((ri.ComputedMinISize(), ri.ComputedMaxISize()), (70 * PX, 120 * PX));
        assert_eq!(ri.ComputedISize(), 120 * PX);
    }

    #[test]
    fn a_height_is_a_length_a_percentage_of_a_definite_container_or_auto() {
        let wm = WritingMode::default();
        // A length.
        let (root, c) = block_in_root("height:80px;padding:5px 0;border:3px solid", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!(ri.ComputedBSize(), 80 * PX);
        assert_eq!(ri.ComputedSizeWithBorderPadding(wm).BSize(wm), (80 + 10 + 6) * PX);

        // A percentage of a container whose height is definite.
        let html = r#"<div id=root style="margin:0;padding:0;border:0;height:200px"><div id=c style="height:25%"></div></div>"#;
        let f = frames(html, &[("root", LayoutFrameType::Block), ("c", LayoutFrameType::Block)], 400, 200);
        let rri = root_input(f[0], 400);
        // (the root input has an unconstrained available block-size: its height comes from the style)
        let mut rri = rri;
        rri.SetComputedBSize(200 * PX, ReflowInput_ResetResizeFlags::No);
        let ri = child_input(&rri, f[1]);
        assert_eq!(ri.ComputedBSize(), 50 * PX);

        // A percentage of a container whose height is auto is auto (CSS 2.2 10.5).
        let (root, c) = block_in_root("height:25%", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!(ri.ComputedBSize(), NS_UNCONSTRAINEDSIZE);
    }

    #[test]
    fn min_height_and_max_height_clamp_a_definite_height() {
        let (root, c) = block_in_root("height:100px;max-height:60px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!(ri.ComputedBSize(), 60 * PX);

        let (root, c) = block_in_root("height:100px;min-height:150px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!(ri.ComputedBSize(), 150 * PX);

        // An auto height stays unconstrained but its limits are recorded; ApplyMinMaxBSize applies them.
        let (root, c) = block_in_root("min-height:30px;max-height:90px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!(ri.ComputedBSize(), NS_UNCONSTRAINEDSIZE);
        assert_eq!((ri.ComputedMinBSize(), ri.ComputedMaxBSize()), (30 * PX, 90 * PX));
        assert_eq!(ri.ApplyMinMaxBSize(10 * PX, 0), 30 * PX);
        assert_eq!(ri.ApplyMinMaxBSize(50 * PX, 0), 50 * PX);
        assert_eq!(ri.ApplyMinMaxBSize(500 * PX, 0), 90 * PX);
        // The consumed part of the block-size (from the previous fragments) is not clamped.
        assert_eq!(ri.ApplyMinMaxBSize(10 * PX, 20 * PX), 10 * PX);
        assert_eq!(ri.ApplyMinMaxISize(5 * PX), 5 * PX);
    }

    #[test]
    fn percentage_padding_and_margin_resolve_against_the_containing_block_inline_size() {
        let (root, c) = block_in_root("padding:10%;margin:0 5%", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        let wm = ri.GetWritingMode();
        // Both axes use the containing block's inline-size (CSS 2.2 8.3 and 8.4): padding 40px on all sides.
        assert_eq!(ri.ComputedLogicalPadding(wm).IStart(wm), 40 * PX);
        assert_eq!(ri.ComputedLogicalPadding(wm).BStart(wm), 40 * PX);
        assert_eq!(ri.ComputedLogicalMargin(wm).IStart(wm), 20 * PX);
        // 400 - 2*20 margins - 2*40 padding
        assert_eq!(ri.ComputedISize(), 280 * PX);
        // Percentages make the padding and margin depend on the layout, so the frame keeps them.
        assert_eq!(c.GetProperty(nsIFrame::UsedPaddingProperty()).map(|m| m.left), Some(40 * PX));
        assert_eq!(c.GetProperty(nsIFrame::UsedMarginProperty()).map(|m| m.left), Some(20 * PX));
        assert!(c.HasPaddingChange());
    }

    #[test]
    fn fixed_padding_and_margin_are_not_stored_as_frame_properties() {
        let (root, c) = block_in_root("padding:3px;margin:2px", 400);
        let rri = root_input(root, 400);
        let _ri = child_input(&rri, c);
        assert!(!c.HasProperty(nsIFrame::UsedPaddingProperty()));
        assert!(!c.HasProperty(nsIFrame::UsedMarginProperty()));
    }

    #[test]
    fn auto_margins_update_the_stored_used_margin() {
        // A percentage margin makes the frame track its used margin; resolving the 'auto' margin
        // afterwards must update the stored value (CalculateBlockSideMargins, end).
        let (root, c) = block_in_root("width:100px;margin-left:auto;margin-right:10%", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        let wm = ri.GetWritingMode();
        assert_eq!(ri.ComputedLogicalMargin(wm).IEnd(wm), 40 * PX);
        assert_eq!(ri.ComputedLogicalMargin(wm).IStart(wm), 260 * PX);
        assert_eq!(c.GetProperty(nsIFrame::UsedMarginProperty()).map(|m| m.left), Some(260 * PX));
    }

    #[test]
    fn a_float_shrink_wraps_to_its_preferred_inline_size_within_the_available_inline_size() {
        // Floats are out-of-flow and inline-level: they shrink-wrap (min(max(min-content, available), max-content)),
        // see ShrinkISizeToFit. The intrinsic sizes come from the frame (SetIntrinsicISizes).
        let html = r#"<div id=root style="margin:0;padding:0;border:0"><div id=c style="float:left;margin:0 10px"></div></div>"#;
        let f = frames(html, &[("root", LayoutFrameType::Block), ("c", LayoutFrameType::Block)], 400, 0);
        f[1].AddStateBits(NS_FRAME_OUT_OF_FLOW);
        f[1].SetIntrinsicISizes(100 * PX, 250 * PX);
        let rri = root_input(f[0], 400);
        let ri = child_input(&rri, f[1]);
        assert!(ri.mComputeSizeFlags.contains(ComputeSizeFlag::ShrinkWrap));
        assert_eq!(ri.ComputedISize(), 250 * PX);

        // Narrower container: the preferred size does not fit, the available size (minus margins) does.
        let rri = root_input(f[0], 200);
        let ri = child_input(&rri, f[1]);
        assert_eq!(ri.ComputedISize(), 180 * PX);

        // Narrower than the min-content size: the min-content size wins.
        let rri = root_input(f[0], 90);
        let ri = child_input(&rri, f[1]);
        assert_eq!(ri.ComputedISize(), 100 * PX);
        // Floats do not get their side margins adjusted.
        let wm = ri.GetWritingMode();
        assert_eq!(ri.ComputedLogicalMargin(wm).IStartEnd(wm), 20 * PX);
    }

    #[test]
    fn fit_content_width_and_stretch_width() {
        let html = r#"<div id=root style="margin:0;padding:0;border:0"><div id=a style="width:fit-content"></div><div id=b style="width:min-content"></div><div id=d style="width:max-content"></div></div>"#;
        let f = frames(html, &[("root", LayoutFrameType::Block), ("a", LayoutFrameType::Block)], 400, 0);
        f[1].SetIntrinsicISizes(100 * PX, 250 * PX);
        let rri = root_input(f[0], 400);
        let ri = child_input(&rri, f[1]);
        assert_eq!(ri.ComputedISize(), 250 * PX);
        let rri = root_input(f[0], 150);
        let ri = child_input(&rri, f[1]);
        assert_eq!(ri.ComputedISize(), 150 * PX);

        let g = frames(html, &[("root", LayoutFrameType::Block), ("b", LayoutFrameType::Block)], 400, 0);
        g[1].SetIntrinsicISizes(100 * PX, 250 * PX);
        let rri = root_input(g[0], 400);
        assert_eq!(child_input(&rri, g[1]).ComputedISize(), 100 * PX);

        let h = frames(html, &[("root", LayoutFrameType::Block), ("d", LayoutFrameType::Block)], 400, 0);
        h[1].SetIntrinsicISizes(100 * PX, 250 * PX);
        let rri = root_input(h[0], 400);
        assert_eq!(child_input(&rri, h[1]).ComputedISize(), 250 * PX);
    }

    #[test]
    fn aspect_ratio_gives_the_block_size_from_the_inline_size() {
        let (root, c) = block_in_root("width:200px;aspect-ratio:2/1", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!(ri.ComputedISize(), 200 * PX);
        assert_eq!(ri.ComputedBSize(), 100 * PX);
        assert!(ri.mFlags.mIsBSizeSetByAspectRatio);

        // From the block-size to the inline-size (an auto inline-size is the ratio-dependent axis
        // when the block-size is definite). The automatic minimum size (CSS Sizing 4, aspect-ratio
        // minimum) is the min-content size, here 30px, which is below the 100px.
        let (root, c) = block_in_root("height:50px;aspect-ratio:2/1", 400);
        c.SetIntrinsicISizes(30 * PX, 90 * PX);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!((ri.ComputedISize(), ri.ComputedBSize()), (100 * PX, 50 * PX));
        assert!(!ri.mFlags.mIsBSizeSetByAspectRatio);

        // When the ratio gives less than the min-content size, the min-content size wins.
        let (root, c) = block_in_root("height:50px;aspect-ratio:2/1", 400);
        c.SetIntrinsicISizes(130 * PX, 190 * PX);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!(ri.ComputedISize(), 130 * PX);
    }

    #[test]
    fn relative_positioning_computes_offsets_and_moves_the_position() {
        let (root, c) = block_in_root("position:relative;left:10px;top:5px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        // 'right' and 'bottom' are the negated 'left' and 'top' (CSS 2.2 9.3.2).
        assert_eq!(ri.ComputedPhysicalOffsets(), nsMargin::new(5 * PX, -10 * PX, -5 * PX, 10 * PX));
        assert_eq!(c.GetProperty(nsIFrame::ComputedOffsetProperty()), Some(nsMargin::new(5 * PX, -10 * PX, -5 * PX, 10 * PX)));

        let mut pos = nsPoint::new(100, 200);
        ReflowInput::ApplyRelativePositioning__nsIFrame_nsMargin_nsPoint(c, ri.ComputedPhysicalOffsets(), &mut pos);
        assert_eq!(pos, nsPoint::new(100 + 10 * PX, 200 + 5 * PX));
        // The position before the offset is kept (nsIFrame::GetNormalPosition).
        assert_eq!(c.GetProperty(nsIFrame::NormalPositionProperty()), Some(nsPoint::new(100, 200)));

        // right/bottom are the negated left/top when only they are given; 'left' wins over 'right'.
        let (root, c) = block_in_root("position:relative;right:7px;left:3px;bottom:4px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!(ri.ComputedPhysicalOffsets(), nsMargin::new(-4 * PX, -3 * PX, 4 * PX, 3 * PX));
    }

    #[test]
    fn a_static_block_has_no_offsets() {
        let (root, c) = block_in_root("left:10px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!(ri.ComputedPhysicalOffsets(), nsMargin::new(0, 0, 0, 0));
    }

    #[test]
    fn abs_pos_auto_margins_follow_css_2_2_10_3_7() {
        let wm = WritingMode::default();
        let mut m = LogicalMargin::new__WritingMode(wm);
        ReflowInput::ComputeAbsPosInlineAutoMargin(100, wm, true, true, &mut m);
        assert_eq!((m.IStart(wm), m.IEnd(wm)), (50, 50));
        // Negative free space: 'margin-right' takes it all, 'margin-left' stays 0.
        let mut m = LogicalMargin::new__WritingMode(wm);
        ReflowInput::ComputeAbsPosInlineAutoMargin(-30, wm, true, true, &mut m);
        assert_eq!((m.IStart(wm), m.IEnd(wm)), (0, -30));
        let mut m = LogicalMargin::new__WritingMode(wm);
        ReflowInput::ComputeAbsPosInlineAutoMargin(40, wm, true, false, &mut m);
        assert_eq!((m.IStart(wm), m.IEnd(wm)), (40, 0));
        let mut m = LogicalMargin::new__WritingMode(wm);
        ReflowInput::ComputeAbsPosBlockAutoMargin(41, wm, true, true, &mut m);
        assert_eq!((m.BStart(wm), m.BEnd(wm)), (20, 21));
        let mut m = LogicalMargin::new__WritingMode(wm);
        ReflowInput::ComputeAbsPosBlockAutoMargin(41, wm, false, true, &mut m);
        assert_eq!((m.BStart(wm), m.BEnd(wm)), (0, 41));
    }

    #[test]
    fn resize_flags_tell_whether_the_frame_changes_size() {
        let (root, c) = block_in_root("margin:0", 400);
        // A new frame is dirty (and in its first reflow): not a resize, it is reflowed anyway.
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert!(!ri.IsIResize() && ri.IsBResize()); // auto block-size of a dirty subtree
        assert!(ri.ShouldReflowAllKids());

        // A clean frame whose width equals the computed width: no resize.
        c.RemoveStateBits(NS_FRAME_IS_DIRTY | NS_FRAME_FIRST_REFLOW);
        c.SetRect(nsRect::new(0, 0, 400 * PX, 20 * PX));
        let ri = child_input(&rri, c);
        assert!(!ri.IsIResize() && !ri.IsBResize());
        assert!(!ri.ShouldReflowAllKids());

        // Its container gets narrower: an inline-size resize, which also resizes an auto block-size.
        let rri = root_input(root, 300);
        let ri = child_input(&rri, c);
        assert!(ri.IsIResize() && ri.IsBResize());
        assert!(ri.ShouldReflowAllKids());
    }

    #[test]
    fn a_definite_block_size_is_a_resize_only_when_it_differs_from_the_frame() {
        let (root, c) = block_in_root("height:50px", 400);
        c.RemoveStateBits(NS_FRAME_IS_DIRTY | NS_FRAME_FIRST_REFLOW);
        c.SetRect(nsRect::new(0, 0, 400 * PX, 50 * PX));
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert!(!ri.IsBResize());
        c.SetRect(nsRect::new(0, 0, 400 * PX, 40 * PX));
        let ri = child_input(&rri, c);
        assert!(ri.IsBResize());
    }

    #[test]
    fn the_containing_block_reflow_input_skips_inline_parents() {
        // block > inline > block: the inner block's containing block is the outer block, not the
        // inline (InitCBReflowInput, MightBeContainingBlockFor).
        let html = r#"<div id=root style="margin:0;padding:0;border:0"><span id=i style="display:inline"><div id=c></div></span></div>"#;
        let f = frames(html, &[("root", LayoutFrameType::Block), ("i", LayoutFrameType::Inline), ("c", LayoutFrameType::Block)], 400, 0);
        let rri = root_input(f[0], 400);
        let iri = child_input(&rri, f[1]);
        let cri = child_input(&iri, f[2]);
        assert!(std::ptr::eq(cri.mCBReflowInput.unwrap(), &rri));
        assert!(std::ptr::eq(iri.mCBReflowInput.unwrap(), &rri));
        assert_eq!(cri.mReflowDepth, 2);
        assert_eq!(cri.GetCBWritingMode(), rri.GetWritingMode());
        // An inline frame computes no size before its reflow.
        assert_eq!((iri.ComputedISize(), iri.ComputedBSize()), (NS_UNCONSTRAINEDSIZE, NS_UNCONSTRAINEDSIZE));
    }

    #[test]
    fn a_placeholder_gets_a_zero_size() {
        let html = r#"<div id=root><div id=p></div></div>"#;
        let f = frames(html, &[("root", LayoutFrameType::Block), ("p", LayoutFrameType::Placeholder)], 400, 0);
        let rri = root_input(f[0], 400);
        let ri = child_input(&rri, f[1]);
        assert_eq!((ri.ComputedISize(), ri.ComputedBSize()), (0, 0));
    }

    #[test]
    fn quirks_mode_resolves_a_percent_height_against_the_viewport_minus_body_and_html_margins() {
        // quirks.css / CalcQuirkContainingBlockHeight: a percentage height on a descendant of an
        // auto-height body is resolved against the canvas, minus the html and body block margins,
        // borders and padding, so that `height:100%` on the body fills the window.
        let html = r#"<div id=canvas></div><div id=html style="margin:0"><div id=body style="margin:8px"><div id=c style="height:50%"></div></div></div>"#;
        let chain = [("canvas", LayoutFrameType::Canvas), ("html", LayoutFrameType::Block), ("body", LayoutFrameType::Block), ("c", LayoutFrameType::Block)];

        let mut pc = nsPresContext::default();
        pc.SetCompatibilityMode(eCompatibility_NavQuirks);
        nsPresContext::SetCurrent(pc);
        let f = frames(html, &chain, 800, 1000);
        let wm = f[0].GetWritingMode();
        let canvas = ReflowInput::new__nsPresContext_nsIFrame_gfxContext_LogicalSize_InitFlags(&pc, f[0], gfxContext, LogicalSize::new(wm, 800 * PX, 1000 * PX), EnumSet::default());
        let html_ri = child_input(&canvas, f[1]);
        let body_ri = child_input(&html_ri, f[2]);
        let c_ri = child_input(&body_ri, f[3]);
        assert_eq!(c_ri.ComputedBSize(), (1000 - 16) * PX / 2);
        assert_eq!(c_ri.mContainingBlockSize.BSize(wm), (1000 - 16) * PX);

        // The same page in standards mode: the percentage of an auto-height block is auto.
        nsPresContext::SetCurrent(nsPresContext::default());
        let f = frames(html, &chain, 800, 1000);
        let canvas = ReflowInput::new__nsPresContext_nsIFrame_gfxContext_LogicalSize_InitFlags(&nsPresContext::Current(), f[0], gfxContext, LogicalSize::new(wm, 800 * PX, 1000 * PX), EnumSet::default());
        let html_ri = child_input(&canvas, f[1]);
        let body_ri = child_input(&html_ri, f[2]);
        let c_ri = child_input(&body_ri, f[3]);
        assert_eq!(c_ri.ComputedBSize(), NS_UNCONSTRAINEDSIZE);
    }

    #[test]
    fn rtl_direction_changes_which_margin_is_ignored_for_the_physical_sides() {
        // `direction: rtl` on the block: its inline-start is on the right. The logical margins
        // follow the writing mode, so 'margin-left:auto' is the inline-end margin.
        let html = r#"<div id=root style="margin:0;padding:0;border:0;direction:rtl"><div id=c style="width:100px;margin-left:auto;margin-right:0"></div></div>"#;
        let f = frames(html, &[("root", LayoutFrameType::Block), ("c", LayoutFrameType::Block)], 400, 0);
        let rri = root_input(f[0], 400);
        let ri = child_input(&rri, f[1]);
        let wm = ri.GetWritingMode();
        assert!(wm.IsBidiRTL());
        assert_eq!(ri.ComputedPhysicalMargin().left, 300 * PX);
        assert_eq!(ri.ComputedPhysicalMargin().right, 0);
        assert_eq!(ri.ComputedLogicalMargin(wm).IEnd(wm), 300 * PX);
    }

    #[test]
    fn computed_size_as_container_if_constrained_replaces_unconstrained_with_zero() {
        let (root, c) = block_in_root("padding:4px;border:1px solid", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        // inline-size 400 - 10 = 390 plus border-padding 10 = 400; auto block-size is 0 (no border-padding added).
        assert_eq!(ri.ComputedSizeAsContainerIfConstrained(), nsSize::new(400 * PX, 0));
        let (root, c) = block_in_root("padding:4px;border:1px solid;height:30px", 400);
        let rri = root_input(root, 400);
        let ri = child_input(&rri, c);
        assert_eq!(ri.ComputedSizeAsContainerIfConstrained(), nsSize::new(400 * PX, 40 * PX));
        assert_eq!(ri.ComputedPhysicalContentBoxRelativeToSelf(), nsRect::new(5 * PX, 5 * PX, 390 * PX, 30 * PX));
    }

    #[test]
    fn line_height_from_a_length_a_number_and_the_fallback_for_normal() {
        let html = r#"<div id=root style="font-size:10px;line-height:20px"><div id=a style="line-height:1.5"></div><div id=b style="line-height:normal"></div><div id=c></div></div>"#;
        let f = frames(html, &[("root", LayoutFrameType::Block), ("a", LayoutFrameType::Block)], 400, 0);
        let rri = root_input(f[0], 400);
        assert_eq!(rri.GetLineHeight(), 20 * PX);
        let ri = child_input(&rri, f[1]);
        // A number is multiplied with the font size: 1.5 * 10px.
        assert_eq!(ri.GetLineHeight(), 15 * PX);

        // 'normal' is the font's own line height (GetNormalLineHeight): em height plus the internal
        // and external leading of the real font, or 1.2em when the font has no leading at all.
        // Written from that rule (ReflowInput.cpp GetNormalLineHeight), with the metrics of the
        // system font that fontconfig picks for the style of the frame.
        let g = frames(html, &[("root", LayoutFrameType::Block), ("b", LayoutFrameType::Block)], 400, 0);
        let rri = root_input(g[0], 400);
        let ri = child_input(&rri, g[1]);
        let fm = nsPresContext::Current().GetMetricsFor(&g[1].StyleFont().mFont(), &nsFontMetrics_Params::default());
        let expected = if fm.InternalLeading() == 0 && fm.ExternalLeading() == 0 {
            NSToCoordRound__float(fm.EmHeight() as f32 * 1.2f32)
        } else {
            fm.EmHeight() + fm.InternalLeading() + fm.ExternalLeading()
        };
        assert_eq!(ri.GetLineHeight(), expected);
        assert_eq!(fm.EmHeight(), 10 * PX);
        assert!(expected >= 10 * PX && expected <= 16 * PX, "{expected}");

        // The used line height can be set (and is then returned unchanged).
        let mut ri = ri;
        ri.SetLineHeight(7 * PX);
        assert_eq!(ri.GetLineHeight(), 7 * PX);
    }

    #[test]
    fn the_auto_float_manager_installs_and_restores_the_float_manager() {
        let (root, _c) = block_in_root("", 400);
        let mut rri = root_input(root, 400);
        assert!(rri.mFloatManager.is_null());
        {
            let mut outer = nsAutoFloatManager::new(&mut rri);
            outer.CreateFloatManager(&nsPresContext::Current());
            assert!(!rri.mFloatManager.is_null());
            let first = rri.mFloatManager;
            {
                let mut inner = nsAutoFloatManager::new(&mut rri);
                inner.CreateFloatManager(&nsPresContext::Current());
                assert!(!rri.mFloatManager.is_null() && rri.mFloatManager != first);
            }
            // The inner helper restored the outer one.
            assert_eq!(rri.mFloatManager, first);
        }
        assert!(rri.mFloatManager.is_null());
    }

    #[test]
    fn a_child_reflow_input_inherits_the_float_manager_and_the_break_type() {
        let (root, c) = block_in_root("", 400);
        let mut rri = root_input(root, 400);
        let mut fm = nsFloatManager::new(rri.GetWritingMode());
        rri.mFloatManager = &mut fm as *mut nsFloatManager;
        rri.mBreakType = BreakType::Page;
        let ri = child_input(&rri, c);
        assert_eq!(ri.mFloatManager, rri.mFloatManager);
        assert_eq!(ri.mBreakType, BreakType::Page);
        assert!(ri.mLineLayout.is_null());
    }

    #[test]
    fn css_min_max_prefers_the_minimum_when_the_limits_cross() {
        assert_eq!(CSSMinMax(5, 1, 10), 5);
        assert_eq!(CSSMinMax(0, 1, 10), 1);
        assert_eq!(CSSMinMax(20, 1, 10), 10);
        // min > max: the minimum wins (unlike std::clamp, which asserts).
        assert_eq!(CSSMinMax(5, 8, 3), 8);
    }
}
