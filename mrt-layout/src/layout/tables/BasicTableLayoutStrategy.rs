// Translated from layout/tables/BasicTableLayoutStrategy.h and BasicTableLayoutStrategy.cpp (GECKO_REV).
/*
 * Web-compatible algorithms that determine column and table widths,
 * used for CSS2's 'table-layout: auto'.
 */
//
// Rust shape: the class derives from the abstract nsITableLayoutStrategy (a trait in
// nsITableLayoutStrategy.rs). The methods are written once as inherent methods under their own
// names, in the order of the C++ file; the `impl nsITableLayoutStrategy` block at the end of the file
// is the virtual interface and only forwards to them. The class is held by the table frame as
// `Box<dyn nsITableLayoutStrategy>`. `nsTableFrame*` is the handle `nsTableFrame`, `nsTableCellMap*`
// is whatever `nsTableFrame::GetCellMap()` returns (used as a reference).
//
// NOT PORTED: the debug prints (DEBUG_TABLE_STRATEGY, DEBUG_dbaron_off), AutoMaybeDisableFontInflation
// (font size inflation is off), the `nowrap` attribute quirk (nsIContent keeps no attributes; marked
// where it occurs), the AutoStackArena (the SpanningCellSorter owns its items).
use crate::*;
use std::ops::{Deref, DerefMut};

// Gecko: layout/tables/BasicTableLayoutStrategy.h BasicTableLayoutStrategy
pub struct BasicTableLayoutStrategy {
    mBase: nsITableLayoutStrategy_Base,
    mTableFrame: nsTableFrame,
    mMinISize: nscoord,
    mPrefISize: nscoord,
    mPrefISizePctExpand: nscoord,
    mLastCalcISize: nscoord,
}

impl Deref for BasicTableLayoutStrategy {
    type Target = nsITableLayoutStrategy_Base;
    fn deref(&self) -> &nsITableLayoutStrategy_Base {
        &self.mBase
    }
}

impl DerefMut for BasicTableLayoutStrategy {
    fn deref_mut(&mut self) -> &mut nsITableLayoutStrategy_Base {
        &mut self.mBase
    }
}

// Gecko: layout/tables/BasicTableLayoutStrategy.h BasicTableLayoutStrategy::BtlsISizeType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BasicTableLayoutStrategy_BtlsISizeType {
    MinISize,
    PrefISize,
    FinalISize,
}

impl BasicTableLayoutStrategy {
    // Gecko: layout/tables/BasicTableLayoutStrategy.cpp BasicTableLayoutStrategy::BasicTableLayoutStrategy
    pub fn new(aTableFrame: nsTableFrame) -> BasicTableLayoutStrategy {
        let mut result = BasicTableLayoutStrategy {
            mBase: nsITableLayoutStrategy_Base::new(nsITableLayoutStrategy_Type::Auto),
            mTableFrame: aTableFrame,
            mMinISize: 0,
            mPrefISize: 0,
            mPrefISizePctExpand: 0,
            mLastCalcISize: 0,
        };
        result.MarkIntrinsicISizesDirty();
        result
    }

    // BasicTableLayoutStrategy::~BasicTableLayoutStrategy (header only)
    // (= default)

    /* virtual */
    // Gecko: layout/tables/BasicTableLayoutStrategy.cpp BasicTableLayoutStrategy::GetMinISize
    pub fn GetMinISize(&mut self, aRenderingContext: Option<gfxContext>) -> nscoord {
        if self.mMinISize == NS_INTRINSIC_ISIZE_UNKNOWN {
            self.ComputeIntrinsicISizes(aRenderingContext);
        }
        self.mMinISize
    }

    /* virtual */
    // Gecko: layout/tables/BasicTableLayoutStrategy.cpp BasicTableLayoutStrategy::GetPrefISize
    pub fn GetPrefISize(&mut self, aRenderingContext: Option<gfxContext>, aComputingSize: bool) -> nscoord {
        NS_ASSERTION!((self.mPrefISize == NS_INTRINSIC_ISIZE_UNKNOWN) == (self.mPrefISizePctExpand == NS_INTRINSIC_ISIZE_UNKNOWN), "dirtyness out of sync");
        if self.mPrefISize == NS_INTRINSIC_ISIZE_UNKNOWN {
            self.ComputeIntrinsicISizes(aRenderingContext);
        }
        if aComputingSize { self.mPrefISizePctExpand } else { self.mPrefISize }
    }
}

// Gecko: layout/tables/BasicTableLayoutStrategy.cpp CellISizeInfo
struct CellISizeInfo {
    hasSpecifiedISize: bool,
    minCoord: nscoord,
    prefCoord: nscoord,
    prefPercent: f32,
}

impl CellISizeInfo {
    // Gecko: layout/tables/BasicTableLayoutStrategy.cpp CellISizeInfo::CellISizeInfo
    fn new(aMinCoord: nscoord, aPrefCoord: nscoord, aPrefPercent: f32, aHasSpecifiedISize: bool) -> CellISizeInfo {
        CellISizeInfo { hasSpecifiedISize: aHasSpecifiedISize, minCoord: aMinCoord, prefCoord: aPrefCoord, prefPercent: aPrefPercent }
    }
}

