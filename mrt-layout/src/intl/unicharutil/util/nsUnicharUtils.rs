// Gecko: intl/unicharutil/util/nsUnicharUtils.h/.cpp, nsBidiUtils.h, nsUnicodeProperties.h/.cpp
// Only the functions that the line breaker and nsTextFrameUtils call, each under its own marker.
// The Unicode data comes from the UnicodeProperties adapter (icu_properties).
use crate::*;

// Gecko: intl/unicharutil/util/nsUnicharUtils.h IS_ZERO_WIDTH_SPACE
pub fn IS_ZERO_WIDTH_SPACE(u: u32) -> bool {
    u == 0x200B
}

// Gecko: intl/unicharutil/util/nsBidiUtils.h LRE_CHAR .. ALM_CHAR (the ones IsBidiControl uses)
pub const LRM_CHAR: u32 = 0x200e;
pub const RLM_CHAR: u32 = 0x200f;
pub const LRE_CHAR: u32 = 0x202a;
pub const RLE_CHAR: u32 = 0x202b;
pub const PDF_CHAR: u32 = 0x202c;
pub const LRO_CHAR: u32 = 0x202d;
pub const RLO_CHAR: u32 = 0x202e;
pub const LRI_CHAR: u32 = 0x2066;
pub const RLI_CHAR: u32 = 0x2067;
pub const FSI_CHAR: u32 = 0x2068;
pub const PDI_CHAR: u32 = 0x2069;
pub const ALM_CHAR: u32 = 0x061C;

// Gecko: intl/unicharutil/util/nsBidiUtils.h IsBidiControl
pub fn IsBidiControl(aChar: u32) -> bool {
    ((aChar & 0x0000ff00) == 0x00002000
        && (aChar.wrapping_sub(LRE_CHAR) <= RLO_CHAR - LRE_CHAR
            || aChar.wrapping_sub(LRI_CHAR) <= PDI_CHAR - LRI_CHAR
            || (aChar & !1) == LRM_CHAR))
        || aChar == ALM_CHAR
}

// Gecko: intl/unicharutil/util/nsBidiUtils.h IS_ARABIC_CHAR
pub fn IS_ARABIC_CHAR(c: u32) -> bool {
    (0x0600 <= c && c <= 0x08FF) && (c <= 0x06ff || (c >= 0x0750 && c <= 0x077f) || c >= 0x08a0)
}

// Gecko: intl/unicharutil/util/nsUnicodeProperties.h mozilla::unicode::IsDefaultIgnorable
pub fn IsDefaultIgnorable(aCh: u32) -> bool {
    UnicodeProperties::IsDefaultIgnorableCodePoint(aCh)
}

// Gecko: intl/unicharutil/util/nsUnicodeProperties.cpp mozilla::unicode::IsClusterExtenderExcludingJoiners(uint32_t, uint8_t)
// aCategory is the HarfBuzz general category in Gecko; here it is the GeneralCategory enum, and
// HB_UNICODE_GENERAL_CATEGORY_SPACING_MARK..NON_SPACING_MARK is Spacing_Mark, Enclosing_Mark, Nonspacing_Mark.
pub fn IsClusterExtenderExcludingJoiners__uint32_t_GeneralCategory(aCh: u32, aCategory: GeneralCategory) -> bool {
    (aCategory == GeneralCategory::Spacing_Mark
        || aCategory == GeneralCategory::Enclosing_Mark
        || aCategory == GeneralCategory::Nonspacing_Mark)
        || (aCh >= 0xff9e && aCh <= 0xff9f)    // katakana sound marks
        || (aCh >= 0x1F3FB && aCh <= 0x1F3FF)  // fitzpatrick skin tone modifiers
        || (aCh >= 0xe0020 && aCh <= 0xe007f)  // emoji (flag) tag characters
}

// Gecko: intl/unicharutil/util/nsUnicodeProperties.h mozilla::unicode::IsClusterExtenderExcludingJoiners(uint32_t)
pub fn IsClusterExtenderExcludingJoiners__uint32_t(aCh: u32) -> bool {
    aCh >= 0x0300
        && IsClusterExtenderExcludingJoiners__uint32_t_GeneralCategory(aCh, UnicodeProperties::CharType(aCh))
}

// The Korean Won currency sign has East Asian Width = HALFWIDTH, and
// Script = COMMON (rather than HANGUL), but we don't want to treat it like
// Chinese/Japanese half-width characters for segment break transformation,
// so we exclude it individually in the two functions here.
pub const kWonCurrencySign: u32 = 0x20A9;

