// Translated from layout/tables/celldata.h (GECKO_REV).
//
// CellData is what nsCellMap stores for every (row, column) of a table; BCData and BCCellData are the
// border-collapse variants.
//
// Rust shape (see CONVENTIONS.md):
//  - The C++ `union { nsTableCellFrame* mOrigCell; uintptr_t mBits; }` is one `mBits: usize`. A cell
//    frame is a handle (a frame index, `nsIFrame(u32)`), so the "pointer" is stored as
//    `(index + 1) << 1`: never 0 (0 is "dead"), and bit 0 (SPAN) clear, exactly the two properties
//    the C++ relies on. `mOrigCell()` decodes it. Everything else (the span offsets and flags) is
//    the C++ bit layout, unchanged.
//  - `BCCellData : public CellData` is a `#[repr(C)]` struct whose first field is the CellData, so a
//    `*mut CellData` made from a `Box<BCCellData>` can be cast back (nsCellMap::DestroyCellData and the
//    `(BCCellData*)` casts do that), and it Derefs to CellData.
//  - The bit fields of BCData are plain u8 fields (the packing is a memory saving only).
//  - The classes' ctors are `pub(crate)` (C++: private, `friend class nsCellMap`).
//
// NOT PORTED: refcount logging (MOZ_COUNT_CTOR / MOZ_COUNT_DTOR), see nsCellMap.rs.
use crate::*;
use std::ops::{Deref, DerefMut};

// Gecko: layout/tables/nsITableCellLayout.h MAX_ROWSPAN and MAX_COLSPAN (celldata.h includes that header
// "for MAX_COLSPAN / MAX_ROWSPAN")
// the cellmap can not handle more.
pub const MAX_ROWSPAN: i32 = 65534;
// limit as IE and opera do.  If this ever changes,
// change COL_SPAN_OFFSET/COL_SPAN_SHIFT accordingly.
pub const MAX_COLSPAN: i32 = 1000;

/**
 * Data stored by nsCellMap to rationalize rowspan and colspan cells.
 */
// Gecko: layout/tables/celldata.h CellData
pub struct CellData {
    // this union relies on the assumption that an object (not primitive type)
    // does not start on an odd bit boundary. If mSpan is 0 then mOrigCell is in
    // effect and the data does not represent a span. If mSpan is 1, then mBits is
    // in effect and the data represents a span. mBits must match the size of
    // mOrigCell on both 32- and 64-bit platforms.
    // ADAPTER: see the top of the file for the encoding of mOrigCell in mBits.
    pub mBits: usize,
}

// Border Collapsing Cell Data
// Gecko: layout/tables/celldata.h BCBorderOwner
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum BCBorderOwner {
    eTableOwner = 0,
    eColGroupOwner = 1,
    eAjaColGroupOwner = 2, // col group to the left
    eColOwner = 3,
    eAjaColOwner = 4, // col to the left
    eRowGroupOwner = 5,
    eAjaRowGroupOwner = 6, // row group above
    eRowOwner = 7,
    eAjaRowOwner = 8, // row above
    eCellOwner = 9,
    eAjaCellOwner = 10, // cell to the top or to the left
}
pub use BCBorderOwner::*;

impl BCBorderOwner {
    // ADAPTER: C++ `(BCBorderOwner)mIStartOwner` (a 4-bit field cast back to the enum).
    fn from_bits(aBits: u8) -> BCBorderOwner {
        match aBits {
            0 => eTableOwner,
            1 => eColGroupOwner,
            2 => eAjaColGroupOwner,
            3 => eColOwner,
            4 => eAjaColOwner,
            5 => eRowGroupOwner,
            6 => eAjaRowGroupOwner,
            7 => eRowOwner,
            8 => eAjaRowOwner,
            9 => eCellOwner,
            _ => eAjaCellOwner,
        }
    }
}

// These are the max sizes that are stored. If they are exceeded, then the max
// is stored and the actual value is computed when needed.
// Gecko: layout/tables/celldata.h MAX_BORDER_WIDTH
pub const MAX_BORDER_WIDTH: nscoord = ((1u32 << (std::mem::size_of::<u16>() * 8)) - 1) as nscoord;

