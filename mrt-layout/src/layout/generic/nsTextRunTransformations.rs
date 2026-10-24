// Gecko: layout/generic/nsTextRunTransformations.h and .cpp, the case transforms of
// nsCaseTransformTextRunFactory::TransformString, for the rendered text (nsTextFrame's TransformChars).
//
// NOT PORTED: the rest of the file (nsTransformedTextRun, nsTransformingTextRunFactory,
// nsCaseTransformTextRunFactory's text runs, MergeCharactersInTextRun: layout's text-transform is
// identity, see nsTextFrame.rs), and in TransformString the language-specific casing (Turkish, Dutch,
// Greek, Irish, Lithuanian: GreekCasing and IrishCasing are not translated; every language casing is
// eLSCB_None), masking, full-width and full-size-kana (aCaseTransformsOnly is true here) and
// math-auto.
use crate::*;
use mrt_style::style::values::specified::text::TextTransformCase;

const GREEK_CAPITAL_LETTER_SIGMA: u32 = 0x03A3;
const GREEK_SMALL_LETTER_FINAL_SIGMA: u32 = 0x03C2;
const GREEK_SMALL_LETTER_SIGMA: u32 = 0x03C3;

// Gecko: intl/unicharutil/util/nsSpecialCasingData.cpp CaseSpecials_Title (generated from Unicode's
// SpecialCasing.txt)
static CaseSpecials_Title: [(u32, [u32; 3]); 48] = [
    (0x00df, [0x0053, 0x0073, 0x0000]), // LATIN SMALL LETTER SHARP S
    (0x0149, [0x02bc, 0x004e, 0x0000]), // LATIN SMALL LETTER N PRECEDED BY APOSTROPHE
    (0x01f0, [0x004a, 0x030c, 0x0000]), // LATIN SMALL LETTER J WITH CARON
    (0x0390, [0x03aa, 0x0301, 0x0000]), // GREEK SMALL LETTER IOTA WITH DIALYTIKA AND TONOS
    (0x03b0, [0x03ab, 0x0301, 0x0000]), // GREEK SMALL LETTER UPSILON WITH DIALYTIKA AND TONOS
    (0x0587, [0x0535, 0x0582, 0x0000]), // ARMENIAN SMALL LIGATURE ECH YIWN
    (0x1e96, [0x0048, 0x0331, 0x0000]), // LATIN SMALL LETTER H WITH LINE BELOW
    (0x1e97, [0x0054, 0x0308, 0x0000]), // LATIN SMALL LETTER T WITH DIAERESIS
    (0x1e98, [0x0057, 0x030a, 0x0000]), // LATIN SMALL LETTER W WITH RING ABOVE
    (0x1e99, [0x0059, 0x030a, 0x0000]), // LATIN SMALL LETTER Y WITH RING ABOVE
    (0x1e9a, [0x0041, 0x02be, 0x0000]), // LATIN SMALL LETTER A WITH RIGHT HALF RING
    (0x1f50, [0x03a5, 0x0313, 0x0000]), // GREEK SMALL LETTER UPSILON WITH PSILI
    (0x1f52, [0x03a5, 0x0313, 0x0300]), // GREEK SMALL LETTER UPSILON WITH PSILI AND VARIA
    (0x1f54, [0x03a5, 0x0313, 0x0301]), // GREEK SMALL LETTER UPSILON WITH PSILI AND OXIA
    (0x1f56, [0x03a5, 0x0313, 0x0342]), // GREEK SMALL LETTER UPSILON WITH PSILI AND PERISPOMENI
    (0x1fb2, [0x1fba, 0x0345, 0x0000]), // GREEK SMALL LETTER ALPHA WITH VARIA AND YPOGEGRAMMENI
    (0x1fb4, [0x0386, 0x0345, 0x0000]), // GREEK SMALL LETTER ALPHA WITH OXIA AND YPOGEGRAMMENI
    (0x1fb6, [0x0391, 0x0342, 0x0000]), // GREEK SMALL LETTER ALPHA WITH PERISPOMENI
    (0x1fb7, [0x0391, 0x0342, 0x0345]), // GREEK SMALL LETTER ALPHA WITH PERISPOMENI AND YPOGEGRAMMENI
    (0x1fc2, [0x1fca, 0x0345, 0x0000]), // GREEK SMALL LETTER ETA WITH VARIA AND YPOGEGRAMMENI
    (0x1fc4, [0x0389, 0x0345, 0x0000]), // GREEK SMALL LETTER ETA WITH OXIA AND YPOGEGRAMMENI
    (0x1fc6, [0x0397, 0x0342, 0x0000]), // GREEK SMALL LETTER ETA WITH PERISPOMENI
    (0x1fc7, [0x0397, 0x0342, 0x0345]), // GREEK SMALL LETTER ETA WITH PERISPOMENI AND YPOGEGRAMMENI
    (0x1fd2, [0x03aa, 0x0300, 0x0000]), // GREEK SMALL LETTER IOTA WITH DIALYTIKA AND VARIA
    (0x1fd3, [0x03aa, 0x0301, 0x0000]), // GREEK SMALL LETTER IOTA WITH DIALYTIKA AND OXIA
    (0x1fd6, [0x0399, 0x0342, 0x0000]), // GREEK SMALL LETTER IOTA WITH PERISPOMENI
    (0x1fd7, [0x03aa, 0x0342, 0x0000]), // GREEK SMALL LETTER IOTA WITH DIALYTIKA AND PERISPOMENI
    (0x1fe2, [0x03ab, 0x0300, 0x0000]), // GREEK SMALL LETTER UPSILON WITH DIALYTIKA AND VARIA
    (0x1fe3, [0x03ab, 0x0301, 0x0000]), // GREEK SMALL LETTER UPSILON WITH DIALYTIKA AND OXIA
    (0x1fe4, [0x03a1, 0x0313, 0x0000]), // GREEK SMALL LETTER RHO WITH PSILI
    (0x1fe6, [0x03a5, 0x0342, 0x0000]), // GREEK SMALL LETTER UPSILON WITH PERISPOMENI
    (0x1fe7, [0x03ab, 0x0342, 0x0000]), // GREEK SMALL LETTER UPSILON WITH DIALYTIKA AND PERISPOMENI
    (0x1ff2, [0x1ffa, 0x0345, 0x0000]), // GREEK SMALL LETTER OMEGA WITH VARIA AND YPOGEGRAMMENI
    (0x1ff4, [0x038f, 0x0345, 0x0000]), // GREEK SMALL LETTER OMEGA WITH OXIA AND YPOGEGRAMMENI
    (0x1ff6, [0x03a9, 0x0342, 0x0000]), // GREEK SMALL LETTER OMEGA WITH PERISPOMENI
    (0x1ff7, [0x03a9, 0x0342, 0x0345]), // GREEK SMALL LETTER OMEGA WITH PERISPOMENI AND YPOGEGRAMMENI
    (0xfb00, [0x0046, 0x0066, 0x0000]), // LATIN SMALL LIGATURE FF
    (0xfb01, [0x0046, 0x0069, 0x0000]), // LATIN SMALL LIGATURE FI
    (0xfb02, [0x0046, 0x006c, 0x0000]), // LATIN SMALL LIGATURE FL
    (0xfb03, [0x0046, 0x0066, 0x0069]), // LATIN SMALL LIGATURE FFI
    (0xfb04, [0x0046, 0x0066, 0x006c]), // LATIN SMALL LIGATURE FFL
    (0xfb05, [0x0053, 0x0074, 0x0000]), // LATIN SMALL LIGATURE LONG S T
    (0xfb06, [0x0053, 0x0074, 0x0000]), // LATIN SMALL LIGATURE ST
    (0xfb13, [0x0544, 0x0576, 0x0000]), // ARMENIAN SMALL LIGATURE MEN NOW
    (0xfb14, [0x0544, 0x0565, 0x0000]), // ARMENIAN SMALL LIGATURE MEN ECH
    (0xfb15, [0x0544, 0x056b, 0x0000]), // ARMENIAN SMALL LIGATURE MEN INI
    (0xfb16, [0x054e, 0x0576, 0x0000]), // ARMENIAN SMALL LIGATURE VEW NOW
    (0xfb17, [0x0544, 0x056d, 0x0000]), // ARMENIAN SMALL LIGATURE MEN XEH
];

