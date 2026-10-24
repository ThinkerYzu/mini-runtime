// Translated from layout/tables/nsTableRowFrame.h and nsTableRowFrame.cpp (GECKO_REV), stage 2b.
//
// Rust shape (see CONVENTIONS.md and the top of nsTableCellFrame.rs): `nsTableRowFrame` is a `Copy`
// handle over a frame of type TableRow that derefs to nsIFrame (the C++ class derives from
// nsContainerFrame). `nsTableRowFrame*` that may be null is `Option<nsTableRowFrame>`. The data
// members live in one heap block per frame behind a frame property (nsTableRowFrame_Members). What is
// virtual in C++ (`Reflow`, `GetUsedMargin`, `GetUsedBorder`, `GetUsedPadding`, `GetLogicalSkipSides`)
// is a `match` on the frame type in nsIFrame.rs / nsLineLayoutAdapter.rs; this file has the
// nsTableRowFrame overrides under their own names.
//
// `TableCellReflowInput` (nsTableRowFrame.cpp, in namespace mozilla) is a struct holding a ReflowInput
// that derefs to it, as C++ inheritance is translated.
//
// NOT PORTED: collapsed borders (`borderCollapse` is never true, the BC border widths are kept as
// members but nothing sets them), painting (BuildDisplayList, PaintCellBackgroundsForFrame),
// invalidation (InvalidateFrame*, InvalidateTableFrame calls), accessibility, style-change handling
// (DidSetComputedStyle), pagination (ReflowCellFrame, the unpaginated block-size code is kept but
// only runs when the pres context is paginated), vertical writing modes where the code is not
// mechanical (kept as written, untested). Each is marked where it occurs. Refcount logging
// (MOZ_COUNT_CTOR) is not ported.
use crate::nsLineLayoutAdapter::*;
use crate::*;
use std::ops::{Deref, DerefMut};

// Yes if table-cells should use 'vertical-align:top' in
// nsTableCellFrame::BlockDirAlignChild(). This is a hack to workaround our
// current table row group fragmentation to avoid data loss.
// Gecko: layout/tables/nsTableRowFrame.h mozilla::ForceAlignTopForTableCell
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForceAlignTopForTableCell {
    No,
    Yes,
}

// Gecko: layout/tables/nsTableRowFrame.cpp mozilla::TableCellReflowInput
// (`struct TableCellReflowInput : public ReflowInput`)
pub struct TableCellReflowInput<'a> {
    mReflowInput: ReflowInput<'a>,
}

impl<'a> Deref for TableCellReflowInput<'a> {
    type Target = ReflowInput<'a>;
    fn deref(&self) -> &ReflowInput<'a> {
        &self.mReflowInput
    }
}

impl<'a> DerefMut for TableCellReflowInput<'a> {
    fn deref_mut(&mut self) -> &mut ReflowInput<'a> {
        &mut self.mReflowInput
    }
}

impl<'a> TableCellReflowInput<'a> {
    // Gecko: layout/tables/nsTableRowFrame.cpp mozilla::TableCellReflowInput::TableCellReflowInput
    // (aFlags defaults to {} in C++)
    pub fn new(aPresContext: &nsPresContext, aParentReflowInput: &'a ReflowInput<'a>, aFrame: nsIFrame, aAvailableSpace: LogicalSize, aFlags: ReflowInput_InitFlags) -> TableCellReflowInput<'a> {
        TableCellReflowInput {
            mReflowInput: ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                aPresContext,
                aParentReflowInput,
                aFrame,
                aAvailableSpace,
                None,
                aFlags,
                StyleSizeOverrides::default(),
                ComputeSizeFlags::default(),
                std::ptr::null_mut(),
            ),
        }
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp mozilla::TableCellReflowInput::FixUp
    pub fn FixUp(&mut self, aAvailSpace: LogicalSize) {
        // fix the mComputed values during a pass 2 reflow since the cell can be a
        // percentage base
        let mWritingMode = self.GetWritingMode();
        NS_WARNING_ASSERTION!(NS_UNCONSTRAINEDSIZE != aAvailSpace.ISize(mWritingMode), "have unconstrained inline-size; this should only result from very large sizes, not attempts at intrinsic inline size calculation");
        if NS_UNCONSTRAINEDSIZE != self.ComputedISize() {
            let mut computedISize = aAvailSpace.ISize(mWritingMode) - self.ComputedLogicalBorderPadding(mWritingMode).IStartEnd(mWritingMode);
            computedISize = std::cmp::max(0, computedISize);
            self.SetComputedISize(computedISize, ReflowInput_ResetResizeFlags::Yes);
        }
        if NS_UNCONSTRAINEDSIZE != self.ComputedBSize() && NS_UNCONSTRAINEDSIZE != aAvailSpace.BSize(mWritingMode) {
            let mut computedBSize = aAvailSpace.BSize(mWritingMode) - self.ComputedLogicalBorderPadding(mWritingMode).BStartEnd(mWritingMode);
            computedBSize = std::cmp::max(0, computedBSize);
            self.SetComputedBSize(computedBSize, ReflowInput_ResetResizeFlags::Yes);
        }
    }
}

// Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame
// ADAPTER: a handle (see the notes at the top of the file).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct nsTableRowFrame(pub nsIFrame);

impl Deref for nsTableRowFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::RowBits
// (the C++ bit fields are plain fields here)
#[derive(Clone, Copy, Default)]
pub struct nsTableRowFrame_RowBits {
    pub mRowIndex: u32, // 29 bits in C++
    pub mHasFixedBSize: bool, // set if the dominating style bsize on the row or any cell is pixel based
    pub mHasPctBSize: bool, // set if the dominating style bsize on the row or any cell is pct based
    pub mFirstInserted: bool, // if true, then it was the bstart-most newly inserted row
}

// ADAPTER: the members of the C++ class nsTableRowFrame that are plain data, in one heap block per
// frame reached through a frame property (see nsBlockFrame_Members for the scheme).
pub struct nsTableRowFrame_Members {
    pub mBits: nsTableRowFrame_RowBits,

    // the desired bsize based on the content of the tallest cell in the row
    pub mContentBSize: nscoord,
    // the bsize based on a style percentage bsize on either the row or any cell
    // if mHasPctBSize is set
    pub mStylePctBSize: nscoord,
    // the bsize based on a style pixel bsize on the row or any
    // cell if mHasFixedBSize is set
    pub mStyleFixedBSize: nscoord,

    // max-ascent and max-descent amongst all cells that have
    // 'vertical-align: baseline'
    pub mMaxCellAscent: nscoord, // does include cells with rowspan > 1
    pub mMaxCellDescent: nscoord, // does *not* include cells with rowspan > 1

    // border widths in the collapsing border model of the *inner*
    // half of the border only
    pub mBStartBorderWidth: nscoord,
    pub mBEndBorderWidth: nscoord,
    pub mIEndContBorderWidth: nscoord,
    pub mBStartContBorderWidth: nscoord,
    pub mIStartContBorderWidth: nscoord,
}

// (ids: this crate's own numbers, 7000..7019 belong to the table cell, row and row group frames.)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(RowMembersProperty, usize, 7011);

// This property is only set on the first-in-flow of nsTableRowFrame.
// Gecko: layout/tables/nsTableRowFrame.cpp TableRowUnpaginatedBSizeProperty
// (NS_DECLARE_FRAME_PROPERTY_SMALL_VALUE(TableRowUnpaginatedBSizeProperty, nscoord))
fn TableRowUnpaginatedBSizeProperty() -> FramePropertyDescriptor<nscoord> {
    FramePropertyDescriptor::new(7002)
}

// Gecko: layout/tables/nsTableRowFrame.cpp NS_NewTableRowFrame
// ADAPTER: frames are made by FrameTree::AddFrame; this gives one the class and what the C++
// constructor sets (`mBits` zero, ResetBSize()).
pub fn NS_NewTableRowFrame(aFrame: nsIFrame) -> nsTableRowFrame {
    aFrame.SetType(LayoutFrameType::TableRow);
    let row = nsTableRowFrame(aFrame);
    row.Construct();
    row
}

