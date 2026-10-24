// Translated from layout/tables/nsTableRowGroupFrame.h and nsTableRowGroupFrame.cpp (GECKO_REV),
// stage 2b.
//
// Rust shape (see CONVENTIONS.md and the top of nsTableCellFrame.rs): `nsTableRowGroupFrame` is a
// `Copy` handle over a frame of type TableRowGroup that derefs to nsIFrame (the C++ class derives
// from nsContainerFrame and nsILineIterator). `nsTableRowGroupFrame*` that may be null is
// `Option<nsTableRowGroupFrame>`. The class has no data members of its own besides the row cursor
// (not ported), so there is no members block. What is virtual in C++ (`Reflow`, `GetUsedMargin`,
// `GetUsedBorder`, `GetUsedPadding`, `GetLogicalSkipSides`) is a `match` on the frame type in
// nsIFrame.rs / nsLineLayoutAdapter.rs; this file has the overrides under their own names.
//
// `TableRowGroupReflowInput` (nsTableRowGroupFrame.cpp, namespace mozilla) is a plain struct.
//
// NOT PORTED: pagination (SplitRowGroup, SplitSpanningCells, CreateContinuingRowFrame,
// UndoContinuedRow, InitRepeatedFrame's callers; each is marked, the first four are
// `unimplemented!`), the row cursor (FrameCursorData, SetupRowCursor, GetFirstRowContaining: they
// speed up painting and hit testing; ClearRowCursor is kept as a no-op because Reflow calls it),
// nsILineIterator (selection, caret and accessibility), painting (BuildDisplayList, DisplayRows),
// invalidation (InvalidateFrame*, InvalidateTableFrame calls), collapsed borders (the BC border
// widths come from the rows and are zero), style-change handling (DidSetComputedStyle). Each is
// marked where it occurs. Refcount logging (MOZ_COUNT_CTOR) is not ported.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::Deref;

// Gecko: layout/tables/nsTableRowGroupFrame.h MIN_ROWS_NEEDING_CURSOR
pub const MIN_ROWS_NEEDING_CURSOR: usize = 20;

// Gecko: layout/tables/nsTableRowGroupFrame.cpp mozilla::TableRowGroupReflowInput
pub struct TableRowGroupReflowInput<'a, 'b> {
    // Our reflow input
    pub mReflowInput: &'a ReflowInput<'b>,

    // The available size (computed from the parent)
    pub mAvailSize: LogicalSize,

    // Running block-offset
    pub mBCoord: nscoord,
}

impl<'a, 'b> TableRowGroupReflowInput<'a, 'b> {
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp mozilla::TableRowGroupReflowInput::TableRowGroupReflowInput
    pub fn new(aReflowInput: &'a ReflowInput<'b>) -> TableRowGroupReflowInput<'a, 'b> {
        TableRowGroupReflowInput { mReflowInput: aReflowInput, mAvailSize: aReflowInput.AvailableSize__(), mBCoord: 0 }
    }
}

// Gecko: layout/tables/nsTableRowGroupFrame.h nsTableRowGroupFrame
// ADAPTER: a handle (see the notes at the top of the file).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct nsTableRowGroupFrame(pub nsIFrame);

impl Deref for nsTableRowGroupFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/tables/nsTableRowGroupFrame.cpp NS_NewTableRowGroupFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class and what the C++
// constructor sets (`SetRepeatable(false)`).
pub fn NS_NewTableRowGroupFrame(aFrame: nsIFrame) -> nsTableRowGroupFrame {
    aFrame.SetType(LayoutFrameType::TableRowGroup);
    let rowGroup = nsTableRowGroupFrame(aFrame);
    rowGroup.SetRepeatable(false);
    rowGroup
}

