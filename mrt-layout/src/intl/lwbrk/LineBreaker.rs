// Gecko: intl/lwbrk/LineBreaker.h, intl/lwbrk/LineBreaker.cpp
//
// icu_segmenter: Gecko's LineBreaker.cpp calls the C++ icu4x bindings (icu4x::LineSegmenter,
// LineBreakIteratorLatin1/Utf16 over capi::icu4x_LineSegmenter_create_*). Those bindings are generated
// from the Rust crate `icu_segmenter` (Firefox: third_party/rust/icu_segmenter 2.0, features auto +
// compiled_data), so here the crate is called directly. Each place where a capi call is replaced is
// marked "icu_segmenter:". The function structure around it is Gecko's.
use crate::*;
use icu_locale_core::LanguageIdentifier;
use icu_segmenter::options::{LineBreakOptions, LineBreakStrictness, LineBreakWordOption};
use icu_segmenter::{LineSegmenter, LineSegmenterBorrowed};
use std::cell::RefCell;

pub const NS_LINEBREAKER_NEED_MORE_TEXT: i32 = -1;

// Gecko: intl/lwbrk/Segmenter.h mozilla::intl::WordBreakRule
// Each enum value has the same meaning with respect to the `word-break`
// property values in the CSS Text spec. See the details in
// https://drafts.csswg.org/css-text-3/#word-break-property
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WordBreakRule {
    Normal = 0,
    BreakAll,
    KeepAll,
}

// Gecko: intl/lwbrk/Segmenter.h mozilla::intl::LineBreakRule
// Each enum value has the same meaning with respect to the `line-break`
// property values in the CSS Text spec. See the details in
// https://drafts.csswg.org/css-text-3/#line-break-property.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineBreakRule {
    Auto = 0,
    Loose,
    Normal,
    Strict,
    Anywhere,
}

// Gecko: intl/lwbrk/LineBreaker.h mozilla::intl::LineBreaker
// LineBreaker is a utility class with only static methods. No need to
// instantiate it.
pub struct LineBreaker;

// Gecko: intl/lwbrk/LineBreaker.h mozilla::intl::NS_IsSpace
pub fn NS_IsSpace(u: u16) -> bool {
    u == 0x0020 ||                   // SPACE
    u == 0x0009 ||                   // CHARACTER TABULATION
    u == 0x000D ||                   // CARRIAGE RETURN
    (0x2000 <= u && u <= 0x2006) ||  // EN QUAD, EM QUAD, EN SPACE,
                                     // EM SPACE, THREE-PER-EM SPACE,
                                     // FOUR-PER-SPACE, SIX-PER-EM SPACE,
    (0x2008 <= u && u <= 0x200B) ||  // PUNCTUATION SPACE, THIN SPACE,
                                     // HAIR SPACE, ZERO WIDTH SPACE
    u == 0x1361 ||                   // ETHIOPIC WORDSPACE
    u == 0x1680 ||                   // OGHAM SPACE MARK
    u == 0x205F                      // MEDIUM MATHEMATICAL SPACE
}

// Gecko: intl/lwbrk/LineBreaker.cpp ConvertLineBreakRuleToICU4X
fn ConvertLineBreakRuleToICU4X(aLevel: LineBreakRule) -> LineBreakStrictness {
    match aLevel {
        LineBreakRule::Auto => LineBreakStrictness::Strict,
        LineBreakRule::Strict => LineBreakStrictness::Strict,
        LineBreakRule::Loose => LineBreakStrictness::Loose,
        LineBreakRule::Normal => LineBreakStrictness::Normal,
        LineBreakRule::Anywhere => LineBreakStrictness::Anywhere,
    }
}

// Gecko: intl/lwbrk/LineBreaker.cpp ConvertWordBreakRuleToICU4X
fn ConvertWordBreakRuleToICU4X(aWordBreak: WordBreakRule) -> LineBreakWordOption {
    match aWordBreak {
        WordBreakRule::Normal => LineBreakWordOption::Normal,
        WordBreakRule::BreakAll => LineBreakWordOption::BreakAll,
        WordBreakRule::KeepAll => LineBreakWordOption::KeepAll,
    }
}

// icu_segmenter: Gecko keeps `static capi::LineSegmenter* sLineSegmenter` and `static capi::Locale*
// sZhLocale` behind std::call_once. The borrowed segmenter of the crate is a cheap value over static
// compiled data, so the statics become thread-locals that are filled on first use (and need no destroy).
thread_local! {
    static sLineSegmenter: RefCell<Option<LineSegmenterBorrowed<'static>>> = RefCell::new(None);
    static sZhLocale: RefCell<Option<LanguageIdentifier>> = RefCell::new(None);
}

