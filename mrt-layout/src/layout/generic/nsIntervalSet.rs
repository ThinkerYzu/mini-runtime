// Gecko: layout/generic/nsIntervalSet.h nsIntervalSet
/* a set of ranges on a number-line */
use crate::*;

/*
 * A list-based class (hopefully tree-based when I get around to it)
 * for representing a set of ranges on a number-line.
 */
// Gecko: layout/generic/nsIntervalSet.h nsIntervalSet
pub struct nsIntervalSet {
    // NOT PORTED: mPresShell (arena allocation of the intervals); they are Boxes here.
    mList: Option<Box<Interval>>,
}

pub type coord_type = nscoord;

struct Interval {
    mBegin: coord_type,
    mEnd: coord_type,
    // NOT PORTED: mPrev (never set in the C++); mNext is the owning link.
    mNext: Option<Box<Interval>>,
}

impl Interval {
    fn new(aBegin: coord_type, aEnd: coord_type) -> Interval {
        Interval { mBegin: aBegin, mEnd: aEnd, mNext: None }
    }
}

impl nsIntervalSet {
    // NOT PORTED: the PresShell argument
    pub fn new() -> nsIntervalSet {
        nsIntervalSet { mList: None }
    }

    /*
     * Include the interval [aBegin, aEnd] in the set.
     *
     * Removal of intervals added is not supported because that would
     * require keeping track of the individual intervals that were
     * added (nsIntervalMap should do that).  It would be simple to
     * implement ExcludeInterval if anyone wants it, though.
     */
    // Gecko: layout/generic/nsIntervalSet.cpp nsIntervalSet::IncludeInterval
    pub fn IncludeInterval(&mut self, aBegin: coord_type, aEnd: coord_type) {
        let mut newInterval = Box::new(Interval::new(aBegin, aEnd));

        // Interval** current = &mList;
        let mut current: &mut Option<Box<Interval>> = &mut self.mList;
        while current.as_ref().is_some_and(|c| c.mEnd < aBegin) {
            current = &mut current.as_mut().unwrap().mNext;
        }

        newInterval.mNext = current.take();

        let mut subsumed = newInterval.mNext.take();
        while subsumed.as_ref().is_some_and(|s| s.mBegin <= aEnd) {
            let mut s = subsumed.unwrap();
            newInterval.mBegin = std::cmp::min(newInterval.mBegin, s.mBegin);
            newInterval.mEnd = std::cmp::max(newInterval.mEnd, s.mEnd);
            subsumed = s.mNext.take();
        }
        newInterval.mNext = subsumed;
        *current = Some(newInterval);
    }

    /*
     * Are _some_ points in [aBegin, aEnd] contained within the set
     * of intervals?
     */
    // Gecko: layout/generic/nsIntervalSet.cpp nsIntervalSet::Intersects
    pub fn Intersects(&self, aBegin: coord_type, aEnd: coord_type) -> bool {
        let mut current = self.mList.as_deref();
        while let Some(c) = current.filter(|c| c.mBegin <= aEnd) {
            if c.mEnd >= aBegin {
                return true;
            }
            current = c.mNext.as_deref();
        }
        false
    }

    /*
     * Are _all_ points in [aBegin, aEnd] contained within the set
     * of intervals?
     */
    // Gecko: layout/generic/nsIntervalSet.cpp nsIntervalSet::Contains
    pub fn Contains(&self, aBegin: coord_type, aEnd: coord_type) -> bool {
        let mut current = self.mList.as_deref();
        while let Some(c) = current.filter(|c| c.mBegin <= aBegin) {
            if c.mEnd >= aEnd {
                return true;
            }
            current = c.mNext.as_deref();
        }
        false
    }

    // Gecko: layout/generic/nsIntervalSet.h nsIntervalSet::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        self.mList.is_none()
    }
}

impl Default for nsIntervalSet {
    fn default() -> Self {
        Self::new()
    }
}