// Gecko: intl/unicharutil/util/nsUnicharUtils.cpp mozilla::IsSegmentBreakSkipChar
pub fn IsSegmentBreakSkipChar(u: u32) -> bool {
    UnicodeProperties::IsEastAsianWidthFHWexcludingEmoji(u)
        && UnicodeProperties::GetScriptCode(u) != Script::HANGUL
        && u != kWonCurrencySign
}

// Gecko: intl/unicharutil/util/nsUnicharUtils.cpp mozilla::IsEastAsianPunctuation
pub fn IsEastAsianPunctuation(u: u32) -> bool {
    // U+FF5E FULLWIDTH TILDE has General Category = Symbol (not Punctuation),
    // but is used similarly to U+301C WAVE DASH (which does have category
    // Punctuation). So we treat FULLWIDTH TILDE as punctuation here to give the
    // two characters consistent behavior.
    const kFullwidthTilde: u32 = 0xFF5E;
    // U+3000 IDEOGRAPHIC SPACE has General Category = Zs (not Punctuation),
    // but it conflicts with a JLReq rule that space added after
    // question or exclamation mark is stipulated to be full-width if line is
    // broken after full-width space following such a punctuation mark but
    // line break is replaced by a space. So we treat IDEOGRAPHIC SPACE as
    // punctuation here to allow line breaks after it while maintaining
    // compatibility with JLReq.
    const kIdeographicSpace: u32 = 0x3000;
    UnicodeProperties::IsEastAsianWidthFHW(u)
        && ((UnicodeProperties::IsPunctuation(u) && u != kWonCurrencySign)
            || u == kFullwidthTilde
            || u == kIdeographicSpace)
}

// Gecko: intl/unicharutil/util/nsUnicharUtils.cpp mozilla::IsPunctuationForWordSelect
pub fn IsPunctuationForWordSelect(aCh: u16) -> bool {
    match UnicodeProperties::CharType(aCh as u32) {
        GeneralCategory::Connector_Punctuation => {
            // NOT PORTED: StaticPrefs::layout_word_select_stop_at_underscore() (default false)
            if aCh == b'_' as u16 && !false {
                return false;
            }
            true
        }
        GeneralCategory::Dash_Punctuation
        | GeneralCategory::Close_Punctuation
        | GeneralCategory::Final_Punctuation
        | GeneralCategory::Initial_Punctuation
        | GeneralCategory::Other_Punctuation
        | GeneralCategory::Open_Punctuation
        | GeneralCategory::Currency_Symbol
        // Deliberately omitted:
        // case HB_UNICODE_GENERAL_CATEGORY_MODIFIER_SYMBOL:     /* Sk */
        | GeneralCategory::Math_Symbol
        | GeneralCategory::Other_Symbol => true,
        _ => false,
    }
}

// Gecko: intl/unicharutil/util/nsUnicharUtils.h ToLowerCaseASCII(char16_t)
pub fn ToLowerCaseASCII(c: u16) -> u16 {
    if c >= 'A' as u16 && c <= 'Z' as u16 { c + 0x20 } else { c }
}

// Gecko: intl/unicharutil/util/nsBidiUtils.h IsBidiControlRTL
pub fn IsBidiControlRTL(aChar: u32) -> bool {
    aChar == RLM_CHAR || aChar == RLE_CHAR || aChar == RLO_CHAR || aChar == RLI_CHAR || aChar == ALM_CHAR
}

// Gecko: intl/unicharutil/util/nsBidiUtils.h HasRTLChars, which calls encoding_rs's
// encoding_mem_is_utf16_bidi. ADAPTER: that function's rule, written out: a code unit in the RTL
// blocks of the BMP, in the RTL presentation forms, a high surrogate that starts an RTL block of
// the SMP, or one of the RTL bidi controls.
pub fn HasRTLChars(aBuffer: &[u16]) -> bool {
    aBuffer.iter().any(|&u| {
        let c = u as u32;
        (0x0590..=0x08FF).contains(&c)
            || (0xFB1D..=0xFDFF).contains(&c)
            || (0xFE70..=0xFEFE).contains(&c)
            || c == 0xD802
            || c == 0xD803
            || c == 0xD83A
            || c == 0xD83B
            || c == RLM_CHAR
            || c == RLE_CHAR
            || c == RLO_CHAR
            || c == RLI_CHAR
    })
}
