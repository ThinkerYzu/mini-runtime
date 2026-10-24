// Gecko: layout/style/CounterStyleManager.h, CounterStyleManager.cpp (REDUCED)
//
// The text of a counter value in a counter style: the digits of 1, 2, 3 in `decimal`, `a`, `b`, `c` in
// `lower-alpha`, the bullets of `disc`, `circle` and `square`, with the prefix and suffix of each.
//
// REDUCED: only the builtin counter styles that Stylo's servo configuration can parse for
// `list-style-type` and `counter()` (`list-style-type` is a keyword list there, not a <counter-style>),
// so there are no @counter-style rules, no `symbols()`, no strings and no extends. A style is named by the
// lowercased Debug name of Stylo's keyword (`loweralpha`, `disclosureopen`). The
// `BuiltinCounterStyle`, `CustomCounterStyle` and `CounterStyleManager` classes collapse into the
// functions below, which keep the Gecko names where there is one.
// NOT PORTED: the rest of the builtin styles (hebrew, ethiopic, the CJK numerals, armenian, georgian,
// roman: Stylo's servo build does not parse them either), additive and fixed systems (`hiragana`,
// `katakana`, `cjk-earthly-branch`, ... fall back to decimal), speak-as, pad, writing mode, and ranges
// other than the alphabetic one.

use crate::*;

// Gecko: layout/style/CounterStyleManager.h CounterValue
pub type CounterValue = i32;

// Gecko: layout/style/CounterStyleManager.cpp kDiscCharacter, kCircleCharacter, kSquareCharacter,
// kRightPointingCharacter, kDownPointingCharacter
pub const kDiscCharacter: char = '\u{2022}';
pub const kCircleCharacter: char = '\u{25e6}';
pub const kSquareCharacter: char = '\u{25aa}';
pub const kRightPointingCharacter: char = '\u{25b8}';
pub const kDownPointingCharacter: char = '\u{25be}';

// Gecko: layout/style/CounterStyleManager.cpp GetAlphabeticCounterText
// (symbols are chars here; Gecko's can be strings)
pub fn GetAlphabeticCounterText(aOrdinal: CounterValue, aResult: &mut String, aSymbols: &[char]) -> bool {
    MOZ_ASSERT!(aSymbols.len() >= 2, "Too few symbols for alphabetic counter.");
    MOZ_ASSERT!(aOrdinal >= 0, "Invalid ordinal.");
    let mut aOrdinal = aOrdinal;
    if aOrdinal == 0 {
        return false;
    }

    let n = aSymbols.len() as i32;
    // The precise length of this array should be
    // ceil(log((double) aOrdinal / n * (n - 1) + 1) / log(n)).
    // The max length is slightly smaller than which defined below.
    let mut indexes: Vec<i32> = Vec::new();
    while aOrdinal > 0 {
        aOrdinal -= 1;
        indexes.push(aOrdinal % n);
        aOrdinal /= n;
    }

    aResult.clear();
    for i in (1..=indexes.len()).rev() {
        aResult.push(aSymbols[indexes[i - 1] as usize]);
    }
    true
}

// Gecko: layout/style/CounterStyleManager.cpp GetNumericCounterText
pub fn GetNumericCounterText(aOrdinal: CounterValue, aResult: &mut String, aSymbols: &[char]) -> bool {
    MOZ_ASSERT!(aSymbols.len() >= 2, "Too few symbols for numeric counter.");
    MOZ_ASSERT!(aOrdinal >= 0, "Invalid ordinal.");
    let mut aOrdinal = aOrdinal;

    if aOrdinal == 0 {
        aResult.clear();
        aResult.push(aSymbols[0]);
        return true;
    }

    let n = aSymbols.len() as i32;
    let mut indexes: Vec<i32> = Vec::new();
    while aOrdinal > 0 {
        indexes.push(aOrdinal % n);
        aOrdinal /= n;
    }

    aResult.clear();
    for i in (1..=indexes.len()).rev() {
        aResult.push(aSymbols[indexes[i - 1] as usize]);
    }
    true
}