// Gecko: intl/unicharutil/util/nsUnicharUtils.cpp ToUpperCase, ToLowerCase with
// unicode::SpecialUpper and SpecialLower.
// ADAPTER: Rust's full case mappings, which are SpecialCasing.txt's unconditional mappings where it
// has one and UnicodeData.txt's simple mapping otherwise: what TransformString's two steps give.
fn FullUpper(aCh: u32, aOut: &mut Vec<u32>) {
    match char::from_u32(aCh) {
        Some(c) => aOut.extend(c.to_uppercase().map(|c| c as u32)),
        None => aOut.push(aCh),
    }
}
fn FullLower(aCh: u32, aOut: &mut Vec<u32>) {
    match char::from_u32(aCh) {
        Some(c) => aOut.extend(c.to_lowercase().map(|c| c as u32)),
        None => aOut.push(aCh),
    }
}

// Gecko: intl/unicharutil/util/nsUnicharUtils.cpp ToTitleCase with unicode::SpecialTitle.
// ADAPTER: the simple titlecase mappings that are not the uppercase one (the DŽ digraphs, Georgian,
// the Greek letters with ypogegrammeni) written out; any other letter titlecases as it uppercases.
fn FullTitle(aCh: u32, aOut: &mut Vec<u32>) {
    if let Ok(i) = CaseSpecials_Title.binary_search_by_key(&aCh, |e| e.0) {
        aOut.extend(CaseSpecials_Title[i].1.iter().copied().filter(|&c| c != 0));
        return;
    }
    let simple = match aCh {
        0x01C4..=0x01C6 => Some(0x01C5),
        0x01C7..=0x01C9 => Some(0x01C8),
        0x01CA..=0x01CC => Some(0x01CB),
        0x01F1..=0x01F3 => Some(0x01F2),
        0x10D0..=0x10FA | 0x10FD..=0x10FF => Some(aCh),
        0x1F80..=0x1FAF => Some(aCh | 0x8),
        0x1FB3 | 0x1FBC => Some(0x1FBC),
        0x1FC3 | 0x1FCC => Some(0x1FCC),
        0x1FF3 | 0x1FFC => Some(0x1FFC),
        _ => None,
    };
    match simple {
        Some(c) => aOut.push(c),
        None => FullUpper(aCh, aOut),
    }
}