// The half of border on inline/block-axis start side
// Gecko: layout/tables/celldata.h BC_BORDER_START_HALF
pub fn BC_BORDER_START_HALF(aCoord: nscoord) -> nscoord {
    aCoord - aCoord / 2
}
// The half of border on inline/block-axis end side
// Gecko: layout/tables/celldata.h BC_BORDER_END_HALF
pub fn BC_BORDER_END_HALF(aCoord: nscoord) -> nscoord {
    aCoord / 2
}

// BCData stores the bstart and istart border info and the corner connecting the
// two.
// Gecko: layout/tables/celldata.h BCData
#[derive(Clone, Copy, Debug)]
pub struct BCData {
    // protected:
    mIStartSize: nscoord,    // size of iStart border
    mBStartSize: nscoord,    // size of bStart border
    mCornerSubSize: nscoord, // size of the largest border not in the
    //   dominant plane (for example, if corner is
    //   owned by the segment to its bStart or bEnd,
    //   then the size is the max of the border
    //   sizes of the segments to its iStart or iEnd.
    mIStartOwner: u8, // owner of iStart border (4 bits in C++)
    mBStartOwner: u8, // owner of bStart border (4 bits)
    mIStartStart: u8, // set if this is the start of a block-dir border segment (1 bit)
    mBStartStart: u8, // set if this is the start of an inline-dir border segment (1 bit)
    mCornerSide: u8,  // LogicalSide of the owner of the bStart-iStart corner relative to the corner (2 bits)
    mCornerBevel: u8, // is the corner beveled (only two segments, perpendicular, not dashed or dotted). (1 bit)
}

// BCCellData entries replace CellData entries in the cell map if the border
// collapsing model is in effect. BCData for a row and col entry contains the
// left and top borders of cell at that row and col and the corner connecting
// the two. The right borders of the cells in the last col and the bottom
// borders of the last row are stored in separate BCData entries in the cell
// map.
// Gecko: layout/tables/celldata.h BCCellData
#[repr(C)]
pub struct BCCellData {
    // class BCCellData : public CellData
    pub mBase: CellData,
    pub mData: BCData,
}

impl Deref for BCCellData {
    type Target = CellData;
    fn deref(&self) -> &CellData {
        &self.mBase
    }
}

impl DerefMut for BCCellData {
    fn deref_mut(&mut self) -> &mut CellData {
        &mut self.mBase
    }
}

// The layout of a celldata is as follows.  The top 10 bits are the colspan
// offset (which is enough to represent our allowed values 1-1000 for colspan).
// Then there are two bits of flags.
// XXXmats Then one unused bit that we should decide how to use in bug 862624.
// Then 16 bits of rowspan offset (which
// lets us represent numbers up to 65535.  Then another 3 bits of flags.

// num bits to shift right to get right aligned col span
pub const COL_SPAN_SHIFT: usize = 22;
// num bits to shift right to get right aligned row span
pub const ROW_SPAN_SHIFT: usize = 3;

// the col offset to the data containing the original cell.
pub const COL_SPAN_OFFSET: usize = 0x3FF << COL_SPAN_SHIFT;
// the row offset to the data containing the original cell
pub const ROW_SPAN_OFFSET: usize = 0xFFFF << ROW_SPAN_SHIFT;

// And the flags
pub const SPAN: usize = 0x00000001; // there a row or col span
pub const ROW_SPAN: usize = 0x00000002; // there is a row span
pub const ROW_SPAN_0: usize = 0x00000004; // the row span is 0
pub const COL_SPAN: usize = 1 << (COL_SPAN_SHIFT - 2); // there is a col span
pub const OVERLAP: usize = 1 << (COL_SPAN_SHIFT - 1); // there is a row span and
// col span but not by
// same cell

impl CellData {
    // Gecko: layout/tables/nsCellMap.cpp CellData::CellData
    // Implemented in nsCellMap.cpp
    //
    // @param aOrigCell  the table cell frame which will be stored in mOrigCell.
    pub(crate) fn new(aOrigCell: Option<nsTableCellFrame>) -> CellData {
        // NOT PORTED: MOZ_COUNT_CTOR(CellData) and the static_assert on the union's size.
        let mut data = CellData { mBits: 0 };
        data.Init(aOrigCell);
        data
    }

