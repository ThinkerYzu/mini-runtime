// Translated from layout/tables/nsCellMap.h and nsCellMap.cpp (GECKO_REV), in the order of the header's
// declarations with the .cpp bodies.
//
// Rust shape (see CONVENTIONS.md):
//  - Gecko's cell map is a set of owning raw pointers: `nsTableCellMap` owns a singly linked list of
//    `nsCellMap` (one per row group, `mFirstMap` / `mNextSibling`), each `nsCellMap` owns its `CellData`
//    objects through `mRows` (an array of rows of `CellData*`, null for an empty slot), and the code
//    passes `nsTableCellMap&` and `this` around while it changes both. That shape is kept with raw
//    pointers: `*mut nsCellMap`, `*mut CellData` (null where C++ has nullptr), allocated with
//    `Box::into_raw` and freed with `Box::from_raw` (nsCellMap::AllocCellData / DestroyCellData,
//    nsTableCellMap::Drop). A BC cell map holds `BCCellData` (celldata.rs) behind the same pointer type.
//    A caller that reads an entry writes `unsafe { &*ptr }` (null checks first); the references made
//    inside this file do not outlive the call that made them.
//  - Frames are handles (`nsTableCellFrame(pub nsIFrame)` ...): `nsTableCellFrame*` that may be null is
//    `Option<nsTableCellFrame>`. `nsTArray<nsTableCellFrame*>&` is `Vec<nsTableCellFrame>`.
//  - Default arguments are written out: `nsCellMap::GetRowCount(bool)` takes `false` for the default,
//    `Grow` takes -1, and so on, as noted at each function.
//  - `nsTableFrame::RowGroupArray` is `nsTableFrame_RowGroupArray` (nsTableFrame.rs), a Vec of
//    `nsTableRowGroupFrame`; `nsTableFrame::OrderedRowGroups()` is called without its two optional
//    out-parameters.
//
// NOT PORTED: the debug dumps (`nsTableCellMap::Dump`, `nsCellMap::Dump`, `#ifdef DEBUG`, printf only),
// DEBUG_TABLE_CELLMAP, refcount logging (MOZ_COUNT_CTOR / MOZ_COUNT_DTOR), the PresShell arena that
// allocates CellData (Box here), the empty static row `sEmptyRow` (a bounds-checked read does the same),
// `nsCellMap::Init` and `Shutdown` (they only create and free that empty row), and the border-collapse
// painting iterators `BCMapCellIterator` and `BCPaintBorderIterator` (nsTableFrame.cpp). The BC data the
// cell map keeps (BCInfo, BCCellData, ResetBStartStart, SetBCBorderEdge, SetBCBorderCorner) is ported.
use crate::*;

// Gecko: layout/tables/nsCellMap.h nsColInfo
#[derive(Clone, Copy, Debug)]
pub struct nsColInfo {
    pub mNumCellsOrig: i32, // number of cells originating in the col
    pub mNumCellsSpan: i32, // number of cells spanning into the col via colspans
                            // (not rowspans)
}

impl nsColInfo {
    // Gecko: layout/tables/nsCellMap.h nsColInfo::nsColInfo()
    pub fn new__() -> nsColInfo {
        nsColInfo { mNumCellsOrig: 0, mNumCellsSpan: 0 }
    }

    // Gecko: layout/tables/nsCellMap.h nsColInfo::nsColInfo(int32_t, int32_t)
    pub fn new(aNumCellsOrig: i32, aNumCellsSpan: i32) -> nsColInfo {
        nsColInfo { mNumCellsOrig: aNumCellsOrig, mNumCellsSpan: aNumCellsSpan }
    }
}

impl Default for nsColInfo {
    fn default() -> nsColInfo {
        nsColInfo::new__()
    }
}

// Gecko: layout/tables/nsCellMap.h BCInfo
#[derive(Default)]
pub struct BCInfo {
    pub mIEndBorders: Vec<BCData>,
    pub mBEndBorders: Vec<BCData>,
    pub mBEndIEndCorner: BCData,
}

// Gecko: layout/tables/nsCellMap.h nsCellMap::CellDataArray
pub type nsCellMap_CellDataArray = Vec<*mut CellData>;

// ADAPTER: `*cellData` for a CellData pointer that is known to be non-null (the C++ dereferences
// it). The reference does not outlive the statement that uses it.
fn CellDataRef(aData: *mut CellData) -> &'static mut CellData {
    debug_assert!(!aData.is_null());
    unsafe { &mut *aData }
}

// ADAPTER: `*cellMap` for an nsCellMap pointer that is known to be non-null.
fn CellMapRef(aMap: *mut nsCellMap) -> &'static mut nsCellMap {
    debug_assert!(!aMap.is_null());
    unsafe { &mut *aMap }
}

// ADAPTER: `row.SafeElementAt(aIndex)` of an nsTArray<CellData*>: nullptr when out of range.
fn SafeElementAt(aRow: &nsCellMap_CellDataArray, aIndex: i32) -> *mut CellData {
    if aIndex < 0 {
        return std::ptr::null_mut();
    }
    aRow.get(aIndex as usize).copied().unwrap_or(std::ptr::null_mut())
}

// Gecko: layout/tables/nsCellMap.cpp SetDamageArea
fn SetDamageArea(aStartCol: i32, aStartRow: i32, aColCount: i32, aRowCount: i32, aDamageArea: &mut TableArea) {
    NS_ASSERTION!(aStartCol >= 0, "negative col index");
    NS_ASSERTION!(aStartRow >= 0, "negative row index");
    NS_ASSERTION!(aColCount >= 0, "negative col count");
    NS_ASSERTION!(aRowCount >= 0, "negative row count");
    *aDamageArea.StartCol_mut() = aStartCol;
    *aDamageArea.StartRow_mut() = aStartRow;
    *aDamageArea.ColCount_mut() = aColCount;
    *aDamageArea.RowCount_mut() = aRowCount;
}

// Gecko: layout/tables/nsCellMap.h nsTableCellMap
pub struct nsTableCellMap {
    // protected:
    // (C++ `nsTableFrame& mTableFrame`; a handle here)
    pub mTableFrame: nsTableFrame,
    // (C++ `AutoTArray<nsColInfo, 8>`)
    pub mCols: Vec<nsColInfo>,
    pub mFirstMap: *mut nsCellMap,
    // border collapsing info
    pub mBCInfo: Option<Box<BCInfo>>,
}

