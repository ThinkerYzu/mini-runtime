// Translated from layout/tables/nsTableFrame.h and nsTableFrame.cpp (GECKO_REV), stage 2b, tables part A.
//
// Rust shape (see CONVENTIONS.md):
//  - `nsTableFrame` is a `Copy` handle over the frame (a frame of type Table) that derefs to nsIFrame,
//    as the C++ class derives from nsContainerFrame. The data members (`mColFrames`, `mBits`, `mCellMap`,
//    `mTableLayoutStrategy`, ...) are in one heap block per frame behind a frame property (see
//    nsTableFrame_Members); `mColFrames()` and the other accessors give them as `&'static mut` (the
//    reference does not outlive the frame; a caller must not hold one across a call that reaches the
//    same member).
//  - Pointers to table frames of other classes are handles: `nsTableColFrame`, `nsTableColGroupFrame`
//    (nsTableColFrame.rs, nsTableColGroupFrame.rs) and the classes of tables part B (`nsTableRowGroupFrame`,
//    `nsTableRowFrame`, `nsTableCellFrame`, `nsTableWrapperFrame`). A pointer that may be null is
//    `Option<handle>`.
//  - Virtual methods are a `match` on the frame type in the adapter layer (nsIFrame.rs,
//    nsContainerFrame.rs, nsLineLayoutAdapter.rs); this file has the nsTableFrame override under its own
//    name. Where the C++ calls the base class (`nsContainerFrame::ComputeSize`) the Rust calls the
//    method `nsIFrame_ComputeSize` (CONVENTIONS.md).
//
// NOT PORTED (the whole file): the collapsing-border model (CalcBCBorders and the BC cell iterator,
// painting, the damage-area bookkeeping is kept), pagination (row group header/footer repetition,
// breaking between row groups; PresContext().IsPaginated() is always false), display lists, positioned
// table parts, restyle (owned anon boxes, DidSetComputedStyle's BC check), invalidation, debug dumps,
// vertical writing modes are kept where the code is mechanical. Each is marked "NOT PORTED" where it
// occurs. Refcount logging (MOZ_COUNT_CTOR) is not ported.
//
// The classes of tables part B (nsTableRowGroupFrame, nsTableRowFrame, nsTableCellFrame) are used by their
// Gecko API. Only nsTableWrapperFrame is a TEMP handle (TableTempStubs.rs).
use crate::*;
use mrt_style::style::properties::ComputedValues;
use mrt_style::style::servo_arc::Arc as ServoArc;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::rc::Rc;

// Gecko: layout/tables/nsTableFrame.h mozilla::TableReflowMode
// An input to nsTableFrame::ReflowTable() and TableReflowInput.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableReflowMode {
    // A reflow to measure the block-size of the table. We use this value to
    // request an unconstrained available block in the first reflow if a second
    // special block-size reflow is needed later.
    Measuring,

    // A final reflow with the available block-size in the table frame's
    // ReflowInput.
    Final,
}

// Gecko: layout/tables/nsTableFrame.h nsDisplayTableItem, nsDisplayTableBackgroundSet
// NOT PORTED: display lists (nsTableFrame.h lines 46-113 at GECKO_REV).

/* ========================================================================== */

// Gecko: layout/tables/nsTableFrame.h nsTableColType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsTableColType {
    eColContent = 0,            // there is real col content associated
    eColAnonymousCol = 1,       // the result of a span on a col
    eColAnonymousColGroup = 2,  // the result of a span on a col group
    eColAnonymousCell = 3,      // the result of a cell alone
}
pub use nsTableColType::*;

// Gecko: layout/tables/nsTableFrame.cpp mozilla::TableReflowInput
pub struct TableReflowInput<'a> {
    // The real reflow input of the table frame.
    pub mReflowInput: &'a ReflowInput<'a>,

    // Stationary inline-offset, which won't change after the constructor.
    pub mICoord: nscoord,

    // Running block-offset, which will be adjusted as we reflow children.
    pub mBCoord: nscoord,

    // mReflowInput's (i.e. table frame's) writing-mode.
    mWM: WritingMode,

    // The available size for children. The inline-size is stationary after the
    // constructor, but the block-size will be adjusted as we reflow children.
    mAvailSize: LogicalSize,
}

impl<'a> TableReflowInput<'a> {
    // Gecko: layout/tables/nsTableFrame.cpp mozilla::TableReflowInput::TableReflowInput
    pub fn new(aReflowInput: &'a ReflowInput<'a>, aBorderPadding: LogicalMargin, aMode: TableReflowMode) -> TableReflowInput<'a> {
        let wm = aReflowInput.GetWritingMode();
        let mut result = TableReflowInput { mReflowInput: aReflowInput, mICoord: 0, mBCoord: 0, mWM: wm, mAvailSize: LogicalSize::new__WritingMode(wm) };
        MOZ_ASSERT!(result.mReflowInput.mFrame.IsTableFrame(), "TableReflowInput should only be created for nsTableFrame");
        let table = nsTableFrame::from_frame(result.mReflowInput.mFrame);

        result.mICoord = aBorderPadding.IStart(wm) + table.GetColSpacing__int32_t(-1);
        *result.mAvailSize.ISize_mut(wm) = std::cmp::max(0, result.mReflowInput.ComputedISize() - table.GetColSpacing__int32_t(-1) - table.GetColSpacing__int32_t(table.GetColCount()));

        *result.mAvailSize.BSize_mut(wm) = if aMode == TableReflowMode::Measuring { NS_UNCONSTRAINEDSIZE } else { result.mReflowInput.AvailableBSize() };
        result.AdvanceBCoord(aBorderPadding.BStart(wm) + (if table.GetPrevInFlow().is_none() { table.GetRowSpacing__int32_t(-1) } else { 0 }));
        if aReflowInput.mStyleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone {
            // At this point, we're assuming we won't be the last fragment, so we only
            // reserve space for block-end border-padding if we're cloning it on each
            // fragment; and we don't need to reserve any row-spacing for this
            // hypothetical fragmentation, either.
            result.ReduceAvailableBSizeBy(aBorderPadding.BEnd(wm));
        }
        result
    }

    // Advance to the next block-offset and reduce the available block-size.
    // Gecko: layout/tables/nsTableFrame.cpp mozilla::TableReflowInput::AdvanceBCoord
    pub fn AdvanceBCoord(&mut self, aAmount: nscoord) {
        self.mBCoord += aAmount;
        self.ReduceAvailableBSizeBy(aAmount);
    }

    // Gecko: layout/tables/nsTableFrame.cpp mozilla::TableReflowInput::AvailableSize
    pub fn AvailableSize(&self) -> LogicalSize {
        self.mAvailSize
    }

    // Gecko: layout/tables/nsTableFrame.cpp mozilla::TableReflowInput::ReduceAvailableBSizeBy
    fn ReduceAvailableBSizeBy(&mut self, aAmount: nscoord) {
        let wm = self.mWM;
        if self.mAvailSize.BSize(wm) == NS_UNCONSTRAINEDSIZE {
            return;
        }
        *self.mAvailSize.BSize_mut(wm) -= aAmount;
        *self.mAvailSize.BSize_mut(wm) = std::cmp::max(0, self.mAvailSize.BSize(wm));
    }
}

// Gecko: layout/tables/nsTableFrame.cpp mozilla::TableBCData
#[derive(Clone, Copy, Default)]
pub struct TableBCData {
    pub mDamageArea: TableArea,
    pub mBStartBorderWidth: nscoord,
    pub mIEndBorderWidth: nscoord,
    pub mBEndBorderWidth: nscoord,
    pub mIStartBorderWidth: nscoord,
}

/********************************************************************************
 ** nsTableFrame **
 ********************************************************************************/

// Gecko: layout/tables/nsTableFrame.h nsTableFrame::TablePartsArray
// NOT PORTED: positioned table parts (absolute positioning is not supported, so no table part is
// registered): nsTableFrame::PositionedTablePartsProperty.

// Gecko: layout/tables/nsTableFrame.h nsTableFrame::RowGroupArray (AutoTArray<nsTableRowGroupFrame*, 8>)
pub type nsTableFrame_RowGroupArray = Vec<nsTableRowGroupFrame>;
// Gecko: layout/tables/nsTableFrame.h nsTableFrame::ColGroupArray (AutoTArray<nsTableColGroupFrame*, 2>)
pub type nsTableFrame_ColGroupArray = Vec<nsTableColGroupFrame>;

// Gecko: layout/tables/nsTableFrame.h nsTableFrame::Groups
#[derive(Clone, Default)]
pub struct nsTableFrame_Groups {
    pub mRowGroups: nsTableFrame_RowGroupArray,
    pub mColGroups: nsTableFrame_ColGroupArray,
    pub mHead: Option<nsTableRowGroupFrame>,
    pub mFoot: Option<nsTableRowGroupFrame>,
}

// Gecko: layout/tables/nsTableFrame.h nsTableFrame::TableBits
#[derive(Clone, Copy, Default)]
pub struct nsTableFrame_TableBits {
    pub mHasPctCol: bool,        // does any cell or col have a pct width
    pub mCellSpansPctCol: bool,  // does any cell span a col with a pct width
                                 // (or containing a cell with a pct width)
    pub mIsBorderCollapse: bool, // border collapsing model vs. separate
                                 // model
    pub mRowInserted: bool,
    pub mNeedToCalcBCBorders: bool,
    pub mGeometryDirty: bool,
    pub mNeedToCollapse: bool, // rows, cols that have visibility:collapse
                               // need to be collapsed
    pub mResizedColumns: bool, // have we resized columns since last reflow?
    pub mNeedToCalcHasBCBorders: bool,
    pub mHasBCBorders: bool,
    pub mIsDestroying: bool, // Whether we're in the process of destroying
                             // this table frame.
}

// ADAPTER: the data members of the C++ class nsTableFrame (`mColFrames`, `mSyntheticColGroup`,
// `mBits`, `mDeletedRowIndexRanges`, `mCellMap`, `mTableLayoutStrategy`). Gecko keeps them in the frame
// object; here they are in one heap block per frame, reached through a frame property that holds
// the pointer (the block lives as long as the frame: Destroy() frees it).
pub struct nsTableFrame_Members {
    // Gecko: nsTableFrame::mColFrames (AutoTArray<nsTableColFrame*, 8>)
    pub mColFrames: Vec<nsTableColFrame>,
    // Gecko: nsTableFrame::mSyntheticColGroup
    pub mSyntheticColGroup: Option<nsTableColGroupFrame>,
    // Gecko: nsTableFrame::mBits
    pub mBits: nsTableFrame_TableBits,
    // maintains ranges of row indices of deleted rows
    // Gecko: nsTableFrame::mDeletedRowIndexRanges (std::map<int32_t, int32_t>)
    pub mDeletedRowIndexRanges: BTreeMap<i32, i32>,
    // maintains the relationships between rows, cols, and cells
    // Gecko: nsTableFrame::mCellMap (UniquePtr<nsTableCellMap>)
    pub mCellMap: Option<Box<nsTableCellMap>>,
    // the layout strategy for this frame
    // Gecko: nsTableFrame::mTableLayoutStrategy (UniquePtr<nsITableLayoutStrategy>)
    pub mTableLayoutStrategy: Option<Box<dyn nsITableLayoutStrategy>>,
}

// Gecko: layout/tables/nsTableFrame.cpp (the frame properties declared in the file)
// (ids: this crate's own numbers, the range 7000..7049 belongs to nsTableFrame.)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(MembersProperty, usize, 7010);
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(TableBCDataProperty, TableBCData, 7001);

// Gecko: layout/tables/nsTableFrame.h nsTableFrame
/**
 * nsTableFrame maps the inner portion of a table (everything except captions.)
 * Used as a pseudo-frame within nsTableWrapperFrame, it may also be used
 * stand-alone as the top-level frame.
 *
 * The principal child list contains row group frames. There is also an
 * additional child list, FrameChildListID::ColGroup, which contains the col
 * group frames.
 */
// ADAPTER: a handle (see the notes at the top of the file). The col groups are in the principal
// child list in this revision of Gecko, there is no ColGroup list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsTableFrame(pub nsIFrame);

impl Deref for nsTableFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

// Gecko: layout/tables/nsTableFrame.cpp NS_NewTableFrame
// ADAPTER: frames are made by whoever builds the frame tree (the frame constructor, a test) with
// FrameTree::AddFrame; this gives one the class.
pub fn NS_NewTableFrame(aFrame: nsIFrame) -> nsTableFrame {
    aFrame.SetType(LayoutFrameType::Table);
    nsTableFrame(aFrame)
}

impl nsTableFrame {
    // ADAPTER: static_cast<nsTableFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsTableFrame {
        nsTableFrame(aFrame)
    }

    // ADAPTER: do_QueryFrame<nsTableFrame>(frame): the frame is of type Table.
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsTableFrame> {
        if aFrame.Type() == LayoutFrameType::Table {
            Some(nsTableFrame(aFrame))
        } else {
            None
        }
    }

    // ADAPTER: this frame's data members (see nsTableFrame_Members). Created on first use.
    fn Members(&self) -> &'static mut nsTableFrame_Members {
        let p = match self.0.GetProperty(MembersProperty()) {
            Some(p) => p,
            None => {
                let p = Box::into_raw(Box::new(nsTableFrame_Members {
                    mColFrames: Vec::new(),
                    mSyntheticColGroup: None,
                    mBits: nsTableFrame_TableBits::default(),
                    mDeletedRowIndexRanges: BTreeMap::new(),
                    mCellMap: None,
                    mTableLayoutStrategy: None,
                })) as usize;
                self.0.SetOwnedPointerProperty::<nsTableFrame_Members>(MembersProperty(), p);
                p
            }
        };
        unsafe { &mut *(p as *mut nsTableFrame_Members) }
    }

    // ADAPTER: nsTableFrame::mColFrames
    pub fn mColFrames(&self) -> &'static mut Vec<nsTableColFrame> {
        &mut self.Members().mColFrames
    }

    // ADAPTER: nsTableFrame::mSyntheticColGroup
    pub fn mSyntheticColGroup(&self) -> &'static mut Option<nsTableColGroupFrame> {
        &mut self.Members().mSyntheticColGroup
    }

    // ADAPTER: nsTableFrame::mBits
    pub fn mBits(&self) -> &'static mut nsTableFrame_TableBits {
        &mut self.Members().mBits
    }

    // ADAPTER: nsTableFrame::mDeletedRowIndexRanges
    pub fn mDeletedRowIndexRanges(&self) -> &'static mut BTreeMap<i32, i32> {
        &mut self.Members().mDeletedRowIndexRanges
    }

    // ADAPTER: nsTableFrame::mCellMap
    pub fn mCellMap(&self) -> &'static mut Option<Box<nsTableCellMap>> {
        &mut self.Members().mCellMap
    }

    // ADAPTER: nsTableFrame::mTableLayoutStrategy
    pub fn mTableLayoutStrategy(&self) -> &'static mut Option<Box<dyn nsITableLayoutStrategy>> {
        &mut self.Members().mTableLayoutStrategy
    }

    // ADAPTER: static_cast<nsTableFrame*>(FirstInFlow())
    fn FirstInFlow_(&self) -> nsTableFrame {
        nsTableFrame(self.0.FirstInFlow())
    }
}

// ADAPTER (agent A, tables): Gecko resolves the style of the anonymous boxes
// `::-moz-table-column-group` and `::-moz-table-column` with `presShell->StyleSet()
// ->ResolveNonInheritingAnonymousBoxStyle(...)`. There is no pres shell here, so whoever builds the frame
// tree (the frame constructor, a test) installs a resolver with SetTableAnonBoxStyleResolver; without
// one a copy of the table's style with the display of the box and automatic sizes is used.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableAnonBox {
    ColumnGroup,
    Column,
}

pub type TableAnonBoxStyleResolver = Rc<dyn Fn(nsIFrame, TableAnonBox) -> ServoArc<ComputedValues>>;

thread_local! {
    static TABLE_ANON_BOX_STYLE_RESOLVER: RefCell<Option<TableAnonBoxStyleResolver>> = RefCell::new(None);
}

// ADAPTER: see TableAnonBox.
pub fn SetTableAnonBoxStyleResolver(aResolver: Option<TableAnonBoxStyleResolver>) {
    TABLE_ANON_BOX_STYLE_RESOLVER.with(|r| *r.borrow_mut() = aResolver);
}

// ADAPTER: ResolveNonInheritingAnonymousBoxStyle(PseudoStyleType::MozTableColumnGroup / MozTableColumn)
// for a box of the table `aTable` (see TableAnonBox).
fn ResolveNonInheritingAnonymousBoxStyle(aTable: nsIFrame, aBox: TableAnonBox) -> ServoArc<ComputedValues> {
    let resolver = TABLE_ANON_BOX_STYLE_RESOLVER.with(|r| r.borrow().clone());
    if let Some(resolver) = resolver {
        return resolver(aTable, aBox);
    }
    // NOT PORTED: the real non-inheriting style (initial values for every property). The table's
    // style with the display and the sizes reset is close enough for a column box: only its
    // display, visibility and sizes are read.
    let mut style = aTable.Style();
    {
        let s = ServoArc::make_mut(&mut style);
        let display = match aBox {
            TableAnonBox::ColumnGroup => mrt_style::style::values::specified::box_::Display::TableColumnGroup,
            TableAnonBox::Column => mrt_style::style::values::specified::box_::Display::TableColumn,
        };
        s.mutate_box().set_display(display);
        let position = s.mutate_position();
        position.set_width(StyleSize::Auto);
        position.set_height(StyleSize::Auto);
        position.set_min_width(StyleSize::Auto);
        position.set_min_height(StyleSize::Auto);
        position.set_max_width(StyleMaxSize::None);
        position.set_max_height(StyleMaxSize::None);
    }
    style
}

// Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetParentComputedStyle
// NOT PORTED: style-parent lookup for the restyle (nsTableFrame.cpp lines 140-153 at GECKO_REV); the
// frame constructor gives every frame its style.

// Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::nsTableFrame
// (memset(&mBits, 0, sizeof(mBits)): nsTableFrame_TableBits::default())

