//! Layout translated from Firefox's C++. See CONVENTIONS.md.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, unused)]

#[macro_use]
#[path = "mfbt/Assertions.rs"]
mod Assertions;
#[macro_use]
#[path = "mfbt/TypedEnumBits.rs"]
mod TypedEnumBits;

#[path = "xpcom/nsError.rs"]
mod nsError_rs;
pub use nsError_rs::*;
#[path = "gfx/2d/Types.rs"]
mod Types_rs;
pub use Types_rs::*;
#[path = "gfx/src/nsCoord.rs"]
mod nsCoord_rs;
pub use nsCoord_rs::*;
#[macro_use]
#[path = "gfx/2d/BasePoint.rs"]
mod BasePoint_rs;
pub use BasePoint_rs::*;
#[macro_use]
#[path = "gfx/2d/BaseSize.rs"]
mod BaseSize_rs;
pub use BaseSize_rs::*;
#[macro_use]
#[path = "gfx/2d/BaseMargin.rs"]
mod BaseMargin_rs;
pub use BaseMargin_rs::*;
#[macro_use]
#[path = "gfx/2d/BaseRect.rs"]
mod BaseRect_rs;
pub use BaseRect_rs::*;
#[path = "gfx/2d/Rect.rs"]
mod Rect_rs;
pub use Rect_rs::*;
#[path = "gfx/2d/Matrix.rs"]
mod Matrix_rs;
pub use Matrix_rs::*;
#[path = "gfx/src/nsPoint.rs"]
mod nsPoint_rs;
pub use nsPoint_rs::*;
#[path = "gfx/src/nsSize.rs"]
mod nsSize_rs;
pub use nsSize_rs::*;
#[path = "gfx/src/nsMargin.rs"]
mod nsMargin_rs;
pub use nsMargin_rs::*;
#[path = "gfx/src/nsRect.rs"]
mod nsRect_rs;
pub use nsRect_rs::*;
#[path = "intl/components/src/BidiEmbeddingLevel.rs"]
mod BidiEmbeddingLevel_rs;
pub use BidiEmbeddingLevel_rs::*;
#[path = "intl/components/src/Bidi.rs"]
mod Bidi_rs;
pub use Bidi_rs::*;
#[path = "layout/generic/WritingModes.rs"]
mod WritingModes_rs;
pub use WritingModes_rs::*;
#[path = "layout/style/nsStyleStruct.rs"]
mod nsStyleStruct_rs;
pub use nsStyleStruct_rs::*;
#[path = "layout/style/nsStyleTransformMatrix.rs"]
mod nsStyleTransformMatrix_rs;
pub use nsStyleTransformMatrix_rs::*;
#[path = "layout/base/FrameProperties.rs"]
mod FrameProperties_rs;
pub use FrameProperties_rs::*;
#[path = "layout/generic/nsIFrame.rs"]
mod nsIFrame_rs;
pub use nsIFrame_rs::*;
#[path = "layout/generic/nsIntervalSet.rs"]
mod nsIntervalSet_rs;
pub use nsIntervalSet_rs::*;
#[path = "layout/generic/nsFloatManager.rs"]
mod nsFloatManager_rs;
pub use nsFloatManager_rs::*;
#[path = "layout/base/LayoutConstants.rs"]
mod LayoutConstants_rs;
pub use LayoutConstants_rs::*;
#[path = "gfx/thebes/gfxContext.rs"]
mod gfxContext_rs;
pub use gfxContext_rs::*;
#[path = "layout/style/ServoStyleConsts.rs"]
mod ServoStyleConsts_rs;
pub use ServoStyleConsts_rs::*;
#[path = "layout/base/nsPresContext.rs"]
mod nsPresContext_rs;
pub use nsPresContext_rs::*;
#[path = "layout/generic/nsFrameState.rs"]
mod nsFrameState_rs;
pub use nsFrameState_rs::*;
#[path = "layout/generic/LayoutFrameType.rs"]
mod LayoutFrameType_rs;
pub use LayoutFrameType_rs::*;
#[path = "layout/generic/AspectRatio.rs"]
mod AspectRatio_rs;
pub use AspectRatio_rs::*;
#[path = "layout/base/LayoutStructs.rs"]
mod LayoutStructs_rs;
pub use LayoutStructs_rs::*;
#[path = "layout/base/nsBidiPresUtils.rs"]
mod nsBidiPresUtils_rs;
pub use nsBidiPresUtils_rs::*;
#[path = "layout/base/nsLayoutUtils.rs"]
mod nsLayoutUtils_rs;
pub use nsLayoutUtils_rs::*;
#[path = "layout/base/nsIPercentBSizeObserver.rs"]
mod nsIPercentBSizeObserver_rs;
pub use nsIPercentBSizeObserver_rs::*;
#[path = "gfx/src/nsBoundingMetrics.rs"]
mod nsBoundingMetrics_rs;
pub use nsBoundingMetrics_rs::*;
#[path = "layout/generic/ReflowOutput.rs"]
mod ReflowOutput_rs;
pub use ReflowOutput_rs::*;
#[path = "layout/generic/ReflowInput.rs"]
mod ReflowInput_rs;
pub use ReflowInput_rs::*;
#[path = "layout/generic/nsBlockFrame.rs"]
mod nsBlockFrame_rs;
pub use nsBlockFrame_rs::*;
#[path = "layout/generic/BlockReflowState.rs"]
mod BlockReflowState_rs;
pub use BlockReflowState_rs::*;
#[path = "layout/generic/nsBlockReflowContext.rs"]
mod nsBlockReflowContext_rs;
pub use nsBlockReflowContext_rs::*;
#[path = "layout/generic/nsIFrameLists.rs"]
mod nsIFrameLists_rs;
pub use nsIFrameLists_rs::*;
#[path = "layout/generic/nsILineIterator.rs"]
mod nsILineIterator_rs;
pub use nsILineIterator_rs::*;
#[path = "layout/generic/nsFrameList.rs"]
mod nsFrameList_rs;
pub use nsFrameList_rs::*;
#[path = "layout/generic/nsLineBox.rs"]
mod nsLineBox_rs;
pub use nsLineBox_rs::*;
#[path = "layout/generic/JustificationUtils.rs"]
mod JustificationUtils_rs;
pub use JustificationUtils_rs::*;
#[path = "layout/generic/nsLineLayoutAdapter.rs"]
pub mod nsLineLayoutAdapter;
#[path = "layout/generic/nsLineLayout.rs"]
mod nsLineLayout_rs;
pub use nsLineLayout_rs::*;
#[path = "layout/generic/IntrinsicISizesCache.rs"]
mod IntrinsicISizesCache_rs;
pub use IntrinsicISizesCache_rs::*;
#[path = "layout/generic/nsContainerFrame.rs"]
mod nsContainerFrame_rs;
pub use nsContainerFrame_rs::*;
#[path = "layout/generic/nsInlineFrame.rs"]
mod nsInlineFrame_rs;
pub use nsInlineFrame_rs::*;
#[path = "layout/generic/nsPlaceholderFrame.rs"]
mod nsPlaceholderFrame_rs;
pub use nsPlaceholderFrame_rs::*;
#[path = "gfx/2d/NumericTools.rs"]
mod NumericTools_rs;
pub use NumericTools_rs::*;
#[path = "layout/forms/nsFormControlAdapter.rs"]
mod nsFormControlAdapter_rs;
pub use nsFormControlAdapter_rs::*;
#[path = "layout/forms/nsCheckboxRadioFrame.rs"]
mod nsCheckboxRadioFrame_rs;
pub use nsCheckboxRadioFrame_rs::*;
#[path = "layout/forms/nsTextControlFrame.rs"]
mod nsTextControlFrame_rs;
pub use nsTextControlFrame_rs::*;
#[path = "layout/forms/nsComboboxControlFrame.rs"]
mod nsComboboxControlFrame_rs;
pub use nsComboboxControlFrame_rs::*;
#[path = "layout/forms/nsListControlFrame.rs"]
mod nsListControlFrame_rs;
pub use nsListControlFrame_rs::*;
#[path = "layout/generic/nsImageFrame.rs"]
mod nsImageFrame_rs;
pub use nsImageFrame_rs::*;
#[path = "layout/generic/nsVideoFrame.rs"]
mod nsVideoFrame_rs;
pub use nsVideoFrame_rs::*;
#[path = "layout/generic/nsSubDocumentFrame.rs"]
mod nsSubDocumentFrame_rs;
pub use nsSubDocumentFrame_rs::*;
#[path = "layout/painting/nsDisplayList.rs"]
mod nsDisplayList_rs;
pub use nsDisplayList_rs::*;
#[path = "layout/generic/WBRFrame.rs"]
mod WBRFrame_rs;
pub use WBRFrame_rs::*;
#[path = "layout/generic/BRFrame.rs"]
mod BRFrame_rs;
pub use BRFrame_rs::*;
#[path = "layout/generic/AbsoluteContainingBlock.rs"]
mod AbsoluteContainingBlock_rs;
pub use AbsoluteContainingBlock_rs::*;
#[path = "layout/generic/ViewportFrame.rs"]
mod ViewportFrame_rs;
pub use ViewportFrame_rs::*;
#[path = "layout/generic/nsCanvasFrame.rs"]
mod nsCanvasFrame_rs;
pub use nsCanvasFrame_rs::*;
#[path = "mfbt/Utf16.rs"]
mod Utf16_rs;
pub use Utf16_rs::*;
#[path = "intl/components/src/UnicodeProperties.rs"]
mod UnicodeProperties_rs;
pub use UnicodeProperties_rs::*;
#[path = "intl/unicharutil/util/nsUnicharUtils.rs"]
mod nsUnicharUtils_rs;
pub use nsUnicharUtils_rs::*;
#[path = "intl/lwbrk/LineBreaker.rs"]
mod LineBreaker_rs;
pub use LineBreaker_rs::*;
#[path = "intl/lwbrk/WordBreaker.rs"]
mod WordBreaker_rs;
pub use WordBreaker_rs::*;
#[path = "dom/base/nsLineBreaker.rs"]
mod nsLineBreaker_rs;
pub use nsLineBreaker_rs::*;
#[path = "gfx/thebes/gfxSkipChars.rs"]
mod gfxSkipChars_rs;
pub use gfxSkipChars_rs::*;
#[path = "layout/generic/nsTextFrameUtils.rs"]
mod nsTextFrameUtils_rs;
pub use nsTextFrameUtils_rs::*;
#[path = "gfx/thebes/gfxFont.rs"]
mod gfxFont_rs;
pub use gfxFont_rs::*;
#[path = "gfx/thebes/gfxTextRun.rs"]
mod gfxTextRun_rs;
pub use gfxTextRun_rs::*;
#[path = "gfx/src/nsFontMetrics.rs"]
mod nsFontMetrics_rs;
pub use nsFontMetrics_rs::*;
#[path = "gfx/src/nsFontCache.rs"]
mod nsFontCache_rs;
pub use nsFontCache_rs::*;
#[path = "dom/base/CharacterDataBuffer.rs"]
mod CharacterDataBuffer_rs;
pub use CharacterDataBuffer_rs::*;
#[path = "layout/generic/nsTextFrame.rs"]
mod nsTextFrame_rs;
pub use nsTextFrame_rs::*;
#[path = "layout/generic/nsTextRunTransformations.rs"]
mod nsTextRunTransformations_rs;
pub use nsTextRunTransformations_rs::*;
#[path = "layout/style/CounterStyleManager.rs"]
mod CounterStyleManager_rs;
pub use CounterStyleManager_rs::*;
#[path = "layout/base/nsCounterManager.rs"]
mod nsCounterManager_rs;
pub use nsCounterManager_rs::*;
#[path = "layout/svg/SVGOuterSVGFrame.rs"]
mod SVGOuterSVGFrame_rs;
pub use SVGOuterSVGFrame_rs::*;
#[cfg(test)]
#[path = "layout/forms/FormControlTests.rs"]
mod FormControlTests_rs;
#[path = "layout/base/DetailsSvgTests.rs"]
mod DetailsSvgTests_rs;
#[path = "layout/base/GeneratedContentTests.rs"]
mod GeneratedContentTests_rs;
#[path = "layout/base/nsCSSFrameConstructor.rs"]
mod nsCSSFrameConstructor_rs;
pub use nsCSSFrameConstructor_rs::*;
#[path = "layout/base/nsFrameManager.rs"]
mod nsFrameManager_rs;
pub use nsFrameManager_rs::*;
#[path = "layout/base/PresShell.rs"]
mod PresShell_rs;
pub use PresShell_rs::*;
// Tables, part A (agent A) and part B (agent B).
#[path = "layout/tables/TableArea.rs"]
mod TableArea_rs;
pub use TableArea_rs::*;
#[path = "layout/tables/celldata.rs"]
mod celldata_rs;
pub use celldata_rs::*;
#[path = "layout/tables/nsCellMap.rs"]
mod nsCellMap_rs;
pub use nsCellMap_rs::*;
#[path = "layout/tables/nsITableLayoutStrategy.rs"]
mod nsITableLayoutStrategy_rs;
pub use nsITableLayoutStrategy_rs::*;
#[path = "layout/tables/SpanningCellSorter.rs"]
mod SpanningCellSorter_rs;
pub use SpanningCellSorter_rs::*;
#[path = "layout/tables/nsTableColFrame.rs"]
mod nsTableColFrame_rs;
pub use nsTableColFrame_rs::*;
#[path = "layout/tables/nsTableColGroupFrame.rs"]
mod nsTableColGroupFrame_rs;
pub use nsTableColGroupFrame_rs::*;
#[path = "layout/tables/BasicTableLayoutStrategy.rs"]
mod BasicTableLayoutStrategy_rs;
pub use BasicTableLayoutStrategy_rs::*;
#[path = "layout/tables/FixedTableLayoutStrategy.rs"]
mod FixedTableLayoutStrategy_rs;
pub use FixedTableLayoutStrategy_rs::*;
#[path = "layout/tables/nsTableFrame.rs"]
mod nsTableFrame_rs;
pub use nsTableFrame_rs::*;
#[path = "layout/tables/nsTableCellFrame.rs"]
mod nsTableCellFrame_rs;
pub use nsTableCellFrame_rs::*;
#[path = "layout/tables/nsTableRowFrame.rs"]
mod nsTableRowFrame_rs;
pub use nsTableRowFrame_rs::*;
#[path = "layout/tables/nsTableRowGroupFrame.rs"]
mod nsTableRowGroupFrame_rs;
pub use nsTableRowGroupFrame_rs::*;
#[path = "layout/tables/nsTableWrapperFrame.rs"]
mod nsTableWrapperFrame_rs;
pub use nsTableWrapperFrame_rs::*;
#[path = "layout/generic/CSSOrderAwareFrameIterator.rs"]
mod CSSOrderAwareFrameIterator_rs;
pub use CSSOrderAwareFrameIterator_rs::*;
#[path = "layout/generic/nsFlexContainerFrame.rs"]
mod nsFlexContainerFrame_rs;
pub use nsFlexContainerFrame_rs::*;
#[path = "layout/generic/nsGridContainerFrame.rs"]
mod nsGridContainerFrame_rs;
pub use nsGridContainerFrame_rs::*;
#[path = "layout/generic/CSSAlignUtils.rs"]
mod CSSAlignUtils_rs;
pub use CSSAlignUtils_rs::*;
#[path = "layout/style/nsComputedDOMStyle.rs"]
mod nsComputedDOMStyle_rs;
pub use nsComputedDOMStyle_rs::*;
// ADAPTER (not a Firefox file): lays out a styled document, see driver.rs.
pub mod driver;
