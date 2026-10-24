// Gecko: gfx/thebes/gfxSkipChars.h, gfxSkipChars.cpp
use crate::*;

/*
 * gfxSkipChars is a data structure representing a list of characters that
 * have been skipped. The initial string is called the "original string"
 * and after skipping some characters, the result is called the "skipped
 * string". gfxSkipChars provides efficient ways to translate between offsets in
 * the original string and the skipped string. It is used by textrun code to
 * keep track of offsets before and after text transformations such as
 * whitespace compression and control code deletion.
 */

// Gecko: gfx/thebes/gfxSkipChars.h gfxSkipChars::SkippedRange
#[derive(Clone, Copy, Debug)]
pub struct gfxSkipChars_SkippedRange {
    mOffset: u32, // original-string offset at which we want to skip
    mLength: u32, // number of skipped chars at this offset
    mDelta: u32,  // sum of lengths of preceding skipped-ranges
}

impl gfxSkipChars_SkippedRange {
    pub fn new(aOffset: u32, aLength: u32, aDelta: u32) -> Self {
        gfxSkipChars_SkippedRange { mOffset: aOffset, mLength: aLength, mDelta: aDelta }
    }

    pub fn Start(&self) -> u32 {
        self.mOffset
    }

    pub fn End(&self) -> u32 {
        self.mOffset + self.mLength
    }

    pub fn Length(&self) -> u32 {
        self.mLength
    }

    pub fn SkippedOffset(&self) -> u32 {
        self.mOffset - self.mDelta
    }

    pub fn Delta(&self) -> u32 {
        self.mDelta
    }

    pub fn NextDelta(&self) -> u32 {
        self.mDelta + self.mLength
    }

    pub fn Extend(&mut self, aChars: u32) {
        self.mLength += aChars;
    }
}

/**
 * The gfxSkipChars is represented as a sorted array of skipped ranges.
 *
 * A freshly-created gfxSkipChars means "all chars kept".
 */
// Gecko: gfx/thebes/gfxSkipChars.h gfxSkipChars
#[derive(Clone, Default)]
pub struct gfxSkipChars {
    // ADAPTER: an exact-size array, None when empty (Gecko: nsTArray, which keeps spare capacity);
    // use Ranges() to read it, as a slice.
    mRanges: Option<Box<[gfxSkipChars_SkippedRange]>>,
    mCharCount: u32,
}

impl gfxSkipChars {
    // Instrumentation (bench/textrun_profile.py): (ranges len, ranges capacity, size_of range).
    pub fn MemStats(&self) -> (usize, usize, usize) {
        (self.Ranges().len(), self.Ranges().len(), std::mem::size_of::<gfxSkipChars_SkippedRange>())
    }

    // ADAPTER: mRanges as a slice (empty when there is no array).
    pub(crate) fn Ranges(&self) -> &[gfxSkipChars_SkippedRange] {
        self.mRanges.as_deref().unwrap_or(&[])
    }