impl nsTableRowFrame {
    // static_cast<nsTableRowFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsTableRowFrame {
        nsTableRowFrame(aFrame)
    }

    // do_QueryFrame<nsTableRowFrame>(frame)
    // Gecko: layout/tables/nsTableRowFrame.cpp NS_QUERYFRAME_HEAD(nsTableRowFrame)
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsTableRowFrame> {
        if aFrame.Type() == LayoutFrameType::TableRow {
            Some(nsTableRowFrame(aFrame))
        } else {
            None
        }
    }

    // ADAPTER: this frame's data members (see nsTableRowFrame_Members). Created on first use.
    fn Members(&self) -> &'static mut nsTableRowFrame_Members {
        let p = match self.0.GetProperty(RowMembersProperty()) {
            Some(p) => p,
            None => {
                let p = Box::into_raw(Box::new(nsTableRowFrame_Members {
                    mBits: nsTableRowFrame_RowBits::default(),
                    mContentBSize: 0,
                    mStylePctBSize: 0,
                    mStyleFixedBSize: 0,
                    mMaxCellAscent: 0,
                    mMaxCellDescent: 0,
                    mBStartBorderWidth: 0,
                    mBEndBorderWidth: 0,
                    mIEndContBorderWidth: 0,
                    mBStartContBorderWidth: 0,
                    mIStartContBorderWidth: 0,
                })) as usize;
                self.0.SetOwnedPointerProperty::<nsTableRowFrame_Members>(RowMembersProperty(), p);
                p
            }
        };
        unsafe { &mut *(p as *mut nsTableRowFrame_Members) }
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::nsTableRowFrame
    fn Construct(&self) {
        let members = self.Members();
        members.mBits.mRowIndex = 0;
        members.mBits.mHasFixedBSize = false;
        members.mBits.mHasPctBSize = false;
        members.mBits.mFirstInserted = false;
        self.ResetBSize();
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::GetTableRowGroupFrame
    pub fn GetTableRowGroupFrame(&self) -> nsTableRowGroupFrame {
        let parent = self.GetParent();
        MOZ_ASSERT!(parent.is_some() && parent.unwrap().IsTableRowGroupFrame());
        nsTableRowGroupFrame::from_frame(parent.unwrap())
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::GetTableFrame
    pub fn GetTableFrame(&self) -> nsTableFrame {
        self.GetTableRowGroupFrame().GetTableFrame()
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::SetFixedBSize
    pub fn SetFixedBSize(&self, aValue: nscoord) {
        let bsize = std::cmp::max(0, aValue);
        if self.HasFixedBSize() {
            if bsize > self.Members().mStyleFixedBSize {
                self.Members().mStyleFixedBSize = bsize;
            }
        } else {
            self.Members().mStyleFixedBSize = bsize;
            if bsize > 0 {
                self.SetHasFixedBSize(true);
            }
        }
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::SetPctBSize
    // (aForce defaults to false in C++)
    pub fn SetPctBSize(&self, aPctValue: f32, aForce: bool) {
        let bsize = std::cmp::max(0, NSToCoordRound__float(aPctValue * 100.0f32));
        if self.HasPctBSize() {
            if (bsize > self.Members().mStylePctBSize) || aForce {
                self.Members().mStylePctBSize = bsize;
            }
        } else {
            self.Members().mStylePctBSize = bsize;
            if bsize > 0 {
                self.SetHasPctBSize(true);
            }
        }
    }

    /* ----------- nsTableRowFrame ---------- */

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::Init
    pub fn Init(&self, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: Option<nsIFrame>) {
        // Let the base class do its initialization
        self.nsContainerFrame_Init(aContent, aParent, aPrevInFlow);

        NS_ASSERTION!(StyleDisplay::TableRow == self.StyleDisplay().mDisplay(), "wrong display on table row frame");

        if let Some(prevInFlow) = aPrevInFlow {
            // Set the row index
            let rowFrame = nsTableRowFrame(prevInFlow);

            self.SetRowIndex(rowFrame.GetRowIndex());
        } else {
            self.SetWritingMode(self.GetTableFrame().GetWritingMode());
        }
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::Destroy
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        nsTableFrame::MaybeUnregisterPositionedTablePart(self.0);
        self.nsContainerFrame_Destroy(aContext);
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::DidSetComputedStyle
    // NOT PORTED: style changes (no restyling after the first style resolution), accessibility
    // notifications and border-collapse damage areas (nsTableRowFrame.cpp lines 148-185 at GECKO_REV).

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::AppendFrames
    pub fn AppendFrames(&self, aListID: FrameChildListID, aFrameList: nsFrameList) {
        NS_ASSERTION!(aListID == FrameChildListID::Principal, "unexpected child list");

        self.DrainSelfOverflowList(); // ensure the last frame is in mFrames
        let newCells = self.mFrames().AppendFrames(None, aFrameList);

        // Add the new cell frames to the table
        let tableFrame = self.GetTableFrame();
        for childFrame in newCells {
            NS_ASSERTION!(childFrame.IsTableCellFrame(), "Not a table cell frame/pseudo frame construction failure");
            tableFrame.AppendCell(nsTableCellFrame(childFrame), self.GetRowIndex());
        }

        self.PresShell().FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
        tableFrame.SetGeometryDirty();
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::InsertFrames
    pub fn InsertFrames(&self, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aPrevFrameLine: Option<&nsLineList_iterator>, aFrameList: nsFrameList) {
        NS_ASSERTION!(aListID == FrameChildListID::Principal, "unexpected child list");
        NS_ASSERTION!(aPrevFrame.map_or(true, |p| p.GetParent() == Some(self.0)), "inserting after sibling frame with different parent");
        if self.mFrames().IsEmpty() || aPrevFrame.map_or(false, |p| p.GetNextSibling().is_none()) {
            // This is actually an append (though our caller didn't figure that out),
            // and our append codepath is both simpler/faster _and_ less buggy.
            // https://bugzilla.mozilla.org/show_bug.cgi?id=1388898 tracks the bugginess
            self.AppendFrames(aListID, aFrameList);
            return;
        }

        self.DrainSelfOverflowList(); // ensure aPrevFrame is in mFrames
        // Insert Frames in the frame list
        let newCells = self.mFrames().InsertFrames(None, aPrevFrame, aFrameList);

        let prevCellFrame = nsTableFrame::GetFrameAtOrBefore(self.0, aPrevFrame, LayoutFrameType::TableCell).map(nsTableCellFrame);
        let mut cellChildren: Vec<nsTableCellFrame> = Vec::new();
        for childFrame in newCells {
            NS_ASSERTION!(childFrame.IsTableCellFrame(), "Not a table cell frame/pseudo frame construction failure");
            cellChildren.push(nsTableCellFrame(childFrame));
        }
        // insert the cells into the cell map
        let mut colIndex: i32 = -1;
        if let Some(prevCellFrame) = prevCellFrame {
            colIndex = prevCellFrame.ColIndex() as i32;
        }
        let tableFrame = self.GetTableFrame();
        tableFrame.InsertCells(&mut cellChildren, self.GetRowIndex(), colIndex);

        self.PresShell().FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
        tableFrame.SetGeometryDirty();
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::RemoveFrame
    pub fn RemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        NS_ASSERTION!(aListID == FrameChildListID::Principal, "unexpected child list");
        MOZ_ASSERT!(nsTableCellFrame::do_QueryFrame(aOldFrame).is_some());

        let cellFrame = nsTableCellFrame(aOldFrame);
        // remove the cell from the cell map
        let tableFrame = self.GetTableFrame();
        tableFrame.RemoveCell(Some(cellFrame), self.GetRowIndex());

        // Remove the frame and destroy it
        self.mFrames().DestroyFrame(aContext, aOldFrame);

        self.PresShell().FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);

        tableFrame.SetGeometryDirty();
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::GetUsedMargin
    pub fn GetUsedMargin(&self) -> nsMargin {
        nsMargin::new(0, 0, 0, 0)
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::GetUsedBorder
    pub fn GetUsedBorder(&self) -> nsMargin {
        nsMargin::new(0, 0, 0, 0)
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::GetUsedPadding
    pub fn GetUsedPadding(&self) -> nsMargin {
        nsMargin::new(0, 0, 0, 0)
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp GetBSizeOfRowsSpannedBelowFirst
    fn GetBSizeOfRowsSpannedBelowFirst(aTableCellFrame: nsTableCellFrame, aTableFrame: nsTableFrame, aWM: WritingMode) -> nscoord {
        let mut bsize: nscoord = 0;
        let rowSpan = aTableFrame.GetEffectiveRowSpan__nsTableCellFrame_nsCellMap(&aTableCellFrame, None);
        // add in bsize of rows spanned beyond the 1st one
        let mut nextRow = aTableCellFrame.GetParent().unwrap().GetNextSibling();
        let mut rowX: i32 = 1;
        // for (int32_t rowX = 1; ((rowX < rowSpan) && nextRow);)
        while rowX < rowSpan && nextRow.is_some() {
            let next = nextRow.unwrap();
            if next.IsTableRowFrame() {
                bsize += next.BSize(aWM);
                rowX += 1;
            }
            bsize += aTableFrame.GetRowSpacing__int32_t(rowX);
            nextRow = next.GetNextSibling();
        }
        bsize
    }

    /**
     * Post-reflow hook. This is where the table row does its post-processing
     */
    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::DidResize
    // (aForceAlignTop defaults to ForceAlignTopForTableCell::No in C++)
    pub fn DidResize(&self, aForceAlignTop: ForceAlignTopForTableCell) {
        // Resize and re-align the cell frames based on our row bsize
        let tableFrame = self.GetTableFrame();

        let wm = self.GetWritingMode();
        let mut desiredSize = ReflowOutput::new__WritingMode(wm);
        desiredSize.SetSize(wm, &self.GetLogicalSize__WritingMode(wm));
        desiredSize.SetOverflowAreasToDesiredBounds();

        let containerSize = self.GetRect().Size();

        let mut cell = self.GetFirstCell();
        while let Some(cellFrame) = cell {
            let cellBSize = self.BSize(wm) + nsTableRowFrame::GetBSizeOfRowsSpannedBelowFirst(cellFrame, tableFrame, wm);

            // If the bsize for the cell has changed, we need to reset it;
            // and in vertical-rl mode, we need to update the cell's block position
            // to account for the containerSize, which may not have been known
            // earlier, so we always apply it here.
            let mut cellSize = cellFrame.GetLogicalSize__WritingMode(wm);
            if cellSize.BSize(wm) != cellBSize || wm.IsVerticalRL() {
                // nsRect cellOldRect = cellFrame->GetRect();
                // nsRect cellInkOverflow = cellFrame->InkOverflowRect();
                // NOT PORTED: invalidation (only used by InvalidateTableFrame below).

                if wm.IsVerticalRL() {
                    // Get the old position of the cell, as we want to preserve its
                    // inline coordinate.
                    let oldPos = cellFrame.GetLogicalPosition__WritingMode_nsSize(wm, containerSize);

                    // The cell should normally be aligned with the row's block-start,
                    // so set the B component of the position to zero:
                    let mut newPos = LogicalPoint::new(wm, oldPos.I(wm), 0);

                    // ...unless relative positioning is in effect, in which case the
                    // cell may have been moved away from the row's block-start
                    if cellFrame.IsRelativelyOrStickyPositioned() {
                        // Find out where the cell would have been without relative
                        // positioning.
                        let oldNormalPos = cellFrame.GetLogicalNormalPosition(wm, containerSize);
                        // The difference (if any) between oldPos and oldNormalPos reflects
                        // relative positioning that was applied to the cell, and which we
                        // need to incorporate when resetting the position.
                        *newPos.B_mut(wm) = oldPos.B(wm) - oldNormalPos.B(wm);
                    }

                    if oldPos != newPos {
                        cellFrame.SetPosition__WritingMode_LogicalPoint_nsSize(wm, newPos, containerSize);
                    }
                }

                *cellSize.BSize_mut(wm) = cellBSize;
                cellFrame.SetSize__WritingMode_LogicalSize(wm, cellSize);

                if tableFrame.IsBorderCollapse() {
                    // nsTableFrame::InvalidateTableFrame(cellFrame, cellOldRect, cellInkOverflow, false);
                    // NOT PORTED: invalidation.
                }
            }

            // realign cell content based on the new bsize.  We might be able to
            // skip this if the bsize didn't change... maybe.  Hard to tell.
            cellFrame.AlignChildWithinCell(self.Members().mMaxCellAscent, aForceAlignTop);

            // Always store the overflow, even if the height didn't change, since
            // we'll lose part of our overflow area otherwise.
            self.ConsiderChildOverflow(&mut desiredSize.mOverflowAreas, cellFrame.0, OverflowAreaUnionFlags::None);

            // Note that if the cell's *content* needs to change in response
            // to this height, it will get a special bsize reflow.
            cell = cellFrame.GetNextCell();
        }
        self.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(&mut desiredSize, None);
        // Let our base class do the usual work
    }

    // returns max-ascent amongst all cells that have 'vertical-align: baseline'
    // *including* cells with rowspans
    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::GetMaxCellAscent
    pub fn GetMaxCellAscent(&self) -> nscoord {
        self.Members().mMaxCellAscent
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::GetRowBaseline
    pub fn GetRowBaseline(&self, aWM: WritingMode) -> Option<nscoord> {
        if self.Members().mMaxCellAscent != 0 {
            return Some(self.Members().mMaxCellAscent);
        }

        // If we get here, we don't have a baseline on any of the cells in this row.
        let mut ascent: nscoord = 0;
        for childFrame in self.mFrames().begin() {
            MOZ_ASSERT!(childFrame.IsTableCellFrame());
            let s = Baseline::SynthesizeBOffsetFromContentBox(childFrame, aWM, BaselineSharingGroup::First);
            ascent = std::cmp::max(ascent, s);
        }
        Some(ascent)
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::GetInitialBSize
    // (aPctBasis defaults to 0 in C++)
    pub fn GetInitialBSize(&self, aPctBasis: nscoord) -> nscoord {
        let mut bsize: nscoord = 0;
        if (aPctBasis > 0) && self.HasPctBSize() {
            bsize = NSToCoordRound__float(self.GetPctBSize() * (aPctBasis as f32));
        }
        if self.HasFixedBSize() {
            bsize = std::cmp::max(bsize, self.GetFixedBSize());
        }
        std::cmp::max(bsize, self.GetContentBSize())
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::ResetBSize
    pub fn ResetBSize(&self) {
        self.SetHasFixedBSize(false);
        self.SetHasPctBSize(false);
        self.SetFixedBSize(0);
        self.SetPctBSize(0.0, false);
        self.SetContentBSize(0);

        self.Members().mMaxCellAscent = 0;
        self.Members().mMaxCellDescent = 0;
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::UpdateBSize
    pub fn UpdateBSize(&self, aBSize: nscoord, aTableFrame: Option<nsTableFrame>, aCellFrame: Option<nsTableCellFrame>) {
        let (Some(aTableFrame), Some(aCellFrame)) = (aTableFrame, aCellFrame) else {
            MOZ_ASSERT_UNREACHABLE!("Invalid call");
            return;
        };

        if aBSize == NS_UNCONSTRAINEDSIZE {
            return;
        }

        if self.GetInitialBSize(0) < aBSize && aTableFrame.GetEffectiveRowSpan__nsTableCellFrame_nsCellMap(&aCellFrame, None) == 1 {
            self.SetContentBSize(aBSize);
        }

        if aCellFrame.HasTableCellAlignmentBaseline() {
            if let Some(ascent) = aCellFrame.GetCellBaseline() {
                // see if this is a long ascender
                if self.Members().mMaxCellAscent < ascent {
                    self.Members().mMaxCellAscent = ascent;
                }
                let descent = aBSize - ascent;
                if self.Members().mMaxCellDescent < descent && aTableFrame.GetEffectiveRowSpan__nsTableCellFrame_nsCellMap(&aCellFrame, None) == 1 {
                    self.Members().mMaxCellDescent = descent;
                }
            }
        }
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::CalcBSize
    pub fn CalcBSize(&self, aReflowInput: &ReflowInput) -> nscoord {
        let tableFrame = self.GetTableFrame();

        self.ResetBSize();
        let computedBSize = aReflowInput.ComputedBSize();
        if computedBSize != NS_UNCONSTRAINEDSIZE && computedBSize > 0 {
            self.SetFixedBSize(computedBSize);
        }

        let wm = aReflowInput.GetWritingMode();
        let position = self.StylePosition();
        let bsizeStyleCoord = position.BSize(wm, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.0, std::ptr::null_mut()));
        if bsizeStyleCoord.ConvertsToLength() {
            self.SetFixedBSize(bsizeStyleCoord.ToLength());
        } else if bsizeStyleCoord.ConvertsToPercentage() {
            self.SetPctBSize(bsizeStyleCoord.ToPercentage(), false);
        }

        let mut kid = self.GetFirstCell();
        while let Some(kidFrame) = kid {
            MOZ_ASSERT!(kidFrame.GetWritingMode() == wm);
            let mut desSize = kidFrame.GetDesiredSize();
            if NS_UNCONSTRAINEDSIZE == aReflowInput.AvailableBSize() && self.GetPrevInFlow().is_none() {
                *desSize.BSize_mut(wm) = self.CalcCellActualBSize(kidFrame, &desSize.BSize(wm), wm);
            }
            self.UpdateBSize(desSize.BSize(wm), Some(tableFrame), Some(kidFrame));
            kid = kidFrame.GetNextCell();
        }
        self.GetInitialBSize(0)
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::PaintCellBackgroundsForFrame,
    // BuildDisplayList
    // NOT PORTED: painting.

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::GetLogicalSkipSides
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

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::CalcCellActualBSize
    pub fn CalcCellActualBSize(&self, aCellFrame: nsTableCellFrame, aDesiredBSize: &nscoord, aWM: WritingMode) -> nscoord {
        let mut specifiedBSize: nscoord = 0;

        // Get the bsize specified in the style information
        let position = aCellFrame.StylePosition();

        let rowSpan = self.GetTableFrame().GetEffectiveRowSpan__nsTableCellFrame_nsCellMap(&aCellFrame, None);

        let bsizeStyleCoord = position.BSize(aWM, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aCellFrame.0, std::ptr::null_mut()));
        if bsizeStyleCoord.ConvertsToLength() {
            // In quirks mode, table cell bsize should always be border-box.
            // https://quirks.spec.whatwg.org/#the-table-cell-height-box-sizing-quirk
            specifiedBSize = bsizeStyleCoord.ToLength();
            if self.PresContext().CompatibilityMode() != eCompatibility_NavQuirks && position.mBoxSizing() == StyleBoxSizing::ContentBox {
                specifiedBSize += aCellFrame.GetLogicalUsedBorderAndPadding(aWM).BStartEnd(aWM);
            }

            if 1 == rowSpan {
                self.SetFixedBSize(specifiedBSize);
            }
        } else if bsizeStyleCoord.ConvertsToPercentage() {
            if 1 == rowSpan {
                self.SetPctBSize(bsizeStyleCoord.ToPercentage(), false);
            }
        }

        // If the specified bsize is greater than the desired bsize,
        // then use the specified bsize
        std::cmp::max(specifiedBSize, *aDesiredBSize)
    }

    // Calculates the available isize for the table cell based on the known
    // column isizes taking into account column spans and column spacing
    // Gecko: layout/tables/nsTableRowFrame.cpp CalcAvailISize
    fn CalcAvailISize(aTableFrame: nsTableFrame, aCellFrame: nsTableCellFrame) -> nscoord {
        let mut cellAvailISize: nscoord = 0;
        let colIndex = aCellFrame.ColIndex() as i32;
        let colspan = aTableFrame.GetEffectiveColSpan(&aCellFrame, None);
        NS_ASSERTION!(colspan > 0, "effective colspan should be positive");
        let fifTable = nsTableFrame::from_frame(aTableFrame.FirstInFlow());

        for spanX in 0..colspan {
            cellAvailISize += fifTable.GetColumnISizeFromFirstInFlow(colIndex + spanX);
            if spanX > 0 && aTableFrame.ColumnHasCellSpacingBefore(colIndex + spanX) {
                cellAvailISize += aTableFrame.GetColSpacing__int32_t(colIndex + spanX - 1);
            }
        }
        cellAvailISize
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp GetSpaceBetween
    fn GetSpaceBetween(aPrevColIndex: i32, aColIndex: i32, aColSpan: i32, aTableFrame: nsTableFrame, aCheckVisibility: bool) -> nscoord {
        let mut space: nscoord = 0;
        let fifTable = nsTableFrame::from_frame(aTableFrame.FirstInFlow());
        let mut colIdx = aPrevColIndex + 1;
        while aColIndex > colIdx {
            let mut isCollapsed = false;
            if !aCheckVisibility {
                space += fifTable.GetColumnISizeFromFirstInFlow(colIdx);
            } else {
                let colFrame = aTableFrame.GetColFrame(colIdx).unwrap();
                let colVis = colFrame.StyleVisibility();
                let collapseCol = StyleVisibility::Collapse == colVis.mVisible();
                let cgFrame = colFrame.GetParent().unwrap();
                let groupVis = cgFrame.StyleVisibility();
                let collapseGroup = StyleVisibility::Collapse == groupVis.mVisible();
                isCollapsed = collapseCol || collapseGroup;
                if !isCollapsed {
                    space += fifTable.GetColumnISizeFromFirstInFlow(colIdx);
                }
            }
            if !isCollapsed && aTableFrame.ColumnHasCellSpacingBefore(colIdx) {
                space += aTableFrame.GetColSpacing__int32_t(colIdx - 1);
            }
            colIdx += 1;
        }
        space
    }

    // subtract the bsizes of aRow's prev in flows from the unpaginated bsize
    // Gecko: layout/tables/nsTableRowFrame.cpp CalcBSizeFromUnpaginatedBSize
    fn CalcBSizeFromUnpaginatedBSize(aRow: nsTableRowFrame, aWM: WritingMode) -> nscoord {
        let mut bsize: nscoord = 0;
        let firstInFlow = nsTableRowFrame(aRow.FirstInFlow());
        if firstInFlow.HasUnpaginatedBSize() {
            bsize = firstInFlow.GetUnpaginatedBSize();
            let mut prevInFlow = aRow.GetPrevInFlow();
            while let Some(p) = prevInFlow {
                bsize -= p.BSize(aWM);
                prevInFlow = p.GetPrevInFlow();
            }
        }
        std::cmp::max(bsize, 0)
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::InitChildReflowInput
    fn InitChildReflowInput(&self, aPresContext: &nsPresContext, aAvailSize: LogicalSize, aBorderCollapse: bool, aReflowInput: &mut TableCellReflowInput) {
        let mut collapseBorder: Option<LogicalMargin> = None;
        if aBorderCollapse {
            // we only reflow cells, so don't need to check frame type
            // nsBCTableCellFrame* bcCellFrame = (nsBCTableCellFrame*)aReflowInput.mFrame;
            // if (bcCellFrame) { collapseBorder.emplace(bcCellFrame->GetBorderWidth(aReflowInput.GetWritingMode())); }
            // NOT PORTED: collapsed borders (nsBCTableCellFrame).
            collapseBorder = None;
        }
        aReflowInput.Init(aPresContext, None, collapseBorder, None);
        aReflowInput.FixUp(aAvailSize);
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::ReflowChildren
    fn ReflowChildren(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aTableFrame: nsTableFrame, aStatus: &mut nsReflowStatus) {
        aStatus.Reset();

        // XXXldb Should we be checking constrained bsize instead?
        let isPaginated = aPresContext.IsPaginated();
        let borderCollapse = aTableFrame.IsBorderCollapse();

        let mut cellColSpan: i32 = 1; // must be defined here so it's set properly for non-cell kids

        // remember the col index of the previous cell to handle rowspans into this
        // row
        let mut prevColIndex: i32 = -1;
        let mut iCoord: nscoord = 0; // running total of children inline-coord offset

        // This computes the max of all cell bsizes
        let mut cellMaxBSize: nscoord = 0;

        // Reflow each of our existing cell frames
        let wm = aReflowInput.GetWritingMode();
        let containerSize = aReflowInput.ComputedSizeAsContainerIfConstrained();
        let mut hasOrthogonalCell = false;

        let mut kid = self.GetFirstCell();
        while let Some(kidFrame) = kid {
            kid = kidFrame.GetNextCell();

            // If we have any cells with orthogonal content, we'll need to handle them
            // later; record the presence of any such cells.
            if kidFrame.Inner().GetWritingMode().IsOrthogonalTo(&wm) {
                hasOrthogonalCell = true;
            }
            // See if we should only reflow the dirty child frames
            let mut doReflowChild = true;
            if !aReflowInput.ShouldReflowAllKids() && !aTableFrame.IsGeometryDirty() && !kidFrame.IsSubtreeDirty() {
                if !aReflowInput.mFlags.mSpecialBSizeReflow {
                    doReflowChild = false;
                }
            } else if NS_UNCONSTRAINEDSIZE != aReflowInput.AvailableBSize() {
                // We don't reflow a rowspan >1 cell here with a constrained bsize.
                // That happens in nsTableRowGroupFrame::SplitSpanningCells.
                if aTableFrame.GetEffectiveRowSpan__nsTableCellFrame_nsCellMap(&kidFrame, None) > 1 {
                    doReflowChild = false;
                }
            }
            if aReflowInput.mFlags.mSpecialBSizeReflow && !isPaginated && !kidFrame.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE) {
                continue;
            }

            let cellColIndex = kidFrame.ColIndex();
            cellColSpan = aTableFrame.GetEffectiveColSpan(&kidFrame, None);

            // If the adjacent cell is in a prior row (because of a rowspan) add in the
            // space NOTE: prevColIndex can be -1 here.
            if prevColIndex != (cellColIndex as i32 - 1) {
                iCoord += nsTableRowFrame::GetSpaceBetween(prevColIndex, cellColIndex as i32, cellColSpan, aTableFrame, false);
            }

            // remember the rightmost (ltr) or leftmost (rtl) column this cell spans
            // into
            prevColIndex = cellColIndex as i32 + (cellColSpan - 1);

            // Reflow the child frame
            // nsRect kidRect = kidFrame->GetRect();
            let origKidNormalPosition = kidFrame.GetLogicalNormalPosition(wm, containerSize);

            // nsRect kidInkOverflow = kidFrame->InkOverflowRect();
            let mut kidPosition = LogicalPoint::new(wm, iCoord, 0);
            // bool firstReflow = kidFrame->HasAnyStateBits(NS_FRAME_FIRST_REFLOW);
            // NOT PORTED: invalidation (kidRect, kidInkOverflow and firstReflow are only for
            // InvalidateTableFrame).

            if doReflowChild {
                // Calculate the available isize for the table cell using the known
                // column isizes
                let availCellISize = nsTableRowFrame::CalcAvailISize(aTableFrame, kidFrame);

                let mut kidReflowInput: Option<TableCellReflowInput> = None;
                let mut desiredSize = ReflowOutput::new__ReflowInput(aReflowInput);

                // If the avail isize is not the same as last time we reflowed the cell or
                // the cell wants to be bigger than what was available last time or
                // it is a style change reflow or we are printing, then we must reflow the
                // cell. Otherwise we can skip the reflow.
                // XXXldb Why is this condition distinct from doReflowChild above?
                NS_ASSERTION!(kidFrame.GetWritingMode() == wm, "expected consistent writing-mode within table");
                let cellDesiredSize = kidFrame.GetDesiredSize();
                if availCellISize != kidFrame.GetPriorAvailISize()
                    || cellDesiredSize.ISize(wm) > kidFrame.GetPriorAvailISize()
                    || self.HasAnyStateBits(NS_FRAME_IS_DIRTY)
                    || isPaginated
                    || kidFrame.IsSubtreeDirty()
                    // See if it needs a special reflow, or if it had one that we need to
                    // undo.
                    || kidFrame.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE)
                    || kidFrame.BCBordersChanged()
                    || self.HasPctBSize()
                {
                    // Reflow the cell to fit the available isize, bsize
                    // XXX The old IR_ChildIsDirty code used availCellISize here.
                    let kidAvailSize = LogicalSize::new(wm, availCellISize, aReflowInput.AvailableBSize());

                    // Reflow the child
                    let mut initFlags = ReflowInput_InitFlags::new();
                    initFlags += ReflowInput_InitFlag::CallerWillInit;
                    kidReflowInput = Some(TableCellReflowInput::new(aPresContext, aReflowInput, kidFrame.0, kidAvailSize, initFlags));
                    self.InitChildReflowInput(aPresContext, kidAvailSize, borderCollapse, kidReflowInput.as_mut().unwrap());

                    let mut status = nsReflowStatus::new();
                    self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
                        kidFrame.0,
                        aPresContext,
                        &mut desiredSize,
                        kidReflowInput.as_ref().unwrap(),
                        wm,
                        kidPosition,
                        containerSize,
                        nsIFrame_ReflowChildFlags::Default,
                        &mut status,
                        None,
                    );

                    // allow the table to determine if/how the table needs to be rebalanced
                    // If any of the cells are not complete, then we're not complete
                    if status.IsIncomplete() {
                        aStatus.Reset();
                        aStatus.SetIncomplete();
                    }
                } else {
                    if iCoord != origKidNormalPosition.I(wm) {
                        // kidFrame->InvalidateFrameSubtree();
                        // NOT PORTED: invalidation.
                    }

                    desiredSize.SetSize(wm, &cellDesiredSize);
                    desiredSize.mOverflowAreas = kidFrame.GetOverflowAreas();
                }

                if NS_UNCONSTRAINEDSIZE == aReflowInput.AvailableBSize() {
                    if self.GetPrevInFlow().is_none() {
                        *desiredSize.BSize_mut(wm) = self.CalcCellActualBSize(kidFrame, &desiredSize.BSize(wm), wm);
                    }
                    // bsize may have changed, adjust descent to absorb any excess
                    // difference
                    self.UpdateBSize(desiredSize.BSize(wm), Some(aTableFrame), Some(kidFrame));
                } else {
                    cellMaxBSize = std::cmp::max(cellMaxBSize, desiredSize.BSize(wm));
                    let rowSpan = aTableFrame.GetEffectiveRowSpan__nsTableCellFrame_nsCellMap(&kidFrame, None);
                    if 1 == rowSpan {
                        self.SetContentBSize(cellMaxBSize);
                    }
                }

                // Place the child
                *desiredSize.ISize_mut(wm) = availCellISize;

                let mut flags = nsIFrame_ReflowChildFlags::Default;

                if kidReflowInput.is_some() {
                    // We reflowed. Apply relative positioning in the normal way.
                    flags = nsIFrame_ReflowChildFlags::ApplyRelativePositioning;
                } else if kidFrame.IsRelativelyOrStickyPositioned() {
                    // We didn't reflow.  Do the positioning part of what
                    // MovePositionBy does internally.  (This codepath should really
                    // be merged into the else below if we can.)
                    let computedOffsetProp = kidFrame.GetProperty(nsIFrame::ComputedOffsetProperty());

                    // On our fist reflow sticky children may not have the property yet (we
                    // need to reflow the children first to size the scroll frame).
                    let computedOffsets = LogicalMargin::new__WritingMode_nsMargin(wm, computedOffsetProp.unwrap_or_default());
                    ReflowInput::ApplyRelativePositioning__nsIFrame_WritingMode_LogicalMargin_LogicalPoint_nsSize(kidFrame.0, wm, computedOffsets, &mut kidPosition, containerSize);
                }

                // In vertical-rl mode, we are likely to have containerSize.width = 0
                // because ComputedWidth() was NS_UNCONSTRAINEDSIZE.
                // For cases where that's wrong, we will fix up the position later.
                nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
                    kidFrame.0,
                    aPresContext,
                    &desiredSize,
                    kidReflowInput.as_ref().map(|r| &**r),
                    wm,
                    kidPosition,
                    containerSize,
                    flags,
                );

                let tableFrame = self.GetTableFrame();
                if tableFrame.IsBorderCollapse() {
                    // nsTableFrame::InvalidateTableFrame(kidFrame, kidRect, kidInkOverflow, firstReflow);
                    // NOT PORTED: invalidation.
                }

                iCoord += desiredSize.ISize(wm);
            } else {
                if iCoord != origKidNormalPosition.I(wm) {
                    // Invalidate the old position
                    // kidFrame->InvalidateFrameSubtree();
                    // NOT PORTED: invalidation.
                    // Move to the new position. As above, we need to account for relative
                    // positioning.
                    kidFrame.MovePositionBy__WritingMode_LogicalPoint(wm, LogicalPoint::new(wm, iCoord - origKidNormalPosition.I(wm), 0));
                    // invalidate the new position
                    // kidFrame->InvalidateFrameSubtree();
                    // NOT PORTED: invalidation.
                }
                // we need to account for the cell's isize even if it isn't reflowed
                iCoord += kidFrame.ISize(wm);

                if kidFrame.GetNextInFlow().is_some() {
                    aStatus.Reset();
                    aStatus.SetIncomplete();
                }
            }
            self.ConsiderChildOverflow(&mut aDesiredSize.mOverflowAreas, kidFrame.0, OverflowAreaUnionFlags::None);
            iCoord += aTableFrame.GetColSpacing__int32_t(cellColIndex as i32);
        }

        // Just set our isize to what was available.
        // The table will calculate the isize and not use our value.
        *aDesiredSize.ISize_mut(wm) = aReflowInput.AvailableISize();

        if aReflowInput.mFlags.mSpecialBSizeReflow {
            *aDesiredSize.BSize_mut(wm) = self.BSize(wm);
        } else if NS_UNCONSTRAINEDSIZE == aReflowInput.AvailableBSize() {
            *aDesiredSize.BSize_mut(wm) = self.CalcBSize(aReflowInput);
            if self.GetPrevInFlow().is_some() {
                let bsize = nsTableRowFrame::CalcBSizeFromUnpaginatedBSize(*self, wm);
                *aDesiredSize.BSize_mut(wm) = std::cmp::max(aDesiredSize.BSize(wm), bsize);
            } else {
                if isPaginated && self.HasStyleBSize() {
                    // set the unpaginated bsize so next in flows can try to honor it
                    self.SetUnpaginatedBSize(aDesiredSize.BSize(wm));
                }
                if isPaginated && self.HasUnpaginatedBSize() {
                    *aDesiredSize.BSize_mut(wm) = std::cmp::max(aDesiredSize.BSize(wm), self.GetUnpaginatedBSize());
                }
            }
        } else {
            // constrained bsize, paginated
            // Compute the bsize we should have from style (subtracting the
            // bsize from our prev-in-flows from the style bsize)
            let mut styleBSize = nsTableRowFrame::CalcBSizeFromUnpaginatedBSize(*self, wm);
            if styleBSize > aReflowInput.AvailableBSize() {
                styleBSize = aReflowInput.AvailableBSize();
                aStatus.SetIncomplete();
            }
            *aDesiredSize.BSize_mut(wm) = std::cmp::max(cellMaxBSize, styleBSize);
        }

        if wm.IsVerticalRL() {
            // Any children whose width was not the same as our final
            // aDesiredSize.BSize will have been misplaced earlier at the
            // FinishReflowChild stage. So fix them up now.
            for kidFrame in self.mFrames().begin() {
                if kidFrame.BSize(wm) != aDesiredSize.BSize(wm) {
                    kidFrame.MovePositionBy__WritingMode_LogicalPoint(wm, LogicalPoint::new(wm, 0, kidFrame.BSize(wm) - aDesiredSize.BSize(wm)));
                    // Do we need to InvalidateFrameSubtree() here?
                }
            }
        }

        aDesiredSize.UnionOverflowAreasWithDesiredBounds();
        self.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(aDesiredSize, None);

        if hasOrthogonalCell {
            let mut kid = self.GetFirstCell();
            while let Some(kidFrame) = kid {
                kid = kidFrame.GetNextCell();
                if kidFrame.Inner().GetWritingMode().IsOrthogonalTo(&wm) {
                    let mut kidAvailSize = LogicalSize::new__WritingMode_nsSize(wm, kidFrame.GetRectRelativeToSelf().Size());
                    *kidAvailSize.BSize_mut(wm) = aDesiredSize.BSize(wm);

                    // Reflow the child
                    let mut initFlags = ReflowInput_InitFlags::new();
                    initFlags += ReflowInput_InitFlag::CallerWillInit;
                    let mut kidReflowInput = TableCellReflowInput::new(aPresContext, aReflowInput, kidFrame.0, kidAvailSize, initFlags);
                    kidReflowInput.mFlags.mOrthogonalCellFinalReflow = true;
                    self.InitChildReflowInput(aPresContext, kidAvailSize, borderCollapse, &mut kidReflowInput);

                    let mut status = nsReflowStatus::new();
                    let mut reflowOutput = ReflowOutput::new__WritingMode(wm);
                    self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
                        kidFrame.0,
                        aPresContext,
                        &mut reflowOutput,
                        &kidReflowInput,
                        wm,
                        kidFrame.GetLogicalPosition__nsSize(containerSize),
                        containerSize,
                        nsIFrame_ReflowChildFlags::Default,
                        &mut status,
                        None,
                    );
                }
            }
        }
    }

    /** Layout the entire row.
     * This method stacks cells in the inline dir according to HTML 4.0 rules.
     */
    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        // DO_GLOBAL_REFLOW_COUNT("nsTableRowFrame");  NOT PORTED: reflow counts.
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");

        let wm = aReflowInput.GetWritingMode();

        let tableFrame = self.GetTableFrame();
        let rowVis = self.StyleVisibility();
        let collapseRow = StyleVisibility::Collapse == rowVis.mVisible();
        if collapseRow {
            tableFrame.SetNeedToCollapse(true);
        }

        // see if a special bsize reflow needs to occur due to having a pct bsize
        nsTableFrame::CheckRequestSpecialBSizeReflow(aReflowInput);

        // See if we have a cell with specified/pct bsize
        self.InitHasCellWithStyleBSize(tableFrame);

        self.ReflowChildren(aPresContext, aDesiredSize, aReflowInput, tableFrame, aStatus);

        if aPresContext.IsPaginated() && !aStatus.IsFullyComplete() && self.ShouldAvoidBreakInside(aReflowInput) {
            aStatus.SetInlineLineBreakBeforeAndReset();
        }

        // Just set our isize to what was available.
        // The table will calculate the isize and not use our value.
        *aDesiredSize.ISize_mut(wm) = aReflowInput.AvailableISize();

        // If our parent is in initial reflow, it'll handle invalidating our
        // entire overflow rect.
        if !self.GetParent().unwrap().HasAnyStateBits(NS_FRAME_FIRST_REFLOW) && nsSize::new(aDesiredSize.Width(), aDesiredSize.Height()) != self.GetRect().Size() {
            // InvalidateFrame();
            // NOT PORTED: invalidation.
        }

        // Any absolutely-positioned children will get reflowed in
        // nsIFrame::FixupPositionedTableParts in another pass, so propagate our
        // dirtiness to them before our parent clears our dirty bits.
        self.PushDirtyBitToAbsoluteFrames();
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::ReflowCellFrame
    // NOT PORTED: pagination. A cell is reflowed with a constrained block-size only when a row
    // group splits across pages (nsTableRowGroupFrame::SplitSpanningCells, the only caller, is not
    // ported either); the code is at nsTableRowFrame.cpp lines 957-1017 at GECKO_REV.
    pub fn ReflowCellFrame(&self, aPresContext: &nsPresContext, aReflowInput: &ReflowInput, aIsTopOfPage: bool, aCellFrame: nsTableCellFrame, aAvailableBSize: nscoord, aStatus: &mut nsReflowStatus) -> nscoord {
        unimplemented!("NOT PORTED: pagination (nsTableRowFrame::ReflowCellFrame)")
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::CollapseRowIfNecessary
    pub fn CollapseRowIfNecessary(&self, aRowOffset: nscoord, aISize: nscoord, aCollapseGroup: bool, aDidCollapse: &mut bool) -> nscoord {
        let rowVis = self.StyleVisibility();
        let collapseRow = StyleVisibility::Collapse == rowVis.mVisible();
        let tableFrame = nsTableFrame::from_frame(self.GetTableFrame().FirstInFlow());
        if collapseRow {
            tableFrame.SetNeedToCollapse(true);
        }

        if aRowOffset != 0 {
            // We're moving, so invalidate our old position
            // InvalidateFrameSubtree();
            // NOT PORTED: invalidation.
        }

        let wm = self.GetWritingMode();

        let parentSize = self.GetParent().unwrap().GetSize();
        let mut rowRect = self.GetLogicalRect__WritingMode_nsSize(wm, parentSize);
        // nsRect oldRect = mRect;
        // nsRect oldInkOverflow = InkOverflowRect();
        // NOT PORTED: invalidation (only used by InvalidateTableFrame at the end).

        *rowRect.BStart_mut(wm) -= aRowOffset;
        *rowRect.ISize_mut(wm) = aISize;
        let mut overflow = OverflowAreas::default();
        let mut shift: nscoord = 0;
        let containerSize = self.GetRect().Size();

        if aCollapseGroup || collapseRow {
            *aDidCollapse = true;
            shift = rowRect.BSize(wm);
            let mut cell = self.GetFirstCell();
            if let Some(firstCell) = cell {
                let rowIndex = firstCell.RowIndex() as i32;
                shift += tableFrame.GetRowSpacing__int32_t(rowIndex);
                while let Some(cellFrame) = cell {
                    let mut cRect = cellFrame.GetLogicalRect__WritingMode_nsSize(wm, containerSize);
                    // If aRowOffset != 0, there's no point in invalidating the cells, since
                    // we've already invalidated our overflow area.  Note that we _do_ still
                    // need to invalidate if our row is not moving, because the cell might
                    // span out of this row, so invalidating our row rect won't do enough.
                    if aRowOffset == 0 {
                        // InvalidateFrame();
                        // NOT PORTED: invalidation.
                    }
                    *cRect.BSize_mut(wm) = 0;
                    cellFrame.SetRect__WritingMode_LogicalRect_nsSize(wm, cRect, containerSize);
                    cell = cellFrame.GetNextCell();
                }
            } else {
                shift += tableFrame.GetRowSpacing__int32_t(self.GetRowIndex());
            }
            *rowRect.BSize_mut(wm) = 0;
        } else {
            // row is not collapsed
            // remember the col index of the previous cell to handle rowspans into this
            // row
            let mut prevColIndex: i32 = -1;
            let mut iPos: nscoord = 0; // running total of children inline-axis offset
            let fifTable = nsTableFrame::from_frame(tableFrame.FirstInFlow());

            let mut cell = self.GetFirstCell();
            while let Some(cellFrame) = cell {
                let cellColIndex = cellFrame.ColIndex();
                let cellColSpan = tableFrame.GetEffectiveColSpan(&cellFrame, None);

                // If the adjacent cell is in a prior row (because of a rowspan) add in
                // the space
                // NOTE: prevColIndex can be -1 here.
                if prevColIndex != (cellColIndex as i32 - 1) {
                    iPos += nsTableRowFrame::GetSpaceBetween(prevColIndex, cellColIndex as i32, cellColSpan, tableFrame, true);
                }
                let mut cRect = LogicalRect::new(wm, iPos, 0, 0, rowRect.BSize(wm));

                // remember the last (iend-wards-most) column this cell spans into
                prevColIndex = cellColIndex as i32 + cellColSpan - 1;
                let mut actualColSpan = cellColSpan;
                let mut isVisible = false;
                let mut colIdx = cellColIndex as i32;
                while actualColSpan > 0 {
                    let colFrame = tableFrame.GetColFrame(colIdx).unwrap();
                    let colVis = colFrame.StyleVisibility();
                    let collapseCol = StyleVisibility::Collapse == colVis.mVisible();
                    let cgFrame = colFrame.GetParent().unwrap();
                    let groupVis = cgFrame.StyleVisibility();
                    let collapseGroup = StyleVisibility::Collapse == groupVis.mVisible();
                    let isCollapsed = collapseCol || collapseGroup;
                    if !isCollapsed {
                        *cRect.ISize_mut(wm) += fifTable.GetColumnISizeFromFirstInFlow(colIdx);
                        isVisible = true;
                        if actualColSpan > 1 {
                            let nextColFrame = tableFrame.GetColFrame(colIdx + 1).unwrap();
                            let nextColVis = nextColFrame.StyleVisibility();
                            if StyleVisibility::Collapse != nextColVis.mVisible() && tableFrame.ColumnHasCellSpacingBefore(colIdx + 1) {
                                *cRect.ISize_mut(wm) += tableFrame.GetColSpacing__int32_t(cellColIndex as i32);
                            }
                        }
                    }
                    colIdx += 1;
                    actualColSpan -= 1;
                }
                iPos += cRect.ISize(wm);
                if isVisible {
                    iPos += tableFrame.GetColSpacing__int32_t(cellColIndex as i32);
                }
                let mut actualRowSpan = tableFrame.GetEffectiveRowSpan__nsTableCellFrame_nsCellMap(&cellFrame, None);
                let mut rowFrame = self.GetNextRow();
                actualRowSpan -= 1;
                while actualRowSpan > 0 && rowFrame.is_some() {
                    let row = rowFrame.unwrap();
                    let nextRowVis = row.StyleVisibility();
                    let collapseNextRow = StyleVisibility::Collapse == nextRowVis.mVisible();
                    if !collapseNextRow {
                        let nextRect = row.GetLogicalRect__WritingMode_nsSize(wm, containerSize);
                        *cRect.BSize_mut(wm) += nextRect.BSize(wm) + tableFrame.GetRowSpacing__int32_t(row.GetRowIndex());
                    }
                    rowFrame = row.GetNextRow();
                    actualRowSpan -= 1;
                }

                // nsRect oldCellRect = cellFrame->GetRect();
                let oldCellNormalPos = cellFrame.GetLogicalNormalPosition(wm, containerSize);

                // nsRect oldCellInkOverflow = cellFrame->InkOverflowRect();
                // NOT PORTED: invalidation (oldCellRect and oldCellInkOverflow are only for
                // InvalidateTableFrame).

                if aRowOffset == 0 && cRect.Origin(wm) != oldCellNormalPos {
                    // We're moving the cell.  Invalidate the old overflow area
                    // cellFrame->InvalidateFrameSubtree();
                    // NOT PORTED: invalidation.
                }

                cellFrame.MovePositionBy__WritingMode_LogicalPoint(wm, cRect.Origin(wm) - oldCellNormalPos);
                cellFrame.SetSize__WritingMode_LogicalSize(wm, cRect.Size(wm));

                // XXXbz This looks completely bogus in the cases when we didn't
                // collapse the cell!
                let cellBounds = LogicalRect::new(wm, 0, 0, cRect.ISize(wm), cRect.BSize(wm));
                let cellPhysicalBounds = cellBounds.GetPhysicalRect(wm, containerSize);
                let mut cellOverflow = OverflowAreas::new(cellPhysicalBounds, cellPhysicalBounds);
                cellFrame.FinishAndStoreOverflow(&mut cellOverflow, cRect.Size(wm).GetPhysicalSize(wm));
                self.ConsiderChildOverflow(&mut overflow, cellFrame.0, OverflowAreaUnionFlags::None);

                if aRowOffset == 0 {
                    // nsTableFrame::InvalidateTableFrame(cellFrame, oldCellRect, oldCellInkOverflow, false);
                    // NOT PORTED: invalidation.
                }
                cell = cellFrame.GetNextCell();
            }
        }

        self.SetRect__WritingMode_LogicalRect_nsSize(wm, rowRect, containerSize);
        overflow.UnionAllWith(nsRect::new(0, 0, rowRect.Width(wm), rowRect.Height(wm)));
        self.FinishAndStoreOverflow(&mut overflow, rowRect.Size(wm).GetPhysicalSize(wm));
        // nsTableFrame::InvalidateTableFrame(this, oldRect, oldInkOverflow, false);
        // NOT PORTED: invalidation.
        shift
    }

    /*
     * The following method is called by the row group frame's SplitRowGroup()
     * when it creates a continuing cell frame and wants to insert it into the
     * row's child list.
     */
    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::InsertCellFrame
    pub fn InsertCellFrame(&self, aFrame: nsTableCellFrame, aColIndex: i32) {
        // Find the cell frame where col index < aColIndex
        let mut priorCell: Option<nsTableCellFrame> = None;

        let mut cell = self.GetFirstCell();
        while let Some(cellFrame) = cell {
            let colIndex = cellFrame.ColIndex();
            // Can aColIndex be -1 here?  Let's assume it can for now.
            if (colIndex as i32) < aColIndex {
                priorCell = Some(cellFrame);
            } else {
                break;
            }
            cell = cellFrame.GetNextCell();
        }
        self.mFrames().InsertFrame(Some(self.0), priorCell.map(|c| c.0), aFrame.0);
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::GetPrevRow
    pub fn GetPrevRow(&self) -> Option<nsTableRowFrame> {
        let prevSibling = self.GetPrevSibling();
        MOZ_ASSERT!(prevSibling.map_or(true, |s| nsTableRowFrame::do_QueryFrame(s).is_some()), "How do we have a non-row sibling?");
        prevSibling.map(nsTableRowFrame)
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::GetNextRow
    pub fn GetNextRow(&self) -> Option<nsTableRowFrame> {
        let nextSibling = self.GetNextSibling();
        MOZ_ASSERT!(nextSibling.map_or(true, |s| nsTableRowFrame::do_QueryFrame(s).is_some()), "How do we have a non-row sibling?");
        nextSibling.map(nsTableRowFrame)
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::SetUnpaginatedBSize
    pub fn SetUnpaginatedBSize(&self, aValue: nscoord) {
        MOZ_ASSERT!(self.GetPrevInFlow().is_none(), "TableRowUnpaginatedBSizeProperty should only be set on the first-in-flow!");
        self.AddStateBits(NS_TABLE_ROW_HAS_UNPAGINATED_BSIZE);
        self.SetProperty(TableRowUnpaginatedBSizeProperty(), aValue);
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::GetUnpaginatedBSize
    pub fn GetUnpaginatedBSize(&self) -> nscoord {
        self.GetProperty(TableRowUnpaginatedBSizeProperty()).unwrap_or(0)
    }

    /**
     * Sets the NS_ROW_HAS_CELL_WITH_STYLE_BSIZE bit to indicate whether
     * this row has any cells that have non-auto-bsize.  (Row-spanning
     * cells are ignored.)
     */
    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::InitHasCellWithStyleBSize
    fn InitHasCellWithStyleBSize(&self, aTableFrame: nsTableFrame) {
        let wm = self.GetWritingMode();

        let mut cell = self.GetFirstCell();
        while let Some(cellFrame) = cell {
            // Ignore row-spanning cells
            let cellBSize = cellFrame.StylePosition().BSize(wm, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(cellFrame.0, std::ptr::null_mut()));
            if aTableFrame.GetEffectiveRowSpan__nsTableCellFrame_nsCellMap(&cellFrame, None) == 1
                && !cellBSize.IsAuto()
                /* calc() with both percentages and lengths treated like 'auto' */
                && (cellBSize.ConvertsToLength() || cellBSize.ConvertsToPercentage())
            {
                self.AddStateBits(NS_ROW_HAS_CELL_WITH_STYLE_BSIZE);
                return;
            }
            cell = cellFrame.GetNextCell();
        }
        self.RemoveStateBits(NS_ROW_HAS_CELL_WITH_STYLE_BSIZE);
    }

    // Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::InvalidateFrame,
    // InvalidateFrameWithRect
    // NOT PORTED: invalidation.

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::GetBStartBCBorderWidth, GetBEndBCBorderWidth,
    // SetBStartBCBorderWidth, SetBEndBCBorderWidth, GetBCBorderWidth
    pub fn GetBStartBCBorderWidth(&self) -> nscoord {
        self.Members().mBStartBorderWidth
    }
    pub fn GetBEndBCBorderWidth(&self) -> nscoord {
        self.Members().mBEndBorderWidth
    }
    pub fn SetBStartBCBorderWidth(&self, aWidth: nscoord) {
        self.Members().mBStartBorderWidth = aWidth;
    }
    pub fn SetBEndBCBorderWidth(&self, aWidth: nscoord) {
        self.Members().mBEndBorderWidth = aWidth;
    }
    pub fn GetBCBorderWidth(&self, aWM: WritingMode) -> LogicalMargin {
        LogicalMargin::new(aWM, self.Members().mBStartBorderWidth, 0, self.Members().mBEndBorderWidth, 0)
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::HasUnpaginatedBSize
    pub fn HasUnpaginatedBSize(&self) -> bool {
        self.HasAnyStateBits(NS_TABLE_ROW_HAS_UNPAGINATED_BSIZE)
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::GetAdjustmentForStoredIndex
    pub fn GetAdjustmentForStoredIndex(&self, aStoredIndex: i32) -> i32 {
        let parentFrame = self.GetTableRowGroupFrame();
        parentFrame.GetAdjustmentForStoredIndex(aStoredIndex)
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::AddDeletedRowIndex
    pub fn AddDeletedRowIndex(&self) {
        let parentFrame = self.GetTableRowGroupFrame();
        parentFrame.AddDeletedRowIndex(self.Members().mBits.mRowIndex as i32);
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::GetRowIndex
    pub fn GetRowIndex(&self) -> i32 {
        let storedRowIndex = self.Members().mBits.mRowIndex as i32;
        let rowIndexAdjustment = self.GetAdjustmentForStoredIndex(storedRowIndex);
        storedRowIndex - rowIndexAdjustment
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::SetRowIndex
    pub fn SetRowIndex(&self, aRowIndex: i32) {
        // Note: Setting the index of a row (as in the case of adding new rows) should
        // be preceded by a call to nsTableFrame::RecalculateRowIndices()
        // so as to correctly clear mDeletedRowIndexRanges.
        MOZ_ASSERT!(self.GetTableRowGroupFrame().GetTableFrame().IsDeletedRowIndexRangesEmpty(), "mDeletedRowIndexRanges should be empty here!");
        self.Members().mBits.mRowIndex = aRowIndex as u32;
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::IsFirstInserted
    pub fn IsFirstInserted(&self) -> bool {
        self.Members().mBits.mFirstInserted
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::SetFirstInserted
    pub fn SetFirstInserted(&self, aValue: bool) {
        self.Members().mBits.mFirstInserted = aValue;
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::HasStyleBSize
    pub fn HasStyleBSize(&self) -> bool {
        self.Members().mBits.mHasFixedBSize || self.Members().mBits.mHasPctBSize
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::HasFixedBSize
    pub fn HasFixedBSize(&self) -> bool {
        self.Members().mBits.mHasFixedBSize
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::SetHasFixedBSize
    pub fn SetHasFixedBSize(&self, aValue: bool) {
        self.Members().mBits.mHasFixedBSize = aValue;
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::HasPctBSize
    pub fn HasPctBSize(&self) -> bool {
        self.Members().mBits.mHasPctBSize
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::SetHasPctBSize
    pub fn SetHasPctBSize(&self, aValue: bool) {
        self.Members().mBits.mHasPctBSize = aValue;
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::GetContentBSize
    pub fn GetContentBSize(&self) -> nscoord {
        self.Members().mContentBSize
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::SetContentBSize
    pub fn SetContentBSize(&self, aValue: nscoord) {
        self.Members().mContentBSize = aValue;
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::GetFixedBSize
    pub fn GetFixedBSize(&self) -> nscoord {
        if self.Members().mBits.mHasFixedBSize {
            return self.Members().mStyleFixedBSize;
        }
        0
    }

    // Gecko: layout/tables/nsTableRowFrame.h nsTableRowFrame::GetPctBSize
    pub fn GetPctBSize(&self) -> f32 {
        if self.Members().mBits.mHasPctBSize {
            return self.Members().mStylePctBSize as f32 / 100.0f32;
        }
        0.0f32
    }
}

// Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::AccessibleType
// NOT PORTED: accessibility.

// Gecko: layout/tables/nsTableRowFrame.cpp nsTableRowFrame::GetFrameName (DEBUG_FRAME_DUMP)
// NOT PORTED: frame dumps.

//////////////////////////////////////////////////////////////////////////////////////////
// TEMP stubs
//
// Everything below is NOT part of nsTableRowFrame.h/.cpp: members of nsIFrame and friends that the
// row calls and no other translated file has yet. When the real member lands in its own file,
// delete the item here (a duplicate definition fails to compile).
//////////////////////////////////////////////////////////////////////////////////////////

// (nsIFrame::GetOverflowAreas that stood here is in nsIFrame.rs now)

// Tests: written from the documented behaviour (CSS 2.2 17.5.3 table height algorithm, 17.5.5
// collapsing rows) and the comments of nsTableRowFrame.cpp, see nsTableWrapperFrame_test_support.
#[cfg(test)]
mod tests {
    use crate::nsTableWrapperFrame_test_support::*;
    use crate::BlockReflowState_test_support::*;
    use crate::*;

    fn row_heights(t: &TTable) -> Vec<i32> {
        t.rows.iter().map(|r| r.GetSize().height / PX).collect()
    }

    fn row_tops(t: &TTable) -> Vec<i32> {
        t.rows.iter().map(|r| r.GetPosition().y / PX).collect()
    }

    #[test]
    fn a_row_is_as_tall_as_its_tallest_cell() {
        // CSS 2.2 17.5.3: the row height is the maximum of the cells' minimum heights.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10), tcell(20, 35), tcell(20, 20)]]);
        layout_root(&t, 500);
        assert_eq!(row_heights(&t), vec![35]);
        assert_eq!(t.rows[0].GetContentBSize(), 35 * PX);
    }

    #[test]
    fn rows_are_as_wide_as_the_table_and_cells_are_side_by_side() {
        // The cells of a row are placed in the inline direction, each as wide as its column.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10), tcell(30, 10), tcell(40, 10)]]);
        layout_root(&t, 500);
        assert_eq!(t.rows[0].GetSize().width, 90 * PX);
        assert_eq!(t.cells[0][0].GetPosition().x, 0);
        assert_eq!(t.cells[0][1].GetPosition().x, 20 * PX);
        assert_eq!(t.cells[0][2].GetPosition().x, 50 * PX);
        assert_eq!(t.cells[0][2].GetSize().width, 40 * PX);
    }

    #[test]
    fn a_row_height_is_a_minimum() {
        // CSS 2.2 17.5.3: 'height' on a row box is the minimum row height.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).row_css("height:50px")], vec![tcell(20, 40).row_css("height:10px")]]);
        layout_root(&t, 500);
        assert_eq!(row_heights(&t), vec![50, 40]);
        assert!(t.rows[0].HasFixedBSize());
        assert_eq!(t.rows[0].GetFixedBSize(), 50 * PX);
        assert!(t.rows[0].HasStyleBSize());
        assert!(!t.rows[0].HasPctBSize());
    }

    #[test]
    fn rows_are_stacked_in_the_block_direction() {
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10)], vec![tcell(20, 30)], vec![tcell(20, 20)]]);
        layout_root(&t, 500);
        assert_eq!(row_heights(&t), vec![10, 30, 20]);
        assert_eq!(row_tops(&t), vec![0, 10, 40]);
    }

    #[test]
    fn a_cell_spanning_two_rows_shares_its_extra_height_in_proportion_to_the_rows() {
        // CSS 2.2 17.5.3: the height of a row-spanning cell is distributed over the rows it spans;
        // nsTableRowGroupFrame::CalculateRowBSizes gives each row a share proportional to its height
        // (the last row gets the rounded share, the first the remainder).
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 80).span(1, 2), tcell(20, 10)], vec![tcell(20, 30)]]);
        layout_root(&t, 500);
        assert_eq!(row_heights(&t), vec![20, 60]);
        assert_eq!(t.cells[0][0].GetSize().height, 80 * PX);
        assert_eq!(t.cells[0][1].GetSize().height, 20 * PX);
    }

    #[test]
    fn a_row_spanning_cell_that_fits_does_not_change_the_rows() {
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 30).span(1, 2), tcell(20, 40)], vec![tcell(20, 10)]]);
        layout_root(&t, 500);
        assert_eq!(row_heights(&t), vec![40, 10]);
        assert_eq!(t.cells[0][0].GetSize().height, 50 * PX);
    }

    #[test]
    fn a_row_spanning_cell_gives_its_extra_to_the_rows_without_style_height() {
        // CalculateRowBSizes: rows with a style height are left alone while others can take the
        // extra height of a spanning cell.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 100).span(1, 2).row_css("height:30px"), tcell(20, 10)], vec![tcell(20, 10)]]);
        layout_root(&t, 500);
        assert_eq!(row_heights(&t), vec![30, 70]);
    }

    #[test]
    fn rows_pushed_down_by_a_taller_row_above_keep_their_cells_aligned() {
        // The cell positions inside a row do not depend on the rows above.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 50), tcell(30, 10)], vec![tcell(20, 10), tcell(30, 10)]]);
        layout_root(&t, 500);
        assert_eq!(t.cells[1][1].GetPosition(), nsPoint::new(20 * PX, 0));
        assert_eq!(t.rows[1].GetPosition().y, 50 * PX);
    }

    #[test]
    fn a_collapsed_row_takes_no_height() {
        // CSS 2.2 17.5.5: 'visibility: collapse' on a row removes the row, as if display:none, but
        // the cells of a row-spanning cell stay; the following rows move up.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10)], vec![tcell(20, 30).row_css("visibility:collapse")], vec![tcell(20, 20)]]);
        let m = layout_root(&t, 500);
        assert_eq!(row_heights(&t), vec![10, 0, 20]);
        assert_eq!(row_tops(&t), vec![0, 10, 10]);
        assert_eq!(t.table.GetSize().height, 30 * PX);
        assert_eq!(m.BSize(t.root.GetWritingMode()), 30 * PX);
    }

    #[test]
    fn a_percentage_row_height_takes_its_share_of_a_table_with_a_height() {
        // CSS 2.2 17.5.3 and the comments of nsTableRowGroupFrame::CalculateRowBSizes: a row with a
        // percentage height gets that part of the table's height (the table has a height); the
        // other rows take the rest.
        let t = build_table(500, "", "", "height:200px", None, &[vec![tcell(20, 10).row_css("height:50%")], vec![tcell(20, 10)]]);
        layout_root(&t, 500);
        assert_eq!(row_heights(&t), vec![100, 100]);
        assert!(t.rows[0].HasPctBSize());
        assert_eq!(t.rows[0].GetPctBSize(), 0.5);
    }

    #[test]
    fn the_row_has_no_margin_border_or_padding() {
        // CSS 2.2 17.5: rows have no margins, borders (separated model) or padding.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).row_css("margin:5px;border:3px solid;padding:2px")]]);
        layout_root(&t, 500);
        let zero = nsMargin::new(0, 0, 0, 0);
        assert_eq!(t.rows[0].GetUsedMargin(), zero);
        assert_eq!(t.rows[0].GetUsedBorder(), zero);
        assert_eq!(t.rows[0].GetUsedPadding(), zero);
        assert_eq!(t.rows[0].GetPosition(), nsPoint::new(0, 0));
        assert_eq!(t.rows[0].GetSize(), nsSize::new(20 * PX, 10 * PX));
    }

    #[test]
    fn the_row_knows_its_index_and_neighbours() {
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10)], vec![tcell(20, 10)], vec![tcell(20, 10)]]);
        assert_eq!(t.rows[0].GetRowIndex(), 0);
        assert_eq!(t.rows[2].GetRowIndex(), 2);
        assert_eq!(t.rows[0].GetPrevRow(), None);
        assert_eq!(t.rows[1].GetPrevRow(), Some(t.rows[0]));
        assert_eq!(t.rows[1].GetNextRow(), Some(t.rows[2]));
        assert_eq!(t.rows[2].GetNextRow(), None);
        assert_eq!(t.rows[1].GetTableRowGroupFrame(), t.group);
        assert_eq!(t.rows[1].GetTableFrame(), t.table);
        assert!(nsTableRowFrame::do_QueryFrame(t.rows[1].0).is_some());
        assert!(nsTableRowFrame::do_QueryFrame(t.group.0).is_none());
    }

    #[test]
    fn the_fixed_and_percentage_heights_of_a_row_keep_the_largest_value() {
        // SetFixedBSize and SetPctBSize keep the largest value (the dominating one among the row and
        // its cells); GetInitialBSize combines them with the content height.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10)]]);
        let row = t.rows[0];
        row.SetFixedBSize(30 * PX);
        row.SetFixedBSize(20 * PX);
        assert_eq!(row.GetFixedBSize(), 30 * PX);
        row.SetFixedBSize(-5);
        assert_eq!(row.GetFixedBSize(), 30 * PX);
        row.SetPctBSize(0.25, false);
        row.SetPctBSize(0.1, false);
        assert_eq!(row.GetPctBSize(), 0.25);
        row.SetPctBSize(0.1, true);
        assert_eq!(row.GetPctBSize(), 0.1);
        row.SetContentBSize(5 * PX);
        // pct of the basis, the fixed height, or the content, whichever is largest
        assert_eq!(row.GetInitialBSize(1000 * PX), 100 * PX);
        assert_eq!(row.GetInitialBSize(0), 30 * PX);
        row.SetContentBSize(40 * PX);
        assert_eq!(row.GetInitialBSize(0), 40 * PX);
        row.ResetBSize();
        assert!(!row.HasStyleBSize());
        assert_eq!(row.GetInitialBSize(1000 * PX), 0);
        assert_eq!(row.GetMaxCellAscent(), 0);
    }

    #[test]
    fn the_row_baseline_is_the_largest_cell_ascent() {
        // nsTableRowFrame::GetRowBaseline: the largest ascent of the cells that align to the baseline.
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10), tcell(20, 40), tcell(20, 5).css("vertical-align:top")]]);
        layout_root(&t, 500);
        assert_eq!(t.rows[0].GetRowBaseline(t.rows[0].GetWritingMode()), Some(40 * PX));
    }

    #[test]
    fn the_cell_reflow_input_is_fixed_up_to_the_column_width() {
        // TableCellReflowInput::FixUp: the computed inline-size of a cell is the column width minus
        // the cell's border and padding (a pass-2 reflow with the final column widths).
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).css("padding:5px;border:2px solid")], vec![tcell(50, 10)]]);
        layout_root(&t, 500);
        // column 0 is 50 wide (second row's cell); the first row's cell has 50 - 14 = 36 for content
        assert_eq!(t.cells[0][0].GetSize().width, 50 * PX);
        assert_eq!(t.inners[0][0].GetSize().width, 46 * PX);
        assert_eq!(t.contents[0][0].GetPosition().x, 5 * PX);
    }

    #[test]
    fn the_cell_in_a_row_with_a_row_span_above_starts_after_the_spanned_column() {
        // A cell that spans two rows occupies its column in the next row too: the second row's only
        // cell is in the second column (nsTableRowFrame::GetSpaceBetween puts the space of the
        // column the row-span uses in front of it).
        let t = build_table(500, "", "", "", None, &[vec![tcell(20, 10).span(1, 2), tcell(30, 10)], vec![tcell(30, 10)]]);
        layout_root(&t, 500);
        assert_eq!(t.cells[1][0].ColIndex(), 1);
        assert_eq!(t.cells[1][0].GetPosition().x, 20 * PX);
    }
}
