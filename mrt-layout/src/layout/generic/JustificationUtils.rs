// Translated from layout/generic/JustificationUtils.h (GECKO_REV). Header only; no .cpp.
use crate::*;

/*
 * Jutification Algorithm
 *
 * The justification algorithm is based on expansion opportunities
 * between justifiable clusters.  By this algorithm, there is one
 * expansion opportunity at each side of a justifiable cluster, and
 * at most one opportunity between two clusters. For example, if there
 * is a line in a Chinese document is: "你好世界hello world", then
 * the expansion opportunities (marked as '*') would be:
 *
 *                    你*好*世*界*hello*' '*world
 *
 * The spacing left in a line will then be distributed equally to each
 * opportunities. Because we want that, only justifiable clusters get
 * expanded, and the split point between two justifiable clusters would
 * be at the middle of the spacing, each expansion opportunities will be
 * filled by two justification gaps. The example above would be:
 *
 *              你 | 好 | 世 | 界  |hello|  ' '  |world
 *
 * In the algorithm, information about expansion opportunities is stored
 * in structure JustificationInfo, and the assignment of justification
 * gaps is in structure JustificationAssignment.
 */

// Gecko: layout/generic/JustificationUtils.h mozilla::JustificationInfo
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JustificationInfo {
    // Number of expansion opportunities inside a span. It doesn't include
    // any opportunities between this span and the one before or after.
    pub mInnerOpportunities: i32,
    // The justifiability of the start and end sides of the span.
    pub mIsStartJustifiable: bool,
    pub mIsEndJustifiable: bool,
}

impl JustificationInfo {
    // Gecko: layout/generic/JustificationUtils.h JustificationInfo::JustificationInfo
    pub const fn new() -> JustificationInfo {
        JustificationInfo { mInnerOpportunities: 0, mIsStartJustifiable: false, mIsEndJustifiable: false }
    }

    // Claim that the last opportunity should be cancelled
    // because the trailing space just gets trimmed.
    // Gecko: layout/generic/JustificationUtils.h JustificationInfo::CancelOpportunityForTrimmedSpace
    pub fn CancelOpportunityForTrimmedSpace(&mut self) {
        if self.mInnerOpportunities > 0 {
            self.mInnerOpportunities -= 1;
        } else {
            // There is no inner opportunities, hence the whole frame must
            // contain only the trimmed space, because any content before
            // space would cause an inner opportunity. The space made each
            // side justifiable, which should be cancelled now.
            self.mIsStartJustifiable = false;
            self.mIsEndJustifiable = false;
        }
    }
}

// Gecko: layout/generic/JustificationUtils.h mozilla::JustificationAssignment
// (the C++ members are 2-bit bit fields; plain u8 here, and callers only store 0, 1 or 2)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JustificationAssignment {
    // There are at most 2 gaps per end, so it is enough to use 2 bits.
    pub mGapsAtStart: u8,
    pub mGapsAtEnd: u8,
}

impl JustificationAssignment {
    // Gecko: layout/generic/JustificationUtils.h JustificationAssignment::JustificationAssignment
    pub const fn new() -> JustificationAssignment {
        JustificationAssignment { mGapsAtStart: 0, mGapsAtEnd: 0 }
    }

    // Gecko: layout/generic/JustificationUtils.h JustificationAssignment::TotalGaps
    pub fn TotalGaps(&self) -> i32 {
        self.mGapsAtStart as i32 + self.mGapsAtEnd as i32
    }
}

// Gecko: layout/generic/JustificationUtils.h mozilla::JustificationApplicationState::(anonymous struct mGaps)
#[derive(Clone, Copy, Debug, Default)]
pub struct JustificationApplicationState_Gaps {
    // The total number of justification gaps to be processed.
    pub mCount: i32,
    // The number of justification gaps which have been handled.
    pub mHandled: i32,
}

// Gecko: layout/generic/JustificationUtils.h mozilla::JustificationApplicationState::(anonymous struct mWidth)
#[derive(Clone, Copy, Debug, Default)]
pub struct JustificationApplicationState_Width {
    // The total spacing left in a line before justification.
    pub mAvailable: nscoord,
    // The spacing has been consumed by handled justification gaps.
    pub mConsumed: nscoord,
}

