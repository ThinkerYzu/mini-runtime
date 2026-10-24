// Gecko: layout/generic/nsTextFrameUtils.h, layout/generic/nsTextFrameUtils.cpp
//
// Character types: Gecko's TransformText & co. are templates over `CharT` = char16_t or uint8_t. Here
// they are generic functions over the trait `TextFrameChar`, implemented for u16 and u8; the C++
// `if constexpr (sizeof(CharT) > 1)` is `if CharT::IS_WIDE`. Output buffers: where C++ takes a
// `CharT* aOutput` and returns the pointer past the last written character, Rust takes `&mut [CharT]`
// (the start of the buffer) and returns the number of characters written.
use crate::*;
use crate::nsLineLayoutAdapter::nsStyleTextAdapter;

pub const BIG_TEXT_NODE_SIZE: u32 = 4096;

pub const CH_NBSP: u32 = 160;
pub const CH_SHY: u32 = 173;
pub const CH_CJKSP: u32 = 12288; // U+3000 IDEOGRAPHIC SPACE (CJK Full-Width Space)

// These constants are used as textrun flags for textframe textruns.
//
// If you add a flag, please add support for it in gfxTextRun::Dump.
// Gecko: layout/generic/nsTextFrameUtils.h nsTextFrameUtils::Flags
MOZ_ENUM_CLASS_BITS! {
    pub struct nsTextFrameUtils_Flags: u16 {
        // The following flags are set by TransformText

        // the text has at least one untransformed tab character
        HasTab = 0x01,
        // the original text has at least one soft hyphen character
        HasShy = 0x02,
        // the text has at least one untransformed newline character
        HasNewline = 0x04,

        // Flag used in textrun construction to *prevent* hiding of fallback text
        // for pending user-fonts (used for Canvas2d text).
        DontSkipDrawingForPendingUserFonts = 0x08,

        // The following flags are set by nsTextFrame

        IsSimpleFlow = 0x10,
        IncomingWhitespace = 0x20,
        TrailingWhitespace = 0x40,
        CompressedLeadingWhitespace = 0x80,
        NoBreaks = 0x100,
        IsTransformed = 0x200,
        // This gets set if there's a break opportunity at the end of the textrun.
        // We normally don't use this break opportunity because the following text
        // will have a break opportunity at the start, but it's useful for line
        // layout to know about it in case the following content is not text
        HasTrailingBreak = 0x400,

        // This is set if the textrun was created for a textframe whose
        // NS_FRAME_IS_IN_SINGLE_CHAR_MI flag is set.  This occurs if the textframe
        // belongs to a MathML <mi> element whose embedded text consists of a
        // single character.
        IsSingleCharMi = 0x800,

        // This is set if the text run might be observing for glyph changes.
        MightHaveGlyphChanges = 0x1000,

        // For internal use by the memory reporter when accounting for
        // storage used by textruns.
        // Because the reporter may visit each textrun multiple times while
        // walking the frame trees and textrun cache, it needs to mark
        // textruns that have been seen so as to avoid multiple-accounting.
        RunSizeAccounted = 0x2000,

        // The following are defined by gfxTextRunFactory rather than here,
        // so that it also has access to the _INCOMING and MATH_SCRIPT flags
        // for shaping purposes.
        // They live in the gfxShapedText::mFlags field rather than the
        // gfxTextRun::mFlags2 field.
        // TEXT_TRAILING_ARABICCHAR
        // TEXT_INCOMING_ARABICCHAR
        // TEXT_USE_MATH_SCRIPT
    }
}

// Gecko: layout/generic/nsTextFrameUtils.h nsTextFrameUtils::CompressionMode
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsTextFrameUtils_CompressionMode {
    COMPRESS_NONE,
    COMPRESS_WHITESPACE,
    COMPRESS_WHITESPACE_NEWLINE,
    COMPRESS_NONE_TRANSFORM_TO_SPACE,
}

// ADAPTER: the C++ template parameter CharT (char16_t or uint8_t) of TransformText and friends.
pub trait TextFrameChar: Copy + PartialEq {
    // sizeof(CharT) > 1
    const IS_WIDE: bool;
    fn to_u32(self) -> u32;
    fn from_ascii(aCh: u8) -> Self;
    // the overloaded IsDiscardable(char16_t, Flags*) / IsDiscardable(uint8_t, Flags*)
    fn IsDiscardable(self, aFlags: &mut nsTextFrameUtils_Flags) -> bool;
    // the overloaded nsTextFrameUtils::IsSpaceCombiningSequenceTail
    fn IsSpaceCombiningSequenceTail(aChars: &[Self], aLength: i32) -> bool;
}

impl TextFrameChar for u16 {
    const IS_WIDE: bool = true;
    fn to_u32(self) -> u32 {
        self as u32
    }
    fn from_ascii(aCh: u8) -> u16 {
        aCh as u16
    }
    fn IsDiscardable(self, aFlags: &mut nsTextFrameUtils_Flags) -> bool {
        IsDiscardable__char16_t_Flags_ptr(self, aFlags)
    }
    fn IsSpaceCombiningSequenceTail(aChars: &[u16], aLength: i32) -> bool {
        nsTextFrameUtils::IsSpaceCombiningSequenceTail__char16_t_ptr_int32_t(aChars, aLength)
    }
}

impl TextFrameChar for u8 {
    const IS_WIDE: bool = false;
    fn to_u32(self) -> u32 {
        self as u32
    }
    fn from_ascii(aCh: u8) -> u8 {
        aCh
    }
    fn IsDiscardable(self, aFlags: &mut nsTextFrameUtils_Flags) -> bool {
        IsDiscardable__uint8_t_Flags_ptr(self, aFlags)
    }
    fn IsSpaceCombiningSequenceTail(aChars: &[u8], aLength: i32) -> bool {
        nsTextFrameUtils::IsSpaceCombiningSequenceTail__uint8_t_ptr_int32_t(aChars, aLength)
    }
}

// Gecko: layout/generic/nsTextFrameUtils.h nsTextFrameUtils (a class with only static members)
pub struct nsTextFrameUtils;

