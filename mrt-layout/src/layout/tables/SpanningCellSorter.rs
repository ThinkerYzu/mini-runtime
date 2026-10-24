// Translated from layout/tables/SpanningCellSorter.h and SpanningCellSorter.cpp (GECKO_REV).
/*
 * Code to sort cells by their colspan, used by BasicTableLayoutStrategy.
 */
//
// Rust shape: the cells (`Item`s) are allocated from Gecko's StackArena (`AutoStackArena::Allocate`);
// here each one is a `Box` owned by `mItems`, so its address stays the same while the sorter
// lives, and the `next` links are raw pointers into those boxes. `nsTArray<HashTableEntry*>
// mSortedHashTable` (entries of the hash table, sorted by key) is the sorted list of the keys; the
// entry is found again through `mHashTable`.
use crate::*;
use std::collections::HashMap;

// Gecko: layout/tables/SpanningCellSorter.h SpanningCellSorter::Item
pub struct SpanningCellSorter_Item {
    pub row: i32,
    pub col: i32,
    pub next: *mut SpanningCellSorter_Item,
}

// Gecko: layout/tables/SpanningCellSorter.h SpanningCellSorter::State
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SpanningCellSorter_State {
    ADDING,
    ENUMERATING_ARRAY,
    ENUMERATING_HASH,
    DONE,
}

// store small colspans in an array for fast sorting and
// enumeration, and large colspans in a hash table
// Gecko: layout/tables/SpanningCellSorter.h SpanningCellSorter::ARRAY_BASE, ARRAY_SIZE
const ARRAY_BASE: i32 = 2;
const ARRAY_SIZE: usize = 8;

/**
 * The SpanningCellSorter is responsible for accumulating lists of cells
 * with colspans so that those cells can later be enumerated, sorted
 * from lowest number of columns spanned to highest.  It does not use a
 * stable sort (in fact, it currently reverses).
 */
// Gecko: layout/tables/SpanningCellSorter.h SpanningCellSorter
pub struct SpanningCellSorter {
    mState: SpanningCellSorter_State,
    mArray: [*mut SpanningCellSorter_Item; ARRAY_SIZE],
    // Map from colSpan -> items
    mHashTable: HashMap<i32, *mut SpanningCellSorter_Item>,
    /* state used only during enumeration */
    mEnumerationIndex: u32, // into mArray or mSortedHashTable
    mSortedHashTable: Vec<i32>,
    // ADAPTER: the arena of the items (see the top of the file).
    mItems: Vec<Box<SpanningCellSorter_Item>>,
}

impl SpanningCellSorter {
    // Gecko: layout/tables/SpanningCellSorter.cpp SpanningCellSorter::SpanningCellSorter
    pub fn new() -> SpanningCellSorter {
        SpanningCellSorter {
            mState: SpanningCellSorter_State::ADDING,
            mArray: [std::ptr::null_mut(); ARRAY_SIZE],
            mHashTable: HashMap::new(),
            mEnumerationIndex: 0,
            mSortedHashTable: Vec::new(),
            mItems: Vec::new(),
        }
    }

    // Gecko: layout/tables/SpanningCellSorter.h SpanningCellSorter::SpanToIndex
    fn SpanToIndex(aSpan: i32) -> i32 {
        aSpan - ARRAY_BASE
    }
    // Gecko: layout/tables/SpanningCellSorter.h SpanningCellSorter::IndexToSpan
    fn IndexToSpan(aIndex: i32) -> i32 {
        aIndex + ARRAY_BASE
    }
    // Gecko: layout/tables/SpanningCellSorter.h SpanningCellSorter::UseArrayForSpan
    fn UseArrayForSpan(aSpan: i32) -> bool {
        NS_ASSERTION!(SpanningCellSorter::SpanToIndex(aSpan) >= 0, "cell without colspan");
        SpanningCellSorter::SpanToIndex(aSpan) < ARRAY_SIZE as i32
    }

    /**
     * Add a cell to the sorter.  Returns false on out of memory.
     * aColSpan is the number of columns spanned, and aRow/aCol are the
     * position of the cell in the table (for GetCellInfoAt).
     */
    // Gecko: layout/tables/SpanningCellSorter.cpp SpanningCellSorter::AddCell
    pub fn AddCell(&mut self, aColSpan: i32, aRow: i32, aCol: i32) -> bool {
        NS_ASSERTION!(self.mState == SpanningCellSorter_State::ADDING, "cannot call AddCell after GetNext");
        NS_ASSERTION!(aColSpan >= ARRAY_BASE, "cannot add cells with colspan<2");

        // Item* i = (Item*)mozilla::AutoStackArena::Allocate(sizeof(Item));
        // NS_ENSURE_TRUE(i != nullptr, false);
        self.mItems.push(Box::new(SpanningCellSorter_Item { row: aRow, col: aCol, next: std::ptr::null_mut() }));
        let i: *mut SpanningCellSorter_Item = &mut **self.mItems.last_mut().unwrap();

        if SpanningCellSorter::UseArrayForSpan(aColSpan) {
            let index = SpanningCellSorter::SpanToIndex(aColSpan) as usize;
            unsafe { (*i).next = self.mArray[index] };
            self.mArray[index] = i;
        } else {
            // Item*& entryItems = mHashTable.LookupOrInsert(aColSpan, nullptr);
            let entryItems = self.mHashTable.entry(aColSpan).or_insert(std::ptr::null_mut());

            unsafe { (*i).next = *entryItems };
            *entryItems = i;
        }

        true
    }