    // Gecko: gfxSkipChars::gfxSkipChars
    pub fn new() -> Self {
        gfxSkipChars { mRanges: None, mCharCount: 0 }
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipChars::SkipChars
    pub fn SkipChars(&mut self, aChars: u32) {
        NS_ASSERTION!(self.mCharCount + aChars > self.mCharCount, "Character count overflow");
        let rangeCount = self.Ranges().len();
        let mut delta = 0;
        if rangeCount > 0 {
            let lastRange = &mut self.mRanges.as_mut().unwrap()[rangeCount - 1];
            if lastRange.End() == self.mCharCount {
                lastRange.Extend(aChars);
                self.mCharCount += aChars;
                return;
            }
            delta = lastRange.NextDelta();
        }
        // ADAPTER: append by growing the array by exactly one range (no spare capacity).
        let mut ranges: Vec<gfxSkipChars_SkippedRange> = self.mRanges.take().map_or_else(Vec::new, |r| r.into_vec());
        ranges.reserve_exact(1);
        ranges.push(gfxSkipChars_SkippedRange::new(self.mCharCount, aChars, delta));
        self.mRanges = Some(ranges.into_boxed_slice());
        self.mCharCount += aChars;
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipChars::KeepChars
    pub fn KeepChars(&mut self, aChars: u32) {
        NS_ASSERTION!(self.mCharCount + aChars > self.mCharCount, "Character count overflow");
        self.mCharCount += aChars;
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipChars::SkipChar
    pub fn SkipChar(&mut self) {
        self.SkipChars(1);
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipChars::KeepChar
    pub fn KeepChar(&mut self) {
        self.KeepChars(1);
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipChars::TakeFrom
    pub fn TakeFrom(&mut self, aSkipChars: &mut gfxSkipChars) {
        self.mRanges = std::mem::take(&mut aSkipChars.mRanges);
        self.mCharCount = aSkipChars.mCharCount;
        aSkipChars.mCharCount = 0;
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipChars::GetOriginalCharCount
    pub fn GetOriginalCharCount(&self) -> i32 {
        self.mCharCount as i32
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipChars::LastRange
    pub fn LastRange(&self) -> &gfxSkipChars_SkippedRange {
        // this is only valid if mRanges is non-empty; no assertion here
        // because nsTArray will already assert if we abuse it
        &self.Ranges()[self.Ranges().len() - 1]
    }
}

/**
 * A gfxSkipCharsIterator represents a position in the original string. It lets
 * you map efficiently to and from positions in the string after skipped
 * characters have been removed. You can also specify an offset that is added to
 * all incoming original string offsets and subtracted from all outgoing
 * original string offsets --- useful when the gfxSkipChars corresponds to
 * something offset from the original DOM coordinates, which it often does for
 * gfxTextRuns.
 *
 * The current positions (in both the original and skipped strings) are
 * always constrained to be >= 0 and <= the string length. When the position
 * is equal to the string length, it is at the end of the string. The current
 * positions do not include any aOriginalStringToSkipCharsOffset.
 *
 * When the position in the original string corresponds to a skipped character,
 * the skipped-characters offset is the offset of the next unskipped character,
 * or the skipped-characters string length if there is no next unskipped
 * character.
 */
// Gecko: gfx/thebes/gfxSkipChars.h gfxSkipCharsIterator
#[derive(Clone, Copy)]
pub struct gfxSkipCharsIterator<'a> {
    mSkipChars: Option<&'a gfxSkipChars>,

    // Current position
    mOriginalStringOffset: i32,
    mSkippedStringOffset: u32,

    // Index of the last skippedRange that precedes or contains the current
    // position in the original string.
    // If index == -1 then we are before the first skipped char.
    mCurrentRangeIndex: i32,

    // This offset is added to map from "skipped+unskipped characters in
    // the original DOM string" character space to "skipped+unskipped
    // characters in the textrun's gfxSkipChars" character space
    mOriginalStringToSkipCharsOffset: i32,
}

impl<'a> gfxSkipCharsIterator<'a> {
    /**
     * @param aOriginalStringToSkipCharsOffset add this to all incoming and
     * outgoing original string offsets
     */
    // Gecko: gfxSkipCharsIterator::gfxSkipCharsIterator(const gfxSkipChars&, int32_t, int32_t)
    pub fn new(aSkipChars: &'a gfxSkipChars, aOriginalStringToSkipCharsOffset: i32, aOriginalStringOffset: i32) -> Self {
        let mut it = gfxSkipCharsIterator {
            mSkipChars: Some(aSkipChars),
            mOriginalStringOffset: 0,
            mSkippedStringOffset: 0,
            mCurrentRangeIndex: -1,
            mOriginalStringToSkipCharsOffset: aOriginalStringToSkipCharsOffset,
        };
        it.SetOriginalOffset(aOriginalStringOffset);
        it
    }

    // Gecko: gfxSkipCharsIterator::gfxSkipCharsIterator(const gfxSkipChars&, int32_t = 0)
    pub fn new__gfxSkipChars_int32_t(aSkipChars: &'a gfxSkipChars, aOriginalStringToSkipCharsOffset: i32) -> Self {
        let mut it = gfxSkipCharsIterator {
            mSkipChars: Some(aSkipChars),
            mOriginalStringOffset: 0,
            mSkippedStringOffset: 0,
            mCurrentRangeIndex: 0,
            mOriginalStringToSkipCharsOffset: aOriginalStringToSkipCharsOffset,
        };
        it.mCurrentRangeIndex = if aSkipChars.Ranges().is_empty() || aSkipChars.Ranges()[0].Start() > 0 { -1 } else { 0 };
        it
    }

    /**
     * The empty constructor creates an object that is useless until it is
     * assigned.
     */
    // Gecko: gfxSkipCharsIterator::gfxSkipCharsIterator()
    pub fn Default() -> Self {
        gfxSkipCharsIterator {
            mSkipChars: None,
            mOriginalStringOffset: 0,
            mSkippedStringOffset: 0,
            mCurrentRangeIndex: 0,
            mOriginalStringToSkipCharsOffset: 0,
        }
    }

    /**
     * Return true if this iterator is properly initialized and usable.
     */
    // Gecko: gfxSkipCharsIterator::IsInitialized
    pub fn IsInitialized(&self) -> bool {
        self.mSkipChars.is_some()
    }

    /**
     * Set the iterator to aOriginalStringOffset in the original string.
     * This can efficiently move forward or backward from the current position.
     * aOriginalStringOffset is clamped to [0,originalStringLength].
     */
    // Gecko: gfx/thebes/gfxSkipChars.cpp gfxSkipCharsIterator::SetOriginalOffset
    pub fn SetOriginalOffset(&mut self, aOffset: i32) {
        let mSkipChars = self.mSkipChars.unwrap();
        let mut aOffset = aOffset + self.mOriginalStringToSkipCharsOffset;
        if (aOffset as u32) > mSkipChars.mCharCount {
            // gfxCriticalError() << "invalid offset " ...
            NS_WARNING!(format!("invalid offset {} for gfxSkipChars length {}", aOffset, mSkipChars.mCharCount));
            aOffset = mSkipChars.mCharCount as i32;
        }

        self.mOriginalStringOffset = aOffset;

        let rangeCount = mSkipChars.Ranges().len() as u32;
        if rangeCount == 0 {
            self.mSkippedStringOffset = aOffset as u32;
            return;
        }

        // at start of string?
        if aOffset == 0 {
            self.mSkippedStringOffset = 0;
            self.mCurrentRangeIndex = if rangeCount != 0 && mSkipChars.Ranges()[0].Start() == 0 { 0 } else { -1 };
            return;
        }

        // find the range that includes or precedes aOffset
        let ranges = mSkipChars.Ranges();
        // mozilla::BinarySearchIf with SkippedRangeStartComparator (never returns 0):
        // idx is the first range whose Start() is greater than aOffset.
        let idx = ranges.partition_point(|aRange| !((aOffset as u32) < aRange.Start())) as u32;

        if idx == rangeCount {
            self.mCurrentRangeIndex = rangeCount as i32 - 1;
        } else if (aOffset as u32) < ranges[idx as usize].Start() {
            self.mCurrentRangeIndex = idx as i32 - 1;
            if self.mCurrentRangeIndex == -1 {
                self.mSkippedStringOffset = aOffset as u32;
                return;
            }
        } else {
            self.mCurrentRangeIndex = idx as i32;
        }

        let r = &ranges[self.mCurrentRangeIndex as usize];
        if (aOffset as u32) < r.End() {
            self.mSkippedStringOffset = r.SkippedOffset();
            return;
        }

        self.mSkippedStringOffset = aOffset as u32 - r.NextDelta();
    }

    /**
     * Set the iterator to aSkippedStringOffset in the skipped string.
     * This can efficiently move forward or backward from the current position.
     * aSkippedStringOffset is clamped to [0,skippedStringLength].
     */
    // Gecko: gfx/thebes/gfxSkipChars.cpp gfxSkipCharsIterator::SetSkippedOffset
    pub fn SetSkippedOffset(&mut self, aOffset: u32) {
        let mSkipChars = self.mSkipChars.unwrap();
        NS_ASSERTION!(
            (mSkipChars.Ranges().is_empty() && aOffset <= mSkipChars.mCharCount)
                || (!mSkipChars.Ranges().is_empty()
                    && aOffset <= mSkipChars.LastRange().SkippedOffset() + mSkipChars.mCharCount - mSkipChars.LastRange().End()),
            "Invalid skipped offset"
        );
        self.mSkippedStringOffset = aOffset;

        let rangeCount = mSkipChars.Ranges().len() as u32;
        if rangeCount == 0 {
            self.mOriginalStringOffset = aOffset as i32;
            return;
        }

        let ranges = mSkipChars.Ranges();
        // mozilla::BinarySearchIf with SkippedRangeOffsetComparator (never returns 0)
        let idx = ranges.partition_point(|aRange| !(aOffset < aRange.SkippedOffset())) as u32;

        if idx == rangeCount {
            self.mCurrentRangeIndex = rangeCount as i32 - 1;
        } else if aOffset < ranges[idx as usize].SkippedOffset() {
            self.mCurrentRangeIndex = idx as i32 - 1;
            if self.mCurrentRangeIndex == -1 {
                self.mOriginalStringOffset = aOffset as i32;
                return;
            }
        } else {
            self.mCurrentRangeIndex = idx as i32;
        }

        let r = &ranges[self.mCurrentRangeIndex as usize];
        self.mOriginalStringOffset = (r.End() + aOffset - r.SkippedOffset()) as i32;
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipCharsIterator::ConvertOriginalToSkipped
    pub fn ConvertOriginalToSkipped(&mut self, aOriginalStringOffset: i32) -> u32 {
        self.SetOriginalOffset(aOriginalStringOffset);
        self.GetSkippedOffset()
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipCharsIterator::ConvertSkippedToOriginal
    pub fn ConvertSkippedToOriginal(&mut self, aSkippedStringOffset: u32) -> i32 {
        self.SetSkippedOffset(aSkippedStringOffset);
        self.GetOriginalOffset()
    }

    /**
     * Test if the character at the current position in the original string
     * is skipped or not. If aRunLength is non-null, then *aRunLength is set
     * to a number of characters all of which are either skipped or not, starting
     * at this character. When the current position is at the end of the original
     * string, we return true and *aRunLength is set to zero.
     */
    // Gecko: gfx/thebes/gfxSkipChars.cpp gfxSkipCharsIterator::IsOriginalCharSkipped
    pub fn IsOriginalCharSkipped(&self, aRunLength: Option<&mut i32>) -> bool {
        let mSkipChars = self.mSkipChars.unwrap();
        if self.mCurrentRangeIndex == -1 {
            // we're before the first skipped range (if any)
            if let Some(aRunLength) = aRunLength {
                let end = if mSkipChars.Ranges().is_empty() { mSkipChars.mCharCount } else { mSkipChars.Ranges()[0].Start() };
                *aRunLength = (end as i32).wrapping_sub(self.mOriginalStringOffset);
            }
            return mSkipChars.mCharCount == self.mOriginalStringOffset as u32;
        }

        let range = &mSkipChars.Ranges()[self.mCurrentRangeIndex as usize];

        if (self.mOriginalStringOffset as u32) < range.End() {
            if let Some(aRunLength) = aRunLength {
                *aRunLength = (range.End() - self.mOriginalStringOffset as u32) as i32;
            }
            return true;
        }

        if let Some(aRunLength) = aRunLength {
            let end = if (self.mCurrentRangeIndex as u32) + 1 < mSkipChars.Ranges().len() as u32 {
                mSkipChars.Ranges()[(self.mCurrentRangeIndex + 1) as usize].Start()
            } else {
                mSkipChars.mCharCount
            };
            *aRunLength = (end - self.mOriginalStringOffset as u32) as i32;
        }

        mSkipChars.mCharCount == self.mOriginalStringOffset as u32
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipCharsIterator::AdvanceOriginal
    pub fn AdvanceOriginal(&mut self, aDelta: i32) {
        self.SetOriginalOffset(self.GetOriginalOffset() + aDelta);
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipCharsIterator::AdvanceSkipped
    pub fn AdvanceSkipped(&mut self, aDelta: i32) {
        self.SetSkippedOffset((self.GetSkippedOffset() as i32 + aDelta) as u32);
    }

    /**
     * @return the offset within the original string
     */
    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipCharsIterator::GetOriginalOffset
    pub fn GetOriginalOffset(&self) -> i32 {
        self.mOriginalStringOffset - self.mOriginalStringToSkipCharsOffset
    }

    /**
     * @return the offset within the skipped string corresponding to the
     * current position in the original string. If the current position
     * in the original string is a character that is skipped, then we return
     * the position corresponding to the first non-skipped character in the
     * original string after the current position, or the length of the skipped
     * string if there is no such character.
     */
    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipCharsIterator::GetSkippedOffset
    pub fn GetSkippedOffset(&self) -> u32 {
        self.mSkippedStringOffset
    }

    // Gecko: gfx/thebes/gfxSkipChars.h gfxSkipCharsIterator::GetOriginalEnd
    pub fn GetOriginalEnd(&self) -> i32 {
        self.mSkipChars.unwrap().GetOriginalCharCount() - self.mOriginalStringToSkipCharsOffset
    }
}

// Tests: Gecko has no gtest for gfxSkipChars; written from the comments in gfxSkipChars.h.
#[cfg(test)]
mod tests {
    use super::*;

    // "abcdefghij" with c, d (2..4) and h (7) skipped
    fn sample() -> gfxSkipChars {
        let mut s = gfxSkipChars::new();
        s.KeepChars(2);
        s.SkipChars(2);
        s.KeepChars(3);
        s.SkipChar();
        s.KeepChars(2);
        s
    }

    #[test]
    fn a_new_skip_chars_keeps_everything() {
        let mut s = gfxSkipChars::new();
        s.KeepChars(5);
        let mut it = gfxSkipCharsIterator::new__gfxSkipChars_int32_t(&s, 0);
        assert_eq!(s.GetOriginalCharCount(), 5);
        assert_eq!(it.ConvertOriginalToSkipped(3), 3);
        assert_eq!(it.ConvertSkippedToOriginal(4), 4);
        let mut len = 0;
        assert!(!it.IsOriginalCharSkipped(Some(&mut len)));
        assert_eq!(len, 1 + 0); // the whole rest of the string is one unskipped run
    }

    #[test]
    fn original_offsets_map_to_skipped_offsets() {
        let s = sample();
        let mut it = gfxSkipCharsIterator::new__gfxSkipChars_int32_t(&s, 0);
        // original: a b [c d] e f g [h] i j   (the brackets are skipped)
        // a skipped character maps to the skipped offset of the next kept one
        let expect = [(0, 0), (1, 1), (2, 2), (3, 2), (4, 2), (5, 3), (6, 4), (7, 5), (8, 5), (9, 6), (10, 7)];
        for (orig, skipped) in expect {
            assert_eq!(it.ConvertOriginalToSkipped(orig), skipped, "original {orig}");
        }
    }

    #[test]
    fn skipped_offsets_map_back_to_the_first_original_offset_that_is_kept() {
        let s = sample();
        let mut it = gfxSkipCharsIterator::new__gfxSkipChars_int32_t(&s, 0);
        let expect = [(0, 0), (1, 1), (2, 4), (3, 5), (4, 6), (5, 8), (6, 9), (7, 10)];
        for (skipped, orig) in expect {
            assert_eq!(it.ConvertSkippedToOriginal(skipped), orig, "skipped {skipped}");
        }
    }

    #[test]
    fn is_original_char_skipped_reports_runs() {
        let s = sample();
        let mut it = gfxSkipCharsIterator::new__gfxSkipChars_int32_t(&s, 0);
        let mut len = 0;
        it.SetOriginalOffset(0);
        assert!(!it.IsOriginalCharSkipped(Some(&mut len)));
        assert_eq!(len, 2);
        it.SetOriginalOffset(2);
        assert!(it.IsOriginalCharSkipped(Some(&mut len)));
        assert_eq!(len, 2);
        it.SetOriginalOffset(4);
        assert!(!it.IsOriginalCharSkipped(Some(&mut len)));
        assert_eq!(len, 3);
        it.SetOriginalOffset(7);
        assert!(it.IsOriginalCharSkipped(Some(&mut len)));
        assert_eq!(len, 1);
        // at the end: "we return true and *aRunLength is set to zero"
        it.SetOriginalOffset(10);
        assert!(it.IsOriginalCharSkipped(Some(&mut len)));
        assert_eq!(len, 0);
    }

    #[test]
    fn an_original_string_to_skip_chars_offset_shifts_the_original_offsets() {
        let s = sample();
        // offset -4: incoming offsets have it added, outgoing ones have it subtracted
        let mut it = gfxSkipCharsIterator::new(&s, -4, 10);
        assert_eq!(it.GetOriginalOffset(), 10);
        assert_eq!(it.GetSkippedOffset(), 4); // internal offset 6
        it.SetOriginalOffset(9);
        assert_eq!(it.GetSkippedOffset(), 3); // internal offset 5: 'f' is the 4th kept character
        assert_eq!(it.GetOriginalOffset(), 9);
        assert_eq!(it.GetOriginalEnd(), 14);
    }

    #[test]
    fn take_from_moves_the_ranges() {
        let mut a = sample();
        let mut b = gfxSkipChars::new();
        b.TakeFrom(&mut a);
        assert_eq!(b.GetOriginalCharCount(), 10);
        assert_eq!(a.GetOriginalCharCount(), 0);
    }
}