impl nsTableFrame {
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::Init
    pub fn Init(&self, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: Option<nsIFrame>) {
        MOZ_ASSERT!(self.mCellMap().is_none(), "Init called twice");
        MOZ_ASSERT!(self.mTableLayoutStrategy().is_none(), "Init called twice");
        MOZ_ASSERT!(aPrevInFlow.is_none() || aPrevInFlow.unwrap().IsTableFrame(), "prev-in-flow must be of same type");

        // Let the base class do its processing
        self.nsContainerFrame_Init(aContent, aParent, aPrevInFlow);

        // see if border collapse is on, if so set it
        let tableStyle = self.StyleTableBorder();
        let borderCollapse = StyleBorderCollapse::Collapse == tableStyle.mBorderCollapse();
        self.SetBorderCollapse(borderCollapse);
        if borderCollapse {
            self.SetNeedToCalcHasBCBorders(true);
        }

        if aPrevInFlow.is_none() {
            // If we're the first-in-flow, we manage the cell map & layout strategy that
            // get used by our continuation chain:
            *self.mCellMap() = Some(Box::new(nsTableCellMap::new(*self, borderCollapse)));
            if self.IsAutoLayout() {
                *self.mTableLayoutStrategy() = Some(Box::new(BasicTableLayoutStrategy::new(*self)));
            } else {
                *self.mTableLayoutStrategy() = Some(Box::new(FixedTableLayoutStrategy::new(*self)));
            }
        } else {
            // Set my isize, because all frames in a table flow are the same isize and
            // code in nsTableWrapperFrame depends on this being set.
            let wm = self.GetWritingMode();
            self.SetSize__WritingMode_LogicalSize(wm, LogicalSize::new(wm, aPrevInFlow.unwrap().ISize(wm), self.BSize(wm)));
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::~nsTableFrame (= default)
    // NOT PORTED: nothing to do here: the members that own data are released by Destroy().

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::Destroy
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        MOZ_ASSERT!(!self.mBits().mIsDestroying);
        self.mBits().mIsDestroying = true;
        self.nsContainerFrame_Destroy(aContext);

        // ADAPTER: free the member block (see nsTableFrame_Members); the child lists are empty now.
        if let Some(p) = self.GetProperty(MembersProperty()) {
            drop(unsafe { Box::from_raw(p as *mut nsTableFrame_Members) });
            self.RemoveProperty(MembersProperty());
        }
    }
}

// Gecko: layout/tables/nsTableFrame.cpp IsRepeatedFrame
fn IsRepeatedFrame(kidFrame: nsIFrame) -> bool {
    (kidFrame.IsTableRowFrame() || kidFrame.IsTableRowGroupFrame()) && kidFrame.HasAnyStateBits(NS_REPEATED_ROW_OR_ROWGROUP)
}

impl nsTableFrame {
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::PageBreakAfter
    pub fn PageBreakAfter(aSourceFrame: nsIFrame, aNextFrame: Option<nsIFrame>) -> bool {
        let mut display = aSourceFrame.StyleDisplay();
        let prevRg = nsTableRowGroupFrame::do_QueryFrame(aSourceFrame);
        // don't allow a page break after a repeated element ...
        if (display.BreakAfter() || prevRg.map_or(false, |rg| rg.HasInternalBreakAfter())) && !IsRepeatedFrame(aSourceFrame) {
            return !(aNextFrame.map_or(false, |n| IsRepeatedFrame(n))); // or before
        }

        if let Some(aNextFrame) = aNextFrame {
            display = aNextFrame.StyleDisplay();
            // don't allow a page break before a repeated element ...
            let nextRg = nsTableRowGroupFrame::do_QueryFrame(aNextFrame);
            if (display.BreakBefore() || nextRg.map_or(false, |rg| rg.HasInternalBreakBefore())) && !IsRepeatedFrame(aNextFrame) {
                return !IsRepeatedFrame(aSourceFrame); // or after
            }
        }
        false
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::PositionedTablePartMaybeChanged
    // NOT PORTED: positioned table parts (absolute positioning is not supported): nsTableFrame.cpp
    // lines 238-264 at GECKO_REV.
    pub fn PositionedTablePartMaybeChanged(aFrame: nsIFrame, aOldStyle: Option<ServoArc<ComputedValues>>) {}

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::MaybeUnregisterPositionedTablePart
    // NOT PORTED: positioned table parts: nsTableFrame.cpp lines 266-290 at GECKO_REV.
    pub fn MaybeUnregisterPositionedTablePart(aFrame: nsIFrame) {}

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::SetInitialChildList
    pub fn SetInitialChildList(&self, aListID: FrameChildListID, aChildList: nsFrameList) {
        self.nsContainerFrame_SetInitialChildList(aListID, aChildList);
        if aListID != FrameChildListID::Principal {
            return;
        }

        // If we have a prev-in-flow, then we're a table that has been split and
        // so don't treat this like an append
        if self.GetPrevInFlow().is_none() {
            // process col groups first so that real cols get constructed before
            // anonymous ones due to cells in rows.
            self.InsertColGroups(0, nsFrameList_Slice::new__nsFrameList(&*self.mFrames()));
            self.InsertRowGroups(nsFrameList_Slice::new__nsFrameList(&*self.mFrames()));
            // calc collapsing borders
            if self.IsBorderCollapse() {
                self.SetFullBCDamageArea();
            }
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::RowOrColSpanChanged
    pub fn RowOrColSpanChanged(&self, aCellFrame: Option<nsTableCellFrame>) {
        let Some(aCellFrame) = aCellFrame else {
            return;
        };
        let cellMap = self.GetCellMap();
        if cellMap.is_none() {
            return;
        }
        // for now just remove the cell from the map and reinsert it
        let rowIndex = aCellFrame.RowIndex();
        let colIndex = aCellFrame.ColIndex();
        self.RemoveCell(Some(aCellFrame), rowIndex as i32);
        let mut cells: Vec<nsTableCellFrame> = Vec::new();
        cells.push(aCellFrame);
        self.InsertCells(&mut cells, rowIndex as i32, colIndex as i32 - 1);

        // XXX Should this use IntrinsicDirty::FrameAncestorsAndDescendants? It
        // currently doesn't need to, but it might given more optimization.
        PresShell_FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_IS_DIRTY);
    }

    /* ****** CellMap methods ******* */

    /* return the effective col count */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetEffectiveColCount
    pub fn GetEffectiveColCount(&self) -> i32 {
        let mut colCount = self.GetColCount();
        if self.LayoutStrategy().unwrap().GetType() == nsITableLayoutStrategy_Type::Auto {
            let cellMap = self.GetCellMap();
            let Some(cellMap) = cellMap else {
                return 0;
            };
            // don't count cols at the end that don't have originating cells
            let mut colIdx = colCount - 1;
            while colIdx >= 0 {
                if cellMap.GetNumCellsOriginatingInCol(colIdx) > 0 {
                    break;
                }
                colCount -= 1;
                colIdx -= 1;
            }
        }
        colCount
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetIndexOfLastRealCol
    pub fn GetIndexOfLastRealCol(&self) -> i32 {
        let numCols = self.mColFrames().len() as i32;
        if numCols > 0 {
            let mut colIdx = numCols - 1;
            while colIdx >= 0 {
                let colFrame = self.GetColFrame(colIdx);
                if let Some(colFrame) = colFrame {
                    if eColAnonymousCell != colFrame.GetColType() {
                        return colIdx;
                    }
                }
                colIdx -= 1;
            }
        }
        -1
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetColFrame
    pub fn GetColFrame(&self, aColIndex: i32) -> Option<nsTableColFrame> {
        MOZ_ASSERT!(self.GetPrevInFlow().is_none(), "GetColFrame called on next in flow");
        let numCols = self.mColFrames().len() as i32;
        if (aColIndex >= 0) && (aColIndex < numCols) {
            Some(self.mColFrames()[aColIndex as usize])
        } else {
            MOZ_ASSERT_UNREACHABLE!("invalid col index");
            None
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetEffectiveRowSpan(int32_t, const nsTableCellFrame&)
    pub fn GetEffectiveRowSpan__int32_t_nsTableCellFrame(&self, aRowIndex: i32, aCell: &nsTableCellFrame) -> i32 {
        let cellMap = self.GetCellMap();
        MOZ_ASSERT!(cellMap.is_some(), "bad call, cellMap not yet allocated.");

        cellMap.unwrap().GetEffectiveRowSpan(aRowIndex, aCell.ColIndex() as i32)
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetEffectiveRowSpan(const nsTableCellFrame&, nsCellMap*)
    pub fn GetEffectiveRowSpan__nsTableCellFrame_nsCellMap(&self, aCell: &nsTableCellFrame, aCellMap: Option<&nsCellMap>) -> i32 {
        let tableCellMap = self.GetCellMap();
        let Some(tableCellMap) = tableCellMap else {
            NS_ASSERTION!(false, "CellIterator program error");
            return 1;
        };

        let colIndex = aCell.ColIndex() as i32;
        let rowIndex = aCell.RowIndex() as i32;

        if let Some(aCellMap) = aCellMap {
            return aCellMap.GetRowSpan(rowIndex, colIndex, true);
        }
        tableCellMap.GetEffectiveRowSpan(rowIndex, colIndex)
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetEffectiveColSpan
    pub fn GetEffectiveColSpan(&self, aCell: &nsTableCellFrame, aCellMap: Option<&nsCellMap>) -> i32 {
        let tableCellMap = self.GetCellMap();
        let Some(tableCellMap) = tableCellMap else {
            NS_ASSERTION!(false, "CellIterator program error");
            return 1;
        };

        let colIndex = aCell.ColIndex() as i32;
        let rowIndex = aCell.RowIndex() as i32;

        if let Some(aCellMap) = aCellMap {
            return aCellMap.GetEffectiveColSpan(tableCellMap, rowIndex, colIndex);
        }
        tableCellMap.GetEffectiveColSpan(rowIndex, colIndex)
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::HasMoreThanOneCell
    pub fn HasMoreThanOneCell(&self, aRowIndex: i32) -> bool {
        let tableCellMap = self.GetCellMap();
        let Some(tableCellMap) = tableCellMap else {
            NS_ASSERTION!(false, "CellIterator program error");
            return true;
        };
        tableCellMap.HasMoreThanOneCell(aRowIndex)
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::AdjustRowIndices
    pub fn AdjustRowIndices(&self, aRowIndex: i32, aAdjustment: i32) {
        // Iterate over the row groups and adjust the row indices of all rows
        // whose index is >= aRowIndex.
        for rg in self.OrderedGroups().mRowGroups {
            rg.AdjustRowIndices(aRowIndex, aAdjustment);
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::ResetRowIndices
    pub fn ResetRowIndices(&self, aRowGroupsToExclude: nsFrameList_Slice) {
        // Iterate over the row groups and adjust the row indices of all rows
        // omit the rowgroups that will be inserted later
        self.mDeletedRowIndexRanges().clear();

        let rowGroups = self.OrderedRowGroups();

        let mut excludeRowGroups: Vec<nsTableRowGroupFrame> = Vec::new();
        for excludeRowGroup in aRowGroupsToExclude {
            if let Some(rg) = nsTableRowGroupFrame::do_QueryFrame(excludeRowGroup) {
                excludeRowGroups.push(rg);
                #[cfg(debug_assertions)]
                {
                    // Check to make sure that the row indices of all rows in excluded row
                    // groups are '0' (i.e. the initial value since they haven't been added
                    // yet)
                    let rowFrames = excludeRowGroup.GetChildList(FrameChildListID::Principal);
                    for r in rowFrames {
                        let row = nsTableRowFrame::from_frame(r);
                        MOZ_ASSERT!(row.GetRowIndex() == 0, "exclusions cannot be used for rows that were already added,because we'd need to process mDeletedRowIndexRanges");
                    }
                }
            }
        }

        let mut rowIndex = 0;
        for rgIdx in 0..rowGroups.len() {
            let rgFrame = rowGroups[rgIdx];
            if !excludeRowGroups.contains(&rgFrame) {
                let rowFrames = rgFrame.GetChildList(FrameChildListID::Principal);
                for r in rowFrames {
                    if StyleDisplay::TableRow == r.StyleDisplay().mDisplay() {
                        let row = nsTableRowFrame::from_frame(r);
                        row.SetRowIndex(rowIndex);
                        rowIndex += 1;
                    }
                }
            }
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::InsertColGroups
    pub fn InsertColGroups(&self, aStartColIndex: i32, aNewFrames: nsFrameList_Slice) {
        let mut colIndex = aStartColIndex;
        let mut lastFrame: Option<nsIFrame> = None;
        for f in aNewFrames {
            lastFrame = Some(f);
            let cg = nsTableColGroupFrame::do_QueryFrame(f);
            let Some(cg) = cg else {
                continue;
            };
            if cg.IsSynthetic() {
                continue;
            }
            cg.SetStartColumnIndex(colIndex);
            cg.AddColsToTable(colIndex, false, &nsFrameList_Slice::new__nsFrameList(cg.GetChildList(FrameChildListID::Principal)));
            let numCols = cg.GetColCount();
            colIndex += numCols;
        }
        let next = lastFrame.and_then(|f| f.GetNextSibling());
        nsTableColGroupFrame::ResetColIndices(next, self.GetSyntheticColGroup(), colIndex, None);
        #[cfg(debug_assertions)]
        self.VerifyColFrames();
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::InsertCol
    pub fn InsertCol(&self, aColFrame: nsTableColFrame, aColIndex: i32) {
        self.mColFrames().insert(aColIndex as usize, aColFrame);
        let insertedColType = aColFrame.GetColType();
        let numCacheCols = self.mColFrames().len() as i32;
        if let Some(cellMap) = self.GetCellMap() {
            let numMapCols = cellMap.GetColCount();
            if numCacheCols > numMapCols {
                let mut removedFromCache = false;
                if eColAnonymousCell != insertedColType {
                    let lastCol = self.mColFrames()[(numCacheCols - 1) as usize];
                    let lastColType = lastCol.GetColType();
                    if eColAnonymousCell == lastColType {
                        // remove the col from the cache
                        self.mColFrames().pop();
                        if let Some(syntheticColGroup) = *self.mSyntheticColGroup() {
                            MOZ_ASSERT!(syntheticColGroup.IsSynthetic());
                            let mut context = FrameDestroyContext::default();
                            syntheticColGroup.RemoveChild(&mut context, lastCol, false);

                            // remove the col group if it is empty
                            if syntheticColGroup.GetColCount() <= 0 {
                                self.mFrames().DestroyFrame(&mut context, syntheticColGroup.0);
                                *self.mSyntheticColGroup() = None;
                            }
                        }
                        removedFromCache = true;
                    }
                }
                if !removedFromCache {
                    cellMap.AddColsAtEnd(1);
                }
            }
        }
        // for now, just bail and recalc all of the collapsing borders
        if self.IsBorderCollapse() {
            let damageArea = TableArea::new(aColIndex, 0, self.GetColCount() - aColIndex, self.GetRowCount());
            self.AddBCDamageArea(&damageArea);
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::RemoveCol
    pub fn RemoveCol(&self, aColIndex: i32, aRemoveFromCache: bool, aRemoveFromCellMap: bool) {
        if aRemoveFromCache {
            self.mColFrames().remove(aColIndex as usize);
        }
        if aRemoveFromCellMap {
            let cellMap = self.GetCellMap();
            if let Some(cellMap) = cellMap {
                // If we have some anonymous cols at the end already, we just
                // add a new anonymous col.
                if !self.mColFrames().is_empty() && self.mColFrames().last().unwrap().GetColType() == eColAnonymousCell {
                    self.AppendAnonymousColFrames__int32_t(1);
                } else {
                    // All of our colframes correspond to actual <col> tags.  It's possible
                    // that we still have at least as many <col> tags as we have logical
                    // columns from cells, but we might have one less.  Handle the latter
                    // case as follows: First ask the cellmap to drop its last col if it
                    // doesn't have any actual cells in it.  Then call
                    // MatchCellMapToColCache to append an anonymous column if it's needed;
                    // this needs to be after RemoveColsAtEnd, since it will determine the
                    // need for a new column frame based on the width of the cell map.
                    cellMap.RemoveColsAtEnd();
                    self.MatchCellMapToColCache(cellMap);
                }
            }
        }
        // for now, just bail and recalc all of the collapsing borders
        if self.IsBorderCollapse() {
            self.SetFullBCDamageArea();
        }
    }

    /** Get the cell map for this table frame.  It is not always mCellMap.
     * Only the first-in-flow has a legit cell map.
     */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetCellMap
    // ADAPTER: the reference is 'static because the map lives in a Box inside the frame's member
    // block; a caller must not hold it across a call that replaces the map.
    pub fn GetCellMap(&self) -> Option<&'static mut nsTableCellMap> {
        self.FirstInFlow_().mCellMap().as_deref_mut()
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::CreateSyntheticColGroupFrame
    pub fn CreateSyntheticColGroupFrame(&self) -> nsTableColGroupFrame {
        let colGroupContent = self.GetContent();

        let colGroupStyle = ResolveNonInheritingAnonymousBoxStyle(self.0, TableAnonBox::ColumnGroup);
        // Create a col group frame
        let wm = WritingMode::new__ComputedStyle(&colGroupStyle);
        let frame = WithFrameTree(|t| t.AddFrame(None, nsRect::new(0, 0, 0, 0), wm, Some(colGroupStyle)));
        let newFrame = NS_NewTableColGroupFrame(frame);
        newFrame.SetIsSynthetic();
        // nsIFrame::Init: the content and the parent (see nsCSSFrameConstructor::InitFrame)
        frame.SetContent(colGroupContent.clone());
        frame.SetParent(self.0);
        newFrame.Init(colGroupContent, self.0, None);
        newFrame
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetRealColEnd
    pub fn GetRealColEnd(&self) -> i32 {
        for col in self.mColFrames().iter().rev() {
            let cg = nsTableColGroupFrame::from_frame(col.GetParent().unwrap());
            if !cg.IsSynthetic() {
                return col.GetColIndex() + 1;
            }
        }
        0
    }
}

// Gecko: layout/tables/nsTableFrame.cpp GetColStartAfter
fn GetColStartAfter(aPrevSibling: Option<nsIFrame>) -> i32 {
    let mut f = aPrevSibling;
    while let Some(fr) = f {
        if let Some(cg) = nsTableColGroupFrame::do_QueryFrame(fr) {
            if !cg.IsSynthetic() {
                return cg.GetStartColumnIndex() + cg.GetColCount();
            }
        }
        f = fr.GetPrevSibling();
    }
    0
}

impl nsTableFrame {
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::AppendAnonymousColFrames(int32_t)
    pub fn AppendAnonymousColFrames__int32_t(&self, aNumColsToAdd: i32) {
        MOZ_ASSERT!(aNumColsToAdd > 0, "We should be adding _something_.");
        if self.mSyntheticColGroup().is_none() {
            let colIndex = self.GetRealColEnd();
            let syntheticColGroup = self.CreateSyntheticColGroupFrame();
            *self.mSyntheticColGroup() = Some(syntheticColGroup);
            // add the new frame to the child list
            self.mFrames().AppendFrame(Some(self.0), syntheticColGroup.0);
            syntheticColGroup.SetStartColumnIndex(colIndex);
        }
        let syntheticColGroup = self.mSyntheticColGroup().unwrap();
        self.AppendAnonymousColFrames__nsTableColGroupFrame_int32_t_nsTableColType_bool(syntheticColGroup, aNumColsToAdd, eColAnonymousCell, true);
    }

    // XXX this needs to be moved to nsCSSFrameConstructor
    // Right now it only creates the col frames at the end
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::AppendAnonymousColFrames(nsTableColGroupFrame*, int32_t, nsTableColType, bool)
    pub fn AppendAnonymousColFrames__nsTableColGroupFrame_int32_t_nsTableColType_bool(&self, aColGroupFrame: nsTableColGroupFrame, aNumColsToAdd: i32, aColType: nsTableColType, aAddToTable: bool) {
        MOZ_ASSERT!(aColType != eColAnonymousCol, "Shouldn't happen");
        MOZ_ASSERT!(aNumColsToAdd > 0, "We should be adding _something_.");

        // Get the last col frame
        let mut newColFrames = nsFrameList::default();

        let startIndex = self.mColFrames().len() as i32;
        let lastIndex = startIndex + aNumColsToAdd - 1;

        for childX in startIndex..=lastIndex {
            // all anonymous cols that we create here use a pseudo ComputedStyle of the
            // col group
            let iContent = aColGroupFrame.GetContent();
            let computedStyle = ResolveNonInheritingAnonymousBoxStyle(self.0, TableAnonBox::Column);
            // ASSERTION to check for bug 54454 sneaking back in...
            NS_ASSERTION!(iContent.is_some(), "null content in CreateAnonymousColFrames");

            // create the new col frame
            let wm = WritingMode::new__ComputedStyle(&computedStyle);
            let frame = WithFrameTree(|t| t.AddFrame(None, nsRect::new(0, 0, 0, 0), wm, Some(computedStyle)));
            let colFrame = NS_NewTableColFrame(frame);
            colFrame.SetColType(aColType);
            // nsIFrame::Init: the content and the parent (see nsCSSFrameConstructor::InitFrame)
            frame.SetContent(iContent.clone());
            frame.SetParent(aColGroupFrame.0);
            colFrame.Init(iContent, aColGroupFrame.0, None);

            newColFrames.AppendFrame(None, frame);
        }
        let cols = aColGroupFrame.GetWritableChildList();
        let oldLastCol = cols.LastChild();
        let newCols = cols.InsertFrames(None, oldLastCol, newColFrames);
        if aAddToTable {
            // get the starting col index in the cache
            let startColIndex = if let Some(oldLastCol) = oldLastCol { nsTableColFrame::from_frame(oldLastCol).GetColIndex() + 1 } else { aColGroupFrame.GetStartColumnIndex() };

            aColGroupFrame.AddColsToTable(startColIndex, true, &newCols);
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::MatchCellMapToColCache
    pub fn MatchCellMapToColCache(&self, aCellMap: &mut nsTableCellMap) {
        let numColsInMap = self.GetColCount();
        let numColsInCache = self.mColFrames().len() as i32;
        let numColsToAdd = numColsInMap - numColsInCache;
        if numColsToAdd > 0 {
            // this sets the child list, updates the col cache and cell map
            self.AppendAnonymousColFrames__int32_t(numColsToAdd);
        }
        if numColsToAdd < 0 {
            let numColsNotRemoved = self.DestroyAnonymousColFrames(-numColsToAdd);
            // if the cell map has fewer cols than the cache, correct it
            if numColsNotRemoved > 0 {
                aCellMap.AddColsAtEnd(numColsNotRemoved as u32);
            }
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::DidResizeColumns
    pub fn DidResizeColumns(&self) {
        MOZ_ASSERT!(self.GetPrevInFlow().is_none(), "should only be called on first-in-flow");

        if self.mBits().mResizedColumns {
            return; // already marked
        }

        let mut f = Some(*self);
        while let Some(fr) = f {
            fr.mBits().mResizedColumns = true;
            f = fr.GetNextInFlow().map(nsTableFrame::from_frame);
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::AppendCell
    pub fn AppendCell(&self, aCellFrame: nsTableCellFrame, aRowIndex: i32) {
        let cellMap = self.GetCellMap();
        if let Some(cellMap) = cellMap {
            let mut damageArea = TableArea::new(0, 0, 0, 0);
            cellMap.AppendCell(aCellFrame, aRowIndex, true, &mut damageArea);
            self.MatchCellMapToColCache(cellMap);
            if self.IsBorderCollapse() {
                self.AddBCDamageArea(&damageArea);
            }
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::InsertCells
    pub fn InsertCells(&self, aCellFrames: &mut Vec<nsTableCellFrame>, aRowIndex: i32, aColIndexBefore: i32) {
        let cellMap = self.GetCellMap();
        if let Some(cellMap) = cellMap {
            let mut damageArea = TableArea::new(0, 0, 0, 0);
            cellMap.InsertCells(aCellFrames, aRowIndex, aColIndexBefore, &mut damageArea);
            self.MatchCellMapToColCache(cellMap);
            if self.IsBorderCollapse() {
                self.AddBCDamageArea(&damageArea);
            }
        }
    }

    // this removes the frames from the col group and table, but not the cell map
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::DestroyAnonymousColFrames
    pub fn DestroyAnonymousColFrames(&self, aNumFrames: i32) -> i32 {
        // only remove cols that are of type eTypeAnonymous cell (they are at the end)
        let endIndex = self.mColFrames().len() as i32 - 1;
        let startIndex = (endIndex - aNumFrames) + 1;
        let mut numColsRemoved = 0;
        let mut context = FrameDestroyContext::default();
        let mut colIdx = endIndex;
        while colIdx >= startIndex {
            let colFrame = self.GetColFrame(colIdx);
            if colFrame.map_or(false, |c| eColAnonymousCell == c.GetColType()) {
                let colFrame = colFrame.unwrap();
                let cgFrame = nsTableColGroupFrame::from_frame(colFrame.GetParent().unwrap());
                // remove the frame from the colgroup
                cgFrame.RemoveChild(&mut context, colFrame, false);
                // remove the frame from the cache, but not the cell map
                self.RemoveCol(colIdx, true, false);
                numColsRemoved += 1;
            } else {
                break;
            }
            colIdx -= 1;
        }
        aNumFrames - numColsRemoved
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::RemoveCell
    pub fn RemoveCell(&self, aCellFrame: Option<nsTableCellFrame>, aRowIndex: i32) {
        let cellMap = self.GetCellMap();
        if let Some(cellMap) = cellMap {
            let mut damageArea = TableArea::new(0, 0, 0, 0);
            cellMap.RemoveCell(aCellFrame, aRowIndex, &mut damageArea);
            self.MatchCellMapToColCache(cellMap);
            if self.IsBorderCollapse() {
                self.AddBCDamageArea(&damageArea);
            }
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetStartRowIndex
    pub fn GetStartRowIndex(&self, aRowGroupFrame: &nsTableRowGroupFrame) -> i32 {
        let orderedRowGroups = self.OrderedRowGroups();

        let mut rowIndex = 0;
        for rgIndex in 0..orderedRowGroups.len() {
            let rgFrame = orderedRowGroups[rgIndex];
            if rgFrame == *aRowGroupFrame {
                break;
            }
            let numRows = rgFrame.GetRowCount();
            rowIndex += numRows;
        }
        rowIndex
    }

    // this cannot extend beyond a single row group
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::AppendRows
    pub fn AppendRows(&self, aRowGroupFrame: nsTableRowGroupFrame, aRowIndex: i32, aRowFrames: &mut Vec<nsTableRowFrame>) {
        let cellMap = self.GetCellMap();
        if cellMap.is_some() {
            let absRowIndex = self.GetStartRowIndex(&aRowGroupFrame) + aRowIndex;
            self.InsertRows(aRowGroupFrame, aRowFrames, absRowIndex, true);
        }
    }

    // this cannot extend beyond a single row group
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::InsertRows
    pub fn InsertRows(&self, aRowGroupFrame: nsTableRowGroupFrame, aRowFrames: &mut Vec<nsTableRowFrame>, aRowIndex: i32, aConsiderSpans: bool) -> i32 {
        // NOT PORTED: DEBUG_TABLE_CELLMAP dumps.

        let numColsToAdd = 0;
        let cellMap = self.GetCellMap();
        if let Some(cellMap) = cellMap {
            let mut damageArea = TableArea::new(0, 0, 0, 0);
            let shouldRecalculateIndex = !self.IsDeletedRowIndexRangesEmpty();
            if shouldRecalculateIndex {
                self.ResetRowIndices(nsFrameList_Slice::new(None, None));
            }
            let origNumRows = cellMap.GetRowCount();
            let numNewRows = aRowFrames.len() as i32;
            cellMap.InsertRows(aRowGroupFrame, aRowFrames, aRowIndex, aConsiderSpans, &mut damageArea);
            self.MatchCellMapToColCache(cellMap);

            // Perform row index adjustment only if row indices were not
            // reset above
            if !shouldRecalculateIndex {
                if aRowIndex < origNumRows {
                    self.AdjustRowIndices(aRowIndex, numNewRows);
                }

                // assign the correct row indices to the new rows. If they were
                // recalculated above it may not have been done correctly because each row
                // is constructed with index 0
                for rowB in 0..numNewRows {
                    let rowFrame = aRowFrames[rowB as usize];
                    rowFrame.SetRowIndex(aRowIndex + rowB);
                }
            }

            if self.IsBorderCollapse() {
                self.AddBCDamageArea(&damageArea);
            }
        }

        numColsToAdd
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::AddDeletedRowIndex
    pub fn AddDeletedRowIndex(&self, aDeletedRowStoredIndex: i32) {
        let ranges = self.mDeletedRowIndexRanges();
        if ranges.is_empty() {
            ranges.insert(aDeletedRowStoredIndex, aDeletedRowStoredIndex);
            return;
        }

        // Find the position of the current deleted row's stored index
        // among the previous deleted row index ranges and merge ranges if
        // they are consecutive, else add a new (disjoint) range to the map.
        // Call to mDeletedRowIndexRanges.upper_bound is
        // O(log(mDeletedRowIndexRanges.size())) therefore call to
        // AddDeletedRowIndex is also ~O(log(mDeletedRowIndexRanges.size()))

        // greaterIter = will point to smallest range in the map with lower value
        //              greater than the aDeletedRowStoredIndex.
        //              If no such value exists, point to end of map.
        // smallerIter = will point to largest range in the map with higher value
        //              smaller than the aDeletedRowStoredIndex
        //              If no such value exists, point to beginning of map.
        // i.e. when both values exist below is true:
        // smallerIter->second < aDeletedRowStoredIndex < greaterIter->first
        // ADAPTER: the iterators are the keys (std::map::upper_bound is a range query on the BTreeMap).
        use std::ops::Bound::{Excluded, Unbounded};
        let greater: Option<(i32, i32)> = ranges.range((Excluded(aDeletedRowStoredIndex), Unbounded)).next().map(|(k, v)| (*k, *v));
        // "smallerIter" is the previous range, or the first range when there is none before it.
        let smaller: (i32, i32) = match ranges.range(..=aDeletedRowStoredIndex).next_back() {
            Some((k, v)) => (*k, *v),
            None => {
                let (k, v) = ranges.iter().next().unwrap();
                (*k, *v)
            }
        };
        let smallerIsGreater = ranges.range(..=aDeletedRowStoredIndex).next_back().is_none();

        // Note: smallerIter can only be equal to greaterIter when both
        // of them point to the beginning of the map and in that case smallerIter
        // does not "exist" but we clip smallerIter to point to beginning of map
        // so that it doesn't point to something unknown or outside the map boundry.
        // Note: When greaterIter is not the end (i.e. it "exists") upper_bound()
        // ensures aDeletedRowStoredIndex < greaterIter->first so no need to
        // assert that.
        MOZ_ASSERT!(smallerIsGreater || aDeletedRowStoredIndex > smaller.1, "aDeletedRowIndexRanges already contains aDeletedRowStoredIndex! Trying to delete an already deleted row?");

        if smaller.1 == aDeletedRowStoredIndex - 1 {
            if greater.map_or(false, |g| g.0 == aDeletedRowStoredIndex + 1) {
                // merge current index with smaller and greater range as they are
                // consecutive
                let g = greater.unwrap();
                ranges.insert(smaller.0, g.1);
                ranges.remove(&g.0);
            } else {
                // add aDeletedRowStoredIndex in the smaller range as it is consecutive
                ranges.insert(smaller.0, aDeletedRowStoredIndex);
            }
        } else if greater.map_or(false, |g| g.0 == aDeletedRowStoredIndex + 1) {
            // add aDeletedRowStoredIndex in the greater range as it is consecutive
            let g = greater.unwrap();
            ranges.insert(aDeletedRowStoredIndex, g.1);
            ranges.remove(&g.0);
        } else {
            // add new range as aDeletedRowStoredIndex is disjoint from existing ranges
            ranges.insert(aDeletedRowStoredIndex, aDeletedRowStoredIndex);
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetAdjustmentForStoredIndex
    pub fn GetAdjustmentForStoredIndex(&self, aStoredIndex: i32) -> i32 {
        let ranges = self.mDeletedRowIndexRanges();
        if ranges.is_empty() {
            return 0;
        }

        let mut adjustment = 0;

        // O(log(mDeletedRowIndexRanges.size()))
        for (first, second) in ranges.range(..=aStoredIndex) {
            adjustment += *second - *first + 1;
        }

        adjustment
    }

    // this cannot extend beyond a single row group
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::RemoveRows
    pub fn RemoveRows(&self, aFirstRowFrame: nsTableRowFrame, aNumRowsToRemove: i32, aConsiderSpans: bool) {
        // NOT PORTED: TBD_OPTIMIZATION (an #ifdef that is never defined, nsTableFrame.cpp lines 940-957
        // at GECKO_REV).

        let firstRowIndex = aFirstRowFrame.GetRowIndex();
        // NOT PORTED: DEBUG_TABLE_CELLMAP dumps.
        let cellMap = self.GetCellMap();
        if let Some(cellMap) = cellMap {
            let mut damageArea = TableArea::new(0, 0, 0, 0);

            // Mark rows starting from aFirstRowFrame to the next 'aNumRowsToRemove-1'
            // number of rows as deleted.
            let parentFrame = aFirstRowFrame.GetTableRowGroupFrame();
            parentFrame.MarkRowsAsDeleted(aFirstRowFrame, aNumRowsToRemove);

            cellMap.RemoveRows(firstRowIndex, aNumRowsToRemove, aConsiderSpans, &mut damageArea);
            self.MatchCellMapToColCache(cellMap);
            if self.IsBorderCollapse() {
                self.AddBCDamageArea(&damageArea);
            }
        }
    }

    // collect the rows ancestors of aFrame
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::CollectRows
    pub fn CollectRows(&self, aFrame: nsIFrame, aCollection: &mut Vec<nsTableRowFrame>) -> i32 {
        let mut numRows = 0;
        for childFrame in aFrame.GetChildList(FrameChildListID::Principal) {
            aCollection.push(nsTableRowFrame::from_frame(childFrame));
            numRows += 1;
        }
        numRows
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::InsertRowGroups
    pub fn InsertRowGroups(&self, aRowGroups: nsFrameList_Slice) {
        // NOT PORTED: DEBUG_TABLE_CELLMAP dumps.
        let cellMap = self.GetCellMap();
        if let Some(cellMap) = cellMap {
            let orderedRowGroups = self.OrderedRowGroups();

            let mut rows: Vec<nsTableRowFrame> = Vec::new();
            // Loop over the rowgroups and check if some of them are new, if they are
            // insert cellmaps in the order that is predefined by OrderedRowGroups.
            // XXXbz this code is O(N*M) where N is number of new rowgroups
            // and M is number of rowgroups we have!
            for rgIndex in 0..orderedRowGroups.len() {
                for rowGroup in aRowGroups.clone() {
                    if orderedRowGroups[rgIndex].0 == rowGroup {
                        let priorRG = if 0 == rgIndex { None } else { Some(orderedRowGroups[rgIndex - 1]) };
                        // create and add the cell map for the row group
                        let mut priorRG = priorRG;
                        cellMap.InsertGroupCellMap__nsTableRowGroupFrame_nsTableRowGroupFrame(orderedRowGroups[rgIndex], &mut priorRG);

                        break;
                    }
                }
            }
            cellMap.Synchronize(*self);
            self.ResetRowIndices(aRowGroups.clone());

            // now that the cellmaps are reordered too insert the rows
            for rgIndex in 0..orderedRowGroups.len() {
                for rowGroup in aRowGroups.clone() {
                    if orderedRowGroups[rgIndex].0 == rowGroup {
                        let priorRG = if 0 == rgIndex { None } else { Some(orderedRowGroups[rgIndex - 1]) };
                        // collect the new row frames in an array and add them to the table
                        let numRows = self.CollectRows(rowGroup, &mut rows);
                        if numRows > 0 {
                            let mut rowIndex = 0;
                            if let Some(priorRG) = priorRG {
                                let priorNumRows = priorRG.GetRowCount();
                                rowIndex = priorRG.GetStartRowIndex() + priorNumRows;
                            }
                            self.InsertRows(orderedRowGroups[rgIndex], &mut rows, rowIndex, true);
                            rows.clear();
                        }
                        break;
                    }
                }
            }
        }
        // NOT PORTED: DEBUG_TABLE_CELLMAP dumps.
    }
}

/////////////////////////////////////////////////////////////////////////////
// Child frame enumeration

// Gecko: layout/tables/nsTableFrame.cpp FrameHasBorder
// NOT PORTED: nsStyleBorder::HasBorder is only read by the collapsed-border code (CalcHasBCBorders).
fn FrameHasBorder(f: nsIFrame) -> bool {
    if !f.StyleVisibility().IsVisible() {
        return false;
    }

    let b = f.StyleBorder().GetComputedBorder();
    b.top != 0 || b.right != 0 || b.bottom != 0 || b.left != 0
}

impl nsTableFrame {
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::CalcHasBCBorders
    pub fn CalcHasBCBorders(&self) {
        if !self.IsBorderCollapse() {
            self.SetHasBCBorders(false);
            return;
        }

        if FrameHasBorder(self.0) {
            self.SetHasBCBorders(true);
            return;
        }

        let groups = self.OrderedGroups();
        // Check col and col group has borders.
        for cg in groups.mColGroups {
            if FrameHasBorder(cg.0) {
                self.SetHasBCBorders(true);
                return;
            }

            let mut col = cg.GetFirstColumn();
            while let Some(c) = col {
                if FrameHasBorder(c.0) {
                    self.SetHasBCBorders(true);
                    return;
                }
                col = c.GetNextCol();
            }
        }

        // check row group, row and cell has borders.
        for rowGroup in groups.mRowGroups {
            if FrameHasBorder(rowGroup.0) {
                self.SetHasBCBorders(true);
                return;
            }

            let mut row = rowGroup.GetFirstRow();
            while let Some(r) = row {
                if FrameHasBorder(r.0) {
                    self.SetHasBCBorders(true);
                    return;
                }

                let mut cell = r.GetFirstCell();
                while let Some(c) = cell {
                    if FrameHasBorder(c.0) {
                        self.SetHasBCBorders(true);
                        return;
                    }
                    cell = c.GetNextCell();
                }
                row = r.GetNextRow();
            }
        }

        self.SetHasBCBorders(false);
    }

    // table paint code is concerned primarily with borders and bg color
    // SEC: TODO: adjust the rect for captions
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::BuildDisplayList
    // NOT PORTED: display lists (nsTableFrame.cpp lines 1128-1177 at GECKO_REV).

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetLogicalSkipSides
    // NOT PORTED: nothing calls it (nsIFrame::GetSkipSides is a stub that skips no side).
    pub fn GetLogicalSkipSides(&self) -> LogicalSides {
        let mut skip = LogicalSides::new__WritingMode(self.GetWritingMode());
        if self.StyleBorder().mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone {
            return skip;
        }

        // frame attribute was accounted for in nsHTMLTableElement::MapTableBorderInto
        // account for pagination
        if self.GetPrevInFlow().is_some() {
            skip += LogicalSide::BStart;
        }
        if self.GetNextInFlow().is_some() {
            skip += LogicalSide::BEnd;
        }
        skip
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::SetColumnDimensions
    pub fn SetColumnDimensions(&self, aBSize: nscoord, aWM: WritingMode, aBorderPadding: LogicalMargin, aContainerSize: nsSize) {
        let colBSize = aBSize - (aBorderPadding.BStartEnd(aWM) + self.GetRowSpacing__int32_t(-1) + self.GetRowSpacing__int32_t(self.GetRowCount()));
        let mut colIdx = 0;
        let mut colGroupOrigin = LogicalPoint::new(aWM, aBorderPadding.IStart(aWM) + self.GetColSpacing__int32_t(-1), aBorderPadding.BStart(aWM) + self.GetRowSpacing__int32_t(-1));
        let fif = self.FirstInFlow_();
        for colGroupFrame in self.OrderedGroups().mColGroups {
            let colGroupFrame = colGroupFrame.0;
            MOZ_ASSERT!(colGroupFrame.IsTableColGroupFrame());
            // first we need to figure out the size of the colgroup
            let groupFirstCol = colIdx;
            let mut colGroupISize = 0;
            let mut colSpacing = 0;
            let columnList = colGroupFrame.GetChildList(FrameChildListID::Principal);
            for colFrame in columnList {
                if StyleDisplay::TableColumn == colFrame.StyleDisplay().mDisplay() {
                    NS_ASSERTION!(colIdx < self.GetColCount(), "invalid number of columns");
                    colSpacing = self.GetColSpacing__int32_t(colIdx);
                    colGroupISize += fif.GetColumnISizeFromFirstInFlow(colIdx) + colSpacing;
                    colIdx += 1;
                }
            }
            if colGroupISize != 0 {
                colGroupISize -= colSpacing;
            }

            let colGroupRect = LogicalRect::new(aWM, colGroupOrigin.I(aWM), colGroupOrigin.B(aWM), colGroupISize, colBSize);
            colGroupFrame.SetRect__WritingMode_LogicalRect_nsSize(aWM, colGroupRect, aContainerSize);
            let colGroupSize = colGroupFrame.GetSize();

            // then we can place the columns correctly within the group
            colIdx = groupFirstCol;
            let mut colOrigin = LogicalPoint::new__WritingMode(aWM);
            for colFrame in columnList {
                if StyleDisplay::TableColumn == colFrame.StyleDisplay().mDisplay() {
                    let colISize = fif.GetColumnISizeFromFirstInFlow(colIdx);
                    let colRect = LogicalRect::new(aWM, colOrigin.I(aWM), colOrigin.B(aWM), colISize, colBSize);
                    colFrame.SetRect__WritingMode_LogicalRect_nsSize(aWM, colRect, colGroupSize);
                    colSpacing = self.GetColSpacing__int32_t(colIdx);
                    *colOrigin.I_mut(aWM) += colISize + colSpacing;
                    colIdx += 1;
                }
            }

            *colGroupOrigin.I_mut(aWM) += colGroupISize + colSpacing;
        }
    }

    // SEC: TODO need to worry about continuing frames prev/next in flow for
    // splitting across pages.

    // XXX this could be made more general to handle row modifications that change
    // the table bsize, but first we need to scrutinize every Invalidate
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::ProcessRowInserted
    pub fn ProcessRowInserted(&self, aNewBSize: nscoord) {
        self.SetRowInserted(false); // reset the bit that got us here
        let rowGroups = self.OrderedRowGroups();
        // find the row group containing the inserted row
        for rgIdx in 0..rowGroups.len() {
            let rgFrame = rowGroups[rgIdx];
            // find the row that was inserted first
            for childFrame in rgFrame.GetChildList(FrameChildListID::Principal) {
                let rowFrame = nsTableRowFrame::do_QueryFrame(childFrame);
                if let Some(rowFrame) = rowFrame {
                    if rowFrame.IsFirstInserted() {
                        rowFrame.SetFirstInserted(false);
                        // damage the table from the 1st row inserted to the end of the table
                        // nsIFrame::InvalidateFrame(); NOT PORTED: invalidation
                        // XXXbz didn't we do this up front?  Why do we need to do it again?
                        self.SetRowInserted(false);
                        return; // found it, so leave
                    }
                }
            }
        }
    }

    /* virtual */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::MarkIntrinsicISizesDirty
    pub fn MarkIntrinsicISizesDirty(&self) {
        let tls = self.LayoutStrategy();
        let Some(tls) = tls else {
            // This is a FrameNeedsReflow() from nsBlockFrame::RemoveFrame()
            // walking up the ancestor chain in a table next-in-flow.  In this case
            // our original first-in-flow (which owns the TableLayoutStrategy) has
            // already been destroyed and unhooked from the flow chain and thusly
            // LayoutStrategy() returns null.  All the frames in the flow will be
            // destroyed so no need to mark anything dirty here.  See bug 595758.
            return;
        };
        tls.MarkIntrinsicISizesDirty();

        // XXXldb Call SetBCDamageArea?

        self.nsContainerFrame_MarkIntrinsicISizesDirty();
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::IntrinsicISize
    pub fn IntrinsicISize(&self, aInput: &IntrinsicSizeInput, aType: IntrinsicISizeType) -> nscoord {
        if self.NeedToCalcBCBorders() {
            self.CalcBCBorders();
        }

        if aType == IntrinsicISizeType::MinISize { self.LayoutStrategy().unwrap().GetMinISize(aInput.mContext) } else { self.LayoutStrategy().unwrap().GetPrefISize(aInput.mContext, false) }
    }

    /* virtual */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::IntrinsicISizeOffsets
    pub fn IntrinsicISizeOffsets(&self, aPercentageBasis: nscoord) -> nsIFrame_IntrinsicSizeOffsetData {
        let mut result = self.nsIFrame_IntrinsicISizeOffsets(aPercentageBasis);

        result.margin = 0;

        if self.IsBorderCollapse() {
            result.padding = 0;

            let wm = self.GetWritingMode();
            let outerBC = self.GetOuterBCBorder(wm);
            result.border = outerBC.IStartEnd(wm);
        }

        result
    }

    /* virtual */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::ComputeSize
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
        // Only table wrapper calls this method, and it should use our writing mode.
        MOZ_ASSERT!(aWM == self.GetWritingMode(), "aWM should be the same as our writing mode!");

        let mut result = self.nsIFrame_ComputeSize(aSizingInput, aWM, aCBSize, aAvailableISize, aMargin, aBorderPadding, aSizeOverrides, aFlags);

        // If our containing block wants to override inner table frame's inline-size
        // (e.g. when resolving flex base size), don't enforce the min inline-size
        // later in this method.
        if aSizeOverrides.mApplyOverridesVerbatim && aSizeOverrides.mStyleISize.as_ref().map_or(false, |s| s.IsLengthPercentage()) {
            return result;
        }

        // If we're a container for font size inflation, then shrink
        // wrapping inside of us should not apply font size inflation.
        // (AutoMaybeDisableFontInflation an(this): NOT PORTED, font size inflation is off.)

        // Tables never shrink below their min inline-size.
        let input = IntrinsicSizeInput::new(aSizingInput.mRenderingContext, Some(aCBSize), None);
        let minISize = self.GetMinISize(&input);
        if minISize > result.mLogicalSize.ISize(aWM) {
            *result.mLogicalSize.ISize_mut(aWM) = minISize;
        }

        result
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::TableShrinkISizeToFit
    pub fn TableShrinkISizeToFit(&self, aRenderingContext: Option<gfxContext>, aISizeInCB: nscoord) -> nscoord {
        // If we're a container for font size inflation, then shrink
        // wrapping inside of us should not apply font size inflation.
        // (AutoMaybeDisableFontInflation an(this): NOT PORTED, font size inflation is off.)

        let result;
        let input = IntrinsicSizeInput::new(aRenderingContext, None, None);
        let minISize = self.GetMinISize(&input);
        if minISize > aISizeInCB {
            result = minISize;
        } else {
            // Tables shrink inline-size to fit with a slightly different algorithm
            // from the one they use for their intrinsic isize (the difference
            // relates to handling of percentage isizes on columns).  So this
            // function differs from nsIFrame::ShrinkISizeToFit by only the
            // following line.
            // Since we've already called GetMinISize, we don't need to do any
            // of the other stuff GetPrefISize does.
            let prefISize = self.LayoutStrategy().unwrap().GetPrefISize(aRenderingContext, true);
            if prefISize > aISizeInCB {
                result = aISizeInCB;
            } else {
                result = prefISize;
            }
        }
        result
    }

    /* virtual */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::ComputeAutoSize
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
        // Tables always shrink-wrap.
        let cbBased = aAvailableISize - aMargin.ISize(aWM) - aBorderPadding.ISize(aWM);
        LogicalSize::new(aWM, self.TableShrinkISizeToFit(aSizingInput.mRenderingContext, cbBased), NS_UNCONSTRAINEDSIZE)
    }

    // Return true if aParentReflowInput.frame or any of its ancestors within
    // the containing table have non-auto bsize. (e.g. pct or fixed bsize)
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::AncestorsHaveStyleBSize
    pub fn AncestorsHaveStyleBSize(aParentReflowInput: &ReflowInput) -> bool {
        let wm = aParentReflowInput.GetWritingMode();
        let mut rs: Option<&ReflowInput> = Some(aParentReflowInput);
        while let Some(r) = rs {
            let frameType = r.mFrame.Type();
            if LayoutFrameType::TableCell == frameType || LayoutFrameType::TableRow == frameType || LayoutFrameType::TableRowGroup == frameType {
                let bsize = r.mStylePosition.BSize(wm, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(r.mFrame, std::ptr::null_mut()));
                // calc() with both lengths and percentages treated like 'auto' on
                // internal table elements
                if !bsize.IsAuto() && !bsize.HasLengthAndPercentage() {
                    return true;
                }
            } else if LayoutFrameType::Table == frameType {
                // we reached the containing table, so always return
                return !r.mStylePosition.BSize(wm, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(r.mFrame, std::ptr::null_mut())).IsAuto();
            }
            rs = r.mParentReflowInput;
        }
        false
    }

    // See if a special block-size reflow needs to occur and if so,
    // call RequestSpecialBSizeReflow
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::CheckRequestSpecialBSizeReflow
    pub fn CheckRequestSpecialBSizeReflow(aReflowInput: &ReflowInput) {
        NS_ASSERTION!(aReflowInput.mFrame.IsTableCellFrame() || aReflowInput.mFrame.IsTableRowFrame() || aReflowInput.mFrame.IsTableRowGroupFrame() || aReflowInput.mFrame.IsTableFrame(), "unexpected frame type");
        let wm = aReflowInput.GetWritingMode();
        if aReflowInput.mFrame.GetPrevInFlow().is_none()  // 1st in flow
            && (NS_UNCONSTRAINEDSIZE == aReflowInput.ComputedBSize()  // no computed bsize
                || 0 == aReflowInput.ComputedBSize())
            && aReflowInput.mStylePosition.BSize(wm, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(aReflowInput.mFrame, std::ptr::null_mut())).ConvertsToPercentage()  // pct bsize
            && nsTableFrame::AncestorsHaveStyleBSize(aReflowInput.mParentReflowInput.unwrap())
        {
            nsTableFrame::RequestSpecialBSizeReflow(aReflowInput);
        }
    }

    // Notify the frame and its ancestors (up to the containing table) that a
    // special bsize reflow will occur. During a special bsize reflow, a table, row
    // group, row, or cell returns the last size it was reflowed at. However, the
    // table may change the bsize of row groups, rows, cells in
    // DistributeBSizeToRows after. And the row group can change the bsize of rows,
    // cells in CalculateRowBSizes.
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::RequestSpecialBSizeReflow
    pub fn RequestSpecialBSizeReflow(aReflowInput: &ReflowInput) {
        // notify the frame and its ancestors of the special reflow, stopping at the
        // containing table
        let mut rs: Option<&ReflowInput> = Some(aReflowInput);
        while let Some(r) = rs {
            let frameType = r.mFrame.Type();
            NS_ASSERTION!(LayoutFrameType::TableCell == frameType || LayoutFrameType::TableRow == frameType || LayoutFrameType::TableRowGroup == frameType || LayoutFrameType::Table == frameType, "unexpected frame type");

            r.mFrame.AddStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE);
            if LayoutFrameType::Table == frameType {
                NS_ASSERTION!(!std::ptr::eq(r, aReflowInput), "should not request special bsize reflow for table");
                // always stop when we reach a table
                break;
            }
            rs = r.mParentReflowInput;
        }
    }
}

/******************************************************************************************
 * Before reflow, intrinsic inline-size calculation is done using GetMinISize
 * and GetPrefISize.  This used to be known as pass 1 reflow.
 *
 * After the intrinsic isize calculation, the table determines the
 * column widths using BalanceColumnISizes() and
 * then reflows each child again with a constrained avail isize. This reflow is
 * referred to as the pass 2 reflow.
 *
 * A special bsize reflow (pass 3 reflow) can occur during an initial or resize
 * reflow if (a) a row group, row, cell, or a frame inside a cell has a percent
 * bsize but no computed bsize or (b) in paginated mode, a table has a bsize.
 * (a) supports percent nested tables contained inside cells whose bsizes aren't
 * known until after the pass 2 reflow. (b) is necessary because the table
 * cannot split until after the pass 2 reflow. The mechanics of the special
 * bsize reflow (variety a) are as follows:
 *
 * 1) Each table related frame (table, row group, row, cell) implements
 *    NeedsSpecialReflow() to indicate that it should get the reflow. It does
 *    this when it has a percent bsize but no computed bsize by calling
 *    CheckRequestSpecialBSizeReflow(). This method calls
 *    RequestSpecialBSizeReflow() which calls SetNeedSpecialReflow() on its
 *    ancestors until it reaches the containing table and calls
 *    SetNeedToInitiateSpecialReflow() on it. For percent bsize frames inside
 *    cells, during DidReflow(), the cell's NotifyPercentBSize() is called
 *    (the cell is the reflow input's mPercentBSizeObserver in this case).
 *    NotifyPercentBSize() calls RequestSpecialBSizeReflow().
 *
 * XXX (jfkthame) This comment appears to be out of date; it refers to
 * methods/flags that are no longer present in the code.
 *
 * 2) After the pass 2 reflow, if the table's NeedToInitiateSpecialReflow(true)
 *    was called, it will do the special bsize reflow, setting the reflow
 *    input's mFlags.mSpecialBSizeReflow to true and mSpecialHeightInitiator to
 *    itself. It won't do this if IsPrematureSpecialHeightReflow() returns true
 *    because in that case another special bsize reflow will be coming along
 *    with the containing table as the mSpecialHeightInitiator. It is only
 *    relevant to do the reflow when the mSpecialHeightInitiator is the
 *    containing table, because if it is a remote ancestor, then appropriate
 *    bsizes will not be known.
 *
 * 3) Since the bsizes of the table, row groups, rows, and cells was determined
 *    during the pass 2 reflow, they return their last desired sizes during the
 *    special bsize reflow. The reflow only permits percent bsize frames inside
 *    the cells to resize based on the cells bsize and that bsize was
 *    determined during the pass 2 reflow.
 *
 * So, in the case of deeply nested tables, all of the tables that were told to
 * initiate a special reflow will do so, but if a table is already in a special
 * reflow, it won't inititate the reflow until the current initiator is its
 * containing table. Since these reflows are only received by frames that need
 * them and they don't cause any rebalancing of tables, the extra overhead is
 * minimal.
 *
 * The type of special reflow that occurs during printing (variety b) follows
 * the same mechanism except that all frames will receive the reflow even if
 * they don't really need them.
 *
 * Open issues with the special bsize reflow:
 *
 * 1) At some point there should be 2 kinds of special bsize reflows because (a)
 *    and (b) above are really quite different. This would avoid unnecessary
 *    reflows during printing.
 *
 * 2) When a cell contains frames whose percent bsizes > 100%, there is data
 *    loss (see bug 115245). However, this can also occur if a cell has a fixed
 *    bsize and there is no special bsize reflow.
 *
 * XXXldb Special bsize reflow should really be its own method, not
 * part of nsIFrame::Reflow.  It should then call nsIFrame::Reflow on
 * the contents of the cells to do the necessary block-axis resizing.
 *
 ******************************************************************************************/

impl nsTableFrame {
    /* Layout the entire inner table. */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::Reflow
    #[allow(invalid_reference_casting)]
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        // DO_GLOBAL_REFLOW_COUNT("nsTableFrame"): NOT PORTED, reflow counting.
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");
        MOZ_ASSERT!(!self.HasAnyStateBits(NS_FRAME_OUT_OF_FLOW), "The nsTableWrapperFrame should be the out-of-flow if needed");

        let wm = aReflowInput.GetWritingMode();
        MOZ_ASSERT!(aReflowInput.ComputedLogicalMargin(wm).IsAllZero(), "Only nsTableWrapperFrame can have margins!");

        let isPaginated = aPresContext.IsPaginated();

        if self.GetPrevInFlow().is_none() && self.mTableLayoutStrategy().is_none() {
            NS_ERROR!("strategy should have been created in Init");
            return;
        }

        // see if collapsing borders need to be calculated
        if self.GetPrevInFlow().is_none() && self.IsBorderCollapse() && self.NeedToCalcBCBorders() {
            self.CalcBCBorders();
        }

        // Check for an overflow list, and append any row group frames being pushed
        self.MoveOverflowToChildList();

        let mut haveCalledCalcDesiredBSize = false;
        let mut borderPadding = aReflowInput.ComputedLogicalBorderPadding(wm);
        borderPadding.ApplySkipSides(self.PreReflowBlockLevelLogicalSkipSides());
        let mut lastChildReflowed: Option<nsIFrame> = None;
        let containerSize = aReflowInput.ComputedSizeAsContainerIfConstrained();

        // The tentative width is the width we assumed for the table when the child
        // frames were positioned (which only matters in vertical-rl mode, because
        // they're positioned relative to the right-hand edge). Then, after reflowing
        // the kids, we can check whether the table ends up with a different width
        // than this tentative value (either because it was unconstrained, so we used
        // zero, or because it was enlarged by the child frames), we make the
        // necessary positioning adjustments along the x-axis.
        let mut tentativeContainerWidth = 0;
        let mut mayAdjustXForAllChildren = false;
        let mut groups = self.OrderedGroups();

        // Reflow the entire table (pass 2 and possibly pass 3). This phase is
        // necessary during a constrained initial reflow and other reflows which
        // require either a strategy init or balance. This isn't done during an
        // unconstrained reflow, because it will occur later when the parent reflows
        // with a constrained isize.
        if self.IsSubtreeDirty() || aReflowInput.ShouldReflowAllKids() || self.IsGeometryDirty() || isPaginated || aReflowInput.IsBResize() || self.NeedToCollapse() {
            if aReflowInput.ComputedBSize() != NS_UNCONSTRAINEDSIZE
                // Also check IsBResize(), to handle the first Reflow preceding a
                // special bsize Reflow, when we've already had a special bsize
                // Reflow (where ComputedBSize() would not be
                // NS_UNCONSTRAINEDSIZE, but without a style change in between).
                || aReflowInput.IsBResize()
            {
                // XXX Eventually, we should modify DistributeBSizeToRows to use
                // nsTableRowFrame::GetInitialBSize instead of nsIFrame::BSize().
                // That way, it will make its calculations based on internal table
                // frame bsizes as they are before they ever had any extra bsize
                // distributed to them.  In the meantime, this reflows all the
                // internal table frames, which restores them to their state before
                // DistributeBSizeToRows was called.
                self.SetGeometryDirty();
            }

            let mut needToInitiateSpecialReflow = false;
            if isPaginated {
                // see if an extra reflow will be necessary in pagination mode
                // when there is a specified table bsize
                if self.GetPrevInFlow().is_none() && NS_UNCONSTRAINEDSIZE != aReflowInput.AvailableBSize() {
                    let tableSpecifiedBSize = self.CalcBorderBoxBSize(aReflowInput, borderPadding, NS_UNCONSTRAINEDSIZE);
                    if tableSpecifiedBSize != NS_UNCONSTRAINEDSIZE && tableSpecifiedBSize > 0 {
                        needToInitiateSpecialReflow = true;
                    }
                }
            } else {
                needToInitiateSpecialReflow = self.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE);
            }

            NS_ASSERTION!(!aReflowInput.mFlags.mSpecialBSizeReflow, "Shouldn't be in special bsize reflow here!");

            let firstReflowMode = if needToInitiateSpecialReflow { TableReflowMode::Measuring } else { TableReflowMode::Final };
            self.ReflowTable(aDesiredSize, aReflowInput, borderPadding, firstReflowMode, &mut groups, &mut lastChildReflowed, aStatus);

            // When in vertical-rl mode, there may be two kinds of scenarios in which
            // the positioning of all the children need to be adjusted along the x-axis
            // because the width we assumed for the table when the child frames were
            // being positioned(i.e. tentative width) may be different from the final
            // width for the table:
            // 1. If the computed width for the table is unconstrained, a dummy zero
            //    width was assumed as the tentative width to begin with.
            // 2. If the child frames enlarge the width for the table, the final width
            //    becomes larger than the tentative one.
            // Let's record the tentative width here, if later the final width turns out
            // to be different from this tentative one, it means one of the above
            // scenarios happens, then we adjust positioning of all the children.
            // Note that vertical-lr, unlike vertical-rl, doesn't need to take special
            // care of this situation, because they're positioned relative to the
            // left-hand edge.
            if wm.IsVerticalRL() {
                tentativeContainerWidth = containerSize.width;
                mayAdjustXForAllChildren = true;
            }

            // reevaluate special bsize reflow conditions
            if self.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE) {
                needToInitiateSpecialReflow = true;
            }

            // XXXldb Are all these conditions correct?
            if needToInitiateSpecialReflow && aStatus.IsComplete() {
                // XXXldb Do we need to set the IsBResize flag on any reflow inputs?

                // ADAPTER: `const_cast<ReflowInput&>(aReflowInput)`, as elsewhere in the crate.
                let mutable_rs: &mut ReflowInput = unsafe { &mut *(aReflowInput as *const ReflowInput as *mut ReflowInput) };

                // distribute extra block-direction space to rows
                *aDesiredSize.BSize_mut(wm) = self.CalcDesiredBSize(aReflowInput, borderPadding, aStatus);
                haveCalledCalcDesiredBSize = true;

                mutable_rs.mFlags.mSpecialBSizeReflow = true;

                self.ReflowTable(aDesiredSize, aReflowInput, borderPadding, TableReflowMode::Final, &mut groups, &mut lastChildReflowed, aStatus);

                mutable_rs.mFlags.mSpecialBSizeReflow = false;
            }
        }

        if aStatus.IsIncomplete() && aReflowInput.mStyleBorder.mBoxDecorationBreak() == StyleBoxDecorationBreak::Slice {
            *borderPadding.BEnd_mut(wm) = 0;
        }

        *aDesiredSize.ISize_mut(wm) = aReflowInput.ComputedISize() + borderPadding.IStartEnd(wm);
        if !haveCalledCalcDesiredBSize {
            *aDesiredSize.BSize_mut(wm) = self.CalcDesiredBSize(aReflowInput, borderPadding, aStatus);
        } else if lastChildReflowed.is_some() && aStatus.IsIncomplete() {
            // If there is an incomplete child, then set the desired block-size to
            // include it but not the next one.
            *aDesiredSize.BSize_mut(wm) = borderPadding.BEnd(wm) + lastChildReflowed.unwrap().GetLogicalNormalRect(wm, containerSize).BEnd(wm);
        }
        if self.IsRowInserted() {
            self.ProcessRowInserted(aDesiredSize.BSize(wm));
        }

        // For more information on the reason for what we should do this, refer to the
        // code which defines and evaluates the variables xAdjustmentForAllKids and
        // tentativeContainerWidth in the previous part in this function.
        if mayAdjustXForAllChildren {
            let xAdjustmentForAllKids = aDesiredSize.Width() - tentativeContainerWidth;
            if 0 != xAdjustmentForAllKids {
                for kid in self.mFrames().as_static() {
                    kid.MovePositionBy(nsPoint::new(xAdjustmentForAllKids, 0));
                }
            }
        }

        self.SetColumnDimensions(aDesiredSize.BSize(wm), wm, borderPadding, aDesiredSize.PhysicalSize());
        NS_WARNING_ASSERTION!(NS_UNCONSTRAINEDSIZE != aReflowInput.AvailableISize(), "reflow branch removed unconstrained available isizes");
        if self.NeedToCollapse() {
            // This code and the code it depends on assumes that all row groups
            // and rows have just been reflowed (i.e., it makes adjustments to
            // their rects that are not idempotent).  Thus the reflow code
            // checks NeedToCollapse() to ensure this is true.
            self.AdjustForCollapsingRowsCols(aDesiredSize, wm, &groups.mRowGroups, borderPadding);
        }

        // Calculate the overflow area contribution from our children. We couldn't
        // do this on the fly during ReflowChildren(), because in vertical-rl mode
        // with unconstrained width, we weren't placing them in their final positions
        // until the fixupKidPositions loop just above.
        for kid in &groups.mRowGroups {
            self.ConsiderChildOverflow(&mut aDesiredSize.mOverflowAreas, kid.0, OverflowAreaUnionFlags::None);
        }

        // If there are any relatively-positioned table parts, we need to reflow their
        // absolutely-positioned descendants now that their dimensions are final.
        self.FixupPositionedTableParts(aPresContext, aDesiredSize, aReflowInput);

        // make sure the table overflow area does include the table rect.
        let tableRect = nsRect::new(0, 0, aDesiredSize.Width(), aDesiredSize.Height());

        aDesiredSize.mOverflowAreas.UnionAllWith(tableRect);

        self.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(aDesiredSize, None);
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::FixupPositionedTableParts
    // NOT PORTED: positioned table parts (nsTableFrame.cpp lines 1775-1832 at GECKO_REV); without
    // absolute positioning no table part is registered, so the body would return at once.
    pub fn FixupPositionedTableParts(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput) {}

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::ComputeCustomOverflow
    pub fn ComputeCustomOverflow(&self, aOverflowAreas: &mut OverflowAreas) -> bool {
        // return nsContainerFrame::ComputeCustomOverflow(aOverflowAreas);
        // NOT PORTED: nsIFrame::ComputeCustomOverflow (overflow clipping and text-overflow are not
        // translated); nothing calls this override.
        false
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::ReflowTable
    pub fn ReflowTable(&self, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aBorderPadding: LogicalMargin, aReflowMode: TableReflowMode, aGroups: &mut nsTableFrame_Groups, aLastChildReflowed: &mut Option<nsIFrame>, aStatus: &mut nsReflowStatus) {
        *aLastChildReflowed = None;

        if self.GetPrevInFlow().is_none() {
            self.mTableLayoutStrategy().as_mut().unwrap().ComputeColumnISizes(aReflowInput);
        }

        let mut reflowInput = TableReflowInput::new(aReflowInput, aBorderPadding, aReflowMode);
        self.ReflowChildren(&mut reflowInput, aStatus, aGroups, aLastChildReflowed, &mut aDesiredSize.mOverflowAreas);
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::PushChildrenToOverflow
    pub fn PushChildrenToOverflow(&self, aRowGroups: &nsTableFrame_RowGroupArray, aPushFrom: usize) {
        MOZ_ASSERT!(aPushFrom > 0, "pushing first child");

        // Extract the frames from the array into a frame list.
        let mut frames = nsFrameList::default();
        for childX in aPushFrom..aRowGroups.len() {
            let rgFrame = aRowGroups[childX];
            if !rgFrame.IsRepeatable() {
                self.mFrames().RemoveFrame(rgFrame.0);
                frames.AppendFrame(None, rgFrame.0);
            }
        }

        if frames.IsEmpty() {
            return;
        }

        // Add the frames to our overflow list.
        self.SetOverflowFrames(frames);
    }

    // collapsing row groups, rows, col groups and cols are accounted for after both
    // passes of reflow so that it has no effect on the calculations of reflow.
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::AdjustForCollapsingRowsCols
    pub fn AdjustForCollapsingRowsCols(&self, aDesiredSize: &mut ReflowOutput, aWM: WritingMode, aRowGroups: &Vec<nsTableRowGroupFrame>, aBorderPadding: LogicalMargin) {
        let mut bTotalOffset = 0; // total offset among all rows in all row groups

        // reset the bit, it will be set again if row/rowgroup or col/colgroup are
        // collapsed
        self.SetNeedToCollapse(false);

        // collapse the rows and/or row groups as necessary
        let firstInFlow = self.FirstInFlow_();
        let iSize = firstInFlow.GetCollapsedISize(aWM, aBorderPadding);
        let rgISize = iSize - self.GetColSpacing__int32_t(-1) - self.GetColSpacing__int32_t(self.GetColCount());
        // Walk the list of children
        for rg in aRowGroups {
            bTotalOffset += rg.CollapseRowGroupIfNecessary(bTotalOffset, rgISize, aWM);
        }

        *aDesiredSize.BSize_mut(aWM) -= bTotalOffset;
        *aDesiredSize.ISize_mut(aWM) = iSize;
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetCollapsedISize
    pub fn GetCollapsedISize(&self, aWM: WritingMode, aBorderPadding: LogicalMargin) -> nscoord {
        NS_ASSERTION!(self.GetPrevInFlow().is_none(), "GetCollapsedISize called on next in flow");
        let mut iSize = self.GetColSpacing__int32_t(self.GetColCount());
        iSize += aBorderPadding.IStartEnd(aWM);
        for colFrame in self.mColFrames().clone() {
            let collapseGroup = colFrame.GetParent().unwrap().StyleVisibility().mVisible() == StyleVisibility::Collapse;
            let colVis = colFrame.StyleVisibility();
            let collapseCol = StyleVisibility::Collapse == colVis.mVisible();
            if collapseGroup || collapseCol {
                self.SetNeedToCollapse(true);
                continue;
            }
            let colISize = colFrame.GetFinalISize();
            iSize += colISize;
            if self.ColumnHasCellSpacingBefore(colFrame.GetColIndex()) {
                iSize += self.GetColSpacing__int32_t(colFrame.GetColIndex() - 1);
            }
        }
        iSize
    }

    /* virtual */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::DidSetComputedStyle
    // NOT PORTED: nothing calls it (style changes need a restyle, which does not exist here); kept
    // for the strategy switch.
    pub fn DidSetComputedStyle(&self, aOldComputedStyle: Option<ServoArc<ComputedValues>>) {
        // nsContainerFrame::DidSetComputedStyle(aOldComputedStyle);

        if aOldComputedStyle.is_none() {
            // avoid this on init
            return;
        }

        // if (IsBorderCollapse() && BCRecalcNeeded(aOldComputedStyle, Style())) { SetFullBCDamageArea(); }
        // NOT PORTED: BCRecalcNeeded (style difference of the border structs, nsTableFrame.cpp lines
        // 4112-4138 at GECKO_REV).

        // avoid this on init or nextinflow
        if self.mTableLayoutStrategy().is_none() || self.GetPrevInFlow().is_some() {
            return;
        }

        let isAuto = self.IsAutoLayout();
        if isAuto != (self.LayoutStrategy().unwrap().GetType() == nsITableLayoutStrategy_Type::Auto) {
            if isAuto {
                *self.mTableLayoutStrategy() = Some(Box::new(BasicTableLayoutStrategy::new(*self)));
            } else {
                *self.mTableLayoutStrategy() = Some(Box::new(FixedTableLayoutStrategy::new(*self)));
            }
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::AppendFrames
    pub fn AppendFrames(&self, aListID: FrameChildListID, aFrameList: nsFrameList) {
        NS_ASSERTION!(aListID == FrameChildListID::Principal, "unexpected child list");

        let firstNew = aFrameList.FirstChild();
        if firstNew.is_none() {
            return;
        }

        self.DrainSelfOverflowList(); // ensure the last frame is in mFrames
        self.nsContainerFrame_AppendFrames(aListID, aFrameList);

        let newFrames = nsFrameList_Slice::new(firstNew, None);
        self.InsertColGroups(self.GetRealColEnd(), newFrames.clone());
        self.InsertRowGroups(newFrames);

        // NOT PORTED: DEBUG_TABLE_CELLMAP dumps.
        PresShell_FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
        self.SetGeometryDirty();
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::InsertFrames
    pub fn InsertFrames(&self, aListID: FrameChildListID, aPrevFrame: Option<nsIFrame>, aPrevFrameLine: Option<&nsLineList_iterator>, aFrameList: nsFrameList) {
        NS_ASSERTION!(aPrevFrame.is_none() || aPrevFrame.unwrap().GetParent() == Some(self.0), "inserting after sibling frame with different parent");

        let firstNew = aFrameList.FirstChild();
        if firstNew.is_none() {
            return;
        }

        self.DrainSelfOverflowList();

        // Capture the frame that will follow the inserted frames, before insertion
        // changes the sibling chain. Skip the synthetic colgroup as sentinel because
        // InsertColGroups -> AddColsToTable -> InsertCol can destroy it.
        let mut afterInserted = match aPrevFrame {
            Some(p) => p.GetNextSibling(),
            None => self.mFrames().FirstChild(),
        };
        if let Some(syn) = *self.mSyntheticColGroup() {
            if afterInserted == Some(syn.0) {
                afterInserted = afterInserted.unwrap().GetNextSibling();
            }
        }
        self.nsContainerFrame_InsertFrames(aListID, aPrevFrame, aPrevFrameLine, aFrameList);

        let newFrames = nsFrameList_Slice::new(firstNew, afterInserted);
        self.InsertColGroups(GetColStartAfter(aPrevFrame), newFrames.clone());
        self.InsertRowGroups(newFrames);

        PresShell_FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
        self.SetGeometryDirty();
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::DoRemoveFrame
    pub fn DoRemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        NS_ASSERTION!(aListID == FrameChildListID::Principal, "unexpected child list");
        let cellMap = self.GetCellMap();

        if let Some(cgFrame) = nsTableColGroupFrame::do_QueryFrame(aOldFrame) {
            let nextSibling = aOldFrame.GetNextSibling();
            let colsStart = cgFrame.GetStartColumnIndex();
            let colCount = cgFrame.GetColCount();
            if Some(cgFrame) == *self.mSyntheticColGroup() {
                *self.mSyntheticColGroup() = None;
            }
            self.mFrames().DestroyFrame(aContext, aOldFrame);
            nsTableColGroupFrame::ResetColIndices(nextSibling, self.GetSyntheticColGroup(), colsStart, None);
            // NB: Frames in mColFrames might be dead already.
            self.mColFrames().drain(colsStart as usize..(colsStart + colCount) as usize);
            if self.IsBorderCollapse() {
                self.SetFullBCDamageArea();
            }

            if !self.mColFrames().is_empty() && self.mColFrames().last().unwrap().GetColType() == eColAnonymousCell {
                let numAnonymousColsToAdd = self.GetColCount() - self.mColFrames().len() as i32;
                if numAnonymousColsToAdd > 0 {
                    self.AppendAnonymousColFrames__int32_t(numAnonymousColsToAdd);
                }
            } else if let Some(cellMap) = cellMap {
                cellMap.RemoveColsAtEnd();
                self.MatchCellMapToColCache(cellMap);
            }
            #[cfg(debug_assertions)]
            self.VerifyColFrames();
            return;
        }

        let rgFrame = nsTableRowGroupFrame::do_QueryFrame(aOldFrame);
        if let (Some(cellMap), Some(rgFrame)) = (self.GetCellMap(), rgFrame) {
            // remove the row group from the cell map
            cellMap.RemoveGroupCellMap(rgFrame);
        }

        self.mFrames().DestroyFrame(aContext, aOldFrame);

        // the removal of a row group changes the cellmap, the columns might change
        if let (Some(cellMap), Some(rgFrame)) = (cellMap, rgFrame) {
            cellMap.Synchronize(*self);
            // Create an empty slice
            self.ResetRowIndices(nsFrameList_Slice::new(None, None));
            let mut damageArea = TableArea::default();
            cellMap.RebuildConsideringCells(std::ptr::null_mut(), None, 0, 0, false, &mut damageArea);

            self.FirstInFlow_().MatchCellMapToColCache(cellMap);
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::RemoveFrame
    pub fn RemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        let mut aOldFrame = Some(aOldFrame);
        let mut lastParent: Option<nsTableFrame> = None;
        while let Some(old) = aOldFrame {
            let oldFrameNextContinuation = old.GetNextContinuation();
            let parent = nsTableFrame::from_frame(old.GetParent().unwrap());
            if Some(parent) != lastParent {
                parent.DrainSelfOverflowList();
            }
            parent.DoRemoveFrame(aContext, aListID, old);
            aOldFrame = oldFrameNextContinuation;
            if Some(parent) != lastParent {
                // for now, just bail and recalc all of the collapsing borders
                // as the cellmap changes we need to recalc
                if parent.IsBorderCollapse() {
                    parent.SetFullBCDamageArea();
                }
                parent.SetGeometryDirty();
                PresShell_FrameNeedsReflow(parent.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
                lastParent = Some(parent);
            }
        }
        // NOT PORTED: DEBUG_TABLE_CELLMAP dumps.
    }

    /* virtual */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetUsedBorder
    pub fn GetUsedBorder(&self) -> nsMargin {
        if !self.IsBorderCollapse() {
            return self.nsIFrame_GetUsedBorder();
        }

        let wm = self.GetWritingMode();
        self.GetOuterBCBorder(wm).GetPhysicalMargin(wm)
    }

    /* virtual */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetUsedPadding
    pub fn GetUsedPadding(&self) -> nsMargin {
        if !self.IsBorderCollapse() {
            return self.nsIFrame_GetUsedPadding();
        }

        nsMargin::new(0, 0, 0, 0)
    }

    /* virtual */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetUsedMargin
    pub fn GetUsedMargin(&self) -> nsMargin {
        // The margin is inherited to the table wrapper frame via
        // the ::-moz-table-wrapper rule in ua.css.
        nsMargin::new(0, 0, 0, 0)
    }

    // TODO(TYLin, dshin): This ideally should be set only in first-in-flow.
    // However, the current implementation of border-collapsed table does not
    // handle continuation gracefully. One concrete issue is shown in bug 1881157
    // comment 3. It is also unclear if the damage area, current included in this
    // property, should be stored separately per-continuation.
    // (NS_DECLARE_FRAME_PROPERTY_DELETABLE(TableBCDataProperty, TableBCData) is at the top of the file.)

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetTableBCData
    // ADAPTER: returns a copy of the data (Gecko returns a pointer into the property list); a
    // change is stored back with SetTableBCData.
    pub fn GetTableBCData(&self) -> Option<TableBCData> {
        self.GetProperty(TableBCDataProperty())
    }

    // ADAPTER: GetOrCreateDeletableProperty(TableBCDataProperty()) then assigning through the
    // pointer.
    fn SetTableBCData(&self, aData: TableBCData) {
        self.SetProperty(TableBCDataProperty(), aData);
    }
}

// Gecko: layout/tables/nsTableFrame.cpp DivideBCBorderSize
fn DivideBCBorderSize(aPixelSize: nscoord, aSmallHalf: &mut nscoord, aLargeHalf: &mut nscoord) {
    *aSmallHalf = aPixelSize / 2;
    *aLargeHalf = aPixelSize - *aSmallHalf;
}

impl nsTableFrame {
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetOuterBCBorder
    pub fn GetOuterBCBorder(&self, aWM: WritingMode) -> LogicalMargin {
        if self.NeedToCalcBCBorders() {
            self.CalcBCBorders();
        }
        let propData = self.GetTableBCData();
        if let Some(propData) = propData {
            return LogicalMargin::new(
                aWM,
                BC_BORDER_START_HALF(propData.mBStartBorderWidth),
                BC_BORDER_END_HALF(propData.mIEndBorderWidth),
                BC_BORDER_END_HALF(propData.mBEndBorderWidth),
                BC_BORDER_START_HALF(propData.mIStartBorderWidth),
            );
        }
        LogicalMargin::new__WritingMode(aWM)
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetCollapsedBorderPadding
    pub fn GetCollapsedBorderPadding(&self, aBorder: &mut Option<LogicalMargin>, aPadding: &mut Option<LogicalMargin>) {
        if self.IsBorderCollapse() {
            // Border-collapsed tables don't use any of their padding, and only part of
            // their border.
            let wm = self.GetWritingMode();
            *aBorder = Some(self.GetOuterBCBorder(wm));
            *aPadding = Some(LogicalMargin::new__WritingMode(wm));
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::InitChildReflowInput
    pub fn InitChildReflowInput(&self, aReflowInput: &mut ReflowInput) {
        let childWM = aReflowInput.GetWritingMode();
        let mut border = LogicalMargin::new__WritingMode(childWM);
        if self.IsBorderCollapse() {
            let rgFrame = nsTableRowGroupFrame::from_frame(aReflowInput.mFrame);
            border = rgFrame.GetBCBorderWidth(childWM);
        }
        let zeroPadding = LogicalMargin::new__WritingMode(childWM);
        aReflowInput.Init(&self.PresContext(), None, Some(border), Some(zeroPadding));

        NS_ASSERTION!(!self.mBits().mResizedColumns || !aReflowInput.mParentReflowInput.unwrap().mFlags.mSpecialBSizeReflow, "should not resize columns on special bsize reflow");
        if self.mBits().mResizedColumns {
            aReflowInput.SetIResize(true);
        }
    }

    // Position and size aKidFrame and update our reflow input. The origin of
    // aKidRect is relative to the upper-left origin of our frame
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::PlaceChild
    pub fn PlaceChild(&self, aReflowInput: &mut TableReflowInput, aKidFrame: nsIFrame, aKidReflowInput: &ReflowInput, aKidPosition: LogicalPoint, aContainerSize: nsSize, aKidDesiredSize: &mut ReflowOutput, aOriginalKidRect: nsRect, aOriginalKidInkOverflow: nsRect) {
        let wm = aReflowInput.mReflowInput.GetWritingMode();
        let isFirstReflow = aKidFrame.HasAnyStateBits(NS_FRAME_FIRST_REFLOW);

        // Place and size the child
        nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
            aKidFrame,
            &self.PresContext(),
            aKidDesiredSize,
            Some(aKidReflowInput),
            wm,
            aKidPosition,
            aContainerSize,
            nsIFrame_ReflowChildFlags::ApplyRelativePositioning,
        );

        nsTableFrame::InvalidateTableFrame(aKidFrame, aOriginalKidRect, aOriginalKidInkOverflow, isFirstReflow);

        aReflowInput.AdvanceBCoord(aKidDesiredSize.BSize(wm));
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::OrderedGroups
    pub fn OrderedGroups(&self) -> nsTableFrame_Groups {
        let mut children = nsTableFrame_Groups::default();

        let mut kidFrame = self.mFrames().FirstChild();
        while let Some(kid) = kidFrame {
            if let Some(rowGroup) = nsTableRowGroupFrame::do_QueryFrame(kid) {
                match kid.StyleDisplay().DisplayInside() {
                    StyleDisplayInside::TableHeaderGroup => {
                        if children.mHead.is_some() {
                            // treat additional thead like tbody
                            children.mRowGroups.push(rowGroup);
                        } else {
                            children.mHead = Some(rowGroup);
                        }
                    }
                    StyleDisplayInside::TableFooterGroup => {
                        if children.mFoot.is_some() {
                            // treat additional tfoot like tbody
                            children.mRowGroups.push(rowGroup);
                        } else {
                            children.mFoot = Some(rowGroup);
                        }
                    }
                    StyleDisplayInside::TableRowGroup => {
                        children.mRowGroups.push(rowGroup);
                    }
                    _ => {
                        MOZ_ASSERT_UNREACHABLE!("How did this produce an nsTableRowGroupFrame?");
                        // Just ignore it
                    }
                }
            } else if let Some(cg) = nsTableColGroupFrame::do_QueryFrame(kid) {
                if Some(cg) != *self.mSyntheticColGroup() {
                    children.mColGroups.push(cg);
                }
            }
            // Get the next sibling but skip it if it's also the next-in-flow, since
            // a next-in-flow will not be part of the current table.
            let mut cur = Some(kid);
            kidFrame = loop {
                let Some(c) = cur else {
                    break None;
                };
                let nif = c.GetNextInFlow();
                let next = c.GetNextSibling();
                if next != nif {
                    break next;
                }
                cur = next;
            };
        }

        // put the thead first
        if let Some(head) = children.mHead {
            children.mRowGroups.insert(0, head);
        }
        // put the tfoot after the last tbody
        if let Some(foot) = children.mFoot {
            children.mRowGroups.push(foot);
        }
        // Put the synthetic colgroup after all the non-synthetic ones.
        if let Some(syn) = *self.mSyntheticColGroup() {
            children.mColGroups.push(syn);
        }
        children
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::OrderedRowGroups
    pub fn OrderedRowGroups(&self) -> nsTableFrame_RowGroupArray {
        self.OrderedGroups().mRowGroups
    }
}

// Gecko: layout/tables/nsTableFrame.cpp IsRepeatable
fn IsRepeatable(aFrameBSize: nscoord, aPageBSize: nscoord) -> bool {
    aFrameBSize < (aPageBSize / 4)
}

impl nsTableFrame {
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::SetupHeaderFooterChild
    // NOT PORTED: pagination (only called when the context is paginated, which it never is).
    pub fn SetupHeaderFooterChild(&self, aReflowInput: &TableReflowInput, aFrame: nsTableRowGroupFrame) -> nscoord {
        let presContext = self.PresContext();
        let wm = self.GetWritingMode();
        let pageBSize = LogicalSize::new__WritingMode_nsSize(wm, presContext.GetPageSize()).BSize(wm);

        // Reflow the child with unconstrained block-size.
        let mut availSize = aReflowInput.AvailableSize();
        *availSize.BSize_mut(wm) = NS_UNCONSTRAINEDSIZE;

        let containerSize = aReflowInput.mReflowInput.ComputedSizeAsContainerIfConstrained();
        let mut initFlags = ReflowInput_InitFlags::new();
        initFlags += ReflowInput_InitFlag::CallerWillInit;
        let mut kidReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            &presContext,
            aReflowInput.mReflowInput,
            aFrame.0,
            availSize,
            None,
            initFlags,
            StyleSizeOverrides::default(),
            ComputeSizeFlags::default(),
            std::ptr::null_mut(),
        );
        self.InitChildReflowInput(&mut kidReflowInput);
        kidReflowInput.mFlags.mIsTopOfPage = true;
        let mut desiredSize = ReflowOutput::new__ReflowInput(aReflowInput.mReflowInput);
        let mut status = nsReflowStatus::new();
        self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
            aFrame.0,
            &presContext,
            &mut desiredSize,
            &kidReflowInput,
            wm,
            LogicalPoint::new(wm, aReflowInput.mICoord, aReflowInput.mBCoord),
            containerSize,
            nsIFrame_ReflowChildFlags::Default,
            &mut status,
            None,
        );
        // The child will be reflowed again "for real" so no need to place it now

        aFrame.SetRepeatable(IsRepeatable(desiredSize.BSize(wm), pageBSize));
        desiredSize.BSize(wm)
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::PlaceRepeatedFooter
    // NOT PORTED: pagination (only called when the context is paginated, which it never is).
    pub fn PlaceRepeatedFooter(&self, aReflowInput: &mut TableReflowInput, aTfoot: nsTableRowGroupFrame, aFooterBSize: nscoord) {
        let presContext = self.PresContext();
        let wm = self.GetWritingMode();
        let mut kidAvailSize = aReflowInput.AvailableSize();
        *kidAvailSize.BSize_mut(wm) = aFooterBSize;

        let containerSize = aReflowInput.mReflowInput.ComputedSizeAsContainerIfConstrained();
        let mut initFlags = ReflowInput_InitFlags::new();
        initFlags += ReflowInput_InitFlag::CallerWillInit;
        let mut footerReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
            &presContext,
            aReflowInput.mReflowInput,
            aTfoot.0,
            kidAvailSize,
            None,
            initFlags,
            StyleSizeOverrides::default(),
            ComputeSizeFlags::default(),
            std::ptr::null_mut(),
        );
        self.InitChildReflowInput(&mut footerReflowInput);

        let origTfootRect = aTfoot.GetRect();
        let origTfootInkOverflow = aTfoot.InkOverflowRect();

        let mut footerStatus = nsReflowStatus::new();
        let mut desiredSize = ReflowOutput::new__ReflowInput(aReflowInput.mReflowInput);
        let kidPosition = LogicalPoint::new(wm, aReflowInput.mICoord, aReflowInput.mBCoord);
        self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
            aTfoot.0,
            &presContext,
            &mut desiredSize,
            &footerReflowInput,
            wm,
            kidPosition,
            containerSize,
            nsIFrame_ReflowChildFlags::Default,
            &mut footerStatus,
            None,
        );

        self.PlaceChild(aReflowInput, aTfoot.0, &footerReflowInput, kidPosition, containerSize, &mut desiredSize, origTfootRect, origTfootInkOverflow);
    }
}

// ADAPTER: the lambda `MaybePlaceRepeatedFooter` of nsTableFrame::ReflowChildren, which captures
// the table, the reflow input, the groups and two locals.
fn MaybePlaceRepeatedFooter(aTable: &nsTableFrame, aReflowInput: &mut TableReflowInput, aGroups: &nsTableFrame_Groups, allowRepeatedFooter: bool, footerBSize: nscoord) {
    if allowRepeatedFooter {
        aTable.PlaceRepeatedFooter(aReflowInput, aGroups.mFoot.unwrap(), footerBSize);
    } else if aGroups.mFoot.map_or(false, |f| f.IsRepeatable()) {
        aGroups.mFoot.unwrap().SetRepeatable(false);
    }
}

impl nsTableFrame {
    // Reflow the children based on the avail size and reason in aReflowInput
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::ReflowChildren
    pub fn ReflowChildren(&self, aReflowInput: &mut TableReflowInput, aStatus: &mut nsReflowStatus, aGroups: &mut nsTableFrame_Groups, aLastChildReflowed: &mut Option<nsIFrame>, aOverflowAreas: &mut OverflowAreas) {
        aStatus.Reset();
        *aLastChildReflowed = None;

        let mut prevKidFrame: Option<nsIFrame> = None;
        let wm = aReflowInput.mReflowInput.GetWritingMode();
        NS_WARNING_ASSERTION!(wm.IsVertical() || NS_UNCONSTRAINEDSIZE != aReflowInput.mReflowInput.ComputedWidth(), "shouldn't have unconstrained width in horizontal mode");
        let containerSize = aReflowInput.mReflowInput.ComputedSizeAsContainerIfConstrained();

        let presContext = self.PresContext();
        // nsTableFrame is not able to pull back children from its next-in-flow, per
        // bug 1772383.  So even under paginated contexts, tables should not fragment
        // if they are inside of (i.e. potentially being fragmented by) a column-set
        // frame.  (This is indicated by the "mTableIsSplittable" flag.)
        let isPaginated = presContext.IsPaginated() && aReflowInput.mReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE && aReflowInput.mReflowInput.mFlags.mTableIsSplittable;

        // Tables currently (though we ought to fix this) only fragment in
        // paginated contexts, not in multicolumn contexts.  (See bug 888257.)
        // This is partly because they don't correctly handle incremental
        // layout when paginated.
        //
        // Since we propagate NS_FRAME_IS_DIRTY from parent to child at the
        // start of the parent's reflow (behavior that's new as of bug
        // 1308876), we can do things that are effectively incremental reflow
        // during paginated layout.  Since the table code doesn't handle this
        // correctly, we need to set the flag that says to reflow everything
        // within the table structure.
        if presContext.IsPaginated() {
            self.SetGeometryDirty();
        }

        aOverflowAreas.Clear();

        let reflowAllKids = aReflowInput.mReflowInput.ShouldReflowAllKids() || self.mBits().mResizedColumns || self.IsGeometryDirty() || self.NeedToCollapse();

        let mut pageBreak = false;
        let mut footerBSize = 0;

        // Determine the repeatablility of headers and footers, and also the desired
        // height of any repeatable footer.
        // The repeatability of headers on continued tables is handled
        // when they are created in nsCSSFrameConstructor::CreateContinuingTableFrame.
        // We handle the repeatability of footers again here because we need to
        // determine the footer's height anyway. We could perhaps optimize by
        // using the footer's prev-in-flow's height instead of reflowing it again,
        // but there's no real need.
        if isPaginated {
            let mut reorder = false;
            if aGroups.mHead.is_some() && self.GetPrevInFlow().is_none() {
                reorder = aGroups.mHead.unwrap().GetNextInFlow().is_some();
                self.SetupHeaderFooterChild(aReflowInput, aGroups.mHead.unwrap());
            }
            if aGroups.mFoot.is_some() {
                reorder = reorder || aGroups.mFoot.unwrap().GetNextInFlow().is_some();
                footerBSize = self.SetupHeaderFooterChild(aReflowInput, aGroups.mFoot.unwrap());
            }
            if reorder {
                // Reorder row groups - the reflow may have changed the nextinflows.
                *aGroups = self.OrderedGroups();
            }
        }
        let mut allowRepeatedFooter = false;
        let mut childX = 0usize;
        while childX < aGroups.mRowGroups.len() {
            let kidFrame = aGroups.mRowGroups[childX];
            let rowSpacing = self.GetRowSpacing__int32_t(kidFrame.GetStartRowIndex() + kidFrame.GetRowCount());
            // See if we should only reflow the dirty child frames
            if reflowAllKids || kidFrame.IsSubtreeDirty() || (aReflowInput.mReflowInput.mFlags.mSpecialBSizeReflow && (isPaginated || kidFrame.HasAnyStateBits(NS_FRAME_CONTAINS_RELATIVE_BSIZE))) {
                if pageBreak {
                    MaybePlaceRepeatedFooter(self, aReflowInput, aGroups, allowRepeatedFooter, footerBSize);
                    self.PushChildrenToOverflow(&aGroups.mRowGroups, childX);
                    aStatus.Reset();
                    aStatus.SetIncomplete();
                    *aLastChildReflowed = if allowRepeatedFooter { aGroups.mFoot.map(|f| f.0) } else { prevKidFrame };
                    break;
                }

                let mut kidAvailSize = aReflowInput.AvailableSize();
                allowRepeatedFooter = false;

                // If the child is a tbody in paginated mode, reduce the available
                // block-size by a repeated footer.
                if isPaginated && (NS_UNCONSTRAINEDSIZE != kidAvailSize.BSize(wm)) {
                    if Some(kidFrame) != aGroups.mHead && Some(kidFrame) != aGroups.mFoot && aGroups.mFoot.map_or(false, |f| f.IsRepeatable()) {
                        // the child is a tbody and there is a repeatable footer
                        NS_ASSERTION!(aGroups.mFoot == aGroups.mRowGroups.last().copied(), "Missing footer!");
                        if footerBSize + rowSpacing < kidAvailSize.BSize(wm) {
                            allowRepeatedFooter = true;
                            *kidAvailSize.BSize_mut(wm) -= footerBSize + rowSpacing;
                        }
                    }
                }

                let oldKidRect = kidFrame.GetRect();
                let oldKidInkOverflow = kidFrame.InkOverflowRect();

                let mut desiredSize = ReflowOutput::new__ReflowInput(aReflowInput.mReflowInput);

                // Reflow the child into the available space
                let mut initFlags = ReflowInput_InitFlags::new();
                initFlags += ReflowInput_InitFlag::CallerWillInit;
                let mut kidReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                    &presContext,
                    aReflowInput.mReflowInput,
                    kidFrame.0,
                    kidAvailSize,
                    None,
                    initFlags,
                    StyleSizeOverrides::default(),
                    ComputeSizeFlags::default(),
                    std::ptr::null_mut(),
                );
                self.InitChildReflowInput(&mut kidReflowInput);

                // If this isn't the first row group, and the previous row group has a
                // nonzero BEnd, then we can't be at the top of the page.
                // We ignore a repeated head row group in this check to avoid causing
                // infinite loops in some circumstances - see bug 344883.
                if childX > (if aGroups.mHead.map_or(false, |h| IsRepeatedFrame(h.0)) { 1 } else { 0 }) && (aGroups.mRowGroups[childX - 1].GetLogicalNormalRect(wm, containerSize).BEnd(wm) > 0) {
                    kidReflowInput.mFlags.mIsTopOfPage = false;
                }

                // record the presence of a next in flow, it might get destroyed so we
                // need to reorder the row group array
                let reorder = kidFrame.GetNextInFlow().is_some();

                let kidPosition = LogicalPoint::new(wm, aReflowInput.mICoord, aReflowInput.mBCoord);
                aStatus.Reset();
                self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
                    kidFrame.0,
                    &presContext,
                    &mut desiredSize,
                    &kidReflowInput,
                    wm,
                    kidPosition,
                    containerSize,
                    nsIFrame_ReflowChildFlags::Default,
                    aStatus,
                    None,
                );

                if reorder {
                    // Reorder row aGroups - the reflow may have changed the nextinflows.
                    *aGroups = self.OrderedGroups();
                    let idx = aGroups.mRowGroups.iter().position(|g| *g == kidFrame);
                    MOZ_ASSERT!(idx.is_some(), "kidFrame should still be in rowGroups!");
                    childX = idx.unwrap();
                }
                if isPaginated && !aStatus.IsFullyComplete() && self.ShouldAvoidBreakInside(aReflowInput.mReflowInput) {
                    aStatus.SetInlineLineBreakBeforeAndReset();
                    break;
                }
                // see if the rowgroup did not fit on this page might be pushed on
                // the next page
                if isPaginated && (aStatus.IsInlineBreakBefore() || (aStatus.IsComplete() && (kidReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE) && kidReflowInput.AvailableBSize() < desiredSize.BSize(wm))) {
                    if self.ShouldAvoidBreakInside(aReflowInput.mReflowInput) {
                        aStatus.SetInlineLineBreakBeforeAndReset();
                        break;
                    }
                    // if we are on top of the page place with dataloss
                    if kidReflowInput.mFlags.mIsTopOfPage {
                        if childX + 1 < aGroups.mRowGroups.len() {
                            self.PlaceChild(aReflowInput, kidFrame.0, &kidReflowInput, kidPosition, containerSize, &mut desiredSize, oldKidRect, oldKidInkOverflow);
                            MaybePlaceRepeatedFooter(self, aReflowInput, aGroups, allowRepeatedFooter, footerBSize);
                            aStatus.Reset();
                            aStatus.SetIncomplete();
                            self.PushChildrenToOverflow(&aGroups.mRowGroups, childX + 1);
                            *aLastChildReflowed = if allowRepeatedFooter { aGroups.mFoot.map(|f| f.0) } else { Some(kidFrame.0) };
                            break;
                        }
                    } else {
                        // we are not on top, push this rowgroup onto the next page
                        if prevKidFrame.is_some() {
                            // we had a rowgroup before so push this
                            MaybePlaceRepeatedFooter(self, aReflowInput, aGroups, allowRepeatedFooter, footerBSize);
                            aStatus.Reset();
                            aStatus.SetIncomplete();
                            self.PushChildrenToOverflow(&aGroups.mRowGroups, childX);
                            *aLastChildReflowed = if allowRepeatedFooter { aGroups.mFoot.map(|f| f.0) } else { prevKidFrame };
                            break;
                        } else {
                            // we can't push so lets make clear how much space we need
                            self.PlaceChild(aReflowInput, kidFrame.0, &kidReflowInput, kidPosition, containerSize, &mut desiredSize, oldKidRect, oldKidInkOverflow);
                            MaybePlaceRepeatedFooter(self, aReflowInput, aGroups, allowRepeatedFooter, footerBSize);
                            *aLastChildReflowed = if allowRepeatedFooter { aGroups.mFoot.map(|f| f.0) } else { Some(kidFrame.0) };
                            break;
                        }
                    }
                }

                *aLastChildReflowed = Some(kidFrame.0);

                pageBreak = false;
                // see if there is a page break after this row group or before the next
                // one
                if aStatus.IsComplete() && isPaginated && (kidReflowInput.AvailableBSize() != NS_UNCONSTRAINEDSIZE) {
                    let nextKid = if childX + 1 < aGroups.mRowGroups.len() { Some(aGroups.mRowGroups[childX + 1].0) } else { None };
                    pageBreak = nsTableFrame::PageBreakAfter(kidFrame.0, nextKid);
                }

                // Place the child
                self.PlaceChild(aReflowInput, kidFrame.0, &kidReflowInput, kidPosition, containerSize, &mut desiredSize, oldKidRect, oldKidInkOverflow);
                aReflowInput.AdvanceBCoord(rowSpacing);

                // Remember where we just were in case we end up pushing children
                prevKidFrame = Some(kidFrame.0);

                MOZ_ASSERT!(!aStatus.IsIncomplete() || isPaginated, "Table contents should only fragment in paginated contexts");

                // Special handling for incomplete children
                if isPaginated && aStatus.IsIncomplete() {
                    let kidNextInFlow = kidFrame.GetNextInFlow();
                    if kidNextInFlow.is_none() {
                        // The child doesn't have a next-in-flow so create a continuing
                        // frame. This hooks the child into the flow
                        // kidNextInFlow = PresShell()->FrameConstructor()->CreateContinuingFrame(kidFrame, this);
                        // NOT PORTED: continuing frames (pagination; the frame constructor does not
                        // create them).
                        unimplemented!("NOT PORTED: continuing row groups (pagination)");
                    } else if kidNextInFlow == kidFrame.GetNextSibling() {
                        // OrderedRowGroups excludes NIFs in the child list from 'rowGroups'
                        // so we deal with that here to make sure they get pushed.
                        MOZ_ASSERT!(!aGroups.mRowGroups.iter().any(|g| Some(g.0) == kidNextInFlow), "OrderedRowGroups must not put our NIF in 'rowGroups'");
                        aGroups.mRowGroups.insert(childX + 1, nsTableRowGroupFrame::from_frame(kidNextInFlow.unwrap()));
                    }

                    // We've used up all of our available space so push the remaining
                    // children.
                    MaybePlaceRepeatedFooter(self, aReflowInput, aGroups, allowRepeatedFooter, footerBSize);
                    if kidFrame.GetNextSibling().is_some() {
                        self.PushChildrenToOverflow(&aGroups.mRowGroups, childX + 1);
                    }
                    *aLastChildReflowed = if allowRepeatedFooter { aGroups.mFoot.map(|f| f.0) } else { Some(kidFrame.0) };
                    break;
                }
            } else {
                // it isn't being reflowed
                aReflowInput.AdvanceBCoord(rowSpacing);
                let kidRect = kidFrame.GetLogicalNormalRect(wm, containerSize);
                if kidRect.BStart(wm) != aReflowInput.mBCoord {
                    // invalidate the old position
                    // kidFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                    // move to the new position
                    kidFrame.MovePositionBy__WritingMode_LogicalPoint(wm, LogicalPoint::new(wm, 0, aReflowInput.mBCoord - kidRect.BStart(wm)));
                    // invalidate the new position
                    // kidFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                }

                aReflowInput.AdvanceBCoord(kidRect.BSize(wm));
            }
            childX += 1;
        }

        for cg in aGroups.mColGroups.clone() {
            // The column groups don't care about dimensions or reflow inputs. In fact,
            // we mostly need to do this to deal with dynamic style changes to a
            // colgroup having to reflow the table with border-collapsing.
            if !cg.IsSubtreeDirty() {
                continue;
            }
            let mut kidSize = ReflowOutput::new__WritingMode(wm);
            let kidReflowInput = ReflowInput::new__nsPresContext_nsIFrame_gfxContext_LogicalSize_InitFlags(&presContext, cg.0, aReflowInput.mReflowInput.mRenderingContext.unwrap_or(gfxContext), LogicalSize::new__WritingMode(cg.GetWritingMode()), ReflowInput_InitFlags::new());
            let mut cgStatus = nsReflowStatus::new();
            let dummyPos = LogicalPoint::new__WritingMode(wm);
            let dummyContainerSize = nsSize::default();
            self.ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
                cg.0,
                &presContext,
                &mut kidSize,
                &kidReflowInput,
                wm,
                dummyPos,
                dummyContainerSize,
                nsIFrame_ReflowChildFlags::Default,
                &mut cgStatus,
                None,
            );
            nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(cg.0, &presContext, &kidSize, Some(&kidReflowInput), wm, dummyPos, dummyContainerSize, nsIFrame_ReflowChildFlags::Default);
        }

        // We've now propagated the column resizes and geometry changes to all
        // the children.
        self.mBits().mResizedColumns = false;
        self.ClearGeometryDirty();

        // nsTableFrame does not pull children from its next-in-flow (bug 1772383).
        // This is generally fine, since tables only fragment for printing
        // (bug 888257) where incremental-reflow is impossible, and so children don't
        // usually dynamically move back and forth between continuations. However,
        // there are edge cases even with printing where nsTableFrame:
        // (1) Generates a continuation and passes children to it,
        // (2) Receives another call to Reflow, during which it
        // (3) Successfully lays out its remaining children.
        // If the completed status flows up as-is, the continuation will be destroyed.
        // To avoid that, we return an incomplete status if the continuation contains
        // any child that is not a repeated frame.
        let hasNextInFlowThatMustBePreserved = || -> bool {
            if !isPaginated {
                return false;
            }
            let nextInFlow = self.GetNextInFlow().map(nsTableFrame::from_frame);
            let Some(nextInFlow) = nextInFlow else {
                return false;
            };
            for kidFrame in nextInFlow.mFrames().as_static() {
                if !IsRepeatedFrame(kidFrame) {
                    return true;
                }
            }
            false
        };
        if aStatus.IsComplete() && hasNextInFlowThatMustBePreserved() {
            aStatus.SetIncomplete();
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::CalcDesiredBSize
    pub fn CalcDesiredBSize(&self, aReflowInput: &ReflowInput, aBorderPadding: LogicalMargin, aStatus: &nsReflowStatus) -> nscoord {
        let wm = aReflowInput.GetWritingMode();

        let rowGroups = self.OrderedGroups().mRowGroups;
        if rowGroups.is_empty() {
            if eCompatibility_NavQuirks == self.PresContext().CompatibilityMode() {
                // empty tables should not have a size in quirks mode
                return 0;
            }
            return self.CalcBorderBoxBSize(aReflowInput, aBorderPadding, aBorderPadding.BStartEnd(wm));
        }

        let cellMap = self.GetCellMap();
        MOZ_ASSERT!(cellMap.is_some());
        let cellMap = cellMap.unwrap();
        let rowCount = cellMap.GetRowCount();
        let colCount = cellMap.GetColCount();
        let mut desiredBSize = aBorderPadding.BStartEnd(wm);
        if rowCount > 0 && colCount > 0 {
            if self.GetPrevInFlow().is_none() {
                desiredBSize += self.GetRowSpacing__int32_t(-1);
            }
            let lastRG = *rowGroups.last().unwrap();
            for rg in &rowGroups {
                desiredBSize += rg.BSize(wm);
                if *rg != lastRG || aStatus.IsFullyComplete() {
                    desiredBSize += self.GetRowSpacing__int32_t(rg.GetStartRowIndex() + rg.GetRowCount());
                }
            }
            if aReflowInput.ComputedBSize() == NS_UNCONSTRAINEDSIZE && aStatus.IsIncomplete() {
                desiredBSize = std::cmp::max(desiredBSize, aReflowInput.AvailableBSize());
            }
        }

        // see if a specified table bsize requires dividing additional space to rows
        if self.GetPrevInFlow().is_none() {
            let bSize = self.CalcBorderBoxBSize(aReflowInput, aBorderPadding, desiredBSize);
            if bSize > desiredBSize {
                // proportionately distribute the excess bsize to unconstrained rows in
                // each unconstrained row group.
                self.DistributeBSizeToRows(aReflowInput, bSize - desiredBSize);
                return bSize;
            }
            // Tables don't shrink below their intrinsic size, apparently, even when
            // constrained by stuff like flex / grid or what not.
            return desiredBSize;
        }

        // FIXME(emilio): Is this right? This only affects fragmented tables...
        desiredBSize
    }
}

// Gecko: layout/tables/nsTableFrame.cpp ResizeCells
fn ResizeCells(aTableFrame: &nsTableFrame) {
    let rowGroups = aTableFrame.OrderedGroups().mRowGroups;
    let wm = aTableFrame.GetWritingMode();
    let mut tableDesiredSize = ReflowOutput::new__WritingMode(wm);
    tableDesiredSize.SetSize(wm, &aTableFrame.GetLogicalSize__WritingMode(wm));
    tableDesiredSize.SetOverflowAreasToDesiredBounds();

    for rgIdx in 0..rowGroups.len() {
        let rgFrame = rowGroups[rgIdx];

        let mut groupDesiredSize = ReflowOutput::new__WritingMode(wm);
        groupDesiredSize.SetSize(wm, &rgFrame.GetLogicalSize__WritingMode(wm));
        groupDesiredSize.SetOverflowAreasToDesiredBounds();

        let mut rowFrame = rgFrame.GetFirstRow();
        while let Some(row) = rowFrame {
            row.DidResize(ForceAlignTopForTableCell::No);
            rgFrame.ConsiderChildOverflow(&mut groupDesiredSize.mOverflowAreas, row.0, OverflowAreaUnionFlags::None);
            rowFrame = row.GetNextRow();
        }
        rgFrame.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(&mut groupDesiredSize, None);
        let shifted = groupDesiredSize.mOverflowAreas + rgFrame.GetPosition();
        tableDesiredSize.mOverflowAreas.UnionWith(&shifted);
    }
    aTableFrame.FinishAndStoreOverflow__ReflowOutput_nsStyleDisplay(&mut tableDesiredSize, None);
}

impl nsTableFrame {
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::DistributeBSizeToRows
    pub fn DistributeBSizeToRows(&self, aReflowInput: &ReflowInput, aAmount: nscoord) {
        let wm = aReflowInput.GetWritingMode();
        let borderPadding = aReflowInput.ComputedLogicalBorderPadding(wm);

        let containerSize = aReflowInput.ComputedSizeAsContainerIfConstrained();

        let rowGroups = self.OrderedGroups().mRowGroups;

        let mut amountUsed = 0;
        // distribute space to each pct bsize row whose row group doesn't have a
        // computed bsize, and base the pct on the table bsize. If the row group had a
        // computed bsize, then this was already done in
        // nsTableRowGroupFrame::CalculateRowBSizes
        let pctBasis = aReflowInput.ComputedBSize() - self.GetRowSpacing__int32_t_int32_t(-1, self.GetRowCount());
        let mut bOriginRG = borderPadding.BStart(wm) + self.GetRowSpacing__int32_t(0);
        let mut bEndRG = bOriginRG;
        for rgIdx in 0..rowGroups.len() {
            let rgFrame = rowGroups[rgIdx];
            let mut amountUsedByRG = 0;
            let mut bOriginRow = 0;
            let rgNormalRect = rgFrame.GetLogicalNormalRect(wm, containerSize);
            if !rgFrame.HasStyleBSize() {
                let mut rowFrame = rgFrame.GetFirstRow();
                while let Some(row) = rowFrame {
                    // We don't know the final width of the rowGroupFrame yet, so use 0,0
                    // as a dummy containerSize here; we'll adjust the row positions at
                    // the end, after the rowGroup size is finalized.
                    let dummyContainerSize = nsSize::default();
                    let rowNormalRect = row.GetLogicalNormalRect(wm, dummyContainerSize);
                    let rowSpacing = self.GetRowSpacing__int32_t(row.GetRowIndex());
                    if (amountUsed < aAmount) && row.HasPctBSize() {
                        let pctBSize = row.GetInitialBSize(pctBasis);
                        let amountForRow = std::cmp::min(aAmount - amountUsed, pctBSize - rowNormalRect.BSize(wm));
                        if amountForRow > 0 {
                            // XXXbz we don't need to move the row's b-position to bOriginRow?
                            let origRowRect = row.GetRect();
                            let newRowBSize = rowNormalRect.BSize(wm) + amountForRow;
                            row.SetSize__WritingMode_LogicalSize(wm, LogicalSize::new(wm, rowNormalRect.ISize(wm), newRowBSize));
                            bOriginRow += newRowBSize + rowSpacing;
                            bEndRG += newRowBSize + rowSpacing;
                            amountUsed += amountForRow;
                            amountUsedByRG += amountForRow;
                            // rowFrame->DidResize();

                            // rgFrame->InvalidateFrameWithRect(origRowRect); rgFrame->InvalidateFrame();
                            // NOT PORTED: invalidation
                        }
                    } else {
                        if amountUsed > 0 && bOriginRow != rowNormalRect.BStart(wm) && !self.HasAnyStateBits(NS_FRAME_FIRST_REFLOW) {
                            // rowFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                            row.MovePositionBy__WritingMode_LogicalPoint(wm, LogicalPoint::new(wm, 0, bOriginRow - rowNormalRect.BStart(wm)));
                            // rowFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                        }
                        bOriginRow += rowNormalRect.BSize(wm) + rowSpacing;
                        bEndRG += rowNormalRect.BSize(wm) + rowSpacing;
                    }
                    rowFrame = row.GetNextRow();
                }
                if amountUsed > 0 {
                    if rgNormalRect.BStart(wm) != bOriginRG {
                        // rgFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                    }

                    let origRgNormalRect = rgFrame.GetRect();
                    let origRgInkOverflow = rgFrame.InkOverflowRect();

                    rgFrame.MovePositionBy__WritingMode_LogicalPoint(wm, LogicalPoint::new(wm, 0, bOriginRG - rgNormalRect.BStart(wm)));
                    rgFrame.SetSize__WritingMode_LogicalSize(wm, LogicalSize::new(wm, rgNormalRect.ISize(wm), rgNormalRect.BSize(wm) + amountUsedByRG));

                    nsTableFrame::InvalidateTableFrame(rgFrame.0, origRgNormalRect, origRgInkOverflow, false);
                }
            } else if amountUsed > 0 && bOriginRG != rgNormalRect.BStart(wm) {
                // rgFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                rgFrame.MovePositionBy__WritingMode_LogicalPoint(wm, LogicalPoint::new(wm, 0, bOriginRG - rgNormalRect.BStart(wm)));
                // Make sure child views are properly positioned
                // rgFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
            }
            bOriginRG = bEndRG;
        }

        if amountUsed >= aAmount {
            ResizeCells(self);
            return;
        }

        // get the first row without a style bsize where its row group has an
        // unconstrained bsize
        let mut firstUnStyledRG: Option<nsTableRowGroupFrame> = None;
        let mut firstUnStyledRow: Option<nsTableRowFrame> = None;
        let mut rgIdx = 0;
        while rgIdx < rowGroups.len() && firstUnStyledRG.is_none() {
            let rgFrame = rowGroups[rgIdx];
            if !rgFrame.HasStyleBSize() {
                let mut rowFrame = rgFrame.GetFirstRow();
                while let Some(row) = rowFrame {
                    if !row.HasStyleBSize() {
                        firstUnStyledRG = Some(rgFrame);
                        firstUnStyledRow = Some(row);
                        break;
                    }
                    rowFrame = row.GetNextRow();
                }
            }
            rgIdx += 1;
        }

        let mut lastEligibleRow: Option<nsTableRowFrame> = None;
        // Accumulate the correct divisor. This will be the total bsize of all
        // unstyled rows inside unstyled row groups, unless there are none, in which
        // case, it will be number of all rows. If the unstyled rows don't have a
        // bsize, divide the space equally among them.
        let mut divisor: nscoord = 0;
        let mut eligibleRows = 0;
        let mut expandEmptyRows = false;

        if firstUnStyledRow.is_none() {
            // there is no unstyled row
            divisor = self.GetRowCount();
        } else {
            for rgIdx in 0..rowGroups.len() {
                let rgFrame = rowGroups[rgIdx];
                if firstUnStyledRG.is_none() || !rgFrame.HasStyleBSize() {
                    let mut rowFrame = rgFrame.GetFirstRow();
                    while let Some(row) = rowFrame {
                        if firstUnStyledRG.is_none() || !row.HasStyleBSize() {
                            NS_ASSERTION!(row.BSize(wm) >= 0, "negative row frame block-size");
                            divisor += row.BSize(wm);
                            eligibleRows += 1;
                            lastEligibleRow = Some(row);
                        }
                        rowFrame = row.GetNextRow();
                    }
                }
            }
            if divisor <= 0 {
                if eligibleRows > 0 {
                    expandEmptyRows = true;
                } else {
                    NS_ERROR!("invalid divisor");
                    return;
                }
            }
        }
        // allocate the extra bsize to the unstyled row groups and rows
        let bSizeToDistribute = aAmount - amountUsed;
        bOriginRG = borderPadding.BStart(wm) + self.GetRowSpacing__int32_t(-1);
        bEndRG = bOriginRG;
        for rgIdx in 0..rowGroups.len() {
            let rgFrame = rowGroups[rgIdx];
            let mut amountUsedByRG = 0;
            let mut bOriginRow = 0;
            let rgNormalRect = rgFrame.GetLogicalNormalRect(wm, containerSize);
            let rgInkOverflow = rgFrame.InkOverflowRect();
            // see if there is an eligible row group or we distribute to all rows
            if firstUnStyledRG.is_none() || !rgFrame.HasStyleBSize() || eligibleRows == 0 {
                let mut rowFrame = rgFrame.GetFirstRow();
                while let Some(row) = rowFrame {
                    let rowSpacing = self.GetRowSpacing__int32_t(row.GetRowIndex());
                    // We don't know the final width of the rowGroupFrame yet, so use 0,0
                    // as a dummy containerSize here; we'll adjust the row positions at
                    // the end, after the rowGroup size is finalized.
                    let dummyContainerSize = nsSize::default();
                    let rowNormalRect = row.GetLogicalNormalRect(wm, dummyContainerSize);
                    let rowInkOverflow = row.InkOverflowRect();
                    // see if there is an eligible row or we distribute to all rows
                    if firstUnStyledRow.is_none() || !row.HasStyleBSize() || eligibleRows == 0 {
                        let ratio: f32;
                        if eligibleRows != 0 {
                            if !expandEmptyRows {
                                // The amount of additional space each row gets is proportional
                                // to its bsize
                                ratio = rowNormalRect.BSize(wm) as f32 / divisor as f32;
                            } else {
                                // empty rows get all the same additional space
                                ratio = 1.0f32 / eligibleRows as f32;
                            }
                        } else {
                            // all rows get the same additional space
                            ratio = 1.0f32 / divisor as f32;
                        }
                        // give rows their additional space, except for the last row which
                        // gets the remainder
                        let mut amountForRow = if Some(row) == lastEligibleRow { aAmount - amountUsed } else { NSToCoordRound__float(bSizeToDistribute as f32 * ratio) };
                        amountForRow = std::cmp::min(amountForRow, aAmount - amountUsed);

                        if bOriginRow != rowNormalRect.BStart(wm) {
                            // rowFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                        }

                        // update the row bsize
                        let origRowRect = row.GetRect();
                        let newRowBSize = rowNormalRect.BSize(wm) + amountForRow;
                        row.MovePositionBy__WritingMode_LogicalPoint(wm, LogicalPoint::new(wm, 0, bOriginRow - rowNormalRect.BStart(wm)));
                        row.SetSize__WritingMode_LogicalSize(wm, LogicalSize::new(wm, rowNormalRect.ISize(wm), newRowBSize));

                        bOriginRow += newRowBSize + rowSpacing;
                        bEndRG += newRowBSize + rowSpacing;

                        amountUsed += amountForRow;
                        amountUsedByRG += amountForRow;
                        NS_ASSERTION!(amountUsed <= aAmount, "invalid row allocation");
                        // rowFrame->DidResize();
                        nsTableFrame::InvalidateTableFrame(row.0, origRowRect, rowInkOverflow, false);
                    } else {
                        if amountUsed > 0 && bOriginRow != rowNormalRect.BStart(wm) {
                            // rowFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                            row.MovePositionBy__WritingMode_LogicalPoint(wm, LogicalPoint::new(wm, 0, bOriginRow - rowNormalRect.BStart(wm)));
                            // rowFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                        }
                        bOriginRow += rowNormalRect.BSize(wm) + rowSpacing;
                        bEndRG += rowNormalRect.BSize(wm) + rowSpacing;
                    }
                    rowFrame = row.GetNextRow();
                }

                if amountUsed > 0 {
                    if rgNormalRect.BStart(wm) != bOriginRG {
                        // rgFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                    }

                    let origRgNormalRect = rgFrame.GetRect();
                    rgFrame.MovePositionBy__WritingMode_LogicalPoint(wm, LogicalPoint::new(wm, 0, bOriginRG - rgNormalRect.BStart(wm)));
                    rgFrame.SetSize__WritingMode_LogicalSize(wm, LogicalSize::new(wm, rgNormalRect.ISize(wm), rgNormalRect.BSize(wm) + amountUsedByRG));

                    nsTableFrame::InvalidateTableFrame(rgFrame.0, origRgNormalRect, rgInkOverflow, false);
                }

                // For vertical-rl mode, we needed to position the rows relative to the
                // right-hand (block-start) side of the group; but we couldn't do that
                // above, as we didn't know the rowGroupFrame's final block size yet.
                // So we used a dummyContainerSize of 0,0 earlier, placing the rows to
                // the left of the rowGroupFrame's (physical) origin. Now we move them
                // all rightwards by its final width.
                if wm.IsVerticalRL() {
                    let rgWidth = rgFrame.GetSize().width;
                    let mut rowFrame = rgFrame.GetFirstRow();
                    while let Some(row) = rowFrame {
                        // rowFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                        row.MovePositionBy(nsPoint::new(rgWidth, 0));
                        // rowFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                        rowFrame = row.GetNextRow();
                    }
                }
            } else if amountUsed > 0 && bOriginRG != rgNormalRect.BStart(wm) {
                // rgFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
                rgFrame.MovePositionBy__WritingMode_LogicalPoint(wm, LogicalPoint::new(wm, 0, bOriginRG - rgNormalRect.BStart(wm)));
                // Make sure child views are properly positioned
                // rgFrame->InvalidateFrameSubtree(); NOT PORTED: invalidation
            }
            bOriginRG = bEndRG;
        }

        ResizeCells(self);
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetColumnISizeFromFirstInFlow
    pub fn GetColumnISizeFromFirstInFlow(&self, aColIndex: i32) -> nscoord {
        MOZ_ASSERT!(Some(self.0) == Some(self.0.FirstInFlow()));
        let colFrame = self.GetColFrame(aColIndex);
        match colFrame {
            Some(c) => c.GetFinalISize(),
            None => 0,
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetColSpacing()
    pub fn GetColSpacing__(&self) -> nscoord {
        if self.IsBorderCollapse() {
            return 0;
        }
        self.StyleTableBorder().mBorderSpacingWidth_ToAppUnits()
    }

    // XXX: could cache this.  But be sure to check style changes if you do!
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetColSpacing(int32_t)
    pub fn GetColSpacing__int32_t(&self, aColIndex: i32) -> nscoord {
        NS_ASSERTION!(aColIndex >= -1 && aColIndex <= self.GetColCount(), "Column index exceeds the bounds of the table");
        // Index is irrelevant for ordinary tables.  We check that it falls within
        // appropriate bounds to increase confidence of correctness in situations
        // where it does matter.
        self.GetColSpacing__()
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetColSpacing(int32_t, int32_t)
    pub fn GetColSpacing__int32_t_int32_t(&self, aStartColIndex: i32, aEndColIndex: i32) -> nscoord {
        NS_ASSERTION!(aStartColIndex >= -1 && aStartColIndex <= self.GetColCount(), "Start column index exceeds the bounds of the table");
        NS_ASSERTION!(aEndColIndex >= -1 && aEndColIndex <= self.GetColCount(), "End column index exceeds the bounds of the table");
        NS_ASSERTION!(aStartColIndex <= aEndColIndex, "End index must not be less than start index");
        // Only one possible value so just multiply it out. Tables where index
        // matters will override this function
        self.GetColSpacing__() * (aEndColIndex - aStartColIndex)
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetRowSpacing()
    pub fn GetRowSpacing__(&self) -> nscoord {
        if self.IsBorderCollapse() {
            return 0;
        }
        self.StyleTableBorder().mBorderSpacingHeight_ToAppUnits()
    }

    // XXX: could cache this. But be sure to check style changes if you do!
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetRowSpacing(int32_t)
    pub fn GetRowSpacing__int32_t(&self, aRowIndex: i32) -> nscoord {
        NS_ASSERTION!(aRowIndex >= -1 && aRowIndex <= self.GetRowCount(), "Row index exceeds the bounds of the table");
        // Index is irrelevant for ordinary tables.  We check that it falls within
        // appropriate bounds to increase confidence of correctness in situations
        // where it does matter.
        self.GetRowSpacing__()
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetRowSpacing(int32_t, int32_t)
    pub fn GetRowSpacing__int32_t_int32_t(&self, aStartRowIndex: i32, aEndRowIndex: i32) -> nscoord {
        NS_ASSERTION!(aStartRowIndex >= -1 && aStartRowIndex <= self.GetRowCount(), "Start row index exceeds the bounds of the table");
        NS_ASSERTION!(aEndRowIndex >= -1 && aEndRowIndex <= self.GetRowCount(), "End row index exceeds the bounds of the table");
        NS_ASSERTION!(aStartRowIndex <= aEndRowIndex, "End index must not be less than start index");
        // Only one possible value so just multiply it out. Tables where index
        // matters will override this function
        self.GetRowSpacing__() * (aEndRowIndex - aStartRowIndex)
    }

    /* virtual */
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetNaturalBaselineBOffset
    pub fn GetNaturalBaselineBOffset(&self, aWM: WritingMode, aBaselineGroup: BaselineSharingGroup, aExportContext: BaselineExportContext) -> Option<nscoord> {
        if self.StyleDisplay().IsContainLayout() {
            return None;
        }

        let orderedRowGroups = self.OrderedRowGroups();
        // XXX not sure if this should be the size of the containing block instead.
        let containerSize = self.GetRect().Size();
        let TableBaseline = |aRowGroup: nsTableRowGroupFrame, aRow: nsTableRowFrame| -> Option<nscoord> {
            let rgBStart = aRowGroup.GetLogicalNormalRect(aWM, containerSize).BStart(aWM);
            let rowBStart = aRow.GetLogicalNormalRect(aWM, aRowGroup.GetSize()).BStart(aWM);
            aRow.GetRowBaseline(aWM).map(|aBaseline| rgBStart + rowBStart + aBaseline)
        };
        if aBaselineGroup == BaselineSharingGroup::First {
            for rgIndex in 0..orderedRowGroups.len() {
                let rgFrame = orderedRowGroups[rgIndex];
                let row = rgFrame.GetFirstRow();
                if let Some(row) = row {
                    return TableBaseline(rgFrame, row);
                }
            }
        } else {
            let mut rgIndex = orderedRowGroups.len();
            while rgIndex > 0 {
                rgIndex -= 1;
                let rgFrame = orderedRowGroups[rgIndex];
                let row = rgFrame.GetLastRow();
                if let Some(row) = row {
                    return TableBaseline(rgFrame, row).map(|aBaseline| self.BSize(aWM) - aBaseline);
                }
            }
        }
        None
    }
}

/* ----- global methods ----- */

impl nsTableFrame {
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetTableFrame
    pub fn GetTableFrame(aFrame: nsIFrame) -> nsTableFrame {
        let mut ancestor = aFrame.GetParent();
        while let Some(a) = ancestor {
            if a.IsTableFrame() {
                return nsTableFrame::from_frame(a);
            }
            ancestor = a.GetParent();
        }
        MOZ_CRASH!("unable to find table parent");
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::IsAutoBSize
    pub fn IsAutoBSize(&self, aWM: WritingMode) -> bool {
        let bsize = self.StylePosition().BSize(aWM, &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.0, std::ptr::null_mut()));
        if bsize.IsAuto() {
            return true;
        }
        bsize.ConvertsToPercentage() && bsize.ToPercentage() <= 0.0f32
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::CalcBorderBoxBSize
    pub fn CalcBorderBoxBSize(&self, aReflowInput: &ReflowInput, aBorderPadding: LogicalMargin, aIntrinsicBorderBoxBSize: nscoord) -> nscoord {
        let wm = aReflowInput.GetWritingMode();
        let mut bSize = aReflowInput.ComputedBSize();
        let bp = aBorderPadding.BStartEnd(wm);
        if bSize == NS_UNCONSTRAINEDSIZE {
            if aIntrinsicBorderBoxBSize == NS_UNCONSTRAINEDSIZE {
                return NS_UNCONSTRAINEDSIZE;
            }
            bSize = std::cmp::max(0, aIntrinsicBorderBoxBSize - bp);
        }
        aReflowInput.ApplyMinMaxBSize(bSize, 0) + bp
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::IsAutoLayout
    pub fn IsAutoLayout(&self) -> bool {
        if self.StyleTable().mLayoutStrategy() == StyleTableLayout::Auto {
            return true;
        }
        // a fixed-layout inline-table must have a inline size
        // and tables with inline size set to 'max-content' must be
        // auto-layout (at least as long as
        // FixedTableLayoutStrategy::GetPrefISize returns nscoord_MAX)
        let iSize = self.StylePosition().ISize(self.GetWritingMode(), &AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(self.0, std::ptr::null_mut()));
        iSize.IsAuto() || iSize.IsMaxContent()
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetFrameName
    // NOT PORTED: frame dumps (DEBUG_FRAME_DUMP).

    // Find the closet sibling before aPriorChildFrame (including aPriorChildFrame)
    // that is of type aChildType
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::GetFrameAtOrBefore
    pub fn GetFrameAtOrBefore(aParentFrame: nsIFrame, aPriorChildFrame: Option<nsIFrame>, aChildType: LayoutFrameType) -> Option<nsIFrame> {
        let Some(aPriorChildFrame) = aPriorChildFrame else {
            return None;
        };
        if aChildType == aPriorChildFrame.Type() {
            return Some(aPriorChildFrame);
        }

        // aPriorChildFrame is not of type aChildType, so we need start from
        // the beginnng and find the closest one
        let mut lastMatchingFrame: Option<nsIFrame> = None;
        let mut childFrame = aParentFrame.GetChildList(FrameChildListID::Principal).FirstChild();
        while let Some(child) = childFrame {
            if child == aPriorChildFrame {
                break;
            }
            if aChildType == child.Type() {
                lastMatchingFrame = Some(child);
            }
            childFrame = child.GetNextSibling();
        }
        lastMatchingFrame
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::DumpRowGroup, Dump
    // NOT PORTED: debug dumps (nsTableFrame.cpp lines 3237-3340 at GECKO_REV, except VerifyColFrames).

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::VerifyColFrames
    #[cfg(debug_assertions)]
    pub fn VerifyColFrames(&self) {
        for i in 0..self.mColFrames().len() as i32 {
            let col = self.mColFrames()[i as usize];
            MOZ_ASSERT!(col.GetParent().is_some());
            MOZ_ASSERT!(col.GetColIndex() == i);
            MOZ_ASSERT!(col.GetParent().unwrap().IsTableColGroupFrame());
            let cg = nsTableColGroupFrame::from_frame(col.GetParent().unwrap());
            let cgStart = cg.GetStartColumnIndex();
            let cgEnd = cgStart + cg.GetColCount();
            MOZ_ASSERT!(i >= cgStart && i < cgEnd, "col index not within its colgroup's range");
        }
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::ColumnHasCellSpacingBefore
    pub fn ColumnHasCellSpacingBefore(&self, aColIndex: i32) -> bool {
        if aColIndex == 0 {
            return true;
        }
        // Since fixed-layout tables should not have their column sizes change
        // as they load, we assume that all columns are significant.
        let fif = self.FirstInFlow_();
        if fif.LayoutStrategy().unwrap().GetType() == nsITableLayoutStrategy_Type::Fixed {
            return true;
        }
        let cellMap = fif.GetCellMap();
        let Some(cellMap) = cellMap else {
            return false;
        };
        if cellMap.GetNumCellsOriginatingInCol(aColIndex) > 0 {
            return true;
        }
        // Check if we have a <col> element with a non-zero definite inline size.
        // Note: percentages and calc(%) are intentionally not considered.
        if let Some(col) = fif.GetColFrame(aColIndex) {
            let anchorResolutionParams = AnchorPosResolutionParams::From__nsIFrame_AnchorPosResolutionCache(col.0, std::ptr::null_mut());
            let iSize = col.StylePosition().ISize(self.GetWritingMode(), &anchorResolutionParams);
            if iSize.ConvertsToLength() && iSize.ToLength() > 0 {
                let maxISize = col.StylePosition().MaxISize(self.GetWritingMode(), &anchorResolutionParams);
                if !maxISize.ConvertsToLength() || maxISize.ToLength() > 0 {
                    return true;
                }
            }
            let minISize = col.StylePosition().MinISize(self.GetWritingMode(), &anchorResolutionParams);
            if minISize.ConvertsToLength() && minISize.ToLength() > 0 {
                return true;
            }
        }
        false
    }
}

/********************************************************************************
 * Collapsing Borders
 *
 *  The CSS spec says to resolve border conflicts in this order:
 *  1) any border with the style HIDDEN wins
 *  2) the widest border with a style that is not NONE wins
 *  3) the border styles are ranked in this order, highest to lowest precedence:
 *     double, solid, dashed, dotted, ridge, outset, groove, inset
 *  4) borders that are of equal width and style (differ only in color) have
 *     this precedence: cell, row, rowgroup, col, colgroup, table
 *  5) if all border styles are NONE, then that's the computed border style.
 *******************************************************************************/

impl nsTableFrame {
    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::AddBCDamageArea
    pub fn AddBCDamageArea(&self, aValue: &TableArea) {
        MOZ_ASSERT!(self.IsBorderCollapse(), "Why call this if we are not border-collapsed?");
        // NOT PORTED: VerifyDamageRect (debug-only).

        self.SetNeedToCalcBCBorders(true);
        self.SetNeedToCalcHasBCBorders(true);
        // Get the property
        let mut value = self.GetTableBCData().unwrap_or_default();

        // Clamp the old damage area to the current table area in case it shrunk.
        let cols = self.GetColCount();
        if value.mDamageArea.EndCol() > cols {
            if value.mDamageArea.StartCol() > cols {
                *value.mDamageArea.StartCol_mut() = cols;
                *value.mDamageArea.ColCount_mut() = 0;
            } else {
                *value.mDamageArea.ColCount_mut() = cols - value.mDamageArea.StartCol();
            }
        }
        let rows = self.GetRowCount();
        if value.mDamageArea.EndRow() > rows {
            if value.mDamageArea.StartRow() > rows {
                *value.mDamageArea.StartRow_mut() = rows;
                *value.mDamageArea.RowCount_mut() = 0;
            } else {
                *value.mDamageArea.RowCount_mut() = rows - value.mDamageArea.StartRow();
            }
        }

        // Construct a union of the new and old damage areas.
        let old = value.mDamageArea;
        value.mDamageArea.UnionArea(&old, aValue);
        self.SetTableBCData(value);
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::SetFullBCDamageArea
    pub fn SetFullBCDamageArea(&self) {
        MOZ_ASSERT!(self.IsBorderCollapse(), "Why call this if we are not border-collapsed?");

        self.SetNeedToCalcBCBorders(true);
        self.SetNeedToCalcHasBCBorders(true);

        let mut value = self.GetTableBCData().unwrap_or_default();
        value.mDamageArea = TableArea::new(0, 0, self.GetColCount(), self.GetRowCount());
        self.SetTableBCData(value);
    }

    // Gecko: layout/tables/nsTableFrame.cpp BCCellBorder, BCMapCellInfo, BCMapTableInfo,
    // BCMapCellIterator, GetColorAndStyle, GetPaintStyleInfo, nsDelayedCalcBCBorders,
    // nsTableFrame::BCRecalcNeeded, CompareBorders, BCCornerInfo, BCCorners, BCCellBorders,
    // nsTableFrame::ExpandBCDamageArea, nsTableFrame::CalcBCBorders
    // NOT PORTED: the collapsing-border computation (nsTableFrame.cpp lines 3470-5405 at GECKO_REV).
    // CalcBCBorders only clears the "needs to calculate" bit: a border-collapse table is laid out with
    // an outer collapsed border of 0 and the cells' own borders (what the separate model uses),
    // the BC data of the cells and the table stay zero.
    pub fn BCRecalcNeeded(&self, aOldComputedStyle: &ServoArc<ComputedValues>, aNewComputedStyle: &ServoArc<ComputedValues>) -> bool {
        false
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::ExpandBCDamageArea
    // NOT PORTED: collapsing borders (nsTableFrame.cpp lines 4471-4609 at GECKO_REV).
    pub fn ExpandBCDamageArea(&self, aArea: &mut TableArea) {}

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::CalcBCBorders
    // NOT PORTED: collapsing borders (see above).
    pub fn CalcBCBorders(&self) {
        self.SetNeedToCalcBCBorders(false);
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::IterateBCBorders, PaintBCBorders,
    // CreateWebRenderCommandsForBCBorders and the BCPaint* classes
    // NOT PORTED: painting of collapsed borders (nsTableFrame.cpp lines 5407-6968 at GECKO_REV).

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::RowHasSpanningCells
    pub fn RowHasSpanningCells(&self, aRowIndex: i32, aNumEffCols: i32) -> bool {
        let mut result = false;
        let cellMap = self.GetCellMap();
        MOZ_ASSERT!(cellMap.is_some(), "bad call, cellMap not yet allocated.");
        if let Some(cellMap) = cellMap {
            result = cellMap.RowHasSpanningCells(aRowIndex, aNumEffCols);
        }
        result
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::RowIsSpannedInto
    pub fn RowIsSpannedInto(&self, aRowIndex: i32, aNumEffCols: i32) -> bool {
        let mut result = false;
        let cellMap = self.GetCellMap();
        MOZ_ASSERT!(cellMap.is_some(), "bad call, cellMap not yet allocated.");
        if let Some(cellMap) = cellMap {
            result = cellMap.RowIsSpannedInto(aRowIndex, aNumEffCols);
        }
        result
    }

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::InvalidateTableFrame
    // NOT PORTED: invalidation (painting): the body only calls InvalidateFrame and
    // InvalidateFrameWithRect (nsTableFrame.cpp lines 6992-7027 at GECKO_REV).
    pub fn InvalidateTableFrame(aFrame: nsIFrame, aOrigRect: nsRect, aOrigInkOverflow: nsRect, aIsFirstReflow: bool) {}

    // Gecko: layout/tables/nsTableFrame.cpp nsTableFrame::AppendDirectlyOwnedAnonBoxes,
    // UpdateStyleOfOwnedAnonBoxesForTableWrapper
    // NOT PORTED: restyle (nsTableFrame.cpp lines 7029-7079 at GECKO_REV).

    // Gecko: layout/tables/nsTableFrame.cpp nsDisplayTableItem::GetBounds,
    // nsDisplayTableBackgroundSet, nsDisplayTableBorderCollapse
    // NOT PORTED: display lists (nsTableFrame.cpp lines 7081-7156 at GECKO_REV).
}

// Gecko: layout/tables/nsTableFrame.h the inline members at the end of the header, in order.
impl nsTableFrame {
    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::LayoutStrategy
    // ADAPTER: the reference is 'static because the strategy lives in a Box inside the first-in-flow's
    // member block; a caller must not hold it across a call that replaces the strategy. None when the
    // first-in-flow has none (it was destroyed, see MarkIntrinsicISizesDirty).
    pub fn LayoutStrategy(&self) -> Option<&'static mut dyn nsITableLayoutStrategy> {
        let first = self.FirstInFlow_();
        let p = first.mTableLayoutStrategy().as_mut()?;
        Some(unsafe { &mut *(p.as_mut() as *mut dyn nsITableLayoutStrategy) })
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::HasPctCol
    pub fn HasPctCol(&self) -> bool {
        self.mBits().mHasPctCol
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::SetHasPctCol
    pub fn SetHasPctCol(&self, aValue: bool) {
        self.mBits().mHasPctCol = aValue;
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::HasCellSpanningPctCol
    pub fn HasCellSpanningPctCol(&self) -> bool {
        self.mBits().mCellSpansPctCol
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::SetHasCellSpanningPctCol
    pub fn SetHasCellSpanningPctCol(&self, aValue: bool) {
        self.mBits().mCellSpansPctCol = aValue;
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::IsRowInserted
    pub fn IsRowInserted(&self) -> bool {
        self.mBits().mRowInserted
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::SetRowInserted
    pub fn SetRowInserted(&self, aValue: bool) {
        self.mBits().mRowInserted = aValue;
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::SetNeedToCollapse
    pub fn SetNeedToCollapse(&self, aValue: bool) {
        self.FirstInFlow_().mBits().mNeedToCollapse = aValue;
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::NeedToCollapse
    pub fn NeedToCollapse(&self) -> bool {
        self.FirstInFlow_().mBits().mNeedToCollapse
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::GetColCache
    pub fn GetColCache(&self) -> &'static mut Vec<nsTableColFrame> {
        self.mColFrames()
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::IsBorderCollapse
    pub fn IsBorderCollapse(&self) -> bool {
        self.mBits().mIsBorderCollapse
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::SetBorderCollapse
    pub fn SetBorderCollapse(&self, aValue: bool) {
        self.mBits().mIsBorderCollapse = aValue;
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::NeedToCalcBCBorders
    pub fn NeedToCalcBCBorders(&self) -> bool {
        self.mBits().mNeedToCalcBCBorders
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::SetNeedToCalcBCBorders
    pub fn SetNeedToCalcBCBorders(&self, aValue: bool) {
        self.mBits().mNeedToCalcBCBorders = aValue;
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::NeedToCalcHasBCBorders
    pub fn NeedToCalcHasBCBorders(&self) -> bool {
        self.mBits().mNeedToCalcHasBCBorders
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::SetNeedToCalcHasBCBorders
    pub fn SetNeedToCalcHasBCBorders(&self, aValue: bool) {
        self.mBits().mNeedToCalcHasBCBorders = aValue;
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::HasBCBorders
    pub fn HasBCBorders(&self) -> bool {
        if self.NeedToCalcHasBCBorders() {
            self.CalcHasBCBorders();
            self.SetNeedToCalcHasBCBorders(false);
        }
        self.mBits().mHasBCBorders
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::SetHasBCBorders
    pub fn SetHasBCBorders(&self, aValue: bool) {
        self.mBits().mHasBCBorders = aValue;
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::SetGeometryDirty
    pub fn SetGeometryDirty(&self) {
        self.mBits().mGeometryDirty = true;
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::ClearGeometryDirty
    pub fn ClearGeometryDirty(&self) {
        self.mBits().mGeometryDirty = false;
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::IsGeometryDirty
    pub fn IsGeometryDirty(&self) -> bool {
        self.mBits().mGeometryDirty
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::GetRowCount
    pub fn GetRowCount(&self) -> i32 {
        self.GetCellMap().unwrap().GetRowCount()
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::GetColCount
    pub fn GetColCount(&self) -> i32 {
        self.GetCellMap().unwrap().GetColCount()
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::IsDeletedRowIndexRangesEmpty
    pub fn IsDeletedRowIndexRangesEmpty(&self) -> bool {
        self.mDeletedRowIndexRanges().is_empty()
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::IsDestroying
    pub fn IsDestroying(&self) -> bool {
        self.mBits().mIsDestroying
    }

    // Gecko: layout/tables/nsTableFrame.h nsTableFrame::GetSyntheticColGroup
    pub fn GetSyntheticColGroup(&self) -> Option<nsTableColGroupFrame> {
        *self.mSyntheticColGroup()
    }
}

// Gecko: layout/tables/nsTableFrame.h BC_BORDER_START_HALF / BC_BORDER_END_HALF (from nsCellMap.h /
// celldata.h in Gecko: layout/tables/celldata.h)
// ADAPTER: if celldata.rs defines them, delete these.
fn BC_BORDER_START_HALF(aPixelSize: nscoord) -> nscoord {
    aPixelSize - aPixelSize / 2
}
fn BC_BORDER_END_HALF(aPixelSize: nscoord) -> nscoord {
    aPixelSize / 2
}

// Gecko: layout/tables/nsTableFrame.h ABORT0, ABORT1
// (the two macros are used inline above, as NS_ASSERTION!(false, "CellIterator program error"))

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockReflowState_test_support::*;

    // Written from the documented behaviour of table layout (CSS 2.2 17.5.2.2 auto table layout,
    // 17.5.2.1 fixed table layout, 17.6.1 border-spacing, the HTML rowspan/colspan rules); Gecko has no
    // gtest for nsTableFrame.

    const AUTO: nscoord = NS_UNCONSTRAINEDSIZE;

    // One cell of the test table.
    #[derive(Clone)]
    struct Cell {
        // the css of the cell box, for example "width:50%"
        style: String,
        // the content: a block of this width and height (px); when `intrinsic` is given the content has
        // an auto width and these are its min-content and max-content widths (px)
        width: Option<i32>,
        intrinsic: Option<(i32, i32)>,
        height: i32,
        colspan: i32,
        rowspan: i32,
    }

    // A cell whose content is a block of `w` by `h` px.
    fn cell(w: i32, h: i32) -> Cell {
        Cell { style: String::new(), width: Some(w), intrinsic: None, height: h, colspan: 1, rowspan: 1 }
    }

    // A cell whose content has min-content width `min` and max-content width `pref` (px).
    fn flex_cell(min: i32, pref: i32, h: i32) -> Cell {
        Cell { style: String::new(), width: None, intrinsic: Some((min, pref)), height: h, colspan: 1, rowspan: 1 }
    }

    impl Cell {
        fn style(mut self, aStyle: &str) -> Cell {
            self.style = aStyle.to_string();
            self
        }
        fn span(mut self, aColSpan: i32, aRowSpan: i32) -> Cell {
            self.colspan = aColSpan;
            self.rowspan = aRowSpan;
            self
        }
    }

    struct TestTable {
        root: nsIFrame,
        wrapper: nsTableWrapperFrame,
        table: nsTableFrame,
        rows: Vec<nsTableRowFrame>,
        cells: Vec<Vec<nsTableCellFrame>>,
    }

    // Builds the frames of `<table style=aTableStyle><tbody>` with the rows of cells in `aRows` inside
    // its table wrapper frame (which has the table's `width`, margins, ...), and does what the frame
    // constructor does: the class of each frame, Init, and SetInitialChildList from the leaves up (so
    // that the table's SetInitialChildList builds the cell map from complete rows).
    fn build(aTableStyle: &str, aRows: &[Vec<Cell>]) -> TestTable {
        let mut html = format!(r#"<div id=root style="display:flow-root;margin:0;padding:0;border:0"><div id=w style="display:block;margin:0"><div id=t style="display:table;margin:0;padding:0;border:0;{aTableStyle}"><div id=g style="display:table-row-group">"#);
        let mut tree: Vec<(String, Option<usize>)> = vec![("root".into(), None), ("w".into(), Some(0)), ("t".into(), Some(1)), ("g".into(), Some(2))];
        // (kind, row index, cell index)
        let mut kinds: Vec<(char, usize, usize)> = vec![('R', 0, 0), ('w', 0, 0), ('t', 0, 0), ('g', 0, 0)];
        for (r, cells) in aRows.iter().enumerate() {
            html += &format!(r#"<div id=r{r} style="display:table-row">"#);
            let rowFrame = tree.len();
            tree.push((format!("r{r}"), Some(3)));
            kinds.push(('r', r, 0));
            for (c, cell) in cells.iter().enumerate() {
                html += &format!(r#"<div id=c{r}x{c} style="display:table-cell;padding:0;border:0;{}">"#, cell.style);
                let cellFrame = tree.len();
                tree.push((format!("c{r}x{c}"), Some(rowFrame)));
                kinds.push(('c', r, c));
                let widthCss = match cell.width {
                    Some(w) => format!("width:{w}px;"),
                    None => String::new(),
                };
                html += &format!(r#"<div id=k{r}x{c} style="margin:0;{widthCss}height:{}px"></div></div>"#, cell.height);
                tree.push((format!("k{r}x{c}"), Some(cellFrame)));
                kinds.push(('k', r, c));
            }
            html += "</div>";
        }
        html += "</div></div></div></div>";
        let treeRef: Vec<(&str, Option<usize>)> = tree.iter().map(|(n, p)| (n.as_str(), *p)).collect();
        let f = frames(&html, &treeRef, 1000);
        let wrapper = NS_NewTableWrapperFrame(f[1]);
        let table = NS_NewTableFrame(f[2]);
        // the classes
        let mut rows: Vec<nsTableRowFrame> = Vec::new();
        let mut cells: Vec<Vec<nsTableCellFrame>> = vec![Vec::new(); aRows.len()];
        for (i, &(kind, r, c)) in kinds.iter().enumerate() {
            match kind {
                'g' => {
                    NS_NewTableRowGroupFrame(f[i]);
                }
                'r' => rows.push(NS_NewTableRowFrame(f[i])),
                'c' => {
                    let cellFrame = NS_NewTableCellFrame(f[i], table);
                    cellFrame.SetSpans(aRows[r][c].rowspan, aRows[r][c].colspan);
                    cells[r].push(cellFrame);
                }
                'k' => {
                    if let Some((min, pref)) = aRows[r][c].intrinsic {
                        f[i].SetIntrinsicISizes(min * PX, pref * PX);
                    }
                }
                _ => {}
            }
        }
        // The frame constructor makes a frame, Inits it, and only later gives it its children
        // (SetInitialChildList); FrameTree::AddFrame already put the children in their parent's list, so
        // take them out until then.
        let mut lists: Vec<nsFrameList> = f.iter().map(|fr| std::mem::take(&mut *fr.mFrames())).collect();
        // Init, parents first
        for (i, &(kind, _, _)) in kinds.iter().enumerate() {
            let parent = tree[i].1.map(|p| f[p]);
            f[i].SetContent(Some(nsIContent::HTMLElement("div")));
            if let Some(p) = parent {
                f[i].SetParent(p);
            }
            match kind {
                'R' => nsBlockFrame::from_frame(f[i]).Init(f[i].GetContent(), f[i], None),
                't' => table.Init(f[i].GetContent(), parent.unwrap(), None),
                'g' => nsTableRowGroupFrame::from_frame(f[i]).Init(f[i].GetContent(), parent.unwrap(), None),
                'r' => nsTableRowFrame::from_frame(f[i]).Init(f[i].GetContent(), parent.unwrap(), None),
                'c' => nsTableCellFrame::from_frame(f[i]).Init(f[i].GetContent(), parent.unwrap(), None),
                'k' => nsBlockFrame::from_frame(f[i]).Init(f[i].GetContent(), parent.unwrap(), None),
                _ => {}
            }
        }
        // SetInitialChildList, leaves first
        for i in (0..kinds.len()).rev() {
            if kinds[i].0 == 'k' {
                continue;
            }
            let list = std::mem::take(&mut lists[i]);
            if list.NotEmpty() {
                f[i].SetInitialChildList(FrameChildListID::Principal, list);
            }
        }
        TestTable { root: f[0], wrapper, table, rows, cells }
    }

    // Reflows the root block (with the table wrapper in it) with this available inline-size (px) and
    // unconstrained block-size, and sets its size from the result, as its parent would. The returned
    // size is the inner table's.
    fn layout(aTable: &TestTable, aAvail: i32) -> LogicalSize {
        let ri = root_input(aTable.root, aAvail, AUTO);
        let wm = aTable.root.GetWritingMode();
        let mut metrics = ReflowOutput::new__ReflowInput(&ri);
        let mut status = nsReflowStatus::new();
        aTable.root.Reflow(&nsPresContext::Current(), &mut metrics, &ri, &mut status);
        assert!(status.IsFullyComplete());
        aTable.root.SetSize__WritingMode_LogicalSize(wm, metrics.Size(wm));
        aTable.table.GetLogicalSize__WritingMode(wm)
    }

    // The final inline-sizes of the table's columns in px.
    fn column_widths(aTable: &TestTable) -> Vec<i32> {
        (0..aTable.table.GetColCount()).map(|i| aTable.table.GetColFrame(i).unwrap().GetFinalISize() / PX).collect()
    }

    #[test]
    fn auto_layout_columns_fit_their_content() {
        // CSS 2.2 17.5.2.2: the column width is the largest minimum / maximum content width of its
        // cells; a table with width:auto is as wide as the sum of the maximum content widths.
        let t = build("", &[vec![cell(50, 10), cell(30, 10)]]);
        let m = layout(&t, 400);
        assert_eq!(column_widths(&t), vec![50, 30]);
        assert_eq!(m.ISize(t.table.GetWritingMode()), 80 * PX);
        assert_eq!(t.cells[0][0].GetPosition().x, 0);
        assert_eq!(t.cells[0][1].GetPosition().x, 50 * PX);
        assert_eq!(t.rows[0].GetSize().height, 10 * PX);
        assert_eq!(m.BSize(t.table.GetWritingMode()), 10 * PX);
    }

    #[test]
    fn border_spacing_surrounds_every_cell() {
        // CSS 2.2 17.6.1: in the separated borders model border-spacing is added between cells and
        // between the cells and the table's border edge (padding edge).
        let t = build("border-collapse:separate;border-spacing:2px", &[vec![cell(50, 10), cell(30, 10)], vec![cell(50, 10), cell(30, 10)]]);
        let m = layout(&t, 400);
        let wm = t.table.GetWritingMode();
        assert_eq!(m.ISize(wm), (2 + 50 + 2 + 30 + 2) * PX);
        assert_eq!(m.BSize(wm), (2 + 10 + 2 + 10 + 2) * PX);
        assert_eq!(t.cells[0][0].GetPosition().x, 0);
        let rg = t.rows[0].GetParent().unwrap();
        assert_eq!(rg.GetPosition(), nsPoint::new(2 * PX, 2 * PX));
        assert_eq!(t.rows[1].GetPosition().y, (10 + 2) * PX);
        assert_eq!(t.cells[0][1].GetPosition().x, (50 + 2) * PX);
        assert_eq!(t.table.GetColSpacing__int32_t_int32_t(-1, 2), 6 * PX);
    }

    #[test]
    fn collapsed_borders_table_ignores_border_spacing() {
        // CSS 2.2 17.6.2: border-spacing does not apply in the collapsing borders model. (The collapsed
        // border widths themselves are NOT PORTED, so this table has none.)
        let t = build("border-collapse:collapse;border-spacing:5px", &[vec![cell(50, 10), cell(30, 10)]]);
        let m = layout(&t, 400);
        assert!(t.table.IsBorderCollapse());
        assert_eq!(t.table.GetColSpacing__int32_t(0), 0);
        assert_eq!(m.ISize(t.table.GetWritingMode()), 80 * PX);
        assert_eq!(m.BSize(t.table.GetWritingMode()), 10 * PX);
    }

    #[test]
    fn cell_map_has_the_rows_and_columns_of_the_table() {
        // A cell with rowspan=2 in the first column makes a 2 x 2 grid out of 3 cells; the cell
        // map says it spans two rows.
        let t = build("", &[vec![cell(20, 10).span(1, 2), cell(20, 10)], vec![cell(20, 10)]]);
        assert_eq!(t.table.GetRowCount(), 2);
        assert_eq!(t.table.GetColCount(), 2);
        assert_eq!(t.table.GetEffectiveRowSpan__int32_t_nsTableCellFrame(0, &t.cells[0][0]), 2);
        assert_eq!(t.cells[1][0].ColIndex(), 1);
        assert_eq!(t.table.GetEffectiveColSpan(&t.cells[0][1], None), 1);
        // one anonymous column frame per column, in the synthetic column group
        assert_eq!(t.table.GetColFrame(0).unwrap().GetColType(), eColAnonymousCell);
        assert!(t.table.GetSyntheticColGroup().is_some());
    }

    #[test]
    fn colspan_cell_widens_the_columns_it_spans() {
        // CSS 2.2 17.5.2.2: a cell that spans columns needs the sum of its columns to be at least its
        // own width; the missing width is distributed to the columns.
        let t = build("", &[vec![cell(100, 10).span(2, 1)], vec![cell(20, 10), cell(20, 10)]]);
        let m = layout(&t, 400);
        assert_eq!(t.table.GetColCount(), 2);
        assert_eq!(column_widths(&t), vec![50, 50]);
        assert_eq!(m.ISize(t.table.GetWritingMode()), 100 * PX);
        assert_eq!(t.cells[0][0].GetSize().width, 100 * PX);
    }

    #[test]
    fn percent_column_gets_its_share_of_the_table() {
        // CSS 2.2 17.5.2.2: a column with a percentage width gets that share of a table with a
        // specified width; the other column takes what is left.
        let t = build("width:200px", &[vec![cell(10, 10).style("width:50%"), cell(40, 10)]]);
        layout(&t, 400);
        assert_eq!(column_widths(&t), vec![100, 100]);
    }

    #[test]
    fn auto_columns_share_extra_width_by_their_preferred_widths() {
        // When the table is wider than the sum of the maximum content widths the extra width goes to
        // the auto columns in proportion to their maximum content widths.
        let t = build("width:600px", &[vec![flex_cell(10, 100, 10), flex_cell(10, 200, 10)]]);
        layout(&t, 700);
        assert_eq!(column_widths(&t), vec![200, 400]);
    }

    #[test]
    fn narrow_table_is_squeezed_down_to_the_minimum_widths() {
        // With less room than the maximum content widths the columns get their minimum widths plus
        // a share of the rest; with width:auto the table shrinks to the available width.
        let t = build("", &[vec![flex_cell(20, 100, 10), flex_cell(40, 100, 10)]]);
        let m = layout(&t, 120);
        let widths: Vec<nscoord> = (0..t.table.GetColCount()).map(|i| t.table.GetColFrame(i).unwrap().GetFinalISize()).collect();
        assert_eq!(widths.iter().sum::<nscoord>(), 120 * PX);
        assert!(widths[0] >= 20 * PX && widths[1] >= 40 * PX);
        assert_eq!(m.ISize(t.table.GetWritingMode()), 120 * PX);
    }

    #[test]
    fn table_intrinsic_widths_are_the_sums_of_the_column_widths() {
        // The min-content width of a table is the sum of its columns' minimum widths and its
        // max-content width the sum of the maximum widths, plus the border-spacing.
        let t = build("border-spacing:3px", &[vec![flex_cell(20, 100, 10), flex_cell(40, 70, 10)]]);
        let input = IntrinsicSizeInput::new(Some(gfxContext), None, None);
        assert_eq!(t.table.GetMinISize(&input), (3 + 20 + 3 + 40 + 3) * PX);
        assert_eq!(t.table.GetPrefISize(&input), (3 + 100 + 3 + 70 + 3) * PX);
    }

    #[test]
    fn fixed_layout_uses_the_first_row_for_the_columns() {
        // CSS 2.2 17.5.2.1: with table-layout:fixed the column widths come from the first row (and the
        // col elements); auto columns share the width that is left of the table's width.
        let t = build("table-layout:fixed;width:300px", &[vec![cell(10, 10).style("width:100px"), cell(10, 10), cell(10, 10).style("width:50px")], vec![cell(200, 10), cell(10, 10), cell(10, 10)]]);
        let m = layout(&t, 600);
        assert_eq!(column_widths(&t), vec![100, 150, 50]);
        assert_eq!(m.ISize(t.table.GetWritingMode()), 300 * PX);
        assert_eq!(t.table.LayoutStrategy().unwrap().GetType(), nsITableLayoutStrategy_Type::Fixed);
    }

    #[test]
    fn table_height_is_the_sum_of_the_row_heights() {
        let t = build("", &[vec![cell(10, 10), cell(10, 25)], vec![cell(10, 5), cell(10, 5)]]);
        let m = layout(&t, 400);
        let wm = t.table.GetWritingMode();
        assert_eq!(t.rows[0].GetSize().height, 25 * PX);
        assert_eq!(t.rows[1].GetPosition().y, 25 * PX);
        assert_eq!(m.BSize(wm), 30 * PX);
    }

    #[test]
    fn rowspan_cell_stretches_over_its_rows() {
        // A cell with rowspan=2 that is taller than its first row makes the rows share the extra
        // block-size.
        let t = build("", &[vec![cell(10, 40).span(1, 2), cell(10, 10)], vec![cell(10, 10)]]);
        let m = layout(&t, 400);
        assert_eq!(m.BSize(t.table.GetWritingMode()), 40 * PX);
        assert_eq!(t.cells[0][0].GetSize().height, 40 * PX);
    }

    #[test]
    fn deleted_row_index_ranges_merge_neighbours() {
        // The ranges of deleted row indices of nsTableFrame::AddDeletedRowIndex merge consecutive
        // indices and a lookup sums the sizes of the ranges up to the index.
        let t = build("", &[vec![cell(10, 10)]]);
        let table = t.table;
        table.AddDeletedRowIndex(5);
        table.AddDeletedRowIndex(7);
        assert_eq!(table.mDeletedRowIndexRanges().len(), 2);
        table.AddDeletedRowIndex(6);
        assert_eq!(table.mDeletedRowIndexRanges().len(), 1);
        assert_eq!(table.mDeletedRowIndexRanges().get(&5), Some(&7));
        table.AddDeletedRowIndex(1);
        assert_eq!(table.GetAdjustmentForStoredIndex(4), 1);
        assert_eq!(table.GetAdjustmentForStoredIndex(9), 4);
        assert!(!table.IsDeletedRowIndexRangesEmpty());
    }

    #[test]
    fn ordered_groups_put_the_header_first_and_the_footer_last() {
        // thead is laid out before the tbody, tfoot after it, whatever the order in the markup.
        let html = r#"<div id=t style="display:table"><div id=f style="display:table-footer-group"></div><div id=b style="display:table-row-group"></div><div id=h style="display:table-header-group"></div></div>"#;
        let f = frames(html, &[("t", None), ("f", Some(0)), ("b", Some(0)), ("h", Some(0))], 100);
        let table = NS_NewTableFrame(f[0]);
        for i in 1..4 {
            NS_NewTableRowGroupFrame(f[i]);
        }
        let groups = table.OrderedGroups();
        assert_eq!(groups.mRowGroups.iter().map(|g| g.0).collect::<Vec<_>>(), vec![f[3], f[2], f[1]]);
        assert_eq!(groups.mHead.map(|g| g.0), Some(f[3]));
        assert_eq!(groups.mFoot.map(|g| g.0), Some(f[1]));
    }
}