impl nsTextFrameUtils {
    // These constants are used in TransformText to represent context information
    // from previous textruns.
    pub const INCOMING_NONE: u8 = 0;
    pub const INCOMING_WHITESPACE: u8 = 1;
    pub const INCOMING_ARABICCHAR: u8 = 2;

    /**
     * Returns true if aChars/aLength are something that make a space
     * character not be whitespace when they follow the space character
     * (combining mark or join control, ignoring intervening direction
     * controls).
     */
    // Gecko: layout/generic/nsTextFrameUtils.cpp nsTextFrameUtils::IsSpaceCombiningSequenceTail(const char16_t*, int32_t)
    pub fn IsSpaceCombiningSequenceTail__char16_t_ptr_int32_t(aChars: &[u16], aLength: i32) -> bool {
        aLength > 0
            && (IsClusterExtenderExcludingJoiners__uint32_t(aChars[0] as u32)
                || (IsBidiControl(aChars[0] as u32)
                    && nsTextFrameUtils::IsSpaceCombiningSequenceTail__char16_t_ptr_int32_t(&aChars[1..], aLength - 1)))
    }

    // Gecko: layout/generic/nsTextFrameUtils.h nsTextFrameUtils::IsSpaceCombiningSequenceTail(const uint8_t*, int32_t)
    pub fn IsSpaceCombiningSequenceTail__uint8_t_ptr_int32_t(aChars: &[u8], aLength: i32) -> bool {
        false
    }

    /**
     * Returns whether aChar is a character that nsTextFrameUtils::TransformText
     * might mark as skipped.  This is used by
     * SVGTextContentElement::GetNumberOfChars to know whether reflowing frames,
     * so that we have the results of TransformText, is required, or whether we
     * can use a fast path instead.
     */
    // Gecko: layout/generic/nsTextFrameUtils.cpp nsTextFrameUtils::IsSkippableCharacterForTransformText
    pub fn IsSkippableCharacterForTransformText<CharT: TextFrameChar>(aChar: CharT) -> bool {
        let c = aChar.to_u32();
        c == ' ' as u32
            || c == '\t' as u32
            || c == '\n' as u32
            || c == CH_SHY
            || (c > 0xFF && IsBidiControl(c))
    }