// A helper for ComputeColumnIntrinsicISizes(), used for both column and cell
// intrinsic inline size calculations. The parts needed only for cells are
// skipped when aIsCell is false.
// Gecko: layout/tables/BasicTableLayoutStrategy.cpp GetISizeInfo
fn GetISizeInfo(aRenderingContext: Option<gfxContext>, aFrame: nsIFrame, aWM: WritingMode, aIsCell: bool) -> CellISizeInfo {
    MOZ_ASSERT!(aFrame.GetWritingMode() == aWM, "The caller is expected to pass aFrame's writing mode!");
    let mut minCoord: nscoord;
    let mut prefCoord: nscoord;
    let stylePos = aFrame.StylePosition();
    let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aFrame, std::ptr::null_mut());
    let isQuirks = aFrame.PresContext().CompatibilityMode() == eCompatibility_NavQuirks;
    let mut boxSizingToBorderEdge: nscoord = 0;
    if aIsCell {
        // If aFrame is a container for font size inflation, then shrink
        // wrapping inside of it should not apply font size inflation.
        // AutoMaybeDisableFontInflation an(aFrame);
        // NOT PORTED: font size inflation is off.

        // Resolve the cell's block size 'cellBSize' as a percentage basis, in case
        // it impacts its children's inline-size contributions (e.g. via percentage
        // block size + aspect-ratio). However, this behavior might not be
        // web-compatible (Bug 1461852).
        //
        // Note that if the cell *itself* has a percentage-based block size, we
        // treat it as unresolvable here by using an unconstrained cbBSize. It will
        // be resolved during the "special bsize reflow" pass if the table has a
        // specified block size. See nsTableFrame::Reflow() and
        // ReflowInput::Flags::mSpecialBSizeReflow.
        let cbBSize: nscoord = NS_UNCONSTRAINEDSIZE;
        let contentEdgeToBoxSizingBSize: nscoord = if stylePos.mBoxSizing() == StyleBoxSizing::BorderBox { aFrame.IntrinsicBSizeOffsets(NS_UNCONSTRAINEDSIZE).BorderPadding() } else { 0 };
        let cellBSize = nsIFrame::ComputeBSizeValueAsPercentageBasis(
            &stylePos.BSize(aWM, &anchorResolutionParams),
            &stylePos.MinBSize(aWM, &anchorResolutionParams),
            &stylePos.MaxBSize(aWM, &anchorResolutionParams),
            cbBSize,
            contentEdgeToBoxSizingBSize,
        );

        let input = IntrinsicSizeInput::new(aRenderingContext, None, Some(LogicalSize::new(aWM, NS_UNCONSTRAINEDSIZE, cellBSize)));
        minCoord = aFrame.GetMinISize(&input);
        prefCoord = aFrame.GetPrefISize(&input);
        // Until almost the end of this function, minCoord and prefCoord
        // represent the box-sizing based isize values (which mean they
        // should include inline padding and border width when
        // box-sizing is set to border-box).
        // Note that this function returns border-box isize, we add the
        // outer edges near the end of this function.

        // XXX Should we ignore percentage padding?
        let offsets = aFrame.IntrinsicISizeOffsets(NS_UNCONSTRAINEDSIZE);
        if stylePos.mBoxSizing() == StyleBoxSizing::ContentBox {
            boxSizingToBorderEdge = offsets.padding + offsets.border;
        } else {
            // StyleBoxSizing::BorderBox
            minCoord += offsets.padding + offsets.border;
            prefCoord += offsets.padding + offsets.border;
        }
    } else {
        minCoord = 0;
        prefCoord = 0;
    }
    let mut prefPercent: f32 = 0.0;
    let mut hasSpecifiedISize = false;

    let iSize = stylePos.ISize(aWM, &anchorResolutionParams);
    // NOTE: We're ignoring calc() units with both lengths and percentages here,
    // for lack of a sensible idea for what to do with them.  This means calc()
    // with percentages is basically handled like 'auto' for table cells and
    // columns.
    if iSize.ConvertsToLength() {
        hasSpecifiedISize = true;
        let c: nscoord = iSize.ToLength();
        // Quirk: A cell with "nowrap" set and a coord value for the
        // isize which is bigger than the intrinsic minimum isize uses
        // that coord value as the minimum isize.
        // This is kept up-to-date with dynamic changes to nowrap by code in
        // nsTableCellFrame::AttributeChanged
        // NOT PORTED: `aFrame->GetContent()->AsElement()->HasAttr(nsGkAtoms::nowrap)`; nsIContent
        // keeps no attributes, so the quirk never applies (the condition below is `false`).
        let hasNowrapAttr = false;
        if aIsCell && c > minCoord && isQuirks && hasNowrapAttr {
            minCoord = c;
        }
        prefCoord = std::cmp::max(c, minCoord);
    } else if iSize.ConvertsToPercentage() {
        prefPercent = iSize.ToPercentage();
    } else if aIsCell {
        // switch (iSize->tag)
        if iSize.IsMaxContent() {
            // 'inline-size' only affects pref isize, not min
            // isize, so don't change anything
        } else if iSize.IsMinContent() {
            prefCoord = minCoord;
        }
        // case MozAvailable, WebkitFillAvailable, Stretch, FitContent, FitContentFunction
        // (TODO: Bug 1708310: Make sure fit-content() work properly in table.), Auto,
        // LengthPercentage, AnchorSizeFunction, AnchorContainingCalcFunction: break;
    }

    let mut maxISize = stylePos.MaxISize(aWM, &anchorResolutionParams);
    if nsIFrame::ToExtremumLength(&maxISize).is_some() {
        if !aIsCell || maxISize.BehavesLikeStretchOnInlineAxis() {
            maxISize = AnchorResolvedMaxSizeHelper::None();
        } else if maxISize.IsFitContent() || maxISize.IsFitContentFunction() {
            // TODO: Bug 1708310: Make sure fit-content() work properly in table.
            // for 'max-inline-size', '-moz-fit-content' is like 'max-content'
            // (AnchorResolvedMaxSizeHelper::MaxContent())
            maxISize = StyleMaxSize::MaxContent;
        }
    }
    // XXX To really implement 'max-inline-size' well, we'd need to store
    // it separately on the columns.
    let zeroSize = LogicalSize::new__WritingMode(aWM);
    if maxISize.ConvertsToLength() || nsIFrame::ToExtremumLength(&maxISize).is_some() {
        let c: nscoord = aFrame
            .ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_SizeOrMaxSize_StyleSize_AspectRatio_ComputeSizeFlags(
                aRenderingContext,
                aWM,
                zeroSize,
                zeroSize,
                0,
                &maxISize,
                &stylePos.BSize(aWM, &anchorResolutionParams),
                aFrame.GetAspectRatio(),
                ComputeSizeFlags::default(),
            )
            .mISize;
        minCoord = std::cmp::min(c, minCoord);
        prefCoord = std::cmp::min(c, prefCoord);
    } else if maxISize.ConvertsToPercentage() {
        let p: f32 = maxISize.ToPercentage();
        if p < prefPercent {
            prefPercent = p;
        }
    }

    let mut minISize = stylePos.MinISize(aWM, &anchorResolutionParams);
    // (sic: Gecko tests maxISize here, not minISize)
    if nsIFrame::ToExtremumLength(&maxISize).is_some() {
        if !aIsCell || minISize.BehavesLikeStretchOnInlineAxis() {
            // AnchorResolvedSizeHelper::Zero()
            minISize = StyleSize::FromAppUnits(0);
        } else if minISize.IsFitContent() || minISize.IsFitContentFunction() {
            // TODO: Bug 1708310: Make sure fit-content() work properly in table.
            // for 'min-inline-size', '-moz-fit-content' is like 'min-content'
            // (AnchorResolvedSizeHelper::MinContent())
            minISize = StyleSize::MinContent;
        }
    }

    if minISize.ConvertsToLength() || nsIFrame::ToExtremumLength(&minISize).is_some() {
        let c: nscoord = aFrame
            .ComputeISizeValue__gfxContext_WritingMode_LogicalSize_LogicalSize_nscoord_SizeOrMaxSize_StyleSize_AspectRatio_ComputeSizeFlags(
                aRenderingContext,
                aWM,
                zeroSize,
                zeroSize,
                0,
                &minISize,
                &stylePos.BSize(aWM, &anchorResolutionParams),
                aFrame.GetAspectRatio(),
                ComputeSizeFlags::default(),
            )
            .mISize;
        minCoord = std::cmp::max(c, minCoord);
        prefCoord = std::cmp::max(c, prefCoord);
    } else if minISize.ConvertsToPercentage() {
        let p: f32 = minISize.ToPercentage();
        if p > prefPercent {
            prefPercent = p;
        }
    }

    // XXX Should col frame have border/padding considered?
    if aIsCell {
        minCoord += boxSizingToBorderEdge;
        prefCoord = NSCoordSaturatingAdd(prefCoord, boxSizingToBorderEdge);
    }

    CellISizeInfo::new(minCoord, prefCoord, prefPercent, hasSpecifiedISize)
}

