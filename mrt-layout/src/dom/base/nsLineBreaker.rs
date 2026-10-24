// Gecko: dom/base/nsLineBreaker.h, dom/base/nsLineBreaker.cpp
use crate::*;

// NOT PORTED: hyphenation (nsHyphenator, nsHyphenationManager, intl/hyphenation). A line breaker
// never finds a hyphenation point here: `hyphenator` is always null, BREAK_USE_AUTO_HYPHENATION is
// accepted and ignored, and FindHyphenationPoints is a stub. The mHyphenateLimit* members stay.
// Gecko: intl/hyphenation/glue/nsHyphenator.h nsHyphenator
pub struct nsHyphenator;

// Gecko: gfx/thebes/gfxFont.h gfxShapedText::CompressedGlyph::FLAG_BREAK_TYPE_*
pub mod gfxTextRun_CompressedGlyph {
    pub const FLAG_BREAK_TYPE_NONE: u8 = 0;
    pub const FLAG_BREAK_TYPE_NORMAL: u8 = 1;
    pub const FLAG_BREAK_TYPE_HYPHEN: u8 = 2;
}

/**
 * A receiver of line break data.
 */
// Gecko: dom/base/nsLineBreaker.h nsILineBreakSink
// (the abstract class becomes a trait; the sink pointer that nsLineBreaker keeps is a raw
// `*mut dyn nsILineBreakSink`, as in C++ the sink must outlive the calls that reference it)
pub trait nsILineBreakSink {
    /**
     * Sets the break data for a substring of the associated text chunk.
     * One or more of these calls will be performed; the union of all substrings
     * will cover the entire text chunk. Substrings may overlap (i.e., we may
     * set the break-before state of a character more than once).
     * @param aBreakBefore the break-before states for the characters in the
     * substring. These are enum values from gfxTextRun::CompressedGlyph:
     *    FLAG_BREAK_TYPE_NONE     - no linebreak is allowed here
     *    FLAG_BREAK_TYPE_NORMAL   - a normal (whitespace) linebreak
     *    FLAG_BREAK_TYPE_HYPHEN   - a hyphenation point
     */
    fn SetBreaks(&mut self, aStart: u32, aLength: u32, aBreakBefore: &mut [u8]);

    /**
     * Indicates which characters should be capitalized. Only called if
     * BREAK_NEED_CAPITALIZATION was requested.
     */
    fn SetCapitalization(&mut self, aStart: u32, aLength: u32, aCapitalize: &mut [bool]);
}

// A `nsILineBreakSink*` (null is None).
pub type nsILineBreakSinkPtr = Option<*mut dyn nsILineBreakSink>;

// There is no break opportunity between any pair of characters that has line
// break class of either AL (Alphabetic), IS (Infix Numeric Separator), NU
// (Numeric), or QU (Quotation). See
// https://www.unicode.org/Public/UCD/latest/ucd/LineBreak.txt for Unicode code
// point and line break class mapping.
const kNonBreakableASCII: [u8; 96] = [
    // 0x20-0x2f
    0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0,
    // 0x30-0x3f
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0,
    // 0x40-0x4f
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    // 0x50-0x5f
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1,
    // 0x60-0x6f
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    // 0x70-0x7f
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0,
];

// Gecko: dom/base/nsLineBreaker.cpp IsNonBreakableChar<T>
// (the C++ template over char16_t / uint8_t is a generic over `Into<u32>`)
fn IsNonBreakableChar<T: Into<u32> + Copy>(aChar: T) -> bool {
    let aChar: u32 = aChar.into();
    if aChar < 0x20 || aChar > 0x7f {
        return false;
    }
    kNonBreakableASCII[(aChar - 0x20) as usize] != 0
}

// State for the nonwhitespace "word" that started in previous text and hasn't
// finished yet.
// This is a list of text sources that make up the "current word" (i.e.,
// run of text which does not contain any whitespace). All the mLengths
// are are nonzero, these cannot overlap.
// Gecko: dom/base/nsLineBreaker.h nsLineBreaker::TextItem
pub struct nsLineBreaker_TextItem {
    mSink: nsILineBreakSinkPtr,
    mSinkOffset: u32,
    mLength: u32,
    mFlags: u32,
}

impl nsLineBreaker_TextItem {
    pub fn new(aSink: nsILineBreakSinkPtr, aSinkOffset: u32, aLength: u32, aFlags: u32) -> Self {
        nsLineBreaker_TextItem { mSink: aSink, mSinkOffset: aSinkOffset, mLength: aLength, mFlags: aFlags }
    }
}

/**
 * A line-breaking state machine. You feed text into it via AppendText calls
 * and it computes the possible line breaks. Because break decisions can
 * require a lot of context, the breaks for a piece of text are sometimes not
 * known until later text has been seen (or all text ends). So breaks are
 * returned via a call to SetBreaks on the nsILineBreakSink object passed
 * with each text chunk, which might happen during the corresponding AppendText
 * call, or might happen during a later AppendText call or even a Reset()
 * call.
 *
 * The linebreak results MUST NOT depend on how the text is broken up
 * into AppendText calls.
 *
 * The current strategy is that we break the overall text into
 * whitespace-delimited "words". Then those words are passed to the LineBreaker
 * for deeper analysis if they might contain breakable characters.
 *
 * This class also handles detection of which characters should be capitalized
 * for text-transform:capitalize. This is a good place to handle that because
 * we have all the context we need.
 */
// Gecko: dom/base/nsLineBreaker.h nsLineBreaker
pub struct nsLineBreaker {
    mCurrentWord: Vec<u16>,
    // All the items that contribute to mCurrentWord
    mTextItems: Vec<nsLineBreaker_TextItem>,
    mCurrentWordLanguage: Option<nsAtom>,

    // Constraints from CSS `hyphenate-limit-chars` property, to block the use of
    // auto-hyphenation if the word is too short, or at positions too near the
    // beginning/end of the word.
    // (Note that per CSS Text spec, these counts ignore combining marks, etc.,
    // so they are not purely codepoint or character counts.)
    // (Zero values would have no effect; but text-frame code will update the
    // values from CSS before calling the line-breaker.)
    mHyphenateLimitWord: u8,  // Min word length to auto-hyphenate
    mHyphenateLimitStart: u8, // Min number of chars before the break
    mHyphenateLimitEnd: u8,   // Min number of chars after the break

    mCurrentWordContainsMixedLang: bool,
    mCurrentWordMightBeBreakable: bool,
    mScriptIsChineseOrJapanese: bool,