// ADAPTER: the ten digits of a numeric style that starts at the code point aZero, or the explicit
// symbols of `cjk-decimal`.
fn NumericSymbols(aStyle: &str) -> Option<Vec<char>> {
    let zero: u32 = match aStyle {
        "decimal" => '0' as u32,
        "arabicindic" => 0x660,
        "bengali" => 0x9E6,
        "cambodian" | "khmer" => 0x17E0,
        "devanagari" => 0x966,
        "gujarati" => 0xAE6,
        "gurmukhi" => 0xA66,
        "kannada" => 0xCE6,
        "lao" => 0xED0,
        "malayalam" => 0xD66,
        "mongolian" => 0x1810,
        "myanmar" => 0x1040,
        "oriya" => 0xB66,
        "persian" => 0x6F0,
        "telugu" => 0xC66,
        "thai" => 0xE50,
        "tibetan" => 0xF20,
        "cjkdecimal" => return Some("\u{3007}\u{4E00}\u{4E8C}\u{4E09}\u{56DB}\u{4E94}\u{516D}\u{4E03}\u{516B}\u{4E5D}".chars().collect()),
        _ => return None,
    };
    Some((0..10).map(|i| char::from_u32(zero + i).unwrap()).collect())
}

// ADAPTER: the symbols of an alphabetic style.
fn AlphabeticSymbols(aStyle: &str) -> Option<Vec<char>> {
    match aStyle {
        "loweralpha" | "lowerlatin" => Some(('a'..='z').collect()),
        "upperalpha" | "upperlatin" => Some(('A'..='Z').collect()),
        "lowergreek" => Some("\u{3B1}\u{3B2}\u{3B3}\u{3B4}\u{3B5}\u{3B6}\u{3B7}\u{3B8}\u{3B9}\u{3BA}\u{3BB}\u{3BC}\u{3BD}\u{3BE}\u{3BF}\u{3C0}\u{3C1}\u{3C3}\u{3C4}\u{3C5}\u{3C6}\u{3C7}\u{3C8}\u{3C9}".chars().collect()),
        _ => None,
    }
}

// Gecko: layout/style/CounterStyleManager.cpp BuiltinCounterStyle::IsBullet
// (aStyle: the lowercased Debug name of Stylo's list-style-type keyword)
pub fn CounterStyle_IsBullet(aStyle: &str) -> bool {
    matches!(aStyle, "disc" | "circle" | "square" | "disclosureclosed" | "disclosureopen")
}

// Gecko: layout/style/CounterStyleManager.cpp BuiltinCounterStyle::GetPrefix
pub fn CounterStyle_GetPrefix(_aStyle: &str) -> &'static str {
    ""
}

// Gecko: layout/style/CounterStyleManager.cpp BuiltinCounterStyle::GetSuffix
pub fn CounterStyle_GetSuffix(aStyle: &str) -> &'static str {
    match aStyle {
        "none" => "",
        "disc" | "circle" | "square" | "disclosureclosed" | "disclosureopen" => " ",
        "cjkdecimal" | "cjkearthlybranch" | "cjkheavenlystem" => "\u{3001}",
        _ => ". ",
    }
}