    // ADAPTER: the C++ reads the union member `mOrigCell`; see the top of the file for the encoding.
    pub fn mOrigCell(&self) -> Option<nsTableCellFrame> {
        if self.mBits == 0 {
            None
        } else {
            Some(nsTableCellFrame(nsIFrame(((self.mBits >> 1) - 1) as u32)))
        }
    }

    // Gecko: layout/tables/celldata.h CellData::GetCellFrame
    /** get the table cell frame for this entry
     * @return    a pointer to the cellframe, this will be nullptr when the entry
     *            is only a spanned entry
     */
    pub fn GetCellFrame(&self) -> Option<nsTableCellFrame> {
        if SPAN != (SPAN & self.mBits) {
            return self.mOrigCell();
        }
        None
    }

    // Gecko: layout/tables/celldata.h CellData::Init
    /** Initialize the mOrigCell pointer
     * @param aOrigCell  the table cell frame which will be stored in mOrigCell.
     */
    pub fn Init(&mut self, aCellFrame: Option<nsTableCellFrame>) {
        self.mBits = match aCellFrame {
            None => 0,
            Some(cell) => ((cell.0.0 as usize) + 1) << 1,
        };
    }

    // Gecko: layout/tables/celldata.h CellData::IsOrig
    /** does a cell originate from here
     * @return    is true if a cell corresponds to this cellmap entry
     */
    pub fn IsOrig(&self) -> bool {
        self.mOrigCell().is_some() && (SPAN != (SPAN & self.mBits))
    }

    // Gecko: layout/tables/celldata.h CellData::IsDead
    /** is the celldata valid
     * @return    is true if no cell originates and the cell is not spanned by
     *            a row- or colspan. mBits are 0 in this case and mOrigCell is
     *            nullptr
     */
    pub fn IsDead(&self) -> bool {
        0 == self.mBits
    }

    // Gecko: layout/tables/celldata.h CellData::IsSpan
    /** is the entry spanned by row- or a colspan
     * @return    is true if the entry is spanned by a row- or colspan
     */
    pub fn IsSpan(&self) -> bool {
        SPAN == (SPAN & self.mBits)
    }

    // Gecko: layout/tables/celldata.h CellData::IsRowSpan
    /** is the entry spanned by rowspan
     * @return    is true if the entry is spanned by a rowspan
     */
    pub fn IsRowSpan(&self) -> bool {
        (SPAN == (SPAN & self.mBits)) && (ROW_SPAN == (ROW_SPAN & self.mBits))
    }

    // Gecko: layout/tables/celldata.h CellData::IsZeroRowSpan
    /** is the entry spanned by a zero rowspan
     * zero rowspans span all cells starting from the originating cell down to
     * the end of the rowgroup or a cell originating in the same column
     * @return    is true if the entry is spanned by a zero rowspan
     */
    pub fn IsZeroRowSpan(&self) -> bool {
        (SPAN == (SPAN & self.mBits)) && (ROW_SPAN == (ROW_SPAN & self.mBits)) && (ROW_SPAN_0 == (ROW_SPAN_0 & self.mBits))
    }

    // Gecko: layout/tables/celldata.h CellData::SetZeroRowSpan
    /** mark the current entry as spanned by a zero rowspan
     * @param aIsZero    if true mark the entry as covered by a zero rowspan
     */
    pub fn SetZeroRowSpan(&mut self, aIsZeroSpan: bool) {
        if SPAN == (SPAN & self.mBits) {
            if aIsZeroSpan {
                self.mBits |= ROW_SPAN_0;
            } else {
                self.mBits &= !ROW_SPAN_0;
            }
        }
    }

    // Gecko: layout/tables/celldata.h CellData::GetRowSpanOffset
    /** get the distance from the current entry to the corresponding origin of the
     * rowspan
     * @return    containing the distance in the column to the originating cell
     */
    pub fn GetRowSpanOffset(&self) -> u32 {
        if (SPAN == (SPAN & self.mBits)) && (ROW_SPAN == (ROW_SPAN & self.mBits)) {
            return ((self.mBits & ROW_SPAN_OFFSET) >> ROW_SPAN_SHIFT) as u32;
        }
        0
    }