    // True if the previous character was breakable whitespace
    mAfterBreakableSpace: bool,
    // True if a break must be allowed at the current position because
    // a run of breakable whitespace ends here
    mBreakHere: bool,
    // Break rules for letters from the "word-break" property.
    mWordBreak: WordBreakRule,
    // Line breaking strictness from the "line-break" property.
    mLineBreak: LineBreakRule,
    // Should the text be treated as continuing a word-in-progress (for purposes
    // of initial capitalization)? Normally this is set to false whenever we
    // start using a linebreaker, but it may be set to true if the line-breaker
    // has been explicitly flushed mid-word.
    mWordContinuation: bool,
}

// If the aFlags parameter to AppendText has all these bits set,
// then we don't need to worry about finding break opportunities
// in the appended text.
// Gecko: dom/base/nsLineBreaker.cpp NO_BREAKS_NEEDED_FLAGS
const NO_BREAKS_NEEDED_FLAGS: u32 = nsLineBreaker::BREAK_SUPPRESS_INITIAL
    | nsLineBreaker::BREAK_SUPPRESS_INSIDE
    | nsLineBreaker::BREAK_SKIP_SETTING_NO_BREAKS;

impl Default for nsLineBreaker {
    // Gecko: dom/base/nsLineBreaker.h nsLineBreaker::nsLineBreaker
    fn default() -> Self {
        nsLineBreaker {
            mCurrentWord: Vec::new(),
            mTextItems: Vec::new(),
            mCurrentWordLanguage: None,
            mHyphenateLimitWord: 0,
            mHyphenateLimitStart: 0,
            mHyphenateLimitEnd: 0,
            mCurrentWordContainsMixedLang: false,
            mCurrentWordMightBeBreakable: false,
            mScriptIsChineseOrJapanese: false,
            mAfterBreakableSpace: false,
            mBreakHere: false,
            mWordBreak: WordBreakRule::Normal,
            mLineBreak: LineBreakRule::Auto,
            mWordContinuation: false,
        }
    }
}

impl Drop for nsLineBreaker {
    // Gecko: dom/base/nsLineBreaker.cpp nsLineBreaker::~nsLineBreaker
    fn drop(&mut self) {
        if !std::thread::panicking() {
            NS_ASSERTION!(self.mCurrentWord.len() == 0, "Should have Reset() before destruction!");
        }
    }
}

impl nsLineBreaker {
    /**
     * Flags passed with each chunk of text.
     */
    /*
     * Do not introduce a break opportunity at the start of this chunk of text.
     */
    pub const BREAK_SUPPRESS_INITIAL: u32 = 0x01;
    /**
     * Do not introduce a break opportunity in the interior of this chunk of
     * text. Also, whitespace in this chunk is treated as non-breakable.
     */
    pub const BREAK_SUPPRESS_INSIDE: u32 = 0x02;
    /**
     * The sink currently is already set up to have no breaks in it;
     * if no breaks are possible, nsLineBreaker does not need to call
     * SetBreaks on it. This is useful when handling large quantities of
     * preformatted text; the textruns will never have any breaks set on them,
     * and there is no need to ever actually scan the text for breaks, except
     * at the end of textruns in case context is needed for following breakable
     * text.
     */
    pub const BREAK_SKIP_SETTING_NO_BREAKS: u32 = 0x04;
    /**
     * We need to be notified of characters that should be capitalized
     * (as in text-transform:capitalize) in this chunk of text.
     */
    pub const BREAK_NEED_CAPITALIZATION: u32 = 0x08;
    /**
     * Auto-hyphenation is enabled, so we need to get a hyphenator
     * (if available) and use it to find breakpoints.
     */
    pub const BREAK_USE_AUTO_HYPHENATION: u32 = 0x10;

    // Gecko: dom/base/nsLineBreaker.h nsLineBreaker::IsSpace
    pub fn IsSpace(u: u16) -> bool {
        NS_IsSpace(u)
    }

    // Helper also used by nsCaseTransformTextRunFactory::TransformString.
    // aChar is the current character to be examined;
    // aCapitalizeNext is a state variable: initialize it to true at start-of-
    // text, then pass it back to this function as each successive character is
    // considered.
    // Gecko: dom/base/nsLineBreaker.cpp nsLineBreaker::ShouldCapitalize
    pub fn ShouldCapitalize(aChar: u32, aCapitalizeNext: &mut bool) -> bool {
        let category = UnicodeProperties::CharType(aChar);
        match category {
            GeneralCategory::Uppercase_Letter
            | GeneralCategory::Lowercase_Letter
            | GeneralCategory::Titlecase_Letter
            | GeneralCategory::Modifier_Letter
            | GeneralCategory::Other_Letter
            | GeneralCategory::Decimal_Number
            | GeneralCategory::Letter_Number
            | GeneralCategory::Other_Number => {
                if *aCapitalizeNext {
                    *aCapitalizeNext = false;
                    return true;
                }
            }
            GeneralCategory::Space_Separator
            | GeneralCategory::Line_Separator
            | GeneralCategory::Paragraph_Separator
            | GeneralCategory::Dash_Punctuation
            | GeneralCategory::Initial_Punctuation => {
                /* These punctuation categories are excluded, for examples like
                 *   "what colo[u]r" -> "What Colo[u]r?" (rather than "What Colo[U]R?")
                 * and
                 *   "snake_case" -> "Snake_case" (to match word selection behavior)
                case GeneralCategory::Open_Punctuation:
                case GeneralCategory::Close_Punctuation:
                case GeneralCategory::Connector_Punctuation:
                 */
                *aCapitalizeNext = true;
            }
            GeneralCategory::Final_Punctuation => {
                /* Special-case: exclude Unicode single-close-quote/apostrophe,
                   for examples like "Lowe’s" etc. */
                if aChar != 0x2019 {
                    *aCapitalizeNext = true;
                }
            }
            GeneralCategory::Other_Punctuation => {
                /* Special-case: exclude ASCII apostrophe, for "Lowe's" etc.,
                   and MIDDLE DOT, for Catalan "l·l". */
                if aChar != '\'' as u32 && aChar != 0x00B7 {
                    *aCapitalizeNext = true;
                }
            }
            _ => {}
        }
        false
    }

}

// Gecko: dom/base/nsLineBreaker.cpp SetupCapitalization
fn SetupCapitalization(aWord: &[u16], aLength: u32, aCapitalization: &mut [bool]) {
    // Capitalize the first alphanumeric character after a space or punctuation.
    let mut capitalizeNextChar = true;
    let mut i: u32 = 0;
    while i < aLength {
        let mut ch: u32 = aWord[i as usize] as u32;
        if i + 1 < aLength && IsSurrogatePair(ch, aWord[(i + 1) as usize] as u32) {
            ch = SurrogateToUCS4(aWord[i as usize], aWord[(i + 1) as usize]);
        }
        aCapitalization[i as usize] = nsLineBreaker::ShouldCapitalize(ch, &mut capitalizeNextChar);

        if !IsInBMP(ch) {
            i += 1;
        }
        i += 1;
    }
}