// Gecko: intl/lwbrk/LineBreaker.cpp GetDefaultLineSegmenter
fn GetDefaultLineSegmenter() -> LineSegmenterBorrowed<'static> {
    sLineSegmenter.with(|s| {
        let mut s = s.borrow_mut();
        if s.is_none() {
            // icu_segmenter: capi::icu4x_LineSegmenter_create_auto_mv1()
            *s = Some(LineSegmenter::new_auto(LineBreakOptions::default()));
        }
        s.clone().unwrap()
    })
}

// Gecko: intl/lwbrk/LineBreaker.cpp UseDefaultLineSegmenter
fn UseDefaultLineSegmenter(aWordBreak: WordBreakRule, aLevel: LineBreakRule, aIsChineseOrJapanese: bool) -> bool {
    aWordBreak == WordBreakRule::Normal
        && (aLevel == LineBreakRule::Strict || aLevel == LineBreakRule::Auto)
        && !aIsChineseOrJapanese
}

// Gecko: intl/lwbrk/LineBreaker.cpp InitDefaultLocale
fn InitDefaultLocale() {
    sZhLocale.with(|l| {
        let mut l = l.borrow_mut();
        if l.is_none() {
            // icu_segmenter: capi::icu4x_Locale_from_string_mv1("zh")
            if let Ok(locale) = "zh".parse::<LanguageIdentifier>() {
                *l = Some(locale);
            }
        }
    });
}

// Gecko: intl/lwbrk/LineBreaker.cpp GetLineSegmenter
fn GetLineSegmenter(aUseDefault: bool, aWordBreak: WordBreakRule, aLevel: LineBreakRule, aIsChineseOrJapanese: bool) -> LineSegmenterBorrowed<'static> {
    if aUseDefault {
        MOZ_ASSERT!(UseDefaultLineSegmenter(aWordBreak, aLevel, aIsChineseOrJapanese));
        return GetDefaultLineSegmenter();
    }

    if sZhLocale.with(|l| l.borrow().is_none()) && aIsChineseOrJapanese {
        InitDefaultLocale();
    }

    let mut options = LineBreakOptions::default();
    options.word_option = Some(ConvertWordBreakRuleToICU4X(aWordBreak));
    options.strictness = Some(ConvertLineBreakRuleToICU4X(aLevel));
    let locale: Option<LanguageIdentifier> = if aIsChineseOrJapanese { sZhLocale.with(|l| l.borrow().clone()) } else { None };
    options.content_locale = locale.as_ref();

    // icu_segmenter: capi::icu4x_LineSegmenter_create_lstm_with_options_v2_mv1(locale, options.AsFFI())
    // The segmenter does not keep the locale (the options are resolved here), so `locale` can go.
    LineSegmenter::new_lstm(options)
}

impl LineBreaker {
    /**
     * Call this on a word with whitespace at either end. We will apply UAX#29
     * rules to find breaks inside the word. aBreakBefore is set to the break-
     * before status of each character; aBreakBefore[0] will always be false
     * because we never return a break before the first character.
     * aLength is the length of the aText array and also the length of the
     * aBreakBefore output array.
     */
    // Gecko: intl/lwbrk/LineBreaker.cpp LineBreaker::ComputeBreakPositions(const char16_t*, ...)
    pub fn ComputeBreakPositions__char16_t_ptr_uint32_t_WordBreakRule_LineBreakRule_bool_uint8_t_ptr(
        aChars: &[u16],
        aLength: u32,
        aWordBreak: WordBreakRule,
        aLevel: LineBreakRule,
        aIsChineseOrJapanese: bool,
        aBreakBefore: &mut [u8],
    ) {
        if aLength == 1 {
            // Although UAX#14 LB2 rule requires never breaking at the start of text
            // (SOT), ICU4X line segmenter API is designed to match other segmenter in
            // UAX#29 to always break at the start of text. Hence the optimization
            // here to avoid calling into ICU4X line segmenter.
            aBreakBefore[0] = 1;
            return;
        }

        // We only cache line-breaks if we think the text is likely to hit the slow
        // (LSTM) codepath in icu_segmenter. To avoid scanning the entire text just
        // to make that decision, we probe every /kStride/ characters.
        // NOT PORTED: LineBreakCache (intl/lwbrk/LineBreakCache.h/.cpp, a hash cache of the results below);
        // the computation is always done. The probe was:
        //   bool useCache = [=]() { const uint32_t kStride = 8;
        //     for (uint32_t i = 0; i < aLength; i += kStride) { if (UnicodeProperties::IsScriptioContinua(aChars[i])) return true; }
        //     return false; }();
        //   Maybe<LineBreakCache::Entry> entry; if (useCache) { key{..}; entry.emplace(Cache()->Lookup(key)); if (*entry) { CopyAndFill(..); return; } }
        // and, after the computation, the trailing-false trimming and entry->Set(..).

        aBreakBefore[..aLength as usize].fill(0);

        // CheckedInt<int32_t> length = aLength;
        let length: Option<i32> = i32::try_from(aLength).ok();
        if let Some(length) = length {
            let useDefault = UseDefaultLineSegmenter(aWordBreak, aLevel, aIsChineseOrJapanese);
            let lineSegmenter = GetLineSegmenter(useDefault, aWordBreak, aLevel, aIsChineseOrJapanese);
            // icu_segmenter: segmenter->segment16(std::u16string_view{aChars, aLength})
            let mut iterator = lineSegmenter.segment_utf16(&aChars[..aLength as usize]);

            loop {
                // icu_segmenter: iterator->next() returns -1 at the end, the crate returns None.
                let nextPos: i32 = match iterator.next() {
                    Some(p) => p as i32,
                    None => -1,
                };
                if nextPos < 0 || nextPos >= length {
                    break;
                }
                aBreakBefore[nextPos as usize] = 1;
            }

            // icu_segmenter: capi::icu4x_LineSegmenter_destroy_mv1 is not needed (Rust drops it).
        }
    }

