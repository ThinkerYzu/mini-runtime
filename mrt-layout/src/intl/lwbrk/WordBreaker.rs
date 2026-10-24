// Gecko: intl/lwbrk/WordBreaker.h, intl/lwbrk/WordBreaker.cpp
//
// icu_segmenter: WordBreaker.cpp calls icu4x::WordSegmenter::create_auto() and segment16(); here the
// `icu_segmenter` crate is called directly (WordSegmenter::new_auto, segment_utf16).
use crate::*;
use icu_segmenter::options::WordBreakInvariantOptions;
use icu_segmenter::WordSegmenter;

pub const NS_WORDBREAKER_NEED_MORE_TEXT: i32 = -1;

// Gecko: intl/lwbrk/WordBreaker.h mozilla::intl::WordRange
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WordRange {
    pub mBegin: u32,
    pub mEnd: u32,
}

// Gecko: intl/lwbrk/WordBreaker.h mozilla::intl::WordBreaker (a class with only static methods)
pub struct WordBreaker;

// Find the word boundary by scanning forward and backward from aPos.
//
// If setting StopAtPunctuation, even if using UAX#29 word segmenter rule,
// there will be break opportunities on characters with punctuation class.
// Gecko: intl/lwbrk/WordBreaker.h WordBreaker::FindWordOptions
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WordBreaker_FindWordOptions {
    None,
    StopAtPunctuation,
}

impl WordBreaker {
    // Find the word boundary by scanning forward and backward from aPos.
    //
    // @return WordRange where mBegin equals to the offset to first character in
    // the word and mEnd equals to the offset to the last character plus 1. mEnd
    // can be aText.Lengh() if the desired word is at the end of aText.
    //
    // If aPos is already at the end of aText or beyond, both mBegin and mEnd
    // equals to aText.Length().
    // Gecko: intl/lwbrk/WordBreaker.cpp WordBreaker::FindWord
    pub fn FindWord(aText: &[u16], aPos: u32, aOptions: WordBreaker_FindWordOptions) -> WordRange {
        // const CheckedInt<uint32_t> len = aText.Length();
        let len: u32 = u32::try_from(aText.len()).expect("MOZ_RELEASE_ASSERT(len.isValid())");

        if aPos >= len {
            return WordRange { mBegin: len, mEnd: len };
        }

        let mut range = WordRange { mBegin: 0, mEnd: len };

        // icu_segmenter: icu4x::WordSegmenter::create_auto() and segmenter->segment16(..)
        let segmenter = WordSegmenter::new_auto(WordBreakInvariantOptions::default());
        let mut iterator = segmenter.segment_utf16(aText);

        let mut previousPos: u32 = 0;
        loop {
            // icu_segmenter: iterator->next() returns -1 at the end, the crate returns None.
            let nextPos: i32 = match iterator.next() {
                Some(p) => p as i32,
                None => -1,
            };
            if nextPos < 0 {
                range.mBegin = previousPos;
                range.mEnd = len;
                break;
            }
            if (nextPos as u32) > aPos {
                range.mBegin = previousPos;
                range.mEnd = nextPos as u32;
                break;
            }

            previousPos = nextPos as u32;
        }

        if aOptions != WordBreaker_FindWordOptions::StopAtPunctuation {
            return range;
        }

        for i in range.mBegin..range.mEnd {
            if IsPunctuationForWordSelect(aText[i as usize]) {
                if i > aPos {
                    range.mEnd = i;
                    break;
                }
                if i == aPos {
                    range.mBegin = i;
                    range.mEnd = i + 1;
                    break;
                }
                if i < aPos {
                    range.mBegin = i + 1;
                }
            }
        }

        range
    }
}

// Tests: translated from intl/lwbrk/gtest/TestBreak.cpp (WordBreak.*), same names.
#[cfg(test)]
mod tests {
    use super::*;