impl nsLineBreaker {
    /*
     * Set word-break mode for line breaker. This is set by word-break property.
     */
    // Gecko: dom/base/nsLineBreaker.h nsLineBreaker::SetWordBreak
    pub fn SetWordBreak(&mut self, aMode: WordBreakRule) {
        // If current word is non-empty and mode is changing, flush the breaker.
        if aMode != self.mWordBreak && !self.mCurrentWord.is_empty() {
            let rv = self.FlushCurrentWord();
            if NS_FAILED(rv) {
                NS_WARNING!("FlushCurrentWord failed, line-breaks may be wrong");
            }
            // If previous mode was break-all, we should allow a break here.
            // XXX (jfkthame) css-text spec seems unclear on this, raised question in
            // https://github.com/w3c/csswg-drafts/issues/3897
            if self.mWordBreak == WordBreakRule::BreakAll {
                self.mBreakHere = true;
            }
        }
        self.mWordBreak = aMode;
    }

    /*
     * Set line-break rule strictness mode for line breaker. This is set by the
     * line-break property.
     */
    // Gecko: dom/base/nsLineBreaker.h nsLineBreaker::SetStrictness
    pub fn SetStrictness(&mut self, aMode: LineBreakRule) {
        if aMode != self.mLineBreak && !self.mCurrentWord.is_empty() {
            let rv = self.FlushCurrentWord();
            if NS_FAILED(rv) {
                NS_WARNING!("FlushCurrentWord failed, line-breaks may be wrong");
            }
            // If previous mode was anywhere, we should allow a break here.
            if self.mLineBreak == LineBreakRule::Anywhere {
                self.mBreakHere = true;
            }
        }
        self.mLineBreak = aMode;
    }

    /**
     * Return whether the line-breaker has a buffered "current word" that may
     * be extended with additional word-forming characters.
     */
    // Gecko: dom/base/nsLineBreaker.h nsLineBreaker::InWord
    pub fn InWord(&self) -> bool {
        !self.mCurrentWord.is_empty()
    }

    /**
     * Set the word-continuation state, which will suppress capitalization of
     * the next letter that might otherwise apply.
     */
    // Gecko: dom/base/nsLineBreaker.h nsLineBreaker::SetWordContinuation
    pub fn SetWordContinuation(&mut self, aContinuation: bool) {
        self.mWordContinuation = aContinuation;
    }

    /**
     * Set the hyphenate-limit-chars values. Values are clamped to be <= 255.
     */
    // Gecko: dom/base/nsLineBreaker.h nsLineBreaker::SetHyphenateLimitChars
    pub fn SetHyphenateLimitChars(&mut self, aWordLength: u32, aStartLength: u32, aEndLength: u32) {
        self.mHyphenateLimitWord = std::cmp::min(255u32, aWordLength) as u8;
        self.mHyphenateLimitStart = std::cmp::min(255u32, aStartLength) as u8;
        self.mHyphenateLimitEnd = std::cmp::min(255u32, aEndLength) as u8;
    }

    // Gecko: dom/base/nsLineBreaker.h nsLineBreaker::IsSegmentSpace
    fn IsSegmentSpace(&self, u: u16) -> bool {
        u == 0x0020 ||  // SPACE u
        u == 0x0009 ||  // CHARACTER TABULATION
        u == 0x000D     // CARRIAGE RETURN
    }

    // When the current word ends, this computes the linebreak opportunities
    // *inside* the word (excluding either end) and sets them through the
    // appropriate sink(s). Then we clear the current word state.
    // Gecko: dom/base/nsLineBreaker.cpp nsLineBreaker::FlushCurrentWord
    fn FlushCurrentWord(&mut self) -> nsresult {
        // auto cleanup = mozilla::MakeScopeExit([&] { ... });
        // The body runs in a closure and the clean-up below runs after it, whichever way it returns.
        let rv: nsresult = (|| -> nsresult {
            let length = self.mCurrentWord.len() as u32;
            let mut breakState: Vec<u8> = vec![0; length as usize];

            if self.mLineBreak == LineBreakRule::Anywhere {
                breakState.fill(gfxTextRun_CompressedGlyph::FLAG_BREAK_TYPE_NORMAL);
            } else if !self.mCurrentWordMightBeBreakable && self.mWordBreak != WordBreakRule::BreakAll {
                // word-break: normal or keep-all has no break opportunity if the word
                // is non-breakable. (See the comment of kNonBreakableASCII).
                breakState.fill(gfxTextRun_CompressedGlyph::FLAG_BREAK_TYPE_NONE);
            } else {
                LineBreaker::ComputeBreakPositions__char16_t_ptr_uint32_t_WordBreakRule_LineBreakRule_bool_uint8_t_ptr(
                    &self.mCurrentWord,
                    length,
                    self.mWordBreak,
                    self.mLineBreak,
                    self.mScriptIsChineseOrJapanese,
                    &mut breakState,
                );
            }

            let mut autoHyphenate = self.mCurrentWordLanguage.is_some() && !self.mCurrentWordContainsMixedLang;
            let mut i: usize = 0;
            while autoHyphenate && i < self.mTextItems.len() {
                let ti = &self.mTextItems[i];
                if (ti.mFlags & nsLineBreaker::BREAK_USE_AUTO_HYPHENATION) == 0 {
                    autoHyphenate = false;
                }
                i += 1;
            }
            if autoHyphenate {
                // NOT PORTED: hyphenation.
                //   RefPtr<nsHyphenator> hyphenator =
                //       nsHyphenationManager::Instance()->GetHyphenator(mCurrentWordLanguage);
                //   if (hyphenator) {
                //     FindHyphenationPoints(hyphenator, mCurrentWord.Elements(),
                //                           mCurrentWord.Elements() + length, breakState.Elements());
                //   }
            }

            let mut capitalizationState: Vec<bool> = Vec::new();
            let mut offset: u32 = 0;
            for i in 0..self.mTextItems.len() {
                let ti = &self.mTextItems[i];
                NS_ASSERTION!(ti.mLength > 0, "Zero length word contribution?");

                if (ti.mFlags & nsLineBreaker::BREAK_SUPPRESS_INITIAL) != 0 && ti.mSinkOffset == 0 {
                    breakState[offset as usize] = gfxTextRun_CompressedGlyph::FLAG_BREAK_TYPE_NONE;
                }
                if (ti.mFlags & nsLineBreaker::BREAK_SUPPRESS_INSIDE) != 0 {
                    let exclude: u32 = if ti.mSinkOffset == 0 { 1 } else { 0 };
                    let from = (offset + exclude) as usize;
                    let to = from + (ti.mLength - exclude) as usize;
                    breakState[from..to].fill(gfxTextRun_CompressedGlyph::FLAG_BREAK_TYPE_NONE);
                }

                // Don't set the break state for the first character of the word, because
                // it was already set correctly earlier and we don't know what the true
                // value should be.
                let skipSet: u32 = if i == 0 { 1 } else { 0 };
                if let Some(sink) = ti.mSink {
                    let sink = unsafe { &mut *sink };
                    let from = (offset + skipSet) as usize;
                    let to = (offset + ti.mLength) as usize;
                    sink.SetBreaks(ti.mSinkOffset + skipSet, ti.mLength - skipSet, &mut breakState[from..to]);

                    if !self.mWordContinuation && (ti.mFlags & nsLineBreaker::BREAK_NEED_CAPITALIZATION) != 0 {
                        if capitalizationState.len() == 0 {
                            capitalizationState = vec![false; length as usize];
                            SetupCapitalization(&self.mCurrentWord, length, &mut capitalizationState);
                        }
                        let from = offset as usize;
                        let to = (offset + ti.mLength) as usize;
                        sink.SetCapitalization(ti.mSinkOffset, ti.mLength, &mut capitalizationState[from..to]);
                    }
                }

                offset += ti.mLength;
            }

            NS_OK
        })();

        self.mCurrentWord.clear();
        self.mTextItems.clear();
        self.mCurrentWordMightBeBreakable = false;
        self.mCurrentWordContainsMixedLang = false;
        self.mCurrentWordLanguage = None;
        self.mWordContinuation = false;
        rv
    }

