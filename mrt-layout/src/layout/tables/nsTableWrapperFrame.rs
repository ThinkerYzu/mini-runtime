// Translated from layout/tables/nsTableWrapperFrame.h and nsTableWrapperFrame.cpp (GECKO_REV),
// stage 2b.
//
// Rust shape (see CONVENTIONS.md and the top of nsTableCellFrame.rs): `nsTableWrapperFrame` is a
// `Copy` handle over a frame of type TableWrapper that derefs to nsIFrame (the C++ class derives from
// nsContainerFrame). The class has no data members. What is virtual in C++ (`Reflow`,
// `IntrinsicISize`, `ComputeSize`, `ComputeAutoSize`, `AppendFrames`, ...) is a `match` on the frame
// type in nsIFrame.rs / nsLineLayoutAdapter.rs / nsContainerFrame.rs; this file has the
// nsTableWrapperFrame overrides under their own names.
//
// The wrapper is the outer table box: it has the margins and the `width` the author wrote, and one or
// two children, the inner table frame (first) and an optional caption. Its Reflow reflows the caption,
// then the inner table, and puts them above each other.
//
// NOT PORTED: border-collapse (GetCollapsedBorderPadding is called as in C++ and answers nothing for a
// separated-borders table), painting (BuildDisplayList), accessibility, the style context parent
// (GetParentComputedStyle: the frame constructor gives the wrapper its style), invalidation
// (InvalidateTableFrame), pagination (the previous-in-flow case of Reflow), the cell lookup helpers
// of the nsITableLayout editor interface that need the cell map (GetCellAt and the like are kept
// where the table frame has the method). Each is marked where it occurs. Refcount logging
// (MOZ_COUNT_CTOR) is not ported.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::Deref;

// Gecko: layout/tables/nsTableWrapperFrame.h nsTableWrapperFrame
// ADAPTER: a handle (see the notes at the top of the file).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct nsTableWrapperFrame(pub nsIFrame);

impl Deref for nsTableWrapperFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/tables/nsTableWrapperFrame.h nsTableWrapperFrame::MaybeCaptionSide
pub type nsTableWrapperFrame_MaybeCaptionSide = Option<StyleCaptionSide>;

// Gecko: layout/tables/nsTableWrapperFrame.cpp NS_NewTableWrapperFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class.
pub fn NS_NewTableWrapperFrame(aFrame: nsIFrame) -> nsTableWrapperFrame {
    aFrame.SetType(LayoutFrameType::TableWrapper);
    nsTableWrapperFrame(aFrame)
}