fn IsLetter(aCategory: GeneralCategory) -> bool {
    matches!(aCategory, GeneralCategory::Uppercase_Letter | GeneralCategory::Lowercase_Letter | GeneralCategory::Titlecase_Letter | GeneralCategory::Modifier_Letter | GeneralCategory::Other_Letter)
}
fn IsMark(aCategory: GeneralCategory) -> bool {
    matches!(aCategory, GeneralCategory::Nonspacing_Mark | GeneralCategory::Spacing_Mark | GeneralCategory::Enclosing_Mark)
}

// Gecko: layout/generic/nsTextRunTransformations.cpp nsCaseTransformTextRunFactory::TransformString,
// called with aCaseTransformsOnly true, no mask character, aUseCapitalEsZet false (the pref
// layout.css.text-transform.uppercase-eszett.enabled is off in release) and no language: `aString`
// with `aTransform` applied, appended to `aConvertedString`. `aTextRun` is the text run and the
// offset in it of `aString`'s first character, whose marks say what to capitalize; without one,
// capitalization follows nsLineBreaker::ShouldCapitalize from the start of `aString`.
// ADAPTER: the transform is one for the whole string (Gecko reads each character's from the
// transformed run's styles, unless aGlobalTransform is given). The arrays of merged and deleted
// characters are not made.
pub fn nsCaseTransformTextRunFactory_TransformString(aString: &[u16], aConvertedString: &mut Vec<u16>, aTransform: TextTransformCase, aTextRun: Option<(&gfxTextRun, u32)>) {
    let length = aString.len();
    let mut prevIsLetter = false;
    let mut sigmaIndex: Option<usize> = None;
    let mut mapped: Vec<u32> = Vec::with_capacity(3);
    let mut aOffsetInTextRun = aTextRun.map_or(0, |r| r.1);
    // If we're doing capitalization and don't have a textrun, this is the state
    // to be passed to each call to nsLineBreaker::ShouldCapitalize.
    let mut capitalizeNext = true;

    let mut i = 0;
    while i < length {
        let mut ch = aString[i] as u32;
        if i + 1 < length && IsSurrogatePair(ch, aString[i + 1] as u32) {
            ch = SurrogateToUCS4(aString[i], aString[i + 1]);
        }
        let originalCh = ch;
        mapped.clear();

        match aTransform {
            TextTransformCase::None | TextTransformCase::MathAuto => mapped.push(ch),
            TextTransformCase::Lowercase => {
                let cat = UnicodeProperties::CharType(ch);

                // If sigmaIndex is not -1, it marks where we have provisionally
                // mapped a CAPITAL SIGMA to FINAL SIGMA; if we now find another
                // letter, we need to change it to SMALL SIGMA.
                if let Some(index) = sigmaIndex {
                    if IsLetter(cat) {
                        aConvertedString[index] = GREEK_SMALL_LETTER_SIGMA as u16;
                    }
                }

                if ch == GREEK_CAPITAL_LETTER_SIGMA {
                    // If preceding char was a letter, map to FINAL instead of SMALL,
                    // and note where it occurred by setting sigmaIndex; we'll change
                    // it to standard SMALL SIGMA later if another letter follows
                    if prevIsLetter {
                        mapped.push(GREEK_SMALL_LETTER_FINAL_SIGMA);
                        sigmaIndex = Some(aConvertedString.len());
                    } else {
                        // CAPITAL SIGMA not preceded by a letter is unconditionally
                        // mapped to SMALL SIGMA
                        mapped.push(GREEK_SMALL_LETTER_SIGMA);
                        sigmaIndex = None;
                    }
                    prevIsLetter = true;
                } else {
                    // ignore diacritics for the purpose of contextual sigma mapping;
                    // otherwise, reset prevIsLetter appropriately and clear the
                    // sigmaIndex marker
                    if !IsMark(cat) {
                        prevIsLetter = IsLetter(cat);
                        sigmaIndex = None;
                    }
                    FullLower(ch, &mut mapped);
                }
            }
            TextTransformCase::Uppercase => FullUpper(ch, &mut mapped),
            TextTransformCase::Capitalize => {
                // If we have a textrun, its mCapitalize array tells us which chars
                // are to be capitalized. If not, we track the state locally, and
                // assume there's no context to be considered.
                let doCapitalize = match aTextRun {
                    Some((run, _)) => run.Capitalize(aOffsetInTextRun),
                    None => nsLineBreaker::ShouldCapitalize(ch, &mut capitalizeNext),
                };
                if doCapitalize {
                    FullTitle(ch, &mut mapped);
                } else {
                    mapped.push(ch);
                }
            }
        }

        for &c in &mapped {
            if c <= 0xFFFF {
                aConvertedString.push(c as u16);
            } else {
                aConvertedString.push(HighSurrogate(c));
                aConvertedString.push(LowSurrogate(c));
            }
        }
        if originalCh > 0xFFFF {
            // Skip the trailing surrogate.
            aOffsetInTextRun += 1;
            i += 1;
        }
        aOffsetInTextRun += 1;
        i += 1;
    }
}