    /**
     * Append "invisible whitespace". This acts like whitespace, but there is
     * no actual text associated with it. Only the BREAK_SUPPRESS_INSIDE flag
     * is relevant here.
     */
    // Gecko: dom/base/nsLineBreaker.cpp nsLineBreaker::AppendInvisibleWhitespace
    pub fn AppendInvisibleWhitespace(&mut self, aFlags: u32) -> nsresult {
        let rv = self.FlushCurrentWord();
        if NS_FAILED(rv) {
            return rv;
        }

        let isBreakableSpace = (aFlags & nsLineBreaker::BREAK_SUPPRESS_INSIDE) == 0;
        if self.mAfterBreakableSpace && !isBreakableSpace {
            self.mBreakHere = true;
        }
        self.mAfterBreakableSpace = isBreakableSpace;
        self.mWordContinuation = false;
        NS_OK
    }

    /**
     * Feed Unicode text into the linebreaker for analysis. aLength must be
     * nonzero.
     * @param aSink can be null if the breaks are not actually needed (we may
     * still be setting up state for later breaks)
     */
    // Gecko: dom/base/nsLineBreaker.cpp nsLineBreaker::AppendText(nsAtom*, const char16_t*, ...)
    pub fn AppendText__nsAtom_ptr_char16_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(
        &mut self,
        aHyphenationLanguage: Option<nsAtom>,
        aText: &[u16],
        aLength: u32,
        aFlags: u32,
        aSink: nsILineBreakSinkPtr,
    ) -> nsresult {
        NS_ASSERTION!(aLength > 0, "Appending empty text...");

        let mut offset: u32 = 0;

        // Continue the current word
        if self.mCurrentWord.len() > 0 {
            NS_ASSERTION!(!self.mAfterBreakableSpace && !self.mBreakHere, "These should not be set");

            while offset < aLength && !self.IsSegmentSpace(aText[offset as usize]) {
                self.mCurrentWord.push(aText[offset as usize]);
                if !self.mCurrentWordMightBeBreakable && !IsNonBreakableChar::<u16>(aText[offset as usize]) {
                    self.mCurrentWordMightBeBreakable = true;
                }
                self.UpdateCurrentWordLanguage(aHyphenationLanguage);
                offset += 1;
            }

            if offset > 0 {
                self.mTextItems.push(nsLineBreaker_TextItem::new(aSink, 0, offset, aFlags));
            }

            if offset == aLength {
                return NS_OK;
            }

            // We encountered whitespace, so we're done with this word
            let rv = self.FlushCurrentWord();
            if NS_FAILED(rv) {
                return rv;
            }
        }

        let mut breakState: Vec<u8> = Vec::new();
        if aSink.is_some() {
            breakState = vec![0; aLength as usize];
        }

        let mut noCapitalizationNeeded = true;
        let mut capitalizationState: Vec<bool> = Vec::new();
        if aSink.is_some() && (aFlags & nsLineBreaker::BREAK_NEED_CAPITALIZATION) != 0 {
            capitalizationState = vec![false; aLength as usize];
            noCapitalizationNeeded = false;
        }

        let start = offset;
        let noBreaksNeeded = aSink.is_none()
            || ((aFlags & NO_BREAKS_NEEDED_FLAGS) == NO_BREAKS_NEEDED_FLAGS && !self.mBreakHere && !self.mAfterBreakableSpace);
        if noBreaksNeeded && noCapitalizationNeeded {
            // Skip to the space before the last word, since either the break data
            // here is not needed, or no breaks are set in the sink and there cannot
            // be any breaks in this chunk; and we don't need to do word-initial
            // capitalization. All we need is the context for the next chunk (if any).
            offset = aLength;
            while offset > start {
                offset -= 1;
                if self.IsSegmentSpace(aText[offset as usize]) {
                    break;
                }
            }
        }
        let mut wordStart = offset;
        let mut wordMightBeBreakable = false;

        // NOT PORTED: hyphenation.
        //   RefPtr<nsHyphenator> hyphenator;
        //   if ((aFlags & BREAK_USE_AUTO_HYPHENATION) && !(aFlags & BREAK_SUPPRESS_INSIDE) && aHyphenationLanguage) {
        //     hyphenator = nsHyphenationManager::Instance()->GetHyphenator(aHyphenationLanguage);
        //   }
        let hyphenator: Option<&nsHyphenator> = None;

        loop {
            let ch: u16 = aText[offset as usize];
            let isSpace = self.IsSegmentSpace(ch);
            let isBreakableSpace = isSpace && (aFlags & nsLineBreaker::BREAK_SUPPRESS_INSIDE) == 0;

            if aSink.is_some() && !noBreaksNeeded {
                breakState[offset as usize] = if self.mBreakHere
                    || (self.mAfterBreakableSpace && !isBreakableSpace)
                    || self.mWordBreak == WordBreakRule::BreakAll
                    || self.mLineBreak == LineBreakRule::Anywhere
                {
                    gfxTextRun_CompressedGlyph::FLAG_BREAK_TYPE_NORMAL
                } else {
                    gfxTextRun_CompressedGlyph::FLAG_BREAK_TYPE_NONE
                };
            }
            self.mBreakHere = false;
            self.mAfterBreakableSpace = isBreakableSpace;

            if isSpace || ch == '\n' as u16 {
                if offset > wordStart && aSink.is_some() {
                    if (aFlags & nsLineBreaker::BREAK_SUPPRESS_INSIDE) == 0 {
                        if self.mLineBreak == LineBreakRule::Anywhere {
                            breakState[wordStart as usize..offset as usize].fill(gfxTextRun_CompressedGlyph::FLAG_BREAK_TYPE_NORMAL);
                        } else if wordMightBeBreakable {
                            // Save current start-of-word state because ComputeBreakPositions()
                            // will set it to false.
                            // AutoRestore<uint8_t> saveWordStartBreakState(breakState[wordStart]);
                            let saveWordStartBreakState = breakState[wordStart as usize];
                            LineBreaker::ComputeBreakPositions__char16_t_ptr_uint32_t_WordBreakRule_LineBreakRule_bool_uint8_t_ptr(
                                &aText[wordStart as usize..],
                                offset - wordStart,
                                self.mWordBreak,
                                self.mLineBreak,
                                self.mScriptIsChineseOrJapanese,
                                &mut breakState[wordStart as usize..],
                            );
                            breakState[wordStart as usize] = saveWordStartBreakState;
                        }
                        if let Some(hyphenator) = hyphenator {
                            self.FindHyphenationPoints(
                                hyphenator,
                                &aText[wordStart as usize..offset as usize],
                                &mut breakState[wordStart as usize..],
                            );
                        }
                    }
                    if !self.mWordContinuation && !noCapitalizationNeeded {
                        SetupCapitalization(
                            &aText[wordStart as usize..],
                            offset - wordStart,
                            &mut capitalizationState[wordStart as usize..],
                        );
                    }
                }
                wordMightBeBreakable = false;
                self.mWordContinuation = false;
                offset += 1;
                if offset >= aLength {
                    break;
                }
                wordStart = offset;
                continue;
            }

            if !wordMightBeBreakable && !IsNonBreakableChar::<u16>(ch) {
                wordMightBeBreakable = true;
            }
            offset += 1;
            if offset >= aLength {
                // Save this word
                self.mCurrentWordMightBeBreakable = wordMightBeBreakable;
                let len = offset - wordStart;
                self.mCurrentWord.extend_from_slice(&aText[wordStart as usize..(wordStart + len) as usize]);
                self.mTextItems.push(nsLineBreaker_TextItem::new(aSink, wordStart, len, aFlags));
                // Ensure that the break-before for this word is written out
                offset = wordStart + 1;
                self.UpdateCurrentWordLanguage(aHyphenationLanguage);
                break;
            }
        }

        if let Some(sink) = aSink {
            let sink = unsafe { &mut *sink };
            if !noBreaksNeeded {
                sink.SetBreaks(start, offset - start, &mut breakState[start as usize..offset as usize]);
            }
            if !noCapitalizationNeeded {
                sink.SetCapitalization(start, offset - start, &mut capitalizationState[start as usize..offset as usize]);
            }
        }
        NS_OK
    }