    /**
     * Create a text run from a run of Unicode text. The text may have whitespace
     * compressed. A preformatted tab is sent to the text run as a single space.
     * (Tab spacing must be performed by textframe later.) Certain other
     * characters are discarded.
     *
     * @param aCompression control what is compressed to a
     * single space character: no compression, compress spaces (not followed
     * by combining mark) and tabs, compress those plus newlines, or
     * no compression except newlines are discarded.
     * @param aIncomingFlags a flag indicating whether there was whitespace
     * or an Arabic character preceding this text. We set it to indicate if
     * there's an Arabic character or whitespace preceding the end of this text.
     * @param aLanguage Content language (used to select Japanese/Chinese behavior
     * at punctuation, see https://bugzilla.mozilla.org/show_bug.cgi?id=1935148).
     *
     * Rust: aOutput is the start of the output buffer; the return value is the number of characters
     * written to it (C++: the pointer past the last one).
     */
    // Gecko: layout/generic/nsTextFrameUtils.cpp nsTextFrameUtils::TransformText
    pub fn TransformText<CharT: TextFrameChar>(
        aText: &[CharT],
        aLength: u32,
        aOutput: &mut [CharT],
        aCompression: nsTextFrameUtils_CompressionMode,
        aIncomingFlags: &mut u8,
        aSkipChars: &mut gfxSkipChars,
        aAnalysisFlags: &mut nsTextFrameUtils_Flags,
        aLanguage: Option<nsAtom>,
    ) -> usize {
        use nsTextFrameUtils_CompressionMode::*;
        let mut flags = nsTextFrameUtils_Flags::default();
        #[cfg(debug_assertions)]
        let skipCharsOffset: i32 = aSkipChars.GetOriginalCharCount();

        // Rust: the output position (C++ advances the pointer aOutput).
        let mut out: usize = 0;

        let mut lastCharArabic = false;
        if aCompression == COMPRESS_NONE || aCompression == COMPRESS_NONE_TRANSFORM_TO_SPACE {
            // Skip discardables.
            let mut i: u32 = 0;
            while i < aLength {
                let mut ch: CharT = aText[i as usize];

                // Batch-process runs of non-discardable chars >= ' '.
                if ch.to_u32() >= ' ' as u32 && !ch.IsDiscardable(&mut flags) {
                    let batchStart = i;
                    while i + 1 < aLength {
                        let next = aText[(i + 1) as usize];
                        if next.to_u32() < ' ' as u32 || next.IsDiscardable(&mut flags) {
                            break;
                        }
                        i += 1;
                    }
                    if CharT::IS_WIDE {
                        lastCharArabic = IS_ARABIC_CHAR(aText[i as usize].to_u32());
                    }
                    let batchLen = i - batchStart + 1;
                    aOutput[out..out + batchLen as usize].copy_from_slice(&aText[batchStart as usize..(batchStart + batchLen) as usize]);
                    out += batchLen as usize;
                    aSkipChars.KeepChars(batchLen);
                    i += 1;
                    continue;
                }

                if ch.IsDiscardable(&mut flags) {
                    aSkipChars.SkipChar();
                } else {
                    aSkipChars.KeepChar();
                    if ch.to_u32() > ' ' as u32 {
                        if CharT::IS_WIDE {
                            lastCharArabic = IS_ARABIC_CHAR(ch.to_u32());
                        }
                    } else if aCompression == COMPRESS_NONE_TRANSFORM_TO_SPACE {
                        if ch.to_u32() == '\t' as u32 || ch.to_u32() == '\n' as u32 {
                            ch = CharT::from_ascii(b' ');
                        }
                    } else {
                        // aCompression == COMPRESS_NONE
                        if ch.to_u32() == '\t' as u32 {
                            flags |= nsTextFrameUtils_Flags::HasTab;
                        } else if ch.to_u32() == '\n' as u32 {
                            flags |= nsTextFrameUtils_Flags::HasNewline;
                        }
                    }
                    aOutput[out] = ch;
                    out += 1;
                }
                i += 1;
            }
            *aIncomingFlags &= !nsTextFrameUtils::INCOMING_WHITESPACE;
        } else {
            let langIsJapaneseOrChinese: bool = (|| {
                // aLanguage->GetLength() < 2; GetUTF16String()
                let Some(aLanguage) = aLanguage else {
                    return false;
                };
                let text: Vec<u16> = aLanguage.encode_utf16().collect();
                if text.len() < 2 {
                    return false;
                }
                if (ToLowerCaseASCII(text[0]) == 'j' as u16 && ToLowerCaseASCII(text[1]) == 'a' as u16)
                    || (ToLowerCaseASCII(text[0]) == 'z' as u16 && ToLowerCaseASCII(text[1]) == 'h' as u16)
                {
                    return text.len() == 2 || text[2] == '-' as u16;
                }
                false
            })();
            let mut inWhitespace = (*aIncomingFlags & nsTextFrameUtils::INCOMING_WHITESPACE) != 0;
            let mut i: u32 = 0;
            while i < aLength {
                let ch: CharT = aText[i as usize];

                // Batch-process runs of non-discardable, non-document-whitespace,
                // non-segment-break chars.
                if !IsSpaceOrTabOrSegmentBreak(ch) && !ch.IsDiscardable(&mut flags) {
                    let batchStart = i;
                    while i + 1 < aLength {
                        let next = aText[(i + 1) as usize];
                        if IsSpaceOrTabOrSegmentBreak(next) || next.IsDiscardable(&mut flags) {
                            break;
                        }
                        i += 1;
                    }
                    if CharT::IS_WIDE {
                        lastCharArabic = IS_ARABIC_CHAR(aText[i as usize].to_u32());
                    }
                    let batchLen = i - batchStart + 1;
                    aOutput[out..out + batchLen as usize].copy_from_slice(&aText[batchStart as usize..(batchStart + batchLen) as usize]);
                    out += batchLen as usize;
                    aSkipChars.KeepChars(batchLen);
                    inWhitespace = false;
                    i += 1;
                    continue;
                }

                // CSS Text 3 - 4.1. The White Space Processing Rules
                // White space processing in CSS affects only the document white space
                // characters: spaces (U+0020), tabs (U+0009), and segment breaks.
                // Since we need the context of segment breaks and their surrounding
                // white spaces to proceed the white space processing, a consecutive run
                // of spaces/tabs/segment breaks is collected in a first pass loop, then
                // we apply the collapsing and transformation rules to this run in a
                // second pass loop.
                if IsSpaceOrTabOrSegmentBreak(ch) {
                    let mut keepLastSpace = false;
                    let mut hasSegmentBreak = IsSegmentBreak(ch);
                    let mut countTrailingDiscardables: u32 = 0;
                    let mut j: u32 = i + 1;
                    while j < aLength && (IsSpaceOrTabOrSegmentBreak(aText[j as usize]) || aText[j as usize].IsDiscardable(&mut flags)) {
                        if IsSegmentBreak(aText[j as usize]) {
                            hasSegmentBreak = true;
                        }
                        j += 1;
                    }
                    // Exclude trailing discardables before checking space combining
                    // sequence tail.
                    while aText[(j - 1) as usize].IsDiscardable(&mut flags) {
                        countTrailingDiscardables += 1;
                        j -= 1;
                    }
                    // If the last white space is followed by a combining sequence tail,
                    // exclude it from the range of TransformWhiteSpaces.
                    if CharT::IS_WIDE {
                        if aText[(j - 1) as usize].to_u32() == ' ' as u32
                            && j < aLength
                            && CharT::IsSpaceCombiningSequenceTail(&aText[j as usize..], (aLength - j) as i32)
                        {
                            keepLastSpace = true;
                            j -= 1;
                        }
                    }
                    if j > i {
                        out = TransformWhiteSpaces(
                            aText,
                            aLength,
                            i,
                            j,
                            hasSegmentBreak,
                            &mut inWhitespace,
                            aOutput,
                            out,
                            &mut flags,
                            aCompression,
                            aSkipChars,
                            langIsJapaneseOrChinese,
                        );
                    }
                    // We need to keep KeepChar()/SkipChar() in order, so process the
                    // last white space first, then process the trailing discardables.
                    if keepLastSpace {
                        keepLastSpace = false;
                        aOutput[out] = CharT::from_ascii(b' ');
                        out += 1;
                        aSkipChars.KeepChar();
                        if CharT::IS_WIDE {
                            lastCharArabic = false;
                        }
                        j += 1;
                    }
                    while countTrailingDiscardables > 0 {
                        aSkipChars.SkipChar();
                        j += 1;
                        countTrailingDiscardables -= 1;
                    }
                    i = j - 1;
                    i += 1;
                    continue;
                }
                // Process characters other than the document white space characters.
                if ch.IsDiscardable(&mut flags) {
                    aSkipChars.SkipChar();
                } else {
                    aOutput[out] = ch;
                    out += 1;
                    aSkipChars.KeepChar();
                }
                if CharT::IS_WIDE {
                    lastCharArabic = IS_ARABIC_CHAR(ch.to_u32());
                }
                inWhitespace = false;
                i += 1;
            }

            if inWhitespace {
                *aIncomingFlags |= nsTextFrameUtils::INCOMING_WHITESPACE;
            } else {
                *aIncomingFlags &= !nsTextFrameUtils::INCOMING_WHITESPACE;
            }
        }

        if lastCharArabic {
            *aIncomingFlags |= nsTextFrameUtils::INCOMING_ARABICCHAR;
        } else {
            *aIncomingFlags &= !nsTextFrameUtils::INCOMING_ARABICCHAR;
        }

        *aAnalysisFlags = flags;

        #[cfg(debug_assertions)]
        AssertSkippedExpectedChars(aText, aSkipChars, skipCharsOffset);
        out
    }