    // Gecko: layout/tables/celldata.h CellData::SetRowSpanOffset
    /** set the distance from the current entry to the corresponding origin of
     * the rowspan
     * @param    the distance in the column to the originating cell
     */
    pub fn SetRowSpanOffset(&mut self, mut aSpan: u32) {
        MOZ_ASSERT!(aSpan > 0, "a zero-sized span is nonsensical");
        MOZ_ASSERT!(aSpan <= MAX_ROWSPAN as u32, "span shouldn't exceed what we can handle");
        aSpan = aSpan.min(MAX_ROWSPAN as u32);

        self.mBits &= !ROW_SPAN_OFFSET;
        self.mBits |= (aSpan as usize) << ROW_SPAN_SHIFT;
        self.mBits |= SPAN;
        self.mBits |= ROW_SPAN;
    }

    // Gecko: layout/tables/celldata.h CellData::IsColSpan
    /** is the entry spanned by colspan
     * @return    is true if the entry is spanned by a colspan
     */
    pub fn IsColSpan(&self) -> bool {
        (SPAN == (SPAN & self.mBits)) && (COL_SPAN == (COL_SPAN & self.mBits))
    }

    // Gecko: layout/tables/celldata.h CellData::GetColSpanOffset
    /** get the distance from the current entry to the corresponding origin of
     *  the colspan
     * @return    containing the distance in the row to the originating cell
     */
    pub fn GetColSpanOffset(&self) -> u32 {
        if (SPAN == (SPAN & self.mBits)) && (COL_SPAN == (COL_SPAN & self.mBits)) {
            return ((self.mBits & COL_SPAN_OFFSET) >> COL_SPAN_SHIFT) as u32;
        }
        0
    }

    // Gecko: layout/tables/celldata.h CellData::SetColSpanOffset
    /** set the distance from the current entry to the corresponding origin of the
     * colspan
     * @param    the distance in the column to the originating cell
     */
    pub fn SetColSpanOffset(&mut self, mut aSpan: u32) {
        MOZ_ASSERT!(aSpan > 0, "a zero-sized span is nonsensical");
        MOZ_ASSERT!(aSpan <= MAX_COLSPAN as u32, "span shouldn't exceed what we can handle");
        aSpan = aSpan.min(MAX_COLSPAN as u32);

        self.mBits &= !COL_SPAN_OFFSET;
        self.mBits |= (aSpan as usize) << COL_SPAN_SHIFT;

        self.mBits |= SPAN;
        self.mBits |= COL_SPAN;
    }

    // Gecko: layout/tables/celldata.h CellData::IsOverlap
    /** is the entry spanned by a row- and a colspan
     * @return    is true if the entry is spanned by a row- and a colspan
     */
    pub fn IsOverlap(&self) -> bool {
        (SPAN == (SPAN & self.mBits)) && (OVERLAP == (OVERLAP & self.mBits))
    }

    // Gecko: layout/tables/celldata.h CellData::SetOverlap
    /** mark the current entry as spanned by a row- and a colspan
     * @param aOverlap    if true mark the entry as covered by a row- and
     *                    a colspan
     */
    pub fn SetOverlap(&mut self, aOverlap: bool) {
        if SPAN == (SPAN & self.mBits) {
            if aOverlap {
                self.mBits |= OVERLAP;
            } else {
                self.mBits &= !OVERLAP;
            }
        }
    }
}

impl BCCellData {
    // Gecko: layout/tables/nsCellMap.cpp BCCellData::BCCellData
    pub(crate) fn new(aOrigCell: Option<nsTableCellFrame>) -> BCCellData {
        // NOT PORTED: MOZ_COUNT_CTOR(BCCellData)
        BCCellData { mBase: CellData::new(aOrigCell), mData: BCData::new() }
    }
}

impl BCData {
    // Gecko: layout/tables/celldata.h BCData::BCData
    pub fn new() -> BCData {
        let mut data = BCData {
            mIStartSize: 0,
            mBStartSize: 0,
            mCornerSubSize: 0,
            mIStartOwner: eCellOwner as u8,
            mBStartOwner: eCellOwner as u8,
            mIStartStart: 0,
            mBStartStart: 0,
            mCornerSide: LogicalSide::BStart as u8,
            mCornerBevel: 0,
        };
        data.SetBStartStart(true);
        data.SetIStartStart(true);
        data
    }