// Gecko: layout/tables/BasicTableLayoutStrategy.cpp GetCellISizeInfo
#[inline]
fn GetCellISizeInfo(aRenderingContext: Option<gfxContext>, aCellFrame: nsTableCellFrame, aWM: WritingMode) -> CellISizeInfo {
    GetISizeInfo(aRenderingContext, aCellFrame.0, aWM, true)
}

// Gecko: layout/tables/BasicTableLayoutStrategy.cpp GetColISizeInfo
#[inline]
fn GetColISizeInfo(aRenderingContext: Option<gfxContext>, aFrame: nsIFrame, aWM: WritingMode) -> CellISizeInfo {
    GetISizeInfo(aRenderingContext, aFrame, aWM, false)
}

impl BasicTableLayoutStrategy {
    /**
     * The algorithm in this function, in addition to meeting the
     * requirements of Web-compatibility, is also invariant under reordering
     * of the rows within a table (something that most, but not all, other
     * browsers are).
     */
    // Gecko: layout/tables/BasicTableLayoutStrategy.cpp BasicTableLayoutStrategy::ComputeColumnIntrinsicISizes
    fn ComputeColumnIntrinsicISizes(&mut self, aRenderingContext: Option<gfxContext>) {
        let tableFrame = self.mTableFrame;
        let cellMap = tableFrame.GetCellMap().unwrap();
        let wm = tableFrame.GetWritingMode();

        // mozilla::AutoStackArena arena;
        // NOT PORTED: the sorter owns its items.
        let mut spanningCells = SpanningCellSorter::new();

        // Loop over the columns to consider the columns and cells *without*
        // a colspan.
        let mut col: i32;
        let mut col_end: i32;
        col = 0;
        col_end = cellMap.GetColCount();
        while col < col_end {
            let Some(colFrame) = tableFrame.GetColFrame(col) else {
                NS_ERROR!("column frames out of sync with cell map");
                col += 1;
                continue;
            };
            colFrame.ResetIntrinsics();
            colFrame.ResetSpanIntrinsics();

            // Consider the isizes on the column.
            let mut colInfo = GetColISizeInfo(aRenderingContext, colFrame.0, wm);
            colFrame.AddCoords(colInfo.minCoord, colInfo.prefCoord, colInfo.hasSpecifiedISize);
            colFrame.AddPrefPercent(colInfo.prefPercent);

            // Consider the isizes on the column-group.  Note that we follow
            // what the HTML spec says here, and make the isize apply to
            // each column in the group, not the group as a whole.

            // If column has isize, column-group doesn't override isize.
            if colInfo.minCoord == 0 && colInfo.prefCoord == 0 && colInfo.prefPercent == 0.0 {
                NS_ASSERTION!(colFrame.GetParent().unwrap().IsTableColGroupFrame(), "expected a column-group");
                colInfo = GetColISizeInfo(aRenderingContext, colFrame.GetParent().unwrap(), wm);
                colFrame.AddCoords(colInfo.minCoord, colInfo.prefCoord, colInfo.hasSpecifiedISize);
                colFrame.AddPrefPercent(colInfo.prefPercent);
            }

            // Consider the contents of and the isizes on the cells without
            // colspans.
            let mut columnIter = nsCellMapColumnIterator::new(cellMap, col);
            let mut row: i32 = 0;
            let mut colSpan: i32 = 0;
            while let Some(cellFrame) = columnIter.GetNextFrame(&mut row, &mut colSpan) {
                if colSpan > 1 {
                    spanningCells.AddCell(colSpan, row, col);
                    continue;
                }

                let info = GetCellISizeInfo(aRenderingContext, cellFrame, wm);

                colFrame.AddCoords(info.minCoord, info.prefCoord, info.hasSpecifiedISize);
                colFrame.AddPrefPercent(info.prefPercent);
            }
            // #ifdef DEBUG_dbaron_off ... #endif
            // NOT PORTED: debug print.
            col += 1;
        }
        // #ifdef DEBUG_TABLE_STRATEGY ... #endif
        // NOT PORTED: debug print.

        // Consider the cells with a colspan that we saved in the loop above
        // into the spanning cell sorter.  We consider these cells by seeing
        // if they require adding to the isizes resulting only from cells
        // with a smaller colspan, and therefore we must process them sorted
        // in increasing order by colspan.  For each colspan group, we
        // accumulate new values to accumulate in the column frame's Span*
        // members.
        //
        // Considering things only relative to the isizes resulting from
        // cells with smaller colspans (rather than incrementally including
        // the results from spanning cells, or doing spanning and
        // non-spanning cells in a single pass) means that layout remains
        // row-order-invariant and (except for percentage isizes that add to
        // more than 100%) column-order invariant.
        //
        // Starting with smaller colspans makes it more likely that we
        // satisfy all the constraints given and don't distribute space to
        // columns where we don't need it.
        let mut item: *mut SpanningCellSorter_Item;
        let mut colSpan: i32 = 0;
        loop {
            item = spanningCells.GetNext(&mut colSpan);
            if item.is_null() {
                break;
            }
            NS_ASSERTION!(colSpan > 1, "cell should not have been put in spanning cell sorter");
            loop {
                let row = unsafe { (*item).row };
                col = unsafe { (*item).col };
                // CellData* cellData = cellMap->GetDataAt(row, col); (a raw pointer, as in nsCellMap.rs)
                let cellData = cellMap.GetDataAt(row, col);
                NS_ASSERTION!(!cellData.is_null() && unsafe { &*cellData }.IsOrig(), "bogus result from spanning cell sorter");

                let cellFrame = unsafe { &*cellData }.GetCellFrame();
                NS_ASSERTION!(cellFrame.is_some(), "bogus result from spanning cell sorter");
                let cellFrame = cellFrame.unwrap();

                let info = GetCellISizeInfo(aRenderingContext, cellFrame, wm);

                if info.prefPercent > 0.0 {
                    self.DistributePctISizeToColumns(info.prefPercent, col, colSpan);
                }
                self.DistributeISizeToColumns(info.minCoord, col, colSpan, BasicTableLayoutStrategy_BtlsISizeType::MinISize, info.hasSpecifiedISize);
                self.DistributeISizeToColumns(info.prefCoord, col, colSpan, BasicTableLayoutStrategy_BtlsISizeType::PrefISize, info.hasSpecifiedISize);

                item = unsafe { (*item).next };
                if item.is_null() {
                    break;
                }
            }

            // Combine the results of the span analysis into the main results,
            // for each increment of colspan.

            col = 0;
            col_end = cellMap.GetColCount();
            while col < col_end {
                let Some(colFrame) = tableFrame.GetColFrame(col) else {
                    NS_ERROR!("column frames out of sync with cell map");
                    col += 1;
                    continue;
                };

                colFrame.AccumulateSpanIntrinsics();
                colFrame.ResetSpanIntrinsics();

                // #ifdef DEBUG_dbaron_off ... #endif
                // NOT PORTED: debug print.
                col += 1;
            }
        }

        // Prevent percentages from adding to more than 100% by (to be
        // compatible with other browsers) treating any percentages that would
        // increase the total percentage to more than 100% as the number that
        // would increase it to only 100% (which is 0% if we've already hit
        // 100%).  This means layout depends on the order of columns.
        let mut pct_used: f32 = 0.0;
        col = 0;
        col_end = cellMap.GetColCount();
        while col < col_end {
            let Some(colFrame) = tableFrame.GetColFrame(col) else {
                NS_ERROR!("column frames out of sync with cell map");
                col += 1;
                continue;
            };

            colFrame.AdjustPrefPercent(&mut pct_used);
            col += 1;
        }

        // #ifdef DEBUG_TABLE_STRATEGY ... #endif
        // NOT PORTED: debug print.
    }