impl nsTableWrapperFrame {
    // static_cast<nsTableWrapperFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsTableWrapperFrame {
        nsTableWrapperFrame(aFrame)
    }

    // do_QueryFrame<nsTableWrapperFrame>(frame)
    // Gecko: layout/tables/nsTableWrapperFrame.cpp NS_QUERYFRAME_HEAD(nsTableWrapperFrame)
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsTableWrapperFrame> {
        if aFrame.Type() == LayoutFrameType::TableWrapper {
            Some(nsTableWrapperFrame(aFrame))
        } else {
            None
        }
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::GetNaturalBaselineBOffset
    pub fn GetNaturalBaselineBOffset(&self, aWM: WritingMode, aBaselineGroup: BaselineSharingGroup, aExportContext: BaselineExportContext) -> Option<nscoord> {
        // Baseline is determined by row
        // (https://drafts.csswg.org/css-align-3/#baseline-export). If the row
        // direction is going to be orthogonal to the parent's writing mode, the
        // resulting baseline wouldn't be valid, so we use the fallback baseline
        // instead.
        if self.StyleDisplay().IsContainLayout() || self.GetWritingMode().IsOrthogonalTo(&aWM) {
            return None;
        }
        let innerTable = self.InnerTableFrame();
        innerTable.GetNaturalBaselineBOffset(aWM, aBaselineGroup, aExportContext).map(|aBaseline| {
            let bStart = innerTable.GetLogicalRect__WritingMode_nsSize(aWM, self.GetSize()).BStart(aWM);
            if aBaselineGroup == BaselineSharingGroup::First {
                return aBaseline + bStart;
            }
            let bEnd = bStart + innerTable.BSize(aWM);
            self.BSize(aWM) - (bEnd - aBaseline)
        })
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::AccessibleType
    // NOT PORTED: accessibility.

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::Destroy
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        self.DestroyAbsoluteFrames(aContext);
        self.nsContainerFrame_Destroy(aContext);
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::AppendFrames
    pub fn AppendFrames(&self, aListID: FrameChildListID, aFrameList: nsFrameList) {
        // We only have two child frames: the inner table and a caption frame.
        // The inner frame is provided when we're initialized, and it cannot change
        MOZ_ASSERT!(FrameChildListID::Principal == aListID, "unexpected child list");
        MOZ_ASSERT!(aFrameList.IsEmpty() || aFrameList.FirstChild().unwrap().IsTableCaption(), "Why are we appending non-caption frames?");
        self.nsContainerFrame_AppendFrames(aListID, aFrameList);
        // The presence of caption frames makes us sort our display list differently,
        // so mark us as changed for the new ordering.
        // MarkNeedsDisplayItemRebuild();
        // NOT PORTED: display lists.
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::InsertFrames
    pub fn InsertFrames(&self, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aPrevFrameLine: Option<&nsLineList_iterator>, aFrameList: nsFrameList) {
        MOZ_ASSERT!(FrameChildListID::Principal == aListID, "unexpected child list");
        MOZ_ASSERT!(aFrameList.IsEmpty() || aFrameList.FirstChild().unwrap().IsTableCaption(), "Why are we inserting non-caption frames?");
        MOZ_ASSERT!(aPrevFrame.map_or(true, |p| p.GetParent() == Some(self.0)), "inserting after sibling frame with different parent");
        self.nsContainerFrame_InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList);
        // MarkNeedsDisplayItemRebuild();
        // NOT PORTED: display lists.
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::RemoveFrame
    pub fn RemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        // We only have two child frames: the inner table and one caption frame.
        // The inner frame can't be removed so this should be the caption
        MOZ_ASSERT!(aOldFrame.IsTableCaption(), "can't remove inner frame");
        self.nsContainerFrame_RemoveFrame(aContext, aListID, aOldFrame);
        // MarkNeedsDisplayItemRebuild();
        // NOT PORTED: display lists.
    }

    // Gecko: layout/tables/nsTableWrapperFrame.h nsTableWrapperFrame::GetContentInsertionFrame
    pub fn GetContentInsertionFrame(&self) -> Option<nsIFrame> {
        self.mFrames().FirstChild().unwrap().GetContentInsertionFrame()
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::BuildDisplayList
    // NOT PORTED: painting (nsTableWrapperFrame.cpp lines 113-155 at GECKO_REV).

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::GetParentComputedStyle
    // NOT PORTED: the style of the wrapper (the ::-moz-table-wrapper pseudo-element, a child of the
    // inner table's style) is resolved by the frame constructor, not asked for later.

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        let mut iSize = nsLayoutUtils::IntrinsicForContainer(aInput.mContext, self.InnerTableFrame().0, aType, None, 0, &StyleSizeOverrides::default());

        {
            // If aFrame is a container for font size inflation, then shrink
            // wrapping inside of it should not apply font size inflation.
            // AutoMaybeDisableFontInflation an(this);
            // NOT PORTED: font size inflation is off.

            // Tables can't shrink smaller than their intrinsic minimum inline size,
            // no matter what.
            let input = IntrinsicSizeInput::new(aInput.mContext, None, None);

            // GetMinISize() returns a content-box inline size, but we need the
            // margin-box inline size as the contribution in the inline axis.
            let offset = self.InnerTableFrame().IntrinsicISizeOffsets(NS_UNCONSTRAINEDSIZE);
            let innerTableMinISize = self.InnerTableFrame().GetMinISize(&input) + offset.MarginBorderPadding();
            iSize = std::cmp::max(iSize, innerTableMinISize);
        }

        if let Some(caption) = self.GetCaption() {
            // The table wrapper's intrinsic inline size should be as least as large as
            // caption's min inline size.
            let capMinISize = nsLayoutUtils::IntrinsicForContainer(aInput.mContext, caption, IntrinsicISizeType::MinISize, None, 0, &StyleSizeOverrides::default());
            iSize = std::cmp::max(iSize, capMinISize);
        }
        iSize
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::InnerTableShrinkWrapSize
    fn InnerTableShrinkWrapSize(&self, aSizingInput: &SizeComputationInput, aTableFrame: nsTableFrame, aWM: WritingMode, aCBSize: LogicalSize, aAvailableISize: nscoord, aSizeOverrides: &StyleSizeOverrides, aFlags: ComputeSizeFlags) -> LogicalSize {
        MOZ_ASSERT!(self.InnerTableFrame() == aTableFrame);

        // AutoMaybeDisableFontInflation an(aTableFrame);
        // NOT PORTED: font size inflation is off.

        let mut collapseBorder: Option<LogicalMargin> = None;
        let mut collapsePadding: Option<LogicalMargin> = None;
        aTableFrame.GetCollapsedBorderPadding(&mut collapseBorder, &mut collapsePadding);

        let input = SizeComputationInput::new__nsIFrame_gfxContext_WritingMode_nscoord_Maybe_Maybe(aTableFrame.0, aSizingInput.mRenderingContext, aWM, aCBSize.ISize(aWM), collapseBorder, collapsePadding);
        let marginSize = LogicalSize::new__WritingMode(aWM); // Inner table doesn't have any margin
        let bpSize = input.ComputedLogicalBorderPadding(aWM).Size(aWM);

        // Note that we pass an empty caption-area here (rather than the caption's
        // actual size). This is fine because:
        //
        // 1) nsTableWrapperFrame::ComputeSize() only uses the size returned by this
        //    method (indirectly via calling nsTableWrapperFrame::ComputeAutoSize())
        //    if it get a aSizeOverrides arg containing any size overrides with
        //    mApplyOverridesVerbatim=true. The aSizeOverrides arg is passed to this
        //    method without any modifications.
        //
        // 2) With 1), that means the aSizeOverrides passing into this method should
        //    be applied to the inner table directly, so we don't need to subtract
        //    caption-area when preparing innerOverrides for
        //    nsTableFrame::ComputeSize().
        let innerOverrides = self.ComputeSizeOverridesForInnerTable(aTableFrame, aSizeOverrides, bpSize, /* aBSizeOccupiedByCaption = */ 0);
        let mut size = aTableFrame.ComputeSize(&input, aWM, aCBSize, aAvailableISize, marginSize, bpSize, &innerOverrides, aFlags).mLogicalSize;
        *size.ISize_mut(aWM) += bpSize.ISize(aWM);
        if size.BSize(aWM) != NS_UNCONSTRAINEDSIZE {
            *size.BSize_mut(aWM) += bpSize.BSize(aWM);
        }
        size
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::CaptionShrinkWrapSize
    fn CaptionShrinkWrapSize(&self, aSizingInput: &SizeComputationInput, aCaptionFrame: nsIFrame, aWM: WritingMode, aCBSize: LogicalSize, aAvailableISize: nscoord, aFlags: ComputeSizeFlags) -> LogicalSize {
        MOZ_ASSERT!(Some(aCaptionFrame) != self.mFrames().FirstChild());

        // AutoMaybeDisableFontInflation an(aCaptionFrame);
        // NOT PORTED: font size inflation is off.

        let input = SizeComputationInput::new__nsIFrame_gfxContext_WritingMode_nscoord_Maybe_Maybe(aCaptionFrame, aSizingInput.mRenderingContext, aWM, aCBSize.ISize(aWM), None, None);
        let marginSize = input.ComputedLogicalMargin(aWM).Size(aWM);
        let bpSize = input.ComputedLogicalBorderPadding(aWM).Size(aWM);

        let mut size = aCaptionFrame.ComputeSize(&input, aWM, aCBSize, aAvailableISize, marginSize, bpSize, &StyleSizeOverrides::default(), aFlags).mLogicalSize;
        *size.ISize_mut(aWM) += marginSize.ISize(aWM) + bpSize.ISize(aWM);
        if size.BSize(aWM) != NS_UNCONSTRAINEDSIZE {
            *size.BSize_mut(aWM) += marginSize.BSize(aWM) + bpSize.BSize(aWM);
        }
        size
    }

    // Create a new StyleSize by reducing the size by aAmountToReduce.
    //
    // @param aStyleSize must be a Length.
    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::ReduceStyleSizeBy
    fn ReduceStyleSizeBy(&self, aStyleSize: &StyleSize, aAmountToReduce: nscoord) -> StyleSize {
        MOZ_ASSERT!(aStyleSize.ConvertsToLength(), "Only handles 'Length' StyleSize!");
        let size = std::cmp::max(0, aStyleSize.ToLength() - aAmountToReduce);
        StyleSize::FromAppUnits(size)
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::ComputeSizeOverridesForInnerTable
    fn ComputeSizeOverridesForInnerTable(&self, aTableFrame: nsTableFrame, aWrapperSizeOverrides: &StyleSizeOverrides, aBorderPadding: LogicalSize, aBSizeOccupiedByCaption: nscoord) -> StyleSizeOverrides {
        if aWrapperSizeOverrides.mApplyOverridesVerbatim || !aWrapperSizeOverrides.HasAnyLengthOverrides() {
            // We are asked to apply the size overrides directly to the inner table, or
            // there's no 'Length' size overrides. No need to tweak the size overrides.
            return aWrapperSizeOverrides.clone();
        }

        let wm = aTableFrame.GetWritingMode();
        let mut areaOccupied = LogicalSize::new(wm, 0, aBSizeOccupiedByCaption);
        if aTableFrame.StylePosition().mBoxSizing() == StyleBoxSizing::ContentBox {
            // If the inner table frame has 'box-sizing: content', enlarge the occupied
            // area by adding border & padding because they should also be subtracted
            // from the size overrides.
            areaOccupied += aBorderPadding;
        }

        let mut innerSizeOverrides = StyleSizeOverrides::default();
        let wrapperISize = &aWrapperSizeOverrides.mStyleISize;
        if let Some(wrapperISize) = wrapperISize {
            MOZ_ASSERT!(!wrapperISize.HasPercent(), "Table doesn't support size overrides containing percentages!");
            innerSizeOverrides.mStyleISize = Some(if wrapperISize.ConvertsToLength() { self.ReduceStyleSizeBy(wrapperISize, areaOccupied.ISize(wm)) } else { wrapperISize.clone() });
        }

        let wrapperBSize = &aWrapperSizeOverrides.mStyleBSize;
        if let Some(wrapperBSize) = wrapperBSize {
            MOZ_ASSERT!(!wrapperBSize.HasPercent(), "Table doesn't support size overrides containing percentages!");
            innerSizeOverrides.mStyleBSize = Some(if wrapperBSize.ConvertsToLength() { self.ReduceStyleSizeBy(wrapperBSize, areaOccupied.BSize(wm)) } else { wrapperBSize.clone() });
        }

        innerSizeOverrides
    }

    /* virtual */
    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::ComputeSize
    pub fn ComputeSize(
        &self,
        aSizingInput: &SizeComputationInput,
        aWM: WritingMode,
        aCBSize: LogicalSize,
        aAvailableISize: nscoord,
        aMargin: LogicalSize,
        aBorderPadding: LogicalSize,
        aSizeOverrides: &StyleSizeOverrides,
        aFlags: ComputeSizeFlags,
    ) -> nsIFrame_SizeComputationResult {
        // nsContainerFrame::ComputeSize is nsIFrame::ComputeSize (nsContainerFrame does not override it)
        let mut result = self.nsIFrame_ComputeSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags);

        if aSizeOverrides.mApplyOverridesVerbatim && aSizeOverrides.HasAnyOverrides() {
            // We are asked to apply the size overrides directly to the inner table, but
            // we still want ourselves to remain 'auto'-sized and shrink-wrapping our
            // children's sizes. (Table wrapper frames always have 'auto' inline-size
            // and block-size, since we don't inherit those properties from inner table,
            // and authors can't target them with styling.)
            let size = self.ComputeAutoSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags);
            result.mLogicalSize = size;
        }

        result
    }

    /* virtual */
    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::ComputeAutoSize
    pub fn ComputeAutoSize(
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
        let kidAvailableISize = aAvailableISize - aMargin.ISize(aWM);
        NS_ASSERTION!(aBorderPadding.IsAllZero(), "Table wrapper frames cannot have borders or paddings");

        // When we're shrink-wrapping, our auto size needs to wrap around the
        // actual size of the table, which (if it is specified as a percent)
        // could be something that is not reflected in our GetMinISize and
        // GetPrefISize.  See bug 349457 for an example.
        let flags = self.CreateComputeSizeFlagsForChild();

        // Match the logic in Reflow() that sets aside space for the caption.
        let captionSide = self.GetCaptionSide();

        let innerTableSize = self.InnerTableShrinkWrapSize(aSizingInput, self.InnerTableFrame(), aWM, aCBSize, kidAvailableISize, aSizeOverrides, flags);
        if captionSide.is_none() {
            return innerTableSize;
        }
        let captionSize = self.CaptionShrinkWrapSize(aSizingInput, self.GetCaption().unwrap(), aWM, aCBSize, innerTableSize.ISize(aWM), flags);
        let iSize = std::cmp::max(innerTableSize.ISize(aWM), captionSize.ISize(aWM));
        let mut bSize = NS_UNCONSTRAINEDSIZE;
        if innerTableSize.BSize(aWM) != NS_UNCONSTRAINEDSIZE && captionSize.BSize(aWM) != NS_UNCONSTRAINEDSIZE {
            bSize = innerTableSize.BSize(aWM) + captionSize.BSize(aWM);
        }
        LogicalSize::new(aWM, iSize, bSize)
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::GetCaptionSide
    fn GetCaptionSide(&self) -> nsTableWrapperFrame_MaybeCaptionSide {
        if !self.HasCaption() {
            return None;
        }
        // ADAPTER: GetCaption()->StyleTableBorder()->mCaptionSide (nsStyleTableBorder has no accessor
        // for it yet; Stylo's `caption-side` is in its InheritedTable struct).
        Some(self.GetCaption().unwrap().Style().get_inherited_table().caption_side)
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::ComputeFinalBSize
    fn ComputeFinalBSize(&self, aInnerSize: LogicalSize, aCaptionSize: LogicalSize, aCaptionMargin: LogicalMargin, aWM: WritingMode) -> nscoord {
        // negative sizes can upset overflow-area code
        std::cmp::max(0, aInnerSize.BSize(aWM) + std::cmp::max(0, aCaptionSize.BSize(aWM) + aCaptionMargin.BStartEnd(aWM)))
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::GetCaptionOrigin
    fn GetCaptionOrigin(&self, aCaptionSide: StyleCaptionSide, aInnerSize: LogicalSize, aCaptionSize: LogicalSize, aCaptionMargin: &mut LogicalMargin, aOrigin: &mut LogicalPoint, aWM: WritingMode) {
        *aOrigin.I_mut(aWM) = 0;
        *aOrigin.B_mut(aWM) = 0;
        if (NS_UNCONSTRAINEDSIZE == aInnerSize.ISize(aWM)) || (NS_UNCONSTRAINEDSIZE == aInnerSize.BSize(aWM)) || (NS_UNCONSTRAINEDSIZE == aCaptionSize.ISize(aWM)) || (NS_UNCONSTRAINEDSIZE == aCaptionSize.BSize(aWM)) {
            return;
        }
        if !self.HasCaption() {
            return;
        }

        NS_ASSERTION!(NS_AUTOMARGIN != aCaptionMargin.IStart(aWM) && NS_AUTOMARGIN != aCaptionMargin.BStart(aWM) && NS_AUTOMARGIN != aCaptionMargin.BEnd(aWM), "The computed caption margin is auto?");

        *aOrigin.I_mut(aWM) = aCaptionMargin.IStart(aWM);

        // block-dir computation
        match aCaptionSide {
            StyleCaptionSide::Bottom => {
                *aOrigin.B_mut(aWM) = aInnerSize.BSize(aWM) + aCaptionMargin.BStart(aWM);
            }
            StyleCaptionSide::Top => {
                *aOrigin.B_mut(aWM) = aCaptionMargin.BStart(aWM);
            }
        }
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::GetInnerOrigin
    fn GetInnerOrigin(&self, aCaptionSide: &nsTableWrapperFrame_MaybeCaptionSide, aCaptionSize: LogicalSize, aCaptionMargin: LogicalMargin, aInnerSize: LogicalSize, aOrigin: &mut LogicalPoint, aWM: WritingMode) {
        NS_ASSERTION!(NS_AUTOMARGIN != aCaptionMargin.IStart(aWM) && NS_AUTOMARGIN != aCaptionMargin.IEnd(aWM), "The computed caption margin is auto?");

        *aOrigin.I_mut(aWM) = 0;
        *aOrigin.B_mut(aWM) = 0;
        if (NS_UNCONSTRAINEDSIZE == aInnerSize.ISize(aWM)) || (NS_UNCONSTRAINEDSIZE == aInnerSize.BSize(aWM)) || (NS_UNCONSTRAINEDSIZE == aCaptionSize.ISize(aWM)) || (NS_UNCONSTRAINEDSIZE == aCaptionSize.BSize(aWM)) {
            return;
        }

        // block-dir computation
        if let Some(captionSide) = aCaptionSide {
            match captionSide {
                StyleCaptionSide::Bottom => {
                    // Leave at zero.
                }
                StyleCaptionSide::Top => {
                    *aOrigin.B_mut(aWM) = aCaptionSize.BSize(aWM) + aCaptionMargin.BStartEnd(aWM);
                }
            }
        }
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::CreateComputeSizeFlagsForChild
    fn CreateComputeSizeFlagsForChild(&self) -> ComputeSizeFlags {
        // Shrink-wrap child frames by default, except if we're a stretched grid item.
        if self.IsGridItem() {
            let gridContainer = nsGridContainerFrame::from_frame(self.GetParent().unwrap());
            if gridContainer.GridItemShouldStretch(self.0, LogicalAxis::Inline) {
                return ComputeSizeFlags::default();
            }
        }
        let mut flags = ComputeSizeFlags::default();
        flags += ComputeSizeFlag::ShrinkWrap;
        flags
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::CreateReflowInputForInnerTable
    // (aBSizeOccupiedByCaption defaults to 0 in C++)
    fn CreateReflowInputForInnerTable<'a>(&self, aPresContext: &nsPresContext, aTableFrame: nsTableFrame, aOuterRI: &'a ReflowInput<'a>, aChildRI: &mut Option<ReflowInput<'a>>, aAvailISize: nscoord, aBSizeOccupiedByCaption: nscoord) {
        MOZ_ASSERT!(self.InnerTableFrame() == aTableFrame);

        let wm = aTableFrame.GetWritingMode();
        // If we have a caption occupied our content-box area, reduce the available
        // block-size by the amount.
        let mut availBSize = aOuterRI.AvailableBSize();
        if availBSize != NS_UNCONSTRAINEDSIZE {
            availBSize = std::cmp::max(0, availBSize - aBSizeOccupiedByCaption);
        }
        let availSize = LogicalSize::new(wm, aAvailISize, availBSize);

        // For inner table frames, the containing block is the same as for the outer
        // table frame.
        let mut cbSize: Option<LogicalSize> = Some(aOuterRI.mContainingBlockSize);

        // However, if we are a grid item, the CB size needs to subtract our margins
        // and the area occupied by the caption.
        //
        // Note that inner table computed margins are always zero, they're inherited
        // by the table wrapper, so we need to get our margin from aOuterRI.
        if self.IsGridItem() {
            let margin = aOuterRI.ComputedLogicalMargin(wm);
            let cb = cbSize.as_mut().unwrap();
            *cb.ISize_mut(wm) = std::cmp::max(0, cb.ISize(wm) - margin.IStartEnd(wm));
            if cb.BSize(wm) != NS_UNCONSTRAINEDSIZE {
                *cb.BSize_mut(wm) = std::cmp::max(0, cb.BSize(wm) - margin.BStartEnd(wm) - aBSizeOccupiedByCaption);
            }
        }

        let csFlags = self.CreateComputeSizeFlagsForChild();
        if !aTableFrame.IsBorderCollapse() && !aOuterRI.mStyleSizeOverrides.HasAnyOverrides() {
            // We are not border-collapsed and not given any size overrides. It's
            // sufficient to call the standard ReflowInput constructor.
            *aChildRI = Some(ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                aPresContext,
                aOuterRI,
                aTableFrame.0,
                availSize,
                cbSize,
                ReflowInput_InitFlags::new(),
                StyleSizeOverrides::default(),
                csFlags,
                std::ptr::null_mut(),
            ));
            return;
        }

        let borderPadding: LogicalMargin;
        let padding: LogicalMargin;
        {
            // Compute inner table frame's border & padding because we may need to
            // reduce the size for inner table's size overrides. We won't waste time if
            // they are not used, because we can use them directly by passing them into
            // ReflowInput::Init().
            let mut collapseBorder: Option<LogicalMargin> = None;
            let mut collapsePadding: Option<LogicalMargin> = None;
            aTableFrame.GetCollapsedBorderPadding(&mut collapseBorder, &mut collapsePadding);
            let input = SizeComputationInput::new__nsIFrame_gfxContext_WritingMode_nscoord_Maybe_Maybe(aTableFrame.0, aOuterRI.mRenderingContext, wm, cbSize.unwrap().ISize(wm), collapseBorder, collapsePadding);
            borderPadding = input.ComputedLogicalBorderPadding(wm);
            padding = input.ComputedLogicalPadding(wm);
        }

        let innerOverrides = self.ComputeSizeOverridesForInnerTable(aTableFrame, &aOuterRI.mStyleSizeOverrides, borderPadding.Size(wm), aBSizeOccupiedByCaption);

        let mut initFlags = ReflowInput_InitFlags::new();
        initFlags += ReflowInput_InitFlag::CallerWillInit;
        let mut childRI = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            aPresContext,
            aOuterRI,
            aTableFrame.0,
            availSize,
            None,
            initFlags,
            innerOverrides,
            csFlags,
            std::ptr::null_mut(),
        );
        childRI.Init(aPresContext, cbSize, Some(borderPadding - padding), Some(padding));
        *aChildRI = Some(childRI);
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::CreateReflowInputForCaption
    fn CreateReflowInputForCaption<'a>(&self, aPresContext: &nsPresContext, aCaptionFrame: nsIFrame, aOuterRI: &'a ReflowInput<'a>, aChildRI: &mut Option<ReflowInput<'a>>, aAvailISize: nscoord) {
        MOZ_ASSERT!(Some(aCaptionFrame) == self.GetCaption());

        let wm = aCaptionFrame.GetWritingMode();

        // Use unconstrained available block-size so that the caption is always
        // fully-complete.
        let availSize = LogicalSize::new(wm, aAvailISize, NS_UNCONSTRAINEDSIZE);
        let mut childRI = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            aPresContext,
            aOuterRI,
            aCaptionFrame,
            availSize,
            None,
            ReflowInput_InitFlags::new(),
            StyleSizeOverrides::default(),
            ComputeSizeFlags::default(),
            std::ptr::null_mut(),
        );

        // See if we need to reset mIsTopOfPage flag.
        if childRI.mFlags.mIsTopOfPage {
            if let Some(captionSide) = self.GetCaptionSide() {
                if captionSide == StyleCaptionSide::Bottom {
                    childRI.mFlags.mIsTopOfPage = false;
                }
            }
        }
        *aChildRI = Some(childRI);
    }

    // Reflow the child (caption or inner table frame).
    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::ReflowChild
    fn ReflowChild(&self, aPresContext: &nsPresContext, aChildFrame: nsIFrame, aChildRI: &ReflowInput, aMetrics: &mut ReflowOutput, aStatus: &mut nsReflowStatus) {
        // Using zero as containerSize here because we want consistency between
        // the GetLogicalPosition and ReflowChild calls, to avoid unnecessarily
        // changing the frame's coordinates; but we don't yet know its final
        // position anyway so the actual value is unimportant.
        let zeroCSize = nsSize::new(0, 0);
        let wm = aChildRI.GetWritingMode();

        // Use the current position as a best guess for placement.
        let childPt = aChildFrame.GetLogicalPosition__WritingMode_nsSize(wm, zeroCSize);
        let mut flags = nsIFrame_ReflowChildFlags::NoMoveFrame;

        // We don't want to delete our next-in-flow's child if it's an inner table
        // frame, because table wrapper frames always assume that their inner table
        // frames don't go away. If a table wrapper frame is removed because it is
        // a next-in-flow of an already complete table wrapper frame, then it will
        // take care of removing it's inner table frame.
        if aChildFrame == self.InnerTableFrame().0 {
            flags |= nsIFrame_ReflowChildFlags::NoDeleteNextInFlowChild;
        }

        self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(aChildFrame, aPresContext, aMetrics, aChildRI, wm, childPt, zeroCSize, flags, aStatus, None);
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::UpdateOverflowAreas
    fn UpdateOverflowAreas(&self, aMet: &mut ReflowOutput) {
        aMet.SetOverflowAreasToDesiredBounds();
        for frame in self.mFrames().begin() {
            self.ConsiderChildOverflow(&mut aMet.mOverflowAreas, frame, OverflowAreaUnionFlags::None);
        }
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aOuterRI: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        // DO_GLOBAL_REFLOW_COUNT("nsTableWrapperFrame");  NOT PORTED: reflow counts.
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        // Initialize out parameters
        aDesiredSize.ClearSize();

        if !self.HasAnyStateBits(NS_FRAME_FIRST_REFLOW) {
            // Set up our kids.  They're already present, on an overflow list,
            // or there are none so we'll create them now
            self.MoveOverflowToChildList();
        }

        let mut captionRI: Option<ReflowInput> = None;
        let mut innerRI: Option<ReflowInput> = None;

        // nsRect origCaptionRect; nsRect origCaptionInkOverflow; bool captionFirstReflow = false;
        // if (nsIFrame* caption = GetCaption()) { ... }
        // NOT PORTED: invalidation (these are only used by InvalidateTableFrame at the end).

        // ComputeAutoSize has to match this logic.
        let wm = aOuterRI.GetWritingMode();
        let captionSide = self.GetCaptionSide();
        let contentBoxISize = aOuterRI.ComputedSize__WritingMode(wm).ISize(wm);

        MOZ_ASSERT!(self.HasCaption() == captionSide.is_some());

        // Compute the table's size first, and then prevent the caption from
        // being larger in the inline dir unless it has to be.
        //
        // Note that CSS 2.1 (but not 2.0) says:
        //   The width of the anonymous box is the border-edge width of the
        //   table box inside it
        // We don't actually make our anonymous box that isize (if we did,
        // it would break 'auto' margins), but this effectively does that.
        self.CreateReflowInputForInnerTable(aPresContext, self.InnerTableFrame(), aOuterRI, &mut innerRI, contentBoxISize, 0);

        // First reflow the caption.
        let mut captionMet = ReflowOutput::new__WritingMode(wm);
        let mut captionSize = LogicalSize::new__WritingMode(wm);
        let mut captionMargin = LogicalMargin::new__WritingMode(wm);
        if captionSide.is_some() {
            // It's good that CSS 2.1 says not to include margins, since we can't, since
            // they already been converted so they exactly fill the available isize
            // (ignoring the margin on one side if neither are auto).  (We take
            // advantage of that later when we call GetCaptionOrigin, though.)
            let innerBorderISize = innerRI.as_ref().unwrap().ComputedSizeWithBorderPadding(wm).ISize(wm);
            self.CreateReflowInputForCaption(aPresContext, self.GetCaption().unwrap(), aOuterRI, &mut captionRI, innerBorderISize);

            // We intentionally don't merge capStatus into aStatus, since we currently
            // can't handle caption continuations, but we probably should.
            let mut capStatus = nsReflowStatus::new();
            self.ReflowChild(aPresContext, self.GetCaption().unwrap(), captionRI.as_ref().unwrap(), &mut captionMet, &mut capStatus);
            captionSize = captionMet.Size(wm);
            captionMargin = captionRI.as_ref().unwrap().ComputedLogicalMargin(wm);
            let bSizeOccupiedByCaption = captionSize.BSize(wm) + captionMargin.BStartEnd(wm);
            if bSizeOccupiedByCaption != 0 {
                // Reset the inner table's ReflowInput to reduce various sizes because of
                // the area occupied by caption.
                innerRI = None;
                self.CreateReflowInputForInnerTable(aPresContext, self.InnerTableFrame(), aOuterRI, &mut innerRI, contentBoxISize, bSizeOccupiedByCaption);
            }
        }

        // Now we know how much to reduce the block-size for the inner table to
        // account for captions. Reflow the inner table.
        let mut innerMet = ReflowOutput::new__WritingMode(innerRI.as_ref().unwrap().GetWritingMode());
        self.ReflowChild(aPresContext, self.InnerTableFrame().0, innerRI.as_ref().unwrap(), &mut innerMet, aStatus);
        let innerSize = LogicalSize::new(wm, innerMet.ISize(wm), innerMet.BSize(wm));

        // Now that we've reflowed both we can place them.
        // Compute the desiredSize so that we can use it as the containerSize
        // for the FinishReflowChild calls below.
        let mut desiredSize = LogicalSize::new__WritingMode(wm);

        // We have zero border and padding, so content-box inline-size is our desired
        // border-box inline-size.
        *desiredSize.ISize_mut(wm) = contentBoxISize;
        *desiredSize.BSize_mut(wm) = self.ComputeFinalBSize(innerSize, captionSize, captionMargin, wm);

        aDesiredSize.SetSize(wm, &desiredSize);
        let containerSize = aDesiredSize.PhysicalSize();

        MOZ_ASSERT!(self.HasCaption() == captionSide.is_some());
        if let Some(caption) = self.GetCaption() {
            let mut captionOrigin = LogicalPoint::new__WritingMode(wm);
            self.GetCaptionOrigin(captionSide.unwrap(), innerSize, captionSize, &mut captionMargin, &mut captionOrigin, wm);
            nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
                caption,
                aPresContext,
                &captionMet,
                captionRI.as_ref(),
                wm,
                captionOrigin,
                containerSize,
                nsIFrame_ReflowChildFlags::ApplyRelativePositioning,
            );
            captionRI = None;
        }
        // XXX If the bsize is constrained then we need to check whether
        // everything still fits...

        let mut innerOrigin = LogicalPoint::new__WritingMode(wm);
        self.GetInnerOrigin(&captionSide, captionSize, captionMargin, innerSize, &mut innerOrigin, wm);
        // NOTE: Relative positioning on the table applies to the whole table wrapper.
        nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
            self.InnerTableFrame().0,
            aPresContext,
            &innerMet,
            innerRI.as_ref(),
            wm,
            innerOrigin,
            containerSize,
            nsIFrame_ReflowChildFlags::Default,
        );
        innerRI = None;

        if self.HasCaption() {
            // nsTableFrame::InvalidateTableFrame(GetCaption(), origCaptionRect, origCaptionInkOverflow, captionFirstReflow);
            // NOT PORTED: invalidation.
        }

        self.UpdateOverflowAreas(aDesiredSize);

        if self.GetPrevInFlow().is_some() {
            // ReflowOverflowContainerChildren(aPresContext, aOuterRI, aDesiredSize.mOverflowAreas, ReflowChildFlags::Default, aStatus);
            // NOT PORTED: pagination (a wrapper has a previous-in-flow only when a table is split).
            unimplemented!("NOT PORTED: pagination (nsTableWrapperFrame continuations)")
        }

        self.FinishReflowWithAbsoluteFrames(aPresContext, aDesiredSize, aOuterRI, aStatus);
    }

    /* ----- global methods ----- */

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::GetCellAt
    pub fn GetCellAt(&self, aRowIdx: u32, aColIdx: u32) -> Option<nsIContent> {
        let cellMap = self.InnerTableFrame().GetCellMap();
        let cellMap = cellMap?;

        let cell = cellMap.GetCellInfoAt(aRowIdx as i32, aColIdx as i32, None, None)?;

        cell.GetContent()
    }

    // Gecko: layout/tables/nsTableWrapperFrame.cpp nsTableWrapperFrame::GetFrameName (DEBUG_FRAME_DUMP)
    // NOT PORTED: frame dumps.

    // Return the number of rows in the table.
    // Gecko: layout/tables/nsTableWrapperFrame.h nsTableWrapperFrame::GetRowCount
    pub fn GetRowCount(&self) -> i32 {
        self.InnerTableFrame().GetRowCount()
    }

    // Return the number of columns in the table.
    // Gecko: layout/tables/nsTableWrapperFrame.h nsTableWrapperFrame::GetColCount
    pub fn GetColCount(&self) -> i32 {
        self.InnerTableFrame().GetColCount()
    }

    // Gecko: layout/tables/nsTableWrapperFrame.h nsTableWrapperFrame::HasCaption
    pub fn HasCaption(&self) -> bool {
        self.mFrames().OnlyChild().is_none()
    }

    // Gecko: layout/tables/nsTableWrapperFrame.h nsTableWrapperFrame::GetCaption
    pub fn GetCaption(&self) -> Option<nsIFrame> {
        if self.HasCaption() { self.mFrames().FirstChild().unwrap().GetNextSibling() } else { None }
    }

    // Always non-null unless we are mid-destruction.
    // Gecko: layout/tables/nsTableWrapperFrame.h nsTableWrapperFrame::InnerTableFrame
    pub fn InnerTableFrame(&self) -> nsTableFrame {
        nsTableFrame::from_frame(self.mFrames().FirstChild().unwrap())
    }
}

// Gecko: layout/tables/nsTableWrapperFrame.h nsTableWrapperFrame::GetIndexByRowAndColumn,
// GetRowAndColumnByIndex, GetCellFrameAt, GetEffectiveColSpanAt, GetEffectiveRowSpanAt
// NOT PORTED: the nsITableLayout editor interface (nothing in layout calls them).

//////////////////////////////////////////////////////////////////////////////////////////
// TEMP stubs
//
// Everything below is NOT part of nsTableWrapperFrame.h/.cpp: members of nsIFrame and the style
// structs that the wrapper calls and no other translated file has yet. When the real member lands in
// its own file, delete the item here (a duplicate definition fails to compile).
//////////////////////////////////////////////////////////////////////////////////////////

// Gecko: generated by cbindgen from Stylo (no header): mozilla::StyleCaptionSide
pub type StyleCaptionSide = mrt_style::style::computed_values::caption_side::T;

impl nsIFrame {
    // Gecko: layout/generic/nsIFrameInlines.h nsIFrame::IsTableCaption
    // ADAPTER: `GetParent()->Style()->GetPseudoType() == PseudoStyleType::MozTableWrapper` is
    // "the parent is a table wrapper frame" (the servo configuration has no such pseudo type).
    pub fn IsTableCaption(&self) -> bool {
        self.StyleDisplay().DisplayOutside() == StyleDisplayOutside::TableCaption && self.GetParent().map_or(false, |p| p.Type() == LayoutFrameType::TableWrapper)
    }
}

// Tests for nsTableWrapperFrame, nsTableRowGroupFrame, nsTableRowFrame and nsTableCellFrame:
// Firefox has no gtest for them (layout/tables has no gtest directory), so the tests here are written
// from the documented behaviour (CSS 2.2 section 17 tables and 10.3.3 block widths; the comments of
// the C++ files) on real Stylo styles with hand-built frame trees. This module builds the trees; the
// tests of each file are in a `mod tests` next to it.
#[cfg(test)]
pub mod nsTableWrapperFrame_test_support {
    use crate::BlockReflowState_test_support::*;
    use crate::*;

    pub const AUTO: nscoord = NS_UNCONSTRAINEDSIZE;

    // One cell of a test table.
    #[derive(Clone)]
    pub struct TCell {
        // the css of the cell box ("padding:5px", "vertical-align:middle", ...)
        pub css: String,
        // the content: a block of this width and height (px); width None is an auto width block
        // whose min-content and max-content widths are `intrinsic`
        pub width: Option<i32>,
        pub intrinsic: Option<(i32, i32)>,
        pub height: i32,
        pub colspan: i32,
        pub rowspan: i32,
        // the css of the row box, taken from the first cell of the row
        pub row_css: String,
        // the css of the row group box, taken from the first cell of the first row
        pub group_css: String,
    }

    // A cell whose content is a block of `w` by `h` px.
    pub fn tcell(w: i32, h: i32) -> TCell {
        TCell { css: String::new(), width: Some(w), intrinsic: None, height: h, colspan: 1, rowspan: 1, row_css: String::new(), group_css: String::new() }
    }

    impl TCell {
        pub fn css(mut self, aCss: &str) -> TCell {
            self.css = aCss.to_string();
            self
        }
        // the css of the row this cell is the first of ("height:50px", "visibility:collapse", ...)
        pub fn row_css(mut self, aCss: &str) -> TCell {
            self.row_css = aCss.to_string();
            self
        }
        // the css of the row group (give it to the first cell of the first row)
        pub fn group_css(mut self, aCss: &str) -> TCell {
            self.group_css = aCss.to_string();
            self
        }
        pub fn span(mut self, aColSpan: i32, aRowSpan: i32) -> TCell {
            self.colspan = aColSpan;
            self.rowspan = aRowSpan;
            self
        }
        // an auto-width content with these min-content and max-content widths (px)
        pub fn flex(mut self, aMin: i32, aPref: i32) -> TCell {
            self.width = None;
            self.intrinsic = Some((aMin, aPref));
            self
        }
    }

    pub struct TTable {
        pub root: nsIFrame,
        pub wrapper: nsTableWrapperFrame,
        pub table: nsTableFrame,
        pub group: nsTableRowGroupFrame,
        pub caption: Option<nsIFrame>,
        pub rows: Vec<nsTableRowFrame>,
        pub cells: Vec<Vec<nsTableCellFrame>>,
        // the anonymous cell-content block of each cell, and the content block in it
        pub inners: Vec<Vec<nsIFrame>>,
        pub contents: Vec<Vec<nsIFrame>>,
    }

    // Builds `<div root><div wrapper><div table><div group><div row><div cell><div inner><div content>`
    // for the rows of cells in `aRows` (and a caption block of this css and height, after the table),
    // and does what the frame constructor does: the class of each frame, Init, and SetInitialChildList
    // from the leaves up (so that the table's SetInitialChildList builds the cell map from complete
    // rows). `aRootWidth` is the width of the root block in px; the table has no border spacing unless
    // `aTableCss` says so.
    pub fn build_table(aRootWidth: i32, aRootCss: &str, aWrapperCss: &str, aTableCss: &str, aCaption: Option<(&str, i32)>, aRows: &[Vec<TCell>]) -> TTable {
        let groupCss = aRows.first().and_then(|r| r.first()).map_or("", |c| c.group_css.as_str());
        let mut html = format!(r#"<div id=root style="margin:0;padding:0;border:0;display:flow-root;{aRootCss}"><div id=w style="{aWrapperCss}"><div id=t style="display:table;margin:0;padding:0;border:0;border-spacing:0;{aTableCss}"><div id=g style="display:table-row-group;{groupCss}">"#);
        let mut tree: Vec<(String, Option<usize>)> = vec![("root".into(), None), ("w".into(), Some(0)), ("t".into(), Some(1)), ("g".into(), Some(2))];
        // (kind, row index, cell index): r = row, c = cell, i = inner block, k = content
        let mut kinds: Vec<(char, usize, usize)> = vec![('o', 0, 0), ('w', 0, 0), ('t', 0, 0), ('g', 0, 0)];
        for (r, cells) in aRows.iter().enumerate() {
            let rowCss = cells.first().map_or("", |c| c.row_css.as_str());
            html += &format!(r#"<div id=r{r} style="display:table-row;{rowCss}">"#);
            let rowFrame = tree.len();
            tree.push((format!("r{r}"), Some(3)));
            kinds.push(('r', r, 0));
            for (c, cell) in cells.iter().enumerate() {
                html += &format!(r#"<div id=c{r}x{c} style="display:table-cell;padding:0;border:0;{}"><div id=i{r}x{c} style="margin:0;padding:0;border:0">"#, cell.css);
                let cellFrame = tree.len();
                tree.push((format!("c{r}x{c}"), Some(rowFrame)));
                kinds.push(('c', r, c));
                tree.push((format!("i{r}x{c}"), Some(cellFrame)));
                kinds.push(('i', r, c));
                let widthCss = match cell.width {
                    Some(w) => format!("width:{w}px;"),
                    None => String::new(),
                };
                html += &format!(r#"<div id=k{r}x{c} style="margin:0;{widthCss}height:{}px"></div></div></div>"#, cell.height);
                tree.push((format!("k{r}x{c}"), Some(cellFrame + 1)));
                kinds.push(('k', r, c));
            }
            html += "</div>";
        }
        // closes the row group and the table; the caption is the wrapper's second child, after the table
        html += "</div></div>";
        if let Some((css, h)) = aCaption {
            html += &format!(r#"<div id=cap style="display:table-caption;margin:0;height:{h}px;{css}"></div>"#);
        }
        // closes the wrapper and the root
        html += "</div></div>";
        if aCaption.is_some() {
            tree.push(("cap".into(), Some(1)));
            kinds.push(('p', 0, 0));
        }
        let treeRef: Vec<(&str, Option<usize>)> = tree.iter().map(|(n, p)| (n.as_str(), *p)).collect();
        let f = frames(&html, &treeRef, aRootWidth);
        let table = NS_NewTableFrame(f[2]);
        let wrapper = NS_NewTableWrapperFrame(f[1]);
        let group = NS_NewTableRowGroupFrame(f[3]);
        let mut rows: Vec<nsTableRowFrame> = Vec::new();
        let mut cells: Vec<Vec<nsTableCellFrame>> = vec![Vec::new(); aRows.len()];
        let mut inners: Vec<Vec<nsIFrame>> = vec![Vec::new(); aRows.len()];
        let mut contents: Vec<Vec<nsIFrame>> = vec![Vec::new(); aRows.len()];
        let mut caption = None;
        for (i, &(kind, r, c)) in kinds.iter().enumerate() {
            match kind {
                'r' => rows.push(NS_NewTableRowFrame(f[i])),
                'c' => {
                    let cellFrame = NS_NewTableCellFrame(f[i], table);
                    cellFrame.SetSpans(aRows[r][c].rowspan, aRows[r][c].colspan);
                    cells[r].push(cellFrame);
                }
                'i' => {
                    f[i].SetIsBlockWrapper(true);
                    inners[r].push(f[i]);
                }
                'k' => {
                    if let Some((min, pref)) = aRows[r][c].intrinsic {
                        f[i].SetIntrinsicISizes(min * PX, pref * PX);
                    }
                    contents[r].push(f[i]);
                }
                'p' => caption = Some(f[i]),
                _ => {}
            }
        }
        // Init, parents first
        for (i, &(kind, _, _)) in kinds.iter().enumerate() {
            let parent = tree[i].1.map(|p| f[p]);
            f[i].SetContent(Some(nsIContent::HTMLElement("div")));
            match kind {
                'o' | 'i' | 'k' | 'p' => nsBlockFrame::from_frame(f[i]).Init(f[i].GetContent(), parent.unwrap_or(f[i]), None),
                't' => table.Init(f[i].GetContent(), parent.unwrap(), None),
                'g' => group.Init(f[i].GetContent(), parent.unwrap(), None),
                'r' => nsTableRowFrame::from_frame(f[i]).Init(f[i].GetContent(), parent.unwrap(), None),
                'c' => nsTableCellFrame::from_frame(f[i]).Init(f[i].GetContent(), parent.unwrap(), None),
                _ => {}
            }
        }
        // SetInitialChildList, leaves first
        for i in (0..kinds.len()).rev() {
            if kinds[i].0 == 'k' || kinds[i].0 == 'p' {
                continue;
            }
            let list = std::mem::take(&mut *f[i].mFrames());
            if list.NotEmpty() {
                f[i].SetInitialChildList(FrameChildListID::Principal, list);
            }
        }
        TTable { root: f[0], wrapper, table, group, caption, rows, cells, inners, contents }
    }

    // Reflows the root block with this available inline-size (px) and unconstrained block-size, and
    // sets its size from the result, as its parent would.
    pub fn layout_root(aTable: &TTable, aAvail: i32) -> ReflowOutput {
        let ri = root_input(aTable.root, aAvail, AUTO);
        let wm = aTable.root.GetWritingMode();
        let mut metrics = ReflowOutput::new__ReflowInput(&ri);
        let mut status = nsReflowStatus::new();
        aTable.root.Reflow(&nsPresContext::Current(), &mut metrics, &ri, &mut status);
        assert!(status.IsFullyComplete());
        aTable.root.SetSize__WritingMode_LogicalSize(wm, metrics.Size(wm));
        metrics
    }
}

#[cfg(test)]
mod tests {
    use super::nsTableWrapperFrame_test_support::*;
    use crate::BlockReflowState_test_support::*;
    use crate::*;

    #[test]
    fn the_wrapper_is_as_wide_as_the_table_and_as_tall_as_the_table_without_a_caption() {
        // CSS 2.2 17.4: the table wrapper box contains the table box (and the caption box), and its
        // width is the width of the table box (a shrink-to-fit table is as wide as its columns).
        let t = build_table(500, "", "", "", None, &[vec![tcell(50, 10), tcell(30, 10)]]);
        let m = layout_root(&t, 500);
        let wm = t.root.GetWritingMode();
        assert_eq!(t.wrapper.GetSize(), nsSize::new(80 * PX, 10 * PX));
        assert_eq!(t.table.GetPosition(), nsPoint::new(0, 0));
        assert_eq!(t.table.GetSize(), nsSize::new(80 * PX, 10 * PX));
        assert_eq!(m.BSize(wm), 10 * PX);
    }

    #[test]
    fn a_table_with_a_width_makes_the_wrapper_that_wide() {
        // CSS 2.2 17.5.2: 'width' on the table sets the table box width (the columns grow to fill it).
        let t = build_table(500, "", "", "width:200px", None, &[vec![tcell(50, 10), tcell(30, 10)]]);
        layout_root(&t, 500);
        assert_eq!(t.wrapper.GetSize().width, 200 * PX);
        assert_eq!(t.table.GetSize().width, 200 * PX);
    }

    #[test]
    fn wrapper_margins_are_outside_the_table() {
        // CSS 2.2 17.4: the margins of the table apply to the wrapper box, outside the table box.
        let t = build_table(500, "", "margin:10px 20px 30px 40px", "", None, &[vec![tcell(50, 10)]]);
        let m = layout_root(&t, 500);
        let wm = t.root.GetWritingMode();
        assert_eq!(t.wrapper.GetPosition(), nsPoint::new(40 * PX, 10 * PX));
        assert_eq!(t.wrapper.GetSize(), nsSize::new(50 * PX, 10 * PX));
        assert_eq!(t.table.GetPosition(), nsPoint::new(0, 0));
        assert_eq!(m.BSize(wm), (10 + 10 + 30) * PX);
    }

    #[test]
    fn auto_side_margins_center_the_table() {
        // CSS 2.2 10.3.3: for a block-level box whose width is not auto, auto margins share the free
        // space equally; the wrapper's width is the table's, so `margin: 0 auto` centers the table.
        let t = build_table(500, "", "margin:0 auto", "width:200px", None, &[vec![tcell(50, 10)]]);
        layout_root(&t, 500);
        assert_eq!(t.wrapper.GetPosition().x, 150 * PX);
        assert_eq!(t.wrapper.GetSize().width, 200 * PX);
    }

    #[test]
    fn a_table_wider_than_its_container_is_not_squeezed_below_its_minimum() {
        // CSS 2.2 17.5.2.2: a table is never narrower than the sum of its columns' minimum widths,
        // even when the containing block is narrower; the content overflows.
        let t = build_table(100, "", "", "", None, &[vec![tcell(80, 10), tcell(60, 10)]]);
        layout_root(&t, 100);
        assert_eq!(t.wrapper.GetSize().width, 140 * PX);
    }

    #[test]
    fn the_wrapper_asks_the_table_for_its_intrinsic_inline_sizes() {
        // nsTableWrapperFrame::IntrinsicISize: the wrapper's contribution is the table's, and it is
        // never smaller than the table's minimum (CSS 2.2 17.5.2.2).
        let t = build_table(500, "", "", "", None, &[vec![tcell(50, 10).flex(20, 50), tcell(30, 10).flex(10, 30)]]);
        layout_root(&t, 500);
        let input = IntrinsicSizeInput::new(Some(gfxContext), None, None);
        assert_eq!(t.wrapper.GetMinISize(&input), 30 * PX);
        assert_eq!(t.wrapper.GetPrefISize(&input), 80 * PX);
    }

    #[test]
    fn a_caption_on_top_is_above_the_table() {
        // CSS 2.2 17.4.1: 'caption-side: top' puts the caption box above the table box, inside the
        // wrapper; the wrapper is as tall as both and as wide as the wider of the two.
        let t = build_table(500, "", "", "", Some(("caption-side:top", 20)), &[vec![tcell(50, 10)]]);
        layout_root(&t, 500);
        let cap = t.caption.unwrap();
        assert_eq!(cap.GetPosition(), nsPoint::new(0, 0));
        assert_eq!(cap.GetSize().height, 20 * PX);
        assert_eq!(t.table.GetPosition(), nsPoint::new(0, 20 * PX));
        assert_eq!(t.wrapper.GetSize(), nsSize::new(50 * PX, 30 * PX));
    }

    #[test]
    fn a_caption_at_the_bottom_is_below_the_table() {
        // CSS 2.2 17.4.1: 'caption-side: bottom' puts the caption box below the table box.
        let t = build_table(500, "", "", "", Some(("caption-side:bottom", 20)), &[vec![tcell(50, 10)]]);
        layout_root(&t, 500);
        let cap = t.caption.unwrap();
        assert_eq!(t.table.GetPosition(), nsPoint::new(0, 0));
        assert_eq!(cap.GetPosition(), nsPoint::new(0, 10 * PX));
        assert_eq!(t.wrapper.GetSize(), nsSize::new(50 * PX, 30 * PX));
    }

    #[test]
    fn the_caption_is_as_wide_as_the_table() {
        // CSS 2.2 17.4.1: the caption box is a block box whose width is the table box width.
        let t = build_table(500, "", "", "width:200px", Some(("caption-side:top", 20)), &[vec![tcell(50, 10)]]);
        layout_root(&t, 500);
        assert_eq!(t.caption.unwrap().GetSize().width, 200 * PX);
    }

    #[test]
    fn the_wrapper_and_inner_table_are_found_through_each_other() {
        let t = build_table(500, "", "", "", Some(("", 5)), &[vec![tcell(50, 10)]]);
        assert_eq!(t.wrapper.InnerTableFrame(), t.table);
        assert!(t.wrapper.HasCaption());
        assert_eq!(t.wrapper.GetCaption(), t.caption);
        assert_eq!(t.wrapper.GetRowCount(), 1);
        assert_eq!(t.wrapper.GetColCount(), 1);
        assert!(nsTableWrapperFrame::do_QueryFrame(t.wrapper.0).is_some());
        assert!(nsTableWrapperFrame::do_QueryFrame(t.table.0).is_none());
        let t2 = build_table(500, "", "", "", None, &[vec![tcell(50, 10)]]);
        assert!(!t2.wrapper.HasCaption());
        assert_eq!(t2.wrapper.GetCaption(), None);
    }
}