impl nsTableRowGroupFrame {
    // static_cast<nsTableRowGroupFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsTableRowGroupFrame {
        nsTableRowGroupFrame(aFrame)
    }

    // do_QueryFrame<nsTableRowGroupFrame>(frame)
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp NS_QUERYFRAME_HEAD(nsTableRowGroupFrame)
    // (the class also answers to nsILineIterator; NOT PORTED)
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsTableRowGroupFrame> {
        if aFrame.Type() == LayoutFrameType::TableRowGroup {
            Some(nsTableRowGroupFrame(aFrame))
        } else {
            None
        }
    }

    // nsIFrame overrides
    // Gecko: layout/tables/nsTableRowGroupFrame.h nsTableRowGroupFrame::Init
    pub fn Init(&self, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: Option<nsIFrame>) {
        self.nsContainerFrame_Init(aContent, aParent, aPrevInFlow);
        if aPrevInFlow.is_none() {
            self.SetWritingMode(self.GetTableFrame().GetWritingMode());
        }
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::Destroy
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        nsTableFrame::MaybeUnregisterPositionedTablePart(self.0);
        self.nsContainerFrame_Destroy(aContext);
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetRowCount
    pub fn GetRowCount(&self) -> i32 {
        #[cfg(debug_assertions)]
        {
            for f in self.mFrames().begin() {
                NS_ASSERTION!(f.StyleDisplay().mDisplay() == StyleDisplay::TableRow, "Unexpected display");
                NS_ASSERTION!(f.IsTableRowFrame(), "Unexpected frame type");
            }
        }

        self.mFrames().GetLength()
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetStartRowIndex
    pub fn GetStartRowIndex(&self) -> i32 {
        let mut result: i32 = -1;
        if self.mFrames().NotEmpty() {
            NS_ASSERTION!(self.mFrames().FirstChild().unwrap().IsTableRowFrame(), "Unexpected frame type");
            result = nsTableRowFrame::from_frame(self.mFrames().FirstChild().unwrap()).GetRowIndex();
        }
        // if the row group doesn't have any children, get it the hard way
        if -1 == result {
            return self.GetTableFrame().GetStartRowIndex(&*self);
        }

        result
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::AdjustRowIndices
    pub fn AdjustRowIndices(&self, aRowIndex: i32, anAdjustment: i32) {
        for rowFrame in self.mFrames().begin() {
            if StyleDisplay::TableRow == rowFrame.StyleDisplay().mDisplay() {
                let index = nsTableRowFrame::from_frame(rowFrame).GetRowIndex();
                if index >= aRowIndex {
                    nsTableRowFrame::from_frame(rowFrame).SetRowIndex(index + anAdjustment);
                }
            }
        }
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetAdjustmentForStoredIndex
    pub fn GetAdjustmentForStoredIndex(&self, aStoredIndex: i32) -> i32 {
        let tableFrame = self.GetTableFrame();
        tableFrame.GetAdjustmentForStoredIndex(aStoredIndex)
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::MarkRowsAsDeleted
    pub fn MarkRowsAsDeleted(&self, aStartRowFrame: nsTableRowFrame, mut aNumRowsToDelete: i32) {
        let mut currentRowFrame = aStartRowFrame;
        loop {
            // XXXneerja - Instead of calling AddDeletedRowIndex() per row frame
            // it is possible to change AddDeleteRowIndex to instead take
            // <start row index> and <num of rows to mark for deletion> as arguments.
            // The problem that emerges here is mDeletedRowIndexRanges only stores
            // disjoint index ranges and since AddDeletedRowIndex() must operate on
            // the "stored" index, in some cases it is possible that the range
            // of indices to delete becomes overlapping EG: Deleting rows 9 - 11 and
            // then from the remaining rows deleting the *new* rows 7 to 20.
            // Handling these overlapping ranges is much more complicated to
            // implement and so I opted to add the deleted row index of one row at a
            // time and maintain the invariant that the range of deleted row indices
            // is always disjoint.
            currentRowFrame.AddDeletedRowIndex();
            aNumRowsToDelete -= 1;
            if aNumRowsToDelete == 0 {
                break;
            }
            match currentRowFrame.GetNextSibling().and_then(nsTableRowFrame::do_QueryFrame) {
                Some(next) => currentRowFrame = next,
                None => {
                    MOZ_ASSERT_UNREACHABLE!("expected another row frame");
                    break;
                }
            }
        }
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::AddDeletedRowIndex
    pub fn AddDeletedRowIndex(&self, aDeletedRowStoredIndex: i32) {
        let tableFrame = self.GetTableFrame();
        tableFrame.AddDeletedRowIndex(aDeletedRowStoredIndex)
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::InitRepeatedFrame
    pub fn InitRepeatedFrame(&self, aHeaderFooterFrame: nsTableRowGroupFrame) {
        let mut copyRowFrame = self.GetFirstRow();
        let mut originalRowFrame = aHeaderFooterFrame.GetFirstRow();
        self.AddStateBits(NS_REPEATED_ROW_OR_ROWGROUP);
        while let (Some(copyRow), Some(originalRow)) = (copyRowFrame, originalRowFrame) {
            copyRow.AddStateBits(NS_REPEATED_ROW_OR_ROWGROUP);
            let rowIndex = originalRow.GetRowIndex();
            copyRow.SetRowIndex(rowIndex);

            // For each table cell frame set its column index
            let mut originalCellFrame = originalRow.GetFirstCell();
            let mut copyCellFrame = copyRow.GetFirstCell();
            while let (Some(copyCell), Some(originalCell)) = (copyCellFrame, originalCellFrame) {
                NS_ASSERTION!(nsIContent::IsSameNode(&originalCell.GetContent(), &copyCell.GetContent()), "cell frames have different content");
                let colIndex = originalCell.ColIndex();
                copyCell.SetColIndex(colIndex as i32);

                // Move to the next cell frame
                copyCellFrame = copyCell.GetNextCell();
                originalCellFrame = originalCell.GetNextCell();
            }

            // Move to the next row frame
            originalRowFrame = originalRow.GetNextRow();
            copyRowFrame = copyRow.GetNextRow();
        }
    }

    // Handle the child-traversal part of DisplayGenericTablePart
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp DisplayRows
    // NOT PORTED: painting (nsTableRowGroupFrame.cpp lines 175-229 at GECKO_REV).

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::BuildDisplayList
    // NOT PORTED: painting.

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetLogicalSkipSides
    pub fn GetLogicalSkipSides(&self) -> LogicalSides {
        let mWritingMode = self.GetWritingMode();
        let mut skip = LogicalSides::new__WritingMode(mWritingMode);
        if self.StyleBorder().mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone {
            return skip;
        }

        if self.GetPrevInFlow().is_some() {
            skip += LogicalSide::BStart;
        }
        if self.GetNextInFlow().is_some() {
            skip += LogicalSide::BEnd;
        }
        skip
    }

    // Position and size aKidFrame and update our reflow input.
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::PlaceChild
    fn PlaceChild(
        &self,
        aPresContext: &nsPresContext,
        aReflowInput: &mut TableRowGroupReflowInput,
        aKidFrame: nsIFrame,
        aKidReflowInput: &ReflowInput,
        aWM: WritingMode,
        aKidPosition: LogicalPoint,
        aContainerSize: nsSize,
        aDesiredSize: &mut ReflowOutput,
        aOriginalKidRect: nsRect,
        aOriginalKidInkOverflow: nsRect,
    ) {
        let isFirstReflow = aKidFrame.HasAnyStateBits(NS_FRAME_FIRST_REFLOW);

        // Place and size the child
        nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
            aKidFrame,
            aPresContext,
            aDesiredSize,
            Some(aKidReflowInput),
            aWM,
            aKidPosition,
            aContainerSize,
            nsIFrame_ReflowChildFlags::ApplyRelativePositioning,
        );

        let tableFrame = self.GetTableFrame();
        if tableFrame.IsBorderCollapse() {
            // nsTableFrame::InvalidateTableFrame(aKidFrame, aOriginalKidRect, aOriginalKidInkOverflow, isFirstReflow);
            // NOT PORTED: invalidation.
        }

        // Adjust the running block-offset
        aReflowInput.mBCoord += aDesiredSize.BSize(aWM);

        // If our block-size is constrained then update the available bsize
        if NS_UNCONSTRAINEDSIZE != aReflowInput.mAvailSize.BSize(aWM) {
            *aReflowInput.mAvailSize.BSize_mut(aWM) -= aDesiredSize.BSize(aWM);
        }
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::InitChildReflowInput
    fn InitChildReflowInput(&self, aPresContext: &nsPresContext, aBorderCollapse: bool, aReflowInput: &mut ReflowInput) {
        let childWM = aReflowInput.GetWritingMode();
        let mut border = LogicalMargin::new__WritingMode(childWM);
        if aBorderCollapse {
            let rowFrame = nsTableRowFrame::from_frame(aReflowInput.mFrame);
            border = rowFrame.GetBCBorderWidth(childWM);
        }
        let zeroPadding = LogicalMargin::new__WritingMode(childWM);
        aReflowInput.Init(aPresContext, None, Some(border), Some(zeroPadding));
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp CacheRowBSizesForPrinting
    fn CacheRowBSizesForPrinting(aFirstRow: Option<nsTableRowFrame>, aWM: WritingMode) {
        let mut next = aFirstRow;
        while let Some(row) = next {
            if row.GetPrevInFlow().is_none() {
                row.SetUnpaginatedBSize(row.BSize(aWM));
            }
            next = row.GetNextRow();
        }
    }

    // Reflow the frames we've already created
    //
    // @param   aPresContext presentation context to use
    // @param   aReflowInput current inline state
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::ReflowChildren
    // (aPageBreakBeforeEnd defaults to nullptr in C++)
    fn ReflowChildren(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &mut TableRowGroupReflowInput, aStatus: &mut nsReflowStatus, aPageBreakBeforeEnd: Option<&mut bool>) {
        let mut aPageBreakBeforeEnd = aPageBreakBeforeEnd;
        if let Some(b) = aPageBreakBeforeEnd.as_deref_mut() {
            *b = false;
        }

        let wm = aReflowInput.mReflowInput.GetWritingMode();
        let tableFrame = self.GetTableFrame();
        let borderCollapse = tableFrame.IsBorderCollapse();

        // XXXldb Should we really be checking IsPaginated(),
        // or should we *only* check available block-size?
        // (Think about multi-column layout!)
        let isPaginated = aPresContext.IsPaginated() && NS_UNCONSTRAINEDSIZE != aReflowInput.mAvailSize.BSize(wm);

        let reflowAllKids = aReflowInput.mReflowInput.ShouldReflowAllKids() || tableFrame.IsGeometryDirty() || tableFrame.NeedToCollapse();

        // in vertical-rl mode, we always need the row bsizes in order to
        // get the necessary containerSize for placing our kids
        let mut needToCalcRowBSizes = reflowAllKids || wm.IsVerticalRL();

        let containerSize = aReflowInput.mReflowInput.ComputedSizeAsContainerIfConstrained();

        let mut prevKidFrame: Option<nsIFrame> = None;
        let mut kid = self.GetFirstRow();
        while let Some(kidFrame) = kid {
            let rowSpacing = tableFrame.GetRowSpacing__int32_t(kidFrame.GetRowIndex());

            // Reflow the row frame
            if reflowAllKids || kidFrame.IsSubtreeDirty() || (aReflowInput.mReflowInput.mFlags.mSpecialBSizeReflow && (isPaginated || kidFrame.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE))) {
                let oldKidRect = kidFrame.GetLogicalRect__WritingMode_nsSize(wm, containerSize);
                let oldKidInkOverflow = nsRect::default();
                // nsRect oldKidInkOverflow = kidFrame->InkOverflowRect();
                // NOT PORTED: ink overflow is only used by InvalidateTableFrame (the value is the
                // empty rect here).

                let mut kidDesiredSize = ReflowOutput::new__ReflowInput(aReflowInput.mReflowInput);

                // Reflow the child into the available space, giving it as much bsize as
                // it wants. We'll deal with splitting later after we've computed the row
                // bsizes, taking into account cells with row spans...
                let mut kidAvailSize = aReflowInput.mAvailSize;
                *kidAvailSize.BSize_mut(wm) = NS_UNCONSTRAINEDSIZE;
                let mut initFlags = ReflowInput_InitFlags::new();
                initFlags += ReflowInput_InitFlag::CallerWillInit;
                let mut kidReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                    aPresContext,
                    aReflowInput.mReflowInput,
                    kidFrame.0,
                    kidAvailSize,
                    None,
                    initFlags,
                    StyleSizeOverrides::default(),
                    ComputeSizeFlags::default(),
                    std::ptr::null_mut(),
                );
                self.InitChildReflowInput(aPresContext, borderCollapse, &mut kidReflowInput);

                // This can indicate that columns were resized.
                if aReflowInput.mReflowInput.IsIResize() {
                    kidReflowInput.SetIResize(true);
                }

                NS_ASSERTION!(Some(kidFrame.0) == self.mFrames().FirstChild() || prevKidFrame.is_some(), "If we're not on the first frame, we should have a previous sibling...");
                // If prev row has nonzero YMost, then we can't be at the top of the page
                if let Some(prev) = prevKidFrame {
                    if prev.GetNormalRect().YMost() > 0 {
                        kidReflowInput.mFlags.mIsTopOfPage = false;
                    }
                }

                let kidPosition = LogicalPoint::new(wm, 0, aReflowInput.mBCoord);
                self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
                    kidFrame.0,
                    aPresContext,
                    &mut kidDesiredSize,
                    &kidReflowInput,
                    wm,
                    kidPosition,
                    containerSize,
                    nsIFrame_ReflowChildFlags::Default,
                    aStatus,
                    None,
                );

                // Place the child
                self.PlaceChild(aPresContext, aReflowInput, kidFrame.0, &kidReflowInput, wm, kidPosition, containerSize, &mut kidDesiredSize, oldKidRect.GetPhysicalRect(wm, containerSize), oldKidInkOverflow);
                aReflowInput.mBCoord += rowSpacing;

                if !reflowAllKids {
                    if self.IsSimpleRowFrame(tableFrame, kidFrame) {
                        // Inform the row of its new bsize.
                        kidFrame.DidResize(ForceAlignTopForTableCell::No);
                        // the overflow area may have changed inflate the overflow area
                        let stylePos = self.StylePosition();
                        if tableFrame.IsAutoBSize(wm) && !stylePos.BSize(wm, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.0, std::ptr::null_mut())).ConvertsToLength() {
                            // Because other cells in the row may need to be aligned
                            // differently, repaint the entire row
                            // InvalidateFrame();
                            // NOT PORTED: invalidation.
                        } else if oldKidRect.BSize(wm) != kidDesiredSize.BSize(wm) {
                            needToCalcRowBSizes = true;
                        }
                    } else {
                        needToCalcRowBSizes = true;
                    }
                }

                if isPaginated {
                    if let Some(b) = aPageBreakBeforeEnd.as_deref_mut() {
                        if !*b {
                            let nextRow = kidFrame.GetNextRow();
                            if let Some(nextRow) = nextRow {
                                *b = nsTableFrame::PageBreakAfter(kidFrame.0, Some(nextRow.0));
                            }
                        }
                    }
                }
            } else {
                // Move a child that was skipped during a reflow.
                let oldPosition = kidFrame.GetLogicalNormalPosition(wm, containerSize);
                if oldPosition.B(wm) != aReflowInput.mBCoord {
                    // kidFrame->InvalidateFrameSubtree();
                    // NOT PORTED: invalidation.
                    let offset = LogicalPoint::new(wm, 0, aReflowInput.mBCoord - oldPosition.B(wm));
                    kidFrame.MovePositionBy__WritingMode_LogicalPoint(wm, offset);
                    // kidFrame->InvalidateFrameSubtree();
                    // NOT PORTED: invalidation.
                }

                // Adjust the running b-offset so we know where the next row should be
                // placed
                let bSize = kidFrame.BSize(wm) + rowSpacing;
                aReflowInput.mBCoord += bSize;

                if NS_UNCONSTRAINEDSIZE != aReflowInput.mAvailSize.BSize(wm) {
                    *aReflowInput.mAvailSize.BSize_mut(wm) -= bSize;
                }
            }
            self.ConsiderChildOverflow(&mut aDesiredSize.mOverflowAreas, kidFrame.0, OverflowAreaUnionFlags::None);

            prevKidFrame = Some(kidFrame.0);
            kid = kidFrame.GetNextRow();
        }

        if self.GetFirstRow().is_some() {
            aReflowInput.mBCoord -= tableFrame.GetRowSpacing__int32_t(self.GetStartRowIndex() + self.GetRowCount());
        }

        // Return our desired rect
        *aDesiredSize.ISize_mut(wm) = aReflowInput.mReflowInput.AvailableISize();
        *aDesiredSize.BSize_mut(wm) = aReflowInput.mBCoord;

        if aReflowInput.mReflowInput.mFlags.mSpecialBSizeReflow {
            self.DidResizeRows(aDesiredSize);
            if isPaginated {
                nsTableRowGroupFrame::CacheRowBSizesForPrinting(self.GetFirstRow(), wm);
            }
        } else if needToCalcRowBSizes {
            self.CalculateRowBSizes(aPresContext, aDesiredSize, aReflowInput.mReflowInput);
            if !reflowAllKids {
                // InvalidateFrame();
                // NOT PORTED: invalidation.
            }
        }
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetFirstRow
    pub fn GetFirstRow(&self) -> Option<nsTableRowFrame> {
        let firstChild = self.mFrames().FirstChild();
        MOZ_ASSERT!(firstChild.map_or(true, |c| nsTableRowFrame::do_QueryFrame(c).is_some()), "How do we have a non-row child?");
        firstChild.map(nsTableRowFrame)
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetLastRow
    pub fn GetLastRow(&self) -> Option<nsTableRowFrame> {
        let lastChild = self.mFrames().LastChild();
        MOZ_ASSERT!(lastChild.map_or(true, |c| nsTableRowFrame::do_QueryFrame(c).is_some()), "How do we have a non-row child?");
        lastChild.map(nsTableRowFrame)
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp UpdateBSizes
    fn UpdateBSizes(aRowInfo: &mut RowInfo, aAdditionalBSize: nscoord, aTotal: &mut nscoord, aUnconstrainedTotal: &mut nscoord) {
        aRowInfo.bSize = (aRowInfo.bSize as i64 + aAdditionalBSize as i64) as u32;
        *aTotal += aAdditionalBSize;
        if !aRowInfo.hasStyleBSize {
            *aUnconstrainedTotal += aAdditionalBSize;
        }
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::DidResizeRows
    fn DidResizeRows(&self, aDesiredSize: &mut ReflowOutput) {
        // Update the cells spanning rows with their new bsizes.
        // This is the place where all of the cells in the row get set to the bsize
        // of the row.
        // Reset the overflow area.
        aDesiredSize.mOverflowAreas.Clear();
        let mut row = self.GetFirstRow();
        while let Some(rowFrame) = row {
            rowFrame.DidResize(ForceAlignTopForTableCell::No);
            self.ConsiderChildOverflow(&mut aDesiredSize.mOverflowAreas, rowFrame.0, OverflowAreaUnionFlags::None);
            row = rowFrame.GetNextRow();
        }
    }

    // This calculates the bsize of all the rows and takes into account
    // style bsize on the row group, style bsizes on rows and cells, style bsizes on
    // rowspans. Actual row bsizes will be adjusted later if the table has a style
    // bsize. Even if rows don't change bsize, this method must be called to set the
    // bsizes of each cell in the row to the bsize of its row.
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::CalculateRowBSizes
    fn CalculateRowBSizes(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput) {
        let tableFrame = self.GetTableFrame();
        let isPaginated = aPresContext.IsPaginated();

        let numEffCols = tableFrame.GetEffectiveColCount();

        let startRowIndex = self.GetStartRowIndex();
        // find the row corresponding to the row index we just found
        let startRowFrame = self.GetFirstRow();

        let Some(startRowFrame) = startRowFrame else {
            return;
        };

        // The current row group block-size is the block-origin of the 1st row
        // we are about to calculate a block-size for.
        let wm = aReflowInput.GetWritingMode();
        let mut containerSize = nsSize::new(0, 0); // actual value is unimportant as we're initially
                                                   // computing sizes, not physical positions
        let startRowGroupBSize = startRowFrame.GetLogicalNormalPosition(wm, containerSize).B(wm);

        let numRows = self.GetRowCount() - (startRowFrame.GetRowIndex() - self.GetStartRowIndex());
        // Collect the current bsize of each row.
        if numRows <= 0 {
            return;
        }

        // AutoTArray<RowInfo, 32> rowInfo; rowInfo.AppendElements(numRows);
        let mut rowInfo: Vec<RowInfo> = (0..numRows).map(|_| RowInfo::default()).collect();

        let mut hasRowSpanningCell = false;
        let mut bSizeOfRows: nscoord = 0;
        let mut bSizeOfUnStyledRows: nscoord = 0;
        // Get the bsize of each row without considering rowspans. This will be the
        // max of the largest desired bsize of each cell, the largest style bsize of
        // each cell, the style bsize of the row.
        let pctBSizeBasis = self.GetBSizeBasis(aReflowInput);
        let mut rowIndex: i32; // the index in rowInfo, not among the rows in the row group
        let mut rowFrame: Option<nsTableRowFrame>;
        rowFrame = Some(startRowFrame);
        rowIndex = 0;
        while let Some(rowF) = rowFrame {
            let mut nonPctBSize = rowF.GetContentBSize();
            if isPaginated {
                nonPctBSize = std::cmp::max(nonPctBSize, rowF.BSize(wm));
            }
            if rowF.GetPrevInFlow().is_none() {
                if rowF.HasPctBSize() {
                    rowInfo[rowIndex as usize].hasPctBSize = true;
                    rowInfo[rowIndex as usize].pctBSize = rowF.GetInitialBSize(pctBSizeBasis) as u32;
                }
                rowInfo[rowIndex as usize].hasStyleBSize = rowF.HasStyleBSize();
                nonPctBSize = std::cmp::max(nonPctBSize, rowF.GetFixedBSize());
            }
            nsTableRowGroupFrame::UpdateBSizes(&mut rowInfo[rowIndex as usize], nonPctBSize, &mut bSizeOfRows, &mut bSizeOfUnStyledRows);

            if !rowInfo[rowIndex as usize].hasStyleBSize {
                if isPaginated || tableFrame.HasMoreThanOneCell(rowIndex + startRowIndex) {
                    rowInfo[rowIndex as usize].isSpecial = true;
                    // iteratate the row's cell frames to see if any do not have rowspan > 1
                    let mut cell = rowF.GetFirstCell();
                    while let Some(cellFrame) = cell {
                        let rowSpan = tableFrame.GetEffectiveRowSpan__int32_t_nsTableCellFrame(rowIndex + startRowIndex, &cellFrame);
                        if 1 == rowSpan {
                            rowInfo[rowIndex as usize].isSpecial = false;
                            break;
                        }
                        cell = cellFrame.GetNextCell();
                    }
                }
            }
            // See if a cell spans into the row. If so we'll have to do the next step
            if !hasRowSpanningCell {
                if tableFrame.RowIsSpannedInto(rowIndex + startRowIndex, numEffCols) {
                    hasRowSpanningCell = true;
                }
            }
            rowFrame = rowF.GetNextRow();
            rowIndex += 1;
        }

        if hasRowSpanningCell {
            // Get the bsize of cells with rowspans and allocate any extra space to the
            // rows they span iteratate the child frames and process the row frames
            // among them
            rowFrame = Some(startRowFrame);
            rowIndex = 0;
            while let Some(rowF) = rowFrame {
                // See if the row has an originating cell with rowspan > 1. We cannot
                // determine this for a row in a continued row group by calling
                // RowHasSpanningCells, because the row's fif may not have any originating
                // cells yet the row may have a continued cell which originates in it.
                if self.GetPrevInFlow().is_some() || tableFrame.RowHasSpanningCells(startRowIndex + rowIndex, numEffCols) {
                    let mut cell = rowF.GetFirstCell();
                    // iteratate the row's cell frames
                    while let Some(cellFrame) = cell {
                        let rowSpacing = tableFrame.GetRowSpacing__int32_t(startRowIndex + rowIndex);
                        let mut rowSpan = tableFrame.GetEffectiveRowSpan__int32_t_nsTableCellFrame(rowIndex + startRowIndex, &cellFrame);
                        if (rowIndex + rowSpan) > numRows {
                            // there might be rows pushed already to the nextInFlow
                            rowSpan = numRows - rowIndex;
                        }
                        if rowSpan > 1 {
                            // a cell with rowspan > 1, determine the bsize of
                            // the rows it spans
                            let mut bsizeOfRowsSpanned: nscoord = 0;
                            let mut bsizeOfUnStyledRowsSpanned: nscoord = 0;
                            let mut numSpecialRowsSpanned: nscoord = 0;
                            let mut cellSpacingTotal: nscoord = 0;
                            let mut spanX: i32;
                            spanX = 0;
                            while spanX < rowSpan {
                                bsizeOfRowsSpanned += rowInfo[(rowIndex + spanX) as usize].bSize as nscoord;
                                if !rowInfo[(rowIndex + spanX) as usize].hasStyleBSize {
                                    bsizeOfUnStyledRowsSpanned += rowInfo[(rowIndex + spanX) as usize].bSize as nscoord;
                                }
                                if 0 != spanX {
                                    cellSpacingTotal += rowSpacing;
                                }
                                if rowInfo[(rowIndex + spanX) as usize].isSpecial {
                                    numSpecialRowsSpanned += 1;
                                }
                                spanX += 1;
                            }
                            let bsizeOfAreaSpanned = bsizeOfRowsSpanned + cellSpacingTotal;
                            // get the bsize of the cell
                            let mut cellFrameSize = cellFrame.GetLogicalSize__WritingMode(wm);
                            let mut cellDesSize = cellFrame.GetDesiredSize();
                            *cellDesSize.BSize_mut(wm) = rowF.CalcCellActualBSize(cellFrame, &cellDesSize.BSize(wm), wm);
                            *cellFrameSize.BSize_mut(wm) = cellDesSize.BSize(wm);

                            if bsizeOfAreaSpanned < cellFrameSize.BSize(wm) {
                                // the cell's bsize is larger than the available space of the rows
                                // it spans so distribute the excess bsize to the rows affected
                                let extra = cellFrameSize.BSize(wm) - bsizeOfAreaSpanned;
                                let mut extraUsed: nscoord = 0;
                                if 0 == numSpecialRowsSpanned {
                                    // NS_ASSERTION(bsizeOfRowsSpanned > 0, "invalid row span
                                    // situation");
                                    let haveUnStyledRowsSpanned = bsizeOfUnStyledRowsSpanned > 0;
                                    let divisor = if haveUnStyledRowsSpanned { bsizeOfUnStyledRowsSpanned } else { bsizeOfRowsSpanned };
                                    if divisor > 0 {
                                        spanX = rowSpan - 1;
                                        while spanX >= 0 {
                                            if !haveUnStyledRowsSpanned || !rowInfo[(rowIndex + spanX) as usize].hasStyleBSize {
                                                // The amount of additional space each row gets is
                                                // proportional to its bsize
                                                let percent = (rowInfo[(rowIndex + spanX) as usize].bSize as f32) / (divisor as f32);

                                                // give rows their percentage, except for the first row
                                                // which gets the remainder
                                                let mut extraForRow = if 0 == spanX { extra - extraUsed } else { NSToCoordRound__float((extra as f32) * percent) };
                                                extraForRow = std::cmp::min(extraForRow, extra - extraUsed);
                                                // update the row bsize
                                                nsTableRowGroupFrame::UpdateBSizes(&mut rowInfo[(rowIndex + spanX) as usize], extraForRow, &mut bSizeOfRows, &mut bSizeOfUnStyledRows);
                                                extraUsed += extraForRow;
                                                if extraUsed >= extra {
                                                    NS_ASSERTION!(extraUsed == extra, "invalid row bsize calculation");
                                                    break;
                                                }
                                            }
                                            spanX -= 1;
                                        }
                                    } else {
                                        // put everything in the last row
                                        nsTableRowGroupFrame::UpdateBSizes(&mut rowInfo[(rowIndex + rowSpan - 1) as usize], extra, &mut bSizeOfRows, &mut bSizeOfUnStyledRows);
                                    }
                                } else {
                                    // give the extra to the special rows
                                    let mut numSpecialRowsAllocated: nscoord = 0;
                                    spanX = rowSpan - 1;
                                    while spanX >= 0 {
                                        if rowInfo[(rowIndex + spanX) as usize].isSpecial {
                                            // The amount of additional space each degenerate row gets
                                            // is proportional to the number of them
                                            let percent = 1.0f32 / (numSpecialRowsSpanned as f32);

                                            // give rows their percentage, except for the first row
                                            // which gets the remainder
                                            let mut extraForRow = if numSpecialRowsSpanned - 1 == numSpecialRowsAllocated { extra - extraUsed } else { NSToCoordRound__float((extra as f32) * percent) };
                                            extraForRow = std::cmp::min(extraForRow, extra - extraUsed);
                                            // update the row bsize
                                            nsTableRowGroupFrame::UpdateBSizes(&mut rowInfo[(rowIndex + spanX) as usize], extraForRow, &mut bSizeOfRows, &mut bSizeOfUnStyledRows);
                                            extraUsed += extraForRow;
                                            if extraUsed >= extra {
                                                NS_ASSERTION!(extraUsed == extra, "invalid row bsize calculation");
                                                break;
                                            }
                                        }
                                        spanX -= 1;
                                    }
                                }
                            }
                        } // if (rowSpan > 1)
                        cell = cellFrame.GetNextCell();
                    } // while (cellFrame)
                } // if (tableFrame->RowHasSpanningCells(startRowIndex + rowIndex) {
                rowFrame = rowF.GetNextRow();
                rowIndex += 1;
            } // while (rowFrame)
        }

        // pct bsize rows have already got their content bsizes.
        // Give them their pct bsizes up to pctBSizeBasis
        let mut extra = pctBSizeBasis - bSizeOfRows;
        rowFrame = Some(startRowFrame);
        rowIndex = 0;
        while let Some(rowF) = rowFrame {
            if extra <= 0 {
                break;
            }
            let rInfo = &mut rowInfo[rowIndex as usize];
            if rInfo.hasPctBSize {
                let mut rowExtra = if rInfo.pctBSize > rInfo.bSize { (rInfo.pctBSize - rInfo.bSize) as nscoord } else { 0 };
                rowExtra = std::cmp::min(rowExtra, extra);
                nsTableRowGroupFrame::UpdateBSizes(rInfo, rowExtra, &mut bSizeOfRows, &mut bSizeOfUnStyledRows);
                extra -= rowExtra;
            }
            rowFrame = rowF.GetNextRow();
            rowIndex += 1;
        }

        let mut styleBSizeAllocation = false;
        let mut rowGroupBSize = startRowGroupBSize + bSizeOfRows + tableFrame.GetRowSpacing__int32_t_int32_t(0, numRows - 1);
        // if we have a style bsize, allocate the extra bsize to unconstrained rows
        if (aReflowInput.ComputedBSize() > rowGroupBSize) && (NS_UNCONSTRAINEDSIZE != aReflowInput.ComputedBSize()) {
            let extraComputedBSize = aReflowInput.ComputedBSize() - rowGroupBSize;
            let mut extraUsed: nscoord = 0;
            let haveUnStyledRows = bSizeOfUnStyledRows > 0;
            let divisor = if haveUnStyledRows { bSizeOfUnStyledRows } else { bSizeOfRows };
            if divisor > 0 {
                styleBSizeAllocation = true;
                rowIndex = 0;
                while rowIndex < numRows {
                    if !haveUnStyledRows || !rowInfo[rowIndex as usize].hasStyleBSize {
                        // The amount of additional space each row gets is based on the
                        // percentage of space it occupies
                        let percent = (rowInfo[rowIndex as usize].bSize as f32) / (divisor as f32);
                        // give rows their percentage, except for the last row which gets the
                        // remainder
                        let mut extraForRow = if numRows - 1 == rowIndex { extraComputedBSize - extraUsed } else { NSToCoordRound__float((extraComputedBSize as f32) * percent) };
                        extraForRow = std::cmp::min(extraForRow, extraComputedBSize - extraUsed);
                        // update the row bsize
                        nsTableRowGroupFrame::UpdateBSizes(&mut rowInfo[rowIndex as usize], extraForRow, &mut bSizeOfRows, &mut bSizeOfUnStyledRows);
                        extraUsed += extraForRow;
                        if extraUsed >= extraComputedBSize {
                            NS_ASSERTION!(extraUsed == extraComputedBSize, "invalid row bsize calculation");
                            break;
                        }
                    }
                    rowIndex += 1;
                }
            }
            rowGroupBSize = aReflowInput.ComputedBSize();
        }

        if wm.IsVertical() {
            // we need the correct containerSize below for block positioning in
            // vertical-rl writing mode
            containerSize.width = rowGroupBSize;
        }

        let mut bOrigin = startRowGroupBSize;
        // update the rows with their (potentially) new bsizes
        rowFrame = Some(startRowFrame);
        rowIndex = 0;
        while let Some(rowF) = rowFrame {
            let rowBounds = rowF.GetRect();
            let rowBoundsSize = LogicalSize::new__WritingMode_nsSize(wm, rowBounds.Size());
            // nsRect rowInkOverflow = rowFrame->InkOverflowRect();
            // NOT PORTED: invalidation (only used by InvalidateTableFrame).
            let deltaB = bOrigin - rowF.GetLogicalNormalPosition(wm, containerSize).B(wm);

            let rowBSize = if rowInfo[rowIndex as usize].bSize > 0 { rowInfo[rowIndex as usize].bSize as nscoord } else { 0 };

            if deltaB != 0 || (rowBSize != rowBoundsSize.BSize(wm)) {
                // Resize/move the row to its final size and position
                if deltaB != 0 {
                    // rowFrame->InvalidateFrameSubtree();
                    // NOT PORTED: invalidation.
                }

                rowF.MovePositionBy__WritingMode_LogicalPoint(wm, LogicalPoint::new(wm, 0, deltaB));
                rowF.SetSize__WritingMode_LogicalSize(rowF.GetWritingMode(), LogicalSize::new(wm, rowBoundsSize.ISize(wm), rowBSize));
                // (rowFrame->SetSize(LogicalSize) takes the frame's own writing mode; the rows have
                // the table's writing mode, which is wm.)

                // nsTableFrame::InvalidateTableFrame(rowFrame, rowBounds, rowInkOverflow, false);
                // NOT PORTED: invalidation.

                if deltaB != 0 {
                    // XXXbz we don't need to update our overflow area?
                }
            }
            bOrigin += rowBSize + tableFrame.GetRowSpacing__int32_t(startRowIndex + rowIndex);
            rowFrame = rowF.GetNextRow();
            rowIndex += 1;
        }

        if isPaginated && styleBSizeAllocation {
            // since the row group has a style bsize, cache the row bsizes,
            // so next in flows can honor them
            nsTableRowGroupFrame::CacheRowBSizesForPrinting(self.GetFirstRow(), wm);
        }

        self.DidResizeRows(aDesiredSize);

        *aDesiredSize.BSize_mut(wm) = rowGroupBSize; // Adjust our desired size
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::CollapseRowGroupIfNecessary
    pub fn CollapseRowGroupIfNecessary(&self, aBTotalOffset: nscoord, aISize: nscoord, aWM: WritingMode) -> nscoord {
        let tableFrame = self.GetTableFrame();
        let containerSize = tableFrame.GetSize();
        let groupVis = self.StyleVisibility();
        let collapseGroup = StyleVisibility::Collapse == groupVis.mVisible();
        if collapseGroup {
            tableFrame.SetNeedToCollapse(true);
        }

        let mut overflow = OverflowAreas::default();

        let mut rowFrame = self.GetFirstRow();
        let mut didCollapse = false;
        let mut bGroupOffset: nscoord = 0;
        while let Some(row) = rowFrame {
            bGroupOffset += row.CollapseRowIfNecessary(bGroupOffset, aISize, collapseGroup, &mut didCollapse);
            self.ConsiderChildOverflow(&mut overflow, row.0, OverflowAreaUnionFlags::None);
            rowFrame = row.GetNextRow();
        }

        let mut groupRect = self.GetLogicalRect__WritingMode_nsSize(aWM, containerSize);
        // nsRect oldGroupRect = GetRect();
        // nsRect oldGroupInkOverflow = InkOverflowRect();
        // NOT PORTED: invalidation (only used by InvalidateTableFrame at the end).

        *groupRect.BSize_mut(aWM) -= bGroupOffset;
        if didCollapse {
            // add back the cellspacing between rowgroups
            *groupRect.BSize_mut(aWM) += tableFrame.GetRowSpacing__int32_t(self.GetStartRowIndex() + self.GetRowCount());
        }

        *groupRect.BStart_mut(aWM) -= aBTotalOffset;
        *groupRect.ISize_mut(aWM) = aISize;

        if aBTotalOffset != 0 {
            // InvalidateFrameSubtree();
            // NOT PORTED: invalidation.
        }

        self.SetRect__WritingMode_LogicalRect_nsSize(aWM, groupRect, containerSize);
        overflow.UnionAllWith(nsRect::new(0, 0, groupRect.Width(aWM), groupRect.Height(aWM)));
        self.FinishAndStoreOverflow(&mut overflow, groupRect.Size(aWM).GetPhysicalSize(aWM));
        // nsTableFrame::InvalidateTableFrame(this, oldGroupRect, oldGroupInkOverflow, false);
        // NOT PORTED: invalidation.

        bGroupOffset
    }

    // Create a continuing table row frame, add it to the child list, and then
    // push it and its later siblings to our overflow frames list.
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::CreateContinuingRowFrame
    // NOT PORTED: pagination (continuing frames; no frame is ever split here).
    fn CreateContinuingRowFrame(&self, aRowFrame: nsIFrame) -> Option<nsTableRowFrame> {
        unimplemented!("NOT PORTED: pagination (nsTableRowGroupFrame::CreateContinuingRowFrame)")
    }

    // Reflow the cells with rowspan > 1 which originate between aFirstRow
    // and end on or after aLastRow. aFirstTruncatedRow is the highest row on the
    // page that contains a cell which cannot split on this page
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::SplitSpanningCells
    // NOT PORTED: pagination (nsTableRowGroupFrame.cpp lines 913-1003 at GECKO_REV).
    fn SplitSpanningCells(
        &self,
        aPresContext: &nsPresContext,
        aReflowInput: &ReflowInput,
        aTable: nsTableFrame,
        aFirstRow: nsTableRowFrame,
        aLastRow: nsTableRowFrame,
        aFirstRowIsTopOfPage: bool,
        aSpanningRowBEnd: nscoord,
        aContainerSize: nsSize,
        aContRow: &mut Option<nsTableRowFrame>,
        aFirstTruncatedRow: &mut Option<nsTableRowFrame>,
        aDesiredBSize: &mut nscoord,
    ) {
        unimplemented!("NOT PORTED: pagination (nsTableRowGroupFrame::SplitSpanningCells)")
    }

    // Remove the next-in-flow of the row, its cells and their cell blocks. This
    // is necessary in case the row doesn't need a continuation later on or needs
    // a continuation which doesn't have the same number of cells that now exist.
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::UndoContinuedRow
    // NOT PORTED: pagination (nsTableRowGroupFrame.cpp lines 1005-1037 at GECKO_REV).
    fn UndoContinuedRow(&self, aPresContext: &nsPresContext, aRow: Option<nsTableRowFrame>) {
        if aRow.is_none() {
            return; // allow null aRow to avoid callers doing null checks
        }
        unimplemented!("NOT PORTED: pagination (nsTableRowGroupFrame::UndoContinuedRow)")
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::SplitRowGroup
    // NOT PORTED: pagination (nsTableRowGroupFrame.cpp lines 1040-1314 at GECKO_REV). Reflow reaches it
    // only for a table that is splittable (mTableIsSplittable), which needs a paginated pres context.
    fn SplitRowGroup(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aTableFrame: nsTableFrame, aStatus: &mut nsReflowStatus, aRowForcedPageBreak: bool) {
        unimplemented!("NOT PORTED: pagination (nsTableRowGroupFrame::SplitRowGroup)")
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::Reflow
    #[allow(invalid_reference_casting)]
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        // DO_GLOBAL_REFLOW_COUNT("nsTableRowGroupFrame");  NOT PORTED: reflow counts.
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        // Row geometry may be going to change so we need to invalidate any row
        // cursor.
        self.ClearRowCursor();

        // see if a special bsize reflow needs to occur due to having a pct bsize
        nsTableFrame::CheckRequestSpecialBSizeReflow(aReflowInput);

        let tableFrame = self.GetTableFrame();
        let mut state = TableRowGroupReflowInput::new(aReflowInput);
        let groupVis = self.StyleVisibility();
        let collapseGroup = StyleVisibility::Collapse == groupVis.mVisible();
        if collapseGroup {
            tableFrame.SetNeedToCollapse(true);
        }

        // Check for an overflow list
        self.MoveOverflowToChildList();

        // Reflow the existing frames.
        let mut splitDueToPageBreak = false;
        self.ReflowChildren(aPresContext, aDesiredSize, &mut state, aStatus, Some(&mut splitDueToPageBreak));

        // See if all the frames fit. Do not try to split anything if we're
        // not paginated ... we can't split across columns yet.
        let wm = aReflowInput.GetWritingMode();
        if aReflowInput.mFlags.mTableIsSplittable && aReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE && (aStatus.IsIncomplete() || splitDueToPageBreak || aDesiredSize.BSize(wm) > aReflowInput.AvailableBSize()) {
            // Nope, find a place to split the row group
            // auto& mutableRIFlags = const_cast<ReflowInput::Flags&>(aReflowInput.mFlags);
            let mutableRIFlags: *mut ReflowInput_Flags = &aReflowInput.mFlags as *const ReflowInput_Flags as *mut ReflowInput_Flags;
            let savedSpecialBSizeReflow = aReflowInput.mFlags.mSpecialBSizeReflow;
            unsafe { &mut *mutableRIFlags }.mSpecialBSizeReflow = false;

            self.SplitRowGroup(aPresContext, aDesiredSize, aReflowInput, tableFrame, aStatus, splitDueToPageBreak);

            unsafe { &mut *mutableRIFlags }.mSpecialBSizeReflow = savedSpecialBSizeReflow;
        }

        // XXXmats The following is just bogus.  We leave it here for now because
        // ReflowChildren should pull up rows from our next-in-flow before returning
        // a Complete status, but doesn't (bug 804888).
        if let Some(nextInFlow) = self.GetNextInFlow() {
            if nextInFlow.mFrames().FirstChild().is_some() {
                aStatus.SetIncomplete();
            }
        }

        self.SetHasStyleBSize((NS_UNCONSTRAINEDSIZE != aReflowInput.ComputedBSize()) && (aReflowInput.ComputedBSize() > 0));

        // Just set our isize to what was available.
        // The table will calculate the isize and not use our value.
        *aDesiredSize.ISize_mut(wm) = aReflowInput.AvailableISize();

        aDesiredSize.UnionOverflowAreasWithDesiredBounds();

        // If our parent is in initial reflow, it'll handle invalidating our
        // entire overflow rect.
        if !self.GetParent().unwrap().HasAnyStateBits(NS_FRAME_FIRST_REFLOW) && aDesiredSize.Size(wm) != self.GetLogicalSize__WritingMode(wm) {
            // InvalidateFrame();
            // NOT PORTED: invalidation.
        }

        self.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(aDesiredSize, None);

        // Any absolutely-positioned children will get reflowed in
        // nsIFrame::FixupPositionedTableParts in another pass, so propagate our
        // dirtiness to them before our parent clears our dirty bits.
        self.PushDirtyBitToAbsoluteFrames();
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::ComputeCustomOverflow
    pub fn ComputeCustomOverflow(&self, aOverflowAreas: &mut OverflowAreas) -> bool {
        // Row cursor invariants depend on the ink overflow area of the rows,
        // which may have changed, so we need to clear the cursor now.
        self.ClearRowCursor();
        // return nsContainerFrame::ComputeCustomOverflow(aOverflowAreas);
        // NOT PORTED: nsIFrame::ComputeCustomOverflow (always true for a container).
        true
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::DidSetComputedStyle
    // NOT PORTED: style changes (no restyling after the first style resolution) and border-collapse
    // damage areas (nsTableRowGroupFrame.cpp lines 1405-1421 at GECKO_REV).

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::AppendFrames
    pub fn AppendFrames(&self, aListID: FrameChildListID, aFrameList: nsFrameList) {
        NS_ASSERTION!(aListID == FrameChildListID::Principal, "unexpected child list");

        self.DrainSelfOverflowList(); // ensure the last frame is in mFrames
        self.ClearRowCursor();

        // collect the new row frames in an array
        // XXXbz why are we doing the QI stuff?  There shouldn't be any non-rows here.
        let mut rows: Vec<nsTableRowFrame> = Vec::new();
        for f in &aFrameList {
            let rowFrame = nsTableRowFrame::do_QueryFrame(f);
            NS_ASSERTION!(rowFrame.is_some(), "Unexpected frame; frame constructor screwed up");
            if let Some(rowFrame) = rowFrame {
                NS_ASSERTION!(StyleDisplay::TableRow == f.StyleDisplay().mDisplay(), "wrong display type on rowframe");
                rows.push(rowFrame);
            }
        }

        let rowIndex = self.GetRowCount();
        // Append the frames to the sibling chain
        self.mFrames().AppendFrames(None, aFrameList);

        if rows.len() > 0 {
            let tableFrame = self.GetTableFrame();
            tableFrame.AppendRows(*self, rowIndex, &mut rows);
            self.PresShell().FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
            tableFrame.SetGeometryDirty();
        }
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::InsertFrames
    pub fn InsertFrames(&self, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aPrevFrameLine: Option<&nsLineList_iterator>, aFrameList: nsFrameList) {
        NS_ASSERTION!(aListID == FrameChildListID::Principal, "unexpected child list");
        NS_ASSERTION!(aPrevFrame.map_or(true, |p| p.GetParent() == Some(self.0)), "inserting after sibling frame with different parent");

        self.DrainSelfOverflowList(); // ensure aPrevFrame is in mFrames
        self.ClearRowCursor();

        // collect the new row frames in an array
        // XXXbz why are we doing the QI stuff?  There shouldn't be any non-rows here.
        let tableFrame = self.GetTableFrame();
        let mut rows: Vec<nsTableRowFrame> = Vec::new();
        let mut gotFirstRow = false;
        for f in &aFrameList {
            let rowFrame = nsTableRowFrame::do_QueryFrame(f);
            NS_ASSERTION!(rowFrame.is_some(), "Unexpected frame; frame constructor screwed up");
            if let Some(rowFrame) = rowFrame {
                NS_ASSERTION!(StyleDisplay::TableRow == f.StyleDisplay().mDisplay(), "wrong display type on rowframe");
                rows.push(rowFrame);
                if !gotFirstRow {
                    rowFrame.SetFirstInserted(true);
                    gotFirstRow = true;
                    tableFrame.SetRowInserted(true);
                }
            }
        }

        let startRowIndex = self.GetStartRowIndex();
        // Insert the frames in the sibling chain
        self.mFrames().InsertFrames(None, aPrevFrame, aFrameList);

        let numRows = rows.len() as i32;
        if numRows > 0 {
            let prevRow = nsTableFrame::GetFrameAtOrBefore(self.0, aPrevFrame, LayoutFrameType::TableRow).map(nsTableRowFrame);
            let rowIndex = match prevRow {
                Some(prevRow) => prevRow.GetRowIndex() + 1,
                None => startRowIndex,
            };
            tableFrame.InsertRows(*self, &mut rows, rowIndex, true);

            self.PresShell().FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
            tableFrame.SetGeometryDirty();
        }
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::RemoveFrame
    pub fn RemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        NS_ASSERTION!(aListID == FrameChildListID::Principal, "unexpected child list");

        self.ClearRowCursor();

        // XXX why are we doing the QI stuff?  There shouldn't be any non-rows here.
        let rowFrame = nsTableRowFrame::do_QueryFrame(aOldFrame);
        if let Some(rowFrame) = rowFrame {
            let tableFrame = self.GetTableFrame();
            // remove the rows from the table (and flag a rebalance)
            tableFrame.RemoveRows(rowFrame, 1, true);

            self.PresShell().FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
            tableFrame.SetGeometryDirty();
        }
        self.mFrames().DestroyFrame(aContext, aOldFrame);
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetUsedMargin
    pub fn GetUsedMargin(&self) -> nsMargin {
        nsMargin::new(0, 0, 0, 0)
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetUsedBorder
    pub fn GetUsedBorder(&self) -> nsMargin {
        nsMargin::new(0, 0, 0, 0)
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetUsedPadding
    pub fn GetUsedPadding(&self) -> nsMargin {
        nsMargin::new(0, 0, 0, 0)
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetBSizeBasis
    pub fn GetBSizeBasis(&self, aReflowInput: &ReflowInput) -> nscoord {
        let mut result: nscoord = 0;
        let tableFrame = self.GetTableFrame();
        let startRowIndex = self.GetStartRowIndex();
        if (aReflowInput.ComputedBSize() > 0) && (aReflowInput.ComputedBSize() < NS_UNCONSTRAINEDSIZE) {
            let cellSpacing = tableFrame.GetRowSpacing__int32_t_int32_t(startRowIndex, std::cmp::max(startRowIndex, startRowIndex + self.GetRowCount() - 1));
            result = aReflowInput.ComputedBSize() - cellSpacing;
        } else {
            let mut parentRI = aReflowInput.mParentReflowInput;
            if let Some(p) = parentRI {
                if tableFrame.0 != p.mFrame {
                    parentRI = p.mParentReflowInput;
                }
            }
            if let Some(p) = parentRI {
                if tableFrame.0 == p.mFrame && (p.ComputedBSize() > 0) && (p.ComputedBSize() < NS_UNCONSTRAINEDSIZE) {
                    let cellSpacing = tableFrame.GetRowSpacing__int32_t_int32_t(-1, tableFrame.GetRowCount());
                    result = p.ComputedBSize() - cellSpacing;
                }
            }
        }

        result
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::IsSimpleRowFrame
    fn IsSimpleRowFrame(&self, aTableFrame: nsTableFrame, aRowFrame: nsTableRowFrame) -> bool {
        let rowIndex = aRowFrame.GetRowIndex();

        // It's a simple row frame if there are no cells that span into or
        // across the row
        let numEffCols = aTableFrame.GetEffectiveColCount();
        if !aTableFrame.RowIsSpannedInto(rowIndex, numEffCols) && !aTableFrame.RowHasSpanningCells(rowIndex, numEffCols) {
            return true;
        }

        false
    }

    /** find page break before the first row **/
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::HasInternalBreakBefore
    // NOT PORTED: pagination (`firstChild->StyleDisplay()->BreakBefore()`, page breaks).
    pub fn HasInternalBreakBefore(&self) -> bool {
        unimplemented!("NOT PORTED: pagination (nsTableRowGroupFrame::HasInternalBreakBefore)")
    }

    /** find page break after the last row **/
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::HasInternalBreakAfter
    // NOT PORTED: pagination (`lastChild->StyleDisplay()->BreakAfter()`, page breaks).
    pub fn HasInternalBreakAfter(&self) -> bool {
        unimplemented!("NOT PORTED: pagination (nsTableRowGroupFrame::HasInternalBreakAfter)")
    }

    /* ----- global methods ----- */

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetFrameName (DEBUG_FRAME_DUMP)
    // NOT PORTED: frame dumps.

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetBCBorderWidth
    pub fn GetBCBorderWidth(&self, aWM: WritingMode) -> LogicalMargin {
        let mut border = LogicalMargin::new__WritingMode(aWM);
        let firstRowFrame = self.GetFirstRow();
        let Some(firstRowFrame) = firstRowFrame else {
            return border;
        };
        let mut lastRowFrame = firstRowFrame;
        let mut next = firstRowFrame.GetNextRow();
        while let Some(rowFrame) = next {
            lastRowFrame = rowFrame;
            next = rowFrame.GetNextRow();
        }
        *border.BStart_mut(aWM) = firstRowFrame.GetBStartBCBorderWidth();
        *border.BEnd_mut(aWM) = lastRowFrame.GetBEndBCBorderWidth();
        border
    }

    // nsILineIterator methods
    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::GetNumLines,
    // IsLineIteratorFlowRTL, GetLine, FindLineContaining, FindFrameAt, CheckLineOrder
    // NOT PORTED: nsILineIterator (selection, caret and accessibility; nsTableRowGroupFrame.cpp lines
    // 1635-1748 at GECKO_REV).

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::ClearRowCursor
    // NOT PORTED: the row cursor (it speeds up painting and hit testing; nothing sets it, so there
    // is nothing to clear). Reflow and ComputeCustomOverflow call it.
    pub fn ClearRowCursor(&self) {}

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::SetupRowCursor,
    // GetFirstRowContaining, FrameCursorData::AppendFrame
    // NOT PORTED: the row cursor.

    // Gecko: layout/tables/nsTableRowGroupFrame.cpp nsTableRowGroupFrame::InvalidateFrame,
    // InvalidateFrameWithRect
    // NOT PORTED: invalidation.

    // Gecko: layout/tables/nsTableRowGroupFrame.h nsTableRowGroupFrame::IsRepeatable
    pub fn IsRepeatable(&self) -> bool {
        self.HasAnyStateBits(NS_ROWGROUP_REPEATABLE)
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.h nsTableRowGroupFrame::SetRepeatable
    pub fn SetRepeatable(&self, aRepeatable: bool) {
        if aRepeatable {
            self.AddStateBits(NS_ROWGROUP_REPEATABLE);
        } else {
            self.RemoveStateBits(NS_ROWGROUP_REPEATABLE);
        }
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.h nsTableRowGroupFrame::HasStyleBSize
    pub fn HasStyleBSize(&self) -> bool {
        self.HasAnyStateBits(NS_ROWGROUP_HAS_STYLE_BSIZE)
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.h nsTableRowGroupFrame::SetHasStyleBSize
    pub fn SetHasStyleBSize(&self, aValue: bool) {
        if aValue {
            self.AddStateBits(NS_ROWGROUP_HAS_STYLE_BSIZE);
        } else {
            self.RemoveStateBits(NS_ROWGROUP_HAS_STYLE_BSIZE);
        }
    }

    // Gecko: layout/tables/nsTableRowGroupFrame.h nsTableRowGroupFrame::GetTableFrame
    pub fn GetTableFrame(&self) -> nsTableFrame {
        let parent = self.GetParent();
        MOZ_ASSERT!(parent.is_some() && parent.unwrap().IsTableFrame());
        nsTableFrame::from_frame(parent.unwrap())
    }
}

// Gecko: layout/tables/nsTableRowGroupFrame.cpp RowInfo
// (the C++ bit fields are plain fields here)
#[derive(Clone, Copy, Default)]
struct RowInfo {
    bSize: u32, // content bsize or fixed bsize, excluding pct bsize
    pctBSize: u32, // pct bsize (29 bits in C++)
    hasStyleBSize: bool,
    hasPctBSize: bool,
    isSpecial: bool, // there is no cell originating in the row with
                     // rowspan=1 and there are at least 2 cells spanning
                     // the row and there is no style bsize on the row
}

//////////////////////////////////////////////////////////////////////////////////////////
// TEMP stubs
//
// Everything below is NOT part of nsTableRowGroupFrame.h/.cpp: members of nsIFrame and friends that the
// row group calls and no other translated file has yet. When the real member lands in its own file,
// delete the item here (a duplicate definition fails to compile).
//////////////////////////////////////////////////////////////////////////////////////////

impl nsIFrame {
    // Gecko: layout/generic/nsIFrame.h nsIFrame::GetNormalRect
    pub fn GetNormalRect(&self) -> nsRect {
        nsRect::new(self.GetNormalPosition().x, self.GetNormalPosition().y, self.GetSize().width, self.GetSize().height)
    }
}

// Tests: written from the documented behaviour (CSS 2.2 17.5.3 table height algorithm, 17.5.5
// collapsing rows) and the comments of nsTableRowGroupFrame.cpp, see nsTableWrapperFrame_test_support.
#[cfg(test)]
mod tests {
    use crate::nsTableWrapperFrame_test_support::*;
    use crate::BlockReflowState_test_support::*;
    use crate::*;

    fn row_heights(t: &TTable) -> Vec<i32> {
        t.rows.iter().map(|r| r.GetSize().height / PX).collect()
    }

    #[test]
    fn the_row_group_is_as_tall_as_its_rows_and_as_wide_as_the_table() {
        // CSS 2.2 17.5: the row group box contains its rows, stacked one under the other.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10), tcell(30, 10)], vec![tcell(20, 30), tcell(30, 5)]]);
        layout_root(&t, 500);
        assert_eq!(t.group.GetPosition(), nsPoint::new(0, 0));
        assert_eq!(t.group.GetSize(), nsSize::new(50 * PX, 40 * PX));
        assert_eq!(t.rows[1].GetPosition().y, 10 * PX);
    }

    #[test]
    fn the_row_group_knows_its_rows() {
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10)], vec![tcell(20, 10)], vec![tcell(20, 10)]]);
        assert_eq!(t.group.GetRowCount(), 3);
        assert_eq!(t.group.GetStartRowIndex(), 0);
        assert_eq!(t.group.GetFirstRow(), Some(t.rows[0]));
        assert_eq!(t.group.GetLastRow(), Some(t.rows[2]));
        assert_eq!(t.group.GetTableFrame(), t.table);
        assert!(nsTableRowGroupFrame::do_QueryFrame(t.group.0).is_some());
        assert!(nsTableRowGroupFrame::do_QueryFrame(t.rows[0].0).is_none());
        assert!(!t.group.IsRepeatable());
        assert!(!t.group.HasStyleBSize());
    }

    #[test]
    fn adjusting_row_indices_shifts_the_rows_from_the_index_on() {
        // nsTableRowGroupFrame::AdjustRowIndices: the rows whose index is at least aRowIndex move by
        // the adjustment.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10)], vec![tcell(20, 10)], vec![tcell(20, 10)]]);
        t.group.AdjustRowIndices(1, 3);
        let indices: Vec<i32> = t.rows.iter().map(|r| r.GetRowIndex()).collect();
        assert_eq!(indices, vec![0, 4, 5]);
    }

    #[test]
    fn marked_rows_are_skipped_in_the_row_indices_of_the_rows_after_them() {
        // nsTableRowGroupFrame::MarkRowsAsDeleted records the stored index of the rows, and the
        // rows after them read an index that does not count the deleted ones.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10)], vec![tcell(20, 10)], vec![tcell(20, 10)]]);
        t.group.MarkRowsAsDeleted(t.rows[1], 1);
        assert_eq!(t.rows[0].GetRowIndex(), 0);
        assert_eq!(t.rows[2].GetRowIndex(), 1);
    }

    #[test]
    fn the_row_group_has_no_margin_border_or_padding() {
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).group_css("margin:5px;border:3px solid;padding:2px")]]);
        layout_root(&t, 500);
        let zero = nsMargin::new(0, 0, 0, 0);
        assert_eq!(t.group.GetUsedMargin(), zero);
        assert_eq!(t.group.GetUsedBorder(), zero);
        assert_eq!(t.group.GetUsedPadding(), zero);
        assert_eq!(t.group.GetPosition(), nsPoint::new(0, 0));
        assert_eq!(t.group.GetSize(), nsSize::new(20 * PX, 10 * PX));
    }

    #[test]
    fn a_row_group_height_is_shared_by_its_rows_in_proportion_to_their_heights() {
        // CSS 2.2 17.5.3 leaves the distribution to the user agent; CalculateRowBSizes gives each
        // row a share of the extra height proportional to its own height (the last row gets the
        // rounded share, the others the remainder).
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).group_css("height:100px")], vec![tcell(20, 30)]]);
        layout_root(&t, 500);
        assert_eq!(row_heights(&t), vec![25, 75]);
        assert_eq!(t.group.GetSize().height, 100 * PX);
        assert!(t.group.HasStyleBSize());
    }

    #[test]
    fn rows_with_a_style_height_do_not_take_part_of_the_extra_height_of_the_group() {
        // CalculateRowBSizes: "if we have a style bsize, allocate the extra bsize to unconstrained
        // rows": a row with a height of its own keeps it while the others share the extra.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).group_css("height:100px").row_css("height:20px")], vec![tcell(20, 10)]]);
        layout_root(&t, 500);
        assert_eq!(row_heights(&t), vec![20, 80]);
    }

    #[test]
    fn a_table_height_is_shared_by_the_rows() {
        // The same distribution when the table has the height (nsTableFrame hands the extra height
        // to the row groups, which keep the proportions).
        let t = build_table(500, "", "", "height:100px", None, &[vec![tcell(20, 10)], vec![tcell(20, 30)]]);
        layout_root(&t, 500);
        assert_eq!(row_heights(&t), vec![25, 75]);
        assert_eq!(t.table.GetSize().height, 100 * PX);
    }

    #[test]
    fn a_collapsed_row_group_has_no_height() {
        // CSS 2.2 17.5.5: 'visibility: collapse' on a row group removes all its rows.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).group_css("visibility:collapse")], vec![tcell(20, 30)]]);
        let m = layout_root(&t, 500);
        assert_eq!(t.group.GetSize().height, 0);
        assert_eq!(m.BSize(t.root.GetWritingMode()), 0);
    }

    #[test]
    fn the_block_size_basis_of_the_rows_is_the_computed_height_of_the_group_or_the_table() {
        // nsTableRowGroupFrame::GetBSizeBasis: the height that percentage row heights are of: the
        // computed height of the group when it has one, otherwise the table's.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10)]]);
        let wm = t.group.GetWritingMode();
        let ctx = nsPresContext::Current();
        let mut tableRI = root_input(t.table.0, 500, AUTO);
        tableRI.SetComputedBSize(200 * PX, ReflowInput_ResetResizeFlags::Yes);
        let mut flags = ReflowInput_InitFlags::new();
        flags += ReflowInput_InitFlag::CallerWillInit;
        let mut groupRI = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            &ctx,
            &tableRI,
            t.group.0,
            LogicalSize::new(wm, 500 * PX, AUTO),
            None,
            flags,
            StyleSizeOverrides::default(),
            ComputeSizeFlags::default(),
            std::ptr::null_mut(),
        );
        groupRI.Init(&ctx, None, None, None);
        // no height of its own: the table's
        assert_eq!(t.group.GetBSizeBasis(&groupRI), 200 * PX);
        // a height of its own wins
        groupRI.SetComputedBSize(100 * PX, ReflowInput_ResetResizeFlags::Yes);
        assert_eq!(t.group.GetBSizeBasis(&groupRI), 100 * PX);
    }
}