    // Gecko: layout/generic/nsTextFrameUtils.h nsTextFrameUtils::AppendLineBreakOffset
    pub fn AppendLineBreakOffset(aArray: &mut Vec<u32>, aOffset: u32) {
        if aArray.len() > 0 && aArray[aArray.len() - 1] == aOffset {
            return;
        }
        aArray.push(aOffset);
    }

    // Gecko: layout/generic/nsTextFrameUtils.cpp nsTextFrameUtils::ComputeApproximateLengthWithWhitespaceCompression(Text*, const nsStyleText*)
    pub fn ComputeApproximateLengthWithWhitespaceCompression__Text_ptr_nsStyleText_ptr(_aText: (), _aStyleText: &nsStyleTextAdapter) -> u32 {
        // NOT PORTED: mozilla::dom::Text / CharacterDataBuffer (the DOM text node). The body was:
        //   const CharacterDataBuffer* characterDataBuffer = &aText->DataBuffer();
        //   if (characterDataBuffer->Is2b()) { return DoComputeApproximateLengthWithWhitespaceCompression(
        //       characterDataBuffer->Get2b(), characterDataBuffer->GetLength(), aStyleText); }
        //   return DoComputeApproximateLengthWithWhitespaceCompression(
        //       characterDataBuffer->Get1b(), characterDataBuffer->GetLength(), aStyleText);
        // Callers with 8-bit or 16-bit text call DoComputeApproximateLengthWithWhitespaceCompression directly.
        unimplemented!("NOT PORTED: dom::Text / CharacterDataBuffer")
    }

    // Gecko: layout/generic/nsTextFrameUtils.cpp nsTextFrameUtils::ComputeApproximateLengthWithWhitespaceCompression(const nsAString&, const nsStyleText*)
    pub fn ComputeApproximateLengthWithWhitespaceCompression__nsAString_nsStyleText_ptr(aString: &[u16], aStyleText: &nsStyleTextAdapter) -> u32 {
        DoComputeApproximateLengthWithWhitespaceCompression(aString, aString.len() as u32, aStyleText)
    }
}

// Gecko: layout/generic/nsTextFrameUtils.cpp IsDiscardable(char16_t, Flags*)
fn IsDiscardable__char16_t_Flags_ptr(ch: u16, aFlags: &mut nsTextFrameUtils_Flags) -> bool {
    // Unlike IS_DISCARDABLE, we don't discard \r. \r will be ignored by
    // gfxTextRun and discarding it would force us to copy text in many cases of
    // preformatted text containing \r\n.
    if ch as u32 == CH_SHY {
        *aFlags |= nsTextFrameUtils_Flags::HasShy;
        return true;
    }
    IsBidiControl(ch as u32)
}

// Gecko: layout/generic/nsTextFrameUtils.cpp IsDiscardable(uint8_t, Flags*)
fn IsDiscardable__uint8_t_Flags_ptr(ch: u8, aFlags: &mut nsTextFrameUtils_Flags) -> bool {
    if ch as u32 == CH_SHY {
        *aFlags |= nsTextFrameUtils_Flags::HasShy;
        return true;
    }
    false
}

// Gecko: layout/generic/nsTextFrameUtils.cpp IsSegmentBreak
fn IsSegmentBreak<CharT: TextFrameChar>(aCh: CharT) -> bool {
    aCh.to_u32() == '\n' as u32
}

// Gecko: layout/generic/nsTextFrameUtils.cpp IsSpaceOrTab
fn IsSpaceOrTab<CharT: TextFrameChar>(aCh: CharT) -> bool {
    aCh.to_u32() == ' ' as u32 || aCh.to_u32() == '\t' as u32
}

// Gecko: layout/generic/nsTextFrameUtils.cpp IsSpaceOrTabOrSegmentBreak
fn IsSpaceOrTabOrSegmentBreak<CharT: TextFrameChar>(aCh: CharT) -> bool {
    IsSpaceOrTab(aCh) || IsSegmentBreak(aCh)
}

// Gecko: layout/generic/nsTextFrameUtils.cpp AssertSkippedExpectedChars
#[cfg(debug_assertions)]
fn AssertSkippedExpectedChars<CharT: TextFrameChar>(aText: &[CharT], aSkipChars: &gfxSkipChars, aSkipCharsOffset: i32) {
    let mut it = gfxSkipCharsIterator::new__gfxSkipChars_int32_t(aSkipChars, 0);
    it.AdvanceOriginal(aSkipCharsOffset);
    while it.GetOriginalOffset() < it.GetOriginalEnd() {
        let ch = aText[(it.GetOriginalOffset() - aSkipCharsOffset) as usize];
        MOZ_ASSERT!(
            !it.IsOriginalCharSkipped(None) || nsTextFrameUtils::IsSkippableCharacterForTransformText(ch),
            "skipped unexpected character; need to update IsSkippableCharacterForTransformText?"
        );
        it.AdvanceOriginal(1);
    }
}