impl nsTableCellMap {
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::nsTableCellMap
    pub fn new(aTableFrame: nsTableFrame, aBorderCollapse: bool) -> nsTableCellMap {
        // NOT PORTED: MOZ_COUNT_CTOR(nsTableCellMap)
        let mut this = nsTableCellMap { mTableFrame: aTableFrame, mCols: Vec::new(), mFirstMap: std::ptr::null_mut(), mBCInfo: None };

        let orderedRowGroups: nsTableFrame_RowGroupArray = aTableFrame.OrderedRowGroups();

        let mut prior: Option<nsTableRowGroupFrame> = None;
        for rgX in 0..orderedRowGroups.len() {
            let rgFrame = orderedRowGroups[rgX];
            this.InsertGroupCellMap__nsTableRowGroupFrame_nsTableRowGroupFrame(rgFrame, &mut prior);
            prior = Some(rgFrame);
        }
        if aBorderCollapse {
            this.mBCInfo = Some(Box::new(BCInfo::default()));
        }
        this
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::RemoveGroupCellMap
    pub fn RemoveGroupCellMap(&mut self, aGroup: nsTableRowGroupFrame) {
        let mut map = self.mFirstMap;
        let mut prior: *mut nsCellMap = std::ptr::null_mut();
        while !map.is_null() {
            if CellMapRef(map).GetRowGroup() == aGroup {
                let next = CellMapRef(map).GetNextSibling();
                if self.mFirstMap == map {
                    self.mFirstMap = next;
                } else {
                    CellMapRef(prior).SetNextSibling(next);
                }
                // delete map;
                drop(unsafe { Box::from_raw(map) });
                break;
            }
            prior = map;
            map = CellMapRef(map).GetNextSibling();
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::InsertGroupCellMap
    // (the overload that makes the cell map: C++ `InsertGroupCellMap(nsTableRowGroupFrame*,
    // nsTableRowGroupFrame*&)`)
    pub fn InsertGroupCellMap__nsTableRowGroupFrame_nsTableRowGroupFrame(&mut self, aNewGroup: nsTableRowGroupFrame, aPrevGroup: &mut Option<nsTableRowGroupFrame>) {
        let newMap: *mut nsCellMap = Box::into_raw(Box::new(nsCellMap::new(aNewGroup, self.mBCInfo.is_some())));
        let mut prevMap: *mut nsCellMap = std::ptr::null_mut();
        let mut lastMap = self.mFirstMap;
        if aPrevGroup.is_some() {
            let mut map = self.mFirstMap;
            while !map.is_null() {
                lastMap = map;
                if Some(CellMapRef(map).GetRowGroup()) == *aPrevGroup {
                    prevMap = map;
                    break;
                }
                map = CellMapRef(map).GetNextSibling();
            }
        }
        if prevMap.is_null() {
            if aPrevGroup.is_some() {
                prevMap = lastMap;
                *aPrevGroup = if !prevMap.is_null() { Some(CellMapRef(prevMap).GetRowGroup()) } else { None };
            } else {
                *aPrevGroup = None;
            }
        }
        self.InsertGroupCellMap__nsCellMap_nsCellMap(prevMap, newMap);
    }

    /**
     * Get the nsCellMap for the given row group.  If aStartHint is non-null,
     * will start looking with that cellmap and only fall back to starting at the
     * beginning of the list if that doesn't find us the right nsCellMap.
     * Otherwise, just start at the beginning.
     *
     * aRowGroup must not be null.
     */
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetMapFor
    pub fn GetMapFor(&self, aRowGroup: nsTableRowGroupFrame, aStartHint: *mut nsCellMap) -> *mut nsCellMap {
        NS_ASSERTION!(aRowGroup.GetPrevInFlow().is_none(), "GetMapFor called with continuation");
        if !aStartHint.is_null() {
            let map = FindMapFor(aRowGroup, aStartHint, std::ptr::null());
            if !map.is_null() {
                return map;
            }
        }

        let map = FindMapFor(aRowGroup, self.mFirstMap, aStartHint);
        if !map.is_null() {
            return map;
        }

        // If aRowGroup is a repeated header or footer find the header or footer it
        // was repeated from.
        // Bug 1442018: we also need this search for header/footer frames that are
        // not marked as _repeatable_ because they have a next-in-flow, as they may
        // nevertheless have been _repeated_ from an earlier fragment.
        let isTableHeaderFooterGroup = |aRG: nsTableRowGroupFrame| -> bool {
            let display = aRG.StyleDisplay().mDisplay();
            display == StyleDisplay::TableHeaderGroup || display == StyleDisplay::TableFooterGroup
        };
        if aRowGroup.IsRepeatable() || (aRowGroup.GetNextInFlow().is_some() && isTableHeaderFooterGroup(aRowGroup)) {
            let findOtherRowGroupOfType = |aTable: nsTableFrame| -> Option<nsTableRowGroupFrame> {
                let display = aRowGroup.StyleDisplay().mDisplay();
                let mut table = Some(nsTableFrame::from_frame(aTable.FirstContinuation()));
                while let Some(t) = table {
                    // for (auto* child : table->PrincipalChildList())
                    let mut child = t.mFrames().FirstChild();
                    while let Some(c) = child {
                        if c.StyleDisplay().mDisplay() == display && c != aRowGroup.0 {
                            return Some(nsTableRowGroupFrame::from_frame(c));
                        }
                        child = c.GetNextSibling();
                    }
                    table = t.GetNextContinuation().map(nsTableFrame::from_frame);
                }
                None
            };
            if let Some(rgOrig) = findOtherRowGroupOfType(self.mTableFrame) {
                return self.GetMapFor(rgOrig, aStartHint);
            }
            MOZ_ASSERT_UNREACHABLE!("A repeated header/footer should always have an original header/footer it was repeated from");
        }

        std::ptr::null_mut()
    }

    /** synchronize the cellmaps with the rowgroups again **/
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::Synchronize
    pub fn Synchronize(&mut self, aTableFrame: nsTableFrame) {
        let mut maps: Vec<*mut nsCellMap> = Vec::new();

        let orderedRowGroups: nsTableFrame_RowGroupArray = aTableFrame.OrderedRowGroups();
        if orderedRowGroups.is_empty() {
            return;
        }

        // XXXbz this fails if orderedRowGroups is missing some row groups
        // (due to OOM when appending to the array, e.g. -- we leak maps in
        // that case).

        // Scope |map| outside the loop so we can use it as a hint.
        let mut map: *mut nsCellMap = std::ptr::null_mut();
        for rgX in 0..orderedRowGroups.len() {
            let rgFrame = orderedRowGroups[rgX];
            map = self.GetMapFor(nsTableRowGroupFrame::from_frame(rgFrame.FirstInFlow()), map);
            if !map.is_null() {
                // XXX(Bug 1631371) Check if this should use a fallible operation as it
                // pretended earlier, or change the return type to void.
                maps.push(map);
            }
        }
        if maps.is_empty() {
            MOZ_ASSERT!(self.mFirstMap.is_null());
            return;
        }

        let mut mapIndex = maps.len() as i32 - 1; // Might end up -1
        let mut nextMap = maps[mapIndex as usize];
        CellMapRef(nextMap).SetNextSibling(std::ptr::null_mut());
        mapIndex -= 1;
        while mapIndex >= 0 {
            let map = maps[mapIndex as usize];
            CellMapRef(map).SetNextSibling(nextMap);
            nextMap = map;
            mapIndex -= 1;
        }
        self.mFirstMap = nextMap;
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetCellFrame
    pub fn GetCellFrame(&self, aRowIndex: i32, aColIndex: i32, aData: &CellData, aUseRowIfOverlap: bool) -> Option<nsTableCellFrame> {
        let mut rowIndex = aRowIndex;
        let mut map = self.mFirstMap;
        while !map.is_null() {
            if CellMapRef(map).GetRowCount(false) > rowIndex {
                return CellMapRef(map).GetCellFrame(rowIndex, aColIndex, aData, aUseRowIfOverlap);
            }
            rowIndex -= CellMapRef(map).GetRowCount(false);
            map = CellMapRef(map).GetNextSibling();
        }
        None
    }

    /** return the CellData for the cell at (aRowIndex, aColIndex) */
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetDataAt
    pub fn GetDataAt(&self, aRowIndex: i32, aColIndex: i32) -> *mut CellData {
        let mut rowIndex = aRowIndex;
        let mut map = self.mFirstMap;
        while !map.is_null() {
            if CellMapRef(map).GetRowCount(false) > rowIndex {
                return CellMapRef(map).GetDataAt(rowIndex, aColIndex);
            }
            rowIndex -= CellMapRef(map).GetRowCount(false);
            map = CellMapRef(map).GetNextSibling();
        }
        std::ptr::null_mut()
    }

    // this function creates a col if needed
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetColInfoAt
    // ADAPTER: never null (the C++ returns `&mCols.ElementAt(aColIndex)` after adding the column), so
    // it is a reference; it is valid until the next call that adds or removes columns.
    pub fn GetColInfoAt(&mut self, aColIndex: i32) -> &'static mut nsColInfo {
        let numColsToAdd = aColIndex + 1 - self.mCols.len() as i32;
        if numColsToAdd > 0 {
            self.AddColsAtEnd(numColsToAdd as u32); // XXX this could fail to add cols in theory
        }
        unsafe { &mut *(&mut self.mCols[aColIndex as usize] as *mut nsColInfo) }
    }

    /** append the cellFrame at the end of the row at aRowIndex and return the col
     * index
     */
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::AppendCell
    pub fn AppendCell(&mut self, aCellFrame: nsTableCellFrame, aRowIndex: i32, aRebuildIfNecessary: bool, aDamageArea: &mut TableArea) -> *mut CellData {
        MOZ_ASSERT!(aCellFrame.0 == aCellFrame.FirstInFlow(), "invalid call on continuing frame");
        let Some(row) = aCellFrame.GetParent() else {
            // get the row
            return std::ptr::null_mut();
        };
        let Some(rgFrame) = row.GetParent() else {
            // get the row group
            return std::ptr::null_mut();
        };

        let mut result: *mut CellData = std::ptr::null_mut();
        let mut rowIndex = aRowIndex;
        let mut rgStartRowIndex = 0;
        let mut cellMap = self.mFirstMap;
        while !cellMap.is_null() {
            if CellMapRef(cellMap).GetRowGroup().0 == rgFrame {
                result = CellMapRef(cellMap).AppendCell(self, Some(aCellFrame), rowIndex, aRebuildIfNecessary, rgStartRowIndex, aDamageArea, None);
                break;
            }
            let rowCount = CellMapRef(cellMap).GetRowCount(false);
            rgStartRowIndex += rowCount;
            rowIndex -= rowCount;
            cellMap = CellMapRef(cellMap).GetNextSibling();
        }
        // NOT PORTED: #ifdef DEBUG_TABLE_CELLMAP Dump("after AppendCell");
        result
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::InsertCells
    pub fn InsertCells(&mut self, aCellFrames: &mut Vec<nsTableCellFrame>, aRowIndex: i32, aColIndexBefore: i32, aDamageArea: &mut TableArea) {
        let mut rowIndex = aRowIndex;
        let mut rgStartRowIndex = 0;
        let mut cellMap = self.mFirstMap;
        while !cellMap.is_null() {
            let rowCount = CellMapRef(cellMap).GetRowCount(false);
            if rowCount > rowIndex {
                CellMapRef(cellMap).InsertCells(self, aCellFrames, rowIndex, aColIndexBefore, rgStartRowIndex, aDamageArea);
                break;
            }
            rgStartRowIndex += rowCount;
            rowIndex -= rowCount;
            cellMap = CellMapRef(cellMap).GetNextSibling();
        }
        // NOT PORTED: #ifdef DEBUG_TABLE_CELLMAP Dump("after InsertCells");
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::RemoveCell
    pub fn RemoveCell(&mut self, aCellFrame: Option<nsTableCellFrame>, aRowIndex: i32, aDamageArea: &mut TableArea) {
        let Some(aCellFrame) = aCellFrame else {
            // if (!aCellFrame) ABORT0();
            NS_ASSERTION!(false, "CellIterator program error");
            return;
        };
        MOZ_ASSERT!(aCellFrame.0 == aCellFrame.FirstInFlow(), "invalid call on continuing frame");
        let mut rowIndex = aRowIndex;
        let mut rgStartRowIndex = 0;
        let mut cellMap = self.mFirstMap;
        while !cellMap.is_null() {
            let rowCount = CellMapRef(cellMap).GetRowCount(false);
            if rowCount > rowIndex {
                CellMapRef(cellMap).RemoveCell(self, aCellFrame, rowIndex, rgStartRowIndex, aDamageArea);
                // NOT PORTED: #ifdef DEBUG_TABLE_CELLMAP Dump("after RemoveCell");
                return;
            }
            rgStartRowIndex += rowCount;
            rowIndex -= rowCount;
            cellMap = CellMapRef(cellMap).GetNextSibling();
        }
        // if we reach this point - the cell did not get removed, the caller of this
        // routine will delete the cell and the cellmap will probably hold a reference
        // to the deleted cell which will cause a subsequent crash when this cell is
        // referenced later
        NS_ERROR!("nsTableCellMap::RemoveCell - could not remove cell");
    }

    /** Remove the previously gathered column information */
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::ClearCols
    pub fn ClearCols(&mut self) {
        self.mCols.clear();
        if let Some(bcInfo) = self.mBCInfo.as_mut() {
            bcInfo.mBEndBorders.clear();
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::InsertRows
    pub fn InsertRows(&mut self, aParent: nsTableRowGroupFrame, aRows: &mut Vec<nsTableRowFrame>, aFirstRowIndex: i32, aConsiderSpans: bool, aDamageArea: &mut TableArea) {
        let numNewRows = aRows.len() as i32;
        if (numNewRows <= 0) || (aFirstRowIndex < 0) {
            // ABORT0();
            NS_ASSERTION!(false, "CellIterator program error");
            return;
        }

        let mut rowIndex = aFirstRowIndex;
        let mut rgStartRowIndex = 0;
        let mut cellMap = self.mFirstMap;
        while !cellMap.is_null() {
            let rg = CellMapRef(cellMap).GetRowGroup();
            if rg == aParent {
                CellMapRef(cellMap).InsertRows(self, aRows, rowIndex, aConsiderSpans, rgStartRowIndex, aDamageArea);
                // NOT PORTED: #ifdef DEBUG_TABLE_CELLMAP Dump("after InsertRows");
                if self.mBCInfo.is_some() {
                    let count = self.mBCInfo.as_ref().unwrap().mIEndBorders.len() as i32;
                    if aFirstRowIndex < count {
                        for rowX in aFirstRowIndex..aFirstRowIndex + numNewRows {
                            self.mBCInfo.as_mut().unwrap().mIEndBorders.insert(rowX as usize, BCData::new());
                        }
                    } else {
                        self.GetIEndMostBorder(aFirstRowIndex); // this will create missing entries
                        for rowX in aFirstRowIndex + 1..aFirstRowIndex + numNewRows {
                            self.mBCInfo.as_mut().unwrap().mIEndBorders.push(BCData::new());
                        }
                    }
                }
                return;
            }
            let rowCount = CellMapRef(cellMap).GetRowCount(false);
            rgStartRowIndex += rowCount;
            rowIndex -= rowCount;
            cellMap = CellMapRef(cellMap).GetNextSibling();
        }

        NS_ERROR!("Attempt to insert row into wrong map.");
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::RemoveRows
    pub fn RemoveRows(&mut self, aFirstRowIndex: i32, aNumRowsToRemove: i32, aConsiderSpans: bool, aDamageArea: &mut TableArea) {
        let mut rowIndex = aFirstRowIndex;
        let mut rgStartRowIndex = 0;
        let mut cellMap = self.mFirstMap;
        while !cellMap.is_null() {
            let rowCount = CellMapRef(cellMap).GetRowCount(false);
            if rowCount > rowIndex {
                CellMapRef(cellMap).RemoveRows(self, rowIndex, aNumRowsToRemove, aConsiderSpans, rgStartRowIndex, aDamageArea);
                if let Some(bcInfo) = self.mBCInfo.as_mut() {
                    let mut rowX = aFirstRowIndex + aNumRowsToRemove - 1;
                    while rowX >= aFirstRowIndex {
                        if (rowX as u32 as usize) < bcInfo.mIEndBorders.len() {
                            bcInfo.mIEndBorders.remove(rowX as usize);
                        }
                        rowX -= 1;
                    }
                }
                break;
            }
            rgStartRowIndex += rowCount;
            rowIndex -= rowCount;
            cellMap = CellMapRef(cellMap).GetNextSibling();
        }
        // NOT PORTED: #ifdef DEBUG_TABLE_CELLMAP Dump("after RemoveRows");
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetNumCellsOriginatingInRow
    pub fn GetNumCellsOriginatingInRow(&self, aRowIndex: i32) -> i32 {
        let mut rowIndex = aRowIndex;
        let mut map = self.mFirstMap;
        while !map.is_null() {
            if CellMapRef(map).GetRowCount(false) > rowIndex {
                return CellMapRef(map).GetNumCellsOriginatingInRow(rowIndex);
            }
            rowIndex -= CellMapRef(map).GetRowCount(false);
            map = CellMapRef(map).GetNextSibling();
        }
        0
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetNumCellsOriginatingInCol
    pub fn GetNumCellsOriginatingInCol(&self, aColIndex: i32) -> i32 {
        let colCount = self.mCols.len() as i32;
        if (aColIndex >= 0) && (aColIndex < colCount) {
            self.mCols[aColIndex as usize].mNumCellsOrig
        } else {
            NS_ERROR!("nsCellMap::GetNumCellsOriginatingInCol - bad col index");
            0
        }
    }

    /** indicate whether the row has more than one cell that either originates
     * or is spanned from the rows above
     */
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::HasMoreThanOneCell
    pub fn HasMoreThanOneCell(&self, aRowIndex: i32) -> bool {
        let mut rowIndex = aRowIndex;
        let mut map = self.mFirstMap;
        while !map.is_null() {
            if CellMapRef(map).GetRowCount(false) > rowIndex {
                return CellMapRef(map).HasMoreThanOneCell(rowIndex);
            }
            rowIndex -= CellMapRef(map).GetRowCount(false);
            map = CellMapRef(map).GetNextSibling();
        }
        false
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetEffectiveRowSpan
    pub fn GetEffectiveRowSpan(&self, aRowIndex: i32, aColIndex: i32) -> i32 {
        let mut rowIndex = aRowIndex;
        let mut map = self.mFirstMap;
        while !map.is_null() {
            if CellMapRef(map).GetRowCount(false) > rowIndex {
                return CellMapRef(map).GetRowSpan(rowIndex, aColIndex, true);
            }
            rowIndex -= CellMapRef(map).GetRowCount(false);
            map = CellMapRef(map).GetNextSibling();
        }
        MOZ_ASSERT_UNREACHABLE!("Bogus row index?");
        0
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetEffectiveColSpan
    pub fn GetEffectiveColSpan(&self, aRowIndex: i32, aColIndex: i32) -> i32 {
        let mut rowIndex = aRowIndex;
        let mut map = self.mFirstMap;
        while !map.is_null() {
            if CellMapRef(map).GetRowCount(false) > rowIndex {
                return CellMapRef(map).GetEffectiveColSpan(self, rowIndex, aColIndex);
            }
            rowIndex -= CellMapRef(map).GetRowCount(false);
            map = CellMapRef(map).GetNextSibling();
        }
        MOZ_ASSERT_UNREACHABLE!("Bogus row index?");
        0
    }

    /** return the total number of columns in the table represented by this
     * CellMap */
    // Gecko: layout/tables/nsCellMap.h nsTableCellMap::GetColCount
    pub fn GetColCount(&self) -> i32 {
        self.mCols.len() as i32
    }

    /** return the actual number of rows in the table represented by this CellMap
     */
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetRowCount
    pub fn GetRowCount(&self) -> i32 {
        let mut numRows = 0;
        let mut map = self.mFirstMap;
        while !map.is_null() {
            numRows += CellMapRef(map).GetRowCount(false);
            map = CellMapRef(map).GetNextSibling();
        }
        numRows
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetCellInfoAt
    // (C++ default arguments `aOriginates = nullptr, aColSpan = nullptr` are `None`)
    pub fn GetCellInfoAt(&self, aRowIndex: i32, aColIndex: i32, aOriginates: Option<&mut bool>, aColSpan: Option<&mut i32>) -> Option<nsTableCellFrame> {
        let mut rowIndex = aRowIndex;
        let mut cellMap = self.mFirstMap;
        while !cellMap.is_null() {
            if CellMapRef(cellMap).GetRowCount(false) > rowIndex {
                return CellMapRef(cellMap).GetCellInfoAt(self, rowIndex, aColIndex, aOriginates, aColSpan);
            }
            rowIndex -= CellMapRef(cellMap).GetRowCount(false);
            cellMap = CellMapRef(cellMap).GetNextSibling();
        }
        None
    }

    /**
     * Returns the index at the given row and column coordinates.
     *
     * @see  nsITableLayout::GetIndexByRowAndColumn()
     *
     * @param aRow     [in] the row coordinate
     * @param aColumn  [in] the column coordinate
     * @returns             the index for the cell
     */
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetIndexByRowAndColumn
    pub fn GetIndexByRowAndColumn(&self, aRow: i32, aColumn: i32) -> i32 {
        let mut index = 0;

        let colCount = self.mCols.len() as i32;
        let mut rowIndex = aRow;

        let mut cellMap = self.mFirstMap;
        while !cellMap.is_null() {
            let rowCount = CellMapRef(cellMap).GetRowCount(false);
            if rowIndex >= rowCount {
                // If the rowCount is less than the rowIndex, this means that the index is
                // not within the current map. If so, get the index of the last cell in
                // the last row.
                rowIndex -= rowCount;

                let cellMapIdx = CellMapRef(cellMap).GetHighestIndex(colCount);
                if cellMapIdx != -1 {
                    index += cellMapIdx + 1;
                }
            } else {
                // Index is in valid range for this cellmap, so get the index of rowIndex
                // and aColumn.
                let cellMapIdx = CellMapRef(cellMap).GetIndexByRowAndColumn(colCount, rowIndex, aColumn);
                if cellMapIdx == -1 {
                    return -1; // no cell at the given row and column.
                }

                index += cellMapIdx;
                return index; // no need to look through further maps here
            }

            cellMap = CellMapRef(cellMap).GetNextSibling();
        }

        -1
    }

    /**
     * Retrieves the row and column coordinates for the given index.
     *
     * @see  nsITableLayout::GetRowAndColumnByIndex()
     *
     * @param aIndex  [in] the index for which coordinates are to be retrieved
     * @param aRow    [out] the row coordinate to be returned
     * @param aColumn [out] the column coordinate to be returned
     */
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetRowAndColumnByIndex
    pub fn GetRowAndColumnByIndex(&self, aIndex: i32, aRow: &mut i32, aColumn: &mut i32) {
        *aRow = -1;
        *aColumn = -1;

        let colCount = self.mCols.len() as i32;

        let mut previousRows = 0;
        let mut index = aIndex;

        let mut cellMap = self.mFirstMap;
        while !cellMap.is_null() {
            let rowCount = CellMapRef(cellMap).GetRowCount(false);
            // Determine the highest possible index in this map to see
            // if wanted index is in here.
            let cellMapIdx = CellMapRef(cellMap).GetHighestIndex(colCount);
            if cellMapIdx == -1 {
                // The index is not within this map, increase the total row index
                // accordingly.
                previousRows += rowCount;
            } else if index > cellMapIdx {
                // The index is not within this map, so decrease it by the cellMapIdx
                // determined index and increase the total row index accordingly.
                index -= cellMapIdx + 1;
                previousRows += rowCount;
            } else {
                CellMapRef(cellMap).GetRowAndColumnByIndex(colCount, index, aRow, aColumn);
                // If there were previous indexes, take them into account.
                *aRow += previousRows;
                return; // no need to look any further.
            }

            cellMap = CellMapRef(cellMap).GetNextSibling();
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::AddColsAtEnd
    pub fn AddColsAtEnd(&mut self, aNumCols: u32) {
        // XXX(Bug 1631371) Check if this should use a fallible operation as it
        // pretended earlier.
        for _ in 0..aNumCols {
            self.mCols.push(nsColInfo::new__());
        }
        if let Some(bcInfo) = self.mBCInfo.as_mut() {
            // XXX(Bug 1631371) Check if this should use a fallible operation as it
            // pretended earlier.
            for _ in 0..aNumCols {
                bcInfo.mBEndBorders.push(BCData::new());
            }
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::RemoveColsAtEnd
    pub fn RemoveColsAtEnd(&mut self) {
        // Remove the cols at the end which don't have originating cells or cells
        // spanning into them. Only do this if the col was created as
        // eColAnonymousCell
        let numCols = self.GetColCount();
        let lastGoodColIndex = self.mTableFrame.GetIndexOfLastRealCol();
        MOZ_ASSERT!(lastGoodColIndex >= -1);
        let mut colX = numCols - 1;
        while colX > lastGoodColIndex {
            let colInfo = self.mCols[colX as usize];
            if (colInfo.mNumCellsOrig <= 0) && (colInfo.mNumCellsSpan <= 0) {
                self.mCols.remove(colX as usize);

                if let Some(bcInfo) = self.mBCInfo.as_mut() {
                    let count = bcInfo.mBEndBorders.len() as i32;
                    if colX < count {
                        bcInfo.mBEndBorders.remove(colX as usize);
                    }
                }
            } else {
                break; // only remove until we encounter the 1st valid one
            }
            colX -= 1;
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::RowIsSpannedInto
    pub fn RowIsSpannedInto(&self, aRowIndex: i32, aNumEffCols: i32) -> bool {
        let mut rowIndex = aRowIndex;
        let mut cellMap = self.mFirstMap;
        while !cellMap.is_null() {
            if CellMapRef(cellMap).GetRowCount(false) > rowIndex {
                return CellMapRef(cellMap).RowIsSpannedInto(rowIndex, aNumEffCols);
            }
            rowIndex -= CellMapRef(cellMap).GetRowCount(false);
            cellMap = CellMapRef(cellMap).GetNextSibling();
        }
        false
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::RowHasSpanningCells
    pub fn RowHasSpanningCells(&self, aRowIndex: i32, aNumEffCols: i32) -> bool {
        let mut rowIndex = aRowIndex;
        let mut cellMap = self.mFirstMap;
        while !cellMap.is_null() {
            if CellMapRef(cellMap).GetRowCount(false) > rowIndex {
                return CellMapRef(cellMap).RowHasSpanningCells(rowIndex, aNumEffCols);
            }
            rowIndex -= CellMapRef(cellMap).GetRowCount(false);
            cellMap = CellMapRef(cellMap).GetNextSibling();
        }
        false
    }

    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::RebuildConsideringCells
    // (`aCellMap` is compared by address with the maps of the list, so it is a pointer)
    pub fn RebuildConsideringCells(&mut self, aCellMap: *mut nsCellMap, aCellFrames: Option<&Vec<nsTableCellFrame>>, aRowIndex: i32, aColIndex: i32, aInsert: bool, aDamageArea: &mut TableArea) {
        let numOrigCols = self.GetColCount();
        self.ClearCols();
        let mut cellMap = self.mFirstMap;
        let mut rowCount = 0;
        while !cellMap.is_null() {
            if cellMap == aCellMap {
                CellMapRef(cellMap).RebuildConsideringCells(self, numOrigCols, aCellFrames, aRowIndex, aColIndex, aInsert);
            } else {
                CellMapRef(cellMap).RebuildConsideringCells(self, numOrigCols, None, -1, 0, false);
            }
            rowCount += CellMapRef(cellMap).GetRowCount(false);
            cellMap = CellMapRef(cellMap).GetNextSibling();
        }
        SetDamageArea(0, 0, self.GetColCount(), rowCount, aDamageArea);
    }

    /**
     * Rebuild due to rows being inserted or deleted with cells spanning
     * into or out of the rows.  This function can only handle insertion
     * or deletion but NOT both.  So either aRowsToInsert must be null
     * or aNumRowsToRemove must be 0.
     *
     * // XXXbz are both allowed to happen?  That'd be a no-op...
     */
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::RebuildConsideringRows
    pub fn RebuildConsideringRows(&mut self, aCellMap: *mut nsCellMap, aStartRowIndex: i32, aRowsToInsert: Option<&Vec<nsTableRowFrame>>, aNumRowsToRemove: i32, aDamageArea: &mut TableArea) {
        MOZ_ASSERT!(aRowsToInsert.is_none() || aNumRowsToRemove == 0, "Can't handle both removing and inserting rows at once");

        let numOrigCols = self.GetColCount();
        self.ClearCols();
        let mut cellMap = self.mFirstMap;
        let mut rowCount = 0;
        while !cellMap.is_null() {
            if cellMap == aCellMap {
                CellMapRef(cellMap).RebuildConsideringRows(self, aStartRowIndex, aRowsToInsert, aNumRowsToRemove);
            } else {
                CellMapRef(cellMap).RebuildConsideringCells(self, numOrigCols, None, -1, 0, false);
            }
            rowCount += CellMapRef(cellMap).GetRowCount(false);
            cellMap = CellMapRef(cellMap).GetNextSibling();
        }
        SetDamageArea(0, 0, self.GetColCount(), rowCount, aDamageArea);
    }

    // FIXME: The only value callers pass for aSide is LogicalSide::BEnd.
    // Consider removing support for the other three values.
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::ResetBStartStart
    pub fn ResetBStartStart(&mut self, aSide: LogicalSide, aCellMap: &mut nsCellMap, aRowGroupStart: u32, mut aRowIndex: u32, mut aColIndex: u32) {
        if self.mBCInfo.is_none() {
            // ABORT0();
            NS_ASSERTION!(false, "CellIterator program error");
            return;
        }

        let mut cellData: *mut BCCellData;
        let mut bcData: *mut BCData = std::ptr::null_mut();

        // (the C++ switch falls through BEnd into BStart and IEnd into IStart)
        match aSide {
            LogicalSide::BEnd | LogicalSide::BStart => {
                if aSide == LogicalSide::BEnd {
                    aRowIndex += 1;
                }
                cellData = aCellMap.GetDataAt(aRowIndex as i32 - aRowGroupStart as i32, aColIndex as i32) as *mut BCCellData;
                if !cellData.is_null() {
                    bcData = unsafe { &mut (*cellData).mData };
                } else {
                    NS_ASSERTION!(aSide == LogicalSide::BEnd, "program error");
                    // try the next row group
                    let cellMap = aCellMap.GetNextSibling();
                    if !cellMap.is_null() {
                        cellData = CellMapRef(cellMap).GetDataAt(0, aColIndex as i32) as *mut BCCellData;
                        if !cellData.is_null() {
                            bcData = unsafe { &mut (*cellData).mData };
                        } else {
                            bcData = self.GetBEndMostBorder(aColIndex as i32);
                        }
                    }
                }
            }
            LogicalSide::IEnd | LogicalSide::IStart => {
                if aSide == LogicalSide::IEnd {
                    aColIndex += 1;
                }
                cellData = aCellMap.GetDataAt(aRowIndex as i32 - aRowGroupStart as i32, aColIndex as i32) as *mut BCCellData;
                if !cellData.is_null() {
                    bcData = unsafe { &mut (*cellData).mData };
                } else {
                    NS_ASSERTION!(aSide == LogicalSide::IEnd, "program error");
                    bcData = self.GetIEndMostBorder(aRowIndex as i32);
                }
            }
        }
        if !bcData.is_null() {
            unsafe { &mut *bcData }.SetBStartStart(false);
        }
    }

    // store the aSide border segment at coord = (aRowIndex, aColIndex). For
    // bStart/iStart, store the info at coord. For bEnd/iEnd store it at the
    // adjacent location so that it is bStart/iStart at that location. If the new
    // location is at the iEnd or bEnd edge of the table, then store it one of the
    // special arrays (iEnd-most borders, bEnd-most borders).
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::SetBCBorderEdge
    pub fn SetBCBorderEdge(&mut self, aSide: LogicalSide, aCellMap: &mut nsCellMap, aCellMapStart: u32, aRowIndex: u32, aColIndex: u32, aLength: u32, aOwner: BCBorderOwner, aSize: nscoord, aChanged: bool) {
        if self.mBCInfo.is_none() {
            // ABORT0();
            NS_ASSERTION!(false, "CellIterator program error");
            return;
        }

        let mut cellData: *mut BCCellData = std::ptr::null_mut();
        let mut xPos = aColIndex as i32;
        let mut rgYPos = aRowIndex as i32 - aCellMapStart as i32;

        match aSide {
            LogicalSide::BEnd | LogicalSide::BStart => {
                if aSide == LogicalSide::BEnd {
                    rgYPos += 1;
                }
                let lastIndex = xPos + aLength as i32 - 1;
                for xIndex in xPos..=lastIndex {
                    let changed = aChanged && (xIndex == xPos);
                    let mut bcData: *mut BCData = std::ptr::null_mut();
                    if rgYPos < aCellMap.GetRowCount(false) {
                        cellData = aCellMap.GetDataAt(rgYPos, xIndex) as *mut BCCellData;
                        if cellData.is_null() {
                            // add a dead cell data
                            let mut damageArea = TableArea::default();
                            cellData = aCellMap.AppendCell(self, None, rgYPos, false, 0, &mut damageArea, None) as *mut BCCellData;
                            if cellData.is_null() {
                                // ABORT0();
                                NS_ASSERTION!(false, "CellIterator program error");
                                return;
                            }
                        }
                    } else {
                        // We are past this row group's content rows (note that mRows might
                        // have an entry there for a rowspan extending past the row group).
                        // Go to the next non-empty row group.
                        NS_ASSERTION!(aSide == LogicalSide::BEnd, "program error");
                        let mut cellMap = aCellMap.GetNextSibling();
                        while !cellMap.is_null() && (0 == CellMapRef(cellMap).GetRowCount(false)) {
                            cellMap = CellMapRef(cellMap).GetNextSibling();
                        }
                        if !cellMap.is_null() {
                            cellData = CellMapRef(cellMap).GetDataAt(0, xIndex) as *mut BCCellData;
                            if cellData.is_null() {
                                // add a dead cell
                                let mut damageArea = TableArea::default();
                                cellData = CellMapRef(cellMap).AppendCell(self, None, 0, false, 0, &mut damageArea, None) as *mut BCCellData;
                            }
                        } else {
                            // must be at the end of the table
                            bcData = self.GetBEndMostBorder(xIndex);
                        }
                    }
                    if bcData.is_null() && !cellData.is_null() {
                        bcData = unsafe { &mut (*cellData).mData };
                    }
                    if !bcData.is_null() {
                        unsafe { &mut *bcData }.SetBStartEdge(aOwner, aSize, changed);
                    } else {
                        NS_ERROR!("Cellmap: BStart edge not found");
                    }
                }
            }
            LogicalSide::IEnd | LogicalSide::IStart => {
                if aSide == LogicalSide::IEnd {
                    xPos += 1;
                }
                // since bStart, bEnd borders were set, there should already be a cellData
                // entry
                let lastIndex = rgYPos + aLength as i32 - 1;
                for yIndex in rgYPos..=lastIndex {
                    let changed = aChanged && (yIndex == rgYPos);
                    cellData = aCellMap.GetDataAt(yIndex, xPos) as *mut BCCellData;
                    if !cellData.is_null() {
                        unsafe { &mut *cellData }.mData.SetIStartEdge(aOwner, aSize, changed);
                    } else {
                        NS_ASSERTION!(aSide == LogicalSide::IEnd, "program error");
                        let bcData = self.GetIEndMostBorder(yIndex + aCellMapStart as i32);
                        if !bcData.is_null() {
                            unsafe { &mut *bcData }.SetIStartEdge(aOwner, aSize, changed);
                        } else {
                            NS_ERROR!("Cellmap: IStart edge not found");
                        }
                    }
                }
            }
        }
    }

    // store corner info (aOwner, aSubSize, aBevel). For aCorner = eBStartIStart,
    // store the info at (aRowIndex, aColIndex). For eBStartIEnd, store it in the
    // entry to the iEnd-wards where it would be BStartIStart. For eBEndIEnd, store
    // it in the entry to the bEnd-wards. etc.
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::SetBCBorderCorner
    // (the C++ default `aIsBottomRight = false` is written out by the caller)
    pub fn SetBCBorderCorner(&mut self, aCorner: LogicalCorner, aCellMap: &mut nsCellMap, aCellMapStart: u32, aRowIndex: u32, aColIndex: u32, aOwner: LogicalSide, aSubSize: nscoord, aBevel: bool, aIsBEndIEnd: bool) {
        if self.mBCInfo.is_none() {
            // ABORT0();
            NS_ASSERTION!(false, "CellIterator program error");
            return;
        }

        if aIsBEndIEnd {
            self.mBCInfo.as_mut().unwrap().mBEndIEndCorner.SetCorner(aSubSize, aOwner, aBevel);
            return;
        }

        let mut xPos = aColIndex as i32;
        let mut yPos = aRowIndex as i32;
        let mut rgYPos = aRowIndex as i32 - aCellMapStart as i32;

        if LogicalCorner::BStartIEnd == aCorner {
            xPos += 1;
        } else if LogicalCorner::BEndIEnd == aCorner {
            xPos += 1;
            rgYPos += 1;
            yPos += 1;
        } else if LogicalCorner::BEndIStart == aCorner {
            rgYPos += 1;
            yPos += 1;
        }

        let mut cellData: *mut BCCellData = std::ptr::null_mut();
        let mut bcData: *mut BCData = std::ptr::null_mut();
        if self.GetColCount() <= xPos {
            NS_ASSERTION!(xPos == self.GetColCount(), "program error");
            // at the iEnd edge of the table as we checked the corner before
            NS_ASSERTION!(!aIsBEndIEnd, "should be handled before");
            bcData = self.GetIEndMostBorder(yPos);
        } else if rgYPos < aCellMap.GetRowCount(false) {
            // See the comments in SetBCBorderEdge for why we need to check
            // GetRowCount() before null-checking cellData.
            cellData = aCellMap.GetDataAt(rgYPos, xPos) as *mut BCCellData;
            if cellData.is_null() {
                // add a dead cell data
                let mut damageArea = TableArea::default();
                cellData = aCellMap.AppendCell(self, None, rgYPos, false, 0, &mut damageArea, None) as *mut BCCellData;
            }
        } else {
            // try the next non empty row group
            let mut cellMap = aCellMap.GetNextSibling();
            while !cellMap.is_null() && (0 == CellMapRef(cellMap).GetRowCount(false)) {
                cellMap = CellMapRef(cellMap).GetNextSibling();
            }
            if !cellMap.is_null() {
                cellData = CellMapRef(cellMap).GetDataAt(0, xPos) as *mut BCCellData;
                if cellData.is_null() {
                    // add a dead cell
                    let mut damageArea = TableArea::default();
                    cellData = CellMapRef(cellMap).AppendCell(self, None, 0, false, 0, &mut damageArea, None) as *mut BCCellData;
                }
            } else {
                // must be at the bEnd of the table
                bcData = self.GetBEndMostBorder(xPos);
            }
        }
        if bcData.is_null() && !cellData.is_null() {
            bcData = unsafe { &mut (*cellData).mData };
        }
        if !bcData.is_null() {
            unsafe { &mut *bcData }.SetCorner(aSubSize, aOwner, aBevel);
        } else {
            NS_ERROR!("program error: Corner not found");
        }
    }

    /** dump a representation of the cell map to stdout for debugging */
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::Dump
    // NOT PORTED: debug dump (printf only), nsCellMap.cpp lines 537-596 at GECKO_REV.

    // Get the bcData holding the border segments of the iEnd edge of the table
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetIEndMostBorder
    // (a pointer into the array; null when the table is not border-collapse)
    pub fn GetIEndMostBorder(&mut self, aRowIndex: i32) -> *mut BCData {
        let Some(bcInfo) = self.mBCInfo.as_mut() else {
            // ABORT1(nullptr);
            NS_ASSERTION!(false, "CellIterator program error");
            return std::ptr::null_mut();
        };

        let numRows = bcInfo.mIEndBorders.len() as i32;
        if aRowIndex < numRows {
            return &mut bcInfo.mIEndBorders[aRowIndex as usize] as *mut BCData;
        }

        bcInfo.mIEndBorders.resize(aRowIndex as usize + 1, BCData::new());
        &mut bcInfo.mIEndBorders[aRowIndex as usize] as *mut BCData
    }

    // Get the bcData holding the border segments of the bEnd edge of the table
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::GetBEndMostBorder
    pub fn GetBEndMostBorder(&mut self, aColIndex: i32) -> *mut BCData {
        let Some(bcInfo) = self.mBCInfo.as_mut() else {
            // ABORT1(nullptr);
            NS_ASSERTION!(false, "CellIterator program error");
            return std::ptr::null_mut();
        };

        let numCols = bcInfo.mBEndBorders.len() as i32;
        if aColIndex < numCols {
            return &mut bcInfo.mBEndBorders[aColIndex as usize] as *mut BCData;
        }

        bcInfo.mBEndBorders.resize(aColIndex as usize + 1, BCData::new());
        &mut bcInfo.mBEndBorders[aColIndex as usize] as *mut BCData
    }

    /** Insert a row group cellmap after aPrevMap, if aPrefMap is null insert it
     * at the beginning, the ordering of the cellmap corresponds to the ordering
     * of rowgroups once OrderRowGroups has been called
     */
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::InsertGroupCellMap
    // (the overload that links a made cell map: C++ `InsertGroupCellMap(nsCellMap*, nsCellMap&)`)
    pub fn InsertGroupCellMap__nsCellMap_nsCellMap(&mut self, aPrevMap: *mut nsCellMap, aNewMap: *mut nsCellMap) {
        let next: *mut nsCellMap;
        if !aPrevMap.is_null() {
            next = CellMapRef(aPrevMap).GetNextSibling();
            CellMapRef(aPrevMap).SetNextSibling(aNewMap);
        } else {
            next = self.mFirstMap;
            self.mFirstMap = aNewMap;
        }
        CellMapRef(aNewMap).SetNextSibling(next);
    }
}

impl Drop for nsTableCellMap {
    // Gecko: layout/tables/nsCellMap.cpp nsTableCellMap::~nsTableCellMap
    fn drop(&mut self) {
        // NOT PORTED: MOZ_COUNT_DTOR(nsTableCellMap)
        let mut cellMap = self.mFirstMap;
        while !cellMap.is_null() {
            let next = CellMapRef(cellMap).GetNextSibling();
            // delete cellMap;
            drop(unsafe { Box::from_raw(cellMap) });
            cellMap = next;
        }
        self.mFirstMap = std::ptr::null_mut();
    }
}

// Gecko: layout/tables/nsCellMap.cpp FindMapFor
fn FindMapFor(aRowGroup: nsTableRowGroupFrame, aStart: *mut nsCellMap, aEnd: *const nsCellMap) -> *mut nsCellMap {
    let mut map = aStart;
    while map as *const nsCellMap != aEnd {
        if aRowGroup == CellMapRef(map).GetRowGroup() {
            return map;
        }
        map = CellMapRef(map).GetNextSibling();
    }

    std::ptr::null_mut()
}

/**
 * It maintains an Rows x Columns grid onto which the cells of the table are
 * mapped. This makes processing of rowspan and colspan attributes much easier.
 * Each cell is represented by a CellData object.
 *
 * @see CellData
 * @see nsTableFrame::AddCellToMap
 * @see nsTableFrame::GrowCellMap
 * @see nsTableFrame::BuildCellIntoMap
 *
 * mRows is an array of rows.  Each row is an array of cells.  a cell
 * can be null.
 */
// Gecko: layout/tables/nsCellMap.h nsCellMap
pub struct nsCellMap {
    // protected:
    /** an array containing, for each row, the CellDatas for the cells
     * in that row.  It can be larger than mContentRowCount due to row spans
     * extending beyond the table */
    // XXXbz once we have auto TArrays, we should probably use them here.
    pub mRows: Vec<nsCellMap_CellDataArray>,

    /** the number of rows in the table (content) which is not indentical to the
     * number of rows in the cell map due to row spans extending beyond the end
     * of thetable (dead rows) or empty tr tags
     */
    pub mContentRowCount: i32,

    // the row group that corresponds to this map
    pub mRowGroupFrame: nsTableRowGroupFrame,

    // the next row group cell map
    pub mNextSibling: *mut nsCellMap,

    // Whether this is a BC cellmap or not
    pub mIsBC: bool,

    // Prescontext to deallocate and allocate celldata
    // (C++ `RefPtr<nsPresContext>`; only the arena allocation used it, see NOT PORTED at the top)
    pub mPresContext: nsPresContext,
}

impl nsCellMap {
    /** constructor
     * @param aRowGroupFrame the row group frame this is a cellmap for
     * @param aIsBC whether the table is doing border-collapse
     */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::nsCellMap
    pub fn new(aRowGroup: nsTableRowGroupFrame, aIsBC: bool) -> nsCellMap {
        // NOT PORTED: MOZ_COUNT_CTOR(nsCellMap)
        nsCellMap { mRows: Vec::with_capacity(8), mContentRowCount: 0, mRowGroupFrame: aRowGroup, mNextSibling: std::ptr::null_mut(), mIsBC: aIsBC, mPresContext: aRowGroup.PresContext() }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::Init
    // NOT PORTED: creates the static empty row `sEmptyRow` (see the top of the file).

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::Shutdown
    // NOT PORTED: frees the static empty row `sEmptyRow` (see the top of the file).

    // Gecko: layout/tables/nsCellMap.h nsCellMap::GetNextSibling
    pub fn GetNextSibling(&self) -> *mut nsCellMap {
        self.mNextSibling
    }

    // Gecko: layout/tables/nsCellMap.h nsCellMap::SetNextSibling
    pub fn SetNextSibling(&mut self, aSibling: *mut nsCellMap) {
        self.mNextSibling = aSibling;
    }

    // Gecko: layout/tables/nsCellMap.h nsCellMap::GetRowGroup
    pub fn GetRowGroup(&self) -> nsTableRowGroupFrame {
        self.mRowGroupFrame
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::GetCellFrame
    pub fn GetCellFrame(&self, aRowIndexIn: i32, aColIndexIn: i32, aData: &CellData, aUseRowIfOverlap: bool) -> Option<nsTableCellFrame> {
        let mut rowIndex = aRowIndexIn - aData.GetRowSpanOffset() as i32;
        let mut colIndex = aColIndexIn - aData.GetColSpanOffset() as i32;
        if aData.IsOverlap() {
            if aUseRowIfOverlap {
                colIndex = aColIndexIn;
            } else {
                rowIndex = aRowIndexIn;
            }
        }

        let data = self.SafeRowAt(rowIndex).map_or(std::ptr::null_mut(), |row| SafeElementAt(row, colIndex));
        if !data.is_null() {
            return CellDataRef(data).GetCellFrame();
        }
        None
    }

    // ADAPTER: `mRows.SafeElementAt(aRowIndex, *sEmptyRow)`: None where the C++ gets the empty row.
    fn SafeRowAt(&self, aRowIndex: i32) -> Option<&nsCellMap_CellDataArray> {
        if aRowIndex < 0 {
            return None;
        }
        self.mRows.get(aRowIndex as usize)
    }

    /**
     * Returns highest cell index within the cell map.
     *
     * @param  aColCount  [in] the number of columns in the table
     */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::GetHighestIndex
    pub fn GetHighestIndex(&mut self, aColCount: i32) -> i32 {
        let mut index = -1;
        let rowCount = self.mRows.len() as i32;
        for rowIdx in 0..rowCount {
            let row = &self.mRows[rowIdx as usize];

            for colIdx in 0..aColCount {
                let data = SafeElementAt(row, colIdx);
                // No data means row doesn't have more cells.
                if data.is_null() {
                    break;
                }

                if CellDataRef(data).IsOrig() {
                    index += 1;
                }
            }
        }

        index
    }

    /**
     * Returns the index of the given row and column coordinates.
     *
     * @see  nsITableLayout::GetIndexByRowAndColumn()
     *
     * @param aColCount    [in] the number of columns in the table
     * @param aRow         [in] the row coordinate
     * @param aColumn      [in] the column coordinate
     */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::GetIndexByRowAndColumn
    pub fn GetIndexByRowAndColumn(&self, aColCount: i32, aRow: i32, aColumn: i32) -> i32 {
        if aRow as u32 as usize >= self.mRows.len() {
            return -1;
        }

        let mut index = -1;
        let lastColsIdx = aColCount - 1;

        // Find row index of the cell where row span is started.
        let row = &self.mRows[aRow as usize];
        let mut data = SafeElementAt(row, aColumn);
        let origRow = if !data.is_null() { aRow - CellDataRef(data).GetRowSpanOffset() as i32 } else { aRow };

        // Calculate cell index.
        for rowIdx in 0..=origRow {
            let row = &self.mRows[rowIdx as usize];
            let colCount = if rowIdx == origRow { aColumn } else { lastColsIdx };

            for colIdx in 0..=colCount {
                data = SafeElementAt(row, colIdx);
                // No data means row doesn't have more cells.
                if data.is_null() {
                    break;
                }

                if CellDataRef(data).IsOrig() {
                    index += 1;
                }
            }
        }

        // Given row and column don't point to the cell.
        if data.is_null() {
            return -1;
        }

        index
    }

    /**
     * Get the row and column coordinates at the given index.
     *
     * @see  nsITableLayout::GetRowAndColumnByIndex()
     *
     * @param aColCount  [in] the number of columns in the table
     * @param aIndex     [in] the index for which coordinates are to be retrieved
     * @param aRow       [out] the row coordinate to be returned
     * @param aColumn    [out] the column coordinate to be returned
     */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::GetRowAndColumnByIndex
    pub fn GetRowAndColumnByIndex(&self, aColCount: i32, aIndex: i32, aRow: &mut i32, aColumn: &mut i32) {
        *aRow = -1;
        *aColumn = -1;

        let mut index = aIndex;
        let rowCount = self.mRows.len() as i32;

        for rowIdx in 0..rowCount {
            let row = &self.mRows[rowIdx as usize];

            for colIdx in 0..aColCount {
                let data = SafeElementAt(row, colIdx);

                // The row doesn't have more cells.
                if data.is_null() {
                    break;
                }

                if CellDataRef(data).IsOrig() {
                    index -= 1;
                }

                if index < 0 {
                    *aRow = rowIdx;
                    *aColumn = colIdx;
                    return;
                }
            }
        }
    }

    /** append the cellFrame at an empty or dead cell or finally at the end of
     * the row at aRowIndex and return a pointer to the celldata entry in the
     * cellmap
     *
     * @param aMap               - reference to the table cell map
     * @param aCellFrame         - a pointer to the cellframe which will be
     *                             appended to the row
     * @param aRowIndex          - to this row the celldata entry will be added
     * @param aRebuildIfNecessay - if a cell spans into a row below it might be
     *                             necesserary to rebuild the cellmap as this
     *                             rowspan might overlap another cell.
     * @param aDamageArea        - area in cellmap coordinates which have been
     *                             updated.
     * @param aColToBeginSearch  - if not null contains the column number where
     *                             the search for a empty or dead cell in the
     *                             row should start
     * @return                   - a pointer to the celldata entry inserted into
     *                             the cellmap
     */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::AppendCell
    // (C++ default `aBeginSearchAtCol = nullptr` is `None`)
    pub fn AppendCell(&mut self, aMap: &mut nsTableCellMap, aCellFrame: Option<nsTableCellFrame>, aRowIndex: i32, aRebuildIfNecessary: bool, aRgFirstRowIndex: i32, aDamageArea: &mut TableArea, aColToBeginSearch: Option<&mut i32>) -> *mut CellData {
        NS_ASSERTION!(aMap.mBCInfo.is_some() == self.mIsBC, "BC state mismatch");
        let origNumMapRows = self.mRows.len() as i32;
        let origNumCols = aMap.GetColCount();
        let mut zeroRowSpan = false;
        let rowSpan = match aCellFrame {
            Some(cellFrame) => self.GetRowSpanForNewCell(cellFrame, aRowIndex, &mut zeroRowSpan),
            None => 1,
        };
        // add new rows if necessary
        let endRowIndex = aRowIndex + rowSpan - 1;
        if endRowIndex >= origNumMapRows {
            // XXXbz handle allocation failures?
            self.Grow(aMap, 1 + endRowIndex - origNumMapRows, -1);
        }

        // get the first null or dead CellData in the desired row. It will equal
        // origNumCols if there are none
        let mut origData: *mut CellData = std::ptr::null_mut();
        let mut startColIndex = 0;
        if let Some(col) = aColToBeginSearch.as_deref() {
            startColIndex = *col;
        }
        while startColIndex < origNumCols {
            let data = self.GetDataAt(aRowIndex, startColIndex);
            if data.is_null() {
                break;
            }
            // The border collapse code relies on having multiple dead cell data entries
            // in a row.
            if CellDataRef(data).IsDead() && aCellFrame.is_some() {
                origData = data;
                break;
            }
            startColIndex += 1;
        }
        // We found the place to append the cell, when the next cell is appended
        // the next search does not need to duplicate the search but can start
        // just at the next cell.
        if let Some(col) = aColToBeginSearch {
            *col = startColIndex + 1;
        }

        let colSpan = match aCellFrame {
            Some(cellFrame) => cellFrame.GetColSpan(),
            None => 1,
        };

        // if the new cell could potentially span into other rows and collide with
        // originating cells there, we will play it safe and just rebuild the map
        if aRebuildIfNecessary && (aRowIndex < self.mContentRowCount - 1) && (rowSpan > 1) {
            let mut newCellArray: Vec<nsTableCellFrame> = Vec::with_capacity(1);
            // (the C++ appends aCellFrame, which is not null here: rowSpan > 1 needs a cell)
            newCellArray.push(aCellFrame.unwrap());
            aMap.RebuildConsideringCells(self as *mut nsCellMap, Some(&newCellArray), aRowIndex, startColIndex, true, aDamageArea);
            return origData;
        }
        self.mContentRowCount = self.mContentRowCount.max(aRowIndex + 1);

        // add new cols to the table map if necessary
        let endColIndex = startColIndex + colSpan - 1;
        if endColIndex >= origNumCols {
            NS_ASSERTION!(aCellFrame.is_some(), "dead cells should not require new columns");
            aMap.AddColsAtEnd((1 + endColIndex - origNumCols) as u32);
        }

        // Setup CellData for this cell
        if !origData.is_null() {
            NS_ASSERTION!(CellDataRef(origData).IsDead(), "replacing a non dead cell is a memory leak");
            if let Some(cellFrame) = aCellFrame {
                // do nothing to replace a dead cell with a dead cell
                CellDataRef(origData).Init(Some(cellFrame));
                // we are replacing a dead cell, increase the number of cells
                // originating at this column
                let colInfo = aMap.GetColInfoAt(startColIndex);
                // NS_ASSERTION(colInfo, "access to a non existing column"); (never null here)
                // if (colInfo) {
                colInfo.mNumCellsOrig += 1;
                // }
            }
        } else {
            origData = self.AllocCellData(aCellFrame);
            if origData.is_null() {
                // ABORT1(origData);
                NS_ASSERTION!(false, "CellIterator program error");
                return origData;
            }
            self.SetDataAt(aMap, origData, aRowIndex, startColIndex);
        }

        if aRebuildIfNecessary {
            // the caller depends on the damageArea
            // The special case for zeroRowSpan is to adjust for the '2' in
            // GetRowSpanForNewCell.
            let height = (if zeroRowSpan { rowSpan - 1 } else { rowSpan }).min(self.GetRowCount(false) - aRowIndex);
            SetDamageArea(startColIndex, aRgFirstRowIndex + aRowIndex, 1 + endColIndex - startColIndex, height, aDamageArea);
        }

        let Some(cellFrame) = aCellFrame else {
            return origData;
        };

        // initialize the cell frame
        cellFrame.SetColIndex(startColIndex);

        // Create CellData objects for the rows that this cell spans. Set
        // their mOrigCell to nullptr and their mSpanData to point to data.
        for rowX in aRowIndex..=endRowIndex {
            // The row at rowX will need to have at least endColIndex columns
            // mRows[rowX].SetCapacity(endColIndex);
            let have = self.mRows[rowX as usize].len();
            self.mRows[rowX as usize].reserve((endColIndex as usize).saturating_sub(have));
            for colX in startColIndex..=endColIndex {
                if (rowX != aRowIndex) || (colX != startColIndex) {
                    // skip orig cell data done above
                    let mut cellData = self.GetDataAt(rowX, colX);
                    if !cellData.is_null() {
                        if CellDataRef(cellData).IsOrig() {
                            NS_ERROR!("cannot overlap originating cell");
                            continue;
                        }
                        if rowX > aRowIndex {
                            // row spanning into cell
                            if CellDataRef(cellData).IsRowSpan() {
                                // do nothing, this can be caused by rowspan which is overlapped
                                // by a another cell with a rowspan and a colspan
                            } else {
                                CellDataRef(cellData).SetRowSpanOffset((rowX - aRowIndex) as u32);
                                if zeroRowSpan {
                                    CellDataRef(cellData).SetZeroRowSpan(true);
                                }
                            }
                        }
                        if colX > startColIndex {
                            // col spanning into cell
                            if !CellDataRef(cellData).IsColSpan() {
                                if CellDataRef(cellData).IsRowSpan() {
                                    CellDataRef(cellData).SetOverlap(true);
                                }
                                CellDataRef(cellData).SetColSpanOffset((colX - startColIndex) as u32);
                                let colInfo = aMap.GetColInfoAt(colX);
                                colInfo.mNumCellsSpan += 1;
                            }
                        }
                    } else {
                        cellData = self.AllocCellData(None);
                        if cellData.is_null() {
                            return origData;
                        }
                        if rowX > aRowIndex {
                            CellDataRef(cellData).SetRowSpanOffset((rowX - aRowIndex) as u32);
                            if zeroRowSpan {
                                CellDataRef(cellData).SetZeroRowSpan(true);
                            }
                        }
                        if colX > startColIndex {
                            CellDataRef(cellData).SetColSpanOffset((colX - startColIndex) as u32);
                        }
                        self.SetDataAt(aMap, cellData, rowX, colX);
                    }
                }
            }
        }
        // NOT PORTED: #ifdef DEBUG_TABLE_CELLMAP printf + aMap.Dump()
        origData
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::InsertCells
    pub fn InsertCells(&mut self, aMap: &mut nsTableCellMap, aCellFrames: &mut Vec<nsTableCellFrame>, aRowIndex: i32, mut aColIndexBefore: i32, aRgFirstRowIndex: i32, aDamageArea: &mut TableArea) {
        if aCellFrames.is_empty() {
            return;
        }
        NS_ASSERTION!(aColIndexBefore >= -1, "index out of range");
        let numCols = aMap.GetColCount();
        if aColIndexBefore >= numCols {
            NS_ERROR!("Inserting instead of appending cells indicates a serious cellmap error");
            aColIndexBefore = numCols - 1;
        }

        // get the starting col index of the 1st new cells
        let mut startColIndex = aColIndexBefore + 1;
        while startColIndex < numCols {
            let data = self.GetDataAt(aRowIndex, startColIndex);
            if data.is_null() || CellDataRef(data).IsOrig() || CellDataRef(data).IsDead() {
                // // Not a span.  Stop.
                break;
            }
            startColIndex += 1;
        }

        let mut spansCauseRebuild = false;

        // check that all cells have the same row span
        let numNewCells = aCellFrames.len() as i32;
        let mut zeroRowSpan = false;
        let mut rowSpan = 0;
        for cellX in 0..numNewCells {
            let cell = aCellFrames[cellX as usize];
            let rowSpan2 = self.GetRowSpanForNewCell(cell, aRowIndex, &mut zeroRowSpan);
            if rowSpan == 0 {
                rowSpan = rowSpan2;
            } else if rowSpan != rowSpan2 {
                spansCauseRebuild = true;
                break;
            }
        }

        // check if the new cells will cause the table to add more rows
        if !spansCauseRebuild && (self.mRows.len() as u32) < (aRowIndex + rowSpan) as u32 {
            spansCauseRebuild = true;
        }

        if !spansCauseRebuild {
            spansCauseRebuild = self.CellsSpanInOrOut(aRowIndex, aRowIndex + rowSpan - 1, startColIndex, numCols - 1);
        }
        if spansCauseRebuild {
            aMap.RebuildConsideringCells(self as *mut nsCellMap, Some(&*aCellFrames), aRowIndex, startColIndex, true, aDamageArea);
        } else {
            self.ExpandWithCells(aMap, aCellFrames, aRowIndex, startColIndex, rowSpan, zeroRowSpan, aRgFirstRowIndex, aDamageArea);
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::RemoveCell
    pub fn RemoveCell(&mut self, aMap: &mut nsTableCellMap, aCellFrame: nsTableCellFrame, aRowIndex: i32, aRgFirstRowIndex: i32, aDamageArea: &mut TableArea) {
        let numRows = self.mRows.len() as u32;
        if aRowIndex as u32 >= numRows {
            NS_ERROR!("bad arg in nsCellMap::RemoveCell");
            return;
        }
        let numCols = aMap.GetColCount();

        // Now aRowIndex is guaranteed OK.

        // get the starting col index of the cell to remove
        let mut startColIndex = 0;
        while startColIndex < numCols {
            let data = SafeElementAt(&self.mRows[aRowIndex as usize], startColIndex);
            if !data.is_null() && CellDataRef(data).IsOrig() && Some(aCellFrame) == CellDataRef(data).GetCellFrame() {
                break; // we found the col index
            }
            startColIndex += 1;
        }

        let rowSpan = self.GetRowSpan(aRowIndex, startColIndex, false);
        // record whether removing the cells is going to cause complications due
        // to existing row spans, col spans or table sizing.
        let mut spansCauseRebuild = self.CellsSpanInOrOut(aRowIndex, aRowIndex + rowSpan - 1, startColIndex, numCols - 1);
        // XXX if the cell has a col span to the end of the map, and the end has no
        // originating cells, we need to assume that this the only such cell, and
        // rebuild so that there are no extraneous cols at the end. The same is true
        // for removing rows.
        if !spansCauseRebuild && (aCellFrame.GetRowSpan() == 0 || aCellFrame.GetColSpan() == 0) {
            spansCauseRebuild = true;
        }

        if spansCauseRebuild {
            aMap.RebuildConsideringCells(self as *mut nsCellMap, None, aRowIndex, startColIndex, false, aDamageArea);
        } else {
            self.ShrinkWithoutCell(aMap, aCellFrame, aRowIndex, startColIndex, aRgFirstRowIndex, aDamageArea);
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::InsertRows
    pub fn InsertRows(&mut self, aMap: &mut nsTableCellMap, aRows: &mut Vec<nsTableRowFrame>, aFirstRowIndex: i32, aConsiderSpans: bool, aRgFirstRowIndex: i32, aDamageArea: &mut TableArea) {
        let numCols = aMap.GetColCount();
        NS_ASSERTION!(aFirstRowIndex >= 0, "nsCellMap::InsertRows called with negative rowIndex");
        if aFirstRowIndex as u32 as usize > self.mRows.len() {
            // create (aFirstRowIndex - mRows.Length()) empty rows up to aFirstRowIndex
            let numEmptyRows = aFirstRowIndex - self.mRows.len() as i32;
            if !self.Grow(aMap, numEmptyRows, -1) {
                return;
            }
        }

        if !aConsiderSpans {
            // update mContentRowCount, since non-empty rows will be added
            self.mContentRowCount = aFirstRowIndex.max(self.mContentRowCount);
            self.ExpandWithRows(aMap, aRows, aFirstRowIndex, aRgFirstRowIndex, aDamageArea);
            return;
        }

        // if any cells span into or out of the row being inserted, then rebuild
        let mut spansCauseRebuild = self.CellsSpanInOrOut(aFirstRowIndex, aFirstRowIndex, 0, numCols - 1);

        // update mContentRowCount, since non-empty rows will be added
        self.mContentRowCount = aFirstRowIndex.max(self.mContentRowCount);

        // if any of the new cells span out of the new rows being added, then rebuild
        // XXX it would be better to only rebuild the portion of the map that follows
        // the new rows
        if !spansCauseRebuild && (aFirstRowIndex as u32 as usize) < self.mRows.len() {
            spansCauseRebuild = self.CellsSpanOut(aRows);
        }
        if spansCauseRebuild {
            aMap.RebuildConsideringRows(self as *mut nsCellMap, aFirstRowIndex, Some(&*aRows), 0, aDamageArea);
        } else {
            self.ExpandWithRows(aMap, aRows, aFirstRowIndex, aRgFirstRowIndex, aDamageArea);
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::RemoveRows
    pub fn RemoveRows(&mut self, aMap: &mut nsTableCellMap, aFirstRowIndex: i32, aNumRowsToRemove: i32, aConsiderSpans: bool, aRgFirstRowIndex: i32, aDamageArea: &mut TableArea) {
        let numRows = self.mRows.len() as i32;
        let numCols = aMap.GetColCount();

        if aFirstRowIndex >= numRows {
            // reduce the content based row count based on the function arguments
            // as they are known to be real rows even if the cell map did not create
            // rows for them before.
            self.mContentRowCount -= aNumRowsToRemove;
            return;
        }
        if !aConsiderSpans {
            self.ShrinkWithoutRows(aMap, aFirstRowIndex, aNumRowsToRemove, aRgFirstRowIndex, aDamageArea);
            return;
        }
        let mut endRowIndex = aFirstRowIndex + aNumRowsToRemove - 1;
        if endRowIndex >= numRows {
            NS_ERROR!("nsCellMap::RemoveRows tried to remove too many rows");
            endRowIndex = numRows - 1;
        }
        let spansCauseRebuild = self.CellsSpanInOrOut(aFirstRowIndex, endRowIndex, 0, numCols - 1);
        if spansCauseRebuild {
            aMap.RebuildConsideringRows(self as *mut nsCellMap, aFirstRowIndex, None, aNumRowsToRemove, aDamageArea);
        } else {
            self.ShrinkWithoutRows(aMap, aFirstRowIndex, aNumRowsToRemove, aRgFirstRowIndex, aDamageArea);
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::GetNumCellsOriginatingInRow
    pub fn GetNumCellsOriginatingInRow(&self, aRowIndex: i32) -> i32 {
        let Some(row) = self.SafeRowAt(aRowIndex) else {
            return 0;
        };
        let mut count = 0;
        let maxColIndex = row.len();
        for colIndex in 0..maxColIndex {
            let cellData = row[colIndex];
            if !cellData.is_null() && CellDataRef(cellData).IsOrig() {
                count += 1;
            }
        }
        count
    }

    // nsCellMap::GetNumCellsOriginatingInCol (nsCellMap.h)
    // NOT PORTED: declared in nsCellMap.h but never defined in nsCellMap.cpp (no caller; the count lives in
    // nsTableCellMap::mCols), so there is no body to translate.

    /** return the number of rows in the table represented by this CellMap */
    // Gecko: layout/tables/nsCellMap.h nsCellMap::GetRowCount
    // (C++ default argument `aConsiderDeadRowSpanRows = false`: the caller writes `false`)
    pub fn GetRowCount(&self, aConsiderDeadRowSpanRows: bool) -> i32 {
        if aConsiderDeadRowSpanRows { self.mRows.len() as i32 } else { self.mContentRowCount }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::GetCellInfoAt
    // (C++ default arguments `aOriginates = nullptr, aColSpan = nullptr` are `None`)
    pub fn GetCellInfoAt(&self, aMap: &nsTableCellMap, aRowX: i32, aColX: i32, aOriginates: Option<&mut bool>, aColSpan: Option<&mut i32>) -> Option<nsTableCellFrame> {
        let mut aOriginates = aOriginates;
        if let Some(originates) = aOriginates.as_mut() {
            **originates = false;
        }
        let data = self.GetDataAt(aRowX, aColX);
        let mut cellFrame: Option<nsTableCellFrame> = None;
        if !data.is_null() {
            if CellDataRef(data).IsOrig() {
                cellFrame = CellDataRef(data).GetCellFrame();
                if let Some(originates) = aOriginates.as_mut() {
                    **originates = true;
                }
            } else {
                cellFrame = self.GetCellFrame(aRowX, aColX, CellDataRef(data), true);
            }
            if let (Some(cellFrame), Some(colSpan)) = (cellFrame, aColSpan) {
                let initialColIndex = cellFrame.ColIndex();
                *colSpan = self.GetEffectiveColSpan(aMap, aRowX, initialColIndex as i32);
            }
        }
        cellFrame
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::RowIsSpannedInto
    pub fn RowIsSpannedInto(&self, aRowIndex: i32, aNumEffCols: i32) -> bool {
        if (0 > aRowIndex) || (aRowIndex >= self.mContentRowCount) {
            return false;
        }
        for colIndex in 0..aNumEffCols {
            let cd = self.GetDataAt(aRowIndex, colIndex);
            if !cd.is_null() {
                // there's really a cell at (aRowIndex, colIndex)
                if CellDataRef(cd).IsSpan() {
                    // the cell at (aRowIndex, colIndex) is the result of
                    // a span
                    if CellDataRef(cd).IsRowSpan() && self.GetCellFrame(aRowIndex, colIndex, CellDataRef(cd), true).is_some() {
                        // XXX why the last check
                        return true;
                    }
                }
            }
        }
        false
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::RowHasSpanningCells
    pub fn RowHasSpanningCells(&self, aRowIndex: i32, aNumEffCols: i32) -> bool {
        if (0 > aRowIndex) || (aRowIndex >= self.mContentRowCount) {
            return false;
        }
        if aRowIndex != self.mContentRowCount - 1 {
            // aRowIndex is not the last row, so we check the next row after aRowIndex
            // for spanners
            for colIndex in 0..aNumEffCols {
                let cd = self.GetDataAt(aRowIndex, colIndex);
                if !cd.is_null() && CellDataRef(cd).IsOrig() {
                    // cell originates
                    let cd2 = self.GetDataAt(aRowIndex + 1, colIndex);
                    if !cd2.is_null() && CellDataRef(cd2).IsRowSpan() {
                        // cd2 is spanned by a row
                        if CellDataRef(cd).GetCellFrame() == self.GetCellFrame(aRowIndex + 1, colIndex, CellDataRef(cd2), true) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /** indicate whether the row has more than one cell that either originates
     * or is spanned from the rows above
     */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::HasMoreThanOneCell
    pub fn HasMoreThanOneCell(&self, aRowIndex: i32) -> bool {
        let Some(row) = self.SafeRowAt(aRowIndex) else {
            return false;
        };
        let maxColIndex = row.len();
        let mut foundOne = false;
        for colIndex in 0..maxColIndex {
            let cellData = row[colIndex];
            if !cellData.is_null() && (CellDataRef(cellData).GetCellFrame().is_some() || CellDataRef(cellData).IsRowSpan()) {
                if foundOne {
                    return true;
                }
                foundOne = true;
            }
        }
        false
    }

    /* Get the rowspan for a cell starting at aRowIndex and aColIndex.
     * If aGetEffective is true the size will not exceed the last content based
     * row. Cells can have a specified rowspan that extends below the last
     * content based row. This is legitimate considering incr. reflow where the
     * content rows will arive later.
     */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::GetRowSpan
    pub fn GetRowSpan(&self, aRowIndex: i32, aColIndex: i32, aGetEffective: bool) -> i32 {
        let mut rowSpan = 1;
        let rowCount = if aGetEffective { self.mContentRowCount } else { self.mRows.len() as i32 };
        let mut rowX = aRowIndex + 1;
        while rowX < rowCount {
            let data = self.GetDataAt(rowX, aColIndex);
            if !data.is_null() {
                if CellDataRef(data).IsRowSpan() {
                    rowSpan += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
            rowX += 1;
        }

        // Enforce that the effective rowSpan is between 1 and MAX_ROWSPAN:
        rowSpan.clamp(1, MAX_ROWSPAN)
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::GetEffectiveColSpan
    pub fn GetEffectiveColSpan(&self, aMap: &nsTableCellMap, aRowIndex: i32, aColIndex: i32) -> i32 {
        let numColsInTable = aMap.GetColCount();
        let mut colSpan = 1;
        if aRowIndex as u32 as usize >= self.mRows.len() {
            return colSpan;
        }

        let row = &self.mRows[aRowIndex as usize];
        let mut maxCols = numColsInTable;
        let hitOverlap = false; // XXX this is not ever being set to true
        let mut colX = aColIndex + 1;
        while colX < maxCols {
            let data = SafeElementAt(row, colX);
            if !data.is_null() {
                // for an overlapping situation get the colspan from the originating cell
                // and use that as the max number of cols to iterate. Since this is rare,
                // only pay the price of looking up the cell's colspan here.
                if !hitOverlap && CellDataRef(data).IsOverlap() {
                    let origData = SafeElementAt(row, aColIndex);
                    if !origData.is_null() && CellDataRef(origData).IsOrig() {
                        let cellFrame = CellDataRef(origData).GetCellFrame();
                        if let Some(cellFrame) = cellFrame {
                            // possible change the number of colums to iterate
                            maxCols = (aColIndex + cellFrame.GetColSpan()).min(maxCols);
                            if colX >= maxCols {
                                break;
                            }
                        }
                    }
                }
                if CellDataRef(data).IsColSpan() {
                    colSpan += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
            colX += 1;
        }

        // Enforce that the effective colSpan is between 1 and MAX_COLSPAN:
        colSpan.clamp(1, MAX_COLSPAN)
    }

    /** dump a representation of the cell map to stdout for debugging */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::Dump
    // NOT PORTED: debug dump (printf only), nsCellMap.cpp lines 2205-2302 at GECKO_REV.

    // (nsCellMap::CellDataArray is `nsCellMap_CellDataArray`, above the class.)

    /**
     * Increase the number of rows in this cellmap by aNumRows.  Put the
     * new rows at aRowIndex.  If aRowIndex is -1, put them at the end.
     */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::Grow
    // (C++ default argument `aRowIndex = -1`: the caller writes `-1`)
    pub fn Grow(&mut self, aMap: &mut nsTableCellMap, aNumRows: i32, aRowIndex: i32) -> bool {
        NS_ASSERTION!(aNumRows >= 1, "Why are we calling this?");

        // Get the number of cols we want to use for preallocating the row arrays.
        let mut numCols = aMap.GetColCount();
        if numCols == 0 {
            numCols = 4;
        }
        let startRowIndex = if aRowIndex >= 0 { aRowIndex as u32 } else { self.mRows.len() as u32 };
        NS_ASSERTION!(startRowIndex as usize <= self.mRows.len(), "Missing grow call inbetween");

        // XXX Change the return type of this function to void, or use a fallible
        // operation.
        // mRows.InsertElementsAt(startRowIndex, aNumRows, numCols);
        let at = startRowIndex as usize;
        self.mRows.splice(at..at, (0..aNumRows).map(|_| Vec::with_capacity(numCols as usize)));
        true
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::GrowRow
    pub fn GrowRow(&mut self, aRow: usize, aNumCols: i32) {
        // Have to have the cast to get the template to do the right thing.
        // (`aRow` is the index of the row in mRows: a Rust reference to the row cannot be passed
        // together with &mut self.)
        let row = &mut self.mRows[aRow];
        let at = row.len();
        row.splice(at..at, (0..aNumCols).map(|_| std::ptr::null_mut::<CellData>()));
    }

    /** assign aCellData to the cell at (aRow,aColumn) */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::SetDataAt
    // (`aNewCell` is the pointer the map takes ownership of)
    pub fn SetDataAt(&mut self, aMap: &mut nsTableCellMap, aNewCell: *mut CellData, aMapRowIndex: i32, aColIndex: i32) {
        NS_ASSERTION!(aMap.mBCInfo.is_some() == self.mIsBC, "BC state mismatch");
        if aMapRowIndex as u32 as usize >= self.mRows.len() {
            NS_ERROR!("SetDataAt called with row index > num rows");
            return;
        }

        // the table map may need cols added
        let mut numColsToAdd = aColIndex + 1 - aMap.GetColCount();
        if numColsToAdd > 0 {
            aMap.AddColsAtEnd(numColsToAdd as u32);
        }
        // the row may need cols added
        numColsToAdd = aColIndex + 1 - self.mRows[aMapRowIndex as usize].len() as i32;
        if numColsToAdd > 0 {
            // XXXbz need to handle allocation failures.
            self.GrowRow(aMapRowIndex as usize, numColsToAdd);
        }

        let old = self.mRows[aMapRowIndex as usize][aColIndex as usize];
        self.DestroyCellData(old);

        self.mRows[aMapRowIndex as usize][aColIndex as usize] = aNewCell;
        // update the originating cell counts if cell originates in this row, col
        let colInfo = aMap.GetColInfoAt(aColIndex);
        // (never null here: GetColInfoAt adds the column)
        if CellDataRef(aNewCell).IsOrig() {
            colInfo.mNumCellsOrig += 1;
        } else if CellDataRef(aNewCell).IsColSpan() {
            colInfo.mNumCellsSpan += 1;
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::GetDataAt
    pub fn GetDataAt(&self, aMapRowIndex: i32, aColIndex: i32) -> *mut CellData {
        match self.SafeRowAt(aMapRowIndex) {
            Some(row) => SafeElementAt(row, aColIndex),
            None => std::ptr::null_mut(),
        }
    }

    // Gecko: layout/tables/nsCellMap.h nsCellMap::GetNumCellsIn
    // NOT PORTED: declared in nsCellMap.h, never defined in nsCellMap.cpp.

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::ExpandWithRows
    pub fn ExpandWithRows(&mut self, aMap: &mut nsTableCellMap, aRowFrames: &mut Vec<nsTableRowFrame>, aStartRowIndexIn: i32, aRgFirstRowIndex: i32, aDamageArea: &mut TableArea) {
        let startRowIndex = if aStartRowIndexIn >= 0 { aStartRowIndexIn } else { 0 };
        NS_ASSERTION!(startRowIndex as u32 as usize <= self.mRows.len(), "caller should have grown cellmap before");

        let numNewRows = aRowFrames.len() as i32;
        self.mContentRowCount += numNewRows;

        let endRowIndex = startRowIndex + numNewRows - 1;

        // shift the rows after startRowIndex down and insert empty rows that will
        // be filled via the AppendCell call below
        if !self.Grow(aMap, numNewRows, startRowIndex) {
            return;
        }

        let mut newRowIndex = 0;
        for rowX in startRowIndex..=endRowIndex {
            let rFrame = aRowFrames[newRowIndex as usize];
            // append cells
            let mut colIndex = 0;
            let mut cellFrame = rFrame.GetFirstCell();
            while let Some(cell) = cellFrame {
                self.AppendCell(aMap, Some(cell), rowX, false, aRgFirstRowIndex, aDamageArea, Some(&mut colIndex));
                cellFrame = cell.GetNextCell();
            }
            newRowIndex += 1;
        }
        // mark all following rows damaged, they might contain a previously set
        // damage area which we can not shift.
        let firstDamagedRow = aRgFirstRowIndex + startRowIndex;
        SetDamageArea(0, firstDamagedRow, aMap.GetColCount(), aMap.GetRowCount() - firstDamagedRow, aDamageArea);
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::ExpandWithCells
    pub fn ExpandWithCells(
        &mut self,
        aMap: &mut nsTableCellMap,
        aCellFrames: &mut Vec<nsTableCellFrame>,
        aRowIndex: i32,
        aColIndex: i32,
        aRowSpan: i32, // same for all cells
        aRowSpanIsZero: bool,
        aRgFirstRowIndex: i32,
        aDamageArea: &mut TableArea,
    ) {
        NS_ASSERTION!(aMap.mBCInfo.is_some() == self.mIsBC, "BC state mismatch");
        let endRowIndex = aRowIndex + aRowSpan - 1;
        let mut startColIndex = aColIndex;
        let mut endColIndex = aColIndex;
        let numCells = aCellFrames.len() as i32;
        let mut totalColSpan = 0;

        // add cellData entries for the space taken up by the new cells
        for cellX in 0..numCells {
            let cellFrame = aCellFrames[cellX as usize];
            let origData = self.AllocCellData(Some(cellFrame)); // the originating cell
            if origData.is_null() {
                return;
            }

            // set the starting and ending col index for the new cell
            let colSpan = cellFrame.GetColSpan();
            totalColSpan += colSpan;
            if cellX == 0 {
                endColIndex = aColIndex + colSpan - 1;
            } else {
                startColIndex = endColIndex + 1;
                endColIndex = startColIndex + colSpan - 1;
            }

            // add the originating cell data and any cell data corresponding to row/col
            // spans
            for rowX in aRowIndex..=endRowIndex {
                // Pre-allocate all the cells we'll need in this array, setting
                // them to null.
                // Have to have the cast to get the template to do the right thing.
                let row = &mut self.mRows[rowX as usize];
                let mut insertionIndex = row.len() as i32;
                if insertionIndex > startColIndex {
                    insertionIndex = startColIndex;
                }
                let at = insertionIndex as usize;
                row.splice(at..at, (0..endColIndex - insertionIndex + 1).map(|_| std::ptr::null_mut::<CellData>()));

                for colX in startColIndex..=endColIndex {
                    let mut data = origData;
                    if (rowX != aRowIndex) || (colX != startColIndex) {
                        data = self.AllocCellData(None);
                        if data.is_null() {
                            return;
                        }
                        if rowX > aRowIndex {
                            CellDataRef(data).SetRowSpanOffset((rowX - aRowIndex) as u32);
                            if aRowSpanIsZero {
                                CellDataRef(data).SetZeroRowSpan(true);
                            }
                        }
                        if colX > startColIndex {
                            CellDataRef(data).SetColSpanOffset((colX - startColIndex) as u32);
                        }
                    }
                    self.SetDataAt(aMap, data, rowX, colX);
                }
            }
            cellFrame.SetColIndex(startColIndex);
        }
        let damageHeight = (self.GetRowGroup().GetRowCount() - aRowIndex).min(aRowSpan);
        SetDamageArea(aColIndex, aRgFirstRowIndex + aRowIndex, 1 + endColIndex - aColIndex, damageHeight, aDamageArea);

        // update the row and col info due to shifting
        for rowX in aRowIndex..=endRowIndex {
            let numCols = self.mRows[rowX as usize].len() as u32;
            let mut colX = (aColIndex + totalColSpan) as u32;
            while colX < numCols {
                let data = self.mRows[rowX as usize][colX as usize];
                if !data.is_null() {
                    // increase the origin and span counts beyond the spanned cols
                    if CellDataRef(data).IsOrig() {
                        // a cell that gets moved needs adjustment as well as it new
                        // orignating col
                        CellDataRef(data).GetCellFrame().unwrap().SetColIndex(colX as i32);
                        let colInfo = aMap.GetColInfoAt(colX as i32);
                        colInfo.mNumCellsOrig += 1;
                    }
                    if CellDataRef(data).IsColSpan() {
                        let colInfo = aMap.GetColInfoAt(colX as i32);
                        colInfo.mNumCellsSpan += 1;
                    }

                    // decrease the origin and span counts within the spanned cols
                    let colX2 = colX as i32 - totalColSpan;
                    let colInfo2 = aMap.GetColInfoAt(colX2);
                    if CellDataRef(data).IsOrig() {
                        // the old originating col of a moved cell needs adjustment
                        colInfo2.mNumCellsOrig -= 1;
                    }
                    if CellDataRef(data).IsColSpan() {
                        colInfo2.mNumCellsSpan -= 1;
                    }
                }
                colX += 1;
            }
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::ShrinkWithoutRows
    pub fn ShrinkWithoutRows(&mut self, aMap: &mut nsTableCellMap, aStartRowIndex: i32, aNumRowsToRemove: i32, aRgFirstRowIndex: i32, aDamageArea: &mut TableArea) {
        NS_ASSERTION!(aMap.mBCInfo.is_some() == self.mIsBC, "BC state mismatch");
        let endRowIndex = aStartRowIndex + aNumRowsToRemove - 1;
        let colCount = aMap.GetColCount() as u32;
        let mut rowX = endRowIndex;
        while rowX >= aStartRowIndex {
            for colX in 0..colCount {
                let data = SafeElementAt(&self.mRows[rowX as usize], colX as i32);
                if !data.is_null() {
                    // Adjust the column counts.
                    if CellDataRef(data).IsOrig() {
                        // Decrement the column count.
                        let colInfo = aMap.GetColInfoAt(colX as i32);
                        colInfo.mNumCellsOrig -= 1;
                    }
                    // colspan=0 is only counted as a spanned cell in the 1st col it spans
                    else if CellDataRef(data).IsColSpan() {
                        let colInfo = aMap.GetColInfoAt(colX as i32);
                        colInfo.mNumCellsSpan -= 1;
                    }
                }
            }

            let rowLength = self.mRows[rowX as usize].len();
            // Delete our row information.
            for colX in 0..rowLength {
                let data = self.mRows[rowX as usize][colX];
                self.DestroyCellData(data);
            }

            self.mRows.remove(rowX as usize);

            // Decrement our row and next available index counts.
            self.mContentRowCount -= 1;
            rowX -= 1;
        }
        aMap.RemoveColsAtEnd();
        // mark all following rows damaged, they might contain a previously set
        // damage area which we can not shift.
        let firstDamagedRow = aRgFirstRowIndex + aStartRowIndex;
        SetDamageArea(0, firstDamagedRow, aMap.GetColCount(), aMap.GetRowCount() - firstDamagedRow, aDamageArea);
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::ShrinkWithoutCell
    pub fn ShrinkWithoutCell(&mut self, aMap: &mut nsTableCellMap, aCellFrame: nsTableCellFrame, aRowIndex: i32, aColIndex: i32, aRgFirstRowIndex: i32, aDamageArea: &mut TableArea) {
        NS_ASSERTION!(aMap.mBCInfo.is_some() == self.mIsBC, "BC state mismatch");

        // get the rowspan and colspan from the cell map since the content may have
        // changed
        let rowSpan = self.GetRowSpan(aRowIndex, aColIndex, true);
        let colSpan = self.GetEffectiveColSpan(aMap, aRowIndex, aColIndex) as u32;
        let endRowIndex = (aRowIndex + rowSpan - 1) as u32;
        let endColIndex = aColIndex as u32 + colSpan - 1;

        // adjust the col counts due to the deleted cell before removing it
        for colX in aColIndex as u32..=endColIndex {
            let colInfo = aMap.GetColInfoAt(colX as i32);
            if colX == aColIndex as u32 {
                colInfo.mNumCellsOrig -= 1;
            } else {
                colInfo.mNumCellsSpan -= 1;
            }
        }

        // remove the deleted cell and cellData entries for it
        for rowX in aRowIndex as u32..=endRowIndex {
            // endIndexForRow points at the first slot we don't want to clean up.  This
            // makes the aColIndex == 0 case work right with our unsigned int colX.
            NS_ASSERTION!(endColIndex as usize + 1 <= self.mRows[rowX as usize].len(), "span beyond the row size!");
            let endIndexForRow = (endColIndex + 1).min(self.mRows[rowX as usize].len() as u32);

            // Since endIndexForRow <= row.Length(), enough to compare aColIndex to it.
            if (aColIndex as u32) < endIndexForRow {
                let mut colX = endIndexForRow;
                while colX > aColIndex as u32 {
                    let data = self.mRows[rowX as usize][colX as usize - 1];
                    self.DestroyCellData(data);
                    colX -= 1;
                }
                self.mRows[rowX as usize].drain(aColIndex as usize..endIndexForRow as usize);
            }
        }

        let numCols = aMap.GetColCount() as u32;

        // update the row and col info due to shifting
        for rowX in aRowIndex as u32..=endRowIndex {
            let mut colX = aColIndex as u32;
            while colX < numCols - colSpan {
                let data = SafeElementAt(&self.mRows[rowX as usize], colX as i32);
                if !data.is_null() {
                    if CellDataRef(data).IsOrig() {
                        // a cell that gets moved to the left needs adjustment in its new
                        // location
                        CellDataRef(data).GetCellFrame().unwrap().SetColIndex(colX as i32);
                        let colInfo = aMap.GetColInfoAt(colX as i32);
                        colInfo.mNumCellsOrig += 1;
                        // a cell that gets moved to the left needs adjustment in its old
                        // location
                        let colInfo = aMap.GetColInfoAt((colX + colSpan) as i32);
                        // (never null: GetColInfoAt adds the column) if (colInfo) {
                        colInfo.mNumCellsOrig -= 1;
                        // }
                    } else if CellDataRef(data).IsColSpan() {
                        // a cell that gets moved to the left needs adjustment
                        // in its new location
                        let colInfo = aMap.GetColInfoAt(colX as i32);
                        colInfo.mNumCellsSpan += 1;
                        // a cell that gets moved to the left needs adjustment
                        // in its old location
                        let colInfo = aMap.GetColInfoAt((colX + colSpan) as i32);
                        // (never null) if (colInfo) {
                        colInfo.mNumCellsSpan -= 1;
                        // }
                    }
                }
                colX += 1;
            }
        }
        aMap.RemoveColsAtEnd();
        SetDamageArea(aColIndex, aRgFirstRowIndex + aRowIndex, 0.max(aMap.GetColCount() - aColIndex - 1), 1 + endRowIndex as i32 - aRowIndex, aDamageArea);
    }

    /**
     * Rebuild due to rows being inserted or deleted with cells spanning
     * into or out of the rows.  This function can only handle insertion
     * or deletion but NOT both.  So either aRowsToInsert must be null
     * or aNumRowsToRemove must be 0.
     *
     * // XXXbz are both allowed to happen?  That'd be a no-op...
     */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::RebuildConsideringRows
    pub fn RebuildConsideringRows(&mut self, aMap: &mut nsTableCellMap, aStartRowIndex: i32, aRowsToInsert: Option<&Vec<nsTableRowFrame>>, aNumRowsToRemove: i32) {
        NS_ASSERTION!(aMap.mBCInfo.is_some() == self.mIsBC, "BC state mismatch");
        // copy the old cell map into a new array
        let numOrigRows = self.mRows.len() as u32;
        let origRows: Vec<nsCellMap_CellDataArray> = std::mem::take(&mut self.mRows);

        let rowNumberChange = match aRowsToInsert {
            Some(rows) => rows.len() as i32,
            None => -aNumRowsToRemove,
        };

        // adjust mContentRowCount based on the function arguments as they are known
        // to be real rows.
        self.mContentRowCount += rowNumberChange;
        NS_ASSERTION!(self.mContentRowCount >= 0, "previous mContentRowCount was wrong");
        // mRows is empty now.  Grow it to the size we expect it to have.
        if self.mContentRowCount != 0 {
            if !self.Grow(aMap, self.mContentRowCount, -1) {
                // Bail, I guess...  Not sure what else we can do here.
                return;
            }
        }

        // aStartRowIndex might be after all existing rows so we should limit the
        // copy to the amount of exisiting rows
        let copyEndRowIndex = numOrigRows.min(aStartRowIndex as u32);

        // rowX keeps track of where we are in mRows while setting up the
        // new cellmap.
        let mut rowX: u32 = 0;
        let mut damageArea = TableArea::default();
        // put back the rows before the affected ones just as before.  Note that we
        // can't just copy the old rows in bit-for-bit, because they might be
        // spanning out into the rows we're adding/removing.
        while rowX < copyEndRowIndex {
            let row = &origRows[rowX as usize];
            let numCols = row.len() as u32;
            for colX in 0..numCols {
                // put in the original cell from the cell map
                let data = row[colX as usize];
                if !data.is_null() && CellDataRef(data).IsOrig() {
                    self.AppendCell(aMap, CellDataRef(data).GetCellFrame(), rowX as i32, false, 0, &mut damageArea, None);
                }
            }
            rowX += 1;
        }

        // Now handle the new rows being inserted, if any.
        let copyStartRowIndex: u32;
        rowX = aStartRowIndex as u32;
        if let Some(rowsToInsert) = aRowsToInsert {
            // add in the new cells and create rows if necessary
            let numNewRows = rowsToInsert.len() as i32;
            for newRowX in 0..numNewRows {
                let rFrame = rowsToInsert[newRowX as usize];
                let mut cellFrame = rFrame.GetFirstCell();
                while let Some(cell) = cellFrame {
                    self.AppendCell(aMap, Some(cell), rowX as i32, false, 0, &mut damageArea, None);
                    cellFrame = cell.GetNextCell();
                }
                rowX += 1;
            }
            copyStartRowIndex = aStartRowIndex as u32;
        } else {
            copyStartRowIndex = (aStartRowIndex + aNumRowsToRemove) as u32;
        }

        // put back the rows after the affected ones just as before.  Again, we can't
        // just copy the old bits because that would not handle the new rows spanning
        // out or our earlier old rows spanning through the damaged area.
        let mut copyRowX = copyStartRowIndex;
        while copyRowX < numOrigRows {
            let row = &origRows[copyRowX as usize];
            let numCols = row.len() as u32;
            for colX in 0..numCols {
                // put in the original cell from the cell map
                let data = row[colX as usize];
                if !data.is_null() && CellDataRef(data).IsOrig() {
                    self.AppendCell(aMap, CellDataRef(data).GetCellFrame(), rowX as i32, false, 0, &mut damageArea, None);
                }
            }
            rowX += 1;
            copyRowX += 1;
        }

        // delete the old cell map.  Now rowX no longer has anything to do with mRows
        for rowX in 0..numOrigRows {
            let row = &origRows[rowX as usize];
            let len = row.len() as u32;
            for colX in 0..len {
                self.DestroyCellData(row[colX as usize]);
            }
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::RebuildConsideringCells
    pub fn RebuildConsideringCells(&mut self, aMap: &mut nsTableCellMap, aNumOrigCols: i32, aCellFrames: Option<&Vec<nsTableCellFrame>>, aRowIndex: i32, aColIndex: i32, aInsert: bool) {
        NS_ASSERTION!(aMap.mBCInfo.is_some() == self.mIsBC, "BC state mismatch");
        // copy the old cell map into a new array
        let numOrigRows = self.mRows.len() as i32;
        let origRows: Vec<nsCellMap_CellDataArray> = std::mem::take(&mut self.mRows);

        let numNewCells = aCellFrames.map_or(0, |cells| cells.len() as i32);

        // the new cells might extend the previous column number
        NS_ASSERTION!(aNumOrigCols >= aColIndex, "Appending cells far beyond cellmap data?!");
        let numCols = if aInsert { aNumOrigCols.max(aColIndex + 1) } else { aNumOrigCols };

        // build the new cell map.  Hard to say what, if anything, we can preallocate
        // here...  Should come back to that sometime, perhaps.
        let mut damageArea = TableArea::default();
        for rowX in 0..numOrigRows {
            let row = &origRows[rowX as usize];
            for colX in 0..numCols {
                if (rowX == aRowIndex) && (colX == aColIndex) {
                    if aInsert {
                        // put in the new cells
                        for cellX in 0..numNewCells {
                            let cell = aCellFrames.unwrap()[cellX as usize];
                            // (the C++ skips a null cell; a Vec<nsTableCellFrame> has none)
                            self.AppendCell(aMap, Some(cell), rowX, false, 0, &mut damageArea, None);
                        }
                    } else {
                        continue; // do not put the deleted cell back
                    }
                }
                // put in the original cell from the cell map
                let data = SafeElementAt(row, colX);
                if !data.is_null() && CellDataRef(data).IsOrig() {
                    self.AppendCell(aMap, CellDataRef(data).GetCellFrame(), rowX, false, 0, &mut damageArea, None);
                }
            }
        }
        if aInsert && numOrigRows <= aRowIndex {
            // append the new cells below the last original row
            NS_ASSERTION!(numOrigRows == aRowIndex, "Appending cells far beyond the last row");
            for cellX in 0..numNewCells {
                let cell = aCellFrames.unwrap()[cellX as usize];
                self.AppendCell(aMap, Some(cell), aRowIndex, false, 0, &mut damageArea, None);
            }
        }

        // delete the old cell map
        for rowX in 0..numOrigRows {
            let row = &origRows[rowX as usize];
            let len = row.len() as u32;
            for colX in 0..len {
                self.DestroyCellData(SafeElementAt(row, colX as i32));
            }
        }
        // expand the cellmap to cover empty content rows
        if (self.mRows.len() as u32) < self.mContentRowCount as u32 {
            self.Grow(aMap, self.mContentRowCount - self.mRows.len() as i32, -1);
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::CellsSpanOut
    pub fn CellsSpanOut(&self, aRows: &Vec<nsTableRowFrame>) -> bool {
        let numNewRows = aRows.len() as i32;
        for rowX in 0..numNewRows {
            let rowFrame = aRows[rowX as usize];
            let mut cellFrame = rowFrame.GetFirstCell();
            while let Some(cell) = cellFrame {
                let mut zeroSpan = false;
                let rowSpan = self.GetRowSpanForNewCell(cell, rowX, &mut zeroSpan);
                if zeroSpan || rowX + rowSpan > numNewRows {
                    return true;
                }
                cellFrame = cell.GetNextCell();
            }
        }
        false
    }

    // return true if any cells have rows spans into or out of the region
    // defined by the row and col indices or any cells have colspans into the region
    /** If a cell spans out of the area defined by aStartRowIndex, aEndRowIndex
      * and aStartColIndex, aEndColIndex the cellmap changes are more severe so
      * the corresponding routines needs to be called. This is also necessary if
      * cells outside spans into this region.
      * @aStartRowIndex       - y start index
      * @aEndRowIndex         - y end index
      * @param aStartColIndex - x start index
      * @param aEndColIndex   - x end index
      * @return               - true if a cell span crosses the border of the
                                region
      */
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::CellsSpanInOrOut
    pub fn CellsSpanInOrOut(&self, aStartRowIndex: i32, aEndRowIndex: i32, aStartColIndex: i32, aEndColIndex: i32) -> bool {
        /*
         * this routine will watch the cells adjacent to the region or at the edge
         * they are marked with *. The routine will verify whether they span in or
         * are spanned out.
         *
         *                           startCol          endCol
         *             r1c1   r1c2   r1c3      r1c4    r1c5    r1rc6  r1c7
         *  startrow   r2c1   r2c2  *r2c3     *r2c4   *r2c5   *r2rc6  r2c7
         *  endrow     r3c1   r3c2  *r3c3      r3c4    r3c5   *r3rc6  r3c7
         *             r4c1   r4c2  *r4c3     *r4c4   *r4c5    r4rc6  r4c7
         *             r5c1   r5c2   r5c3      r5c4    r5c5    r5rc6  r5c7
         */

        let numRows = self.mRows.len() as i32; // use the cellmap rows to determine the
        // current cellmap extent.
        for colX in aStartColIndex..=aEndColIndex {
            let mut cellData: *mut CellData;
            if aStartRowIndex > 0 {
                cellData = self.GetDataAt(aStartRowIndex, colX);
                if !cellData.is_null() && CellDataRef(cellData).IsRowSpan() {
                    return true; // there is a row span into the region
                }
                if (aStartRowIndex >= self.mContentRowCount) && (self.mContentRowCount > 0) {
                    cellData = self.GetDataAt(self.mContentRowCount - 1, colX);
                    if !cellData.is_null() && CellDataRef(cellData).IsZeroRowSpan() {
                        return true; // When we expand the zerospan it'll span into our row
                    }
                }
            }
            if aEndRowIndex < numRows - 1 {
                // is there anything below aEndRowIndex
                cellData = self.GetDataAt(aEndRowIndex + 1, colX);
                if !cellData.is_null() && CellDataRef(cellData).IsRowSpan() {
                    return true; // there is a row span out of the region
                }
            } else {
                cellData = self.GetDataAt(aEndRowIndex, colX);
                if !cellData.is_null() && CellDataRef(cellData).IsRowSpan() && (self.mContentRowCount < numRows) {
                    return true; // this cell might be the cause of a dead row
                }
            }
        }
        if aStartColIndex > 0 {
            for rowX in aStartRowIndex..=aEndRowIndex {
                let mut cellData = self.GetDataAt(rowX, aStartColIndex);
                if !cellData.is_null() && CellDataRef(cellData).IsColSpan() {
                    return true; // there is a col span into the region
                }
                cellData = self.GetDataAt(rowX, aEndColIndex + 1);
                if !cellData.is_null() && CellDataRef(cellData).IsColSpan() {
                    return true; // there is a col span out of the region
                }
            }
        }
        false
    }

    // Gecko: layout/tables/nsCellMap.h nsCellMap::CreateEmptyRow
    // NOT PORTED: declared in nsCellMap.h, never defined in nsCellMap.cpp.

    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::GetRowSpanForNewCell
    pub fn GetRowSpanForNewCell(&self, aCellFrameToAdd: nsTableCellFrame, aRowIndex: i32, aIsZeroRowSpan: &mut bool) -> i32 {
        *aIsZeroRowSpan = false;
        let mut rowSpan = aCellFrameToAdd.GetRowSpan();
        if 0 == rowSpan {
            // Use a min value of 2 for a zero rowspan to make computations easier
            // elsewhere. Zero rowspans are only content dependent!
            rowSpan = (self.mContentRowCount - aRowIndex).clamp(2, MAX_ROWSPAN);
            *aIsZeroRowSpan = true;
        }
        rowSpan
    }

    // Destroy a CellData struct.  This will handle the case of aData
    // actually being a BCCellData properly.
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::DestroyCellData
    pub fn DestroyCellData(&self, aData: *mut CellData) {
        if aData.is_null() {
            return;
        }

        if self.mIsBC {
            // BCCellData* bcData = static_cast<BCCellData*>(aData); bcData->~BCCellData(); free it
            drop(unsafe { Box::from_raw(aData as *mut BCCellData) });
        } else {
            // aData->~CellData(); free it
            drop(unsafe { Box::from_raw(aData) });
        }
    }

    // Allocate a CellData struct.  This will handle needing to create a
    // BCCellData properly.
    // @param aOrigCell the originating cell to pass to the celldata constructor
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::AllocCellData
    pub fn AllocCellData(&self, aOrigCell: Option<nsTableCellFrame>) -> *mut CellData {
        if self.mIsBC {
            return Box::into_raw(Box::new(BCCellData::new(aOrigCell))) as *mut CellData;
        }

        Box::into_raw(Box::new(CellData::new(aOrigCell)))
    }
}

impl Drop for nsCellMap {
    // Gecko: layout/tables/nsCellMap.cpp nsCellMap::~nsCellMap
    fn drop(&mut self) {
        // NOT PORTED: MOZ_COUNT_DTOR(nsCellMap)
        let mapRowCount = self.mRows.len();
        for rowX in 0..mapRowCount {
            let colCount = self.mRows[rowX].len();
            for colX in 0..colCount {
                let data = self.mRows[rowX][colX];
                self.DestroyCellData(data);
            }
        }
    }
}

/**
 * A class for iterating the cells in a given column.  Must be given a
 * non-null nsTableCellMap and a column number valid for that cellmap.
 */
// Gecko: layout/tables/nsCellMap.h nsCellMapColumnIterator
pub struct nsCellMapColumnIterator<'a> {
    mMap: &'a nsTableCellMap,
    mCurMap: *const nsCellMap,

    // mCurMapStart is the row in the entire nsTableCellMap where
    // mCurMap starts.  This is used to compute row indices to pass to
    // nsTableCellMap::GetDataAt, so must be a _content_ row index.
    mCurMapStart: u32,

    // In steady-state mCurMapRow is the row in our current nsCellMap
    // that we'll use the next time GetNextFrame() is called.  Due to
    // the way we skip over rowspans, the entry in mCurMapRow and mCol
    // is either null, dead, originating, or a colspan.  In particular,
    // it cannot be a rowspan or overlap entry.
    mCurMapRow: u32,
    mCol: i32,
    mOrigCells: u32,
    mFoundCells: u32,

    // The number of content rows in mCurMap.  This may be bigger than the number
    // of "relevant" rows, or it might be smaller.
    mCurMapContentRowCount: u32,

    // The number of "relevant" rows in mCurMap.  That is, the number of rows
    // which might have an originating cell in them.  Once mCurMapRow reaches
    // mCurMapRelevantRowCount, we should move to the next map.
    mCurMapRelevantRowCount: u32,
}

impl<'a> nsCellMapColumnIterator<'a> {
    // Gecko: layout/tables/nsCellMap.h nsCellMapColumnIterator::nsCellMapColumnIterator
    pub fn new(aMap: &'a nsTableCellMap, aCol: i32) -> nsCellMapColumnIterator<'a> {
        let mut this = nsCellMapColumnIterator {
            mMap: aMap,
            mCurMap: aMap.mFirstMap,
            mCurMapStart: 0,
            mCurMapRow: 0,
            mCol: aCol,
            mFoundCells: 0,
            mOrigCells: 0,
            mCurMapContentRowCount: 0,
            mCurMapRelevantRowCount: 0,
        };
        MOZ_ASSERT!(this.mCol < aMap.GetColCount(), "Invalid column");
        this.mOrigCells = aMap.GetNumCellsOriginatingInCol(this.mCol) as u32;
        if !this.mCurMap.is_null() {
            let curMap = unsafe { &*this.mCurMap };
            this.mCurMapContentRowCount = curMap.GetRowCount(false) as u32;
            let rowArrayLength = curMap.mRows.len() as u32;
            this.mCurMapRelevantRowCount = this.mCurMapContentRowCount.min(rowArrayLength);
            if this.mCurMapRelevantRowCount == 0 && this.mOrigCells > 0 {
                // This row group is useless; advance!
                this.AdvanceRowGroup();
            }
        } else {
            #[cfg(debug_assertions)]
            NS_ASSERTION!(this.mOrigCells == 0, "Why no rowgroups?");
        }
        this
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMapColumnIterator::GetNextFrame
    pub fn GetNextFrame(&mut self, aRow: &mut i32, aColSpan: &mut i32) -> Option<nsTableCellFrame> {
        // Fast-path for the case when we don't have anything left in the column and
        // we know it.
        if self.mFoundCells == self.mOrigCells {
            *aRow = 0;
            *aColSpan = 1;
            return None;
        }

        loop {
            NS_ASSERTION!(self.mCurMapRow < self.mCurMapRelevantRowCount, "Bogus mOrigCells?");
            let curMap = unsafe { &*self.mCurMap };
            // Safe to just get the row (which is faster than calling GetDataAt(), but
            // there may not be that many cells in it, so have to use SafeElementAt for
            // the mCol.
            let row = &curMap.mRows[self.mCurMapRow as usize];
            let cellData = SafeElementAt(row, self.mCol);
            if cellData.is_null() || CellDataRef(cellData).IsDead() {
                // Could hit this if there are fewer cells in this row than others, for
                // example.
                self.IncrementRow(1);
                continue;
            }

            if CellDataRef(cellData).IsColSpan() {
                // Look up the originating data for this cell, advance by its relative
                // rowspan.
                let rowspanOffset = CellDataRef(cellData).GetRowSpanOffset() as i32;
                let cellFrame = curMap.GetCellFrame(self.mCurMapRow as i32, self.mCol, CellDataRef(cellData), false);
                NS_ASSERTION!(cellFrame.is_some(), "Must have usable originating data here");
                let rowSpan = cellFrame.unwrap().GetRowSpan();
                if rowSpan == 0 {
                    self.AdvanceRowGroup();
                } else {
                    self.IncrementRow(rowSpan - rowspanOffset);
                }
                continue;
            }

            NS_ASSERTION!(CellDataRef(cellData).IsOrig(), "Must have originating cellData by this point.  See comment on mCurMapRow in header.");

            let cellFrame = CellDataRef(cellData).GetCellFrame();
            NS_ASSERTION!(cellFrame.is_some(), "Orig data without cellframe?");
            let cellFrame = cellFrame.unwrap();

            *aRow = (self.mCurMapStart + self.mCurMapRow) as i32;
            *aColSpan = curMap.GetEffectiveColSpan(self.mMap, self.mCurMapRow as i32, self.mCol);

            self.IncrementRow(cellFrame.GetRowSpan());

            self.mFoundCells += 1;

            MOZ_ASSERT!(cellData == self.mMap.GetDataAt(*aRow, self.mCol), "Giving caller bogus row?");

            return Some(cellFrame);
        }
    }

    // Gecko: layout/tables/nsCellMap.cpp nsCellMapColumnIterator::AdvanceRowGroup
    fn AdvanceRowGroup(&mut self) {
        loop {
            self.mCurMapStart += self.mCurMapContentRowCount;
            self.mCurMap = unsafe { &*self.mCurMap }.GetNextSibling();
            if self.mCurMap.is_null() {
                // Set mCurMapContentRowCount and mCurMapRelevantRowCount to 0 in case
                // mCurMap has no next sibling.  This can happen if we just handled the
                // last originating cell.  Future calls will end up with mFoundCells ==
                // mOrigCells, but for this one mFoundCells was definitely not big enough
                // if we got here.
                self.mCurMapContentRowCount = 0;
                self.mCurMapRelevantRowCount = 0;
                break;
            }

            let curMap = unsafe { &*self.mCurMap };
            self.mCurMapContentRowCount = curMap.GetRowCount(false) as u32;
            let rowArrayLength = curMap.mRows.len() as u32;
            self.mCurMapRelevantRowCount = self.mCurMapContentRowCount.min(rowArrayLength);
            if 0 != self.mCurMapRelevantRowCount {
                break;
            }
        }

        NS_ASSERTION!(self.mCurMapRelevantRowCount != 0 || self.mCurMap.is_null(), "How did that happen?");

        // Set mCurMapRow to 0, since cells can't span across table row groups.
        self.mCurMapRow = 0;
    }

    // Advance the row; aIncrement is considered to be a cell's rowspan,
    // so if 0 is passed in we'll advance to the next rowgroup.
    // Gecko: layout/tables/nsCellMap.cpp nsCellMapColumnIterator::IncrementRow
    fn IncrementRow(&mut self, aIncrement: i32) {
        MOZ_ASSERT!(aIncrement >= 0, "Bogus increment");
        MOZ_ASSERT!(!self.mCurMap.is_null(), "Bogus mOrigCells?");
        if aIncrement == 0 {
            self.AdvanceRowGroup();
        } else {
            self.mCurMapRow += aIncrement as u32;
            if self.mCurMapRow >= self.mCurMapRelevantRowCount {
                self.AdvanceRowGroup();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockReflowState_test_support::*;

    // Written from the documented behaviour of the cell map (nsCellMap.h comments, CSS 2.2 17.5 and
    // the HTML rowspan/colspan rules); Gecko has no gtest for nsCellMap.

    // A table frame, its one row group, its rows with their cells, and one more row (`extra`) that
    // holds cells the test adds later with AppendCell or InsertCells.
    struct TestTable {
        table: nsTableFrame,
        rg: nsTableRowGroupFrame,
        rows: Vec<nsTableRowFrame>,
        cells: Vec<Vec<nsTableCellFrame>>,
        extra: Vec<nsTableCellFrame>,
    }

    // `aRows[r][c]` is (colspan, rowspan) of cell c of row r; `aExtra` are the cells of the extra row.
    fn build(aRows: &[&[(i32, i32)]], aExtra: &[(i32, i32)]) -> TestTable {
        let mut html = String::from(r#"<div id=t style="display:table"><div id=g style="display:table-row-group">"#);
        let mut tree: Vec<(String, Option<usize>)> = vec![("t".into(), None), ("g".into(), Some(0))];
        let mut rowSpecs: Vec<&[(i32, i32)]> = aRows.to_vec();
        rowSpecs.push(aExtra);
        for (r, cells) in rowSpecs.iter().enumerate() {
            html += &format!(r#"<div id=r{r} style="display:table-row">"#);
            let rowIndex = tree.len();
            tree.push((format!("r{r}"), Some(1)));
            for c in 0..cells.len() {
                html += &format!(r#"<div id=c{r}x{c} style="display:table-cell"></div>"#);
                tree.push((format!("c{r}x{c}"), Some(rowIndex)));
            }
            html += "</div>";
        }
        html += "</div></div>";
        let treeRef: Vec<(&str, Option<usize>)> = tree.iter().map(|(n, p)| (n.as_str(), *p)).collect();
        let f = frames(&html, &treeRef, 100);
        f[0].SetType(LayoutFrameType::Table);
        f[1].SetType(LayoutFrameType::TableRowGroup);
        let mut rows = Vec::new();
        let mut cells: Vec<Vec<nsTableCellFrame>> = Vec::new();
        let mut at = 2;
        for spec in &rowSpecs {
            f[at].SetType(LayoutFrameType::TableRow);
            rows.push(nsTableRowFrame::from_frame(f[at]));
            at += 1;
            let mut rowCells = Vec::new();
            for &(colspan, rowspan) in spec.iter() {
                f[at].SetType(LayoutFrameType::TableCell);
                let cell = nsTableCellFrame::from_frame(f[at]);
                cell.SetSpans(rowspan, colspan);
                rowCells.push(cell);
                at += 1;
            }
            cells.push(rowCells);
        }
        let extraRow = rows.pop().unwrap();
        let extra = cells.pop().unwrap();
        TestTable { table: nsTableFrame::from_frame(f[0]), rg: nsTableRowGroupFrame::from_frame(f[1]), rows, cells, extra }
    }

    // The cell map of the table with its real rows inserted into the row group's cell map, as
    // nsTableFrame::InsertRows does for the rows of a new row group.
    fn map_of(t: &TestTable) -> nsTableCellMap {
        let mut map = nsTableCellMap::new(t.table, false);
        let mut rows = t.rows.clone();
        let mut damage = TableArea::default();
        map.InsertRows(t.rg, &mut rows, 0, false, &mut damage);
        map
    }

    fn data<'a>(aMap: &'a nsTableCellMap, aRow: i32, aCol: i32) -> &'a CellData {
        let p = aMap.GetDataAt(aRow, aCol);
        assert!(!p.is_null(), "no data at ({aRow}, {aCol})");
        unsafe { &*p }
    }

    #[test]
    fn simple_grid_maps_every_cell_to_its_own_entry() {
        let t = build(&[&[(1, 1), (1, 1), (1, 1)], &[(1, 1), (1, 1), (1, 1)]], &[]);
        let map = map_of(&t);
        assert_eq!((map.GetRowCount(), map.GetColCount()), (2, 3));
        for r in 0..2 {
            for c in 0..3 {
                let d = data(&map, r, c);
                assert!(d.IsOrig() && !d.IsSpan() && !d.IsDead());
                assert_eq!(d.GetCellFrame(), Some(t.cells[r as usize][c as usize]));
                assert_eq!(t.cells[r as usize][c as usize].ColIndex(), c as u32);
            }
            assert_eq!(map.GetNumCellsOriginatingInRow(r), 3);
        }
        for c in 0..3 {
            assert_eq!(map.GetNumCellsOriginatingInCol(c), 2);
        }
        assert!(map.HasMoreThanOneCell(0));
        assert_eq!(map.GetEffectiveColSpan(0, 0), 1);
        assert_eq!(map.GetEffectiveRowSpan(0, 0), 1);
    }

    #[test]
    fn colspan_fills_the_spanned_columns_with_col_span_entries() {
        let t = build(&[&[(2, 1), (1, 1)], &[(1, 1), (1, 1), (1, 1)]], &[]);
        let map = map_of(&t);
        assert_eq!(map.GetColCount(), 3);
        assert!(data(&map, 0, 0).IsOrig());
        let spanned = data(&map, 0, 1);
        assert!(spanned.IsColSpan() && !spanned.IsRowSpan() && !spanned.IsOrig());
        assert_eq!(spanned.GetColSpanOffset(), 1);
        assert_eq!(data(&map, 0, 2).GetCellFrame(), Some(t.cells[0][1]));
        assert_eq!(t.cells[0][1].ColIndex(), 2);
        assert_eq!(map.GetEffectiveColSpan(0, 0), 2);
        assert_eq!(map.GetEffectiveColSpan(0, 2), 1);
        // column 0 has a cell originating in each row, column 1 has one spanning into it
        assert_eq!(map.GetNumCellsOriginatingInCol(0), 2);
        assert_eq!(map.mCols[1].mNumCellsSpan, 1);
        assert_eq!(map.mCols[1].mNumCellsOrig, 1);
        // the cell of a spanned entry is the originating cell
        let mut originates = true;
        let mut colSpan = 0;
        assert_eq!(map.GetCellInfoAt(0, 1, Some(&mut originates), Some(&mut colSpan)), Some(t.cells[0][0]));
        assert!(!originates);
        assert_eq!(colSpan, 2);
    }

    #[test]
    fn rowspan_pushes_the_next_rows_cells_aside() {
        // HTML table model: a cell covered by a rowspan is not available for the next row's cells.
        let t = build(&[&[(1, 2), (1, 1)], &[(1, 1)]], &[]);
        let map = map_of(&t);
        assert_eq!((map.GetRowCount(), map.GetColCount()), (2, 2));
        let spanned = data(&map, 1, 0);
        assert!(spanned.IsRowSpan() && !spanned.IsColSpan());
        assert_eq!(spanned.GetRowSpanOffset(), 1);
        assert_eq!(data(&map, 1, 1).GetCellFrame(), Some(t.cells[1][0]));
        assert_eq!(t.cells[1][0].ColIndex(), 1);
        assert_eq!(map.GetEffectiveRowSpan(0, 0), 2);
        assert_eq!(map.GetEffectiveRowSpan(0, 1), 1);
        let mut originates = true;
        assert_eq!(map.GetCellInfoAt(1, 0, Some(&mut originates), None), Some(t.cells[0][0]));
        assert!(!originates);
        assert!(map.RowIsSpannedInto(1, 2));
        assert!(!map.RowIsSpannedInto(0, 2));
        assert!(map.RowHasSpanningCells(0, 2));
        assert!(!map.RowHasSpanningCells(1, 2));
        assert_eq!(map.GetNumCellsOriginatingInRow(1), 1);
        // row 1 has the cell spanned in from above and its own cell
        assert!(map.HasMoreThanOneCell(1));
    }

    #[test]
    fn row_and_col_span_together_cover_a_rectangle() {
        let t = build(&[&[(2, 2), (1, 1)], &[(1, 1)]], &[]);
        let map = map_of(&t);
        assert_eq!(map.GetColCount(), 3);
        assert!(data(&map, 0, 0).IsOrig());
        assert!(data(&map, 0, 1).IsColSpan() && !data(&map, 0, 1).IsRowSpan());
        assert!(data(&map, 1, 0).IsRowSpan() && !data(&map, 1, 0).IsColSpan());
        let corner = data(&map, 1, 1);
        assert!(corner.IsRowSpan() && corner.IsColSpan() && !corner.IsOverlap());
        assert_eq!((corner.GetRowSpanOffset(), corner.GetColSpanOffset()), (1, 1));
        assert_eq!(map.GetCellFrame(1, 1, corner, true), Some(t.cells[0][0]));
        // row 0's second cell is at column 2, row 1's only cell goes after the spanned area
        assert_eq!(t.cells[0][1].ColIndex(), 2);
        assert_eq!(t.cells[1][0].ColIndex(), 2);
        assert_eq!(map.GetEffectiveRowSpan(0, 0), 2);
        assert_eq!(map.GetEffectiveColSpan(0, 0), 2);
        assert_eq!(map.GetEffectiveColSpan(1, 0), 2);
    }

    #[test]
    fn zero_rowspan_spans_to_the_end_of_the_row_group() {
        // HTML: rowspan=0 spans all the remaining rows of the row group.
        let t = build(&[&[(1, 0)], &[(1, 1)], &[(1, 1)]], &[]);
        let map = map_of(&t);
        assert_eq!(map.GetRowCount(), 3);
        assert!(data(&map, 1, 0).IsZeroRowSpan());
        assert!(data(&map, 2, 0).IsZeroRowSpan());
        assert_eq!(map.GetEffectiveRowSpan(0, 0), 3);
        assert_eq!(t.cells[1][0].ColIndex(), 1);
        assert_eq!(t.cells[2][0].ColIndex(), 1);
    }

    #[test]
    fn appending_a_dead_cell_makes_a_dead_entry() {
        // A dead cell only fills a free slot of an existing column ("dead cells should not require new
        // columns"): row 1 has one cell for the two columns, so the dead cell takes column 1.
        let t = build(&[&[(1, 1), (1, 1)], &[(1, 1)]], &[]);
        let mut map = map_of(&t);
        let mut damage = TableArea::default();
        let cellMap = map.mFirstMap;
        let dead = unsafe { &mut *cellMap }.AppendCell(&mut map, None, 1, false, 0, &mut damage, None);
        assert!(!dead.is_null());
        let dead = unsafe { &*dead };
        assert!(dead.IsDead() && !dead.IsOrig() && !dead.IsSpan());
        assert!(dead.GetCellFrame().is_none());
        assert_eq!(map.GetColCount(), 2);
        assert!(data(&map, 1, 1).IsDead());
    }

    #[test]
    fn append_cell_after_a_rowspan_skips_the_covered_column() {
        let t = build(&[&[(1, 2), (1, 1)], &[]], &[(1, 1)]);
        let mut map = nsTableCellMap::new(t.table, false);
        let mut damage = TableArea::default();
        // the content row 0 only (row 1 is empty), then the cell for row 1 arrives
        let mut rows = vec![t.rows[0]];
        map.InsertRows(t.rg, &mut rows, 0, false, &mut damage);
        let extra = t.extra[0];
        // nsTableCellMap::AppendCell looks at the cell's row group, so the extra cell is a child of
        // a row of the same row group
        assert!(!map.AppendCell(extra, 1, true, &mut damage).is_null());
        assert!(data(&map, 1, 0).IsRowSpan());
        assert_eq!(data(&map, 1, 1).GetCellFrame(), Some(extra));
        assert_eq!(extra.ColIndex(), 1);
        assert_eq!(map.GetRowCount(), 2);
        assert_eq!(map.GetColCount(), 2);
    }

    #[test]
    fn append_cell_with_a_rowspan_over_later_rows_rebuilds_the_map() {
        // The new cell spans into row 1, where a cell already originates: the map is rebuilt and the
        // old cell of row 1 moves to the first free column.
        let t = build(&[&[(1, 1)], &[(1, 1)]], &[(1, 2)]);
        let mut map = map_of(&t);
        let mut damage = TableArea::default();
        let extra = t.extra[0];
        map.AppendCell(extra, 0, true, &mut damage);
        assert_eq!(map.GetColCount(), 2);
        assert_eq!(data(&map, 0, 0).GetCellFrame(), Some(t.cells[0][0]));
        assert_eq!(data(&map, 0, 1).GetCellFrame(), Some(extra));
        assert!(data(&map, 1, 1).IsRowSpan());
        assert_eq!(data(&map, 1, 0).GetCellFrame(), Some(t.cells[1][0]));
        assert_eq!(extra.ColIndex(), 1);
        assert_eq!(t.cells[1][0].ColIndex(), 0);
        // the whole map is damaged
        assert_eq!((damage.StartCol(), damage.StartRow(), damage.ColCount(), damage.RowCount()), (0, 0, 2, 2));
    }

    #[test]
    fn insert_cells_in_the_middle_shifts_the_cells_after_them() {
        let t = build(&[&[(1, 1), (1, 1)]], &[(1, 1)]);
        let mut map = map_of(&t);
        assert_eq!(map.GetColCount(), 2);
        let (a, b) = (t.cells[0][0], t.cells[0][1]);
        let n = t.extra[0];
        let mut damage = TableArea::default();
        let mut newCells = vec![n];
        map.InsertCells(&mut newCells, 0, 0, &mut damage);
        assert_eq!(map.GetColCount(), 3);
        assert_eq!(data(&map, 0, 0).GetCellFrame(), Some(a));
        assert_eq!(data(&map, 0, 1).GetCellFrame(), Some(n));
        assert_eq!(data(&map, 0, 2).GetCellFrame(), Some(b));
        assert_eq!((a.ColIndex(), n.ColIndex(), b.ColIndex()), (0, 1, 2));
        for c in 0..3 {
            assert_eq!(map.GetNumCellsOriginatingInCol(c), 1);
        }
        // the damaged area is the new cell's columns (nsTableFrame::ExpandBCDamageArea widens it for the
        // collapsed borders of the cells that moved)
        assert_eq!((damage.StartCol(), damage.StartRow(), damage.ColCount(), damage.RowCount()), (1, 0, 1, 1));
    }

    #[test]
    fn insert_cells_with_a_colspan_makes_room_for_the_whole_span() {
        let t = build(&[&[(1, 1), (1, 1)]], &[(2, 1)]);
        let mut map = map_of(&t);
        let n = t.extra[0];
        let mut damage = TableArea::default();
        map.InsertCells(&mut vec![n], 0, 0, &mut damage);
        assert_eq!(map.GetColCount(), 4);
        assert!(data(&map, 0, 1).IsOrig());
        assert!(data(&map, 0, 2).IsColSpan());
        assert_eq!(data(&map, 0, 3).GetCellFrame(), Some(t.cells[0][1]));
        assert_eq!(t.cells[0][1].ColIndex(), 3);
        assert_eq!(map.GetEffectiveColSpan(0, 1), 2);
    }

    #[test]
    fn remove_cell_closes_the_gap_and_drops_the_empty_column() {
        let t = build(&[&[(1, 1), (1, 1), (1, 1)]], &[]);
        let mut map = map_of(&t);
        let mut damage = TableArea::default();
        map.RemoveCell(Some(t.cells[0][1]), 0, &mut damage);
        assert_eq!(map.GetColCount(), 2);
        assert_eq!(data(&map, 0, 0).GetCellFrame(), Some(t.cells[0][0]));
        assert_eq!(data(&map, 0, 1).GetCellFrame(), Some(t.cells[0][2]));
        assert_eq!(t.cells[0][2].ColIndex(), 1);
        assert_eq!(map.GetNumCellsOriginatingInCol(0), 1);
        assert_eq!(map.GetNumCellsOriginatingInCol(1), 1);
    }

    #[test]
    fn remove_cell_with_a_colspan_removes_all_its_columns() {
        let t = build(&[&[(2, 1), (1, 1)]], &[]);
        let mut map = map_of(&t);
        let mut damage = TableArea::default();
        assert_eq!(map.GetColCount(), 3);
        map.RemoveCell(Some(t.cells[0][0]), 0, &mut damage);
        assert_eq!(map.GetColCount(), 1);
        assert_eq!(data(&map, 0, 0).GetCellFrame(), Some(t.cells[0][1]));
        assert_eq!(t.cells[0][1].ColIndex(), 0);
        assert_eq!(map.mCols[0].mNumCellsSpan, 0);
    }

    #[test]
    fn remove_rows_without_spans_shifts_the_following_rows_up() {
        let t = build(&[&[(1, 1), (1, 1)], &[(1, 1), (1, 1)]], &[]);
        let mut map = map_of(&t);
        let mut damage = TableArea::default();
        map.RemoveRows(0, 1, false, &mut damage);
        assert_eq!(map.GetRowCount(), 1);
        assert_eq!(data(&map, 0, 0).GetCellFrame(), Some(t.cells[1][0]));
        assert_eq!(data(&map, 0, 1).GetCellFrame(), Some(t.cells[1][1]));
        assert_eq!(map.GetNumCellsOriginatingInCol(0), 1);
    }

    #[test]
    fn remove_rows_with_a_rowspan_into_them_rebuilds_the_map() {
        let t = build(&[&[(1, 2), (1, 1)], &[(1, 1)], &[(1, 1), (1, 1)]], &[]);
        let mut map = map_of(&t);
        let mut damage = TableArea::default();
        assert_eq!(map.GetRowCount(), 3);
        // removing row 1 shortens the rowspan of the cell of row 0 (it is rebuilt from the cells)
        map.RemoveRows(1, 1, true, &mut damage);
        // and the cell of the old row 2 (the new row 1) is pushed aside by it: the rowspan now covers
        // the old row 2, as the cell's rowspan attribute still says 2
        assert_eq!(map.GetRowCount(), 2);
        assert_eq!(map.GetColCount(), 3);
        assert_eq!(data(&map, 0, 0).GetCellFrame(), Some(t.cells[0][0]));
        assert!(data(&map, 1, 0).IsRowSpan());
        assert_eq!(data(&map, 1, 1).GetCellFrame(), Some(t.cells[2][0]));
        assert_eq!(data(&map, 1, 2).GetCellFrame(), Some(t.cells[2][1]));
    }

    #[test]
    fn grid_index_and_coordinates_round_trip() {
        let t = build(&[&[(1, 1), (1, 1)], &[(1, 1), (1, 1)]], &[]);
        let map = map_of(&t);
        assert_eq!(map.GetIndexByRowAndColumn(0, 0), 0);
        assert_eq!(map.GetIndexByRowAndColumn(0, 1), 1);
        assert_eq!(map.GetIndexByRowAndColumn(1, 0), 2);
        assert_eq!(map.GetIndexByRowAndColumn(1, 1), 3);
        assert_eq!(map.GetIndexByRowAndColumn(5, 0), -1);
        for index in 0..4 {
            let (mut row, mut col) = (0, 0);
            map.GetRowAndColumnByIndex(index, &mut row, &mut col);
            assert_eq!((row, col), (index / 2, index % 2));
        }
    }

    #[test]
    fn column_iterator_skips_cells_hidden_by_rowspans() {
        // column 0: a cell with rowspan 2 in row 0, then a cell in row 2
        let t = build(&[&[(1, 2), (1, 1)], &[(1, 1)], &[(1, 1), (1, 1)]], &[]);
        let map = map_of(&t);
        let mut it = nsCellMapColumnIterator::new(&map, 0);
        let (mut row, mut colSpan) = (-1, -1);
        assert_eq!(it.GetNextFrame(&mut row, &mut colSpan), Some(t.cells[0][0]));
        assert_eq!((row, colSpan), (0, 1));
        assert_eq!(it.GetNextFrame(&mut row, &mut colSpan), Some(t.cells[2][0]));
        assert_eq!((row, colSpan), (2, 1));
        assert_eq!(it.GetNextFrame(&mut row, &mut colSpan), None);
        // column 1: the cells of rows 0, 1 and 2
        let mut it = nsCellMapColumnIterator::new(&map, 1);
        let mut rows = Vec::new();
        while let Some(_) = it.GetNextFrame(&mut row, &mut colSpan) {
            rows.push(row);
        }
        assert_eq!(rows, vec![0, 1, 2]);
    }

    #[test]
    fn border_collapse_map_keeps_bc_data_with_each_entry() {
        let t = build(&[&[(1, 1), (1, 1)]], &[]);
        // (as in the frame constructor the table has no child frames yet when its cell map is made:
        // the constructor makes the maps of the row groups it finds before it creates the BC info, and
        // those would not be border collapse maps)
        let children = std::mem::take(&mut *t.table.mFrames());
        let mut map = nsTableCellMap::new(t.table, true);
        *t.table.mFrames() = children;
        let mut prior = None;
        map.InsertGroupCellMap__nsTableRowGroupFrame_nsTableRowGroupFrame(t.rg, &mut prior);
        let mut damage = TableArea::default();
        let mut rows = t.rows.clone();
        map.InsertRows(t.rg, &mut rows, 0, false, &mut damage);
        assert_eq!(map.GetColCount(), 2);
        assert!(data(&map, 0, 1).IsOrig());
        // BCCellData sits behind the same pointer; its BCData starts as a cell-owned segment start
        let bc = map.GetDataAt(0, 1) as *mut BCCellData;
        let (mut owner, mut start) = (eTableOwner, false);
        assert_eq!(unsafe { &*bc }.mData.GetIStartEdge(&mut owner, &mut start), 0);
        assert_eq!((owner, start), (eCellOwner, true));
        // the bEnd-most border array grew with the columns
        assert_eq!(map.mBCInfo.as_ref().unwrap().mBEndBorders.len(), 2);
        // a border written through the map lands in the entry
        let cellMap = map.mFirstMap;
        map.SetBCBorderEdge(LogicalSide::BStart, unsafe { &mut *cellMap }, 0, 0, 0, 2, eRowOwner, 3 * 60, true);
        assert_eq!(unsafe { &*bc }.mData.GetBStartEdge(&mut owner, &mut start), 3 * 60);
        assert_eq!((owner, start), (eRowOwner, false));
    }
}