    // Gecko: layout/tables/BasicTableLayoutStrategy.cpp BasicTableLayoutStrategy::ComputeIntrinsicISizes
    fn ComputeIntrinsicISizes(&mut self, aRenderingContext: Option<gfxContext>) {
        self.ComputeColumnIntrinsicISizes(aRenderingContext);

        let cellMap = self.mTableFrame.GetCellMap().unwrap();
        let mut min: nscoord = 0;
        let mut pref: nscoord = 0;
        let mut max_small_pct_pref: nscoord = 0;
        let mut nonpct_pref_total: nscoord = 0;
        let mut pct_total: f32 = 0.0; // always from 0.0f - 1.0f
        let colCount = cellMap.GetColCount();
        // add a total of (colcount + 1) lots of cellSpacingX for columns where a
        // cell originates
        let mut add: nscoord = self.mTableFrame.GetColSpacing__int32_t(colCount);

        for col in 0..colCount {
            let Some(colFrame) = self.mTableFrame.GetColFrame(col) else {
                NS_ERROR!("column frames out of sync with cell map");
                continue;
            };
            if self.mTableFrame.ColumnHasCellSpacingBefore(col) {
                add += self.mTableFrame.GetColSpacing__int32_t(col - 1);
            }
            min += colFrame.GetMinCoord();
            pref = NSCoordSaturatingAdd(pref, colFrame.GetPrefCoord());

            // Percentages are of the table, so we have to reverse them for
            // intrinsic isizes.
            let p: f32 = colFrame.GetPrefPercent();
            if p > 0.0 {
                let colPref: nscoord = colFrame.GetPrefCoord();
                let new_small_pct_expand: nscoord = if colPref == nscoord_MAX { nscoord_MAX } else { (colPref as f32 / p) as nscoord };
                if new_small_pct_expand > max_small_pct_pref {
                    max_small_pct_pref = new_small_pct_expand;
                }
                pct_total += p;
            } else {
                nonpct_pref_total = NSCoordSaturatingAdd(nonpct_pref_total, colFrame.GetPrefCoord());
            }
        }

        let mut pref_pct_expand: nscoord = pref;

        // Account for small percentages expanding the preferred isize of
        // *other* columns.
        if max_small_pct_pref > pref_pct_expand {
            pref_pct_expand = max_small_pct_pref;
        }

        // Account for large percentages expanding the preferred isize of
        // themselves.  There's no need to iterate over the columns multiple
        // times, since when there is such a need, the small percentage
        // effect is bigger anyway.  (I think!)
        NS_ASSERTION!(0.0 <= pct_total && pct_total <= 1.0, "column percentage inline-sizes not adjusted down to 100%");
        if pct_total == 1.0 {
            if nonpct_pref_total > 0 {
                pref_pct_expand = nscoord_MAX;
                // XXX Or should I use some smaller value?  (Test this using
                // nested tables!)
            }
        } else {
            let large_pct_pref: nscoord = if nonpct_pref_total == nscoord_MAX { nscoord_MAX } else { (nonpct_pref_total as f32 / (1.0 - pct_total)) as nscoord };
            if large_pct_pref > pref_pct_expand {
                pref_pct_expand = large_pct_pref;
            }
        }

        // border-spacing isn't part of the basis for percentages
        if colCount > 0 {
            min += add;
            pref = NSCoordSaturatingAdd(pref, add);
            pref_pct_expand = NSCoordSaturatingAdd(pref_pct_expand, add);
        }

        self.mMinISize = min;
        self.mPrefISize = pref;
        self.mPrefISizePctExpand = pref_pct_expand;
    }