    fn u16s(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    //                         012345678901234
    const wb0: &str = "T";
    const wb1: &str = "h";
    const wb2: &str = "";
    const wb3: &str = "is   is a int";
    const wb4: &str = "";
    const wb5: &str = "";
    const wb6: &str = "ernationali";
    const wb7: &str = "zation work.";

    const wb: [&str; 8] = [wb0, wb1, wb2, wb3, wb4, wb5, wb6, wb7];

    // This function searches a complete word starting from |offset| in wb[fragN].
    // If it reaches the end of wb[fragN], and there is no word break opportunity
    // between wb[fragN] and wb[fragN+1], it will continue the search in wb[fragN+1]
    // until a word break.
    fn TestFindWordBreakFromPosition(fragN: usize, offset: u32, expected: &str) {
        let numOfFragment = wb.len();

        let fragText = u16s(wb[fragN]);

        let res = WordBreaker::FindWord(&fragText, offset, WordBreaker_FindWordOptions::None);

        let mut result: Vec<u16> = fragText[res.mBegin as usize..res.mEnd as usize].to_vec();

        if (fragText.len() as u32) <= res.mEnd {
            // if we hit the end of the fragment
            for p in fragN + 1..numOfFragment {
                let nextFragText = u16s(wb[p]);
                if nextFragText.is_empty() {
                    // If nextFragText is empty, there's no new possible word break
                    // opportunity between previous word and nextFragText.
                    continue;
                }

                let r = WordBreaker::FindWord(&nextFragText, 0, WordBreaker_FindWordOptions::None);

                result.extend_from_slice(&nextFragText[r.mBegin as usize..r.mEnd as usize]);

                if nextFragText.len() as u32 != r.mEnd {
                    break;
                }
            }
        }

        assert_eq!(expected, String::from_utf16(&result).unwrap(), "FindWordBreakFromPosition({}, {})", fragN, offset);
    }

    #[test]
    fn TestFindWordWithEmptyString() {
        let expect = WordRange { mBegin: 0, mEnd: 0 };
        let result = WordBreaker::FindWord(&[], 0, WordBreaker_FindWordOptions::None);
        assert_eq!(expect.mBegin, result.mBegin);
        assert_eq!(expect.mEnd, result.mEnd);
    }

    #[test]
    fn TestFindWordBreakFromPosition_() {
        TestFindWordBreakFromPosition(0, 0, "This");
        TestFindWordBreakFromPosition(1, 0, "his");
        TestFindWordBreakFromPosition(2, 0, "is");
        TestFindWordBreakFromPosition(3, 0, "is");
        TestFindWordBreakFromPosition(3, 1, "is");
        TestFindWordBreakFromPosition(3, 9, " ");
        TestFindWordBreakFromPosition(3, 10, "internationalization");
        TestFindWordBreakFromPosition(4, 0, "ernationalization");
        TestFindWordBreakFromPosition(5, 0, "ernationalization");
        TestFindWordBreakFromPosition(6, 4, "ernationalization");
        TestFindWordBreakFromPosition(6, 8, "ernationalization");
        TestFindWordBreakFromPosition(7, 6, " ");
        TestFindWordBreakFromPosition(7, 7, "work");
    }

    // Test for StopAtPunctuation option.
    #[test]
    fn TestFindBreakWithStopAtPunctuation() {
        let fragText = u16s("one.two");

        let result4 = WordBreaker::FindWord(&fragText, 0, WordBreaker_FindWordOptions::StopAtPunctuation);
        assert_eq!(0u32, result4.mBegin);
        assert_eq!(3u32, result4.mEnd);
        let result5 = WordBreaker::FindWord(&fragText, 3, WordBreaker_FindWordOptions::StopAtPunctuation);
        assert_eq!(3u32, result5.mBegin);
        assert_eq!(4u32, result5.mEnd);
        let result6 = WordBreaker::FindWord(&fragText, 4, WordBreaker_FindWordOptions::StopAtPunctuation);
        assert_eq!(4u32, result6.mBegin);
        assert_eq!(7u32, result6.mEnd);

        // Default (without StopAtPunctuation)
        let result7 = WordBreaker::FindWord(&fragText, 0, WordBreaker_FindWordOptions::None);
        assert_eq!(0u32, result7.mBegin);
        assert_eq!(7u32, result7.mEnd);
        let result8 = WordBreaker::FindWord(&fragText, 3, WordBreaker_FindWordOptions::None);
        assert_eq!(0u32, result8.mBegin);
        assert_eq!(7u32, result8.mEnd);
        let result9 = WordBreaker::FindWord(&fragText, 4, WordBreaker_FindWordOptions::None);
        assert_eq!(0u32, result9.mBegin);
        assert_eq!(7u32, result9.mEnd);
    }

    // Not in the gtest: aPos at or past the end gives an empty range at the end (WordBreaker.h comment).
    #[test]
    fn a_position_at_the_end_gives_an_empty_range_at_the_end() {
        let t = u16s("abc");
        assert_eq!(WordBreaker::FindWord(&t, 3, WordBreaker_FindWordOptions::None), WordRange { mBegin: 3, mEnd: 3 });
        assert_eq!(WordBreaker::FindWord(&t, 10, WordBreaker_FindWordOptions::None), WordRange { mBegin: 3, mEnd: 3 });
    }
}
