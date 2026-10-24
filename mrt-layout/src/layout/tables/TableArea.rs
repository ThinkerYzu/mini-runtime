// Translated from layout/tables/TableArea.h (GECKO_REV).
use crate::*;

// Gecko: layout/tables/TableArea.h mozilla::TableArea
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableArea {
    // private:
    mStartCol: i32,
    mStartRow: i32,
    mColCount: i32,
    mRowCount: i32,
}

impl Default for TableArea {
    // Gecko: layout/tables/TableArea.h mozilla::TableArea::TableArea()
    fn default() -> TableArea {
        TableArea { mStartCol: 0, mStartRow: 0, mColCount: 0, mRowCount: 0 }
    }
}

impl TableArea {
    // Gecko: layout/tables/TableArea.h mozilla::TableArea::TableArea
    pub fn new(aStartCol: i32, aStartRow: i32, aColCount: i32, aRowCount: i32) -> TableArea {
        TableArea { mStartCol: aStartCol, mStartRow: aStartRow, mColCount: aColCount, mRowCount: aRowCount }
    }

    // The C++ has `int32_t& StartCol()` (a reference used for assignment) next to
    // `int32_t StartCol() const`; the reference one is `StartCol_mut` here (the `_mut` convention of
    // this crate), the value one keeps the plain name.
    // Gecko: layout/tables/TableArea.h mozilla::TableArea::StartCol
    pub fn StartCol_mut(&mut self) -> &mut i32 {
        &mut self.mStartCol
    }
    // Gecko: layout/tables/TableArea.h mozilla::TableArea::StartRow
    pub fn StartRow_mut(&mut self) -> &mut i32 {
        &mut self.mStartRow
    }
    // Gecko: layout/tables/TableArea.h mozilla::TableArea::ColCount
    pub fn ColCount_mut(&mut self) -> &mut i32 {
        &mut self.mColCount
    }
    // Gecko: layout/tables/TableArea.h mozilla::TableArea::RowCount
    pub fn RowCount_mut(&mut self) -> &mut i32 {
        &mut self.mRowCount
    }

    // Gecko: layout/tables/TableArea.h mozilla::TableArea::StartCol
    pub fn StartCol(&self) -> i32 {
        self.mStartCol
    }
    // Gecko: layout/tables/TableArea.h mozilla::TableArea::StartRow
    pub fn StartRow(&self) -> i32 {
        self.mStartRow
    }
    // Gecko: layout/tables/TableArea.h mozilla::TableArea::ColCount
    pub fn ColCount(&self) -> i32 {
        self.mColCount
    }
    // Gecko: layout/tables/TableArea.h mozilla::TableArea::RowCount
    pub fn RowCount(&self) -> i32 {
        self.mRowCount
    }
    // Gecko: layout/tables/TableArea.h mozilla::TableArea::EndCol
    pub fn EndCol(&self) -> i32 {
        self.mStartCol + self.mColCount
    }
    // Gecko: layout/tables/TableArea.h mozilla::TableArea::EndRow
    pub fn EndRow(&self) -> i32 {
        self.mStartRow + self.mRowCount
    }

    // Gecko: layout/tables/TableArea.h mozilla::TableArea::UnionArea
    // ADAPTER: the C++ unions two `nsIntRect`s (BaseRect::UnionRect). nsIntRect (gfx::IntRect) is not
    // ported (see nsRect.rs), so BaseRect::UnionRect's rule is written out on the four ints: if one
    // rectangle is empty (width or height <= 0) the result is the other, if both are, the second;
    // otherwise the bounding box.
    pub fn UnionArea(&mut self, aArea1: &TableArea, aArea2: &TableArea) {
        // nsIntRect rect(aArea1.mStartCol, aArea1.mStartRow, aArea1.mColCount, aArea1.mRowCount);
        let r1 = (aArea1.mStartCol, aArea1.mStartRow, aArea1.mColCount, aArea1.mRowCount);
        // rect.UnionRect(rect, nsIntRect(aArea2.mStartCol, aArea2.mStartRow, aArea2.mColCount, aArea2.mRowCount));
        let r2 = (aArea2.mStartCol, aArea2.mStartRow, aArea2.mColCount, aArea2.mRowCount);
        let isEmpty = |r: (i32, i32, i32, i32)| r.2 <= 0 || r.3 <= 0;
        let rect = if isEmpty(r1) {
            r2
        } else if isEmpty(r2) {
            r1
        } else {
            let x = r1.0.min(r2.0);
            let y = r1.1.min(r2.1);
            let xmost = (r1.0 + r1.2).max(r2.0 + r2.2);
            let ymost = (r1.1 + r1.3).max(r2.1 + r2.3);
            (x, y, xmost - x, ymost - y)
        };
        // rect.GetRect(&mStartCol, &mStartRow, &mColCount, &mRowCount);
        self.mStartCol = rect.0;
        self.mStartRow = rect.1;
        self.mColCount = rect.2;
        self.mRowCount = rect.3;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Written from the documented behaviour of TableArea (no gtest exists for it in Gecko).
    #[test]
    fn end_col_and_end_row_are_start_plus_count() {
        let a = TableArea::new(1, 2, 3, 4);
        assert_eq!((a.EndCol(), a.EndRow()), (4, 6));
    }

    #[test]
    fn union_is_the_bounding_box() {
        let mut u = TableArea::default();
        u.UnionArea(&TableArea::new(0, 0, 2, 2), &TableArea::new(3, 1, 2, 4));
        assert_eq!((u.StartCol(), u.StartRow(), u.ColCount(), u.RowCount()), (0, 0, 5, 5));
    }

    #[test]
    fn union_with_an_empty_area_is_the_other_one() {
        let mut u = TableArea::default();
        u.UnionArea(&TableArea::default(), &TableArea::new(2, 3, 1, 1));
        assert_eq!(u, TableArea::new(2, 3, 1, 1));
    }
}