// Gecko: layout/generic/nsTextFrameUtils.cpp TransformWhiteSpaces
// Rust: aOutput/aOutPos are the buffer and the position in it; the new position is returned
// (C++: CharT* aOutput in and out).
fn TransformWhiteSpaces<CharT: TextFrameChar>(
    aText: &[CharT],
    aLength: u32,
    aBegin: u32,
    aEnd: u32,
    aHasSegmentBreak: bool,
    aInWhitespace: &mut bool,
    aOutput: &mut [CharT],
    aOutPos: usize,
    aFlags: &mut nsTextFrameUtils_Flags,
    aCompression: nsTextFrameUtils_CompressionMode,
    aSkipChars: &mut gfxSkipChars,
    aLangIsJapaneseOrChinese: bool,
) -> usize {
    use nsTextFrameUtils_CompressionMode::*;
    let mut out = aOutPos;
    MOZ_ASSERT!(
        aCompression == COMPRESS_WHITESPACE || aCompression == COMPRESS_WHITESPACE_NEWLINE,
        "whitespaces should be skippable!!"
    );
    // Get the context preceding/following this white space range.
    // For 8-bit text (sizeof CharT == 1), the checks here should get optimized
    // out, and isSegmentBreakSkippable is simply initialized to false.
    let mut isSegmentBreakSkippable = false;
    if CharT::IS_WIDE {
        if (aBegin > 0 && IS_ZERO_WIDTH_SPACE(aText[(aBegin - 1) as usize].to_u32()))
            || (aEnd < aLength && IS_ZERO_WIDTH_SPACE(aText[aEnd as usize].to_u32()))
        {
            isSegmentBreakSkippable = true;
        } else if aBegin > 0 && aEnd < aLength {
            // Get the characters before and after the segment break, skipping past
            // any default-ignorable characters (e.g. variation selectors, various
            // invisible control chars, etc)
            let mut ucs4before: u32;
            let mut ucs4after: u32;
            let mut pos: u32 = aBegin;
            loop {
                if pos > 1 && IsSurrogatePair(aText[(pos - 2) as usize].to_u32(), aText[(pos - 1) as usize].to_u32()) {
                    ucs4before = SurrogateToUCS4(aText[(pos - 2) as usize].to_u32() as u16, aText[(pos - 1) as usize].to_u32() as u16);
                    pos -= 2;
                } else {
                    ucs4before = aText[(pos - 1) as usize].to_u32();
                    pos -= 1;
                }
                if !(IsDefaultIgnorable(ucs4before) && pos > 0) {
                    break;
                }
            }

            pos = aEnd;
            loop {
                if pos + 1 < aLength && IsSurrogatePair(aText[pos as usize].to_u32(), aText[(pos + 1) as usize].to_u32()) {
                    ucs4after = SurrogateToUCS4(aText[pos as usize].to_u32() as u16, aText[(pos + 1) as usize].to_u32() as u16);
                    pos += 2;
                } else {
                    ucs4after = aText[pos as usize].to_u32();
                    pos += 1;
                }
                if !(IsDefaultIgnorable(ucs4after) && pos < aLength) {
                    break;
                }
            }

            // Discard newlines between characters that have F/W/H EastAsianWidth
            // property and neither side is Hangul.
            // For Japanese/Chinese, also discard if either character is a fullwidth/
            // wide punctuation character.
            isSegmentBreakSkippable = (IsSegmentBreakSkipChar(ucs4before) && IsSegmentBreakSkipChar(ucs4after))
                || (aLangIsJapaneseOrChinese && (IsEastAsianPunctuation(ucs4before) || IsEastAsianPunctuation(ucs4after)));
        }
    }

    for i in aBegin..aEnd {
        let ch: CharT = aText[i as usize];
        let mut keepChar = false;
        let mut keepTransformedWhiteSpace = false;
        if ch.IsDiscardable(aFlags) {
            aSkipChars.SkipChar();
            continue;
        }
        if IsSpaceOrTab(ch) {
            if aHasSegmentBreak {
                // If white-space is set to normal, nowrap, or pre-line, white space
                // characters are considered collapsible and all spaces and tabs
                // immediately preceding or following a segment break are removed.
                aSkipChars.SkipChar();
                continue;
            }

            if *aInWhitespace {
                aSkipChars.SkipChar();
                continue;
            } else {
                keepTransformedWhiteSpace = true;
            }
        } else {
            // Apply Segment Break Transformation Rules (CSS Text 3 - 4.1.2) for
            // segment break characters.
            if aCompression == COMPRESS_WHITESPACE ||
                // XXX: According to CSS Text 3, a lone CR should not always be
                //      kept, but still go through the Segment Break Transformation
                //      Rules. However, this is what current modern browser engines
                //      (webkit/blink/edge) do. So, once we can get some clarity
                //      from the specification issue, we should either remove the
                //      lone CR condition here, or leave it here with this comment
                //      being rephrased.
                //      Please see https://github.com/w3c/csswg-drafts/issues/855.
                ch.to_u32() == '\r' as u32
            {
                keepChar = true;
            } else {
                // aCompression == COMPRESS_WHITESPACE_NEWLINE

                // Any collapsible segment break immediately following another
                // collapsible segment break is removed.  Then the remaining segment
                // break is either transformed into a space (U+0020) or removed
                // depending on the context before and after the break.
                if isSegmentBreakSkippable || *aInWhitespace {
                    aSkipChars.SkipChar();
                    continue;
                }
                isSegmentBreakSkippable = true;
                keepTransformedWhiteSpace = true;
            }
        }

        if keepChar {
            aOutput[out] = ch;
            out += 1;
            aSkipChars.KeepChar();
            *aInWhitespace = IsSpaceOrTab(ch);
        } else if keepTransformedWhiteSpace {
            aOutput[out] = CharT::from_ascii(b' ');
            out += 1;
            aSkipChars.KeepChar();
            *aInWhitespace = true;
        } else {
            MOZ_ASSERT_UNREACHABLE!("Should've skipped the character!!");
        }
    }
    out
}

/*
 * NOTE: The TransformText and IsSkippableCharacterForTransformText template
 * functions are part of the public API of nsTextFrameUtils, while
 * their function bodies are not available in the header. They may stop working
 * (fail to resolve symbol in link time) once their callsites are moved to a
 * different translation unit (e.g. a different unified source file).
 * Explicit instantiating this function template with `uint8_t` and `char16_t`
 * could prevent us from the potential risk.
 */
// (NOT PORTED: explicit template instantiations; Rust generics are instantiated where they are used.)

// Gecko: layout/generic/nsTextFrameUtils.cpp DoComputeApproximateLengthWithWhitespaceCompression
fn DoComputeApproximateLengthWithWhitespaceCompression<CharT: TextFrameChar>(aChars: &[CharT], aLength: u32, aStyleText: &nsStyleTextAdapter) -> u32 {
    // This is an approximation so we don't really need anything
    // too fancy here.
    let mut len: u32;
    if aStyleText.WhiteSpaceIsSignificant() {
        return aLength;
    }
    let mut prevWS = true; // more important to ignore blocks with
                           // only whitespace than get inline boundaries
                           // exactly right
    len = 0;
    for i in 0..aLength {
        let c = aChars[i as usize].to_u32();
        if c == ' ' as u32 || c == '\n' as u32 || c == '\t' as u32 || c == '\r' as u32 {
            if !prevWS {
                len += 1;
            }
            prevWS = true;
        } else {
            len += 1;
            prevWS = false;
        }
    }
    len
}