    /* virtual */
    // Gecko: layout/tables/BasicTableLayoutStrategy.cpp BasicTableLayoutStrategy::MarkIntrinsicISizesDirty
    pub fn MarkIntrinsicISizesDirty(&mut self) {
        self.mMinISize = NS_INTRINSIC_ISIZE_UNKNOWN;
        self.mPrefISize = NS_INTRINSIC_ISIZE_UNKNOWN;
        self.mPrefISizePctExpand = NS_INTRINSIC_ISIZE_UNKNOWN;
        self.mLastCalcISize = nscoord_MIN;
    }

    /* virtual */
    // Gecko: layout/tables/BasicTableLayoutStrategy.cpp BasicTableLayoutStrategy::ComputeColumnISizes
    pub fn ComputeColumnISizes(&mut self, aReflowInput: &ReflowInput) {
        let iSize: nscoord = aReflowInput.ComputedISize();

        if self.mLastCalcISize == iSize {
            return;
        }
        self.mLastCalcISize = iSize;

        NS_ASSERTION!((self.mMinISize == NS_INTRINSIC_ISIZE_UNKNOWN) == (self.mPrefISize == NS_INTRINSIC_ISIZE_UNKNOWN), "dirtyness out of sync");
        NS_ASSERTION!((self.mMinISize == NS_INTRINSIC_ISIZE_UNKNOWN) == (self.mPrefISizePctExpand == NS_INTRINSIC_ISIZE_UNKNOWN), "dirtyness out of sync");
        // XXX Is this needed?
        if self.mMinISize == NS_INTRINSIC_ISIZE_UNKNOWN {
            self.ComputeIntrinsicISizes(aReflowInput.mRenderingContext);
        }

        let cellMap = self.mTableFrame.GetCellMap().unwrap();
        let colCount = cellMap.GetColCount();
        if colCount <= 0 {
            return; // nothing to do
        }

        self.DistributeISizeToColumns(iSize, 0, colCount, BasicTableLayoutStrategy_BtlsISizeType::FinalISize, false);

        // #ifdef DEBUG_TABLE_STRATEGY ... #endif
        // NOT PORTED: debug print.
    }

    // Gecko: layout/tables/BasicTableLayoutStrategy.cpp BasicTableLayoutStrategy::DistributePctISizeToColumns
    fn DistributePctISizeToColumns(&mut self, mut aSpanPrefPct: f32, aFirstCol: i32, aColCount: i32) {
        // First loop to determine:
        let mut nonPctColCount: i32 = 0; // number of spanned columns without % isize
        let mut nonPctTotalPrefISize: nscoord = 0; // total pref isize of those columns
        // and to reduce aSpanPrefPct by columns that already have % isize

        let mut scol: i32;
        let mut scol_end: i32;
        let cellMap = self.mTableFrame.GetCellMap().unwrap();
        scol = aFirstCol;
        scol_end = aFirstCol + aColCount;
        while scol < scol_end {
            let Some(scolFrame) = self.mTableFrame.GetColFrame(scol) else {
                NS_ERROR!("column frames out of sync with cell map");
                scol += 1;
                continue;
            };
            let scolPct: f32 = scolFrame.GetPrefPercent();
            if scolPct == 0.0 {
                nonPctTotalPrefISize += scolFrame.GetPrefCoord();
                if cellMap.GetNumCellsOriginatingInCol(scol) > 0 {
                    nonPctColCount += 1;
                }
            } else {
                aSpanPrefPct -= scolPct;
            }
            scol += 1;
        }

        if aSpanPrefPct <= 0.0 || nonPctColCount == 0 {
            // There's no %-isize on the colspan left over to distribute,
            // or there are no columns to which we could distribute %-isize
            return;
        }

        // Second loop, to distribute what remains of aSpanPrefPct
        // between the non-percent-isize spanned columns
        let spanHasNonPctPref = nonPctTotalPrefISize > 0; // Loop invariant
        scol = aFirstCol;
        scol_end = aFirstCol + aColCount;
        while scol < scol_end {
            let Some(scolFrame) = self.mTableFrame.GetColFrame(scol) else {
                NS_ERROR!("column frames out of sync with cell map");
                scol += 1;
                continue;
            };

            if scolFrame.GetPrefPercent() == 0.0 {
                NS_ASSERTION!((!spanHasNonPctPref || nonPctTotalPrefISize != 0) && nonPctColCount != 0, "should not be zero if we haven't allocated all pref percent");

                let allocatedPct: f32; // % isize to be given to this column
                if spanHasNonPctPref {
                    // Group so we're multiplying by 1.0f when we need
                    // to use up aSpanPrefPct.
                    allocatedPct = aSpanPrefPct * (scolFrame.GetPrefCoord() as f32 / nonPctTotalPrefISize as f32);
                } else if cellMap.GetNumCellsOriginatingInCol(scol) > 0 {
                    // distribute equally when all pref isizes are 0
                    allocatedPct = aSpanPrefPct / nonPctColCount as f32;
                } else {
                    allocatedPct = 0.0;
                }
                // Allocate the percent
                scolFrame.AddSpanPrefPercent(allocatedPct);

                // To avoid accumulating rounding error from division,
                // subtract this column's values from the totals.
                aSpanPrefPct -= allocatedPct;
                nonPctTotalPrefISize -= scolFrame.GetPrefCoord();
                if cellMap.GetNumCellsOriginatingInCol(scol) > 0 {
                    nonPctColCount -= 1;
                }

                if aSpanPrefPct == 0.0 {
                    // No more span-percent-isize to distribute --> we're done.
                    NS_ASSERTION!(
                        if spanHasNonPctPref { nonPctTotalPrefISize == 0 } else { nonPctColCount == 0 },
                        "No more pct inline-size to distribute, but there are still cols that need some."
                    );
                    return;
                }
            }
            scol += 1;
        }
    }

