// ADAPTER (not a translation of a whole file): the parts of intl/components/src/UnicodeProperties.h
// that the line-breaking and text code use.
//
// In Gecko `mozilla::intl::UnicodeProperties` answers through ICU4C's u_* functions
// (`u_charType`, `u_getIntPropertyValue`, `u_hasBinaryProperty`, `uscript_getScript`). Here the
// answers come from the `icu_properties` crate (Firefox vendors it in third_party/rust, with
// `compiled_data`), which has the same Unicode data. Names and the meaning of each function are Gecko's.
use crate::*;
use icu_properties::props::{
    DefaultIgnorableCodePoint, EastAsianWidth, Emoji, GeneralCategory as IcuGeneralCategory, Script as IcuScript,
};
use icu_properties::{CodePointMapData, CodePointSetData};

// Gecko: intl/components/src/UnicodeProperties.h mozilla::intl::GeneralCategory (the ICU UCharCategory
// values, with their Gecko spelling).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GeneralCategory {
    Unassigned,
    Uppercase_Letter,
    Lowercase_Letter,
    Titlecase_Letter,
    Modifier_Letter,
    Other_Letter,
    Nonspacing_Mark,
    Enclosing_Mark,
    Spacing_Mark,
    Decimal_Number,
    Letter_Number,
    Other_Number,
    Space_Separator,
    Line_Separator,
    Paragraph_Separator,
    Control,
    Format,
    Private_Use,
    Surrogate,
    Dash_Punctuation,
    Open_Punctuation,
    Close_Punctuation,
    Connector_Punctuation,
    Other_Punctuation,
    Math_Symbol,
    Currency_Symbol,
    Modifier_Symbol,
    Other_Symbol,
    Initial_Punctuation,
    Final_Punctuation,
}

// Gecko: intl/components/src/UnicodeProperties.h mozilla::intl::Script (only the values used)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Script {
    THAI,
    MYANMAR,
    KHMER,
    JAVANESE,
    BALINESE,
    SUNDANESE,
    LAO,
    HANGUL,
    OTHER,
}

// Gecko: intl/components/src/UnicodeProperties.h mozilla::intl::UnicodeProperties
pub struct UnicodeProperties;

impl UnicodeProperties {
    /**
     * Get the Unicode general category of aCh (u_charType).
     */
    // Gecko: intl/components/src/UnicodeProperties.h UnicodeProperties::CharType
    pub fn CharType(aCh: u32) -> GeneralCategory {
        // ADAPTER: icu_properties instead of ICU4C u_charType.
        match CodePointMapData::<IcuGeneralCategory>::new().get32(aCh) {
            IcuGeneralCategory::Unassigned => GeneralCategory::Unassigned,
            IcuGeneralCategory::UppercaseLetter => GeneralCategory::Uppercase_Letter,
            IcuGeneralCategory::LowercaseLetter => GeneralCategory::Lowercase_Letter,
            IcuGeneralCategory::TitlecaseLetter => GeneralCategory::Titlecase_Letter,
            IcuGeneralCategory::ModifierLetter => GeneralCategory::Modifier_Letter,
            IcuGeneralCategory::OtherLetter => GeneralCategory::Other_Letter,
            IcuGeneralCategory::NonspacingMark => GeneralCategory::Nonspacing_Mark,
            IcuGeneralCategory::EnclosingMark => GeneralCategory::Enclosing_Mark,
            IcuGeneralCategory::SpacingMark => GeneralCategory::Spacing_Mark,
            IcuGeneralCategory::DecimalNumber => GeneralCategory::Decimal_Number,
            IcuGeneralCategory::LetterNumber => GeneralCategory::Letter_Number,
            IcuGeneralCategory::OtherNumber => GeneralCategory::Other_Number,
            IcuGeneralCategory::SpaceSeparator => GeneralCategory::Space_Separator,
            IcuGeneralCategory::LineSeparator => GeneralCategory::Line_Separator,
            IcuGeneralCategory::ParagraphSeparator => GeneralCategory::Paragraph_Separator,
            IcuGeneralCategory::Control => GeneralCategory::Control,
            IcuGeneralCategory::Format => GeneralCategory::Format,
            IcuGeneralCategory::PrivateUse => GeneralCategory::Private_Use,
            IcuGeneralCategory::Surrogate => GeneralCategory::Surrogate,
            IcuGeneralCategory::DashPunctuation => GeneralCategory::Dash_Punctuation,
            IcuGeneralCategory::OpenPunctuation => GeneralCategory::Open_Punctuation,
            IcuGeneralCategory::ClosePunctuation => GeneralCategory::Close_Punctuation,
            IcuGeneralCategory::ConnectorPunctuation => GeneralCategory::Connector_Punctuation,
            IcuGeneralCategory::OtherPunctuation => GeneralCategory::Other_Punctuation,
            IcuGeneralCategory::MathSymbol => GeneralCategory::Math_Symbol,
            IcuGeneralCategory::CurrencySymbol => GeneralCategory::Currency_Symbol,
            IcuGeneralCategory::ModifierSymbol => GeneralCategory::Modifier_Symbol,
            IcuGeneralCategory::OtherSymbol => GeneralCategory::Other_Symbol,
            IcuGeneralCategory::InitialPunctuation => GeneralCategory::Initial_Punctuation,
            IcuGeneralCategory::FinalPunctuation => GeneralCategory::Final_Punctuation,
        }
    }