    // Gecko: dom/base/nsLineBreaker.cpp nsLineBreaker::FindHyphenationPoints
    fn FindHyphenationPoints(&mut self, aHyphenator: &nsHyphenator, aText: &[u16], aBreakState: &mut [u8]) {
        // (C++ takes aTextStart/aTextLimit pointers; the slice is the range between them.)
        unimplemented!("NOT PORTED: hyphenation (nsLineBreaker.cpp FindHyphenationPoints, nsHyphenator)")
    }

    /**
     * Feed 8-bit text into the linebreaker for analysis. aLength must be nonzero.
     * @param aSink can be null if the breaks are not actually needed (we may
     * still be setting up state for later breaks)
     */
    // Gecko: dom/base/nsLineBreaker.cpp nsLineBreaker::AppendText(nsAtom*, const uint8_t*, ...)
    pub fn AppendText__nsAtom_ptr_uint8_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(
        &mut self,
        aHyphenationLanguage: Option<nsAtom>,
        aText: &[u8],
        aLength: u32,
        aFlags: u32,
        aSink: nsILineBreakSinkPtr,
    ) -> nsresult {
        NS_ASSERTION!(aLength > 0, "Appending empty text...");

        if (aFlags & (nsLineBreaker::BREAK_NEED_CAPITALIZATION | nsLineBreaker::BREAK_USE_AUTO_HYPHENATION)) != 0 {
            // Defer to the Unicode path if capitalization or hyphenation is required
            // CopyASCIItoUTF16(..., str): each byte widens to one char16_t
            let str: Vec<u16> = aText[..aLength as usize].iter().map(|&b| b as u16).collect();
            return self.AppendText__nsAtom_ptr_char16_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(
                aHyphenationLanguage,
                &str,
                aLength,
                aFlags,
                aSink,
            );
        }

        let mut offset: u32 = 0;

        // Continue the current word
        if self.mCurrentWord.len() > 0 {
            NS_ASSERTION!(!self.mAfterBreakableSpace && !self.mBreakHere, "These should not be set");

            while offset < aLength && !self.IsSegmentSpace(aText[offset as usize] as u16) {
                self.mCurrentWord.push(aText[offset as usize] as u16);
                if !self.mCurrentWordMightBeBreakable && !IsNonBreakableChar::<u8>(aText[offset as usize]) {
                    self.mCurrentWordMightBeBreakable = true;
                }
                offset += 1;
            }

            if offset > 0 {
                self.mTextItems.push(nsLineBreaker_TextItem::new(aSink, 0, offset, aFlags));
            }

            if offset == aLength {
                // We did not encounter whitespace so the word hasn't finished yet.
                return NS_OK;
            }

            // We encountered whitespace, so we're done with this word
            let rv = self.FlushCurrentWord();
            if NS_FAILED(rv) {
                return rv;
            }
        }

        let mut breakState: Vec<u8> = Vec::new();
        if aSink.is_some() {
            breakState = vec![0; aLength as usize];
        }

        let start = offset;
        let noBreaksNeeded = aSink.is_none()
            || ((aFlags & NO_BREAKS_NEEDED_FLAGS) == NO_BREAKS_NEEDED_FLAGS && !self.mBreakHere && !self.mAfterBreakableSpace);
        if noBreaksNeeded {
            // Skip to the space before the last word, since either the break data
            // here is not needed, or no breaks are set in the sink and there cannot
            // be any breaks in this chunk; all we need is the context for the next
            // chunk (if any)
            offset = aLength;
            while offset > start {
                offset -= 1;
                if self.IsSegmentSpace(aText[offset as usize] as u16) {
                    break;
                }
            }
        }
        let mut wordStart = offset;
        let mut wordMightBeBreakable = false;

        loop {
            let ch: u8 = aText[offset as usize];
            let isSpace = self.IsSegmentSpace(ch as u16);
            let isBreakableSpace = isSpace && (aFlags & nsLineBreaker::BREAK_SUPPRESS_INSIDE) == 0;

            if aSink.is_some() {
                // Consider word-break style.  Since the break position of CJK scripts
                // will be set by nsILineBreaker, we don't consider CJK at this point.
                // (Gecko writes breakState[offset] here even when noBreaksNeeded; the buffer exists then.)
                breakState[offset as usize] = if self.mBreakHere
                    || (self.mAfterBreakableSpace && !isBreakableSpace)
                    || self.mWordBreak == WordBreakRule::BreakAll
                    || self.mLineBreak == LineBreakRule::Anywhere
                {
                    gfxTextRun_CompressedGlyph::FLAG_BREAK_TYPE_NORMAL
                } else {
                    gfxTextRun_CompressedGlyph::FLAG_BREAK_TYPE_NONE
                };
            }
            self.mBreakHere = false;
            self.mAfterBreakableSpace = isBreakableSpace;

            if isSpace {
                if offset > wordStart && aSink.is_some() && (aFlags & nsLineBreaker::BREAK_SUPPRESS_INSIDE) == 0 {
                    if self.mLineBreak == LineBreakRule::Anywhere {
                        breakState[wordStart as usize..offset as usize].fill(gfxTextRun_CompressedGlyph::FLAG_BREAK_TYPE_NORMAL);
                    } else if wordMightBeBreakable {
                        // Save current start-of-word state because ComputeBreakPositions()
                        // will set it to false.
                        // AutoRestore<uint8_t> saveWordStartBreakState(breakState[wordStart]);
                        let saveWordStartBreakState = breakState[wordStart as usize];
                        LineBreaker::ComputeBreakPositions__uint8_t_ptr_uint32_t_WordBreakRule_LineBreakRule_bool_uint8_t_ptr(
                            &aText[wordStart as usize..],
                            offset - wordStart,
                            self.mWordBreak,
                            self.mLineBreak,
                            self.mScriptIsChineseOrJapanese,
                            &mut breakState[wordStart as usize..],
                        );
                        breakState[wordStart as usize] = saveWordStartBreakState;
                    }
                }

                wordMightBeBreakable = false;
                self.mWordContinuation = false;
                offset += 1;
                if offset >= aLength {
                    break;
                }
                wordStart = offset;
                continue;
            }

            if !wordMightBeBreakable && !IsNonBreakableChar::<u8>(ch) {
                wordMightBeBreakable = true;
            }
            offset += 1;
            if offset >= aLength {
                // Save this word
                self.mCurrentWordMightBeBreakable = wordMightBeBreakable;
                let len = offset - wordStart;
                for i in wordStart..offset {
                    self.mCurrentWord.push(aText[i as usize] as u16);
                }
                self.mTextItems.push(nsLineBreaker_TextItem::new(aSink, wordStart, len, aFlags));
                // Ensure that the break-before for this word is written out
                offset = wordStart + 1;
                break;
            }
        }

        if !noBreaksNeeded {
            let sink = unsafe { &mut *aSink.unwrap() };
            sink.SetBreaks(start, offset - start, &mut breakState[start as usize..offset as usize]);
        }
        NS_OK
    }