/**
 * nsSkipCharsRunIterator is a layout-oriented convenience wrapper over
 * gfxSkipCharsIterator, which iterates over runs in the string. Each run
 * consists entirely of either "unskipped" or "skipped" characters.
 *
 * By default, calling NextRun() iterates over only unskipped runs. Call
 * SetVisitSkipped() to let NextRun() also iterates over skipped runs where
 * IsSkipped() is true.
 */
// Gecko: layout/generic/nsTextFrameUtils.h nsSkipCharsRunIterator
#[derive(Clone, Copy)]
pub struct nsSkipCharsRunIterator<'a> {
    mIterator: gfxSkipCharsIterator<'a>,
    mRemainingLength: i32,
    mRunLength: i32,
    mSkipped: bool,
    mVisitSkipped: bool,
    mLengthIncludesSkipped: bool,
}

// Indicates whether the constructor's aRemainingLength counts only unskipped
// characters, or unskipped + skipped characters.
// Gecko: layout/generic/nsTextFrameUtils.h nsSkipCharsRunIterator::LengthMode
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsSkipCharsRunIterator_LengthMode {
    LENGTH_UNSKIPPED_ONLY = 0,
    LENGTH_INCLUDES_SKIPPED = 1,
}

impl<'a> nsSkipCharsRunIterator<'a> {
    // Gecko: layout/generic/nsTextFrameUtils.h nsSkipCharsRunIterator::nsSkipCharsRunIterator
    pub fn new(aStart: &gfxSkipCharsIterator<'a>, aLengthIncludesSkipped: nsSkipCharsRunIterator_LengthMode, aRemainingLength: u32) -> Self {
        nsSkipCharsRunIterator {
            mIterator: *aStart,
            mRemainingLength: aRemainingLength as i32,
            mRunLength: 0,
            mSkipped: false,
            mVisitSkipped: false,
            mLengthIncludesSkipped: aLengthIncludesSkipped == nsSkipCharsRunIterator_LengthMode::LENGTH_INCLUDES_SKIPPED,
        }
    }

    // Include skipped runs in iteration results.
    // Gecko: layout/generic/nsTextFrameUtils.h nsSkipCharsRunIterator::SetVisitSkipped
    pub fn SetVisitSkipped(&mut self) {
        self.mVisitSkipped = true;
    }

    // Gecko: layout/generic/nsTextFrameUtils.h nsSkipCharsRunIterator::SetOriginalOffset
    pub fn SetOriginalOffset(&mut self, aOffset: i32) {
        self.mIterator.SetOriginalOffset(aOffset);
    }

    // Gecko: layout/generic/nsTextFrameUtils.h nsSkipCharsRunIterator::SetSkippedOffset
    pub fn SetSkippedOffset(&mut self, aOffset: u32) {
        self.mIterator.SetSkippedOffset(aOffset);
    }

    // Advance to the next run. Return false when no runs remain. On success,
    // GetRunLength() is guaranteed to return positive length.
    // Gecko: layout/generic/nsTextFrameUtils.cpp nsSkipCharsRunIterator::NextRun
    pub fn NextRun(&mut self) -> bool {
        loop {
            if self.mRunLength != 0 {
                self.mIterator.AdvanceOriginal(self.mRunLength);
                NS_ASSERTION!(self.mRunLength > 0, "No characters in run (initial length too large?)");
                if !self.mSkipped || self.mLengthIncludesSkipped {
                    self.mRemainingLength -= self.mRunLength;
                }
            }
            if self.mRemainingLength == 0 {
                return false;
            }
            let mut length: i32 = 0;
            self.mSkipped = self.mIterator.IsOriginalCharSkipped(Some(&mut length));
            self.mRunLength = std::cmp::min(length, self.mRemainingLength);
            if !(!self.mVisitSkipped && self.mSkipped) {
                break;
            }
        }

        true
    }

    // Return true if the current run consists of skipped characters.
    // Gecko: layout/generic/nsTextFrameUtils.h nsSkipCharsRunIterator::IsSkipped
    pub fn IsSkipped(&self) -> bool {
        self.mSkipped
    }

    // Length of the current run. The return value is always positive.
    // Gecko: layout/generic/nsTextFrameUtils.h nsSkipCharsRunIterator::GetRunLength
    pub fn GetRunLength(&self) -> i32 {
        self.mRunLength
    }

    // Gecko: layout/generic/nsTextFrameUtils.h nsSkipCharsRunIterator::GetPos
    pub fn GetPos(&self) -> &gfxSkipCharsIterator<'a> {
        &self.mIterator
    }

    // Gecko: layout/generic/nsTextFrameUtils.h nsSkipCharsRunIterator::GetOriginalOffset
    pub fn GetOriginalOffset(&self) -> i32 {
        self.mIterator.GetOriginalOffset()
    }

    // Gecko: layout/generic/nsTextFrameUtils.h nsSkipCharsRunIterator::GetSkippedOffset
    pub fn GetSkippedOffset(&self) -> u32 {
        self.mIterator.GetSkippedOffset()
    }
}

// Tests: Gecko has no gtest for nsTextFrameUtils (it is covered by reftests and web-platform tests); these
// are written from the CSS Text 3 white space processing rules (4.1) that TransformText implements:
//   white-space: normal / nowrap  -> COMPRESS_WHITESPACE_NEWLINE
//   white-space: pre-line         -> COMPRESS_WHITESPACE
//   white-space: pre / pre-wrap   -> COMPRESS_NONE
#[cfg(test)]
mod tests {
    use super::*;
    use nsTextFrameUtils_CompressionMode::*;

    struct Out {
        text: String,
        incoming: u8,
        flags: nsTextFrameUtils_Flags,
        skip: gfxSkipChars,
    }