    // Gecko: layout/tables/BasicTableLayoutStrategy.cpp BasicTableLayoutStrategy::DistributeISizeToColumns
    fn DistributeISizeToColumns(&mut self, mut aISize: nscoord, aFirstCol: i32, aColCount: i32, aISizeType: BasicTableLayoutStrategy_BtlsISizeType, aSpanHasSpecifiedISize: bool) {
        use BasicTableLayoutStrategy_BtlsISizeType as BtlsISizeType;
        NS_ASSERTION!(
            aISizeType != BtlsISizeType::FinalISize || (aFirstCol == 0 && aColCount == self.mTableFrame.GetCellMap().unwrap().GetColCount()),
            "Computing final column isizes, but didn't get full column range"
        );

        let mut subtract: nscoord = 0;
        // aISize initially includes border-spacing for the boundaries in between
        // each of the columns. We start at aFirstCol + 1 because the first
        // in-between boundary would be at the left edge of column aFirstCol + 1
        let mut col = aFirstCol + 1;
        while col < aFirstCol + aColCount {
            if self.mTableFrame.ColumnHasCellSpacingBefore(col) {
                // border-spacing isn't part of the basis for percentages.
                subtract += self.mTableFrame.GetColSpacing__int32_t(col - 1);
            }
            col += 1;
        }
        if aISizeType == BtlsISizeType::FinalISize {
            // If we're computing final col-isize, then aISize initially includes
            // border spacing on the table's far istart + far iend edge, too.  Need
            // to subtract those out, too.
            subtract += self.mTableFrame.GetColSpacing__int32_t(-1) + self.mTableFrame.GetColSpacing__int32_t(aColCount);
        }
        aISize = NSCoordSaturatingSubtract(aISize, subtract, nscoord_MAX);

        /*
         * The goal of this function is to distribute |aISize| between the
         * columns by making an appropriate AddSpanCoords or SetFinalISize
         * call for each column.  (We call AddSpanCoords if we're
         * distributing a column-spanning cell's minimum or preferred isize
         * to its spanned columns.  We call SetFinalISize if we're
         * distributing a table's final isize to its columns.)
         *
         * The idea is to either assign one of the following sets of isizes
         * or a weighted average of two adjacent sets of isizes.  It is not
         * possible to assign values smaller than the smallest set of
         * isizes.  However, see below for handling the case of assigning
         * values larger than the largest set of isizes.  From smallest to
         * largest, these are:
         *
         * 1. [guess_min] Assign all columns their min isize.
         *
         * 2. [guess_min_pct] Assign all columns with percentage isizes
         * their percentage isize, and all other columns their min isize.
         *
         * 3. [guess_min_spec] Assign all columns with percentage isizes
         * their percentage isize, all columns with specified coordinate
         * isizes their pref isize (since it doesn't matter whether it's the
         * largest contributor to the pref isize that was the specified
         * contributor), and all other columns their min isize.
         *
         * 4. [guess_pref] Assign all columns with percentage isizes their
         * specified isize, and all other columns their pref isize.
         *
         * If |aISize| is *larger* than what we would assign in (4), then we
         * expand the columns:
         *
         *   a. if any columns without a specified coordinate isize or
         *   percent isize have nonzero pref isize, in proportion to pref
         *   isize [total_flex_pref]
         *
         *   b. otherwise, if any columns without a specified coordinate
         *   isize or percent isize, but with cells originating in them,
         *   have zero pref isize, equally between these
         *   [numNonSpecZeroISizeCols]
         *
         *   c. otherwise, if any columns without percent isize have nonzero
         *   pref isize, in proportion to pref isize [total_fixed_pref]
         *
         *   d. otherwise, if any columns have nonzero percentage isizes, in
         *   proportion to the percentage isizes [total_pct]
         *
         *   e. otherwise, equally.
         */

        // Loop #1 over the columns, to figure out the four values above so
        // we know which case we're dealing with.

        let mut guess_min: nscoord = 0;
        let mut guess_min_pct: nscoord = 0;
        let mut guess_min_spec: nscoord = 0;
        let mut guess_pref: nscoord = 0;
        let mut total_flex_pref: nscoord = 0;
        let mut total_fixed_pref: nscoord = 0;
        let mut total_pct: f32 = 0.0; // 0.0f to 1.0f
        let mut numInfiniteISizeCols: i32 = 0;
        let mut numNonSpecZeroISizeCols: i32 = 0;

        let cellMap = self.mTableFrame.GetCellMap().unwrap();
        col = aFirstCol;
        while col < aFirstCol + aColCount {
            let Some(colFrame) = self.mTableFrame.GetColFrame(col) else {
                NS_ERROR!("column frames out of sync with cell map");
                col += 1;
                continue;
            };
            let min_iSize: nscoord = colFrame.GetMinCoord();
            guess_min += min_iSize;
            if colFrame.GetPrefPercent() != 0.0 {
                let pct: f32 = colFrame.GetPrefPercent();
                total_pct += pct;
                let mut val: nscoord = (aISize as f32 * pct) as nscoord;
                if val < min_iSize {
                    val = min_iSize;
                }
                guess_min_pct = NSCoordSaturatingAdd(guess_min_pct, val);
                guess_pref = NSCoordSaturatingAdd(guess_pref, val);
            } else {
                let pref_iSize: nscoord = colFrame.GetPrefCoord();
                if pref_iSize == nscoord_MAX {
                    numInfiniteISizeCols += 1;
                }
                guess_pref = NSCoordSaturatingAdd(guess_pref, pref_iSize);
                guess_min_pct = NSCoordSaturatingAdd(guess_min_pct, min_iSize);
                if colFrame.GetHasSpecifiedCoord() {
                    // we'll add on the rest of guess_min_spec outside the
                    // loop
                    let delta: nscoord = NSCoordSaturatingSubtract(pref_iSize, min_iSize, 0);
                    guess_min_spec = NSCoordSaturatingAdd(guess_min_spec, delta);
                    total_fixed_pref = NSCoordSaturatingAdd(total_fixed_pref, pref_iSize);
                } else if pref_iSize == 0 {
                    if cellMap.GetNumCellsOriginatingInCol(col) > 0 {
                        numNonSpecZeroISizeCols += 1;
                    }
                } else {
                    total_flex_pref = NSCoordSaturatingAdd(total_flex_pref, pref_iSize);
                }
            }
            col += 1;
        }
        guess_min_spec = NSCoordSaturatingAdd(guess_min_spec, guess_min_pct);

        // Determine what we're flexing:
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        enum Loop2Type {
            FLEX_PCT_SMALL,        // between (1) and (2) above
            FLEX_FIXED_SMALL,      // between (2) and (3) above
            FLEX_FLEX_SMALL,       // between (3) and (4) above
            FLEX_FLEX_LARGE,       // greater than (4) above, case (a)
            FLEX_FLEX_LARGE_ZERO,  // greater than (4) above, case (b)
            FLEX_FIXED_LARGE,      // greater than (4) above, case (c)
            FLEX_PCT_LARGE,        // greater than (4) above, case (d)
            FLEX_ALL_LARGE,        // greater than (4) above, case (e)
        }

        let l2t: Loop2Type;
        // These are constants (over columns) for each case's math.  We use
        // a pair of nscoords rather than a float so that we can subtract
        // each column's allocation so we avoid accumulating rounding error.
        let mut space: nscoord; // the amount of extra isize to allocate
        // union { nscoord c; float f; } basis;  // the sum of the statistic over columns to divide it
        // (a union in C++; two variables here, only one of them is meaningful in each case)
        let mut basis_c: nscoord = 0;
        let mut basis_f: f32 = 0.0;
        if aISize < guess_pref {
            if aISizeType != BtlsISizeType::FinalISize && aISize <= guess_min {
                // Return early -- we don't have any extra space to distribute.
                return;
            }
            NS_ASSERTION!(!(aISizeType == BtlsISizeType::FinalISize && aISize < guess_min), "Table inline-size is less than the sum of its columns' min inline-sizes");
            if aISize < guess_min_pct {
                l2t = Loop2Type::FLEX_PCT_SMALL;
                space = aISize - guess_min;
                basis_c = guess_min_pct - guess_min;
            } else if aISize < guess_min_spec {
                l2t = Loop2Type::FLEX_FIXED_SMALL;
                space = aISize - guess_min_pct;
                basis_c = NSCoordSaturatingSubtract(guess_min_spec, guess_min_pct, nscoord_MAX);
            } else {
                l2t = Loop2Type::FLEX_FLEX_SMALL;
                space = aISize - guess_min_spec;
                basis_c = NSCoordSaturatingSubtract(guess_pref, guess_min_spec, nscoord_MAX);
            }
        } else {
            space = NSCoordSaturatingSubtract(aISize, guess_pref, nscoord_MAX);
            if total_flex_pref > 0 {
                l2t = Loop2Type::FLEX_FLEX_LARGE;
                basis_c = total_flex_pref;
            } else if numNonSpecZeroISizeCols > 0 {
                l2t = Loop2Type::FLEX_FLEX_LARGE_ZERO;
                basis_c = numNonSpecZeroISizeCols;
            } else if total_fixed_pref > 0 {
                l2t = Loop2Type::FLEX_FIXED_LARGE;
                basis_c = total_fixed_pref;
            } else if total_pct > 0.0 {
                l2t = Loop2Type::FLEX_PCT_LARGE;
                basis_f = total_pct;
            } else {
                l2t = Loop2Type::FLEX_ALL_LARGE;
                basis_c = aColCount;
            }
        }

        // #ifdef DEBUG_dbaron_off ... #endif
        // NOT PORTED: debug print.

        col = aFirstCol;
        while col < aFirstCol + aColCount {
            let Some(colFrame) = self.mTableFrame.GetColFrame(col) else {
                NS_ERROR!("column frames out of sync with cell map");
                col += 1;
                continue;
            };
            let mut col_iSize: nscoord;

            let pct: f32 = colFrame.GetPrefPercent();
            if pct != 0.0 {
                col_iSize = (aISize as f32 * pct) as nscoord;
                let col_min: nscoord = colFrame.GetMinCoord();
                if col_iSize < col_min {
                    col_iSize = col_min;
                }
            } else {
                col_iSize = colFrame.GetPrefCoord();
            }

            let mut col_iSize_before_adjust: nscoord = col_iSize;

            match l2t {
                Loop2Type::FLEX_PCT_SMALL => {
                    col_iSize = colFrame.GetMinCoord();
                    col_iSize_before_adjust = col_iSize;
                    if pct != 0.0 {
                        let pct_minus_min: nscoord = (aISize as f32 * pct) as nscoord - col_iSize;
                        if pct_minus_min > 0 {
                            let c: f32 = space as f32 / basis_c as f32;
                            basis_c -= pct_minus_min;
                            col_iSize = NSCoordSaturatingAdd(col_iSize, NSToCoordRound__float(pct_minus_min as f32 * c));
                        }
                    }
                }
                Loop2Type::FLEX_FIXED_SMALL => {
                    if pct == 0.0 {
                        NS_ASSERTION!(col_iSize == colFrame.GetPrefCoord(), "wrong inline-size assigned");
                        if colFrame.GetHasSpecifiedCoord() {
                            let col_min: nscoord = colFrame.GetMinCoord();
                            let pref_minus_min: nscoord = col_iSize - col_min;
                            col_iSize = col_min;
                            col_iSize_before_adjust = col_min;
                            if pref_minus_min != 0 {
                                let c: f32 = space as f32 / basis_c as f32;
                                basis_c = NSCoordSaturatingSubtract(basis_c, pref_minus_min, nscoord_MAX);
                                col_iSize = NSCoordSaturatingAdd(col_iSize, NSToCoordRound__float(pref_minus_min as f32 * c));
                            }
                        } else {
                            col_iSize = colFrame.GetMinCoord();
                            col_iSize_before_adjust = col_iSize;
                        }
                    }
                }
                Loop2Type::FLEX_FLEX_SMALL => {
                    if pct == 0.0 && !colFrame.GetHasSpecifiedCoord() {
                        NS_ASSERTION!(col_iSize == colFrame.GetPrefCoord(), "wrong inline-size assigned");
                        let col_min: nscoord = colFrame.GetMinCoord();
                        let pref_minus_min: nscoord = NSCoordSaturatingSubtract(col_iSize, col_min, 0);
                        col_iSize = col_min;
                        col_iSize_before_adjust = col_min;
                        if pref_minus_min != 0 {
                            let mut c: f32 = space as f32 / basis_c as f32;
                            // If we have infinite-isize cols, then the standard
                            // adjustment to col_iSize using 'c' won't work,
                            // because basis.c and pref_minus_min are both
                            // nscoord_MAX and will cancel each other out in the
                            // col_iSize adjustment (making us assign all the
                            // space to the first inf-isize col).  To correct for
                            // this, we'll also divide by numInfiniteISizeCols to
                            // spread the space equally among the inf-isize cols.
                            if numInfiniteISizeCols != 0 {
                                if colFrame.GetPrefCoord() == nscoord_MAX {
                                    c = c / numInfiniteISizeCols as f32;
                                    numInfiniteISizeCols -= 1;
                                } else {
                                    c = 0.0;
                                }
                            }
                            basis_c = NSCoordSaturatingSubtract(basis_c, pref_minus_min, nscoord_MAX);
                            col_iSize = NSCoordSaturatingAdd(col_iSize, NSToCoordRound__float(pref_minus_min as f32 * c));
                        }
                    }
                }
                Loop2Type::FLEX_FLEX_LARGE => {
                    if pct == 0.0 && !colFrame.GetHasSpecifiedCoord() {
                        NS_ASSERTION!(col_iSize == colFrame.GetPrefCoord(), "wrong inline-size assigned");
                        if col_iSize != 0 {
                            if space == nscoord_MAX {
                                basis_c -= col_iSize;
                                col_iSize = nscoord_MAX;
                            } else {
                                let c: f32 = space as f32 / basis_c as f32;
                                basis_c = NSCoordSaturatingSubtract(basis_c, col_iSize, nscoord_MAX);
                                col_iSize = NSCoordSaturatingAdd(col_iSize, NSToCoordRound__float(col_iSize as f32 * c));
                            }
                        }
                    }
                }
                Loop2Type::FLEX_FLEX_LARGE_ZERO => {
                    if pct == 0.0 && !colFrame.GetHasSpecifiedCoord() && cellMap.GetNumCellsOriginatingInCol(col) > 0 {
                        NS_ASSERTION!(col_iSize == 0 && colFrame.GetPrefCoord() == 0, "Since we're in FLEX_FLEX_LARGE_ZERO case, all auto-inline-size cols should have zero pref inline-size.");
                        let c: f32 = space as f32 / basis_c as f32;
                        col_iSize += NSToCoordRound__float(c);
                        basis_c -= 1;
                    }
                }
                Loop2Type::FLEX_FIXED_LARGE => {
                    if pct == 0.0 {
                        NS_ASSERTION!(col_iSize == colFrame.GetPrefCoord(), "wrong inline-size assigned");
                        NS_ASSERTION!(colFrame.GetHasSpecifiedCoord() || colFrame.GetPrefCoord() == 0, "wrong case");
                        if col_iSize != 0 {
                            let c: f32 = space as f32 / basis_c as f32;
                            basis_c = NSCoordSaturatingSubtract(basis_c, col_iSize, nscoord_MAX);
                            col_iSize = NSCoordSaturatingAdd(col_iSize, NSToCoordRound__float(col_iSize as f32 * c));
                        }
                    }
                }
                Loop2Type::FLEX_PCT_LARGE => {
                    NS_ASSERTION!(pct != 0.0 || colFrame.GetPrefCoord() == 0, "wrong case");
                    if pct != 0.0 {
                        let c: f32 = space as f32 / basis_f;
                        col_iSize = NSCoordSaturatingAdd(col_iSize, NSToCoordRound__float(pct * c));
                        basis_f -= pct;
                    }
                }
                Loop2Type::FLEX_ALL_LARGE => {
                    let c: f32 = space as f32 / basis_c as f32;
                    col_iSize = NSCoordSaturatingAdd(col_iSize, NSToCoordRound__float(c));
                    basis_c -= 1;
                }
            }

            // Only subtract from space if it's a real number.
            if space != nscoord_MAX {
                NS_ASSERTION!(col_iSize != nscoord_MAX, "How is col_iSize nscoord_MAX if space isn't?");
                NS_ASSERTION!(col_iSize_before_adjust != nscoord_MAX, "How is col_iSize_before_adjust nscoord_MAX if space isn't?");
                space -= col_iSize - col_iSize_before_adjust;
            }

            NS_ASSERTION!(col_iSize >= colFrame.GetMinCoord(), "assigned inline-size smaller than min");

            // Apply the new isize
            match aISizeType {
                BtlsISizeType::MinISize => {
                    // Note: AddSpanCoords requires both a min and pref isize.
                    // For the pref isize, we'll just pass in our computed
                    // min isize, because the real pref isize will be at least
                    // as big
                    colFrame.AddSpanCoords(col_iSize, col_iSize, aSpanHasSpecifiedISize);
                }
                BtlsISizeType::PrefISize => {
                    // Note: AddSpanCoords requires both a min and pref isize.
                    // For the min isize, we'll just pass in 0, because
                    // the real min isize will be at least 0
                    colFrame.AddSpanCoords(0, col_iSize, aSpanHasSpecifiedISize);
                }
                BtlsISizeType::FinalISize => {
                    let old_final: nscoord = colFrame.GetFinalISize();
                    colFrame.SetFinalISize(col_iSize);

                    if old_final != col_iSize {
                        self.mTableFrame.DidResizeColumns();
                    }
                }
            }
            col += 1;
        }
        NS_ASSERTION!(
            (space == 0 || space == nscoord_MAX) && (if l2t == Loop2Type::FLEX_PCT_LARGE { -0.001 < basis_f && basis_f < 0.001 } else { basis_c == 0 || basis_c == nscoord_MAX }),
            "didn't subtract all that we added"
        );
    }
}

// ADAPTER: the virtual interface (nsITableLayoutStrategy); every method forwards to the inherent
// method of the same name above.
impl nsITableLayoutStrategy for BasicTableLayoutStrategy {
    fn GetMinISize(&mut self, aRenderingContext: Option<gfxContext>) -> nscoord {
        BasicTableLayoutStrategy::GetMinISize(self, aRenderingContext)
    }
    fn GetPrefISize(&mut self, aRenderingContext: Option<gfxContext>, aComputingSize: bool) -> nscoord {
        BasicTableLayoutStrategy::GetPrefISize(self, aRenderingContext, aComputingSize)
    }
    fn MarkIntrinsicISizesDirty(&mut self) {
        BasicTableLayoutStrategy::MarkIntrinsicISizesDirty(self)
    }
    fn ComputeColumnISizes(&mut self, aReflowInput: &ReflowInput) {
        BasicTableLayoutStrategy::ComputeColumnISizes(self, aReflowInput)
    }
    fn GetType(&self) -> nsITableLayoutStrategy_Type {
        self.mBase.GetType()
    }
}

// The column-width tests need a whole table (frames, cell map, table frame); they are in
// nsTableFrame.rs.