    // Gecko: intl/lwbrk/LineBreaker.cpp LineBreaker::ComputeBreakPositions(const uint8_t*, ...)
    pub fn ComputeBreakPositions__uint8_t_ptr_uint32_t_WordBreakRule_LineBreakRule_bool_uint8_t_ptr(
        aChars: &[u8],
        aLength: u32,
        aWordBreak: WordBreakRule,
        aLevel: LineBreakRule,
        aIsChineseOrJapanese: bool,
        aBreakBefore: &mut [u8],
    ) {
        if aLength == 1 {
            // Although UAX#14 LB2 rule requires never breaking at the start of text
            // (SOT), ICU4X line segmenter API is designed to match other segmenter in
            // UAX#29 to always break at the start of text. Hence the optimization
            // here to avoid calling into ICU4X line segmenter.
            aBreakBefore[0] = 1;
            return;
        }

        aBreakBefore[..aLength as usize].fill(0);

        // CheckedInt<int32_t> length = aLength;
        let length: Option<i32> = i32::try_from(aLength).ok();
        let Some(length) = length else {
            return;
        };

        let useDefault = UseDefaultLineSegmenter(aWordBreak, aLevel, aIsChineseOrJapanese);
        let lineSegmenter = GetLineSegmenter(useDefault, aWordBreak, aLevel, aIsChineseOrJapanese);
        // icu_segmenter: segmenter->segment_latin1(diplomat::span<const uint8_t>{aChars, aLength})
        let mut iterator = lineSegmenter.segment_latin1(&aChars[..aLength as usize]);

        loop {
            let nextPos: i32 = match iterator.next() {
                Some(p) => p as i32,
                None => -1,
            };
            if nextPos < 0 || nextPos >= length {
                break;
            }
            aBreakBefore[nextPos as usize] = 1;
        }

        // icu_segmenter: capi::icu4x_LineSegmenter_destroy_mv1 is not needed (Rust drops it).
    }

    // Gecko: intl/lwbrk/LineBreaker.cpp LineBreaker::Shutdown
    pub fn Shutdown() {
        // icu_segmenter: the capi objects were destroyed here; the thread-locals are just cleared.
        sLineSegmenter.with(|s| *s.borrow_mut() = None);
        sZhLocale.with(|l| *l.borrow_mut() = None);
    }
}