    fn transform_with(s: &str, mode: nsTextFrameUtils_CompressionMode, incoming: u8, lang: Option<nsAtom>) -> Out {
        let text: Vec<u16> = s.encode_utf16().collect();
        let mut output = vec![0u16; text.len()];
        let mut skip = gfxSkipChars::new();
        let mut incoming = incoming;
        let mut flags = nsTextFrameUtils_Flags::default();
        let n = nsTextFrameUtils::TransformText(&text, text.len() as u32, &mut output, mode, &mut incoming, &mut skip, &mut flags, lang);
        Out { text: String::from_utf16(&output[..n]).unwrap(), incoming, flags, skip }
    }

    fn transform(s: &str, mode: nsTextFrameUtils_CompressionMode) -> Out {
        transform_with(s, mode, 0, None)
    }

    fn normal(s: &str) -> String {
        transform(s, COMPRESS_WHITESPACE_NEWLINE).text
    }

    #[test]
    fn white_space_normal_collapses_runs_of_spaces() {
        assert_eq!(normal("a   b"), "a b");
        assert_eq!(normal("a b"), "a b");
        assert_eq!(normal("a \t \t b"), "a b");
        // the leading and trailing space are collapsed to one space (they are removed later, by the frame)
        assert_eq!(normal("  a  "), " a ");
    }

    #[test]
    fn white_space_normal_turns_a_newline_into_a_space() {
        assert_eq!(normal("a\nb"), "a b");
        assert_eq!(normal("a \n b"), "a b");
        assert_eq!(normal("a\n\n\nb"), "a b");
        assert_eq!(normal("a\n \n b"), "a b");
    }

    #[test]
    fn a_non_breaking_space_is_not_collapsed() {
        assert_eq!(normal("a\u{a0}\u{a0}b"), "a\u{a0}\u{a0}b");
        assert_eq!(normal("a \u{a0} b"), "a \u{a0} b");
        // U+3000 IDEOGRAPHIC SPACE is not a document white space character either
        assert_eq!(normal("a\u{3000}\u{3000}b"), "a\u{3000}\u{3000}b");
    }

    #[test]
    fn white_space_pre_line_keeps_newlines_and_removes_the_spaces_around_them() {
        let t = |s: &str| transform(s, COMPRESS_WHITESPACE).text;
        assert_eq!(t("a   b"), "a b");
        assert_eq!(t("a\nb"), "a\nb");
        assert_eq!(t("a  \n  b"), "a\nb");
        assert_eq!(t("a\n\nb"), "a\n\nb");
    }

    #[test]
    fn white_space_pre_and_pre_wrap_keep_everything() {
        let o = transform("a   b\tc\nd", COMPRESS_NONE);
        assert_eq!(o.text, "a   b\tc\nd");
        assert!(o.flags.0 & nsTextFrameUtils_Flags::HasTab.0 != 0);
        assert!(o.flags.0 & nsTextFrameUtils_Flags::HasNewline.0 != 0);
        let o = transform("a   b", COMPRESS_NONE);
        assert_eq!(o.flags, nsTextFrameUtils_Flags::default());
    }

    #[test]
    fn compress_none_transform_to_space_changes_tab_and_newline_to_space() {
        assert_eq!(transform("a\tb\nc  d", COMPRESS_NONE_TRANSFORM_TO_SPACE).text, "a b c  d");
    }

    #[test]
    fn a_soft_hyphen_is_dropped_and_flagged() {
        for mode in [COMPRESS_NONE, COMPRESS_WHITESPACE, COMPRESS_WHITESPACE_NEWLINE] {
            let o = transform("co\u{ad}op", mode);
            assert_eq!(o.text, "coop");
            assert!(o.flags.0 & nsTextFrameUtils_Flags::HasShy.0 != 0);
        }
    }

    #[test]
    fn bidi_controls_are_dropped_but_a_carriage_return_is_kept() {
        assert_eq!(transform("a\u{200e}b", COMPRESS_NONE).text, "ab");
        assert_eq!(transform("a\r\nb", COMPRESS_NONE).text, "a\r\nb");
    }

    #[test]
    fn the_skip_chars_record_what_was_removed() {
        let o = transform("a   b", COMPRESS_WHITESPACE_NEWLINE);
        assert_eq!(o.skip.GetOriginalCharCount(), 5);
        let mut it = gfxSkipCharsIterator::new__gfxSkipChars_int32_t(&o.skip, 0);
        // original: a [kept ' '] [skipped ' '] [skipped ' '] b
        assert_eq!(it.ConvertOriginalToSkipped(4), 3 - 1);
        assert_eq!(it.ConvertSkippedToOriginal(2), 4);
        it.SetOriginalOffset(2);
        assert!(it.IsOriginalCharSkipped(None));
        it.SetOriginalOffset(1);
        assert!(!it.IsOriginalCharSkipped(None));
    }

    #[test]
    fn incoming_whitespace_collapses_a_leading_space() {
        // The previous text run ended in white space: a leading space of this run is removed.
        let o = transform_with(" a", COMPRESS_WHITESPACE_NEWLINE, nsTextFrameUtils::INCOMING_WHITESPACE, None);
        assert_eq!(o.text, "a");
        // ... and the flag is output: the run ends in a space or not
        assert_eq!(o.incoming & nsTextFrameUtils::INCOMING_WHITESPACE, 0);
        let o = transform_with("a ", COMPRESS_WHITESPACE_NEWLINE, 0, None);
        assert_eq!(o.text, "a ");
        assert_eq!(o.incoming & nsTextFrameUtils::INCOMING_WHITESPACE, nsTextFrameUtils::INCOMING_WHITESPACE);
        // with preserved white space the incoming flag is cleared
        let o = transform_with("a", COMPRESS_NONE, nsTextFrameUtils::INCOMING_WHITESPACE, None);
        assert_eq!(o.incoming & nsTextFrameUtils::INCOMING_WHITESPACE, 0);
    }

    #[test]
    fn the_last_character_arabic_is_reported() {
        let o = transform("ab\u{0633}", COMPRESS_WHITESPACE_NEWLINE);
        assert_eq!(o.incoming & nsTextFrameUtils::INCOMING_ARABICCHAR, nsTextFrameUtils::INCOMING_ARABICCHAR);
        let o = transform("\u{0633}b", COMPRESS_WHITESPACE_NEWLINE);
        assert_eq!(o.incoming & nsTextFrameUtils::INCOMING_ARABICCHAR, 0);
    }