    /**
     * Get the next *list* of cells.  Each list contains all the cells
     * for a colspan value, and the lists are given in order from lowest
     * to highest colspan.  The colspan value is filled in to *aColSpan.
     */
    // Gecko: layout/tables/SpanningCellSorter.cpp SpanningCellSorter::GetNext
    // (returns null when done)
    pub fn GetNext(&mut self, aColSpan: &mut i32) -> *mut SpanningCellSorter_Item {
        NS_ASSERTION!(self.mState != SpanningCellSorter_State::DONE, "done enumerating, stop calling");

        // Our comparator needs the SpanningCellSorter private HashTableEntry
        // (class HashTableEntryComparator: NOT PORTED, the keys are sorted with Vec::sort below.)

        // The C++ switch falls through from one state to the next; the three `if`s below do the same.
        if self.mState == SpanningCellSorter_State::ADDING {
            /* prepare to enumerate the array */
            self.mState = SpanningCellSorter_State::ENUMERATING_ARRAY;
            self.mEnumerationIndex = 0;
        }
        if self.mState == SpanningCellSorter_State::ENUMERATING_ARRAY {
            while (self.mEnumerationIndex as usize) < ARRAY_SIZE && self.mArray[self.mEnumerationIndex as usize].is_null() {
                self.mEnumerationIndex += 1;
            }
            if (self.mEnumerationIndex as usize) < ARRAY_SIZE {
                let result = self.mArray[self.mEnumerationIndex as usize];
                *aColSpan = SpanningCellSorter::IndexToSpan(self.mEnumerationIndex as i32);
                NS_ASSERTION!(!result.is_null(), "logic error");
                self.mEnumerationIndex += 1;
                return result;
            }
            /* prepare to enumerate the hash */
            self.mState = SpanningCellSorter_State::ENUMERATING_HASH;
            self.mEnumerationIndex = 0;
            if !self.mHashTable.is_empty() {
                // This clear is a no-op if the array is empty and it makes us
                // resilient against re-entrance.
                self.mSortedHashTable.clear();
                self.mSortedHashTable.reserve(self.mHashTable.len());
                for (key, _entry) in self.mHashTable.iter() {
                    self.mSortedHashTable.push(*key);
                }
                self.mSortedHashTable.sort();
            }
        }
        if self.mState == SpanningCellSorter_State::ENUMERATING_HASH {
            if (self.mEnumerationIndex as usize) < self.mSortedHashTable.len() {
                let key = self.mSortedHashTable[self.mEnumerationIndex as usize];
                let result = self.mHashTable[&key];
                *aColSpan = key;
                NS_ASSERTION!(!result.is_null(), "holes in hash table");
                self.mEnumerationIndex += 1;
                return result;
            }
            self.mState = SpanningCellSorter_State::DONE;
        }
        // case DONE:;
        std::ptr::null_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cells of one list, in the order GetNext gives them, as (row, col).
    fn list(mut aItem: *mut SpanningCellSorter_Item) -> Vec<(i32, i32)> {
        let mut result = Vec::new();
        while !aItem.is_null() {
            unsafe {
                result.push(((*aItem).row, (*aItem).col));
                aItem = (*aItem).next;
            }
        }
        result
    }

    // Written from the class comment: lists come out from the lowest colspan to the highest.
    #[test]
    fn lists_come_out_in_increasing_colspan_order() {
        let mut sorter = SpanningCellSorter::new();
        assert!(sorter.AddCell(5, 0, 0));
        assert!(sorter.AddCell(2, 1, 0));
        assert!(sorter.AddCell(3, 2, 0));
        let mut span = 0;
        let mut spans = Vec::new();
        loop {
            let item = sorter.GetNext(&mut span);
            if item.is_null() {
                break;
            }
            spans.push(span);
        }
        assert_eq!(spans, vec![2, 3, 5]);
    }

    // "It does not use a stable sort (in fact, it currently reverses)": the last cell added is first.
    #[test]
    fn cells_of_one_colspan_are_in_reverse_order_of_adding() {
        let mut sorter = SpanningCellSorter::new();
        sorter.AddCell(2, 0, 0);
        sorter.AddCell(2, 0, 3);
        sorter.AddCell(2, 1, 1);
        let mut span = 0;
        let item = sorter.GetNext(&mut span);
        assert_eq!(span, 2);
        assert_eq!(list(item), vec![(1, 1), (0, 3), (0, 0)]);
        assert!(sorter.GetNext(&mut span).is_null());
    }

    // Colspans 2..9 are kept in the array (ARRAY_SIZE is 8), larger ones in the hash table; both
    // are enumerated in one increasing sequence.
    #[test]
    fn large_colspans_come_after_array_colspans_in_order() {
        let mut sorter = SpanningCellSorter::new();
        sorter.AddCell(30, 0, 0);
        sorter.AddCell(10, 0, 1);
        sorter.AddCell(9, 0, 2);
        sorter.AddCell(2, 0, 3);
        sorter.AddCell(12, 0, 4);
        sorter.AddCell(10, 1, 5);
        let mut span = 0;
        let mut seen: Vec<(i32, Vec<(i32, i32)>)> = Vec::new();
        loop {
            let item = sorter.GetNext(&mut span);
            if item.is_null() {
                break;
            }
            seen.push((span, list(item)));
        }
        assert_eq!(
            seen,
            vec![(2, vec![(0, 3)]), (9, vec![(0, 2)]), (10, vec![(1, 5), (0, 1)]), (12, vec![(0, 4)]), (30, vec![(0, 0)])]
        );
    }

    #[test]
    fn an_empty_sorter_has_no_lists() {
        let mut sorter = SpanningCellSorter::new();
        let mut span = 0;
        assert!(sorter.GetNext(&mut span).is_null());
    }
}