// Tests: Gecko's intl/lwbrk/gtest/TestBreak.cpp only has word-break tests (translated in
// WordBreaker.rs); the line-break tests here are written from UAX #14 and the CSS Text
// line-break / word-break definitions.
#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    // The offsets with a break opportunity before them, except offset 0: icu_segmenter (like the icu4x
    // bindings Gecko calls) always reports a break at the start of the text, although the header
    // comment says aBreakBefore[0] is false. nsLineBreaker saves and restores that entry
    // (AutoRestore saveWordStartBreakState), so the test ignores it too.
    fn breaks_of(s: &str, aWordBreak: WordBreakRule, aLevel: LineBreakRule, aCJ: bool) -> Vec<u32> {
        let text = utf16(s);
        let mut bb = vec![0u8; text.len()];
        LineBreaker::ComputeBreakPositions__char16_t_ptr_uint32_t_WordBreakRule_LineBreakRule_bool_uint8_t_ptr(
            &text,
            text.len() as u32,
            aWordBreak,
            aLevel,
            aCJ,
            &mut bb,
        );
        (1..text.len() as u32).filter(|&i| bb[i as usize] != 0).collect()
    }

    fn breaks(s: &str) -> Vec<u32> {
        breaks_of(s, WordBreakRule::Normal, LineBreakRule::Auto, false)
    }

    #[test]
    fn breaks_after_spaces() {
        // 'Hello world, this is a test': a break opportunity after each space, i.e. before the next word
        assert_eq!(breaks("Hello world, this is a test"), vec![6, 13, 18, 21, 23]);
    }

    #[test]
    fn a_word_without_spaces_has_no_break() {
        assert_eq!(breaks("Hello"), Vec::<u32>::new());
    }

    #[test]
    fn a_single_character_is_a_break_before_itself() {
        // The aLength == 1 shortcut of ComputeBreakPositions.
        let mut bb = [0u8; 1];
        LineBreaker::ComputeBreakPositions__char16_t_ptr_uint32_t_WordBreakRule_LineBreakRule_bool_uint8_t_ptr(
            &utf16("a"), 1, WordBreakRule::Normal, LineBreakRule::Auto, false, &mut bb,
        );
        assert_eq!(bb, [1]);
    }

    #[test]
    fn breaks_after_a_hyphen() {
        // UAX #14 LB21: break after a hyphen (class HY) when followed by a letter
        assert_eq!(breaks("well-known"), vec![5]);
    }

    #[test]
    fn no_break_before_closing_punctuation() {
        // UAX #14 LB13: no break before ',', '.', '!', '?' or ')' (classes IS, CL, EX)
        assert_eq!(breaks("Hello, world."), vec![7]);
        assert_eq!(breaks("(hi) there"), vec![5]);
        assert_eq!(breaks("what? yes"), vec![6]);
    }

    #[test]
    fn breaks_between_cjk_ideographs() {
        // UAX #14 LB (ID class): a break opportunity between ideographs
        assert_eq!(breaks("日本語"), vec![1, 2]);
    }

    #[test]
    fn no_break_before_cjk_closing_punctuation() {
        // 'IDEOGRAPHIC FULL STOP' is class CL: no break before it
        assert_eq!(breaks("日本。語"), vec![1, 3]);
    }

    #[test]
    fn url_like_text_breaks_after_slashes() {
        // UAX #14 LB13/LB25: SY (slash) allows a break after it when followed by a letter ...
        let b = breaks("see http://example.com/path/to");
        assert!(b.contains(&4)); // after the space
        // ... but no break between 'example' and '.com' (LB13: no break before the full stop is IS)
        let dot = "see http://example.com".find('.').unwrap() as u32;
        assert!(!b.contains(&dot));
        assert!(!b.contains(&(dot + 1)));
    }

    #[test]
    fn word_break_break_all_breaks_between_letters() {
        // CSS word-break: break-all
        assert_eq!(breaks_of("abc", WordBreakRule::BreakAll, LineBreakRule::Auto, false), vec![1, 2]);
    }

    #[test]
    fn word_break_keep_all_does_not_break_cjk() {
        // CSS word-break: keep-all
        assert_eq!(breaks_of("日本語", WordBreakRule::KeepAll, LineBreakRule::Auto, false), Vec::<u32>::new());
        // ... but breakable spaces still break
        assert_eq!(breaks_of("日本 語", WordBreakRule::KeepAll, LineBreakRule::Auto, false), vec![3]);
    }

    #[test]
    fn line_break_loose_allows_a_break_before_small_kana() {
        // CSS line-break: the strict level keeps the small kana "ゃ" with the previous character (class CJ);
        // loose allows a break before it.
        let strict = breaks_of("きゃ", WordBreakRule::Normal, LineBreakRule::Strict, true);
        let loose = breaks_of("きゃ", WordBreakRule::Normal, LineBreakRule::Loose, true);
        assert_eq!(strict, Vec::<u32>::new());
        assert_eq!(loose, vec![1]);
    }

    #[test]
    fn latin1_text_breaks_like_utf16() {
        let text: Vec<u8> = b"Hello world, this".to_vec();
        let mut bb = vec![0u8; text.len()];
        LineBreaker::ComputeBreakPositions__uint8_t_ptr_uint32_t_WordBreakRule_LineBreakRule_bool_uint8_t_ptr(
            &text,
            text.len() as u32,
            WordBreakRule::Normal,
            LineBreakRule::Auto,
            false,
            &mut bb,
        );
        let got: Vec<usize> = (1..bb.len()).filter(|&i| bb[i] != 0).collect();
        assert_eq!(got, vec![6, 13]);
    }

    #[test]
    fn ns_is_space_is_the_breakable_whitespace_set() {
        for c in [0x20u16, 0x09, 0x0D, 0x2000, 0x2006, 0x2008, 0x200B, 0x1361, 0x1680, 0x205F] {
            assert!(NS_IsSpace(c), "{c:x}");
        }
        // not breakable whitespace: LF (a segment break), NBSP, U+2007 FIGURE SPACE, U+3000
        for c in [0x0A_u16, 0xA0, 0x2007, 0x3000, 0x41] {
            assert!(!NS_IsSpace(c), "{c:x}");
        }
    }
}