// Gecko: layout/generic/JustificationUtils.h mozilla::JustificationApplicationState
#[derive(Clone, Copy, Debug)]
pub struct JustificationApplicationState {
    pub mGaps: JustificationApplicationState_Gaps,
    pub mWidth: JustificationApplicationState_Width,
}

impl JustificationApplicationState {
    // Gecko: layout/generic/JustificationUtils.h JustificationApplicationState::JustificationApplicationState
    pub fn new(aGaps: i32, aWidth: nscoord) -> JustificationApplicationState {
        JustificationApplicationState {
            mGaps: JustificationApplicationState_Gaps { mCount: aGaps, mHandled: 0 },
            mWidth: JustificationApplicationState_Width { mAvailable: aWidth, mConsumed: 0 },
        }
    }

    // Gecko: layout/generic/JustificationUtils.h JustificationApplicationState::IsJustifiable
    pub fn IsJustifiable(&self) -> bool {
        self.mGaps.mCount > 0 && self.mWidth.mAvailable > 0
    }

    // Gecko: layout/generic/JustificationUtils.h JustificationApplicationState::Consume
    pub fn Consume(&mut self, aGaps: i32) -> nscoord {
        self.mGaps.mHandled += aGaps;
        let newAllocate: nscoord = (self.mWidth.mAvailable * self.mGaps.mHandled) / self.mGaps.mCount;
        let deltaWidth: nscoord = newAllocate - self.mWidth.mConsumed;
        self.mWidth.mConsumed = newAllocate;
        deltaWidth
    }
}

// Gecko: layout/generic/JustificationUtils.h mozilla::JustificationUtils
pub struct JustificationUtils;

impl JustificationUtils {
    // Compute justification gaps should be applied on a unit.
    // Gecko: layout/generic/JustificationUtils.h JustificationUtils::CountGaps
    pub fn CountGaps(aInfo: &JustificationInfo, aAssign: &JustificationAssignment) -> i32 {
        // Justification gaps include two gaps for each inner opportunities
        // and the gaps given assigned to the ends.
        aInfo.mInnerOpportunities * 2 + aAssign.TotalGaps()
    }
}

// Firefox has no gtest for this header; written from the documented behaviour (the comment above
// and the code of nsLineLayout::TextAlignLine, which is its user).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn CancelOpportunityForTrimmedSpace() {
        let mut info = JustificationInfo { mInnerOpportunities: 2, mIsStartJustifiable: true, mIsEndJustifiable: true };
        info.CancelOpportunityForTrimmedSpace();
        assert_eq!(info.mInnerOpportunities, 1);
        assert!(info.mIsStartJustifiable && info.mIsEndJustifiable);
        info.CancelOpportunityForTrimmedSpace();
        assert_eq!(info.mInnerOpportunities, 0);
        info.CancelOpportunityForTrimmedSpace();
        assert!(!info.mIsStartJustifiable && !info.mIsEndJustifiable);
    }

    #[test]
    fn CountGaps() {
        let info = JustificationInfo { mInnerOpportunities: 3, mIsStartJustifiable: false, mIsEndJustifiable: false };
        let assign = JustificationAssignment { mGapsAtStart: 1, mGapsAtEnd: 2 };
        assert_eq!(JustificationUtils::CountGaps(&info, &assign), 9);
    }

    #[test]
    fn ConsumeSpreadsWidthEvenly() {
        // 10 gaps share 100 app units, so every gap gets 10; consuming 3 then 7 gaps hands out all of it.
        let mut state = JustificationApplicationState::new(10, 100);
        assert!(state.IsJustifiable());
        assert_eq!(state.Consume(3), 30);
        assert_eq!(state.Consume(7), 70);
        assert_eq!(state.mGaps.mHandled, state.mGaps.mCount);
        assert_eq!(state.mWidth.mConsumed, state.mWidth.mAvailable);
    }

    #[test]
    fn ConsumeRoundsWithoutLosingWidth() {
        // 3 gaps share 10 units: the running total is rounded down each time, the last call catches up.
        let mut state = JustificationApplicationState::new(3, 10);
        let a = state.Consume(1);
        let b = state.Consume(1);
        let c = state.Consume(1);
        assert_eq!((a, b, c), (3, 3, 4));
    }

    #[test]
    fn NotJustifiable() {
        assert!(!JustificationApplicationState::new(0, 100).IsJustifiable());
        assert!(!JustificationApplicationState::new(4, 0).IsJustifiable());
    }
}