    // Gecko: dom/base/nsLineBreaker.cpp nsLineBreaker::UpdateCurrentWordLanguage
    fn UpdateCurrentWordLanguage(&mut self, aHyphenationLanguage: Option<nsAtom>) {
        if self.mCurrentWordLanguage.is_some() && self.mCurrentWordLanguage != aHyphenationLanguage {
            self.mCurrentWordContainsMixedLang = true;
            self.mScriptIsChineseOrJapanese = false;
            return;
        }

        if aHyphenationLanguage.is_some() && self.mCurrentWordLanguage.is_none() {
            // NOT PORTED: the one-entry cache (sLastHyphenationLanguage / sLastScriptIsChineseOrJapanese,
            // cleared with ClearOnShutdown); it is only a speed-up.
            //   Locale loc;
            //   auto result = LocaleParser::TryParse(nsAtomCString(aHyphenationLanguage), loc);
            //   if (result.isErr()) return;
            //   if (loc.Script().Missing() && loc.AddLikelySubtags().isErr()) return;
            //   mScriptIsChineseOrJapanese = loc.Script().EqualTo("Hans") || ... "Hant" || "Jpan" || "Hrkt";
            // icu_locale_core parses the tag. NOT PORTED: AddLikelySubtags (needs icu_locale's
            // LocaleExpander); a small table of the likely scripts of the Chinese and Japanese languages stands in.
            let Ok(loc) = aHyphenationLanguage.unwrap().parse::<icu_locale_core::LanguageIdentifier>() else {
                return;
            };
            let script: Option<String> = match loc.script {
                Some(s) => Some(s.as_str().to_string()),
                None => match loc.language.as_str() {
                    "zh" | "wuu" | "gan" | "hak" | "cmn" => Some("Hans".to_string()),
                    "yue" => Some("Hant".to_string()),
                    "ja" => Some("Jpan".to_string()),
                    _ => None,
                },
            };
            self.mScriptIsChineseOrJapanese = match script.as_deref() {
                Some(s) => s == "Hans" || s == "Hant" || s == "Jpan" || s == "Hrkt",
                None => false,
            };
        }
        self.mCurrentWordLanguage = aHyphenationLanguage;
    }

    /**
     * Reset all state. This means the current run has ended; any outstanding
     * calls through nsILineBreakSink are made, and all outstanding references to
     * nsILineBreakSink objects are dropped.
     * After this call, this linebreaker can be reused.
     * This must be called at least once between any call to AppendText() and
     * destroying the object.
     * @param aTrailingBreak this is set to true when there is a break opportunity
     * at the end of the text. This will normally only be declared true when there
     * is breakable whitespace at the end.
     */
    // Gecko: dom/base/nsLineBreaker.cpp nsLineBreaker::Reset
    pub fn Reset(&mut self, aTrailingBreak: &mut bool) -> nsresult {
        let rv = self.FlushCurrentWord();
        if NS_FAILED(rv) {
            return rv;
        }

        *aTrailingBreak = self.mBreakHere || self.mAfterBreakableSpace;
        self.mBreakHere = false;
        self.mAfterBreakableSpace = false;
        NS_OK
    }
}

// Tests: Gecko has no gtest for nsLineBreaker (there are web-platform tests); these are written from the
// documented behaviour in nsLineBreaker.h and CSS Text 3.
#[cfg(test)]
mod tests {
    use super::*;
    use gfxTextRun_CompressedGlyph::*;