    // Gecko: layout/tables/celldata.h BCData::GetIStartEdge
    pub fn GetIStartEdge(&self, aOwner: &mut BCBorderOwner, aStart: &mut bool) -> nscoord {
        *aOwner = BCBorderOwner::from_bits(self.mIStartOwner);
        *aStart = self.IsIStartStart();

        self.mIStartSize as nscoord
    }

    // Gecko: layout/tables/celldata.h BCData::SetIStartEdge
    pub fn SetIStartEdge(&mut self, aOwner: BCBorderOwner, aSize: nscoord, aStart: bool) {
        self.mIStartOwner = aOwner as u8;
        self.mIStartSize = if aSize > MAX_BORDER_WIDTH { MAX_BORDER_WIDTH } else { aSize };
        self.SetIStartStart(aStart);
    }

    // Gecko: layout/tables/celldata.h BCData::GetBStartEdge
    pub fn GetBStartEdge(&self, aOwner: &mut BCBorderOwner, aStart: &mut bool) -> nscoord {
        *aOwner = BCBorderOwner::from_bits(self.mBStartOwner);
        *aStart = self.IsBStartStart();

        self.mBStartSize as nscoord
    }

    // Gecko: layout/tables/celldata.h BCData::SetBStartEdge
    pub fn SetBStartEdge(&mut self, aOwner: BCBorderOwner, aSize: nscoord, aStart: bool) {
        self.mBStartOwner = aOwner as u8;
        self.mBStartSize = if aSize > MAX_BORDER_WIDTH { MAX_BORDER_WIDTH } else { aSize };
        self.SetBStartStart(aStart);
    }

    // Gecko: layout/tables/celldata.h BCData::GetCorner
    pub fn GetCorner(&self, aOwnerSide: &mut LogicalSide, aBevel: &mut bool) -> nscoord {
        *aOwnerSide = match self.mCornerSide {
            0 => LogicalSide::BStart,
            1 => LogicalSide::BEnd,
            2 => LogicalSide::IStart,
            _ => LogicalSide::IEnd,
        };
        *aBevel = self.mCornerBevel != 0;
        self.mCornerSubSize
    }

    // Gecko: layout/tables/celldata.h BCData::SetCorner
    pub fn SetCorner(&mut self, aSubSize: nscoord, aOwnerSide: LogicalSide, aBevel: bool) {
        self.mCornerSubSize = aSubSize;
        self.mCornerSide = aOwnerSide as u8;
        self.mCornerBevel = aBevel as u8;
    }

    // Gecko: layout/tables/celldata.h BCData::IsIStartStart
    pub fn IsIStartStart(&self) -> bool {
        self.mIStartStart != 0
    }

    // Gecko: layout/tables/celldata.h BCData::SetIStartStart
    pub fn SetIStartStart(&mut self, aValue: bool) {
        self.mIStartStart = aValue as u8;
    }

    // Gecko: layout/tables/celldata.h BCData::IsBStartStart
    pub fn IsBStartStart(&self) -> bool {
        self.mBStartStart != 0
    }

    // Gecko: layout/tables/celldata.h BCData::SetBStartStart
    pub fn SetBStartStart(&mut self, aValue: bool) {
        self.mBStartStart = aValue as u8;
    }
}