    /**
     * Check if the width of aCh is full width, half width or wide.
     */
    // Gecko: intl/components/src/UnicodeProperties.h UnicodeProperties::IsEastAsianWidthFHW
    pub fn IsEastAsianWidthFHW(aCh: u32) -> bool {
        match CodePointMapData::<EastAsianWidth>::new().get32(aCh) {
            EastAsianWidth::Fullwidth | EastAsianWidth::Halfwidth | EastAsianWidth::Wide => true,
            _ => false,
        }
    }

    /**
     * Check if the width of aCh is full width, half width or wide
     * excluding emoji.
     */
    // Gecko: intl/components/src/UnicodeProperties.h UnicodeProperties::IsEastAsianWidthFHWexcludingEmoji
    pub fn IsEastAsianWidthFHWexcludingEmoji(aCh: u32) -> bool {
        match CodePointMapData::<EastAsianWidth>::new().get32(aCh) {
            EastAsianWidth::Fullwidth | EastAsianWidth::Halfwidth => true,
            EastAsianWidth::Wide => !CodePointSetData::new::<Emoji>().contains32(aCh),
            _ => false,
        }
    }

    // Gecko: intl/components/src/UnicodeProperties.h UnicodeProperties::HasBinaryProperty(DefaultIgnorableCodePoint)
    pub fn IsDefaultIgnorableCodePoint(aCh: u32) -> bool {
        CodePointSetData::new::<DefaultIgnorableCodePoint>().contains32(aCh)
    }

    // Gecko: intl/components/src/UnicodeProperties.h UnicodeProperties::GetScriptCode
    pub fn GetScriptCode(aCh: u32) -> Script {
        let sc = CodePointMapData::<IcuScript>::new().get32(aCh);
        if sc == IcuScript::Thai {
            Script::THAI
        } else if sc == IcuScript::Myanmar {
            Script::MYANMAR
        } else if sc == IcuScript::Khmer {
            Script::KHMER
        } else if sc == IcuScript::Javanese {
            Script::JAVANESE
        } else if sc == IcuScript::Balinese {
            Script::BALINESE
        } else if sc == IcuScript::Sundanese {
            Script::SUNDANESE
        } else if sc == IcuScript::Lao {
            Script::LAO
        } else if sc == IcuScript::Hangul {
            Script::HANGUL
        } else {
            Script::OTHER
        }
    }

    // Gecko: intl/components/src/UnicodeProperties.h UnicodeProperties::IsScriptioContinua
    pub fn IsScriptioContinua(aChar: u16) -> bool {
        let sc = UnicodeProperties::GetScriptCode(aChar as u32);
        sc == Script::THAI
            || sc == Script::MYANMAR
            || sc == Script::KHMER
            || sc == Script::JAVANESE
            || sc == Script::BALINESE
            || sc == Script::SUNDANESE
            || sc == Script::LAO
    }

    /**
     * Check if the CharType of aCh is punctuation.
     */
    // Gecko: intl/components/src/UnicodeProperties.h UnicodeProperties::IsPunctuation
    pub fn IsPunctuation(aCh: u32) -> bool {
        match UnicodeProperties::CharType(aCh) {
            GeneralCategory::Dash_Punctuation
            | GeneralCategory::Open_Punctuation
            | GeneralCategory::Close_Punctuation
            | GeneralCategory::Connector_Punctuation
            | GeneralCategory::Other_Punctuation
            | GeneralCategory::Initial_Punctuation
            | GeneralCategory::Final_Punctuation => true,
            _ => false,
        }
    }
}