// Firefox has no gtest for TransformString; these are written from SpecialCasing.txt and the
// comments of nsCaseTransformTextRunFactory::TransformString and nsLineBreaker::ShouldCapitalize.
#[cfg(test)]
mod tests {
    use super::*;

    fn transform(aText: &str, aTransform: TextTransformCase) -> String {
        let source: Vec<u16> = aText.encode_utf16().collect();
        let mut out = Vec::new();
        nsCaseTransformTextRunFactory_TransformString(&source, &mut out, aTransform, None);
        String::from_utf16(&out).unwrap()
    }

    #[test]
    fn upper_and_lower() {
        assert_eq!(transform("Straße ŉ 𐐨", TextTransformCase::Uppercase), "STRASSE ʼN 𐐀");
        assert_eq!(transform("İSTANBUL", TextTransformCase::Lowercase), "i\u{307}stanbul");
        assert_eq!(transform("abc", TextTransformCase::None), "abc");
    }

    #[test]
    fn final_sigma() {
        // final when a letter precedes and none follows; diacritics do not count
        assert_eq!(transform("ΟΔΟΣ ΣΑ Σ ΑΣ\u{301}.", TextTransformCase::Lowercase), "οδος σα σ ας\u{301}.");
    }

    #[test]
    fn capitalize() {
        assert_eq!(transform("hello world-wide it's l·l “quoted” ǆungla ﬁsh ßa 3rd", TextTransformCase::Capitalize), "Hello World-Wide It's L·l “Quoted” ǅungla Fish Ssa 3rd");
    }
}