    #[test]
    fn a_space_before_a_combining_mark_is_not_white_space() {
        // IsSpaceCombiningSequenceTail: the last space belongs to the combining mark and is kept as it is;
        // the spaces before it collapse into one
        assert_eq!(normal("a   \u{301}b"), "a  \u{301}b");
        assert_eq!(normal("a  \u{301}b"), "a  \u{301}b");
        assert!(nsTextFrameUtils::IsSpaceCombiningSequenceTail__char16_t_ptr_int32_t(&[0x301], 1));
        // direction controls in between are ignored
        assert!(nsTextFrameUtils::IsSpaceCombiningSequenceTail__char16_t_ptr_int32_t(&[0x200e, 0x301], 2));
        assert!(!nsTextFrameUtils::IsSpaceCombiningSequenceTail__char16_t_ptr_int32_t(&['b' as u16], 1));
        assert!(!nsTextFrameUtils::IsSpaceCombiningSequenceTail__char16_t_ptr_int32_t(&[], 0));
        assert!(!nsTextFrameUtils::IsSpaceCombiningSequenceTail__uint8_t_ptr_int32_t(b"x", 1));
    }

    #[test]
    fn a_newline_between_east_asian_wide_characters_is_removed() {
        // CSS Text 3 4.1.2: no space where a segment break is between two wide (F/W/H) characters
        assert_eq!(normal("日\n本"), "日本");
        assert_eq!(normal("日 \n 本"), "日本");
        // not when one side is Latin ...
        assert_eq!(normal("日\nb"), "日 b");
        // ... nor Hangul
        assert_eq!(normal("한\n글"), "한 글");
        // a zero width space on one side also removes it
        assert_eq!(normal("a\u{200b}\nb"), "a\u{200b}b");
    }

    #[test]
    fn the_language_matters_for_east_asian_punctuation() {
        // Bug 1935148: in Japanese and Chinese a newline next to a wide punctuation mark is removed
        let t = |lang: Option<nsAtom>| transform_with("a。\nb", COMPRESS_WHITESPACE_NEWLINE, 0, lang).text;
        assert_eq!(t(None), "a。 b");
        assert_eq!(t(Some("en")), "a。 b");
        assert_eq!(t(Some("ja")), "a。b");
        assert_eq!(t(Some("zh-TW")), "a。b");
        assert_eq!(t(Some("zho")), "a。 b");
    }

    #[test]
    fn eight_bit_text_is_transformed_the_same_way() {
        let text = b"a  \n b\t\tc".to_vec();
        let mut output = vec![0u8; text.len()];
        let mut skip = gfxSkipChars::new();
        let mut incoming = 0u8;
        let mut flags = nsTextFrameUtils_Flags::default();
        let n = nsTextFrameUtils::TransformText(&text, text.len() as u32, &mut output, COMPRESS_WHITESPACE_NEWLINE, &mut incoming, &mut skip, &mut flags, None);
        assert_eq!(&output[..n], b"a b c");
        // the 8-bit soft hyphen
        let text = vec![b'a', 0xad, b'b'];
        let mut output = vec![0u8; 3];
        let n = nsTextFrameUtils::TransformText(&text, 3, &mut output, COMPRESS_NONE, &mut incoming, &mut skip, &mut flags, None);
        assert_eq!(&output[..n], b"ab");
        assert!(flags.0 & nsTextFrameUtils_Flags::HasShy.0 != 0);
    }

    #[test]
    fn skippable_characters_are_the_ones_transform_text_may_skip() {
        for c in [' ', '\t', '\n', '\u{ad}', '\u{200e}'] {
            assert!(nsTextFrameUtils::IsSkippableCharacterForTransformText(c as u32 as u16), "{c:?}");
        }
        assert!(!nsTextFrameUtils::IsSkippableCharacterForTransformText('a' as u16));
        assert!(!nsTextFrameUtils::IsSkippableCharacterForTransformText(0xa0u16));
        assert!(!nsTextFrameUtils::IsSkippableCharacterForTransformText(0xa0u8));
        assert!(nsTextFrameUtils::IsSkippableCharacterForTransformText(0xadu8));
    }

    #[test]
    fn append_line_break_offset_skips_duplicates() {
        let mut v = Vec::new();
        nsTextFrameUtils::AppendLineBreakOffset(&mut v, 3);
        nsTextFrameUtils::AppendLineBreakOffset(&mut v, 3);
        nsTextFrameUtils::AppendLineBreakOffset(&mut v, 5);
        assert_eq!(v, vec![3, 5]);
    }

    #[test]
    fn the_skip_chars_run_iterator_walks_kept_and_skipped_runs() {
        // "a   b" -> "a b": runs are kept(2) skipped(2) kept(1)
        let o = transform("a   b", COMPRESS_WHITESPACE_NEWLINE);
        let start = gfxSkipCharsIterator::new__gfxSkipChars_int32_t(&o.skip, 0);
        let mut it = nsSkipCharsRunIterator::new(&start, nsSkipCharsRunIterator_LengthMode::LENGTH_UNSKIPPED_ONLY, 3);
        let mut runs = Vec::new();
        while it.NextRun() {
            runs.push((it.IsSkipped(), it.GetRunLength(), it.GetOriginalOffset()));
        }
        assert_eq!(runs, vec![(false, 2, 0), (false, 1, 4)]);
        // visiting skipped runs as well, with the length counting the skipped characters
        let mut it = nsSkipCharsRunIterator::new(&start, nsSkipCharsRunIterator_LengthMode::LENGTH_INCLUDES_SKIPPED, 5);
        it.SetVisitSkipped();
        let mut runs = Vec::new();
        while it.NextRun() {
            runs.push((it.IsSkipped(), it.GetRunLength()));
        }
        assert_eq!(runs, vec![(false, 2), (true, 2), (false, 1)]);
    }
}