    // A sink for one text chunk: the break and capitalization state that the line breaker reports.
    struct TestSink {
        breaks: Vec<u8>,
        caps: Vec<bool>,
        calls: u32,
    }

    impl TestSink {
        fn new(len: usize) -> TestSink {
            TestSink { breaks: vec![0xff; len], caps: vec![false; len], calls: 0 }
        }
        fn ptr(&mut self) -> nsILineBreakSinkPtr {
            Some(self as *mut TestSink as *mut dyn nsILineBreakSink)
        }
        // offsets with a normal break before them
        fn break_offsets(&self) -> Vec<usize> {
            (0..self.breaks.len()).filter(|&i| self.breaks[i] == FLAG_BREAK_TYPE_NORMAL).collect()
        }
        fn cap_offsets(&self) -> Vec<usize> {
            (0..self.caps.len()).filter(|&i| self.caps[i]).collect()
        }
    }

    impl nsILineBreakSink for TestSink {
        fn SetBreaks(&mut self, aStart: u32, aLength: u32, aBreakBefore: &mut [u8]) {
            self.calls += 1;
            for i in 0..aLength as usize {
                self.breaks[aStart as usize + i] = aBreakBefore[i];
            }
        }
        fn SetCapitalization(&mut self, aStart: u32, aLength: u32, aCapitalize: &mut [bool]) {
            for i in 0..aLength as usize {
                self.caps[aStart as usize + i] = aCapitalize[i];
            }
        }
    }