// Gecko: layout/style/CounterStyleManager.cpp BuiltinCounterStyle::GetInitialCounterText
// and CounterStyle::GetCounterText (the sign and the fallback to decimal)
// ADAPTER: the text of aOrdinal in the style named aStyle, without prefix and suffix.
pub fn CounterStyle_GetCounterText(aStyle: &str, aOrdinal: CounterValue) -> String {
    let mut result = String::new();
    match aStyle {
        "none" => {}
        "disc" => result.push(kDiscCharacter),
        "circle" => result.push(kCircleCharacter),
        "square" => result.push(kSquareCharacter),
        "disclosureclosed" => result.push(kRightPointingCharacter),
        "disclosureopen" => result.push(kDownPointingCharacter),
        _ => {
            let negative = aOrdinal < 0;
            let ordinal = aOrdinal.checked_abs().unwrap_or(i32::MAX);
            if let Some(symbols) = AlphabeticSymbols(aStyle) {
                // range: 1 infinite
                if aOrdinal >= 1 && GetAlphabeticCounterText(aOrdinal, &mut result, &symbols) {
                    return result;
                }
                return CounterStyle_GetCounterText("decimal", aOrdinal);
            }
            // decimal and the numeric styles; a style that is not ported falls back to decimal
            // (CallFallbackStyle)
            let symbols = NumericSymbols(aStyle).unwrap_or_else(|| NumericSymbols("decimal").unwrap());
            GetNumericCounterText(ordinal, &mut result, &symbols);
            if negative {
                result.insert(0, '-');
            }
        }
    }
    result
}

// Gecko: layout/generic/nsCounterManager.cpp nsCounterUseNode::GetText (the legacy bullet case)
// ADAPTER: prefix, number and suffix, which is what a list marker shows ("1. ", "a. ", a bullet and a space).
pub fn CounterStyle_GetMarkerText(aStyle: &str, aOrdinal: CounterValue) -> String {
    if aStyle == "none" {
        return String::new();
    }
    let mut text = String::from(CounterStyle_GetPrefix(aStyle));
    text.push_str(&CounterStyle_GetCounterText(aStyle, aOrdinal));
    text.push_str(CounterStyle_GetSuffix(aStyle));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests from the behaviour documented in CSS Counter Styles 3 (alphabetic, numeric, fallback).
    #[test]
    fn alphabetic() {
        let t = |n| CounterStyle_GetCounterText("loweralpha", n);
        assert_eq!(t(1), "a");
        assert_eq!(t(26), "z");
        assert_eq!(t(27), "aa");
        assert_eq!(t(52), "az");
        assert_eq!(t(53), "ba");
        assert_eq!(t(702), "zz");
        assert_eq!(t(703), "aaa");
        // out of the range 1..: decimal
        assert_eq!(t(0), "0");
        assert_eq!(t(-3), "-3");
        assert_eq!(CounterStyle_GetCounterText("upperalpha", 28), "AB");
        assert_eq!(CounterStyle_GetCounterText("lowergreek", 25), "\u{3b1}\u{3b1}");
    }

    #[test]
    fn numeric() {
        assert_eq!(CounterStyle_GetCounterText("decimal", 0), "0");
        assert_eq!(CounterStyle_GetCounterText("decimal", 1234), "1234");
        assert_eq!(CounterStyle_GetCounterText("decimal", -7), "-7");
        assert_eq!(CounterStyle_GetCounterText("thai", 12), "\u{e51}\u{e52}");
        assert_eq!(CounterStyle_GetCounterText("cjkdecimal", 10), "\u{4e00}\u{3007}");
        // not ported: decimal
        assert_eq!(CounterStyle_GetCounterText("hiragana", 3), "3");
    }

    #[test]
    fn marker_text() {
        assert_eq!(CounterStyle_GetMarkerText("decimal", 3), "3. ");
        assert_eq!(CounterStyle_GetMarkerText("loweralpha", 2), "b. ");
        assert_eq!(CounterStyle_GetMarkerText("disc", 9), "\u{2022} ");
        assert_eq!(CounterStyle_GetMarkerText("circle", 1), "\u{25e6} ");
        assert_eq!(CounterStyle_GetMarkerText("square", 1), "\u{25aa} ");
        assert_eq!(CounterStyle_GetMarkerText("disclosureclosed", 1), "\u{25b8} ");
        assert_eq!(CounterStyle_GetMarkerText("disclosureopen", 1), "\u{25be} ");
        assert_eq!(CounterStyle_GetMarkerText("none", 1), "");
        assert_eq!(CounterStyle_GetMarkerText("cjkdecimal", 2), "\u{4e8c}\u{3001}");
    }
}