impl Default for BCData {
    fn default() -> BCData {
        BCData::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Written from the documented layout of CellData (no gtest exists for it in Gecko).
    fn cell(aIndex: u32) -> nsTableCellFrame {
        nsTableCellFrame(nsIFrame(aIndex))
    }

    #[test]
    fn a_new_entry_without_a_cell_is_dead() {
        let data = CellData::new(None);
        assert!(data.IsDead());
        assert!(!data.IsOrig() && !data.IsSpan() && !data.IsRowSpan() && !data.IsColSpan());
        assert!(data.GetCellFrame().is_none());
    }

    #[test]
    fn an_originating_entry_holds_its_cell_even_for_frame_zero() {
        for index in [0u32, 1, 77] {
            let data = CellData::new(Some(cell(index)));
            assert!(data.IsOrig());
            assert!(!data.IsDead());
            assert!(!data.IsSpan());
            assert_eq!(data.GetCellFrame(), Some(cell(index)));
        }
    }

    #[test]
    fn row_span_offset_makes_a_row_span_entry() {
        let mut data = CellData::new(None);
        data.SetRowSpanOffset(3);
        assert!(data.IsSpan() && data.IsRowSpan() && !data.IsColSpan());
        assert!(!data.IsOrig() && !data.IsDead());
        assert_eq!(data.GetRowSpanOffset(), 3);
        assert_eq!(data.GetColSpanOffset(), 0);
        assert!(data.GetCellFrame().is_none());
    }

    #[test]
    fn col_span_offset_and_row_span_offset_are_independent() {
        let mut data = CellData::new(None);
        data.SetRowSpanOffset(2);
        data.SetColSpanOffset(5);
        assert!(data.IsRowSpan() && data.IsColSpan());
        assert_eq!((data.GetRowSpanOffset(), data.GetColSpanOffset()), (2, 5));
        data.SetOverlap(true);
        assert!(data.IsOverlap());
        data.SetOverlap(false);
        assert!(!data.IsOverlap());
        data.SetZeroRowSpan(true);
        assert!(data.IsZeroRowSpan());
        data.SetZeroRowSpan(false);
        assert!(!data.IsZeroRowSpan());
        // the offsets are untouched by the flags
        assert_eq!((data.GetRowSpanOffset(), data.GetColSpanOffset()), (2, 5));
    }

    #[test]
    fn flags_do_nothing_on_a_non_span_entry() {
        let mut data = CellData::new(Some(cell(4)));
        data.SetOverlap(true);
        data.SetZeroRowSpan(true);
        assert!(data.IsOrig() && !data.IsOverlap() && !data.IsZeroRowSpan());
        assert_eq!(data.GetCellFrame(), Some(cell(4)));
    }

    #[test]
    fn the_largest_col_span_offset_fits() {
        let mut data = CellData::new(None);
        data.SetColSpanOffset(MAX_COLSPAN as u32);
        assert_eq!(data.GetColSpanOffset(), MAX_COLSPAN as u32);
        data.SetRowSpanOffset(MAX_ROWSPAN as u32);
        assert_eq!(data.GetRowSpanOffset(), MAX_ROWSPAN as u32);
        assert_eq!(data.GetColSpanOffset(), MAX_COLSPAN as u32);
    }

    #[test]
    fn bc_data_starts_as_cell_owned_segment_starts() {
        let d = BCData::new();
        let (mut owner, mut start) = (eTableOwner, false);
        assert_eq!(d.GetIStartEdge(&mut owner, &mut start), 0);
        assert_eq!((owner, start), (eCellOwner, true));
        assert_eq!(d.GetBStartEdge(&mut owner, &mut start), 0);
        assert_eq!((owner, start), (eCellOwner, true));
        let (mut side, mut bevel) = (LogicalSide::IEnd, true);
        assert_eq!(d.GetCorner(&mut side, &mut bevel), 0);
        assert_eq!((side, bevel), (LogicalSide::BStart, false));
    }

    #[test]
    fn bc_data_clamps_border_widths_and_halves_split_odd_sizes() {
        let mut d = BCData::new();
        d.SetIStartEdge(eRowOwner, MAX_BORDER_WIDTH + 100, false);
        let (mut owner, mut start) = (eTableOwner, true);
        assert_eq!(d.GetIStartEdge(&mut owner, &mut start), MAX_BORDER_WIDTH);
        assert_eq!((owner, start), (eRowOwner, false));
        assert_eq!(BC_BORDER_START_HALF(5) + BC_BORDER_END_HALF(5), 5);
        assert_eq!((BC_BORDER_START_HALF(5), BC_BORDER_END_HALF(5)), (3, 2));
    }

    #[test]
    fn bc_cell_data_derefs_to_cell_data() {
        let mut d = BCCellData::new(Some(cell(9)));
        assert!(d.IsOrig());
        d.mData.SetCorner(7, LogicalSide::IEnd, true);
        let (mut side, mut bevel) = (LogicalSide::BStart, false);
        assert_eq!(d.mData.GetCorner(&mut side, &mut bevel), 7);
        assert_eq!((side, bevel), (LogicalSide::IEnd, true));
    }
}