    fn u16s(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    // Feed `chunks` through one line breaker, each with `flags`, and return the sinks and the trailing break.
    fn run(chunks: &[&str], flags: u32, setup: impl FnOnce(&mut nsLineBreaker)) -> (Vec<TestSink>, bool) {
        let mut lb = nsLineBreaker::default();
        setup(&mut lb);
        let mut sinks: Vec<TestSink> = chunks.iter().map(|c| TestSink::new(c.encode_utf16().count())).collect();
        for (i, c) in chunks.iter().enumerate() {
            let t = u16s(c);
            let sink = sinks[i].ptr();
            let rv = lb.AppendText__nsAtom_ptr_char16_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(None, &t, t.len() as u32, flags, sink);
            assert_eq!(rv, NS_OK);
        }
        let mut trailing = false;
        assert_eq!(lb.Reset(&mut trailing), NS_OK);
        (sinks, trailing)
    }

    #[test]
    fn breaks_after_spaces_in_one_chunk() {
        let (s, trailing) = run(&["Hello world, this is"], 0, |_| {});
        assert_eq!(s[0].break_offsets(), vec![6, 13, 18]);
        // the first character never has a break before it from this chunk
        assert_eq!(s[0].breaks[0], FLAG_BREAK_TYPE_NONE);
        assert!(!trailing);
    }

    #[test]
    fn the_result_does_not_depend_on_how_the_text_is_split() {
        // nsLineBreaker.h: "The linebreak results MUST NOT depend on how the text is broken up".
        let whole = "Hello world, this is a test";
        let (w, _) = run(&[whole], 0, |_| {});
        let expect = w[0].break_offsets();
        for cut in 1..whole.len() {
            let (a, b) = whole.split_at(cut);
            let (s, _) = run(&[a, b], 0, |_| {});
            let mut got = s[0].break_offsets();
            got.extend(s[1].break_offsets().iter().map(|o| o + a.len()));
            assert_eq!(got, expect, "split at {cut}");
        }
        // three pieces, with a split in the middle of a word and a split right at a space
        let (s, _) = run(&["Hel", "lo wor", "ld"], 0, |_| {});
        assert_eq!(s[0].break_offsets(), vec![]);
        assert_eq!(s[1].break_offsets(), vec![3]);
        assert_eq!(s[2].break_offsets(), vec![]);
    }

    #[test]
    fn a_break_follows_a_space_at_the_end_of_a_chunk() {
        // the space ends the first chunk; the break is before the first letter of the next chunk
        let (s, trailing) = run(&["Hello ", "world"], 0, |_| {});
        assert_eq!(s[0].break_offsets(), vec![]);
        assert_eq!(s[1].break_offsets(), vec![0]);
        assert!(!trailing);
    }

    #[test]
    fn reset_reports_a_trailing_break_after_trailing_whitespace() {
        let (_, trailing) = run(&["Hello world "], 0, |_| {});
        assert!(trailing);
        let (_, trailing) = run(&["Hello world"], 0, |_| {});
        assert!(!trailing);
    }

    #[test]
    fn a_hyphen_gives_a_break_inside_the_word() {
        let (s, _) = run(&["well-known word"], 0, |_| {});
        assert_eq!(s[0].break_offsets(), vec![5, 11]);
    }

    #[test]
    fn cjk_text_breaks_between_characters() {
        let (s, _) = run(&["日本語です"], 0, |_| {});
        assert_eq!(s[0].break_offsets(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn break_suppress_inside_makes_spaces_unbreakable() {
        // white-space: nowrap / pre: BREAK_SUPPRESS_INSIDE
        let (s, _) = run(&["Hello world, this"], nsLineBreaker::BREAK_SUPPRESS_INSIDE, |_| {});
        assert_eq!(s[0].break_offsets(), vec![]);
    }

    #[test]
    fn break_suppress_initial_removes_the_break_at_the_start_of_a_chunk_inside_a_word() {
        // "ab" then "日本" form one word; ICU puts a break between the letters and the ideograph.
        // BREAK_SUPPRESS_INITIAL on the second chunk takes the break at its start away (FlushCurrentWord).
        for (flags, expected) in [(0, FLAG_BREAK_TYPE_NORMAL), (nsLineBreaker::BREAK_SUPPRESS_INITIAL, FLAG_BREAK_TYPE_NONE)] {
            let mut lb = nsLineBreaker::default();
            let a = u16s("ab");
            let b = u16s("日本");
            let mut sa = TestSink::new(a.len());
            let mut sb = TestSink::new(b.len());
            let (pa, pb) = (sa.ptr(), sb.ptr());
            lb.AppendText__nsAtom_ptr_char16_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(None, &a, 2, 0, pa);
            lb.AppendText__nsAtom_ptr_char16_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(None, &b, 2, flags, pb);
            let mut trailing = false;
            lb.Reset(&mut trailing);
            assert_eq!(sb.breaks[0], expected, "flags {flags}");
            assert_eq!(sb.breaks[1], FLAG_BREAK_TYPE_NORMAL);
        }
    }

    #[test]
    fn break_skip_setting_no_breaks_only_sets_the_last_word_for_context() {
        // With all of SUPPRESS_INITIAL | SUPPRESS_INSIDE | SKIP_SETTING_NO_BREAKS the scan jumps to the
        // space before the last word. Only that last word is kept (for the context of following text),
        // and it is reported when it is flushed; the text before it is never written.
        let flags = nsLineBreaker::BREAK_SUPPRESS_INITIAL | nsLineBreaker::BREAK_SUPPRESS_INSIDE | nsLineBreaker::BREAK_SKIP_SETTING_NO_BREAKS;
        let (s, _) = run(&["Hello world"], flags, |_| {});
        assert!(s[0].breaks[..6].iter().all(|&b| b == 0xff));
        assert_eq!(s[0].calls, 1);
        assert_eq!(s[0].break_offsets(), vec![]);
    }

    #[test]
    fn word_break_break_all_breaks_between_letters() {
        let (s, _) = run(&["abc def"], 0, |lb| lb.SetWordBreak(WordBreakRule::BreakAll));
        // every character, including the first one of the chunk, may be broken before
        assert_eq!(s[0].break_offsets(), vec![0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn line_break_anywhere_breaks_between_all_characters() {
        let (s, _) = run(&["abc"], 0, |lb| lb.SetStrictness(LineBreakRule::Anywhere));
        assert_eq!(s[0].break_offsets(), vec![0, 1, 2]);
    }

    #[test]
    fn non_breakable_words_are_not_given_to_the_icu_segmenter() {
        // letters and digits only: kNonBreakableASCII says no break inside, even next to a hyphen-less run
        assert!(IsNonBreakableChar::<u16>('a' as u16));
        assert!(IsNonBreakableChar::<u16>('7' as u16));
        assert!(!IsNonBreakableChar::<u16>('-' as u16));
        assert!(!IsNonBreakableChar::<u16>('日' as u16));
        assert!(!IsNonBreakableChar::<u8>(b'/'));
    }

    #[test]
    fn capitalization_marks_the_first_letter_of_each_word() {
        // text-transform: capitalize
        let (s, _) = run(&["hello world, it's foo-bar"], nsLineBreaker::BREAK_NEED_CAPITALIZATION, |_| {});
        // "Hello World, It's Foo-Bar": the apostrophe in "it's" does not start a word
        assert_eq!(s[0].cap_offsets(), vec![0, 6, 13, 18, 22]);
    }

    #[test]
    fn capitalization_works_across_chunks_and_respects_word_continuation() {
        let (s, _) = run(&["hel", "lo wor", "ld"], nsLineBreaker::BREAK_NEED_CAPITALIZATION, |_| {});
        assert_eq!(s[0].cap_offsets(), vec![0]);
        assert_eq!(s[1].cap_offsets(), vec![3]);
        assert_eq!(s[2].cap_offsets(), Vec::<usize>::new());
        // SetWordContinuation(true): the text continues a word that already started, so no capital
        let (s, _) = run(&["hello world"], nsLineBreaker::BREAK_NEED_CAPITALIZATION, |lb| lb.SetWordContinuation(true));
        assert_eq!(s[0].cap_offsets(), vec![6]);
    }

    #[test]
    fn should_capitalize_follows_the_general_category() {
        let mut next = true;
        assert!(nsLineBreaker::ShouldCapitalize('a' as u32, &mut next));
        assert!(!next);
        assert!(!nsLineBreaker::ShouldCapitalize('b' as u32, &mut next));
        assert!(!nsLineBreaker::ShouldCapitalize(' ' as u32, &mut next));
        assert!(next); // a space re-arms it
        // digits count
        assert!(nsLineBreaker::ShouldCapitalize('1' as u32, &mut next));
        // a closing quote U+2019 and an ASCII apostrophe do not re-arm it
        next = false;
        nsLineBreaker::ShouldCapitalize(0x2019, &mut next);
        assert!(!next);
        nsLineBreaker::ShouldCapitalize('\'' as u32, &mut next);
        assert!(!next);
        // a hyphen does
        nsLineBreaker::ShouldCapitalize('-' as u32, &mut next);
        assert!(next);
    }

    #[test]
    fn latin1_text_gives_the_same_breaks() {
        let mut lb = nsLineBreaker::default();
        let t = b"Hello world, this".to_vec();
        let mut sink = TestSink::new(t.len());
        let p = sink.ptr();
        assert_eq!(lb.AppendText__nsAtom_ptr_uint8_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(None, &t, t.len() as u32, 0, p), NS_OK);
        let mut trailing = false;
        lb.Reset(&mut trailing);
        assert_eq!(sink.break_offsets(), vec![6, 13]);
    }

    #[test]
    fn invisible_whitespace_acts_like_a_space() {
        // AppendInvisibleWhitespace between two chunks: a break opportunity before the second
        let mut lb = nsLineBreaker::default();
        let a = u16s("foo");
        let b = u16s("bar");
        let mut sa = TestSink::new(3);
        let mut sb = TestSink::new(3);
        let (pa, pb) = (sa.ptr(), sb.ptr());
        lb.AppendText__nsAtom_ptr_char16_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(None, &a, 3, 0, pa);
        lb.AppendInvisibleWhitespace(0);
        lb.AppendText__nsAtom_ptr_char16_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(None, &b, 3, 0, pb);
        let mut trailing = false;
        lb.Reset(&mut trailing);
        assert_eq!(sb.break_offsets(), vec![0]);
        // ... but not when it is not breakable
        let mut lb = nsLineBreaker::default();
        let mut sa = TestSink::new(3);
        let mut sb = TestSink::new(3);
        let (pa, pb) = (sa.ptr(), sb.ptr());
        lb.AppendText__nsAtom_ptr_char16_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(None, &a, 3, 0, pa);
        lb.AppendInvisibleWhitespace(nsLineBreaker::BREAK_SUPPRESS_INSIDE);
        lb.AppendText__nsAtom_ptr_char16_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(None, &b, 3, 0, pb);
        lb.Reset(&mut trailing);
        assert_eq!(sb.break_offsets(), vec![]);
    }

    #[test]
    fn chinese_or_japanese_language_selects_the_cjk_line_breaking() {
        // the language's script (zh -> Hans, ja -> Jpan) sets mScriptIsChineseOrJapanese
        let mut lb = nsLineBreaker::default();
        lb.UpdateCurrentWordLanguage(Some("ja"));
        assert!(lb.mScriptIsChineseOrJapanese);
        let mut lb = nsLineBreaker::default();
        lb.UpdateCurrentWordLanguage(Some("zh-Hant-TW"));
        assert!(lb.mScriptIsChineseOrJapanese);
        let mut lb = nsLineBreaker::default();
        lb.UpdateCurrentWordLanguage(Some("en-US"));
        assert!(!lb.mScriptIsChineseOrJapanese);
        // a different language in the same word: mixed, and not CJK any more
        lb.UpdateCurrentWordLanguage(Some("fr"));
        assert!(lb.mCurrentWordContainsMixedLang);
    }
}
